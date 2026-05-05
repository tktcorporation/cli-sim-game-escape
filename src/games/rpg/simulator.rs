//! Dungeon Dive — シミュレーションランナー (バランス調整用)。
//!
//! 本体ゲームと同じ `logic::*` 関数を駆動し、`Policy` が UI 入力の代わりに
//! 自動行動を返す。
//!
//! 実行例:
//! ```bash
//! cargo test simulate_rpg_default -- --nocapture
//! cargo test simulate_rpg_runs -- --nocapture
//! cargo test simulate_rpg_policy_comparison -- --nocapture --include-ignored
//! ```
//!
//! ## Issue #92 バランス調整メモ
//!
//! ### 調整前 (#86 マージ直後, 2026-05-04)
//! - 30 run × default policy: クリア 0%, 撤退 ~100%, 最深 B5F〜B8F
//! - 死亡内訳: 飢餓 主要因
//! - affix ドロップ: 中盤で殆ど 0
//!
//! ### 調整後 (issue #92, 2026-05-05)
//! 本コミットで以下を修正:
//! - 満腹度減衰: 1/turn → 1/2 turns (`tick_satiety`)
//! - 序盤敵 (Slime/Rat/Goblin) の atk/hp を僅かに下方修正
//! - mid-tier kill の affix ドロップ率を +10〜25% (Skeleton/Golem/DarkKnight 35%, Demon/Dragon 55%)
//! - `AutoExplorerPolicy` に `careful` / `reckless` バリエーション追加
//!
//! `simulate_rpg_policy_comparison` で 3 policy を比較できる。default policy
//! は撤退バイアスが強く 30% クリア率には届きにくいが、reckless では深層
//! 突破例が増える (= 玩家にとって調整後の難易度カーブはちゃんと "押せる"
//! 状態になっている)。
//!
//! ### simulator_smoke
//! CI で常時実行する短時間ヘルスチェック。3 ラン × 2000 アクションで
//! クラッシュしないことだけを確認する (時間切れ・死亡・撤退いずれも OK)。

#![cfg(test)]

use super::logic;
use super::state::{
    enemy_info, EnemyKind, Facing, ItemCategory, ItemKind, Overlay, RpgState, Scene,
};

// ── Policy ─────────────────────────────────────────────────

/// 1 ターン分の行動。シミュレータは行動を 1 つ実行→ on_player_action までを 1 単位とする。
#[allow(dead_code)] // some variants reserved for future policies
#[derive(Clone, Debug)]
pub enum Action {
    Move(Facing),
    UseItemByKind(ItemKind),
    UseSkill(usize),
    EventChoice(usize),
    OpenInventory,
    OpenSkill,
    OpenShop,
    OpenQuestBoard,
    OpenPrayMenu,
    AcceptQuest(usize),
    Pray,
    EnterDungeon,
    Inn,
    BuyItem(ItemKind),
    Retreat,
    CloseOverlay,
    Noop,
}

pub trait Policy {
    fn choose(&mut self, state: &RpgState) -> Action;
}

/// 「賢めの自動プレイヤー」: 隣接敵は殴る、HPが低ければヒール、空腹なら食う、
/// それ以外は階段を目指して降りていく。死ぬまで試行を繰り返す。
///
/// `retreat_hp_pct` を変えることで careful / reckless のバリエーションが
/// 作れる (issue #92 の "比較" 要件)。
pub struct AutoExplorerPolicy {
    pub last_dir_seed: u64,
    pub last_pos: Option<(usize, usize)>,
    pub stuck_count: u32,
    /// 1ダンジョン訪問あたり最大ターン数 (これを超えたら撤退)。
    pub max_turns_per_visit: u32,
    /// 現在の訪問のターンカウント。
    pub turns_this_visit: u32,
    /// HP がこの % を切ったら撤退するしきい値 (0..=100)。
    pub retreat_hp_pct: u32,
}

impl Default for AutoExplorerPolicy {
    fn default() -> Self {
        Self {
            last_dir_seed: 0,
            last_pos: None,
            stuck_count: 0,
            max_turns_per_visit: 1500,
            turns_this_visit: 0,
            retreat_hp_pct: 30,
        }
    }
}

impl AutoExplorerPolicy {
    /// "careful" — 早めに撤退する慎重派。
    #[allow(dead_code)]
    pub fn careful() -> Self {
        Self {
            retreat_hp_pct: 50,
            ..Self::default()
        }
    }
    /// "reckless" — HP 残り少なくても押し続ける突撃派。
    #[allow(dead_code)]
    pub fn reckless() -> Self {
        Self {
            retreat_hp_pct: 15,
            max_turns_per_visit: 2500,
            ..Self::default()
        }
    }
}

impl Policy for AutoExplorerPolicy {
    fn choose(&mut self, state: &RpgState) -> Action {
        // Overlay優先処理
        if let Some(ov) = state.overlay {
            return self.handle_overlay(state, ov);
        }

        match state.scene {
            Scene::Intro(_) => Action::EventChoice(0),
            Scene::Town => self.choose_in_town(state),
            Scene::DungeonExplore => {
                if state.active_event.is_some() {
                    self.choose_event(state)
                } else {
                    self.choose_in_dungeon(state)
                }
            }
            Scene::GameClear => Action::Noop,
        }
    }
}

impl AutoExplorerPolicy {
    fn handle_overlay(&mut self, state: &RpgState, ov: Overlay) -> Action {
        match ov {
            Overlay::Inventory => {
                // HP/MP/満腹度に応じて使う
                if let Some(act) = self.consume_if_needed(state) {
                    return act;
                }
                Action::CloseOverlay
            }
            Overlay::Status => Action::CloseOverlay,
            Overlay::Shop => {
                // 必要なものを順番に買う
                let bread = state.inventory.iter().filter(|i| i.kind == ItemKind::Bread).map(|i| i.count).sum::<u32>();
                let herb = state.inventory.iter().filter(|i| i.kind == ItemKind::Herb).map(|i| i.count).sum::<u32>();
                if bread < 5 && state.gold >= 15 {
                    return Action::BuyItem(ItemKind::Bread);
                }
                if herb < 5 && state.gold >= 20 {
                    return Action::BuyItem(ItemKind::Herb);
                }
                if state.gold >= 50 && state.inventory.iter().filter(|i| i.kind == ItemKind::MagicWater).map(|i| i.count).sum::<u32>() < 3 {
                    return Action::BuyItem(ItemKind::MagicWater);
                }
                Action::CloseOverlay
            }
            Overlay::SkillMenu => {
                if state.hp * 100 / state.effective_max_hp() < 35
                    && state.mp >= 6
                    && state.level >= 2
                {
                    return Action::UseSkill(skill_index(state, |i| i.name.contains("ヒール")));
                }
                if adjacent_enemy_idx(state).is_some() {
                    if state.mp >= 14 && state.level >= 5 {
                        return Action::UseSkill(skill_index(state, |i| i.name.contains("サンダー")));
                    }
                    if state.mp >= 8 {
                        return Action::UseSkill(skill_index(state, |i| i.name.contains("ファイア")));
                    }
                }
                Action::CloseOverlay
            }
            Overlay::QuestBoard => {
                if state.active_quest.is_none() {
                    Action::AcceptQuest(0)
                } else {
                    Action::CloseOverlay
                }
            }
            Overlay::PrayMenu => {
                if !state.prayed_this_run {
                    Action::Pray
                } else {
                    Action::CloseOverlay
                }
            }
        }
    }

    fn choose_in_town(&mut self, state: &RpgState) -> Action {
        // 新しい訪問のためにリセット
        self.turns_this_visit = 0;
        let max_hp = state.effective_max_hp();
        // 怪我していて、宿代が払えるなら宿
        if (state.hp * 4 < max_hp * 3 || state.satiety * 2 < state.satiety_max) && state.gold >= 10 {
            return Action::Inn;
        }
        // 食料/薬草が少なければショップ
        let bread = state.inventory.iter().filter(|i| i.kind == ItemKind::Bread).map(|i| i.count).sum::<u32>();
        let herb = state.inventory.iter().filter(|i| i.kind == ItemKind::Herb).map(|i| i.count).sum::<u32>();
        if (bread < 3 || herb < 3) && state.gold >= 20 {
            return Action::OpenShop;
        }
        // クエスト未受託なら受託
        if state.active_quest.is_none() {
            return Action::OpenQuestBoard;
        }
        // 祈ってから入る
        if !state.prayed_this_run {
            return Action::OpenPrayMenu;
        }
        Action::EnterDungeon
    }

    fn choose_in_dungeon(&mut self, state: &RpgState) -> Action {
        let max_hp = state.effective_max_hp();
        let map = match &state.dungeon {
            Some(m) => m,
            None => return Action::Noop,
        };

        // 訪問が長すぎる場合は撤退
        self.turns_this_visit += 1;
        if self.turns_this_visit >= self.max_turns_per_visit {
            self.turns_this_visit = 0;
            return Action::Retreat;
        }

        // 同じ位置に2回連続なら強制的にランダム方向 (スタック検知)
        let cur = (map.player_x, map.player_y);
        if self.last_pos == Some(cur) {
            self.stuck_count += 1;
        } else {
            self.stuck_count = 0;
            self.last_pos = Some(cur);
        }
        if self.stuck_count >= 2 {
            self.stuck_count = 0;
            self.last_dir_seed = self.last_dir_seed.wrapping_mul(2654435769).wrapping_add(1);
            if let Some(d) = pick_walkable(map, self.last_dir_seed) {
                return Action::Move(d);
            }
        }

        // HP critical: ヒール or 薬草 or 撤退
        if state.hp * 100 / max_hp < self.retreat_hp_pct {
            // 薬草
            if state.inventory.iter().any(|i| i.kind == ItemKind::Herb) {
                return Action::UseItemByKind(ItemKind::Herb);
            }
            // ヒール
            if state.mp >= 6 {
                return Action::OpenSkill;
            }
            // 撤退 (入口に向かって移動するシンプル化版: 退却ロジック)
            return Action::Retreat;
        }

        // 飢餓: 食料を食う
        if state.satiety < 200 {
            for kind in [ItemKind::CookedMeal, ItemKind::Jerky, ItemKind::Bread, ItemKind::Apple] {
                if state.inventory.iter().any(|i| i.kind == kind) {
                    return Action::UseItemByKind(kind);
                }
            }
        }

        // 隣接敵がいたら殴る
        if let Some(idx) = adjacent_enemy_idx(state) {
            let m = &map.monsters[idx];
            let dx = m.x as i32 - map.player_x as i32;
            let dy = m.y as i32 - map.player_y as i32;
            let dir = if dx > 0 { Facing::East }
                else if dx < 0 { Facing::West }
                else if dy > 0 { Facing::South }
                else { Facing::North };
            return Action::Move(dir);
        }

        // 階段が見えてれば階段方向、なければ未踏方向に進む
        if let Some(dir) = direction_toward_stairs(state) {
            return Action::Move(dir);
        }
        if let Some(dir) = direction_toward_unvisited(state) {
            return Action::Move(dir);
        }
        // どこにも行けない → ランダム移動
        self.last_dir_seed = self.last_dir_seed.wrapping_mul(2654435769).wrapping_add(1);
        let pseudo = (self.last_dir_seed >> 32) as u32;
        let dir = match pseudo % 4 {
            0 => Facing::North, 1 => Facing::East, 2 => Facing::South, _ => Facing::West,
        };
        Action::Move(dir)
    }

    fn choose_event(&mut self, state: &RpgState) -> Action {
        let event = match &state.active_event {
            Some(e) => e,
            None => return Action::Noop,
        };
        // 階段なら降りる、入口なら帰還しない (探索続行)、宝は慎重に開ける
        for (i, c) in event.choices.iter().enumerate() {
            use super::state::EventAction as EA;
            match c.action {
                EA::DescendStairs => return Action::EventChoice(i),
                EA::SearchTreasure => return Action::EventChoice(i),
                EA::DrinkSpring => {
                    if state.hp * 2 < state.effective_max_hp() || state.mp * 2 < state.max_mp {
                        return Action::EventChoice(i);
                    }
                }
                EA::FillBottle => {
                    if state.hp * 4 >= state.effective_max_hp() * 3 {
                        return Action::EventChoice(i);
                    }
                }
                EA::ReadLore => return Action::EventChoice(i),
                EA::TalkNpc => return Action::EventChoice(i),
                EA::Continue => return Action::EventChoice(i),
                EA::ReturnToTown => {
                    // HPが本当にやばい時だけ帰る
                    if state.hp * 4 < state.effective_max_hp() {
                        return Action::EventChoice(i);
                    }
                }
                // Issue #90 events: pick safe / beneficial choices.
                EA::ReviveAdventurer => return Action::EventChoice(i),
                EA::PickFruit => {
                    if state.satiety * 2 < state.satiety_max {
                        return Action::EventChoice(i);
                    }
                }
                EA::DrinkWell => {
                    if state.hp * 2 < state.effective_max_hp() {
                        return Action::EventChoice(i);
                    }
                }
                EA::BottleWell => {
                    if state.hp * 4 >= state.effective_max_hp() * 3 {
                        return Action::EventChoice(i);
                    }
                }
                EA::PrayIdol => return Action::EventChoice(i),
                EA::PeddlerBuyHerb => {
                    if state.gold >= 15 && state.inventory.iter().filter(|x| x.kind == ItemKind::Herb).map(|x| x.count).sum::<u32>() < 3 {
                        return Action::EventChoice(i);
                    }
                }
                EA::PeddlerBuyBread => {
                    if state.gold >= 12 && state.inventory.iter().filter(|x| x.kind == ItemKind::Bread).map(|x| x.count).sum::<u32>() < 3 {
                        return Action::EventChoice(i);
                    }
                }
                EA::TakeEgg => {
                    if state.pet.is_none() {
                        return Action::EventChoice(i);
                    }
                }
                EA::BreakEgg => {
                    if state.satiety * 2 < state.satiety_max {
                        return Action::EventChoice(i);
                    }
                }
                _ => {}
            }
        }
        Action::EventChoice(0)
    }

    fn consume_if_needed(&self, state: &RpgState) -> Option<Action> {
        let max_hp = state.effective_max_hp();
        if state.hp * 100 / max_hp < 60 && state.inventory.iter().any(|i| i.kind == ItemKind::Herb) {
            return Some(Action::UseItemByKind(ItemKind::Herb));
        }
        if state.mp * 2 < state.max_mp && state.inventory.iter().any(|i| i.kind == ItemKind::MagicWater) {
            return Some(Action::UseItemByKind(ItemKind::MagicWater));
        }
        None
    }
}

// ── Helpers ────────────────────────────────────────────────

fn adjacent_enemy_idx(state: &RpgState) -> Option<usize> {
    let map = state.dungeon.as_ref()?;
    let px = map.player_x as i32;
    let py = map.player_y as i32;
    map.monsters
        .iter()
        .position(|m| m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1)
}

fn skill_index<F: Fn(&super::state::SkillInfo) -> bool>(state: &RpgState, pred: F) -> usize {
    let skills = logic::available_skills(state.level);
    skills
        .iter()
        .position(|&s| pred(&super::state::skill_info(s)))
        .unwrap_or(0)
}

/// 階段が現在の部屋内にある or 直線上にあるなら、その方向の最初の一歩を返す。
fn direction_toward_stairs(state: &RpgState) -> Option<Facing> {
    let map = state.dungeon.as_ref()?;
    use super::state::CellType;
    // 全マップから階段位置を検索 (revealed のもの)
    for y in 0..map.height {
        for x in 0..map.width {
            let cell = &map.grid[y][x];
            if cell.cell_type == CellType::Stairs && cell.revealed {
                return greedy_step(map, x, y);
            }
        }
    }
    None
}

fn direction_toward_unvisited(state: &RpgState) -> Option<Facing> {
    let map = state.dungeon.as_ref()?;
    // 隣接マスで未訪問+歩行可なら優先
    for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
        let nx = map.player_x as i32 + dir.dx();
        let ny = map.player_y as i32 + dir.dy();
        if !map.in_bounds(nx, ny) { continue; }
        let c = map.cell(nx as usize, ny as usize);
        if c.is_walkable() && !c.visited { return Some(dir); }
    }
    // それもなければ revealed unvisited の方向に貪欲にステップ
    for y in 0..map.height {
        for x in 0..map.width {
            let c = &map.grid[y][x];
            if c.is_walkable() && c.revealed && !c.visited {
                return greedy_step(map, x, y);
            }
        }
    }
    None
}

/// 任意の歩行可能方向を seed-based に選ぶ。
fn pick_walkable(map: &super::state::DungeonMap, seed: u64) -> Option<Facing> {
    let dirs = [Facing::North, Facing::East, Facing::South, Facing::West];
    let start = (seed % 4) as usize;
    for off in 0..4 {
        let d = dirs[(start + off) % 4];
        let nx = map.player_x as i32 + d.dx();
        let ny = map.player_y as i32 + d.dy();
        if !map.in_bounds(nx, ny) { continue; }
        if map.cell(nx as usize, ny as usize).is_walkable() { return Some(d); }
    }
    None
}

fn greedy_step(map: &super::state::DungeonMap, tx: usize, ty: usize) -> Option<Facing> {
    let dx = tx as i32 - map.player_x as i32;
    let dy = ty as i32 - map.player_y as i32;
    let mut tries: Vec<Facing> = Vec::new();
    if dx.abs() > dy.abs() {
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
    } else {
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
    }
    for &dir in &tries {
        let nx = map.player_x as i32 + dir.dx();
        let ny = map.player_y as i32 + dir.dy();
        if !map.in_bounds(nx, ny) { continue; }
        if map.cell(nx as usize, ny as usize).is_walkable() {
            return Some(dir);
        }
    }
    None
}

// ── Simulator ───────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct SimMetrics {
    pub runs: u32,
    pub clears: u32,
    pub deaths_in_dungeon: u32,
    pub retreats: u32,

    pub timeouts: u32,
    pub total_actions: u64,
    pub max_floor_reached: u32,
    pub final_level: u32,
    pub final_gold: u32,

    pub deaths_by_starvation: u32,
    pub total_kills: u64,
    pub total_quests_completed: u32,

    /// (run_idx, max_floor_reached, end_reason)
    pub run_outcomes: Vec<(u32, u32, &'static str)>,
}

impl SimMetrics {
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("── Dungeon Dive Sim Report ───────────────────\n");
        s.push_str(&format!(
            "総走行: {} / クリア: {} / 死亡: {} / 撤退: {} / 時間切れ: {}\n",
            self.runs, self.clears, self.deaths_in_dungeon, self.retreats, self.timeouts
        ));
        s.push_str(&format!(
            "クリア率: {:.1}%\n",
            self.clears as f64 / self.runs.max(1) as f64 * 100.0
        ));
        s.push_str(&format!(
            "死亡内訳: 飢餓 {} / 戦闘 {}\n",
            self.deaths_by_starvation,
            self.deaths_in_dungeon - self.deaths_by_starvation
        ));
        s.push_str(&format!("総アクション: {}\n", self.total_actions));
        s.push_str(&format!("最深到達: B{}F\n", self.max_floor_reached));
        s.push_str(&format!("最終 Lv: {}\n", self.final_level));
        s.push_str(&format!("最終 Gold: {}G\n", self.final_gold));
        s.push_str(&format!("総撃破: {}\n", self.total_kills));
        s.push_str(&format!("完了依頼: {}\n", self.total_quests_completed));
        s.push_str("\n各ラン結果:\n");
        for (i, floor, reason) in &self.run_outcomes {
            s.push_str(&format!("  Run {}: B{}F → {}\n", i + 1, floor, reason));
        }
        s
    }
}

pub struct Simulator {
    pub state: RpgState,
    policy: Box<dyn Policy>,
    metrics: SimMetrics,
}

impl Simulator {
    pub fn new(seed: u64, policy: Box<dyn Policy>) -> Self {
        let mut state = RpgState::new();
        state.rng_seed = seed;
        // Skip intro
        logic::advance_intro(&mut state);
        logic::advance_intro(&mut state);
        Self { state, policy, metrics: SimMetrics::default() }
    }

    pub fn metrics(&self) -> &SimMetrics { &self.metrics }

    /// 1ラン = 町→ダンジョン→帰還 or 死亡 まで
    pub fn run_single(&mut self, max_actions: u32) -> &'static str {
        let mut deaths_before = self.metrics.deaths_in_dungeon;
        let mut retreats_before = self.metrics.retreats;
        let mut clears_before = self.metrics.clears;
        let _ = (&mut deaths_before, &mut retreats_before, &mut clears_before);

        let was_starving = self.state.satiety == 0;
        let _ = was_starving;
        let starting_floor = 0u32;
        let _ = starting_floor;

        let mut run_actions = 0;
        let mut hp_check_starvation_warned = false;

        // While in town, drive policy until enter dungeon
        // While in dungeon, drive policy until back in town or game cleared
        let entry_scene = self.state.scene;
        let _ = entry_scene;

        loop {
            if run_actions >= max_actions {
                self.metrics.timeouts += 1;
                self.metrics.run_outcomes.push((
                    self.metrics.runs,
                    self.state.max_floor_reached,
                    "timeout",
                ));
                return "timeout";
            }

            // Detect death/retreat/clear
            if self.state.scene == Scene::GameClear {
                self.metrics.clears += 1;
                self.metrics.run_outcomes.push((
                    self.metrics.runs,
                    self.state.max_floor_reached,
                    "clear",
                ));
                return "clear";
            }

            // Track starvation
            if self.state.satiety == 0 && self.state.dungeon.is_some() && !hp_check_starvation_warned {
                hp_check_starvation_warned = true;
            }

            let action = self.policy.choose(&self.state);
            let prev_dungeon = self.state.dungeon.is_some();
            self.apply_action(action);
            run_actions += 1;
            self.metrics.total_actions += 1;

            // Detect transition: was in dungeon, now in town
            if prev_dungeon && self.state.dungeon.is_none() && self.state.scene == Scene::Town {
                let last_log = self.state.log.last().cloned().unwrap_or_default();
                if last_log.starts_with("力尽きた") {
                    self.metrics.deaths_in_dungeon += 1;
                    if hp_check_starvation_warned {
                        self.metrics.deaths_by_starvation += 1;
                    }
                    self.metrics.run_outcomes.push((
                        self.metrics.runs,
                        self.state.max_floor_reached,
                        "died",
                    ));
                    return "died";
                }
                self.metrics.retreats += 1;
                self.metrics.run_outcomes.push((
                    self.metrics.runs,
                    self.state.max_floor_reached,
                    "retreat",
                ));
                return "retreat";
            }
        }
    }

    pub fn run_many(&mut self, num_runs: u32, max_actions_per_run: u32) {
        for i in 0..num_runs {
            self.metrics.runs = i;
            let _ = self.run_single(max_actions_per_run);
            self.metrics.runs = i + 1;
            // Reset prayed_in_session-like state (next run can pray again)
            // Note: state.prayed_this_run is reset by enter_dungeon(floor=1) automatically
            // Update aggregate
            self.metrics.max_floor_reached =
                self.metrics.max_floor_reached.max(self.state.max_floor_reached);
            self.metrics.final_level = self.state.level;
            self.metrics.final_gold = self.state.gold;
            self.metrics.total_kills += self.state.run_enemies_killed as u64;
            self.metrics.total_quests_completed = self.state.completed_quests;
            // If game cleared, stop running
            if self.state.game_cleared {
                break;
            }
        }
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Move(d) => { logic::try_move(&mut self.state, d); }
            Action::UseItemByKind(kind) => {
                if let Some(idx) = self.state.inventory.iter().position(|i| i.kind == kind) {
                    logic::use_item(&mut self.state, idx);
                }
            }
            Action::UseSkill(idx) => { logic::use_skill(&mut self.state, idx); }
            Action::EventChoice(i) => {
                if self.state.scene == Scene::Intro(0) || self.state.scene == Scene::Intro(1) {
                    logic::advance_intro(&mut self.state);
                } else {
                    logic::resolve_event_choice(&mut self.state, i);
                }
            }
            Action::OpenInventory => self.state.overlay = Some(Overlay::Inventory),
            Action::OpenSkill => self.state.overlay = Some(Overlay::SkillMenu),
            Action::OpenShop => self.state.overlay = Some(Overlay::Shop),
            Action::OpenQuestBoard => self.state.overlay = Some(Overlay::QuestBoard),
            Action::OpenPrayMenu => self.state.overlay = Some(Overlay::PrayMenu),
            Action::AcceptQuest(i) => { logic::accept_quest(&mut self.state, i); }
            Action::Pray => { logic::pray(&mut self.state); }
            Action::EnterDungeon => { logic::enter_dungeon(&mut self.state, 1); }
            Action::Inn => { logic::execute_town_choice(&mut self.state, 4); }
            Action::BuyItem(kind) => {
                let shop = super::state::shop_items(self.state.max_floor_reached);
                if let Some(idx) = shop.iter().position(|(k, _)| *k == kind) {
                    logic::buy_item(&mut self.state, idx);
                }
            }
            Action::Retreat => { logic::retreat_to_town(&mut self.state); }
            Action::CloseOverlay => { self.state.overlay = None; }
            Action::Noop => {}
        }
    }
}

// ── Tests / Sim Runs ───────────────────────────────────────

#[cfg(test)]
mod runs {
    use super::*;

    fn run_seed(seed: u64) -> SimMetrics {
        let policy = Box::new(AutoExplorerPolicy::default());
        let mut sim = Simulator::new(seed, policy);
        sim.run_many(20, 5000);
        sim.metrics().clone()
    }

    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_default() {
        let m = run_seed(0xC0FFEE);
        println!("{}", m.report());
        // Sanity: at least made some progress
        assert!(m.runs > 0);
    }

    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_runs_seeds() {
        let seeds = [42u64, 1234, 9999, 0xC0FFEE, 0xDEADBEEF];
        for s in seeds {
            let m = run_seed(s);
            println!("=== seed {} ===", s);
            println!("{}", m.report());
        }
    }

    /// 1 ラン詳細: 開始時の状態 → 各 floor 突入時のリソース
    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_first_run_trace() {
        let policy = Box::new(AutoExplorerPolicy::default());
        let mut sim = Simulator::new(42, policy);
        // 最初の3ラン
        for i in 0..3 {
            let result = sim.run_single(5000);
            println!(
                "Run {}: result={}, lvl={}, gold={}, max_floor={}, kills={}, quests={}",
                i + 1,
                result,
                sim.state.level,
                sim.state.gold,
                sim.state.max_floor_reached,
                sim.state.run_enemies_killed,
                sim.state.completed_quests,
            );
        }
    }

    /// A/B 比較用: 満腹度減衰を変えた時の挙動を見る
    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_satiety_sensitivity() {
        // current implementation has satiety drain rate fixed at 1/turn
        // This test just shows what the baseline looks like
        let m = run_seed(7);
        println!("{}", m.report());
    }

    /// 30ラン回して死亡率/クリア率を見る
    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_long_session() {
        let policy = Box::new(AutoExplorerPolicy::default());
        let mut sim = Simulator::new(0xC0FFEE, policy);
        sim.run_many(30, 8000);
        println!("{}", sim.metrics().report());
    }

    /// Issue #92: 3 policy 比較 (careful / default / reckless)。
    /// 同じシードで挙動を見比べ、バランスがどの戦略向けか確認する。
    /// 手動実行用 — `--include-ignored --nocapture` を付けて回す。
    #[test]
    #[ignore = "manual run for balance tuning"]
    fn simulate_rpg_policy_comparison() {
        // 15 ラン × 4000 アクションに留めるとローカルでは数秒で終わる。
        // 統計的な有意性より「3 policy の差を一目で見る」のが目的。
        let runs = 15;
        let max = 4000;
        let seed = 0xC0FFEE;

        let mut careful = Simulator::new(seed, Box::new(AutoExplorerPolicy::careful()));
        careful.run_many(runs, max);
        println!("=== careful ===\n{}", careful.metrics().report());

        let mut default = Simulator::new(seed, Box::new(AutoExplorerPolicy::default()));
        default.run_many(runs, max);
        println!("=== default ===\n{}", default.metrics().report());

        let mut reckless = Simulator::new(seed, Box::new(AutoExplorerPolicy::reckless()));
        reckless.run_many(runs, max);
        println!("=== reckless ===\n{}", reckless.metrics().report());
    }

    /// 短時間ヘルスチェック (--no-ignore で常時実行可)
    #[test]
    fn simulator_smoke() {
        let policy = Box::new(AutoExplorerPolicy::default());
        let mut sim = Simulator::new(42, policy);
        // 3 ラン回るだけ確認
        sim.run_many(3, 2000);
        let m = sim.metrics();
        assert!(m.runs > 0);
        assert!(m.total_actions > 0);
        let _ = enemy_info(EnemyKind::Slime);
        // Suppress dead_code on lookup helpers
        let _ = ItemCategory::Consumable;
    }
}
