//! 星環 (Star Ring) の状態定義。
//!
//! 外周から流れてくる鉱石を、公転する武装の連射で砕いて星屑を稼ぐ放置ゲーム。
//! 「守る」ではなく「刈り取る」——中心の星は採掘の核であり、防衛対象ではない。

/// ワールド幅 (Canvas x_bounds)。
pub const WORLD_W: f64 = 60.0;
/// ワールド高さ (Canvas y_bounds)。
pub const WORLD_H: f64 = 80.0;
/// 中心 X。
pub const CX: f64 = WORLD_W * 0.5;
/// 中心 Y。
pub const CY: f64 = WORLD_H * 0.5;
/// 中心到達半径。ここに達した鉱石は逸失 (報酬なし・ペナルティなし) で消える。
pub const INNER_RADIUS: f64 = 5.5;
/// 砲台の基準軌道半径。
pub const BASE_RING_R: f64 = 12.0;
/// 軌道の Y 方向潰し (立体感)。
pub const ORBIT_Y_SQUASH: f64 = 0.45;
/// 砲台スロット上限。
pub const MAX_TURRETS: u32 = 8;
/// 鉱石の出現外半径。
pub const SPAWN_RADIUS: f64 = 36.0;
/// 手動タップの火力ブースト持続 (tick)。
pub const BOOST_DURATION: u32 = 40;
/// 武器種数。
pub const WEAPON_COUNT: usize = 5;
/// 層到達フラッシュの長さ (tick)。
pub const LAYER_FLASH_TICKS: u32 = 28;

/// 武装の種類。層進行で解放され、解放後は個別に強化できる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponKind {
    /// 流星 — 弱いが連射。初期武装。
    Pulse = 0,
    /// 光線 — 貫通する一筋。第2層〜。
    Ray = 1,
    /// 散弾 — 扇状に広がる弾幕。第3層〜。
    Scatter = 2,
    /// 環弾 — 軌道に沿って曲がる弾。第4層〜。
    Arc = 3,
    /// 新星 — 着弾で小爆発。第5層〜。
    Nova = 4,
}

impl WeaponKind {
    pub const ALL: [WeaponKind; WEAPON_COUNT] = [
        WeaponKind::Pulse,
        WeaponKind::Ray,
        WeaponKind::Scatter,
        WeaponKind::Arc,
        WeaponKind::Nova,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            WeaponKind::Pulse => "流星",
            WeaponKind::Ray => "光線",
            WeaponKind::Scatter => "散弾",
            WeaponKind::Arc => "環弾",
            WeaponKind::Nova => "新星",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            WeaponKind::Pulse => "·›",
            WeaponKind::Ray => "═▷",
            WeaponKind::Scatter => "※",
            WeaponKind::Arc => "☾",
            WeaponKind::Nova => "✸",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            WeaponKind::Pulse => "弱く速い連射。星屑を削るように撃つ",
            WeaponKind::Ray => "貫通しやすい長い一筋。硬い核向き",
            WeaponKind::Scatter => "扇に広がる弾幕。群れを薙ぐ",
            WeaponKind::Arc => "軌道をなぞる曲射。側面から噛む",
            WeaponKind::Nova => "着弾で小さく弾け、周囲を巻き込む",
        }
    }

    /// 解放に必要な層 (1起点)。
    pub fn unlock_layer(self) -> u32 {
        match self {
            WeaponKind::Pulse => 1,
            WeaponKind::Ray => 2,
            WeaponKind::Scatter => 3,
            WeaponKind::Arc => 4,
            WeaponKind::Nova => 5,
        }
    }

}

/// 武器ごとの強化項目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponStat {
    /// 弾数 / 同時発射数
    Count = 0,
    /// 連射 (発射間隔短縮)
    Rate = 1,
    /// 威力
    Power = 2,
}

impl WeaponStat {
    pub const ALL: [WeaponStat; 3] = [WeaponStat::Count, WeaponStat::Rate, WeaponStat::Power];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            WeaponStat::Count => "弾数",
            WeaponStat::Rate => "連射",
            WeaponStat::Power => "威力",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            WeaponStat::Count => "1回の斉射で出る弾が増える",
            WeaponStat::Rate => "撃ち出しの間隔が短くなる",
            WeaponStat::Power => "1発あたりのダメージが上がる",
        }
    }

    pub fn base_cost(self) -> f64 {
        match self {
            WeaponStat::Count => 30.0,
            WeaponStat::Rate => 22.0,
            WeaponStat::Power => 26.0,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            WeaponStat::Count => 2.10,
            WeaponStat::Rate => 1.80,
            WeaponStat::Power => 1.85,
        }
    }

    pub fn max_level(self) -> Option<u32> {
        match self {
            WeaponStat::Count => Some(7),
            WeaponStat::Rate => Some(12),
            WeaponStat::Power => None,
        }
    }
}

/// 環全体の強化 (武装とは別タブ)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingUpgrade {
    /// 公転速度
    OrbitSpeed = 0,
    /// 収率 (撃破時の星屑倍率)
    Yield = 1,
}

impl RingUpgrade {
    pub const ALL: [RingUpgrade; 2] = [RingUpgrade::OrbitSpeed, RingUpgrade::Yield];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            RingUpgrade::OrbitSpeed => "公転速度",
            RingUpgrade::Yield => "収率",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            RingUpgrade::OrbitSpeed => "武装の軌道が速く回る",
            RingUpgrade::Yield => "砕いた星屑が増える",
        }
    }

    pub fn base_cost(self) -> f64 {
        match self {
            RingUpgrade::OrbitSpeed => 18.0,
            RingUpgrade::Yield => 40.0,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            RingUpgrade::OrbitSpeed => 1.75,
            RingUpgrade::Yield => 2.00,
        }
    }
}

/// 採掘の層。累計撃破で進み、敵構成・報酬・武装解放が段で切り替わる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer;

impl Layer {
    /// 各層の開始に必要な累計撃破 (index 0 = 第1層)。
    /// ぬるっと上がらず、到達ごとに場面が切り替わる間隔を取る。
    pub const THRESHOLDS: [u64; 8] = [0, 80, 250, 600, 1400, 3000, 6000, 12000];

    pub fn from_kills(kills: u64) -> u32 {
        let mut layer = 1u32;
        for (i, &th) in Self::THRESHOLDS.iter().enumerate() {
            if kills >= th {
                layer = (i as u32) + 1;
            }
        }
        // 最終閾値以降はゆるやかに延長
        if kills >= *Self::THRESHOLDS.last().unwrap() {
            let extra = kills - Self::THRESHOLDS.last().unwrap();
            layer += (extra / 8000) as u32;
        }
        layer
    }

    pub fn next_threshold(layer: u32) -> Option<u64> {
        let idx = layer as usize; // 次の層の index
        if idx < Self::THRESHOLDS.len() {
            Some(Self::THRESHOLDS[idx])
        } else {
            let base = *Self::THRESHOLDS.last().unwrap();
            let past = layer - Self::THRESHOLDS.len() as u32;
            Some(base + (past as u64 + 1) * 8000)
        }
    }

    pub fn title(layer: u32) -> &'static str {
        match layer {
            1 => "外縁の砂",
            2 => "岩石帯",
            3 => "結晶海",
            4 => "輝晶嵐",
            5 => "新星域",
            6 => "深層流",
            7 => "暗黒潮",
            _ => "無限輪",
        }
    }

    pub fn spawn_interval_ticks(layer: u32) -> u64 {
        let l = layer.saturating_sub(1) as u64;
        (14u64.saturating_sub(l)).max(4)
    }

    pub fn spawn_batch(layer: u32) -> usize {
        1 + ((layer.saturating_sub(1) / 2) as usize).min(5)
    }

    pub fn hp_mult(layer: u32) -> f64 {
        1.0 + (layer.saturating_sub(1) as f64) * 0.40
    }

    pub fn value_mult(layer: u32) -> f64 {
        1.0 + (layer.saturating_sub(1) as f64) * 0.50
    }

    pub fn speed_mult(layer: u32) -> f64 {
        1.0 + (layer.saturating_sub(1) as f64) * 0.06
    }
}

/// 鉱石の種類。層に応じて出現テーブルへ合流する。
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
            OreKind::Rock => 2.8,
            OreKind::Crystal => 5.5,
            OreKind::Prism => 11.0,
            OreKind::Nova => 24.0,
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

    /// 出現に必要な層。
    pub fn unlock_layer(self) -> u32 {
        match self {
            OreKind::Dust => 1,
            OreKind::Rock => 2,
            OreKind::Crystal => 3,
            OreKind::Prism => 4,
            OreKind::Nova => 5,
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

/// 飛翔弾。武装から飛び、鉱石に当たって消える (または貫通する)。
#[derive(Clone, Debug)]
pub struct Projectile {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub damage: f64,
    pub life: u32,
    pub radius: f64,
    pub pierce: u8,
    pub splash: f64,
    pub kind: WeaponKind,
    /// 環弾用: 角速度
    pub spin: f64,
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

/// UI タブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// 武装一覧・個別強化
    Armory,
    /// 環全体の強化
    Ring,
    /// 鉱石図鑑
    Codex,
}

#[derive(Clone, Debug)]
pub struct StarRingState {
    pub shards: f64,
    /// 撃破で得た累計星屑。シミュレータ不変条件用。
    pub shards_earned: f64,
    pub total_kills: u64,
    /// 中心まで達して逸失した数 (ペナルティなし、統計のみ)。
    pub missed_count: u64,
    /// 武器ごとの [Count, Rate, Power] レベル。
    pub weapon_levels: [[u32; 3]; WEAPON_COUNT],
    /// 環強化レベル [OrbitSpeed, Yield]。
    pub ring_levels: [u32; 2],
    pub ores: Vec<Ore>,
    pub projectiles: Vec<Projectile>,
    pub particles: Vec<Particle>,
    pub elapsed_ticks: u64,
    pub rng_state: u32,
    pub shake_ticks: u32,
    pub core_flash_ticks: u32,
    pub boost_ticks: u32,
    /// 層到達時の演出残り。
    pub layer_flash_ticks: u32,
    /// 前回 tick 時点の層 (進行検知用)。
    pub last_layer: u32,
    pub tab: Tab,
    /// 武装タブで選択中の武器。
    pub selected_weapon: WeaponKind,
    /// 直近の星屑獲得量 (shards/sec 表示用、リングバッファ)。
    pub recent_gain: [f64; 20],
    pub recent_gain_idx: usize,
    /// 今 tick で得た星屑 (tick 末尾で recent_gain に積む)。
    pub tick_gain: f64,
}

impl StarRingState {
    pub fn new() -> Self {
        Self {
            shards: 12.0,
            shards_earned: 0.0,
            total_kills: 0,
            missed_count: 0,
            weapon_levels: [[0; 3]; WEAPON_COUNT],
            ring_levels: [0; 2],
            ores: Vec::new(),
            projectiles: Vec::new(),
            particles: Vec::new(),
            elapsed_ticks: 0,
            rng_state: 0xC0FFEE42,
            shake_ticks: 0,
            core_flash_ticks: 0,
            boost_ticks: 0,
            layer_flash_ticks: 0,
            last_layer: 1,
            tab: Tab::Armory,
            selected_weapon: WeaponKind::Pulse,
            recent_gain: [0.0; 20],
            recent_gain_idx: 0,
            tick_gain: 0.0,
        }
    }

    pub fn layer(&self) -> u32 {
        Layer::from_kills(self.total_kills)
    }

    pub fn weapon_stat(&self, weapon: WeaponKind, stat: WeaponStat) -> u32 {
        self.weapon_levels[weapon.index()][stat.index()]
    }

    pub fn ring_level(&self, kind: RingUpgrade) -> u32 {
        self.ring_levels[kind.index()]
    }

    pub fn is_weapon_unlocked(&self, weapon: WeaponKind) -> bool {
        self.layer() >= weapon.unlock_layer()
    }

    pub fn unlocked_weapons(&self) -> Vec<WeaponKind> {
        WeaponKind::ALL
            .iter()
            .copied()
            .filter(|w| self.is_weapon_unlocked(*w))
            .collect()
    }

    /// 軌道上の砲台数。解放済み武器の弾数強化から決める。
    pub fn turret_count(&self) -> u32 {
        let mut max_count = 1u32;
        for w in self.unlocked_weapons() {
            let c = 1 + self.weapon_stat(w, WeaponStat::Count);
            max_count = max_count.max(c.min(MAX_TURRETS));
        }
        max_count.min(MAX_TURRETS)
    }

    pub fn orbit_speed(&self) -> f64 {
        0.028 * (1.0 + 0.22 * self.ring_level(RingUpgrade::OrbitSpeed) as f64)
    }

    pub fn ring_radius(&self) -> f64 {
        BASE_RING_R + self.turret_count() as f64 * 0.55
    }

    pub fn yield_mult(&self) -> f64 {
        (1.0 + self.ring_level(RingUpgrade::Yield) as f64 * 0.40) * Layer::value_mult(self.layer())
    }

    /// 武器の1発ダメージ (ブースト込み)。
    pub fn weapon_damage(&self, weapon: WeaponKind) -> f64 {
        let power = self.weapon_stat(weapon, WeaponStat::Power) as f64;
        let base = match weapon {
            WeaponKind::Pulse => 0.35 + power * 0.18,
            WeaponKind::Ray => 1.10 + power * 0.55,
            WeaponKind::Scatter => 0.45 + power * 0.22,
            WeaponKind::Arc => 0.70 + power * 0.35,
            WeaponKind::Nova => 1.60 + power * 0.70,
        };
        if self.boost_ticks > 0 {
            base * 1.85
        } else {
            base
        }
    }

    /// 発射間隔 (tick)。小さいほど連射。
    pub fn fire_interval(&self, weapon: WeaponKind) -> u64 {
        let rate = self.weapon_stat(weapon, WeaponStat::Rate) as u64;
        let base = match weapon {
            WeaponKind::Pulse => 4u64,
            WeaponKind::Ray => 14u64,
            WeaponKind::Scatter => 10u64,
            WeaponKind::Arc => 9u64,
            WeaponKind::Nova => 18u64,
        };
        base.saturating_sub(rate).max(2)
    }

    /// 1斉射あたりの弾数。
    pub fn volley_count(&self, weapon: WeaponKind) -> usize {
        let c = self.weapon_stat(weapon, WeaponStat::Count) as usize;
        match weapon {
            WeaponKind::Pulse => 1 + c,
            WeaponKind::Ray => 1 + c / 2,
            WeaponKind::Scatter => 3 + c,
            WeaponKind::Arc => 1 + c,
            WeaponKind::Nova => 1 + c / 2,
        }
    }

    pub fn unlocked_ore_kinds(&self) -> Vec<OreKind> {
        let layer = self.layer();
        OreKind::ALL
            .iter()
            .copied()
            .filter(|k| layer >= k.unlock_layer())
            .collect()
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
