//! Dungeon Dive — シミュレーションランナー (難易度調整用)。
//!
//! 本体ゲームと完全に同じ `commands::apply_action` を駆動する。違いは
//! `Policy` が UI 入力ではなく自動行動を返すことだけ。難易度調整は
//! `BalanceConfig` を差し替えるだけで済む — 効果は構造的に同等。
//!
//! 実行例:
//! ```bash
//! cargo test simulate_dungeon_default -- --nocapture
//! cargo test simulate_dungeon_balance_sweep -- --nocapture
//! ```
//!
//! 構造:
//! - `Policy` trait: 各 step で `PlayerAction` を返す主体。
//! - `Simulator`:    state + policy + metrics をまとめて run() する駆動部。
//! - `SimMetrics`:   試行ごとの最深到達階・死亡履歴・ターン数を記録する観測部。

#![cfg(test)]

use super::balance::BalanceConfig;
use super::commands::{apply_action, PlayerAction};
use super::logic;
use super::state::{
    item_info, BattlePhase, EventAction, Facing, ItemCategory, ItemKind, RpgState, Scene,
    SkillKind,
};

// ───────────────────────────────────────────────────────────────
// Policy: 自動プレイヤーの抽象。
// ───────────────────────────────────────────────────────────────

/// 行動を選び続ける主体。本体ゲームは入力ハンドラ、シミュレータは Policy 実装。
pub trait Policy {
    /// 現在の `state` を見て、この瞬間に取りたい 1 アクションを返す。
    /// `None` を返すと「もう何もしない」= 試行終了。
    fn next_action(&mut self, state: &RpgState) -> Option<PlayerAction>;
}

/// 何もしない Policy。「町から出ないとどう詰むか?」を測るときに。
pub struct NoActionPolicy;

impl Policy for NoActionPolicy {
    fn next_action(&mut self, _state: &RpgState) -> Option<PlayerAction> {
        None
    }
}

/// 標準的な「適度に慎重」プレイヤー。HP が減れば撤退・回復し、敵の弱点
/// にスキルを当て、宝箱は開ける。難易度調整のベースライン Policy。
pub struct BalancedPolicy {
    /// 探索中に HP がこれ未満になったら町へ撤退する。
    pub retreat_hp_frac: f32,
    /// 戦闘中に HP がこれ未満になったら逃走を試みる (非ボス時のみ)。
    pub flee_hp_frac: f32,
    /// 戦闘中に HP がこれ未満で薬草持ちなら飲む。
    pub heal_item_hp_frac: f32,
    /// 戦闘中に HP がこれ未満で MP があれば Heal を唱える。
    pub heal_skill_hp_frac: f32,
    /// 弱点が一致するスキルが撃てれば優先する。
    pub use_skills: bool,
    /// 階段に着いたら積極的に降りる。
    pub aggressive_descend: bool,
    /// 宝箱は罠率に関係なく開ける。
    pub greedy_treasure: bool,
    /// 敵を見つけたら奇襲ではなくこっそり通る。
    pub prefer_sneak: bool,
}

impl Default for BalancedPolicy {
    fn default() -> Self {
        Self {
            retreat_hp_frac: 0.25,
            flee_hp_frac: 0.15,
            heal_item_hp_frac: 0.4,
            heal_skill_hp_frac: 0.55,
            use_skills: true,
            aggressive_descend: true,
            greedy_treasure: true,
            prefer_sneak: false,
        }
    }
}

impl BalancedPolicy {
    /// 慎重派 — 早めに撤退、薬草を抱え込む、奇襲より忍び。
    pub fn cautious() -> Self {
        Self {
            retreat_hp_frac: 0.45,
            flee_hp_frac: 0.30,
            heal_item_hp_frac: 0.55,
            heal_skill_hp_frac: 0.70,
            greedy_treasure: false,
            prefer_sneak: true,
            ..Self::default()
        }
    }

    /// 突撃派 — ほぼ撤退しない、宝箱は全開け、忍ばずに正面戦闘。
    pub fn reckless() -> Self {
        Self {
            retreat_hp_frac: 0.0,
            flee_hp_frac: 0.05,
            heal_item_hp_frac: 0.25,
            heal_skill_hp_frac: 0.35,
            greedy_treasure: true,
            prefer_sneak: false,
            ..Self::default()
        }
    }

    fn hp_frac(state: &RpgState) -> f32 {
        if state.max_hp == 0 {
            0.0
        } else {
            state.hp as f32 / state.max_hp as f32
        }
    }

    fn decide_town(&self, state: &RpgState) -> PlayerAction {
        // 0 = ダンジョン、1 = ショップ、2 = 休息 (HP/MP が満タンでないとき)
        if state.hp < state.max_hp || state.mp < state.max_mp {
            // herb 在庫が乏しいなら買う、それ以外は休息して出発。薬草0G コスト分岐は
            // 単純化のため省略 (現実装の town_choices では Rest が優先)。
            if needs_more_herbs(state)
                && state.gold >= item_info(ItemKind::Herb).buy_price
            {
                return PlayerAction::OpenShop;
            }
            return PlayerAction::TownChoice(2);
        }
        PlayerAction::TownChoice(0)
    }

    fn decide_shop(&self, state: &RpgState) -> PlayerAction {
        // 薬草が 3 個未満なら追加で買う。それ以外は overlay を閉じる。
        if needs_more_herbs(state)
            && state.gold >= item_info(ItemKind::Herb).buy_price
        {
            return PlayerAction::BuyShopItem(0);
        }
        PlayerAction::CloseOverlay
    }

    fn decide_explore(&self, state: &RpgState) -> PlayerAction {
        if Self::hp_frac(state) < self.retreat_hp_frac {
            return PlayerAction::RetreatToTown;
        }
        match pick_move_direction(state) {
            Some(d) => PlayerAction::Move(d),
            None => PlayerAction::RetreatToTown,
        }
    }

    fn decide_event(&self, state: &RpgState) -> PlayerAction {
        let event = match &state.active_event {
            Some(e) => e,
            None => return PlayerAction::RetreatToTown,
        };
        let idx = pick_event_choice(&event.choices, self);
        PlayerAction::PickEventChoice(idx)
    }

    fn decide_battle(&self, state: &RpgState) -> PlayerAction {
        let phase = state.battle.as_ref().map(|b| b.phase);
        let phase = match phase {
            Some(p) => p,
            None => return PlayerAction::BattleAttack,
        };
        match phase {
            BattlePhase::SelectAction => self.decide_battle_action(state),
            BattlePhase::SelectSkill | BattlePhase::SelectItem => {
                // サブメニューに入ったまま判断するロジックを持たないので、
                // 即座に SelectAction に戻す。
                PlayerAction::BattleBackToActions
            }
            BattlePhase::Victory | BattlePhase::Defeat | BattlePhase::Fled => {
                PlayerAction::BattleAcknowledgeOutcome
            }
        }
    }

    fn decide_battle_action(&self, state: &RpgState) -> PlayerAction {
        let frac = Self::hp_frac(state);
        let is_boss = state.battle.as_ref().map(|b| b.is_boss).unwrap_or(false);

        // 1. 致命的: 逃走 (非ボス)。
        if !is_boss && frac < self.flee_hp_frac {
            return PlayerAction::BattleFlee;
        }
        // 2. HP 低 + 薬草あり: 薬草。
        if frac < self.heal_item_hp_frac {
            if let Some(idx) = find_consumable_index(state, ItemKind::Herb) {
                return PlayerAction::BattleUseItem(idx);
            }
        }
        // 3. HP 低 + MP あり: Heal スキル。
        if self.use_skills && frac < self.heal_skill_hp_frac {
            if let Some(idx) = skill_index(state, SkillKind::Heal) {
                let cost = super::state::skill_info(SkillKind::Heal).mp_cost;
                if state.mp >= cost {
                    return PlayerAction::BattleUseSkill(idx);
                }
            }
        }
        // 4. 弱点を突けるなら攻撃スキル。
        if self.use_skills {
            if let Some((skill_idx, _)) = pick_offensive_skill(state) {
                return PlayerAction::BattleUseSkill(skill_idx);
            }
        }
        // 5. 通常攻撃。
        PlayerAction::BattleAttack
    }
}

impl Policy for BalancedPolicy {
    fn next_action(&mut self, state: &RpgState) -> Option<PlayerAction> {
        // overlay (Shop / Inventory / Status) が開いているときは閉じるか操作する。
        if let Some(overlay) = state.overlay {
            return Some(match overlay {
                super::state::Overlay::Shop => self.decide_shop(state),
                super::state::Overlay::Inventory | super::state::Overlay::Status => {
                    PlayerAction::CloseOverlay
                }
            });
        }
        match state.scene {
            Scene::Intro(_) => Some(PlayerAction::AdvanceIntro),
            Scene::Town => Some(self.decide_town(state)),
            Scene::DungeonExplore => Some(self.decide_explore(state)),
            Scene::DungeonEvent => Some(self.decide_event(state)),
            Scene::DungeonResult => Some(PlayerAction::ContinueExploration),
            Scene::Battle => Some(self.decide_battle(state)),
            Scene::GameClear => None,
        }
    }
}

// ───────────────────────────────────────────────────────────────
// 内部ヘルパー (Policy が使う読み取り専用クエリ)。
// ───────────────────────────────────────────────────────────────

fn pick_move_direction(state: &RpgState) -> Option<Facing> {
    let map = state.dungeon.as_ref()?;
    let order = [Facing::North, Facing::East, Facing::South, Facing::West];
    let last_back = map.last_dir.reverse();

    let mut unvisited: Option<Facing> = None;
    let mut visited_forward: Option<Facing> = None;
    let mut visited_back: Option<Facing> = None;

    for &dir in &order {
        let nx = map.player_x as i32 + dir.dx();
        let ny = map.player_y as i32 + dir.dy();
        if !map.in_bounds(nx, ny) {
            continue;
        }
        let cell = &map.grid[ny as usize][nx as usize];
        if !cell.is_walkable() {
            continue;
        }
        if !cell.visited {
            if dir == last_back && unvisited.is_some() {
                continue;
            }
            unvisited = Some(dir);
            if dir != last_back {
                break;
            }
        } else if dir != last_back {
            if visited_forward.is_none() {
                visited_forward = Some(dir);
            }
        } else if visited_back.is_none() {
            visited_back = Some(dir);
        }
    }
    unvisited.or(visited_forward).or(visited_back)
}

fn pick_event_choice(
    choices: &[super::state::EventChoice],
    policy: &BalancedPolicy,
) -> usize {
    let mut best: Option<(i32, usize)> = None;
    for (i, c) in choices.iter().enumerate() {
        let score = match c.action {
            EventAction::DescendStairs => {
                if policy.aggressive_descend { 100 } else { 30 }
            }
            EventAction::ReturnToTown => -50,
            EventAction::OpenTreasure => {
                if policy.greedy_treasure { 80 } else { 30 }
            }
            EventAction::SearchTreasure => {
                if policy.greedy_treasure { 60 } else { 90 }
            }
            EventAction::Ignore => 0,
            EventAction::Ambush => 70,
            EventAction::SneakPast => {
                if policy.prefer_sneak { 90 } else { 40 }
            }
            EventAction::FightNormally => 50,
            EventAction::DrinkSpring => 75,
            EventAction::FillBottle => 35,
            EventAction::ReadLore => 20,
            EventAction::TalkNpc => 25,
            EventAction::TradeNpc => 40,
            EventAction::Continue => 10,
        };
        if best.map(|(s, _)| score > s).unwrap_or(true) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i).unwrap_or(0)
}

fn pick_offensive_skill(state: &RpgState) -> Option<(usize, SkillKind)> {
    use super::state::skill_element;
    let enemy_kind = state.battle.as_ref().map(|b| b.enemy.kind)?;
    let einfo = super::state::enemy_info(enemy_kind);
    let weakness = einfo.weakness;
    let available = logic::available_skills(state.level);

    if let Some(elem) = weakness {
        for (i, &s) in available.iter().enumerate() {
            if skill_element(s) == Some(elem) {
                let cost = super::state::skill_info(s).mp_cost;
                if state.mp >= cost {
                    return Some((i, s));
                }
            }
        }
    }
    let offensive = [
        SkillKind::Thunder,
        SkillKind::IceBlade,
        SkillKind::Fire,
        SkillKind::Drain,
    ];
    for kind in offensive {
        if let Some(i) = available.iter().position(|&s| s == kind) {
            let cost = super::state::skill_info(kind).mp_cost;
            if state.mp >= cost && state.mp > state.max_mp / 3 {
                return Some((i, kind));
            }
        }
    }
    None
}

fn find_consumable_index(state: &RpgState, kind: ItemKind) -> Option<usize> {
    let consumables: Vec<usize> = state
        .inventory
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            item_info(it.kind).category == ItemCategory::Consumable && it.count > 0
        })
        .map(|(idx, _)| idx)
        .collect();
    let raw = state
        .inventory
        .iter()
        .position(|it| it.kind == kind && it.count > 0)?;
    consumables.iter().position(|&c| c == raw)
}

fn skill_index(state: &RpgState, kind: SkillKind) -> Option<usize> {
    logic::available_skills(state.level)
        .iter()
        .position(|&s| s == kind)
}

fn needs_more_herbs(state: &RpgState) -> bool {
    let count = state
        .inventory
        .iter()
        .find(|it| it.kind == ItemKind::Herb)
        .map(|it| it.count)
        .unwrap_or(0);
    count < 3
}

// ───────────────────────────────────────────────────────────────
// SimMetrics: 観測部。
// ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct SimMetrics {
    pub total_actions: u64,
    pub total_kills: u64,
    pub deaths: u64,
    pub deepest_floor: u32,
    pub final_floor: u32,
    pub final_level: u32,
    pub final_gold: u32,
    pub final_hp_frac: f32,
    pub cleared: bool,
    /// 死亡したフロア (試行を跨いで蓄積)。
    pub death_floors: Vec<u32>,
    /// 各フロア初到達時の累積 action 数。
    pub first_reached: Vec<(u32, u64)>,
    /// (action 数, deepest_floor) のサンプル — 進捗カーブを描く用。
    pub floor_samples: Vec<(u64, u32)>,
}

impl SimMetrics {
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("── Dungeon Dive Sim Report ────────────────\n");
        s.push_str(&format!("総 action 数: {}\n", self.total_actions));
        s.push_str(&format!(
            "最深: B{}F  最終: B{}F  level: {}\n",
            self.deepest_floor, self.final_floor, self.final_level
        ));
        s.push_str(&format!(
            "最終 gold: {}  HP残: {:.0}%  cleared: {}\n",
            self.final_gold,
            self.final_hp_frac * 100.0,
            self.cleared
        ));
        s.push_str(&format!(
            "総撃破: {}, 死亡: {}\n",
            self.total_kills, self.deaths
        ));
        if !self.first_reached.is_empty() {
            s.push_str("到達 (フロア → action):\n");
            let milestones = [2u32, 3, 5, 7, 10];
            for &m in &milestones {
                if let Some((_, t)) = self.first_reached.iter().find(|(f, _)| *f == m) {
                    s.push_str(&format!("  B{:>2}F: {} actions\n", m, t));
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

// ───────────────────────────────────────────────────────────────
// Simulator: 駆動部。
// ───────────────────────────────────────────────────────────────

pub struct Simulator {
    pub state: RpgState,
    policy: Box<dyn Policy>,
    metrics: SimMetrics,
}

impl Simulator {
    pub fn new(config: BalanceConfig, policy: Box<dyn Policy>) -> Self {
        Self::with_seed(config, policy, 0xC0FF_EEBA_BE00)
    }

    pub fn with_seed(
        config: BalanceConfig,
        policy: Box<dyn Policy>,
        seed: u64,
    ) -> Self {
        let mut state = RpgState::new();
        state.rng_seed = seed;
        state.difficulty = config;
        Self {
            state,
            policy,
            metrics: SimMetrics::default(),
        }
    }

    pub fn metrics(&self) -> &SimMetrics {
        &self.metrics
    }

    pub fn report(&self) -> String {
        self.metrics.report()
    }

    /// 最大 `max_actions` 回 Policy に行動を聞いて apply_action する。
    /// `Policy` が `None` を返すか、ゲームクリアか、`max_actions` に達したら停止。
    pub fn run(&mut self, max_actions: u32) {
        let mut prev_floor: u32 = 0;
        let mut prev_kills: u32 = 0;
        let mut prev_dead = false;

        for _ in 0..max_actions {
            if self.state.game_cleared {
                self.metrics.cleared = true;
                break;
            }
            let action = match self.policy.next_action(&self.state) {
                Some(a) => a,
                None => break,
            };
            apply_action(&mut self.state, action);
            self.metrics.total_actions += 1;

            // フロア更新の検出。
            if let Some(d) = &self.state.dungeon {
                let f = d.floor_num;
                if f > prev_floor {
                    if !self.metrics.first_reached.iter().any(|(ff, _)| *ff == f) {
                        self.metrics
                            .first_reached
                            .push((f, self.metrics.total_actions));
                    }
                    prev_floor = f;
                }
                if f > self.metrics.deepest_floor {
                    self.metrics.deepest_floor = f;
                }
                self.metrics.final_floor = f;
            }

            // 撃破カウント (run_enemies_killed は撤退でリセットされるので差分で追う)。
            let kills = self.state.run_enemies_killed;
            if kills > prev_kills {
                self.metrics.total_kills += (kills - prev_kills) as u64;
                prev_kills = kills;
            } else if kills < prev_kills {
                // 撤退でリセット — 累計はそのまま、prev だけ更新。
                prev_kills = kills;
            }

            // 死亡検出。process_dungeon_death は scene を Town に戻し HP を半分に
            // するので、ここでは「直前 dungeon にいて HP=0 になった」を監視する。
            let dead_now = matches!(self.state.scene, Scene::Town) && self.state.hp == 0;
            if dead_now && !prev_dead {
                self.metrics.deaths += 1;
                self.metrics
                    .death_floors
                    .push(self.metrics.final_floor.max(1));
            }
            prev_dead = dead_now;

            // 進捗サンプル (50 action ごと)。
            if self.metrics.total_actions.is_multiple_of(50) {
                self.metrics
                    .floor_samples
                    .push((self.metrics.total_actions, self.metrics.deepest_floor));
            }
        }

        self.metrics.final_level = self.state.level;
        self.metrics.final_gold = self.state.gold;
        let mhp = self.state.max_hp.max(1) as f32;
        self.metrics.final_hp_frac = self.state.hp as f32 / mhp;
        if self.state.game_cleared {
            self.metrics.cleared = true;
        }
    }
}

// ───────────────────────────────────────────────────────────────
// Sanity tests
// ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod sanity_tests {
    use super::*;

    #[test]
    fn no_action_policy_stays_in_intro() {
        let mut sim = Simulator::new(BalanceConfig::standard(), Box::new(NoActionPolicy));
        sim.run(100);
        // Intro で行動を返さないので、scene は Intro のまま。
        assert!(matches!(sim.state.scene, Scene::Intro(_)));
        assert_eq!(sim.metrics().total_actions, 0);
    }

    #[test]
    fn balanced_policy_progresses_to_dungeon() {
        let mut sim = Simulator::new(
            BalanceConfig::standard(),
            Box::new(BalancedPolicy::default()),
        );
        sim.run(50);
        // 50 action 後にはイントロを抜けてダンジョンに踏み込んでいるはず。
        assert!(sim.metrics().total_actions > 0);
        assert!(matches!(
            sim.state.scene,
            Scene::Town | Scene::DungeonExplore | Scene::DungeonEvent | Scene::Battle
        ));
    }

    #[test]
    fn brutal_is_harder_than_easy() {
        let mut easy_sim = Simulator::with_seed(
            BalanceConfig::easy(),
            Box::new(BalancedPolicy::default()),
            123,
        );
        easy_sim.run(800);

        let mut brutal_sim = Simulator::with_seed(
            BalanceConfig::brutal(),
            Box::new(BalancedPolicy::default()),
            123,
        );
        brutal_sim.run(800);

        // 同条件 + 同 seed なら brutal の方が死亡数 ≥ easy。
        assert!(
            brutal_sim.metrics().deaths >= easy_sim.metrics().deaths,
            "expected brutal deaths ({}) >= easy ({}). easy report:\n{}\nbrutal report:\n{}",
            brutal_sim.metrics().deaths,
            easy_sim.metrics().deaths,
            easy_sim.report(),
            brutal_sim.report(),
        );
    }

    #[test]
    fn report_renders_basic_fields() {
        let mut sim = Simulator::new(
            BalanceConfig::standard(),
            Box::new(BalancedPolicy::default()),
        );
        sim.run(100);
        let r = sim.report();
        assert!(r.contains("総 action 数"));
        assert!(r.contains("最深"));
    }

    #[test]
    fn descend_stairs_chosen_when_aggressive() {
        // aggressive_descend=true なら DescendStairs が最高スコア。
        let pol = BalancedPolicy::default();
        let choices = vec![
            super::super::state::EventChoice {
                label: "降りる".into(),
                action: EventAction::DescendStairs,
            },
            super::super::state::EventChoice {
                label: "続ける".into(),
                action: EventAction::Continue,
            },
        ];
        assert_eq!(pick_event_choice(&choices, &pol), 0);
    }
}

// ───────────────────────────────────────────────────────────────
// Tuning runners (cargo test simulate_dungeon_* -- --nocapture)
// ───────────────────────────────────────────────────────────────
//
// 印字された結果を見ながら BalanceConfig / BalancedPolicy を弄る。
// abyss / cookie のシミュレータと同じ流儀 (--nocapture でだけ可視化される)。

#[cfg(test)]
mod runners {
    use super::*;

    /// Runner に `with_seed` を使うのは、`dungeon_map::generate_map` の
    /// reach-fixup ループが特定 seed × 高難易度で収束しない既知挙動を
    /// 避けるため。本 PR のスコープ外なので、ここでは検証済み seed を使う。
    const RUNNER_SEED: u64 = 0xDEAD_BEEF_CAFE_BABE;

    fn run_and_print(label: &str, config: BalanceConfig, policy: Box<dyn Policy>, actions: u32) {
        let mut sim = Simulator::with_seed(config, policy, RUNNER_SEED);
        sim.run(actions);
        eprintln!("\n══ {} ══", label);
        eprint!("{}", sim.report());
    }

    /// 既定バランスで 4 種の Policy を比較。
    /// `cargo test simulate_dungeon_default -- --nocapture`
    #[test]
    #[ignore = "tuning runner — execute manually with --ignored"]
    fn simulate_dungeon_default() {
        let actions = 800;
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Dungeon Dive Sim — preset: standard, actions: {}    ┃", actions);
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        run_and_print(
            "standard + Balanced",
            BalanceConfig::standard(),
            Box::new(BalancedPolicy::default()),
            actions,
        );
        run_and_print(
            "standard + Cautious",
            BalanceConfig::standard(),
            Box::new(BalancedPolicy::cautious()),
            actions,
        );
        run_and_print(
            "standard + Reckless",
            BalanceConfig::standard(),
            Box::new(BalancedPolicy::reckless()),
            actions,
        );
        run_and_print(
            "standard + NoAction",
            BalanceConfig::standard(),
            Box::new(NoActionPolicy),
            actions,
        );
    }

    /// 4 つの難易度プリセットを Balanced Policy で横並び比較。
    /// `cargo test simulate_dungeon_balance_sweep -- --nocapture`
    #[test]
    #[ignore = "tuning runner — execute manually with --ignored"]
    fn simulate_dungeon_balance_sweep() {
        let actions = 400;
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Balance Sweep — Balanced policy, actions: {}        ┃", actions);
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        for cfg in [
            BalanceConfig::easy(),
            BalanceConfig::standard(),
            BalanceConfig::hard(),
            BalanceConfig::brutal(),
        ] {
            run_and_print(
                cfg.name,
                cfg.clone(),
                Box::new(BalancedPolicy::default()),
                actions,
            );
        }
    }

    /// 多数 seed での平均を見る。バランス調整時の安定指標。
    /// `cargo test simulate_dungeon_seed_average -- --nocapture`
    #[test]
    #[ignore = "tuning runner — execute manually with --ignored"]
    fn simulate_dungeon_seed_average() {
        let actions = 600;
        let trials = 6u64;

        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Seed Average — Balanced policy, {} trials × {} actions ┃", trials, actions);
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        for cfg in [BalanceConfig::standard(), BalanceConfig::hard()] {
            let mut deepest_sum: u64 = 0;
            let mut deaths_sum: u64 = 0;
            let mut kills_sum: u64 = 0;
            for t in 0..trials {
                let seed = 0x1234_5678u64
                    .wrapping_add(t.wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let mut sim = Simulator::with_seed(
                    cfg.clone(),
                    Box::new(BalancedPolicy::default()),
                    seed,
                );
                sim.run(actions);
                deepest_sum += sim.metrics().deepest_floor as u64;
                deaths_sum += sim.metrics().deaths;
                kills_sum += sim.metrics().total_kills;
            }
            eprintln!(
                "{}: 平均 deepest=B{:.1}F, deaths={:.2}/trial, kills={:.1}/trial",
                cfg.name,
                deepest_sum as f64 / trials as f64,
                deaths_sum as f64 / trials as f64,
                kills_sum as f64 / trials as f64,
            );
        }
    }
}
