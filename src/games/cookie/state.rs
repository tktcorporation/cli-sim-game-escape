//! Cookie Factory game state definitions.

/// Kinds of producers (auto-clickers).
#[derive(Clone, Debug, PartialEq)]
pub enum ProducerKind {
    Cursor,
    Grandma,
    Farm,
    Mine,
    Factory,
    Temple,
    WizardTower,
    Shipment,
    AlchemyLab,
    Portal,
    TimeMachine,
    AntimatterCondenser,
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
            ProducerKind::Temple,
            ProducerKind::WizardTower,
            ProducerKind::Shipment,
            ProducerKind::AlchemyLab,
            ProducerKind::Portal,
            ProducerKind::TimeMachine,
            ProducerKind::AntimatterCondenser,
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
            ProducerKind::Temple => "Temple",
            ProducerKind::WizardTower => "WzTower",
            ProducerKind::Shipment => "Shipment",
            ProducerKind::AlchemyLab => "Alchemy",
            ProducerKind::Portal => "Portal",
            ProducerKind::TimeMachine => "TimeMchn",
            ProducerKind::AntimatterCondenser => "Antimtr",
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
            ProducerKind::Temple => 1_400_000.0,
            ProducerKind::WizardTower => 20_000_000.0,
            ProducerKind::Shipment => 330_000_000.0,
            ProducerKind::AlchemyLab => 5_100_000_000.0,
            ProducerKind::Portal => 75_000_000_000.0,
            ProducerKind::TimeMachine => 1_100_000_000_000.0,
            ProducerKind::AntimatterCondenser => 17_000_000_000_000.0,
        }
    }

    /// Base cookies per second per unit.
    ///
    /// Design: lower-tier producers have better initial cost/rate ratio
    /// but higher-tier ones scale better with count (see `cost_growth_rate`).
    /// This creates a "cheap & capped vs expensive & scaling" dilemma.
    pub fn base_rate(&self) -> f64 {
        match self {
            ProducerKind::Cursor => 0.1,
            ProducerKind::Grandma => 1.0,
            ProducerKind::Farm => 8.0,
            ProducerKind::Mine => 47.0,
            ProducerKind::Factory => 260.0,
            ProducerKind::Temple => 1_400.0,
            ProducerKind::WizardTower => 7_800.0,
            ProducerKind::Shipment => 44_000.0,
            ProducerKind::AlchemyLab => 260_000.0,
            ProducerKind::Portal => 1_600_000.0,
            ProducerKind::TimeMachine => 9_800_000.0,
            ProducerKind::AntimatterCondenser => 64_000_000.0,
        }
    }

    /// Cost growth rate per additional unit purchased.
    ///
    /// Lower-tier producers have higher growth rates (costs skyrocket fast),
    /// while higher-tier producers grow more slowly (stay investable longer).
    /// This means cheap producers are great early but hit a wall,
    /// while expensive producers become the better long-term investment.
    pub fn cost_growth_rate(&self) -> f64 {
        match self {
            ProducerKind::Cursor => 1.20,           // Cheap but costs explode
            ProducerKind::Grandma => 1.18,
            ProducerKind::Farm => 1.17,
            ProducerKind::Mine => 1.16,
            ProducerKind::Factory => 1.15,           // Middle ground
            ProducerKind::Temple => 1.14,
            ProducerKind::WizardTower => 1.13,
            ProducerKind::Shipment => 1.12,
            ProducerKind::AlchemyLab => 1.11,
            ProducerKind::Portal => 1.10,
            ProducerKind::TimeMachine => 1.09,
            ProducerKind::AntimatterCondenser => 1.08, // Expensive but scales well
        }
    }

    /// Key to buy (1-8, 9, 0, -, = mapped to producer index).
    pub fn key(&self) -> char {
        match self {
            ProducerKind::Cursor => '1',
            ProducerKind::Grandma => '2',
            ProducerKind::Farm => '3',
            ProducerKind::Mine => '4',
            ProducerKind::Factory => '5',
            ProducerKind::Temple => '6',
            ProducerKind::WizardTower => '7',
            ProducerKind::Shipment => '8',
            ProducerKind::AlchemyLab => '9',
            ProducerKind::Portal => '0',
            ProducerKind::TimeMachine => '-',
            ProducerKind::AntimatterCondenser => '=',
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
            ProducerKind::Temple => 5,
            ProducerKind::WizardTower => 6,
            ProducerKind::Shipment => 7,
            ProducerKind::AlchemyLab => 8,
            ProducerKind::Portal => 9,
            ProducerKind::TimeMachine => 10,
            ProducerKind::AntimatterCondenser => 11,
        }
    }

    /// Convert an index back to a ProducerKind.
    pub fn from_index(idx: usize) -> Option<ProducerKind> {
        ProducerKind::all().get(idx).cloned()
    }
}

/// ROI (Return on Investment) information for a producer.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RoiInfo {
    /// CPS gained per cookie spent (higher = more efficient).
    pub efficiency: f64,
    /// Seconds to recoup the investment (lower = faster payback).
    pub payback_seconds: f64,
    /// Star rating (0-3).
    pub rating: u8,
    /// Whether the producer is affordable.
    pub affordable: bool,
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
    ///
    /// Each producer has its own cost growth rate: cheap producers
    /// escalate faster (1.20x) while expensive ones grow slower (1.08x).
    pub fn cost(&self) -> f64 {
        self.kind.base_cost() * self.kind.cost_growth_rate().powi(self.count as i32)
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
    /// Kitten upgrade: multiplies CPS by (1 + milk * multiplier).
    KittenBoost { multiplier: f64 },
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
    /// Exclusive group: only ONE upgrade per group can be purchased.
    /// `None` = no exclusivity (can always buy).
    /// `Some(n)` = buying this locks out all other upgrades in group `n`.
    pub exclusive_group: Option<u8>,
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

/// Milestone condition types.
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneCondition {
    /// Total cookies baked all-time >= threshold.
    TotalCookies(f64),
    /// A specific producer count >= threshold.
    ProducerCount(ProducerKind, u32),
    /// Total CPS >= threshold.
    CpsReached(f64),
    /// Total manual clicks >= threshold.
    TotalClicks(u64),
    /// Golden cookies claimed >= threshold.
    GoldenClaimed(u32),
}

/// Milestone status: locked → ready (condition met) → claimed (player collected).
#[derive(Clone, Debug, PartialEq)]
pub enum MilestoneStatus {
    /// Condition not yet met.
    Locked,
    /// Condition met, waiting for player to claim.
    Ready,
    /// Player has claimed the milestone (milk applied).
    Claimed,
}

/// A milestone (achievement) definition.
#[derive(Clone, Debug)]
pub struct Milestone {
    pub name: String,
    pub description: String,
    pub condition: MilestoneCondition,
    pub status: MilestoneStatus,
}

/// Particle style for different visual effects.
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleStyle {
    /// Normal click "+N" particle (rises up).
    Click,
    /// Emoji burst particle (rises up with drift).
    Emoji,
    /// Sparkle ambient particle (twinkles in place).
    Sparkle,
    /// Celebration burst particle (explodes outward from center).
    Celebration,
    /// Combo indicator text.
    Combo,
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
    /// Visual style of this particle.
    pub style: ParticleStyle,
    /// Row offset for celebration particles (signed, from center).
    pub row_offset: i16,
}

/// Log entry for the Cookie game.
#[derive(Clone, Debug)]
pub struct CookieLogEntry {
    pub text: String,
    pub is_important: bool,
}

/// Prestige upgrade path (for tree structure).
#[derive(Clone, Debug, PartialEq)]
pub enum PrestigePath {
    /// Root upgrade (天使の贈り物)
    Root,
    /// 生産パス — 放置向け、CPS強化
    Production,
    /// クリックパス — アクティブ向け、クリック強化
    Click,
    /// 幸運パス — イベント向け、ゴールデン強化
    Luck,
}

/// Prestige upgrade definition.
#[derive(Clone, Debug)]
pub struct PrestigeUpgrade {
    /// Unique identifier for prerequisite checking.
    pub id: &'static str,
    pub name: String,
    pub description: String,
    /// Cost in heavenly chips.
    pub cost: u64,
    pub purchased: bool,
    pub effect: PrestigeEffect,
    /// ID of the required upgrade (None = no prerequisite).
    pub requires: Option<&'static str>,
    /// Which path this upgrade belongs to.
    pub path: PrestigePath,
}

/// Prestige upgrade effects.
#[derive(Clone, Debug, PartialEq)]
pub enum PrestigeEffect {
    /// Start each run with N cookies.
    StartingCookies(f64),
    /// Multiply CPS permanently (stacks multiplicatively).
    CpsMultiplier(f64),
    /// Multiply click power permanently.
    ClickMultiplier(f64),
    /// Golden cookies appear faster (multiply spawn delay by this factor, < 1.0).
    GoldenCookieSpeed(f64),
    /// Retain a percentage of milk across resets.
    MilkRetention(f64),
    /// Reduce all producer costs by this fraction (e.g. 0.1 = 10% off).
    ProducerCostReduction(f64),
    /// Start with N Cursors after prestige.
    StartingCursors(u32),
    /// Sugar boost effectiveness multiplier.
    SugarBoostMultiplier(f64),
    /// Golden cookie effect duration multiplier.
    GoldenDuration(f64),
    /// Golden cookie effect strength multiplier.
    GoldenEffectMultiplier(f64),
}

// ═══════════════════════════════════════════════════════
// Sugar System — 生産ブースト用消費リソース
// ═══════════════════════════════════════════════════════

/// Sugar boost types.
#[derive(Clone, Debug, PartialEq)]
pub enum SugarBoostKind {
    /// シュガーラッシュ: CPS ×2, 30秒
    Rush,
    /// シュガーフィーバー: CPS ×5, 30秒
    Fever,
    /// シュガーフレンジー: CPS ×10, 60秒 (転生3回で解放)
    Frenzy,
}

impl SugarBoostKind {
    /// Cost in sugar.
    pub fn cost(&self) -> u64 {
        match self {
            SugarBoostKind::Rush => 1,
            SugarBoostKind::Fever => 5,
            SugarBoostKind::Frenzy => 20,
        }
    }

    /// CPS multiplier.
    pub fn multiplier(&self) -> f64 {
        match self {
            SugarBoostKind::Rush => 2.0,
            SugarBoostKind::Fever => 5.0,
            SugarBoostKind::Frenzy => 10.0,
        }
    }

    /// Duration in ticks (10 ticks = 1 second).
    pub fn duration_ticks(&self) -> u32 {
        match self {
            SugarBoostKind::Rush => 300,   // 30秒
            SugarBoostKind::Fever => 300,  // 30秒
            SugarBoostKind::Frenzy => 600, // 60秒
        }
    }

    /// Name for display.
    pub fn name(&self) -> &'static str {
        match self {
            SugarBoostKind::Rush => "シュガーラッシュ",
            SugarBoostKind::Fever => "シュガーフィーバー",
            SugarBoostKind::Frenzy => "シュガーフレンジー",
        }
    }

    /// Prestige count required to unlock.
    pub fn required_prestige(&self) -> u32 {
        match self {
            SugarBoostKind::Rush => 0,
            SugarBoostKind::Fever => 0,
            SugarBoostKind::Frenzy => 3,
        }
    }
}

/// Active sugar boost.
#[derive(Clone, Debug)]
pub struct ActiveSugarBoost {
    pub kind: SugarBoostKind,
    pub ticks_left: u32,
}

// ═══════════════════════════════════════════════════════
// Research Tree — 2つの研究パス（転生でリセット）
// ═══════════════════════════════════════════════════════

/// Research path — exclusive choice, resets on prestige.
#[derive(Clone, Debug, PartialEq)]
pub enum ResearchPath {
    None,
    /// 量産路線: cheaper producers, more scaling.
    MassProduction,
    /// 品質路線: stronger clicks, buffs, synergies.
    Quality,
}

/// Research node effect.
#[derive(Clone, Debug, PartialEq)]
pub enum ResearchEffect {
    /// Reduce all producer costs by this fraction (e.g. 0.15 = 15% off).
    CostReduction(f64),
    /// Multiply all producer CPS.
    CpsMultiplier(f64),
    /// Click power gets bonus = total_CPS × percentage.
    ClickCpsPercent(f64),
    /// Golden cookie buff duration multiplied.
    BuffDuration(f64),
    /// Additional synergy multiplier (stacks multiplicatively).
    SynergyMultiplier(f64),
    /// Count scaling bonuses multiplied.
    CountScalingMultiplier(f64),
    /// All buff multiplier values boosted (production frenzy, click frenzy).
    BuffEffectMultiplier(f64),
}

/// A research node in the tech tree.
#[derive(Clone, Debug)]
pub struct ResearchNode {
    pub name: String,
    pub description: String,
    pub cost: f64,
    pub tier: u8,
    pub path: ResearchPath,
    pub purchased: bool,
    pub effect: ResearchEffect,
}

// ═══════════════════════════════════════════════════════
// Market — 相場変動システム
// ═══════════════════════════════════════════════════════

/// Market phase — cycles periodically, affects CPS and costs.
///
/// Design: Market phases create "buy timing" decisions.
/// - Normal: baseline, no pressure
/// - Bull: CPS high but costs high — good for earning, bad for buying
/// - Bear: CPS low but costs low — bad for earning, great for buying
/// - Bubble: extreme Bull — massive CPS but costs are absurd
/// - Crash: extreme Bear — CPS tanks but everything is dirt cheap (BUY NOW!)
///
/// The key insight: Crash + Bubble create dramatic "act NOW" moments.
/// A crash is an opportunity disguised as disaster.
#[derive(Clone, Debug, PartialEq)]
pub enum MarketPhase {
    /// 好景気: CPS↑, costs↑
    Bull,
    /// 不景気: CPS↓, costs↓
    Bear,
    /// 通常
    Normal,
    /// バブル: CPS大幅↑, costs大幅↑ — earn big but can't buy efficiently
    Bubble,
    /// 暴落: CPS大幅↓, costs大幅↓ — everything on sale, buy buy buy!
    Crash,
}

impl MarketPhase {
    pub fn cps_multiplier(&self) -> f64 {
        match self {
            MarketPhase::Bull => 1.5,
            MarketPhase::Bear => 0.7,
            MarketPhase::Normal => 1.0,
            MarketPhase::Bubble => 3.0,
            MarketPhase::Crash => 0.3,
        }
    }

    pub fn cost_multiplier(&self) -> f64 {
        match self {
            MarketPhase::Bull => 1.6,
            MarketPhase::Bear => 0.5,
            MarketPhase::Normal => 1.0,
            MarketPhase::Bubble => 3.0,
            MarketPhase::Crash => 0.25,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            MarketPhase::Bull => "好景気",
            MarketPhase::Bear => "不景気",
            MarketPhase::Normal => "通常",
            MarketPhase::Bubble => "バブル！",
            MarketPhase::Crash => "暴落！",
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            MarketPhase::Bull => "📈",
            MarketPhase::Bear => "📉",
            MarketPhase::Normal => "📊",
            MarketPhase::Bubble => "🚀",
            MarketPhase::Crash => "💥",
        }
    }
}

// ═══════════════════════════════════════════════════════
// Dragon — 育成 & オーラシステム
// ═══════════════════════════════════════════════════════

/// Dragon aura — passive bonus, choose one at a time.
#[derive(Clone, Debug, PartialEq)]
pub enum DragonAura {
    /// No aura selected.
    None,
    /// 富の吐息: CPS ×1.15 per dragon level.
    BreathOfRiches,
    /// ドラゴンカーソル: Click power ×1.2 per dragon level.
    DragonCursor,
    /// 倹約の翼: Producer costs -5% per dragon level.
    ElderPact,
    /// ドラゴンの収穫: Golden cookie spawn 10% faster per level.
    DragonHarvest,
}

impl DragonAura {
    pub fn name(&self) -> &str {
        match self {
            DragonAura::None => "なし",
            DragonAura::BreathOfRiches => "富の吐息",
            DragonAura::DragonCursor => "ドラゴンカーソル",
            DragonAura::ElderPact => "倹約の翼",
            DragonAura::DragonHarvest => "ドラゴンの収穫",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            DragonAura::None => "オーラ未選択",
            DragonAura::BreathOfRiches => "レベル毎にCPS+15%",
            DragonAura::DragonCursor => "レベル毎にクリック力+20%",
            DragonAura::ElderPact => "レベル毎に生産者コスト-5%",
            DragonAura::DragonHarvest => "レベル毎にゴールデン出現+10%速",
        }
    }

    pub fn all() -> &'static [DragonAura] {
        &[
            DragonAura::BreathOfRiches,
            DragonAura::DragonCursor,
            DragonAura::ElderPact,
            DragonAura::DragonHarvest,
        ]
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn index(&self) -> usize {
        match self {
            DragonAura::None => 0,
            DragonAura::BreathOfRiches => 1,
            DragonAura::DragonCursor => 2,
            DragonAura::ElderPact => 3,
            DragonAura::DragonHarvest => 4,
        }
    }
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
    /// Whether showing research panel.
    pub show_research: bool,
    /// Animation frame counter (incremented every tick).
    pub anim_frame: u32,
    /// Recent click flash timer (ticks remaining for visual feedback).
    pub click_flash: u32,
    /// Purchase celebration flash timer.
    pub purchase_flash: u32,
    /// Active floating particles.
    pub particles: Vec<Particle>,
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
    /// Milestones (achievements).
    pub milestones: Vec<Milestone>,
    /// Milk level: achieved milestones / total milestones (0.0 to 1.0+).
    pub milk: f64,
    /// Whether showing milestones panel.
    pub show_milestones: bool,
    /// Flash timer for milestone achievement notification.
    pub milestone_flash: u32,
    /// Kitten multiplier applied to CPS (computed from milk × kitten upgrades).
    pub kitten_multiplier: f64,

    // === Prestige (転生) system — survives reset ===
    /// Total prestige resets performed.
    pub prestige_count: u32,
    /// Heavenly chips earned (permanent currency).
    pub heavenly_chips: u64,
    /// Heavenly chips spent on prestige upgrades.
    pub heavenly_chips_spent: u64,
    /// CPS multiplier from heavenly chips (1.0 + chips * 0.10).
    pub prestige_multiplier: f64,
    /// Total cookies baked across all runs (for prestige calculation).
    pub cookies_all_runs: f64,
    /// Whether showing the prestige/stats panel.
    pub show_prestige: bool,
    /// Active sub-section within the Prestige tab (0=upgrades, 1=boosts, 2=dragon, 3=stats).
    pub prestige_section: u8,
    /// Scroll offset within the active Prestige sub-section (in lines).
    pub prestige_scroll: u16,
    /// Prestige upgrades purchased.
    pub prestige_upgrades: Vec<PrestigeUpgrade>,
    /// Flash timer for prestige action.
    pub prestige_flash: u32,

    // === Sugar system — 生産ブースト用消費リソース ===
    /// Current sugar amount.
    pub sugar: u64,
    /// Total sugar earned all time.
    pub sugar_all_time: u64,
    /// Active sugar boost (if any).
    pub active_sugar_boost: Option<ActiveSugarBoost>,
    /// Whether showing the sugar boost panel.

    // === Auto-clicker system — unlocked at prestige 1 ===
    /// Whether auto-clicker is enabled.
    pub auto_clicker_enabled: bool,
    /// Ticks until next auto-click (internal timer).
    pub auto_clicker_timer: u32,

    // === Statistics — survives reset ===
    /// Total ticks played across all runs.
    pub total_ticks: u64,
    /// Highest CPS ever achieved.
    pub best_cps: f64,
    /// Highest cookies in a single run.
    pub best_cookies_single_run: f64,

    // === Combo system ===
    /// Ticks since last click (resets on click, increments on tick).
    pub click_cooldown: u32,
    /// Current combo count (consecutive clicks within the combo window).
    pub combo_count: u32,
    /// Peak combo in current session.
    pub best_combo: u32,

    // === Analytics (not saved) ===
    /// CPS history for sparkline graph (sampled every 10 ticks = 1 second).
    /// Stores the last 40 samples.
    pub cps_history: Vec<f64>,
    /// Tick counter for CPS sampling interval.
    pub cps_sample_counter: u32,
    /// Cookies per second delta (change from last sample).
    pub cps_delta: f64,
    /// Previous CPS for delta calculation.
    pub prev_cps: f64,
    /// Cookies earned in the last 10 ticks (for "per second" display).
    pub cookies_earned_window: f64,
    /// Peak cookies earned in a single tick window.
    pub peak_cookies_per_sec: f64,

    // === Research Tree (転生でリセット) ===
    /// Selected research path (None until first research purchased).
    pub research_path: ResearchPath,
    /// Research nodes (tech tree).
    pub research_nodes: Vec<ResearchNode>,

    // === Market (相場変動) ===
    /// Current market phase.
    pub market_phase: MarketPhase,
    /// Ticks until next phase change.
    pub market_ticks_left: u32,

    // === Dragon (転生後も保持) ===
    /// Dragon level (0 = egg, max 7).
    pub dragon_level: u32,
    /// Selected dragon aura.
    pub dragon_aura: DragonAura,
    /// Total producers fed to dragon (across all feeding).
    pub dragon_fed_total: u32,
}

impl CookieState {
    pub fn new() -> Self {
        let producers = ProducerKind::all()
            .iter()
            .map(|k| Producer::new(k.clone()))
            .collect();

        let upgrades = Self::create_upgrades();

        let milestones = Self::create_milestones();

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
            show_research: false,
            anim_frame: 0,
            click_flash: 0,
            purchase_flash: 0,
            particles: Vec::new(),
            synergy_multiplier: 1.0,
            cross_synergies: Vec::new(),
            golden_next_spawn: 200, // First golden cookie after 20 seconds (faster early excitement)
            golden_event: None,
            active_buffs: Vec::new(),
            golden_cookies_claimed: 0,
            rng_state: 42,
            count_scalings: Vec::new(),
            cps_percent_bonuses: Vec::new(),
            mini_event_next: 100, // First mini-event after 10 seconds (faster early excitement)
            active_discount: 0.0,
            milestones,
            milk: 0.0,
            show_milestones: false,
            milestone_flash: 0,
            kitten_multiplier: 1.0,
            // Prestige fields
            prestige_count: 0,
            heavenly_chips: 0,
            heavenly_chips_spent: 0,
            prestige_multiplier: 1.0,
            cookies_all_runs: 0.0,
            show_prestige: false,
            prestige_section: 0,
            prestige_scroll: 0,
            prestige_upgrades: Self::create_prestige_upgrades(),
            prestige_flash: 0,
            // Sugar system
            sugar: 0,
            sugar_all_time: 0,
            active_sugar_boost: None,
            // Auto-clicker
            auto_clicker_enabled: false,
            auto_clicker_timer: 0,
            // Statistics
            total_ticks: 0,
            best_cps: 0.0,
            best_cookies_single_run: 0.0,
            // Combo system
            click_cooldown: 0,
            combo_count: 0,
            best_combo: 0,
            // Analytics
            cps_history: Vec::new(),
            cps_sample_counter: 0,
            cps_delta: 0.0,
            prev_cps: 0.0,
            cookies_earned_window: 0.0,
            peak_cookies_per_sec: 0.0,
            // Research
            research_path: ResearchPath::None,
            research_nodes: Self::create_research_nodes(),
            // Market
            market_phase: MarketPhase::Normal,
            market_ticks_left: 450, // First phase change after ~45 seconds
            // Dragon
            dragon_level: 0,
            dragon_aura: DragonAura::None,
            dragon_fed_total: 0,
        }
    }

    pub fn create_upgrades() -> Vec<Upgrade> {
        vec![
            // === Phase 1: Basic upgrades (original) ===
            Upgrade {
                name: "強化クリック".into(),
                description: "クリック +1".into(),
                cost: 100.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(1.0),
                unlock_condition: None,
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
            },
            Upgrade {
                name: "超強化クリック".into(),
                description: "クリック +5".into(),
                cost: 50_000.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(5.0),
                unlock_condition: Some((ProducerKind::Cursor, 50)),
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
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
                exclusive_group: None,
            },
            // === Phase 7: 超強化クリック上位 & シナジー倍化2 ===
            Upgrade {
                name: "究極クリック".into(),
                description: "クリック +50".into(),
                cost: 1_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(50.0),
                unlock_condition: Some((ProducerKind::Cursor, 75)),
                exclusive_group: None,
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
                exclusive_group: None,
            },
            // === Kitten upgrades (scale with milk from milestones) ===
            // === Phase 4.5: New producer base multipliers ===
            Upgrade {
                name: "Temple x2".into(),
                description: "Temple の生産 2倍".into(),
                cost: 14_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Temple,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::Temple, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "WzTower x2".into(),
                description: "WzTower の生産 2倍".into(),
                cost: 200_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::WizardTower,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::WizardTower, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Shipment x2".into(),
                description: "Shipment の生産 2倍".into(),
                cost: 3_300_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Shipment,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::Shipment, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Temple x3".into(),
                description: "Temple の生産 3倍".into(),
                cost: 140_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Temple,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Temple, 10)),
                exclusive_group: None,
            },
            Upgrade {
                name: "WzTower x3".into(),
                description: "WzTower の生産 3倍".into(),
                cost: 2_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::WizardTower,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::WizardTower, 10)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Shipment x3".into(),
                description: "Shipment の生産 3倍".into(),
                cost: 33_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Shipment,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Shipment, 10)),
                exclusive_group: None,
            },
            // === Alchemy Lab upgrades ===
            Upgrade {
                name: "Alchemy x2".into(),
                description: "Alchemy の生産 2倍".into(),
                cost: 51_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::AlchemyLab,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::AlchemyLab, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Alchemy x3".into(),
                description: "Alchemy の生産 3倍".into(),
                cost: 510_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::AlchemyLab,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::AlchemyLab, 10)),
                exclusive_group: None,
            },
            // === Portal upgrades ===
            Upgrade {
                name: "Portal x2".into(),
                description: "Portal の生産 2倍".into(),
                cost: 750_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Portal,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::Portal, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Portal x3".into(),
                description: "Portal の生産 3倍".into(),
                cost: 7_500_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Portal,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::Portal, 10)),
                exclusive_group: None,
            },
            // === Time Machine upgrades ===
            Upgrade {
                name: "TimeMchn x2".into(),
                description: "TimeMachine の生産 2倍".into(),
                cost: 11_000_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::TimeMachine,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::TimeMachine, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "TimeMchn x3".into(),
                description: "TimeMachine の生産 3倍".into(),
                cost: 110_000_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::TimeMachine,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::TimeMachine, 10)),
                exclusive_group: None,
            },
            // === Antimatter Condenser upgrades ===
            Upgrade {
                name: "Antimtr x2".into(),
                description: "Antimatter の生産 2倍".into(),
                cost: 170_000_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::AntimatterCondenser,
                    multiplier: 2.0,
                },
                unlock_condition: Some((ProducerKind::AntimatterCondenser, 1)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Antimtr x3".into(),
                description: "Antimatter の生産 3倍".into(),
                cost: 1_700_000_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::AntimatterCondenser,
                    multiplier: 3.0,
                },
                unlock_condition: Some((ProducerKind::AntimatterCondenser, 10)),
                exclusive_group: None,
            },
            // === New producer synergy upgrades ===
            Upgrade {
                name: "神殿の祝福".into(),
                description: "Temple1台→Factory+4%".into(),
                cost: 7_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Temple,
                    target: ProducerKind::Factory,
                    bonus_per_unit: 0.04,
                },
                unlock_condition: Some((ProducerKind::Temple, 5)),
                exclusive_group: None,
            },
            Upgrade {
                name: "魔法の加速".into(),
                description: "WzTower1台→Temple+3%".into(),
                cost: 100_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::WizardTower,
                    target: ProducerKind::Temple,
                    bonus_per_unit: 0.03,
                },
                unlock_condition: Some((ProducerKind::WizardTower, 5)),
                exclusive_group: None,
            },
            Upgrade {
                name: "星間輸送網".into(),
                description: "Shipment1台→WzTower+2%".into(),
                cost: 1_650_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CrossSynergy {
                    source: ProducerKind::Shipment,
                    target: ProducerKind::WizardTower,
                    bonus_per_unit: 0.02,
                },
                unlock_condition: Some((ProducerKind::Shipment, 5)),
                exclusive_group: None,
            },
            // === New producer count scaling ===
            Upgrade {
                name: "Templeの結束".into(),
                description: "各Temple毎に全Temple+2%".into(),
                cost: 500_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Temple,
                    bonus_per_unit: 0.02,
                },
                unlock_condition: Some((ProducerKind::Temple, 15)),
                exclusive_group: None,
            },
            Upgrade {
                name: "WzTowerの結束".into(),
                description: "各WzTower毎に全WzTower+2.5%".into(),
                cost: 5_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::WizardTower,
                    bonus_per_unit: 0.025,
                },
                unlock_condition: Some((ProducerKind::WizardTower, 15)),
                exclusive_group: None,
            },
            Upgrade {
                name: "Shipmentの結束".into(),
                description: "各Shipment毎に全Shipment+3%".into(),
                cost: 50_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::CountScaling {
                    target: ProducerKind::Shipment,
                    bonus_per_unit: 0.03,
                },
                unlock_condition: Some((ProducerKind::Shipment, 15)),
                exclusive_group: None,
            },
            // === Kitten upgrades (scale with milk from milestones) ===
            Upgrade {
                name: "子猫の手伝い".into(),
                description: "ミルク×5%のCPSボーナス".into(),
                cost: 9_000.0,
                purchased: false,
                effect: UpgradeEffect::KittenBoost { multiplier: 0.05 },
                unlock_condition: None, // unlocked by milk check in logic
                exclusive_group: None,
            },
            Upgrade {
                name: "子猫の労働者".into(),
                description: "ミルク×10%のCPSボーナス".into(),
                cost: 900_000.0,
                purchased: false,
                effect: UpgradeEffect::KittenBoost { multiplier: 0.10 },
                unlock_condition: None,
                exclusive_group: None,
            },
            Upgrade {
                name: "子猫のエンジニア".into(),
                description: "ミルク×20%のCPSボーナス".into(),
                cost: 90_000_000.0,
                purchased: false,
                effect: UpgradeEffect::KittenBoost { multiplier: 0.20 },
                unlock_condition: None,
                exclusive_group: None,
            },
            Upgrade {
                name: "子猫のマネージャー".into(),
                description: "ミルク×30%のCPSボーナス".into(),
                cost: 9_000_000_000.0,
                purchased: false,
                effect: UpgradeEffect::KittenBoost { multiplier: 0.30 },
                unlock_condition: None,
                exclusive_group: None,
            },
            // ═══════════════════════════════════════════════════════
            // Exclusive Choice Upgrades — pick ONE per group
            // These create "which path?" dilemmas at key progression points
            // ═══════════════════════════════════════════════════════

            // --- Group 1: Grandma specialization (unlocks at 10 Grandma) ---
            Upgrade {
                name: "おばあちゃん特化".into(),
                description: "Grandma×5倍！(他の低Tier無効)".into(),
                cost: 5_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Grandma,
                    multiplier: 5.0,
                },
                unlock_condition: Some((ProducerKind::Grandma, 10)),
                exclusive_group: Some(1),
            },
            Upgrade {
                name: "万能おばあちゃん".into(),
                description: "全生産者×1.3倍(広く薄く)".into(),
                cost: 5_000.0,
                purchased: false,
                effect: UpgradeEffect::CpsPercentBonus {
                    target: ProducerKind::Grandma,
                    percentage: 0.002,
                },
                unlock_condition: Some((ProducerKind::Grandma, 10)),
                exclusive_group: Some(1),
            },
            // --- Group 2: Mine vs Farm focus (unlocks at 10 Mine) ---
            Upgrade {
                name: "深層採掘".into(),
                description: "Mine×4倍＋コスト上昇緩和".into(),
                cost: 500_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Mine,
                    multiplier: 4.0,
                },
                unlock_condition: Some((ProducerKind::Mine, 10)),
                exclusive_group: Some(2),
            },
            Upgrade {
                name: "有機農法".into(),
                description: "Farm×6倍(安い方を極める)".into(),
                cost: 500_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Farm,
                    multiplier: 6.0,
                },
                unlock_condition: Some((ProducerKind::Mine, 10)),
                exclusive_group: Some(2),
            },
            // --- Group 3: Click vs Idle (unlocks at 100 total clicks) ---
            Upgrade {
                name: "クリック狂戦士".into(),
                description: "クリック+10, CPS-20%".into(),
                cost: 200_000.0,
                purchased: false,
                effect: UpgradeEffect::ClickPower(10.0),
                unlock_condition: Some((ProducerKind::Cursor, 30)),
                exclusive_group: Some(3),
            },
            Upgrade {
                name: "放置の達人".into(),
                description: "CPS×1.5倍(クリック変化なし)".into(),
                cost: 200_000.0,
                purchased: false,
                effect: UpgradeEffect::SynergyBoost {
                    target: ProducerKind::Factory,
                },
                unlock_condition: Some((ProducerKind::Cursor, 30)),
                exclusive_group: Some(3),
            },
            // --- Group 4: Late-game specialization (unlocks at 5 Temple) ---
            Upgrade {
                name: "神殿の集中".into(),
                description: "Temple×8倍(Temple特化)".into(),
                cost: 50_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Temple,
                    multiplier: 8.0,
                },
                unlock_condition: Some((ProducerKind::Temple, 5)),
                exclusive_group: Some(4),
            },
            Upgrade {
                name: "産業革命".into(),
                description: "Factory×8倍(Factory特化)".into(),
                cost: 50_000_000.0,
                purchased: false,
                effect: UpgradeEffect::ProducerMultiplier {
                    target: ProducerKind::Factory,
                    multiplier: 8.0,
                },
                unlock_condition: Some((ProducerKind::Temple, 5)),
                exclusive_group: Some(4),
            },
        ]
    }

    pub fn create_milestones() -> Vec<Milestone> {
        vec![
            // === Cookie milestones ===
            Milestone {
                name: "はじめの一歩".into(),
                description: "クッキーを100枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(100.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "駆け出しベイカー".into(),
                description: "クッキーを1,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(1_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "パン屋の朝".into(),
                description: "クッキーを10,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(10_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "繁盛店".into(),
                description: "クッキーを100,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(100_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー長者".into(),
                description: "クッキーを1,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(1_000_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー大王".into(),
                description: "クッキーを10,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(10_000_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー財閥".into(),
                description: "クッキーを100,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(100_000_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー王朝".into(),
                description: "クッキーを1,000,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(1_000_000_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー帝国".into(),
                description: "クッキーを10,000,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(10_000_000_000.0),
                status: MilestoneStatus::Locked,
            },
            // === Click milestones ===
            Milestone {
                name: "クリッカー".into(),
                description: "100回クリック".into(),
                condition: MilestoneCondition::TotalClicks(100),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "連打の達人".into(),
                description: "1,000回クリック".into(),
                condition: MilestoneCondition::TotalClicks(1_000),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "指が止まらない".into(),
                description: "10,000回クリック".into(),
                condition: MilestoneCondition::TotalClicks(10_000),
                status: MilestoneStatus::Locked,
            },
            // === CPS milestones ===
            Milestone {
                name: "自動化の兆し".into(),
                description: "CPS 10 達成".into(),
                condition: MilestoneCondition::CpsReached(10.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "小さな工場".into(),
                description: "CPS 100 達成".into(),
                condition: MilestoneCondition::CpsReached(100.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "工業化の波".into(),
                description: "CPS 500 達成".into(),
                condition: MilestoneCondition::CpsReached(500.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "産業革命".into(),
                description: "CPS 1,000 達成".into(),
                condition: MilestoneCondition::CpsReached(1_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "大量生産".into(),
                description: "CPS 5,000 達成".into(),
                condition: MilestoneCondition::CpsReached(5_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー王国".into(),
                description: "CPS 10,000 達成".into(),
                condition: MilestoneCondition::CpsReached(10_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "無限の生産力".into(),
                description: "CPS 100,000 達成".into(),
                condition: MilestoneCondition::CpsReached(100_000.0),
                status: MilestoneStatus::Locked,
            },
            // === Producer milestones ===
            Milestone {
                name: "Cursorコレクター".into(),
                description: "Cursor 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Cursor, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "Cursor軍団".into(),
                description: "Cursor 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Cursor, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "Cursorの海".into(),
                description: "Cursor 100台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Cursor, 100),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "おばあちゃんの集い".into(),
                description: "Grandma 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Grandma, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "おばあちゃんの楽園".into(),
                description: "Grandma 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Grandma, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "農場主".into(),
                description: "Farm 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Farm, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "大農場経営".into(),
                description: "Farm 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Farm, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "鉱山王".into(),
                description: "Mine 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Mine, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "深層採掘".into(),
                description: "Mine 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Mine, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "工場長".into(),
                description: "Factory 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Factory, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "産業コンツェルン".into(),
                description: "Factory 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Factory, 50),
                status: MilestoneStatus::Locked,
            },
            // === New producer milestones ===
            Milestone {
                name: "神官".into(),
                description: "Temple 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Temple, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "大神殿".into(),
                description: "Temple 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Temple, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "魔法使い".into(),
                description: "WzTower 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::WizardTower, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "大魔導師".into(),
                description: "WzTower 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::WizardTower, 50),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "宇宙輸送".into(),
                description: "Shipment 10台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Shipment, 10),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "銀河帝国".into(),
                description: "Shipment 50台".into(),
                condition: MilestoneCondition::ProducerCount(ProducerKind::Shipment, 50),
                status: MilestoneStatus::Locked,
            },
            // === Higher CPS milestones ===
            Milestone {
                name: "クッキー銀河".into(),
                description: "CPS 1,000,000 達成".into(),
                condition: MilestoneCondition::CpsReached(1_000_000.0),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "クッキー宇宙".into(),
                description: "CPS 100,000,000 達成".into(),
                condition: MilestoneCondition::CpsReached(100_000_000.0),
                status: MilestoneStatus::Locked,
            },
            // === Higher cookie milestones ===
            Milestone {
                name: "兆の壁".into(),
                description: "クッキーを1,000,000,000,000枚焼く".into(),
                condition: MilestoneCondition::TotalCookies(1_000_000_000_000.0),
                status: MilestoneStatus::Locked,
            },
            // === Golden cookie milestones ===
            Milestone {
                name: "幸運の始まり".into(),
                description: "ゴールデンクッキーを5回取得".into(),
                condition: MilestoneCondition::GoldenClaimed(5),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "ゴールドハンター".into(),
                description: "ゴールデンクッキーを25回取得".into(),
                condition: MilestoneCondition::GoldenClaimed(25),
                status: MilestoneStatus::Locked,
            },
            Milestone {
                name: "ゴールデンマスター".into(),
                description: "ゴールデンクッキーを77回取得".into(),
                condition: MilestoneCondition::GoldenClaimed(77),
                status: MilestoneStatus::Locked,
            },
        ]
    }

    fn create_prestige_upgrades() -> Vec<PrestigeUpgrade> {
        vec![
            // ═══════════════════════════════════════════════════════
            // Root — すべてのパスの前提
            // ═══════════════════════════════════════════════════════
            PrestigeUpgrade {
                id: "angels_gift",
                name: "天使の贈り物".into(),
                description: "転生後 10,000 クッキーで開始".into(),
                cost: 1,
                purchased: false,
                effect: PrestigeEffect::StartingCookies(10_000.0),
                requires: None,
                path: PrestigePath::Root,
            },
            // ═══════════════════════════════════════════════════════
            // 生産パス — 放置向け、CPS強化
            // ═══════════════════════════════════════════════════════
            PrestigeUpgrade {
                id: "heavenly_power",
                name: "天界の力".into(),
                description: "CPS 永続 ×1.5".into(),
                cost: 5,
                purchased: false,
                effect: PrestigeEffect::CpsMultiplier(1.5),
                requires: Some("angels_gift"),
                path: PrestigePath::Production,
            },
            PrestigeUpgrade {
                id: "angels_aura",
                name: "天使のオーラ".into(),
                description: "CPS 永続 ×2".into(),
                cost: 25,
                purchased: false,
                effect: PrestigeEffect::CpsMultiplier(2.0),
                requires: Some("heavenly_power"),
                path: PrestigePath::Production,
            },
            PrestigeUpgrade {
                id: "factory_memory",
                name: "工場の記憶".into(),
                description: "転生後 Cursor 500台で開始".into(),
                cost: 125,
                purchased: false,
                effect: PrestigeEffect::StartingCursors(500),
                requires: Some("angels_aura"),
                path: PrestigePath::Production,
            },
            PrestigeUpgrade {
                id: "efficiency_peak",
                name: "効率の極致".into(),
                description: "全生産者コスト -25%".into(),
                cost: 500,
                purchased: false,
                effect: PrestigeEffect::ProducerCostReduction(0.25),
                requires: Some("factory_memory"),
                path: PrestigePath::Production,
            },
            PrestigeUpgrade {
                id: "heavenly_wealth",
                name: "天界の富".into(),
                description: "転生後 100億クッキーで開始".into(),
                cost: 2000,
                purchased: false,
                effect: PrestigeEffect::StartingCookies(10_000_000_000.0),
                requires: Some("efficiency_peak"),
                path: PrestigePath::Production,
            },
            // ═══════════════════════════════════════════════════════
            // クリックパス — アクティブ向け、クリック強化
            // ═══════════════════════════════════════════════════════
            PrestigeUpgrade {
                id: "angels_click",
                name: "天使のクリック".into(),
                description: "クリック力 永続 ×2".into(),
                cost: 5,
                purchased: false,
                effect: PrestigeEffect::ClickMultiplier(2.0),
                requires: Some("angels_gift"),
                path: PrestigePath::Click,
            },
            PrestigeUpgrade {
                id: "gods_click",
                name: "神のクリック".into(),
                description: "クリック力 永続 ×3".into(),
                cost: 25,
                purchased: false,
                effect: PrestigeEffect::ClickMultiplier(3.0),
                requires: Some("angels_click"),
                path: PrestigePath::Click,
            },
            PrestigeUpgrade {
                id: "sugar_alchemy",
                name: "砂糖錬金術".into(),
                description: "砂糖ブースト効果 +50%".into(),
                cost: 125,
                purchased: false,
                effect: PrestigeEffect::SugarBoostMultiplier(1.5),
                requires: Some("gods_click"),
                path: PrestigePath::Click,
            },
            PrestigeUpgrade {
                id: "combo_mastery",
                name: "連撃の極意".into(),
                description: "クリック力 永続 ×2".into(),
                cost: 500,
                purchased: false,
                effect: PrestigeEffect::ClickMultiplier(2.0),
                requires: Some("sugar_alchemy"),
                path: PrestigePath::Click,
            },
            PrestigeUpgrade {
                id: "click_sovereign",
                name: "クリックの覇者".into(),
                description: "クリック力 永続 ×25".into(),
                cost: 2000,
                purchased: false,
                effect: PrestigeEffect::ClickMultiplier(25.0),
                requires: Some("combo_mastery"),
                path: PrestigePath::Click,
            },
            // ═══════════════════════════════════════════════════════
            // 幸運パス — イベント向け、ゴールデン強化
            // ═══════════════════════════════════════════════════════
            PrestigeUpgrade {
                id: "golden_rush",
                name: "ゴールデンラッシュ".into(),
                description: "ゴールデンクッキー出現 1.5倍速".into(),
                cost: 5,
                purchased: false,
                effect: PrestigeEffect::GoldenCookieSpeed(0.67),
                requires: Some("angels_gift"),
                path: PrestigePath::Luck,
            },
            PrestigeUpgrade {
                id: "golden_intuition",
                name: "黄金の直感".into(),
                description: "ゴールデン効果時間 +30%".into(),
                cost: 25,
                purchased: false,
                effect: PrestigeEffect::GoldenDuration(1.3),
                requires: Some("golden_rush"),
                path: PrestigePath::Luck,
            },
            PrestigeUpgrade {
                id: "luck_extension",
                name: "幸運の延長".into(),
                description: "ゴールデン効果時間 +50%".into(),
                cost: 125,
                purchased: false,
                effect: PrestigeEffect::GoldenDuration(1.5),
                requires: Some("golden_intuition"),
                path: PrestigePath::Luck,
            },
            PrestigeUpgrade {
                id: "milk_memory",
                name: "ミルクの記憶".into(),
                description: "転生後にミルクを50%保持".into(),
                cost: 500,
                purchased: false,
                effect: PrestigeEffect::MilkRetention(0.5),
                requires: Some("luck_extension"),
                path: PrestigePath::Luck,
            },
            PrestigeUpgrade {
                id: "luck_sovereign",
                name: "幸運の支配者".into(),
                description: "ゴールデン効果 ×2".into(),
                cost: 2000,
                purchased: false,
                effect: PrestigeEffect::GoldenEffectMultiplier(2.0),
                requires: Some("milk_memory"),
                path: PrestigePath::Luck,
            },
        ]
    }

    pub fn create_research_nodes() -> Vec<ResearchNode> {
        vec![
            // === Path A: 量産路線 (Mass Production) ===
            ResearchNode {
                name: "効率生産".into(),
                description: "全生産者コスト -15%".into(),
                cost: 10_000.0,
                tier: 1,
                path: ResearchPath::MassProduction,
                purchased: false,
                effect: ResearchEffect::CostReduction(0.15),
            },
            ResearchNode {
                name: "大量発注".into(),
                description: "全生産者 CPS ×2".into(),
                cost: 500_000.0,
                tier: 2,
                path: ResearchPath::MassProduction,
                purchased: false,
                effect: ResearchEffect::CpsMultiplier(2.0),
            },
            ResearchNode {
                name: "規模の経済".into(),
                description: "台数ボーナス効果 ×2".into(),
                cost: 5_000_000.0,
                tier: 3,
                path: ResearchPath::MassProduction,
                purchased: false,
                effect: ResearchEffect::CountScalingMultiplier(2.0),
            },
            ResearchNode {
                name: "産業帝国".into(),
                description: "コスト -30%, CPS ×3".into(),
                cost: 50_000_000.0,
                tier: 4,
                path: ResearchPath::MassProduction,
                purchased: false,
                effect: ResearchEffect::CpsMultiplier(3.0),
            },
            // === Path B: 品質路線 (Quality) ===
            ResearchNode {
                name: "熟練の技".into(),
                description: "クリック力 += CPS×1%".into(),
                cost: 10_000.0,
                tier: 1,
                path: ResearchPath::Quality,
                purchased: false,
                effect: ResearchEffect::ClickCpsPercent(0.01),
            },
            ResearchNode {
                name: "黄金の時".into(),
                description: "ゴールデンバフ時間 ×2".into(),
                cost: 500_000.0,
                tier: 2,
                path: ResearchPath::Quality,
                purchased: false,
                effect: ResearchEffect::BuffDuration(2.0),
            },
            ResearchNode {
                name: "共鳴増幅".into(),
                description: "シナジー効果 ×2".into(),
                cost: 5_000_000.0,
                tier: 3,
                path: ResearchPath::Quality,
                purchased: false,
                effect: ResearchEffect::SynergyMultiplier(2.0),
            },
            ResearchNode {
                name: "極致の道".into(),
                description: "クリック += CPS×5%, バフ ×1.5".into(),
                cost: 50_000_000.0,
                tier: 4,
                path: ResearchPath::Quality,
                purchased: false,
                effect: ResearchEffect::BuffEffectMultiplier(1.5),
            },
        ]
    }

    // === Research helper methods ===

    /// Total cost reduction from research (multiplicative, e.g. 0.85 = 15% off).
    pub fn research_cost_modifier(&self) -> f64 {
        let mut modifier = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::CostReduction(reduction) = &node.effect {
                    modifier *= 1.0 - reduction;
                }
            }
        }
        // Tier 4 of mass production also has cost reduction (-30%)
        if self.research_path == ResearchPath::MassProduction {
            if let Some(node) = self.research_nodes.iter().find(|n| n.name == "産業帝国") {
                if node.purchased {
                    modifier *= 0.7; // additional -30%
                }
            }
        }
        modifier
    }

    /// Total CPS multiplier from research.
    pub fn research_cps_modifier(&self) -> f64 {
        let mut mult = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::CpsMultiplier(m) = &node.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Click bonus from research: adds CPS × percentage to click power.
    pub fn research_click_cps_bonus(&self) -> f64 {
        let mut pct = 0.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::ClickCpsPercent(p) = &node.effect {
                    pct += p;
                }
            }
        }
        pct
    }

    /// Buff duration multiplier from research.
    pub fn research_buff_duration(&self) -> f64 {
        let mut mult = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::BuffDuration(m) = &node.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Buff duration multiplier from prestige upgrades.
    pub fn prestige_buff_duration(&self) -> f64 {
        let mut mult = 1.0;
        for upgrade in &self.prestige_upgrades {
            if upgrade.purchased {
                if let PrestigeEffect::GoldenDuration(m) = &upgrade.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Golden effect multiplier from prestige upgrades.
    pub fn prestige_golden_effect_multiplier(&self) -> f64 {
        let mut mult = 1.0;
        for upgrade in &self.prestige_upgrades {
            if upgrade.purchased {
                if let PrestigeEffect::GoldenEffectMultiplier(m) = &upgrade.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Total buff duration multiplier (research + prestige).
    pub fn total_buff_duration(&self) -> f64 {
        self.research_buff_duration() * self.prestige_buff_duration()
    }

    /// Sugar boost effectiveness multiplier from prestige upgrades.
    pub fn prestige_sugar_boost_multiplier(&self) -> f64 {
        let mut mult = 1.0;
        for upgrade in &self.prestige_upgrades {
            if upgrade.purchased {
                if let PrestigeEffect::SugarBoostMultiplier(m) = &upgrade.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Current sugar boost CPS multiplier (1.0 if no boost active).
    pub fn sugar_boost_multiplier(&self) -> f64 {
        if let Some(ref boost) = self.active_sugar_boost {
            boost.kind.multiplier() * self.prestige_sugar_boost_multiplier()
        } else {
            1.0
        }
    }

    /// Whether auto-clicker is unlocked (prestige >= 1).
    pub fn is_auto_clicker_unlocked(&self) -> bool {
        self.prestige_count >= 1
    }

    /// Auto-clicker rate (clicks per second).
    /// Returns 1 at prestige 1-9, 5 at prestige 10+.
    pub fn auto_clicker_rate(&self) -> u32 {
        if self.prestige_count >= 10 {
            5
        } else {
            1
        }
    }

    /// Ticks between auto-clicks.
    pub fn auto_clicker_interval(&self) -> u32 {
        10 / self.auto_clicker_rate() // 10 ticks/sec ÷ rate
    }

    /// Synergy multiplier from research.
    pub fn research_synergy_modifier(&self) -> f64 {
        let mut mult = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::SynergyMultiplier(m) = &node.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Count scaling multiplier from research.
    pub fn research_count_scaling_modifier(&self) -> f64 {
        let mut mult = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::CountScalingMultiplier(m) = &node.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Buff effect multiplier from research (multiplies buff values).
    pub fn research_buff_effect_modifier(&self) -> f64 {
        let mut mult = 1.0;
        for node in &self.research_nodes {
            if node.purchased {
                if let ResearchEffect::BuffEffectMultiplier(m) = &node.effect {
                    mult *= m;
                }
            }
        }
        mult
    }

    /// Max research tier purchased on current path.
    pub fn research_max_tier(&self) -> u8 {
        self.research_nodes
            .iter()
            .filter(|n| n.purchased)
            .map(|n| n.tier)
            .max()
            .unwrap_or(0)
    }

    // === Dragon helper methods ===

    /// Producers needed to reach next dragon level.
    pub fn dragon_feed_cost(&self) -> u32 {
        if self.dragon_level >= 7 {
            return 0; // max level
        }
        match self.dragon_level {
            0 => 10,
            1 => 25,
            2 => 50,
            3 => 100,
            4 => 200,
            5 => 400,
            6 => 800,
            _ => 0,
        }
    }

    /// Total producers already fed toward next level.
    pub fn dragon_fed_toward_next(&self) -> u32 {
        let total_needed_for_current: u32 = (0..self.dragon_level)
            .map(|l| match l {
                0 => 10,
                1 => 25,
                2 => 50,
                3 => 100,
                4 => 200,
                5 => 400,
                6 => 800,
                _ => 0,
            })
            .sum();
        self.dragon_fed_total.saturating_sub(total_needed_for_current)
    }

    /// CPS multiplier from dragon aura.
    pub fn dragon_cps_modifier(&self) -> f64 {
        if self.dragon_level == 0 {
            return 1.0;
        }
        match self.dragon_aura {
            DragonAura::BreathOfRiches => 1.0 + 0.15 * self.dragon_level as f64,
            _ => 1.0,
        }
    }

    /// Click power multiplier from dragon aura.
    pub fn dragon_click_modifier(&self) -> f64 {
        if self.dragon_level == 0 {
            return 1.0;
        }
        match self.dragon_aura {
            DragonAura::DragonCursor => 1.0 + 0.20 * self.dragon_level as f64,
            _ => 1.0,
        }
    }

    /// Cost reduction from dragon aura (multiplicative).
    pub fn dragon_cost_modifier(&self) -> f64 {
        if self.dragon_level == 0 {
            return 1.0;
        }
        match self.dragon_aura {
            DragonAura::ElderPact => (1.0 - 0.05 * self.dragon_level as f64).max(0.3),
            _ => 1.0,
        }
    }

    /// Golden cookie spawn speed modifier from dragon aura (< 1.0 = faster).
    pub fn dragon_golden_speed(&self) -> f64 {
        if self.dragon_level == 0 {
            return 1.0;
        }
        match self.dragon_aura {
            DragonAura::DragonHarvest => (1.0 - 0.10 * self.dragon_level as f64).max(0.3),
            _ => 1.0,
        }
    }

    // === Market helper ===

    /// Combined cost modifier from market, research, dragon, and discount.
    /// Cost reduction from prestige upgrades (e.g. 0.1 = 10% off).
    pub fn prestige_cost_reduction(&self) -> f64 {
        self.prestige_upgrades
            .iter()
            .filter(|u| u.purchased)
            .filter_map(|u| {
                if let PrestigeEffect::ProducerCostReduction(pct) = &u.effect {
                    Some(*pct)
                } else {
                    None
                }
            })
            .sum()
    }

    pub fn total_cost_modifier(&self) -> f64 {
        let market = self.market_phase.cost_multiplier();
        let research = self.research_cost_modifier();
        let dragon = self.dragon_cost_modifier();
        let discount = 1.0 - self.active_discount;
        let prestige = 1.0 - self.prestige_cost_reduction();
        market * research * dragon * discount * prestige
    }

    /// Available heavenly chips (earned - spent).
    pub fn available_chips(&self) -> u64 {
        self.heavenly_chips.saturating_sub(self.heavenly_chips_spent)
    }

    /// Calculate how many new heavenly chips would be earned from current run.
    /// Threshold: 10億 (1e9) cookies per chip² — inspired by Cookie Clicker's
    /// prestige scaling.  First chip requires ~10億 cookies, preventing one-reset
    /// full-unlock and encouraging multiple prestige cycles.
    pub fn pending_heavenly_chips(&self) -> u64 {
        let total = self.cookies_all_runs + self.cookies_all_time;
        let total_chips = (total / 1e9).sqrt().floor() as u64;
        total_chips.saturating_sub(self.heavenly_chips)
    }

    /// Count of claimed milestones (milk applied).
    pub fn achieved_milestone_count(&self) -> usize {
        self.milestones.iter().filter(|m| m.status == MilestoneStatus::Claimed).count()
    }

    /// Count of ready-to-claim milestones.
    pub fn ready_milestone_count(&self) -> usize {
        self.milestones.iter().filter(|m| m.status == MilestoneStatus::Ready).count()
    }

    /// Calculate the synergy bonus for a specific producer kind.
    /// Returns the total bonus as a fraction (e.g. 0.10 = +10%).
    pub fn synergy_bonus(&self, target: &ProducerKind) -> f64 {
        let mut bonus = 0.0;

        // Built-in synergies (always active, part of game design)
        // Circular synergy: each producer boosts the next in the chain
        let base_synergies: &[(ProducerKind, ProducerKind, f64)] = &[
            // (source, target, bonus_per_source_unit)
            (ProducerKind::Grandma, ProducerKind::Cursor, 0.01),               // +1% per Grandma
            (ProducerKind::Farm, ProducerKind::Grandma, 0.02),                 // +2% per Farm
            (ProducerKind::Mine, ProducerKind::Farm, 0.03),                    // +3% per Mine
            (ProducerKind::Factory, ProducerKind::Mine, 0.05),                 // +5% per Factory
            (ProducerKind::Temple, ProducerKind::Factory, 0.04),               // +4% per Temple
            (ProducerKind::WizardTower, ProducerKind::Temple, 0.03),           // +3% per WzTower
            (ProducerKind::Shipment, ProducerKind::WizardTower, 0.02),         // +2% per Shipment
            (ProducerKind::AlchemyLab, ProducerKind::Shipment, 0.015),         // +1.5% per Alchemy
            (ProducerKind::Portal, ProducerKind::AlchemyLab, 0.02),            // +2% per Portal
            (ProducerKind::TimeMachine, ProducerKind::Portal, 0.025),          // +2.5% per TimeMachine
            (ProducerKind::AntimatterCondenser, ProducerKind::TimeMachine, 0.03), // +3% per Antimatter
            (ProducerKind::Cursor, ProducerKind::AntimatterCondenser, 0.0005), // +0.05% per Cursor (closes loop)
            // Tree synergies: upper-tier producers boost specific lower-tier producers
            (ProducerKind::AlchemyLab, ProducerKind::Mine, 0.015),             // Alchemy → Mine +1.5%
            (ProducerKind::AlchemyLab, ProducerKind::Farm, 0.015),             // Alchemy → Farm +1.5%
            (ProducerKind::Portal, ProducerKind::Temple, 0.02),                // Portal → Temple +2%
            (ProducerKind::Portal, ProducerKind::WizardTower, 0.02),           // Portal → WzTower +2%
            (ProducerKind::TimeMachine, ProducerKind::Cursor, 0.01),           // TimeMachine → Cursor +1%
            (ProducerKind::TimeMachine, ProducerKind::Grandma, 0.01),          // TimeMachine → Grandma +1%
            (ProducerKind::TimeMachine, ProducerKind::Farm, 0.01),             // TimeMachine → Farm +1%
            (ProducerKind::AntimatterCondenser, ProducerKind::Factory, 0.025), // Antimatter → Factory +2.5%
            (ProducerKind::AntimatterCondenser, ProducerKind::Shipment, 0.025),// Antimatter → Shipment +2.5%
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
        let research_syn = self.research_synergy_modifier();
        let research_cs = self.research_count_scaling_modifier();

        // Step 1: base CPS with synergies + count scaling
        let base: f64 = self.producers.iter().map(|p| {
            let syn = self.synergy_bonus(&p.kind) * research_syn;
            let cs = self.count_scaling_bonus(&p.kind) * research_cs;
            p.cps_with_synergy(syn + cs)
        }).sum();

        // Step 2: CPS-percent bonuses (based on base total, to avoid infinite recursion)
        let extra = self.cps_percent_extra(base);
        let total = base + extra;

        // Step 3: Apply kitten (milk) multiplier
        let after_kitten = total * self.kitten_multiplier;

        // Step 3.5: Apply prestige multiplier
        let after_prestige = after_kitten * self.prestige_multiplier;

        // Step 4: Apply research CPS multiplier
        let after_research = after_prestige * self.research_cps_modifier();

        // Step 5: Apply dragon CPS aura
        let after_dragon = after_research * self.dragon_cps_modifier();

        // Step 6: Apply market phase
        let after_market = after_dragon * self.market_phase.cps_multiplier();

        // Step 7: Apply production frenzy buff (with research buff effect modifier)
        let buff_effect_mult = self.research_buff_effect_modifier();
        let mut multiplier = 1.0;
        for buff in &self.active_buffs {
            if let GoldenEffect::ProductionFrenzy { multiplier: m } = &buff.effect {
                let effective_m = 1.0 + (m - 1.0) * buff_effect_mult;
                multiplier *= effective_m;
            }
        }

        // Step 7.5: Buff combo bonus — stacking different buff types is rewarded
        let combo_mult = self.buff_combo_multiplier();

        // Step 8: Apply sugar boost
        let sugar_mult = self.sugar_boost_multiplier();

        // Step 9: Apply savings bonus (reward for holding cookies)
        let savings = self.savings_bonus();

        after_market * multiplier * combo_mult * sugar_mult * savings
    }

    /// Combo click multiplier: +2% per combo hit, max 3x at 100 combo.
    pub fn combo_click_multiplier(&self) -> f64 {
        1.0 + (self.combo_count as f64 * 0.02).min(2.0)
    }

    /// Critical click chance: 3% base + 0.1% per combo, max 15%.
    pub fn critical_chance(&self) -> f64 {
        (0.03 + self.combo_count as f64 * 0.001).min(0.15)
    }

    /// Savings bonus multiplier: holding more cookies gives a significant CPS bonus.
    ///
    /// Design: creates a "spend vs save" dilemma. Buying producers/upgrades
    /// costs cookies, which reduces this bonus. Players must weigh
    /// "is this purchase worth losing X% savings bonus?"
    ///
    /// +3% per order of magnitude of saved cookies (was 0.5%, now much stronger).
    /// At 1M cookies saved: +18%, at 1B: +27%, at 1T: +36%
    pub fn savings_bonus(&self) -> f64 {
        if self.cookies > 10.0 {
            1.0 + self.cookies.log10() * 0.03
        } else {
            1.0
        }
    }

    /// Calculate what the savings bonus would be after spending `amount` cookies.
    /// Used by render to show "Savings bonus: X% → Y%" on purchase buttons.
    pub fn savings_bonus_after_spend(&self, amount: f64) -> f64 {
        let remaining = (self.cookies - amount).max(0.0);
        if remaining > 10.0 {
            1.0 + remaining.log10() * 0.03
        } else {
            1.0
        }
    }

    /// Buff combo multiplier: having multiple DIFFERENT buff types active
    /// simultaneously gives a stacking bonus. This rewards skilled play
    /// (catching golden cookies while buffs are active).
    ///
    /// - 1 buff type: ×1.0 (no bonus)
    /// - 2 different buff types: ×1.5
    /// - 3 different buff types: ×3.0 (jackpot!)
    /// - Market Bubble/Crash also count as a "buff type" for combo purposes
    pub fn buff_combo_multiplier(&self) -> f64 {
        let mut distinct_types = 0u8;
        let mut has_production = false;
        let mut has_click = false;
        for buff in &self.active_buffs {
            match &buff.effect {
                GoldenEffect::ProductionFrenzy { .. } if !has_production => {
                    has_production = true;
                    distinct_types += 1;
                }
                GoldenEffect::ClickFrenzy { .. } if !has_click => {
                    has_click = true;
                    distinct_types += 1;
                }
                _ => {}
            }
        }
        if self.active_sugar_boost.is_some() {
            distinct_types += 1;
        }
        // Extreme market phases count as a combo element
        if matches!(self.market_phase, MarketPhase::Bubble | MarketPhase::Crash) {
            distinct_types += 1;
        }

        match distinct_types {
            0 | 1 => 1.0,
            2 => 1.5,
            3 => 3.0,
            _ => 5.0, // 4+ = legendary combo
        }
    }

    /// Whether any golden buff is active (for chain bonus check).
    pub fn has_active_golden_buff(&self) -> bool {
        self.active_buffs.iter().any(|b| {
            matches!(
                &b.effect,
                GoldenEffect::ProductionFrenzy { .. } | GoldenEffect::ClickFrenzy { .. }
            )
        })
    }

    /// Effective cookies per click (with buffs, research, dragon, combo).
    pub fn effective_click_power(&self) -> f64 {
        let mut power = self.cookies_per_click;

        // Research: add CPS-based click bonus
        let click_cps_pct = self.research_click_cps_bonus();
        if click_cps_pct > 0.0 {
            power += self.total_cps() * click_cps_pct;
        }

        // Dragon: click multiplier
        power *= self.dragon_click_modifier();

        // Combo multiplier
        power *= self.combo_click_multiplier();

        // Buffs: click frenzy (with research buff effect modifier)
        let buff_effect_mult = self.research_buff_effect_modifier();
        for buff in &self.active_buffs {
            if let GoldenEffect::ClickFrenzy { multiplier } = &buff.effect {
                let effective_m = 1.0 + (multiplier - 1.0) * buff_effect_mult;
                power *= effective_m;
            }
        }
        power
    }

    /// Check if an upgrade's unlock condition is met.
    pub fn is_upgrade_unlocked(&self, upgrade: &Upgrade) -> bool {
        // Kitten upgrades require minimum milk level
        if let UpgradeEffect::KittenBoost { multiplier } = &upgrade.effect {
            let required_milk = match *multiplier as u32 {
                0 => 0.10,  // 5% → 10% milk (≈3 milestones)
                _ => *multiplier * 2.0, // 10%→20% milk, 20%→40%, 30%→60%
            };
            // First kitten is always available if milk > 0
            let req = if *multiplier <= 0.05 { 0.0 } else { required_milk };
            if self.milk < req {
                return false;
            }
        }
        match &upgrade.unlock_condition {
            None => true,
            Some((kind, required_count)) => {
                self.producers[kind.index()].count >= *required_count
            }
        }
    }

    /// Get CPS contribution of each producer as (name, cps, fraction_of_total).
    pub fn producer_contributions(&self) -> Vec<(&str, f64, f64)> {
        let total = self.total_cps().max(0.001);
        self.producers
            .iter()
            .filter(|p| p.count > 0)
            .map(|p| {
                let syn = self.synergy_bonus(&p.kind);
                let cs = self.count_scaling_bonus(&p.kind);
                let cps = p.cps_with_synergy(syn + cs);
                (p.kind.name(), cps, cps / total)
            })
            .collect()
    }

    // ── Shared filter methods (used by both render and input) ──

    /// Unpurchased upgrades with their real index.
    /// Both render (to display) and input (to map display_idx→real_idx) use this.
    pub fn available_upgrades(&self) -> Vec<usize> {
        self.upgrades
            .iter()
            .enumerate()
            .filter(|(_, u)| !u.purchased)
            .map(|(i, _)| i)
            .collect()
    }

    /// Visible (unpurchased, path-matching) research nodes with their real index.
    pub fn visible_research(&self) -> Vec<usize> {
        self.research_nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                if self.research_path != ResearchPath::None
                    && n.path != self.research_path
                {
                    return false;
                }
                !n.purchased
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Ready-to-claim milestones with their real index.
    pub fn ready_milestones(&self) -> Vec<usize> {
        self.milestones
            .iter()
            .enumerate()
            .filter(|(_, m)| m.status == MilestoneStatus::Ready)
            .map(|(i, _)| i)
            .collect()
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
        // Cursor has cost_growth_rate 1.20
        let mut p = Producer::new(ProducerKind::Cursor);
        p.count = 1;
        let expected = 15.0 * 1.20;
        assert!((p.cost() - expected).abs() < 0.01);

        p.count = 10;
        let expected = 15.0 * 1.20_f64.powi(10);
        assert!((p.cost() - expected).abs() < 0.1);

        // Factory has cost_growth_rate 1.15
        let mut p2 = Producer::new(ProducerKind::Factory);
        p2.count = 5;
        let expected2 = ProducerKind::Factory.base_cost() * 1.15_f64.powi(5);
        assert!((p2.cost() - expected2).abs() < 0.1);

        // AntimatterCondenser has cost_growth_rate 1.08
        let mut p3 = Producer::new(ProducerKind::AntimatterCondenser);
        p3.count = 5;
        let expected3 = ProducerKind::AntimatterCondenser.base_cost() * 1.08_f64.powi(5);
        assert!((p3.cost() - expected3).abs() < 1.0);
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
        state.producers[5].count = 10;  // 10 temples → Factory +4% each = +40%
        state.producers[4].count = 5;   // 5 factories → Mine +5% each = +25%
        state.producers[8].count = 10;  // 10 alchemy labs → Shipment +1.5% each = +15%
        state.producers[0].count = 100; // 100 cursors → Antimatter +0.05% each = +5%
        let factory_bonus = state.synergy_bonus(&ProducerKind::Factory);
        assert!((factory_bonus - 0.40).abs() < 0.01); // 10 * 0.04 = 0.40
        let mine_bonus = state.synergy_bonus(&ProducerKind::Mine);
        // Mine gets +25% from Factory (circular) + 15% from AlchemyLab (tree) = +40%
        assert!((mine_bonus - 0.40).abs() < 0.01);
        let shipment_bonus = state.synergy_bonus(&ProducerKind::Shipment);
        assert!((shipment_bonus - 0.15).abs() < 0.01); // 10 * 0.015 = 0.15
        let antimatter_bonus = state.synergy_bonus(&ProducerKind::AntimatterCondenser);
        assert!((antimatter_bonus - 0.05).abs() < 0.01); // 100 * 0.0005 = 0.05
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
