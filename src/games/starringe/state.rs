//! 星環 (Star Ring) の状態定義。
//!
//! 中心コアを囲む砲台が公転し、外周から迫る鉱石を砕いて星屑を稼ぐ放置ゲーム。

/// ワールド幅 (Canvas x_bounds)。
pub const WORLD_W: f64 = 60.0;
/// ワールド高さ (Canvas y_bounds)。
pub const WORLD_H: f64 = 80.0;
/// 中心 X。
pub const CX: f64 = WORLD_W * 0.5;
/// 中心 Y。
pub const CY: f64 = WORLD_H * 0.5;
/// コア漏洩判定半径。ここに達した鉱石は撃破せず漏洩する。
pub const INNER_RADIUS: f64 = 5.5;
/// 砲台の基準軌道半径。
pub const BASE_RING_R: f64 = 12.0;
/// 軌道の Y 方向潰し (立体感)。
pub const ORBIT_Y_SQUASH: f64 = 0.45;
/// 砲台数の上限。
pub const MAX_TURRETS: u32 = 8;
/// 鉱石の出現外半径。
pub const SPAWN_RADIUS: f64 = 36.0;
/// 手動タップの火力ブースト持続 (tick)。
pub const BOOST_DURATION: u32 = 40;

/// 強化種別。購入でレベルが上がり、コストは指数成長する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    /// 砲台数 (1起点、上限 MAX_TURRETS)
    Turrets = 0,
    /// 公転速度
    OrbitSpeed = 1,
    /// 火力 (ダメージ)
    Damage = 2,
    /// 連射 (発射間隔短縮)
    FireRate = 3,
    /// 鉱脈密度 (スポーン間隔短縮 / 同時数)
    Density = 4,
    /// 収率 (撃破時の星屑倍率)
    Yield = 5,
}

impl UpgradeKind {
    pub const ALL: [UpgradeKind; 6] = [
        UpgradeKind::Turrets,
        UpgradeKind::OrbitSpeed,
        UpgradeKind::Damage,
        UpgradeKind::FireRate,
        UpgradeKind::Density,
        UpgradeKind::Yield,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            UpgradeKind::Turrets => "砲台数",
            UpgradeKind::OrbitSpeed => "公転速度",
            UpgradeKind::Damage => "火力",
            UpgradeKind::FireRate => "連射",
            UpgradeKind::Density => "鉱脈密度",
            UpgradeKind::Yield => "収率",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            UpgradeKind::Turrets => "回る砲が増える",
            UpgradeKind::OrbitSpeed => "軌道が速く回る",
            UpgradeKind::Damage => "一撃が強くなる",
            UpgradeKind::FireRate => "撃ち合いが速くなる",
            UpgradeKind::Density => "鉱石が多く迫る",
            UpgradeKind::Yield => "砕いた星屑が増える",
        }
    }

    pub fn base_cost(self) -> f64 {
        match self {
            UpgradeKind::Turrets => 25.0,
            UpgradeKind::OrbitSpeed => 18.0,
            UpgradeKind::Damage => 22.0,
            UpgradeKind::FireRate => 28.0,
            UpgradeKind::Density => 35.0,
            UpgradeKind::Yield => 45.0,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            UpgradeKind::Turrets => 2.35,
            UpgradeKind::OrbitSpeed => 1.75,
            UpgradeKind::Damage => 1.80,
            UpgradeKind::FireRate => 1.85,
            UpgradeKind::Density => 1.90,
            UpgradeKind::Yield => 2.00,
        }
    }

    /// 砲台数のみ上限あり。それ以外は実質無制限。
    pub fn max_level(self) -> Option<u32> {
        match self {
            UpgradeKind::Turrets => Some(MAX_TURRETS - 1), // 1 + level
            _ => None,
        }
    }
}

/// 鉱石の種類。累計撃破で解放され、色・価値・HP が違う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OreKind {
    Dust = 0,
    Rock = 1,
    Crystal = 2,
    Prism = 3,
    Nova = 4,
}

impl OreKind {
    pub const ALL: [OreKind; 5] = [
        OreKind::Dust,
        OreKind::Rock,
        OreKind::Crystal,
        OreKind::Prism,
        OreKind::Nova,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OreKind::Dust => "星塵",
            OreKind::Rock => "岩石",
            OreKind::Crystal => "結晶",
            OreKind::Prism => "輝晶",
            OreKind::Nova => "新星核",
        }
    }

    pub fn base_value(self) -> f64 {
        match self {
            OreKind::Dust => 1.0,
            OreKind::Rock => 3.0,
            OreKind::Crystal => 8.0,
            OreKind::Prism => 20.0,
            OreKind::Nova => 55.0,
        }
    }

    pub fn base_hp(self) -> f64 {
        match self {
            OreKind::Dust => 1.0,
            OreKind::Rock => 2.5,
            OreKind::Crystal => 5.0,
            OreKind::Prism => 10.0,
            OreKind::Nova => 22.0,
        }
    }

    pub fn radius(self) -> f64 {
        match self {
            OreKind::Dust => 1.4,
            OreKind::Rock => 1.9,
            OreKind::Crystal => 2.3,
            OreKind::Prism => 2.8,
            OreKind::Nova => 3.4,
        }
    }

    pub fn speed(self) -> f64 {
        match self {
            OreKind::Dust => 0.55,
            OreKind::Rock => 0.48,
            OreKind::Crystal => 0.40,
            OreKind::Prism => 0.32,
            OreKind::Nova => 0.26,
        }
    }

    /// 累計撃破で解放される閾値。
    pub fn unlock_kills(self) -> u64 {
        match self {
            OreKind::Dust => 0,
            OreKind::Rock => 12,
            OreKind::Crystal => 60,
            OreKind::Prism => 180,
            OreKind::Nova => 450,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ore {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub hp: f64,
    pub kind: OreKind,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    Spark,
    Dust,
    Shard,
    Ember,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub life: u32,
    pub kind: ParticleKind,
}

#[derive(Clone, Debug)]
pub struct BeamFlash {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub life: u32,
}

/// UI タブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Upgrades,
    Codex,
}

#[derive(Clone, Debug)]
pub struct StarRingState {
    pub shards: f64,
    /// 撃破で得た累計星屑 (漏洩損失を差し引かない)。シミュレータ不変条件用。
    pub shards_earned: f64,
    /// 漏洩で失った累計。
    pub shards_leaked: f64,
    pub total_kills: u64,
    pub leak_count: u64,
    /// 各強化のレベル (UpgradeKind::ALL 順)。
    pub upgrade_levels: [u32; 6],
    pub ores: Vec<Ore>,
    pub beams: Vec<BeamFlash>,
    pub particles: Vec<Particle>,
    pub elapsed_ticks: u64,
    pub rng_state: u32,
    pub shake_ticks: u32,
    pub core_flash_ticks: u32,
    pub boost_ticks: u32,
    pub tab: Tab,
    /// 直近の星屑獲得量 (shards/sec 表示用、リングバッファ)。
    pub recent_gain: [f64; 20],
    pub recent_gain_idx: usize,
    /// 今 tick で得た星屑 (tick 末尾で recent_gain に積む)。
    pub tick_gain: f64,
}

impl StarRingState {
    pub fn new() -> Self {
        Self {
            shards: 8.0,
            shards_earned: 0.0,
            shards_leaked: 0.0,
            total_kills: 0,
            leak_count: 0,
            upgrade_levels: [0; 6],
            ores: Vec::new(),
            beams: Vec::new(),
            particles: Vec::new(),
            elapsed_ticks: 0,
            rng_state: 0xC0FFEE42,
            shake_ticks: 0,
            core_flash_ticks: 0,
            boost_ticks: 0,
            tab: Tab::Upgrades,
            recent_gain: [0.0; 20],
            recent_gain_idx: 0,
            tick_gain: 0.0,
        }
    }

    pub fn level(&self, kind: UpgradeKind) -> u32 {
        self.upgrade_levels[kind.index()]
    }

    pub fn turret_count(&self) -> u32 {
        (1 + self.level(UpgradeKind::Turrets)).min(MAX_TURRETS)
    }

    pub fn orbit_speed(&self) -> f64 {
        0.028 * (1.0 + 0.22 * self.level(UpgradeKind::OrbitSpeed) as f64)
    }

    pub fn ring_radius(&self) -> f64 {
        BASE_RING_R + self.level(UpgradeKind::Turrets) as f64 * 0.6
    }

    pub fn damage(&self) -> f64 {
        let base = 1.0 + self.level(UpgradeKind::Damage) as f64 * 0.85;
        if self.boost_ticks > 0 {
            base * 2.0
        } else {
            base
        }
    }

    pub fn fire_interval(&self) -> u64 {
        let lv = self.level(UpgradeKind::FireRate);
        (10u64.saturating_sub(lv as u64)).max(2)
    }

    pub fn spawn_interval(&self) -> u64 {
        let lv = self.level(UpgradeKind::Density);
        (16u64.saturating_sub(lv as u64)).max(3)
    }

    pub fn spawn_batch(&self) -> usize {
        1 + (self.level(UpgradeKind::Density) / 3) as usize
    }

    pub fn yield_mult(&self) -> f64 {
        1.0 + self.level(UpgradeKind::Yield) as f64 * 0.40
    }

    pub fn unlocked_ore_kinds(&self) -> Vec<OreKind> {
        OreKind::ALL
            .iter()
            .copied()
            .filter(|k| self.total_kills >= k.unlock_kills())
            .collect()
    }

    /// 10 ticks/sec 換算の星屑/秒。
    pub fn shards_per_sec(&self) -> f64 {
        let sum: f64 = self.recent_gain.iter().sum();
        // 20 スロット × 1tick を 10 ticks/sec で割る
        sum * (10.0 / 20.0)
    }
}

impl Default for StarRingState {
    fn default() -> Self {
        Self::new()
    }
}
