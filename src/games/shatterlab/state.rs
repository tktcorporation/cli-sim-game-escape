//! 破壊VFX比較ラボの状態。
//!
//! 「強化しがい」が見える舞台案を並べて見比べる試作。
//! 各スタイルは自動再生し、威力Lv (弱/中/強) で見た目の密度が変わる。

/// 比較対象のコンセプト。進行方向があり、強化で画面が育つもの。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoStyle {
    /// 宇宙クルーズで航路デブリを掃討しながら前進
    SpaceCruise,
    /// 軌道上の小惑星帯をレーザーで削る採掘船
    OrbitMine,
    /// 列車が障害を正面突破しながら進む
    RailBreak,
    /// 防衛衛星が降り注ぐ隕石を迎撃する
    SatDefense,
}

impl DemoStyle {
    pub const ALL: [DemoStyle; 4] = [
        DemoStyle::SpaceCruise,
        DemoStyle::OrbitMine,
        DemoStyle::RailBreak,
        DemoStyle::SatDefense,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DemoStyle::SpaceCruise => "宇宙クルーズ",
            DemoStyle::OrbitMine => "軌道採掘",
            DemoStyle::RailBreak => "列車突破",
            DemoStyle::SatDefense => "衛星防衛",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            DemoStyle::SpaceCruise => "前進しながらデブリを薙ぎ払う",
            DemoStyle::OrbitMine => "帯を削り、資源を吸い込む",
            DemoStyle::RailBreak => "一直線に障害を砕いて進む",
            DemoStyle::SatDefense => "降り注ぐ隕石を全遮断する",
        }
    }
}

/// 威力段階。強化しがいを一目で見せるための3段階。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerLevel {
    Low = 0,
    Mid = 1,
    High = 2,
}

impl PowerLevel {
    pub const ALL: [PowerLevel; 3] = [PowerLevel::Low, PowerLevel::Mid, PowerLevel::High];

    pub fn label(self) -> &'static str {
        match self {
            PowerLevel::Low => "弱",
            PowerLevel::Mid => "中",
            PowerLevel::High => "強",
        }
    }

    pub fn from_index(i: u8) -> Self {
        match i {
            0 => PowerLevel::Low,
            1 => PowerLevel::Mid,
            _ => PowerLevel::High,
        }
    }

    pub fn next(self) -> Self {
        Self::from_index((self as u8 + 1) % 3)
    }

    /// 砲門数 / 同時破壊数の目安
    pub fn gun_count(self) -> usize {
        match self {
            PowerLevel::Low => 1,
            PowerLevel::Mid => 3,
            PowerLevel::High => 6,
        }
    }

    pub fn spawn_interval(self) -> u32 {
        match self {
            PowerLevel::Low => 14,
            PowerLevel::Mid => 8,
            PowerLevel::High => 4,
        }
    }

    pub fn burst_count(self) -> usize {
        match self {
            PowerLevel::Low => 6,
            PowerLevel::Mid => 14,
            PowerLevel::High => 28,
        }
    }

    pub fn ship_scale(self) -> f64 {
        match self {
            PowerLevel::Low => 0.7,
            PowerLevel::Mid => 1.0,
            PowerLevel::High => 1.45,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    Debris,
    Spark,
    Dust,
    Ember,
    Shard,
    Beam,
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

impl Particle {
    pub fn alive(&self) -> bool {
        self.life > 0
    }
}

/// 流れてくる破壊対象。
#[derive(Clone, Debug)]
pub struct Target {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub radius: f64,
    pub hp: u8,
    /// 見た目バリエーション (強化で種類が増える)
    pub variety: u8,
}

#[derive(Clone, Debug)]
pub struct BeamFlash {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub life: u32,
}

pub const WORLD_W: f64 = 60.0;
pub const WORLD_H: f64 = 80.0;

/// 何tickごとに威力Lvを自動で上げて「強化しがい」を見せるか。
pub const AUTO_POWER_TICKS: u32 = 80;

#[derive(Clone, Debug)]
pub struct ShatterLabState {
    pub style: DemoStyle,
    pub power: PowerLevel,
    /// true のとき一定時間で弱→中→強を循環
    pub auto_power: bool,
    pub auto_power_t: u32,
    pub targets: Vec<Target>,
    pub beams: Vec<BeamFlash>,
    pub particles: Vec<Particle>,
    pub elapsed_ticks: u64,
    pub shake_ticks: u32,
    /// スクロール演出用 (前進感)
    pub scroll: f64,
    /// 撃破カウンタ (表示用)
    pub cleared: u32,
}

impl ShatterLabState {
    pub fn new() -> Self {
        Self {
            style: DemoStyle::SpaceCruise,
            power: PowerLevel::Low,
            auto_power: true,
            auto_power_t: 0,
            targets: Vec::new(),
            beams: Vec::new(),
            particles: Vec::new(),
            elapsed_ticks: 0,
            shake_ticks: 0,
            scroll: 0.0,
            cleared: 0,
        }
    }

    pub fn set_style(&mut self, style: DemoStyle) {
        if self.style == style {
            return;
        }
        self.style = style;
        self.reset_scene();
    }

    pub fn set_power(&mut self, power: PowerLevel) {
        self.power = power;
        self.auto_power = false;
        self.auto_power_t = 0;
    }

    pub fn enable_auto_power(&mut self) {
        self.auto_power = true;
        self.auto_power_t = 0;
        self.power = PowerLevel::Low;
        self.reset_scene();
    }

    pub fn reset_scene(&mut self) {
        self.targets.clear();
        self.beams.clear();
        self.particles.clear();
        self.shake_ticks = 0;
        self.scroll = 0.0;
        self.cleared = 0;
    }
}

impl Default for ShatterLabState {
    fn default() -> Self {
        Self::new()
    }
}
