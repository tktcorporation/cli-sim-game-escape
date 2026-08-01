//! Dungeon Dive — grid-based dungeon crawler with inline combat.
//!
//! Roguelike gameplay: monsters live on the same grid as the player.
//! Movement against a monster tile = attack. Each player action triggers
//! a monster turn (chase + attack). No separate battle screen.

pub mod actions;
pub mod dungeon_map;
pub mod dungeon_view;
pub mod effects;
pub mod events;
pub mod logic;
pub mod lore;
pub mod overworld_map;
pub mod render;
pub mod state;
#[cfg(test)]
pub mod simulator;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::effects::FrameClock;
use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};
use crate::sound;
use crate::time;

use actions::*;
use effects::RpgEffects;
use state::{Overlay, RpgState, Scene};

pub struct RpgGame {
    state: RpgState,
    effects: RefCell<RpgEffects>,
    prev: Cell<PrevSnapshot>,
    frame_clock: FrameClock,
}

/// 前フレームの state スナップショット。render 毎に差分を見て演出をトリガする
/// (`detect_transitions`)。
///
/// 被弾/クリティカル検知は `hero_hit_count`/`enemy_hit_count`/`crit_count` の
/// 差分で行い、`FlashTimer::is_active()` のエッジは使わない。Swift affix の
/// 同ターン2回行動や複数体の同時攻撃で1ターン内に複数回ヒットが起きうるため、
/// エッジ検知だと2発目以降を見落とす。
#[derive(Clone, Copy)]
struct PrevSnapshot {
    scene: Scene,
    /// 村マップは 0、ダンジョンは 1 以上。フロア到達・帰還の両方をこの1値の
    /// 増減だけで判定できる (overworld_map.rs が floor_num=0 を保証する)。
    floor_num: u32,
    hero_hit_count: u32,
    enemy_hit_count: u32,
    crit_count: u32,
    charge_count: u32,
    level: u32,
    max_floor_reached: u32,
    known_weaknesses_count: usize,
}

/// 致命打を受けた瞬間 (同じフレーム内で `hero_hit_count` の増加と死亡による
/// 村への帰還が両方進むケース) かどうか。true の間は被弾演出/音を抑制し、
/// 死亡演出/音 (DEFEAT) だけを単独で伝える — 両方鳴らすと Web Audio の
/// 再生と Vibration API の呼び出しが競合し、被弾側の振動が死亡側に
/// 上書きされてしまうため。
fn is_death_exit_this_frame(floor_num: u32, prev_floor_num: u32, last_dungeon_exit_was_death: bool) -> bool {
    floor_num == 0 && prev_floor_num > 0 && last_dungeon_exit_was_death
}

impl RpgGame {
    pub fn new() -> Self {
        let state = RpgState::new();
        let prev = Self::snapshot(&state);
        Self {
            state,
            effects: RefCell::new(RpgEffects::new()),
            prev: Cell::new(prev),
            frame_clock: FrameClock::new(),
        }
    }

    fn snapshot(s: &RpgState) -> PrevSnapshot {
        PrevSnapshot {
            scene: s.scene,
            floor_num: s.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(0),
            hero_hit_count: s.hero_hit_count,
            enemy_hit_count: s.enemy_hit_count,
            crit_count: s.crit_count,
            charge_count: s.charge_count,
            level: s.level,
            max_floor_reached: s.max_floor_reached,
            known_weaknesses_count: s.known_weaknesses.len(),
        }
    }

    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let s = &self.state;
        let in_dungeon = s.dungeon.is_some();
        let layout = render::compute_layout(area, in_dungeon, s.scene == Scene::DungeonExplore);
        let floor_num = s.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(0);
        let died_this_frame = is_death_exit_this_frame(floor_num, prev.floor_num, s.last_dungeon_exit_was_death);

        if s.hero_hit_count != prev.hero_hit_count && !died_this_frame {
            effects.push_hero_hit(layout.status_bar);
            sound::play(sound::HIT_HERO);
        }

        // 敵ヒットは音を付けない (abyss と同じ理由: 1戦闘で何度も鳴って耳障り)。
        if s.enemy_hit_count != prev.enemy_hit_count {
            effects.push_enemy_hit(layout.scene_content);
        }

        if s.crit_count != prev.crit_count {
            effects.push_critical(layout.scene_content);
            sound::play(sound::CRITICAL);
        }

        if s.charge_count != prev.charge_count {
            effects.push_charge_warning(layout.scene_content);
            sound::play(sound::ERROR);
        }

        if s.known_weaknesses.len() > prev.known_weaknesses_count {
            effects.push_weakness_discovered(layout.scene_content);
            sound::play(sound::SELECT);
        }

        // 新たな最深到達 (自己ベスト更新) と、既に踏破済みの階への降下は
        // 同じ「floor_numが増える」イベントだが体験としては別物なので、
        // 両方の演出を毎回重ねがけせず新記録の方を優先する。
        if floor_num > prev.floor_num {
            if s.max_floor_reached > prev.max_floor_reached {
                effects.push_new_record(area);
                sound::play(sound::FLOOR_CLEAR);
            } else {
                effects.push_descend(area);
                sound::play(sound::FLOOR_CLEAR);
            }
        } else if floor_num > 0 && floor_num < prev.floor_num {
            effects.push_ascend(area);
        } else if floor_num == 0 && prev.floor_num > 0 {
            if s.last_dungeon_exit_was_death {
                effects.push_death(area);
                sound::play(sound::DEFEAT);
            } else {
                effects.push_return_to_town(area);
                sound::play(sound::VICTORY);
            }
        }

        if prev.scene != Scene::GameClear && s.scene == Scene::GameClear {
            effects.push_boss_defeated(layout.scene_content);
            sound::play(sound::VICTORY);
        }

        if s.level > prev.level {
            effects.push_level_up(layout.status_bar);
            sound::play(sound::LEVEL_UP);
        }

        self.prev.set(Self::snapshot(s));
    }
}

impl Game for RpgGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Rpg
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(ch) => handle_key(&mut self.state, *ch),
            InputEvent::Click(_, id) => {
                let consumed = handle_click(&mut self.state, *id);
                // マップタップ/D-padは移動と同時に攻撃を伴うことがあり、
                // 攻撃時は detect_transitions が専用音 (HIT_HERO/CRITICAL等) を
                // 鳴らすため、ここで汎用クリック音を重ねると二重に鳴ってしまう。
                // 純粋な移動タップはキー入力と同様に無音のままにし、それ以外の
                // 明示的な操作 (ボタン・メニュー) だけに成功/失敗の音を付ける。
                let is_movement_tap = (MAP_TAP_BASE..DPAD_BASE + 9).contains(id);
                if !is_movement_tap {
                    sound::play(if consumed { sound::CLICK } else { sound::ERROR });
                }
                consumed
            }
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        self.detect_transitions(area);
        render::render(&self.state, f, area, click_state);
        let elapsed = self.frame_clock.elapsed(time::now_ms().unwrap_or(0.0));
        self.effects.borrow_mut().process(elapsed, f.buffer_mut(), area);
    }
}

// ── Input Handling ──────────────────────────────────────────

/// Whether the current scene/overlay treats the arrow keys as cursor
/// navigation. In dungeon explore / overworld (with no popup or overlay),
/// arrows are player movement instead.
fn arrows_navigate_cursor(state: &RpgState) -> bool {
    if state.overlay.is_some() {
        return true;
    }
    matches!(state.scene, Scene::GameClear)
        || (matches!(state.scene, Scene::DungeonExplore | Scene::Overworld)
            && state.active_event.is_some())
}

fn handle_key(state: &mut RpgState, ch: char) -> bool {
    // Keep the cursor inside the current menu's bounds before any handler
    // reads it (menus may have shrunk since the last input — e.g. consumed
    // an inventory item).
    logic::cursor_clamp(state);

    // Arrow-key cursor navigation, applied uniformly across scenes/overlays
    // that have a selectable list. In dungeon explore (no popup) the same
    // keys fall through to player movement.
    if arrows_navigate_cursor(state) {
        match ch {
            'j' => {
                logic::cursor_move(state, 1);
                return true;
            }
            'k' => {
                logic::cursor_move(state, -1);
                return true;
            }
            _ => {}
        }
    }

    if state.overlay.is_some() {
        return handle_overlay_key(state, ch);
    }

    match state.scene {
        Scene::Overworld | Scene::DungeonExplore => {
            // When an event popup is active, route input there first.
            if state.active_event.is_some() {
                handle_dungeon_event_key(state, ch)
            } else {
                handle_dungeon_explore_key(state, ch)
            }
        }
        Scene::GameClear => handle_game_clear_key(state, ch),
    }
}

fn handle_click(state: &mut RpgState, id: u16) -> bool {
    if state.overlay.is_some() {
        return handle_overlay_click(state, id);
    }

    match state.scene {
        Scene::Overworld | Scene::DungeonExplore => {
            if state.active_event.is_some() {
                handle_dungeon_event_click(state, id)
            } else {
                handle_dungeon_explore_click(state, id)
            }
        }
        Scene::GameClear => handle_game_clear_click(state, id),
    }
}

// ── Dungeon Explore / Overworld ────────────────────────────

fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        // Movement: arrow keys (h/j/k/l after KeyCode mapping) and WASD.
        // 'a' is reserved for the A button; use 'h' or arrow-left for west.
        'W' | 'w' | 'k' => logic::try_move(state, state::Facing::North),
        'h' => logic::try_move(state, state::Facing::West),
        'S' | 's' | 'j' => logic::try_move(state, state::Facing::South),
        'D' | 'd' | 'l' => logic::try_move(state, state::Facing::East),
        // A button — context-sensitive primary action.
        ' ' | 'A' | 'a' => trigger_a_button(state),
        // B button — unified menu (持ち物 / スキル / ステータス).
        'b' | 'B' | 'I' | 'i' => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        // Skill / Status shortcuts kept for keyboard users.
        'Z' | 'z' => {
            state.open_overlay(Overlay::SkillMenu);
            true
        }
        'X' | 'x' => {
            state.open_overlay(Overlay::Status);
            true
        }
        _ => false,
    }
}

/// Implements the A button:
/// foot event → confirm cursor's choice (was hardcoded to choice 0 before
///   the cursor unification — now respects whichever option the player
///   has highlighted with arrow keys),
/// adjacent enemy → open skill menu,
/// otherwise → wait one turn (or no-op in overworld).
fn trigger_a_button(state: &mut RpgState) -> bool {
    if state.active_event.is_some() {
        return logic::resolve_event_choice(state, state.cursor);
    }
    if let Some(map) = &state.dungeon {
        // Overworld: no monsters, no waiting — A on an empty tile does nothing.
        if map.is_overworld {
            return false;
        }
        let px = map.player_x as i32;
        let py = map.player_y as i32;
        if map
            .monsters
            .iter()
            .any(|m| m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1)
        {
            state.open_overlay(Overlay::SkillMenu);
            return true;
        }
    }
    logic::wait_in_place(state)
}

fn handle_dungeon_explore_click(state: &mut RpgState, id: u16) -> bool {
    if id == AB_A_BUTTON {
        return trigger_a_button(state);
    }
    if id == AB_B_BUTTON {
        state.open_overlay(Overlay::Inventory);
        return true;
    }
    handle_dpad_tap(state, id)
        || handle_map_tap(state, id)
        || handle_overlay_open_click(state, id)
}

fn handle_dpad_tap(state: &mut RpgState, id: u16) -> bool {
    use crate::widgets::ClickableGrid;
    let Some((col, row)) = ClickableGrid::decode(DPAD_BASE, 3, id) else {
        return false;
    };
    let dir = match (col, row) {
        (1, 0) => Some(state::Facing::North),
        (0, 1) => Some(state::Facing::West),
        (2, 1) => Some(state::Facing::East),
        (1, 2) => Some(state::Facing::South),
        _ => None,
    };
    match dir {
        Some(d) => logic::try_move(state, d),
        None => false,
    }
}

fn handle_map_tap(state: &mut RpgState, id: u16) -> bool {
    use crate::widgets::ClickableGrid;
    let Some((col, row)) = ClickableGrid::decode(MAP_TAP_BASE, 3, id) else {
        return false;
    };
    let screen_dir = match (col, row) {
        (_, 0) => Some(state::Facing::North),
        (0, 1) => Some(state::Facing::West),
        (2, 1) => Some(state::Facing::East),
        (_, 2) => Some(state::Facing::South),
        _ => None,
    };
    match screen_dir {
        Some(dir) => logic::move_direction(state, dir),
        None => false,
    }
}

// ── Dungeon Event ─────────────────────────────────────────

fn handle_dungeon_event_key(state: &mut RpgState, ch: char) -> bool {
    let choice_index = match ch {
        '1' => Some(0),
        '2' => Some(1),
        '3' => Some(2),
        '4' => Some(3),
        '5' => Some(4),
        _ => None,
    };
    if let Some(idx) = choice_index {
        return logic::resolve_event_choice(state, idx);
    }

    match ch {
        // A button — confirm cursor's choice in the popup.
        ' ' | 'A' | 'a' => logic::resolve_event_choice(state, state.cursor),
        // B button — skip / "explore on" (last choice, conventionally Ignore).
        'b' | 'B' => {
            let last = state
                .active_event
                .as_ref()
                .map(|e| e.choices.len().saturating_sub(1))
                .unwrap_or(0);
            logic::resolve_event_choice(state, last)
        }
        'I' | 'i' => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        _ => false,
    }
}

fn handle_dungeon_event_click(state: &mut RpgState, id: u16) -> bool {
    if id == AB_A_BUTTON {
        return logic::resolve_event_choice(state, state.cursor);
    }
    if id == AB_B_BUTTON {
        let last = state
            .active_event
            .as_ref()
            .map(|e| e.choices.len().saturating_sub(1))
            .unwrap_or(0);
        return logic::resolve_event_choice(state, last);
    }
    if (EVENT_CHOICE_BASE..EVENT_CHOICE_BASE + 10).contains(&id) {
        let index = (id - EVENT_CHOICE_BASE) as usize;
        return logic::resolve_event_choice(state, index);
    }
    handle_overlay_open_click(state, id)
}

// ── Overlay open (shared) ─────────────────────────────────

fn handle_overlay_open_click(state: &mut RpgState, id: u16) -> bool {
    match id {
        OPEN_INVENTORY => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        OPEN_STATUS => {
            state.open_overlay(Overlay::Status);
            true
        }
        OPEN_SKILL_MENU => {
            state.open_overlay(Overlay::SkillMenu);
            true
        }
        _ => false,
    }
}

// ── Overlays ───────────────────────────────────────────────

fn handle_overlay_key(state: &mut RpgState, ch: char) -> bool {
    // SkillChoice is a forced pick: cancel/close shortcuts must not
    // dismiss it, otherwise the level-up gate would be bypassed and the
    // run could continue with `pending_skill_choice` lingering.
    if state.overlay == Some(Overlay::SkillChoice) {
        return match ch {
            ' ' | 'A' | 'a' => logic::confirm_skill_choice(state, state.cursor),
            '1' => logic::confirm_skill_choice(state, 0),
            '2' => logic::confirm_skill_choice(state, 1),
            _ => false,
        };
    }

    // B button / common close shortcuts work for every overlay.
    if matches!(ch, 'b' | 'B' | '0' | '-') {
        state.close_overlay();
        return true;
    }

    // Tab cycle (h/l) when on a menu tab.
    if state.overlay.map(|o| o.is_menu_tab()).unwrap_or(false) {
        match ch {
            'l' => {
                let next = match state.overlay.unwrap() {
                    Overlay::Inventory => Overlay::SkillMenu,
                    Overlay::SkillMenu => Overlay::Status,
                    _ => Overlay::Inventory,
                };
                state.open_overlay(next);
                return true;
            }
            'h' => {
                let next = match state.overlay.unwrap() {
                    Overlay::Status => Overlay::SkillMenu,
                    Overlay::SkillMenu => Overlay::Inventory,
                    _ => Overlay::Status,
                };
                state.open_overlay(next);
                return true;
            }
            _ => {}
        }
    }
    match state.overlay {
        Some(Overlay::Inventory) => match ch {
            // A button — use the highlighted item.
            ' ' | 'A' | 'a' => logic::use_item(state, state.cursor),
            // Number-key shortcut still works for direct access.
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Shop) => match ch {
            ' ' | 'A' | 'a' => logic::buy_item(state, state.cursor),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::buy_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Status) => false, // status has no clickable items
        Some(Overlay::SkillMenu) => match ch {
            ' ' | 'A' | 'a' => logic::use_skill(state, state.cursor),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_skill(state, idx)
            }
            _ => false,
        },
        Some(Overlay::QuestBoard) => match ch {
            ' ' | 'A' | 'a' => {
                if state.active_quest.is_some() {
                    logic::abandon_quest(state)
                } else {
                    logic::accept_quest(state, state.cursor)
                }
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                if state.active_quest.is_some() {
                    logic::abandon_quest(state)
                } else {
                    logic::accept_quest(state, idx)
                }
            }
            _ => false,
        },
        Some(Overlay::PrayMenu) => match ch {
            ' ' | '1' | 'A' | 'a' => logic::pray(state),
            _ => false,
        },
        Some(Overlay::SkillChoice) => match ch {
            ' ' | 'A' | 'a' => logic::confirm_skill_choice(state, state.cursor),
            '1' => logic::confirm_skill_choice(state, 0),
            '2' => logic::confirm_skill_choice(state, 1),
            _ => false,
        },
        None => false,
    }
}

fn handle_overlay_click(state: &mut RpgState, id: u16) -> bool {
    // SkillChoice: forced pick — only accept the two skill-choice buttons,
    // never the close-overlay click. Mirrors the key-input guard above.
    if state.overlay == Some(Overlay::SkillChoice) {
        return match id {
            SKILL_CHOICE_LEFT => logic::confirm_skill_choice(state, 0),
            SKILL_CHOICE_RIGHT => logic::confirm_skill_choice(state, 1),
            _ => false,
        };
    }

    if id == CLOSE_OVERLAY {
        state.close_overlay();
        return true;
    }

    // Tab switch within the unified menu (Inventory / SkillMenu / Status).
    if state.overlay.map(|o| o.is_menu_tab()).unwrap_or(false) {
        match id {
            MENU_TAB_INVENTORY => {
                state.open_overlay(Overlay::Inventory);
                return true;
            }
            MENU_TAB_SKILL => {
                state.open_overlay(Overlay::SkillMenu);
                return true;
            }
            MENU_TAB_STATUS => {
                state.open_overlay(Overlay::Status);
                return true;
            }
            _ => {}
        }
    }

    match state.overlay {
        Some(Overlay::Inventory) => {
            if (INV_USE_BASE..INV_USE_BASE + 20).contains(&id) {
                return logic::use_item(state, (id - INV_USE_BASE) as usize);
            }
            false
        }
        Some(Overlay::Shop) => {
            if (SHOP_BUY_BASE..SHOP_BUY_BASE + 20).contains(&id) {
                return logic::buy_item(state, (id - SHOP_BUY_BASE) as usize);
            }
            false
        }
        Some(Overlay::SkillMenu) => {
            if (SKILL_BASE..SKILL_BASE + 10).contains(&id) {
                return logic::use_skill(state, (id - SKILL_BASE) as usize);
            }
            false
        }
        Some(Overlay::QuestBoard) => {
            if (QUEST_ACCEPT_BASE..QUEST_ACCEPT_BASE + 5).contains(&id) {
                return logic::accept_quest(state, (id - QUEST_ACCEPT_BASE) as usize);
            }
            if id == QUEST_ABANDON {
                return logic::abandon_quest(state);
            }
            false
        }
        Some(Overlay::PrayMenu) => {
            if id == PRAY_CONFIRM {
                return logic::pray(state);
            }
            false
        }
        Some(Overlay::SkillChoice) => {
            if id == SKILL_CHOICE_LEFT {
                return logic::confirm_skill_choice(state, 0);
            }
            if id == SKILL_CHOICE_RIGHT {
                return logic::confirm_skill_choice(state, 1);
            }
            false
        }
        _ => false,
    }
}

// ── Game Clear ──────────────────────────────────────────────

fn handle_game_clear_key(state: &mut RpgState, ch: char) -> bool {
    let _ = state;
    ch == '1' || ch == ' '
}

fn handle_game_clear_click(_state: &mut RpgState, id: u16) -> bool {
    id == CHOICE_BASE
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn make_game() -> RpgGame {
        RpgGame::new()
    }

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Rpg), id)
    }

    /// Helper: skip the village walk, jump straight into B1F.
    fn into_dungeon(g: &mut RpgGame) {
        logic::enter_dungeon(&mut g.state, 1);
    }

    /// `EffectHost::is_running()` は `process()` で経過時間を消化するまで
    /// true のまま残る。「baselineをprevに記録するための detect_transitions
    /// 呼び出し」自体が (init_dungeon 等の) 本物の遷移を検知して演出を積んで
    /// しまうケースでは、その残留エフェクトが後続の assert!(is_running()) を
    /// 汚染してしまう。この関数で確実に使い切ってから本題の検証に入る。
    fn drain_effects(g: &RpgGame, area: Rect) {
        let mut buf = ratzilla::ratatui::buffer::Buffer::empty(area);
        g.effects.borrow_mut().process(tachyonfx::Duration::from_millis(5000), &mut buf, area);
        assert!(!g.effects.borrow().is_running(), "drain_effects後は演出が残っていないはず");
    }

    #[test]
    fn starts_in_overworld_with_village_loaded() {
        let g = make_game();
        assert_eq!(g.state.scene, Scene::Overworld);
        let map = g.state.dungeon.as_ref().unwrap();
        assert!(map.is_overworld);
        assert_eq!(map.floor_num, 0);
        assert!(g.state.weapon().is_none(), "no starter weapon until NPC");
    }

    #[test]
    fn enter_dungeon_jumps_to_b1f() {
        let mut g = make_game();
        into_dungeon(&mut g);
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        let map = g.state.dungeon.as_ref().unwrap();
        assert!(!map.is_overworld);
        assert_eq!(map.floor_num, 1);
    }

    #[test]
    fn dungeon_wasd_movement() {
        let mut g = make_game();
        into_dungeon(&mut g);
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        g.handle_input(&InputEvent::Key('W'));
        g.handle_input(&InputEvent::Key('D'));
        g.handle_input(&InputEvent::Key('A'));
        g.handle_input(&InputEvent::Key('S'));
    }

    #[test]
    fn retreat_returns_to_overworld() {
        let mut g = make_game();
        into_dungeon(&mut g);
        logic::retreat_to_town(&mut g.state);
        assert_eq!(g.state.scene, Scene::Overworld);
        let map = g.state.dungeon.as_ref().unwrap();
        assert!(map.is_overworld);
    }

    fn area() -> Rect {
        Rect::new(0, 0, 80, 30)
    }

    #[test]
    fn detect_transitions_pushes_hero_hit_on_count_change() {
        let mut g = make_game();
        // 構築直後 (村, floor_numも変化なし) なので演出は何も積まれていない。
        assert!(!g.effects.borrow().is_running());
        g.state.hero_hurt_flash.trigger(3);
        g.state.hero_hit_count = g.state.hero_hit_count.wrapping_add(1);
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    /// 回帰テスト: Swift affix の同ターン2回行動などで hero_hit_count が
    /// render を挟まず連続して増える場合でも、差分検知なので毎回演出が
    /// 積まれるはず (`FlashTimer::is_active()` のエッジ検知だと2発目以降を
    /// 見落とすため、カウンタ差分方式を採用している)。
    #[test]
    fn detect_transitions_keeps_pushing_on_consecutive_hits_within_one_turn() {
        let mut g = make_game();
        assert!(!g.effects.borrow().is_running());
        g.state.hero_hurt_flash.trigger(3);
        g.state.hero_hit_count = g.state.hero_hit_count.wrapping_add(2); // 1ターン内に2回ヒット
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running(), "2回ヒット分もまとめて検知され演出が積まれるはず");
    }

    #[test]
    fn detect_transitions_pushes_critical_on_crit_count_change() {
        let mut g = make_game();
        assert!(!g.effects.borrow().is_running());
        g.state.crit_count = g.state.crit_count.wrapping_add(1);
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    #[test]
    fn detect_transitions_pushes_effect_on_first_floor_entry() {
        let mut g = make_game();
        g.detect_transitions(area()); // 初期状態 (村, floor_num=0) を prev に記録
        into_dungeon(&mut g); // floor_num: 0 -> 1 (同時に max_floor_reached も 0 -> 1)
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    /// 回帰テスト: 既に到達済みの深さへ再度潜った場合 (自己ベスト更新を
    /// 伴わない降下) は push_new_record ではなく push_descend が発火する
    /// はず — floor_num の直接操作で「既踏破フロアへの降下」を再現する
    /// (ascend/descend の実際の導線 (階段) はイベント解決を介するため単体
    /// テストでは floor_num を直接書き換える方が意図が明確)。
    #[test]
    fn detect_transitions_pushes_plain_descend_when_not_a_new_record() {
        let mut g = make_game();
        into_dungeon(&mut g); // floor_num: 0 -> 1, max_floor_reached: 0 -> 1
        g.state.max_floor_reached = 3; // 既に3階まで到達済みという体で上書きする
        g.state.dungeon.as_mut().unwrap().floor_num = 2;
        // ここまでの変化 (入場・到達済み扱いへの書き換え) を prev に吸収する。
        // この呼び出し自体は floor_num の増加を検知して何かしら演出を積むため、
        // drain_effects で使い切ってからでないと次のアサーションが
        // 「本当に検証対象の分岐が発火したか」を保証できない。
        g.detect_transitions(area());
        drain_effects(&g, area());
        g.state.dungeon.as_mut().unwrap().floor_num = 3; // 既踏破の3階まで降りる (自己ベスト更新なし)
        g.detect_transitions(area());
        assert!(
            g.effects.borrow().is_running(),
            "自己ベスト更新を伴わない降下でも通常の descend 演出は積まれるはず"
        );
    }

    #[test]
    fn detect_transitions_pushes_ascend_when_floor_decreases_without_reaching_village() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.state.dungeon.as_mut().unwrap().floor_num = 3;
        g.detect_transitions(area()); // ここまでを prev に吸収する
        drain_effects(&g, area()); // 上の呼び出しで積まれた演出を使い切る
        g.state.dungeon.as_mut().unwrap().floor_num = 2; // 村へは戻らず1つ浅い階へ
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    #[test]
    fn detect_transitions_pushes_effect_on_death_and_marks_reason() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.detect_transitions(area()); // ダンジョン内にいる状態を prev に記録
        drain_effects(&g, area()); // 入場自体の演出を使い切ってからでないと死亡演出の検証が汚染される
        g.state.hp = 1; // スライムの一撃 (Slime.max(1)ダメージ) で確実に0になる
        let map = g.state.dungeon.as_mut().unwrap();
        let (px, py) = (map.player_x, map.player_y);
        map.monsters.clear();
        let mut placed = false;
        for &dir in &[state::Facing::North, state::Facing::East, state::Facing::South, state::Facing::West] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            let (ux, uy) = (nx as usize, ny as usize);
            if !map.cell(ux, uy).is_walkable() { continue; }
            // can_charge=false のスライムを隣接させ、必ず通常攻撃で hp を 0 にする。
            map.monsters.push(state::Monster {
                kind: state::EnemyKind::Slime, x: ux, y: uy, hp: 10, max_hp: 10,
                awake: true, charging: false, affix: None,
            });
            placed = true;
            break;
        }
        assert!(placed, "隣接できる歩行可能マスが見つからなかった");
        logic::on_player_action(&mut g.state);
        assert_eq!(g.state.scene, Scene::Overworld, "HP0での行動後は村へ戻るはず");
        assert!(g.state.last_dungeon_exit_was_death);
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    /// `is_death_exit_this_frame` の境界値テスト。tachyonfx の `EffectManager`
    /// は「どの演出が積まれたか」を外から観測する手段を提供しないため
    /// (`is_running()` は積まれた演出の集合ではなく1つでも残っているかしか
    /// 返さない)、被弾演出の抑制条件そのものはこの純粋関数を直接検証する
    /// 方が `detect_transitions` 経由の統合テストより確実に検証できる。
    #[test]
    fn death_exit_gate_true_only_when_leaving_dungeon_via_death() {
        assert!(
            is_death_exit_this_frame(0, 1, true),
            "ダンジョン(floor_num>0)から村(floor_num=0)への死亡による退出はゲートがかかるはず"
        );
        assert!(
            !is_death_exit_this_frame(0, 0, true),
            "元々村にいた(floor_numの変化なし)場合はゲートがかからないはず"
        );
        assert!(
            !is_death_exit_this_frame(0, 1, false),
            "生還 (撤退・クリア) による退出はゲートがかからないはず"
        );
        assert!(
            !is_death_exit_this_frame(1, 2, true),
            "村に戻っていない (階段で1つ浅い階へ) 場合はゲートがかからないはず"
        );
    }

    /// 回帰テスト: 致命打を受けた瞬間、実際に `hero_hit_count` の増加と
    /// `last_dungeon_exit_was_death=true` への遷移が同じ `on_player_action`
    /// 呼び出し内で同時に起きることを確認する。これが起きなければ
    /// `is_death_exit_this_frame` によるゲート自体が不要になるため、
    /// このテストは上のゲート判定テストとセットで初めて修正の妥当性を示す。
    #[test]
    fn lethal_hit_and_death_transition_happen_within_the_same_action() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.detect_transitions(area());
        drain_effects(&g, area());
        g.state.hp = 1;
        let map = g.state.dungeon.as_mut().unwrap();
        let (px, py) = (map.player_x, map.player_y);
        map.monsters.clear();
        let mut placed = false;
        for &dir in &[state::Facing::North, state::Facing::East, state::Facing::South, state::Facing::West] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            let (ux, uy) = (nx as usize, ny as usize);
            if !map.cell(ux, uy).is_walkable() { continue; }
            map.monsters.push(state::Monster {
                kind: state::EnemyKind::Slime, x: ux, y: uy, hp: 10, max_hp: 10,
                awake: true, charging: false, affix: None,
            });
            placed = true;
            break;
        }
        assert!(placed, "隣接できる歩行可能マスが見つからなかった");
        let hero_hit_count_before = g.state.hero_hit_count;
        let floor_num_before = g.prev.get().floor_num;
        logic::on_player_action(&mut g.state);
        assert_ne!(
            g.state.hero_hit_count, hero_hit_count_before,
            "致命打も通常の被弾としてカウントされるはず"
        );
        assert!(g.state.last_dungeon_exit_was_death);
        let floor_num_after = g.state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(0);
        assert!(
            is_death_exit_this_frame(floor_num_after, floor_num_before, g.state.last_dungeon_exit_was_death),
            "このフレームは被弾検知と死亡検知が衝突するため、ゲートが働くはず"
        );
    }

    #[test]
    fn detect_transitions_pushes_effect_on_voluntary_return_and_marks_reason() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.detect_transitions(area()); // ダンジョン内にいる状態を prev に記録
        drain_effects(&g, area()); // 入場自体の演出を使い切ってからでないと帰還演出の検証が汚染される
        logic::retreat_to_town(&mut g.state);
        assert_eq!(g.state.scene, Scene::Overworld);
        assert!(!g.state.last_dungeon_exit_was_death);
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    #[test]
    fn detect_transitions_pushes_level_up_effect() {
        let mut g = make_game();
        g.detect_transitions(area()); // 起動直後 (Lv1) を prev に記録
        assert!(!g.effects.borrow().is_running());
        // 実際の撃破経路 (attack_monster) を通すと enemy_hit_count も同時に
        // 増えてしまい、「level up 由来で発火したか」を切り分けられない。
        // レベル値だけを直接変えて level up 検知を単独で検証する。
        g.state.level += 1;
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    #[test]
    fn detect_transitions_pushes_new_record_effect() {
        let mut g = make_game();
        g.detect_transitions(area());
        into_dungeon(&mut g); // max_floor_reached: 0 -> 1
        g.detect_transitions(area());
        assert!(g.effects.borrow().is_running());
    }

    #[test]
    fn overlay_open_close_in_overworld() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('I'));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
        g.handle_input(&InputEvent::Key('0'));
        assert_eq!(g.state.overlay, None);
    }

    #[test]
    fn skill_overlay_opens_in_dungeon() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.handle_input(&InputEvent::Key('Z'));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
    }

    #[test]
    fn a_button_waits_when_no_event_no_enemy() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.state.dungeon.as_mut().unwrap().monsters.clear();
        let turns_before = g.state.turn_count;
        g.handle_input(&click(AB_A_BUTTON));
        assert_eq!(g.state.turn_count, turns_before + 1);
        assert!(g.state.overlay.is_none());
    }

    #[test]
    fn a_button_does_nothing_in_overworld_without_event() {
        let mut g = make_game();
        let turns_before = g.state.turn_count;
        g.handle_input(&click(AB_A_BUTTON));
        // No event under foot, no monsters, no time tick in village.
        assert_eq!(g.state.turn_count, turns_before);
        assert!(g.state.overlay.is_none());
    }

    #[test]
    fn a_button_opens_skill_when_enemy_adjacent() {
        let mut g = make_game();
        into_dungeon(&mut g);
        let map = g.state.dungeon.as_mut().unwrap();
        let px = map.player_x;
        let py = map.player_y;
        for &dir in &[
            state::Facing::North,
            state::Facing::East,
            state::Facing::South,
            state::Facing::West,
        ] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            let ux = nx as usize; let uy = ny as usize;
            if !map.cell(ux, uy).is_walkable() { continue; }
            map.monsters.clear();
            map.monsters.push(state::Monster {
                kind: state::EnemyKind::Slime,
                x: ux, y: uy, hp: 12, max_hp: 12,
                awake: true, charging: false, affix: None,
            });
            break;
        }
        g.handle_input(&click(AB_A_BUTTON));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
    }

    #[test]
    fn b_button_opens_unified_menu() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.handle_input(&click(AB_B_BUTTON));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn menu_tab_switch_via_click() {
        let mut g = make_game();
        g.state.overlay = Some(Overlay::Inventory);
        g.handle_input(&click(MENU_TAB_SKILL));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
        g.handle_input(&click(MENU_TAB_STATUS));
        assert_eq!(g.state.overlay, Some(Overlay::Status));
        g.handle_input(&click(MENU_TAB_INVENTORY));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn failed_peddler_purchase_keeps_event_alive() {
        // Codex P1 (#95): a failed buy (insufficient gold) must NOT consume
        // the peddler tile, so the player can pick a different choice.
        let mut g = make_game();
        into_dungeon(&mut g);
        g.state.gold = 0;
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["peddler".into()],
            choices: vec![state::EventChoice {
                label: "buy".into(),
                action: state::EventAction::PeddlerBuyHerb,
            }],
        });
        let map = g.state.dungeon.as_mut().unwrap();
        let (px, py) = (map.player_x, map.player_y);
        map.grid[py][px].cell_type = state::CellType::Peddler;
        map.grid[py][px].event_done = false;
        let resolved = logic::resolve_event_choice(&mut g.state, 0);
        assert!(!resolved, "failed purchase should report failure");
        assert!(g.state.active_event.is_some(), "event should remain open");
        let map = g.state.dungeon.as_ref().unwrap();
        assert!(
            !map.grid[py][px].event_done,
            "tile should not be marked done after failed purchase"
        );
    }

    #[test]
    fn dungeon_event_stays_in_explore_scene() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["test".into()],
            choices: vec![state::EventChoice {
                label: "ok".into(),
                action: state::EventAction::Continue,
            }],
        });
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        g.handle_input(&click(AB_A_BUTTON));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        assert!(g.state.active_event.is_none());
    }

    #[test]
    fn arrow_keys_navigate_event_popup() {
        let mut g = make_game();
        into_dungeon(&mut g);
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["x".into()],
            choices: vec![
                state::EventChoice { label: "a".into(), action: state::EventAction::Continue },
                state::EventChoice { label: "b".into(), action: state::EventAction::Continue },
                state::EventChoice { label: "c".into(), action: state::EventAction::Continue },
            ],
        });
        g.state.cursor = 0;
        g.handle_input(&InputEvent::Key('j'));
        assert_eq!(g.state.cursor, 1);
        g.handle_input(&InputEvent::Key(' '));
        assert!(g.state.active_event.is_none());
    }

    #[test]
    fn b_button_closes_overlay_from_keyboard() {
        let mut g = make_game();
        g.state.overlay = Some(Overlay::Inventory);
        g.handle_input(&InputEvent::Key('b'));
        assert!(g.state.overlay.is_none());
    }

    #[test]
    fn shop_overlay_buy() {
        let mut g = make_game();
        g.state.overlay = Some(Overlay::Shop);
        g.state.gold = 200;
        g.handle_input(&InputEvent::Key('1'));
        assert!(g.state.gold < 200);
    }

    // ── Overworld-specific tests ─────────────────────────────

    #[test]
    fn reception_npc_first_visit_grants_starter_supplies() {
        let mut g = make_game();
        // Inject the reception event and resolve choice 0 (accept).
        g.state.active_event = logic::generate_overworld_event(
            &g.state,
            state::CellType::ReceptionNpc,
        );
        assert!(g.state.active_event.is_some());
        let resolved = logic::resolve_event_choice(&mut g.state, 0);
        assert!(resolved);
        assert!(g.state.met_reception);
        assert_eq!(g.state.gold, 50);
        assert!(g.state.inventory.iter().any(|i| i.kind == state::ItemKind::Herb));
    }

    #[test]
    fn blacksmith_npc_first_visit_grants_starter_equipment() {
        let mut g = make_game();
        g.state.active_event = logic::generate_overworld_event(
            &g.state,
            state::CellType::BlacksmithNpc,
        );
        let resolved = logic::resolve_event_choice(&mut g.state, 0);
        assert!(resolved);
        assert!(g.state.met_blacksmith);
        assert!(g.state.weapon().is_some());
        assert!(g.state.armor().is_some());
    }

    #[test]
    fn dungeon_entrance_event_descends() {
        let mut g = make_game();
        g.state.active_event = logic::generate_overworld_event(
            &g.state,
            state::CellType::DungeonEntrance,
        );
        let resolved = logic::resolve_event_choice(&mut g.state, 0);
        assert!(resolved);
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        assert_eq!(g.state.dungeon.as_ref().unwrap().floor_num, 1);
    }

    #[test]
    fn shop_tile_event_opens_shop_overlay() {
        let mut g = make_game();
        g.state.active_event = logic::generate_overworld_event(
            &g.state,
            state::CellType::ShopTile,
        );
        logic::resolve_event_choice(&mut g.state, 0);
        assert_eq!(g.state.overlay, Some(Overlay::Shop));
    }

    #[test]
    fn inn_tile_costs_gold_and_heals() {
        let mut g = make_game();
        g.state.gold = 50;
        g.state.hp = 1;
        g.state.active_event = logic::generate_overworld_event(
            &g.state,
            state::CellType::InnTile,
        );
        logic::resolve_event_choice(&mut g.state, 0);
        assert_eq!(g.state.gold, 40);
        assert_eq!(g.state.hp, g.state.effective_max_hp());
    }

    /// Regression for codex review (P1): the SkillChoice overlay must
    /// not be dismissible by the shared close shortcuts (B button, '0',
    /// '-', or the CLOSE_OVERLAY click). Otherwise the player can bypass
    /// the level-up gate while `pending_skill_choice` lingers.
    #[test]
    fn skill_choice_cannot_be_dismissed_by_close_shortcuts() {
        let mut g = make_game();
        g.state.overlay = Some(Overlay::SkillChoice);
        g.state.pending_skill_choice = Some((
            state::SkillKind::Heal,
            state::SkillKind::Shield,
        ));

        // Each of these would close any other overlay — they must NOT
        // close SkillChoice.
        for ch in ['b', 'B', '0', '-'] {
            let consumed = handle_key(&mut g.state, ch);
            assert!(
                !consumed,
                "key '{}' must not be accepted while SkillChoice is open",
                ch
            );
            assert_eq!(
                g.state.overlay,
                Some(Overlay::SkillChoice),
                "SkillChoice overlay must remain after key '{}'",
                ch
            );
            assert!(
                g.state.pending_skill_choice.is_some(),
                "pending_skill_choice must remain after key '{}'",
                ch
            );
        }

        // CLOSE_OVERLAY click must also be ignored.
        let click_consumed = handle_click(&mut g.state, CLOSE_OVERLAY);
        assert!(!click_consumed, "CLOSE_OVERLAY click must not be accepted");
        assert_eq!(g.state.overlay, Some(Overlay::SkillChoice));
        assert!(g.state.pending_skill_choice.is_some());

        // The valid SKILL_CHOICE_LEFT click commits and closes.
        let left_consumed = handle_click(&mut g.state, SKILL_CHOICE_LEFT);
        assert!(left_consumed);
        assert_eq!(g.state.overlay, None);
        assert!(g.state.pending_skill_choice.is_none());
        assert!(g.state.learned_skills.contains(&state::SkillKind::Heal));
    }
}
