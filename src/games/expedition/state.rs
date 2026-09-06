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
pub const STAGES_PER_CHAPTER: u32 = 4;
/// 節クリアで得られる補給の基礎値。
pub const SUPPLY_CLEAR_BASE: u32 = 3;
/// 敗退時の持ち帰り補給。
pub const SUPPLY_FAIL: u32 = 1;

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
    Train,
}

impl HubTab {
    pub fn label(self) -> &'static str {
        match self {
            HubTab::Camp => "拠点",
            HubTab::Train => "育成",
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
    pub level: u32,
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
        base + self.level as i32
    }

    pub fn level_max_hp(&self) -> i32 {
        let base = match self.role {
            Role::Vanguard => 28,
            Role::Striker => 18,
            Role::Support => 20,
        };
        base + self.level as i32 * 2
    }

    pub fn refresh_max_hp(&mut self) {
        let new_max = self.level_max_hp();
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
    pub chapter: u32,
    pub stage: u32,
    /// 敵スケール用。`(chapter-1)*STAGES + stage`
    pub difficulty: u32,
    pub node_index: u32,
    pub nodes_total: u32,
    pub party: [u8; PARTY_SIZE],
    pub enemy: Option<Enemy>,
    pub used_scout: bool,
    pub scout_hint: Option<&'static str>,
    /// 遠征中に一度だけ使える任意援護。使わなくても自動で完走する。
    pub aid_ready: bool,
    pub combat_tick: u32,
    pub supplies_gained: u32,
    pub last_hit_log: String,
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
    /// 次に攻略する章（1始まり）。
    pub chapter: u32,
    /// 次に攻略する節（1..=STAGES_PER_CHAPTER）。
    pub stage: u32,
    /// 拠点育成用の通貨。探索クリアで増え、レベル上げで減る。
    pub supplies: u32,
    /// 直近の探索が敗退なら true（育成誘導用）。
    pub last_failed: bool,
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
            Hero { id: 0, name: "灰", role: Role::Vanguard, level: 1, hp: 28, max_hp: 28 },
            Hero { id: 1, name: "焔", role: Role::Striker, level: 1, hp: 18, max_hp: 18 },
            Hero { id: 2, name: "雫", role: Role::Support, level: 1, hp: 20, max_hp: 20 },
            Hero { id: 3, name: "嵐", role: Role::Striker, level: 1, hp: 18, max_hp: 18 },
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
            chapter: 1,
            stage: 1,
            supplies: 0,
            last_failed: false,
            sortie: None,
            log: Vec::new(),
            result_summary: String::new(),
            elapsed_ticks: 0,
            last_wall_ms: 0,
            camp_scroll: Cell::new(0),
        }
    }

    pub fn total_level(&self) -> u32 {
        self.roster.iter().map(|h| h.level).sum()
    }

    pub fn ration_cap(&self) -> u32 {
        BASE_RATION_CAP + (self.total_level() / 8).min(MAX_BONUS_RATION_CAP)
    }

    pub fn ration_regen_ticks(&self) -> u32 {
        let bonus = self.total_level().min(25);
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

    pub fn stage_label(chapter: u32, stage: u32) -> String {
        format!("{chapter}-{stage}")
    }

    pub fn current_stage_label(&self) -> String {
        Self::stage_label(self.chapter, self.stage)
    }

    pub fn difficulty(chapter: u32, stage: u32) -> u32 {
        chapter.saturating_sub(1) * STAGES_PER_CHAPTER + stage
    }

    pub fn current_difficulty(&self) -> u32 {
        Self::difficulty(self.chapter, self.stage)
    }

    pub fn is_boss_stage(stage: u32) -> bool {
        stage >= STAGES_PER_CHAPTER
    }

    /// 団員1人を1レベル上げる補給コスト。
    pub fn upgrade_cost(level: u32) -> u32 {
        level + 1
    }
}
