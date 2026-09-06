//! 遠征団の純粋ロジック。

use super::state::{
    Enemy, ExpeditionState, HubTab, NodeKind, Role, Screen, Sortie, COMBAT_ROUND_TICKS,
    PARTY_SIZE, SCOUT_MEMO_CAP, SCOUT_MEMO_REGEN_TICKS, STAGES_PER_CHAPTER, SUPPLY_CLEAR_BASE,
    SUPPLY_FAIL,
};

fn enemy_for_depth(depth: u32, node: u32, is_boss: bool) -> Enemy {
    let scale = depth + node / 2;
    if is_boss {
        let hp = 40 + scale as i32 * 14;
        Enemy {
            name: match depth % 3 {
                0 => "鬼灯の首領",
                1 => "錆びた甲殻",
                _ => "霧の大口",
            },
            hp,
            max_hp: hp,
            atk: 5 + scale as i32 * 2,
        }
    } else {
        let hp = 16 + scale as i32 * 6;
        Enemy {
            name: match (depth + node) % 4 {
                0 => "野犬",
                1 => "岩蟲",
                2 => "影兵",
                _ => "迷い火",
            },
            hp,
            max_hp: hp,
            atk: 3 + scale as i32,
        }
    }
}

fn scout_hint_for(enemy_name: &str) -> &'static str {
    match enemy_name {
        "鬼灯の首領" | "霧の大口" => "高火力・単体",
        "錆びた甲殻" | "岩蟲" => "硬い",
        "影兵" => "速い",
        "欲深き番人" => "強化個体",
        _ => "標準的な敵",
    }
}

fn node_kind_at(nodes_total: u32, node_index: u32) -> NodeKind {
    if node_index + 1 >= nodes_total {
        NodeKind::Boss
    } else {
        NodeKind::Battle
    }
}

pub fn apply_offline_regen(state: &mut ExpeditionState, elapsed_ticks: u64) {
    let capped = elapsed_ticks.min(10 * 60 * 60 * 8);
    for _ in 0..capped {
        tick_fuel(state);
    }
}

fn tick_fuel(state: &mut ExpeditionState) {
    let cap = state.ration_cap();
    if state.rations < cap {
        state.ration_progress += 1;
        if state.ration_progress >= state.ration_regen_ticks() {
            state.ration_progress = 0;
            state.rations += 1;
        }
    } else {
        state.ration_progress = 0;
    }

    if state.scout_memos < SCOUT_MEMO_CAP {
        state.scout_progress += 1;
        if state.scout_progress >= SCOUT_MEMO_REGEN_TICKS {
            state.scout_progress = 0;
            state.scout_memos += 1;
        }
    } else {
        state.scout_progress = 0;
    }
}

pub fn tick(state: &mut ExpeditionState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        state.elapsed_ticks = state.elapsed_ticks.saturating_add(1);
        tick_fuel(state);
        if state.screen == Screen::Running {
            tick_combat(state);
        }
    }
}

pub fn begin_forming(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Camp {
        return false;
    }
    if state.rations == 0 {
        state.push_log("行軍糧が足りない。");
        return false;
    }
    if state.forming_count() < PARTY_SIZE {
        state.forming = [None; PARTY_SIZE];
        for (slot, hero) in state.roster.iter().take(PARTY_SIZE).enumerate() {
            state.forming[slot] = Some(hero.id);
        }
    }
    state.screen = Screen::Forming;
    state.push_log("出撃メンバーを選ぼう。");
    true
}

pub fn toggle_forming_hero(state: &mut ExpeditionState, hero_id: u8) -> bool {
    if state.screen != Screen::Forming {
        return false;
    }
    if state.hero(hero_id).is_none() {
        return false;
    }
    if let Some(pos) = state.forming.iter().position(|s| *s == Some(hero_id)) {
        state.forming[pos] = None;
        return true;
    }
    if let Some(pos) = state.forming.iter().position(|s| s.is_none()) {
        state.forming[pos] = Some(hero_id);
        return true;
    }
    state.push_log("パーティは3人まで。");
    false
}

pub fn cancel_forming(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Forming {
        return false;
    }
    state.screen = Screen::Camp;
    true
}

pub fn primary_depart(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Camp {
        return false;
    }
    if state.rations == 0 {
        state.push_log("行軍糧が足りない。");
        return false;
    }
    if state.forming_count() != PARTY_SIZE {
        return begin_forming(state);
    }
    // 拠点の主ボタンはワンタップ出撃。編成は副次操作。
    launch_sortie(state, false)
}

pub fn launch_sortie(state: &mut ExpeditionState, use_scout: bool) -> bool {
    if !matches!(state.screen, Screen::Forming | Screen::Camp) {
        return false;
    }
    if state.forming_count() != PARTY_SIZE {
        state.push_log("3人揃えてから出撃。");
        return false;
    }
    if state.rations == 0 {
        state.push_log("行軍糧が足りない。");
        return false;
    }
    if use_scout && state.scout_memos == 0 {
        state.push_log("下調べメモがない。");
        return false;
    }

    let mut party = [0u8; PARTY_SIZE];
    for (i, id) in state.forming.iter().flatten().enumerate() {
        party[i] = *id;
    }
    for id in party {
        if let Some(h) = state.hero_mut(id) {
            h.refresh_max_hp();
            h.hp = h.max_hp;
        }
    }

    state.rations -= 1;
    let used_scout = use_scout && state.scout_memos > 0;
    if used_scout {
        state.scout_memos -= 1;
    }

    let chapter = state.chapter.max(1);
    let stage = state.stage.clamp(1, STAGES_PER_CHAPTER);
    let difficulty = ExpeditionState::difficulty(chapter, stage);
    let is_boss = ExpeditionState::is_boss_stage(stage);
    let enemy = enemy_for_depth(difficulty, 0, is_boss);
    let hint = used_scout.then(|| scout_hint_for(enemy.name));
    let label = ExpeditionState::stage_label(chapter, stage);

    state.sortie = Some(Sortie {
        chapter,
        stage,
        difficulty,
        node_index: 0,
        nodes_total: 1,
        party,
        enemy: Some(enemy),
        used_scout,
        scout_hint: hint,
        aid_ready: true,
        combat_tick: 0,
        supplies_gained: 0,
        last_hit_log: String::new(),
    });
    state.screen = Screen::Running;
    state.last_failed = false;
    state.push_log(format!("{label} へ出撃。"));
    if let Some(h) = hint {
        state.push_log(format!("下調べ: {h}"));
    }
    true
}


fn party_alive(state: &ExpeditionState, party: &[u8; PARTY_SIZE]) -> bool {
    party
        .iter()
        .any(|&id| state.hero(id).map(|h| h.hp > 0).unwrap_or(false))
}

fn tick_combat(state: &mut ExpeditionState) {
    let Some(sortie) = state.sortie.as_mut() else {
        return;
    };
    if sortie.enemy.is_none() {
        return;
    }
    sortie.combat_tick += 1;
    if sortie.combat_tick < COMBAT_ROUND_TICKS {
        return;
    }
    sortie.combat_tick = 0;

    let party = sortie.party;
    let _difficulty = sortie.difficulty;
    let enemy_atk = sortie.enemy.as_ref().map(|e| e.atk).unwrap_or(0);

    let mut total_atk = 0;
    for &id in &party {
        if let Some(h) = state.hero(id) {
            if h.hp > 0 {
                total_atk += h.atk().max(1);
            }
        }
    }
    if let Some(enemy) = state.sortie.as_mut().and_then(|s| s.enemy.as_mut()) {
        enemy.hp -= total_atk;
    }

    for &id in &party {
        let is_support = state
            .hero(id)
            .map(|h| h.role == Role::Support && h.hp > 0)
            .unwrap_or(false);
        if !is_support {
            continue;
        }
        for &tid in &party {
            if let Some(t) = state.hero_mut(tid) {
                if t.hp > 0 {
                    t.hp = (t.hp + 2).min(t.max_hp);
                }
            }
        }
    }

    let enemy_dead = state
        .sortie
        .as_ref()
        .and_then(|s| s.enemy.as_ref())
        .map(|e| e.hp <= 0)
        .unwrap_or(false);
    if enemy_dead {
        let name = state
            .sortie
            .as_ref()
            .and_then(|s| s.enemy.as_ref())
            .map(|e| e.name)
            .unwrap_or("敵");
        state.push_log(format!("{name}を倒した。"));
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.supplies_gained += SUPPLY_CLEAR_BASE + s.chapter;
            s.last_hit_log = format!("{name}を倒した");
        }
        advance_after_battle(state);
        return;
    }

    let mut remaining = enemy_atk;
    let mut order = party.to_vec();
    order.sort_by_key(|id| {
        state
            .hero(*id)
            .map(|h| match h.role {
                Role::Vanguard => 0,
                Role::Support => 1,
                Role::Striker => 2,
            })
            .unwrap_or(9)
    });
    for id in order {
        if remaining <= 0 {
            break;
        }
        if let Some(h) = state.hero_mut(id) {
            if h.hp <= 0 {
                continue;
            }
            let take = remaining.min(h.hp);
            h.hp -= take;
            remaining -= take;
        }
    }

    if !party_alive(state, &party) {
        fail_sortie(state);
    }
}

fn advance_after_battle(state: &mut ExpeditionState) {
    let Some(sortie) = state.sortie.as_mut() else {
        return;
    };
    sortie.node_index += 1;
    if sortie.node_index >= sortie.nodes_total {
        clear_sortie(state);
        return;
    }
    // 現状は1節1戦なのでここには来ない。将来の複数ノード用。
    let is_boss = ExpeditionState::is_boss_stage(sortie.stage)
        && sortie.node_index + 1 >= sortie.nodes_total;
    let difficulty = sortie.difficulty;
    let node = sortie.node_index;
    let enemy = enemy_for_depth(difficulty, node, is_boss);
    if sortie.used_scout {
        sortie.scout_hint = Some(scout_hint_for(enemy.name));
    }
    sortie.enemy = Some(enemy);
    sortie.combat_tick = 0;
    state.screen = Screen::Running;
}

pub fn use_aid(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Running {
        return false;
    }
    let Some(sortie) = state.sortie.as_mut() else {
        return false;
    };
    if !sortie.aid_ready || sortie.enemy.is_none() {
        return false;
    }
    sortie.aid_ready = false;
    let party = sortie.party;
    let _difficulty = sortie.difficulty;

    for id in party {
        if let Some(h) = state.hero_mut(id) {
            if h.hp > 0 {
                h.hp = (h.hp + h.max_hp / 2).min(h.max_hp);
            }
        }
    }

    let mut burst = 0;
    for id in party {
        if let Some(h) = state.hero(id) {
            if h.hp > 0 {
                burst += h.atk().max(1);
            }
        }
    }
    burst *= 2;
    if let Some(enemy) = state.sortie.as_mut().and_then(|s| s.enemy.as_mut()) {
        enemy.hp -= burst;
    }
    state.push_log("援護！ 傷を癒し、追い打ちした。");

    let enemy_dead = state
        .sortie
        .as_ref()
        .and_then(|s| s.enemy.as_ref())
        .map(|e| e.hp <= 0)
        .unwrap_or(false);
    if enemy_dead {
        let name = state
            .sortie
            .as_ref()
            .and_then(|s| s.enemy.as_ref())
            .map(|e| e.name)
            .unwrap_or("敵");
        state.push_log(format!("{name}を倒した。"));
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.supplies_gained += SUPPLY_CLEAR_BASE + s.chapter;
            s.last_hit_log = format!("{name}を倒した");
        }
        advance_after_battle(state);
    }
    true
}

fn clear_sortie(state: &mut ExpeditionState) {
    let (chapter, stage, supplies, party) = match state.sortie.as_ref() {
        Some(s) => (s.chapter, s.stage, s.supplies_gained.max(SUPPLY_CLEAR_BASE), s.party),
        None => return,
    };
    let label = ExpeditionState::stage_label(chapter, stage);
    state.supplies = state.supplies.saturating_add(supplies);
    // レベルは上げない。マップ進行だけ進める。
    for id in party {
        if let Some(h) = state.hero_mut(id) {
            h.refresh_max_hp();
            h.hp = h.max_hp;
        }
    }
    advance_map_progress(state);
    let next = state.current_stage_label();
    state.result_summary = format!(
        "{label} クリア！ 補給+{supplies}\n次は {next} — 足りなければ育成で鍛えよう"
    );
    state.sortie = None;
    state.last_failed = false;
    state.screen = Screen::Result;
    state.push_log(state.result_summary.clone());
}

fn advance_map_progress(state: &mut ExpeditionState) {
    if state.stage >= STAGES_PER_CHAPTER {
        state.chapter = state.chapter.saturating_add(1);
        state.stage = 1;
    } else {
        state.stage += 1;
    }
}

fn fail_sortie(state: &mut ExpeditionState) {
    let (chapter, stage) = match state.sortie.as_ref() {
        Some(s) => (s.chapter, s.stage),
        None => (state.chapter, state.stage),
    };
    let label = ExpeditionState::stage_label(chapter, stage);
    state.supplies = state.supplies.saturating_add(SUPPLY_FAIL);
    if let Some(s) = state.sortie.clone() {
        for id in s.party {
            if let Some(h) = state.hero_mut(id) {
                h.refresh_max_hp();
                h.hp = h.max_hp;
            }
        }
    }
    state.result_summary = format!(
        "{label} で敗退。補給+{SUPPLY_FAIL}\n育成でレベルを上げてから再挑戦"
    );
    state.sortie = None;
    state.last_failed = true;
    state.screen = Screen::Result;
    state.push_log(state.result_summary.clone());
}

/// 拠点育成: 補給を消費して団員を1レベル上げる。
pub fn upgrade_hero(state: &mut ExpeditionState, hero_id: u8) -> bool {
    if !matches!(state.screen, Screen::Camp | Screen::Result) {
        // 育成タブは Camp 画面の hub_tab=Train で描画する
        if state.screen != Screen::Camp {
            return false;
        }
    }
    let level = match state.hero(hero_id) {
        Some(h) => h.level,
        None => return false,
    };
    let cost = ExpeditionState::upgrade_cost(level);
    if state.supplies < cost {
        state.push_log("補給が足りない。探索で集めよう。");
        return false;
    }
    state.supplies -= cost;
    if let Some(h) = state.hero_mut(hero_id) {
        h.level += 1;
        h.refresh_max_hp();
        h.hp = h.max_hp;
        let name = h.name;
        let lv = h.level;
        state.push_log(format!("{name} は Lv{lv} になった。"));
    }
    true
}

pub fn set_hub_tab(state: &mut ExpeditionState, tab: HubTab) -> bool {
    state.hub_tab = tab;
    true
}

pub fn acknowledge_result(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Result {
        return false;
    }
    state.screen = Screen::Camp;
    state.result_summary.clear();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_depart_launches_from_camp_when_party_ready() {
        let mut state = ExpeditionState::new();
        assert!(state.rations > 0);
        assert_eq!(state.forming_count(), PARTY_SIZE);
        assert!(primary_depart(&mut state));
        assert_eq!(state.screen, Screen::Running);
        assert!(state.sortie.is_some());
    }

    #[test]
    fn primary_depart_without_rations_stays_in_camp() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        assert!(!primary_depart(&mut state));
        assert_eq!(state.screen, Screen::Camp);
    }
    use crate::games::expedition::state::BASE_RATION_CAP;

    #[test]
    fn idle_regen_fills_rations_without_raising_level() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        let level_before = state.total_level();
        let ticks = state.ration_regen_ticks() as u64;
        apply_offline_regen(&mut state, ticks);
        assert!(state.rations >= 1);
        assert_eq!(state.total_level(), level_before);
    }

    #[test]
    fn sortie_consumes_ration_and_grants_supplies_not_levels() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        assert!(begin_forming(&mut state));
        assert!(launch_sortie(&mut state, false));
        assert_eq!(state.rations, 1);
        let level_before = state.total_level();
        let supplies_before = state.supplies;
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.node_index = s.nodes_total;
            s.supplies_gained = 3;
        }
        clear_sortie(&mut state);
        assert_eq!(state.total_level(), level_before);
        assert!(state.supplies > supplies_before);
        assert_eq!(state.screen, Screen::Result);
        assert_eq!(state.stage, 2);
    }

    #[test]
    fn upgrade_hero_spends_supplies_to_raise_level() {
        let mut state = ExpeditionState::new();
        state.supplies = 10;
        let before = state.hero(0).unwrap().level;
        assert!(upgrade_hero(&mut state, 0));
        assert_eq!(state.hero(0).unwrap().level, before + 1);
        assert!(state.supplies < 10);
    }

    #[test]
    fn fail_keeps_stage_and_marks_last_failed() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        assert!(launch_sortie(&mut state, false));
        assert_eq!(state.stage, 1);
        fail_sortie(&mut state);
        assert_eq!(state.stage, 1);
        assert!(state.last_failed);
        assert!(state.supplies >= SUPPLY_FAIL);
    }

    #[test]
    fn cannot_launch_without_rations() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        assert!(!begin_forming(&mut state));
        assert_eq!(state.screen, Screen::Camp);
    }

    #[test]
    fn ration_cap_starts_at_base() {
        let state = ExpeditionState::new();
        assert_eq!(state.ration_cap(), BASE_RATION_CAP);
    }

    #[test]
    fn auto_run_reaches_result_without_manual_choice() {
        let mut state = ExpeditionState::new();
        state.rations = 3;
        assert!(begin_forming(&mut state));
        assert!(launch_sortie(&mut state, false));
        for _ in 0..20_000 {
            if matches!(state.screen, Screen::Result) {
                break;
            }
            tick(&mut state, 1);
        }
        assert!(matches!(state.screen, Screen::Result));
        assert!(state.supplies > 0 || state.stage > 1 || state.last_failed);
    }

    #[test]
    fn use_aid_heals_and_is_optional_once() {
        let mut state = ExpeditionState::new();
        state.rations = 3;
        begin_forming(&mut state);
        launch_sortie(&mut state, false);
        for h in &mut state.roster {
            h.hp = 1;
        }
        let hp_before: i32 = state.roster.iter().map(|h| h.hp).sum();
        assert!(use_aid(&mut state));
        let hp_after: i32 = state.roster.iter().map(|h| h.hp).sum();
        // 追い打ちで敵を倒して結果画面に入ることもある
        if state.screen == Screen::Running {
            assert!(hp_after > hp_before);
            assert!(!use_aid(&mut state));
            assert!(!state.sortie.as_ref().unwrap().aid_ready);
        } else {
            assert_eq!(state.screen, Screen::Result);
        }
    }

    #[test]
    fn mid_nodes_are_battles_not_choices() {
        assert_eq!(node_kind_at(4, 0), NodeKind::Battle);
        assert_eq!(node_kind_at(4, 1), NodeKind::Battle);
        assert_eq!(node_kind_at(4, 2), NodeKind::Battle);
        assert_eq!(node_kind_at(4, 3), NodeKind::Boss);
    }
}
