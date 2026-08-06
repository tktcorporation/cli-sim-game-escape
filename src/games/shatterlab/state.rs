//! 破壊VFX比較ラボの状態。
//!
//! 本番ゲームではなく、破壊表現の手触りを並べて見比べるための試作。
//! 各スタイルは自動でループ再生し、タブ切替で即座に差し替える。

/// 比較対象の破壊スタイル。ストア系の既存表現を TUI+Braille に寄せた試作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoStyle {
    /// 爆弾で鉱石を吹き飛ばす（放置採掘クリッカー系）
    OreBomb,
    /// 油圧プレスで押し潰す（Crusher / Rock Crusher 系）
    PressCrush,
    /// 惑星を層ごと剥がす（Planet Buster 系）
    PlanetPeel,
    /// 建物が階層遅延で崩落する（解体・崩壊系）
    CityCollapse,
}

impl DemoStyle {
    pub const ALL: [DemoStyle; 4] = [
        DemoStyle::OreBomb,
        DemoStyle::PressCrush,
        DemoStyle::PlanetPeel,
        DemoStyle::CityCollapse,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DemoStyle::OreBomb => "鉱石爆砕",
            DemoStyle::PressCrush => "油圧粉砕",
            DemoStyle::PlanetPeel => "惑星剥離",
            DemoStyle::CityCollapse => "都市崩落",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            DemoStyle::OreBomb => "爆弾→爆風リング→破片飛び散り",
            DemoStyle::PressCrush => "プレス下降→押し潰し→横噴射",
            DemoStyle::PlanetPeel => "地殻→マントル→核→大爆発",
            DemoStyle::CityCollapse => "上から階が落ち、粉塵が舞う",
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

/// スタイル固有のシーン進行。tick で進み、終わるとループ。
#[derive(Clone, Debug)]
pub enum Scene {
    OreBomb {
        /// 0=待機, 1=爆弾落下, 2=爆発中
        phase: u8,
        phase_t: u32,
        bomb_y: f64,
        rock_hp_frac: f64,
    },
    PressCrush {
        phase: u8,
        phase_t: u32,
        press_y: f64,
        rock_squash: f64,
    },
    PlanetPeel {
        /// 残っている層数 (3=地殻, 2=マントル, 1=核, 0=爆発後)
        layers_left: u8,
        phase_t: u32,
        crack: f64,
    },
    CityCollapse {
        /// まだ立っている階数 (下から数える残存)
        floors_left: u8,
        phase_t: u32,
        falling_y: f64,
    },
}

pub const WORLD_W: f64 = 60.0;
pub const WORLD_H: f64 = 80.0;

#[derive(Clone, Debug)]
pub struct ShatterLabState {
    pub style: DemoStyle,
    pub scene: Scene,
    pub particles: Vec<Particle>,
    pub elapsed_ticks: u64,
    /// 画面を揺らす残り tick（衝撃演出）
    pub shake_ticks: u32,
}

impl ShatterLabState {
    pub fn new() -> Self {
        let style = DemoStyle::OreBomb;
        Self {
            style,
            scene: Self::fresh_scene(style),
            particles: Vec::new(),
            elapsed_ticks: 0,
            shake_ticks: 0,
        }
    }

    pub fn set_style(&mut self, style: DemoStyle) {
        if self.style == style {
            return;
        }
        self.style = style;
        self.scene = Self::fresh_scene(style);
        self.particles.clear();
        self.shake_ticks = 0;
    }

    pub fn fresh_scene(style: DemoStyle) -> Scene {
        match style {
            DemoStyle::OreBomb => Scene::OreBomb {
                phase: 0,
                phase_t: 0,
                bomb_y: WORLD_H - 8.0,
                rock_hp_frac: 1.0,
            },
            DemoStyle::PressCrush => Scene::PressCrush {
                phase: 0,
                phase_t: 0,
                press_y: WORLD_H - 6.0,
                rock_squash: 0.0,
            },
            DemoStyle::PlanetPeel => Scene::PlanetPeel {
                layers_left: 3,
                phase_t: 0,
                crack: 0.0,
            },
            DemoStyle::CityCollapse => Scene::CityCollapse {
                floors_left: 6,
                phase_t: 0,
                falling_y: 0.0,
            },
        }
    }
}

impl Default for ShatterLabState {
    fn default() -> Self {
        Self::new()
    }
}
