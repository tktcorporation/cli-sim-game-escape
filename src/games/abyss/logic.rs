//! 深淵潜行 — 純粋ロジック関数群。
//!
//! tick / spawn / 攻撃判定 / 強化購入など全てここに集約する。
//! `state.rs` のフィールドだけを操作し、IOやレンダーは触らない。
//!
//! 数値バランスは `state.config` (BalanceConfig) から取り出すので、
//! 本体ゲームとシミュレータで完全に同じコードを通る ─ 動作の乖離は
//! 構造的に起きない。

use super::config::BalanceConfig;
use super::policy::PlayerAction;
use super::state::{
    AbyssState, Enemy, FloorKind, GachaResultSummary, GachaTier, SoulPerk, Tab, UpgradeKind,
};

/// メインの tick 処理。delta_ticks 回ぶん戦闘を進める。
pub fn tick(state: &mut AbyssState, delta_ticks: u32) {
    if delta_ticks == 0 {
        return;
    }

    // 演出 tick の減衰
    state.hero_hurt_flash = state.hero_hurt_flash.saturating_sub(delta_ticks);
    state.enemy_hurt_flash = state.enemy_hurt_flash.saturating_sub(delta_ticks);
    state.descent_flash = state.descent_flash.saturating_sub(delta_ticks);
    if let Some((_, ref mut life, _)) = state.last_enemy_damage {
        *life = life.saturating_sub(delta_ticks);
    }
    if let Some((_, ref mut life)) = state.last_hero_damage {
        *life = life.saturating_sub(delta_ticks);
    }
    if matches!(state.last_enemy_damage, Some((_, 0, _))) {
        state.last_enemy_damage = None;
    }
    if matches!(state.last_hero_damage, Some((_, 0))) {
        state.last_hero_damage = None;
    }
    if let Some(r) = state.last_gacha.as_mut() {
        r.life_ticks = r.life_ticks.saturating_sub(delta_ticks);
    }

    // tick を 1 ステップずつ処理する (敵の交代が連続で起きうるため)。
    for _ in 0..delta_ticks {
        step_one_tick(state);
        state.total_ticks += 1;
    }
}

fn step_one_tick(state: &mut AbyssState) {
    // ── HP regen (10 tick = 1秒 → 1秒あたりの規定値を 10 tick に分散) ──
    if state.hero_hp > 0 && state.hero_hp < state.hero_max_hp() {
        // regen_per_sec * 100 を 10tickで足す → 1tickあたり regen_per_sec * 10
        let regen_per_tick_x100 = (state.hero_regen_per_sec() * 10.0).round() as u32;
        state.hero_regen_acc_x100 = state.hero_regen_acc_x100.saturating_add(regen_per_tick_x100);
        if state.hero_regen_acc_x100 >= 100 {
            let heal = (state.hero_regen_acc_x100 / 100) as u64;
            state.hero_regen_acc_x100 %= 100;
            let max = state.hero_max_hp();
            state.hero_hp = (state.hero_hp + heal).min(max);
        }
    }

    // ── 敵が居ない (HP 0 のプレースホルダ) → 新しい敵スポーン ──
    if state.current_enemy.hp == 0 || state.current_enemy.max_hp == 0 {
        spawn_next_enemy(state);
        return;
    }

    // ── hero attack ──
    state.hero_atk_cooldown = state.hero_atk_cooldown.saturating_sub(1);
    if state.hero_atk_cooldown == 0 {
        let crit = roll_crit(state);
        let raw = state.hero_atk();
        let dmg_after_def = raw.saturating_sub(state.current_enemy.def).max(1);
        let dmg = if crit { dmg_after_def * 2 } else { dmg_after_def };
        let actual = dmg.min(state.current_enemy.hp);
        state.current_enemy.hp -= actual;
        state.last_enemy_damage = Some((actual, 6, crit));
        state.enemy_hurt_flash = 3;

        // 攻撃成功で focus +1。次攻撃の cooldown は焼成された focus を反映する。
        let focus_max = state.config.hero.focus_max;
        state.combat_focus = (state.combat_focus + 1).min(focus_max);
        state.hero_atk_cooldown = state.hero_atk_period();

        if state.current_enemy.hp == 0 {
            on_enemy_killed(state);
            return;
        }
    }

    // ── enemy attack ──
    state.current_enemy.atk_cooldown = state.current_enemy.atk_cooldown.saturating_sub(1);
    if state.current_enemy.atk_cooldown == 0 {
        let raw = state.current_enemy.atk;
        let dmg = raw.saturating_sub(state.hero_def()).max(1);
        let actual = dmg.min(state.hero_hp);
        state.hero_hp -= actual;
        state.last_hero_damage = Some((actual, 6));
        state.hero_hurt_flash = 3;
        state.current_enemy.atk_cooldown = state.current_enemy.atk_period;

        if state.hero_hp == 0 {
            on_hero_died(state);
        }
    }
}

/// 撃破時の処理: gold/魂を加算し、kill カウンタを進める。
fn on_enemy_killed(state: &mut AbyssState) {
    let was_boss = state.current_enemy.is_boss;
    let kind_gold = state.floor_kind.gold_mult();
    let gold_drop = ((state.current_enemy.gold as f64) * state.gold_multiplier() * kind_gold)
        .round() as u64;
    state.gold = state.gold.saturating_add(gold_drop);
    state.run_gold_earned = state.run_gold_earned.saturating_add(gold_drop);

    let pacing = &state.config.pacing;
    let base_souls = if was_boss {
        (state.floor as u64) * pacing.boss_souls_mult
    } else {
        // 0 だと div_ceil が panic するので最小 1 にクランプ。
        // tuning config が誤って 0 を入れても fail-graceful にする。
        let div = pacing.normal_souls_div.max(1);
        state.floor.div_ceil(div) as u64
    };
    let souls = (base_souls as f64 * state.soul_multiplier()).round() as u64;
    state.souls = state.souls.saturating_add(souls);

    state.run_kills = state.run_kills.saturating_add(1);
    state.total_kills = state.total_kills.saturating_add(1);

    if was_boss {
        // 鍵ドロップ: 基本 + フロア種別ボーナス + 深層 (10F毎) ボーナス。
        let g = &state.config.gacha;
        let mut keys_dropped = g.keys_per_boss + state.floor_kind.bonus_keys_on_boss();
        if g.deep_floor_step > 0 && state.floor.is_multiple_of(g.deep_floor_step) {
            keys_dropped = keys_dropped.saturating_add(g.deep_floor_bonus_keys);
        }
        state.keys = state.keys.saturating_add(keys_dropped);

        state.add_log(format!(
            "▶ ボス {} 撃破！ +{}g +{}魂 +{}🔑",
            state.current_enemy.name, gold_drop, souls, keys_dropped
        ));
        // ボス撃破 → 次階層へ進むか、現フロアに留まるか
        if state.auto_descend {
            descend_to_next_floor(state);
        } else {
            // 自動潜行 OFF → 同じフロアに留まり、kill カウンタをリセット (=次のボスへ向けて再周回)
            state.kills_on_floor = 0;
            spawn_next_enemy(state);
        }
    } else {
        state.kills_on_floor = state.kills_on_floor.saturating_add(1);
        spawn_next_enemy(state);
    }
}

fn descend_to_next_floor(state: &mut AbyssState) {
    state.floor = state.floor.saturating_add(1);
    if state.floor > state.max_floor {
        state.max_floor = state.floor;
    }
    if state.floor > state.deepest_floor_ever {
        state.deepest_floor_ever = state.floor;
    }
    state.kills_on_floor = 0;
    state.descent_flash = 8;
    state.floor_kind = roll_floor_kind(state.floor, &state.config, &mut state.rng_state);
    let kind_suffix = match state.floor_kind {
        FloorKind::Normal => String::new(),
        other => format!(" 〔{} {}〕", other.short_label(), other.name()),
    };
    state.add_log(format!("▼ B{}F に到達{}", state.floor, kind_suffix));
    spawn_next_enemy(state);
}

/// フロア種別を抽選する。`floor_kind_normal_below` 階以下は必ず Normal。
fn roll_floor_kind(floor: u32, config: &BalanceConfig, rng_seed: &mut u32) -> FloorKind {
    let g = &config.gacha;
    if floor < g.floor_kind_normal_below {
        return FloorKind::Normal;
    }
    let weights = g.floor_kind_weights;
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return FloorKind::Normal;
    }
    let r = rng_next(rng_seed) % total;
    let mut acc = 0u32;
    let kinds = [
        FloorKind::Normal,
        FloorKind::Treasure,
        FloorKind::Elite,
        FloorKind::Bonanza,
    ];
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if r < acc {
            return kinds[i];
        }
    }
    FloorKind::Normal
}

fn on_hero_died(state: &mut AbyssState) {
    state.deaths = state.deaths.saturating_add(1);
    let mult = state.config.pacing.death_souls_mult;
    let bonus_souls =
        ((state.floor as u64).saturating_mul(mult)) as f64 * state.soul_multiplier();
    let bonus_souls = bonus_souls.round() as u64;
    state.souls = state.souls.saturating_add(bonus_souls);

    state.add_log(format!(
        "✝ B{}F で力尽きた… +{}魂 / 浅瀬に帰還",
        state.floor, bonus_souls
    ));

    // ラン開始地点に戻る
    state.floor = 1;
    state.floor_kind = FloorKind::Normal;
    state.kills_on_floor = 0;
    state.run_kills = 0;
    state.run_gold_earned = 0;
    state.hero_hp = state.hero_max_hp();
    state.combat_focus = 0;
    state.hero_atk_cooldown = state.hero_atk_period();
    state.hero_regen_acc_x100 = 0;
    spawn_next_enemy(state);
}

/// 次の敵 (雑魚 or ボス) を生成して current_enemy にセット。
fn spawn_next_enemy(state: &mut AbyssState) {
    let is_boss = state.kills_on_floor >= state.enemies_per_floor();
    let mut e = make_enemy(state.floor, is_boss, &state.config, &mut state.rng_state);
    apply_floor_kind_to_enemy(&mut e, state.floor_kind);
    state.current_enemy = e;
}

/// フロア種別による敵の数値修正を適用する。
/// gold は `gold_drop` 計算側で `floor_kind.gold_mult()` を掛けるので、
/// ここでは hp / atk のみ調整する (二重適用を避ける)。
fn apply_floor_kind_to_enemy(e: &mut Enemy, kind: FloorKind) {
    let hp_m = kind.enemy_hp_mult();
    let atk_m = kind.enemy_atk_mult();
    if (hp_m - 1.0).abs() > f64::EPSILON {
        let new_hp = ((e.max_hp as f64) * hp_m).round().max(1.0) as u64;
        e.max_hp = new_hp;
        e.hp = new_hp;
    }
    if (atk_m - 1.0).abs() > f64::EPSILON {
        e.atk = ((e.atk as f64) * atk_m).round().max(1.0) as u64;
    }
}

/// シンプルな擬似乱数 (xorshift32)。
fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

fn roll_crit(state: &mut AbyssState) -> bool {
    let r = rng_next(&mut state.rng_state) % 1000;
    let threshold = (state.hero_crit_rate() * 1000.0) as u32;
    r < threshold
}

/// 与えられた floor / boss フラグから敵を生成する。
/// 数値スケーリングは config から、名前テーブルは固定で持つ。
pub fn make_enemy(floor: u32, is_boss: bool, config: &BalanceConfig, rng_seed: &mut u32) -> Enemy {
    // 名前テーブル (フロア帯ごとに変わる)。
    let normal_names: &[&str] = match floor {
        1..=2 => &["スライム", "大ネズミ", "コウモリ"],
        3..=5 => &["ゴブリン", "スケルトン", "影の犬"],
        6..=9 => &["オーガ", "リッチ", "屍鬼"],
        10..=14 => &["ガーゴイル", "ワイト", "影食らい"],
        15..=19 => &["デーモン", "屍王", "鋼の番兵"],
        20..=29 => &["古代の悪魔", "灼熱竜", "虚無の使徒"],
        _ => &["奈落の主", "深淵の眷属", "終焉の影"],
    };
    let boss_names: &[&str] = match floor {
        1..=4 => &["ゴブリン王", "巨大スライム", "墓守"],
        5..=9 => &["ミノタウロス", "リッチロード", "石化竜"],
        10..=14 => &["デーモンロード", "黒鎧将軍"],
        15..=19 => &["堕天の王", "魔神ベルゼブ"],
        20..=29 => &["竜帝バハムート", "深淵の門番"],
        _ => &["奈落王", "終焉竜", "深淵そのもの"],
    };

    let names = if is_boss { boss_names } else { normal_names };
    let r = (rng_next(rng_seed) as usize) % names.len();
    let name = names[r].to_string();

    let e = &config.enemy;
    let f = floor as f64;
    let mut hp = e.hp_base * e.hp_growth.powf(f - 1.0);
    let mut atk = e.atk_base * e.atk_growth.powf(f - 1.0);
    let mut def = e.def_base + (f - 1.0) * e.def_per_floor;
    let mut gold = e.gold_base * e.gold_growth.powf(f - 1.0);

    if is_boss {
        hp *= e.boss_hp_mult;
        atk *= e.boss_atk_mult;
        def *= e.boss_def_mult;
        gold *= e.boss_gold_mult;
    }

    let max_hp = hp.round().max(1.0) as u64;
    let atk = atk.round().max(1.0) as u64;
    let def = def.round() as u64;
    let gold = gold.round().max(1.0) as u64;

    let atk_period = if is_boss { e.boss_atk_period } else { e.normal_atk_period };

    Enemy {
        name,
        max_hp,
        hp: max_hp,
        atk,
        def,
        gold,
        is_boss,
        atk_cooldown: atk_period,
        atk_period,
    }
}

// ── プレイヤーアクション ──

/// 強化を 1 段階購入する。gold が足りなければ false。
pub fn buy_upgrade(state: &mut AbyssState, kind: UpgradeKind) -> bool {
    let cost = state.upgrade_cost(kind);
    if state.gold < cost {
        return false;
    }
    state.gold -= cost;

    // Vitality は最大値増加分をそのまま現 HP にも乗せて「気持ち良さ」を出す。
    // 増分は config (hp_per_vitality_lv) と Endurance 倍率に依存するので、
    // ハードコードせず Lv 上げ前後の hero_max_hp() の差分で計算する。
    let max_before = if matches!(kind, UpgradeKind::Vitality) {
        Some(state.hero_max_hp())
    } else {
        None
    };

    // 段階制カーブを持つ強化は、購入前後で段階名が変わったかチェックする (演出用)。
    let tier_before = state.upgrade_tier(kind).map(|(name, _)| name);

    state.upgrades[kind.index()] = state.upgrades[kind.index()].saturating_add(1);

    if let Some(before) = max_before {
        let after = state.hero_max_hp();
        let delta = after.saturating_sub(before);
        state.hero_hp = state.hero_hp.saturating_add(delta).min(after);
    }

    state.add_log(format!("◆ {} Lv.{}", kind.name(), state.upgrades[kind.index()]));

    // 段階突破 → 専用ログで "層が上がった" 感を出す。
    if let Some(before_name) = tier_before {
        if let Some((after_name, _)) = state.upgrade_tier(kind) {
            if before_name != after_name {
                state.add_log(format!(
                    "☆ {} 段階突破: {} → {}",
                    kind.name(),
                    before_name,
                    after_name
                ));
            }
        }
    }
    true
}

/// 魂強化を 1 段階購入する。
pub fn buy_soul_perk(state: &mut AbyssState, perk: SoulPerk) -> bool {
    let cost = state.soul_perk_cost(perk);
    if state.souls < cost {
        return false;
    }
    state.souls -= cost;
    state.soul_perks[perk.index()] = state.soul_perks[perk.index()].saturating_add(1);

    if matches!(perk, SoulPerk::Endurance) {
        let max = state.hero_max_hp();
        if state.hero_hp > max {
            state.hero_hp = max;
        }
    }

    state.add_log(format!("✦ {} Lv.{}", perk.name(), state.soul_perks[perk.index()]));
    true
}

/// 自動潜行のON/OFFを切替。
pub fn toggle_auto_descend(state: &mut AbyssState) {
    state.auto_descend = !state.auto_descend;
    if state.auto_descend {
        state.add_log("▼ 自動潜行 ON");
    } else {
        state.add_log("■ 自動潜行 OFF (現フロアで周回)");
    }
}

/// タブ切替。
pub fn set_tab(state: &mut AbyssState, tab: Tab) {
    state.tab = tab;
    // タブ切替時にスクロール位置を初期化。タブごとに別 scroll を持たない設計
    // (シンプルさ優先、cookie の prestige_scroll と同じ流儀)。
    state.tab_scroll.set(0);
}

/// プレイヤー行動を適用する統一エントリ。本体ゲームの入力ハンドラも、
/// シミュレータの Policy も、最終的にこの関数を通る。返値は「行動が
/// 何らかの状態変化を起こしたか」のフラグ (買えなかった等で false)。
pub fn apply_action(state: &mut AbyssState, action: PlayerAction) -> bool {
    match action {
        PlayerAction::BuyUpgrade(kind) => buy_upgrade(state, kind),
        PlayerAction::BuySoulPerk(perk) => buy_soul_perk(state, perk),
        PlayerAction::ToggleAutoDescend => {
            toggle_auto_descend(state);
            true
        }
        PlayerAction::Retreat => {
            retreat(state);
            true
        }
        PlayerAction::SetTab(tab) => {
            set_tab(state, tab);
            true
        }
        PlayerAction::GachaPull(count) => gacha_pull(state, count),
        PlayerAction::ScrollUp => {
            // 上限 clamp は render 直前で行うのでここは飽和減算のみ。
            let v = state.tab_scroll.get().saturating_sub(SCROLL_STEP);
            state.tab_scroll.set(v);
            true
        }
        PlayerAction::ScrollDown => {
            // 下限 clamp は render が visual_height から算出して書き戻す。
            // ここで盲目的に増やしておき、次フレームで補正される。
            let v = state.tab_scroll.get().saturating_add(SCROLL_STEP);
            state.tab_scroll.set(v);
            true
        }
    }
}

/// 1 操作あたりのスクロール幅 (visual rows)。Cookie の prestige_scroll と同じ単位。
const SCROLL_STEP: u16 = 3;

// ── ガチャ ────────────────────────────────────────────────

/// 鍵を消費してガチャを `count` 回引く。鍵が足りなければ引ける分だけ。
/// 1 回でも引けたら true。
pub fn gacha_pull(state: &mut AbyssState, count: u32) -> bool {
    if count == 0 || state.keys == 0 {
        return false;
    }
    let actual = (count as u64).min(state.keys) as u32;
    let mut summary = GachaResultSummary {
        count: actual,
        by_tier: [0; 4],
        gained_gold: 0,
        gained_souls: 0,
        gained_keys: 0,
        gained_upgrade_lv: 0,
        life_ticks: 30,
    };

    for _ in 0..actual {
        state.keys -= 1;
        state.total_pulls = state.total_pulls.saturating_add(1);
        let tier = roll_gacha_tier(state);
        match tier {
            GachaTier::Common => summary.by_tier[0] += 1,
            GachaTier::Rare => summary.by_tier[1] += 1,
            GachaTier::Epic => summary.by_tier[2] += 1,
            GachaTier::Legendary => summary.by_tier[3] += 1,
        }
        // pity counter は Epic 以上で reset (Rare までは天井加算継続)。
        if matches!(tier, GachaTier::Epic | GachaTier::Legendary) {
            state.pulls_since_epic = 0;
        } else {
            state.pulls_since_epic = state.pulls_since_epic.saturating_add(1);
        }
        apply_gacha_reward(state, tier, &mut summary);
    }

    state.add_log(format!(
        "🎲 ガチャ x{}: C{} R{} E{} L{}",
        actual, summary.by_tier[0], summary.by_tier[1], summary.by_tier[2], summary.by_tier[3],
    ));
    state.last_gacha = Some(summary);
    true
}

/// gacha_weights_milli + 天井 (pulls_since_epic) を加味して tier を決定。
fn roll_gacha_tier(state: &mut AbyssState) -> GachaTier {
    let g = &state.config.gacha;
    let pity_active = g.gacha_pity > 0 && state.pulls_since_epic >= g.gacha_pity.saturating_sub(1);
    if pity_active {
        // Epic / Legendary を [Epic 比率] : [Legendary 比率] で抽選。
        let epic_w = g.gacha_weights_milli[2].max(1);
        let leg_w = g.gacha_weights_milli[3];
        let total = epic_w + leg_w;
        let r = rng_next(&mut state.rng_state) % total;
        return if r < epic_w {
            GachaTier::Epic
        } else {
            GachaTier::Legendary
        };
    }
    let total: u32 = g.gacha_weights_milli.iter().sum::<u32>().max(1);
    let r = rng_next(&mut state.rng_state) % total;
    let mut acc = 0u32;
    let tiers = [
        GachaTier::Common,
        GachaTier::Rare,
        GachaTier::Epic,
        GachaTier::Legendary,
    ];
    for (i, &w) in g.gacha_weights_milli.iter().enumerate() {
        acc += w;
        if r < acc {
            return tiers[i];
        }
    }
    GachaTier::Common
}

fn apply_gacha_reward(state: &mut AbyssState, tier: GachaTier, summary: &mut GachaResultSummary) {
    let g = state.config.gacha.clone();
    match tier {
        GachaTier::Common => {
            // 現フロアの基礎雑魚 gold (config から再計算) × ランダム倍率 × gold倍率。
            let base = base_normal_gold(state.floor, &state.config);
            let lo = g.common_gold_mult_min.max(1);
            let hi = g.common_gold_mult_max.max(lo);
            let mult_range = hi - lo + 1;
            let r = rng_next(&mut state.rng_state) % mult_range;
            let mult = lo + r;
            let gold = ((base as f64) * (mult as f64) * state.gold_multiplier()).round() as u64;
            let gold = gold.max(1);
            state.gold = state.gold.saturating_add(gold);
            state.run_gold_earned = state.run_gold_earned.saturating_add(gold);
            summary.gained_gold = summary.gained_gold.saturating_add(gold);
        }
        GachaTier::Rare => {
            // ランダムな upgrade を Lv +1 (永続)。Vitality なら現 HP も bump。
            let idx = (rng_next(&mut state.rng_state) as usize) % UpgradeKind::all().len();
            let kind = UpgradeKind::from_index(idx).unwrap_or(UpgradeKind::Sword);
            let max_before = if matches!(kind, UpgradeKind::Vitality) {
                Some(state.hero_max_hp())
            } else {
                None
            };
            state.upgrades[kind.index()] = state.upgrades[kind.index()].saturating_add(1);
            if let Some(before) = max_before {
                let after = state.hero_max_hp();
                let delta = after.saturating_sub(before);
                state.hero_hp = state.hero_hp.saturating_add(delta).min(after);
            }
            summary.gained_upgrade_lv = summary.gained_upgrade_lv.saturating_add(1);
        }
        GachaTier::Epic => {
            let souls = ((state.floor as u64).saturating_mul(g.epic_souls_mult)) as f64
                * state.soul_multiplier();
            let souls = souls.round() as u64;
            state.souls = state.souls.saturating_add(souls);
            summary.gained_souls = summary.gained_souls.saturating_add(souls);
        }
        GachaTier::Legendary => {
            state.keys = state.keys.saturating_add(g.legendary_keys);
            summary.gained_keys = summary.gained_keys.saturating_add(g.legendary_keys);
        }
    }
}

/// `make_enemy` と同じ式で「指定フロアの基礎雑魚 gold」を再計算する。
/// ガチャ Common の報酬基準にだけ使う (敵生成と数値が乖離しないよう同式に揃える)。
fn base_normal_gold(floor: u32, config: &BalanceConfig) -> u64 {
    let f = floor as f64;
    let g = config.enemy.gold_base * config.enemy.gold_growth.powf(f - 1.0);
    g.round().max(1.0) as u64
}

/// 自分の意思で浅瀬 (B1F) に戻る。死亡扱いにはしない (魂ボーナスなし)。
pub fn retreat(state: &mut AbyssState) {
    if state.floor == 1 {
        state.add_log("既に B1F に居る");
        return;
    }
    state.add_log(format!("△ 自主撤退: B{}F → B1F", state.floor));
    state.floor = 1;
    state.floor_kind = FloorKind::Normal;
    state.kills_on_floor = 0;
    state.hero_hp = state.hero_max_hp();
    state.combat_focus = 0;
    state.hero_atk_cooldown = state.hero_atk_period();
    spawn_next_enemy(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticked_state() -> AbyssState {
        let mut s = AbyssState::new();
        // 最初は placeholder なので 1 tick 進めて初期敵を作る
        tick(&mut s, 1);
        s
    }

    #[test]
    fn first_tick_spawns_enemy() {
        let s = ticked_state();
        assert!(s.current_enemy.max_hp > 0);
        assert!(!s.current_enemy.name.is_empty());
        assert!(!s.current_enemy.is_boss);
    }

    #[test]
    fn hero_attacks_enemy_over_time() {
        let mut s = ticked_state();
        let initial_hp = s.current_enemy.hp;
        // hero_atk_period (12) tick 進めれば 1 攻撃は確実に発生
        tick(&mut s, 30);
        assert!(s.current_enemy.hp < initial_hp || s.run_kills > 0);
    }

    #[test]
    fn killing_enemies_advances_floor_with_auto_descend() {
        let mut s = AbyssState::new();
        // 大量の強化で確実にフロアを進める
        s.upgrades[UpgradeKind::Sword.index()] = 50;
        s.upgrades[UpgradeKind::Speed.index()] = 10;
        s.upgrades[UpgradeKind::Vitality.index()] = 50;
        s.hero_hp = s.hero_max_hp();
        s.auto_descend = true;
        // 適度に長く進める
        tick(&mut s, 2000);
        assert!(s.floor >= 2, "floor should advance, got {}", s.floor);
    }

    #[test]
    fn no_descend_when_auto_descend_off() {
        let mut s = AbyssState::new();
        s.upgrades[UpgradeKind::Sword.index()] = 50;
        s.upgrades[UpgradeKind::Vitality.index()] = 50;
        s.upgrades[UpgradeKind::Speed.index()] = 10;
        s.hero_hp = s.hero_max_hp();
        s.auto_descend = false;
        let per_floor = s.enemies_per_floor() as u64;
        tick(&mut s, 2000);
        assert_eq!(s.floor, 1);
        assert!(s.run_kills > per_floor);
    }

    #[test]
    fn weak_hero_dies_eventually() {
        let mut s = AbyssState::new();
        // 弱い hero / 強い floor
        s.floor = 30;
        s.auto_descend = false;
        s.upgrades[UpgradeKind::Vitality.index()] = 0;
        s.hero_hp = s.hero_max_hp();
        tick(&mut s, 10_000);
        // 死亡しているか、何度かリセットされて floor=1 に戻っているはず
        assert!(s.deaths > 0 || s.floor == 1);
    }

    #[test]
    fn buy_upgrade_with_enough_gold() {
        let mut s = ticked_state();
        s.gold = 1_000_000;
        let before_atk = s.hero_atk();
        let ok = buy_upgrade(&mut s, UpgradeKind::Sword);
        assert!(ok);
        assert_eq!(s.upgrades[UpgradeKind::Sword.index()], 1);
        assert!(s.hero_atk() > before_atk);
    }

    #[test]
    fn buy_upgrade_fails_without_gold() {
        let mut s = ticked_state();
        s.gold = 0;
        let ok = buy_upgrade(&mut s, UpgradeKind::Sword);
        assert!(!ok);
        assert_eq!(s.upgrades[UpgradeKind::Sword.index()], 0);
    }

    #[test]
    fn vitality_increases_current_hp_too() {
        let mut s = ticked_state();
        s.gold = 1_000_000;
        // ダメージを受けた状態を作る
        let max = s.hero_max_hp();
        s.hero_hp = max - 5;
        let before_hp = s.hero_hp;
        buy_upgrade(&mut s, UpgradeKind::Vitality);
        assert!(s.hero_hp > before_hp);
    }

    #[test]
    fn vitality_current_hp_bump_matches_config() {
        // hp_per_vitality_lv を変えても、現 HP 増分と最大 HP 増分が一致することを確認。
        // (固定 +10 を使っていた旧実装に対する回帰テスト)
        let mut cfg = BalanceConfig::default();
        cfg.hero.hp_per_vitality_lv = 25; // 既定 10 から変更
        cfg.hero.vitality_curve = None; // 段階制を無効化して旧経路を検証
        let mut s = AbyssState::with_config(cfg);
        s.gold = 1_000_000;
        let max_before = s.hero_max_hp();
        s.hero_hp = max_before - 7;
        let hp_before = s.hero_hp;

        let ok = buy_upgrade(&mut s, UpgradeKind::Vitality);
        assert!(ok);

        let max_after = s.hero_max_hp();
        // max は config 通りに増えているはず
        assert_eq!(max_after - max_before, 25);
        // 現 HP も同じ delta だけ増えているはず (キャップ未達の状態)
        assert_eq!(s.hero_hp - hp_before, 25);
    }

    #[test]
    fn vitality_current_hp_capped_at_new_max() {
        // 満タンで Vitality を買ったら、新最大値まで bump、上回らない。
        let mut cfg = BalanceConfig::default();
        cfg.hero.hp_per_vitality_lv = 5;
        let mut s = AbyssState::with_config(cfg);
        s.gold = 1_000_000;
        let max_before = s.hero_max_hp();
        s.hero_hp = max_before; // 満タン
        buy_upgrade(&mut s, UpgradeKind::Vitality);
        let max_after = s.hero_max_hp();
        assert_eq!(s.hero_hp, max_after);
        assert!(s.hero_hp <= max_after);
    }

    #[test]
    fn soul_perk_purchase() {
        let mut s = AbyssState::new();
        s.souls = 1000;
        let ok = buy_soul_perk(&mut s, SoulPerk::Might);
        assert!(ok);
        assert_eq!(s.soul_perks[SoulPerk::Might.index()], 1);
    }

    #[test]
    fn normal_souls_div_zero_does_not_panic() {
        // tuning ミスで normal_souls_div = 0 が入っても panic せず、最低 1 にクランプされる。
        let mut cfg = BalanceConfig::default();
        cfg.pacing.normal_souls_div = 0;
        let mut s = AbyssState::with_config(cfg);
        s.upgrades[UpgradeKind::Sword.index()] = 100;
        s.upgrades[UpgradeKind::Speed.index()] = 20;
        s.hero_hp = s.hero_max_hp();
        // 雑魚撃破まで進める。panic しなければ OK。
        tick(&mut s, 5_000);
        assert!(s.run_kills > 0);
    }

    #[test]
    fn toggle_auto_descend_works() {
        let mut s = AbyssState::new();
        let before = s.auto_descend;
        toggle_auto_descend(&mut s);
        assert_ne!(s.auto_descend, before);
    }

    #[test]
    fn retreat_resets_floor() {
        let mut s = AbyssState::new();
        s.floor = 5;
        s.hero_hp = 1;
        retreat(&mut s);
        assert_eq!(s.floor, 1);
        assert!(s.hero_hp > 1);
    }

    #[test]
    fn boss_spawns_after_enough_kills() {
        let mut s = AbyssState::new();
        s.kills_on_floor = s.enemies_per_floor();
        // 1 tick 進めれば boss spawn
        tick(&mut s, 1);
        assert!(s.current_enemy.is_boss);
    }

    #[test]
    fn rng_state_advances() {
        let mut seed = 12345;
        let a = rng_next(&mut seed);
        let b = rng_next(&mut seed);
        assert_ne!(a, b);
    }

    #[test]
    fn enemy_scaling_with_floor() {
        let cfg = BalanceConfig::default();
        let mut seed = 1;
        let e1 = make_enemy(1, false, &cfg, &mut seed);
        let e10 = make_enemy(10, false, &cfg, &mut seed);
        assert!(e10.max_hp > e1.max_hp);
        assert!(e10.atk > e1.atk);
        assert!(e10.gold > e1.gold);
    }

    #[test]
    fn boss_is_tougher() {
        let cfg = BalanceConfig::default();
        let mut seed = 1;
        let normal = make_enemy(5, false, &cfg, &mut seed);
        let boss = make_enemy(5, true, &cfg, &mut seed);
        assert!(boss.max_hp > normal.max_hp);
        assert!(boss.gold > normal.gold);
    }

    // ── ガチャ / フロア種別 ────────────────────────────────

    #[test]
    fn gacha_pull_requires_keys() {
        let mut s = AbyssState::new();
        s.keys = 0;
        assert!(!gacha_pull(&mut s, 1));
        assert_eq!(s.total_pulls, 0);
    }

    #[test]
    fn gacha_pull_consumes_keys_and_increments_pulls() {
        let mut s = AbyssState::new();
        s.keys = 5;
        let ok = gacha_pull(&mut s, 3);
        assert!(ok);
        assert_eq!(s.keys, 2);
        assert_eq!(s.total_pulls, 3);
        assert!(s.last_gacha.is_some());
    }

    #[test]
    fn gacha_pull_clamped_to_available_keys() {
        // 鍵 2 個しかないのに 10 連を要求 → 2 連だけ実行される。
        let mut s = AbyssState::new();
        s.keys = 2;
        let ok = gacha_pull(&mut s, 10);
        assert!(ok);
        assert_eq!(s.keys, 0);
        assert_eq!(s.total_pulls, 2);
    }

    #[test]
    fn gacha_pity_forces_epic_or_better() {
        // pity 直前の状態から 1 連引くと、必ず Epic か Legendary が出る。
        let mut s = AbyssState::new();
        s.keys = 1;
        s.pulls_since_epic = s.config.gacha.gacha_pity.saturating_sub(1);
        gacha_pull(&mut s, 1);
        let r = s.last_gacha.as_ref().unwrap();
        // Epic か Legendary が 1 個入っているはず。
        assert_eq!(r.by_tier[2] + r.by_tier[3], 1);
        assert_eq!(s.pulls_since_epic, 0);
    }

    #[test]
    fn gacha_legendary_grants_keys() {
        // 高確率で Legendary を出すよう gacha_weights を差し替えて確認。
        let mut cfg = BalanceConfig::default();
        cfg.gacha.gacha_weights_milli = [0, 0, 0, 1000];
        let mut s = AbyssState::with_config(cfg);
        s.keys = 1;
        gacha_pull(&mut s, 1);
        // 1 鍵消費 → Legendary で legendary_keys 鍵が返ってくる。
        assert_eq!(s.keys, s.config.gacha.legendary_keys);
    }

    #[test]
    fn floor_kind_first_floors_normal() {
        let cfg = BalanceConfig::default();
        let mut seed = 1;
        for f in 1..cfg.gacha.floor_kind_normal_below {
            let kind = roll_floor_kind(f, &cfg, &mut seed);
            assert_eq!(kind, FloorKind::Normal, "B{}F should be Normal", f);
        }
    }

    #[test]
    fn floor_kind_zero_weights_falls_back_to_normal() {
        let mut cfg = BalanceConfig::default();
        cfg.gacha.floor_kind_weights = [0, 0, 0, 0];
        let mut seed = 1;
        let kind = roll_floor_kind(50, &cfg, &mut seed);
        assert_eq!(kind, FloorKind::Normal);
    }

    #[test]
    fn elite_floor_drops_extra_keys_on_boss() {
        // floor_kind = Elite で B5F のボスを撃破した時、鍵が 1+2=3 もらえる。
        let mut s = AbyssState::new();
        s.floor = 5;
        s.floor_kind = FloorKind::Elite;
        s.kills_on_floor = s.enemies_per_floor();
        s.upgrades[UpgradeKind::Sword.index()] = 200;
        s.upgrades[UpgradeKind::Speed.index()] = 20;
        s.hero_hp = s.hero_max_hp();
        // tick で boss spawn → 撃破まで進む。
        let keys_before = s.keys;
        for _ in 0..400 {
            tick(&mut s, 1);
            if s.keys > keys_before {
                break;
            }
        }
        assert!(
            s.keys >= keys_before + 3,
            "Elite boss should drop +3 keys (1 base + 2 elite), got {}",
            s.keys - keys_before
        );
    }

    #[test]
    fn deep_floor_bonus_keys_at_step() {
        // B10F (深層) のボス撃破で鍵が 1 (base) + 2 (deep) = 3 もらえる。
        let mut s = AbyssState::new();
        s.floor = 10;
        s.floor_kind = FloorKind::Normal;
        s.kills_on_floor = s.enemies_per_floor();
        s.upgrades[UpgradeKind::Sword.index()] = 500;
        s.upgrades[UpgradeKind::Speed.index()] = 20;
        s.hero_hp = s.hero_max_hp();
        let keys_before = s.keys;
        for _ in 0..1000 {
            tick(&mut s, 1);
            if s.keys > keys_before {
                break;
            }
        }
        assert!(
            s.keys >= keys_before + 3,
            "B10F boss should drop +3 keys, got {}",
            s.keys - keys_before
        );
    }

    #[test]
    fn floor_kind_resets_on_death() {
        let mut s = AbyssState::new();
        s.floor = 5;
        // 先に 1 tick 進めて敵を spawn させてから (placeholder を解消)、override する。
        tick(&mut s, 1);
        s.floor_kind = FloorKind::Elite;
        s.hero_hp = 1;
        s.current_enemy.atk_cooldown = 1;
        s.current_enemy.atk = 1000;
        // 1 tick 進めると敵が攻撃 → hero 死亡 → 浅瀬に戻る。
        tick(&mut s, 1);
        assert_eq!(s.floor, 1);
        assert_eq!(s.floor_kind, FloorKind::Normal);
    }

    #[test]
    fn retreat_resets_floor_kind() {
        let mut s = AbyssState::new();
        s.floor = 5;
        s.floor_kind = FloorKind::Treasure;
        retreat(&mut s);
        assert_eq!(s.floor_kind, FloorKind::Normal);
    }

    #[test]
    fn combat_focus_increases_with_attacks() {
        let mut s = ticked_state();
        let initial_focus = s.combat_focus;
        // hero_atk が enemy hp を上回らないように、適度に強い設定を作る。
        // ただしすぐに敵を倒すと focus が運用しきれないので、ターゲットを高 HP に置く。
        s.current_enemy.hp = 10_000;
        s.current_enemy.max_hp = 10_000;
        s.current_enemy.def = 0;
        // 次の hero attack まで進める (atk_period 弱)
        tick(&mut s, 100);
        assert!(
            s.combat_focus > initial_focus,
            "focus should grow after attacks (got {} → {})",
            initial_focus,
            s.combat_focus
        );
    }

    #[test]
    fn combat_focus_shortens_attack_period() {
        let mut s = AbyssState::new();
        let period_at_zero = s.hero_atk_period();
        s.combat_focus = s.config.hero.focus_max;
        let period_at_max = s.hero_atk_period();
        assert!(
            period_at_max < period_at_zero,
            "max focus should shorten period ({} → {})",
            period_at_zero,
            period_at_max
        );
    }

    #[test]
    fn combat_focus_reset_on_death() {
        let mut s = AbyssState::new();
        s.combat_focus = s.config.hero.focus_max;
        s.floor = 30;
        s.hero_hp = 1;
        // 敵を強制的にスポーン → 1 tick で死亡
        tick(&mut s, 1);
        s.current_enemy.atk = 9999;
        s.current_enemy.atk_cooldown = 1;
        tick(&mut s, 1);
        assert!(s.deaths > 0);
        assert_eq!(s.combat_focus, 0, "death should reset focus");
    }

    #[test]
    fn combat_focus_reset_on_retreat() {
        let mut s = AbyssState::new();
        s.floor = 5;
        s.combat_focus = 20;
        retreat(&mut s);
        assert_eq!(s.combat_focus, 0);
    }

    #[test]
    fn config_swap_changes_enemy_scaling() {
        // 同 seed / 同 floor で config を変えると敵 HP が変わることを確認。
        // 本体ゲームと sim の DI が機能している証拠。
        let mut seed_a = 42;
        let mut seed_b = 42;
        let easy = BalanceConfig::easy();
        let hard = BalanceConfig::hard();
        let f = 15;
        let easy_enemy = make_enemy(f, false, &easy, &mut seed_a);
        let hard_enemy = make_enemy(f, false, &hard, &mut seed_b);
        assert!(hard_enemy.max_hp > easy_enemy.max_hp);
    }
}
