//! Game state for 神の戦場 (God Field 風).
//!
//! Pure data: no rendering, no I/O. All state mutations live in `logic.rs`.

use std::collections::VecDeque;

// ── Cards ──────────────────────────────────────────────────────

/// All cards in the game.  Each card has a static definition queried via
/// [`Card::def`].  The deck is a randomized stream of these IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    // Weapons
    Fist,
    Knife,
    Sword,
    Greatsword,
    Spear,
    Axe,
    Bow,
    Gun,
    Wand,
    GodSword,
    // Armors
    SmallShield,
    Shield,
    Armor,
    GreatShield,
    Plate,
    Robe,
    Barrier,
    // Heals
    Herb,
    FirstAid,
    Elixir,
    // Specials
    Pray,
    Reflect,
    Steal,
    Trial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardKind {
    Weapon,
    Armor,
    Heal,
    Special,
}

/// Static description of a card. `power` semantics depend on `kind`:
/// weapon damage, armor defense, heal amount, or special-specific value.
pub struct CardDef {
    pub name: &'static str,
    pub kind: CardKind,
    pub power: u8,
    pub hits: u8,
    /// Magic weapon: only blocked by armors that block magic.
    pub magic: bool,
    /// Piercing weapon: ignores 2 points of armor defense.
    pub pierce: bool,
    /// Magic-blocking armor: blocks magic weapons in addition to physical.
    pub blocks_magic: bool,
}

impl Card {
    pub fn def(self) -> &'static CardDef {
        use Card::*;
        match self {
            // Weapons (kind: Weapon, power: damage)
            Fist => &CardDef { name: "拳",       kind: CardKind::Weapon, power: 1,  hits: 1, magic: false, pierce: false, blocks_magic: false },
            Knife => &CardDef { name: "短剣",    kind: CardKind::Weapon, power: 2,  hits: 1, magic: false, pierce: false, blocks_magic: false },
            Sword => &CardDef { name: "剣",      kind: CardKind::Weapon, power: 4,  hits: 1, magic: false, pierce: false, blocks_magic: false },
            Greatsword => &CardDef { name: "大剣", kind: CardKind::Weapon, power: 6, hits: 1, magic: false, pierce: false, blocks_magic: false },
            Spear => &CardDef { name: "槍",      kind: CardKind::Weapon, power: 4,  hits: 1, magic: false, pierce: true,  blocks_magic: false },
            Axe => &CardDef { name: "斧",        kind: CardKind::Weapon, power: 5,  hits: 1, magic: false, pierce: false, blocks_magic: false },
            Bow => &CardDef { name: "弓",        kind: CardKind::Weapon, power: 3,  hits: 2, magic: false, pierce: false, blocks_magic: false },
            Gun => &CardDef { name: "銃",        kind: CardKind::Weapon, power: 8,  hits: 1, magic: false, pierce: false, blocks_magic: false },
            Wand => &CardDef { name: "魔法の杖", kind: CardKind::Weapon, power: 4,  hits: 1, magic: true,  pierce: false, blocks_magic: false },
            GodSword => &CardDef { name: "神剣", kind: CardKind::Weapon, power: 12, hits: 1, magic: false, pierce: true,  blocks_magic: false },
            // Armors (kind: Armor, power: defense)
            SmallShield => &CardDef { name: "小盾", kind: CardKind::Armor, power: 2, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Shield => &CardDef { name: "盾",       kind: CardKind::Armor, power: 3, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Armor => &CardDef { name: "鎧",        kind: CardKind::Armor, power: 4, hits: 0, magic: false, pierce: false, blocks_magic: false },
            GreatShield => &CardDef { name: "大盾", kind: CardKind::Armor, power: 6, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Plate => &CardDef { name: "プレート",  kind: CardKind::Armor, power: 5, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Robe => &CardDef { name: "法衣",       kind: CardKind::Armor, power: 3, hits: 0, magic: false, pierce: false, blocks_magic: true },
            Barrier => &CardDef { name: "結界",    kind: CardKind::Armor, power: 8, hits: 0, magic: false, pierce: false, blocks_magic: true },
            // Heals
            Herb => &CardDef { name: "薬草",       kind: CardKind::Heal, power: 5,  hits: 0, magic: false, pierce: false, blocks_magic: false },
            FirstAid => &CardDef { name: "救急箱", kind: CardKind::Heal, power: 10, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Elixir => &CardDef { name: "天恵",     kind: CardKind::Heal, power: 20, hits: 0, magic: false, pierce: false, blocks_magic: false },
            // Specials
            Pray => &CardDef { name: "祈り",       kind: CardKind::Special, power: 3, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Reflect => &CardDef { name: "反射",    kind: CardKind::Special, power: 0, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Steal => &CardDef { name: "略奪",      kind: CardKind::Special, power: 0, hits: 0, magic: false, pierce: false, blocks_magic: false },
            Trial => &CardDef { name: "神の試練",  kind: CardKind::Special, power: 5, hits: 0, magic: false, pierce: false, blocks_magic: false },
        }
    }

    pub fn kind(self) -> CardKind { self.def().kind }

    /// Cards in the random draw pool. Common cards appear more frequently
    /// (multiplicity in this slice).
    pub fn pool() -> &'static [Card] {
        use Card::*;
        &[
            // Weapons (skewed toward weak)
            Fist, Fist, Fist,
            Knife, Knife, Knife,
            Sword, Sword,
            Greatsword,
            Spear, Spear,
            Axe, Axe,
            Bow, Bow,
            Gun,
            Wand,
            GodSword,
            // Armors
            SmallShield, SmallShield, SmallShield,
            Shield, Shield, Shield,
            Armor, Armor,
            GreatShield,
            Plate, Plate,
            Robe,
            Barrier,
            // Heals
            Herb, Herb, Herb,
            FirstAid, FirstAid,
            Elixir,
            // Specials
            Pray, Pray,
            Reflect,
            Steal,
            Trial,
        ]
    }
}

// ── Players ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Player {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub hand: Vec<Card>,
    pub alive: bool,
    pub is_human: bool,
}

impl Player {
    pub fn new(name: impl Into<String>, max_hp: i32, is_human: bool) -> Self {
        Self {
            name: name.into(),
            hp: max_hp,
            max_hp,
            hand: Vec::new(),
            alive: true,
            is_human,
        }
    }
}

pub const STARTING_HP: i32 = 30;
pub const HAND_SIZE: usize = 5;
pub const NUM_PLAYERS: usize = 4;

// ── Phases ─────────────────────────────────────────────────────

/// What screen / interaction the game is in.  Only the human player has
/// active selection phases; CPU turns are auto-driven by ticks.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Title / "tap to start" screen.
    Intro,
    /// Human's turn — pick an action.
    PlayerAction,
    /// Human is selecting weapons (multi-select before attack confirm).
    PlayerSelectWeapons,
    /// Human picked weapons, now choose target.
    PlayerSelectTarget,
    /// Human is selecting a heal card.
    PlayerSelectHeal,
    /// Human is selecting a special card.
    PlayerSelectSpecial,
    /// CPU player's turn is being animated.  `ticks_left` counts down before
    /// the CPU actually executes its action so the player can read the log.
    CpuTurn { idx: usize, ticks_left: u32 },
    /// Brief pause between turns to let log scroll.  Advances to the next
    /// player when ticks_left hits 0.
    BetweenTurns { ticks_left: u32 },
    /// All enemies down — human wins.
    Victory,
    /// Human HP reached 0.
    Defeat,
}

// ── Action records (for log) ───────────────────────────────────

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub line: String,
    pub kind: LogKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Attack,
    Defend,
    Heal,
    Damage,
    Death,
    Special,
}

// ── Top-level state ────────────────────────────────────────────

pub struct GfState {
    pub players: Vec<Player>,
    /// Index into `players` whose turn it currently is.
    pub turn: usize,
    pub phase: Phase,
    /// Most recent ~12 log lines, oldest first.
    pub log: VecDeque<LogEntry>,
    /// Selected weapon indices in the human's hand (for multi-card combo).
    pub selected_weapons: Vec<usize>,
    /// Random seed (xorshift32).
    pub rng_seed: u32,
    /// Round counter — purely cosmetic, increments when we wrap back to player 0.
    pub round: u32,
}

impl GfState {
    pub fn new(seed: u32) -> Self {
        let mut s = Self {
            players: vec![
                Player::new("あなた",      STARTING_HP, true),
                Player::new("赤の戦神",    STARTING_HP, false),
                Player::new("青の魔導士",  STARTING_HP, false),
                Player::new("緑の聖騎士",  STARTING_HP, false),
            ],
            turn: 0,
            phase: Phase::Intro,
            log: VecDeque::new(),
            selected_weapons: Vec::new(),
            rng_seed: if seed == 0 { 0xDEAD_BEEF } else { seed },
            round: 1,
        };
        // Pre-deal hands so the intro screen can preview them.
        for i in 0..s.players.len() {
            for _ in 0..HAND_SIZE {
                let c = crate::games::godfield::logic::draw_card(&mut s.rng_seed);
                s.players[i].hand.push(c);
            }
        }
        s.push_log("神々が見守る戦場、ここに開幕。", LogKind::Info);
        s
    }

    pub fn push_log(&mut self, msg: impl Into<String>, kind: LogKind) {
        self.log.push_back(LogEntry { line: msg.into(), kind });
        while self.log.len() > 30 {
            self.log.pop_front();
        }
    }

    pub fn human_idx(&self) -> usize { 0 }

    pub fn alive_count(&self) -> usize {
        self.players.iter().filter(|p| p.alive).count()
    }

    /// Living opponents from the perspective of `from_idx`.
    pub fn living_opponents(&self, from_idx: usize) -> Vec<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(i, p)| *i != from_idx && p.alive)
            .map(|(i, _)| i)
            .collect()
    }
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_has_four_players_with_full_hands() {
        let s = GfState::new(1);
        assert_eq!(s.players.len(), NUM_PLAYERS);
        for p in &s.players {
            assert_eq!(p.hp, STARTING_HP);
            assert_eq!(p.hand.len(), HAND_SIZE);
            assert!(p.alive);
        }
        assert_eq!(s.phase, Phase::Intro);
    }

    #[test]
    fn human_is_player_zero() {
        let s = GfState::new(1);
        assert_eq!(s.human_idx(), 0);
        assert!(s.players[0].is_human);
        for p in &s.players[1..] {
            assert!(!p.is_human);
        }
    }

    #[test]
    fn card_pool_covers_all_kinds() {
        let pool = Card::pool();
        let kinds: std::collections::HashSet<_> = pool.iter().map(|c| c.kind()).collect();
        assert!(kinds.contains(&CardKind::Weapon));
        assert!(kinds.contains(&CardKind::Armor));
        assert!(kinds.contains(&CardKind::Heal));
        assert!(kinds.contains(&CardKind::Special));
    }

    #[test]
    fn card_def_consistency() {
        // Sanity: weapon power > 0, armor power > 0, heal power > 0.
        for &c in Card::pool() {
            let d = c.def();
            match d.kind {
                CardKind::Weapon => assert!(d.power > 0 && d.hits > 0, "{}", d.name),
                CardKind::Armor => assert!(d.power > 0, "{}", d.name),
                CardKind::Heal => assert!(d.power > 0, "{}", d.name),
                CardKind::Special => {}
            }
        }
    }

    #[test]
    fn log_caps_at_30() {
        let mut s = GfState::new(1);
        for i in 0..50 {
            s.push_log(format!("entry {}", i), LogKind::Info);
        }
        assert_eq!(s.log.len(), 30);
        // Oldest should be entry 21 (50 entries pushed - 30 kept + 1 from new()
        // already evicted; just check we kept the latest)
        assert!(s.log.back().unwrap().line.ends_with("49"));
    }

    #[test]
    fn living_opponents_excludes_self_and_dead() {
        let mut s = GfState::new(1);
        s.players[2].alive = false;
        let opps = s.living_opponents(0);
        assert_eq!(opps, vec![1, 3]);
    }
}
