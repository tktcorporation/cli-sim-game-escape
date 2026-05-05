//! City state for Idle Metropolis.
//!
//! AI-driven idle city builder.  The player only sets a strategy and buys
//! upgrades; the CPU does all placement.  At low Tiers the CPU is dumb on
//! purpose, so we need a balance simulator (see `simulator.rs`) to confirm
//! the game is still progressing.

pub const GRID_W: usize = 24;
pub const GRID_H: usize = 12;

pub const TICKS_PER_SEC: u32 = 10;

/// What occupies a single map cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    Empty,
    /// Construction in progress: target building, ticks remaining.
    Construction {
        target: Building,
        ticks_remaining: u32,
    },
    Built(Building),
}

/// Buildings the AI can place.  Kept small for the MVP — three types is
/// enough to express the "houses feed shops feed cash" loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Building {
    /// Connector: enables shops to be supplied.
    Road,
    /// Adds population.
    House,
    /// Generates cash, but only if it has at least one road neighbor AND
    /// at least one house within Manhattan distance 3 (a "customer base").
    Shop,
}

impl Building {
    /// One-time build cost in cash.
    pub fn cost(self) -> i64 {
        match self {
            Building::Road => 10,
            Building::House => 40,
            Building::Shop => 150,
        }
    }

    /// Ticks needed to finish construction.
    pub fn build_ticks(self) -> u32 {
        match self {
            Building::Road => 30,    // 3 sec
            Building::House => 100,  // 10 sec
            Building::Shop => 200,   // 20 sec
        }
    }

}

/// Player's strategic preference.  Drives how Tier-2+ AI weights its choices;
/// Tier-1 ignores this field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strategy {
    Growth,   // prefer Houses
    Income,   // prefer Shops
    Balanced, // mix
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
    pub cash: i64,
    pub tick: u64,

    /// AI brain in use.
    pub ai_tier: AiTier,
    pub strategy: Strategy,

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
}

pub const MAX_EVENTS: usize = 8;

impl City {
    pub fn new() -> Self {
        Self::with_seed(0xC1A5_5EED)
    }

    pub fn with_seed(seed: u64) -> Self {
        let mut grid = Vec::with_capacity(GRID_H);
        for _ in 0..GRID_H {
            grid.push(vec![Tile::Empty; GRID_W]);
        }
        Self {
            grid,
            cash: 200, // enough seed money for 5 houses or a shop
            tick: 0,
            ai_tier: AiTier::Random,
            strategy: Strategy::Balanced,
            workers: 1,
            rng_state: seed,
            buildings_started: 0,
            buildings_finished: 0,
            cash_earned_total: 0,
            cash_spent_total: 0,
            events: Vec::new(),
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
}

impl Default for City {
    fn default() -> Self {
        City::new()
    }
}
