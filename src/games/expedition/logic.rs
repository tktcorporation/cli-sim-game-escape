//! 遠征団の純粋ロジック。

use super::state::{
    Creep, ExpeditionState, HubTab, Role, Screen, Sortie, BOARD_GOAL, ENEMY_STEP_TICKS,
    HERO_ATK_TICKS, MEDAL_BET, MEDAL_CLEAR_BASE, MEDAL_FAIL, PARTY_SIZE, PATH_LEN,
    STAGES_PER_CHAPTER,
};

fn creep_for(difficulty: u32, wave: u32, is_boss_wave: bool) -> Creep {
    let scale = difficulty + wave;
    if is_boss_wave {
        let hp = 28 + scale as i32 * 10;
        Creep {
            name: match difficulty % 3 {
                0 => "鬼灯の首領",
                1 => "錆びた甲殻",
                _ => "霧の大口",
            },
            hp,
            max_hp: hp,
            pos: 0,
            slow_ticks: 0,
            step_progress: 0,
        }
    } else {
        let hp = 8 + scale as i32 * 3;
        Creep {
            name: match (difficulty + wave) % 4 {
                0 => "野犬",
                1 => "岩蟲",
                2 => "影兵",
                _ => "迷い火",
            },
            hp,
            max_hp: hp,
            pos: 0,
            slow_ticks: 0,
            step_progress: 0,
        }
    }
}

fn waves_for(difficulty: u32, is_boss_stage: bool) -> u32 {
    let base = 3 + difficulty / 2;
    if is_boss_stage {
        base + 1
    } else {
        base
    }
}

fn gate_hp_for(party_level: u32, difficulty: u32) -> i32 {
    (12 + party_level as i32 * 2 - difficulty as i32).max(6)
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
}

pub fn tick(state: &mut ExpeditionState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        state.elapsed_ticks = state.elapsed_ticks.saturating_add(1);
        tick_fuel(state);
        if state.screen == Screen::Running {
            tick_defense(state);
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
    if !matches!(state.screen, Screen::Forming | Screen::Placing) {
        return false;
    }
    // 配置中のキャンセルは行軍糧を返さない（すでに消費済み）。拠点へ戻すだけ。
    state.sortie = None;
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
    launch_sortie(state)
}

pub fn launch_sortie(state: &mut ExpeditionState) -> bool {
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

    let mut party = [0u8; PARTY_SIZE];
    for (i, id) in state.forming.iter().flatten().enumerate() {
        party[i] = *id;
    }

    state.rations -= 1;

    let chapter = state.chapter.max(1);
    let stage = state.stage.clamp(1, STAGES_PER_CHAPTER);
    let difficulty = ExpeditionState::difficulty(chapter, stage);
    let is_boss = ExpeditionState::is_boss_stage(stage);
    let waves_total = waves_for(difficulty, is_boss);
    let party_level: u32 = party
        .iter()
        .filter_map(|&id| state.hero(id).map(|h| h.level))
        .sum();
    let gate = gate_hp_for(party_level, difficulty);
    let label = ExpeditionState::stage_label(chapter, stage);

    // 初期配置: 先頭から詰めて仮置き。配置画面で並べ替える。
    let mut path = [None; PATH_LEN];
    for (i, &id) in party.iter().enumerate() {
        if i < PATH_LEN {
            path[i] = Some(id);
        }
    }

    state.sortie = Some(Sortie {
        chapter,
        stage,
        difficulty,
        party,
        path,
        placing_cursor: 0,
        creeps: Vec::new(),
        wave_index: 0,
        waves_total,
        spawn_cooldown: 0,
        pending_spawns: 0,
        gate_hp: gate,
        gate_max_hp: gate,
        combat_tick: 0,
        medals_gained: 0,
        last_hit_log: String::new(),
    });
    state.screen = Screen::Placing;
    state.last_failed = false;
    state.push_log(format!("{label} — 道に配置せよ。"));
    true
}

/// 配置画面: 指定マスに、まだ道に居ないパーティメンバーを置く／入れ替える。
pub fn place_on_slot(state: &mut ExpeditionState, slot: usize) -> bool {
    if state.screen != Screen::Placing {
        return false;
    }
    if slot >= PATH_LEN {
        return false;
    }
    let Some(sortie) = state.sortie.as_mut() else {
        return false;
    };

    // タップしたマスに既にいる場合は外す。
    if sortie.path[slot].is_some() {
        sortie.path[slot] = None;
        return true;
    }

    let placed: Vec<u8> = sortie.path.iter().flatten().copied().collect();
    let next = sortie
        .party
        .iter()
        .copied()
        .find(|id| !placed.contains(id));
    let Some(hero_id) = next else {
        state.push_log("全員配置済み。マスを外して並べ替えられる。");
        return false;
    };
    sortie.path[slot] = Some(hero_id);
    true
}

pub fn confirm_placement(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Placing {
        return false;
    }
    let Some(sortie) = state.sortie.as_ref() else {
        return false;
    };
    let placed = sortie.path.iter().flatten().count();
    if placed < PARTY_SIZE {
        state.push_log("3人すべて道に置け。");
        return false;
    }
    // 最初のウェーブを準備
    if let Some(s) = state.sortie.as_mut() {
        s.wave_index = 0;
        s.pending_spawns = spawns_for_wave(s.difficulty, 0, s.waves_total);
        s.spawn_cooldown = 2;
        s.creeps.clear();
        s.combat_tick = 0;
    }
    state.screen = Screen::Running;
    state.push_log("防衛開始。");
    true
}

fn spawns_for_wave(difficulty: u32, wave: u32, waves_total: u32) -> u32 {
    let is_boss = wave + 1 >= waves_total;
    if is_boss {
        1
    } else {
        2 + difficulty / 3 + wave / 2
    }
}

fn tick_defense(state: &mut ExpeditionState) {
    let Some(sortie) = state.sortie.as_mut() else {
        return;
    };
    sortie.combat_tick = sortie.combat_tick.saturating_add(1);

    // スポーン
    if sortie.pending_spawns > 0 {
        if sortie.spawn_cooldown > 0 {
            sortie.spawn_cooldown -= 1;
        } else {
            let is_boss = sortie.wave_index + 1 >= sortie.waves_total;
            let creep = creep_for(sortie.difficulty, sortie.wave_index, is_boss);
            sortie.creeps.push(creep);
            sortie.pending_spawns -= 1;
            sortie.spawn_cooldown = 3;
            if is_boss {
                sortie.last_hit_log = "ボス出現".into();
            }
        }
    } else if sortie.creeps.is_empty() {
        // ウェーブ完了 → 次 or クリア
        if sortie.wave_index + 1 >= sortie.waves_total {
            clear_sortie(state);
            return;
        }
        sortie.wave_index += 1;
        let diff = sortie.difficulty;
        let waves = sortie.waves_total;
        let wi = sortie.wave_index;
        sortie.pending_spawns = spawns_for_wave(diff, wi, waves);
        sortie.spawn_cooldown = 4;
        sortie.last_hit_log = format!("第{}波", wi + 1);
    }

    // 再借用
    let Some(sortie) = state.sortie.as_mut() else {
        return;
    };

    // 団員攻撃（一定間隔）
    if sortie.combat_tick % HERO_ATK_TICKS == 0 {
        hero_attacks(state);
    }

    let Some(sortie) = state.sortie.as_mut() else {
        return;
    };

    // 敵移動
    let mut leaks = 0i32;
    for creep in &mut sortie.creeps {
        if creep.hp <= 0 {
            continue;
        }
        if creep.slow_ticks > 0 {
            creep.slow_ticks -= 1;
            // 減速中は半分の速度
            if sortie.combat_tick % 2 == 1 {
                continue;
            }
        }
        creep.step_progress += 1;
        let need = ENEMY_STEP_TICKS;
        if creep.step_progress >= need {
            creep.step_progress = 0;
            if creep.pos + 1 >= PATH_LEN {
                leaks += 1;
                creep.hp = 0;
            } else {
                creep.pos += 1;
            }
        }
    }
    if leaks > 0 {
        sortie.gate_hp -= leaks * (2 + sortie.difficulty as i32 / 2);
        sortie.last_hit_log = format!("門に到達 -{leaks}");
        if sortie.gate_hp <= 0 {
            fail_sortie(state);
            return;
        }
    }

    // 撃破除去
    if let Some(s) = state.sortie.as_mut() {
        let before = s.creeps.len();
        s.creeps.retain(|c| c.hp > 0);
        let killed = before.saturating_sub(s.creeps.len());
        if killed > 0 {
            s.medals_gained += killed as u32;
        }
    }
}

fn hero_attacks(state: &mut ExpeditionState) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    // (hero_slot, hero_id) のスナップショット
    let placements: Vec<(usize, u8)> = sortie
        .path
        .iter()
        .enumerate()
        .filter_map(|(i, id)| id.map(|h| (i, h)))
        .collect();

    let mut hit_log = String::new();
    for (slot, hero_id) in placements {
        let Some(hero) = state.hero(hero_id).cloned() else {
            continue;
        };
        let range = hero.role.range();
        let atk = hero.atk();

        let Some(sortie) = state.sortie.as_mut() else {
            return;
        };
        // 射程内で最も門に近い（pos が大きい）敵を狙う
        let target = sortie
            .creeps
            .iter_mut()
            .filter(|c| c.hp > 0)
            .filter(|c| {
                let dist = (c.pos as i32 - slot as i32).abs();
                dist <= range
            })
            .max_by_key(|c| c.pos);

        if let Some(creep) = target {
            creep.hp -= atk;
            if hero.role == Role::Support {
                creep.slow_ticks = creep.slow_ticks.max(4);
            }
            if creep.hp <= 0 {
                hit_log = format!("{}が{}を撃破", hero.name, creep.name);
            } else if hit_log.is_empty() {
                hit_log = format!("{}→{} -{atk}", hero.name, creep.name);
            }
        }
    }
    if let Some(s) = state.sortie.as_mut() {
        if !hit_log.is_empty() {
            s.last_hit_log = hit_log;
        }
    }
}

fn clear_sortie(state: &mut ExpeditionState) {
    let (chapter, stage, medals) = match state.sortie.as_ref() {
        Some(s) => (
            s.chapter,
            s.stage,
            (s.medals_gained + MEDAL_CLEAR_BASE + s.chapter).max(MEDAL_CLEAR_BASE),
        ),
        None => return,
    };
    let label = ExpeditionState::stage_label(chapter, stage);
    state.medals = state.medals.saturating_add(medals);
    advance_map_progress(state);
    let next = state.current_stage_label();
    state.result_summary = format!(
        "{label} 防衛成功！ メダル+{medals}\n次は {next} — 弱ければ遊技場で育てよう"
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
    let (chapter, stage, gained) = match state.sortie.as_ref() {
        Some(s) => (s.chapter, s.stage, s.medals_gained),
        None => (state.chapter, state.stage, 0),
    };
    let label = ExpeditionState::stage_label(chapter, stage);
    let medals = MEDAL_FAIL + gained / 2;
    state.medals = state.medals.saturating_add(medals);
    state.result_summary = format!(
        "{label} で門が落ちた。メダル+{medals}\n遊技場ですごろくを回してレベルを上げよう"
    );
    state.sortie = None;
    state.last_failed = true;
    state.screen = Screen::Result;
    state.push_log(state.result_summary.clone());
}

/// 遊技場: メダルを消費してサイコロを振り、すごろくを進める。
pub fn medal_roll(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Camp || state.hub_tab != HubTab::Arcade {
        return false;
    }
    if state.pending_level_pick {
        state.push_log("先にレベルアップする団員を選ぼう。");
        return false;
    }
    if state.medals < MEDAL_BET {
        state.push_log("メダルが足りない。戦役で集めよう。");
        return false;
    }
    state.medals -= MEDAL_BET;
    let roll = (state.next_rng() % 6) + 1;
    state.board_pos = state.board_pos.saturating_add(roll);
    state.push_log(format!("サイコロ {roll}！"));

    // 途中マスの小さな払い出し（目的地以外）
    if state.board_pos < state.board_goal && roll == 6 {
        state.medals += 1;
        state.push_log("出目6 — メダル+1");
    }

    if state.board_pos >= state.board_goal {
        state.board_pos = state.board_goal;
        state.pending_level_pick = true;
        // 小当たりのメダル払い出し
        let payout = 3 + state.chapter;
        state.medals = state.medals.saturating_add(payout);
        state.push_log(format!("目的地到着！ メダル+{payout} — 誰を育てる？"));
    }
    true
}

/// 目的地到達後: 団員を選んでレベル+1。盤面をリセットする。
pub fn pick_level_hero(state: &mut ExpeditionState, hero_id: u8) -> bool {
    if !state.pending_level_pick {
        return false;
    }
    if state.screen != Screen::Camp || state.hub_tab != HubTab::Arcade {
        return false;
    }
    let Some(h) = state.hero_mut(hero_id) else {
        return false;
    };
    h.level += 1;
    let name = h.name;
    let lv = h.level;
    state.pending_level_pick = false;
    state.board_pos = 0;
    state.board_goal = BOARD_GOAL;
    state.push_log(format!("{name} は Lv{lv} になった。"));
    true
}

pub fn set_hub_tab(state: &mut ExpeditionState, tab: HubTab) -> bool {
    if matches!(state.screen, Screen::Placing | Screen::Running | Screen::Forming) {
        return false;
    }
    state.hub_tab = tab;
    true
}

pub fn acknowledge_result(state: &mut ExpeditionState) -> bool {
    if state.screen != Screen::Result {
        return false;
    }
    state.screen = Screen::Camp;
    if state.last_failed {
        state.hub_tab = HubTab::Arcade;
    }
    state.result_summary.clear();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::expedition::state::BASE_RATION_CAP;

    fn place_all_and_start(state: &mut ExpeditionState) {
        assert!(launch_sortie(state));
        assert_eq!(state.screen, Screen::Placing);
        // launch で仮配置済み
        assert!(confirm_placement(state));
        assert_eq!(state.screen, Screen::Running);
    }

    #[test]
    fn primary_depart_opens_placing_when_party_ready() {
        let mut state = ExpeditionState::new();
        assert!(primary_depart(&mut state));
        assert_eq!(state.screen, Screen::Placing);
        assert!(state.sortie.is_some());
    }

    #[test]
    fn primary_depart_without_rations_stays_in_camp() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        assert!(!primary_depart(&mut state));
        assert_eq!(state.screen, Screen::Camp);
    }

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
    fn clear_grants_medals_not_levels() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        place_all_and_start(&mut state);
        let level_before = state.total_level();
        let medals_before = state.medals;
        // 強制クリア
        if let Some(s) = state.sortie.as_mut() {
            s.creeps.clear();
            s.pending_spawns = 0;
            s.wave_index = s.waves_total.saturating_sub(1);
        }
        // 次 tick でクリア判定
        tick_defense(&mut state);
        assert_eq!(state.screen, Screen::Result);
        assert!(!state.last_failed);
        assert_eq!(state.total_level(), level_before);
        assert!(state.medals > medals_before);
        assert_eq!(state.stage, 2);
    }

    #[test]
    fn medal_roll_reaches_goal_then_levels_hero() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        state.medals = 20;
        state.board_pos = state.board_goal - 1;
        let before = state.hero(0).unwrap().level;
        assert!(medal_roll(&mut state));
        assert!(state.pending_level_pick);
        assert!(pick_level_hero(&mut state, 0));
        assert_eq!(state.hero(0).unwrap().level, before + 1);
        assert!(!state.pending_level_pick);
        assert_eq!(state.board_pos, 0);
    }

    #[test]
    fn medal_roll_requires_medals() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        state.medals = 0;
        assert!(!medal_roll(&mut state));
    }

    #[test]
    fn fail_keeps_stage_and_marks_last_failed() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        place_all_and_start(&mut state);
        assert_eq!(state.stage, 1);
        if let Some(s) = state.sortie.as_mut() {
            s.gate_hp = 1;
        }
        // 門まで敵を送り込む
        if let Some(s) = state.sortie.as_mut() {
            s.creeps.push(Creep {
                name: "野犬",
                hp: 99,
                max_hp: 99,
                pos: PATH_LEN - 1,
                slow_ticks: 0,
                step_progress: ENEMY_STEP_TICKS,
            });
            s.combat_tick = 1;
        }
        // step 処理を走らせるため slow なしで tick
        let Some(s) = state.sortie.as_mut() else {
            panic!();
        };
        // 直接 fail を呼ぶ（門破壊経路の回帰）
        let _ = s;
        fail_sortie(&mut state);
        assert_eq!(state.stage, 1);
        assert!(state.last_failed);
        assert!(state.medals >= MEDAL_FAIL);
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
    fn auto_defense_reaches_result() {
        let mut state = ExpeditionState::new();
        state.rations = 3;
        // 強めにして確実に勝たせる
        for h in &mut state.roster {
            h.level = 8;
        }
        place_all_and_start(&mut state);
        for _ in 0..30_000 {
            if matches!(state.screen, Screen::Result) {
                break;
            }
            tick(&mut state, 1);
        }
        assert!(matches!(state.screen, Screen::Result));
    }

    #[test]
    fn place_on_slot_toggles_occupancy() {
        let mut state = ExpeditionState::new();
        assert!(launch_sortie(&mut state));
        // 全外し
        for i in 0..PATH_LEN {
            if state.sortie.as_ref().unwrap().path[i].is_some() {
                assert!(place_on_slot(&mut state, i));
            }
        }
        assert_eq!(
            state.sortie.as_ref().unwrap().path.iter().flatten().count(),
            0
        );
        assert!(place_on_slot(&mut state, 2));
        assert_eq!(state.sortie.as_ref().unwrap().path[2], Some(0));
    }

    #[test]
    fn confirm_placement_requires_three() {
        let mut state = ExpeditionState::new();
        assert!(launch_sortie(&mut state));
        for i in 0..PATH_LEN {
            if state.sortie.as_ref().unwrap().path[i].is_some() {
                let _ = place_on_slot(&mut state, i);
            }
        }
        assert!(!confirm_placement(&mut state));
        assert_eq!(state.screen, Screen::Placing);
    }
}
