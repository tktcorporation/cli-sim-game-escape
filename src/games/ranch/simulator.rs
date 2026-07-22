//! つぶ牧場 — シミュレーションランナー (バランス調整用)。
//!
//! 本体ゲームと完全に同じ `logic::tick` / `logic::apply_action` を駆動する。
//! 違いは Policy が UI 入力ではなく自動行動を返すことだけ。
//!
//! 実行例:
//! ```bash
//! cargo test simulate_ranch_default -- --nocapture
//! cargo test simulate_ranch_long_run -- --nocapture
//! ```

#![cfg(test)]

use super::actions::PlayerAction;
use super::logic;
use super::state::{Affinity, RanchState, Species, SPECIES_COUNT};

// ───────────────────────────────────────────────────────────────
// Policy: 自動プレイヤーの抽象。
// ───────────────────────────────────────────────────────────────

pub trait Policy {
    fn choose_actions(&mut self, state: &RanchState) -> Vec<PlayerAction>;
    fn on_start(&mut self, _state: &RanchState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 何もしない Policy。「操作ゼロでも増殖・成長・進化は自動で進むか?」を測る基準線。
/// 対戦チームを一切編成しないので、ステージは B1 のまま進まないのが期待値。
pub struct NoActionPolicy;

impl Policy for NoActionPolicy {
    fn choose_actions(&mut self, _state: &RanchState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 「1つの属性に餌やり方針を固定し、収容数が買えたら買い、チームは常に最強3体で
/// 埋める」貪欲 Policy。素朴な「とりあえず育てて放置する」プレイヤーの近似。
///
/// `rotate_every` を指定すると、一定間隔で3属性を巡回する「頭打ちに気付いたら
/// 方針を変える」プレイヤーの近似になる。1属性に固定し続けると進化系統が偏り、
/// 対戦チームに使える種の多様性が乏しくなる (`pick_team_action` はレベルしか
/// 見ないため、選択肢が少ないと弱い構成に固定されがち) — その影響を切り分けて
/// 測るための比較用バリエーション。
pub struct AutoRanchPolicy {
    pub focus: Affinity,
    rotate_every: Option<u64>,
}

impl AutoRanchPolicy {
    pub fn new(focus: Affinity) -> Self {
        Self { focus, rotate_every: None }
    }

    pub fn rotating(interval_ticks: u64) -> Self {
        Self { focus: Affinity::Aqua, rotate_every: Some(interval_ticks) }
    }

    fn desired_focus(&self, state: &RanchState) -> Affinity {
        match self.rotate_every {
            Some(interval) if interval > 0 => {
                const ORDER: [Affinity; 3] = [Affinity::Aqua, Affinity::Flare, Affinity::Earth];
                ORDER[(state.total_ticks / interval) as usize % ORDER.len()]
            }
            _ => self.focus,
        }
    }
}

impl Policy for AutoRanchPolicy {
    fn choose_actions(&mut self, state: &RanchState) -> Vec<PlayerAction> {
        let mut actions = Vec::new();

        let desired = self.desired_focus(state);
        if state.feed_focus != Some(desired) {
            actions.push(PlayerAction::ToggleFeedFocus(desired));
        }

        if state.food >= state.capacity_upgrade_cost() {
            actions.push(PlayerAction::UpgradeCapacity);
        }

        if let Some(action) = pick_team_action(state) {
            actions.push(action);
        }

        actions
    }
}

/// チーム編成を1手だけ進める: 絶滅した種が編成中なら解除、空きスロットが
/// あれば未編成の種の中で最もレベルが高い個体を持つ種を追加する。
fn pick_team_action(state: &RanchState) -> Option<PlayerAction> {
    for &sp in state.team.iter().flatten() {
        if state.population[sp.index()].is_empty() {
            return Some(PlayerAction::ToggleTeamMember(sp));
        }
    }

    if state.team.iter().any(|slot| slot.is_none()) {
        let candidate = Species::all()
            .iter()
            .copied()
            .filter(|&sp| !state.team.contains(&Some(sp)))
            .filter_map(|sp| state.strongest(sp).map(|c| (sp, c.level)))
            .max_by_key(|&(_, lv)| lv);
        if let Some((sp, _)) = candidate {
            return Some(PlayerAction::ToggleTeamMember(sp));
        }
    }

    None
}

// ───────────────────────────────────────────────────────────────
// Simulator: 駆動部。
// ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct SimMetrics {
    pub total_ticks: u64,
    pub final_stage: u32,
    pub final_food: u64,
    pub final_population: u32,
    pub final_capacity: u32,
    pub final_discovered: usize,
    pub stage_clears: u64,
    pub capacity_upgrades: u32,

    pub population_samples: Vec<(u64, u32)>,
    pub stage_samples: Vec<(u64, u32)>,
    pub food_samples: Vec<(u64, u64)>,

    /// 各種の初発見 tick (`Species::index()` 順)。`None` = 未発見。
    /// 繁殖による進化でもワイルド遭遇でも、`discovered` が立った瞬間を記録する。
    pub species_first_discovered_tick: [Option<u64>; SPECIES_COUNT],
}

impl SimMetrics {
    pub fn report(&self) -> String {
        let secs = self.total_ticks as f64 / 10.0;
        let mut s = String::new();
        s.push_str("── Ranch Sim Report ────────────────────────\n");
        s.push_str(&format!(
            "経過: {} ticks ({:.1} 分 / {:.1} 時間)\n",
            self.total_ticks,
            secs / 60.0,
            secs / 3600.0
        ));
        s.push_str(&format!(
            "最終ステージ: {} (クリア{}回)\n",
            self.final_stage, self.stage_clears
        ));
        s.push_str(&format!(
            "最終個体数: {}/{} (収容拡張{}回)\n",
            self.final_population, self.final_capacity, self.capacity_upgrades
        ));
        s.push_str(&format!("最終食料: {}\n", self.final_food));
        s.push_str(&format!(
            "発見数: {}/{}\n",
            self.final_discovered, SPECIES_COUNT
        ));

        s.push_str("種の初発見時刻 (種 → 分):\n");
        for &sp in Species::all() {
            match self.species_first_discovered_tick[sp.index()] {
                Some(t) => s.push_str(&format!(
                    "  {:<10} (tier{}): {:>7.1} 分 ({:>4.1} h)\n",
                    sp.name(),
                    sp.tier(),
                    t as f64 / 600.0,
                    t as f64 / 36000.0
                )),
                None => s.push_str(&format!("  {:<10} (tier{}): 未発見\n", sp.name(), sp.tier())),
            }
        }

        s
    }
}

pub struct Simulator {
    pub state: RanchState,
    policy: Box<dyn Policy>,
    metrics: SimMetrics,
    pub sample_every: u64,
}

impl Simulator {
    pub fn new(policy: Box<dyn Policy>) -> Self {
        Self::with_seed(policy, 0xC0FFEE)
    }

    pub fn with_seed(mut policy: Box<dyn Policy>, seed: u32) -> Self {
        let mut state = RanchState::new();
        state.rng_state = if seed == 0 { 0xC0FFEE } else { seed };

        let init_actions = policy.on_start(&state);
        for a in init_actions {
            logic::apply_action(&mut state, a);
        }

        Self {
            state,
            policy,
            metrics: SimMetrics::default(),
            sample_every: 600,
        }
    }

    pub fn run(&mut self, total_ticks: u64) {
        for _ in 0..total_ticks {
            self.step_one();
        }
        self.finalize();
    }

    fn step_one(&mut self) {
        let actions = self.policy.choose_actions(&self.state);
        for a in actions {
            logic::apply_action(&mut self.state, a);
        }

        logic::tick(&mut self.state, 1);

        self.metrics.total_ticks += 1;
        let cur_tick = self.metrics.total_ticks;

        for &sp in Species::all() {
            let idx = sp.index();
            if self.state.discovered[idx] && self.metrics.species_first_discovered_tick[idx].is_none() {
                self.metrics.species_first_discovered_tick[idx] = Some(cur_tick);
            }
        }

        if self.sample_every > 0 && cur_tick.is_multiple_of(self.sample_every) {
            self.metrics
                .population_samples
                .push((cur_tick, self.state.total_population()));
            self.metrics.stage_samples.push((cur_tick, self.state.stage));
            self.metrics.food_samples.push((cur_tick, self.state.food));
        }
    }

    fn finalize(&mut self) {
        self.metrics.final_stage = self.state.stage;
        self.metrics.final_food = self.state.food;
        self.metrics.final_population = self.state.total_population();
        self.metrics.final_capacity = self.state.capacity();
        self.metrics.final_discovered = self.state.discovered.iter().filter(|&&d| d).count();
        self.metrics.stage_clears = self.state.stage_clears;
        self.metrics.capacity_upgrades = self.state.capacity_upgrades;
    }

    pub fn metrics(&self) -> &SimMetrics {
        &self.metrics
    }

    pub fn report(&self) -> String {
        self.metrics.report()
    }
}

// ───────────────────────────────────────────────────────────────
// Tests / Sanity checks
// ───────────────────────────────────────────────────────────────

mod sanity_tests {
    use super::*;

    /// `PlayerAction::ScrollUp/ScrollDown` は UI only。simulator policy が
    /// 絶対に生成しないことを確認する。
    #[test]
    fn simulator_policies_never_emit_scroll() {
        let state = RanchState::new();
        let policies: Vec<Box<dyn Policy>> = vec![
            Box::new(NoActionPolicy),
            Box::new(AutoRanchPolicy::new(Affinity::Aqua)),
        ];
        for mut policy in policies {
            for _ in 0..100 {
                for action in policy.choose_actions(&state) {
                    assert!(
                        !matches!(action, PlayerAction::ScrollUp | PlayerAction::ScrollDown),
                        "policy emitted UI-only action: {:?}",
                        action
                    );
                }
            }
        }
    }

    /// バランス崩壊の回帰防止: 進化判定が毎tick(0.1秒間隔)のままだった旧実装では、
    /// 自動方針プレイで1時間どころか2分足らずで図鑑が完全制覇されていた
    /// (`cargo test simulate_ranch_default -- --nocapture` で実測して発覚)。
    /// 収集ゲームとして進行に意味を持たせるため、1時間では埋まりきらないことを保証する。
    #[test]
    fn auto_policy_does_not_complete_dex_within_one_hour() {
        let mut sim = Simulator::new(Box::new(AutoRanchPolicy::new(Affinity::Aqua)));
        sim.run(36_000);
        assert!(
            sim.metrics().final_discovered < SPECIES_COUNT,
            "1時間で図鑑が完全制覇されるのは早すぎる (収集の進行がすぐ終わってしまう)"
        );
    }

    /// バランス崩壊の回帰防止: 敵のステージスケーリングが線形だった旧実装では、
    /// 自動方針プレイのチームが無双し1時間で200ステージ超クリアしていた。
    /// 指数関数的スケーリングでチームの成長曲線に追いつく壁を作った。
    #[test]
    fn auto_policy_does_not_blitz_through_stages() {
        let mut sim = Simulator::new(Box::new(AutoRanchPolicy::new(Affinity::Aqua)));
        sim.run(36_000);
        assert!(
            sim.metrics().final_stage < 200,
            "1時間で200ステージ以上進むのは対戦の難易度カーブがチームの成長に追いついていない (got {})",
            sim.metrics().final_stage
        );
    }

    /// 操作ゼロでも成長・繁殖・進化は自動で進む (対戦だけがプレイヤー依存)。
    #[test]
    fn no_action_policy_still_grows_and_evolves() {
        let mut sim = Simulator::new(Box::new(NoActionPolicy));
        sim.run(36_000); // 1時間
        assert!(
            sim.metrics().final_population > 3,
            "無操作でも繁殖で個体数は増えるはず (got {})",
            sim.metrics().final_population
        );
        assert!(
            sim.metrics().final_discovered > 1,
            "無操作でも1時間あれば進化で新種が発見されるはず (発見数: {})",
            sim.metrics().final_discovered
        );
        assert_eq!(
            sim.metrics().final_stage,
            1,
            "チーム未編成なら対戦は進まずステージ1のまま"
        );
    }

    /// 自動方針(餌固定 + 収容拡張 + チーム自動編成)は無操作より対戦が進む。
    #[test]
    fn auto_policy_progresses_stage_further_than_no_action() {
        let mut sim_no_action =
            Simulator::with_seed(Box::new(NoActionPolicy), 0xA1A1A1);
        let mut sim_auto =
            Simulator::with_seed(Box::new(AutoRanchPolicy::new(Affinity::Aqua)), 0xA1A1A1);
        sim_no_action.run(36_000);
        sim_auto.run(36_000);
        assert!(
            sim_auto.metrics().final_stage > sim_no_action.metrics().final_stage,
            "自動方針(stage={})は無操作(stage={})よりステージが進むはず",
            sim_auto.metrics().final_stage,
            sim_no_action.metrics().final_stage
        );
    }

    #[test]
    fn population_never_exceeds_capacity() {
        let mut sim = Simulator::new(Box::new(AutoRanchPolicy::new(Affinity::Flare)));
        sim.run(36_000);
        for &(tick, pop) in &sim.metrics().population_samples {
            assert!(
                pop <= sim.state.capacity(),
                "tick {tick} で個体数({pop})が収容数({})を超えている",
                sim.state.capacity()
            );
        }
    }

    /// 長時間の自動運用で収容数が初期値のまま止まらないこと (経済が回っている証拠)。
    #[test]
    fn auto_policy_eventually_upgrades_capacity() {
        let mut sim = Simulator::new(Box::new(AutoRanchPolicy::new(Affinity::Earth)));
        sim.run(36_000);
        assert!(
            sim.metrics().capacity_upgrades > 0,
            "1時間の自動運用で収容数拡張が1回も起きないのは食料経済が詰んでいる"
        );
    }

    /// 1つの属性に餌やり方針を固定し続けると、統計的にその属性へ偏った進化が
    /// 優勢になる (`evolution_bias_favors_the_most_fed_affinity` の統合版)。
    #[test]
    fn focused_policy_biases_evolution_toward_chosen_affinity() {
        let mut aqua_first = 0;
        for seed in 1..20u32 {
            let mut sim = Simulator::with_seed(Box::new(AutoRanchPolicy::new(Affinity::Aqua)), seed);
            sim.run(72_000); // 2時間
            let aqua_tick = sim.metrics().species_first_discovered_tick[Species::AquaTsubu.index()];
            let flare_tick = sim.metrics().species_first_discovered_tick[Species::FlareTsubu.index()];
            let earth_tick = sim.metrics().species_first_discovered_tick[Species::EarthTsubu.index()];
            let earliest = [aqua_tick, flare_tick, earth_tick]
                .iter()
                .flatten()
                .min()
                .copied();
            if earliest.is_some() && earliest == aqua_tick {
                aqua_first += 1;
            }
        }
        assert!(
            aqua_first >= 12,
            "Aqua固定なら大半のシードでAquaTsubuが最初に出るはず (実績: {aqua_first}/19)"
        );
    }

    #[test]
    fn deterministic_with_same_seed() {
        let run = || {
            let mut sim =
                Simulator::with_seed(Box::new(AutoRanchPolicy::new(Affinity::Aqua)), 0x12345);
            sim.run(18_000);
            (
                sim.metrics().final_stage,
                sim.metrics().final_population,
                sim.metrics().final_food,
                sim.metrics().final_discovered,
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn report_renders() {
        let mut sim = Simulator::new(Box::new(AutoRanchPolicy::new(Affinity::Aqua)));
        sim.run(6_000);
        let report = sim.report();
        assert!(report.contains("最終ステージ"));
    }
}

// ───────────────────────────────────────────────────────────────
// Tuning runners (cargo test simulate_ranch_* -- --nocapture)
// ───────────────────────────────────────────────────────────────

mod runners {
    use super::*;

    fn run_and_print(label: &str, policy: Box<dyn Policy>, ticks: u64) {
        let mut sim = Simulator::new(policy);
        sim.run(ticks);
        eprintln!("\n══ {} ══", label);
        eprint!("{}", sim.report());
    }

    /// 1時間プレイでの基本挙動を、無操作 / 自動方針(属性違い)で比較。
    #[test]
    fn simulate_ranch_default() {
        let ticks = 36_000; // 60 分
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Ranch Balance Sim — 60 分                             ┃");
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        run_and_print("NoAction", Box::new(NoActionPolicy), ticks);
        run_and_print("Auto + Aqua固定", Box::new(AutoRanchPolicy::new(Affinity::Aqua)), ticks);
        run_and_print("Auto + Flare固定", Box::new(AutoRanchPolicy::new(Affinity::Flare)), ticks);
        run_and_print("Auto + Earth固定", Box::new(AutoRanchPolicy::new(Affinity::Earth)), ticks);
        run_and_print("Auto + 10分毎に巡回", Box::new(AutoRanchPolicy::rotating(6_000)), ticks);
    }

    /// 24h ロングランで最終形態まで発見できるか、対戦がどこまで進むかを見る。
    /// `cargo test simulate_ranch_long_run -- --nocapture`
    #[test]
    fn simulate_ranch_long_run() {
        let ticks = 864_000; // 24h
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Ranch Long Run — 24h, Auto + Aqua固定                 ┃");
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        let mut sim = Simulator::with_seed(Box::new(AutoRanchPolicy::new(Affinity::Aqua)), 0xC0FFEE);
        sim.run(ticks);
        eprintln!("{}", sim.report());
    }
}
