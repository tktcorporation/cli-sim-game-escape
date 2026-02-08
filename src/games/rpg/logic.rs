//! RPG Quest — pure game logic (no rendering / IO).

use super::state::{
    enemy_info, encounter_table, item_info, level_stats, location_info, quest_info, skill_info,
    shop_inventory, BattleAction, BattleEnemy, BattleState, DialogueState, EnemyKind,
    InventoryItem, ItemCategory, ItemKind, LocationId, QuestGoal, QuestId, QuestKind,
    QuestStatus, RpgState, Screen, SkillKind, ALL_QUESTS, ALL_SKILLS, MAX_LEVEL,
};

// ── Tick (no-op: command-based game) ─────────────────────────

pub fn tick(_state: &mut RpgState, _delta_ticks: u32) {
    // RPG is command-based; no per-tick logic.
}

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

// ── Travel ───────────────────────────────────────────────────

pub fn travel(state: &mut RpgState, dest_index: usize) -> bool {
    let info = location_info(state.location);
    if dest_index >= info.connections.len() {
        return false;
    }
    let dest = info.connections[dest_index];

    // Check if MountainPath requires AncientKey
    if dest == LocationId::MountainPath && state.item_count(ItemKind::AncientKey) == 0 {
        // Only block if coming from Cave (need key from cave quest)
        let main_cave = quest_status(state, QuestId::MainCave);
        if main_cave != QuestStatus::Completed {
            state.add_log("古代の鍵が必要です...");
            return false;
        }
    }

    // Check if DemonCastle requires MainMountain complete
    if dest == LocationId::DemonCastle {
        let main_mountain = quest_status(state, QuestId::MainMountain);
        if main_mountain != QuestStatus::Completed {
            state.add_log("山道の試練を突破する必要があります...");
            return false;
        }
    }

    state.location = dest;
    let dest_info = location_info(dest);
    state.add_log(&format!("{}に到着した", dest_info.name));

    // Random encounter in areas with enemies
    if dest_info.has_encounters {
        let encounter_chance = match dest {
            LocationId::DemonCastle => 80,
            LocationId::MountainPath => 70,
            _ => 50,
        };
        if rng_range(state, 100) < encounter_chance {
            start_random_encounter(state);
        }
    }

    true
}

// ── Explore ──────────────────────────────────────────────────

pub fn explore(state: &mut RpgState) -> bool {
    let loc = state.location;
    let loc_info = location_info(loc);

    // Check for quest items to find
    if check_explore_find(state, loc) {
        return true;
    }

    // Random encounter in exploration
    if loc_info.has_encounters {
        state.add_log("周囲を探索した...");
        if rng_range(state, 100) < 60 {
            start_random_encounter(state);
        } else {
            // Find some gold
            let gold = rng_range(state, 15) + 5;
            state.gold += gold;
            state.add_log(&format!("探索中に{}Gを見つけた！", gold));
        }
    } else {
        state.add_log("周囲を探索したが、何も見つからなかった");
    }
    true
}

fn check_explore_find(state: &mut RpgState, loc: LocationId) -> bool {
    // Check active quests for FindItem goals at this location
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        let status = quest_status(state, quest_id);
        if status != QuestStatus::Active {
            continue;
        }

        if let QuestGoal::FindItem(item_kind) = &info.goal {
            // Determine valid find location for each item
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

                // For herb collection quest, need 3
                if quest_id == QuestId::SideHerbCollect {
                    if state.item_count(ItemKind::Herb) >= 3 {
                        set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                        state.add_log("薬草を3つ集めた！村に報告しよう");
                    }
                } else {
                    set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                    state.add_log(&format!("クエスト「{}」の目標を達成した！", info.name));
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
    if table.is_empty() {
        return;
    }
    let idx = rng_range(state, table.len() as u32) as usize;
    let enemy_kind = table[idx];
    start_battle(state, enemy_kind, false);
}

pub fn start_battle(state: &mut RpgState, enemy_kind: EnemyKind, is_boss: bool) {
    let info = enemy_info(enemy_kind);
    state.battle = Some(BattleState {
        enemy: BattleEnemy {
            kind: enemy_kind,
            hp: info.max_hp,
            max_hp: info.max_hp,
        },
        action: BattleAction::SelectAction,
        player_def_boost: 0,
        player_atk_boost: 0,
        battle_log: vec![format!("{}が現れた！", info.name)],
        is_boss,
    });
    state.screen = Screen::Battle;
}

pub fn battle_attack(state: &mut RpgState) -> bool {
    let player_atk = state.total_atk();

    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };
    if battle.action != BattleAction::SelectAction {
        return false;
    }

    let total_atk = player_atk + battle.player_atk_boost;
    let enemy = &battle.enemy;
    let einfo = enemy_info(enemy.kind);
    let damage = total_atk.saturating_sub(einfo.def / 2).max(1);

    battle
        .battle_log
        .push(format!("攻撃！ {}に{}ダメージ！", einfo.name, damage));

    let battle = state.battle.as_mut().unwrap();
    battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);

    if battle.enemy.hp == 0 {
        battle.action = BattleAction::Victory;
        let einfo = enemy_info(battle.enemy.kind);
        battle
            .battle_log
            .push(format!("{}を倒した！", einfo.name));
    } else {
        process_enemy_turn(state);
    }
    true
}

pub fn battle_use_skill(state: &mut RpgState, skill_index: usize) -> bool {
    let available = available_skills(state.level);
    if skill_index >= available.len() {
        return false;
    }
    let skill_kind = available[skill_index];
    let sinfo = skill_info(skill_kind);

    if state.mp < sinfo.mp_cost {
        if let Some(battle) = &mut state.battle {
            battle.battle_log.push("MPが足りない！".to_string());
        }
        return false;
    }

    state.mp -= sinfo.mp_cost;

    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };

    match skill_kind {
        SkillKind::Fire => {
            let damage = (state.mag * sinfo.value).saturating_sub(enemy_info(battle.enemy.kind).def / 3).max(1);
            battle
                .battle_log
                .push(format!("ファイア！ {}ダメージ！", damage));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Heal => {
            let heal = state.mag * sinfo.value;
            state.hp = (state.hp + heal).min(state.max_hp);
            battle
                .battle_log
                .push(format!("ヒール！ HP{}回復！", heal));
        }
        SkillKind::Shield => {
            battle.player_def_boost += sinfo.value;
            battle.battle_log.push(format!(
                "シールド！ DEF+{}！",
                sinfo.value
            ));
        }
    }

    // Set back to SelectAction for rendering, then process enemy turn
    let battle = state.battle.as_mut().unwrap();
    if battle.enemy.hp == 0 {
        battle.action = BattleAction::Victory;
        let einfo = enemy_info(battle.enemy.kind);
        battle
            .battle_log
            .push(format!("{}を倒した！", einfo.name));
    } else {
        battle.action = BattleAction::SelectAction;
        process_enemy_turn(state);
    }
    true
}

pub fn battle_use_item(state: &mut RpgState, inv_index: usize) -> bool {
    let consumables: Vec<usize> = state
        .inventory
        .iter()
        .enumerate()
        .filter(|(_, i)| item_info(i.kind).category == ItemCategory::Consumable && i.count > 0)
        .map(|(idx, _)| idx)
        .collect();

    if inv_index >= consumables.len() {
        return false;
    }
    let actual_idx = consumables[inv_index];
    let item_kind = state.inventory[actual_idx].kind;
    let iinfo = item_info(item_kind);

    // Use the item
    state.inventory[actual_idx].count -= 1;
    if state.inventory[actual_idx].count == 0 {
        state.inventory.remove(actual_idx);
    }

    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };

    match item_kind {
        ItemKind::Herb => {
            state.hp = (state.hp + iinfo.value).min(state.max_hp);
            battle
                .battle_log
                .push(format!("薬草を使った！ HP{}回復！", iinfo.value));
        }
        ItemKind::MagicWater => {
            state.mp = (state.mp + iinfo.value).min(state.max_mp);
            battle
                .battle_log
                .push(format!("魔法の水を使った！ MP{}回復！", iinfo.value));
        }
        ItemKind::StrengthPotion => {
            battle.player_atk_boost += iinfo.value;
            battle
                .battle_log
                .push(format!("力の薬を使った！ ATK+{}！", iinfo.value));
        }
        _ => {
            battle.battle_log.push("そのアイテムは使えない".to_string());
            return false;
        }
    }

    let battle = state.battle.as_mut().unwrap();
    battle.action = BattleAction::SelectAction;
    process_enemy_turn(state);
    true
}

pub fn battle_flee(state: &mut RpgState) -> bool {
    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };
    if battle.is_boss {
        battle.battle_log.push("ボスからは逃げられない！".to_string());
        return false;
    }

    if rng_chance(state, 0.6) {
        let battle = state.battle.as_mut().unwrap();
        battle.battle_log.push("うまく逃げ切った！".to_string());
        battle.action = BattleAction::Fled;
    } else {
        let battle = state.battle.as_mut().unwrap();
        battle.battle_log.push("逃げられなかった！".to_string());
        process_enemy_turn(state);
    }
    true
}

fn process_enemy_turn(state: &mut RpgState) {
    let player_def = state.total_def();

    let battle = match &mut state.battle {
        Some(b) => b,
        None => return,
    };
    if battle.enemy.hp == 0 {
        return;
    }

    let einfo = enemy_info(battle.enemy.kind);
    let total_def = player_def + battle.player_def_boost;
    let damage = einfo.atk.saturating_sub(total_def / 2).max(1);

    battle
        .battle_log
        .push(format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));

    state.hp = state.hp.saturating_sub(damage);
    if state.hp == 0 {
        let battle = state.battle.as_mut().unwrap();
        battle.action = BattleAction::Defeat;
        battle.battle_log.push("力尽きた...".to_string());
    }
}

pub fn process_victory(state: &mut RpgState) {
    let battle = match &state.battle {
        Some(b) => b,
        None => return,
    };
    let enemy_kind = battle.enemy.kind;
    let einfo = enemy_info(enemy_kind);

    // Gain EXP and gold
    state.exp += einfo.exp;
    state.gold += einfo.gold;
    state.add_log(&format!(
        "{}を倒した！ EXP+{} {}G獲得",
        einfo.name, einfo.exp, einfo.gold
    ));

    // Track kills
    if let Some(kc) = state.kill_counts.iter_mut().find(|k| k.0 == enemy_kind) {
        kc.1 += 1;
    } else {
        state.kill_counts.push((enemy_kind, 1));
    }

    // Check for item drop
    if let Some((drop_item, drop_chance)) = einfo.drop {
        if rng_chance(state, drop_chance) {
            add_item(state, drop_item, 1);
            let iinfo = item_info(drop_item);
            state.add_log(&format!("{}をドロップした！", iinfo.name));
        }
    }

    // Check level up
    check_level_up(state);

    // Update quest progress
    update_quest_kills(state, enemy_kind);

    // Clear battle
    state.battle = None;
    state.screen = Screen::World;

    // Check game clear (demon lord defeated)
    if enemy_kind == EnemyKind::DemonLord {
        // Complete the final quest
        set_quest_status(state, QuestId::MainFinal, QuestStatus::Completed);
        state.game_cleared = true;
        state.screen = Screen::GameClear;
    }
}

pub fn process_defeat(state: &mut RpgState) {
    // Revive at village with half gold
    state.hp = state.max_hp / 2;
    state.mp = state.max_mp / 2;
    state.gold /= 2;
    state.location = LocationId::StartVillage;
    state.battle = None;
    state.screen = Screen::World;
    state.add_log("村で目を覚ました... 所持金の半分を失った");
}

pub fn process_fled(state: &mut RpgState) {
    state.battle = None;
    state.screen = Screen::World;
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
            // Full recovery on level up
            state.hp = state.max_hp;
            state.mp = state.max_mp;
            state.add_log(&format!(
                "レベルアップ！ Lv.{} HP:{} MP:{} ATK:{} DEF:{} MAG:{}",
                state.level,
                new_stats.max_hp,
                new_stats.max_mp,
                new_stats.atk,
                new_stats.def,
                new_stats.mag
            ));
            // Check for new skills
            for &skill in ALL_SKILLS {
                let sinfo = skill_info(skill);
                if sinfo.learn_level == state.level {
                    state.add_log(&format!("スキル「{}」を習得した！", sinfo.name));
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
}

pub fn use_item(state: &mut RpgState, inv_index: usize) -> bool {
    if inv_index >= state.inventory.len() {
        return false;
    }
    let kind = state.inventory[inv_index].kind;
    let iinfo = item_info(kind);

    match iinfo.category {
        ItemCategory::Consumable => {
            match kind {
                ItemKind::Herb => {
                    if state.hp >= state.max_hp {
                        state.add_log("HPは満タンです");
                        return false;
                    }
                    state.hp = (state.hp + iinfo.value).min(state.max_hp);
                    state.add_log(&format!("薬草を使った！ HP{}回復", iinfo.value));
                }
                ItemKind::MagicWater => {
                    if state.mp >= state.max_mp {
                        state.add_log("MPは満タンです");
                        return false;
                    }
                    state.mp = (state.mp + iinfo.value).min(state.max_mp);
                    state.add_log(&format!("魔法の水を使った！ MP{}回復", iinfo.value));
                }
                ItemKind::StrengthPotion => {
                    state.add_log("戦闘中にしか使えません");
                    return false;
                }
                _ => {
                    state.add_log("使えないアイテムです");
                    return false;
                }
            }
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 {
                state.inventory.remove(inv_index);
            }
            true
        }
        ItemCategory::Weapon => {
            // Unequip current weapon (put back in inventory count)
            // Equip new weapon
            let old_weapon = state.weapon;
            state.weapon = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 {
                state.inventory.remove(inv_index);
            }
            if let Some(old) = old_weapon {
                add_item(state, old, 1);
                let old_info = item_info(old);
                state.add_log(&format!(
                    "{}を装備した ({}を外した)",
                    iinfo.name, old_info.name
                ));
            } else {
                state.add_log(&format!("{}を装備した", iinfo.name));
            }
            true
        }
        ItemCategory::Armor => {
            let old_armor = state.armor;
            state.armor = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 {
                state.inventory.remove(inv_index);
            }
            if let Some(old) = old_armor {
                add_item(state, old, 1);
                let old_info = item_info(old);
                state.add_log(&format!(
                    "{}を装備した ({}を外した)",
                    iinfo.name, old_info.name
                ));
            } else {
                state.add_log(&format!("{}を装備した", iinfo.name));
            }
            true
        }
        ItemCategory::KeyItem => {
            state.add_log("キーアイテムは使えません");
            false
        }
    }
}

// ── Shop ─────────────────────────────────────────────────────

pub fn buy_item(state: &mut RpgState, shop_index: usize) -> bool {
    let shop = shop_inventory(state.location);
    if shop_index >= shop.len() {
        return false;
    }
    let (kind, _stock) = shop[shop_index];
    let iinfo = item_info(kind);

    if state.gold < iinfo.buy_price {
        state.add_log("お金が足りません");
        return false;
    }

    state.gold -= iinfo.buy_price;
    add_item(state, kind, 1);
    state.add_log(&format!("{}を購入した ({}G)", iinfo.name, iinfo.buy_price));
    true
}

// ── NPC / Dialogue ───────────────────────────────────────────

pub fn talk_npc(state: &mut RpgState) -> bool {
    let loc = state.location;

    // Check for quest completions first
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        let status = quest_status(state, quest_id);

        if status == QuestStatus::ReadyToComplete && info.accept_location == loc {
            complete_quest(state, quest_id);
            return true;
        }
    }

    // Check for new quests to accept
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        if info.accept_location != loc {
            continue;
        }
        let status = quest_status(state, quest_id);
        if status != QuestStatus::Available {
            continue;
        }
        // Check prerequisite
        if let Some(prereq) = info.prerequisite {
            if quest_status(state, prereq) != QuestStatus::Completed {
                continue;
            }
        }

        // Special handling for MainPrepare quest (instant complete on talk)
        if quest_id == QuestId::MainPrepare {
            set_quest_status(state, quest_id, QuestStatus::Active);
            complete_quest(state, quest_id);
            // Give starting equipment
            add_item(state, ItemKind::TravelClothes, 1);
            state.add_log("旅人の服も受け取った");
            return true;
        }

        // Accept quest
        set_quest_status(state, quest_id, QuestStatus::Active);
        let kind_str = if info.kind == QuestKind::Main {
            "メイン"
        } else {
            "サイド"
        };
        state.dialogue = Some(DialogueState {
            lines: vec![
                format!("【{}クエスト受注】", kind_str),
                format!("「{}」", info.name),
                info.description.to_string(),
                format!(
                    "報酬: {}G / {}EXP",
                    info.reward_gold, info.reward_exp
                ),
            ],
            current_line: 0,
        });
        state.screen = Screen::Dialogue;
        state.add_log(&format!("クエスト「{}」を受注した", info.name));
        return true;
    }

    // Default NPC dialogue
    let dialogue_lines = match loc {
        LocationId::StartVillage => vec![
            "長老「魔王を倒してくれる冒険者を待っておった」".to_string(),
            "長老「まずは装備を整え、森を越えるのじゃ」".to_string(),
        ],
        LocationId::HiddenLake => vec![
            "精霊「この湖には秘宝が眠っている...」".to_string(),
            "精霊「勇気ある者だけが見つけられるだろう」".to_string(),
        ],
        _ => vec!["特に話すことはないようだ".to_string()],
    };

    state.dialogue = Some(DialogueState {
        lines: dialogue_lines,
        current_line: 0,
    });
    state.screen = Screen::Dialogue;
    true
}

pub fn advance_dialogue(state: &mut RpgState) -> bool {
    let dialogue = match &mut state.dialogue {
        Some(d) => d,
        None => return false,
    };

    dialogue.current_line += 1;
    if dialogue.current_line >= dialogue.lines.len() {
        state.dialogue = None;
        state.screen = Screen::World;
    }
    true
}

// ── Rest ─────────────────────────────────────────────────────

pub fn rest(state: &mut RpgState) -> bool {
    if state.hp >= state.max_hp && state.mp >= state.max_mp {
        state.add_log("もう十分に休息している");
        return false;
    }

    let cost = if location_info(state.location).has_shop {
        // Inn at village
        let cost = 10_u32.min(state.gold);
        if cost > 0 {
            state.gold -= cost;
        }
        cost
    } else {
        0
    };

    let hp_recover = state.max_hp / 3;
    let mp_recover = state.max_mp / 3;
    state.hp = (state.hp + hp_recover).min(state.max_hp);
    state.mp = (state.mp + mp_recover).min(state.max_mp);

    if cost > 0 {
        state.add_log(&format!(
            "宿屋で休んだ ({}G) HP+{} MP+{}",
            cost, hp_recover, mp_recover
        ));
    } else {
        state.add_log(&format!("少し休んだ HP+{} MP+{}", hp_recover, mp_recover));
    }
    true
}

// ── Quest Helpers ────────────────────────────────────────────

pub fn quest_status(state: &RpgState, quest_id: QuestId) -> QuestStatus {
    state
        .quests
        .iter()
        .find(|q| q.quest_id == quest_id)
        .map(|q| q.status)
        .unwrap_or(QuestStatus::Available)
}

fn set_quest_status(state: &mut RpgState, quest_id: QuestId, status: QuestStatus) {
    if let Some(q) = state.quests.iter_mut().find(|q| q.quest_id == quest_id) {
        q.status = status;
    }
}

fn update_quest_kills(state: &mut RpgState, enemy_kind: EnemyKind) {
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        let status = quest_status(state, quest_id);
        if status != QuestStatus::Active {
            continue;
        }

        match &info.goal {
            QuestGoal::DefeatEnemies(target_kind, required) => {
                if *target_kind == enemy_kind {
                    let count = state.kill_count(enemy_kind);
                    if count >= *required {
                        set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                        state.add_log(&format!("クエスト「{}」の目標を達成！", info.name));
                    }
                }
            }
            QuestGoal::DefeatBoss(boss_kind) => {
                if *boss_kind == enemy_kind {
                    set_quest_status(state, quest_id, QuestStatus::ReadyToComplete);
                    state.add_log(&format!("クエスト「{}」の目標を達成！", info.name));
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
        let iinfo = item_info(reward_item);
        state.add_log(&format!("報酬: {}を入手！", iinfo.name));
    }

    let kind_str = if info.kind == QuestKind::Main {
        "メイン"
    } else {
        "サイド"
    };
    state.add_log(&format!(
        "【{}クエスト完了】「{}」 {}G / {}EXP",
        kind_str, info.name, info.reward_gold, info.reward_exp
    ));

    check_level_up(state);

    // Show dialogue
    state.dialogue = Some(DialogueState {
        lines: vec![
            format!("【クエスト完了】「{}」", info.name),
            format!("報酬: {}G / {}EXP", info.reward_gold, info.reward_exp),
        ],
        current_line: 0,
    });
    state.screen = Screen::Dialogue;
}

// ── Skills Query ─────────────────────────────────────────────

pub fn available_skills(level: u32) -> Vec<SkillKind> {
    ALL_SKILLS
        .iter()
        .filter(|&&s| skill_info(s).learn_level <= level)
        .copied()
        .collect()
}

/// Get consumable items for battle use.
pub fn battle_consumables(state: &RpgState) -> Vec<(usize, ItemKind, u32)> {
    state
        .inventory
        .iter()
        .enumerate()
        .filter(|(_, i)| item_info(i.kind).category == ItemCategory::Consumable && i.count > 0)
        .map(|(idx, i)| (idx, i.kind, i.count))
        .collect()
}

/// Get active quests visible to the player.
pub fn visible_quests(state: &RpgState) -> Vec<(QuestId, QuestStatus)> {
    let mut result = Vec::new();
    for &quest_id in ALL_QUESTS {
        let info = quest_info(quest_id);
        let status = quest_status(state, quest_id);

        // Show completed and active quests
        if status == QuestStatus::Active
            || status == QuestStatus::ReadyToComplete
            || status == QuestStatus::Completed
        {
            result.push((quest_id, status));
            continue;
        }

        // Show available quests if prerequisite is met
        if status == QuestStatus::Available {
            let prereq_met = match info.prerequisite {
                None => true,
                Some(prereq) => quest_status(state, prereq) == QuestStatus::Completed,
            };
            if prereq_met {
                result.push((quest_id, status));
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
        assert_eq!(s.gold, 0);
    }

    #[test]
    fn travel_to_forest() {
        let mut s = RpgState::new();
        // StartVillage connections: [Forest]
        s.rng_seed = 999; // avoid encounter
        assert!(travel(&mut s, 0));
        assert_eq!(s.location, LocationId::Forest);
    }

    #[test]
    fn travel_invalid_index() {
        let mut s = RpgState::new();
        assert!(!travel(&mut s, 99));
        assert_eq!(s.location, LocationId::StartVillage);
    }

    #[test]
    fn travel_to_mountain_needs_key() {
        let mut s = RpgState::new();
        s.location = LocationId::Cave;
        // Cave connections: [Forest, MountainPath]
        assert!(!travel(&mut s, 1)); // MountainPath needs AncientKey
        assert_eq!(s.location, LocationId::Cave);
    }

    #[test]
    fn travel_to_mountain_with_key() {
        let mut s = RpgState::new();
        s.location = LocationId::Cave;
        // Complete MainCave quest
        set_quest_status(&mut s, QuestId::MainCave, QuestStatus::Completed);
        s.rng_seed = 999;
        assert!(travel(&mut s, 1));
        assert_eq!(s.location, LocationId::MountainPath);
    }

    #[test]
    fn battle_attack_damages_enemy() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::IronSword); // ATK = 5 + 8 = 13
        start_battle(&mut s, EnemyKind::Slime, false);
        assert!(battle_attack(&mut s));
        let battle = s.battle.as_ref().unwrap();
        assert!(battle.enemy.hp < 15); // Slime max HP = 15
    }

    #[test]
    fn battle_victory_gives_rewards() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::HolySword); // ATK = 5 + 25 = 30
        start_battle(&mut s, EnemyKind::Slime, false);
        battle_attack(&mut s);
        // Slime should be dead
        let battle = s.battle.as_ref().unwrap();
        assert_eq!(battle.action, BattleAction::Victory);
        process_victory(&mut s);
        assert!(s.exp > 0);
        assert!(s.gold > 0);
        assert!(s.battle.is_none());
        assert_eq!(s.screen, Screen::World);
    }

    #[test]
    fn battle_flee_works() {
        let mut s = RpgState::new();
        s.rng_seed = 42; // deterministic
        start_battle(&mut s, EnemyKind::Slime, false);
        // Try fleeing multiple times
        for _ in 0..10 {
            if let Some(b) = &s.battle {
                if b.action == BattleAction::Fled {
                    break;
                }
                if b.action == BattleAction::Defeat {
                    break;
                }
            } else {
                break;
            }
            battle_flee(&mut s);
            if let Some(b) = &s.battle {
                if b.action != BattleAction::Fled && b.action != BattleAction::Defeat {
                    s.battle.as_mut().unwrap().action = BattleAction::SelectAction;
                }
            }
        }
        // Should have either fled or been defeated
    }

    #[test]
    fn battle_flee_boss_fails() {
        let mut s = RpgState::new();
        start_battle(&mut s, EnemyKind::DemonLord, true);
        assert!(!battle_flee(&mut s));
    }

    #[test]
    fn defeat_revives_at_village() {
        let mut s = RpgState::new();
        s.location = LocationId::Forest;
        s.gold = 100;
        start_battle(&mut s, EnemyKind::Slime, false);
        s.hp = 0;
        s.battle.as_mut().unwrap().action = BattleAction::Defeat;
        process_defeat(&mut s);
        assert_eq!(s.location, LocationId::StartVillage);
        assert_eq!(s.gold, 50);
        assert!(s.hp > 0);
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
    fn use_herb_heals() {
        let mut s = RpgState::new();
        s.hp = 20;
        add_item(&mut s, ItemKind::Herb, 1);
        assert!(use_item(&mut s, 0));
        assert_eq!(s.hp, 50); // 20 + 30, capped at max_hp 50
    }

    #[test]
    fn use_herb_full_hp_fails() {
        let mut s = RpgState::new();
        add_item(&mut s, ItemKind::Herb, 1);
        assert!(!use_item(&mut s, 0));
        assert_eq!(s.item_count(ItemKind::Herb), 1); // not consumed
    }

    #[test]
    fn equip_weapon() {
        let mut s = RpgState::new();
        add_item(&mut s, ItemKind::IronSword, 1);
        assert!(use_item(&mut s, 0));
        assert_eq!(s.weapon, Some(ItemKind::IronSword));
        assert_eq!(s.total_atk(), 13); // 5 + 8
    }

    #[test]
    fn equip_weapon_unequips_old() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::WoodenSword);
        add_item(&mut s, ItemKind::IronSword, 1);
        use_item(&mut s, 0);
        assert_eq!(s.weapon, Some(ItemKind::IronSword));
        // Old weapon should be back in inventory
        assert_eq!(s.item_count(ItemKind::WoodenSword), 1);
    }

    #[test]
    fn buy_item_at_shop() {
        let mut s = RpgState::new();
        s.gold = 100;
        // Shop index 0 = Herb (20G)
        assert!(buy_item(&mut s, 0));
        assert_eq!(s.gold, 80);
        assert_eq!(s.item_count(ItemKind::Herb), 1);
    }

    #[test]
    fn buy_item_no_money() {
        let mut s = RpgState::new();
        s.gold = 5;
        assert!(!buy_item(&mut s, 0)); // Herb costs 20G
    }

    #[test]
    fn level_up_from_exp() {
        let mut s = RpgState::new();
        s.exp = 25; // Enough for level 2 (need 20)
        check_level_up(&mut s);
        assert_eq!(s.level, 2);
        assert_eq!(s.max_hp, 65);
        assert_eq!(s.hp, 65); // full heal on level up
    }

    #[test]
    fn quest_main_prepare() {
        let mut s = RpgState::new();
        // Talk to NPC to start and complete MainPrepare
        talk_npc(&mut s);
        assert_eq!(quest_status(&s, QuestId::MainPrepare), QuestStatus::Completed);
        // Should get wooden sword from reward
        assert_eq!(s.item_count(ItemKind::WoodenSword), 1);
        // Should also get travel clothes
        assert_eq!(s.item_count(ItemKind::TravelClothes), 1);
    }

    #[test]
    fn quest_chain_unlock() {
        let mut s = RpgState::new();
        // Complete MainPrepare
        set_quest_status(&mut s, QuestId::MainPrepare, QuestStatus::Completed);
        // MainForest should now be available
        let visible = visible_quests(&s);
        assert!(visible
            .iter()
            .any(|(id, _)| *id == QuestId::MainForest));
    }

    #[test]
    fn available_skills_by_level() {
        assert_eq!(available_skills(1).len(), 1); // Fire
        assert_eq!(available_skills(2).len(), 2); // Fire, Heal
        assert_eq!(available_skills(4).len(), 3); // Fire, Heal, Shield
    }

    #[test]
    fn rest_recovers_hp_mp() {
        let mut s = RpgState::new();
        s.hp = 10;
        s.mp = 5;
        assert!(rest(&mut s));
        assert!(s.hp > 10);
        assert!(s.mp > 5);
    }

    #[test]
    fn rest_at_full_fails() {
        let mut s = RpgState::new();
        assert!(!rest(&mut s));
    }

    #[test]
    fn kill_tracking_updates_quest() {
        let mut s = RpgState::new();
        set_quest_status(&mut s, QuestId::MainPrepare, QuestStatus::Completed);
        set_quest_status(&mut s, QuestId::MainForest, QuestStatus::Active);

        // Kill 3 slimes
        for _ in 0..3 {
            s.weapon = Some(ItemKind::HolySword);
            start_battle(&mut s, EnemyKind::Slime, false);
            battle_attack(&mut s);
            process_victory(&mut s);
        }
        assert!(s.kill_count(EnemyKind::Slime) >= 3);
        assert_eq!(
            quest_status(&s, QuestId::MainForest),
            QuestStatus::ReadyToComplete
        );
    }

    #[test]
    fn demon_lord_defeat_clears_game() {
        let mut s = RpgState::new();
        s.level = 10;
        let stats = level_stats(10);
        s.max_hp = stats.max_hp;
        s.hp = stats.max_hp;
        s.base_atk = stats.atk;
        s.weapon = Some(ItemKind::HolySword);
        set_quest_status(&mut s, QuestId::MainFinal, QuestStatus::Active);

        start_battle(&mut s, EnemyKind::DemonLord, true);
        // Beat the demon lord by attacking until dead
        for _ in 0..20 {
            if let Some(b) = &s.battle {
                if b.action == BattleAction::Victory {
                    break;
                }
                if b.action == BattleAction::Defeat {
                    break;
                }
            }
            battle_attack(&mut s);
            if let Some(b) = &s.battle {
                if b.action != BattleAction::Victory && b.action != BattleAction::Defeat {
                    s.battle.as_mut().unwrap().action = BattleAction::SelectAction;
                }
            }
        }
        if let Some(b) = &s.battle {
            if b.action == BattleAction::Victory {
                process_victory(&mut s);
                assert!(s.game_cleared);
                assert_eq!(s.screen, Screen::GameClear);
            }
        }
    }
}
