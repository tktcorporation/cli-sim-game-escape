//! 周回討伐 — ゲームロジック。純粋関数のみ、フルテスト可能。

use crate::effects::FlashTimer;

use super::state::{
    CampUpgrades, LoopMarchState, Monster, Phase, PathSlot, Terrain, ATTACK_PER_LEVEL, HAND_MAX,
    HP_PER_LEVEL, MOVE_TICKS, PATH_LEN, REFILL_STONE_COST, REFILL_WOOD_COST, RING_H, RING_W,
};

/// 周回を重ねるごとの敵強化率 (1周あたり)。
pub const DIFFICULTY_PER_LAP: f64 = 0.15;

/// xorshift32。seed=0 は不動点になるため固定値に補正する。
pub fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

fn rng_below(seed: &mut u32, bound: u32) -> u32 {
    if bound == 0 {
        return 0;
    }
    rng_next(seed) % bound
}

fn random_terrain(seed: &mut u32) -> Terrain {
    let all = Terrain::all();
    all[rng_below(seed, all.len() as u32) as usize]
}

/// 遠征開始時の手札を組み立てる。森と岩山を必ず1枚ずつ確保する —
/// でないと木材と石材のどちらかが永久に0のままとなり、両方を要求する
/// 手札補充 (`refill_hand`) が二度と成立しない詰み状態になり得る
/// (草原/墓地はどちらの資源も生まないため)。残り枠のみ完全ランダム。
fn draw_starting_hand(hand_size: usize, seed: &mut u32) -> Vec<Option<Terrain>> {
    let mut hand = vec![None; HAND_MAX];
    if hand_size > 0 {
        hand[0] = Some(Terrain::Forest);
    }
    if hand_size > 1 {
        hand[1] = Some(Terrain::Mountain);
    }
    for slot in hand.iter_mut().skip(2).take(hand_size.saturating_sub(2)) {
        *slot = Some(random_terrain(seed));
    }
    hand
}

/// リング上の各 `path` インデックスに対応する矩形グリッド座標 `(gx, gy)` を
/// 時計回りに列挙する。`render.rs` の表示と `mod.rs` のクリック判定の
/// 両方から参照される、道の唯一の座標変換ソース。
pub fn ring_positions() -> Vec<(usize, usize)> {
    let mut v = Vec::with_capacity(PATH_LEN);
    // 上辺: 左→右
    for gx in 0..RING_W {
        v.push((gx, 0));
    }
    // 右辺: 上→下 (角は上辺/下辺で数えているので中間のみ)
    for gy in 1..RING_H - 1 {
        v.push((RING_W - 1, gy));
    }
    // 下辺: 右→左
    for gx in (0..RING_W).rev() {
        v.push((gx, RING_H - 1));
    }
    // 左辺: 下→上
    for gy in (1..RING_H - 1).rev() {
        v.push((0, gy));
    }
    v
}

/// グリッド座標から道インデックスを逆引きする。道の外 (リング内部) なら `None`。
pub fn ring_index_at(gx: usize, gy: usize) -> Option<usize> {
    ring_positions().iter().position(|&(x, y)| x == gx && y == gy)
}

/// 隣接 (ループ上で前後1マス) している岩山タイルのペア数。
/// 盤面から毎tick再計算する常時ボーナスとして勇者の防御に乗る
/// (シナジー: 岩山の連なり)。
pub fn mountain_synergy_defense(path: &[PathSlot]) -> i32 {
    let n = path.len();
    let mut pairs = 0;
    for i in 0..n {
        let j = (i + 1) % n;
        if path[i].terrain == Some(Terrain::Mountain) && path[j].terrain == Some(Terrain::Mountain)
        {
            pairs += 1;
        }
    }
    pairs
}

/// `index` を含む森タイルの連続数 (ループの前後をたどって数える)。
/// `render.rs` がシナジー成立(≥2)の視覚ヒントを出す判定にも使う。
pub fn forest_cluster_size(path: &[PathSlot], index: usize) -> usize {
    let n = path.len();
    if path[index].terrain != Some(Terrain::Forest) {
        return 0;
    }
    let mut size = 1;
    let mut i = (index + 1) % n;
    while i != index && path[i].terrain == Some(Terrain::Forest) {
        size += 1;
        i = (i + 1) % n;
    }
    let mut i = (index + n - 1) % n;
    while i != index && path[i].terrain == Some(Terrain::Forest) {
        size += 1;
        i = (i + n - 1) % n;
    }
    // リング全周が森だと前方・後方の両ループが同じ他マスを踏破し、
    // 二重計上されて n を超える。ループ上に存在するタイル数を超えない。
    size.min(n)
}

/// ゲーム全体を `n` tick 進める。
pub fn tick_n(state: &mut LoopMarchState, n: u32) {
    for _ in 0..n {
        tick(state);
    }
}

/// 1 tick 進める。拠点にいる間は何もしない。
pub fn tick(state: &mut LoopMarchState) {
    state.hero_hurt_flash.tick(1);
    state.enemy_hurt_flash.tick(1);
    if state.phase != Phase::Expedition || state.hero.hp <= 0 {
        return;
    }

    let pos = state.hero.position;
    if state.path[pos].monster.is_some() {
        resolve_combat_tick(state);
    } else {
        state.move_progress += 1;
        if state.move_progress >= MOVE_TICKS {
            state.move_progress = 0;
            advance_hero(state);
        }
    }
}

/// 勇者が現在マスのモンスターと1 tick分の攻防を行う。
fn resolve_combat_tick(state: &mut LoopMarchState) {
    let pos = state.hero.position;
    let hero_atk = state.hero.attack;

    let defeated_reward = match state.path[pos].monster.as_mut() {
        Some(monster) => {
            monster.hp -= hero_atk;
            state.enemy_hurt_flash.trigger(3);
            if monster.hp <= 0 {
                Some((monster.terrain, monster.elite))
            } else {
                None
            }
        }
        None => return,
    };

    if let Some((terrain, elite)) = defeated_reward {
        state.path[pos].monster = None;
        grant_kill_reward(state, terrain, elite);
        return; // 倒した瞬間は反撃を受けない
    }

    let defense = mountain_synergy_defense(&state.path);
    let monster_attack = match &state.path[pos].monster {
        Some(m) => m.attack,
        None => return,
    };
    let dmg = (monster_attack - defense).max(1);
    state.hero.hp -= dmg;
    state.hero_hurt_flash.trigger(3);
    if state.hero.hp <= 0 {
        state.hero.hp = 0;
        handle_death(state);
    }
}

fn grant_kill_reward(state: &mut LoopMarchState, terrain: Terrain, elite: bool) {
    match terrain {
        Terrain::Forest => {
            let gained = if elite { 6 } else { 3 };
            state.wood += gained;
            state.add_log(format!("狼を倒した。木材+{gained}"));
        }
        Terrain::Mountain => {
            state.stone += 4;
            state.add_log("ゴーレムを倒した。石材+4");
        }
        Terrain::Graveyard => {
            state.soul += 2;
            state.add_log("スケルトンを倒した。魂+2");
        }
        Terrain::Meadow => {}
    }
}

fn advance_hero(state: &mut LoopMarchState) {
    let n = state.path.len();
    state.hero.position = (state.hero.position + 1) % n;
    if state.hero.position == 0 {
        state.lap += 1;
        if state.lap > state.best_lap {
            state.best_lap = state.lap;
        }
    }
    arrive_at_tile(state);
}

fn arrive_at_tile(state: &mut LoopMarchState) {
    let pos = state.hero.position;
    let terrain = match state.path[pos].terrain {
        Some(t) => t,
        None => return,
    };
    if state.path[pos].monster.is_some() {
        return;
    }

    match terrain {
        Terrain::Meadow => {
            state.soul += 1;
        }
        Terrain::Forest | Terrain::Mountain | Terrain::Graveyard => {
            let chance = terrain.spawn_chance_per_mille();
            if rng_below(&mut state.rng_state, 1000) < chance {
                spawn_monster(state, pos, terrain);
            }
        }
    }
}

fn spawn_monster(state: &mut LoopMarchState, pos: usize, terrain: Terrain) {
    let difficulty = 1.0 + state.lap as f64 * DIFFICULTY_PER_LAP;
    let elite = terrain == Terrain::Forest
        && forest_cluster_size(&state.path, pos) >= 2
        && rng_below(&mut state.rng_state, 1000) < 500;

    // 初期HP/攻撃力の勇者でも、初回の手札補充(木材3+石材3)成立前に
    // 死んでしまう確率が低くなるよう調整した値 (simulator.rs の
    // 統計テストで検証)。
    let (base_hp, base_atk) = match (terrain, elite) {
        (Terrain::Forest, false) => (6, 1),
        (Terrain::Forest, true) => (10, 2),
        (Terrain::Mountain, _) => (11, 2),
        (Terrain::Graveyard, _) => (4, 1),
        (Terrain::Meadow, _) => unreachable!("Meadow は arrive_at_tile で別処理される"),
    };
    let hp = ((base_hp as f64) * difficulty).round().max(1.0) as i32;
    let attack = ((base_atk as f64) * difficulty).round().max(1.0) as i32;

    state.path[pos].monster = Some(Monster {
        terrain,
        hp,
        max_hp: hp,
        attack,
        elite,
    });
    if elite {
        state.add_log("森の奥で唸り声がした…気のせいだろうか");
    }
}

/// 勇者が力尽きた時の処理。ローグライトの非対称性が核:
/// 遠征スコープ (道の配置・木材・石材) は失うが、永続資源 (魂) と
/// 拠点強化は残る。
fn handle_death(state: &mut LoopMarchState) {
    state.add_log(format!(
        "力尽きた…拠点へ撤退する。(第{}周まで到達)",
        state.lap + 1
    ));
    // 「魂だけが残る」非対称性を結論として言わず、失った量と残った量を
    // 数字でそのまま並べて見せる — プレイヤー自身に気付いてもらう。
    state.add_log(format!(
        "木材 {}→0 / 石材 {}→0 / 魂 {} (そのまま)",
        state.wood, state.stone, state.soul
    ));

    state.phase = Phase::Camp;
    state.run_active = false;
    state.path = vec![PathSlot::default(); PATH_LEN];
    state.wood = 0;
    state.stone = 0;
    state.lap = 0;
    state.move_progress = 0;
    state.selected_hand = None;
    state.cursor = 0;
    state.hero = state.camp.fresh_hero();
    state.hand = vec![None; HAND_MAX];
    // 次の遠征に前回の残りフラッシュが漏れて見えないようリセットする。
    state.hero_hurt_flash = FlashTimer::new();
    state.enemy_hurt_flash = FlashTimer::new();
}

/// 拠点から遠征に出発 (または再開) する。
///
/// `run_active` が false (新規開始 or 死亡直後) の場合のみ遠征スコープを
/// 全リセットする。true (道中に拠点を覗きに来ただけ) の場合は表示を
/// 遠征画面に戻すだけで、道の配置・勇者HP等はそのまま維持される。
pub fn start_or_resume_expedition(state: &mut LoopMarchState) {
    if state.run_active {
        state.phase = Phase::Expedition;
        return;
    }

    state.run_active = true;
    state.phase = Phase::Expedition;
    state.path = vec![PathSlot::default(); PATH_LEN];
    state.wood = 0;
    state.stone = 0;
    state.lap = 0;
    state.move_progress = 0;
    state.cursor = 0;
    state.hero = state.camp.fresh_hero();
    state.hero_hurt_flash = FlashTimer::new();
    state.enemy_hurt_flash = FlashTimer::new();

    state.hand = draw_starting_hand(state.camp.starting_hand_size(), &mut state.rng_state);
    state.selected_hand = None;
    state.add_log("遠征開始！");
}

/// 遠征中に拠点画面へ切り替える (状態は変更しない、表示の切り替えのみ)。
pub fn go_to_camp(state: &mut LoopMarchState) {
    if state.phase == Phase::Expedition {
        state.phase = Phase::Camp;
    }
}

/// キーボード操作用の道カーソルを前後に動かす (ループするので端は無い)。
pub fn move_cursor(state: &mut LoopMarchState, delta: i32) {
    let n = state.path.len() as i32;
    let next = (state.cursor as i32 + delta).rem_euclid(n);
    state.cursor = next as usize;
}

/// 手札のカードを選択/選択解除する。
pub fn select_hand(state: &mut LoopMarchState, index: usize) {
    if index >= state.hand.len() || state.hand[index].is_none() {
        return;
    }
    state.selected_hand = if state.selected_hand == Some(index) {
        None
    } else {
        Some(index)
    };
}

/// 選択中の手札カードを道の `path_index` に配置する。
pub fn place_selected(state: &mut LoopMarchState, path_index: usize) -> bool {
    if state.phase != Phase::Expedition || path_index >= state.path.len() {
        return false;
    }
    let hand_index = match state.selected_hand {
        Some(i) => i,
        None => return false,
    };
    let terrain = match state.hand.get(hand_index).copied().flatten() {
        Some(t) => t,
        None => return false,
    };
    if state.path[path_index].terrain.is_some() {
        state.add_log("そこには既に地形がある");
        return false;
    }

    state.path[path_index].terrain = Some(terrain);
    state.hand[hand_index] = None;
    state.selected_hand = None;
    state.add_log(format!("{}を配置した", terrain.name()));
    true
}

/// 木材・石材を消費して手札にランダムな地形カードを1枚補充する。
pub fn refill_hand(state: &mut LoopMarchState) -> bool {
    if state.phase != Phase::Expedition {
        return false;
    }
    let empty_slot = match state.hand.iter().position(|c| c.is_none()) {
        Some(i) => i,
        None => {
            state.add_log("手札はいっぱいだ");
            return false;
        }
    };
    if state.wood < REFILL_WOOD_COST || state.stone < REFILL_STONE_COST {
        state.add_log("資源が足りない (木材/石材が必要)");
        return false;
    }
    state.wood -= REFILL_WOOD_COST;
    state.stone -= REFILL_STONE_COST;
    let terrain = random_terrain(&mut state.rng_state);
    state.hand[empty_slot] = Some(terrain);
    state.add_log(format!("{}のカードを補充した", terrain.name()));
    true
}

/// 拠点で購入できる恒久強化の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    MaxHp,
    Attack,
    ExtraCard,
}

/// 魂を消費して拠点強化を購入する。遠征中の勇者にも即座に反映される
/// (次の遠征開始時に camp の値から再計算されるので二重反映にはならない)。
/// 最大HP強化は現在HPも同じ量だけ回復する — 拠点訪問がついでの回復
/// 手段にもなる仕様として意図的にそうしている。
pub fn purchase_upgrade(state: &mut LoopMarchState, kind: UpgradeKind) -> bool {
    match kind {
        UpgradeKind::MaxHp => {
            let cost = state.camp.max_hp_cost();
            if state.soul < cost {
                state.add_log("魂が足りない");
                return false;
            }
            state.soul -= cost;
            state.camp.max_hp_level += 1;
            state.hero.max_hp += HP_PER_LEVEL;
            state.hero.hp += HP_PER_LEVEL;
            state.add_log("最大HPを強化した");
            true
        }
        UpgradeKind::Attack => {
            let cost = state.camp.attack_cost();
            if state.soul < cost {
                state.add_log("魂が足りない");
                return false;
            }
            state.soul -= cost;
            state.camp.attack_level += 1;
            state.hero.attack += ATTACK_PER_LEVEL;
            state.add_log("攻撃力を強化した");
            true
        }
        UpgradeKind::ExtraCard => {
            if state.camp.extra_card_level >= 1 {
                state.add_log("既に習得済み");
                return false;
            }
            if state.soul < CampUpgrades::EXTRA_CARD_COST {
                state.add_log("魂が足りない");
                return false;
            }
            state.soul -= CampUpgrades::EXTRA_CARD_COST;
            state.camp.extra_card_level = 1;
            state.add_log("初期手札+1を習得した");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::{BASE_ATTACK, BASE_MAX_HP};

    fn expedition_state() -> LoopMarchState {
        let mut s = LoopMarchState::new();
        start_or_resume_expedition(&mut s);
        s
    }

    // ── リング座標 ──

    #[test]
    fn ring_positions_len_matches_path_len() {
        assert_eq!(ring_positions().len(), PATH_LEN);
    }

    #[test]
    fn ring_positions_are_unique() {
        let positions = ring_positions();
        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                assert_ne!(positions[i], positions[j], "重複座標: index {i} と {j}");
            }
        }
    }

    #[test]
    fn ring_index_at_roundtrip() {
        let positions = ring_positions();
        for (idx, &(gx, gy)) in positions.iter().enumerate() {
            assert_eq!(ring_index_at(gx, gy), Some(idx));
        }
    }

    #[test]
    fn ring_index_at_interior_cell_is_none() {
        // (1,1) は RING_W=8, RING_H=4 の矩形の内部 (リング上ではない)。
        assert_eq!(ring_index_at(1, 1), None);
    }

    // ── シナジー ──

    #[test]
    fn mountain_synergy_defense_zero_when_no_mountains() {
        let path = vec![PathSlot::default(); PATH_LEN];
        assert_eq!(mountain_synergy_defense(&path), 0);
    }

    #[test]
    fn mountain_synergy_defense_counts_adjacent_pairs() {
        let mut path = vec![PathSlot::default(); PATH_LEN];
        path[0].terrain = Some(Terrain::Mountain);
        path[1].terrain = Some(Terrain::Mountain);
        path[2].terrain = Some(Terrain::Mountain);
        // (0,1) と (1,2) の2ペア。(2,3)は3が岩山でないので不成立。
        assert_eq!(mountain_synergy_defense(&path), 2);
    }

    #[test]
    fn mountain_synergy_defense_wraps_around_loop() {
        let mut path = vec![PathSlot::default(); PATH_LEN];
        let last = PATH_LEN - 1;
        path[last].terrain = Some(Terrain::Mountain);
        path[0].terrain = Some(Terrain::Mountain);
        assert_eq!(mountain_synergy_defense(&path), 1);
    }

    #[test]
    fn forest_cluster_size_single_tile() {
        let mut path = vec![PathSlot::default(); PATH_LEN];
        path[5].terrain = Some(Terrain::Forest);
        assert_eq!(forest_cluster_size(&path, 5), 1);
    }

    #[test]
    fn forest_cluster_size_counts_contiguous_run() {
        let mut path = vec![PathSlot::default(); PATH_LEN];
        path[5].terrain = Some(Terrain::Forest);
        path[6].terrain = Some(Terrain::Forest);
        path[7].terrain = Some(Terrain::Forest);
        assert_eq!(forest_cluster_size(&path, 6), 3);
    }

    #[test]
    fn forest_cluster_size_full_ring_does_not_double_count() {
        // 全マス森だと前方探索・後方探索が同じ他マスを両方踏破するため、
        // 単純合算では PATH_LEN を超えてしまう (2*PATH_LEN-1)。
        let path = vec![
            PathSlot { terrain: Some(Terrain::Forest), monster: None };
            PATH_LEN
        ];
        assert_eq!(forest_cluster_size(&path, 0), PATH_LEN);
    }

    #[test]
    fn forest_cluster_size_non_forest_tile_is_zero() {
        let path = vec![PathSlot::default(); PATH_LEN];
        assert_eq!(forest_cluster_size(&path, 0), 0);
    }

    // ── 湧き判定 (シナジーの end-to-end 検証) ──

    /// 勇者を `target` の1マス手前・移動直前まで進める。次の `tick` 1回で
    /// `target` に到着し、地形の湧き判定 (arrive_at_tile) が走る。
    fn place_hero_just_before(s: &mut LoopMarchState, target: usize) {
        s.hero.position = (target + PATH_LEN - 1) % PATH_LEN;
        s.move_progress = MOVE_TICKS - 1;
    }

    #[test]
    fn forest_cluster_synergy_can_spawn_elite_over_many_attempts() {
        // 隣接森3タイル (シナジー成立条件) で十分な回数試行すれば、
        // 少なくとも1回は elite が湧くはず (湧き55% × elite50%)。
        let mut elite_seen = false;
        for seed in 1..500u32 {
            let mut s = expedition_state();
            s.rng_state = seed;
            s.path[0].terrain = Some(Terrain::Forest);
            s.path[1].terrain = Some(Terrain::Forest);
            s.path[2].terrain = Some(Terrain::Forest);
            place_hero_just_before(&mut s, 0);
            tick(&mut s);
            if let Some(m) = &s.path[0].monster {
                if m.elite {
                    elite_seen = true;
                    break;
                }
            }
        }
        assert!(elite_seen, "森クラスタ内では複数回試行すればeliteが出るはず");
    }

    #[test]
    fn isolated_forest_tile_never_spawns_elite() {
        // 隣接森が無い (クラスタサイズ1) 森タイルからは、何度試行しても
        // elite は絶対に湧かない — シナジーの成立条件そのものの保証。
        for seed in 1..500u32 {
            let mut s = expedition_state();
            s.rng_state = seed;
            s.path[0].terrain = Some(Terrain::Forest);
            place_hero_just_before(&mut s, 0);
            tick(&mut s);
            if let Some(m) = &s.path[0].monster {
                assert!(!m.elite, "隣接森が無いのにeliteが湧いた (seed={seed})");
            }
        }
    }

    #[test]
    fn arriving_at_meadow_grants_soul() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Meadow);
        place_hero_just_before(&mut s, 0);
        let soul_before = s.soul;
        tick(&mut s);
        assert_eq!(s.soul, soul_before + 1);
    }

    // ── 移動・戦闘 ──

    #[test]
    fn hero_advances_after_move_ticks() {
        let mut s = expedition_state();
        assert_eq!(s.hero.position, 0);
        tick_n(&mut s, MOVE_TICKS - 1);
        assert_eq!(s.hero.position, 0, "MOVE_TICKS未満ではまだ移動しない");
        tick_n(&mut s, 1);
        assert_eq!(s.hero.position, 1);
    }

    #[test]
    fn lap_increments_on_full_loop() {
        let mut s = expedition_state();
        assert_eq!(s.lap, 0);
        tick_n(&mut s, MOVE_TICKS * PATH_LEN as u32);
        assert_eq!(s.lap, 1);
    }

    #[test]
    fn combat_defeats_monster_and_grants_wood() {
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100; // 即死させて戦闘フローだけ検証
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: false,
        });
        let wood_before = s.wood;
        tick(&mut s);
        assert!(s.path[3].monster.is_none());
        assert_eq!(s.wood, wood_before + 3);
    }

    #[test]
    fn combat_elite_forest_monster_grants_more_wood() {
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: true,
        });
        let wood_before = s.wood;
        tick(&mut s);
        assert_eq!(s.wood, wood_before + 6);
    }

    #[test]
    fn monster_attack_reduced_by_mountain_synergy_defense() {
        let mut s = expedition_state();
        s.path[10].terrain = Some(Terrain::Mountain);
        s.path[11].terrain = Some(Terrain::Mountain); // defense = 1
        s.hero.position = 3;
        s.hero.attack = 0; // 勇者は倒せない → 反撃を繰り返し受ける
        s.hero.hp = 100;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 3,
            elite: false,
        });
        let hp_before = s.hero.hp;
        tick(&mut s);
        // defense=1 なので dmg = max(1, 3-1) = 2
        assert_eq!(s.hero.hp, hp_before - 2);
    }

    #[test]
    fn monster_damage_never_below_one() {
        let mut s = expedition_state();
        // 大量の岩山ペアで defense を敵の攻撃力より大きくする
        for i in 0..10 {
            s.path[i].terrain = Some(Terrain::Mountain);
        }
        s.hero.position = 15;
        s.hero.attack = 0;
        s.hero.hp = 100;
        s.path[15].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 1,
            elite: false,
        });
        let hp_before = s.hero.hp;
        tick(&mut s);
        assert_eq!(s.hero.hp, hp_before - 1);
    }

    #[test]
    fn combat_tick_triggers_hero_and_enemy_hurt_flash_when_monster_survives() {
        let mut s = expedition_state();
        let pos = s.hero.position;
        s.path[pos].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 3,
            elite: false,
        });
        assert!(!s.hero_hurt_flash.is_active());
        assert!(!s.enemy_hurt_flash.is_active());

        tick(&mut s);

        assert!(s.hero_hurt_flash.is_active(), "モンスターの反撃で勇者側のフラッシュが立つはず");
        assert!(s.enemy_hurt_flash.is_active(), "勇者の攻撃でモンスター側のフラッシュが立つはず");
    }

    #[test]
    fn defeating_monster_triggers_enemy_hurt_flash_but_not_hero_hurt_flash() {
        let mut s = expedition_state();
        let pos = s.hero.position;
        s.hero.attack = 100;
        s.path[pos].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 1,
            max_hp: 1,
            attack: 3,
            elite: false,
        });

        tick(&mut s);

        assert!(s.enemy_hurt_flash.is_active(), "撃破の一撃でもフラッシュは立つ");
        assert!(!s.hero_hurt_flash.is_active(), "倒した瞬間は反撃を受けないので勇者側は立たない");
    }

    #[test]
    fn hurt_flash_decays_over_ticks() {
        let mut s = expedition_state();
        s.hero_hurt_flash.trigger(2);
        tick(&mut s);
        assert!(s.hero_hurt_flash.is_active());
        tick(&mut s);
        assert!(!s.hero_hurt_flash.is_active());
    }

    #[test]
    fn death_resets_run_scope_but_keeps_soul_and_camp_upgrades() {
        let mut s = expedition_state();
        s.soul = 50;
        s.camp.max_hp_level = 2;
        s.wood = 10;
        s.stone = 10;
        s.path[0].terrain = Some(Terrain::Forest);
        s.hero.position = 3;
        s.hero.attack = 0;
        s.hero.hp = 1;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 50,
            elite: false,
        });

        tick(&mut s);

        assert_eq!(s.phase, Phase::Camp);
        assert!(!s.run_active);
        assert_eq!(s.soul, 50, "魂は死亡しても残る");
        assert_eq!(s.camp.max_hp_level, 2, "拠点強化は死亡しても残る");
        assert_eq!(s.wood, 0, "木材は死亡でリセットされる");
        assert_eq!(s.stone, 0, "石材は死亡でリセットされる");
        assert!(
            s.path.iter().all(|slot| slot.terrain.is_none()),
            "道の配置は死亡でリセットされる"
        );
        assert_eq!(s.hero.hp, s.camp.hero_max_hp());
    }

    #[test]
    fn death_resets_hurt_flash() {
        let mut s = expedition_state();
        s.hero.attack = 0;
        s.hero.hp = 1;
        let pos = s.hero.position;
        s.path[pos].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 50,
            elite: false,
        });

        tick(&mut s);

        assert_eq!(s.phase, Phase::Camp, "この tick で死亡しているはず");
        assert!(!s.hero_hurt_flash.is_active(), "死亡直後の演出フラグは持ち越さない");
        assert!(!s.enemy_hurt_flash.is_active());
    }

    #[test]
    fn death_logs_lost_and_kept_resources_explicitly() {
        let mut s = expedition_state();
        s.wood = 7;
        s.stone = 4;
        s.soul = 12;
        s.hero.position = 3;
        s.hero.attack = 0;
        s.hero.hp = 1;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 50,
            elite: false,
        });

        tick(&mut s);

        let recap = s.log.iter().find(|l| l.contains("7→0"));
        assert!(
            recap.is_some(),
            "死亡時に失った資源/残った魂を数字で明示していない: {:?}",
            s.log
        );
        assert!(recap.unwrap().contains("12"), "残った魂の量が見えていない");
    }

    // ── 遠征の開始/再開 ──

    #[test]
    fn start_expedition_gives_starting_hand() {
        let s = expedition_state();
        let filled = s.hand.iter().filter(|c| c.is_some()).count();
        assert_eq!(filled, 3);
    }

    #[test]
    fn starting_hand_always_guarantees_wood_and_stone_sources() {
        // 森(木材源)/岩山(石材源)のどちらかが初期手札に無いと、両方を
        // 要求する refill_hand が永久に成立しない詰み状態になり得る。
        // 何度出発し直しても必ず両方が手札に含まれることを保証する。
        for seed in 1..200u32 {
            let mut s = LoopMarchState::new();
            s.rng_state = seed;
            start_or_resume_expedition(&mut s);
            let has_forest = s.hand.contains(&Some(Terrain::Forest));
            let has_mountain = s.hand.contains(&Some(Terrain::Mountain));
            assert!(has_forest, "seed={seed}: 初期手札に森が無い");
            assert!(has_mountain, "seed={seed}: 初期手札に岩山が無い");
        }
    }

    #[test]
    fn extra_card_upgrade_increases_starting_hand() {
        let mut s = LoopMarchState::new();
        s.camp.extra_card_level = 1;
        start_or_resume_expedition(&mut s);
        let filled = s.hand.iter().filter(|c| c.is_some()).count();
        assert_eq!(filled, 4);
    }

    #[test]
    fn resume_does_not_reset_active_run() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Forest);
        s.hero.position = 5;
        go_to_camp(&mut s);
        assert_eq!(s.phase, Phase::Camp);
        start_or_resume_expedition(&mut s);
        assert_eq!(s.phase, Phase::Expedition);
        assert_eq!(s.hero.position, 5, "再開では進行状況が保持される");
        assert_eq!(s.path[0].terrain, Some(Terrain::Forest));
    }

    #[test]
    fn go_to_camp_noop_when_already_in_camp() {
        let mut s = LoopMarchState::new();
        go_to_camp(&mut s);
        assert_eq!(s.phase, Phase::Camp);
    }

    // ── 配置 / 補充 ──

    #[test]
    fn place_selected_consumes_hand_card() {
        let mut s = expedition_state();
        s.hand[0] = Some(Terrain::Forest);
        s.selected_hand = Some(0);
        assert!(place_selected(&mut s, 5));
        assert_eq!(s.path[5].terrain, Some(Terrain::Forest));
        assert_eq!(s.hand[0], None);
        assert_eq!(s.selected_hand, None);
    }

    #[test]
    fn place_selected_fails_on_occupied_tile() {
        let mut s = expedition_state();
        s.path[5].terrain = Some(Terrain::Meadow);
        s.hand[0] = Some(Terrain::Forest);
        s.selected_hand = Some(0);
        assert!(!place_selected(&mut s, 5));
        assert_eq!(s.hand[0], Some(Terrain::Forest), "失敗時は手札が消費されない");
    }

    #[test]
    fn place_selected_fails_without_selection() {
        let mut s = expedition_state();
        assert!(!place_selected(&mut s, 5));
    }

    #[test]
    fn select_hand_toggles_selection() {
        let mut s = expedition_state();
        s.hand[0] = Some(Terrain::Forest);
        select_hand(&mut s, 0);
        assert_eq!(s.selected_hand, Some(0));
        select_hand(&mut s, 0);
        assert_eq!(s.selected_hand, None, "同じカードを再選択すると解除される");
    }

    #[test]
    fn select_hand_ignores_empty_slot() {
        let mut s = expedition_state();
        s.hand[0] = None;
        select_hand(&mut s, 0);
        assert_eq!(s.selected_hand, None);
    }

    // ── キーボードカーソル ──

    #[test]
    fn move_cursor_advances_and_wraps_forward() {
        let mut s = expedition_state();
        s.cursor = PATH_LEN - 1;
        move_cursor(&mut s, 1);
        assert_eq!(s.cursor, 0, "末尾から進むと先頭に戻る (ループ)");
    }

    #[test]
    fn move_cursor_wraps_backward() {
        let mut s = expedition_state();
        s.cursor = 0;
        move_cursor(&mut s, -1);
        assert_eq!(s.cursor, PATH_LEN - 1, "先頭から戻ると末尾に回る (ループ)");
    }

    #[test]
    fn place_selected_at_cursor_works_like_click_placement() {
        let mut s = expedition_state();
        s.hand[0] = Some(Terrain::Forest);
        s.selected_hand = Some(0);
        s.cursor = 7;
        let cursor = s.cursor;
        assert!(place_selected(&mut s, cursor));
        assert_eq!(s.path[7].terrain, Some(Terrain::Forest));
    }

    #[test]
    fn refill_hand_consumes_resources_and_fills_empty_slot() {
        let mut s = expedition_state();
        s.hand = vec![None; HAND_MAX];
        s.wood = 10;
        s.stone = 10;
        assert!(refill_hand(&mut s));
        assert_eq!(s.wood, 10 - REFILL_WOOD_COST);
        assert_eq!(s.stone, 10 - REFILL_STONE_COST);
        assert!(s.hand.iter().any(|c| c.is_some()));
    }

    #[test]
    fn refill_hand_fails_without_resources() {
        let mut s = expedition_state();
        s.hand = vec![None; HAND_MAX];
        s.wood = 0;
        s.stone = 0;
        assert!(!refill_hand(&mut s));
    }

    #[test]
    fn refill_hand_fails_when_hand_full() {
        let mut s = expedition_state();
        s.wood = 100;
        s.stone = 100;
        for slot in s.hand.iter_mut() {
            *slot = Some(Terrain::Meadow);
        }
        assert!(!refill_hand(&mut s));
    }

    // ── 拠点強化 ──

    #[test]
    fn purchase_max_hp_upgrade_boosts_hero_immediately() {
        let mut s = LoopMarchState::new();
        s.soul = 100;
        let hp_before = s.hero.max_hp;
        assert!(purchase_upgrade(&mut s, UpgradeKind::MaxHp));
        assert_eq!(s.hero.max_hp, hp_before + HP_PER_LEVEL);
        assert_eq!(s.camp.max_hp_level, 1);
    }

    #[test]
    fn purchase_upgrade_fails_without_enough_soul() {
        let mut s = LoopMarchState::new();
        s.soul = 0;
        assert!(!purchase_upgrade(&mut s, UpgradeKind::MaxHp));
        assert_eq!(s.camp.max_hp_level, 0);
    }

    #[test]
    fn purchase_extra_card_is_one_time_only() {
        let mut s = LoopMarchState::new();
        s.soul = 100;
        assert!(purchase_upgrade(&mut s, UpgradeKind::ExtraCard));
        assert!(!purchase_upgrade(&mut s, UpgradeKind::ExtraCard));
        assert_eq!(s.camp.extra_card_level, 1);
    }

    #[test]
    fn next_expedition_reflects_purchased_upgrades() {
        let mut s = LoopMarchState::new();
        s.soul = 100;
        purchase_upgrade(&mut s, UpgradeKind::MaxHp);
        purchase_upgrade(&mut s, UpgradeKind::Attack);
        start_or_resume_expedition(&mut s);
        assert_eq!(s.hero.max_hp, BASE_MAX_HP + HP_PER_LEVEL);
        assert_eq!(s.hero.attack, BASE_ATTACK + ATTACK_PER_LEVEL);
    }
}
