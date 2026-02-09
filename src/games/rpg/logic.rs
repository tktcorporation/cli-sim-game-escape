//! RPG Quest — pure game logic (no rendering / IO).
//!
//! Scene-based system: each action transitions between scenes
//! and updates the narrative scene_text.

use super::state::{
    enemy_info, encounter_table, item_info, level_stats, location_info, quest_info, skill_info,
    shop_inventory, BattleEnemy, BattlePhase, BattleState, EnemyKind,
    InventoryItem, ItemCategory, ItemKind, LocationId, QuestGoal, QuestId, QuestKind,
    QuestStatus, RpgState, Scene, SkillKind, ALL_QUESTS, ALL_SKILLS, MAX_LEVEL,
};

// ── Tick (no-op: command-based game) ─────────────────────────

pub fn tick(_state: &mut RpgState, _delta_ticks: u32) {}

// ── RNG ──────────────────────────────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn rng_range(state: &mut RpgState, max: u32) -> u32 {
    state.rng_seed = next_rng(state.rng_seed);
    ((state.rng_seed >> 33) % max as u64) as u32
}

fn rng_chance(state: &mut RpgState, chance: f64) -> bool {
    let roll = rng_range(state, 1000);
    (roll as f64) < (chance * 1000.0)
}

// ── Prologue ────────────────────────────────────────────────

pub fn advance_prologue(state: &mut RpgState) {
    let step = match state.scene {
        Scene::Prologue(s) => s,
        _ => return,
    };

    match step {
        0 => {
            // Player pressed [1] on "辺りを見回す"
            state.scene_text.set(vec![
                "穏やかな風が吹く小さな村。".into(),
                "広場の中央に、白髪の長老が立っている。".into(),
                "あなたに気づくと、ゆっくりと歩み寄ってきた。".into(),
                "".into(),
                "「おお…ようやく目を覚ましたか。".into(),
                "  この世界は今、魔王の脅威に晒されておる。".into(),
                "  おぬしに頼みたいことがあるのだ」".into(),
            ]);
            state.scene = Scene::Prologue(1);
        }
        1 => {
            // Player chose to listen or ask
            state.scene_text.set(vec![
                "「魔王は西の城から闇を広げておる。".into(),
                "  まずは森を越え、洞窟を抜け、".into(),
                "  山道の先にある魔王城を目指すのじゃ」".into(),
                "".into(),
                "「これを持っていけ。".into(),
                "  古い剣と旅人の服じゃが、無いよりはマシじゃろう」".into(),
            ]);
            // Give starting equipment
            add_item(state, ItemKind::WoodenSword, 1);
            add_item(state, ItemKind::TravelClothes, 1);
            // Auto-equip
            state.weapon = Some(ItemKind::WoodenSword);
            state.armor = Some(ItemKind::TravelClothes);
            // Remove from inventory (since equipped)
            state.inventory.retain(|i| i.kind != ItemKind::WoodenSword && i.kind != ItemKind::TravelClothes);
            // Complete MainPrepare quest
            set_quest_status(state, QuestId::MainPrepare, QuestStatus::Active);
            set_quest_status(state, QuestId::MainPrepare, QuestStatus::Completed);
            state.gold += 50;
            state.exp += 10;
            check_level_up(state);
            // Accept MainForest
            set_quest_status(state, QuestId::MainForest, QuestStatus::Active);
            state.add_log("木の剣と旅人の服を受け取った！");
            state.add_log("50Gを受け取った！");
            state.scene = Scene::Prologue(2);
        }
        2 => {
            // Transition to World scene
            state.scene_text.set(vec![]);
            state.unlocks.status_bar = true;
            state.unlocks.quest_objective = true;
            state.unlocks.inventory_shortcut = true;
            state.scene = Scene::World;
            update_world_text(state);
            state.add_log("冒険が始まった！");
        }
        _ => {
            state.scene = Scene::World;
            update_world_text(state);
        }
    }
}

// ── World Scene Text ────────────────────────────────────────

pub fn update_world_text(state: &mut RpgState) {
    let loc = location_info(state.location);
    let mut lines = vec![
        format!("＜{}＞", loc.name),
        "".into(),
        loc.description.to_string(),
    ];

    // Add contextual description based on location
    match state.location {
        LocationId::StartVillage => {
            lines.push("".into());
            // Check for quest-related NPC text
            let has_ready = ALL_QUESTS.iter().any(|&qid| {
                let info = quest_info(qid);
                info.accept_location == LocationId::StartVillage
                    && quest_status(state, qid) == QuestStatus::ReadyToComplete
            });
            let has_available = ALL_QUESTS.iter().any(|&qid| {
                let info = quest_info(qid);
                if info.accept_location != LocationId::StartVillage { return false; }
                if quest_status(state, qid) != QuestStatus::Available { return false; }
                match info.prerequisite {
                    None => true,
                    Some(pre) => quest_status(state, pre) == QuestStatus::Completed,
                }
            });
            if has_ready {
                lines.push("長老が嬉しそうにこちらを見ている。".into());
            } else if has_available {
                lines.push("長老が手招きしている。新しい依頼がありそうだ。".into());
            } else {
                lines.push("村は穏やかだ。".into());
            }
        }
        LocationId::Forest => {
            lines.push("".into());
            lines.push("木々の間から獣の唸り声が聞こえる…".into());
        }
        LocationId::Cave => {
            lines.push("".into());
            lines.push("洞窟の奥から不気味な音が響いている。".into());
        }
        LocationId::HiddenLake => {
            lines.push("".into());
            let has_npc_quest = ALL_QUESTS.iter().any(|&qid| {
                let info = quest_info(qid);
                info.accept_location == LocationId::HiddenLake
                    && (quest_status(state, qid) == QuestStatus::Available
                        || quest_status(state, qid) == QuestStatus::ReadyToComplete)
                    && match info.prerequisite {
                        None => true,
                        Some(pre) => quest_status(state, pre) == QuestStatus::Completed,
                    }
            });
            if has_npc_quest {
                lines.push("湖のほとりに精霊の姿が見える。".into());
            } else {
                lines.push("静寂に包まれた美しい湖。".into());
            }
        }
        LocationId::MountainPath => {
            lines.push("".into());
            lines.push("吹き荒ぶ風の中、強敵の気配がする。".into());
        }
        LocationId::DemonCastle => {
            lines.push("".into());
            lines.push("禍々しいオーラが全身を包む。覚悟を決めろ。".into());
        }
    }

    state.scene_text.set(lines);
}

// ── World Choices ────────────────────────────────────────────
//
// Returns a list of (label, is_quest_related) for the current situation.
// The render layer will display these as [1], [2], [3], etc.

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
    pub quest_related: bool,
}

pub fn world_choices(state: &RpgState) -> Vec<Choice> {
    let loc = location_info(state.location);
    let mut choices = Vec::new();

    // 1. NPC interaction (if available and quest-relevant)
    if loc.has_npc {
        let has_ready = ALL_QUESTS.iter().any(|&qid| {
            let info = quest_info(qid);
            info.accept_location == state.location
                && quest_status(state, qid) == QuestStatus::ReadyToComplete
        });
        let has_new = ALL_QUESTS.iter().any(|&qid| {
            let info = quest_info(qid);
            if info.accept_location != state.location { return false; }
            if quest_status(state, qid) != QuestStatus::Available { return false; }
            match info.prerequisite {
                None => true,
                Some(pre) => quest_status(state, pre) == QuestStatus::Completed,
            }
        });
        if has_ready {
            choices.push(Choice { label: "報告する".into(), quest_related: true });
        } else if has_new {
            choices.push(Choice { label: "話しかける".into(), quest_related: true });
        } else {
            choices.push(Choice { label: "話しかける".into(), quest_related: false });
        }
    }

    // 2. Explore (if encounters or quest items possible)
    if loc.has_encounters || state.location == LocationId::HiddenLake {
        choices.push(Choice { label: "探索する".into(), quest_related: false });
    }

    // 3. Shop
    if loc.has_shop {
        choices.push(Choice { label: "店に寄る".into(), quest_related: false });
    }

    // 4. Rest (only if HP or MP not full)
    if state.hp < state.max_hp || state.mp < state.max_mp {
        let cost_str = if loc.has_shop { " (10G)" } else { "" };
        choices.push(Choice { label: format!("休む{}", cost_str), quest_related: false });
    }

    // 5. Travel destinations
    for &dest in loc.connections {
        let dest_info = location_info(dest);
        choices.push(Choice {
            label: format!("{}へ向かう", dest_info.name),
            quest_related: false,
        });
    }

    choices
}

/// Execute a choice by index (0-based). Returns true if something happened.
pub fn execute_world_choice(state: &mut RpgState, index: usize) -> bool {
    let choices = world_choices(state);
    if index >= choices.len() {
        return false;
    }
    let label = &choices[index].label;

    // Determine what action this choice maps to
    let loc = location_info(state.location);

    // Track position in choice list to map back
    let mut pos = 0;

    // NPC
    if loc.has_npc {
        if index == pos {
            return talk_npc(state);
        }
        pos += 1;
    }

    // Explore
    if loc.has_encounters || state.location == LocationId::HiddenLake {
        if index == pos {
            return explore(state);
        }
        pos += 1;
    }

    // Shop
    if loc.has_shop {
        if index == pos {
            state.overlay = Some(super::state::Overlay::Shop);
            return true;
        }
        pos += 1;
    }

    // Rest
    if state.hp < state.max_hp || state.mp < state.max_mp {
        if index == pos {
            return rest(state);
        }
        pos += 1;
    }

    // Travel destinations
    for (i, &dest) in loc.connections.iter().enumerate() {
        if index == pos + i {
            return travel(state, dest);
        }
    }

    let _ = (label, pos);
    false
}

// ── Travel ───────────────────────────────────────────────────

pub fn travel(state: &mut RpgState, dest: LocationId) -> bool {
    // Gate checks
    if dest == LocationId::MountainPath {
        let cave_done = quest_status(state, QuestId::MainCave) == QuestStatus::Completed;
        if !cave_done {
            state.add_log("古代の鍵が必要だ…");
            return false;
        }
    }
    if dest == LocationId::DemonCastle {
        let mountain_done = quest_status(state, QuestId::MainMountain) == QuestStatus::Completed;
        if !mountain_done {
            state.add_log("山道の試練を突破する必要がある…");
            return false;
        }
    }

    state.location = dest;
    let dest_info = location_info(dest);
    state.add_log(&format!("{}に到着した", dest_info.name));

    // Random encounter on arrival
    if dest_info.has_encounters {
        let chance = match dest {
            LocationId::DemonCastle => 80,
            LocationId::MountainPath => 70,
            _ => 50,
        };
        if rng_range(state, 100) < chance {
            start_random_encounter(state);
            return true;
        }
    }

    update_world_text(state);
    true
}

// ── Explore ──────────────────────────────────────────────────

pub fn explore(state: &mut RpgState) -> bool {
    let loc = state.location;

    // Check for quest items
    if check_explore_find(state, loc) {
        update_world_text(state);
        return true;
    }

    let loc_info = location_info(loc);
    if loc_info.has_encounters {
        state.add_log("周囲を探索した…");
        if rng_range(state, 100) < 60 {
            start_random_encounter(state);
        } else {
            let gold = rng_range(state, 15) + 5;
            state.gold += gold;
            state.add_log(&format!("探索中に{}Gを見つけた！", gold));
            update_world_text(state);
        }
    } else {
        state.add_log("周囲を探索したが、何も見つからなかった");
    }
    true
}

fn check_explore_find(state: &mut RpgState, loc: LocationId) -> bool {
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        if quest_status(state, quest_id) != QuestStatus::Active {
            continue;
        }

        if let QuestGoal::FindItem(item_kind) = &info.goal {
            let valid_loc = match *item_kind {
                ItemKind::AncientKey => loc == LocationId::Cave,
                ItemKind::LakeTreasure => loc == LocationId::HiddenLake,
                ItemKind::Herb => loc == LocationId::Forest,
                _ => false,
            };

            if valid_loc && rng_chance(state, 0.5) {
                add_item(state, *item_kind, 1);
                let iinfo = item_info(*item_kind);
                state.add_log(&format!("{}を見つけた！", iinfo.name));

                if quest_id == QuestId::SideHerbCollect {
                    if state.item_count(ItemKind::Herb) >= 3 {
                        set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                        state.add_log("薬草を3つ集めた！村に報告しよう");
                    }
                } else {
                    set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                    state.add_log(&format!("クエスト「{}」達成！", info.name));
                }
                return true;
            }
        }
    }
    false
}

// ── Battle ───────────────────────────────────────────────────

fn start_random_encounter(state: &mut RpgState) {
    let table = encounter_table(state.location);
    if table.is_empty() { return; }
    let idx = rng_range(state, table.len() as u32) as usize;
    start_battle(state, table[idx], false);
}

pub fn start_battle(state: &mut RpgState, enemy_kind: EnemyKind, is_boss: bool) {
    let info = enemy_info(enemy_kind);
    state.battle = Some(BattleState {
        enemy: BattleEnemy { kind: enemy_kind, hp: info.max_hp, max_hp: info.max_hp },
        phase: BattlePhase::SelectAction,
        player_def_boost: 0, player_atk_boost: 0,
        log: vec![format!("{}が現れた！", info.name)],
        is_boss,
    });
    state.scene = Scene::Battle;
    // Unlock status shortcut after first battle
    state.unlocks.status_shortcut = true;
}

pub fn battle_attack(state: &mut RpgState) -> bool {
    let player_atk = state.total_atk();
    let battle = match &mut state.battle { Some(b) => b, None => return false };
    if battle.phase != BattlePhase::SelectAction { return false; }

    let total_atk = player_atk + battle.player_atk_boost;
    let einfo = enemy_info(battle.enemy.kind);
    let damage = total_atk.saturating_sub(einfo.def / 2).max(1);
    battle.log.push(format!("攻撃！ {}に{}ダメージ！", einfo.name, damage));

    let battle = state.battle.as_mut().unwrap();
    battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
    if battle.enemy.hp == 0 {
        battle.phase = BattlePhase::Victory;
        let name = enemy_info(battle.enemy.kind).name;
        battle.log.push(format!("{}を倒した！", name));
    } else {
        process_enemy_turn(state);
    }
    true
}

pub fn battle_use_skill(state: &mut RpgState, skill_index: usize) -> bool {
    let available = available_skills(state.level);
    if skill_index >= available.len() { return false; }
    let skill_kind = available[skill_index];
    let sinfo = skill_info(skill_kind);

    if state.mp < sinfo.mp_cost {
        if let Some(b) = &mut state.battle { b.log.push("MPが足りない！".into()); }
        return false;
    }
    state.mp -= sinfo.mp_cost;

    let battle = match &mut state.battle { Some(b) => b, None => return false };
    match skill_kind {
        SkillKind::Fire => {
            let damage = (state.mag * sinfo.value).saturating_sub(enemy_info(battle.enemy.kind).def / 3).max(1);
            battle.log.push(format!("ファイア！ {}ダメージ！", damage));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Heal => {
            let heal = state.mag * sinfo.value;
            state.hp = (state.hp + heal).min(state.max_hp);
            battle.log.push(format!("ヒール！ HP{}回復！", heal));
        }
        SkillKind::Shield => {
            battle.player_def_boost += sinfo.value;
            battle.log.push(format!("シールド！ DEF+{}！", sinfo.value));
        }
    }

    let battle = state.battle.as_mut().unwrap();
    if battle.enemy.hp == 0 {
        battle.phase = BattlePhase::Victory;
        let name = enemy_info(battle.enemy.kind).name;
        battle.log.push(format!("{}を倒した！", name));
    } else {
        battle.phase = BattlePhase::SelectAction;
        process_enemy_turn(state);
    }
    true
}

pub fn battle_use_item(state: &mut RpgState, inv_index: usize) -> bool {
    let consumables: Vec<usize> = state.inventory.iter().enumerate()
        .filter(|(_, i)| item_info(i.kind).category == ItemCategory::Consumable && i.count > 0)
        .map(|(idx, _)| idx)
        .collect();
    if inv_index >= consumables.len() { return false; }

    let actual_idx = consumables[inv_index];
    let item_kind = state.inventory[actual_idx].kind;
    let iinfo = item_info(item_kind);
    state.inventory[actual_idx].count -= 1;
    if state.inventory[actual_idx].count == 0 { state.inventory.remove(actual_idx); }

    let battle = match &mut state.battle { Some(b) => b, None => return false };
    match item_kind {
        ItemKind::Herb => {
            state.hp = (state.hp + iinfo.value).min(state.max_hp);
            battle.log.push(format!("薬草を使った！ HP{}回復！", iinfo.value));
        }
        ItemKind::MagicWater => {
            state.mp = (state.mp + iinfo.value).min(state.max_mp);
            battle.log.push(format!("魔法の水を使った！ MP{}回復！", iinfo.value));
        }
        ItemKind::StrengthPotion => {
            battle.player_atk_boost += iinfo.value;
            battle.log.push(format!("力の薬を使った！ ATK+{}！", iinfo.value));
        }
        _ => {
            battle.log.push("そのアイテムは使えない".into());
            return false;
        }
    }

    let battle = state.battle.as_mut().unwrap();
    battle.phase = BattlePhase::SelectAction;
    process_enemy_turn(state);
    true
}

pub fn battle_flee(state: &mut RpgState) -> bool {
    let battle = match &mut state.battle { Some(b) => b, None => return false };
    if battle.is_boss {
        battle.log.push("ボスからは逃げられない！".into());
        return false;
    }
    if rng_chance(state, 0.6) {
        let battle = state.battle.as_mut().unwrap();
        battle.log.push("うまく逃げ切った！".into());
        battle.phase = BattlePhase::Fled;
    } else {
        let battle = state.battle.as_mut().unwrap();
        battle.log.push("逃げられなかった！".into());
        process_enemy_turn(state);
    }
    true
}

fn process_enemy_turn(state: &mut RpgState) {
    let player_def = state.total_def();
    let battle = match &mut state.battle { Some(b) => b, None => return };
    if battle.enemy.hp == 0 { return; }

    let einfo = enemy_info(battle.enemy.kind);
    let total_def = player_def + battle.player_def_boost;
    let damage = einfo.atk.saturating_sub(total_def / 2).max(1);
    battle.log.push(format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));

    state.hp = state.hp.saturating_sub(damage);
    if state.hp == 0 {
        let battle = state.battle.as_mut().unwrap();
        battle.phase = BattlePhase::Defeat;
        battle.log.push("力尽きた...".into());
    }
}

pub fn process_victory(state: &mut RpgState) {
    let battle = match &state.battle { Some(b) => b, None => return };
    let enemy_kind = battle.enemy.kind;
    let einfo = enemy_info(enemy_kind);

    state.exp += einfo.exp;
    state.gold += einfo.gold;
    state.add_log(&format!("{}を倒した！ EXP+{} {}G", einfo.name, einfo.exp, einfo.gold));

    if let Some(kc) = state.kill_counts.iter_mut().find(|k| k.0 == enemy_kind) {
        kc.1 += 1;
    } else {
        state.kill_counts.push((enemy_kind, 1));
    }

    if let Some((drop_item, drop_chance)) = einfo.drop {
        if rng_chance(state, drop_chance) {
            add_item(state, drop_item, 1);
            state.add_log(&format!("{}をドロップ！", item_info(drop_item).name));
        }
    }

    check_level_up(state);
    update_quest_kills(state, enemy_kind);
    state.battle = None;

    // Check game clear
    if enemy_kind == EnemyKind::DemonLord {
        set_quest_status(state, QuestId::MainFinal, QuestStatus::Completed);
        state.game_cleared = true;
        state.scene = Scene::GameClear;
    } else {
        state.scene = Scene::World;
        update_world_text(state);
    }

    // Unlock quest log after second quest-related kill
    if state.kill_counts.iter().map(|k| k.1).sum::<u32>() >= 2 {
        state.unlocks.quest_log_shortcut = true;
    }
}

pub fn process_defeat(state: &mut RpgState) {
    state.hp = state.max_hp / 2;
    state.mp = state.max_mp / 2;
    state.gold /= 2;
    state.location = LocationId::StartVillage;
    state.battle = None;
    state.scene = Scene::World;
    state.add_log("村で目を覚ました… 所持金の半分を失った");
    update_world_text(state);
}

pub fn process_fled(state: &mut RpgState) {
    state.battle = None;
    state.scene = Scene::World;
    update_world_text(state);
}

// ── Level Up ─────────────────────────────────────────────────

fn check_level_up(state: &mut RpgState) {
    while state.level < MAX_LEVEL {
        let stats = level_stats(state.level);
        if state.exp >= stats.exp_to_next {
            state.exp -= stats.exp_to_next;
            state.level += 1;
            let new_stats = level_stats(state.level);
            state.max_hp = new_stats.max_hp;
            state.max_mp = new_stats.max_mp;
            state.base_atk = new_stats.atk;
            state.base_def = new_stats.def;
            state.mag = new_stats.mag;
            state.hp = state.max_hp;
            state.mp = state.max_mp;
            state.add_log(&format!("レベルアップ！ Lv.{}", state.level));
            for &skill in ALL_SKILLS {
                let sinfo = skill_info(skill);
                if sinfo.learn_level == state.level {
                    state.add_log(&format!("スキル「{}」を習得！", sinfo.name));
                }
            }
        } else {
            break;
        }
    }
}

// ── Inventory ────────────────────────────────────────────────

pub fn add_item(state: &mut RpgState, kind: ItemKind, count: u32) {
    if let Some(entry) = state.inventory.iter_mut().find(|i| i.kind == kind) {
        entry.count += count;
    } else {
        state.inventory.push(InventoryItem { kind, count });
    }
    state.unlocks.inventory_shortcut = true;
}

pub fn use_item(state: &mut RpgState, inv_index: usize) -> bool {
    if inv_index >= state.inventory.len() { return false; }
    let kind = state.inventory[inv_index].kind;
    let iinfo = item_info(kind);

    match iinfo.category {
        ItemCategory::Consumable => {
            match kind {
                ItemKind::Herb => {
                    if state.hp >= state.max_hp { state.add_log("HPは満タン"); return false; }
                    state.hp = (state.hp + iinfo.value).min(state.max_hp);
                    state.add_log(&format!("薬草を使った！ HP{}回復", iinfo.value));
                }
                ItemKind::MagicWater => {
                    if state.mp >= state.max_mp { state.add_log("MPは満タン"); return false; }
                    state.mp = (state.mp + iinfo.value).min(state.max_mp);
                    state.add_log(&format!("魔法の水を使った！ MP{}回復", iinfo.value));
                }
                ItemKind::StrengthPotion => { state.add_log("戦闘中にしか使えない"); return false; }
                _ => { state.add_log("使えないアイテム"); return false; }
            }
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 { state.inventory.remove(inv_index); }
            true
        }
        ItemCategory::Weapon => {
            let old = state.weapon;
            state.weapon = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 { state.inventory.remove(inv_index); }
            if let Some(old_kind) = old { add_item(state, old_kind, 1); }
            state.add_log(&format!("{}を装備した", iinfo.name));
            true
        }
        ItemCategory::Armor => {
            let old = state.armor;
            state.armor = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 { state.inventory.remove(inv_index); }
            if let Some(old_kind) = old { add_item(state, old_kind, 1); }
            state.add_log(&format!("{}を装備した", iinfo.name));
            true
        }
        ItemCategory::KeyItem => { state.add_log("キーアイテムは使えない"); false }
    }
}

// ── Shop ─────────────────────────────────────────────────────

pub fn buy_item(state: &mut RpgState, shop_index: usize) -> bool {
    let shop = shop_inventory(state.location);
    if shop_index >= shop.len() { return false; }
    let (kind, _) = shop[shop_index];
    let iinfo = item_info(kind);
    if state.gold < iinfo.buy_price { state.add_log("お金が足りない"); return false; }
    state.gold -= iinfo.buy_price;
    add_item(state, kind, 1);
    state.add_log(&format!("{}を購入 ({}G)", iinfo.name, iinfo.buy_price));
    true
}

// ── NPC / Quest ──────────────────────────────────────────────

pub fn talk_npc(state: &mut RpgState) -> bool {
    let loc = state.location;

    // Check for quest completions first
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        if quest_status(state, quest_id) == QuestStatus::ReadyToComplete
            && info.accept_location == loc
        {
            complete_quest(state, quest_id);
            update_world_text(state);
            return true;
        }
    }

    // Accept new quests
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        if info.accept_location != loc { continue; }
        if quest_status(state, quest_id) != QuestStatus::Available { continue; }
        if let Some(prereq) = info.prerequisite {
            if quest_status(state, prereq) != QuestStatus::Completed { continue; }
        }

        set_quest_status(state, quest_id, QuestStatus::Active);
        let kind_str = if info.kind == QuestKind::Main { "メイン" } else { "サイド" };
        state.add_log(&format!("【{}】「{}」を受注", kind_str, info.name));
        state.unlocks.quest_log_shortcut = true;
        update_world_text(state);
        return true;
    }

    // Default NPC dialogue
    match loc {
        LocationId::StartVillage => state.add_log("長老「気をつけて行くのじゃぞ」"),
        LocationId::HiddenLake => state.add_log("精霊「この湖には秘宝が眠っている…」"),
        _ => state.add_log("特に話すことはないようだ"),
    }
    true
}

pub fn rest(state: &mut RpgState) -> bool {
    if state.hp >= state.max_hp && state.mp >= state.max_mp {
        state.add_log("十分に休息している");
        return false;
    }
    let cost = if location_info(state.location).has_shop {
        let c = 10_u32.min(state.gold);
        if c > 0 { state.gold -= c; }
        c
    } else { 0 };

    let hp_r = state.max_hp / 3;
    let mp_r = state.max_mp / 3;
    state.hp = (state.hp + hp_r).min(state.max_hp);
    state.mp = (state.mp + mp_r).min(state.max_mp);

    if cost > 0 {
        state.add_log(&format!("宿屋で休んだ ({}G) HP+{} MP+{}", cost, hp_r, mp_r));
    } else {
        state.add_log(&format!("少し休んだ HP+{} MP+{}", hp_r, mp_r));
    }
    update_world_text(state);
    true
}

// ── Quest Helpers ────────────────────────────────────────────

pub fn quest_status(state: &RpgState, quest_id: QuestId) -> QuestStatus {
    state.quests.iter().find(|q| q.quest_id == quest_id)
        .map(|q| q.status).unwrap_or(QuestStatus::Available)
}

pub fn set_quest_status(state: &mut RpgState, quest_id: QuestId, status: QuestStatus) {
    if let Some(q) = state.quests.iter_mut().find(|q| q.quest_id == quest_id) {
        q.status = status;
    }
}

fn update_quest_kills(state: &mut RpgState, enemy_kind: EnemyKind) {
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        if quest_status(state, quest_id) != QuestStatus::Active { continue; }
        match &info.goal {
            QuestGoal::DefeatEnemies(target, required) => {
                if *target == enemy_kind && state.kill_count(enemy_kind) >= *required {
                    set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                    state.add_log(&format!("「{}」達成！ 報告しよう", info.name));
                }
            }
            QuestGoal::DefeatBoss(boss) => {
                if *boss == enemy_kind {
                    set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                    state.add_log(&format!("「{}」達成！", info.name));
                }
            }
            _ => {}
        }
    }
}

fn complete_quest(state: &mut RpgState, quest_id: QuestId) {
    let info = quest_info(quest_id);
    set_quest_status(state, quest_id, QuestStatus::Completed);
    state.gold += info.reward_gold;
    state.exp += info.reward_exp;
    if let Some(reward_item) = info.reward_item {
        add_item(state, reward_item, 1);
        state.add_log(&format!("報酬: {}を入手！", item_info(reward_item).name));
    }
    let kind_str = if info.kind == QuestKind::Main { "メイン" } else { "サイド" };
    state.add_log(&format!("【{}完了】「{}」 {}G / {}EXP", kind_str, info.name, info.reward_gold, info.reward_exp));
    check_level_up(state);
}

// ── Skills Query ─────────────────────────────────────────────

pub fn available_skills(level: u32) -> Vec<SkillKind> {
    ALL_SKILLS.iter().filter(|&&s| skill_info(s).learn_level <= level).copied().collect()
}

pub fn battle_consumables(state: &RpgState) -> Vec<(usize, ItemKind, u32)> {
    state.inventory.iter().enumerate()
        .filter(|(_, i)| item_info(i.kind).category == ItemCategory::Consumable && i.count > 0)
        .map(|(idx, i)| (idx, i.kind, i.count))
        .collect()
}

pub fn visible_quests(state: &RpgState) -> Vec<(QuestId, QuestStatus)> {
    let mut result = Vec::new();
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        let status = quest_status(state, quest_id);
        match status {
            QuestStatus::Active | QuestStatus::ReadyToComplete | QuestStatus::Completed => {
                result.push((quest_id, status));
            }
            QuestStatus::Available => {
                let prereq_met = match info.prerequisite {
                    None => true,
                    Some(pre) => quest_status(state, pre) == QuestStatus::Completed,
                };
                if prereq_met { result.push((quest_id, status)); }
            }
        }
    }
    result
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_is_noop() {
        let mut s = RpgState::new();
        tick(&mut s, 1000);
        assert_eq!(s.hp, 50);
    }

    #[test]
    fn prologue_step0() {
        let mut s = RpgState::new();
        assert_eq!(s.scene, Scene::Prologue(0));
        advance_prologue(&mut s);
        assert_eq!(s.scene, Scene::Prologue(1));
    }

    #[test]
    fn prologue_full_sequence() {
        let mut s = RpgState::new();
        advance_prologue(&mut s); // 0 -> 1
        advance_prologue(&mut s); // 1 -> 2 (get equipment)
        assert!(s.weapon.is_some());
        assert!(s.armor.is_some());
        assert_eq!(s.gold, 50);
        advance_prologue(&mut s); // 2 -> World
        assert_eq!(s.scene, Scene::World);
        assert!(s.unlocks.status_bar);
    }

    #[test]
    fn world_choices_at_village() {
        let mut s = RpgState::new();
        s.scene = Scene::World;
        // Complete prologue setup
        set_quest_status(&mut s, QuestId::MainPrepare, QuestStatus::Completed);
        set_quest_status(&mut s, QuestId::MainForest, QuestStatus::Active);
        s.hp = 40; // make rest visible
        let choices = world_choices(&s);
        // Should have: talk, shop, rest, travel to forest
        assert!(choices.len() >= 3);
    }

    #[test]
    fn travel_to_forest() {
        let mut s = RpgState::new();
        s.scene = Scene::World;
        s.rng_seed = 999;
        assert!(travel(&mut s, LocationId::Forest));
        assert_eq!(s.location, LocationId::Forest);
    }

    #[test]
    fn travel_mountain_needs_quest() {
        let mut s = RpgState::new();
        s.location = LocationId::Cave;
        assert!(!travel(&mut s, LocationId::MountainPath));
    }

    #[test]
    fn travel_mountain_after_quest() {
        let mut s = RpgState::new();
        s.location = LocationId::Cave;
        set_quest_status(&mut s, QuestId::MainCave, QuestStatus::Completed);
        s.rng_seed = 999;
        assert!(travel(&mut s, LocationId::MountainPath));
    }

    #[test]
    fn battle_attack_damages_enemy() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::IronSword);
        start_battle(&mut s, EnemyKind::Slime, false);
        assert!(battle_attack(&mut s));
        let b = s.battle.as_ref().unwrap();
        assert!(b.enemy.hp < 15);
    }

    #[test]
    fn battle_victory_gives_rewards() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::HolySword);
        start_battle(&mut s, EnemyKind::Slime, false);
        battle_attack(&mut s);
        assert_eq!(s.battle.as_ref().unwrap().phase, BattlePhase::Victory);
        process_victory(&mut s);
        assert!(s.exp > 0);
        assert!(s.gold > 0);
        assert_eq!(s.scene, Scene::World);
    }

    #[test]
    fn defeat_revives_at_village() {
        let mut s = RpgState::new();
        s.location = LocationId::Forest;
        s.gold = 100;
        start_battle(&mut s, EnemyKind::Slime, false);
        s.hp = 0;
        s.battle.as_mut().unwrap().phase = BattlePhase::Defeat;
        process_defeat(&mut s);
        assert_eq!(s.location, LocationId::StartVillage);
        assert_eq!(s.gold, 50);
    }

    #[test]
    fn add_item_stacks() {
        let mut s = RpgState::new();
        add_item(&mut s, ItemKind::Herb, 2);
        assert_eq!(s.item_count(ItemKind::Herb), 2);
        add_item(&mut s, ItemKind::Herb, 3);
        assert_eq!(s.item_count(ItemKind::Herb), 5);
    }

    #[test]
    fn equip_weapon() {
        let mut s = RpgState::new();
        add_item(&mut s, ItemKind::IronSword, 1);
        assert!(use_item(&mut s, 0));
        assert_eq!(s.weapon, Some(ItemKind::IronSword));
    }

    #[test]
    fn buy_item_at_shop() {
        let mut s = RpgState::new();
        s.gold = 100;
        assert!(buy_item(&mut s, 0));
        assert_eq!(s.gold, 80);
    }

    #[test]
    fn level_up_from_exp() {
        let mut s = RpgState::new();
        s.exp = 25;
        check_level_up(&mut s);
        assert_eq!(s.level, 2);
        assert_eq!(s.max_hp, 65);
    }

    #[test]
    fn quest_chain_unlock() {
        let mut s = RpgState::new();
        set_quest_status(&mut s, QuestId::MainPrepare, QuestStatus::Completed);
        let visible = visible_quests(&s);
        assert!(visible.iter().any(|(id, _)| *id == QuestId::MainForest));
    }

    #[test]
    fn demon_lord_clears_game() {
        let mut s = RpgState::new();
        s.level = 10;
        s.hp = 250; s.max_hp = 250;
        s.base_atk = 35;
        s.weapon = Some(ItemKind::HolySword);
        set_quest_status(&mut s, QuestId::MainFinal, QuestStatus::Active);
        start_battle(&mut s, EnemyKind::DemonLord, true);
        for _ in 0..20 {
            if s.battle.as_ref().map(|b| b.phase) == Some(BattlePhase::Victory) { break; }
            if s.battle.as_ref().map(|b| b.phase) == Some(BattlePhase::Defeat) { break; }
            battle_attack(&mut s);
            if let Some(b) = &mut s.battle {
                if b.phase != BattlePhase::Victory && b.phase != BattlePhase::Defeat {
                    b.phase = BattlePhase::SelectAction;
                }
            }
        }
        if s.battle.as_ref().map(|b| b.phase) == Some(BattlePhase::Victory) {
            process_victory(&mut s);
            assert!(s.game_cleared);
            assert_eq!(s.scene, Scene::GameClear);
        }
    }
}
