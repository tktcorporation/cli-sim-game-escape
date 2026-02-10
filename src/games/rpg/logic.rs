//! Dungeon Dive — pure game logic (no rendering / IO).
//!
//! Core loop: Town → Dungeon (room-by-room) → Battle → Town.
//! Resource management across floors is the central tension.

use super::state::{
    enemy_info, floor_enemies, item_info, level_stats, shop_items, skill_element, skill_info,
    BattleEnemy, BattlePhase, BattleState, DungeonFloor, EnemyKind, InventoryItem, ItemCategory,
    ItemKind, Overlay, Room, RoomKind, RoomResult, RpgState, Scene, SkillKind, ALL_SKILLS,
    MAX_FLOOR, MAX_LEVEL,
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

// ── Intro ─────────────────────────────────────────────────────

pub fn advance_intro(state: &mut RpgState) {
    let step = match state.scene {
        Scene::Intro(s) => s,
        _ => return,
    };

    match step {
        0 => {
            state.scene_text = vec![
                "冒険者ギルドの受付嬢が語りかける。".into(),
                "".into(),
                "「ようこそ、冒険者さん。".into(),
                "  この先にあるダンジョンには魔物が棲んでいます。".into(),
                "  奥深くには魔王が潜んでいるとか…」".into(),
                "".into(),
                "「まずはこれを持って行ってください」".into(),
            ];
            state.scene = Scene::Intro(1);
        }
        1 => {
            // Give starting equipment
            state.weapon = Some(ItemKind::WoodenSword);
            state.armor = Some(ItemKind::TravelClothes);
            state.gold += 50;
            add_item(state, ItemKind::Herb, 3);
            state.add_log("木の剣と旅人の服を受け取った！");
            state.add_log("薬草x3と50Gを受け取った！");
            state.scene = Scene::Town;
            update_town_text(state);
        }
        _ => {
            state.scene = Scene::Town;
            update_town_text(state);
        }
    }
}

// ── Town ──────────────────────────────────────────────────────

pub fn update_town_text(state: &mut RpgState) {
    let mut lines = vec![
        "＜冒険者ギルド＞".into(),
        "".into(),
    ];

    if state.max_floor_reached == 0 {
        lines.push("受付嬢「ダンジョンに挑戦してみましょう！」".into());
    } else if state.game_cleared {
        lines.push("受付嬢「おめでとうございます！もう一度挑戦も歓迎ですよ」".into());
    } else {
        lines.push(format!(
            "受付嬢「最深到達: B{}F。さらに奥を目指しましょう！」",
            state.max_floor_reached
        ));
    }

    state.scene_text = lines;
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
}

pub fn town_choices(state: &RpgState) -> Vec<Choice> {
    let mut choices = vec![Choice {
        label: "ダンジョンに入る".into(),
    }];
    choices.push(Choice {
        label: "ショップ".into(),
    });
    if state.hp < state.max_hp || state.mp < state.max_mp {
        choices.push(Choice {
            label: "休息 (HP/MP全回復)".into(),
        });
    }
    choices
}

pub fn execute_town_choice(state: &mut RpgState, index: usize) -> bool {
    let choices = town_choices(state);
    if index >= choices.len() {
        return false;
    }

    let mut pos = 0;

    // Enter dungeon
    if index == pos {
        enter_dungeon(state, 1);
        return true;
    }
    pos += 1;

    // Shop
    if index == pos {
        state.overlay = Some(Overlay::Shop);
        return true;
    }
    pos += 1;

    // Rest
    if (state.hp < state.max_hp || state.mp < state.max_mp) && index == pos {
        state.hp = state.max_hp;
        state.mp = state.max_mp;
        state.add_log("ゆっくり休んだ。HP/MPが全回復した！");
        update_town_text(state);
        return true;
    }

    let _ = pos;
    false
}

// ── Dungeon ──────────────────────────────────────────────────

/// Number of rooms per floor (increases with depth, last room is always stairs).
pub fn rooms_for_floor(floor: u32) -> u32 {
    // 4 rooms on F1, up to 7 on F10
    (3 + floor / 3).min(7)
}

pub fn enter_dungeon(state: &mut RpgState, floor: u32) {
    // Reset run stats if starting fresh
    if floor == 1 {
        state.run_gold_earned = 0;
        state.run_exp_earned = 0;
        state.run_enemies_killed = 0;
        state.run_rooms_cleared = 0;
    }

    let num_rooms = rooms_for_floor(floor) as usize;
    let mut rooms = Vec::with_capacity(num_rooms);

    for i in 0..num_rooms {
        if i == num_rooms - 1 {
            // Last room is always stairs (or boss on floor 10)
            if floor >= MAX_FLOOR {
                rooms.push(Room {
                    kind: RoomKind::Enemy,
                    visited: false,
                });
            } else {
                rooms.push(Room {
                    kind: RoomKind::Stairs,
                    visited: false,
                });
            }
        } else {
            rooms.push(Room {
                kind: generate_room_kind(state, floor),
                visited: false,
            });
        }
    }

    state.dungeon = Some(DungeonFloor {
        floor_num: floor,
        rooms,
        current_room: 0,
    });

    state.scene = Scene::Dungeon;
    state.room_result = None;
    state.add_log(&format!("B{}Fに踏み込んだ…", floor));

    // Update max floor
    if floor > state.max_floor_reached {
        state.max_floor_reached = floor;
    }
}

fn generate_room_kind(state: &mut RpgState, floor: u32) -> RoomKind {
    let roll = rng_range(state, 100);

    // Probabilities shift with depth: deeper = more enemies, fewer springs
    let (enemy, treasure, trap, spring) = match floor {
        1..=3 => (40, 20, 10, 15),  // shallow: gentle
        4..=6 => (50, 15, 15, 10),  // mid: balanced
        7..=9 => (55, 15, 20, 5),   // deep: hostile
        _ => (60, 15, 20, 5),       // deepest
    };

    if roll < enemy {
        RoomKind::Enemy
    } else if roll < enemy + treasure {
        RoomKind::Treasure
    } else if roll < enemy + treasure + trap {
        RoomKind::Trap
    } else if roll < enemy + treasure + trap + spring {
        RoomKind::Spring
    } else {
        RoomKind::Empty
    }
}

/// Advance to the current room and resolve its event.
pub fn enter_current_room(state: &mut RpgState) {
    let dungeon = match &mut state.dungeon {
        Some(d) => d,
        None => return,
    };

    let room_idx = dungeon.current_room;
    if room_idx >= dungeon.rooms.len() {
        return;
    }

    dungeon.rooms[room_idx].visited = true;
    let kind = dungeon.rooms[room_idx].kind;
    let floor = dungeon.floor_num;

    match kind {
        RoomKind::Enemy => {
            // Start a battle
            let enemies = floor_enemies(floor);
            let idx = rng_range(state, enemies.len() as u32) as usize;
            let is_boss = floor >= MAX_FLOOR
                && room_idx
                    == state
                        .dungeon
                        .as_ref()
                        .map(|d| d.rooms.len() - 1)
                        .unwrap_or(0);
            start_battle(state, enemies[idx], is_boss);
        }
        RoomKind::Treasure => {
            resolve_treasure(state, floor);
        }
        RoomKind::Trap => {
            resolve_trap(state, floor);
        }
        RoomKind::Spring => {
            resolve_spring(state);
        }
        RoomKind::Empty => {
            resolve_empty(state, floor);
        }
        RoomKind::Stairs => {
            resolve_stairs(state, floor);
        }
    }
}

fn resolve_treasure(state: &mut RpgState, floor: u32) {
    let roll = rng_range(state, 100);
    let mut desc = vec!["宝箱を発見した！".into()];

    if roll < 40 {
        // Gold
        let gold = 10 + floor * 8 + rng_range(state, floor * 5);
        state.gold += gold;
        state.run_gold_earned += gold;
        desc.push(format!("{}Gを手に入れた！", gold));
    } else if roll < 70 {
        // Herb
        let count = 1 + rng_range(state, 2);
        add_item(state, ItemKind::Herb, count);
        desc.push(format!("薬草x{}を手に入れた！", count));
    } else if roll < 85 {
        // Magic Water
        add_item(state, ItemKind::MagicWater, 1);
        desc.push("魔法の水を手に入れた！".into());
    } else {
        // Strength Potion
        add_item(state, ItemKind::StrengthPotion, 1);
        desc.push("力の薬を手に入れた！".into());
    }

    state.room_result = Some(RoomResult { description: desc });
    state.scene = Scene::DungeonResult;
}

fn resolve_trap(state: &mut RpgState, floor: u32) {
    let damage = 5 + floor * 3 + rng_range(state, floor * 2);
    // Higher level = chance to partially avoid
    let avoided = if state.level >= 5 {
        rng_range(state, 100) < 30
    } else {
        false
    };

    let actual_damage = if avoided { damage / 2 } else { damage };
    state.hp = state.hp.saturating_sub(actual_damage);

    let mut desc = vec!["罠だ！".into()];
    if avoided {
        desc.push(format!(
            "とっさに身をかわした！ {}ダメージ (軽減)",
            actual_damage
        ));
    } else {
        desc.push(format!("{}ダメージを受けた！", actual_damage));
    }

    if state.hp == 0 {
        desc.push("力尽きた…".into());
    }

    state.room_result = Some(RoomResult { description: desc });
    state.scene = Scene::DungeonResult;
}

fn resolve_spring(state: &mut RpgState) {
    let hp_heal = state.max_hp / 4;
    let mp_heal = state.max_mp / 4;
    state.hp = (state.hp + hp_heal).min(state.max_hp);
    state.mp = (state.mp + mp_heal).min(state.max_mp);

    state.room_result = Some(RoomResult {
        description: vec![
            "澄んだ泉を見つけた。".into(),
            format!("体を癒した。HP+{} MP+{}", hp_heal, mp_heal),
        ],
    });
    state.scene = Scene::DungeonResult;
}

fn resolve_empty(state: &mut RpgState, floor: u32) {
    let descriptions = match floor {
        1..=3 => &[
            "薄暗い通路が続いている。",
            "壁に苔が生えた古い部屋だ。",
            "松明の残りが壁にかかっている。",
            "静寂が耳に痛い。",
        ],
        4..=6 => &[
            "崩れかけた柱が並ぶ広間だ。",
            "地面に古い骨が散らばっている。",
            "遠くで何かが蠢く音がする。",
            "壁に古代文字が刻まれている。",
        ],
        _ => &[
            "空気が重い。魔力の気配を感じる。",
            "地面から微かに熱気が立ち上っている。",
            "闇が濃い。松明の光が届かない。",
            "壁面が赤黒く脈動している。",
        ],
    };

    let idx = rng_range(state, descriptions.len() as u32) as usize;
    state.room_result = Some(RoomResult {
        description: vec![descriptions[idx].into(), "何もなかった。".into()],
    });
    state.scene = Scene::DungeonResult;
}

fn resolve_stairs(state: &mut RpgState, floor: u32) {
    state.room_result = Some(RoomResult {
        description: vec![
            "下り階段を見つけた！".into(),
            format!("B{}Fへの道が開けている。", floor + 1),
        ],
    });
    state.scene = Scene::DungeonResult;
}

/// Advance to the next room in the dungeon.
pub fn advance_room(state: &mut RpgState) {
    state.room_result = None;

    // Check if dead from trap
    if state.hp == 0 {
        process_dungeon_death(state);
        return;
    }

    state.run_rooms_cleared += 1;

    let dungeon = match &mut state.dungeon {
        Some(d) => d,
        None => return,
    };

    dungeon.current_room += 1;

    if dungeon.current_room >= dungeon.rooms.len() {
        // Floor complete — this shouldn't happen normally (stairs handles it)
        retreat_to_town(state);
        return;
    }

    state.scene = Scene::Dungeon;
}

/// Descend to next floor (from stairs room).
pub fn descend_floor(state: &mut RpgState) {
    let floor = match &state.dungeon {
        Some(d) => d.floor_num,
        None => return,
    };
    state.run_rooms_cleared += 1;
    let next_floor = floor + 1;
    enter_dungeon(state, next_floor);
}

/// Retreat back to town, keeping all loot + return bonus.
pub fn retreat_to_town(state: &mut RpgState) {
    let run_gold = state.run_gold_earned;
    let run_exp = state.run_exp_earned;
    let run_kills = state.run_enemies_killed;
    let rooms = state.run_rooms_cleared;
    let floor = state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(1);

    // Return bonus: floor × rooms × 3
    let bonus = return_bonus(floor, rooms);
    if bonus > 0 {
        state.gold += bonus;
    }

    state.dungeon = None;
    state.battle = None;
    state.room_result = None;
    state.scene = Scene::Town;
    if run_kills > 0 || run_gold > 0 {
        if bonus > 0 {
            state.add_log(&format!(
                "帰還！ {}G/{}EXP/{}体撃破 帰還ボーナス+{}G",
                run_gold, run_exp, run_kills, bonus
            ));
        } else {
            state.add_log(&format!(
                "帰還！ 獲得: {}G / {}EXP / {}体撃破",
                run_gold, run_exp, run_kills
            ));
        }
    } else {
        state.add_log("町に戻った。");
    }
    update_town_text(state);
}

/// Calculate return bonus for surviving a dungeon run.
pub fn return_bonus(floor: u32, rooms_cleared: u32) -> u32 {
    floor * rooms_cleared * 3
}

fn process_dungeon_death(state: &mut RpgState) {
    // Death penalty: lose all gold earned this run + 20% of pre-run gold
    let run_gold = state.run_gold_earned;
    let pre_run_gold = state.gold.saturating_sub(run_gold);
    let extra_penalty = pre_run_gold / 5;
    let lost_gold = (run_gold + extra_penalty).min(state.gold);
    state.gold -= lost_gold;
    state.hp = state.max_hp / 2;
    state.mp = state.max_mp / 2;
    state.dungeon = None;
    state.battle = None;
    state.room_result = None;
    state.scene = Scene::Town;
    state.add_log(&format!("力尽きた… {}G失った", lost_gold));
    update_town_text(state);
}

// ── Battle ───────────────────────────────────────────────────

pub fn start_battle(state: &mut RpgState, enemy_kind: EnemyKind, is_boss: bool) {
    let info = enemy_info(enemy_kind);
    state.battle = Some(BattleState {
        enemy: BattleEnemy {
            kind: enemy_kind,
            hp: info.max_hp,
            max_hp: info.max_hp,
        },
        phase: BattlePhase::SelectAction,
        player_def_boost: 0,
        player_atk_boost: 0,
        log: vec![format!("{}が現れた！", info.name)],
        is_boss,
        enemy_charging: false,
        player_berserk: false,
    });
    state.scene = Scene::Battle;
}

pub fn battle_attack(state: &mut RpgState) -> bool {
    let player_atk = state.total_atk();
    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };
    if battle.phase != BattlePhase::SelectAction {
        return false;
    }

    let total_atk = player_atk + battle.player_atk_boost;
    let enemy_kind = battle.enemy.kind;
    let einfo = enemy_info(enemy_kind);
    let base_damage = total_atk.saturating_sub(einfo.def / 2).max(1);

    // Critical hit: 10% chance, 1.5x damage
    let crit_roll = rng_range(state, 100);
    let is_critical = crit_roll < 10;
    let damage = if is_critical {
        base_damage * 3 / 2
    } else {
        base_damage
    };

    let battle = state.battle.as_mut().unwrap();
    if is_critical {
        battle
            .log
            .push(format!("会心の一撃！ {}に{}ダメージ！", einfo.name, damage));
    } else {
        battle
            .log
            .push(format!("攻撃！ {}に{}ダメージ！", einfo.name, damage));
    }

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
    if skill_index >= available.len() {
        return false;
    }
    let skill_kind = available[skill_index];
    let sinfo = skill_info(skill_kind);

    if state.mp < sinfo.mp_cost {
        if let Some(b) = &mut state.battle {
            b.log.push("MPが足りない！".into());
        }
        return false;
    }
    state.mp -= sinfo.mp_cost;

    let enemy_kind = match &state.battle {
        Some(b) => b.enemy.kind,
        None => return false,
    };
    let einfo = enemy_info(enemy_kind);

    // Check elemental weakness
    let element = skill_element(skill_kind);
    let is_weak = element.is_some() && einfo.weakness == element;
    let weak_str = if is_weak { " [弱点!]" } else { "" };

    // Pre-compute values that require &self before mutable borrow of battle
    let player_atk = state.total_atk();
    let mag = state.mag;
    let max_hp = state.max_hp;

    let battle = state.battle.as_mut().unwrap();
    match skill_kind {
        SkillKind::Fire => {
            let base = (mag * sinfo.value)
                .saturating_sub(einfo.def / 3)
                .max(1);
            let damage = if is_weak { base * 3 / 2 } else { base };
            battle
                .log
                .push(format!("ファイア！ {}ダメージ！{}", damage, weak_str));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Heal => {
            let heal = mag * sinfo.value;
            state.hp = (state.hp + heal).min(max_hp);
            battle.log.push(format!("ヒール！ HP{}回復！", heal));
        }
        SkillKind::Shield => {
            battle.player_def_boost += sinfo.value;
            battle
                .log
                .push(format!("シールド！ DEF+{}！", sinfo.value));
        }
        SkillKind::IceBlade => {
            // Hybrid ATK + MAG attack with Ice element
            let base = (player_atk / 2 + mag * sinfo.value)
                .saturating_sub(einfo.def / 3)
                .max(1);
            let damage = if is_weak { base * 3 / 2 } else { base };
            battle
                .log
                .push(format!("アイスブレード！ {}ダメージ！{}", damage, weak_str));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Thunder => {
            // High power magic with Thunder element
            let base = (mag * sinfo.value)
                .saturating_sub(einfo.def / 4)
                .max(1);
            let damage = if is_weak { base * 3 / 2 } else { base };
            battle
                .log
                .push(format!("サンダー！ {}ダメージ！{}", damage, weak_str));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Drain => {
            // Damage + heal 50%
            let damage = (mag * sinfo.value)
                .saturating_sub(einfo.def / 3)
                .max(1);
            let heal = damage / 2;
            state.hp = (state.hp + heal).min(max_hp);
            battle.log.push(format!(
                "ドレイン！ {}ダメージ！ HP{}回復！",
                damage, heal
            ));
            battle.enemy.hp = battle.enemy.hp.saturating_sub(damage);
        }
        SkillKind::Berserk => {
            // ATK up, DEF down
            battle.player_atk_boost += sinfo.value;
            battle.player_def_boost = battle.player_def_boost.saturating_sub(5);
            battle.player_berserk = true;
            battle
                .log
                .push(format!("バーサク！ ATK+{}！ DEF-5！", sinfo.value));
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
                .log
                .push(format!("薬草を使った！ HP{}回復！", iinfo.value));
        }
        ItemKind::MagicWater => {
            state.mp = (state.mp + iinfo.value).min(state.max_mp);
            battle
                .log
                .push(format!("魔法の水を使った！ MP{}回復！", iinfo.value));
        }
        ItemKind::StrengthPotion => {
            battle.player_atk_boost += iinfo.value;
            battle
                .log
                .push(format!("力の薬を使った！ ATK+{}！", iinfo.value));
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
    let battle = match &mut state.battle {
        Some(b) => b,
        None => return false,
    };
    if battle.is_boss {
        battle.log.push("ボスからは逃げられない！".into());
        return false;
    }
    if rng_range(state, 100) < 60 {
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

    // Extract battle state info to avoid borrow conflicts with rng_range
    let (enemy_kind, enemy_hp, is_charging, can_charge, def_boost) = {
        let battle = match &state.battle {
            Some(b) => b,
            None => return,
        };
        if battle.enemy.hp == 0 {
            return;
        }
        (
            battle.enemy.kind,
            battle.enemy.hp,
            battle.enemy_charging,
            enemy_info(battle.enemy.kind).can_charge,
            battle.player_def_boost,
        )
    };

    let _ = enemy_hp;
    let einfo = enemy_info(enemy_kind);
    let total_def = player_def + def_boost;

    if is_charging {
        // Execute charged attack — 2x ATK
        let damage = (einfo.atk * 2).saturating_sub(total_def / 2).max(1);
        let battle = state.battle.as_mut().unwrap();
        battle.enemy_charging = false;
        let msg = match enemy_kind {
            EnemyKind::Dragon => format!("{}のブレス！ {}ダメージ！", einfo.name, damage),
            EnemyKind::DemonLord => {
                format!("{}の闇の波動！ {}ダメージ！", einfo.name, damage)
            }
            _ => format!("{}の渾身の一撃！ {}ダメージ！", einfo.name, damage),
        };
        battle.log.push(msg);
        state.hp = state.hp.saturating_sub(damage);
    } else if can_charge {
        // 25% chance to charge instead of attacking
        let charge_roll = rng_range(state, 100);
        if charge_roll < 25 {
            let battle = state.battle.as_mut().unwrap();
            battle.enemy_charging = true;
            let telegraph = match enemy_kind {
                EnemyKind::Golem => format!("{}は大振りの構えを取った！", einfo.name),
                EnemyKind::Dragon => format!("{}がブレスの準備をしている！", einfo.name),
                EnemyKind::DemonLord => format!("{}が闇の力を集めている…", einfo.name),
                _ => format!("{}は力を溜めている！", einfo.name),
            };
            battle.log.push(telegraph);
            return; // No damage this turn — player has a chance to Shield
        } else {
            let battle = state.battle.as_mut().unwrap();
            let damage = einfo.atk.saturating_sub(total_def / 2).max(1);
            battle
                .log
                .push(format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));
            state.hp = state.hp.saturating_sub(damage);
        }
    } else {
        // Normal attack
        let battle = state.battle.as_mut().unwrap();
        let damage = einfo.atk.saturating_sub(total_def / 2).max(1);
        battle
            .log
            .push(format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));
        state.hp = state.hp.saturating_sub(damage);
    }

    if state.hp == 0 {
        let battle = state.battle.as_mut().unwrap();
        battle.phase = BattlePhase::Defeat;
        battle.log.push("力尽きた...".into());
    }
}

pub fn process_victory(state: &mut RpgState) {
    let battle = match &state.battle {
        Some(b) => b,
        None => return,
    };
    let enemy_kind = battle.enemy.kind;
    let einfo = enemy_info(enemy_kind);

    state.exp += einfo.exp;
    state.gold += einfo.gold;
    state.run_gold_earned += einfo.gold;
    state.run_exp_earned += einfo.exp;
    state.run_enemies_killed += 1;
    state.add_log(&format!(
        "{}を倒した！ EXP+{} {}G",
        einfo.name, einfo.exp, einfo.gold
    ));

    // Item drop
    if let Some((drop_item, drop_pct)) = einfo.drop {
        if rng_range(state, 100) < drop_pct {
            add_item(state, drop_item, 1);
            state.add_log(&format!("{}をドロップ！", item_info(drop_item).name));
        }
    }

    check_level_up(state);
    state.battle = None;

    // Check game clear (demon lord defeated)
    if enemy_kind == EnemyKind::DemonLord {
        state.game_cleared = true;
        state.total_clears += 1;
        state.scene = Scene::GameClear;
        return;
    }

    // Return to dungeon exploration
    state.scene = Scene::DungeonResult;
    state.room_result = Some(RoomResult {
        description: vec![
            format!("{}を倒した！", einfo.name),
            format!("EXP+{} {}G獲得", einfo.exp, einfo.gold),
        ],
    });
}

pub fn process_defeat(state: &mut RpgState) {
    state.battle = None;
    process_dungeon_death(state);
}

pub fn process_fled(state: &mut RpgState) {
    state.battle = None;
    // Return to dungeon — same room, can try to advance
    state.scene = Scene::DungeonResult;
    state.room_result = Some(RoomResult {
        description: vec!["うまく逃げ切った！".into()],
    });
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
}

pub fn use_item(state: &mut RpgState, inv_index: usize) -> bool {
    if inv_index >= state.inventory.len() {
        return false;
    }
    let kind = state.inventory[inv_index].kind;
    let iinfo = item_info(kind);

    match iinfo.category {
        ItemCategory::Consumable => match kind {
            ItemKind::Herb => {
                if state.hp >= state.max_hp {
                    state.add_log("HPは満タン");
                    return false;
                }
                state.hp = (state.hp + iinfo.value).min(state.max_hp);
                state.add_log(&format!("薬草を使った！ HP{}回復", iinfo.value));
            }
            ItemKind::MagicWater => {
                if state.mp >= state.max_mp {
                    state.add_log("MPは満タン");
                    return false;
                }
                state.mp = (state.mp + iinfo.value).min(state.max_mp);
                state.add_log(&format!("魔法の水を使った！ MP{}回復", iinfo.value));
            }
            ItemKind::StrengthPotion => {
                state.add_log("戦闘中にしか使えない");
                return false;
            }
            _ => {
                state.add_log("使えないアイテム");
                return false;
            }
        },
        ItemCategory::Weapon => {
            let old = state.weapon;
            state.weapon = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 {
                state.inventory.remove(inv_index);
            }
            if let Some(old_kind) = old {
                add_item(state, old_kind, 1);
            }
            state.add_log(&format!("{}を装備した", iinfo.name));
            return true;
        }
        ItemCategory::Armor => {
            let old = state.armor;
            state.armor = Some(kind);
            state.inventory[inv_index].count -= 1;
            if state.inventory[inv_index].count == 0 {
                state.inventory.remove(inv_index);
            }
            if let Some(old_kind) = old {
                add_item(state, old_kind, 1);
            }
            state.add_log(&format!("{}を装備した", iinfo.name));
            return true;
        }
    }
    // Consumable used
    state.inventory[inv_index].count -= 1;
    if state.inventory[inv_index].count == 0 {
        state.inventory.remove(inv_index);
    }
    true
}

// ── Shop ─────────────────────────────────────────────────────

pub fn buy_item(state: &mut RpgState, shop_index: usize) -> bool {
    let shop = shop_items(state.max_floor_reached);
    if shop_index >= shop.len() {
        return false;
    }
    let (kind, _) = shop[shop_index];
    let iinfo = item_info(kind);
    if state.gold < iinfo.buy_price {
        state.add_log("お金が足りない");
        return false;
    }
    state.gold -= iinfo.buy_price;
    add_item(state, kind, 1);
    state.add_log(&format!("{}を購入 ({}G)", iinfo.name, iinfo.buy_price));
    true
}

// ── Skills Query ─────────────────────────────────────────────

pub fn available_skills(level: u32) -> Vec<SkillKind> {
    ALL_SKILLS
        .iter()
        .filter(|&&s| skill_info(s).learn_level <= level)
        .copied()
        .collect()
}

pub fn battle_consumables(state: &RpgState) -> Vec<(usize, ItemKind, u32)> {
    state
        .inventory
        .iter()
        .enumerate()
        .filter(|(_, i)| item_info(i.kind).category == ItemCategory::Consumable && i.count > 0)
        .map(|(idx, i)| (idx, i.kind, i.count))
        .collect()
}

/// Dungeon progress info for display.
pub fn dungeon_progress(state: &RpgState) -> Option<(u32, usize, usize)> {
    state
        .dungeon
        .as_ref()
        .map(|d| (d.floor_num, d.current_room + 1, d.rooms.len()))
}

/// Get the room kind at current position (for display).
pub fn current_room_kind(state: &RpgState) -> Option<RoomKind> {
    state
        .dungeon
        .as_ref()
        .and_then(|d| d.rooms.get(d.current_room))
        .map(|r| r.kind)
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
    fn intro_step0() {
        let mut s = RpgState::new();
        assert_eq!(s.scene, Scene::Intro(0));
        advance_intro(&mut s);
        assert_eq!(s.scene, Scene::Intro(1));
    }

    #[test]
    fn intro_full_sequence() {
        let mut s = RpgState::new();
        advance_intro(&mut s); // 0 -> 1
        advance_intro(&mut s); // 1 -> Town
        assert!(s.weapon.is_some());
        assert!(s.armor.is_some());
        assert_eq!(s.gold, 50);
        assert_eq!(s.scene, Scene::Town);
    }

    #[test]
    fn town_choices_basic() {
        let mut s = RpgState::new();
        s.scene = Scene::Town;
        let choices = town_choices(&s);
        // Full HP/MP: dungeon + shop (no rest)
        assert_eq!(choices.len(), 2);
        // With damaged HP: dungeon + shop + rest
        s.hp = 30;
        let choices2 = town_choices(&s);
        assert_eq!(choices2.len(), 3);
    }

    #[test]
    fn enter_dungeon_creates_floor() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.scene, Scene::Dungeon);
        assert!(s.dungeon.is_some());
        let d = s.dungeon.as_ref().unwrap();
        assert_eq!(d.floor_num, 1);
        assert!(!d.rooms.is_empty());
        assert_eq!(d.current_room, 0);
        // Last room should be stairs
        assert_eq!(d.rooms.last().unwrap().kind, RoomKind::Stairs);
    }

    #[test]
    fn floor10_has_enemy_as_last_room() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 10);
        let d = s.dungeon.as_ref().unwrap();
        assert_eq!(d.rooms.last().unwrap().kind, RoomKind::Enemy);
    }

    #[test]
    fn retreat_to_town_preserves_loot() {
        let mut s = RpgState::new();
        s.gold = 100;
        s.run_gold_earned = 50;
        enter_dungeon(&mut s, 1);
        retreat_to_town(&mut s);
        assert_eq!(s.scene, Scene::Town);
        assert_eq!(s.gold, 100); // gold preserved
        assert!(s.dungeon.is_none());
    }

    #[test]
    fn battle_attack_damages_enemy() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::IronSword);
        start_battle(&mut s, EnemyKind::Slime, false);
        assert!(battle_attack(&mut s));
        let b = s.battle.as_ref().unwrap();
        assert!(b.enemy.hp < 12);
    }

    #[test]
    fn battle_victory_gives_rewards() {
        let mut s = RpgState::new();
        s.weapon = Some(ItemKind::HolySword);
        enter_dungeon(&mut s, 1);
        start_battle(&mut s, EnemyKind::Slime, false);
        battle_attack(&mut s);
        assert_eq!(s.battle.as_ref().unwrap().phase, BattlePhase::Victory);
        process_victory(&mut s);
        assert!(s.exp > 0);
        assert!(s.gold > 0);
        assert_eq!(s.scene, Scene::DungeonResult);
    }

    #[test]
    fn defeat_returns_to_town() {
        let mut s = RpgState::new();
        s.gold = 100;
        enter_dungeon(&mut s, 1);
        start_battle(&mut s, EnemyKind::Slime, false);
        s.hp = 0;
        s.battle.as_mut().unwrap().phase = BattlePhase::Defeat;
        process_defeat(&mut s);
        assert_eq!(s.scene, Scene::Town);
        // Death penalty: lose run gold (0) + 20% pre-run gold (100*20%=20)
        assert_eq!(s.gold, 80);
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
        assert!(buy_item(&mut s, 0)); // Herb costs 20
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
    fn demon_lord_clears_game() {
        let mut s = RpgState::new();
        s.level = 10;
        s.hp = 250;
        s.max_hp = 250;
        s.base_atk = 35;
        s.weapon = Some(ItemKind::HolySword);
        enter_dungeon(&mut s, 10);
        start_battle(&mut s, EnemyKind::DemonLord, true);
        for _ in 0..30 {
            if s.battle.as_ref().map(|b| b.phase) == Some(BattlePhase::Victory) {
                break;
            }
            if s.battle.as_ref().map(|b| b.phase) == Some(BattlePhase::Defeat) {
                break;
            }
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

    #[test]
    fn rooms_for_floor_increases() {
        assert!(rooms_for_floor(1) <= rooms_for_floor(10));
    }

    #[test]
    fn descend_floor_works() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 3);
        descend_floor(&mut s);
        assert_eq!(s.dungeon.as_ref().unwrap().floor_num, 4);
    }

    #[test]
    fn return_bonus_scales_with_floor_and_rooms() {
        assert_eq!(return_bonus(1, 0), 0);
        assert_eq!(return_bonus(1, 3), 9);  // 1 * 3 * 3
        assert_eq!(return_bonus(5, 4), 60); // 5 * 4 * 3
        assert_eq!(return_bonus(10, 7), 210); // 10 * 7 * 3
    }

    #[test]
    fn retreat_gives_return_bonus() {
        let mut s = RpgState::new();
        s.gold = 50;
        enter_dungeon(&mut s, 3);
        s.run_rooms_cleared = 4;
        s.run_gold_earned = 20;
        s.run_enemies_killed = 2;
        let gold_before = s.gold;
        retreat_to_town(&mut s);
        // Bonus = 3 * 4 * 3 = 36
        assert_eq!(s.gold, gold_before + 36);
    }

    #[test]
    fn death_penalty_loses_run_gold() {
        let mut s = RpgState::new();
        s.gold = 200;
        enter_dungeon(&mut s, 1);
        // Simulate earning gold during the run
        s.run_gold_earned = 80;
        // pre_run = 200 - 80 = 120, extra = 120/5 = 24
        // lost = 80 + 24 = 104
        s.hp = 0;
        process_dungeon_death(&mut s);
        assert_eq!(s.gold, 96);
    }

    #[test]
    fn new_skills_available_at_correct_levels() {
        assert_eq!(available_skills(1).len(), 1); // Fire
        assert_eq!(available_skills(2).len(), 2); // +Heal
        assert_eq!(available_skills(3).len(), 3); // +IceBlade
        assert_eq!(available_skills(4).len(), 4); // +Shield
        assert_eq!(available_skills(5).len(), 5); // +Thunder
        assert_eq!(available_skills(6).len(), 6); // +Drain
        assert_eq!(available_skills(8).len(), 7); // +Berserk
    }

    #[test]
    fn rooms_cleared_increments_on_advance() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.run_rooms_cleared, 0);
        s.hp = s.max_hp; // ensure not dead
        // Simulate resolving a room
        s.scene = Scene::DungeonResult;
        s.room_result = Some(RoomResult {
            description: vec!["test".into()],
        });
        advance_room(&mut s);
        assert_eq!(s.run_rooms_cleared, 1);
    }
}
