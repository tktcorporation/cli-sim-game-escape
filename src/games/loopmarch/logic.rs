//! 周回討伐 — ゲームロジック。純粋関数のみ、フルテスト可能。

use crate::effects::FlashTimer;

use super::state::{
    CampUpgrades, LoopMarchState, Monster, Phase, PathSlot, Terrain, ATTACK_PER_LEVEL, HAND_MAX,
    HP_PER_LEVEL, MOVE_TICKS, PATH_LEN, REFILL_STONE_COST, REFILL_WOOD_COST, RING_H, RING_W,
    TERRAIN_TIER_MAX,
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

/// `index` を含む、指定地形が連続しているタイル数 (ループの前後をたどって
/// 数える)。`render.rs` がシナジー成立(≥2)の視覚ヒントを出す判定や、
/// 森/墓地/草原それぞれのクラスターシナジー判定に使う。
pub fn cluster_size(path: &[PathSlot], index: usize, terrain: Terrain) -> usize {
    let n = path.len();
    if path[index].terrain != Some(terrain) {
        return 0;
    }
    let mut size = 1;
    let mut i = (index + 1) % n;
    while i != index && path[i].terrain == Some(terrain) {
        size += 1;
        i = (i + 1) % n;
    }
    let mut i = (index + n - 1) % n;
    while i != index && path[i].terrain == Some(terrain) {
        size += 1;
        i = (i + n - 1) % n;
    }
    // リング全周が同地形だと前方・後方の両ループが同じ他マスを踏破し、
    // 二重計上されて n を超える。ループ上に存在するタイル数を超えない。
    size.min(n)
}

/// `index` を含む森タイルの連続数。
pub fn forest_cluster_size(path: &[PathSlot], index: usize) -> usize {
    cluster_size(path, index, Terrain::Forest)
}

/// 地形強化tierによる報酬・敵性能の倍率 (tier0=1.0倍, tier1=1.5倍, tier2=2.0倍)。
pub fn tier_multiplier(tier: u32) -> f64 {
    1.0 + tier as f64 * 0.5
}

/// 墓地クラスターによる討伐報酬の追加分 (孤立=+0, 2隣接=+1, 3隣接=+2, 4隣接以上=+3)。
/// 森のクラスターが「eliteの出現確率」という確率的な強化になっているのに対し、
/// こちらは確定でスケールする加算量とすることで、地形ごとにシナジーの手触りを変えている。
pub fn graveyard_cluster_bonus(cluster: usize) -> u32 {
    cluster.saturating_sub(1).min(3) as u32
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
    decay_damage_display(&mut state.last_hero_damage);
    decay_damage_display(&mut state.last_enemy_damage);
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

/// 被ダメージ表示 (`last_hero_damage`/`last_enemy_damage`) の残りtickを
/// 1減らし、尽きたら`None`に戻す。ヘッダーの「-N」表示専用のライフサイクルで、
/// `hero_hurt_flash`/`enemy_hurt_flash` の点滅とは独立に管理する — 表示秒数の
/// チューニングを演出側だけで完結させるため。
fn decay_damage_display(display: &mut Option<(i32, u32)>) {
    if let Some((_, life)) = display {
        *life = life.saturating_sub(1);
    }
    if matches!(display, Some((_, 0))) {
        *display = None;
    }
}

/// 被ダメージ表示を何tick残すか。数値がすぐ消えるとダメージ量を読み取れない
/// ため、被弾フラッシュ (3tick) より長めに表示を残す。
const DAMAGE_DISPLAY_TICKS: u32 = 6;

/// 勇者が現在マスのモンスターと1 tick分の攻防を行う。
fn resolve_combat_tick(state: &mut LoopMarchState) {
    let pos = state.hero.position;
    let hero_atk = state.hero.attack;

    let defeated_reward = match state.path[pos].monster.as_mut() {
        Some(monster) => {
            let hp_before_hit = monster.hp;
            monster.hp -= hero_atk;
            state.enemy_hurt_flash.trigger(3);
            state.enemy_hit_count = state.enemy_hit_count.wrapping_add(1);
            // とどめの一撃はオーバーキル分を含むため、表示 (ヘッダー・ログ共通)
            // には実際に削れた量 (残りHPを超えない分) だけを見せる。
            let actual_damage = hero_atk.min(hp_before_hit);
            state.last_enemy_damage = Some((actual_damage, DAMAGE_DISPLAY_TICKS));
            if monster.hp <= 0 {
                Some((monster.terrain, monster.elite, monster.tier, monster.cluster_bonus, actual_damage))
            } else {
                None
            }
        }
        None => return,
    };

    if let Some((terrain, elite, tier, cluster_bonus, final_hit)) = defeated_reward {
        state.path[pos].monster = None;
        grant_kill_reward(state, terrain, elite, tier, cluster_bonus, final_hit);
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
    state.last_hero_damage = Some((dmg, DAMAGE_DISPLAY_TICKS));
    if state.hero.hp <= 0 {
        state.hero.hp = 0;
        handle_death(state);
    }
}

/// `tier`/`cluster_bonus` は討伐したモンスター自身が湧いた瞬間に固定された
/// 値 (`Monster::tier`/`Monster::cluster_bonus`) — 討伐時点のタイル/盤面を
/// 読み直さない。生存中にタイルを強化したり隣接に地形を足したりしても、
/// 「その時に湧いた個体」の脅威度は変わっていないため、報酬だけをリスク無しに
/// 釣り上げられてしまう抜け穴を防ぐ。
///
/// `final_hit` は最後の一撃で実際に削れたHP量 (オーバーキル分は含まない)。
/// 報酬計算には使わず、ログでの表示のみに使う。
fn grant_kill_reward(
    state: &mut LoopMarchState,
    terrain: Terrain,
    elite: bool,
    tier: u32,
    cluster_bonus: u32,
    final_hit: i32,
) {
    let mult = tier_multiplier(tier);
    match terrain {
        Terrain::Forest => {
            let base = if elite { 6 } else { 3 };
            let gained = ((base as f64) * mult).round() as u32;
            state.wood += gained;
            state.add_log(format!("狼を倒した (最後の一撃-{final_hit})。木材+{gained}"));
        }
        Terrain::Mountain => {
            let gained = (4.0 * mult).round() as u32;
            state.stone += gained;
            state.add_log(format!("ゴーレムを倒した (最後の一撃-{final_hit})。石材+{gained}"));
        }
        Terrain::Graveyard => {
            let base = 2 + cluster_bonus;
            let gained = ((base as f64) * mult).round() as u32;
            state.soul += gained;
            state.add_log(format!("スケルトンを倒した (最後の一撃-{final_hit})。魂+{gained}"));
        }
        Terrain::Meadow => {}
    }
}

fn advance_hero(state: &mut LoopMarchState) {
    let n = state.path.len();
    state.hero.position = (state.hero.position + 1) % n;
    if state.hero.position == 0 {
        log_lap_summary(state);
        state.lap += 1;
        if state.lap > state.best_lap {
            state.best_lap = state.lap;
        }
        reset_lap_snapshot(state);
    }
    arrive_at_tile(state);
}

/// 拠点画面の推移グラフに表示する履歴の最大件数。古いラップの分は捨てる。
const SOUL_HISTORY_CAP: usize = 30;

/// 直前に完了したラップで増減した資源をログに出す。ラップ開始時点との
/// 差分を見せることで「今の1周でどれだけ稼げたか」を数字で振り返れるようにする。
fn log_lap_summary(state: &mut LoopMarchState) {
    let wood_delta = state.wood as i64 - state.lap_start_wood as i64;
    let stone_delta = state.stone as i64 - state.lap_start_stone as i64;
    let soul_delta = state.soul as i64 - state.lap_start_soul as i64;
    state.add_log(format!(
        "第{}周 完了！ 木材{} 石材{} 魂{}",
        state.lap + 1,
        signed(wood_delta),
        signed(stone_delta),
        signed(soul_delta),
    ));

    if state.soul_history.len() >= SOUL_HISTORY_CAP {
        state.soul_history.remove(0);
    }
    state.soul_history.push(state.soul);
}

fn signed(delta: i64) -> String {
    if delta >= 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn reset_lap_snapshot(state: &mut LoopMarchState) {
    state.lap_start_wood = state.wood;
    state.lap_start_stone = state.stone;
    state.lap_start_soul = state.soul;
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
            // 他地形は `tier_multiplier` (1.0/1.5/2.0倍) で報酬を倍率スケール
            // するが、草原の基礎収入は1と小さく倍率にすると tier1/tier2 が
            // 丸めで同じ値(共に2)になり強化の意味が消える。草原だけは
            // tierをそのまま加算 (1/2/3) して段階ごとの差を保っている。
            state.soul += 1 + state.path[pos].tier;
            // クラスターは森と同様、演出のみで理由を明示しない
            // (発見の余地として残す)。安全地帯を繋げるほど回復量が増える。
            let cluster = cluster_size(&state.path, pos, Terrain::Meadow);
            if cluster >= 2 && state.hero.hp < state.hero.max_hp {
                let heal = (cluster as i32 - 1).min(3);
                state.hero.hp = (state.hero.hp + heal).min(state.hero.max_hp);
            }
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
    let tier = state.path[pos].tier;
    let tier_mult = tier_multiplier(tier);
    let elite = terrain == Terrain::Forest
        && forest_cluster_size(&state.path, pos) >= 2
        && rng_below(&mut state.rng_state, 1000) < 500;
    // 討伐報酬のクラスターボーナスは湧いた瞬間の盤面で固定する。生存中に
    // タイルの重ね置き/隣接タイルの追加が起きても、この個体自身の強さは
    // 変わっていないため報酬もこの時点の値のまま変えない
    // (grant_kill_reward 側のコメント参照)。
    let cluster_bonus = if terrain == Terrain::Graveyard {
        graveyard_cluster_bonus(cluster_size(&state.path, pos, Terrain::Graveyard))
    } else {
        0
    };

    // 初期HP/攻撃力の勇者でも、初回の手札補充(木材3+石材3)成立前に
    // 死んでしまう確率が低くなるよう調整した値 (simulator.rs の
    // 統計テストで検証)。地形強化tierは湧く頻度は変えず、湧いた1体の
    // 強さと討伐報酬だけを底上げする (リスクとリターンを両方引き上げる)。
    let (base_hp, base_atk) = match (terrain, elite) {
        (Terrain::Forest, false) => (6, 1),
        (Terrain::Forest, true) => (10, 2),
        (Terrain::Mountain, _) => (11, 2),
        (Terrain::Graveyard, _) => (4, 1),
        (Terrain::Meadow, _) => unreachable!("Meadow は arrive_at_tile で別処理される"),
    };
    let hp = ((base_hp as f64) * difficulty * tier_mult).round().max(1.0) as i32;
    let attack = ((base_atk as f64) * difficulty * tier_mult).round().max(1.0) as i32;

    state.path[pos].monster = Some(Monster {
        terrain,
        hp,
        max_hp: hp,
        attack,
        elite,
        tier,
        cluster_bonus,
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
    state.last_hero_damage = None;
    state.last_enemy_damage = None;
    reset_lap_snapshot(state);
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
    state.last_hero_damage = None;
    state.last_enemy_damage = None;
    reset_lap_snapshot(state);

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

/// 選択中の手札カードを道の `path_index` に配置する。空きマスなら新規配置、
/// 既に同じ地形が置かれているマスなら (`TERRAIN_TIER_MAX` まで) 重ね置きで
/// 強化する — 盤面が埋まった後もカードを「使い道のある投資」にし続けるため、
/// 空きマスが尽きても手札消費の判断が終わらないようにしている。
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

    match state.path[path_index].terrain {
        None => {
            state.path[path_index].terrain = Some(terrain);
            state.hand[hand_index] = None;
            state.selected_hand = None;
            state.add_log(format!("{}を配置した", terrain.name()));
            true
        }
        Some(existing) if existing == terrain => {
            if state.path[path_index].tier >= TERRAIN_TIER_MAX {
                state.add_log("これ以上は強化できない");
                false
            } else {
                state.path[path_index].tier += 1;
                let level = state.path[path_index].tier + 1;
                state.hand[hand_index] = None;
                state.selected_hand = None;
                state.add_log(format!("{}を強化した (Lv.{level})", terrain.name()));
                true
            }
        }
        Some(_) => {
            state.add_log("そこには異なる地形がある");
            false
        }
    }
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

    // ── 魂の推移履歴 ──

    #[test]
    fn completing_a_lap_appends_current_soul_to_history() {
        let mut s = expedition_state();
        s.soul = 7;
        s.hero.position = PATH_LEN - 1;

        advance_hero(&mut s);

        assert_eq!(s.soul_history, vec![7]);
    }

    #[test]
    fn soul_history_is_capped_and_drops_the_oldest_entry() {
        let mut s = expedition_state();
        s.soul_history = (0..SOUL_HISTORY_CAP as u32).collect();
        s.soul = 999;
        s.hero.position = PATH_LEN - 1;

        advance_hero(&mut s);

        assert_eq!(s.soul_history.len(), SOUL_HISTORY_CAP);
        assert_eq!(s.soul_history.first(), Some(&1), "最古の要素 (0) が捨てられているはず");
        assert_eq!(s.soul_history.last(), Some(&999));
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
            PathSlot { terrain: Some(Terrain::Forest), monster: None, tier: 0 };
            PATH_LEN
        ];
        assert_eq!(forest_cluster_size(&path, 0), PATH_LEN);
    }

    #[test]
    fn forest_cluster_size_non_forest_tile_is_zero() {
        let path = vec![PathSlot::default(); PATH_LEN];
        assert_eq!(forest_cluster_size(&path, 0), 0);
    }

    #[test]
    fn cluster_size_works_for_graveyard_and_meadow_too() {
        let mut path = vec![PathSlot::default(); PATH_LEN];
        path[5].terrain = Some(Terrain::Graveyard);
        path[6].terrain = Some(Terrain::Graveyard);
        path[10].terrain = Some(Terrain::Meadow);
        assert_eq!(cluster_size(&path, 5, Terrain::Graveyard), 2);
        assert_eq!(cluster_size(&path, 10, Terrain::Meadow), 1);
        // 隣接している地形が別種なら、その種別としてのクラスターには数えない。
        assert_eq!(cluster_size(&path, 5, Terrain::Meadow), 0);
    }

    #[test]
    fn tier_multiplier_scales_linearly_with_tier() {
        assert_eq!(tier_multiplier(0), 1.0);
        assert_eq!(tier_multiplier(1), 1.5);
        assert_eq!(tier_multiplier(2), 2.0);
    }

    #[test]
    fn graveyard_cluster_bonus_caps_at_three() {
        assert_eq!(graveyard_cluster_bonus(1), 0, "孤立した墓地はボーナス無し");
        assert_eq!(graveyard_cluster_bonus(2), 1);
        assert_eq!(graveyard_cluster_bonus(3), 2);
        assert_eq!(graveyard_cluster_bonus(4), 3);
        assert_eq!(graveyard_cluster_bonus(10), 3, "4隣接以上は頭打ち");
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

    #[test]
    fn arriving_at_tiered_meadow_grants_more_soul() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Meadow);
        s.path[0].tier = 2;
        place_hero_just_before(&mut s, 0);
        let soul_before = s.soul;
        tick(&mut s);
        assert_eq!(s.soul, soul_before + 1 + 2, "tier分だけ草原の魂収入が増えるはず");
    }

    #[test]
    fn tiered_terrain_spawns_stronger_monster() {
        // tierは湧く頻度ではなく、湧いた1体の強さ (HP/攻撃力) を上げる。
        // spawn_chance_per_mille=1000のMeadowが存在しないため、確定湧きの
        // Graveyard (700‰) は使わず、湧き判定を経由せず直接spawn_monsterの
        // 出力を比較することで乱数の影響を排除する。
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Mountain);
        spawn_monster(&mut s, 0, Terrain::Mountain);
        let base_hp = s.path[0].monster.as_ref().unwrap().hp;

        let mut s2 = expedition_state();
        s2.path[0].terrain = Some(Terrain::Mountain);
        s2.path[0].tier = 2; // 2.0倍
        spawn_monster(&mut s2, 0, Terrain::Mountain);
        let spawned = s2.path[0].monster.as_ref().unwrap();
        let tiered_hp = spawned.hp;

        assert_eq!(tiered_hp, base_hp * 2, "tier2 (2.0倍) では湧くモンスターのHPも2倍になるはず");
        assert_eq!(spawned.tier, 2, "湧いた瞬間のタイルtierがモンスター自身に固定されるはず");
    }

    #[test]
    fn graveyard_cluster_bonus_is_frozen_onto_monster_at_spawn_time() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Graveyard);
        s.path[1].terrain = Some(Terrain::Graveyard); // クラスター成立 (2隣接)
        spawn_monster(&mut s, 0, Terrain::Graveyard);
        let spawned = s.path[0].monster.as_ref().unwrap();
        assert_eq!(
            spawned.cluster_bonus, 1,
            "湧いた瞬間のクラスターボーナスがモンスター自身に固定されるはず"
        );
    }

    #[test]
    fn clustered_meadow_heals_hero_on_arrival() {
        // 隣接する2枚の草原 (クラスター) に到達すると、魂だけでなく
        // HP回復も発生する — 危険地帯を囲う「安全地帯」としての価値を持たせる。
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Meadow);
        s.path[1].terrain = Some(Terrain::Meadow);
        s.hero.hp = 1;
        s.hero.max_hp = 100;
        place_hero_just_before(&mut s, 1);
        tick(&mut s);
        assert!(s.hero.hp > 1, "隣接草原クラスターへの到達でHPが回復するはず");
    }

    #[test]
    fn isolated_meadow_does_not_heal() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Meadow);
        s.hero.hp = 1;
        s.hero.max_hp = 100;
        place_hero_just_before(&mut s, 0);
        tick(&mut s);
        assert_eq!(s.hero.hp, 1, "孤立した草原1枚では回復しないはず");
    }

    #[test]
    fn meadow_heal_never_exceeds_max_hp() {
        let mut s = expedition_state();
        s.path[0].terrain = Some(Terrain::Meadow);
        s.path[1].terrain = Some(Terrain::Meadow);
        s.path[2].terrain = Some(Terrain::Meadow);
        s.hero.hp = s.hero.max_hp; // 満タン
        place_hero_just_before(&mut s, 1);
        tick(&mut s);
        assert_eq!(s.hero.hp, s.hero.max_hp, "満タンHPを超えて回復してはいけない");
    }

    #[test]
    fn clustered_graveyard_grants_bonus_soul_on_kill() {
        // 討伐報酬はモンスター自身に湧いた瞬間固定された `cluster_bonus`
        // (spawn_monster が書き込む値) を使う — 生存中に隣接タイルを
        // 増やしても、既に湧いている個体の報酬には影響しない
        // (upgrading_tile_after_spawn_does_not_change_live_monster_reward
        // で回帰確認している)。ここではその固定値そのものによる
        // 報酬計算だけを単体で検証する。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100; // 即死させて報酬だけ検証
        s.path[3].terrain = Some(Terrain::Graveyard);
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 1,
            max_hp: 1,
            attack: 1,
            elite: false,
            tier: 0,
            cluster_bonus: 1, // cluster=2相当の固定値
        });
        let soul_before = s.soul;
        tick(&mut s);
        assert_eq!(s.soul, soul_before + 3, "base(2) + cluster_bonus(1) = 3のはず");
    }

    #[test]
    fn tiered_terrain_grants_scaled_kill_reward() {
        // 討伐報酬はモンスター自身に湧いた瞬間固定された `tier` を使う —
        // タイルの現在のtierではない (upgrading_tile_after_spawn_does_not_change_live_monster_reward
        // で回帰確認している)。ここでは固定値による報酬計算だけを検証する。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100;
        s.path[3].terrain = Some(Terrain::Forest);
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 1,
            max_hp: 1,
            attack: 1,
            elite: false,
            tier: 2, // 倍率2.0倍
            cluster_bonus: 0,
        });
        let wood_before = s.wood;
        tick(&mut s);
        // tier0のisolated forest討伐報酬は3。tier2 (2.0倍) なら6のはず。
        assert_eq!(s.wood, wood_before + 6);
    }

    #[test]
    fn upgrading_tile_after_spawn_does_not_change_live_monster_reward() {
        // 生存中のモンスターがいるタイルを (異なる操作で) tier強化しても、
        // 既に湧いている個体の討伐報酬は変わらないはず。変わってしまうと
        // 「敵を強くせずに報酬だけ釣り上げる」抜け穴になる。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100;
        s.path[3].terrain = Some(Terrain::Forest);
        spawn_monster(&mut s, 3, Terrain::Forest); // tier0で湧く
        assert_eq!(s.path[3].monster.as_ref().unwrap().tier, 0);

        s.path[3].tier = 2; // 生存中にタイル側だけ強化 (このモンスターには影響しないはず)

        let wood_before = s.wood;
        tick(&mut s);
        assert_eq!(
            s.wood,
            wood_before + 3,
            "生存中のモンスターの報酬はtier0のまま (タイルの後強化が乗ってはいけない)"
        );
    }

    #[test]
    fn upgrading_adjacent_tile_after_spawn_does_not_change_live_monster_cluster_bonus() {
        // 墓地版の同回帰テスト: 生存中のモンスターの隣に後から墓地タイルを
        // 足してクラスターを成立させても、報酬には影響しないはず。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 100;
        s.path[3].terrain = Some(Terrain::Graveyard);
        spawn_monster(&mut s, 3, Terrain::Graveyard); // 孤立状態 (cluster_bonus=0) で湧く
        assert_eq!(s.path[3].monster.as_ref().unwrap().cluster_bonus, 0);

        s.path[4].terrain = Some(Terrain::Graveyard); // 生存中に隣接タイルを追加

        let soul_before = s.soul;
        tick(&mut s);
        assert_eq!(
            s.soul,
            soul_before + 2,
            "生存中のモンスターの報酬は孤立扱いのまま (後からのクラスター成立が乗ってはいけない)"
        );
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
    fn lap_completion_logs_resource_delta_summary() {
        let mut s = expedition_state();
        s.path[1].terrain = Some(Terrain::Meadow); // 周回中に確実に魂を1つ稼がせる
        tick_n(&mut s, MOVE_TICKS * PATH_LEN as u32);
        let summary = s.log.iter().find(|l| l.contains("第1周 完了！"));
        assert!(summary.is_some(), "ラップ完了時に資源の増減サマリーが出るはず: {:?}", s.log);
        assert!(
            summary.unwrap().contains("魂+1"),
            "この周で稼いだ魂の増分が見えるはず: {:?}",
            summary
        );
    }

    #[test]
    fn lap_summary_resets_after_each_lap() {
        let mut s = expedition_state();
        s.path[1].terrain = Some(Terrain::Meadow);
        tick_n(&mut s, MOVE_TICKS * PATH_LEN as u32); // 1周目完了
        tick_n(&mut s, MOVE_TICKS * PATH_LEN as u32); // 2周目完了
        let second_summary = s.log.iter().rev().find(|l| l.contains("第2周 完了！"));
        assert!(second_summary.is_some(), "2周目のサマリーも出るはず: {:?}", s.log);
        assert!(
            second_summary.unwrap().contains("魂+1"),
            "2周目単体の増分 (1周目からの累積ではない) が見えるはず: {:?}",
            second_summary
        );
    }

    #[test]
    fn lap_summary_shows_negative_delta_when_resources_are_net_spent() {
        let mut s = expedition_state();
        s.wood = 10;
        s.stone = 10;
        reset_lap_snapshot(&mut s); // このラップの基準を10/10にする
        assert!(refill_hand(&mut s), "補充costを賄えるだけの資源があるはず");
        // 道は空のまま (資源獲得源が無い) なので、補充で消費した分だけ純減する。
        tick_n(&mut s, MOVE_TICKS * PATH_LEN as u32);
        let summary = s.log.iter().rev().find(|l| l.contains("第1周 完了！"));
        assert!(summary.is_some(), "{:?}", s.log);
        let summary = summary.unwrap();
        assert!(summary.contains("木材-3"), "純減した資源はマイナス表記になるはず: {summary:?}");
        assert!(summary.contains("石材-3"), "純減した資源はマイナス表記になるはず: {summary:?}");
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
        });

        tick(&mut s);

        assert!(s.enemy_hurt_flash.is_active(), "撃破の一撃でもフラッシュは立つ");
        assert!(!s.hero_hurt_flash.is_active(), "倒した瞬間は反撃を受けないので勇者側は立たない");
    }

    #[test]
    fn combat_tick_records_last_hero_and_enemy_damage_when_monster_survives() {
        let mut s = expedition_state();
        let pos = s.hero.position;
        s.hero.attack = 7;
        s.path[pos].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 3,
            elite: false,
            tier: 0,
            cluster_bonus: 0,
        });
        assert!(s.last_hero_damage.is_none());
        assert!(s.last_enemy_damage.is_none());

        tick(&mut s);

        assert_eq!(s.last_enemy_damage, Some((7, DAMAGE_DISPLAY_TICKS)), "勇者の攻撃力そのものが表示ダメージになるはず");
        let (hero_dmg, life) = s.last_hero_damage.expect("モンスターの反撃が記録されるはず");
        assert_eq!(hero_dmg, 3, "岩山シナジー無しなら反撃ダメージは敵の攻撃力そのまま");
        assert_eq!(life, DAMAGE_DISPLAY_TICKS);
    }

    #[test]
    fn defeating_monster_records_enemy_damage_but_not_hero_damage() {
        let mut s = expedition_state();
        let pos = s.hero.position;
        s.hero.attack = 100;
        s.path[pos].monster = Some(Monster {
            terrain: Terrain::Graveyard,
            hp: 1,
            max_hp: 1,
            attack: 3,
            elite: false,
            tier: 0,
            cluster_bonus: 0,
        });

        tick(&mut s);

        assert_eq!(
            s.last_enemy_damage,
            Some((1, DAMAGE_DISPLAY_TICKS)),
            "オーバーキル分を含まず、実際に削れたHP量がヘッダー表示にも使われるはず"
        );
        assert!(s.last_hero_damage.is_none(), "倒した瞬間は反撃を受けないので記録されない");
    }

    #[test]
    fn damage_display_decays_and_clears_after_ticks() {
        let mut s = expedition_state();
        s.last_hero_damage = Some((5, 2));
        s.last_enemy_damage = Some((3, 1));

        tick(&mut s);
        assert_eq!(s.last_hero_damage, Some((5, 1)), "1tick経過で残りが減るはず");
        assert!(s.last_enemy_damage.is_none(), "残り1tickだったので0になった時点で消えるはず");

        tick(&mut s);
        assert!(s.last_hero_damage.is_none());
    }

    #[test]
    fn death_resets_damage_display() {
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
            tier: 0,
            cluster_bonus: 0,
        });

        tick(&mut s);

        assert_eq!(s.phase, Phase::Camp, "この tick で死亡しているはず");
        assert!(s.last_hero_damage.is_none(), "死亡直後の演出データは持ち越さない");
        assert!(s.last_enemy_damage.is_none());
    }

    #[test]
    fn kill_log_includes_final_blow_damage_capped_to_remaining_hp() {
        // 攻撃力(42) > 残りHP(5) のオーバーキルするケース。ログには実際に
        // 削れた量 (5) だけを見せ、余った分 (37) は数字に含めない。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 42;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: false,
            tier: 0,
            cluster_bonus: 0,
        });

        tick(&mut s);

        let entry = s.log.last().expect("討伐ログが追加されているはず");
        assert!(entry.contains("-5"), "オーバーキルせず実ダメージ量が見えるはず: {entry:?}");
        assert!(!entry.contains("-42"), "攻撃力そのものは表示しない: {entry:?}");
    }

    #[test]
    fn kill_log_shows_full_hit_when_no_overkill() {
        // 攻撃力(3) <= 残りHP(5) のケース。実ダメージ = 攻撃力そのもの。
        let mut s = expedition_state();
        s.hero.position = 3;
        s.hero.attack = 3;
        s.path[3].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 3,
            max_hp: 5,
            attack: 2,
            elite: false,
            tier: 0,
            cluster_bonus: 0,
        });

        tick(&mut s);

        let entry = s.log.last().expect("討伐ログが追加されているはず");
        assert!(entry.contains("-3"), "オーバーキルが無ければ攻撃力そのものが出るはず: {entry:?}");
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
            tier: 0,
            cluster_bonus: 0,
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
    fn place_selected_same_terrain_upgrades_tier_instead_of_failing() {
        let mut s = expedition_state();
        s.path[5].terrain = Some(Terrain::Forest);
        s.hand[0] = Some(Terrain::Forest);
        s.selected_hand = Some(0);
        assert!(place_selected(&mut s, 5), "同じ地形の重ね置きは強化として成功するはず");
        assert_eq!(s.path[5].tier, 1);
        assert_eq!(s.hand[0], None, "強化でも手札は消費される");
        assert_eq!(s.selected_hand, None);
    }

    #[test]
    fn place_selected_upgrade_stops_at_tier_max() {
        let mut s = expedition_state();
        s.path[5].terrain = Some(Terrain::Forest);
        s.path[5].tier = TERRAIN_TIER_MAX;
        s.hand[0] = Some(Terrain::Forest);
        s.selected_hand = Some(0);
        assert!(!place_selected(&mut s, 5), "最大tierを超えて強化できてはいけない");
        assert_eq!(s.path[5].tier, TERRAIN_TIER_MAX);
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
