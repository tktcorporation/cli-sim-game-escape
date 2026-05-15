//! マージゲームのロジック (純粋関数)。tick / タップ / クエスト納品。

use super::state::{Cell, ItemType, MergeState, Quest, MAX_LEVEL, MAX_UPGRADE, QUEST_SLOTS};

/// 起動時に最大このクエスト数まで自動補充。
const TARGET_QUEST_SLOTS: usize = QUEST_SLOTS;

/// 1 tick 進める。複数 tick 分まとめて呼ばれる。
pub fn tick(state: &mut MergeState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        tick_once(state);
    }
}

fn tick_once(state: &mut MergeState) {
    state.anim_frame = state.anim_frame.wrapping_add(1);

    // ジェネレーター cooldown を 1 ずつ減らす。
    for cd in state.gen_cooldown.iter_mut() {
        if *cd > 0 {
            *cd -= 1;
        }
    }

    if let Some((x, y, ttl)) = state.flash_cell {
        if ttl > 1 {
            state.flash_cell = Some((x, y, ttl - 1));
        } else {
            state.flash_cell = None;
        }
    }

    // クエストスロットが空いていれば自動補充。
    refill_quests(state);
}

/// 盤面の (x, y) をタップ。`selected` を踏まえて意味を決める。
pub fn tap_cell(state: &mut MergeState, x: usize, y: usize) {
    if !MergeState::in_bounds(x, y) {
        return;
    }
    let target = state.get(x, y);
    match state.selected {
        None => match target {
            Cell::Generator(t) => {
                fire_generator(state, t);
            }
            Cell::Item(_, _) => {
                state.selected = Some((x, y));
            }
            Cell::Empty => {
                // 空セル単独タップ: 何もしない。
            }
        },
        Some((sx, sy)) => {
            if (sx, sy) == (x, y) {
                state.selected = None;
                return;
            }
            let src = state.get(sx, sy);
            match (src, target) {
                (Cell::Item(_, _), Cell::Empty) => {
                    state.set(x, y, src);
                    state.set(sx, sy, Cell::Empty);
                    state.selected = None;
                    state.flash(x, y);
                }
                (Cell::Item(t1, lv1), Cell::Item(t2, lv2))
                    if t1 == t2 && lv1 == lv2 && lv1 < MAX_LEVEL =>
                {
                    let new_lv = lv1 + 1;
                    state.set(x, y, Cell::Item(t1, new_lv));
                    state.set(sx, sy, Cell::Empty);
                    state.selected = None;
                    state.flash(x, y);
                    if new_lv > state.best_level {
                        state.best_level = new_lv;
                        state.add_log(format!("✨ {} LV{} に到達!", t1.full_name(), new_lv));
                    }
                }
                (Cell::Item(_, _), Cell::Item(_, _)) => {
                    // 異種 / 異 Lv / Lv MAX 同士は選択切り替えのみ。
                    state.selected = Some((x, y));
                }
                (Cell::Item(_, _), Cell::Generator(_)) => {
                    // ジェネレーターには重ねられない → 選択解除して発火だけ走らせる。
                    state.selected = None;
                    if let Cell::Generator(t) = target {
                        fire_generator(state, t);
                    }
                }
                _ => {
                    // ありえない遷移 (src が Generator/Empty なのに selected) は念のため
                    // クリアして無害化する。
                    state.selected = None;
                }
            }
        }
    }
}

/// ジェネレーター発火。cooldown 中 or 盤面満杯なら何もしない (ログだけ残す)。
fn fire_generator(state: &mut MergeState, t: ItemType) {
    let idx = t.gen_index();
    if state.gen_cooldown[idx] > 0 {
        // クールダウン中はサイレント (連打でログがうるさくならないように)。
        return;
    }
    let target = match state.first_empty() {
        Some(p) => p,
        None => {
            state.add_log("盤面がいっぱい — マージで整理しよう");
            return;
        }
    };
    state.set(target.0, target.1, Cell::Item(t, 1));
    state.gen_cooldown[idx] = state.current_cooldown_ticks();
    state.flash(target.0, target.1);
}

/// クエスト納品。在庫が足りていれば実行 → コイン獲得 → クエスト消去。
/// 戻り値: true なら納品成功。
pub fn deliver_quest(state: &mut MergeState, slot: usize) -> bool {
    if slot >= QUEST_SLOTS {
        return false;
    }
    let quest = match state.quests[slot] {
        Some(q) => q,
        None => return false,
    };
    if state.count_items(quest.item_type, quest.level) < quest.needed {
        state.add_log("在庫不足");
        return false;
    }
    let removed = state.remove_items(quest.item_type, quest.level, quest.needed);
    debug_assert_eq!(removed, quest.needed);
    // 削除されたセルを `selected` が指していると、次のタップが「無効遷移」
    // 分岐に吸い取られて操作不能感が出る。納品成功時はカーソルを必ずリセット。
    state.selected = None;
    state.coins = state.coins.saturating_add(quest.reward as u64);
    state.total_coins_earned = state.total_coins_earned.saturating_add(quest.reward as u64);
    state.quests[slot] = None;
    state.add_log(format!(
        "💰 {} LV{} ×{} 納品 +{}",
        quest.item_type.full_name(),
        quest.level,
        quest.needed,
        quest.reward,
    ));
    true
}

/// クエストをリロール (破棄して別のを抽選)。手詰まり打破用。
pub fn reroll_quest(state: &mut MergeState, slot: usize) -> bool {
    if slot >= QUEST_SLOTS {
        return false;
    }
    if state.quests[slot].is_none() {
        return false;
    }
    state.quests[slot] = None;
    refill_quests(state);
    true
}

/// アップグレード購入。コインが足りれば 1 段階上げる。
pub fn buy_upgrade(state: &mut MergeState) -> bool {
    let cost = match state.next_upgrade_cost() {
        Some(c) => c,
        None => {
            state.add_log("これ以上アップグレードできない");
            return false;
        }
    };
    if state.coins < cost {
        state.add_log(format!("コイン不足 (必要 {})", cost));
        return false;
    }
    state.coins -= cost;
    state.gen_upgrade_level = (state.gen_upgrade_level + 1).min(MAX_UPGRADE);
    state.add_log(format!(
        "⚡ ジェネレーター強化 LV{} (cooldown 短縮)",
        state.gen_upgrade_level
    ));
    true
}

pub fn clear_selection(state: &mut MergeState) {
    state.selected = None;
}

/// 空クエストスロットを埋める。レベルや個数は best_level でゆるくスケール。
fn refill_quests(state: &mut MergeState) {
    for slot in 0..TARGET_QUEST_SLOTS {
        if state.quests[slot].is_none() {
            state.quests[slot] = Some(generate_quest(state));
        }
    }
}

fn generate_quest(state: &mut MergeState) -> Quest {
    // xorshift64 で次の seed を作る。決定的なので save/load 後も同じ列。
    fn next_rng(seed: &mut u64) -> u64 {
        let mut x = *seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if x == 0 {
            // xorshift は 0 で停滞する。万が一 0 が混入したら再 seed。
            x = 0xDEAD_BEEF_CAFE_F00D;
        }
        *seed = x;
        x
    }

    let r1 = next_rng(&mut state.rng_state);
    let r2 = next_rng(&mut state.rng_state);
    let r3 = next_rng(&mut state.rng_state);

    let kinds = ItemType::all();
    let item_type = kinds[(r1 % kinds.len() as u64) as usize];

    // best_level に応じて要求 lv をスケール。
    // best=0..=1 → lv1, best=2 → lv1..2, best=3 → lv1..3, best=4 → lv2..3, best=5 → lv2..4
    let (lv_min, lv_max) = match state.best_level {
        0..=1 => (1u8, 1u8),
        2 => (1, 2),
        3 => (1, 3),
        4 => (2, 3),
        _ => (2, 4),
    };
    let lv_range = (lv_max - lv_min + 1) as u64;
    let level = lv_min + (r2 % lv_range) as u8;

    // 個数は lv が低いほど多めにする (LV1 ×3 みたいに「数で稼ぐ」クエストを残す)。
    let needed = match level {
        1 => 1 + (r3 % 3) as u8,    // 1..=3
        2 => 1 + (r3 % 2) as u8,    // 1..=2
        _ => 1,                      // LV3+ は 1 個でも報酬が大きい
    };

    Quest {
        item_type,
        level,
        needed,
        reward: Quest::compute_reward(level, needed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos_of_first_empty(state: &MergeState) -> (usize, usize) {
        state.first_empty().expect("盤面に空き必要")
    }

    #[test]
    fn fire_generator_places_lv1_item() {
        let mut s = MergeState::new();
        let (ex, ey) = pos_of_first_empty(&s);
        // タップ第 1 弾としてジェネレーターを叩く
        tap_cell(&mut s, 0, 0); // Flower generator
        assert_eq!(s.get(ex, ey), Cell::Item(ItemType::Flower, 1));
        assert!(s.gen_cooldown[ItemType::Flower.gen_index()] > 0);
    }

    #[test]
    fn fire_during_cooldown_is_silent() {
        let mut s = MergeState::new();
        tap_cell(&mut s, 0, 0); // 1 回目: 生成
        let count = s.grid.iter().filter(|c| matches!(c, Cell::Item(_, _))).count();
        tap_cell(&mut s, 0, 0); // 2 回目: cooldown 中 → 増えない
        let count2 = s.grid.iter().filter(|c| matches!(c, Cell::Item(_, _))).count();
        assert_eq!(count, count2);
    }

    #[test]
    fn cooldown_decreases_with_ticks() {
        let mut s = MergeState::new();
        tap_cell(&mut s, 0, 0);
        let cd0 = s.gen_cooldown[0];
        tick(&mut s, 10);
        assert_eq!(s.gen_cooldown[0], cd0.saturating_sub(10));
    }

    #[test]
    fn select_item_then_empty_moves() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        tap_cell(&mut s, 1, 1); // select
        assert_eq!(s.selected, Some((1, 1)));
        tap_cell(&mut s, 2, 2); // move
        assert_eq!(s.get(2, 2), Cell::Item(ItemType::Flower, 1));
        assert!(s.get(1, 1).is_empty());
        assert!(s.selected.is_none());
    }

    #[test]
    fn merge_same_type_same_level_produces_next_level() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Gem, 2));
        s.set(2, 2, Cell::Item(ItemType::Gem, 2));
        tap_cell(&mut s, 1, 1);
        tap_cell(&mut s, 2, 2);
        assert_eq!(s.get(2, 2), Cell::Item(ItemType::Gem, 3));
        assert!(s.get(1, 1).is_empty());
        assert_eq!(s.best_level, 3);
    }

    #[test]
    fn merge_at_max_level_does_not_merge() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Tool, MAX_LEVEL));
        s.set(2, 2, Cell::Item(ItemType::Tool, MAX_LEVEL));
        tap_cell(&mut s, 1, 1);
        tap_cell(&mut s, 2, 2);
        // MAX 同士は選択切り替えのみで、消えない
        assert_eq!(s.get(1, 1), Cell::Item(ItemType::Tool, MAX_LEVEL));
        assert_eq!(s.get(2, 2), Cell::Item(ItemType::Tool, MAX_LEVEL));
        assert_eq!(s.selected, Some((2, 2)));
    }

    #[test]
    fn merge_different_type_switches_selection() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 2));
        s.set(2, 2, Cell::Item(ItemType::Gem, 2));
        tap_cell(&mut s, 1, 1);
        tap_cell(&mut s, 2, 2);
        assert_eq!(s.selected, Some((2, 2)));
        // 両方残る
        assert_eq!(s.get(1, 1), Cell::Item(ItemType::Flower, 2));
        assert_eq!(s.get(2, 2), Cell::Item(ItemType::Gem, 2));
    }

    #[test]
    fn same_cell_twice_clears_selection() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        tap_cell(&mut s, 1, 1);
        tap_cell(&mut s, 1, 1);
        assert!(s.selected.is_none());
    }

    #[test]
    fn deliver_clears_selection_pointing_to_removed_item() {
        // 納品で消えるセルを `selected` が指していたら、納品後に必ずクリアする。
        // クリアしないと次のタップが「無効遷移」分岐に吸い取られ、操作不能感が出る。
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        s.set(2, 2, Cell::Item(ItemType::Flower, 1));
        s.quests[0] = Some(Quest {
            item_type: ItemType::Flower,
            level: 1,
            needed: 2,
            reward: 40,
        });
        tap_cell(&mut s, 1, 1);
        assert_eq!(s.selected, Some((1, 1)));
        assert!(deliver_quest(&mut s, 0));
        assert!(s.selected.is_none());
    }

    #[test]
    fn quests_refill_on_tick() {
        let mut s = MergeState::new();
        // 起動直後は空 → tick で埋まる
        assert!(s.quests.iter().all(|q| q.is_none()));
        tick(&mut s, 1);
        assert!(s.quests.iter().all(|q| q.is_some()));
    }

    #[test]
    fn deliver_quest_consumes_items_and_pays() {
        let mut s = MergeState::new();
        s.quests[0] = Some(Quest {
            item_type: ItemType::Flower,
            level: 1,
            needed: 2,
            reward: 40,
        });
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        s.set(2, 2, Cell::Item(ItemType::Flower, 1));
        assert!(deliver_quest(&mut s, 0));
        assert_eq!(s.coins, 40);
        assert!(s.quests[0].is_none());
        assert_eq!(s.count_items(ItemType::Flower, 1), 0);
    }

    #[test]
    fn deliver_quest_fails_when_short() {
        let mut s = MergeState::new();
        s.quests[0] = Some(Quest {
            item_type: ItemType::Tool,
            level: 2,
            needed: 1,
            reward: 50,
        });
        assert!(!deliver_quest(&mut s, 0));
        assert_eq!(s.coins, 0);
        assert!(s.quests[0].is_some());
    }

    #[test]
    fn reroll_replaces_quest_after_tick() {
        let mut s = MergeState::new();
        tick(&mut s, 1);
        let original = s.quests[0];
        assert!(reroll_quest(&mut s, 0));
        // refill_quests は同じ tick 内で別の seed を消費するので別物が入る
        assert!(s.quests[0].is_some());
        assert_ne!(s.quests[0], original);
    }

    #[test]
    fn buy_upgrade_requires_coins() {
        let mut s = MergeState::new();
        assert!(!buy_upgrade(&mut s));
        s.coins = 1000;
        assert!(buy_upgrade(&mut s));
        assert_eq!(s.gen_upgrade_level, 1);
        assert_eq!(s.coins, 800); // 1000 - 200
    }

    #[test]
    fn buy_upgrade_is_capped() {
        let mut s = MergeState::new();
        s.gen_upgrade_level = MAX_UPGRADE;
        s.coins = 10_000;
        assert!(!buy_upgrade(&mut s));
        assert_eq!(s.gen_upgrade_level, MAX_UPGRADE);
        assert_eq!(s.coins, 10_000);
    }

    #[test]
    fn rng_is_deterministic_across_runs() {
        // 同じ seed の MergeState 2 つは同じクエスト列を吐く。save/load 後も
        // クエスト体験が連続する保証。
        let mut a = MergeState::new();
        let mut b = MergeState::new();
        tick(&mut a, 1);
        tick(&mut b, 1);
        assert_eq!(a.quests, b.quests);
    }

    #[test]
    fn tap_out_of_bounds_is_no_op() {
        use super::super::state::{GRID_H, GRID_W};
        let mut s = MergeState::new();
        // 範囲外は無視される (panic しない)
        tap_cell(&mut s, GRID_W, 0);
        tap_cell(&mut s, 0, GRID_H);
    }
}
