//! RPG Quest game state — all data structures, no logic.

// ── Locations ─────────────────────────────────────────────────

/// Location identifiers. Add new locations here to expand the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocationId {
    StartVillage,
    Forest,
    HiddenLake,
    Cave,
    MountainPath,
    DemonCastle,
}

pub struct LocationInfo {
    pub name: &'static str,
    pub description: &'static str,
    /// Adjacent locations the player can travel to.
    pub connections: &'static [LocationId],
    /// Whether random encounters can happen here.
    pub has_encounters: bool,
    /// Whether there is a shop here.
    pub has_shop: bool,
    /// Whether there is an NPC to talk to here.
    pub has_npc: bool,
}

pub fn location_info(id: LocationId) -> LocationInfo {
    match id {
        LocationId::StartVillage => LocationInfo {
            name: "始まりの村",
            description: "穏やかな村。長老が冒険者を待っている。",
            connections: &[LocationId::Forest],
            has_encounters: false,
            has_shop: true,
            has_npc: true,
        },
        LocationId::Forest => LocationInfo {
            name: "迷いの森",
            description: "木々が鬱蒼と茂る森。モンスターが潜んでいる。",
            connections: &[LocationId::StartVillage, LocationId::HiddenLake, LocationId::Cave],
            has_encounters: true,
            has_shop: false,
            has_npc: false,
        },
        LocationId::HiddenLake => LocationInfo {
            name: "隠された湖",
            description: "森の奥にひっそりと佇む美しい湖。",
            connections: &[LocationId::Forest],
            has_encounters: false,
            has_shop: false,
            has_npc: true,
        },
        LocationId::Cave => LocationInfo {
            name: "古代の洞窟",
            description: "壁面に古代文字が刻まれた暗い洞窟。",
            connections: &[LocationId::Forest, LocationId::MountainPath],
            has_encounters: true,
            has_shop: false,
            has_npc: false,
        },
        LocationId::MountainPath => LocationInfo {
            name: "険しい山道",
            description: "魔王城へ続く険しい山道。強力な敵が待ち構える。",
            connections: &[LocationId::Cave, LocationId::DemonCastle],
            has_encounters: true,
            has_shop: false,
            has_npc: false,
        },
        LocationId::DemonCastle => LocationInfo {
            name: "魔王城",
            description: "禍々しいオーラに包まれた魔王の居城。",
            connections: &[LocationId::MountainPath],
            has_encounters: true,
            has_shop: false,
            has_npc: false,
        },
    }
}

/// All locations for iteration.
#[cfg(test)]
pub const ALL_LOCATIONS: &[LocationId] = &[
    LocationId::StartVillage,
    LocationId::Forest,
    LocationId::HiddenLake,
    LocationId::Cave,
    LocationId::MountainPath,
    LocationId::DemonCastle,
];

// ── Enemies ───────────────────────────────────────────────────

/// Enemy type identifiers. Add new enemies here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnemyKind {
    Slime,
    Wolf,
    Goblin,
    Golem,
    DarkKnight,
    DemonLord,
}

pub struct EnemyInfo {
    pub name: &'static str,
    pub max_hp: u32,
    pub atk: u32,
    pub def: u32,
    pub exp: u32,
    pub gold: u32,
    /// Drop item (if any) and drop chance (0.0-1.0).
    pub drop: Option<(ItemKind, f64)>,
}

pub fn enemy_info(kind: EnemyKind) -> EnemyInfo {
    match kind {
        EnemyKind::Slime => EnemyInfo {
            name: "スライム",
            max_hp: 15,
            atk: 4,
            def: 1,
            exp: 5,
            gold: 8,
            drop: Some((ItemKind::Herb, 0.4)),
        },
        EnemyKind::Wolf => EnemyInfo {
            name: "オオカミ",
            max_hp: 25,
            atk: 8,
            def: 3,
            exp: 12,
            gold: 15,
            drop: Some((ItemKind::Herb, 0.3)),
        },
        EnemyKind::Goblin => EnemyInfo {
            name: "ゴブリン",
            max_hp: 35,
            atk: 12,
            def: 5,
            exp: 20,
            gold: 25,
            drop: Some((ItemKind::MagicWater, 0.3)),
        },
        EnemyKind::Golem => EnemyInfo {
            name: "ゴーレム",
            max_hp: 60,
            atk: 15,
            def: 12,
            exp: 40,
            gold: 50,
            drop: Some((ItemKind::StrengthPotion, 0.2)),
        },
        EnemyKind::DarkKnight => EnemyInfo {
            name: "暗黒騎士",
            max_hp: 80,
            atk: 20,
            def: 15,
            exp: 60,
            gold: 80,
            drop: Some((ItemKind::MagicWater, 0.4)),
        },
        EnemyKind::DemonLord => EnemyInfo {
            name: "魔王",
            max_hp: 150,
            atk: 28,
            def: 18,
            exp: 200,
            gold: 500,
            drop: None,
        },
    }
}

/// Encounter table per location. Returns possible enemy kinds.
pub fn encounter_table(loc: LocationId) -> &'static [EnemyKind] {
    match loc {
        LocationId::Forest => &[EnemyKind::Slime, EnemyKind::Wolf],
        LocationId::Cave => &[EnemyKind::Goblin, EnemyKind::Golem],
        LocationId::MountainPath => &[EnemyKind::DarkKnight, EnemyKind::Golem],
        LocationId::DemonCastle => &[EnemyKind::DarkKnight, EnemyKind::DemonLord],
        _ => &[],
    }
}

// ── Items ─────────────────────────────────────────────────────

/// Item identifiers. Add new items here.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Variants reserved for future content expansion
pub enum ItemKind {
    // Consumables
    Herb,
    MagicWater,
    StrengthPotion,
    // Weapons
    WoodenSword,
    IronSword,
    SteelSword,
    HolySword,
    // Armor
    TravelClothes,
    LeatherArmor,
    ChainMail,
    KnightArmor,
    // Key items
    AncientKey,
    LakeTreasure,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemCategory {
    Consumable,
    Weapon,
    Armor,
    KeyItem,
}

#[allow(dead_code)] // sell_price reserved for future sell feature
pub struct ItemInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ItemCategory,
    pub buy_price: u32,
    pub sell_price: u32,
    /// For weapons: ATK bonus. For armor: DEF bonus. For consumables: effect value.
    pub value: u32,
}

pub fn item_info(kind: ItemKind) -> ItemInfo {
    match kind {
        ItemKind::Herb => ItemInfo {
            name: "薬草",
            description: "HPを30回復する",
            category: ItemCategory::Consumable,
            buy_price: 20,
            sell_price: 10,
            value: 30,
        },
        ItemKind::MagicWater => ItemInfo {
            name: "魔法の水",
            description: "MPを20回復する",
            category: ItemCategory::Consumable,
            buy_price: 50,
            sell_price: 25,
            value: 20,
        },
        ItemKind::StrengthPotion => ItemInfo {
            name: "力の薬",
            description: "戦闘中ATK+5",
            category: ItemCategory::Consumable,
            buy_price: 80,
            sell_price: 40,
            value: 5,
        },
        ItemKind::WoodenSword => ItemInfo {
            name: "木の剣",
            description: "ATK+3",
            category: ItemCategory::Weapon,
            buy_price: 30,
            sell_price: 15,
            value: 3,
        },
        ItemKind::IronSword => ItemInfo {
            name: "鉄の剣",
            description: "ATK+8",
            category: ItemCategory::Weapon,
            buy_price: 120,
            sell_price: 60,
            value: 8,
        },
        ItemKind::SteelSword => ItemInfo {
            name: "鋼の剣",
            description: "ATK+15",
            category: ItemCategory::Weapon,
            buy_price: 350,
            sell_price: 175,
            value: 15,
        },
        ItemKind::HolySword => ItemInfo {
            name: "聖剣",
            description: "ATK+25",
            category: ItemCategory::Weapon,
            buy_price: 1000,
            sell_price: 500,
            value: 25,
        },
        ItemKind::TravelClothes => ItemInfo {
            name: "旅人の服",
            description: "DEF+2",
            category: ItemCategory::Armor,
            buy_price: 20,
            sell_price: 10,
            value: 2,
        },
        ItemKind::LeatherArmor => ItemInfo {
            name: "革の鎧",
            description: "DEF+5",
            category: ItemCategory::Armor,
            buy_price: 100,
            sell_price: 50,
            value: 5,
        },
        ItemKind::ChainMail => ItemInfo {
            name: "鎖帷子",
            description: "DEF+12",
            category: ItemCategory::Armor,
            buy_price: 300,
            sell_price: 150,
            value: 12,
        },
        ItemKind::KnightArmor => ItemInfo {
            name: "騎士の鎧",
            description: "DEF+20",
            category: ItemCategory::Armor,
            buy_price: 800,
            sell_price: 400,
            value: 20,
        },
        ItemKind::AncientKey => ItemInfo {
            name: "古代の鍵",
            description: "山道への扉を開ける鍵",
            category: ItemCategory::KeyItem,
            buy_price: 0,
            sell_price: 0,
            value: 0,
        },
        ItemKind::LakeTreasure => ItemInfo {
            name: "湖の秘宝",
            description: "美しく輝く不思議な宝石",
            category: ItemCategory::KeyItem,
            buy_price: 0,
            sell_price: 0,
            value: 0,
        },
    }
}

/// Shop inventory per location. Returns (item, stock_count).
pub fn shop_inventory(loc: LocationId) -> &'static [(ItemKind, u32)] {
    match loc {
        LocationId::StartVillage => &[
            (ItemKind::Herb, 99),
            (ItemKind::MagicWater, 99),
            (ItemKind::WoodenSword, 1),
            (ItemKind::IronSword, 1),
            (ItemKind::TravelClothes, 1),
            (ItemKind::LeatherArmor, 1),
        ],
        _ => &[],
    }
}

// ── Skills ────────────────────────────────────────────────────

/// Skill identifiers. Add new skills here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillKind {
    Fire,
    Heal,
    Shield,
}

#[allow(dead_code)] // is_offensive reserved for future skill targeting system
pub struct SkillInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub mp_cost: u32,
    /// For attack skills: damage multiplier (times MAG).
    /// For heal: heal amount base.
    /// For shield: DEF boost.
    pub value: u32,
    pub is_offensive: bool,
    /// Level required to learn this skill.
    pub learn_level: u32,
}

pub fn skill_info(kind: SkillKind) -> SkillInfo {
    match kind {
        SkillKind::Fire => SkillInfo {
            name: "ファイア",
            description: "炎で敵を攻撃 (魔力依存)",
            mp_cost: 8,
            value: 3,
            is_offensive: true,
            learn_level: 1,
        },
        SkillKind::Heal => SkillInfo {
            name: "ヒール",
            description: "HPを回復 (魔力依存)",
            mp_cost: 6,
            value: 2,
            is_offensive: false,
            learn_level: 2,
        },
        SkillKind::Shield => SkillInfo {
            name: "シールド",
            description: "戦闘中DEF上昇",
            mp_cost: 5,
            value: 8,
            is_offensive: false,
            learn_level: 4,
        },
    }
}

pub const ALL_SKILLS: &[SkillKind] = &[SkillKind::Fire, SkillKind::Heal, SkillKind::Shield];

// ── Quests ────────────────────────────────────────────────────

/// Quest identifiers. Add new quests here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuestId {
    // Main quest chain
    MainPrepare,
    MainForest,
    MainCave,
    MainMountain,
    MainFinal,
    // Side quests
    SideHerbCollect,
    SideLakeTreasure,
    SideWolfHunt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestKind {
    Main,
    Side,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QuestGoal {
    /// Talk to NPC at a location.
    TalkNpc(LocationId),
    /// Defeat N enemies of a specific kind.
    DefeatEnemies(EnemyKind, u32),
    /// Find an item (obtained through explore action).
    FindItem(ItemKind),
    /// Defeat a specific boss.
    DefeatBoss(EnemyKind),
}

#[derive(Clone, Debug)]
pub struct QuestInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: QuestKind,
    pub goal: QuestGoal,
    pub reward_gold: u32,
    pub reward_exp: u32,
    pub reward_item: Option<ItemKind>,
    /// Quest that must be completed before this one is available.
    pub prerequisite: Option<QuestId>,
    /// Location where this quest is accepted.
    pub accept_location: LocationId,
}

pub fn quest_info(id: QuestId) -> QuestInfo {
    match id {
        QuestId::MainPrepare => QuestInfo {
            name: "旅立ちの準備",
            description: "村の長老に話しかけて装備を受け取ろう",
            kind: QuestKind::Main,
            goal: QuestGoal::TalkNpc(LocationId::StartVillage),
            reward_gold: 50,
            reward_exp: 10,
            reward_item: Some(ItemKind::WoodenSword),
            prerequisite: None,
            accept_location: LocationId::StartVillage,
        },
        QuestId::MainForest => QuestInfo {
            name: "森の脅威",
            description: "森でスライムを3体倒して安全を確保せよ",
            kind: QuestKind::Main,
            goal: QuestGoal::DefeatEnemies(EnemyKind::Slime, 3),
            reward_gold: 80,
            reward_exp: 30,
            reward_item: None,
            prerequisite: Some(QuestId::MainPrepare),
            accept_location: LocationId::StartVillage,
        },
        QuestId::MainCave => QuestInfo {
            name: "洞窟の秘密",
            description: "古代の洞窟で「古代の鍵」を見つけ出せ",
            kind: QuestKind::Main,
            goal: QuestGoal::FindItem(ItemKind::AncientKey),
            reward_gold: 120,
            reward_exp: 50,
            reward_item: None,
            prerequisite: Some(QuestId::MainForest),
            accept_location: LocationId::StartVillage,
        },
        QuestId::MainMountain => QuestInfo {
            name: "山道の試練",
            description: "山道を守る暗黒騎士を倒せ",
            kind: QuestKind::Main,
            goal: QuestGoal::DefeatBoss(EnemyKind::DarkKnight),
            reward_gold: 200,
            reward_exp: 80,
            reward_item: Some(ItemKind::SteelSword),
            prerequisite: Some(QuestId::MainCave),
            accept_location: LocationId::Cave,
        },
        QuestId::MainFinal => QuestInfo {
            name: "最終決戦",
            description: "魔王を倒して世界に平和を取り戻せ！",
            kind: QuestKind::Main,
            goal: QuestGoal::DefeatBoss(EnemyKind::DemonLord),
            reward_gold: 1000,
            reward_exp: 500,
            reward_item: None,
            prerequisite: Some(QuestId::MainMountain),
            accept_location: LocationId::MountainPath,
        },
        QuestId::SideHerbCollect => QuestInfo {
            name: "薬草集め",
            description: "薬師のために薬草を3つ集めてこい",
            kind: QuestKind::Side,
            goal: QuestGoal::FindItem(ItemKind::Herb),
            reward_gold: 60,
            reward_exp: 20,
            reward_item: Some(ItemKind::MagicWater),
            prerequisite: Some(QuestId::MainPrepare),
            accept_location: LocationId::StartVillage,
        },
        QuestId::SideLakeTreasure => QuestInfo {
            name: "湖の宝物",
            description: "隠された湖で伝説の秘宝を探せ",
            kind: QuestKind::Side,
            goal: QuestGoal::FindItem(ItemKind::LakeTreasure),
            reward_gold: 150,
            reward_exp: 40,
            reward_item: Some(ItemKind::ChainMail),
            prerequisite: Some(QuestId::MainForest),
            accept_location: LocationId::HiddenLake,
        },
        QuestId::SideWolfHunt => QuestInfo {
            name: "狼退治",
            description: "森のオオカミを5体退治してくれ",
            kind: QuestKind::Side,
            goal: QuestGoal::DefeatEnemies(EnemyKind::Wolf, 5),
            reward_gold: 100,
            reward_exp: 35,
            reward_item: Some(ItemKind::IronSword),
            prerequisite: Some(QuestId::MainPrepare),
            accept_location: LocationId::StartVillage,
        },
    }
}

/// All quests for iteration.
pub const ALL_QUESTS: &[QuestId] = &[
    QuestId::MainPrepare,
    QuestId::MainForest,
    QuestId::MainCave,
    QuestId::MainMountain,
    QuestId::MainFinal,
    QuestId::SideHerbCollect,
    QuestId::SideLakeTreasure,
    QuestId::SideWolfHunt,
];

// ── Quest Progress ────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestStatus {
    /// Not yet accepted.
    Available,
    /// Accepted, in progress.
    Active,
    /// Goal completed, ready to turn in.
    ReadyToComplete,
    /// Completed and rewards claimed.
    Completed,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // counter reserved for DefeatEnemies quest tracking
pub struct QuestProgress {
    pub quest_id: QuestId,
    pub status: QuestStatus,
    /// Current count for DefeatEnemies goals.
    pub counter: u32,
}

// ── Battle State ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct BattleEnemy {
    pub kind: EnemyKind,
    pub hp: u32,
    pub max_hp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // EnemyTurn reserved for future turn-based combat expansion
pub enum BattleAction {
    SelectAction,
    SelectSkill,
    SelectItem,
    EnemyTurn,
    Victory,
    Defeat,
    Fled,
}

#[derive(Clone, Debug)]
pub struct BattleState {
    pub enemy: BattleEnemy,
    pub action: BattleAction,
    pub player_def_boost: u32,
    pub player_atk_boost: u32,
    pub battle_log: Vec<String>,
    /// Is this a boss encounter?
    pub is_boss: bool,
}

// ── Screens ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    World,
    Battle,
    Inventory,
    QuestLog,
    Shop,
    Status,
    Dialogue,
    GameClear,
}

// ── Level / EXP Table ─────────────────────────────────────────

pub struct LevelStats {
    pub max_hp: u32,
    pub max_mp: u32,
    pub atk: u32,
    pub def: u32,
    pub mag: u32,
    pub exp_to_next: u32,
}

/// Returns stats for a given level. Extensible by adding more entries.
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

// ── Inventory Entry ───────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub kind: ItemKind,
    pub count: u32,
}

// ── Dialogue ──────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DialogueState {
    pub lines: Vec<String>,
    pub current_line: usize,
}

// ── Root Game State ───────────────────────────────────────────

pub struct RpgState {
    // Player stats
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

    // Location
    pub location: LocationId,

    // Inventory
    pub inventory: Vec<InventoryItem>,

    // Quests
    pub quests: Vec<QuestProgress>,

    // Battle
    pub battle: Option<BattleState>,

    // Dialogue
    pub dialogue: Option<DialogueState>,

    // UI
    pub screen: Screen,
    pub log: Vec<String>,

    // Game state
    pub game_cleared: bool,
    pub rng_seed: u64,

    // Kill counters for quest tracking
    pub kill_counts: Vec<(EnemyKind, u32)>,
}

impl RpgState {
    pub fn new() -> Self {
        let stats = level_stats(1);

        // Initialize all quests
        let mut quests = Vec::new();
        for &id in ALL_QUESTS {
            let info = quest_info(id);
            let status = if info.prerequisite.is_none() {
                QuestStatus::Available
            } else {
                // Will be unlocked as prerequisites are completed
                QuestStatus::Available
            };
            quests.push(QuestProgress {
                quest_id: id,
                status: if info.prerequisite.is_none() {
                    status
                } else {
                    // Hidden until prerequisite is met — we'll check in logic
                    QuestStatus::Available
                },
                counter: 0,
            });
        }

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
            location: LocationId::StartVillage,
            inventory: Vec::new(),
            quests,
            battle: None,
            dialogue: None,
            screen: Screen::World,
            log: vec!["冒険の世界へようこそ！".into()],
            game_cleared: false,
            rng_seed: 42,
            kill_counts: Vec::new(),
        }
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    /// Total ATK including weapon bonus.
    pub fn total_atk(&self) -> u32 {
        let weapon_bonus = self
            .weapon
            .map(|w| item_info(w).value)
            .unwrap_or(0);
        self.base_atk + weapon_bonus
    }

    /// Total DEF including armor bonus.
    pub fn total_def(&self) -> u32 {
        let armor_bonus = self
            .armor
            .map(|a| item_info(a).value)
            .unwrap_or(0);
        self.base_def + armor_bonus
    }

    /// Get count of an item in inventory.
    pub fn item_count(&self, kind: ItemKind) -> u32 {
        self.inventory
            .iter()
            .find(|i| i.kind == kind)
            .map(|i| i.count)
            .unwrap_or(0)
    }

    /// Get kill count for an enemy kind.
    pub fn kill_count(&self, kind: EnemyKind) -> u32 {
        self.kill_counts
            .iter()
            .find(|k| k.0 == kind)
            .map(|k| k.1)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = RpgState::new();
        assert_eq!(s.level, 1);
        assert_eq!(s.hp, 50);
        assert_eq!(s.max_hp, 50);
        assert_eq!(s.gold, 0);
        assert_eq!(s.location, LocationId::StartVillage);
        assert_eq!(s.screen, Screen::World);
        assert!(!s.game_cleared);
        assert!(s.weapon.is_none());
        assert!(s.armor.is_none());
    }

    #[test]
    fn total_atk_without_weapon() {
        let s = RpgState::new();
        assert_eq!(s.total_atk(), 5); // base ATK at level 1
    }

    #[test]
    fn total_atk_with_weapon() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::IronSword); // +8
        assert_eq!(s.total_atk(), 13);
    }

    #[test]
    fn total_def_with_armor() {
        let mut s = RpgState::new();
        s.armor = Some(ItemKind::LeatherArmor); // +5
        assert_eq!(s.total_def(), 8); // 3 base + 5
    }

    #[test]
    fn item_count_empty() {
        let s = RpgState::new();
        assert_eq!(s.item_count(ItemKind::Herb), 0);
    }

    #[test]
    fn item_count_with_items() {
        let mut s = RpgState::new();
        s.inventory.push(InventoryItem {
            kind: ItemKind::Herb,
            count: 5,
        });
        assert_eq!(s.item_count(ItemKind::Herb), 5);
        assert_eq!(s.item_count(ItemKind::MagicWater), 0);
    }

    #[test]
    fn kill_count_tracking() {
        let mut s = RpgState::new();
        assert_eq!(s.kill_count(EnemyKind::Slime), 0);
        s.kill_counts.push((EnemyKind::Slime, 3));
        assert_eq!(s.kill_count(EnemyKind::Slime), 3);
    }

    #[test]
    fn all_locations_have_info() {
        for &loc in ALL_LOCATIONS {
            let info = location_info(loc);
            assert!(!info.name.is_empty());
            assert!(!info.description.is_empty());
        }
    }

    #[test]
    fn all_quests_have_info() {
        for &id in ALL_QUESTS {
            let info = quest_info(id);
            assert!(!info.name.is_empty());
            assert!(!info.description.is_empty());
        }
    }

    #[test]
    fn village_has_shop_and_npc() {
        let info = location_info(LocationId::StartVillage);
        assert!(info.has_shop);
        assert!(info.has_npc);
        assert!(!info.has_encounters);
    }

    #[test]
    fn forest_has_encounters() {
        let info = location_info(LocationId::Forest);
        assert!(info.has_encounters);
        let table = encounter_table(LocationId::Forest);
        assert!(!table.is_empty());
    }

    #[test]
    fn level_stats_increase() {
        let l1 = level_stats(1);
        let l5 = level_stats(5);
        assert!(l5.max_hp > l1.max_hp);
        assert!(l5.atk > l1.atk);
        assert!(l5.max_mp > l1.max_mp);
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
    fn main_quest_chain_has_prerequisites() {
        let q1 = quest_info(QuestId::MainPrepare);
        assert!(q1.prerequisite.is_none());

        let q2 = quest_info(QuestId::MainForest);
        assert_eq!(q2.prerequisite, Some(QuestId::MainPrepare));

        let q3 = quest_info(QuestId::MainCave);
        assert_eq!(q3.prerequisite, Some(QuestId::MainForest));

        let q4 = quest_info(QuestId::MainMountain);
        assert_eq!(q4.prerequisite, Some(QuestId::MainCave));

        let q5 = quest_info(QuestId::MainFinal);
        assert_eq!(q5.prerequisite, Some(QuestId::MainMountain));
    }

    #[test]
    fn shop_inventory_valid() {
        let items = shop_inventory(LocationId::StartVillage);
        assert!(!items.is_empty());
        for &(kind, count) in items {
            let info = item_info(kind);
            assert!(info.buy_price > 0);
            assert!(count > 0);
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
