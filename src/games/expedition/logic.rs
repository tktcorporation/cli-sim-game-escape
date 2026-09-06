//! 遠征団の純粋ロジック。

use super::state::{
    Enemy, ExpeditionState, HubTab, NodeKind, Role, Screen, Sortie, BASE_NODES, COMBAT_ROUND_TICKS,
    PARTY_SIZE, SCOUT_MEMO_CAP, SCOUT_MEMO_REGEN_TICKS,
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
    } else if node_index == nodes_total / 2 {
        NodeKind::Choice
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

    let depth = state.best_depth.max(1);
    let enemy = enemy_for_depth(depth, 0, false);
    let hint = used_scout.then(|| scout_hint_for(enemy.name));

    state.sortie = Some(Sortie {
        depth,
        node_index: 0,
        nodes_total: BASE_NODES,
        party,
        enemy: Some(enemy),
        used_scout,
        scout_hint: hint,
        pending_choice: false,
        combat_tick: 0,
        bond_gained: 0,
    });
    state.screen = Screen::Running;
    state.push_log(format!("第{depth}層へ出撃。"));
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
    if sortie.pending_choice || sortie.enemy.is_none() {
        return;
    }
    sortie.combat_tick += 1;
    if sortie.combat_tick < COMBAT_ROUND_TICKS {
        return;
    }
    sortie.combat_tick = 0;

    let party = sortie.party;
    let depth = sortie.depth;
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
            s.bond_gained += 1 + depth / 3;
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
    match node_kind_at(sortie.nodes_total, sortie.node_index) {
        NodeKind::Choice => {
            sortie.pending_choice = true;
            state.screen = Screen::Choice;
            state.push_log("分かれ道。休むか、突っ込むか。");
        }
        kind @ (NodeKind::Battle | NodeKind::Boss) => {
            let is_boss = kind == NodeKind::Boss;
            let depth = sortie.depth;
            let node = sortie.node_index;
            let enemy = enemy_for_depth(depth, node, is_boss);
            if sortie.used_scout {
                sortie.scout_hint = Some(scout_hint_for(enemy.name));
            }
            sortie.enemy = Some(enemy);
            sortie.combat_tick = 0;
            state.screen = Screen::Running;
            if is_boss {
                state.push_log("奥の気配が近い。");
            }
        }
    }
}

pub fn choose_rest(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Choice {
        return false;
    }
    let Some(sortie) = state.sortie.as_mut() else {
        return false;
    };
    if !sortie.pending_choice {
        return false;
    }
    let party = sortie.party;
    sortie.pending_choice = false;
    for id in party {
        if let Some(h) = state.hero_mut(id) {
            h.hp = (h.hp + h.max_hp / 2).min(h.max_hp);
        }
    }
    state.push_log("焚き火で傷を癒した。");

    let Some(sortie) = state.sortie.as_mut() else {
        return true;
    };
    let depth = sortie.depth;
    sortie.node_index += 1;
    if sortie.node_index >= sortie.nodes_total {
        clear_sortie(state);
        return true;
    }
    let is_boss = node_kind_at(sortie.nodes_total, sortie.node_index) == NodeKind::Boss;
    let enemy = enemy_for_depth(depth, sortie.node_index, is_boss);
    sortie.enemy = Some(enemy);
    sortie.combat_tick = 0;
    state.screen = Screen::Running;
    true
}

pub fn choose_push(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Choice {
        return false;
    }
    let Some(sortie) = state.sortie.as_mut() else {
        return false;
    };
    if !sortie.pending_choice {
        return false;
    }
    sortie.pending_choice = false;
    let depth = sortie.depth;
    let node = sortie.node_index;
    let mut enemy = enemy_for_depth(depth, node, false);
    enemy.hp = enemy.hp * 3 / 2;
    enemy.max_hp = enemy.hp;
    enemy.atk += 2;
    enemy.name = "欲深き番人";
    sortie.enemy = Some(enemy);
    sortie.bond_gained += 2;
    sortie.combat_tick = 0;
    state.screen = Screen::Running;
    state.push_log("欲を出して踏み込んだ。");
    true
}

fn clear_sortie(state: &mut ExpeditionState) {
    let (depth, bond, party) = match state.sortie.as_ref() {
        Some(s) => (s.depth, s.bond_gained, s.party),
        None => return,
    };
    for id in party {
        if let Some(h) = state.hero_mut(id) {
            h.bond += bond;
            h.refresh_max_hp();
            h.hp = h.max_hp;
        }
    }
    if depth >= state.best_depth {
        state.best_depth = depth + 1;
    }
    state.result_summary = format!(
        "第{depth}層クリア！ 参加者の絆+{bond}\n（絆が上がると力と体力が伸び、糧の回復も少し速くなる）"
    );
    state.sortie = None;
    state.screen = Screen::Result;
    state.push_log(state.result_summary.clone());
}

fn fail_sortie(state: &mut ExpeditionState) {
    let depth = state.sortie.as_ref().map(|s| s.depth).unwrap_or(1);
    if let Some(s) = state.sortie.clone() {
        for id in s.party {
            if let Some(h) = state.hero_mut(id) {
                h.bond += 1;
                h.refresh_max_hp();
                h.hp = h.max_hp;
            }
        }
    }
    state.result_summary =
        format!("第{depth}層で敗退。持ち帰れた絆はわずかに+1。\n次は編成を変えるか、下調べして再挑戦。");
    state.sortie = None;
    state.screen = Screen::Result;
    state.push_log(state.result_summary.clone());
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
    fn idle_regen_fills_rations_without_raising_bond() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        let bond_before = state.total_bond();
        let ticks = state.ration_regen_ticks() as u64;
        apply_offline_regen(&mut state, ticks);
        assert!(state.rations >= 1);
        assert_eq!(state.total_bond(), bond_before);
    }

    #[test]
    fn sortie_consumes_ration_and_grows_bond_only_on_clear() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        assert!(begin_forming(&mut state));
        assert!(launch_sortie(&mut state, false));
        assert_eq!(state.rations, 1);
        let bond_before = state.total_bond();
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.node_index = s.nodes_total;
            s.bond_gained = 3;
        }
        clear_sortie(&mut state);
        assert!(state.total_bond() > bond_before);
        assert_eq!(state.screen, Screen::Result);
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
    fn rest_heals_party() {
        let mut state = ExpeditionState::new();
        state.rations = 3;
        begin_forming(&mut state);
        launch_sortie(&mut state, false);
        for h in &mut state.roster {
            h.hp = 1;
        }
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.node_index = s.nodes_total / 2;
            s.pending_choice = true;
        }
        state.screen = Screen::Choice;
        let hp_before: i32 = state.roster.iter().map(|h| h.hp).sum();
        assert!(choose_rest(&mut state));
        let hp_after: i32 = state.roster.iter().map(|h| h.hp).sum();
        assert!(hp_after > hp_before);
    }

    #[test]
    fn push_spawns_elite() {
        let mut state = ExpeditionState::new();
        state.rations = 3;
        begin_forming(&mut state);
        launch_sortie(&mut state, false);
        if let Some(s) = state.sortie.as_mut() {
            s.enemy = None;
            s.node_index = s.nodes_total / 2;
            s.pending_choice = true;
        }
        state.screen = Screen::Choice;
        assert!(choose_push(&mut state));
        assert_eq!(
            state
                .sortie
                .as_ref()
                .and_then(|s| s.enemy.as_ref())
                .map(|e| e.name),
            Some("欲深き番人")
        );
    }
}
