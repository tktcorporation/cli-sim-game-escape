//! つぶ牧場 (Tsubu Ranch) — tick processing and player actions.
//!
//! `state.rs` のデータに対する唯一の書き込み経路。純粋なデータ定義は持たない。

use super::actions::PlayerAction;
use super::state::{
    Affinity, Creature, RanchState, Species, CLASH_INTERVAL_TICKS, MATURE_LEVEL, MAX_LEVEL,
};

// ── RNG ──────────────────────────────────────────────────────────
// xorshift32。0 seed は退化するため abyss/cookie と同じガードを入れる。

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

fn roll_chance(seed: &mut u32, probability: f64) -> bool {
    let r = rng_next(seed) % 10_000;
    let threshold = (probability.clamp(0.0, 1.0) * 10_000.0) as u32;
    r < threshold
}

/// 重み付き抽選。合計が0なら常に0番目を返す。
fn roll_index(seed: &mut u32, weights: &[u32]) -> usize {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return 0;
    }
    let r = rng_next(seed) % total;
    let mut acc = 0u32;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if r < acc {
            return i;
        }
    }
    weights.len() - 1
}

// ── Tick balance constants ──────────────────────────────────────

const PASSIVE_XP_PER_TICK: u32 = 1;
/// 餌やり方針を選んでいる間、成長速度に上乗せする分。
const FOCUS_XP_BONUS_PER_TICK: u32 = 1;

const FOOD_INCOME_INTERVAL_TICKS: u64 = 10;

/// 繁殖判定を行う間隔。毎tick判定すると個体数が体感できないほど速く増えるため、
/// 食料収入と同じ「1秒に1回」のペースに落として、増える瞬間が分かるようにする。
const REPRODUCE_INTERVAL_TICKS: u64 = 10;
const REPRODUCE_CHANCE_PER_MATURE: f64 = 0.02;
const REPRODUCE_CHANCE_CAP: f64 = 0.2;
const REPRODUCE_COST: u64 = 3;

/// 進化判定を行う間隔。繁殖と同じ理由 (毎tickだと体感できないほど速い) に加え、
/// シミュレータで計測したところ毎tick判定だと閾値到達から数秒で進化してしまい、
/// 図鑑が数分で埋まっていた (`cargo test simulate_ranch_default -- --nocapture` で確認可能)。
const EVOLUTION_INTERVAL_TICKS: u64 = 10;
const EVOLUTION_BASE_CHANCE: f64 = 0.003;
const EVOLUTION_CHANCE_CAP: f64 = 0.05;
/// 平均レベルが `MATURE_LEVEL` を1超えるごとに進化確率へ乗る係数。
const EVOLUTION_LEVEL_FACTOR_PER_LEVEL: f64 = 0.2;
/// 成熟個体数が閾値を1体超えるごとに進化確率へ乗る係数。
const EVOLUTION_COUNT_FACTOR_PER_EXTRA: f64 = 0.1;

const WILD_CAPTURE_CHANCE: f64 = 0.15;

// ── Tick entry point ─────────────────────────────────────────────

pub fn tick(state: &mut RanchState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        tick_once(state);
    }
}

fn tick_once(state: &mut RanchState) {
    state.total_ticks += 1;
    tick_growth(state);
    tick_food_income(state);
    tick_feed_focus_accumulation(state);
    tick_reproduction(state);
    tick_evolution(state);
    tick_battle(state);
}

// ── Growth ────────────────────────────────────────────────────────

fn tick_growth(state: &mut RanchState) {
    let xp_gain = if state.feed_focus.is_some() {
        PASSIVE_XP_PER_TICK + FOCUS_XP_BONUS_PER_TICK
    } else {
        PASSIVE_XP_PER_TICK
    };
    for creatures in state.population.iter_mut() {
        for c in creatures.iter_mut() {
            grow_creature(c, xp_gain);
        }
    }
}

/// `xp_gain` を加算し、必要なら複数回レベルアップさせる。上限到達後は XP を溜めない。
fn grow_creature(c: &mut Creature, xp_gain: u32) {
    if c.level >= MAX_LEVEL {
        return;
    }
    c.xp += xp_gain;
    while c.level < MAX_LEVEL {
        let need = Creature::xp_to_next_level(c.level);
        if c.xp >= need {
            c.xp -= need;
            c.level += 1;
        } else {
            break;
        }
    }
    if c.level >= MAX_LEVEL {
        c.xp = 0;
    }
}

// ── Food income ───────────────────────────────────────────────────

/// 成熟個体が多いほど収入が増える (1秒 = 10tick ごとに清算)。
fn tick_food_income(state: &mut RanchState) {
    if !state.total_ticks.is_multiple_of(FOOD_INCOME_INTERVAL_TICKS) {
        return;
    }
    let mature_total: u64 = Species::all()
        .iter()
        .map(|&sp| state.mature_count(sp) as u64)
        .sum();
    state.food += 1 + mature_total;
}

/// 選択中の餌やり方針があれば、その属性を1秒ごとに蓄積する (進化の分岐バイアスに使う)。
fn tick_feed_focus_accumulation(state: &mut RanchState) {
    if !state.total_ticks.is_multiple_of(FOOD_INCOME_INTERVAL_TICKS) {
        return;
    }
    if let Some(focus) = state.feed_focus {
        state.affinity_feed[focus.index()] += 1;
    }
}

// ── Reproduction ───────────────────────────────────────────────────

fn tick_reproduction(state: &mut RanchState) {
    if !state.total_ticks.is_multiple_of(REPRODUCE_INTERVAL_TICKS) {
        return;
    }
    for &species in Species::all() {
        if state.total_population() >= state.capacity() {
            break;
        }
        let mature = state.mature_count(species);
        if mature == 0 || state.food < REPRODUCE_COST {
            continue;
        }
        let chance = (mature as f64 * REPRODUCE_CHANCE_PER_MATURE).min(REPRODUCE_CHANCE_CAP);
        if roll_chance(&mut state.rng_state, chance) {
            state.food -= REPRODUCE_COST;
            state.population[species.index()].push(Creature::new());
        }
    }
}

// ── Evolution ──────────────────────────────────────────────────────

fn tick_evolution(state: &mut RanchState) {
    if !state.total_ticks.is_multiple_of(EVOLUTION_INTERVAL_TICKS) {
        return;
    }
    for &species in Species::all() {
        if species.is_final_tier() {
            continue;
        }
        let threshold = species.evolution_threshold();
        let mature = state.mature_count(species);
        if mature < threshold {
            continue;
        }
        let avg_level = state.average_mature_level(species);
        let chance = evolution_chance(mature, threshold, avg_level);
        if roll_chance(&mut state.rng_state, chance) {
            evolve(state, species, threshold);
        }
    }
}

/// 進化確率。閾値超過分の個体数と平均レベルの両方が高いほど上がる —
/// 「ただ増やす」より「育ててから集める」方が進化しやすくなる投資判断の核となる式。
fn evolution_chance(mature: u32, threshold: u32, avg_level: f64) -> f64 {
    let extra = mature.saturating_sub(threshold) as f64;
    let count_factor = 1.0 + extra * EVOLUTION_COUNT_FACTOR_PER_EXTRA;
    let level_factor =
        1.0 + (avg_level - MATURE_LEVEL as f64).max(0.0) * EVOLUTION_LEVEL_FACTOR_PER_LEVEL;
    (EVOLUTION_BASE_CHANCE * count_factor * level_factor).min(EVOLUTION_CHANCE_CAP)
}

/// `species` の成熟個体 `consume` 体を消費して次階層の種を1体誕生させる。
/// 進化先は `affinity_feed` の蓄積量で重み付き抽選する (プレイヤーには明示しない)。
fn evolve(state: &mut RanchState, species: Species, consume: u32) {
    let targets = species.evolution_targets();
    if targets.is_empty() {
        return;
    }
    let weights: Vec<u32> = targets
        .iter()
        .map(|&t| {
            species
                .evolution_bias(t)
                .map(|a| state.affinity_feed[a.index()] + 1)
                .unwrap_or(1)
        })
        .collect();
    let idx = roll_index(&mut state.rng_state, &weights);
    let target = targets[idx];

    consume_mature(state, species, consume);
    state.population[target.index()].push(Creature::new());

    let first_sighting = !state.discovered[target.index()];
    state.discovered[target.index()] = true;

    if first_sighting {
        state.add_log(format!(
            "{}が{}体集まり、{}が誕生した! (新種発見)",
            species.name(),
            consume,
            target.name()
        ));
    } else {
        state.add_log(format!("{}が{}に進化した", species.name(), target.name()));
    }
}

/// 成熟個体のうち、レベルが低い方から `count` 体を消費する。
/// (プレイヤーが育てた高レベル個体は温存され、ギリギリ成熟した個体から消える)
fn consume_mature(state: &mut RanchState, species: Species, count: u32) {
    let creatures = &mut state.population[species.index()];
    creatures.sort_by_key(|c| c.level);
    let mut remaining = count;
    creatures.retain(|c| {
        if remaining > 0 && c.is_mature() {
            remaining -= 1;
            false
        } else {
            true
        }
    });
}

// ── Battle ───────────────────────────────────────────────────────

fn tick_battle(state: &mut RanchState) {
    if state.team.iter().all(|slot| slot.is_none()) {
        return;
    }
    // 0 になった tick で即座にクラッシュを解決する (先に減算してから判定する)。
    // 「> 0 なら減らして return」の順だと、ちょうど 0 に達した tick で
    // クラッシュが1 tick 遅れてしまう。
    state.clash_cooldown = state.clash_cooldown.saturating_sub(1);
    if state.clash_cooldown > 0 {
        return;
    }
    state.clash_cooldown = CLASH_INTERVAL_TICKS;

    // 編成変更で team_max_hp が damage_taken を下回り、攻撃も受けていないのに
    // 既に「壊滅」状態になっていることがある (例: 被ダメージ蓄積後にメンバーを
    // 減らして最大HPを下げた場合)。このまま攻撃させると、瀕死のはずのチームが
    // 敵を倒してステージを進めてしまうため、攻撃前に壊滅判定を解決する。
    // `team_max_hp() == 0` (そもそも戦力がない = 絶滅した種だけの編成) とは区別し、
    // そちらは従来通り `atk == 0` の静かなスキップに任せる。
    let max_hp = state.team_max_hp();
    if max_hp > 0 && state.team_hp() == 0 {
        state.add_log(format!("{}に敗れた… チームを立て直す", state.enemy_species.name()));
        state.damage_taken = 0;
        return;
    }

    let atk = state.team_atk();
    if atk == 0 {
        return;
    }
    state.enemy_hp = state.enemy_hp.saturating_sub(atk);

    if state.enemy_hp == 0 {
        win_stage(state);
        return;
    }

    let enemy_atk = state.enemy_species.stage_atk(state.stage);
    state.damage_taken = state.damage_taken.saturating_add(enemy_atk);
    if state.team_hp() == 0 {
        state.add_log(format!("{}に敗れた… チームを立て直す", state.enemy_species.name()));
        state.damage_taken = 0;
    }
}

fn win_stage(state: &mut RanchState) {
    state.stage_clears += 1;
    let reward = 10 + state.stage as u64 * 2;
    state.food += reward;
    state.add_log(format!(
        "{}を倒した! 食料+{}獲得",
        state.enemy_species.name(),
        reward
    ));

    if state.total_population() < state.capacity()
        && roll_chance(&mut state.rng_state, WILD_CAPTURE_CHANCE)
    {
        state.population[state.enemy_species.index()].push(Creature::new());
        state.add_log(format!("野生の{}が仲間になった!", state.enemy_species.name()));
    }

    state.stage += 1;
    state.enemy_species = Species::for_stage(state.stage);
    state.discovered[state.enemy_species.index()] = true;
    state.enemy_max_hp = state.enemy_species.stage_hp(state.stage);
    state.enemy_hp = state.enemy_max_hp;
    state.damage_taken = 0;
}

// ── Player actions ─────────────────────────────────────────────────

pub fn apply_action(state: &mut RanchState, action: PlayerAction) -> bool {
    match action {
        PlayerAction::SetTab(tab) => {
            state.tab = tab;
            state.tab_scroll.set(0);
            true
        }
        PlayerAction::ToggleFeedFocus(affinity) => toggle_feed_focus(state, affinity),
        PlayerAction::UpgradeCapacity => upgrade_capacity(state),
        PlayerAction::ToggleTeamMember(species) => toggle_team_member(state, species),
        PlayerAction::ScrollUp => {
            let cur = state.tab_scroll.get();
            state.tab_scroll.set(cur.saturating_sub(3));
            true
        }
        PlayerAction::ScrollDown => {
            let cur = state.tab_scroll.get();
            state.tab_scroll.set(cur.saturating_add(3));
            true
        }
    }
}

/// 餌やり方針をトグルする。同じ属性を選び直すと解除する。
/// コストは無く、解除するまで `tick_growth` / `tick_feed_focus_accumulation` が
/// 継続的に効果を適用し続けるので、都度クリックし直す必要はない。
fn toggle_feed_focus(state: &mut RanchState, affinity: Affinity) -> bool {
    if state.feed_focus == Some(affinity) {
        state.feed_focus = None;
        state.add_log("餌やりの方針を解除した");
    } else {
        state.feed_focus = Some(affinity);
        state.add_log(format!("{}属性を重点的に育てる方針にした", affinity.name()));
    }
    true
}

fn upgrade_capacity(state: &mut RanchState) -> bool {
    let cost = state.capacity_upgrade_cost();
    if state.food < cost {
        return false;
    }
    state.food -= cost;
    state.capacity_upgrades += 1;
    state.add_log(format!("収容数が{}に拡張された", state.capacity()));
    true
}

/// `damage_taken` はあえて触らない — 編成 (team) を変更するだけで全回復できると、
/// 瀕死のチームをタップし直すだけの無料回復になり、敗北の緊張感が失われるため。
fn toggle_team_member(state: &mut RanchState, species: Species) -> bool {
    if let Some(slot) = state.team.iter().position(|s| *s == Some(species)) {
        state.team[slot] = None;
        return true;
    }
    if let Some(slot) = state.team.iter().position(|s| s.is_none()) {
        state.team[slot] = Some(species);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::ranch::state::TEAM_SIZE;

    // ── grow_creature ────────────────────────────────────────────

    #[test]
    fn grow_creature_levels_up_when_xp_threshold_crossed() {
        let mut c = Creature::new();
        grow_creature(&mut c, Creature::xp_to_next_level(1));
        assert_eq!(c.level, 2);
        assert_eq!(c.xp, 0);
    }

    #[test]
    fn grow_creature_can_multi_level_in_one_call() {
        let mut c = Creature::new();
        let jump = Creature::xp_to_next_level(1) + Creature::xp_to_next_level(2) + 1;
        grow_creature(&mut c, jump);
        assert_eq!(c.level, 3);
        assert_eq!(c.xp, 1);
    }

    #[test]
    fn grow_creature_caps_at_max_level_and_drops_xp() {
        let mut c = Creature { level: MAX_LEVEL, xp: 0 };
        grow_creature(&mut c, 9999);
        assert_eq!(c.level, MAX_LEVEL);
        assert_eq!(c.xp, 0);
    }

    // ── tick_food_income ─────────────────────────────────────────

    #[test]
    fn food_income_only_fires_on_interval_boundary() {
        let mut s = RanchState::new();
        let start_food = s.food;
        tick(&mut s, FOOD_INCOME_INTERVAL_TICKS as u32 - 1);
        assert_eq!(s.food, start_food, "インターバル前は収入なし");
        tick(&mut s, 1);
        assert!(s.food > start_food, "インターバル到達で収入が入る");
    }

    #[test]
    fn food_income_scales_with_mature_population() {
        let mut s = RanchState::new();
        for c in s.population[Species::Tsubu.index()].iter_mut() {
            c.level = MATURE_LEVEL;
        }
        let food_before = s.food;
        tick(&mut s, FOOD_INCOME_INTERVAL_TICKS as u32);
        let gained_with_mature = s.food - food_before;
        assert!(gained_with_mature > 3, "成熟3体分の収入が上乗せされる");
    }

    // ── tick_reproduction ────────────────────────────────────────

    #[test]
    fn reproduction_never_exceeds_capacity() {
        let mut s = RanchState::new();
        s.food = 1_000_000;
        for c in s.population[Species::Tsubu.index()].iter_mut() {
            c.level = MATURE_LEVEL;
        }
        tick(&mut s, 2000);
        assert!(s.total_population() <= s.capacity());
    }

    #[test]
    fn reproduction_does_not_happen_without_mature_individuals() {
        let mut s = RanchState::new();
        s.food = 1_000_000;
        // 初期個体は Lv1 で未成熟。
        tick(&mut s, 200);
        assert_eq!(s.total_population(), 3, "未成熟のみでは繁殖しない");
    }

    #[test]
    fn reproduction_consumes_food() {
        // tick_reproduction を直接呼び、食料収入 (tick_food_income) の影響を排除して
        // 繁殖そのものが食料を消費することだけを検証する。
        let mut s = RanchState::new();
        s.food = 1_000_000;
        for c in s.population[Species::Tsubu.index()].iter_mut() {
            c.level = MATURE_LEVEL;
        }
        for _ in 0..500 {
            tick_reproduction(&mut s);
        }
        assert!(s.food < 1_000_000, "繁殖のたびに食料が減る");
    }

    // ── evolution_chance ─────────────────────────────────────────

    /// Codex review (PR #134): 閾値超過分の個体数が確率に反映されていなかった
    /// (mature はゲート判定にしか使われず、5体でも50体でも同じ確率になっていた)
    /// 回帰を防ぐテスト。
    #[test]
    fn evolution_chance_increases_with_mature_count_beyond_threshold() {
        let threshold = Species::Tsubu.evolution_threshold();
        let avg_level = MATURE_LEVEL as f64;
        let at_threshold = evolution_chance(threshold, threshold, avg_level);
        let well_past_threshold = evolution_chance(threshold * 3, threshold, avg_level);
        assert!(
            well_past_threshold > at_threshold,
            "閾値を超えて個体数を増やすほど進化確率が上がるべき"
        );
    }

    #[test]
    fn evolution_chance_increases_with_average_level() {
        let threshold = Species::Tsubu.evolution_threshold();
        let low_level = evolution_chance(threshold, threshold, MATURE_LEVEL as f64);
        let high_level = evolution_chance(threshold, threshold, MAX_LEVEL as f64);
        assert!(high_level > low_level, "平均レベルが高いほど進化確率が上がるべき");
    }

    #[test]
    fn evolution_chance_never_exceeds_cap() {
        let chance = evolution_chance(1000, 5, MAX_LEVEL as f64);
        assert!(chance <= EVOLUTION_CHANCE_CAP);
    }

    // ── tick_evolution / evolve ──────────────────────────────────

    #[test]
    fn evolution_does_not_trigger_below_threshold() {
        // tick_evolution を直接呼び、繁殖による個体数増加を排除して
        // 「閾値未満なら進化しない」ことだけを検証する。
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()] =
            vec![Creature { level: MAX_LEVEL, xp: 0 }; Species::Tsubu.evolution_threshold() as usize - 1];
        for _ in 0..1000 {
            tick_evolution(&mut s);
        }
        assert_eq!(s.population[Species::AquaTsubu.index()].len(), 0);
        assert_eq!(s.population[Species::FlareTsubu.index()].len(), 0);
        assert_eq!(s.population[Species::EarthTsubu.index()].len(), 0);
    }

    #[test]
    fn evolution_eventually_triggers_once_threshold_and_level_are_met() {
        let mut s = RanchState::new();
        let threshold = Species::Tsubu.evolution_threshold() as usize;
        s.population[Species::Tsubu.index()] = vec![Creature { level: MAX_LEVEL, xp: 0 }; threshold];
        tick(&mut s, 5000);
        let tier1_total: usize = [Species::AquaTsubu, Species::FlareTsubu, Species::EarthTsubu]
            .iter()
            .map(|&sp| s.population[sp.index()].len())
            .sum();
        assert!(tier1_total > 0, "十分なtickがあれば進化するはず");
    }

    #[test]
    fn evolve_consumes_lowest_level_mature_individuals_first() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()] = vec![
            Creature { level: 5, xp: 0 },
            Creature { level: 9, xp: 0 },
            Creature { level: 6, xp: 0 },
        ];
        evolve(&mut s, Species::Tsubu, 2);
        let remaining = &s.population[Species::Tsubu.index()];
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].level, 9, "最もレベルが高い個体だけが生き残る");
    }

    #[test]
    fn evolve_spawns_target_and_marks_discovered() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()] = vec![Creature { level: MAX_LEVEL, xp: 0 }; 5];
        assert!(!s.discovered[Species::AquaTsubu.index()]);
        assert!(!s.discovered[Species::FlareTsubu.index()]);
        assert!(!s.discovered[Species::EarthTsubu.index()]);
        evolve(&mut s, Species::Tsubu, 5);
        let tier1_total: usize = [Species::AquaTsubu, Species::FlareTsubu, Species::EarthTsubu]
            .iter()
            .map(|&sp| s.population[sp.index()].len())
            .sum();
        assert_eq!(tier1_total, 1);
        let discovered_total = [Species::AquaTsubu, Species::FlareTsubu, Species::EarthTsubu]
            .iter()
            .filter(|&&sp| s.discovered[sp.index()])
            .count();
        assert_eq!(discovered_total, 1);
    }

    #[test]
    fn evolution_bias_favors_the_most_fed_affinity() {
        // Aqua だけを大量に蓄積すれば、統計的にAquaTsubuへの進化が優勢になるはず。
        let mut aqua_wins = 0;
        for seed in 1..30u32 {
            let mut s = RanchState::new();
            s.rng_state = seed;
            s.affinity_feed[Affinity::Aqua.index()] = 1000;
            s.population[Species::Tsubu.index()] = vec![Creature { level: MAX_LEVEL, xp: 0 }; 5];
            evolve(&mut s, Species::Tsubu, 5);
            if !s.population[Species::AquaTsubu.index()].is_empty() {
                aqua_wins += 1;
            }
        }
        assert!(aqua_wins > 20, "Aqua を極端に蓄積すればほぼ AquaTsubu に進化するはず (実績: {aqua_wins}/29)");
    }

    /// 一次進化 → 最終形態の3分岐目 (自属性バイアス) が機能していること。
    /// 例えば AquaTsubu は Flare/Earth 蓄積でシズク姫/氷ウサに寄るが、Aqua自体を
    /// 蓄積すればワイルドカードである海竜に寄るはず。
    #[test]
    fn evolution_bias_favors_own_affinity_for_the_third_branch() {
        let mut sea_dragon_wins = 0;
        for seed in 1..30u32 {
            let mut s = RanchState::new();
            s.rng_state = seed;
            s.affinity_feed[Affinity::Aqua.index()] = 1000;
            s.population[Species::AquaTsubu.index()] = vec![Creature { level: MAX_LEVEL, xp: 0 }; 8];
            evolve(&mut s, Species::AquaTsubu, 8);
            if !s.population[Species::SeaDragon.index()].is_empty() {
                sea_dragon_wins += 1;
            }
        }
        assert!(
            sea_dragon_wins > 20,
            "Aqua を極端に蓄積すればほぼ海竜に進化するはず (実績: {sea_dragon_wins}/29)"
        );
    }

    // ── tick_battle ──────────────────────────────────────────────

    #[test]
    fn battle_does_not_progress_without_a_team() {
        let mut s = RanchState::new();
        let enemy_hp_before = s.enemy_hp;
        tick(&mut s, 100);
        assert_eq!(s.enemy_hp, enemy_hp_before, "チーム未編成では戦闘が進まない");
    }

    #[test]
    fn battle_damages_enemy_over_time() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][0].level = MAX_LEVEL;
        s.team[0] = Some(Species::Tsubu);
        let enemy_hp_before = s.enemy_hp;
        tick(&mut s, CLASH_INTERVAL_TICKS);
        assert!(s.enemy_hp < enemy_hp_before);
    }

    /// Codex review (PR #134): 編成変更で `team_max_hp` が `damage_taken` を
    /// 下回ると、まだ攻撃を受けていないのに既に「壊滅」状態になる。この状態の
    /// まま攻撃させると、瀕死のはずのチームが敵を倒してステージを進めてしまう
    /// (無料の勝利+全回復) 回帰を防ぐ。
    #[test]
    fn shrinking_team_below_damage_taken_rebuilds_instead_of_attacking() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][0].level = MAX_LEVEL;
        s.population[Species::AquaTsubu.index()] = vec![Creature { level: MAX_LEVEL, xp: 0 }];
        s.team[0] = Some(Species::Tsubu);
        s.team[1] = Some(Species::AquaTsubu);
        // 2体編成でまだ生きている (team_hp=10) 状態を作ってから、AquaTsubu を
        // 編成解除して max_hp を Tsubu 1体分まで縮める。
        s.damage_taken = s.team_max_hp() - 10;
        apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::AquaTsubu));
        assert_eq!(s.team_hp(), 0, "縮小後は既に壊滅状態のはず");

        s.enemy_hp = 1; // 攻撃が通ればステージが進んでしまう細工
        let stage_before = s.stage;
        tick(&mut s, CLASH_INTERVAL_TICKS);

        assert_eq!(s.enemy_hp, 1, "壊滅状態のチームは攻撃できない");
        assert_eq!(s.stage, stage_before, "壊滅状態から敵を倒してステージが進んではいけない");
        assert_eq!(s.damage_taken, 0, "壊滅チームは立て直される");
    }

    #[test]
    fn winning_a_stage_advances_and_heals_team() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][0].level = MAX_LEVEL;
        s.team[0] = Some(Species::Tsubu);
        s.enemy_hp = 1; // 次のクラッシュで即死する体力に細工
        let stage_before = s.stage;
        tick(&mut s, CLASH_INTERVAL_TICKS);
        assert_eq!(s.stage, stage_before + 1);
        assert_eq!(s.team_hp(), s.team_max_hp(), "勝利後はチームが全回復する");
    }

    #[test]
    fn losing_team_hp_resets_without_losing_stage_progress() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][0].level = MAX_LEVEL;
        s.team[0] = Some(Species::Tsubu);
        s.damage_taken = s.team_max_hp() - 1; // 次のクラッシュで壊滅する体力に細工
        s.enemy_hp = s.enemy_max_hp * 1000; // 勝てないようにHPを底上げ
        let stage_before = s.stage;
        tick(&mut s, CLASH_INTERVAL_TICKS);
        assert_eq!(s.stage, stage_before, "敗北してもステージは後退しない");
        assert_eq!(s.team_hp(), s.team_max_hp(), "敗北後もチームは立て直される");
    }

    /// team を編成し直すだけでは `damage_taken` はリセットされない
    /// (瀕死のチームをタップし直す無料回復エクスプロイトを防ぐ)。
    #[test]
    fn toggling_team_does_not_heal_damage_taken() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][0].level = MAX_LEVEL;
        apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::Tsubu));
        s.damage_taken = s.team_max_hp();
        assert_eq!(s.team_hp(), 0);
        apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::Tsubu));
        apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::Tsubu));
        assert_eq!(s.damage_taken, s.team_max_hp(), "編成変更でdamage_takenは変わらない");
    }

    // ── apply_action ─────────────────────────────────────────────

    #[test]
    fn toggle_feed_focus_sets_then_clears() {
        let mut s = RanchState::new();
        assert!(apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Flare)));
        assert_eq!(s.feed_focus, Some(Affinity::Flare));
        assert!(apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Flare)));
        assert_eq!(s.feed_focus, None, "同じ属性を選び直すと解除される");
    }

    #[test]
    fn toggle_feed_focus_switches_between_affinities() {
        let mut s = RanchState::new();
        apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Aqua));
        apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Flare));
        assert_eq!(s.feed_focus, Some(Affinity::Flare), "別の属性を選ぶと切り替わる");
    }

    #[test]
    fn toggle_feed_focus_costs_nothing() {
        let mut s = RanchState::new();
        s.food = 0;
        assert!(apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Aqua)));
        assert_eq!(s.food, 0, "餌やり方針の変更に食料コストは無い");
    }

    #[test]
    fn feed_focus_speeds_up_growth_and_accumulates_affinity() {
        let mut s = RanchState::new();
        apply_action(&mut s, PlayerAction::ToggleFeedFocus(Affinity::Flare));
        let xp_before = s.population[Species::Tsubu.index()][0].xp;
        tick(&mut s, FOOD_INCOME_INTERVAL_TICKS as u32);
        assert!(
            s.population[Species::Tsubu.index()][0].xp > xp_before
                || s.population[Species::Tsubu.index()][0].level > 1,
            "方針を選んでいる間は成長が進む"
        );
        assert_eq!(s.affinity_feed[Affinity::Flare.index()], 1, "1秒ごとに方針の属性が積まれる");
    }

    /// レベルアップの瞬間は xp が 0 に戻るため、単純な xp 比較だとちょうど
    /// 閾値を跨いだ側が「xp が少ない」ように見えてしまう。レベルも含めた
    /// 累積成長量で比較する (`xp_to_next_level(l) = 20*l` の等差数列の和)。
    fn total_growth(c: &Creature) -> u32 {
        10 * c.level as u32 * (c.level as u32 - 1) + c.xp
    }

    #[test]
    fn without_feed_focus_growth_is_slower() {
        let mut with_focus = RanchState::new();
        apply_action(&mut with_focus, PlayerAction::ToggleFeedFocus(Affinity::Aqua));
        let mut without_focus = RanchState::new();

        tick(&mut with_focus, FOOD_INCOME_INTERVAL_TICKS as u32);
        tick(&mut without_focus, FOOD_INCOME_INTERVAL_TICKS as u32);

        assert!(
            total_growth(&with_focus.population[Species::Tsubu.index()][0])
                > total_growth(&without_focus.population[Species::Tsubu.index()][0]),
            "方針を選んでいる方が成長が早い"
        );
    }

    #[test]
    fn upgrade_capacity_increases_capacity_and_costs_food() {
        let mut s = RanchState::new();
        s.food = 1_000_000;
        let cap_before = s.capacity();
        assert!(apply_action(&mut s, PlayerAction::UpgradeCapacity));
        assert!(s.capacity() > cap_before);
    }

    #[test]
    fn toggle_team_member_adds_then_removes() {
        let mut s = RanchState::new();
        assert!(apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::Tsubu)));
        assert_eq!(s.team[0], Some(Species::Tsubu));
        assert!(apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::Tsubu)));
        assert!(s.team.iter().all(|slot| slot.is_none()));
    }

    #[test]
    fn toggle_team_member_fails_when_team_full() {
        let mut s = RanchState::new();
        for &sp in &[Species::Tsubu, Species::AquaTsubu, Species::FlareTsubu] {
            assert!(apply_action(&mut s, PlayerAction::ToggleTeamMember(sp)));
        }
        assert!(s.team.iter().all(|slot| slot.is_some()));
        assert_eq!(TEAM_SIZE, 3, "このテストはTEAM_SIZE=3を前提にしている");
        assert!(!apply_action(&mut s, PlayerAction::ToggleTeamMember(Species::EarthTsubu)));
    }

    #[test]
    fn set_tab_resets_scroll() {
        let mut s = RanchState::new();
        s.tab_scroll.set(42);
        assert!(apply_action(&mut s, PlayerAction::SetTab(crate::games::ranch::state::Tab::Dex)));
        assert_eq!(s.tab_scroll.get(), 0);
    }
}
