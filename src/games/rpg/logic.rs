//! Dungeon Dive — pure game logic (no rendering / IO).
//!
//! Inline-combat roguelike: player and monsters share the grid.
//! Each player action triggers a monster turn (chase + attack).

use super::dungeon_map::generate_map;
use super::events::{generate_event, resolve_event, EventOutcome};
use super::lore::{atmosphere_text, floor_entry_text, floor_theme};
use super::overworld_map::generate_overworld;
use super::state::{
    affix_info, enemy_info, item_info, level_stats, shop_items, skill_element, skill_info,
    CellType, DungeonEvent, EnemyKind, EventAction, EventChoice, Facing, InventoryItem,
    ItemCategory, ItemKind, Monster, Overlay, Pet, PlayerBuffs, Quest, QuestKind, RpgState,
    Scene, SkillKind, Tile, ALL_AFFIXES, ALL_SKILLS, MAX_FLOOR, MAX_LEVEL,
};

// ── Tick (no-op: command-based game) ─────────────────────────

pub fn tick(_state: &mut RpgState, _delta_ticks: u32) {}

// ── Cursor navigation (Issue: arrow + A/B unification) ───────
//
// All choice-based menus (intro / town / event popup / overlays) share a
// single `state.cursor` index. Arrow keys move it; A confirms; B cancels.
// Number keys still work as direct shortcuts for backward compat.
//
// The owner of "how many choices does THIS menu have" is `cursor_count`;
// every handler that needs cursor selection asks here. Keeping the source
// of truth in one place avoids drift between render highlights and key
// handlers.

/// Count of selectable items in the current scene/overlay/event popup.
/// Returns 0 when no cursor navigation is active (e.g. dungeon explore
/// without a popup — arrow keys move the player there).
pub fn cursor_count(state: &RpgState) -> usize {
    match state.overlay {
        Some(Overlay::Inventory) => state.inventory.len().min(9),
        Some(Overlay::Shop) => shop_items(state.max_floor_reached).len().min(9),
        Some(Overlay::SkillMenu) => available_skills(state.level).len(),
        Some(Overlay::QuestBoard) => {
            if state.active_quest.is_some() {
                1 // abandon button
            } else {
                available_quests(state).len()
            }
        }
        Some(Overlay::PrayMenu) => {
            if state.prayed_this_run {
                0
            } else {
                1
            }
        }
        Some(Overlay::Status) => 0,
        None => match state.scene {
            Scene::Overworld | Scene::DungeonExplore => state
                .active_event
                .as_ref()
                .map(|e| e.choices.len())
                .unwrap_or(0),
            Scene::GameClear => 1,
        },
    }
}

/// Move the cursor by `delta` (-1 / +1) within the current menu's range.
/// Wraps around for a more natural feel on small lists.
pub fn cursor_move(state: &mut RpgState, delta: i32) {
    let n = cursor_count(state);
    if n == 0 {
        state.cursor = 0;
        return;
    }
    let cur = state.cursor.min(n - 1) as i32;
    let next = (cur + delta).rem_euclid(n as i32);
    state.cursor = next as usize;
}

/// Clamp cursor into valid range without changing it otherwise — useful
/// to call once per render so a stale cursor (from a previous menu) never
/// points past the current item count.
pub fn cursor_clamp(state: &mut RpgState) {
    let n = cursor_count(state);
    if n == 0 {
        state.cursor = 0;
    } else if state.cursor >= n {
        state.cursor = n - 1;
    }
}

// ── RNG ──────────────────────────────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

pub(super) fn rng_range(state: &mut RpgState, max: u32) -> u32 {
    if max == 0 { return 0; }
    state.rng_seed = next_rng(state.rng_seed);
    ((state.rng_seed >> 33) % max as u64) as u32
}

// ── Overworld (village) ─────────────────────────────────────

/// Load the village map and switch to the overworld scene. Used at game
/// start, on dungeon retreat, and after death.
pub fn enter_overworld(state: &mut RpgState) {
    state.dungeon = Some(generate_overworld());
    state.scene = Scene::Overworld;
    state.active_event = None;
    state.cursor = 0;
    state.scene_text = vec!["辺境の村に立っている。".into()];
}

/// Build the event triggered by stepping on an overworld facility / NPC tile.
/// Mirrors `events::generate_event` but takes &state because content depends
/// on flags like `met_reception`.
pub fn generate_overworld_event(state: &RpgState, cell_type: CellType) -> Option<DungeonEvent> {
    match cell_type {
        CellType::DungeonEntrance => Some(DungeonEvent {
            description: vec![
                "ダンジョンの入口だ。深い闇が広がっている。".into(),
                "奥には魔王が潜んでいるという…".into(),
            ],
            choices: vec![
                EventChoice { label: "降りる (B1F へ)".into(), action: EventAction::EnterDungeon },
                EventChoice { label: "やめておく".into(), action: EventAction::Ignore },
            ],
        }),
        CellType::ShopTile => Some(DungeonEvent {
            description: vec![
                "武器・道具屋。店主が並べた品を勧めてくる。".into(),
            ],
            choices: vec![
                EventChoice { label: "品物を見る".into(), action: EventAction::OpenShop },
                EventChoice { label: "何も買わずに出る".into(), action: EventAction::Ignore },
            ],
        }),
        CellType::QuestBoardTile => Some(DungeonEvent {
            description: vec![
                "依頼掲示板。何枚もの依頼書が貼られている。".into(),
            ],
            choices: vec![
                EventChoice { label: "依頼を見る".into(), action: EventAction::OpenQuestBoardOverlay },
                EventChoice { label: "離れる".into(), action: EventAction::Ignore },
            ],
        }),
        CellType::InnTile => {
            let needs_rest = state.hp < state.effective_max_hp()
                || state.mp < state.max_mp
                || state.satiety < state.satiety_max;
            let label = if needs_rest {
                "泊まる (10G で全回復)"
            } else {
                "泊まる (回復は不要そうだ)"
            };
            Some(DungeonEvent {
                description: vec![
                    "宿屋。暖炉の火が穏やかに燃えている。".into(),
                ],
                choices: vec![
                    EventChoice { label: label.into(), action: EventAction::RestAtInn },
                    EventChoice { label: "出る".into(), action: EventAction::Ignore },
                ],
            })
        }
        CellType::ShrineTile => Some(DungeonEvent {
            description: vec![
                "村の祭壇。古びた石像が祀られている。".into(),
            ],
            choices: vec![
                EventChoice { label: "祈る".into(), action: EventAction::OpenShrineOverlay },
                EventChoice { label: "立ち去る".into(), action: EventAction::Ignore },
            ],
        }),
        CellType::ReceptionNpc => {
            let (desc, label) = if !state.met_reception {
                (
                    vec![
                        "受付嬢「ようこそ、冒険者さん！」".into(),
                        "「この先にあるダンジョンには魔物が棲んでいます。」".into(),
                        "「奥深くには魔王が潜んでいるとか…」".into(),
                        "「まずはこれを持って行ってください」".into(),
                    ],
                    "受け取る (薬草x3 / パンx2 / 50G)",
                )
            } else if state.game_cleared {
                (
                    vec!["受付嬢「魔王討伐おめでとうございます！」".into()],
                    "雑談する",
                )
            } else if state.max_floor_reached == 0 {
                (
                    vec!["受付嬢「ダンジョンの様子はいかがですか？」".into()],
                    "話を聞く",
                )
            } else {
                (
                    vec![format!(
                        "受付嬢「最深到達 B{}F！さらに奥を目指しましょう！」",
                        state.max_floor_reached
                    )],
                    "話を聞く",
                )
            };
            Some(DungeonEvent {
                description: desc,
                choices: vec![
                    EventChoice { label: label.into(), action: EventAction::TalkReception },
                    EventChoice { label: "離れる".into(), action: EventAction::Ignore },
                ],
            })
        }
        CellType::BlacksmithNpc => {
            let (desc, label) = if !state.met_blacksmith {
                (
                    vec![
                        "武具屋の親父「初めて見る顔だな。」".into(),
                        "「これくらいは持っていけ。安全第一だぞ」".into(),
                    ],
                    "受け取る (木の剣 / 旅人の服)",
                )
            } else {
                (
                    vec![
                        "武具屋の親父「装備が必要なら声をかけてくれ。」".into(),
                        "「店の方で売ってる。」".into(),
                    ],
                    "話を聞く",
                )
            };
            Some(DungeonEvent {
                description: desc,
                choices: vec![
                    EventChoice { label: label.into(), action: EventAction::TalkBlacksmith },
                    EventChoice { label: "離れる".into(), action: EventAction::Ignore },
                ],
            })
        }
        CellType::VillagerNpc => Some(DungeonEvent {
            description: vec![villager_flavor(state).into()],
            choices: vec![
                EventChoice { label: "うなずく".into(), action: EventAction::TalkVillager },
                EventChoice { label: "離れる".into(), action: EventAction::Ignore },
            ],
        }),
        _ => None,
    }
}

fn villager_flavor(state: &RpgState) -> &'static str {
    let bucket = state.turn_count.wrapping_add(state.rng_seed) % 6;
    match bucket {
        0 => "村人「最近、ダンジョンから戻らない冒険者が増えてる…気をつけてな」",
        1 => "村人「お腹が空いたら宿屋でゆっくり休むといい」",
        2 => "村人「掲示板の依頼を受けると、ちょっとした稼ぎになる」",
        3 => "村人「祭壇に祈ると神様が応えてくれることがある」",
        4 => "村人「武具屋の親父は腕がいい。装備はあそこで揃えな」",
        _ => "村人「魔王なんてものが本当にいるのかね…」",
    }
}

/// Apply the result of an overworld event action. Returns true on success.
/// Overworld events are short-circuited here rather than going through
/// `apply_event_outcome` because they do facility/menu transitions, not
/// stat changes.
pub fn resolve_overworld_event_choice(state: &mut RpgState, choice_index: usize) -> bool {
    let event = match &state.active_event {
        Some(e) => e.clone(),
        None => return false,
    };
    if choice_index >= event.choices.len() {
        return false;
    }
    let action = event.choices[choice_index].action.clone();

    match action {
        EventAction::Ignore | EventAction::Continue => {
            state.active_event = None;
            state.cursor = 0;
            true
        }
        EventAction::EnterDungeon => {
            state.active_event = None;
            enter_dungeon(state, 1);
            true
        }
        EventAction::OpenShop => {
            state.active_event = None;
            state.open_overlay(Overlay::Shop);
            true
        }
        EventAction::OpenQuestBoardOverlay => {
            state.active_event = None;
            state.open_overlay(Overlay::QuestBoard);
            true
        }
        EventAction::OpenShrineOverlay => {
            state.active_event = None;
            state.open_overlay(Overlay::PrayMenu);
            true
        }
        EventAction::RestAtInn => {
            if state.gold < 10 {
                state.add_log("お金が足りない (宿代10G)");
                return false;
            }
            state.gold -= 10;
            state.hp = state.effective_max_hp();
            state.mp = state.max_mp;
            state.satiety = state.satiety_max;
            state.buffs = PlayerBuffs::default();
            if let Some(p) = &mut state.pet { p.hp = p.max_hp; }
            state.add_log("宿でゆっくり休んだ。完全回復！");
            state.active_event = None;
            state.cursor = 0;
            true
        }
        EventAction::TalkReception => {
            if !state.met_reception {
                state.met_reception = true;
                state.gold += 50;
                add_item(state, ItemKind::Herb, 3);
                add_item(state, ItemKind::Bread, 2);
                state.add_log("薬草x3 / パンx2 / 50G を受け取った！");
            }
            state.active_event = None;
            state.cursor = 0;
            true
        }
        EventAction::TalkBlacksmith => {
            if !state.met_blacksmith {
                state.met_blacksmith = true;
                // Codex P2 (#98): Overworld 化でダンジョンを先に経験してから
                // 武具屋と初対面、というフローが起き得る。その時点で既に
                // 拾った武器/防具を装備していたら、初期装備で上書きすると
                // ダウングレードになるので、装備スロットが空のときだけ
                // 自動装備する。アイテム自体は inventory に必ず追加するので
                // 不要なら捨てる/置き換えることもできる。
                state.inventory.push(InventoryItem {
                    kind: ItemKind::WoodenSword, count: 1, affix: None,
                });
                let sword_idx = state.inventory.len() - 1;
                if state.weapon_idx.is_none() {
                    state.weapon_idx = Some(sword_idx);
                }
                state.inventory.push(InventoryItem {
                    kind: ItemKind::TravelClothes, count: 1, affix: None,
                });
                let armor_idx = state.inventory.len() - 1;
                if state.armor_idx.is_none() {
                    state.armor_idx = Some(armor_idx);
                }
                state.add_log("木の剣と旅人の服を受け取った！");
            }
            state.active_event = None;
            state.cursor = 0;
            true
        }
        EventAction::TalkVillager => {
            state.active_event = None;
            state.cursor = 0;
            true
        }
        _ => false,
    }
}

// ── Quests ───────────────────────────────────────────────────

/// Available quests at the town board (regenerates on visit).
pub fn available_quests(state: &RpgState) -> Vec<Quest> {
    let mut seed = state.rng_seed ^ (state.completed_quests as u64).wrapping_mul(31);
    let mut roll = |max: u32| -> u32 {
        seed = next_rng(seed);
        if max == 0 { 0 } else { ((seed >> 33) % max as u64) as u32 }
    };
    let max_floor = state.max_floor_reached.max(1);
    let mut quests = Vec::new();

    // Slay quest
    {
        let target_floor = 1 + roll(max_floor);
        let pool = super::state::floor_enemies(target_floor);
        let kind = pool[roll(pool.len() as u32) as usize];
        let count = 2 + roll(3);
        let info = enemy_info(kind);
        quests.push(Quest {
            kind: QuestKind::Slay { target: kind, count, floor: target_floor },
            reward_gold: info.gold * count + 30 * target_floor,
            reward_exp: info.exp * count / 2,
            progress: 0,
        });
    }

    // Reach quest
    {
        let target_floor = (max_floor + 1).min(MAX_FLOOR);
        quests.push(Quest {
            kind: QuestKind::Reach { floor: target_floor },
            reward_gold: 50 * target_floor,
            reward_exp: 20 * target_floor,
            progress: 0,
        });
    }

    // Collect quest
    {
        let item = if max_floor >= 4 { ItemKind::MagicWater } else { ItemKind::Herb };
        let count = 3 + roll(3);
        quests.push(Quest {
            kind: QuestKind::Collect { item, count },
            reward_gold: 40 + 10 * count,
            reward_exp: 10 * count,
            progress: 0,
        });
    }

    quests
}

pub fn accept_quest(state: &mut RpgState, idx: usize) -> bool {
    let qs = available_quests(state);
    if idx >= qs.len() { return false; }
    if state.active_quest.is_some() {
        state.add_log("既に他の依頼を受託中。完了か破棄が必要");
        return false;
    }
    let q = qs[idx].clone();
    state.add_log(&format!("依頼を受けた: {}", q.description()));
    state.active_quest = Some(q);
    state.close_overlay();
    true
}

pub fn abandon_quest(state: &mut RpgState) -> bool {
    if state.active_quest.is_none() { return false; }
    state.active_quest = None;
    state.add_log("依頼を破棄した");
    true
}

fn check_quest_complete(state: &mut RpgState) {
    let complete = state.active_quest.as_ref().map(|q| q.is_complete()).unwrap_or(false);
    if !complete { return; }
    let q = state.active_quest.take().unwrap();
    state.gold += q.reward_gold;
    state.exp += q.reward_exp;
    state.completed_quests += 1;
    state.faith = state.faith.saturating_add(2);
    state.add_log(&format!("依頼達成！ +{}G +{}EXP", q.reward_gold, q.reward_exp));
    check_level_up(state);
}

// ── Prayer ───────────────────────────────────────────────────

pub fn pray(state: &mut RpgState) -> bool {
    if state.prayed_this_run {
        state.add_log("今日はもう祈った。次の冒険まで待つ");
        return false;
    }
    state.prayed_this_run = true;
    state.faith = state.faith.saturating_add(1);
    let roll = rng_range(state, 100);
    let blessing_thresh = 40 + state.faith.min(40);
    state.close_overlay();

    if roll < 10 {
        // Curse (low chance)
        let dmg = state.max_hp / 6;
        state.hp = state.hp.saturating_sub(dmg).max(1);
        state.add_log("…神は応えなかった。心に虚しさが残る…");
    } else if roll < blessing_thresh {
        // Major blessing
        let kind = rng_range(state, 4);
        match kind {
            0 => {
                state.hp = state.max_hp;
                state.mp = state.max_mp;
                state.add_log("神の加護！ HP/MPが完全回復した！");
            }
            1 => {
                add_item(state, ItemKind::CookedMeal, 2);
                state.add_log("神の恵み！ 温かい料理x2を授かった");
            }
            2 => {
                state.gold += 100 + state.faith * 5;
                state.add_log(&format!("神の恵み！ {}Gを授かった", 100 + state.faith * 5));
            }
            _ => {
                // Random affixed weapon (level-appropriate)
                let base = pick_random_weapon(state.max_floor_reached);
                let affix = ALL_AFFIXES[rng_range(state, ALL_AFFIXES.len() as u32) as usize];
                state.inventory.push(InventoryItem {
                    kind: base, count: 1, affix: Some(affix),
                });
                let name = format!("{}{}", affix_info(affix).prefix, item_info(base).name);
                state.add_log(&format!("神の恵み！ {}を授かった", name));
            }
        }
    } else {
        // Minor blessing
        state.hp = (state.hp + state.max_hp / 4).min(state.max_hp);
        state.mp = (state.mp + state.max_mp / 4).min(state.max_mp);
        state.add_log("祈りが届いた。HP/MPが少し回復した");
    }
    true
}

fn pick_random_weapon(max_floor: u32) -> ItemKind {
    if max_floor >= 7 { ItemKind::SteelSword }
    else if max_floor >= 4 { ItemKind::IronSword }
    else { ItemKind::WoodenSword }
}

// ── Dungeon: Grid-Based Exploration ───────────────────────────

pub fn enter_dungeon(state: &mut RpgState, floor: u32) {
    let first_entry = state.max_floor_reached == 0;

    if floor == 1 {
        state.run_gold_earned = 0;
        state.run_exp_earned = 0;
        state.run_enemies_killed = 0;
        state.run_rooms_explored = 0;
        state.prayed_this_run = false;
        state.buffs = PlayerBuffs::default();
    }

    let mut map = generate_map(floor, &mut state.rng_seed);

    let px = map.player_x;
    let py = map.player_y;
    map.grid[py][px].visited = true;
    map.grid[py][px].revealed = true;
    // NOTE: do NOT set event_done = true here. The spawn cell is the
    // floor's Entrance, and we want the entrance event ("町に帰還する"
    // on B1F or "B(N-1)F へ戻る" on B2F+) to re-trigger when the player
    // walks back. `after_move` isn't called on spawn placement so there's
    // no immediate popup to suppress.

    reveal_room(&mut map, px, py);

    // Place pet next to player if possible
    if let Some(pet) = &mut state.pet {
        for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if nx >= 0 && ny >= 0 && (nx as usize) < map.width && (ny as usize) < map.height
                && map.grid[ny as usize][nx as usize].is_walkable()
                && map.monster_at(nx as usize, ny as usize).is_none()
            {
                pet.x = nx as usize;
                pet.y = ny as usize;
                break;
            }
        }
    }

    state.dungeon = Some(map);
    state.scene = Scene::DungeonExplore;
    state.active_event = None;

    if floor > state.max_floor_reached {
        state.max_floor_reached = floor;
    }

    // Quest progress: Reach
    if let Some(q) = &mut state.active_quest {
        if let QuestKind::Reach { floor: target } = q.kind {
            if floor >= target { q.progress = floor; }
        }
    }
    check_quest_complete(state);

    let theme = floor_theme(floor);
    let mut texts = floor_entry_text(floor, theme);

    if first_entry {
        texts.push(String::new());
        texts.push("※ ←↑↓→ で1歩ずつ移動".into());
        texts.push("  敵に隣接して移動方向を押すと攻撃".into());
        texts.push("  満腹度が0になるとHPが減るので食料を持参".into());
    }

    state.scene_text = texts;
    state.add_log(&format!("B{}Fに踏み込んだ…", floor));
}

fn reveal_room(map: &mut super::state::DungeonMap, x: usize, y: usize) {
    let room_id = match map.grid[y][x].room_id {
        Some(id) => id,
        None => return,
    };
    for row in &mut map.grid {
        for cell in row.iter_mut() {
            if cell.room_id == Some(room_id) {
                cell.revealed = true;
            }
        }
    }
}

fn is_adjacent_walkable(map: &super::state::DungeonMap, x: usize, y: usize, dir: Facing) -> bool {
    let nx = x as i32 + dir.dx();
    let ny = y as i32 + dir.dy();
    if !map.in_bounds(nx, ny) {
        return false;
    }
    map.cell(nx as usize, ny as usize).is_walkable()
}

/// Try to move the player one cell. If a monster is on the target,
/// attack it instead. Either way, monsters take their turn after.
pub fn try_move(state: &mut RpgState, dir: Facing) -> bool {
    let (target_action, nx, ny) = compute_move_target(state, dir);
    match target_action {
        MoveAction::Blocked => {
            state.add_log("壁だ。進めない。");
            false
        }
        MoveAction::AttackMonster(idx) => {
            attack_monster(state, idx);
            on_player_action(state);
            true
        }
        MoveAction::SwapPet => {
            // Swap positions with pet (just step onto pet's tile, pet moves to player's old)
            if let (Some(map), Some(pet)) = (&mut state.dungeon, &mut state.pet) {
                let old = (map.player_x, map.player_y);
                pet.x = old.0;
                pet.y = old.1;
                map.player_x = nx;
                map.player_y = ny;
                map.last_dir = dir;
            }
            after_move(state, nx, ny);
            on_player_action(state);
            true
        }
        MoveAction::Walk => {
            if let Some(map) = &mut state.dungeon {
                map.player_x = nx;
                map.player_y = ny;
                map.last_dir = dir;
            }
            after_move(state, nx, ny);
            on_player_action(state);
            true
        }
    }
}

enum MoveAction {
    Blocked,
    Walk,
    AttackMonster(usize),
    SwapPet,
}

fn compute_move_target(state: &RpgState, dir: Facing) -> (MoveAction, usize, usize) {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return (MoveAction::Blocked, 0, 0),
    };
    let tx = map.player_x as i32 + dir.dx();
    let ty = map.player_y as i32 + dir.dy();
    if !map.in_bounds(tx, ty) {
        return (MoveAction::Blocked, 0, 0);
    }
    let ux = tx as usize;
    let uy = ty as usize;
    if !map.cell(ux, uy).is_walkable() {
        return (MoveAction::Blocked, 0, 0);
    }
    if let Some(idx) = map.monster_at(ux, uy) {
        return (MoveAction::AttackMonster(idx), ux, uy);
    }
    if let Some(p) = &state.pet {
        if p.x == ux && p.y == uy {
            return (MoveAction::SwapPet, ux, uy);
        }
    }
    (MoveAction::Walk, ux, uy)
}

fn after_move(state: &mut RpgState, nx: usize, ny: usize) {
    let was_visited;
    let cell_type;
    let event_done;
    let is_overworld;
    {
        let map = state.dungeon.as_mut().unwrap();
        was_visited = map.grid[ny][nx].visited;
        map.grid[ny][nx].visited = true;
        map.grid[ny][nx].revealed = true;
        reveal_room(map, nx, ny);
        cell_type = map.grid[ny][nx].cell_type;
        event_done = map.grid[ny][nx].event_done;
        is_overworld = map.is_overworld;
    }

    if !is_overworld && !was_visited {
        state.run_rooms_explored += 1;
    }

    let floor = state.dungeon.as_ref().unwrap().floor_num;
    let theme = floor_theme(floor);
    let rng_val = rng_range(state, 100);
    let atmo = atmosphere_text(theme, rng_val);
    state.scene_text = vec![atmo.into()];

    if is_overworld {
        if let Some(event) = generate_overworld_event(state, cell_type) {
            state.active_event = Some(event);
            state.cursor = 0;
        }
    } else if cell_type != CellType::Corridor && !event_done {
        if let Some(event) = generate_event(cell_type, floor, theme, &mut state.rng_seed) {
            // Stay in DungeonExplore — the event is rendered as a popup
            // overlay on the same scene (see issue #89).
            state.active_event = Some(event);
            // Reset cursor so the popup starts highlighting choice 0
            // (avoids a stale index from a previously visited menu).
            state.cursor = 0;
        }
    }
}

/// Wait in place for one turn — same monster/satiety tick as moving,
/// but the player doesn't change cell. Used by the A button when there's
/// no contextual action (no event under foot, no adjacent enemy).
/// In overworld this is a no-op (no monsters to react, no satiety drain).
pub fn wait_in_place(state: &mut RpgState) -> bool {
    if state.dungeon.is_none() || state.scene != Scene::DungeonExplore {
        return false;
    }
    state.add_log("一息入れた…");
    on_player_action(state);
    true
}

/// Move in a direction with auto-walk through corridors.
pub fn move_direction(state: &mut RpgState, dir: Facing) -> bool {
    if !try_move(state, dir) {
        return false;
    }
    let mut steps = 0;
    let max_steps = 8;
    while steps < max_steps
        && matches!(state.scene, Scene::DungeonExplore | Scene::Overworld)
    {
        let next_dir = match auto_walk_direction(state) {
            Some(d) => d,
            None => break,
        };
        if !try_move(state, next_dir) {
            break;
        }
        steps += 1;
    }
    if steps > 0 {
        state.scene_text.insert(0, format!("通路を{}歩進んだ。", steps + 1));
    }
    true
}

fn auto_walk_direction(state: &RpgState) -> Option<Facing> {
    let map = state.dungeon.as_ref()?;
    // No auto-walk in the village — the player needs precise control to
    // pick which facility tile to step on.
    if map.is_overworld {
        return None;
    }
    let cell = map.player_cell();

    if cell.cell_type != CellType::Corridor && !cell.event_done { return None; }
    if cell.cell_type == CellType::Entrance || cell.cell_type == CellType::Stairs { return None; }
    if cell.tile == Tile::RoomFloor { return None; }

    // Stop if any awake monster is visible (within 4 tiles)
    let px = map.player_x as i32;
    let py = map.player_y as i32;
    if map.monsters.iter().any(|m| {
        m.hp > 0 && m.awake && {
            let dx = m.x as i32 - px;
            let dy = m.y as i32 - py;
            dx * dx + dy * dy <= 16
        }
    }) {
        return None;
    }

    let came_from = map.last_dir.reverse();
    let mut exits = Vec::new();
    for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
        if dir == came_from { continue; }
        if is_adjacent_walkable(map, map.player_x, map.player_y, dir) {
            exits.push(dir);
        }
    }
    if exits.len() == 1 { Some(exits[0]) } else { None }
}

// ── Inline Combat ────────────────────────────────────────────

/// Player attacks the monster at `monsters[idx]`.
pub fn attack_monster(state: &mut RpgState, idx: usize) {
    let player_atk = state.total_atk();
    let player_element = state.weapon_element();
    let element_dmg = state.weapon_element_dmg();
    let vamp_pct = state.weapon_vampiric_pct();

    let (kind, einfo, hp_before) = {
        let m = &state.dungeon.as_ref().unwrap().monsters[idx];
        (m.kind, enemy_info(m.kind), m.hp)
    };

    let crit_roll = rng_range(state, 100);
    let is_crit = crit_roll < 10;
    let mut base = player_atk.saturating_sub(einfo.def / 2).max(1);
    if is_crit { base = base * 3 / 2; }

    // Affix elemental bonus
    let mut bonus = 0;
    if let Some(elem) = player_element {
        bonus += element_dmg;
        if einfo.weakness == Some(elem) {
            bonus += element_dmg; // weakness doubles affix damage
        }
    }

    let damage = base + bonus;

    {
        let m = &mut state.dungeon.as_mut().unwrap().monsters[idx];
        m.hp = m.hp.saturating_sub(damage);
        m.awake = true;
    }

    let weak_str = match einfo.weakness {
        Some(e) if Some(e) == player_element => " [弱点!]",
        _ => "",
    };
    if is_crit {
        state.add_log(&format!("会心の一撃！ {}に{}ダメージ{}", einfo.name, damage, weak_str));
    } else {
        state.add_log(&format!("{}に{}ダメージ{}", einfo.name, damage, weak_str));
    }

    if vamp_pct > 0 {
        let drain = (damage * vamp_pct / 100).max(1);
        state.hp = (state.hp + drain).min(state.effective_max_hp());
        state.add_log(&format!("血を吸った。HP+{}", drain));
    }

    let died = state.dungeon.as_ref().unwrap().monsters[idx].hp == 0;
    if died {
        on_monster_killed(state, idx, kind, hp_before);
    }
}

fn on_monster_killed(state: &mut RpgState, _idx: usize, kind: EnemyKind, _hp_before: u32) {
    let info = enemy_info(kind);
    state.exp += info.exp;
    state.gold += info.gold;
    state.run_gold_earned += info.gold;
    state.run_exp_earned += info.exp;
    state.run_enemies_killed += 1;
    state.add_log(&format!("{}を倒した！ EXP+{} +{}G", info.name, info.exp, info.gold));

    // Drop
    if let Some((drop_item, pct)) = info.drop {
        if rng_range(state, 100) < pct {
            // Issue #92 (balance): mid-game (B4-7) had near-zero affix
            // drops, leading to a flatline in player power. Bumped the
            // mid-tier kills' affix chance so the difficulty curve has
            // matching gear progression.
            let affixed_chance = match kind {
                EnemyKind::Skeleton | EnemyKind::Golem | EnemyKind::DarkKnight => 35,
                EnemyKind::Demon | EnemyKind::Dragon => 55,
                _ => 8,
            };
            add_item(state, drop_item, 1);
            state.add_log(&format!("{}をドロップ！", item_info(drop_item).name));
            // Bonus affixed equipment chance
            if rng_range(state, 100) < affixed_chance {
                drop_random_affix_equipment(state, kind);
            }
        }
    }

    // Quest progress: Slay
    let floor = state.dungeon.as_ref().unwrap().floor_num;
    if let Some(q) = &mut state.active_quest {
        if let QuestKind::Slay { target, floor: tf, .. } = q.kind {
            if target == kind && tf == floor {
                q.progress += 1;
            }
        }
    }
    check_quest_complete(state);

    check_level_up(state);

    // Game clear (Demon Lord)
    if kind == EnemyKind::DemonLord {
        state.game_cleared = true;
        state.total_clears += 1;
        state.faith = state.faith.saturating_add(20);
        state.scene = Scene::GameClear;
    }
}

fn drop_random_affix_equipment(state: &mut RpgState, killer: EnemyKind) {
    // Pick weapon or armor based on the kind
    let max_floor = state.max_floor_reached.max(1);
    let is_weapon = rng_range(state, 2) == 0;
    let kind = if is_weapon {
        pick_random_weapon(max_floor)
    } else if max_floor >= 7 {
        ItemKind::ChainMail
    } else if max_floor >= 4 {
        ItemKind::LeatherArmor
    } else {
        ItemKind::TravelClothes
    };
    let affix_idx = rng_range(state, ALL_AFFIXES.len() as u32) as usize;
    let affix = ALL_AFFIXES[affix_idx];
    state.inventory.push(InventoryItem {
        kind, count: 1, affix: Some(affix),
    });
    let name = format!("{}{}", affix_info(affix).prefix, item_info(kind).name);
    state.add_log(&format!("{}が{}を落とした！", enemy_info(killer).name, name));
}

/// Called after every player action. Triggers monster turn, satiety,
/// buff tick, pet turn, and death checks. Skipped entirely in the village.
pub fn on_player_action(state: &mut RpgState) {
    if state.dungeon.is_none() || state.scene == Scene::GameClear {
        return;
    }
    // Overworld has no monsters or hunger, so player actions don't tick time.
    if state.dungeon.as_ref().map(|m| m.is_overworld).unwrap_or(false) {
        return;
    }
    state.turn_count = state.turn_count.wrapping_add(1);

    // Buffs tick
    state.buffs.tick_down();

    // Satiety
    tick_satiety(state);
    if state.hp == 0 { return; }

    // Wake monsters
    wake_monsters(state);

    // Monster turns
    monster_turn(state);

    // Pet turn
    pet_turn(state);

    // Cleanup dead monsters
    if let Some(map) = &mut state.dungeon {
        map.monsters.retain(|m| m.hp > 0);
    }

    if state.hp == 0 {
        process_dungeon_death(state);
    }
}

fn tick_satiety(state: &mut RpgState) {
    // Issue #92 (balance): satiety drains every other turn instead of every
    // turn. Pre-tuning the simulator showed >50% of "in-dungeon" deaths were
    // actually starvation cascade (HP drain → bad combat). Halving the drain
    // rate keeps food meaningful (still depletes mid-floor) without making
    // it the dominant failure mode.
    if state.satiety > 0 {
        if state.turn_count.is_multiple_of(2) {
            state.satiety -= 1;
        }
        // Hunger thresholds
        if state.satiety == state.satiety_max / 4 {
            state.add_log("お腹が空いてきた…");
        }
        if state.satiety == 50 {
            state.add_log("飢餓寸前！何か食べないと…");
        }
    } else {
        // Starving — drain HP each turn
        let drain = (state.max_hp / 30).max(1);
        state.hp = state.hp.saturating_sub(drain);
        if state.turn_count.is_multiple_of(5) {
            state.add_log(&format!("飢えで体力が削れる… -{}HP", drain));
        }
    }
}

/// Wake monsters that share the player's room or are within 3 tiles
/// (corridor pursuit). Wakes all monsters in the same room when player
/// enters; in corridors, radius-3 awareness.
fn wake_monsters(state: &mut RpgState) {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return,
    };
    let player_room_id = map.player_cell().room_id;
    let px = map.player_x as i32;
    let py = map.player_y as i32;

    let to_wake: Vec<usize> = map
        .monsters
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if m.awake || m.hp == 0 { return None; }
            let m_room = map.grid[m.y][m.x].room_id;
            let same_room = match (player_room_id, m_room) {
                (Some(p), Some(q)) => p == q,
                _ => false,
            };
            let dx = m.x as i32 - px;
            let dy = m.y as i32 - py;
            let close = dx * dx + dy * dy <= 9;
            if same_room || close { Some(i) } else { None }
        })
        .collect();

    if !to_wake.is_empty() {
        let map = state.dungeon.as_mut().unwrap();
        for i in to_wake { map.monsters[i].awake = true; }
    }
}

/// All awake monsters take a turn: attack player if adjacent, otherwise
/// step toward player (greedy chase).
fn monster_turn(state: &mut RpgState) {
    let count = state.dungeon.as_ref().map(|d| d.monsters.len()).unwrap_or(0);
    for i in 0..count {
        if state.hp == 0 { break; }
        monster_act(state, i);
    }
}

fn monster_act(state: &mut RpgState, idx: usize) {
    let (mx, my, kind, charging, can_charge, awake, hp) = {
        let m = match state.dungeon.as_ref().and_then(|d| d.monsters.get(idx)) {
            Some(m) => m,
            None => return,
        };
        (m.x, m.y, m.kind, m.charging, enemy_info(m.kind).can_charge, m.awake, m.hp)
    };
    if hp == 0 || !awake { return; }
    let einfo = enemy_info(kind);

    let (px, py) = {
        let m = state.dungeon.as_ref().unwrap();
        (m.player_x as i32, m.player_y as i32)
    };

    let dx = px - mx as i32;
    let dy = py - my as i32;
    let adjacent_to_player = dx.abs() + dy.abs() == 1;

    if charging {
        // Release charged attack
        if adjacent_to_player {
            let damage = (einfo.atk * 2).saturating_sub(state.total_def() / 2).max(1);
            state.hp = state.hp.saturating_sub(damage);
            state.add_log(&format!("{}の渾身の一撃！ {}ダメージ！", einfo.name, damage));
        } else {
            state.add_log(&format!("{}の渾身の一撃は空振り…", einfo.name));
        }
        state.dungeon.as_mut().unwrap().monsters[idx].charging = false;
        return;
    }

    if adjacent_to_player {
        // Maybe charge
        if can_charge && rng_range(state, 100) < 25 {
            state.dungeon.as_mut().unwrap().monsters[idx].charging = true;
            state.add_log(&format!("{}は力を溜めている！", einfo.name));
            return;
        }
        // Normal attack
        let damage = einfo.atk.saturating_sub(state.total_def() / 2).max(1);
        state.hp = state.hp.saturating_sub(damage);
        state.add_log(&format!("{}の攻撃！ {}ダメージ！", einfo.name, damage));
        return;
    }

    // Step toward player (or pet if closer)
    let target = best_target(state, mx, my);
    let step = step_toward(state, mx, my, target);
    if let Some((nx, ny)) = step {
        let map = state.dungeon.as_mut().unwrap();
        map.monsters[idx].x = nx;
        map.monsters[idx].y = ny;
    }
}

/// Pick the monster's target — prefer player, fall back to pet if much closer.
fn best_target(state: &RpgState, mx: usize, my: usize) -> (i32, i32) {
    let map = state.dungeon.as_ref().unwrap();
    let to_player = (
        map.player_x as i32 - mx as i32,
        map.player_y as i32 - my as i32,
    );
    let dp_sq = to_player.0 * to_player.0 + to_player.1 * to_player.1;

    if let Some(pet) = &state.pet {
        let to_pet = (pet.x as i32 - mx as i32, pet.y as i32 - my as i32);
        let dpet_sq = to_pet.0 * to_pet.0 + to_pet.1 * to_pet.1;
        if dpet_sq + 4 < dp_sq {
            return (pet.x as i32, pet.y as i32);
        }
    }
    (map.player_x as i32, map.player_y as i32)
}

/// Greedy chase: pick the cardinal step that reduces Manhattan distance
/// most, preferring axis with greater delta. Avoids walls and other entities.
fn step_toward(state: &RpgState, mx: usize, my: usize, target: (i32, i32)) -> Option<(usize, usize)> {
    let map = state.dungeon.as_ref()?;
    let dx = target.0 - mx as i32;
    let dy = target.1 - my as i32;

    let mut tries: Vec<Facing> = Vec::new();
    if dx.abs() > dy.abs() {
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
    } else {
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
    }

    for d in tries {
        let nx = mx as i32 + d.dx();
        let ny = my as i32 + d.dy();
        if !map.in_bounds(nx, ny) { continue; }
        let ux = nx as usize;
        let uy = ny as usize;
        if !map.cell(ux, uy).is_walkable() { continue; }
        if (ux, uy) == (map.player_x, map.player_y) { continue; }
        if map.monsters.iter().any(|m| m.hp > 0 && m.x == ux && m.y == uy) { continue; }
        if let Some(p) = &state.pet { if (ux, uy) == (p.x, p.y) { continue; } }
        return Some((ux, uy));
    }
    None
}

// ── Pet ──────────────────────────────────────────────────────

fn pet_turn(state: &mut RpgState) {
    let pet = match &state.pet {
        Some(p) => p.clone(),
        None => return,
    };
    let map = match &state.dungeon {
        Some(m) => m,
        None => return,
    };

    // Find nearest awake monster
    let nearest = map
        .monsters
        .iter()
        .enumerate()
        .filter(|(_, m)| m.hp > 0 && m.awake)
        .min_by_key(|(_, m)| {
            let dx = m.x as i32 - pet.x as i32;
            let dy = m.y as i32 - pet.y as i32;
            dx * dx + dy * dy
        });

    if let Some((idx, m)) = nearest {
        let dx = m.x as i32 - pet.x as i32;
        let dy = m.y as i32 - pet.y as i32;
        let adj = dx.abs() + dy.abs() == 1;
        if adj {
            // Pet attacks
            let info = enemy_info(pet.kind);
            let pet_atk = info.atk + pet.level * 2;
            let target_def = enemy_info(m.kind).def;
            let dmg = pet_atk.saturating_sub(target_def / 2).max(1);
            let target_name = enemy_info(m.kind).name;
            state.add_log(&format!("{}が{}に{}ダメージ！", pet.name, target_name, dmg));
            let map = state.dungeon.as_mut().unwrap();
            map.monsters[idx].hp = map.monsters[idx].hp.saturating_sub(dmg);
            if map.monsters[idx].hp == 0 {
                let killed_kind = map.monsters[idx].kind;
                let pre = map.monsters[idx].max_hp;
                state.add_log(&format!("{}が{}を倒した！", pet.name, target_name));
                on_monster_killed(state, idx, killed_kind, pre);
            }
            return;
        }
        // Step toward monster
        let step = pet_step_toward(state, &pet, (m.x as i32, m.y as i32));
        if let Some((nx, ny)) = step {
            if let Some(p) = &mut state.pet { p.x = nx; p.y = ny; }
            return;
        }
    }

    // No nearby target — follow player
    let target = (map.player_x as i32, map.player_y as i32);
    let step = pet_step_toward(state, &pet, target);
    if let Some((nx, ny)) = step {
        // Don't step adjacent if already adjacent (idle next to player)
        let dx = (target.0 - nx as i32).abs();
        let dy = (target.1 - ny as i32).abs();
        if dx + dy >= 1 {
            if let Some(p) = &mut state.pet { p.x = nx; p.y = ny; }
        }
    }
}

fn pet_step_toward(state: &RpgState, pet: &Pet, target: (i32, i32)) -> Option<(usize, usize)> {
    let map = state.dungeon.as_ref()?;
    let dx = target.0 - pet.x as i32;
    let dy = target.1 - pet.y as i32;

    let mut tries: Vec<Facing> = Vec::new();
    if dx.abs() > dy.abs() {
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
    } else {
        tries.push(if dy > 0 { Facing::South } else { Facing::North });
        tries.push(if dx > 0 { Facing::East } else { Facing::West });
    }
    for d in tries {
        let nx = pet.x as i32 + d.dx();
        let ny = pet.y as i32 + d.dy();
        if !map.in_bounds(nx, ny) { continue; }
        let ux = nx as usize; let uy = ny as usize;
        if !map.cell(ux, uy).is_walkable() { continue; }
        if (ux, uy) == (map.player_x, map.player_y) { continue; }
        if map.monsters.iter().any(|m| m.hp > 0 && m.x == ux && m.y == uy) { continue; }
        return Some((ux, uy));
    }
    None
}

/// Try to tame an adjacent monster by feeding it a Pet Treat.
pub fn tame_with_treat(state: &mut RpgState, treat_inv_idx: usize) -> bool {
    if state.pet.is_some() {
        state.add_log("既にペットがいる");
        return false;
    }
    if treat_inv_idx >= state.inventory.len() { return false; }
    if state.inventory[treat_inv_idx].kind != ItemKind::PetTreat { return false; }

    // Find adjacent tameable monster
    let map = match &state.dungeon {
        Some(m) => m,
        None => {
            state.add_log("ダンジョン内でしか使えない");
            return false;
        }
    };
    let px = map.player_x as i32;
    let py = map.player_y as i32;
    let candidate = map
        .monsters
        .iter()
        .position(|m| {
            m.hp > 0
                && enemy_info(m.kind).tameable
                && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1
        });

    let idx = match candidate {
        Some(i) => i,
        None => {
            state.add_log("隣接したテイム可能な魔物がいない");
            return false;
        }
    };

    // Tame chance: based on monster HP ratio (lower = easier)
    let m = &state.dungeon.as_ref().unwrap().monsters[idx];
    let hp_ratio = m.hp * 100 / m.max_hp;
    let chance = 80u32.saturating_sub(hp_ratio / 2);

    consume_inventory_slot(state, treat_inv_idx);

    let kind;
    let mx;
    let my;
    let max_hp;
    {
        let m = &state.dungeon.as_ref().unwrap().monsters[idx];
        kind = m.kind;
        mx = m.x;
        my = m.y;
        max_hp = m.max_hp;
    }

    if rng_range(state, 100) < chance {
        // Success
        let info = enemy_info(kind);
        state.pet = Some(Pet {
            kind,
            name: info.name.to_string(),
            x: mx,
            y: my,
            hp: max_hp,
            max_hp,
            level: 1,
        });
        state.dungeon.as_mut().unwrap().monsters.remove(idx);
        state.add_log(&format!("{}が懐いた！仲間になった！", info.name));
    } else {
        let info = enemy_info(kind);
        state.add_log(&format!("{}は餌を食べたがそっぽを向いた…", info.name));
        // Wake the monster (now hostile)
        state.dungeon.as_mut().unwrap().monsters[idx].awake = true;
    }
    on_player_action(state);
    true
}

// ── Dungeon Events ───────────────────────────────────────────

pub fn resolve_event_choice(state: &mut RpgState, choice_index: usize) -> bool {
    // Overworld events use a separate, simpler dispatch (no stat outcomes,
    // just facility/menu transitions).
    if state.scene == Scene::Overworld
        || state.dungeon.as_ref().map(|m| m.is_overworld).unwrap_or(false)
    {
        return resolve_overworld_event_choice(state, choice_index);
    }

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
    let floor = state.dungeon.as_ref().map(|m| m.floor_num).unwrap_or(1);

    let outcome = resolve_event(action, cell_type, floor, state.level, &mut state.rng_seed);

    let succeeded = apply_event_outcome(state, &outcome);

    // Codex P1 (#95): if the outcome bailed out (insufficient gold for a
    // purchase, or the offering item wasn't held), don't mark the event tile
    // as resolved and don't close the popup — the player should be able to
    // pick a different choice or walk away. Otherwise we'd "consume" the
    // peddler / idol after a failed attempt and leave contradictory state.
    if !succeeded {
        return false;
    }

    // Stairs / Entrance are persistent landmarks, not consumable events:
    // the player must be able to come back later to descend or return to
    // town. Marking them done would silently break the "もう一度登る" /
    // "やっぱり帰る" flow because `after_move` skips events on done tiles.
    let consumable = !matches!(cell_type, CellType::Stairs | CellType::Entrance);
    if consumable {
        if let Some(map) = &mut state.dungeon {
            map.grid[map.player_y][map.player_x].event_done = true;
        }
    }
    state.active_event = None;

    if outcome.descend {
        let next_floor = floor + 1;
        enter_dungeon(state, next_floor);
    } else if outcome.ascend {
        // B2F+ 入口階段から前のフロアへ。floor==1 から ascend を出すことは
        // entrance_event 側でガード済みだが、安全側に倒して saturating_sub。
        let prev_floor = floor.saturating_sub(1).max(1);
        enter_dungeon(state, prev_floor);
    } else if outcome.return_to_town {
        retreat_to_town(state);
    } else if state.dungeon.is_some() {
        for desc in &outcome.description {
            if !desc.is_empty() { state.add_log(desc); }
        }
        state.scene_text = outcome.description;
        // Scene remains DungeonExplore — event popup auto-closes
        // because active_event is now None (cleared above).
    }
    true
}

/// Apply the resolved outcome to player state.
///
/// Returns `true` on success, `false` when a precondition (gold cost,
/// require_consume) wasn't met. Callers must check the return value to
/// avoid showing "you bought it" text after a failed purchase (Codex P1
/// review on PR #95).
fn apply_event_outcome(state: &mut RpgState, outcome: &EventOutcome) -> bool {
    // Validate require_consume + gold cost up-front, before mutating
    // anything. This way a failed precondition leaves state untouched and
    // the event tile remains usable.
    let consume_idx = if let Some(needed) = outcome.require_consume {
        let idx = state
            .inventory
            .iter()
            .position(|i| i.kind == needed && i.affix.is_none());
        match idx {
            Some(i) => Some(i),
            None => {
                state.add_log("供える物がない…");
                return false;
            }
        }
    } else {
        None
    };
    if outcome.gold < 0 {
        let cost = (-outcome.gold) as u32;
        if state.gold < cost {
            state.add_log("お金が足りない…");
            return false;
        }
    }

    // Preconditions OK — mutate.
    if let Some(i) = consume_idx {
        consume_inventory_slot(state, i);
    }
    if outcome.gold > 0 {
        state.gold += outcome.gold as u32;
        state.run_gold_earned += outcome.gold as u32;
    } else if outcome.gold < 0 {
        state.gold -= (-outcome.gold) as u32;
    }
    if outcome.hp_change == 9999 {
        let heal = state.max_hp / 4;
        state.hp = (state.hp + heal).min(state.max_hp);
    } else if outcome.hp_change < 0 {
        let damage = (-outcome.hp_change) as u32;
        state.hp = state.hp.saturating_sub(damage);
    } else if outcome.hp_change > 0 {
        state.hp = (state.hp + outcome.hp_change as u32).min(state.max_hp);
    }
    if outcome.mp_change == 9999 {
        let heal = state.max_mp / 4;
        state.mp = (state.mp + heal).min(state.max_mp);
    } else if outcome.mp_change > 0 {
        state.mp = (state.mp + outcome.mp_change as u32).min(state.max_mp);
    }
    if let Some((item_kind, count)) = outcome.item {
        add_item(state, item_kind, count);
        // Quest progress: collect
        if let Some(q) = &mut state.active_quest {
            if let QuestKind::Collect { item, .. } = q.kind {
                if item == item_kind { q.progress += count; }
            }
        }
        check_quest_complete(state);
    }
    if let Some(lore_id) = outcome.lore_id {
        if !state.lore_found.contains(&lore_id) {
            state.lore_found.push(lore_id);
        }
    }
    if outcome.satiety_change != 0 {
        if outcome.satiety_change > 0 {
            state.satiety = (state.satiety + outcome.satiety_change as u32).min(state.satiety_max);
        } else {
            state.satiety = state.satiety.saturating_sub((-outcome.satiety_change) as u32);
        }
    }
    if outcome.faith_change > 0 {
        state.faith = state.faith.saturating_add(outcome.faith_change);
    }
    if let Some(kind) = outcome.spawn_pet {
        if state.pet.is_none() {
            let info = enemy_info(kind);
            // Place pet on the player's tile (they'll swap on next move).
            let (px, py) = state
                .dungeon
                .as_ref()
                .map(|m| (m.player_x, m.player_y))
                .unwrap_or((0, 0));
            state.pet = Some(Pet {
                kind,
                name: info.name.to_string(),
                x: px,
                y: py,
                hp: info.max_hp,
                max_hp: info.max_hp,
                level: 1,
            });
        } else {
            state.add_log("既にペットがいるので卵は連れて行けない");
        }
    }
    if let Some(kind) = outcome.spawn_hostile {
        spawn_hostile_near_player(state, kind);
    }
    if state.hp == 0 {
        process_dungeon_death(state);
    }
    true
}

fn spawn_hostile_near_player(state: &mut RpgState, kind: EnemyKind) {
    let (px, py) = match &state.dungeon {
        Some(m) => (m.player_x, m.player_y),
        None => return,
    };
    let info = enemy_info(kind);
    // Codex P2 (#95): only spawn when a free adjacent tile exists. Falling
    // back to the player's tile created same-cell overlap which the move /
    // attack code can't reason about (combat targets neighbors, not own
    // cell), leading to an unattackable monster on top of the player.
    let dirs = [Facing::North, Facing::East, Facing::South, Facing::West];
    let map = state.dungeon.as_ref().unwrap();
    let mut spot: Option<(usize, usize)> = None;
    for d in &dirs {
        let nx = px as i32 + d.dx();
        let ny = py as i32 + d.dy();
        if !map.in_bounds(nx, ny) { continue; }
        let ux = nx as usize; let uy = ny as usize;
        if !map.cell(ux, uy).is_walkable() { continue; }
        if map.monsters.iter().any(|m| m.hp > 0 && m.x == ux && m.y == uy) {
            continue;
        }
        spot = Some((ux, uy));
        break;
    }
    let Some((sx, sy)) = spot else {
        // No free adjacent tile — skip the spawn rather than overlap the
        // player. The flavor text already played; just log the near-miss.
        state.add_log("…が、すぐには現れなかった。");
        return;
    };
    let map = state.dungeon.as_mut().unwrap();
    map.monsters.push(Monster {
        kind,
        x: sx,
        y: sy,
        hp: info.max_hp,
        max_hp: info.max_hp,
        awake: true,
        charging: false,
    });
}

pub fn retreat_to_town(state: &mut RpgState) {
    let run_gold = state.run_gold_earned;
    let run_exp = state.run_exp_earned;
    let run_kills = state.run_enemies_killed;
    let rooms = state.run_rooms_explored;
    let floor = state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(1);

    let bonus = return_bonus(floor, rooms);
    if bonus > 0 { state.gold += bonus; }

    state.faith = state.faith.saturating_add(1);

    if run_kills > 0 || run_gold > 0 {
        if bonus > 0 {
            state.add_log(&format!(
                "帰還！ {}G/{}EXP/{}体撃破 帰還ボーナス+{}G",
                run_gold, run_exp, run_kills, bonus
            ));
        } else {
            state.add_log(&format!("帰還！ 獲得: {}G / {}EXP / {}体撃破", run_gold, run_exp, run_kills));
        }
    } else {
        state.add_log("村に戻った。");
    }
    enter_overworld(state);
}

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
    state.satiety = state.satiety_max / 2;
    state.add_log(&format!("力尽きた… {}G失った", lost_gold));
    enter_overworld(state);
}

// ── Skills (inline) ──────────────────────────────────────────

/// Use a skill from the skill menu. Returns true if a turn was consumed.
pub fn use_skill(state: &mut RpgState, skill_index: usize) -> bool {
    let skills = available_skills(state.level);
    if skill_index >= skills.len() { return false; }
    let skill = skills[skill_index];
    let info = skill_info(skill);

    if state.mp < info.mp_cost {
        state.add_log("MPが足りない！");
        return false;
    }

    // Find adjacent enemy for damage skills
    let adj_enemy = adjacent_monster(state);

    match skill {
        SkillKind::Fire | SkillKind::IceBlade | SkillKind::Thunder | SkillKind::Drain => {
            let idx = match adj_enemy {
                Some(i) => i,
                None => {
                    state.add_log("隣接した敵がいない");
                    return false;
                }
            };
            state.mp -= info.mp_cost;
            cast_damage_skill(state, skill, idx);
        }
        SkillKind::Heal => {
            state.mp -= info.mp_cost;
            let heal = state.total_mag() * info.value;
            state.hp = (state.hp + heal).min(state.effective_max_hp());
            state.add_log(&format!("ヒール！ HP+{}", heal));
        }
        SkillKind::Shield => {
            state.mp -= info.mp_cost;
            state.buffs.shield_value = info.value;
            state.buffs.shield_turns = 5;
            state.add_log(&format!("シールド！ DEF+{} (5T)", info.value));
        }
        SkillKind::Berserk => {
            state.mp -= info.mp_cost;
            state.buffs.berserk_atk = info.value;
            state.buffs.berserk_turns = 5;
            state.add_log(&format!("バーサク！ ATK+{} DEF-5 (5T)", info.value));
        }
    }

    state.close_overlay();
    on_player_action(state);
    true
}

fn cast_damage_skill(state: &mut RpgState, skill: SkillKind, idx: usize) {
    let info = skill_info(skill);
    let mag = state.total_mag();
    let player_atk = state.total_atk();
    let elem = skill_element(skill);
    let (kind, einfo) = {
        let m = &state.dungeon.as_ref().unwrap().monsters[idx];
        (m.kind, enemy_info(m.kind))
    };

    let mut damage = match skill {
        SkillKind::Fire => (mag * info.value).saturating_sub(einfo.def / 3).max(1),
        SkillKind::IceBlade => (player_atk / 2 + mag * info.value).saturating_sub(einfo.def / 3).max(1),
        SkillKind::Thunder => (mag * info.value).saturating_sub(einfo.def / 4).max(1),
        SkillKind::Drain => (mag * info.value).saturating_sub(einfo.def / 3).max(1),
        _ => 0,
    };
    let is_weak = elem.is_some() && einfo.weakness == elem;
    if is_weak { damage = damage * 3 / 2; }

    {
        let m = &mut state.dungeon.as_mut().unwrap().monsters[idx];
        m.hp = m.hp.saturating_sub(damage);
        m.awake = true;
    }

    let weak_str = if is_weak { " [弱点!]" } else { "" };
    let name = einfo.name;
    state.add_log(&format!("{}！ {}に{}ダメージ{}", info.name, name, damage, weak_str));

    if matches!(skill, SkillKind::Drain) {
        let drain = damage / 2;
        state.hp = (state.hp + drain).min(state.effective_max_hp());
        state.add_log(&format!("HPを{}吸収", drain));
    }

    if state.dungeon.as_ref().unwrap().monsters[idx].hp == 0 {
        on_monster_killed(state, idx, kind, 0);
    }
}

fn adjacent_monster(state: &RpgState) -> Option<usize> {
    let map = state.dungeon.as_ref()?;
    let px = map.player_x as i32;
    let py = map.player_y as i32;
    map.monsters
        .iter()
        .position(|m| {
            m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1
        })
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
            // Pet levels up too
            let pet_msg = state.pet.as_mut().map(|p| {
                p.level += 1;
                p.max_hp += 8;
                p.hp = p.max_hp;
                format!("{}も成長した！", p.name)
            });
            if let Some(msg) = pet_msg { state.add_log(&msg); }
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
    // Stackable items merge with existing non-affixed entry
    if let Some(entry) = state.inventory.iter_mut().find(|i| i.kind == kind && i.affix.is_none()) {
        entry.count += count;
    } else {
        state.inventory.push(InventoryItem { kind, count, affix: None });
    }
}

/// Remove one count from inventory slot, deleting the entry if empty.
/// Adjusts equipped indices to remain valid.
fn consume_inventory_slot(state: &mut RpgState, idx: usize) {
    if idx >= state.inventory.len() { return; }
    state.inventory[idx].count -= 1;
    if state.inventory[idx].count == 0 {
        state.inventory.remove(idx);
        // Re-anchor equipped indices
        if let Some(w) = state.weapon_idx {
            state.weapon_idx = if w == idx { None }
                else if w > idx { Some(w - 1) } else { Some(w) };
        }
        if let Some(a) = state.armor_idx {
            state.armor_idx = if a == idx { None }
                else if a > idx { Some(a - 1) } else { Some(a) };
        }
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
                if state.hp >= state.effective_max_hp() {
                    state.add_log("HPは満タン");
                    return false;
                }
                state.hp = (state.hp + iinfo.value).min(state.effective_max_hp());
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
                state.buffs.potion_atk = iinfo.value;
                state.buffs.potion_turns = 8;
                state.add_log(&format!("力の薬！ ATK+{} (8T)", iinfo.value));
            }
            ItemKind::PetTreat => {
                return tame_with_treat(state, inv_index);
            }
            ItemKind::ReturnScroll => {
                // Only useful inside the dungeon — refuse in the village.
                let in_dungeon = state
                    .dungeon
                    .as_ref()
                    .map(|m| !m.is_overworld)
                    .unwrap_or(false);
                if !in_dungeon {
                    state.add_log("ここでは使えない");
                    return false;
                }
                consume_inventory_slot(state, inv_index);
                state.add_log("帰還の巻物を破った！ 村へ戻る…");
                retreat_to_town(state);
                return true;
            }
            _ => {
                state.add_log("使えないアイテム");
                return false;
            }
        },
        ItemCategory::Food => {
            if state.satiety >= state.satiety_max {
                state.add_log("満腹で食べられない");
                return false;
            }
            state.satiety = (state.satiety + iinfo.value).min(state.satiety_max);
            if matches!(kind, ItemKind::CookedMeal) {
                state.hp = (state.hp + 20).min(state.effective_max_hp());
            }
            state.add_log(&format!("{}を食べた。満腹度+{}", iinfo.name, iinfo.value));
        }
        ItemCategory::Weapon => {
            // Equip
            state.weapon_idx = Some(inv_index);
            let display = state.inventory[inv_index].display_name();
            state.add_log(&format!("{}を装備した", display));
            return true; // do not consume
        }
        ItemCategory::Armor => {
            state.armor_idx = Some(inv_index);
            let display = state.inventory[inv_index].display_name();
            state.add_log(&format!("{}を装備した", display));
            return true;
        }
    }

    // Consume one
    consume_inventory_slot(state, inv_index);
    if state.dungeon.is_some() {
        on_player_action(state);
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

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::{Affix, Monster};

    #[test]
    fn npc_first_meet_grants_starter_kit() {
        let mut s = RpgState::new();
        // Receive both NPC starter packages.
        s.active_event = generate_overworld_event(&s, CellType::ReceptionNpc);
        assert!(resolve_event_choice(&mut s, 0));
        s.active_event = generate_overworld_event(&s, CellType::BlacksmithNpc);
        assert!(resolve_event_choice(&mut s, 0));
        assert!(s.weapon().is_some());
        assert!(s.armor().is_some());
        assert_eq!(s.gold, 50);
        assert!(s.met_reception);
        assert!(s.met_blacksmith);
    }

    /// Codex P2 (#98): もしプレイヤーがダンジョンを先に経験して
    /// 既に強い装備を着けている状態で武具屋と初対面した場合、
    /// 木の剣 / 旅人の服 で装備を上書きしてはいけない。
    /// アイテムは inventory に追加されるが、装備スロットは維持される。
    #[test]
    fn blacksmith_first_meet_does_not_downgrade_existing_gear() {
        let mut s = RpgState::new();
        // Pre-equip something stronger (e.g. iron sword + leather armor).
        s.inventory.push(InventoryItem {
            kind: ItemKind::IronSword, count: 1, affix: None,
        });
        s.weapon_idx = Some(0);
        s.inventory.push(InventoryItem {
            kind: ItemKind::LeatherArmor, count: 1, affix: None,
        });
        s.armor_idx = Some(1);
        let atk_before = s.total_atk();
        let def_before = s.total_def();

        s.active_event = generate_overworld_event(&s, CellType::BlacksmithNpc);
        assert!(resolve_event_choice(&mut s, 0));

        // Equipped slots must still point at the strong gear.
        assert_eq!(s.weapon_idx, Some(0));
        assert_eq!(s.armor_idx, Some(1));
        assert_eq!(s.total_atk(), atk_before, "ATK must not regress");
        assert_eq!(s.total_def(), def_before, "DEF must not regress");
        // Starter items are still added to inventory (player can drop / equip later).
        assert!(s.inventory.iter().any(|i| i.kind == ItemKind::WoodenSword));
        assert!(s.inventory.iter().any(|i| i.kind == ItemKind::TravelClothes));
    }

    #[test]
    fn return_scroll_warps_to_overworld_from_dungeon() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 3);
        add_item(&mut s, ItemKind::ReturnScroll, 1);
        let idx = s.inventory.iter().position(|i| i.kind == ItemKind::ReturnScroll).unwrap();
        assert_eq!(s.scene, Scene::DungeonExplore);
        assert!(use_item(&mut s, idx));
        assert_eq!(s.scene, Scene::Overworld);
        assert!(!s.inventory.iter().any(|i| i.kind == ItemKind::ReturnScroll));
    }

    #[test]
    fn return_scroll_refuses_in_overworld() {
        let mut s = RpgState::new();
        // Already in Overworld at construction.
        add_item(&mut s, ItemKind::ReturnScroll, 1);
        let idx = s.inventory.iter().position(|i| i.kind == ItemKind::ReturnScroll).unwrap();
        assert!(!use_item(&mut s, idx));
        assert!(s.inventory.iter().any(|i| i.kind == ItemKind::ReturnScroll));
    }

    /// 階段や入口は永続ランドマーク。"探索を続ける" を選んでも消費されず、
    /// もう一度踏めば再度ダイアログが出る必要がある。
    /// 旧実装では `event_done = true` がセットされて二度と発火しなくなり、
    /// 帰宅も降下もできなくなっていた。
    #[test]
    fn stairs_re_trigger_after_continue() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        // Find a stairs cell on the generated map.
        let map = s.dungeon.as_mut().unwrap();
        let mut stairs = None;
        for y in 0..map.height {
            for x in 0..map.width {
                if map.grid[y][x].cell_type == CellType::Stairs {
                    stairs = Some((x, y));
                    break;
                }
            }
            if stairs.is_some() { break; }
        }
        let (sx, sy) = stairs.expect("map should have stairs");

        // Find a walkable neighbor; `step_on` is the direction from that
        // neighbor onto the stairs (so reverse() steps off).
        let mut neighbor = None;
        for dir in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let nx = sx as i32 - dir.dx();
            let ny = sy as i32 - dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            if map.grid[ny as usize][nx as usize].is_walkable() {
                neighbor = Some((nx as usize, ny as usize, dir));
                break;
            }
        }
        let (fx, fy, step_on) = neighbor.expect("stairs has a walkable neighbor");
        map.player_x = fx;
        map.player_y = fy;
        map.monsters.clear();

        // Step onto stairs → stairs event fires.
        try_move(&mut s, step_on);
        assert!(s.active_event.is_some(), "stairs should fire event on first step");
        let n_choices = s.active_event.as_ref().unwrap().choices.len();
        // Continue (last choice) — don't descend.
        assert!(resolve_event_choice(&mut s, n_choices - 1));
        assert!(s.active_event.is_none());
        // Cell must NOT be marked done.
        let map = s.dungeon.as_ref().unwrap();
        assert!(!map.grid[sy][sx].event_done, "stairs must remain re-triggerable");

        // Walk off and back — event must fire again.
        try_move(&mut s, step_on.reverse());
        // Stepping off may briefly land on another event tile; clear it so
        // we can isolate the re-entry assertion.
        s.active_event = None;
        try_move(&mut s, step_on);
        assert!(
            s.active_event.is_some(),
            "stairs event must re-trigger after walking back"
        );
    }

    /// 入口セル (B1F の帰還口) も同様に永続ランドマーク。
    /// スポーン直後はポップアップが出ないが、一度離れて戻れば再度発火する。
    #[test]
    fn entrance_re_triggers_after_walking_off_and_back() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        let map = s.dungeon.as_mut().unwrap();
        let (ex, ey) = (map.player_x, map.player_y);
        assert_eq!(map.grid[ey][ex].cell_type, CellType::Entrance);
        // Spawn cell must not pre-mark event_done — that's the bug we fixed.
        assert!(!map.grid[ey][ex].event_done, "entrance must not be pre-consumed");
        map.monsters.clear();

        // Find an adjacent walkable cell.
        let mut step_dir = None;
        for dir in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let nx = ex as i32 + dir.dx();
            let ny = ey as i32 + dir.dy();
            if map.in_bounds(nx, ny) && map.grid[ny as usize][nx as usize].is_walkable() {
                step_dir = Some(dir);
                break;
            }
        }
        let dir = step_dir.expect("entrance has a walkable neighbor");

        // Step away, dismiss any incidental event, step back.
        try_move(&mut s, dir);
        s.active_event = None;
        try_move(&mut s, dir.reverse());
        assert!(
            s.active_event.is_some(),
            "entrance event must fire on return"
        );
    }

    #[test]
    fn ascend_stairs_from_b2_returns_to_b1() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 2);
        // Place player on entrance cell to trigger ascend choice.
        let event = super::super::events::generate_event(
            super::super::state::CellType::Entrance,
            2,
            super::super::state::FloorTheme::Underground,
            &mut s.rng_seed,
        )
        .expect("entrance event exists");
        // First choice on B2F entrance must be AscendStairs.
        assert_eq!(event.choices[0].action, super::super::state::EventAction::AscendStairs);
        s.active_event = Some(event);
        assert!(resolve_event_choice(&mut s, 0));
        let dungeon = s.dungeon.as_ref().expect("still in dungeon");
        assert_eq!(dungeon.floor_num, 1);
    }

    #[test]
    fn enter_dungeon_creates_grid_map_with_monsters() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        assert_eq!(s.scene, Scene::DungeonExplore);
        assert!(s.dungeon.is_some());
        let d = s.dungeon.as_ref().unwrap();
        assert!(!d.monsters.is_empty(), "Should spawn monsters on floor 1");
    }

    #[test]
    fn satiety_decreases_over_time() {
        // Issue #92 (balance): satiety drains every 2 turns now, not every
        // turn — verify it drops after a couple of actions.
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        let before = s.satiety;
        for _ in 0..4 {
            on_player_action(&mut s);
        }
        assert!(s.satiety < before, "satiety should decrease after 4 turns");
    }

    #[test]
    fn starvation_damages_hp() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        s.satiety = 0;
        let before = s.hp;
        on_player_action(&mut s);
        assert!(s.hp < before);
    }

    #[test]
    fn bump_attack_damages_monster() {
        let mut s = RpgState::new();
        enter_dungeon(&mut s, 1);
        // Place a slime adjacent to the player
        let map = s.dungeon.as_mut().unwrap();
        let px = map.player_x;
        let py = map.player_y;
        // Find an adjacent walkable empty tile
        for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            let ux = nx as usize; let uy = ny as usize;
            if !map.cell(ux, uy).is_walkable() { continue; }
            if map.monster_at(ux, uy).is_some() { continue; }
            map.monsters.push(Monster {
                kind: EnemyKind::Slime,
                x: ux, y: uy, hp: 12, max_hp: 12,
                awake: true, charging: false,
            });
            // Attack by trying to move into the monster
            let mlen = map.monsters.len();
            try_move(&mut s, dir);
            let map2 = s.dungeon.as_ref().unwrap();
            // Either the slime is dead (removed) or has less HP
            if map2.monsters.len() < mlen {
                // Killed
            } else {
                let m = map2.monsters.iter().find(|m| m.x == ux && m.y == uy).unwrap();
                assert!(m.hp < 12);
            }
            return;
        }
    }

    #[test]
    fn quest_slay_progress() {
        let mut s = RpgState::new();
        s.active_quest = Some(Quest {
            kind: QuestKind::Slay { target: EnemyKind::Slime, count: 1, floor: 1 },
            reward_gold: 50, reward_exp: 10, progress: 0,
        });
        enter_dungeon(&mut s, 1);
        // Insert and kill a slime via attack
        let map = s.dungeon.as_mut().unwrap();
        map.monsters.clear();
        map.monsters.push(Monster {
            kind: EnemyKind::Slime, x: 0, y: 0, hp: 1, max_hp: 12,
            awake: true, charging: false,
        });
        attack_monster(&mut s, 0);
        // Quest should be cleared (completed and removed)
        assert!(s.active_quest.is_none());
        assert_eq!(s.completed_quests, 1);
    }

    #[test]
    fn pray_consumes_for_run() {
        let mut s = RpgState::new();
        let _ = pray(&mut s);
        assert!(s.prayed_this_run);
        // Second pray should fail
        let result = pray(&mut s);
        assert!(!result);
    }

    #[test]
    fn affix_weapon_increases_atk() {
        let mut s = RpgState::new();
        s.inventory.push(InventoryItem {
            kind: ItemKind::IronSword, count: 1, affix: Some(Affix::Sharp),
        });
        s.weapon_idx = Some(0);
        // 5 base + 8 sword + 4 sharp = 17
        assert_eq!(s.total_atk(), 17);
    }

    #[test]
    fn return_bonus_scales() {
        assert_eq!(return_bonus(1, 0), 0);
        assert_eq!(return_bonus(1, 3), 9);
        assert_eq!(return_bonus(5, 4), 60);
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
    fn defeat_returns_to_overworld() {
        let mut s = RpgState::new();
        s.gold = 100;
        enter_dungeon(&mut s, 1);
        s.hp = 0;
        process_dungeon_death(&mut s);
        assert_eq!(s.scene, Scene::Overworld);
        assert_eq!(s.gold, 80);
    }

    #[test]
    fn buy_item_at_shop() {
        let mut s = RpgState::new();
        s.gold = 100;
        let shop = shop_items(0);
        let herb_idx = shop.iter().position(|(k, _)| *k == ItemKind::Herb).unwrap();
        assert!(buy_item(&mut s, herb_idx));
        assert_eq!(s.gold, 80);
    }

    #[test]
    fn food_restores_satiety() {
        let mut s = RpgState::new();
        s.satiety = 100;
        add_item(&mut s, ItemKind::Bread, 1);
        let idx = s.inventory.iter().position(|i| i.kind == ItemKind::Bread).unwrap();
        assert!(use_item(&mut s, idx));
        assert_eq!(s.satiety, 400);
    }

    #[test]
    fn equip_weapon_via_use_item() {
        let mut s = RpgState::new();
        add_item(&mut s, ItemKind::IronSword, 1);
        let idx = s.inventory.iter().position(|i| i.kind == ItemKind::IronSword).unwrap();
        assert!(use_item(&mut s, idx));
        assert_eq!(s.weapon_idx, Some(idx));
    }
}
