//! RPG Quest game state — all data structures, no logic.
//!
//! Design: "Story RPG" — single screen that changes with context.
//! Scene-based system instead of multi-screen menu navigation.

// ── Locations ─────────────────────────────────────────────────

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
    pub connections: &'static [LocationId],
    pub has_encounters: bool,
    pub has_shop: bool,
    pub has_npc: bool,
}

pub fn location_info(id: LocationId) -> LocationInfo {
    match id {
        LocationId::StartVillage => LocationInfo {
            name: "始まりの村",
            description: "穏やかな風が吹く小さな村。",
            connections: &[LocationId::Forest],
            has_encounters: false,
            has_shop: true,
            has_npc: true,
        },
        LocationId::Forest => LocationInfo {
            name: "迷いの森",
            description: "木々が鬱蒼と茂る森。獣の気配がする。",
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
            description: "魔王城へ続く険しい山道。強者の気配が漂う。",
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

// ── Enemies ───────────────────────────────────────────────────

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
    pub drop: Option<(ItemKind, f64)>,
}

pub fn enemy_info(kind: EnemyKind) -> EnemyInfo {
    match kind {
        EnemyKind::Slime => EnemyInfo {
            name: "スライム", max_hp: 15, atk: 4, def: 1, exp: 5, gold: 8,
            drop: Some((ItemKind::Herb, 0.4)),
        },
        EnemyKind::Wolf => EnemyInfo {
            name: "オオカミ", max_hp: 25, atk: 8, def: 3, exp: 12, gold: 15,
            drop: Some((ItemKind::Herb, 0.3)),
        },
        EnemyKind::Goblin => EnemyInfo {
            name: "ゴブリン", max_hp: 35, atk: 12, def: 5, exp: 20, gold: 25,
            drop: Some((ItemKind::MagicWater, 0.3)),
        },
        EnemyKind::Golem => EnemyInfo {
            name: "ゴーレム", max_hp: 60, atk: 15, def: 12, exp: 40, gold: 50,
            drop: Some((ItemKind::StrengthPotion, 0.2)),
        },
        EnemyKind::DarkKnight => EnemyInfo {
            name: "暗黒騎士", max_hp: 80, atk: 20, def: 15, exp: 60, gold: 80,
            drop: Some((ItemKind::MagicWater, 0.4)),
        },
        EnemyKind::DemonLord => EnemyInfo {
            name: "魔王", max_hp: 150, atk: 28, def: 18, exp: 200, gold: 500,
            drop: None,
        },
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Variants reserved for future content expansion
pub enum ItemKind {
    Herb, MagicWater, StrengthPotion,
    WoodenSword, IronSword, SteelSword, HolySword,
    TravelClothes, LeatherArmor, ChainMail, KnightArmor,
    AncientKey, LakeTreasure,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemCategory { Consumable, Weapon, Armor, KeyItem }

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
            name: "薬草", description: "HPを30回復", category: ItemCategory::Consumable,
            buy_price: 20, value: 30,
        },
        ItemKind::MagicWater => ItemInfo {
            name: "魔法の水", description: "MPを20回復", category: ItemCategory::Consumable,
            buy_price: 50, value: 20,
        },
        ItemKind::StrengthPotion => ItemInfo {
            name: "力の薬", description: "戦闘中ATK+5", category: ItemCategory::Consumable,
            buy_price: 80, value: 5,
        },
        ItemKind::WoodenSword => ItemInfo {
            name: "木の剣", description: "ATK+3", category: ItemCategory::Weapon,
            buy_price: 30, value: 3,
        },
        ItemKind::IronSword => ItemInfo {
            name: "鉄の剣", description: "ATK+8", category: ItemCategory::Weapon,
            buy_price: 120, value: 8,
        },
        ItemKind::SteelSword => ItemInfo {
            name: "鋼の剣", description: "ATK+15", category: ItemCategory::Weapon,
            buy_price: 350, value: 15,
        },
        ItemKind::HolySword => ItemInfo {
            name: "聖剣", description: "ATK+25", category: ItemCategory::Weapon,
            buy_price: 1000, value: 25,
        },
        ItemKind::TravelClothes => ItemInfo {
            name: "旅人の服", description: "DEF+2", category: ItemCategory::Armor,
            buy_price: 20, value: 2,
        },
        ItemKind::LeatherArmor => ItemInfo {
            name: "革の鎧", description: "DEF+5", category: ItemCategory::Armor,
            buy_price: 100, value: 5,
        },
        ItemKind::ChainMail => ItemInfo {
            name: "鎖帷子", description: "DEF+12", category: ItemCategory::Armor,
            buy_price: 300, value: 12,
        },
        ItemKind::KnightArmor => ItemInfo {
            name: "騎士の鎧", description: "DEF+20", category: ItemCategory::Armor,
            buy_price: 800, value: 20,
        },
        ItemKind::AncientKey => ItemInfo {
            name: "古代の鍵", description: "山道への扉を開ける", category: ItemCategory::KeyItem,
            buy_price: 0, value: 0,
        },
        ItemKind::LakeTreasure => ItemInfo {
            name: "湖の秘宝", description: "不思議な宝石", category: ItemCategory::KeyItem,
            buy_price: 0, value: 0,
        },
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillKind { Fire, Heal, Shield }

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
        SkillKind::Shield => SkillInfo {
            name: "シールド", description: "戦闘中DEF上昇",
            mp_cost: 5, value: 8, learn_level: 4,
        },
    }
}

pub const ALL_SKILLS: &[SkillKind] = &[SkillKind::Fire, SkillKind::Heal, SkillKind::Shield];

// ── Quests ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuestId {
    MainPrepare, MainForest, MainCave, MainMountain, MainFinal,
    SideHerbCollect, SideLakeTreasure, SideWolfHunt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestKind { Main, Side }

#[derive(Clone, Debug, PartialEq)]
pub enum QuestGoal {
    TalkNpc(LocationId),
    DefeatEnemies(EnemyKind, u32),
    FindItem(ItemKind),
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
    pub prerequisite: Option<QuestId>,
    pub accept_location: LocationId,
}

pub fn quest_info(id: QuestId) -> QuestInfo {
    match id {
        QuestId::MainPrepare => QuestInfo {
            name: "旅立ちの準備", description: "村の長老に話しかけて装備を受け取ろう",
            kind: QuestKind::Main, goal: QuestGoal::TalkNpc(LocationId::StartVillage),
            reward_gold: 50, reward_exp: 10, reward_item: Some(ItemKind::WoodenSword),
            prerequisite: None, accept_location: LocationId::StartVillage,
        },
        QuestId::MainForest => QuestInfo {
            name: "森の脅威", description: "森でスライムを3体倒して安全を確保せよ",
            kind: QuestKind::Main, goal: QuestGoal::DefeatEnemies(EnemyKind::Slime, 3),
            reward_gold: 80, reward_exp: 30, reward_item: None,
            prerequisite: Some(QuestId::MainPrepare), accept_location: LocationId::StartVillage,
        },
        QuestId::MainCave => QuestInfo {
            name: "洞窟の秘密", description: "古代の洞窟で「古代の鍵」を見つけ出せ",
            kind: QuestKind::Main, goal: QuestGoal::FindItem(ItemKind::AncientKey),
            reward_gold: 120, reward_exp: 50, reward_item: None,
            prerequisite: Some(QuestId::MainForest), accept_location: LocationId::StartVillage,
        },
        QuestId::MainMountain => QuestInfo {
            name: "山道の試練", description: "山道を守る暗黒騎士を倒せ",
            kind: QuestKind::Main, goal: QuestGoal::DefeatBoss(EnemyKind::DarkKnight),
            reward_gold: 200, reward_exp: 80, reward_item: Some(ItemKind::SteelSword),
            prerequisite: Some(QuestId::MainCave), accept_location: LocationId::Cave,
        },
        QuestId::MainFinal => QuestInfo {
            name: "最終決戦", description: "魔王を倒して世界に平和を取り戻せ！",
            kind: QuestKind::Main, goal: QuestGoal::DefeatBoss(EnemyKind::DemonLord),
            reward_gold: 1000, reward_exp: 500, reward_item: None,
            prerequisite: Some(QuestId::MainMountain), accept_location: LocationId::MountainPath,
        },
        QuestId::SideHerbCollect => QuestInfo {
            name: "薬草集め", description: "薬師のために薬草を3つ集めてこい",
            kind: QuestKind::Side, goal: QuestGoal::FindItem(ItemKind::Herb),
            reward_gold: 60, reward_exp: 20, reward_item: Some(ItemKind::MagicWater),
            prerequisite: Some(QuestId::MainPrepare), accept_location: LocationId::StartVillage,
        },
        QuestId::SideLakeTreasure => QuestInfo {
            name: "湖の宝物", description: "隠された湖で伝説の秘宝を探せ",
            kind: QuestKind::Side, goal: QuestGoal::FindItem(ItemKind::LakeTreasure),
            reward_gold: 150, reward_exp: 40, reward_item: Some(ItemKind::ChainMail),
            prerequisite: Some(QuestId::MainForest), accept_location: LocationId::HiddenLake,
        },
        QuestId::SideWolfHunt => QuestInfo {
            name: "狼退治", description: "森のオオカミを5体退治してくれ",
            kind: QuestKind::Side, goal: QuestGoal::DefeatEnemies(EnemyKind::Wolf, 5),
            reward_gold: 100, reward_exp: 35, reward_item: Some(ItemKind::IronSword),
            prerequisite: Some(QuestId::MainPrepare), accept_location: LocationId::StartVillage,
        },
    }
}

pub const ALL_QUESTS: &[QuestId] = &[
    QuestId::MainPrepare, QuestId::MainForest, QuestId::MainCave,
    QuestId::MainMountain, QuestId::MainFinal,
    QuestId::SideHerbCollect, QuestId::SideLakeTreasure, QuestId::SideWolfHunt,
];

// ── Quest Progress ────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestStatus { Available, Active, ReadyToComplete, Completed }

#[derive(Clone, Debug)]
pub struct QuestProgress {
    pub quest_id: QuestId,
    pub status: QuestStatus,
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
}

// ── Scene System ─────────────────────────────────────────────
//
// The scene system replaces the old 8-screen model.
// One main screen renders differently based on the current scene.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scene {
    /// Opening prologue (step 0, 1, 2, ...).
    Prologue(u8),
    /// In the world: exploring, talking, traveling.
    World,
    /// In combat.
    Battle,
    /// Game over — demon lord defeated.
    GameClear,
}

/// Overlay screens that draw on top of the main scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    Inventory,
    QuestLog,
    Status,
    Shop,
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

// ── Feature Unlocks (Progressive Disclosure) ──────────────────

#[derive(Clone, Debug)]
pub struct Unlocks {
    /// Status bar (HP/MP/gold) shown after prologue.
    pub status_bar: bool,
    /// Quest objective shown after first quest.
    pub quest_objective: bool,
    /// [I] inventory shortcut shown after first item.
    pub inventory_shortcut: bool,
    /// [S] status shortcut shown after first battle.
    pub status_shortcut: bool,
    /// [Q] quest log shortcut shown after 2nd quest.
    pub quest_log_shortcut: bool,
}

impl Unlocks {
    pub fn new() -> Self {
        Self {
            status_bar: false,
            quest_objective: false,
            inventory_shortcut: false,
            status_shortcut: false,
            quest_log_shortcut: false,
        }
    }
}

// ── Scene Text ───────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct SceneText {
    /// Narrative text lines displayed in the main area.
    pub lines: Vec<String>,
}

impl SceneText {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }
    pub fn set(&mut self, lines: Vec<String>) {
        self.lines = lines;
    }
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

    // Scene system
    pub scene: Scene,
    pub overlay: Option<Overlay>,
    pub scene_text: SceneText,

    // Log (shown at bottom, 1-2 lines)
    pub log: Vec<String>,

    // Feature unlocks
    pub unlocks: Unlocks,

    // Game state
    pub game_cleared: bool,
    pub rng_seed: u64,

    // Kill counters for quest tracking
    pub kill_counts: Vec<(EnemyKind, u32)>,
}

impl RpgState {
    pub fn new() -> Self {
        let quests = ALL_QUESTS.iter().map(|&id| QuestProgress {
            quest_id: id,
            status: QuestStatus::Available,
        }).collect();

        Self {
            level: 1, exp: 0,
            hp: 50, max_hp: 50,
            mp: 15, max_mp: 15,
            base_atk: 5, base_def: 3, mag: 4,
            gold: 0,
            weapon: None, armor: None,
            location: LocationId::StartVillage,
            inventory: Vec::new(),
            quests,
            battle: None,
            scene: Scene::Prologue(0),
            overlay: None,
            scene_text: SceneText::new(),
            log: Vec::new(),
            unlocks: Unlocks::new(),
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

    pub fn total_atk(&self) -> u32 {
        let weapon_bonus = self.weapon.map(|w| item_info(w).value).unwrap_or(0);
        self.base_atk + weapon_bonus
    }

    pub fn total_def(&self) -> u32 {
        let armor_bonus = self.armor.map(|a| item_info(a).value).unwrap_or(0);
        self.base_def + armor_bonus
    }

    pub fn item_count(&self, kind: ItemKind) -> u32 {
        self.inventory.iter().find(|i| i.kind == kind).map(|i| i.count).unwrap_or(0)
    }

    pub fn kill_count(&self, kind: EnemyKind) -> u32 {
        self.kill_counts.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or(0)
    }

    /// Get the current active main quest objective text (for status bar).
    pub fn current_objective(&self) -> Option<String> {
        // Find the first active main quest
        for q in &self.quests {
            let info = quest_info(q.quest_id);
            if info.kind == QuestKind::Main && q.status == QuestStatus::Active {
                return Some(self.format_objective(q.quest_id, &info));
            }
            if info.kind == QuestKind::Main && q.status == QuestStatus::ReadyToComplete {
                return Some(format!("{} — 報告可！", info.name));
            }
        }
        // Check for available main quests
        for q in &self.quests {
            let info = quest_info(q.quest_id);
            if info.kind != QuestKind::Main { continue; }
            if q.status != QuestStatus::Available { continue; }
            if let Some(prereq) = info.prerequisite {
                if self.quest_progress(prereq).map(|p| p.status) != Some(QuestStatus::Completed) {
                    continue;
                }
            }
            return Some(format!("{} — 話しかけて受注", info.name));
        }
        None
    }

    fn format_objective(&self, quest_id: QuestId, info: &QuestInfo) -> String {
        match &info.goal {
            QuestGoal::DefeatEnemies(kind, required) => {
                let count = self.kill_count(*kind);
                format!("{} ({}/{})", info.name, count.min(*required), required)
            }
            QuestGoal::DefeatBoss(_) => info.name.to_string(),
            QuestGoal::FindItem(_) => info.name.to_string(),
            QuestGoal::TalkNpc(_) => {
                let _ = quest_id;
                info.name.to_string()
            }
        }
    }

    fn quest_progress(&self, quest_id: QuestId) -> Option<&QuestProgress> {
        self.quests.iter().find(|q| q.quest_id == quest_id)
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
        assert_eq!(s.gold, 0);
        assert_eq!(s.location, LocationId::StartVillage);
        assert_eq!(s.scene, Scene::Prologue(0));
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
        s.inventory.push(InventoryItem { kind: ItemKind::Herb, count: 5 });
        assert_eq!(s.item_count(ItemKind::Herb), 5);
    }

    #[test]
    fn log_truncation() {
        let mut s = RpgState::new();
        for i in 0..40 { s.add_log(&format!("msg {}", i)); }
        assert!(s.log.len() <= 30);
    }

    #[test]
    fn current_objective_none_at_start() {
        let s = RpgState::new();
        // MainPrepare has no prereq and is Available
        let obj = s.current_objective();
        assert!(obj.is_some());
        assert!(obj.unwrap().contains("旅立ちの準備"));
    }

    #[test]
    fn main_quest_chain_has_prerequisites() {
        assert!(quest_info(QuestId::MainPrepare).prerequisite.is_none());
        assert_eq!(quest_info(QuestId::MainForest).prerequisite, Some(QuestId::MainPrepare));
        assert_eq!(quest_info(QuestId::MainFinal).prerequisite, Some(QuestId::MainMountain));
    }

    #[test]
    fn all_quests_have_info() {
        for &id in ALL_QUESTS {
            let info = quest_info(id);
            assert!(!info.name.is_empty());
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
    fn shop_inventory_valid() {
        let items = shop_inventory(LocationId::StartVillage);
        assert!(!items.is_empty());
        for &(kind, count) in items {
            assert!(item_info(kind).buy_price > 0);
            assert!(count > 0);
        }
    }
}
