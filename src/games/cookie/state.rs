//! Cookie Factory game state definitions.

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

    /// Index in the producers vec.
    pub fn index(&self) -> usize {
        match self {
            ProducerKind::Cursor => 0,
            ProducerKind::Grandma => 1,
            ProducerKind::Farm => 2,
            ProducerKind::Mine => 3,
            ProducerKind::Factory => 4,
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

    /// Base CPS from this producer type (without synergy).
    pub fn base_cps(&self) -> f64 {
        self.count as f64 * self.kind.base_rate() * self.multiplier
    }

    /// CPS with synergy bonus applied.
    pub fn cps_with_synergy(&self, synergy_bonus: f64) -> f64 {
        self.base_cps() * (1.0 + synergy_bonus)
    }

    /// CPS gained by buying the next unit (with synergy).
    pub fn next_unit_cps_with_synergy(&self, synergy_bonus: f64) -> f64 {
        self.kind.base_rate() * self.multiplier * (1.0 + synergy_bonus)
    }

    /// Payback time in seconds with synergy.
    pub fn payback_seconds_with_synergy(&self, synergy_bonus: f64) -> Option<f64> {
        let cps = self.next_unit_cps_with_synergy(synergy_bonus);
        if cps > 0.0 {
            Some(self.cost() / cps)
        } else {
            None
        }
    }

    /// CPS without synergy (used in tests).
    #[cfg(test)]
    pub fn cps(&self) -> f64 {
        self.base_cps()
    }

    /// Next unit CPS without synergy (used in tests).
    #[cfg(test)]
    pub fn next_unit_cps(&self) -> f64 {
        self.kind.base_rate() * self.multiplier
    }

    /// Payback without synergy (used in tests).
    #[cfg(test)]
    pub fn payback_seconds(&self) -> Option<f64> {
        let cps = self.next_unit_cps();
        if cps > 0.0 {
            Some(self.cost() / cps)
        } else {
            None
        }
    }
}

/// Upgrade effect type.
#[derive(Clone, Debug, PartialEq)]
pub enum UpgradeEffect {
    /// Add to cookies_per_click.
    ClickPower(f64),
    /// Multiply a producer's base rate.
    ProducerMultiplier { target: ProducerKind, multiplier: f64 },
    /// Double the synergy bonus for a producer.
    SynergyBoost { target: ProducerKind },
    /// Each unit of `source` gives `target` +bonus% production.
    CrossSynergy { source: ProducerKind, target: ProducerKind, bonus_per_unit: f64 },
    /// Each unit of `target` boosts all units of `target` by bonus_per_unit.
    /// Creates quadratic scaling: total bonus = count * bonus_per_unit.
    CountScaling { target: ProducerKind, bonus_per_unit: f64 },
    /// Each unit of `target` adds `percentage` of total CPS as bonus production.
    CpsPercentBonus { target: ProducerKind, percentage: f64 },
}

/// An available upgrade.
#[derive(Clone, Debug)]
pub struct Upgrade {
    pub name: String,
    pub description: String,
    pub cost: f64,
    pub purchased: bool,
    /// Effect to apply when purchased.
    pub effect: UpgradeEffect,
    /// Unlock condition: requires this producer to have >= count.
    pub unlock_condition: Option<(ProducerKind, u32)>,
}

/// Golden cookie bonus effect types.
#[derive(Clone, Debug, PartialEq)]
pub enum GoldenEffect {
    /// Multiply all production for duration.
    ProductionFrenzy { multiplier: f64 },
    /// Multiply click power for duration.
    ClickFrenzy { multiplier: f64 },
    /// Instant cookies = CPS * seconds.
    InstantBonus { cps_seconds: f64 },
}

impl GoldenEffect {
    pub fn description(&self) -> &str {
        match self {
            GoldenEffect::ProductionFrenzy { .. } => "生産フィーバー！",
            GoldenEffect::ClickFrenzy { .. } => "クリックラッシュ！",
            GoldenEffect::InstantBonus { .. } => "ラッキークッキー！",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            GoldenEffect::ProductionFrenzy { multiplier } => format!("生産×{} 発動中！", multiplier),
            GoldenEffect::ClickFrenzy { multiplier } => format!("クリック×{} 発動中！", multiplier),
            GoldenEffect::InstantBonus { cps_seconds } => format!("CPS×{}秒分GET！", cps_seconds),
        }
    }
}

/// Active golden cookie event.
#[derive(Clone, Debug)]
pub struct GoldenCookieEvent {
    /// Ticks until the golden cookie disappears if not clicked.
    pub appear_ticks_left: u32,
    /// Whether the player has claimed this event.
    pub claimed: bool,
}

/// Active buff from a golden cookie.
#[derive(Clone, Debug)]
pub struct ActiveBuff {
    pub effect: GoldenEffect,
    /// Ticks remaining for this buff.
    pub ticks_left: u32,
}

/// Mini-event types — smaller, more frequent events that auto-fire.
#[derive(Clone, Debug, PartialEq)]
pub enum MiniEventKind {
    /// Small instant cookie bonus (CPS × seconds).
    LuckyDrop { cps_seconds: f64 },
    /// Temporary click power boost.
    SugarRush { multiplier: f64 },
    /// One random producer gets a temporary boost.
    ProductionSurge { target: ProducerKind, multiplier: f64 },
    /// Next purchase is cheaper.
    DiscountWave { discount: f64 },
}

impl MiniEventKind {
    pub fn description(&self) -> String {
        match self {
            MiniEventKind::LuckyDrop { cps_seconds } => {
                format!("🎁 ラッキードロップ！(CPS×{:.0}秒分)", cps_seconds)
            }
            MiniEventKind::SugarRush { multiplier } => {
                format!("🍬 シュガーラッシュ！クリック×{:.0}(5秒)", multiplier)
            }
            MiniEventKind::ProductionSurge { target, multiplier } => {
                format!("⚡ {}が活性化！×{:.0}(10秒)", target.name(), multiplier)
            }
            MiniEventKind::DiscountWave { discount } => {
                format!("💰 割引ウェーブ！次の購入{:.0}%OFF", discount * 100.0)
            }
        }
    }
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
    /// Synergy bonus multiplier (from upgrades, default 1.0).
    pub synergy_multiplier: f64,
    /// Cross-synergy bonuses: (source, target, bonus_per_unit).
    pub cross_synergies: Vec<(ProducerKind, ProducerKind, f64)>,
    /// Golden cookie: ticks until next spawn.
    pub golden_next_spawn: u32,
    /// Current golden cookie event (if any).
    pub golden_event: Option<GoldenCookieEvent>,
    /// Active buffs from claimed golden cookies.
    pub active_buffs: Vec<ActiveBuff>,
    /// Total golden cookies claimed (for stats).
    pub golden_cookies_claimed: u32,
    /// Pseudo-random state for deterministic golden cookie spawning.
    pub rng_state: u32,
    /// Count-scaling bonuses: (target, bonus_per_unit). Each unit boosts all same-type units.
    pub count_scalings: Vec<(ProducerKind, f64)>,
    /// CPS-percent bonuses: (target, percentage). Each unit adds % of total CPS.
    pub cps_percent_bonuses: Vec<(ProducerKind, f64)>,
    /// Mini-event: ticks until next auto-fire.
    pub mini_event_next: u32,
    /// Active discount (0.0 = no discount, 0.25 = 25% off next purchase).
    pub active_discount: f64,
}

impl CookieState {
    pub fn new() -> Self {
        let producers = ProducerKind::all()
            .iter()
            .map(|k| Producer::new(k.clone()))
            .collect();

        let upgrades = Self::create_upgrades();

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
            synergy_multiplier: 1.0,
            cross_synergies: Vec::new(),
            golden_next_spawn: 300, // First golden cookie after 30 seconds
            golden_event: None,
            active_buffs: Vec::new(),
            golden_cookies_claimed: 0,
            rng_state: 42,
            count_scalings: Vec::new(),
            cps_percent_bonuses: Vec::new(),
            mini_event_next: 150, // First mini-event after 15 seconds
            active_discount: 0.0,
        }
    }

    fn create_upgrades() -> Vec<Upgrade> {
        vec![
            // === Phase 1: Basic upgrades (original) ===
            Upgrade {
                name: "強化クリック".into(),
                description: "クリック +1".into(),
                cost: 100.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(1.0),
                unlock_condition: None,
            },
            Upgrade {
                name: "Cursor x2".into(),
                description: "Cursor の生産 2倍".into(),
                cost: 200.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Cursor,
                    multiplier: 2.0,
                },
                unlock_condition: None,
            },
            Upgrade {
                name: "Grandma x2".into(),
                description: "Grandma の生産 2倍".into(),
                cost: 1_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Grandma,
                    multiplier: 2.0,
                },
                unlock_condition: None,
            },
            Upgrade {
                name: "Farm x2".into(),
                description: "Farm の生産 2倍".into(),
                cost: 11_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Farm,
                    multiplier: 2.0,
                },
                unlock_condition: None,
            },
            Upgrade {
                name: "Mine x2".into(),
                description: "Mine の生産 2倍".into(),
                cost: 120_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Mine,
                    multiplier: 2.0,
                },
                unlock_condition: None,
            },
            Upgrade {
                name: "Factory x2".into(),
                description: "Factory の生産 2倍".into(),
                cost: 1_300_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Factory,
                    multiplier: 2.0,
                },
                unlock_condition: None,
            },
            // === Phase 2: Synergy upgrades (unlocked by milestones) ===
            Upgrade {
                name: "おばあちゃんの知恵".into(),
                description: "Grandma1台→Cursor+1%".into(),
                cost: 500.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Grandma,
                    target: ProducerKind::Cursor,
                    bonus_per_unit: 0.01,
                },
                unlock_condition: Some((ProducerKind::Grandma, 5)),
            },
            Upgrade {
                name: "農場の恵み".into(),
                description: "Farm1台→Grandma+2%".into(),
                cost: 5_500.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Farm,
                    target: ProducerKind::Grandma,
                    bonus_per_unit: 0.02,
                },
                unlock_condition: Some((ProducerKind::Farm, 5)),
            },
            Upgrade {
                name: "鉱石の肥料".into(),
                description: "Mine1台→Farm+3%".into(),
                cost: 60_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Mine,
                    target: ProducerKind::Farm,
                    bonus_per_unit: 0.03,
                },
                unlock_condition: Some((ProducerKind::Mine, 5)),
            },
            Upgrade {
                name: "工場の掘削機".into(),
                description: "Factory1台→Mine+5%".into(),
                cost: 650_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Factory,
                    target: ProducerKind::Mine,
                    bonus_per_unit: 0.05,
                },
                unlock_condition: Some((ProducerKind::Factory, 5)),
            },
            Upgrade {
                name: "自動制御システム".into(),
                description: "Cursor10台毎→Factory+1%".into(),
                cost: 500_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Cursor,
                    target: ProducerKind::Factory,
                    bonus_per_unit: 0.001,
                },
                unlock_condition: Some((ProducerKind::Cursor, 25)),
            },
            // === Phase 3: Advanced multipliers (unlocked by count milestones) ===
            Upgrade {
                name: "Cursor x3".into(),
                description: "Cursor の生産 3倍".into(),
                cost: 5_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Cursor,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Cursor, 25)),
            },
            Upgrade {
                name: "Grandma x3".into(),
                description: "Grandma の生産 3倍".into(),
                cost: 25_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Grandma,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Grandma, 25)),
            },
            Upgrade {
                name: "Farm x3".into(),
                description: "Farm の生産 3倍".into(),
                cost: 275_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Farm,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Farm, 25)),
            },
            Upgrade {
                name: "シナジー倍化".into(),
                description: "全シナジー効果 2倍".into(),
                cost: 2_000_000.0,
                purchased: false,
                effect: UpgradeEffect::SynergyBoost {
                    target: ProducerKind::Factory,
                },
                unlock_condition: Some((ProducerKind::Factory, 10)),
            },
            Upgrade {
                name: "超強化クリック".into(),
                description: "クリック +5".into(),
                cost: 50_000.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(5.0),
                unlock_condition: Some((ProducerKind::Cursor, 50)),
            },
            // === Phase 3.5: Missing x3 multipliers for Mine/Factory ===
            Upgrade {
                name: "Mine x3".into(),
                description: "Mine の生産 3倍".into(),
                cost: 1_500_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Mine,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Mine, 15)),
            },
            Upgrade {
                name: "Factory x3".into(),
                description: "Factory の生産 3倍".into(),
                cost: 15_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Factory,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Factory, 10)),
            },
            // === Phase 4: x5 multipliers (通常強化・上位) ===
            Upgrade {
                name: "Cursor x5".into(),
                description: "Cursor の生産 5倍".into(),
                cost: 200_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Cursor,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Cursor, 50)),
            },
            Upgrade {
                name: "Grandma x5".into(),
                description: "Grandma の生産 5倍".into(),
                cost: 2_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Grandma,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Grandma, 50)),
            },
            Upgrade {
                name: "Farm x5".into(),
                description: "Farm の生産 5倍".into(),
                cost: 15_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Farm,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Farm, 30)),
            },
            Upgrade {
                name: "Mine x5".into(),
                description: "Mine の生産 5倍".into(),
                cost: 150_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Mine,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Mine, 25)),
            },
            Upgrade {
                name: "Factory x5".into(),
                description: "Factory の生産 5倍".into(),
                cost: 1_500_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Factory,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Factory, 15)),
            },
            // === Phase 5: 大幅強化 — 台数ボーナス (CountScaling) ===
            // Each unit boosts all same-type units → quadratic growth
            Upgrade {
                name: "Cursorの結束".into(),
                description: "各Cursor毎に全Cursor+0.5%".into(),
                cost: 100_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Cursor,
                    bonus_per_unit: 0.005,
                },
                unlock_condition: Some((ProducerKind::Cursor, 40)),
            },
            Upgrade {
                name: "Grandmaの結束".into(),
                description: "各Grandma毎に全Grandma+1%".into(),
                cost: 500_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Grandma,
                    bonus_per_unit: 0.01,
                },
                unlock_condition: Some((ProducerKind::Grandma, 30)),
            },
            Upgrade {
                name: "Farmの結束".into(),
                description: "各Farm毎に全Farm+1.5%".into(),
                cost: 5_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Farm,
                    bonus_per_unit: 0.015,
                },
                unlock_condition: Some((ProducerKind::Farm, 20)),
            },
            Upgrade {
                name: "Mineの結束".into(),
                description: "各Mine毎に全Mine+2%".into(),
                cost: 50_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Mine,
                    bonus_per_unit: 0.02,
                },
                unlock_condition: Some((ProducerKind::Mine, 15)),
            },
            Upgrade {
                name: "Factoryの結束".into(),
                description: "各Factory毎に全Factory+3%".into(),
                cost: 500_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Factory,
                    bonus_per_unit: 0.03,
                },
                unlock_condition: Some((ProducerKind::Factory, 10)),
            },
            // === Phase 6: CPS連動ボーナス ===
            // Each unit adds a % of total CPS — rewards balanced growth
            Upgrade {
                name: "CPS吸収:Cursor".into(),
                description: "各Cursorが総CPS×0.01%を追加".into(),
                cost: 500_000.0,
                purchased: false,
                effect: UpgradeEffect::CpsPercentBonus {
                    target: ProducerKind::Cursor,
                    percentage: 0.0001,
                },
                unlock_condition: Some((ProducerKind::Cursor, 60)),
            },
            Upgrade {
                name: "CPS吸収:Grandma".into(),
                description: "各Grandmaが総CPS×0.02%を追加".into(),
                cost: 5_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CpsPercentBonus {
                    target: ProducerKind::Grandma,
                    percentage: 0.0002,
                },
                unlock_condition: Some((ProducerKind::Grandma, 50)),
            },
            Upgrade {
                name: "CPS吸収:Farm".into(),
                description: "各Farmが総CPS×0.05%を追加".into(),
                cost: 50_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CpsPercentBonus {
                    target: ProducerKind::Farm,
                    percentage: 0.0005,
                },
                unlock_condition: Some((ProducerKind::Farm, 30)),
            },
            // === Phase 7: 超強化クリック上位 & シナジー倍化2 ===
            Upgrade {
                name: "究極クリック".into(),
                description: "クリック +50".into(),
                cost: 1_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(50.0),
                unlock_condition: Some((ProducerKind::Cursor, 75)),
            },
            Upgrade {
                name: "シナジー倍化II".into(),
                description: "全シナジー効果 さらに2倍".into(),
                cost: 10_000_000.0,
                purchased: false,
                effect: UpgradeEffect::SynergyBoost {
                    target: ProducerKind::Factory,
                },
                unlock_condition: Some((ProducerKind::Factory, 15)),
            },
        ]
    }

    /// Calculate the synergy bonus for a specific producer kind.
    /// Returns the total bonus as a fraction (e.g. 0.10 = +10%).
    pub fn synergy_bonus(&self, target: &ProducerKind) -> f64 {
        let mut bonus = 0.0;

        // Built-in synergies (always active, part of game design)
        let base_synergies: &[(ProducerKind, ProducerKind, f64)] = &[
            // (source, target, bonus_per_source_unit)
            (ProducerKind::Grandma, ProducerKind::Cursor, 0.01),   // +1% per Grandma
            (ProducerKind::Farm, ProducerKind::Grandma, 0.02),     // +2% per Farm
            (ProducerKind::Mine, ProducerKind::Farm, 0.03),        // +3% per Mine
            (ProducerKind::Factory, ProducerKind::Mine, 0.05),     // +5% per Factory
            (ProducerKind::Cursor, ProducerKind::Factory, 0.001),  // +0.1% per Cursor
        ];

        for (source, tgt, rate) in base_synergies {
            if tgt == target {
                let source_count = self.producers[source.index()].count as f64;
                bonus += source_count * rate;
            }
        }

        // Cross-synergies from upgrades
        for (source, tgt, rate) in &self.cross_synergies {
            if tgt == target {
                let source_count = self.producers[source.index()].count as f64;
                bonus += source_count * rate;
            }
        }

        bonus * self.synergy_multiplier
    }

    /// Count-scaling bonus for a producer (from CountScaling upgrades).
    /// Returns the total bonus as a fraction (e.g. 0.50 = +50%).
    pub fn count_scaling_bonus(&self, target: &ProducerKind) -> f64 {
        let count = self.producers[target.index()].count as f64;
        self.count_scalings
            .iter()
            .filter(|(tgt, _)| tgt == target)
            .map(|(_, bonus)| count * bonus)
            .sum()
    }

    /// CPS-percent bonus for a producer (additional CPS from CpsPercentBonus upgrades).
    /// This depends on base_cps_without_percent, so computed separately.
    fn cps_percent_extra(&self, base_total: f64) -> f64 {
        self.cps_percent_bonuses
            .iter()
            .map(|(target, pct)| {
                let count = self.producers[target.index()].count as f64;
                base_total * count * pct
            })
            .sum()
    }

    /// Total CPS including synergies, count scaling, CPS% bonuses, and active buffs.
    pub fn total_cps(&self) -> f64 {
        // Step 1: base CPS with synergies + count scaling
        let base: f64 = self.producers.iter().map(|p| {
            let syn = self.synergy_bonus(&p.kind);
            let cs = self.count_scaling_bonus(&p.kind);
            p.cps_with_synergy(syn + cs)
        }).sum();

        // Step 2: CPS-percent bonuses (based on base total, to avoid infinite recursion)
        let extra = self.cps_percent_extra(base);
        let total = base + extra;

        // Step 3: Apply production frenzy buff
        let mut multiplier = 1.0;
        for buff in &self.active_buffs {
            if let GoldenEffect::ProductionFrenzy { multiplier: m } = &buff.effect {
                multiplier *= m;
            }
        }

        total * multiplier
    }

    /// Effective cookies per click (with buffs).
    pub fn effective_click_power(&self) -> f64 {
        let mut power = self.cookies_per_click;
        for buff in &self.active_buffs {
            if let GoldenEffect::ClickFrenzy { multiplier } = &buff.effect {
                power *= multiplier;
            }
        }
        power
    }

    /// Check if an upgrade's unlock condition is met.
    pub fn is_upgrade_unlocked(&self, upgrade: &Upgrade) -> bool {
        match &upgrade.unlock_condition {
            None => true,
            Some((kind, required_count)) => {
                self.producers[kind.index()].count >= *required_count
            }
        }
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

    /// Simple pseudo-random number generator (xorshift32).
    pub fn next_random(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
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
        // With synergies: cursor gets +1% per grandma (3%) = 1.0 * 1.03 = 1.03
        // Grandma gets no synergy yet (0 farms) = 3.0
        let expected = 1.0 * 1.03 + 3.0;
        assert!((state.total_cps() - expected).abs() < 0.01);
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

    #[test]
    fn synergy_bonus_grandma_to_cursor() {
        let mut state = CookieState::new();
        state.producers[1].count = 10; // 10 grandmas
        let bonus = state.synergy_bonus(&ProducerKind::Cursor);
        assert!((bonus - 0.10).abs() < 0.001); // 10 * 1% = 10%
    }

    #[test]
    fn synergy_circular_all_producers() {
        let mut state = CookieState::new();
        state.producers[0].count = 100; // 100 cursors → Factory +0.1% each = +10%
        state.producers[4].count = 5;   // 5 factories → Mine +5% each = +25%
        let factory_bonus = state.synergy_bonus(&ProducerKind::Factory);
        assert!((factory_bonus - 0.10).abs() < 0.01); // 100 * 0.001 = 0.10
        let mine_bonus = state.synergy_bonus(&ProducerKind::Mine);
        assert!((mine_bonus - 0.25).abs() < 0.01); // 5 * 0.05 = 0.25
    }

    #[test]
    fn synergy_multiplier_doubles() {
        let mut state = CookieState::new();
        state.producers[1].count = 10;
        state.synergy_multiplier = 2.0;
        let bonus = state.synergy_bonus(&ProducerKind::Cursor);
        assert!((bonus - 0.20).abs() < 0.001); // 10% * 2 = 20%
    }

    #[test]
    fn upgrade_unlock_condition() {
        let mut state = CookieState::new();
        // "おばあちゃんの知恵" requires Grandma >= 5
        let synergy_upgrade = &state.upgrades[6];
        assert!(!state.is_upgrade_unlocked(synergy_upgrade));
        state.producers[1].count = 5;
        assert!(state.is_upgrade_unlocked(synergy_upgrade));
    }

    #[test]
    fn effective_click_with_buff() {
        let mut state = CookieState::new();
        state.cookies_per_click = 2.0;
        state.active_buffs.push(ActiveBuff {
            effect: GoldenEffect::ClickFrenzy { multiplier: 10.0 },
            ticks_left: 100,
        });
        assert!((state.effective_click_power() - 20.0).abs() < 0.001);
    }

    #[test]
    fn production_frenzy_buff() {
        let mut state = CookieState::new();
        state.producers[1].count = 5; // 5 grandmas = 5.0 cps base
        let base = state.total_cps();
        state.active_buffs.push(ActiveBuff {
            effect: GoldenEffect::ProductionFrenzy { multiplier: 7.0 },
            ticks_left: 70,
        });
        let buffed = state.total_cps();
        assert!((buffed - base * 7.0).abs() < 0.01);
    }
}
