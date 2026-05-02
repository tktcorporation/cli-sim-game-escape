//! 深淵潜行 — シミュレーションランナー (難易度調整用)。
//!
//! 本体ゲームと完全に同じ `logic::tick` / `logic::apply_action` を駆動する。
//! 違いは Policy が UI 入力ではなく自動行動を返すことだけ。難易度調整は
//! `BalanceConfig` を差し替えるだけで済む ─ 効果は構造的に同等。
//!
//! 実行例:
//! ```bash
//! cargo test simulate_abyss_default -- --nocapture
//! cargo test simulate_abyss_balance_sweep -- --nocapture
//! ```
//!
//! 構造:
//! - `Policy` trait: 各 tick で `PlayerAction` のリストを返す主体。
//! - `Simulator`:    state + policy + metrics をまとめて run() する駆動部。
//! - `SimMetrics`:   フロア到達時刻・gold/HP サンプル・死亡履歴を記録する観測部。

#![cfg(test)]

use super::config::BalanceConfig;
use super::logic;
use super::policy::PlayerAction;
use super::state::{AbyssState, SoulPerk, UpgradeKind};

// ───────────────────────────────────────────────────────────────
// Policy: 自動プレイヤーの抽象。
// ───────────────────────────────────────────────────────────────

/// 行動を選び続ける主体。本体ゲームは入力ハンドラ、シミュレータは Policy 実装。
pub trait Policy {
    /// 現在の state を見て、この瞬間に取りたい行動を返す。
    /// 空 vec を返せば「何もしない」。
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction>;

    /// シミュレーション開始時に 1 度だけ呼ばれる。初期設定 (auto_descend ON など) に。
    fn on_start(&mut self, _state: &AbyssState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 何もしない Policy。「強化ゼロのまま放置すると何階まで行けるか?」を測る。
pub struct NoActionPolicy;

impl Policy for NoActionPolicy {
    fn choose_actions(&mut self, _state: &AbyssState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 一番安い強化を買い続ける貪欲 Policy。素朴な「だれでも遊べる」プレイヤーの近似。
pub struct GreedyCheapestPolicy {
    pub spend_souls: bool,
}

impl Default for GreedyCheapestPolicy {
    fn default() -> Self {
        Self { spend_souls: true }
    }
}

impl Policy for GreedyCheapestPolicy {
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction> {
        let mut actions = Vec::new();
        if let Some((kind, cost)) = cheapest_upgrade(state) {
            if state.gold >= cost {
                actions.push(PlayerAction::BuyUpgrade(kind));
            }
        }
        if self.spend_souls {
            if let Some((perk, cost)) = cheapest_soul_perk(state) {
                if state.souls >= cost {
                    actions.push(PlayerAction::BuySoulPerk(perk));
                }
            }
        }
        actions
    }
}

/// 戦略的 Policy: weights に従って upgrade を伸ばす。
///
/// `weights` は (Sword, Vitality, Armor, Crit, Speed, Regen, Gold) の優先順位。
/// 値が大きい upgrade ほど早めに買う ─ 「現 level / weight」が最小のものを選ぶ方式。
pub struct WeightedPolicy {
    pub weights: [f64; 7],
    pub spend_souls: bool,
}

impl WeightedPolicy {
    pub fn balanced() -> Self {
        Self {
            weights: [3.0, 3.0, 1.5, 1.0, 1.5, 1.0, 1.5],
            spend_souls: true,
        }
    }

    pub fn offense() -> Self {
        Self {
            weights: [4.0, 1.5, 0.5, 2.5, 3.0, 0.5, 1.0],
            spend_souls: true,
        }
    }

    pub fn defense() -> Self {
        Self {
            weights: [1.5, 4.0, 3.0, 0.5, 1.0, 2.5, 1.0],
            spend_souls: true,
        }
    }
}

impl Policy for WeightedPolicy {
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction> {
        let mut actions = Vec::new();
        let mut best: Option<(UpgradeKind, f64)> = None;
        for kind in UpgradeKind::all() {
            let w = self.weights[kind.index()];
            if w <= 0.0 {
                continue;
            }
            let lv = state.upgrades[kind.index()] as f64;
            let cost = state.upgrade_cost(*kind);
            if state.gold < cost {
                continue;
            }
            let score = lv / w;
            if best.is_none_or(|(_, s)| score < s) {
                best = Some((*kind, score));
            }
        }
        if let Some((kind, _)) = best {
            actions.push(PlayerAction::BuyUpgrade(kind));
        }
        if self.spend_souls {
            if let Some((perk, cost)) = cheapest_soul_perk(state) {
                if state.souls >= cost {
                    actions.push(PlayerAction::BuySoulPerk(perk));
                }
            }
        }
        actions
    }
}

fn cheapest_upgrade(state: &AbyssState) -> Option<(UpgradeKind, u64)> {
    UpgradeKind::all()
        .iter()
        .map(|k| (*k, state.upgrade_cost(*k)))
        .min_by_key(|(_, c)| *c)
}

fn cheapest_soul_perk(state: &AbyssState) -> Option<(SoulPerk, u64)> {
    SoulPerk::all()
        .iter()
        .map(|p| (*p, state.soul_perk_cost(*p)))
        .min_by_key(|(_, c)| *c)
}

// ───────────────────────────────────────────────────────────────
// Simulator: 駆動部。
// ───────────────────────────────────────────────────────────────

/// シミュレーション中に集まるメトリクス。難易度調整の判断材料。
#[derive(Clone, Debug, Default)]
pub struct SimMetrics {
    pub total_ticks: u64,
    pub deepest_floor: u32,
    pub final_floor: u32,
    pub final_gold: u64,
    pub final_souls: u64,
    pub total_kills: u64,
    pub deaths: u64,

    /// (tick, floor) のサンプル。
    pub floor_samples: Vec<(u64, u32)>,
    /// (tick, gold) のサンプル。
    pub gold_samples: Vec<(u64, u64)>,
    /// (tick, hero_hp_ratio) のサンプル。
    pub hp_samples: Vec<(u64, f64)>,

    /// 各死亡時のフロア。
    pub death_floors: Vec<u32>,
    /// 初到達: floor → tick。
    pub first_reached: Vec<(u32, u64)>,
}

impl SimMetrics {
    pub fn report(&self) -> String {
        let secs = self.total_ticks as f64 / 10.0;
        let mut s = String::new();
        s.push_str("── Abyss Sim Report ────────────────────────\n");
        s.push_str(&format!(
            "経過: {} ticks ({:.1} 秒 / {:.1} 分)\n",
            self.total_ticks,
            secs,
            secs / 60.0
        ));
        s.push_str(&format!("最深フロア: B{}F\n", self.deepest_floor));
        s.push_str(&format!("最終フロア: B{}F\n", self.final_floor));
        s.push_str(&format!(
            "最終 gold/souls: {} / {}\n",
            self.final_gold, self.final_souls
        ));
        s.push_str(&format!(
            "総撃破: {}, 死亡数: {}\n",
            self.total_kills, self.deaths
        ));
        s.push_str(&format!(
            "kills/min: {:.1}\n",
            self.total_kills as f64 / (secs / 60.0).max(1.0)
        ));

        if !self.first_reached.is_empty() {
            s.push_str("到達時刻 (フロア → 分):\n");
            // 主要マイルストーンのみ抜粋: B2,5,10,15,20,30,50
            let milestones = [2u32, 5, 10, 15, 20, 30, 50];
            for &m in &milestones {
                if let Some((_, t)) = self.first_reached.iter().find(|(f, _)| *f == m) {
                    s.push_str(&format!(
                        "  B{:>3}F: {:>6.1} 分 ({} ticks)\n",
                        m,
                        *t as f64 / 600.0,
                        t
                    ));
                }
            }
        }

        if !self.death_floors.is_empty() {
            s.push_str("死亡フロア: ");
            for (i, f) in self.death_floors.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&format!("B{}F", f));
                if i >= 9 {
                    s.push_str(", …");
                    break;
                }
            }
            s.push('\n');
        }

        s
    }
}

/// シミュレータ。本体ゲームの「Game trait 実装」に対応するもの ─ ただし UI なし。
pub struct Simulator {
    pub state: AbyssState,
    policy: Box<dyn Policy>,
    metrics: SimMetrics,
    /// メトリクスのサンプリング間隔 (tick 単位)。0 ならサンプリングしない。
    pub sample_every: u64,
    last_seen_floor: u32,
    last_seen_deaths: u64,
}

impl Simulator {
    pub fn new(config: BalanceConfig, policy: Box<dyn Policy>) -> Self {
        Self::with_seed(config, policy, 0xC0FFEE)
    }

    pub fn with_seed(config: BalanceConfig, mut policy: Box<dyn Policy>, seed: u32) -> Self {
        let mut state = AbyssState::with_config(config);
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
            last_seen_floor: 1,
            last_seen_deaths: 0,
        }
    }

    /// `total_ticks` ぶんシミュレーションを進める。
    /// 内部では 1 tick ずつループし、毎 tick で
    /// `policy.choose_actions` → `apply_action` → `logic::tick(1)` の順に実行する。
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

        if self.state.floor > self.last_seen_floor {
            for f in (self.last_seen_floor + 1)..=self.state.floor {
                self.metrics.first_reached.push((f, cur_tick));
            }
            self.last_seen_floor = self.state.floor;
        }

        if self.state.deaths > self.last_seen_deaths {
            self.metrics.death_floors.push(self.last_seen_floor);
            self.last_seen_deaths = self.state.deaths;
            self.last_seen_floor = self.state.floor;
        }

        if self.sample_every > 0 && cur_tick.is_multiple_of(self.sample_every) {
            self.metrics.floor_samples.push((cur_tick, self.state.floor));
            self.metrics.gold_samples.push((cur_tick, self.state.gold));
            let max = self.state.hero_max_hp().max(1) as f64;
            self.metrics
                .hp_samples
                .push((cur_tick, self.state.hero_hp as f64 / max));
        }
    }

    fn finalize(&mut self) {
        self.metrics.deepest_floor = self.state.deepest_floor_ever;
        self.metrics.final_floor = self.state.floor;
        self.metrics.final_gold = self.state.gold;
        self.metrics.final_souls = self.state.souls;
        self.metrics.total_kills = self.state.total_kills;
        self.metrics.deaths = self.state.deaths;
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

    #[test]
    fn no_action_player_stays_shallow() {
        let mut sim = Simulator::new(BalanceConfig::default(), Box::new(NoActionPolicy));
        sim.run(6_000);
        assert!(
            sim.metrics().deepest_floor < 10,
            "no-action policy should stay shallow, got B{}",
            sim.metrics().deepest_floor
        );
    }

    #[test]
    fn greedy_player_progresses() {
        let mut sim = Simulator::new(
            BalanceConfig::default(),
            Box::new(GreedyCheapestPolicy::default()),
        );
        sim.run(6_000);
        assert!(
            sim.metrics().deepest_floor >= 2,
            "greedy should reach at least B2F, got B{}",
            sim.metrics().deepest_floor
        );
        assert!(sim.metrics().total_kills > 5);
    }

    #[test]
    fn balanced_at_least_matches_no_action() {
        let mut sim_a = Simulator::with_seed(
            BalanceConfig::default(),
            Box::new(NoActionPolicy),
            0xA1A1A1,
        );
        let mut sim_b = Simulator::with_seed(
            BalanceConfig::default(),
            Box::new(WeightedPolicy::balanced()),
            0xA1A1A1,
        );
        sim_a.run(6_000);
        sim_b.run(6_000);
        assert!(
            sim_b.metrics().deepest_floor >= sim_a.metrics().deepest_floor,
            "balanced (B{}) should match or beat no-action (B{})",
            sim_b.metrics().deepest_floor,
            sim_a.metrics().deepest_floor
        );
    }

    #[test]
    fn hard_config_makes_it_harder() {
        let mut sim_easy = Simulator::with_seed(
            BalanceConfig::easy(),
            Box::new(GreedyCheapestPolicy::default()),
            0xBEEF,
        );
        let mut sim_hard = Simulator::with_seed(
            BalanceConfig::hard(),
            Box::new(GreedyCheapestPolicy::default()),
            0xBEEF,
        );
        sim_easy.run(6_000);
        sim_hard.run(6_000);
        assert!(
            sim_easy.metrics().deepest_floor >= sim_hard.metrics().deepest_floor,
            "easy (B{}) should reach deeper than hard (B{})",
            sim_easy.metrics().deepest_floor,
            sim_hard.metrics().deepest_floor
        );
    }

    #[test]
    fn deterministic_with_same_seed() {
        let run = || {
            let mut sim = Simulator::with_seed(
                BalanceConfig::default(),
                Box::new(GreedyCheapestPolicy::default()),
                0x12345,
            );
            sim.run(3_000);
            (
                sim.metrics().deepest_floor,
                sim.metrics().total_kills,
                sim.metrics().final_gold,
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn metrics_records_first_reached() {
        let mut sim = Simulator::with_seed(
            BalanceConfig::easy(),
            Box::new(WeightedPolicy::offense()),
            0x1111,
        );
        sim.run(6_000);
        if sim.metrics().deepest_floor >= 2 {
            assert!(!sim.metrics().first_reached.is_empty());
        }
    }

    #[test]
    fn report_renders() {
        let mut sim = Simulator::new(
            BalanceConfig::default(),
            Box::new(GreedyCheapestPolicy::default()),
        );
        sim.run(1_200);
        let report = sim.report();
        assert!(report.contains("最深フロア"));
    }
}

// ───────────────────────────────────────────────────────────────
// Tuning runners (cargo test simulate_abyss_* -- --nocapture)
// ───────────────────────────────────────────────────────────────
//
// 印字された結果を見ながら BalanceConfig を弄る。eprintln! を使うのは cookie の
// シミュレータと同じ流儀 (--nocapture でだけ可視化される)。

mod runners {
    use super::*;

    fn run_and_print(label: &str, config: BalanceConfig, mut policy: Box<dyn Policy>, ticks: u64) {
        let _ = &mut policy; // silence in case future Policy needs &mut for setup
        let mut sim = Simulator::new(config, policy);
        sim.run(ticks);
        eprintln!("\n══ {} ══", label);
        eprint!("{}", sim.report());
    }

    /// 既定バランスで Greedy / Balanced / Offense / Defense を比較。
    /// `cargo test simulate_abyss_default -- --nocapture` で可視化。
    #[test]
    fn simulate_abyss_default() {
        let ticks = 36_000; // 60 分相当 (10 ticks/sec * 60 * 60)
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Abyss Idle Balance Sim — preset: default, ticks: {}  ┃", ticks);
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        run_and_print(
            "default + Greedy",
            BalanceConfig::default(),
            Box::new(GreedyCheapestPolicy::default()),
            ticks,
        );
        run_and_print(
            "default + Balanced",
            BalanceConfig::default(),
            Box::new(WeightedPolicy::balanced()),
            ticks,
        );
        run_and_print(
            "default + Offense",
            BalanceConfig::default(),
            Box::new(WeightedPolicy::offense()),
            ticks,
        );
        run_and_print(
            "default + Defense",
            BalanceConfig::default(),
            Box::new(WeightedPolicy::defense()),
            ticks,
        );
    }

    /// 難易度プリセットを横断: easy / default / hard を Balanced Policy で比較。
    /// 「最深フロア到達時刻が 2 倍以上ズレるか?」を見て難易度差の妥当性を判断する。
    /// `cargo test simulate_abyss_balance_sweep -- --nocapture`
    #[test]
    fn simulate_abyss_balance_sweep() {
        let ticks = 36_000;
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Abyss Idle Balance Sweep — Balanced policy, 60min     ┃");
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        for (label, cfg) in [
            ("easy", BalanceConfig::easy()),
            ("default", BalanceConfig::default()),
            ("hard", BalanceConfig::hard()),
        ] {
            run_and_print(label, cfg, Box::new(WeightedPolicy::balanced()), ticks);
        }
    }
}
