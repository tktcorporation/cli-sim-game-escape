//! 深淵潜行 — シミュレーションランナー (難易度調整用)。
//!
//! 本体ゲームと完全に同じ `logic::tick` / `logic::apply_action` を駆動する。
//! 違いは Policy が UI 入力ではなく自動行動を返すことだけ。難易度調整は
//! `BalanceConfig` を差し替えるだけで済む。
//!
//! 進行軸が「装備購入 + 装着 + 強化」中心になったので、Policy も
//! 「buy → equip (auto) → enhance」のサイクルを回す形になっている。
//!
//! 実行例:
//! ```bash
//! cargo test simulate_abyss_default -- --nocapture
//! cargo test simulate_abyss_long_run -- --nocapture
//! ```

#![cfg(test)]

use super::config::BalanceConfig;
use super::logic;
use super::policy::PlayerAction;
use super::state::{AbyssState, EquipmentId, EquipmentLane, SoulPerk, EQUIPMENT_COUNT};

// ───────────────────────────────────────────────────────────────
// Policy: 自動プレイヤーの抽象。
// ───────────────────────────────────────────────────────────────

pub trait Policy {
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction>;
    fn on_start(&mut self, _state: &AbyssState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 何もしない Policy。「装備ゼロのまま放置すると何階まで行けるか?」を測る。
pub struct NoActionPolicy;

impl Policy for NoActionPolicy {
    fn choose_actions(&mut self, _state: &AbyssState) -> Vec<PlayerAction> {
        Vec::new()
    }
}

/// 「次に解放可能な装備を即買う + 装着中装備のうち最安の lane を強化する」貪欲 Policy。
/// シンプルだが装備中心の進行をしっかり回す。素朴な「だれでも遊べる」プレイヤーの近似。
pub struct GreedyEnhancePolicy {
    pub spend_souls: bool,
}

impl Default for GreedyEnhancePolicy {
    fn default() -> Self {
        Self { spend_souls: true }
    }
}

impl Policy for GreedyEnhancePolicy {
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction> {
        let mut actions = Vec::new();

        // (1) 装備解放を最優先 — 全条件 + gold が揃っていれば即購入。
        if let Some(action) = next_buy_equipment(state) {
            actions.push(action);
            return actions; // 1 tick で 1 個ずつ。次の tick でまた拾う。
        }

        // (2) 装着中装備の中で「いま強化するのが最も安い」lane を強化。
        if let Some(action) = cheapest_enhance(state) {
            actions.push(action);
        }

        // (3) 魂パークも安いものから買う。
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

/// lane 別 weight に従って強化を分配する Policy。
///
/// `weights[0]=武器, [1]=防具, [2]=装飾`。値が大きいほどその lane を優先強化する
/// (現 Lv / weight が最小の lane を選ぶ方式 — WeightedPolicy の発想を踏襲)。
pub struct LaneWeightedPolicy {
    pub weights: [f64; 3],
    pub spend_souls: bool,
}

impl LaneWeightedPolicy {
    pub fn balanced() -> Self {
        Self {
            weights: [3.0, 2.0, 1.5],
            spend_souls: true,
        }
    }

    pub fn offense() -> Self {
        Self {
            weights: [4.0, 1.0, 2.0],
            spend_souls: true,
        }
    }

    pub fn defense() -> Self {
        Self {
            weights: [1.5, 4.0, 1.0],
            spend_souls: true,
        }
    }
}

impl Policy for LaneWeightedPolicy {
    fn choose_actions(&mut self, state: &AbyssState) -> Vec<PlayerAction> {
        let mut actions = Vec::new();

        // (1) 装備解放を最優先。
        if let Some(action) = next_buy_equipment(state) {
            actions.push(action);
            return actions;
        }

        // (2) 装着中の lane から weighted で 1 つ選んで強化。
        if let Some(action) = weighted_enhance(state, &self.weights) {
            actions.push(action);
        }

        // (3) 魂パーク。
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

/// 解放可能な装備のうち最も lane_index が浅いものを購入するアクションを返す。
/// 解放不能 / gold 不足なら None。
///
/// 実装の不変条件: **lane_index が小さいものを優先**して走査する。
/// `EquipmentId::all()` の宣言順は「武器全段階 → 防具全段階 → 装飾全段階」なので、
/// 素朴に for ループすると武器に偏った購入順 (例: 銅剣を買った直後に LeatherArmor
/// より先に SteelSword を買ってしまう) になり、policy が doc の主張する
/// 「lane バランス進行」を表現できなくなる (Codex review #87 P2)。
/// `sort_by_key(|id| id.lane_index())` で並び替えてから走査することで、
/// 同 lane_index 内では `EquipmentId::all()` の順 (Weapon→Armor→Accessory) を
/// 保ちつつ、より浅い段階の装備を全 lane で先に拾えるようにする
/// (sort_by_key は stable sort なのでこの tie-break は自動で効く)。
fn next_buy_equipment(state: &AbyssState) -> Option<PlayerAction> {
    let mut by_lane_idx: Vec<EquipmentId> = EquipmentId::all().to_vec();
    by_lane_idx.sort_by_key(|id| id.lane_index());

    for id in by_lane_idx {
        if state.owned_equipment[id.index()] {
            continue;
        }
        if !logic::equipment_requirements_met(state, id) {
            continue;
        }
        if let Some(def) = state.config.equipment.get(id.index()) {
            if state.gold >= def.gold_cost {
                return Some(PlayerAction::BuyEquipment(id));
            }
        }
    }
    None
}

/// 装着中装備のうち、強化コストが最安で gold が足りるものを返す。
fn cheapest_enhance(state: &AbyssState) -> Option<PlayerAction> {
    let mut best: Option<(EquipmentId, u64)> = None;
    for slot in state.equipped.iter().flatten() {
        let cost = state.enhance_cost(*slot);
        if state.gold < cost {
            continue;
        }
        if best.is_none_or(|(_, c)| cost < c) {
            best = Some((*slot, cost));
        }
    }
    best.map(|(id, _)| PlayerAction::EnhanceEquipment(id))
}

/// lane weight に従い「現 Lv / weight が最小」の装着 lane を強化する。
/// gold 不足の lane はスキップ。
fn weighted_enhance(state: &AbyssState, weights: &[f64; 3]) -> Option<PlayerAction> {
    let mut best: Option<(EquipmentId, f64)> = None;
    for &lane in EquipmentLane::all() {
        let id = match state.equipped_at(lane) {
            Some(id) => id,
            None => continue,
        };
        let w = weights[lane.index()];
        if w <= 0.0 {
            continue;
        }
        let cost = state.enhance_cost(id);
        if state.gold < cost {
            continue;
        }
        let lv = state.equipment_levels[id.index()] as f64;
        let score = lv / w;
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((id, score));
        }
    }
    best.map(|(id, _)| PlayerAction::EnhanceEquipment(id))
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

#[derive(Clone, Debug, Default)]
pub struct SimMetrics {
    pub total_ticks: u64,
    pub deepest_floor: u32,
    pub final_floor: u32,
    pub final_gold: u64,
    pub final_souls: u64,
    pub total_kills: u64,
    pub deaths: u64,

    pub floor_samples: Vec<(u64, u32)>,
    pub gold_samples: Vec<(u64, u64)>,
    pub hp_samples: Vec<(u64, f64)>,

    pub death_floors: Vec<u32>,
    pub first_reached: Vec<(u32, u64)>,

    /// 各装備の **初購入 tick** (`EquipmentId::index()` 順)。`None` = 未購入。
    /// 「装備購入が play time に分散しているか」を可視化するための pacing 指標。
    /// late game に bunching していたら balance 調整のサインになる。
    pub equipment_purchase_ticks: [Option<u64>; EQUIPMENT_COUNT],
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
            // 「ゴール=B100」を中心に節目フロアの到達時刻を出す。B25/B50/B75/B100 で
            // 4 等分の sweep が見えるかが pacing チェックの主軸。
            s.push_str("到達時刻 (フロア → 分):\n");
            let milestones = [2u32, 5, 10, 25, 50, 75, 100];
            for &m in &milestones {
                if let Some((_, t)) = self.first_reached.iter().find(|(f, _)| *f == m) {
                    s.push_str(&format!(
                        "  B{:>3}F: {:>6.1} 分 ({:>4.1} h, {} ticks)\n",
                        m,
                        *t as f64 / 600.0,
                        *t as f64 / 36000.0,
                        t
                    ));
                } else {
                    s.push_str(&format!("  B{:>3}F: 未到達\n", m));
                }
            }
        }

        // 装備購入タイムライン: 全 12 装備の初購入時刻を一覧表示する。
        // 「lane バランス進行」の検証に使う ─ Weapon 列だけが進んで Accessory が
        // 後回しになっていたら policy か balance のバイアスが疑える。
        let any_purchased = self
            .equipment_purchase_ticks
            .iter()
            .any(|t| t.is_some());
        if any_purchased {
            s.push_str("装備購入時刻 (lane × tier × 分):\n");
            for &id in EquipmentId::all() {
                let label = format!(
                    "{} {} (tier {})",
                    lane_short(id.lane()),
                    equipment_name_short(id),
                    id.lane_index()
                );
                match self.equipment_purchase_ticks[id.index()] {
                    Some(t) => s.push_str(&format!(
                        "  {:<24}: {:>7.1} 分 ({:>4.1} h)\n",
                        label,
                        t as f64 / 600.0,
                        t as f64 / 36000.0
                    )),
                    None => s.push_str(&format!("  {:<24}: 未購入\n", label)),
                }
            }

            // 購入間隔の統計 — どこかに大きな gap があれば「退屈な空白」のサイン。
            let purchased_ticks: Vec<u64> = self
                .equipment_purchase_ticks
                .iter()
                .filter_map(|t| *t)
                .collect();
            if purchased_ticks.len() >= 2 {
                let mut sorted = purchased_ticks.clone();
                sorted.sort();
                let gaps: Vec<u64> = sorted.windows(2).map(|w| w[1] - w[0]).collect();
                let max_gap = *gaps.iter().max().unwrap_or(&0);
                let mean_gap = gaps.iter().sum::<u64>() as f64 / gaps.len() as f64;
                let span = sorted.last().unwrap() - sorted.first().unwrap();
                s.push_str(&format!(
                    "購入間隔: 最大 {:.1} 分, 平均 {:.1} 分, 全期間 {:.1} 分 ({:.1} h)\n",
                    max_gap as f64 / 600.0,
                    mean_gap / 600.0,
                    span as f64 / 600.0,
                    span as f64 / 36000.0
                ));
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

/// レポート整形用の lane 短縮ラベル。
fn lane_short(lane: EquipmentLane) -> &'static str {
    match lane {
        EquipmentLane::Weapon => "[武]",
        EquipmentLane::Armor => "[防]",
        EquipmentLane::Accessory => "[飾]",
    }
}

/// レポート整形用の装備名 (config に依らず enum 直接マッピング)。
/// `BalanceConfig::default()` の `equipment[i].name` と一致するよう手書きする
/// (sim report が config のクローンを抱える必要をなくすため)。
fn equipment_name_short(id: EquipmentId) -> &'static str {
    match id {
        EquipmentId::BronzeSword => "銅の剣",
        EquipmentId::SteelSword => "鋼鉄の剣",
        EquipmentId::MithrilSword => "ミスリルの剣",
        EquipmentId::GodSword => "神剣",
        EquipmentId::LeatherArmor => "革鎧",
        EquipmentId::SteelArmor => "鋼鉄の鎧",
        EquipmentId::MithrilArmor => "ミスリルの鎧",
        EquipmentId::GodArmor => "神鎧",
        EquipmentId::SwiftBoots => "速攻のブーツ",
        EquipmentId::TwinWolfRing => "双狼の指輪",
        EquipmentId::SageRobe => "賢者のローブ",
        EquipmentId::EndingCrown => "終焉の冠",
    }
}

pub struct Simulator {
    pub state: AbyssState,
    policy: Box<dyn Policy>,
    metrics: SimMetrics,
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

        let floor_before_tick = self.state.floor;
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
            self.metrics.death_floors.push(floor_before_tick);
            self.last_seen_deaths = self.state.deaths;
        }

        if self.sample_every > 0 && cur_tick.is_multiple_of(self.sample_every) {
            self.metrics.floor_samples.push((cur_tick, self.state.floor));
            self.metrics.gold_samples.push((cur_tick, self.state.gold));
            let max = self.state.hero_max_hp().max(1) as f64;
            self.metrics
                .hp_samples
                .push((cur_tick, self.state.hero_hp as f64 / max));
        }

        // 装備購入の検知 — 既に owned で metrics 側未記録なら今 tick が初購入。
        // 装備は再購入不可なので、一度立ち上がったフラグはずっと true で、
        // metrics 側の `Option` の None→Some 遷移は最大 1 回のみ。
        for &id in EquipmentId::all() {
            let i = id.index();
            if self.state.owned_equipment[i]
                && self.metrics.equipment_purchase_ticks[i].is_none()
            {
                self.metrics.equipment_purchase_ticks[i] = Some(cur_tick);
            }
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

    /// `PlayerAction::ScrollUp/ScrollDown` は UI only。simulator policy が
    /// 絶対に生成しないことを代表的な policy で確認する。
    #[test]
    fn simulator_policies_never_emit_scroll() {
        let state = AbyssState::new();
        let policies: Vec<Box<dyn Policy>> = vec![
            Box::new(NoActionPolicy),
            Box::new(GreedyEnhancePolicy::default()),
            Box::new(LaneWeightedPolicy::balanced()),
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
            for action in policy.on_start(&state) {
                assert!(
                    !matches!(action, PlayerAction::ScrollUp | PlayerAction::ScrollDown),
                );
            }
        }
    }

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

    /// `next_buy_equipment` が **lane_index 主キーで浅いものから** 選ぶこと。
    /// Codex review #87 P2 回帰防止: 旧実装は `EquipmentId::all()` の宣言順
    /// (武器全段階 → 防具全段階 → 装飾全段階) でループしていたため、銅剣所持後に
    /// `LeatherArmor` (lane_index 0) より `SteelSword` (lane_index 1) を先に
    /// 選んでしまい、policy が「lane バランス進行」を表現できていなかった。
    #[test]
    fn next_buy_prefers_lower_lane_index_across_lanes() {
        let mut s = AbyssState::new();
        // 銅剣所持済み + 大量の gold あり。
        // 候補: SteelSword (lane_idx 1, 5000g) / LeatherArmor (lane_idx 0, 150g)
        //       / SwiftBoots (lane_idx 0, 200g)。lane_idx 0 を優先すべき。
        s.owned_equipment[EquipmentId::BronzeSword.index()] = true;
        s.gold = 1_000_000;

        let action = next_buy_equipment(&s).expect("買える装備があるはず");
        match action {
            PlayerAction::BuyEquipment(id) => {
                assert_eq!(
                    id.lane_index(),
                    0,
                    "lane_index 0 (LeatherArmor または SwiftBoots) を選ぶべきだが {:?} (lane_index {}) が返った",
                    id,
                    id.lane_index()
                );
            }
            other => panic!("BuyEquipment 以外が返ってきた: {:?}", other),
        }
    }

    #[test]
    fn greedy_enhance_player_progresses() {
        let mut sim = Simulator::new(
            BalanceConfig::default(),
            Box::new(GreedyEnhancePolicy::default()),
        );
        sim.run(6_000);
        assert!(
            sim.metrics().deepest_floor >= 2,
            "greedy enhance should reach at least B2F, got B{}",
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
            Box::new(LaneWeightedPolicy::balanced()),
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
            Box::new(GreedyEnhancePolicy::default()),
            0xBEEF,
        );
        let mut sim_hard = Simulator::with_seed(
            BalanceConfig::hard(),
            Box::new(GreedyEnhancePolicy::default()),
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
                Box::new(GreedyEnhancePolicy::default()),
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
    fn death_floor_records_actual_not_stale_max() {
        let mut sim = Simulator::with_seed(
            BalanceConfig::default(),
            Box::new(NoActionPolicy),
            0xFEED,
        );
        sim.run(20);

        sim.state.floor = 1;
        sim.last_seen_floor = 5;
        sim.state.hero_hp = 1;
        sim.state.current_enemy.atk_cooldown = 1;

        sim.run(50);

        assert!(!sim.metrics().death_floors.is_empty());
        assert_eq!(sim.metrics().death_floors[0], 1);
    }

    #[test]
    fn metrics_records_first_reached() {
        let mut sim = Simulator::with_seed(
            BalanceConfig::easy(),
            Box::new(LaneWeightedPolicy::offense()),
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
            Box::new(GreedyEnhancePolicy::default()),
        );
        sim.run(1_200);
        let report = sim.report();
        assert!(report.contains("最深フロア"));
    }
}

// ───────────────────────────────────────────────────────────────
// Tuning runners (cargo test simulate_abyss_* -- --nocapture)
// ───────────────────────────────────────────────────────────────

mod runners {
    use super::*;

    fn run_and_print(label: &str, config: BalanceConfig, mut policy: Box<dyn Policy>, ticks: u64) {
        let _ = &mut policy;
        let mut sim = Simulator::new(config, policy);
        sim.run(ticks);
        eprintln!("\n══ {} ══", label);
        eprint!("{}", sim.report());
    }

    /// 既定バランスで Greedy / Balanced / Offense / Defense を比較。
    #[test]
    fn simulate_abyss_default() {
        let ticks = 36_000; // 60 分
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Abyss Idle Balance Sim — preset: default, ticks: {}  ┃", ticks);
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        run_and_print(
            "default + GreedyEnhance",
            BalanceConfig::default(),
            Box::new(GreedyEnhancePolicy::default()),
            ticks,
        );
        run_and_print(
            "default + Balanced",
            BalanceConfig::default(),
            Box::new(LaneWeightedPolicy::balanced()),
            ticks,
        );
        run_and_print(
            "default + Offense",
            BalanceConfig::default(),
            Box::new(LaneWeightedPolicy::offense()),
            ticks,
        );
        run_and_print(
            "default + Defense",
            BalanceConfig::default(),
            Box::new(LaneWeightedPolicy::defense()),
            ticks,
        );
    }

    /// 装備中心の進行で 40h ロングランの最深と全装備到達を測る。
    /// 期待値: 40h で 12 個全装備購入、最深 B100 付近に到達。
    /// `cargo test simulate_abyss_long_run -- --nocapture`
    #[test]
    fn simulate_abyss_long_run() {
        let ticks = 1_440_000; // 40h
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Abyss Idle Long Run — 40h, GreedyEnhance              ┃");
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        let mut sim = Simulator::with_seed(
            BalanceConfig::default(),
            Box::new(GreedyEnhancePolicy::default()),
            0xC0FFEE,
        );
        sim.run(ticks);
        eprintln!("{}", sim.report());
        let owned = sim.state.owned_equipment.iter().filter(|b| **b).count();
        let total_enh: u32 = sim.state.equipment_levels.iter().sum();
        eprintln!(
            "解放装備: {}/{} | 強化総計: +{} | 最深: B{}F | 最終: B{}F",
            owned,
            sim.state.owned_equipment.len(),
            total_enh,
            sim.state.deepest_floor_ever,
            sim.state.floor
        );
        assert_eq!(
            owned,
            sim.state.owned_equipment.len(),
            "40h で全 12 装備解放できないとバランス崩壊"
        );
        assert!(
            sim.state.deepest_floor_ever >= 100,
            "40h で B100 到達できないと『装備中心進行で達成可能』設計が成立しない (got B{})",
            sim.state.deepest_floor_ever
        );
    }

    /// 難易度プリセットを横断: easy / default / hard を Balanced Policy で比較。
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
            run_and_print(label, cfg, Box::new(LaneWeightedPolicy::balanced()), ticks);
        }
    }
}
