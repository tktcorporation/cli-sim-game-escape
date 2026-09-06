//! 遠征団の状態。純粋データと導出値のみ。

use std::cell::Cell;

pub const PARTY_SIZE: usize = 3;
pub const BASE_NODES: u32 = 4;
pub const BASE_RATION_CAP: u32 = 5;
pub const MAX_BONUS_RATION_CAP: u32 = 3;
pub const BASE_RATION_REGEN_TICKS: u32 = 450;
pub const SCOUT_MEMO_CAP: u32 = 3;
pub const SCOUT_MEMO_REGEN_TICKS: u32 = 1_800;
pub const COMBAT_ROUND_TICKS: u32 = 4;
pub const LOG_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Vanguard,
    Striker,
    Support,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Vanguard => "盾",
            Role::Striker => "刃",
            Role::Support => "癒",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubTab {
    Camp,
    Roster,
}

impl HubTab {
    pub fn label(self) -> &'static str {
        match self {
            HubTab::Camp => "拠点",
            HubTab::Roster => "団員",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Camp,
    Forming,
    Running,
    Result,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Battle,
    Boss,
}

#[derive(Clone, Debug)]
pub struct Hero {
    pub id: u8,
    pub name: &'static str,
    pub role: Role,
    pub bond: u32,
    pub hp: i32,
    pub max_hp: i32,
}

impl Hero {
    pub fn atk(&self) -> i32 {
        let base = match self.role {
            Role::Vanguard => 4,
            Role::Striker => 7,
            Role::Support => 3,
        };
        base + self.bond as i32
    }

    pub fn bond_max_hp(&self) -> i32 {
        let base = match self.role {
            Role::Vanguard => 28,
            Role::Striker => 18,
            Role::Support => 20,
        };
        base + self.bond as i32 * 2
    }

    pub fn refresh_max_hp(&mut self) {
        let new_max = self.bond_max_hp();
        let missing = self.max_hp.saturating_sub(self.hp);
        self.max_hp = new_max;
        self.hp = (new_max - missing).clamp(1, new_max);
    }
}

#[derive(Clone, Debug)]
pub struct Enemy {
    pub name: &'static str,
    pub hp: i32,
    pub max_hp: i32,
    pub atk: i32,
}

#[derive(Clone, Debug)]
pub struct Sortie {
    pub depth: u32,
    pub node_index: u32,
    pub nodes_total: u32,
    pub party: [u8; PARTY_SIZE],
    pub enemy: Option<Enemy>,
    pub used_scout: bool,
    pub scout_hint: Option<&'static str>,
    /// 遠征中に一度だけ使える任意援護。使わなくても自動で完走する。
    pub aid_ready: bool,
    pub combat_tick: u32,
    pub bond_gained: u32,
}

#[derive(Clone, Debug)]
pub struct ExpeditionState {
    pub screen: Screen,
    pub hub_tab: HubTab,
    pub roster: Vec<Hero>,
    pub forming: [Option<u8>; PARTY_SIZE],
    pub rations: u32,
    pub ration_progress: u32,
    pub scout_memos: u32,
    pub scout_progress: u32,
    pub best_depth: u32,
    pub sortie: Option<Sortie>,
    pub log: Vec<String>,
    pub result_summary: String,
    pub elapsed_ticks: u64,
    pub last_wall_ms: u64,
    pub camp_scroll: Cell<i32>,
}

impl Default for ExpeditionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpeditionState {
    pub fn new() -> Self {
        let mut roster = vec![
            Hero { id: 0, name: "灰", role: Role::Vanguard, bond: 0, hp: 28, max_hp: 28 },
            Hero { id: 1, name: "焔", role: Role::Striker, bond: 0, hp: 18, max_hp: 18 },
            Hero { id: 2, name: "雫", role: Role::Support, bond: 0, hp: 20, max_hp: 20 },
            Hero { id: 3, name: "嵐", role: Role::Striker, bond: 0, hp: 18, max_hp: 18 },
        ];
        for h in &mut roster {
            h.refresh_max_hp();
            h.hp = h.max_hp;
        }
        Self {
            screen: Screen::Camp,
            hub_tab: HubTab::Camp,
            roster,
            forming: [Some(0), Some(1), Some(2)],
            rations: BASE_RATION_CAP,
            ration_progress: 0,
            scout_memos: 1,
            scout_progress: 0,
            best_depth: 1,
            sortie: None,
            log: Vec::new(),
            result_summary: String::new(),
            elapsed_ticks: 0,
            last_wall_ms: 0,
            camp_scroll: Cell::new(0),
        }
    }

    pub fn total_bond(&self) -> u32 {
        self.roster.iter().map(|h| h.bond).sum()
    }

    pub fn ration_cap(&self) -> u32 {
        BASE_RATION_CAP + (self.total_bond() / 8).min(MAX_BONUS_RATION_CAP)
    }

    pub fn ration_regen_ticks(&self) -> u32 {
        let bonus = self.total_bond().min(25);
        (BASE_RATION_REGEN_TICKS * 100 / (100 + bonus * 2)).max(200)
    }

    pub fn push_log(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
        if self.log.len() > LOG_CAP {
            let n = self.log.len() - LOG_CAP;
            self.log.drain(0..n);
        }
    }

    pub fn hero_mut(&mut self, id: u8) -> Option<&mut Hero> {
        self.roster.iter_mut().find(|h| h.id == id)
    }

    pub fn hero(&self, id: u8) -> Option<&Hero> {
        self.roster.iter().find(|h| h.id == id)
    }

    pub fn forming_count(&self) -> usize {
        self.forming.iter().flatten().count()
    }
}
