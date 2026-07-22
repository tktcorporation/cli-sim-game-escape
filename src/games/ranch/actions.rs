//! Semantic action IDs and player actions for つぶ牧場 (Tsubu Ranch).
//!
//! Each constant represents a distinct clickable action in the UI.
//! These IDs are registered during render and dispatched via `InputEvent::Click`.

use super::state::{Affinity, Species, Tab};

// ── Tab navigation ──────────────────────────────────────────────
pub const TAB_HABITAT: u16 = 1;
pub const TAB_DEX: u16 = 2;
pub const TAB_BATTLE: u16 = 3;

// ── Habitat actions ─────────────────────────────────────────────
/// 餌やり方針のトグル (base + Affinity::index() 0..3)。
pub const FEED_BASE: u16 = 10;
pub const UPGRADE_CAPACITY: u16 = 20;

// ── Battle actions ───────────────────────────────────────────────
/// 対戦チームへの編成トグル (base + Species::index() 0..SPECIES_COUNT)。
pub const TOGGLE_TEAM_BASE: u16 = 100;

// ── Scroll ───────────────────────────────────────────────────────
pub const SCROLL_UP: u16 = 200;
pub const SCROLL_DOWN: u16 = 201;

/// プレイヤーの入力が意味する操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    SetTab(Tab),
    ToggleFeedFocus(Affinity),
    UpgradeCapacity,
    ToggleTeamMember(Species),
    ScrollUp,
    ScrollDown,
}

/// クリックIDから `PlayerAction` を解決する。
pub fn action_for_click(id: u16) -> Option<PlayerAction> {
    match id {
        TAB_HABITAT => Some(PlayerAction::SetTab(Tab::Habitat)),
        TAB_DEX => Some(PlayerAction::SetTab(Tab::Dex)),
        TAB_BATTLE => Some(PlayerAction::SetTab(Tab::Battle)),
        UPGRADE_CAPACITY => Some(PlayerAction::UpgradeCapacity),
        SCROLL_UP => Some(PlayerAction::ScrollUp),
        SCROLL_DOWN => Some(PlayerAction::ScrollDown),
        id if (FEED_BASE..FEED_BASE + super::state::AFFINITY_COUNT as u16).contains(&id) => {
            let idx = (id - FEED_BASE) as usize;
            Affinity::from_index(idx).map(PlayerAction::ToggleFeedFocus)
        }
        id if (TOGGLE_TEAM_BASE..TOGGLE_TEAM_BASE + super::state::SPECIES_COUNT as u16)
            .contains(&id) =>
        {
            let idx = (id - TOGGLE_TEAM_BASE) as usize;
            Species::from_index(idx).map(PlayerAction::ToggleTeamMember)
        }
        _ => None,
    }
}

/// キー入力から `PlayerAction` を解決する。
pub fn action_for_key(ch: char, current_tab: Tab) -> Option<PlayerAction> {
    match ch {
        '{' => Some(PlayerAction::SetTab(Tab::Habitat)),
        '|' => Some(PlayerAction::SetTab(Tab::Dex)),
        '}' => Some(PlayerAction::SetTab(Tab::Battle)),
        'j' | 'J' => Some(PlayerAction::ScrollDown),
        'k' | 'K' => Some(PlayerAction::ScrollUp),
        '1'..='3' if matches!(current_tab, Tab::Habitat) => {
            let idx = (ch as u8 - b'1') as usize;
            Affinity::from_index(idx).map(PlayerAction::ToggleFeedFocus)
        }
        'c' | 'C' if matches!(current_tab, Tab::Habitat) => Some(PlayerAction::UpgradeCapacity),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_resolves_tabs() {
        assert_eq!(action_for_click(TAB_HABITAT), Some(PlayerAction::SetTab(Tab::Habitat)));
        assert_eq!(action_for_click(TAB_DEX), Some(PlayerAction::SetTab(Tab::Dex)));
        assert_eq!(action_for_click(TAB_BATTLE), Some(PlayerAction::SetTab(Tab::Battle)));
    }

    #[test]
    fn click_resolves_feed_focus_by_affinity_index() {
        assert_eq!(
            action_for_click(FEED_BASE),
            Some(PlayerAction::ToggleFeedFocus(Affinity::Aqua))
        );
        assert_eq!(
            action_for_click(FEED_BASE + 1),
            Some(PlayerAction::ToggleFeedFocus(Affinity::Flare))
        );
        assert_eq!(
            action_for_click(FEED_BASE + 2),
            Some(PlayerAction::ToggleFeedFocus(Affinity::Earth))
        );
        assert_eq!(action_for_click(FEED_BASE + 3), None);
    }

    #[test]
    fn click_resolves_toggle_team_by_species_index() {
        assert_eq!(
            action_for_click(TOGGLE_TEAM_BASE),
            Some(PlayerAction::ToggleTeamMember(Species::Tsubu))
        );
        assert_eq!(
            action_for_click(TOGGLE_TEAM_BASE + 9),
            Some(PlayerAction::ToggleTeamMember(Species::SwampTurtle))
        );
        assert_eq!(action_for_click(TOGGLE_TEAM_BASE + 10), None);
    }

    #[test]
    fn click_unknown_id_returns_none() {
        assert_eq!(action_for_click(9999), None);
    }

    #[test]
    fn key_feed_focus_only_active_on_habitat_tab() {
        assert_eq!(
            action_for_key('1', Tab::Habitat),
            Some(PlayerAction::ToggleFeedFocus(Affinity::Aqua))
        );
        assert_eq!(action_for_key('1', Tab::Battle), None);
    }

    #[test]
    fn key_capacity_upgrade_shortcut() {
        assert_eq!(
            action_for_key('c', Tab::Habitat),
            Some(PlayerAction::UpgradeCapacity)
        );
        assert_eq!(action_for_key('c', Tab::Dex), None);
    }

    #[test]
    fn key_tab_group_switch() {
        assert_eq!(action_for_key('{', Tab::Battle), Some(PlayerAction::SetTab(Tab::Habitat)));
        assert_eq!(action_for_key('|', Tab::Habitat), Some(PlayerAction::SetTab(Tab::Dex)));
        assert_eq!(action_for_key('}', Tab::Habitat), Some(PlayerAction::SetTab(Tab::Battle)));
    }
}
