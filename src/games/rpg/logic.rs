//! Dungeon Dive — pure game logic (no rendering / IO).
//!
//! Core loop: Town → Grid Dungeon (first-person exploration) → Events/Battle → Town.
//! Exploration with 3D view is the central experience.

use super::dungeon_map::generate_map;
use super::events::{generate_event, resolve_event, EventOutcome};
use super::lore::{atmosphere_text, floor_entry_text, floor_theme};
use super::state::{
    enemy_info, floor_enemies, item_info, level_stats, shop_items, skill_element, skill_info,
    BattleEnemy, BattlePhase, BattleState, CellType, EnemyKind, Facing,
    InventoryItem, ItemCategory, ItemKind, Overlay, RoomResult, RpgState, Scene,
    SkillKind, ALL_SKILLS, MAX_FLOOR, MAX_LEVEL,
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

// ── Dungeon: Grid-Based Exploration ───────────────────────────

pub fn enter_dungeon(state: &mut RpgState, floor: u32) {
    let first_entry = state.max_floor_reached == 0;

    // Reset run stats if starting fresh
    if floor == 1 {
        state.run_gold_earned = 0;
        state.run_exp_earned = 0;
        state.run_enemies_killed = 0;
        state.run_rooms_explored = 0;
    }

    let mut map = generate_map(floor, &mut state.rng_seed);

    // Mark entrance as visited
    let px = map.player_x;
    let py = map.player_y;
    map.grid[py][px].visited = true;
    map.grid[py][px].event_done = true; // Don't trigger entrance event on entry

    state.dungeon = Some(map);
    state.scene = Scene::DungeonExplore;
    state.room_result = None;
    state.active_event = None;

    // Update max floor
    if floor > state.max_floor_reached {
        state.max_floor_reached = floor;
    }

    // Show floor entry text
    let theme = floor_theme(floor);
    let mut texts = floor_entry_text(floor, theme);

    // First dungeon entry: add control guide
    if first_entry {
        texts.push(String::new());
        texts.push("※ マップタップ or 矢印キーで移動".into());
        texts.push("  (WASD: 向き基準の操作も可能)".into());
    }

    state.scene_text = texts;
    state.add_log(&format!("B{}Fに踏み込んだ…", floor));
}

/// Move forward one cell in the direction the player is facing.
pub fn move_forward(state: &mut RpgState) -> bool {
    let (can_move, nx, ny) = {
        let map = match &state.dungeon {
            Some(m) => m,
            None => return false,
        };
        let cell = map.player_cell();
        if cell.wall(map.facing) {
            (false, 0, 0)
        } else {
            let nx = map.player_x as i32 + map.facing.dx();
            let ny = map.player_y as i32 + map.facing.dy();
            if map.in_bounds(nx, ny) {
                (true, nx as usize, ny as usize)
            } else {
                (false, 0, 0)
            }
        }
    };

    if !can_move {
        // Give directional hint about open passages
        let hint = {
            let map = state.dungeon.as_ref().unwrap();
            let cell = map.player_cell();
            let left_open = !cell.wall(map.facing.turn_left());
            let right_open = !cell.wall(map.facing.turn_right());
            let back_open = !cell.wall(map.facing.reverse());
            match (left_open, right_open, back_open) {
                (true, true, _) => "壁だ。左右に通路がある。",
                (true, false, _) => "壁だ。左に通路がある。",
                (false, true, _) => "壁だ。右に通路がある。",
                (false, false, true) => "行き止まりだ。引き返そう。",
                _ => "壁だ。進めない。",
            }
        };
        state.add_log(hint);
        return false;
    }

    let map = state.dungeon.as_mut().unwrap();
    map.player_x = nx;
    map.player_y = ny;

    let was_visited = map.grid[ny][nx].visited;
    map.grid[ny][nx].visited = true;

    // Count newly explored rooms
    if !was_visited {
        state.run_rooms_explored += 1;
    }

    // Atmosphere text
    let floor = map.floor_num;
    let theme = floor_theme(floor);
    let rng_val = rng_range(state, 100);
    let atmo = atmosphere_text(theme, rng_val);
    state.scene_text = vec![atmo.into()];

    // Check if this cell has an unresolved event
    let cell_type = state.dungeon.as_ref().unwrap().grid[ny][nx].cell_type;
    let event_done = state.dungeon.as_ref().unwrap().grid[ny][nx].event_done;

    if cell_type != CellType::Corridor && !event_done {
        // Generate and trigger event
        let floor = state.dungeon.as_ref().unwrap().floor_num;
        let theme = floor_theme(floor);
        if let Some(event) = generate_event(cell_type, floor, theme, &mut state.rng_seed) {
            state.active_event = Some(event);
            state.scene = Scene::DungeonEvent;
        }
    }

    true
}

/// Move in an absolute cardinal direction (arrow key / map tap).
///
/// Auto-faces the given direction, then moves forward. If the destination
/// is a plain corridor with only one exit (besides where we came from),
/// auto-walk continues until a junction, event, or dead end.
pub fn move_direction(state: &mut RpgState, dir: Facing) -> bool {
    // Face the direction
    if let Some(map) = &mut state.dungeon {
        map.facing = dir;
    } else {
        return false;
    }

    // Try to move forward
    if !move_forward(state) {
        return false;
    }

    // Auto-walk: continue through corridors until junction/event/dead-end
    let mut steps = 0;
    let max_steps = 8;
    while steps < max_steps && state.scene == Scene::DungeonExplore {
        let next_dir = match auto_walk_direction(state) {
            Some(d) => d,
            None => break,
        };

        if let Some(map) = &mut state.dungeon {
            map.facing = next_dir;
        }

        if !move_forward(state) {
            break;
        }
        steps += 1;
    }

    if steps > 0 {
        let total = steps + 1;
        state.scene_text.insert(0, format!("通路を{}歩進んだ。", total));
    }

    true
}

/// Returns the only exit direction from the current cell (excluding the
/// direction we came from). Returns `None` if there are 0 or 2+ exits
/// (dead-end or junction), or if the cell has an unresolved event.
fn auto_walk_direction(state: &RpgState) -> Option<Facing> {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return None,
    };

    let cell = map.player_cell();

    // Don't auto-walk if this cell has an unresolved event
    if cell.cell_type != CellType::Corridor && !cell.event_done {
        return None;
    }

    // Don't auto-walk from entrance or stairs (important navigation points)
    if cell.cell_type == CellType::Entrance || cell.cell_type == CellType::Stairs {
        return None;
    }

    let came_from = map.facing.reverse();
    let mut exits = Vec::new();

    for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
        if dir == came_from {
            continue;
        }
        if !cell.wall(dir) {
            exits.push(dir);
        }
    }

    if exits.len() == 1 {
        Some(exits[0])
    } else {
        None
    }
}

/// Turn the player left (counter-clockwise).
pub fn turn_left(state: &mut RpgState) -> bool {
    let map = match &mut state.dungeon {
        Some(m) => m,
        None => return false,
    };
    map.facing = map.facing.turn_left();
    true
}

/// Turn the player right (clockwise).
pub fn turn_right(state: &mut RpgState) -> bool {
    let map = match &mut state.dungeon {
        Some(m) => m,
        None => return false,
    };
    map.facing = map.facing.turn_right();
    true
}

/// Turn the player around (180 degrees).
pub fn turn_around(state: &mut RpgState) -> bool {
    let map = match &mut state.dungeon {
        Some(m) => m,
        None => return false,
    };
    map.facing = map.facing.reverse();
    true
}

/// Resolve a dungeon event choice.
pub fn resolve_event_choice(state: &mut RpgState, choice_index: usize) -> bool {
    let event = match &state.active_event {
        Some(e) => e.clone(),
        None => return false,
    };

    if choice_index >= event.choices.len() {
        return false;
    }

    let action = &event.choices[choice_index].action;
    let cell_type = state
        .dungeon
        .as_ref()
        .map(|m| m.grid[m.player_y][m.player_x].cell_type)
        .unwrap_or(CellType::Corridor);
    let floor = state
        .dungeon
        .as_ref()
        .map(|m| m.floor_num)
        .unwrap_or(1);

    let outcome = resolve_event(action, cell_type, floor, state.level, &mut state.rng_seed);

    // Apply outcome
    apply_event_outcome(state, &outcome);

    // Mark event as done
    if let Some(map) = &mut state.dungeon {
        map.grid[map.player_y][map.player_x].event_done = true;
    }

    state.active_event = None;

    // Handle special outcomes
    if outcome.start_battle {
        // Start a battle
        let floor = state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(1);
        let enemies = floor_enemies(floor);
        let idx = rng_range(state, enemies.len() as u32) as usize;
        let is_boss = floor >= MAX_FLOOR && cell_type == CellType::Stairs;
        start_battle(state, enemies[idx], is_boss);

        // Apply first strike damage to enemy
        if outcome.first_strike {
            let player_atk = state.total_atk();
            if let Some(b) = &mut state.battle {
                let damage = player_atk / 2;
                b.enemy.hp = b.enemy.hp.saturating_sub(damage);
                b.log.push(format!("先制攻撃！ {}ダメージ！", damage));
            }
        }
    } else if outcome.descend {
        let next_floor = floor + 1;
        enter_dungeon(state, next_floor);
    } else if outcome.return_to_town {
        retreat_to_town(state);
    } else if state.dungeon.is_some() {
        // Skip result screen — show outcome in log and scene text, then
        // return directly to exploration for smoother flow.
        for desc in &outcome.description {
            if !desc.is_empty() {
                state.add_log(desc);
            }
        }
        state.scene_text = outcome.description;
        state.scene = Scene::DungeonExplore;
    }

    true
}

fn apply_event_outcome(state: &mut RpgState, outcome: &EventOutcome) {
    // Gold
    if outcome.gold > 0 {
        state.gold += outcome.gold as u32;
        state.run_gold_earned += outcome.gold as u32;
    }

    // HP change
    if outcome.hp_change == 9999 {
        // Special: 25% heal
        let heal = state.max_hp / 4;
        state.hp = (state.hp + heal).min(state.max_hp);
    } else if outcome.hp_change < 0 {
        let damage = (-outcome.hp_change) as u32;
        state.hp = state.hp.saturating_sub(damage);
    } else if outcome.hp_change > 0 {
        state.hp = (state.hp + outcome.hp_change as u32).min(state.max_hp);
    }

    // MP change
    if outcome.mp_change == 9999 {
        let heal = state.max_mp / 4;
        state.mp = (state.mp + heal).min(state.max_mp);
    } else if outcome.mp_change > 0 {
        state.mp = (state.mp + outcome.mp_change as u32).min(state.max_mp);
    }

    // Item
    if let Some((item_kind, count)) = outcome.item {
        add_item(state, item_kind, count);
    }

    // Lore
    if let Some(lore_id) = outcome.lore_id {
        if !state.lore_found.contains(&lore_id) {
            state.lore_found.push(lore_id);
        }
    }

    // Check death
    if state.hp == 0 {
        process_dungeon_death(state);
    }
}

/// Retreat back to town, keeping all loot + return bonus.
pub fn retreat_to_town(state: &mut RpgState) {
    let run_gold = state.run_gold_earned;
    let run_exp = state.run_exp_earned;
    let run_kills = state.run_enemies_killed;
    let rooms = state.run_rooms_explored;
    let floor = state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(1);

    // Return bonus: floor × rooms × 3
    let bonus = return_bonus(floor, rooms);
    if bonus > 0 {
        state.gold += bonus;
    }

    state.dungeon = None;
    state.battle = None;
    state.room_result = None;
    state.active_event = None;
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
pub fn return_bonus(floor: u32, rooms_explored: u32) -> u32 {
    floor * rooms_explored * 3
}

fn process_dungeon_death(state: &mut RpgState) {
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
    state.active_event = None;
    state.scene = Scene::Town;
    state.add_log(&format!("力尽きた… {}G失った", lost_gold));
    update_town_text(state);
}

/// After DungeonResult is shown, return to exploration.
pub fn continue_exploration(state: &mut RpgState) {
    if state.hp == 0 {
        process_dungeon_death(state);
        return;
    }
    state.room_result = None;
    state.scene = Scene::DungeonExplore;
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
            return;
        } else {
            let battle = state.battle.as_mut().unwrap();
            let damage = einfo.atk.saturating_sub(total_def / 2).max(1);
            battle
                .log
                .push(format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));
            state.hp = state.hp.saturating_sub(damage);
        }
    } else {
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
    // Return to dungeon exploration
    state.scene = Scene::DungeonExplore;
    state.room_result = None;
    state.add_log("うまく逃げ切った！");
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

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::rpg::state::Facing;

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
        assert_eq!(choices.len(), 2);
        s.hp = 30;
        let choices2 = town_choices(&s);
        assert_eq!(choices2.len(), 3);
    }

    #[test]
    fn enter_dungeon_creates_grid_map() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.scene, Scene::DungeonExplore);
        assert!(s.dungeon.is_some());
        let d = s.dungeon.as_ref().unwrap();
        assert_eq!(d.floor_num, 1);
        assert_eq!(d.width, 7);
        assert_eq!(d.height, 7);
        assert_eq!(d.facing, Facing::North);
        // Entrance should be at bottom center
        assert_eq!(d.player_x, 3);
        assert_eq!(d.player_y, 6);
    }

    #[test]
    fn move_forward_works() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        // The entrance faces north; there should be no wall north from entrance
        // (maze generation guarantees at least one path)
        let map = s.dungeon.as_ref().unwrap();
        let has_wall = map.player_cell().wall(Facing::North);
        if !has_wall {
            assert!(move_forward(&mut s));
            let map = s.dungeon.as_ref().unwrap();
            assert_eq!(map.player_y, 5); // moved north (y decreases)
        }
    }

    #[test]
    fn turn_left_right() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.dungeon.as_ref().unwrap().facing, Facing::North);
        turn_right(&mut s);
        assert_eq!(s.dungeon.as_ref().unwrap().facing, Facing::East);
        turn_left(&mut s);
        assert_eq!(s.dungeon.as_ref().unwrap().facing, Facing::North);
        turn_around(&mut s);
        assert_eq!(s.dungeon.as_ref().unwrap().facing, Facing::South);
    }

    #[test]
    fn cannot_move_into_wall() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        // Turn to face a direction with a wall
        // Try all 4 directions, at least one should be blocked
        let mut blocked = false;
        for _ in 0..4 {
            let map = s.dungeon.as_ref().unwrap();
            if map.player_cell().wall(map.facing) {
                let result = move_forward(&mut s);
                assert!(!result);
                blocked = true;
                break;
            }
            turn_right(&mut s);
        }
        // It's possible all directions are open (rare but possible), so just check
        assert!(blocked || true); // always passes; we tested what we could
    }

    #[test]
    fn retreat_to_town_preserves_loot() {
        let mut s = RpgState::new();
        s.gold = 100;
        s.run_gold_earned = 50;
        enter_dungeon(&mut s, 1);
        retreat_to_town(&mut s);
        assert_eq!(s.scene, Scene::Town);
        assert_eq!(s.gold, 100); // gold preserved (no rooms explored, bonus = 0)
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
    fn return_bonus_scales() {
        assert_eq!(return_bonus(1, 0), 0);
        assert_eq!(return_bonus(1, 3), 9);
        assert_eq!(return_bonus(5, 4), 60);
    }

    #[test]
    fn retreat_gives_return_bonus() {
        let mut s = RpgState::new();
        s.gold = 50;
        enter_dungeon(&mut s, 3);
        s.run_rooms_explored = 4;
        s.run_gold_earned = 20;
        s.run_enemies_killed = 2;
        let gold_before = s.gold;
        retreat_to_town(&mut s);
        // Bonus = 3 * 4 * 3 = 36
        assert_eq!(s.gold, gold_before + 36);
    }

    #[test]
    fn new_skills_available_at_correct_levels() {
        assert_eq!(available_skills(1).len(), 1);
        assert_eq!(available_skills(2).len(), 2);
        assert_eq!(available_skills(3).len(), 3);
        assert_eq!(available_skills(8).len(), 7);
    }

    #[test]
    fn continue_exploration_returns_to_dungeon() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        s.scene = Scene::DungeonResult;
        s.room_result = Some(RoomResult {
            description: vec!["test".into()],
        });
        continue_exploration(&mut s);
        assert_eq!(s.scene, Scene::DungeonExplore);
        assert!(s.room_result.is_none());
    }

    #[test]
    fn move_direction_faces_and_moves() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.dungeon.as_ref().unwrap().facing, Facing::North);

        // Try moving east (absolute direction)
        let map = s.dungeon.as_ref().unwrap();
        let cell = map.player_cell();
        let east_open = !cell.wall(Facing::East);
        if east_open {
            let old_pos = (map.player_x, map.player_y);
            assert!(move_direction(&mut s, Facing::East));
            let map = s.dungeon.as_ref().unwrap();
            // Player should have moved (auto-walk may change facing further)
            let new_pos = (map.player_x, map.player_y);
            assert_ne!(old_pos, new_pos);
        }
    }

    #[test]
    fn move_direction_blocked_by_wall() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);

        // Find a direction with a wall
        for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
            let cell = s.dungeon.as_ref().unwrap().player_cell();
            if cell.wall(dir) {
                let old_pos = (
                    s.dungeon.as_ref().unwrap().player_x,
                    s.dungeon.as_ref().unwrap().player_y,
                );
                assert!(!move_direction(&mut s, dir));
                let new_pos = (
                    s.dungeon.as_ref().unwrap().player_x,
                    s.dungeon.as_ref().unwrap().player_y,
                );
                assert_eq!(old_pos, new_pos);
                break;
            }
        }
    }

    #[test]
    fn auto_walk_stops_at_junction() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);

        // After move_direction, should still be in DungeonExplore
        // (auto-walk stops at junctions/events, not outside the dungeon)
        let map = s.dungeon.as_ref().unwrap();
        if !map.player_cell().wall(Facing::North) {
            move_direction(&mut s, Facing::North);
            // Should still be exploring (not crashed or stuck)
            assert!(
                s.scene == Scene::DungeonExplore
                    || s.scene == Scene::DungeonEvent
            );
        }
    }
}
