//! 星環 (Star Ring) の状態定義。
//!
//! 中心コアを防衛し、外周を漂う鉱石を砲台・脈動・穿光で砕いて星屑を稼ぐ。
//! 脅威の増加はプレイヤー強化ではなく「層 (depth)」進行が担う。

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
/// 強化スロット数。
pub const UPGRADE_COUNT: usize = 6;

/// 強化種別。購入でレベルが上がり、コストは指数成長する。
/// 脅威 (出現数) を増やす強化は持たない — それは層進行の役割。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    /// 砲台数 (1起点、上限 MAX_TURRETS)
    Turrets = 0,
    /// 火力 (ダメージ)
    Damage = 1,
    /// 連射 (発射間隔短縮)
    FireRate = 2,
    /// 脈動 — コア周囲の周期 AOE (層2で解放)
    Pulse = 3,
    /// 穿光 — 貫通ビーム (層4で解放)
    Lance = 4,
    /// 収率 (撃破時の星屑倍率)
    Yield = 5,
}

impl UpgradeKind {
    pub const ALL: [UpgradeKind; UPGRADE_COUNT] = [
        UpgradeKind::Turrets,
        UpgradeKind::Damage,
        UpgradeKind::FireRate,
        UpgradeKind::Pulse,
        UpgradeKind::Lance,
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
            UpgradeKind::Damage => "火力",
            UpgradeKind::FireRate => "連射",
            UpgradeKind::Pulse => "脈動",
            UpgradeKind::Lance => "穿光",
            UpgradeKind::Yield => "収率",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            UpgradeKind::Turrets => "回る砲が増える",
            UpgradeKind::Damage => "一撃が強くなる",
            UpgradeKind::FireRate => "撃ち合いが速くなる",
            UpgradeKind::Pulse => "核が波打って周囲を削る",
            UpgradeKind::Lance => "貫通の光が並びを貫く",
            UpgradeKind::Yield => "砕いた星屑が増える",
        }
    }

    pub fn base_cost(self) -> f64 {
        match self {
            UpgradeKind::Turrets => 25.0,
            UpgradeKind::Damage => 22.0,
            UpgradeKind::FireRate => 28.0,
            UpgradeKind::Pulse => 32.0,
            UpgradeKind::Lance => 48.0,
            UpgradeKind::Yield => 50.0,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            UpgradeKind::Turrets => 2.35,
            UpgradeKind::Damage => 1.80,
            UpgradeKind::FireRate => 1.85,
            UpgradeKind::Pulse => 1.85,
            UpgradeKind::Lance => 1.90,
            UpgradeKind::Yield => 2.10,
        }
    }

    /// 砲台数のみ上限あり。それ以外は実質無制限。
    pub fn max_level(self) -> Option<u32> {
        match self {
            UpgradeKind::Turrets => Some(MAX_TURRETS - 1), // 1 + level
            _ => None,
        }
    }

    /// この強化が店に並ぶ最低層。
    pub fn unlock_depth(self) -> u32 {
        match self {
            UpgradeKind::Turrets | UpgradeKind::Damage | UpgradeKind::FireRate | UpgradeKind::Yield => {
                1
            }
            UpgradeKind::Pulse => 2,
            UpgradeKind::Lance => 4,
        }
    }
}

/// 鉱石の種類。層の進行で解放され、挙動・価値・HP が違う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OreKind {
    Dust = 0,
    Rock = 1,
    Crystal = 2,
    Wisp = 3,
    Prism = 4,
    Shell = 5,
    Splitter = 6,
    Nova = 7,
}

impl OreKind {
    pub const ALL: [OreKind; 8] = [
        OreKind::Dust,
        OreKind::Rock,
        OreKind::Crystal,
        OreKind::Wisp,
        OreKind::Prism,
        OreKind::Shell,
        OreKind::Splitter,
        OreKind::Nova,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OreKind::Dust => "星塵",
            OreKind::Rock => "岩石",
            OreKind::Crystal => "結晶",
            OreKind::Wisp => "浮遊片",
            OreKind::Prism => "輝晶",
            OreKind::Shell => "殻石",
            OreKind::Splitter => "裂片",
            OreKind::Nova => "新星核",
        }
    }

    pub fn base_value(self) -> f64 {
        match self {
            OreKind::Dust => 1.0,
            OreKind::Rock => 3.0,
            OreKind::Crystal => 8.0,
            OreKind::Wisp => 6.0,
            OreKind::Prism => 20.0,
            OreKind::Shell => 28.0,
            OreKind::Splitter => 14.0,
            OreKind::Nova => 55.0,
        }
    }

    pub fn base_hp(self) -> f64 {
        match self {
            OreKind::Dust => 1.0,
            OreKind::Rock => 2.5,
            OreKind::Crystal => 5.0,
            OreKind::Wisp => 3.5,
            OreKind::Prism => 10.0,
            OreKind::Shell => 18.0,
            OreKind::Splitter => 7.0,
            OreKind::Nova => 22.0,
        }
    }

    pub fn radius(self) -> f64 {
        match self {
            OreKind::Dust => 1.4,
            OreKind::Rock => 1.9,
            OreKind::Crystal => 2.3,
            OreKind::Wisp => 1.7,
            OreKind::Prism => 2.8,
            OreKind::Shell => 3.0,
            OreKind::Splitter => 2.4,
            OreKind::Nova => 3.4,
        }
    }

    /// 軌道上の角速度 (rad/tick)。符号はスポーン時に決める。
    pub fn ang_speed(self) -> f64 {
        match self {
            OreKind::Dust => 0.035,
            OreKind::Rock => 0.028,
            OreKind::Crystal => 0.022,
            OreKind::Wisp => 0.040,
            OreKind::Prism => 0.030,
            OreKind::Shell => 0.016,
            OreKind::Splitter => 0.026,
            OreKind::Nova => 0.014,
        }
    }

    /// 内側へ沈む速度 (負 = 中心方向)。一直線突進ではなくゆるい螺旋。
    pub fn radial_speed(self) -> f64 {
        match self {
            OreKind::Dust => -0.18,
            OreKind::Rock => -0.14,
            OreKind::Crystal => -0.10,
            OreKind::Wisp => -0.035, // ほぼ周回
            OreKind::Prism => -0.12,
            OreKind::Shell => -0.07,
            OreKind::Splitter => -0.11,
            OreKind::Nova => -0.06,
        }
    }

    /// 登場する最低層。
    pub fn unlock_depth(self) -> u32 {
        match self {
            OreKind::Dust | OreKind::Rock => 1,
            OreKind::Crystal => 2,
            OreKind::Wisp => 3,
            OreKind::Prism => 4,
            OreKind::Shell => 5,
            OreKind::Splitter => 6,
            OreKind::Nova => 8,
        }
    }

    /// 殻石は通常弾に耐性を持つ (穿光・脈動は通しやすい)。
    pub fn armored(self) -> bool {
        matches!(self, OreKind::Shell)
    }

    pub fn splits_on_death(self) -> bool {
        matches!(self, OreKind::Splitter)
    }

    pub fn default_motion(self) -> OreMotion {
        match self {
            OreKind::Wisp => OreMotion::Orbit,
            OreKind::Prism => OreMotion::Zigzag,
            OreKind::Shell | OreKind::Nova => OreMotion::Heavy,
            _ => OreMotion::Spiral,
        }
    }
}

/// 鉱石の軌道パターン。どれも「外周を漂いながらゆっくり沈む」系で、
/// 中心への一直線ミサイルにはしない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OreMotion {
    /// 螺旋漂流 (基本)
    Spiral,
    /// ほぼ周回、沈みはごく遅い
    Orbit,
    /// 螺旋 + 半径の呼吸
    Zigzag,
    /// 重く遅い螺旋
    Heavy,
}

#[derive(Clone, Debug)]
pub struct Ore {
    pub x: f64,
    pub y: f64,
    /// 描画用の直前変位 (軌跡)。
    pub vx: f64,
    pub vy: f64,
    pub hp: f64,
    pub kind: OreKind,
    pub radius: f64,
    pub motion: OreMotion,
    /// 角速度 (符号付き)。
    pub ang_vel: f64,
    pub age: u32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeamKind {
    Laser,
    Lance,
}

#[derive(Clone, Debug)]
pub struct BeamFlash {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub life: u32,
    pub kind: BeamKind,
}

/// 脈動の波紋演出。
#[derive(Clone, Debug)]
pub struct PulseRing {
    pub radius: f64,
    pub life: u32,
    pub max_life: u32,
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
    /// 現在の採掘層。深いほど敵が強く、新鉱石・新武装が開く。
    pub depth: u32,
    /// 今の層で稼いだ撃破数 (次層への進捗)。
    pub depth_kills: u64,
    /// 到達した最深層。
    pub best_depth: u32,
    /// 各強化のレベル (UpgradeKind::ALL 順)。
    pub upgrade_levels: [u32; UPGRADE_COUNT],
    pub ores: Vec<Ore>,
    pub beams: Vec<BeamFlash>,
    pub particles: Vec<Particle>,
    pub pulse_rings: Vec<PulseRing>,
    pub elapsed_ticks: u64,
    pub rng_state: u32,
    pub shake_ticks: u32,
    pub core_flash_ticks: u32,
    pub boost_ticks: u32,
    /// 層到達時の演出残り。
    pub depth_flash_ticks: u32,
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
            depth: 1,
            depth_kills: 0,
            best_depth: 1,
            upgrade_levels: [0; UPGRADE_COUNT],
            ores: Vec::new(),
            beams: Vec::new(),
            particles: Vec::new(),
            pulse_rings: Vec::new(),
            elapsed_ticks: 0,
            rng_state: 0xC0FFEE42,
            shake_ticks: 0,
            core_flash_ticks: 0,
            boost_ticks: 0,
            depth_flash_ticks: 0,
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

    /// 公転は砲台数に連動する見た目・配置用。単独強化にはしない。
    pub fn orbit_speed(&self) -> f64 {
        0.028 * (1.0 + 0.10 * (self.turret_count().saturating_sub(1)) as f64)
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

    /// 出現間隔は層が決める (購入では増やさない)。
    pub fn spawn_interval(&self) -> u64 {
        let d = self.depth.saturating_sub(1) as u64;
        (14u64.saturating_sub(d / 2)).max(4)
    }

    pub fn spawn_batch(&self) -> usize {
        1 + ((self.depth.saturating_sub(1)) / 3) as usize
    }

    pub fn yield_mult(&self) -> f64 {
        1.0 + self.level(UpgradeKind::Yield) as f64 * 0.40
    }

    /// 層に応じた敵 HP 倍率。深い層ほど武装の価値が立つよう加速する。
    pub fn depth_hp_mult(&self) -> f64 {
        let d = self.depth.saturating_sub(1) as f64;
        1.0 + d * 0.16 + (d * d) * 0.012
    }

    /// 脈動の発動間隔。
    pub fn pulse_interval(&self) -> Option<u64> {
        let lv = self.level(UpgradeKind::Pulse);
        if lv == 0 {
            return None;
        }
        Some((24u64.saturating_sub(lv as u64 * 2)).max(8))
    }

    pub fn pulse_radius(&self) -> f64 {
        let lv = self.level(UpgradeKind::Pulse).max(1);
        8.5 + lv as f64 * 1.35
    }

    pub fn pulse_damage(&self) -> f64 {
        let lv = self.level(UpgradeKind::Pulse).max(1) as f64;
        (1.3 + lv * 0.95) * if self.boost_ticks > 0 { 1.5 } else { 1.0 }
    }

    /// 穿光を撃つ周期 (砲台斉射 N 回に1回相当 = tick 倍数)。
    pub fn lance_interval(&self) -> Option<u64> {
        let lv = self.level(UpgradeKind::Lance);
        if lv == 0 {
            return None;
        }
        Some((30u64.saturating_sub(lv as u64 * 3)).max(10))
    }

    pub fn lance_damage(&self) -> f64 {
        self.damage() * (1.55 + self.level(UpgradeKind::Lance) as f64 * 0.40)
    }

    pub fn unlocked_ore_kinds(&self) -> Vec<OreKind> {
        OreKind::ALL
            .iter()
            .copied()
            .filter(|k| self.depth >= k.unlock_depth())
            .collect()
    }

    pub fn upgrade_unlocked(&self, kind: UpgradeKind) -> bool {
        self.depth >= kind.unlock_depth()
    }

    /// 次の層までに必要な撃破数。
    pub fn kills_to_next_depth(&self) -> u64 {
        45 + (self.depth.saturating_sub(1) as u64) * 35
    }

    /// 10 ticks/sec 換算の星屑/秒。
    pub fn shards_per_sec(&self) -> f64 {
        let sum: f64 = self.recent_gain.iter().sum();
        sum * (10.0 / 20.0)
    }
}

impl Default for StarRingState {
    fn default() -> Self {
        Self::new()
    }
}
