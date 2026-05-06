//! City state for Idle Metropolis.
//!
//! AI-driven idle city builder.  The player only sets a strategy and buys
//! upgrades; the CPU does all placement.  At low Tiers the CPU is dumb on
//! purpose, so we need a balance simulator (see `simulator.rs`) to confirm
//! the game is still progressing.

pub const GRID_W: usize = 32;
pub const GRID_H: usize = 16;

pub const TICKS_PER_SEC: u32 = 10;

/// Hard cap on parallel workers.  Doubling cost (`100 << (workers - 1)`)
/// stays comfortably within i64 well below this limit; the cap also keeps
/// gameplay bounded.
pub const MAX_WORKERS: u32 = 8;

/// What occupies a single map cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    Empty,
    /// 整地中。Wasteland / Forest の地形を Plain 化する工程。
    /// 完了すると下層の `terrain` が Plain に書き換わり、再び Empty タイルに戻る。
    /// 続けて何を建てるかは AI が次の tick で決める設計 (= 建物自由度を保つ)。
    Clearing { ticks_remaining: u32 },
    /// Construction in progress: target building, ticks remaining.
    Construction {
        target: Building,
        ticks_remaining: u32,
    },
    Built(Building),
}

/// Buildings the AI can place.
///
/// **経済チェーン**: Road (インフラ) → House (人口) → Workshop (生産) →
/// Shop (販売)。Workshop は House と Shop の中間層として機能し、隣接
/// House の住民を雇って稼ぐ。Workshop が近くにあると House は Apartment に
/// 育つ (`logic::house_tier_for` の判定で `n_workshop_within_5` が寄与)。
///
/// `Park` は経済チェーンと並行する「文化レイヤー」: 直接の収入は無く、
/// 周囲の House を Apartment / Highrise に育てる触媒として機能する
/// (`logic::house_tier_for` で `n_park_within_4` が寄与)。緑地保護派の
/// Eco 戦略 + 高級住宅街を狙う Tier 4 プレイで真価を発揮する。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Building {
    /// Connector: enables shops to be supplied.
    Road,
    /// Adds population.
    House,
    /// 工房。隣接 House (労働力) と Road 接続が必要。Shop より早期に開けて
    /// 「家 → 職場」の経済段階を担当する。
    Workshop,
    /// Generates cash, but only if it has at least one road neighbor AND
    /// at least one house within Manhattan distance 3 (a "customer base").
    Shop,
    /// 公園。直接の収入は無いが、周囲 4 マス以内の House を Apartment 化
    /// する経済刺激源として機能する (Workshop / Shop と同等の Tier 上昇寄与)。
    /// 道路接続不要 — 緑地として独立配置可能。
    Park,
    /// **開拓機材** — 隣接する Rock セルの整地を可能にする特殊建物。
    /// プレイヤーが手動で配置する想定 (AI は手を出さない)。
    /// 高価 ($600)、長時間建設 (300 ticks = 30 sec)。
    /// 一度設置すると周囲 4-近傍の Rock を順次破砕できるようになる。
    /// 撤去コストは「中央からの距離 × 100」(外側ほど高い)。
    Outpost,
}

impl Building {
    /// One-time build cost in cash.
    ///
    /// バランス: Workshop は Shop より安く ($100 vs $150) 早期に開ける。
    /// Park は安め ($80) で「街並み演出」として気軽に置けるが、
    /// 直接収入が無い分、純粋投資としては効率が悪い (= Highrise 化の触媒専用)。
    pub fn cost(self) -> i64 {
        match self {
            Building::Road => 10,
            Building::House => 40,
            Building::Park => 80,
            Building::Workshop => 100,
            Building::Shop => 150,
            Building::Outpost => 600,
        }
    }

    /// Ticks needed to finish construction.
    pub fn build_ticks(self) -> u32 {
        match self {
            Building::Road => 30,        // 3 sec
            Building::House => 100,      // 10 sec
            Building::Park => 80,        // 8 sec — 短め (整地+植栽だけ)
            Building::Workshop => 150,   // 15 sec — Shop より少し短い
            Building::Shop => 200,       // 20 sec
            Building::Outpost => 300,    // 30 sec — 重機の搬入と組立
        }
    }

}

/// 街の発展段階。人口で自動的に判定される (純関数)。
///
/// ティア進化はバナー表示と完成イベントログの主要な「自慢ポイント」。
/// プレイヤーが「次の段階まで pop X」を意識する見出しになる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CityTier {
    Village,
    Town,
    City,
    Metropolis,
}

impl CityTier {
    pub fn name(self) -> &'static str {
        match self {
            CityTier::Village => "Village",
            CityTier::Town => "Town",
            CityTier::City => "City",
            CityTier::Metropolis => "Metropolis",
        }
    }
    pub fn jp(self) -> &'static str {
        match self {
            CityTier::Village => "村",
            CityTier::Town => "町",
            CityTier::City => "市",
            CityTier::Metropolis => "大都市",
        }
    }
}

/// 人口からティアを決定する純関数。
///
/// **TODO (バランス調整ポイント)**: 各閾値を確定する。これは
/// ゲーム体験を直接決める重要な数値で、シミュレーター結果から逆算する
/// と良い。30 分で T4 が pop ~600 に達するため、目安として:
///
///   - Village → Town:  ~50 pop  (序盤、最初の店舗が回り始める頃)
///   - Town → City:     ~250 pop (中盤、複数の住宅クラスター)
///   - City → Metropolis: ~600 pop (終盤、Tier 4 AI で十分到達可能)
///
/// 数字を変える時は、シミュレーターの 30min ベンチを実行して
/// 「ちょうど終盤直前で Metropolis に到達するか」を確認すること。
pub fn city_tier_for(population: u32) -> CityTier {
    if population >= 600 {
        CityTier::Metropolis
    } else if population >= 250 {
        CityTier::City
    } else if population >= 50 {
        CityTier::Town
    } else {
        CityTier::Village
    }
}

/// 次のティアまでの必要人口 (None = 既に Metropolis)。
pub fn next_tier_threshold(t: CityTier) -> Option<u32> {
    match t {
        CityTier::Village => Some(50),
        CityTier::Town => Some(250),
        CityTier::City => Some(600),
        CityTier::Metropolis => None,
    }
}

/// 右パネルのタブ。Status / Manager / Events / World が初期セット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Status,
    Manager,
    Events,
    World,
}

impl PanelTab {
    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Status => "状態",
            PanelTab::Manager => "操作",
            PanelTab::Events => "履歴",
            PanelTab::World => "世界",
        }
    }
}

/// Player's strategic preference.  Drives how Tier-2+ AI weights its choices;
/// Tier-1 ignores this field.
///
/// `Tech` は短期収入を犠牲にして建設速度と (将来の) 研究ポイントを稼ぐ路線。
/// `Eco` は森を残し荒地だけ整地する「環境配慮」型 — 整地メカニクスと組み合わせ。
/// `Balanced` は「中間値で意思決定が薄まる」ため削除し、各択に明確な
/// トレードオフを持たせる方針 (Plan #1)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strategy {
    Growth, // prefer Houses
    Income, // prefer Shops
    /// Tech: 建設速度 +20% / 収入 -20%。AI は道路を優先し展開を重視。
    Tech,
    /// Eco: 森を切らない (Forest 整地を AI が回避)。建設速度 -10% / 収入 +5%。
    /// 「ゆっくり丁寧に育てる」自然と共存する街づくり。
    Eco,
}

/// CPU intelligence tier.  Higher = smarter placement decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiTier {
    /// Random placement, random building.  Wastes money, may strand shops.
    Random = 1,
    /// Builds adjacent to existing tiles.  Has minimal "can I afford it?" sense.
    Greedy = 2,
    /// Plans roads first, then buildings on the road.
    RoadPlanner = 3,
    /// Reads strategy weights and demand (pop vs shop balance).
    DemandAware = 4,
}

impl AiTier {
    /// Cash price to upgrade *into* this tier.
    pub fn upgrade_cost(self) -> i64 {
        match self {
            AiTier::Random => 0, // starting tier
            AiTier::Greedy => 500,
            AiTier::RoadPlanner => 5_000,
            AiTier::DemandAware => 50_000,
        }
    }

    pub fn next(self) -> Option<AiTier> {
        match self {
            AiTier::Random => Some(AiTier::Greedy),
            AiTier::Greedy => Some(AiTier::RoadPlanner),
            AiTier::RoadPlanner => Some(AiTier::DemandAware),
            AiTier::DemandAware => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AiTier::Random => "Random Bot",
            AiTier::Greedy => "Greedy",
            AiTier::RoadPlanner => "Road Planner",
            AiTier::DemandAware => "Demand Aware",
        }
    }
}

/// Whole-city snapshot.  Everything the simulator needs to step forward.
pub struct City {
    pub grid: Vec<Vec<Tile>>,
    /// Background terrain layer, generated once from the seed.
    /// 建物が建っても下に残るため、Water 隣接ボーナスなど将来の拡張に使える。
    pub terrain: super::terrain::TerrainLayer,
    /// 地形生成に使ったシード値 (UI で見せる、リセット時の再現用)。
    /// バナーに表示する予定 (CityTier 実装と一緒に使う)。
    #[allow(dead_code)]
    pub world_seed: u64,
    pub cash: i64,
    pub tick: u64,

    /// AI brain in use.
    pub ai_tier: AiTier,
    pub strategy: Strategy,

    /// 現在表示中の右パネルタブ。
    pub panel_tab: PanelTab,

    /// 直近 tick で観測したティア。次 tick で計算したティアと比較して、
    /// 進化したらフラッシュ + イベントログを発火。
    pub last_observed_tier: CityTier,
    /// ティア進化フラッシュが消える tick。`tick < value` の間バナーを光らせる。
    pub tier_flash_until: u64,

    /// Build queue: how many parallel constructions the AI can run.
    /// (Each Construction tile already counts toward this limit.)
    pub workers: u32,

    /// Deterministic PRNG state.  Seedable so simulator runs are reproducible.
    pub rng_state: u64,

    /// Rolling counters for diagnostics.
    pub buildings_started: u64,
    pub buildings_finished: u64,
    pub cash_earned_total: i64,
    pub cash_spent_total: i64,

    /// Most-recent AI activity for the on-screen "thought log".
    /// Newest first; older entries trimmed to `MAX_EVENTS`.
    pub events: Vec<String>,

    /// セルが完成フラッシュを保つ最終 tick (`tick < value` の間光る)。
    pub completion_flash_until: Vec<Vec<u64>>,
    /// アクティブ店舗が給料フラッシュを保つ最終 tick。
    pub payout_flash_until: Vec<Vec<u64>>,
    /// 直近の収入額 (status の "+$X" フラッシュ用)。
    pub last_payout_amount: i64,
    /// 直近の収入が発生した tick。
    pub last_payout_tick: u64,
}

pub const MAX_EVENTS: usize = 8;

/// 完成タイルが光り続けるtick数 (1.5秒)。
pub const COMPLETION_FLASH_TICKS: u64 = 15;

/// 店舗が給料発生時に光るtick数 (0.6秒)。
pub const PAYOUT_FLASH_TICKS: u64 = 6;

/// ティア進化時のバナー全体フラッシュ tick 数 (3 秒)。
pub const TIER_FLASH_TICKS: u64 = 30;

impl City {
    pub fn new() -> Self {
        Self::with_seed(0xC1A5_5EED)
    }

    pub fn with_seed(seed: u64) -> Self {
        let mut grid = Vec::with_capacity(GRID_H);
        let mut completion_flash_until = Vec::with_capacity(GRID_H);
        let mut payout_flash_until = Vec::with_capacity(GRID_H);
        for _ in 0..GRID_H {
            grid.push(vec![Tile::Empty; GRID_W]);
            completion_flash_until.push(vec![0u64; GRID_W]);
            payout_flash_until.push(vec![0u64; GRID_W]);
        }
        let terrain = super::terrain::generate(seed);
        Self {
            grid,
            terrain,
            world_seed: seed,
            cash: 200, // enough seed money for 5 houses or a shop
            tick: 0,
            ai_tier: AiTier::Random,
            strategy: Strategy::Growth,
            panel_tab: PanelTab::Manager,
            last_observed_tier: CityTier::Village,
            tier_flash_until: 0,
            workers: 1,
            rng_state: seed,
            buildings_started: 0,
            buildings_finished: 0,
            cash_earned_total: 0,
            cash_spent_total: 0,
            events: Vec::new(),
            completion_flash_until,
            payout_flash_until,
            last_payout_amount: 0,
            last_payout_tick: 0,
        }
    }

    /// Record a new AI activity entry, keeping the log bounded.
    pub fn push_event(&mut self, msg: impl Into<String>) {
        self.events.insert(0, msg.into());
        if self.events.len() > MAX_EVENTS {
            self.events.truncate(MAX_EVENTS);
        }
    }

    /// xorshift64* — small, deterministic, good enough for placement noise.
    pub fn next_rand(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    pub fn tile(&self, x: usize, y: usize) -> &Tile {
        &self.grid[y][x]
    }

    #[cfg(test)]
    pub fn set_tile(&mut self, x: usize, y: usize, t: Tile) {
        self.grid[y][x] = t;
    }

    /// How many constructions are currently active.
    pub fn active_constructions(&self) -> u32 {
        let mut n = 0;
        for row in &self.grid {
            for t in row {
                if matches!(t, Tile::Construction { .. }) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Free worker slots = workers - active.
    pub fn free_workers(&self) -> u32 {
        self.workers.saturating_sub(self.active_constructions())
    }

    /// Iterate every cell with its (x, y).
    pub fn cells(&self) -> impl Iterator<Item = (usize, usize, &Tile)> {
        self.grid
            .iter()
            .enumerate()
            .flat_map(|(y, row)| row.iter().enumerate().map(move |(x, t)| (x, y, t)))
    }

    pub fn count_built(&self, kind: Building) -> u32 {
        let mut n = 0;
        for (_, _, t) in self.cells() {
            if let Tile::Built(b) = t {
                if *b == kind {
                    n += 1;
                }
            }
        }
        n
    }

    /// Population from finished houses.  Each House holds 5.
    pub fn population(&self) -> u32 {
        self.count_built(Building::House) * 5
    }

    /// Convenience: 指定セルの地形。境界外は Plain 扱い。
    pub fn terrain_at(&self, x: usize, y: usize) -> super::terrain::Terrain {
        if x >= GRID_W || y >= GRID_H {
            super::terrain::Terrain::Plain
        } else {
            self.terrain[y][x]
        }
    }
}

impl Default for City {
    fn default() -> Self {
        City::new()
    }
}
