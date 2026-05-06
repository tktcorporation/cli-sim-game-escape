//! 深淵潜行 (Abyss Idle) — 自動戦闘でフロアを潜っていく放置型ローグ。
//!
//! コアループ:
//!   1. 勇者が現フロアの敵と自動戦闘 (装着中の装備が英雄ステを決める)
//!   2. 雑魚 8 体を倒すとボス出現 → 撃破で次フロアへ
//!   3. gold で **装備購入** (lane の前装備が prereq) と **装着中装備の強化**
//!   4. 死亡すると B1F に戻されるが、装備・強化レベル・魂は永続
//!
//! 戦略性: 「次の lane 装備を買う」「いま装着中の装備を強化する」「他の lane に
//! 投資する」のジレンマで gold を割り振っていく。

pub mod actions;
pub mod config;
pub mod effects;
pub mod logic;
pub mod policy;
pub mod render;
pub mod save;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;
use tachyonfx::Duration;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

fn now_ms() -> Option<f64> {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now())
}

use actions::*;
use effects::AbyssEffects;
use policy::PlayerAction;
use state::{AbyssState, EquipmentId, EquipmentLane, SoulPerk, Tab, TabGroup, EQUIPMENT_COUNT};

pub struct AbyssGame {
    pub state: AbyssState,
    effects: RefCell<AbyssEffects>,
    prev: Cell<PrevSnapshot>,
    last_render_ms: Cell<f64>,
    save_countdown: u32,
}

/// この PlayerAction を適用したらセーブを発火させるか。
fn is_save_worthy(action: PlayerAction) -> bool {
    matches!(
        action,
        PlayerAction::BuyEquipment(_)
            | PlayerAction::EquipItem(_)
            | PlayerAction::EnhanceEquipment(_)
            | PlayerAction::BuySoulPerk(_)
            | PlayerAction::GachaPull(_)
            | PlayerAction::Retreat
            | PlayerAction::ToggleAutoDescend
    )
}

#[derive(Clone, Copy, Default)]
struct PrevSnapshot {
    floor: u32,
    enemy_hurt_flash: u32,
    hero_hurt_flash: u32,
    enemy_is_boss: bool,
    last_enemy_dmg: Option<(u64, bool)>,
    gacha_total_pulls: u64,
    owned_equipment_count: u32,
}

impl AbyssGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = AbyssState::new();

        #[cfg(target_arch = "wasm32")]
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロードしました");
        }

        let prev = Self::snapshot(&state);
        Self {
            state,
            effects: RefCell::new(AbyssEffects::new()),
            prev: Cell::new(prev),
            last_render_ms: Cell::new(0.0),
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn snapshot(s: &AbyssState) -> PrevSnapshot {
        PrevSnapshot {
            floor: s.floor,
            enemy_hurt_flash: s.enemy_hurt_flash,
            hero_hurt_flash: s.hero_hurt_flash,
            enemy_is_boss: s.current_enemy.is_boss,
            last_enemy_dmg: s.last_enemy_damage.map(|(a, _, c)| (a, c)),
            gacha_total_pulls: s.total_pulls,
            owned_equipment_count: s.owned_equipment.iter().filter(|b| **b).count() as u32,
        }
    }

    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let layout = render::compute_layout(area);
        let s = &self.state;

        if s.floor > prev.floor {
            effects.push_descend(area);
        } else if s.floor < prev.floor {
            effects.push_ascend_or_death(area);
        }

        if prev.enemy_hurt_flash == 0 && s.enemy_hurt_flash > 0 {
            effects.push_enemy_hit(layout.enemy_panel);
        }

        if prev.hero_hurt_flash == 0 && s.hero_hurt_flash > 0 {
            effects.push_hero_hit(layout.hero_panel);
        }

        if !prev.enemy_is_boss && s.current_enemy.is_boss {
            effects.push_boss_appearance(layout.combat);
        }

        if prev.enemy_is_boss && !s.current_enemy.is_boss && s.floor > prev.floor {
            effects.push_boss_defeated(layout.enemy_panel);
        }

        let cur_dmg = s.last_enemy_damage.map(|(a, _, c)| (a, c));
        if cur_dmg != prev.last_enemy_dmg {
            if let Some((_, true)) = cur_dmg {
                effects.push_critical(layout.combat);
            }
        }

        if s.total_pulls > prev.gacha_total_pulls {
            if let Some(g) = &s.last_gacha {
                if g.by_tier[3] > 0 {
                    effects.push_gacha_legendary(layout.body);
                }
            }
        }

        let cur_owned = s.owned_equipment.iter().filter(|b| **b).count() as u32;
        if cur_owned > prev.owned_equipment_count {
            effects.push_equipment_unlock(layout.body);
        }

        self.prev.set(Self::snapshot(s));
    }

    fn compute_elapsed(&self) -> Duration {
        let now = now_ms().unwrap_or(0.0);
        let prev = self.last_render_ms.get();
        self.last_render_ms.set(now);
        if prev == 0.0 {
            Duration::ZERO
        } else {
            let delta_ms = (now - prev).clamp(0.0, 100.0);
            if !delta_ms.is_finite() {
                return Duration::ZERO;
            }
            Duration::from_millis(delta_ms as u32)
        }
    }

    fn click_to_action(&self, action_id: u16) -> Option<PlayerAction> {
        match action_id {
            // ── サブタブ直接切替 ──
            TAB_UPGRADES => Some(PlayerAction::SetTab(Tab::Upgrades)),
            TAB_ROADMAP => Some(PlayerAction::SetTab(Tab::Roadmap)),
            TAB_STATS => Some(PlayerAction::SetTab(Tab::Stats)),
            TAB_GACHA => Some(PlayerAction::SetTab(Tab::Gacha)),
            TAB_SETTINGS => Some(PlayerAction::SetTab(Tab::Settings)),
            TAB_SHOP => Some(PlayerAction::SetTab(Tab::Shop)),
            TAB_SOULS => Some(PlayerAction::SetTab(Tab::Souls)),
            // ── トップグループ切替 ──
            TAB_GROUP_GROWTH => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Growth))),
            TAB_GROUP_INFO => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Info))),
            TAB_GROUP_GACHA => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Gacha))),
            TAB_GROUP_SETTINGS => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Settings))),
            TOGGLE_AUTO_DESCEND => Some(PlayerAction::ToggleAutoDescend),
            RETREAT_TO_SURFACE => Some(PlayerAction::Retreat),
            GACHA_PULL_1 => Some(PlayerAction::GachaPull(1)),
            GACHA_PULL_10 => Some(PlayerAction::GachaPull(10)),
            SCROLL_UP => Some(PlayerAction::ScrollUp),
            SCROLL_DOWN => Some(PlayerAction::ScrollDown),
            id if (BUY_SOUL_PERK_BASE..BUY_SOUL_PERK_BASE + 4).contains(&id) => {
                let idx = (id - BUY_SOUL_PERK_BASE) as usize;
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            id if (BUY_EQUIPMENT_BASE..BUY_EQUIPMENT_BASE + EQUIPMENT_COUNT as u16)
                .contains(&id) =>
            {
                let idx = (id - BUY_EQUIPMENT_BASE) as usize;
                EquipmentId::from_index(idx).map(PlayerAction::BuyEquipment)
            }
            id if (EQUIP_ITEM_BASE..EQUIP_ITEM_BASE + EQUIPMENT_COUNT as u16)
                .contains(&id) =>
            {
                let idx = (id - EQUIP_ITEM_BASE) as usize;
                EquipmentId::from_index(idx).map(PlayerAction::EquipItem)
            }
            id if (ENHANCE_EQUIPMENT_BASE..ENHANCE_EQUIPMENT_BASE + EQUIPMENT_COUNT as u16)
                .contains(&id) =>
            {
                let idx = (id - ENHANCE_EQUIPMENT_BASE) as usize;
                EquipmentId::from_index(idx).map(PlayerAction::EnhanceEquipment)
            }
            _ => None,
        }
    }

    fn preserve_or_default(&self, group: TabGroup) -> Tab {
        if TabGroup::from_tab(self.state.tab) == group {
            self.state.tab
        } else {
            group.default_tab()
        }
    }

    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }

    fn key_to_action(&self, ch: char) -> Option<PlayerAction> {
        match ch {
            // タブグループ切替。
            '{' => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Growth))),
            '|' => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Info))),
            '}' => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Gacha))),
            '~' => Some(PlayerAction::SetTab(self.preserve_or_default(TabGroup::Settings))),
            'a' | 'A' => Some(PlayerAction::ToggleAutoDescend),
            'p' | 'P' => Some(PlayerAction::Retreat),
            // 強化サブタブ: 1=武器 / 2=防具 / 3=装飾 lane の装着中装備を強化。
            // 「装着中の 3 装備しか強化できない」設計なので、数字キーは lane 番号に対応する。
            '1'..='3' if matches!(self.state.tab, Tab::Upgrades) => {
                let lane = match ch {
                    '1' => EquipmentLane::Weapon,
                    '2' => EquipmentLane::Armor,
                    '3' => EquipmentLane::Accessory,
                    _ => unreachable!(),
                };
                self.state.equipped_at(lane).map(PlayerAction::EnhanceEquipment)
            }
            // 魂サブタブ: 1-4 で各魂パーク購入。
            '1'..='4' if matches!(self.state.tab, Tab::Souls) => {
                let idx = (ch as u8 - b'1') as usize;
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            // 旧 Q/W/E/R 互換 (魂サブタブ内のみ)。
            'Q' | 'W' | 'E' | 'R' if matches!(self.state.tab, Tab::Souls) => {
                let idx = match ch {
                    'Q' => 0,
                    'W' => 1,
                    'E' => 2,
                    'R' => 3,
                    _ => unreachable!(),
                };
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            's' | 'S' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(1)),
            'x' | 'X' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(10)),
            'j' | 'J' => Some(PlayerAction::ScrollDown),
            'k' | 'K' => Some(PlayerAction::ScrollUp),
            _ => None,
        }
    }
}

impl Game for AbyssGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Abyss
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let action = match event {
            InputEvent::Key(c) => self.key_to_action(*c),
            InputEvent::Click(_, id) => self.click_to_action(*id),
        };
        if let Some(a) = action {
            let save_after = is_save_worthy(a);
            logic::apply_action(&mut self.state, a);
            if save_after {
                self.flush_save();
            }
            true
        } else {
            false
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let prev_floor = self.state.floor;
        let prev_deaths = self.state.deaths;
        logic::tick(&mut self.state, delta_ticks);

        let event_save = self.state.floor != prev_floor || self.state.deaths != prev_deaths;
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        let timer_save = self.save_countdown == 0;

        if event_save || timer_save {
            self.flush_save();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        self.detect_transitions(area);
        render::render(&self.state, f, area, click_state);
        let elapsed = self.compute_elapsed();
        self.effects
            .borrow_mut()
            .process(elapsed, f.buffer_mut(), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Abyss), id)
    }

    #[test]
    fn create_game() {
        let g = AbyssGame::new();
        assert_eq!(g.state.floor, 1);
    }

    #[test]
    fn click_subtab_switch() {
        let mut g = AbyssGame::new();
        g.handle_input(&click(TAB_ROADMAP));
        assert_eq!(g.state.tab, Tab::Roadmap);
        g.handle_input(&click(TAB_STATS));
        assert_eq!(g.state.tab, Tab::Stats);
        g.handle_input(&click(TAB_UPGRADES));
        assert_eq!(g.state.tab, Tab::Upgrades);
        g.handle_input(&click(TAB_SOULS));
        assert_eq!(g.state.tab, Tab::Souls);
    }

    #[test]
    fn click_top_group_goes_to_default_tab() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Roadmap;
        g.handle_input(&click(TAB_GROUP_GROWTH));
        assert_eq!(g.state.tab, Tab::Upgrades);
        g.handle_input(&click(TAB_GROUP_INFO));
        assert_eq!(g.state.tab, Tab::Roadmap);
    }

    #[test]
    fn click_top_group_preserves_subtab_when_already_inside() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Souls;
        g.handle_input(&click(TAB_GROUP_GROWTH));
        assert_eq!(g.state.tab, Tab::Souls);
    }

    /// 装備購入クリックで gold 消費 + 自動装着。
    #[test]
    fn click_buy_equipment_purchases_and_equips() {
        let mut g = AbyssGame::new();
        g.state.gold = 1_000_000;
        let click_id = BUY_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16;
        g.handle_input(&click(click_id));
        assert!(g.state.owned_equipment[EquipmentId::BronzeSword.index()]);
        assert_eq!(
            g.state.equipped[EquipmentLane::Weapon.index()],
            Some(EquipmentId::BronzeSword)
        );
    }

    /// 強化クリックで Lv が上がる。
    #[test]
    fn click_enhance_equipment_raises_level() {
        let mut g = AbyssGame::new();
        g.state.gold = 1_000_000;
        // 銅剣を買って装着。
        g.handle_input(&click(BUY_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16));
        let click_id = ENHANCE_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16;
        g.handle_input(&click(click_id));
        assert_eq!(g.state.equipment_levels[EquipmentId::BronzeSword.index()], 1);
    }

    /// 強化サブタブで '1' は装着中の武器を強化する。
    #[test]
    fn key_1_in_upgrades_tab_enhances_equipped_weapon() {
        let mut g = AbyssGame::new();
        g.state.gold = 1_000_000;
        // 銅剣を買って装着 (Weapon lane に入る)。
        g.handle_input(&click(BUY_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16));
        // Upgrades タブで '1' → Weapon の装着中装備 (BronzeSword) を強化。
        g.state.tab = Tab::Upgrades;
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.equipment_levels[EquipmentId::BronzeSword.index()], 1);
    }

    /// 装着していない lane で数字キーを押しても何も起きない (no-op)。
    #[test]
    fn key_1_no_op_when_lane_empty() {
        let mut g = AbyssGame::new();
        g.state.gold = 1_000_000;
        g.state.tab = Tab::Upgrades;
        let consumed = g.handle_input(&InputEvent::Key('1'));
        // Weapon lane 装着なしなので Action 生成 → None で消費されない。
        assert!(!consumed);
    }

    #[test]
    fn toggle_auto_descend_via_key() {
        let mut g = AbyssGame::new();
        let before = g.state.auto_descend;
        g.handle_input(&InputEvent::Key('a'));
        assert_ne!(g.state.auto_descend, before);
    }

    #[test]
    fn buy_soul_perk_via_key_on_souls_tab() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Souls;
        g.state.souls = 100;
        g.handle_input(&InputEvent::Key('Q'));
        assert_eq!(g.state.soul_perks[SoulPerk::Might.index()], 1);
        g.handle_input(&InputEvent::Key('2'));
        assert_eq!(g.state.soul_perks[SoulPerk::Endurance.index()], 1);
    }

    /// Tab::Upgrades タブで Q/W/E/R を押しても **魂は買わない** (魂サブタブ独立後の規約)。
    #[test]
    fn upgrades_tab_does_not_consume_soul_keys() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Upgrades;
        g.state.souls = 999;
        let consumed = g.handle_input(&InputEvent::Key('Q'));
        assert!(!consumed);
        assert_eq!(g.state.soul_perks[SoulPerk::Might.index()], 0);
    }

    /// 小文字 `q` は handle_input で消費されない (= main.rs のメニュー戻りキーが効く)。
    #[test]
    fn lowercase_q_does_not_consume_input_on_upgrades_tab() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Upgrades;
        g.state.souls = 999_999;
        let consumed = g.handle_input(&InputEvent::Key('q'));
        assert!(!consumed);
    }

    #[test]
    fn tick_advances_combat() {
        let mut g = AbyssGame::new();
        g.tick(1);
        assert!(g.state.current_enemy.max_hp > 0);
    }

    #[test]
    fn retreat_via_key() {
        let mut g = AbyssGame::new();
        g.state.floor = 5;
        g.handle_input(&InputEvent::Key('p'));
        assert_eq!(g.state.floor, 1);
    }

    #[test]
    fn timer_save_fires_after_autosave_interval() {
        let mut g = AbyssGame::new();
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
        g.tick(save::AUTOSAVE_INTERVAL);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    /// 装備購入はイベントセーブ発火 → タイマー満タンに戻る。
    #[test]
    fn event_save_resets_timer_to_avoid_double_write() {
        let mut g = AbyssGame::new();
        g.state.gold = 1_000_000;
        g.tick(100);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL - 100);
        g.handle_input(&click(BUY_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16));
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    #[test]
    fn save_worthy_actions_classified_correctly() {
        assert!(is_save_worthy(PlayerAction::BuyEquipment(EquipmentId::BronzeSword)));
        assert!(is_save_worthy(PlayerAction::EquipItem(EquipmentId::BronzeSword)));
        assert!(is_save_worthy(PlayerAction::EnhanceEquipment(EquipmentId::BronzeSword)));
        assert!(is_save_worthy(PlayerAction::BuySoulPerk(SoulPerk::Might)));
        assert!(is_save_worthy(PlayerAction::GachaPull(1)));
        assert!(is_save_worthy(PlayerAction::Retreat));
        assert!(is_save_worthy(PlayerAction::ToggleAutoDescend));
        // SetTab はセーブ非発火 (UI のみ)。
        assert!(!is_save_worthy(PlayerAction::SetTab(Tab::Roadmap)));
        assert!(!is_save_worthy(PlayerAction::ScrollUp));
    }
}
