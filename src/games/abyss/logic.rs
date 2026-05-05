//! 深淵潜行 — 純粋ロジック関数群。
//!
//! tick / spawn / 攻撃判定 / 装備購入・装着・強化など全てここに集約する。
//! `state.rs` のフィールドだけを操作し、IOやレンダーは触らない。
//!
//! 数値バランスは `state.config` (BalanceConfig) から取り出すので、
//! 本体ゲームとシミュレータで完全に同じコードを通る。

use super::config::BalanceConfig;
use super::policy::PlayerAction;
use super::state::{
    AbyssState, Enemy, EquipmentId, FloorKind, GachaResultSummary, GachaTier, SoulPerk, Tab,
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

    for _ in 0..delta_ticks {
        step_one_tick(state);
        state.total_ticks += 1;
    }
}

fn step_one_tick(state: &mut AbyssState) {
    // ── HP regen ──
    if state.hero_hp > 0 && state.hero_hp < state.hero_max_hp() {
        let regen_per_tick_x100 = (state.hero_regen_per_sec() * 10.0).round() as u32;
        state.hero_regen_acc_x100 = state.hero_regen_acc_x100.saturating_add(regen_per_tick_x100);
        if state.hero_regen_acc_x100 >= 100 {
            let heal = (state.hero_regen_acc_x100 / 100) as u64;
            state.hero_regen_acc_x100 %= 100;
            let max = state.hero_max_hp();
            state.hero_hp = (state.hero_hp + heal).min(max);
        }
    }

    // ── 敵が居ない → 新しい敵スポーン ──
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
        let div = pacing.normal_souls_div.max(1);
        state.floor.div_ceil(div) as u64
    };
    let souls = (base_souls as f64 * state.soul_multiplier()).round() as u64;
    state.souls = state.souls.saturating_add(souls);

    state.run_kills = state.run_kills.saturating_add(1);
    state.total_kills = state.total_kills.saturating_add(1);

    if was_boss {
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
        if state.auto_descend {
            descend_to_next_floor(state);
        } else {
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

fn spawn_next_enemy(state: &mut AbyssState) {
    let is_boss = state.kills_on_floor >= state.enemies_per_floor();
    let mut e = make_enemy(state.floor, is_boss, &state.config, &mut state.rng_state);
    apply_floor_kind_to_enemy(&mut e, state.floor_kind);
    state.current_enemy = e;
}

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

pub fn make_enemy(floor: u32, is_boss: bool, config: &BalanceConfig, rng_seed: &mut u32) -> Enemy {
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
    let mut hp = e.hp_base * config.enemy_hp_schedule.multiplier(floor);
    let mut atk = e.atk_base * config.enemy_atk_schedule.multiplier(floor);
    let mut def = e.def_base + (f - 1.0) * e.def_per_floor;
    let mut gold = e.gold_base * config.enemy_gold_schedule.multiplier(floor);

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

/// 装備の購入条件 (前装備 prerequisite) を満たしているか判定する。
/// gold は別途チェック。UI 側で「未解放だが解放可能」を表示するために使う。
pub fn equipment_requirements_met(state: &AbyssState, id: EquipmentId) -> bool {
    let def = match state.config.equipment.get(id.index()) {
        Some(d) => d,
        None => return false,
    };
    if let Some(prereq) = def.prerequisite {
        if !state.owned_equipment[prereq.index()] {
            return false;
        }
    }
    true
}

/// 装備を 1 個購入する。条件未達 / gold 不足 / 既に所持済みなら false。
///
/// 購入直後は **その lane に自動装着** する (空スロットも、既に何かが装着されている
/// 場合も置換)。新装備を買ったらすぐ使いたい、という idle UX を素直に表現するため。
/// 旧装備に戻したいときは `equip_item(prev_id)` を呼べば良い (所持装備からの装着切替は無料)。
pub fn buy_equipment(state: &mut AbyssState, id: EquipmentId) -> bool {
    if state.owned_equipment[id.index()] {
        return false;
    }
    if !equipment_requirements_met(state, id) {
        return false;
    }
    let def = match state.config.equipment.get(id.index()) {
        Some(d) => d,
        None => return false,
    };
    let cost = def.gold_cost;
    if state.gold < cost {
        return false;
    }
    state.gold -= cost;
    let name = def.name;
    let label = def.effect_label;
    let lane = id.lane();

    // 装備が変わると max_hp も変わる。max が増えた / 減った両方向に hero_hp を追従させる:
    //   - 増えた場合: delta だけ底上げ (低 HP からの装備変更で over-heal しないが、現 HP は増加分上がる)
    //   - 減った場合: hero_hp を新 max にクランプ (旧 max で生き残ってた hero_hp の取り残しを防ぐ)
    // `equip_item` と同じ分岐にすることで「装備変更時の HP 追従」を 1 つの責務として
    // 統一する (片方だけ修正されてバグる事故を防ぐ目的での明示的対称構造)。
    let max_before = state.hero_max_hp();
    state.owned_equipment[id.index()] = true;
    state.equipped[lane.index()] = Some(id);
    let max_after = state.hero_max_hp();
    if max_after > max_before {
        let delta = max_after - max_before;
        state.hero_hp = state.hero_hp.saturating_add(delta).min(max_after);
    } else {
        state.hero_hp = state.hero_hp.min(max_after);
    }

    state.add_log(format!("✦ 装備購入: {} ({}) → 装着", name, label));
    true
}

/// 既に所持している装備を装着する。装着切替は無料。
/// 所持していない、または既にその装備を装着中なら false。
pub fn equip_item(state: &mut AbyssState, id: EquipmentId) -> bool {
    if !state.owned_equipment[id.index()] {
        return false;
    }
    let lane = id.lane();
    if state.equipped[lane.index()] == Some(id) {
        return false; // 既に同じものを装着中
    }
    let max_before = state.hero_max_hp();
    state.equipped[lane.index()] = Some(id);
    let max_after = state.hero_max_hp();
    // max が増えたなら delta だけ現 HP も bump (装備購入と同じ流儀)。
    // max が減った場合は hp を max に切り詰める。
    if max_after > max_before {
        let delta = max_after - max_before;
        state.hero_hp = state.hero_hp.saturating_add(delta).min(max_after);
    } else {
        state.hero_hp = state.hero_hp.min(max_after);
    }
    let name = state
        .config
        .equipment
        .get(id.index())
        .map(|d| d.name)
        .unwrap_or("装備");
    state.add_log(format!("◆ 装着: {}", name));
    true
}

/// 指定装備を 1 段階強化する。所持していなくても呼べるが、UI 側でフィルタするので
/// 通常は所持装備に対してのみ呼ばれる。
pub fn enhance_equipment(state: &mut AbyssState, id: EquipmentId) -> bool {
    let cost = state.enhance_cost(id);
    if state.gold < cost {
        return false;
    }
    // 装備が定義されていない id は弾く (state.config.equipment が SSOT)。
    if state.config.equipment.get(id.index()).is_none() {
        return false;
    }
    state.gold -= cost;

    // 強化対象が装着中なら hero_max_hp が変動するので、装備購入と同じ
    // before/after 差分で hero_hp を底上げする。装着していない場合は変動なし。
    let is_equipped = state.equipped[id.lane().index()] == Some(id);
    let max_before = if is_equipped {
        Some(state.hero_max_hp())
    } else {
        None
    };
    state.equipment_levels[id.index()] = state.equipment_levels[id.index()].saturating_add(1);
    if let Some(before) = max_before {
        let after = state.hero_max_hp();
        let delta = after.saturating_sub(before);
        state.hero_hp = state.hero_hp.saturating_add(delta).min(after);
    }

    let name = state
        .config
        .equipment
        .get(id.index())
        .map(|d| d.name)
        .unwrap_or("装備");
    let lv = state.equipment_levels[id.index()];
    state.add_log(format!("◆ {} +{}", name, lv));
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
    state.tab_scroll.set(0);
}

/// プレイヤー行動を適用する統一エントリ。
pub fn apply_action(state: &mut AbyssState, action: PlayerAction) -> bool {
    match action {
        PlayerAction::BuyEquipment(id) => buy_equipment(state, id),
        PlayerAction::EquipItem(id) => equip_item(state, id),
        PlayerAction::EnhanceEquipment(id) => enhance_equipment(state, id),
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
            let v = state.tab_scroll.get().saturating_sub(SCROLL_STEP);
            state.tab_scroll.set(v);
            true
        }
        PlayerAction::ScrollDown => {
            let v = state.tab_scroll.get().saturating_add(SCROLL_STEP);
            state.tab_scroll.set(v);
            true
        }
    }
}

const SCROLL_STEP: u16 = 3;

// ── ガチャ ────────────────────────────────────────────────

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
        gained_enh_lv: 0,
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

fn roll_gacha_tier(state: &mut AbyssState) -> GachaTier {
    let g = &state.config.gacha;
    let pity_active = g.gacha_pity > 0 && state.pulls_since_epic >= g.gacha_pity.saturating_sub(1);
    if pity_active {
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
            // Rare 報酬: 装着中装備のうちランダム 1 つの強化 Lv +1。
            // 装備が進行軸の主役になったので「装備の強化を直接ブースト」が
            // 一番ストレートな gacha 報酬。装着していない場合は gold ボーナスにフォールバック。
            let equipped: Vec<EquipmentId> = state
                .equipped
                .iter()
                .filter_map(|s| *s)
                .collect();
            if equipped.is_empty() {
                // 装着なし → Common 相当の gold 大盤振る舞いにフォールバック。
                let base = base_normal_gold(state.floor, &state.config);
                let gold = ((base as f64) * 20.0 * state.gold_multiplier()).round() as u64;
                let gold = gold.max(1);
                state.gold = state.gold.saturating_add(gold);
                state.run_gold_earned = state.run_gold_earned.saturating_add(gold);
                summary.gained_gold = summary.gained_gold.saturating_add(gold);
            } else {
                let idx = (rng_next(&mut state.rng_state) as usize) % equipped.len();
                let target = equipped[idx];
                // hero stats 変動を伴うので、装着中なら hp の bump も忘れずに。
                let max_before = state.hero_max_hp();
                state.equipment_levels[target.index()] =
                    state.equipment_levels[target.index()].saturating_add(1);
                let max_after = state.hero_max_hp();
                let delta = max_after.saturating_sub(max_before);
                state.hero_hp = state.hero_hp.saturating_add(delta).min(max_after);
                summary.gained_enh_lv = summary.gained_enh_lv.saturating_add(1);
            }
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

fn base_normal_gold(floor: u32, config: &BalanceConfig) -> u64 {
    let g = config.enemy.gold_base * config.enemy_gold_schedule.multiplier(floor);
    g.round().max(1.0) as u64
}

/// 自分の意思で浅瀬 (B1F) に戻る。
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
    use crate::games::abyss::state::EquipmentLane;

    fn ticked_state() -> AbyssState {
        let mut s = AbyssState::new();
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

    /// 装備購入が条件未達なら失敗、満たしているなら自動装着まで行うこと。
    #[test]
    fn buy_equipment_auto_equips_into_lane_slot() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000;
        // 銅剣は prereq 無し、購入できる。
        assert!(buy_equipment(&mut s, EquipmentId::BronzeSword));
        assert!(s.owned_equipment[EquipmentId::BronzeSword.index()]);
        assert_eq!(
            s.equipped[EquipmentLane::Weapon.index()],
            Some(EquipmentId::BronzeSword),
            "購入時は自動装着されるべき"
        );
    }

    /// 上位装備を買うと旧装備は所持したままで、装着スロットだけが新しいものに置換される。
    #[test]
    fn buying_higher_tier_replaces_equipped_slot() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        buy_equipment(&mut s, EquipmentId::SteelSword);
        assert!(s.owned_equipment[EquipmentId::BronzeSword.index()]);
        assert!(s.owned_equipment[EquipmentId::SteelSword.index()]);
        assert_eq!(
            s.equipped[EquipmentLane::Weapon.index()],
            Some(EquipmentId::SteelSword)
        );
    }

    /// 既に所持している装備を装着し直せること。
    #[test]
    fn equip_item_can_swap_between_owned() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        buy_equipment(&mut s, EquipmentId::SteelSword); // 自動で SteelSword 装着
        assert!(equip_item(&mut s, EquipmentId::BronzeSword));
        assert_eq!(
            s.equipped[EquipmentLane::Weapon.index()],
            Some(EquipmentId::BronzeSword)
        );
    }

    /// 強化は所持していて装着中なら hero ステを直接押し上げる。
    #[test]
    fn enhance_equipped_increases_stats() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        let atk_before = s.hero_atk();
        assert!(enhance_equipment(&mut s, EquipmentId::BronzeSword));
        assert_eq!(s.equipment_levels[EquipmentId::BronzeSword.index()], 1);
        assert!(s.hero_atk() > atk_before);
    }

    /// 強化コストが gold 不足だと失敗する。
    #[test]
    fn enhance_fails_without_gold() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        s.gold = 0;
        assert!(!enhance_equipment(&mut s, EquipmentId::BronzeSword));
        assert_eq!(s.equipment_levels[EquipmentId::BronzeSword.index()], 0);
    }

    /// 強化 Lv が増えればコストも増える。
    #[test]
    fn enhance_cost_grows_geometrically() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        let c0 = s.enhance_cost(EquipmentId::BronzeSword);
        enhance_equipment(&mut s, EquipmentId::BronzeSword);
        let c1 = s.enhance_cost(EquipmentId::BronzeSword);
        assert!(c1 > c0);
    }

    /// 装備購入時に max_hp が上がっても、現 HP が新 max を超えないこと。
    #[test]
    fn buy_equipment_hp_bump_does_not_overshoot_max() {
        let mut s = AbyssState::new();
        s.gold = 1_000_000_000;
        // 死にかけにしてから防具を買う。装備で max が大幅に上がっても、
        // 現 HP は max を超えないこと。
        s.hero_hp = 1;
        let max_before = s.hero_max_hp();
        assert!(buy_equipment(&mut s, EquipmentId::LeatherArmor));
        let max_after = s.hero_max_hp();
        let delta = max_after - max_before;
        assert!(s.hero_hp <= 1 + delta);
        assert!(s.hero_hp <= max_after);
    }

    /// 装備購入で max_hp が **下がる** ケースでも hero_hp が新 max にクランプされること。
    /// Codex review #87 P2 の回帰防止: カスタム config で同 lane の上位装備を弱体化させ、
    /// 自動装着で max が減ったとき `hero_hp > max` の取り残しが起きないことを保証する。
    /// `equip_item` と同じ「max 増減両方向で hero_hp を追従」スタイルの不変条件。
    #[test]
    fn buy_equipment_clamps_hp_when_new_gear_lowers_max() {
        use crate::games::abyss::state::EquipmentBonus;

        // 上位 (SteelArmor) を意図的に空 bonus にして、LeatherArmor 装着時より max が下がる状況を作る。
        let mut cfg = BalanceConfig::default();
        let steel_idx = EquipmentId::SteelArmor.index();
        cfg.equipment[steel_idx].base_bonus = EquipmentBonus::default();
        cfg.equipment[steel_idx].per_level_bonus = EquipmentBonus::default();

        let mut s = AbyssState::with_config(cfg);
        s.gold = 1_000_000_000;

        // LeatherArmor を買って装着 → 装備込みで max が上がる。
        assert!(buy_equipment(&mut s, EquipmentId::LeatherArmor));
        let max_with_leather = s.hero_max_hp();
        s.hero_hp = max_with_leather; // 満タン

        // SteelArmor を買うと auto-equip で Armor lane が置換され、空 bonus なので max が下がる。
        assert!(buy_equipment(&mut s, EquipmentId::SteelArmor));
        let max_with_steel = s.hero_max_hp();

        // テスト前提: max が実際に下がる装備変更が起きていること。
        assert!(
            max_with_steel < max_with_leather,
            "test setup: SteelArmor (空 bonus) は LeatherArmor より max が低いはず ({} >= {})",
            max_with_steel,
            max_with_leather
        );
        // 不変条件: max が下がっても hero_hp は新 max を超えない。
        assert!(
            s.hero_hp <= max_with_steel,
            "hero_hp ({}) は新 max ({}) を超えてはならない",
            s.hero_hp,
            max_with_steel
        );
    }

    #[test]
    fn killing_enemies_advances_floor_with_auto_descend() {
        let mut s = AbyssState::new();
        // 強い装備を全 lane に装着して確実にフロアを進める。
        s.gold = u64::MAX / 2;
        for &id in EquipmentId::all() {
            // テスト用: 解放条件を全部満たしてから順に購入していく。
            buy_equipment(&mut s, id);
        }
        // 装備の Lv も上げる。
        for &id in EquipmentId::all() {
            for _ in 0..30 {
                enhance_equipment(&mut s, id);
            }
        }
        s.hero_hp = s.hero_max_hp();
        s.auto_descend = true;
        tick(&mut s, 2000);
        assert!(s.floor >= 2, "floor should advance, got {}", s.floor);
    }

    #[test]
    fn no_descend_when_auto_descend_off() {
        let mut s = AbyssState::new();
        s.gold = u64::MAX / 2;
        for &id in EquipmentId::all() {
            buy_equipment(&mut s, id);
        }
        for &id in EquipmentId::all() {
            for _ in 0..20 {
                enhance_equipment(&mut s, id);
            }
        }
        s.hero_hp = s.hero_max_hp();
        s.auto_descend = false;
        let per_floor = s.enemies_per_floor() as u64;
        tick(&mut s, 3000);
        assert_eq!(s.floor, 1);
        assert!(s.run_kills > per_floor);
    }

    #[test]
    fn weak_hero_dies_eventually() {
        let mut s = AbyssState::new();
        s.floor = 30;
        s.auto_descend = false;
        s.hero_hp = s.hero_max_hp();
        tick(&mut s, 10_000);
        assert!(s.deaths > 0 || s.floor == 1);
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
        let mut cfg = BalanceConfig::default();
        cfg.pacing.normal_souls_div = 0;
        let mut s = AbyssState::with_config(cfg);
        s.gold = 1_000_000_000;
        for &id in EquipmentId::all() {
            buy_equipment(&mut s, id);
        }
        for &id in EquipmentId::all() {
            for _ in 0..20 {
                enhance_equipment(&mut s, id);
            }
        }
        s.hero_hp = s.hero_max_hp();
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
        let mut s = AbyssState::new();
        s.keys = 2;
        let ok = gacha_pull(&mut s, 10);
        assert!(ok);
        assert_eq!(s.keys, 0);
        assert_eq!(s.total_pulls, 2);
    }

    #[test]
    fn gacha_pity_forces_epic_or_better() {
        let mut s = AbyssState::new();
        s.keys = 1;
        s.pulls_since_epic = s.config.gacha.gacha_pity.saturating_sub(1);
        gacha_pull(&mut s, 1);
        let r = s.last_gacha.as_ref().unwrap();
        assert_eq!(r.by_tier[2] + r.by_tier[3], 1);
        assert_eq!(s.pulls_since_epic, 0);
    }

    #[test]
    fn gacha_legendary_grants_keys() {
        let mut cfg = BalanceConfig::default();
        cfg.gacha.gacha_weights_milli = [0, 0, 0, 1000];
        let mut s = AbyssState::with_config(cfg);
        s.keys = 1;
        gacha_pull(&mut s, 1);
        assert_eq!(s.keys, s.config.gacha.legendary_keys);
    }

    /// ガチャ Rare は装着中装備の強化 Lv を直接押し上げる。
    /// 装着していない時は gold フォールバックなので、装着あり前提で確認。
    #[test]
    fn gacha_rare_enhances_equipped_item() {
        let mut cfg = BalanceConfig::default();
        cfg.gacha.gacha_weights_milli = [0, 1000, 0, 0]; // 100% Rare
        let mut s = AbyssState::with_config(cfg);
        s.gold = 1_000_000_000;
        buy_equipment(&mut s, EquipmentId::BronzeSword);
        let lv_before = s.equipment_levels[EquipmentId::BronzeSword.index()];
        s.keys = 1;
        gacha_pull(&mut s, 1);
        let lv_after = s.equipment_levels[EquipmentId::BronzeSword.index()];
        assert_eq!(lv_after, lv_before + 1);
    }

    #[test]
    fn floor_kind_first_floors_normal() {
        let cfg = BalanceConfig::default();
        let mut seed = 1;
        for f in 1..cfg.gacha.floor_kind_normal_below {
            let kind = roll_floor_kind(f, &cfg, &mut seed);
            assert_eq!(kind, FloorKind::Normal);
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
    fn config_swap_changes_enemy_scaling() {
        let mut seed_a = 42;
        let mut seed_b = 42;
        let easy = BalanceConfig::easy();
        let hard = BalanceConfig::hard();
        let f = 15;
        let easy_enemy = make_enemy(f, false, &easy, &mut seed_a);
        let hard_enemy = make_enemy(f, false, &hard, &mut seed_b);
        assert!(hard_enemy.max_hp > easy_enemy.max_hp);
    }

    #[test]
    fn combat_focus_shortens_attack_period() {
        let mut s = AbyssState::new();
        let period_at_zero = s.hero_atk_period();
        s.combat_focus = s.config.hero.focus_max;
        let period_at_max = s.hero_atk_period();
        assert!(period_at_max < period_at_zero);
    }

    #[test]
    fn combat_focus_reset_on_retreat() {
        let mut s = AbyssState::new();
        s.floor = 5;
        s.combat_focus = 20;
        retreat(&mut s);
        assert_eq!(s.combat_focus, 0);
    }
}
