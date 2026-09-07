//! 遠征団の状態。純粋データと導出値のみ。

use std::cell::Cell;

pub const PARTY_SIZE: usize = 3;
pub const PATH_LEN: usize = 5;
pub const BASE_RATION_CAP: u32 = 5;
pub const MAX_BONUS_RATION_CAP: u32 = 3;
pub const BASE_RATION_REGEN_TICKS: u32 = 450;
pub const LOG_CAP: usize = 8;
pub const STAGES_PER_CHAPTER: u32 = 4;
/// 節クリアで得られるメダルの基礎値。
pub const MEDAL_CLEAR_BASE: u32 = 5;
/// 敗退時の持ち帰りメダル。
pub const MEDAL_FAIL: u32 = 2;
/// プッシャー横幅（レーン数）。
pub const PUSH_W: usize = 5;
/// プッシャー奥行き。手前端 (DEPTH-1) から落下する。
pub const PUSH_D: usize = 4;
/// マスに積めるメダル上限。
pub const CELL_CAP: u8 = 9;
/// 押し板が進む間隔 (tick)。
pub const PUSHER_STEP_TICKS: u32 = 5;
/// レベルアップに必要な光珠。
pub const ORB_NEED: u32 = 3;
/// 敵が1マス進む間隔。
pub const ENEMY_STEP_TICKS: u32 = 6;
/// 団員が攻撃する間隔。
pub const HERO_ATK_TICKS: u32 = 4;

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

    /// 攻撃が届く最大距離（マス差）。
    pub fn range(self) -> i32 {
        match self {
            Role::Vanguard => 1,
            Role::Striker => 3,
            Role::Support => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubTab {
    Camp,
    Arcade,
}

impl HubTab {
    pub fn label(self) -> &'static str {
        match self {
            HubTab::Camp => "拠点",
            HubTab::Arcade => "遊技場",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Camp,
    Forming,
    Placing,
    Running,
    Result,
}

#[derive(Clone, Debug)]
pub struct Hero {
    pub id: u8,
    pub name: &'static str,
    pub role: Role,
    pub level: u32,
}

impl Hero {
    pub fn atk(&self) -> i32 {
        let base = match self.role {
            Role::Vanguard => 5,
            Role::Striker => 4,
            Role::Support => 2,
        };
        base + self.level as i32
    }
}

#[derive(Clone, Debug)]
pub struct Creep {
    pub name: &'static str,
    pub hp: i32,
    pub max_hp: i32,
    /// 道上の連続位置。0.0 = 出現側、PATH_LEN as f64 = 門（到達で漏洩）。
    pub pos: f64,
    /// 遅延残り tick（癒の減速）。
    pub slow_ticks: u32,
}

/// 防衛画面のワールド座標（Canvas）。道は左→右へ進む。
pub const WORLD_W: f64 = 100.0;
pub const WORLD_H: f64 = 36.0;
pub const PATH_Y: f64 = 18.0;
pub const PATH_X0: f64 = 8.0;
pub const PATH_X1: f64 = 88.0;

/// プッシャー Canvas 座標。
pub const PUSH_WORLD_W: f64 = 50.0;
pub const PUSH_WORLD_H: f64 = 42.0;

/// 拠点情景パネル座標。
pub const CAMP_AMB_W: f64 = 40.0;
pub const CAMP_AMB_H: f64 = 60.0;

/// 道上の連続位置 → ワールド x。
pub fn path_to_world_x(pos: f64) -> f64 {
    let t = (pos / PATH_LEN as f64).clamp(0.0, 1.0);
    PATH_X0 + (PATH_X1 - PATH_X0) * t
}

#[derive(Clone, Debug)]
pub struct Sortie {
    pub chapter: u32,
    pub stage: u32,
    pub difficulty: u32,
    pub party: [u8; PARTY_SIZE],
    /// 道の各マスにいる団員。None = 空き。
    pub path: [Option<u8>; PATH_LEN],
    pub placing_cursor: usize,
    pub creeps: Vec<Creep>,
    pub wave_index: u32,
    pub waves_total: u32,
    pub spawn_cooldown: u32,
    pub pending_spawns: u32,
    pub gate_hp: i32,
    pub gate_max_hp: i32,
    pub combat_tick: u32,
    pub medals_gained: u32,
    pub last_hit_log: String,
}

/// プッシャー1マス。メダル枚数と光珠の有無。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushCell {
    pub medals: u8,
    pub has_orb: bool,
}

#[derive(Clone, Debug)]
pub struct Pusher {
    /// [奥行き][レーン]。row 0 = 投入側、row PUSH_D-1 = 落下端。
    pub cells: [[PushCell; PUSH_W]; PUSH_D],
    pub step_progress: u32,
    /// 押し板の視覚位相 0..PUSH_W（往復表示用）。
    pub plate_col: usize,
    pub plate_dir: i8,
    /// 直近に落ちたメダル（演出用）。
    pub last_drop_medals: u32,
    pub last_drop_orb: bool,
}

impl Pusher {
    pub fn new_seeded(next: &mut dyn FnMut() -> u32) -> Self {
        let mut cells = [[PushCell::default(); PUSH_W]; PUSH_D];
        for row in cells.iter_mut().skip(1) {
            for cell in row.iter_mut() {
                let n = (next() % 3) as u8;
                cell.medals = n;
            }
        }
        let oc = (next() as usize) % PUSH_W;
        cells[1][oc].has_orb = true;
        Self {
            cells,
            step_progress: 0,
            plate_col: 0,
            plate_dir: 1,
            last_drop_medals: 0,
            last_drop_orb: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExpeditionState {
    pub screen: Screen,
    pub hub_tab: HubTab,
    pub roster: Vec<Hero>,
    pub forming: [Option<u8>; PARTY_SIZE],
    pub rations: u32,
    pub ration_progress: u32,
    /// 次に攻略する章（1始まり）。
    pub chapter: u32,
    /// 次に攻略する節（1..=STAGES_PER_CHAPTER）。
    pub stage: u32,
    /// 遊技場用メダル。戦役で増え、プッシャー投入で減る。
    pub medals: u32,
    pub pusher: Pusher,
    /// 落とした光珠の累計（ORB_NEED でレベル選択）。
    pub orb_gauge: u32,
    /// 光珠規定数到達後、レベルを上げる団員を選ぶ待ち。
    pub pending_level_pick: bool,
    /// 直近の探索が敗退なら true（遊技場誘導用）。
    pub last_failed: bool,
    pub sortie: Option<Sortie>,
    pub log: Vec<String>,
    pub result_summary: String,
    pub elapsed_ticks: u64,
    pub last_wall_ms: u64,
    pub rng: u32,
    pub camp_scroll: Cell<i32>,
}

impl Default for ExpeditionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpeditionState {
    pub fn new() -> Self {
        let mut rng = 0xC0FFEE_u32;
        let mut next = || {
            let mut x = rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            rng = if x == 0 { 1 } else { x };
            rng
        };
        let pusher = Pusher::new_seeded(&mut next);
        Self {
            screen: Screen::Camp,
            hub_tab: HubTab::Camp,
            roster: vec![
                Hero {
                    id: 0,
                    name: "灰",
                    role: Role::Vanguard,
                    level: 1,
                },
                Hero {
                    id: 1,
                    name: "焔",
                    role: Role::Striker,
                    level: 1,
                },
                Hero {
                    id: 2,
                    name: "雫",
                    role: Role::Support,
                    level: 1,
                },
                Hero {
                    id: 3,
                    name: "嵐",
                    role: Role::Striker,
                    level: 1,
                },
            ],
            forming: [Some(0), Some(1), Some(2)],
            rations: BASE_RATION_CAP,
            ration_progress: 0,
            chapter: 1,
            stage: 1,
            medals: 8,
            pusher,
            orb_gauge: 0,
            pending_level_pick: false,
            last_failed: false,
            sortie: None,
            log: Vec::new(),
            result_summary: String::new(),
            elapsed_ticks: 0,
            last_wall_ms: 0,
            rng,
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

    pub fn orb_remaining(&self) -> u32 {
        ORB_NEED.saturating_sub(self.orb_gauge.min(ORB_NEED))
    }

    pub fn next_rng(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = if x == 0 { 1 } else { x };
        self.rng
    }
}
