/// Cookie Factory game state definitions.

/// Kinds of producers (auto-clickers).
#[derive(Clone, Debug, PartialEq)]
pub enum ProducerKind {
    Cursor,
    Grandma,
    Farm,
    Mine,
    Factory,
}

impl ProducerKind {
    /// All producer kinds in display order.
    pub fn all() -> &'static [ProducerKind] {
        &[
            ProducerKind::Cursor,
            ProducerKind::Grandma,
            ProducerKind::Farm,
            ProducerKind::Mine,
            ProducerKind::Factory,
        ]
    }

    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            ProducerKind::Cursor => "Cursor",
            ProducerKind::Grandma => "Grandma",
            ProducerKind::Farm => "Farm",
            ProducerKind::Mine => "Mine",
            ProducerKind::Factory => "Factory",
        }
    }

    /// Base cost to buy the first one.
    pub fn base_cost(&self) -> f64 {
        match self {
            ProducerKind::Cursor => 15.0,
            ProducerKind::Grandma => 100.0,
            ProducerKind::Farm => 1_100.0,
            ProducerKind::Mine => 12_000.0,
            ProducerKind::Factory => 130_000.0,
        }
    }

    /// Base cookies per second per unit.
    pub fn base_rate(&self) -> f64 {
        match self {
            ProducerKind::Cursor => 0.1,
            ProducerKind::Grandma => 1.0,
            ProducerKind::Farm => 8.0,
            ProducerKind::Mine => 47.0,
            ProducerKind::Factory => 260.0,
        }
    }

    /// Key to buy (1-5 mapped to producer index).
    pub fn key(&self) -> char {
        match self {
            ProducerKind::Cursor => '1',
            ProducerKind::Grandma => '2',
            ProducerKind::Farm => '3',
            ProducerKind::Mine => '4',
            ProducerKind::Factory => '5',
        }
    }
}

/// A single type of producer.
#[derive(Clone, Debug)]
pub struct Producer {
    pub kind: ProducerKind,
    pub count: u32,
    /// Multiplier from upgrades (default 1.0).
    pub multiplier: f64,
}

impl Producer {
    pub fn new(kind: ProducerKind) -> Self {
        Self {
            kind,
            count: 0,
            multiplier: 1.0,
        }
    }

    /// Current cost to buy the next one.
    pub fn cost(&self) -> f64 {
        self.kind.base_cost() * 1.15_f64.powi(self.count as i32)
    }

    /// Cookies per second from this producer type.
    pub fn cps(&self) -> f64 {
        self.count as f64 * self.kind.base_rate() * self.multiplier
    }

    /// CPS gained by buying the next unit.
    pub fn next_unit_cps(&self) -> f64 {
        self.kind.base_rate() * self.multiplier
    }

    /// Payback time in seconds: how long until the next unit pays for itself.
    /// Returns None if next_unit_cps is zero (shouldn't happen normally).
    pub fn payback_seconds(&self) -> Option<f64> {
        let cps = self.next_unit_cps();
        if cps > 0.0 {
            Some(self.cost() / cps)
        } else {
            None
        }
    }
}

/// An available upgrade.
#[derive(Clone, Debug)]
pub struct Upgrade {
    pub name: String,
    pub description: String,
    pub cost: f64,
    /// Which producer kind this upgrade affects.
    pub target: ProducerKind,
    /// Multiplier to apply (e.g. 2.0 = double rate).
    pub multiplier: f64,
    pub purchased: bool,
}

/// A floating text particle (e.g. "+1" rising from click area).
#[derive(Clone, Debug)]
pub struct Particle {
    /// Text to display.
    pub text: String,
    /// Column offset from the center of the cookie display.
    pub col_offset: i16,
    /// Remaining lifetime in ticks (starts high, counts down).
    pub life: u32,
    /// Maximum lifetime (for computing vertical position).
    pub max_life: u32,
}

/// Log entry for the Cookie game.
#[derive(Clone, Debug)]
pub struct CookieLogEntry {
    pub text: String,
    pub is_important: bool,
}

/// Full state of a Cookie Factory game.
pub struct CookieState {
    /// Total cookies accumulated.
    pub cookies: f64,
    /// Total cookies earned all-time (for stats).
    pub cookies_all_time: f64,
    /// Manual clicks count.
    pub total_clicks: u64,
    /// Cookies per click (base 1.0).
    pub cookies_per_click: f64,
    /// Producers.
    pub producers: Vec<Producer>,
    /// Available upgrades.
    pub upgrades: Vec<Upgrade>,
    /// Message log.
    pub log: Vec<CookieLogEntry>,
    /// Whether showing upgrades panel.
    pub show_upgrades: bool,
    /// Animation frame counter (incremented every tick).
    pub anim_frame: u32,
    /// Recent click flash timer (ticks remaining for visual feedback).
    pub click_flash: u32,
    /// Purchase celebration flash timer.
    pub purchase_flash: u32,
    /// Active floating particles.
    pub particles: Vec<Particle>,
    /// Simple RNG state for particle spread.
    pub rng_state: u32,
}

impl CookieState {
    pub fn new() -> Self {
        let producers = ProducerKind::all()
            .iter()
            .map(|k| Producer::new(k.clone()))
            .collect();

        let upgrades = vec![
            Upgrade {
                name: "強化クリック".into(),
                description: "クリック +1".into(),
                cost: 100.0,
                target: ProducerKind::Cursor,
                multiplier: 1.0, // special: adds to click power
                purchased: false,
            },
            Upgrade {
                name: "Cursor x2".into(),
                description: "Cursor の生産 2倍".into(),
                cost: 200.0,
                target: ProducerKind::Cursor,
                multiplier: 2.0,
                purchased: false,
            },
            Upgrade {
                name: "Grandma x2".into(),
                description: "Grandma の生産 2倍".into(),
                cost: 1_000.0,
                target: ProducerKind::Grandma,
                multiplier: 2.0,
                purchased: false,
            },
            Upgrade {
                name: "Farm x2".into(),
                description: "Farm の生産 2倍".into(),
                cost: 11_000.0,
                target: ProducerKind::Farm,
                multiplier: 2.0,
                purchased: false,
            },
            Upgrade {
                name: "Mine x2".into(),
                description: "Mine の生産 2倍".into(),
                cost: 120_000.0,
                target: ProducerKind::Mine,
                multiplier: 2.0,
                purchased: false,
            },
            Upgrade {
                name: "Factory x2".into(),
                description: "Factory の生産 2倍".into(),
                cost: 1_300_000.0,
                target: ProducerKind::Factory,
                multiplier: 2.0,
                purchased: false,
            },
        ];

        Self {
            cookies: 0.0,
            cookies_all_time: 0.0,
            total_clicks: 0,
            cookies_per_click: 1.0,
            producers,
            upgrades,
            log: vec![CookieLogEntry {
                text: "Cookie Factory へようこそ！".into(),
                is_important: true,
            }],
            show_upgrades: false,
            anim_frame: 0,
            click_flash: 0,
            purchase_flash: 0,
            particles: Vec::new(),
            rng_state: 42,
        }
    }

    /// Total cookies per second from all producers.
    pub fn total_cps(&self) -> f64 {
        self.producers.iter().map(|p| p.cps()).sum()
    }

    pub fn add_log(&mut self, text: &str, is_important: bool) {
        self.log.push(CookieLogEntry {
            text: text.to_string(),
            is_important,
        });
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn producer_initial_cost() {
        let p = Producer::new(ProducerKind::Cursor);
        assert!((p.cost() - 15.0).abs() < 0.001);
    }

    #[test]
    fn producer_cost_scales() {
        let mut p = Producer::new(ProducerKind::Cursor);
        p.count = 1;
        let expected = 15.0 * 1.15;
        assert!((p.cost() - expected).abs() < 0.01);

        p.count = 10;
        let expected = 15.0 * 1.15_f64.powi(10);
        assert!((p.cost() - expected).abs() < 0.1);
    }

    #[test]
    fn producer_cps_zero_count() {
        let p = Producer::new(ProducerKind::Grandma);
        assert!((p.cps() - 0.0).abs() < 0.001);
    }

    #[test]
    fn producer_cps_with_count() {
        let mut p = Producer::new(ProducerKind::Grandma);
        p.count = 5;
        assert!((p.cps() - 5.0).abs() < 0.001); // 5 * 1.0 * 1.0
    }

    #[test]
    fn producer_cps_with_multiplier() {
        let mut p = Producer::new(ProducerKind::Grandma);
        p.count = 5;
        p.multiplier = 2.0;
        assert!((p.cps() - 10.0).abs() < 0.001); // 5 * 1.0 * 2.0
    }

    #[test]
    fn state_total_cps() {
        let mut state = CookieState::new();
        state.producers[0].count = 10; // 10 cursors = 1.0 cps
        state.producers[1].count = 3;  // 3 grandmas = 3.0 cps
        let expected = 10.0 * 0.1 + 3.0 * 1.0;
        assert!((state.total_cps() - expected).abs() < 0.001);
    }

    #[test]
    fn producer_next_unit_cps() {
        let mut p = Producer::new(ProducerKind::Grandma);
        // Base rate is 1.0, multiplier is 1.0
        assert!((p.next_unit_cps() - 1.0).abs() < 0.001);
        p.multiplier = 2.0;
        assert!((p.next_unit_cps() - 2.0).abs() < 0.001);
    }

    #[test]
    fn producer_payback_seconds() {
        let p = Producer::new(ProducerKind::Cursor);
        // Cost 15, rate 0.1 → payback = 150s
        let payback = p.payback_seconds().unwrap();
        assert!((payback - 150.0).abs() < 0.1);
    }

    #[test]
    fn producer_payback_grandma_better_than_cursor() {
        let cursor = Producer::new(ProducerKind::Cursor);
        let grandma = Producer::new(ProducerKind::Grandma);
        // Grandma payback (100/1.0=100s) should be less than Cursor (15/0.1=150s)
        assert!(grandma.payback_seconds().unwrap() < cursor.payback_seconds().unwrap());
    }

    #[test]
    fn log_truncation() {
        let mut state = CookieState::new();
        for i in 0..60 {
            state.add_log(&format!("msg {}", i), false);
        }
        assert!(state.log.len() <= 50);
    }
}
