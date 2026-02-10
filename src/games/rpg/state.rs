//! Dungeon Dive game state — all data structures, no logic.
//!
//! Design: "Dungeon Crawler" — room-by-room exploration with
//! risk/reward resource management across dungeon floors.

// ── Elements ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    Fire,
    Ice,
    Thunder,
}

// ── Enemies ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnemyKind {
    Slime,
    Rat,
    Goblin,
    Bat,
    Skeleton,
    Golem,
    DarkKnight,
    Demon,
    Dragon,
    DemonLord,
}

pub struct EnemyInfo {
    pub name: &'static str,
    pub max_hp: u32,
    pub atk: u32,
    pub def: u32,
    pub exp: u32,
    pub gold: u32,
    pub drop: Option<(ItemKind, u32)>, // (item, chance_pct 0-100)
    pub weakness: Option<Element>,
    pub can_charge: bool, // can do charge attack (telegraph → 2x damage)
}

pub fn enemy_info(kind: EnemyKind) -> EnemyInfo {
    match kind {
        EnemyKind::Slime => EnemyInfo {
            name: "スライム", max_hp: 12, atk: 4, def: 1, exp: 5, gold: 8,
            drop: Some((ItemKind::Herb, 40)),
            weakness: Some(Element::Fire), can_charge: false,
        },
        EnemyKind::Rat => EnemyInfo {
            name: "大ネズミ", max_hp: 10, atk: 6, def: 0, exp: 4, gold: 6,
            drop: Some((ItemKind::Herb, 25)),
            weakness: None, can_charge: false,
        },
        EnemyKind::Goblin => EnemyInfo {
            name: "ゴブリン", max_hp: 28, atk: 10, def: 4, exp: 15, gold: 20,
            drop: Some((ItemKind::MagicWater, 30)),
            weakness: Some(Element::Fire), can_charge: false,
        },
        EnemyKind::Bat => EnemyInfo {
            name: "コウモリ", max_hp: 18, atk: 9, def: 2, exp: 10, gold: 12,
            drop: None,
            weakness: Some(Element::Thunder), can_charge: false,
        },
        EnemyKind::Skeleton => EnemyInfo {
            name: "スケルトン", max_hp: 45, atk: 14, def: 8, exp: 30, gold: 35,
            drop: Some((ItemKind::StrengthPotion, 20)),
            weakness: Some(Element::Fire), can_charge: false,
        },
        EnemyKind::Golem => EnemyInfo {
            name: "ゴーレム", max_hp: 60, atk: 16, def: 14, exp: 40, gold: 50,
            drop: Some((ItemKind::MagicWater, 30)),
            weakness: Some(Element::Thunder), can_charge: true,
        },
        EnemyKind::DarkKnight => EnemyInfo {
            name: "暗黒騎士", max_hp: 75, atk: 20, def: 15, exp: 55, gold: 70,
            drop: Some((ItemKind::StrengthPotion, 35)),
            weakness: Some(Element::Thunder), can_charge: true,
        },
        EnemyKind::Demon => EnemyInfo {
            name: "デーモン", max_hp: 85, atk: 22, def: 12, exp: 65, gold: 80,
            drop: Some((ItemKind::MagicWater, 40)),
            weakness: Some(Element::Ice), can_charge: false,
        },
        EnemyKind::Dragon => EnemyInfo {
            name: "ドラゴン", max_hp: 120, atk: 28, def: 18, exp: 100, gold: 150,
            drop: Some((ItemKind::Herb, 50)),
            weakness: Some(Element::Ice), can_charge: true,
        },
        EnemyKind::DemonLord => EnemyInfo {
            name: "魔王", max_hp: 200, atk: 32, def: 20, exp: 300, gold: 500,
            drop: None,
            weakness: None, can_charge: true,
        },
    }
}

/// Enemies that can appear on a given floor.
pub fn floor_enemies(floor: u32) -> &'static [EnemyKind] {
    match floor {
        1 => &[EnemyKind::Slime, EnemyKind::Rat],
        2 => &[EnemyKind::Slime, EnemyKind::Rat, EnemyKind::Goblin],
        3 => &[EnemyKind::Goblin, EnemyKind::Bat],
        4 => &[EnemyKind::Goblin, EnemyKind::Bat, EnemyKind::Skeleton],
        5 => &[EnemyKind::Skeleton, EnemyKind::Golem],
        6 => &[EnemyKind::Skeleton, EnemyKind::Golem, EnemyKind::DarkKnight],
        7 => &[EnemyKind::DarkKnight, EnemyKind::Demon],
        8 => &[EnemyKind::Demon, EnemyKind::Dragon],
        9 => &[EnemyKind::Dragon, EnemyKind::Demon],
        _ => &[EnemyKind::DemonLord],
    }
}

// ── Items ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemKind {
    Herb,
    MagicWater,
    StrengthPotion,
    WoodenSword,
    IronSword,
    SteelSword,
    HolySword,
    TravelClothes,
    LeatherArmor,
    ChainMail,
    KnightArmor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemCategory {
    Consumable,
    Weapon,
    Armor,
}

pub struct ItemInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ItemCategory,
    pub buy_price: u32,
    pub value: u32,
}

pub fn item_info(kind: ItemKind) -> ItemInfo {
    match kind {
        ItemKind::Herb => ItemInfo {
            name: "薬草", description: "HPを30回復",
            category: ItemCategory::Consumable, buy_price: 20, value: 30,
        },
        ItemKind::MagicWater => ItemInfo {
            name: "魔法の水", description: "MPを20回復",
            category: ItemCategory::Consumable, buy_price: 50, value: 20,
        },
        ItemKind::StrengthPotion => ItemInfo {
            name: "力の薬", description: "戦闘中ATK+5",
            category: ItemCategory::Consumable, buy_price: 80, value: 5,
        },
        ItemKind::WoodenSword => ItemInfo {
            name: "木の剣", description: "ATK+3",
            category: ItemCategory::Weapon, buy_price: 30, value: 3,
        },
        ItemKind::IronSword => ItemInfo {
            name: "鉄の剣", description: "ATK+8",
            category: ItemCategory::Weapon, buy_price: 120, value: 8,
        },
        ItemKind::SteelSword => ItemInfo {
            name: "鋼の剣", description: "ATK+15",
            category: ItemCategory::Weapon, buy_price: 350, value: 15,
        },
        ItemKind::HolySword => ItemInfo {
            name: "聖剣", description: "ATK+25",
            category: ItemCategory::Weapon, buy_price: 1000, value: 25,
        },
        ItemKind::TravelClothes => ItemInfo {
            name: "旅人の服", description: "DEF+2",
            category: ItemCategory::Armor, buy_price: 20, value: 2,
        },
        ItemKind::LeatherArmor => ItemInfo {
            name: "革の鎧", description: "DEF+5",
            category: ItemCategory::Armor, buy_price: 100, value: 5,
        },
        ItemKind::ChainMail => ItemInfo {
            name: "鎖帷子", description: "DEF+12",
            category: ItemCategory::Armor, buy_price: 300, value: 12,
        },
        ItemKind::KnightArmor => ItemInfo {
            name: "騎士の鎧", description: "DEF+20",
            category: ItemCategory::Armor, buy_price: 800, value: 20,
        },
    }
}

/// Shop inventory depends on player's max floor reached.
pub fn shop_items(max_floor: u32) -> Vec<(ItemKind, u32)> {
    let mut items = vec![
        (ItemKind::Herb, 99),
        (ItemKind::MagicWater, 99),
        (ItemKind::WoodenSword, 1),
        (ItemKind::TravelClothes, 1),
    ];
    if max_floor >= 2 {
        items.push((ItemKind::IronSword, 1));
        items.push((ItemKind::LeatherArmor, 1));
    }
    if max_floor >= 4 {
        items.push((ItemKind::StrengthPotion, 99));
        items.push((ItemKind::SteelSword, 1));
        items.push((ItemKind::ChainMail, 1));
    }
    if max_floor >= 7 {
        items.push((ItemKind::HolySword, 1));
        items.push((ItemKind::KnightArmor, 1));
    }
    items
}

// ── Skills ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillKind {
    Fire,
    Heal,
    Shield,
    IceBlade,
    Thunder,
    Drain,
    Berserk,
}

pub struct SkillInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub mp_cost: u32,
    pub value: u32,
    pub learn_level: u32,
}

pub fn skill_info(kind: SkillKind) -> SkillInfo {
    match kind {
        SkillKind::Fire => SkillInfo {
            name: "ファイア", description: "炎で敵を攻撃 (魔力依存)",
            mp_cost: 8, value: 3, learn_level: 1,
        },
        SkillKind::Heal => SkillInfo {
            name: "ヒール", description: "HPを回復 (魔力依存)",
            mp_cost: 6, value: 2, learn_level: 2,
        },
        SkillKind::IceBlade => SkillInfo {
            name: "アイスブレード", description: "氷の刃で斬る (ATK+MAG)",
            mp_cost: 10, value: 2, learn_level: 3,
        },
        SkillKind::Shield => SkillInfo {
            name: "シールド", description: "戦闘中DEF上昇",
            mp_cost: 5, value: 8, learn_level: 4,
        },
        SkillKind::Thunder => SkillInfo {
            name: "サンダー", description: "雷撃 (高威力・魔力依存)",
            mp_cost: 14, value: 4, learn_level: 5,
        },
        SkillKind::Drain => SkillInfo {
            name: "ドレイン", description: "HP吸収攻撃 (魔力依存)",
            mp_cost: 12, value: 2, learn_level: 6,
        },
        SkillKind::Berserk => SkillInfo {
            name: "バーサク", description: "ATK大幅UP / DEF低下",
            mp_cost: 8, value: 15, learn_level: 8,
        },
    }
}

/// Returns the element associated with a skill, if any.
pub fn skill_element(kind: SkillKind) -> Option<Element> {
    match kind {
        SkillKind::Fire => Some(Element::Fire),
        SkillKind::IceBlade => Some(Element::Ice),
        SkillKind::Thunder => Some(Element::Thunder),
        _ => None,
    }
}

pub const ALL_SKILLS: &[SkillKind] = &[
    SkillKind::Fire,
    SkillKind::Heal,
    SkillKind::IceBlade,
    SkillKind::Shield,
    SkillKind::Thunder,
    SkillKind::Drain,
    SkillKind::Berserk,
];

// ── Level / EXP Table ─────────────────────────────────────────

pub struct LevelStats {
    pub max_hp: u32,
    pub max_mp: u32,
    pub atk: u32,
    pub def: u32,
    pub mag: u32,
    pub exp_to_next: u32,
}

pub fn level_stats(level: u32) -> LevelStats {
    match level {
        1 => LevelStats { max_hp: 50, max_mp: 15, atk: 5, def: 3, mag: 4, exp_to_next: 20 },
        2 => LevelStats { max_hp: 65, max_mp: 20, atk: 7, def: 4, mag: 6, exp_to_next: 45 },
        3 => LevelStats { max_hp: 82, max_mp: 26, atk: 9, def: 6, mag: 8, exp_to_next: 80 },
        4 => LevelStats { max_hp: 100, max_mp: 33, atk: 12, def: 8, mag: 10, exp_to_next: 130 },
        5 => LevelStats { max_hp: 120, max_mp: 40, atk: 15, def: 10, mag: 13, exp_to_next: 200 },
        6 => LevelStats { max_hp: 142, max_mp: 48, atk: 18, def: 13, mag: 16, exp_to_next: 300 },
        7 => LevelStats { max_hp: 165, max_mp: 56, atk: 22, def: 16, mag: 19, exp_to_next: 430 },
        8 => LevelStats { max_hp: 190, max_mp: 65, atk: 26, def: 19, mag: 23, exp_to_next: 600 },
        9 => LevelStats { max_hp: 220, max_mp: 75, atk: 30, def: 22, mag: 27, exp_to_next: 999 },
        _ => LevelStats { max_hp: 250, max_mp: 85, atk: 35, def: 26, mag: 32, exp_to_next: 9999 },
    }
}

pub const MAX_LEVEL: u32 = 10;
pub const MAX_FLOOR: u32 = 10;

// ── Inventory Entry ───────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub kind: ItemKind,
    pub count: u32,
}

// ── Dungeon Room System ───────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoomKind {
    /// Monster encounter.
    Enemy,
    /// Treasure chest (gold or item).
    Treasure,
    /// Trap that deals damage.
    Trap,
    /// Healing spring (restores some HP/MP).
    Spring,
    /// Empty room with flavor text.
    Empty,
    /// Stairs to next floor.
    Stairs,
}

#[derive(Clone, Debug)]
pub struct Room {
    pub kind: RoomKind,
    pub visited: bool,
}

#[derive(Clone, Debug)]
pub struct DungeonFloor {
    pub floor_num: u32,
    pub rooms: Vec<Room>,
    pub current_room: usize,
}

/// What happened after resolving a room event.
#[derive(Clone, Debug)]
pub struct RoomResult {
    pub description: Vec<String>,
}

// ── Battle State ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct BattleEnemy {
    pub kind: EnemyKind,
    pub hp: u32,
    pub max_hp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BattlePhase {
    SelectAction,
    SelectSkill,
    SelectItem,
    Victory,
    Defeat,
    Fled,
}

#[derive(Clone, Debug)]
pub struct BattleState {
    pub enemy: BattleEnemy,
    pub phase: BattlePhase,
    pub player_def_boost: u32,
    pub player_atk_boost: u32,
    pub log: Vec<String>,
    pub is_boss: bool,
    pub enemy_charging: bool,
    pub player_berserk: bool,
}

// ── Scene System ─────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scene {
    /// Opening intro (0 = first screen, 1 = receive gear, 2 = transition).
    Intro(u8),
    /// In town: rest, shop, enter dungeon.
    Town,
    /// Exploring dungeon: room-by-room progression.
    Dungeon,
    /// Room event resolved, showing result and choices.
    DungeonResult,
    /// In combat.
    Battle,
    /// Game complete — demon lord defeated.
    GameClear,
}

/// Overlay screens drawn on top of the main scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    Inventory,
    Status,
    Shop,
}

// ── Root Game State ───────────────────────────────────────────

pub struct RpgState {
    // Player stats (persist across runs)
    pub level: u32,
    pub exp: u32,
    pub hp: u32,
    pub max_hp: u32,
    pub mp: u32,
    pub max_mp: u32,
    pub base_atk: u32,
    pub base_def: u32,
    pub mag: u32,
    pub gold: u32,

    // Equipment
    pub weapon: Option<ItemKind>,
    pub armor: Option<ItemKind>,

    // Inventory
    pub inventory: Vec<InventoryItem>,

    // Dungeon
    pub dungeon: Option<DungeonFloor>,
    pub max_floor_reached: u32,
    pub total_clears: u32,

    // Battle
    pub battle: Option<BattleState>,

    // Scene system
    pub scene: Scene,
    pub overlay: Option<Overlay>,
    pub scene_text: Vec<String>,

    // Room result (what happened in last room)
    pub room_result: Option<RoomResult>,

    // Log (shown at bottom)
    pub log: Vec<String>,

    // Game state
    pub game_cleared: bool,
    pub rng_seed: u64,

    // Stats for current dungeon run
    pub run_gold_earned: u32,
    pub run_exp_earned: u32,
    pub run_enemies_killed: u32,
    pub run_rooms_cleared: u32,
}

impl RpgState {
    pub fn new() -> Self {
        let stats = level_stats(1);
        Self {
            level: 1,
            exp: 0,
            hp: stats.max_hp,
            max_hp: stats.max_hp,
            mp: stats.max_mp,
            max_mp: stats.max_mp,
            base_atk: stats.atk,
            base_def: stats.def,
            mag: stats.mag,
            gold: 0,
            weapon: None,
            armor: None,
            inventory: Vec::new(),
            dungeon: None,
            max_floor_reached: 0,
            total_clears: 0,
            battle: None,
            scene: Scene::Intro(0),
            overlay: None,
            scene_text: Vec::new(),
            room_result: None,
            log: Vec::new(),
            game_cleared: false,
            rng_seed: 42,
            run_gold_earned: 0,
            run_exp_earned: 0,
            run_enemies_killed: 0,
            run_rooms_cleared: 0,
        }
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    pub fn total_atk(&self) -> u32 {
        let weapon_bonus = self.weapon.map(|w| item_info(w).value).unwrap_or(0);
        self.base_atk + weapon_bonus
    }

    pub fn total_def(&self) -> u32 {
        let armor_bonus = self.armor.map(|a| item_info(a).value).unwrap_or(0);
        self.base_def + armor_bonus
    }

    #[cfg(test)]
    pub fn item_count(&self, kind: ItemKind) -> u32 {
        self.inventory
            .iter()
            .find(|i| i.kind == kind)
            .map(|i| i.count)
            .unwrap_or(0)
    }
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = RpgState::new();
        assert_eq!(s.level, 1);
        assert_eq!(s.hp, 50);
        assert_eq!(s.gold, 0);
        assert_eq!(s.scene, Scene::Intro(0));
        assert!(s.overlay.is_none());
        assert!(!s.game_cleared);
    }

    #[test]
    fn total_atk_without_weapon() {
        let s = RpgState::new();
        assert_eq!(s.total_atk(), 5);
    }

    #[test]
    fn total_atk_with_weapon() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::IronSword);
        assert_eq!(s.total_atk(), 13);
    }

    #[test]
    fn total_def_with_armor() {
        let mut s = RpgState::new();
        s.armor = Some(ItemKind::LeatherArmor);
        assert_eq!(s.total_def(), 8);
    }

    #[test]
    fn item_count_empty() {
        let s = RpgState::new();
        assert_eq!(s.item_count(ItemKind::Herb), 0);
    }

    #[test]
    fn item_count_with_items() {
        let mut s = RpgState::new();
        s.inventory
            .push(InventoryItem { kind: ItemKind::Herb, count: 5 });
        assert_eq!(s.item_count(ItemKind::Herb), 5);
    }

    #[test]
    fn log_truncation() {
        let mut s = RpgState::new();
        for i in 0..40 {
            s.add_log(&format!("msg {}", i));
        }
        assert!(s.log.len() <= 30);
    }

    #[test]
    fn shop_expands_with_progress() {
        let base = shop_items(0);
        let mid = shop_items(4);
        let late = shop_items(7);
        assert!(mid.len() > base.len());
        assert!(late.len() > mid.len());
    }

    #[test]
    fn floor_enemies_valid() {
        for f in 1..=10 {
            let enemies = floor_enemies(f);
            assert!(!enemies.is_empty());
            for &kind in enemies {
                let info = enemy_info(kind);
                assert!(!info.name.is_empty());
            }
        }
    }

    #[test]
    fn all_skills_have_info() {
        for &skill in ALL_SKILLS {
            let info = skill_info(skill);
            assert!(!info.name.is_empty());
            assert!(info.mp_cost > 0);
        }
    }
}
