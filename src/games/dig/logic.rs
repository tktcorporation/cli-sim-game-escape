//! 穴掘り長屋の純粋関数群 — 乱数抽選、掘る、図鑑コンプリート、日付リセット。

use super::state::{
    CollectionSet, DigState, ItemKind, MAX_SHOVEL_LEVEL, NEIGHBOR_COUNT, YARD_LEN,
};

/// 1日の長さ (ミリ秒)。実際のカレンダー日が変わったかの判定に使う。
pub const DAY_MS: u64 = 86_400_000;

// ── RNG (rpg/merge と同じ LCG) ───────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn rng_range(seed: &mut u64, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    *seed = next_rng(*seed);
    ((*seed >> 33) % max as u64) as u32
}

/// 重み付き抽選テーブルから1つ選ぶ。重み合計が0の場合は先頭を返す。
fn roll_weighted(seed: &mut u64, table: &[(ItemKind, u32)]) -> ItemKind {
    let total: u32 = table.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return table.first().map(|(i, _)| *i).unwrap_or(ItemKind::Dirt);
    }
    let r = rng_range(seed, total);
    let mut acc = 0u32;
    for (item, w) in table {
        acc += w;
        if r < acc {
            return *item;
        }
    }
    table.last().map(|(i, _)| *i).unwrap_or(ItemKind::Dirt)
}

// ── 抽選テーブル ─────────────────────────────────────────────

/// 自分の庭の抽選テーブル。シャベルLvが上がるほど「地面の恵み」は
/// レア寄りに (Dirt/Pebble 減 ⇔ CopperCoin以上・かけら増)、深く掘れるようになる。
pub fn yard_weights(shovel_level: u8) -> [(ItemKind, u32); 12] {
    let lv = shovel_level.min(MAX_SHOVEL_LEVEL) as u32;
    [
        (ItemKind::Dirt, 36u32.saturating_sub(lv * 6)),
        (ItemKind::Pebble, 24u32.saturating_sub(lv * 3)),
        (ItemKind::CopperCoin, 10 + lv * 3),
        (ItemKind::SilverChunk, 4 + lv * 2),
        (ItemKind::GoldNugget, 1 + lv),
        (ItemKind::PotteryTop, 2 + lv),
        (ItemKind::PotteryBottom, 2 + lv),
        (ItemKind::DragonSkull, 2 + lv),
        (ItemKind::DragonSpine, 2 + lv),
        (ItemKind::DragonTail, 2 + lv),
        (ItemKind::ManekiRight, 2 + lv),
        (ItemKind::ManekiLeft, 2 + lv),
    ]
}

/// 友好度レベル (0..=2)。累計で掘らせてもらった回数が増えるほど、
/// その人の専門コレクションが出やすくなる。
pub fn friendship_level(total_digs: u32) -> u8 {
    match total_digs {
        0..=4 => 0,
        5..=14 => 1,
        _ => 2,
    }
}

/// ご近所さんのお福分け穴の抽選テーブル。シャベル補正はかからない代わりに、
/// 専門セットのかけらの重みを友好度に応じて底上げする。
pub fn neighbor_weights(specialty: CollectionSet, friendship: u8) -> Vec<(ItemKind, u32)> {
    let boost = match friendship.min(2) {
        0 => 4,
        1 => 6,
        _ => 8,
    };
    yard_weights(0)
        .into_iter()
        .map(|(item, w)| {
            if item.collection() == Some(specialty) {
                (item, w * boost)
            } else {
                (item, w)
            }
        })
        .collect()
}

// ── 日付リセット ─────────────────────────────────────────────

/// wall-clock ミリ秒から「日インデックス」を求める。UTC 日境界基準。
pub fn day_index(epoch_ms: u64) -> u64 {
    epoch_ms / DAY_MS
}

/// 実際のカレンダー日が進んでいたら行動力・庭・お福分け穴をリセットする。
/// リセットが発生したら `true` を返す。
pub fn maybe_reset_day(state: &mut DigState, now_ms: u64) -> bool {
    let today = day_index(now_ms);
    if today == state.last_reset_day {
        return false;
    }
    state.last_reset_day = today;
    state.actions_remaining = super::state::MAX_ACTIONS_PER_DAY;
    state.yard = [None; YARD_LEN];
    for n in state.neighbors.iter_mut() {
        n.dug_today = false;
    }
    state.add_log("新しい朝が来た。行動力が全回復した！");
    true
}

// ── アクション ───────────────────────────────────────────────

/// 見つけたアイテムを state に反映する (コイン加算 or かけら加算 + 図鑑判定)。
fn apply_found_item(state: &mut DigState, item: ItemKind) {
    if let Some(coins) = item.coin_value() {
        state.coins += coins as u64;
        state.total_coins_earned += coins as u64;
        state.add_log(format!("{}を見つけた (+{}コイン)", item.name(), coins));
    } else if let Some(slot) = item.piece_slot() {
        state.piece_counts[slot] += 1;
        state.add_log(format!("{}を見つけた！", item.name()));
        if let Some(set) = item.collection() {
            maybe_complete_collection(state, set);
        }
    }
}

/// セットの全かけらが揃っていて未コンプリートならボーナスコインを与える。
/// 一度コンプリートしたセットは何度呼んでも再加算されない。
fn maybe_complete_collection(state: &mut DigState, set: CollectionSet) {
    if state.completed_sets[set.index()] {
        return;
    }
    let has_all = set
        .pieces()
        .iter()
        .all(|p| state.piece_counts[p.piece_slot().unwrap()] >= 1);
    if !has_all {
        return;
    }
    state.completed_sets[set.index()] = true;
    let bonus = set.bonus_coins();
    state.coins += bonus as u64;
    state.total_coins_earned += bonus as u64;
    state.add_log(format!(
        "★ 図鑑コンプリート: {} (+{}コイン)",
        set.display_name(),
        bonus
    ));
    state.collection_flash = Some((set, super::state::COLLECTION_FLASH_TTL));
}

/// 自分の庭の `index` セルを掘る。行動力切れ・範囲外・掘り済みなら何もせず false。
pub fn dig_yard(state: &mut DigState, index: usize) -> bool {
    if state.actions_remaining == 0 {
        return false;
    }
    if index >= YARD_LEN || state.yard[index].is_some() {
        return false;
    }
    let table = yard_weights(state.shovel_level);
    let item = roll_weighted(&mut state.rng_state, &table);
    state.yard[index] = Some(item);
    state.actions_remaining -= 1;
    apply_found_item(state, item);
    true
}

/// `idx` 番目のご近所さんのお福分け穴を掘らせてもらう。
/// 行動力切れ・範囲外・本日すでに掘らせてもらった場合は false。
pub fn dig_neighbor(state: &mut DigState, idx: usize) -> bool {
    if state.actions_remaining == 0 || idx >= NEIGHBOR_COUNT {
        return false;
    }
    if state.neighbors[idx].dug_today {
        return false;
    }
    let specialty = state.neighbors[idx].specialty;
    let friendship = friendship_level(state.neighbors[idx].total_digs);
    let table = neighbor_weights(specialty, friendship);
    let item = roll_weighted(&mut state.rng_state, &table);

    state.neighbors[idx].dug_today = true;
    state.neighbors[idx].total_digs += 1;
    state.actions_remaining -= 1;
    let name = state.neighbors[idx].name;
    apply_found_item(state, item);
    state.add_log(format!("{}のお福分け穴を掘らせてもらった", name));
    true
}

/// シャベル強化の次段階の値段。既に MAX なら `None`。
pub fn shovel_upgrade_cost(level: u8) -> Option<u64> {
    match level {
        0 => Some(100),
        1 => Some(400),
        2 => Some(1200),
        _ => None,
    }
}

/// コインが足りていればシャベルを1段階強化する。
pub fn buy_shovel_upgrade(state: &mut DigState) -> bool {
    match shovel_upgrade_cost(state.shovel_level) {
        Some(cost) if state.coins >= cost => {
            state.coins -= cost;
            state.shovel_level += 1;
            state.add_log(format!("シャベルを強化した！ (Lv{})", state.shovel_level));
            true
        }
        _ => false,
    }
}

/// 演出 state (図鑑コンプリートのフラッシュ) を tick 経過分だけ減衰させる。
/// 本ゲームは日付駆動 (行動回数の回復は `maybe_reset_day` 経由) であり、
/// tick ベースでは演出以外の状態は進行しない。
pub fn tick(state: &mut DigState, delta_ticks: u32) {
    if let Some((set, ttl)) = state.collection_flash {
        let remaining = ttl.saturating_sub(delta_ticks);
        state.collection_flash = if remaining == 0 { None } else { Some((set, remaining)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::dig::state::PIECE_COUNT;

    #[test]
    fn day_indexはutc日境界で切り替わる() {
        assert_eq!(day_index(0), 0);
        assert_eq!(day_index(DAY_MS - 1), 0);
        assert_eq!(day_index(DAY_MS), 1);
        assert_eq!(day_index(DAY_MS * 2 - 1), 1);
    }

    #[test]
    fn roll_weightedは重みに応じて選ぶ() {
        // 重み0の項目は絶対に選ばれない。
        let table = [(ItemKind::Dirt, 0), (ItemKind::Pebble, 1)];
        let mut seed = 1u64;
        for _ in 0..50 {
            assert_eq!(roll_weighted(&mut seed, &table), ItemKind::Pebble);
        }
    }

    #[test]
    fn roll_weightedは重み合計が0なら先頭を返す() {
        let table = [(ItemKind::Dirt, 0), (ItemKind::Pebble, 0)];
        let mut seed = 42u64;
        assert_eq!(roll_weighted(&mut seed, &table), ItemKind::Dirt);
    }

    #[test]
    fn yard_weightsはlvが上がるほどレア重みが増える() {
        let w0 = yard_weights(0);
        let w3 = yard_weights(MAX_SHOVEL_LEVEL);
        let gold = |t: &[(ItemKind, u32); 12]| {
            t.iter().find(|(i, _)| *i == ItemKind::GoldNugget).unwrap().1
        };
        let dirt = |t: &[(ItemKind, u32); 12]| {
            t.iter().find(|(i, _)| *i == ItemKind::Dirt).unwrap().1
        };
        assert!(gold(&w3) > gold(&w0), "GoldNugget の重みは lv3 の方が高いはず");
        assert!(dirt(&w3) < dirt(&w0), "Dirt の重みは lv3 の方が低いはず");
    }

    #[test]
    fn yard_weightsはlvが上限を超えても暴走しない() {
        let capped = yard_weights(MAX_SHOVEL_LEVEL);
        let over = yard_weights(u8::MAX);
        assert_eq!(capped, over);
    }

    #[test]
    fn friendship_levelは閾値で段階が上がる() {
        assert_eq!(friendship_level(0), 0);
        assert_eq!(friendship_level(4), 0);
        assert_eq!(friendship_level(5), 1);
        assert_eq!(friendship_level(14), 1);
        assert_eq!(friendship_level(15), 2);
        assert_eq!(friendship_level(1000), 2);
    }

    #[test]
    fn neighbor_weightsは専門セットのかけらの取り分を底上げする() {
        let specialty_share = |friendship: u8| {
            let table = neighbor_weights(CollectionSet::Pottery, friendship);
            let total: u32 = table.iter().map(|(_, w)| *w).sum();
            let specialty: u32 = table
                .iter()
                .filter(|(i, _)| i.collection() == Some(CollectionSet::Pottery))
                .map(|(_, w)| *w)
                .sum();
            specialty as f64 / total as f64
        };
        let share0 = specialty_share(0);
        let share1 = specialty_share(1);
        let share2 = specialty_share(2);
        assert!(share1 > share0, "友好度1は0より専門かけらの取り分が高いはず");
        assert!(share2 > share1, "友好度2は1より専門かけらの取り分が高いはず");

        // 補正がかかっていないベース (lv0 の yard_weights) と比べても常に高い。
        let base_table = yard_weights(0);
        let base_total: u32 = base_table.iter().map(|(_, w)| *w).sum();
        let base_specialty: u32 = base_table
            .iter()
            .filter(|(i, _)| i.collection() == Some(CollectionSet::Pottery))
            .map(|(_, w)| *w)
            .sum();
        assert!(share0 > base_specialty as f64 / base_total as f64);
    }

    #[test]
    fn maybe_reset_dayは日付が変わった時だけ全回復する() {
        let mut s = DigState::new();
        s.actions_remaining = 0;
        s.yard[0] = Some(ItemKind::Dirt);
        s.neighbors[0].dug_today = true;

        // 同じ日 (day_index 0) なら何も変わらない。
        assert!(!maybe_reset_day(&mut s, DAY_MS - 1));
        assert_eq!(s.actions_remaining, 0);
        assert!(s.yard[0].is_some());

        // 翌日になったらリセット。
        assert!(maybe_reset_day(&mut s, DAY_MS));
        assert_eq!(s.actions_remaining, super::super::state::MAX_ACTIONS_PER_DAY);
        assert!(s.yard.iter().all(|c| c.is_none()));
        assert!(!s.neighbors[0].dug_today);
        assert_eq!(s.last_reset_day, 1);

        // 同日中の再呼び出しは false (リセット済み進行を壊さない)。
        s.actions_remaining = 3;
        assert!(!maybe_reset_day(&mut s, DAY_MS + 500));
        assert_eq!(s.actions_remaining, 3);
    }

    #[test]
    fn dig_yardは行動力を1消費してセルを埋める() {
        let mut s = DigState::new();
        let before = s.actions_remaining;
        assert!(dig_yard(&mut s, 0));
        assert_eq!(s.actions_remaining, before - 1);
        assert!(s.yard[0].is_some());
    }

    #[test]
    fn dig_yardは行動力が0なら失敗する() {
        let mut s = DigState::new();
        s.actions_remaining = 0;
        assert!(!dig_yard(&mut s, 0));
        assert!(s.yard[0].is_none());
    }

    #[test]
    fn dig_yardは掘り済みセルには失敗する() {
        let mut s = DigState::new();
        assert!(dig_yard(&mut s, 3));
        let actions_after_first = s.actions_remaining;
        assert!(!dig_yard(&mut s, 3));
        assert_eq!(s.actions_remaining, actions_after_first);
    }

    #[test]
    fn dig_yardは範囲外indexには失敗する() {
        let mut s = DigState::new();
        assert!(!dig_yard(&mut s, YARD_LEN));
    }

    #[test]
    fn dig_neighborは行動力を1消費して本日済みにする() {
        let mut s = DigState::new();
        let before = s.actions_remaining;
        assert!(dig_neighbor(&mut s, 0));
        assert_eq!(s.actions_remaining, before - 1);
        assert!(s.neighbors[0].dug_today);
        assert_eq!(s.neighbors[0].total_digs, 1);
    }

    #[test]
    fn dig_neighborは本日すでに掘っていれば失敗する() {
        let mut s = DigState::new();
        assert!(dig_neighbor(&mut s, 1));
        let actions_after_first = s.actions_remaining;
        assert!(!dig_neighbor(&mut s, 1));
        assert_eq!(s.actions_remaining, actions_after_first);
    }

    #[test]
    fn dig_neighborは行動力が0なら失敗する() {
        let mut s = DigState::new();
        s.actions_remaining = 0;
        assert!(!dig_neighbor(&mut s, 0));
    }

    #[test]
    fn dig_neighborは範囲外indexには失敗する() {
        let mut s = DigState::new();
        assert!(!dig_neighbor(&mut s, NEIGHBOR_COUNT));
    }

    #[test]
    fn apply_found_itemは通貨アイテムでコインが増える() {
        let mut s = DigState::new();
        apply_found_item(&mut s, ItemKind::GoldNugget);
        assert_eq!(s.coins, 40);
        assert_eq!(s.total_coins_earned, 40);
    }

    #[test]
    fn apply_found_itemはかけらで所持数が増える() {
        let mut s = DigState::new();
        apply_found_item(&mut s, ItemKind::DragonSkull);
        assert_eq!(s.piece_counts[ItemKind::DragonSkull.piece_slot().unwrap()], 1);
        assert_eq!(s.coins, 0);
    }

    #[test]
    fn セットの全かけらが揃うとボーナスコインが一度だけ入る() {
        let mut s = DigState::new();
        apply_found_item(&mut s, ItemKind::ManekiRight);
        assert!(!s.completed_sets[CollectionSet::Maneki.index()]);
        apply_found_item(&mut s, ItemKind::ManekiLeft);
        assert!(s.completed_sets[CollectionSet::Maneki.index()]);
        assert_eq!(s.coins, CollectionSet::Maneki.bonus_coins() as u64);
        assert!(s.collection_flash.is_some());

        // 同じセットのかけらをもう一度見つけても再加算されない。
        apply_found_item(&mut s, ItemKind::ManekiRight);
        assert_eq!(s.coins, CollectionSet::Maneki.bonus_coins() as u64);
        assert_eq!(s.piece_counts[ItemKind::ManekiRight.piece_slot().unwrap()], 2);
    }

    #[test]
    fn buy_shovel_upgradeはコインを消費してlvが上がる() {
        let mut s = DigState::new();
        s.coins = 100;
        assert!(buy_shovel_upgrade(&mut s));
        assert_eq!(s.shovel_level, 1);
        assert_eq!(s.coins, 0);
    }

    #[test]
    fn buy_shovel_upgradeはコイン不足なら失敗する() {
        let mut s = DigState::new();
        s.coins = 99;
        assert!(!buy_shovel_upgrade(&mut s));
        assert_eq!(s.shovel_level, 0);
    }

    #[test]
    fn buy_shovel_upgradeは最大lvで失敗する() {
        let mut s = DigState::new();
        s.shovel_level = MAX_SHOVEL_LEVEL;
        s.coins = 999_999;
        assert!(!buy_shovel_upgrade(&mut s));
    }

    #[test]
    fn tickは図鑑フラッシュを減衰させ0でnoneになる() {
        let mut s = DigState::new();
        s.collection_flash = Some((CollectionSet::Dragon, 5));
        tick(&mut s, 3);
        assert_eq!(s.collection_flash, Some((CollectionSet::Dragon, 2)));
        tick(&mut s, 10);
        assert_eq!(s.collection_flash, None);
    }

    #[test]
    fn piece_countの配列長は定数と一致する() {
        let s = DigState::new();
        assert_eq!(s.piece_counts.len(), PIECE_COUNT);
    }
}
