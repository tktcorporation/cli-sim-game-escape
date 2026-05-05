//! Dungeon Dive game state — all data structures, no logic.
//!
//! Design: roguelike grid-based dungeon crawler with inline combat,
//! satiety, random affixes, quests, prayer, and pets.

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
    pub glyph: char, // letter shown on map
    pub max_hp: u32,
    pub atk: u32,
    pub def: u32,
    pub exp: u32,
    pub gold: u32,
    pub drop: Option<(ItemKind, u32)>, // (item, chance_pct 0-100)
    pub weakness: Option<Element>,
    pub can_charge: bool,
    /// Whether this monster can be tamed by feeding.
    pub tameable: bool,
}

pub fn enemy_info(kind: EnemyKind) -> EnemyInfo {
    match kind {
        EnemyKind::Slime => EnemyInfo {
            name: "スライム", glyph: 's', max_hp: 12, atk: 4, def: 1, exp: 5, gold: 8,
            drop: Some((ItemKind::Herb, 40)),
            weakness: Some(Element::Fire), can_charge: false, tameable: true,
        },
        EnemyKind::Rat => EnemyInfo {
            name: "大ネズミ", glyph: 'r', max_hp: 10, atk: 6, def: 0, exp: 4, gold: 6,
            drop: Some((ItemKind::Herb, 25)),
            weakness: None, can_charge: false, tameable: true,
        },
        EnemyKind::Goblin => EnemyInfo {
            name: "ゴブリン", glyph: 'g', max_hp: 28, atk: 10, def: 4, exp: 15, gold: 20,
            drop: Some((ItemKind::MagicWater, 30)),
            weakness: Some(Element::Fire), can_charge: false, tameable: true,
        },
        EnemyKind::Bat => EnemyInfo {
            name: "コウモリ", glyph: 'b', max_hp: 18, atk: 9, def: 2, exp: 10, gold: 12,
            drop: None,
            weakness: Some(Element::Thunder), can_charge: false, tameable: false,
        },
        EnemyKind::Skeleton => EnemyInfo {
            name: "スケルトン", glyph: 'k', max_hp: 45, atk: 14, def: 8, exp: 30, gold: 35,
            drop: Some((ItemKind::StrengthPotion, 20)),
            weakness: Some(Element::Fire), can_charge: false, tameable: false,
        },
        EnemyKind::Golem => EnemyInfo {
            name: "ゴーレム", glyph: 'G', max_hp: 60, atk: 16, def: 14, exp: 40, gold: 50,
            drop: Some((ItemKind::MagicWater, 30)),
            weakness: Some(Element::Thunder), can_charge: true, tameable: false,
        },
        EnemyKind::DarkKnight => EnemyInfo {
            name: "暗黒騎士", glyph: 'K', max_hp: 75, atk: 20, def: 15, exp: 55, gold: 70,
            drop: Some((ItemKind::StrengthPotion, 35)),
            weakness: Some(Element::Thunder), can_charge: true, tameable: false,
        },
        EnemyKind::Demon => EnemyInfo {
            name: "デーモン", glyph: 'D', max_hp: 85, atk: 22, def: 12, exp: 65, gold: 80,
            drop: Some((ItemKind::MagicWater, 40)),
            weakness: Some(Element::Ice), can_charge: false, tameable: false,
        },
        EnemyKind::Dragon => EnemyInfo {
            name: "ドラゴン", glyph: 'R', max_hp: 120, atk: 28, def: 18, exp: 100, gold: 150,
            drop: Some((ItemKind::Herb, 50)),
            weakness: Some(Element::Ice), can_charge: true, tameable: false,
        },
        EnemyKind::DemonLord => EnemyInfo {
            name: "魔王", glyph: 'L', max_hp: 200, atk: 32, def: 20, exp: 300, gold: 500,
            drop: None,
            weakness: None, can_charge: true, tameable: false,
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
    // Foods
    Bread,
    Jerky,
    CookedMeal,
    Apple,
    // Pet food
    PetTreat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemCategory {
    Consumable,
    Weapon,
    Armor,
    Food,
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
        ItemKind::Bread => ItemInfo {
            name: "パン", description: "満腹度+300",
            category: ItemCategory::Food, buy_price: 15, value: 300,
        },
        ItemKind::Jerky => ItemInfo {
            name: "干し肉", description: "満腹度+500",
            category: ItemCategory::Food, buy_price: 35, value: 500,
        },
        ItemKind::CookedMeal => ItemInfo {
            name: "温かい料理", description: "満腹度+800/HP少回復",
            category: ItemCategory::Food, buy_price: 90, value: 800,
        },
        ItemKind::Apple => ItemInfo {
            name: "リンゴ", description: "満腹度+150",
            category: ItemCategory::Food, buy_price: 8, value: 150,
        },
        ItemKind::PetTreat => ItemInfo {
            name: "ペットの餌", description: "野生の魔物に与えると懐く可能性",
            category: ItemCategory::Consumable, buy_price: 60, value: 0,
        },
    }
}

/// Shop inventory depends on player's max floor reached.
pub fn shop_items(max_floor: u32) -> Vec<(ItemKind, u32)> {
    let mut items = vec![
        (ItemKind::Herb, 99),
        (ItemKind::MagicWater, 99),
        (ItemKind::Bread, 99),
        (ItemKind::Apple, 99),
        (ItemKind::WoodenSword, 1),
        (ItemKind::TravelClothes, 1),
        (ItemKind::PetTreat, 99),
    ];
    if max_floor >= 2 {
        items.push((ItemKind::Jerky, 99));
        items.push((ItemKind::IronSword, 1));
        items.push((ItemKind::LeatherArmor, 1));
    }
    if max_floor >= 4 {
        items.push((ItemKind::CookedMeal, 99));
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

// ── Affixes (random magical equipment prefixes) ──────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Affix {
    /// 炎の: +Fire damage on attack
    Fire,
    /// 氷の: +Ice damage on attack
    Ice,
    /// 雷の: +Thunder damage on attack
    Thunder,
    /// 鋭利な: +ATK
    Sharp,
    /// 頑強な: +DEF
    Sturdy,
    /// 神秘の: +MAG
    Mystic,
    /// 吸血の: drain HP on hit
    Vampiric,
    /// 加護の: +max HP
    Blessed,
}

pub struct AffixInfo {
    pub prefix: &'static str,
    pub atk_bonus: i32,
    pub def_bonus: i32,
    pub mag_bonus: i32,
    pub max_hp_bonus: i32,
    pub element: Option<Element>,
    pub element_dmg: u32,
    pub vampiric_pct: u32, // % hp drain on attack
}

pub fn affix_info(affix: Affix) -> AffixInfo {
    match affix {
        Affix::Fire => AffixInfo {
            prefix: "炎の", atk_bonus: 0, def_bonus: 0, mag_bonus: 0, max_hp_bonus: 0,
            element: Some(Element::Fire), element_dmg: 4, vampiric_pct: 0,
        },
        Affix::Ice => AffixInfo {
            prefix: "氷の", atk_bonus: 0, def_bonus: 0, mag_bonus: 0, max_hp_bonus: 0,
            element: Some(Element::Ice), element_dmg: 4, vampiric_pct: 0,
        },
        Affix::Thunder => AffixInfo {
            prefix: "雷の", atk_bonus: 0, def_bonus: 0, mag_bonus: 0, max_hp_bonus: 0,
            element: Some(Element::Thunder), element_dmg: 4, vampiric_pct: 0,
        },
        Affix::Sharp => AffixInfo {
            prefix: "鋭利な", atk_bonus: 4, def_bonus: 0, mag_bonus: 0, max_hp_bonus: 0,
            element: None, element_dmg: 0, vampiric_pct: 0,
        },
        Affix::Sturdy => AffixInfo {
            prefix: "頑強な", atk_bonus: 0, def_bonus: 4, mag_bonus: 0, max_hp_bonus: 0,
            element: None, element_dmg: 0, vampiric_pct: 0,
        },
        Affix::Mystic => AffixInfo {
            prefix: "神秘の", atk_bonus: 0, def_bonus: 0, mag_bonus: 5, max_hp_bonus: 0,
            element: None, element_dmg: 0, vampiric_pct: 0,
        },
        Affix::Vampiric => AffixInfo {
            prefix: "吸血の", atk_bonus: 0, def_bonus: 0, mag_bonus: 0, max_hp_bonus: 0,
            element: None, element_dmg: 0, vampiric_pct: 25,
        },
        Affix::Blessed => AffixInfo {
            prefix: "加護の", atk_bonus: 0, def_bonus: 1, mag_bonus: 0, max_hp_bonus: 15,
            element: None, element_dmg: 0, vampiric_pct: 0,
        },
    }
}

pub const ALL_AFFIXES: &[Affix] = &[
    Affix::Fire, Affix::Ice, Affix::Thunder, Affix::Sharp,
    Affix::Sturdy, Affix::Mystic, Affix::Vampiric, Affix::Blessed,
];

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
            name: "ファイア", description: "炎で隣接敵を焼く (魔力依存)",
            mp_cost: 8, value: 3, learn_level: 1,
        },
        SkillKind::Heal => SkillInfo {
            name: "ヒール", description: "HPを回復 (魔力依存)",
            mp_cost: 6, value: 2, learn_level: 2,
        },
        SkillKind::IceBlade => SkillInfo {
            name: "アイスブレード", description: "氷の刃で隣接敵を斬る",
            mp_cost: 10, value: 2, learn_level: 3,
        },
        SkillKind::Shield => SkillInfo {
            name: "シールド", description: "数ターンDEF上昇",
            mp_cost: 5, value: 8, learn_level: 4,
        },
        SkillKind::Thunder => SkillInfo {
            name: "サンダー", description: "雷撃 (高威力・隣接)",
            mp_cost: 14, value: 4, learn_level: 5,
        },
        SkillKind::Drain => SkillInfo {
            name: "ドレイン", description: "HP吸収攻撃",
            mp_cost: 12, value: 2, learn_level: 6,
        },
        SkillKind::Berserk => SkillInfo {
            name: "バーサク", description: "数ターンATK大幅UP/DEF低下",
            mp_cost: 8, value: 15, learn_level: 8,
        },
    }
}

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

/// A stack of items in the inventory.
///
/// If `affix` is `Some`, the item is unique (cannot stack — each
/// affixed instance is its own entry, count is always 1).
#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub kind: ItemKind,
    pub count: u32,
    pub affix: Option<Affix>,
}

impl InventoryItem {
    /// Display name including affix prefix.
    pub fn display_name(&self) -> String {
        let base = item_info(self.kind).name;
        match self.affix {
            Some(a) => format!("{}{}", affix_info(a).prefix, base),
            None => base.to_string(),
        }
    }
}

// ── Dungeon Grid Map System ───────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Facing {
    North,
    East,
    South,
    West,
}

impl Facing {
    pub fn reverse(self) -> Self {
        match self {
            Facing::North => Facing::South,
            Facing::South => Facing::North,
            Facing::East => Facing::West,
            Facing::West => Facing::East,
        }
    }
    pub fn dx(self) -> i32 {
        match self {
            Facing::East => 1,
            Facing::West => -1,
            _ => 0,
        }
    }
    pub fn dy(self) -> i32 {
        match self {
            Facing::North => -1,
            Facing::South => 1,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tile {
    Wall,
    RoomFloor,
    Corridor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellType {
    Corridor,
    Entrance,
    Stairs,
    Treasure,
    Trap,
    Spring,
    Lore,
    Npc,
}

#[derive(Clone, Debug)]
pub struct MapCell {
    pub tile: Tile,
    pub cell_type: CellType,
    pub visited: bool,
    pub revealed: bool,
    /// Whether the event in this cell has been resolved.
    pub event_done: bool,
    pub room_id: Option<u8>,
}

impl MapCell {
    pub fn is_walkable(&self) -> bool {
        self.tile != Tile::Wall
    }
}

#[derive(Clone, Debug)]
pub struct Room {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// A monster on the dungeon grid (inline-combat entity).
#[derive(Clone, Debug)]
pub struct Monster {
    pub kind: EnemyKind,
    pub x: usize,
    pub y: usize,
    pub hp: u32,
    pub max_hp: u32,
    /// Whether the monster has noticed the player and is active.
    pub awake: bool,
    /// True when the monster is "winding up" a charge attack
    /// (telegraphed → next turn does double damage).
    pub charging: bool,
}

/// A pet/companion on the dungeon grid (ally entity).
#[derive(Clone, Debug)]
pub struct Pet {
    pub kind: EnemyKind,
    pub name: String,
    pub x: usize,
    pub y: usize,
    pub hp: u32,
    pub max_hp: u32,
    pub level: u32,
}

#[derive(Clone, Debug)]
pub struct DungeonMap {
    pub floor_num: u32,
    pub width: usize,
    pub height: usize,
    pub grid: Vec<Vec<MapCell>>,
    pub player_x: usize,
    pub player_y: usize,
    pub last_dir: Facing,
    pub rooms: Vec<Room>,
    pub monsters: Vec<Monster>,
}

impl DungeonMap {
    pub fn cell(&self, x: usize, y: usize) -> &MapCell {
        &self.grid[y][x]
    }
    pub fn player_cell(&self) -> &MapCell {
        &self.grid[self.player_y][self.player_x]
    }
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }
    /// Find index of monster at (x, y), if any.
    pub fn monster_at(&self, x: usize, y: usize) -> Option<usize> {
        self.monsters
            .iter()
            .position(|m| m.x == x && m.y == y && m.hp > 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloorTheme {
    MossyRuins,
    Underground,
    AncientTemple,
    VolcanicDepths,
    DemonCastle,
}

/// Interactive event with choices (treasure / spring / lore / npc).
#[derive(Clone, Debug)]
pub struct DungeonEvent {
    pub description: Vec<String>,
    pub choices: Vec<EventChoice>,
}

#[derive(Clone, Debug)]
pub struct EventChoice {
    pub label: String,
    pub action: EventAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventAction {
    OpenTreasure,
    SearchTreasure,
    Ignore,
    DrinkSpring,
    FillBottle,
    ReadLore,
    TalkNpc,
    TradeNpc,
    DescendStairs,
    ReturnToTown,
    Continue,
}

// ── Quests (Elona-style request board) ────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestKind {
    /// Slay N monsters of a specific kind on a target floor.
    Slay { target: EnemyKind, count: u32, floor: u32 },
    /// Reach a specific floor on this run.
    Reach { floor: u32 },
    /// Collect N specific items.
    Collect { item: ItemKind, count: u32 },
}

#[derive(Clone, Debug)]
pub struct Quest {
    pub kind: QuestKind,
    pub reward_gold: u32,
    pub reward_exp: u32,
    /// Progress counter (e.g. monsters slain so far).
    pub progress: u32,
}

impl Quest {
    pub fn is_complete(&self) -> bool {
        match self.kind {
            QuestKind::Slay { count, .. } => self.progress >= count,
            QuestKind::Reach { floor } => self.progress >= floor,
            QuestKind::Collect { count, .. } => self.progress >= count,
        }
    }

    pub fn description(&self) -> String {
        match self.kind {
            QuestKind::Slay { target, count, floor } => {
                format!(
                    "{}をB{}Fで{}体討伐 ({}/{})",
                    enemy_info(target).name,
                    floor,
                    count,
                    self.progress.min(count),
                    count
                )
            }
            QuestKind::Reach { floor } => {
                format!(
                    "B{}Fまで到達 ({}/{})",
                    floor,
                    self.progress.min(floor),
                    floor
                )
            }
            QuestKind::Collect { item, count } => {
                format!(
                    "{}を{}個集める ({}/{})",
                    item_info(item).name,
                    count,
                    self.progress.min(count),
                    count
                )
            }
        }
    }
}

// ── Scene System ─────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scene {
    Intro(u8),
    Town,
    DungeonExplore,
    DungeonEvent,
    GameClear,
}

/// Overlay screens drawn on top of the main scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    Inventory,
    Status,
    Shop,
    /// Skill list — pick one to cast (counts as a turn).
    SkillMenu,
    /// Quest board (town).
    QuestBoard,
    /// Pray confirmation (town).
    PrayMenu,
}

// ── Player Status Effects ─────────────────────────────────────

/// Temporary buffs applied to the player. Decremented each turn.
#[derive(Clone, Debug, Default)]
pub struct PlayerBuffs {
    /// Extra DEF from Shield skill.
    pub shield_turns: u32,
    pub shield_value: u32,
    /// Extra ATK from Berserk.
    pub berserk_turns: u32,
    pub berserk_atk: u32,
    /// Extra ATK from Strength Potion (in dungeon).
    pub potion_turns: u32,
    pub potion_atk: u32,
}

impl PlayerBuffs {
    pub fn def_bonus(&self) -> u32 {
        if self.shield_turns > 0 { self.shield_value } else { 0 }
    }
    pub fn atk_bonus(&self) -> u32 {
        let mut a = 0;
        if self.berserk_turns > 0 { a += self.berserk_atk; }
        if self.potion_turns > 0 { a += self.potion_atk; }
        a
    }
    pub fn def_penalty(&self) -> u32 {
        if self.berserk_turns > 0 { 5 } else { 0 }
    }
    pub fn tick_down(&mut self) {
        if self.shield_turns > 0 { self.shield_turns -= 1; }
        if self.berserk_turns > 0 { self.berserk_turns -= 1; }
        if self.potion_turns > 0 { self.potion_turns -= 1; }
    }
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

    // Equipment (indices into inventory; equipped item not removed from inv)
    pub weapon_idx: Option<usize>,
    pub armor_idx: Option<usize>,

    // Inventory (supports affixed unique items)
    pub inventory: Vec<InventoryItem>,

    // Dungeon
    pub dungeon: Option<DungeonMap>,
    pub max_floor_reached: u32,
    pub total_clears: u32,

    // Scene system
    pub scene: Scene,
    pub overlay: Option<Overlay>,
    pub scene_text: Vec<String>,

    pub active_event: Option<DungeonEvent>,

    // Log (shown at bottom)
    pub log: Vec<String>,

    // Game state
    pub game_cleared: bool,
    pub rng_seed: u64,

    // Stats for current dungeon run
    pub run_gold_earned: u32,
    pub run_exp_earned: u32,
    pub run_enemies_killed: u32,
    pub run_rooms_explored: u32,

    // Lore collected
    pub lore_found: Vec<u32>,

    // ── Elona-flavor extensions ──

    /// Satiety (満腹度): 0..=satiety_max. Drains each turn in dungeon.
    pub satiety: u32,
    pub satiety_max: u32,

    /// Faith (信仰度): grows on prayer / floor clears. Affects pray outcomes.
    pub faith: u32,
    /// Whether the player has prayed in the current dungeon run.
    pub prayed_this_run: bool,

    /// Currently accepted quest.
    pub active_quest: Option<Quest>,
    pub completed_quests: u32,

    /// Player's pet companion.
    pub pet: Option<Pet>,

    /// Status effect buffs (shield, berserk, potion).
    pub buffs: PlayerBuffs,

    /// Counter that increments on each player action (turn-based).
    pub turn_count: u64,
}

pub const SATIETY_MAX_DEFAULT: u32 = 1000;

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
            weapon_idx: None,
            armor_idx: None,
            inventory: Vec::new(),
            dungeon: None,
            max_floor_reached: 0,
            total_clears: 0,
            scene: Scene::Intro(0),
            overlay: None,
            scene_text: Vec::new(),
            active_event: None,
            log: Vec::new(),
            game_cleared: false,
            rng_seed: 42,
            run_gold_earned: 0,
            run_exp_earned: 0,
            run_enemies_killed: 0,
            run_rooms_explored: 0,
            lore_found: Vec::new(),
            satiety: SATIETY_MAX_DEFAULT,
            satiety_max: SATIETY_MAX_DEFAULT,
            faith: 0,
            prayed_this_run: false,
            active_quest: None,
            completed_quests: 0,
            pet: None,
            buffs: PlayerBuffs::default(),
            turn_count: 0,
        }
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    /// Equipped weapon entry.
    pub fn weapon(&self) -> Option<&InventoryItem> {
        self.weapon_idx.and_then(|i| self.inventory.get(i))
    }
    /// Equipped armor entry.
    pub fn armor(&self) -> Option<&InventoryItem> {
        self.armor_idx.and_then(|i| self.inventory.get(i))
    }

    /// Total ATK including weapon base value, weapon affix bonus, and buffs.
    pub fn total_atk(&self) -> u32 {
        let mut atk = self.base_atk;
        if let Some(w) = self.weapon() {
            atk += item_info(w.kind).value;
            if let Some(a) = w.affix {
                atk = (atk as i32 + affix_info(a).atk_bonus).max(0) as u32;
            }
        }
        atk + self.buffs.atk_bonus()
    }

    pub fn total_def(&self) -> u32 {
        let mut def = self.base_def;
        if let Some(a) = self.armor() {
            def += item_info(a.kind).value;
            if let Some(af) = a.affix {
                def = (def as i32 + affix_info(af).def_bonus).max(0) as u32;
            }
        }
        let bonus = self.buffs.def_bonus();
        let pen = self.buffs.def_penalty();
        def.saturating_add(bonus).saturating_sub(pen)
    }

    /// Effective magic (includes affixes on equipped weapon/armor).
    pub fn total_mag(&self) -> u32 {
        let mut m = self.mag as i32;
        if let Some(w) = self.weapon() {
            if let Some(a) = w.affix {
                m += affix_info(a).mag_bonus;
            }
        }
        if let Some(ar) = self.armor() {
            if let Some(a) = ar.affix {
                m += affix_info(a).mag_bonus;
            }
        }
        m.max(0) as u32
    }

    /// Effective max HP (includes Blessed affixes).
    pub fn effective_max_hp(&self) -> u32 {
        let mut hp = self.max_hp as i32;
        if let Some(w) = self.weapon() {
            if let Some(a) = w.affix { hp += affix_info(a).max_hp_bonus; }
        }
        if let Some(ar) = self.armor() {
            if let Some(a) = ar.affix { hp += affix_info(a).max_hp_bonus; }
        }
        hp.max(1) as u32
    }

    /// Element of equipped weapon (for elemental damage on attack).
    pub fn weapon_element(&self) -> Option<Element> {
        self.weapon()
            .and_then(|w| w.affix)
            .and_then(|a| affix_info(a).element)
    }
    pub fn weapon_element_dmg(&self) -> u32 {
        self.weapon()
            .and_then(|w| w.affix)
            .map(|a| affix_info(a).element_dmg)
            .unwrap_or(0)
    }
    pub fn weapon_vampiric_pct(&self) -> u32 {
        self.weapon()
            .and_then(|w| w.affix)
            .map(|a| affix_info(a).vampiric_pct)
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn item_count(&self, kind: ItemKind) -> u32 {
        self.inventory
            .iter()
            .filter(|i| i.kind == kind && i.affix.is_none())
            .map(|i| i.count)
            .sum()
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
        assert_eq!(s.satiety, SATIETY_MAX_DEFAULT);
        assert_eq!(s.faith, 0);
        assert!(s.active_quest.is_none());
        assert!(s.pet.is_none());
    }

    #[test]
    fn total_atk_without_weapon() {
        let s = RpgState::new();
        assert_eq!(s.total_atk(), 5);
    }

    #[test]
    fn total_atk_with_weapon() {
        let mut s = RpgState::new();
        s.inventory.push(InventoryItem {
            kind: ItemKind::IronSword, count: 1, affix: None,
        });
        s.weapon_idx = Some(0);
        assert_eq!(s.total_atk(), 13); // 5 base + 8 sword
    }

    #[test]
    fn total_atk_with_affixed_weapon() {
        let mut s = RpgState::new();
        s.inventory.push(InventoryItem {
            kind: ItemKind::IronSword, count: 1, affix: Some(Affix::Sharp),
        });
        s.weapon_idx = Some(0);
        // 5 base + 8 sword + 4 Sharp affix = 17
        assert_eq!(s.total_atk(), 17);
    }

    #[test]
    fn affix_display_name_includes_prefix() {
        let it = InventoryItem {
            kind: ItemKind::IronSword, count: 1, affix: Some(Affix::Fire),
        };
        assert_eq!(it.display_name(), "炎の鉄の剣");
    }

    #[test]
    fn buffs_tick_down() {
        let mut b = PlayerBuffs {
            shield_turns: 3,
            shield_value: 8,
            ..Default::default()
        };
        assert_eq!(b.def_bonus(), 8);
        b.tick_down();
        b.tick_down();
        b.tick_down();
        assert_eq!(b.def_bonus(), 0);
    }

    #[test]
    fn quest_progress_complete() {
        let q = Quest {
            kind: QuestKind::Slay {
                target: EnemyKind::Slime, count: 3, floor: 1,
            },
            reward_gold: 50,
            reward_exp: 20,
            progress: 3,
        };
        assert!(q.is_complete());
    }

    #[test]
    fn item_count_excludes_affixed() {
        let mut s = RpgState::new();
        s.inventory.push(InventoryItem {
            kind: ItemKind::Herb, count: 5, affix: None,
        });
        s.inventory.push(InventoryItem {
            kind: ItemKind::Herb, count: 1, affix: Some(Affix::Blessed),
        });
        // Only stackable count
        assert_eq!(s.item_count(ItemKind::Herb), 5);
    }

    #[test]
    fn facing_reverse() {
        assert_eq!(Facing::North.reverse(), Facing::South);
        assert_eq!(Facing::East.reverse(), Facing::West);
    }

    #[test]
    fn shop_includes_food() {
        let s = shop_items(0);
        let kinds: Vec<ItemKind> = s.iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&ItemKind::Bread));
        assert!(kinds.contains(&ItemKind::Apple));
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

    #[test]
    fn all_affixes_have_info() {
        for &a in ALL_AFFIXES {
            let info = affix_info(a);
            assert!(!info.prefix.is_empty());
        }
    }

    #[test]
    fn dungeon_monster_at() {
        let map = DungeonMap {
            floor_num: 1,
            width: 5,
            height: 5,
            grid: vec![vec![MapCell {
                tile: Tile::RoomFloor,
                cell_type: CellType::Corridor,
                visited: false,
                revealed: false,
                event_done: false,
                room_id: None,
            }; 5]; 5],
            player_x: 0,
            player_y: 0,
            last_dir: Facing::North,
            rooms: Vec::new(),
            monsters: vec![Monster {
                kind: EnemyKind::Slime,
                x: 2, y: 2, hp: 12, max_hp: 12,
                awake: false, charging: false,
            }],
        };
        assert_eq!(map.monster_at(2, 2), Some(0));
        assert_eq!(map.monster_at(0, 0), None);
    }
}
