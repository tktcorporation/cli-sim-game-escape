//! 穴掘り長屋の純粋関数群 — 現場生成、ヒント計算、発掘、羅盤、日付リセット。

use super::state::{
    CollectionSet, DigState, Flash, ItemKind, Treasure, FLASH_COMPLETE_TTL, FLASH_HIT_TTL,
    RADAR_MAX_PER_DAY, SHOVELS_PER_DAY, SITE_H, SITE_LEN, SITE_W,
};

/// 1日の長さ (ミリ秒)。実際のカレンダー日が変わったかの判定に使う。
pub const DAY_MS: u64 = 86_400_000;

/// 完全制覇 (その日の宝を全回収) ボーナスコイン。
pub const PERFECT_BONUS: u64 = 100;

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

// ── 現場生成 ─────────────────────────────────────────────────

/// 形を90°単位で `k` 回回転し、オフセットを非負に正規化する。
fn rotated_shape(shape: &[(i8, i8)], k: u32) -> Vec<(i8, i8)> {
    let mut cells: Vec<(i8, i8)> = shape.to_vec();
    for _ in 0..(k % 4) {
        cells = cells.iter().map(|&(x, y)| (y, -x)).collect();
    }
    let min_x = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    cells.iter().map(|&(x, y)| (x - min_x, y - min_y)).collect()
}

/// `origin` に回転済み形を置いた時の flat index 列。境界外や既存の宝と
/// 重なる場合は `None`。
fn try_place(
    occupied: &[bool; SITE_LEN],
    shape: &[(i8, i8)],
    ox: i32,
    oy: i32,
) -> Option<Vec<u16>> {
    let mut cells = Vec::with_capacity(shape.len());
    for &(dx, dy) in shape {
        let x = ox + dx as i32;
        let y = oy + dy as i32;
        if x < 0 || y < 0 || x >= SITE_W as i32 || y >= SITE_H as i32 {
            return None;
        }
        let idx = y as usize * SITE_W + x as usize;
        if occupied[idx] {
            return None;
        }
        cells.push(idx as u16);
    }
    Some(cells)
}

/// 完全制覇に許容する実質シャベル消費 (総宝マス数 − 宝の数) の上限。
/// 完掘ごとに1本返却されるため、実質消費がこの値以下なら
/// 5本のシャベルで最低1回は空振りしても掘り切れる。
pub const MAX_NET_COST: usize = (SHOVELS_PER_DAY - 1) as usize;

/// その日の宝の組み合わせの実質シャベル消費。
fn net_cost(kinds: &[ItemKind]) -> usize {
    kinds.iter().map(|k| k.size()).sum::<usize>() - kinds.len()
}

/// 日付から現場 (埋まっている宝の配置) を決定的に生成する。
/// 同じ日なら全プレイヤーが同じ現場を掘る — 「今日の現場」を共有できる
/// ソーシャル性の核なので、この関数に非決定的な入力を混ぜてはならない。
pub fn generate_site(day: u64) -> Vec<Treasure> {
    let mut seed = day
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xD1CE_5EED_0BAD_C0DE);
    // seed を1回回して day の下位ビット偏りを散らす。
    seed = next_rng(seed);

    let count = 3 + rng_range(&mut seed, 2) as usize;

    // 種類は重複なしで選ぶ (同じ日に同じ宝が2つ埋まると図鑑の意味が薄れる)。
    // 大型宝ばかりの組み合わせはシャベル収支が破綻して「絶対に掘り切れない日」
    // になるため、実質消費が MAX_NET_COST に収まるまで引き直す。
    let draw_kinds = |seed: &mut u64| -> Vec<ItemKind> {
        let mut pool: Vec<ItemKind> = ItemKind::all().to_vec();
        let mut kinds = Vec::with_capacity(count);
        for _ in 0..count {
            let i = rng_range(seed, pool.len() as u32) as usize;
            kinds.push(pool.swap_remove(i));
        }
        kinds
    };
    let mut kinds = draw_kinds(&mut seed);
    let mut attempts = 0;
    while net_cost(&kinds) > MAX_NET_COST {
        attempts += 1;
        if attempts > 100 {
            // 決定的フォールバック (実質消費2)。乱数の偏りでもここへは
            // 実質到達しないが、無限ループの保険。
            kinds = vec![ItemKind::DragonSkull, ItemKind::Magatama, ItemKind::CoinJar];
            break;
        }
        kinds = draw_kinds(&mut seed);
    }

    let mut occupied = [false; SITE_LEN];
    let mut treasures = Vec::with_capacity(count);
    for kind in kinds {
        let mut placed = None;
        for _ in 0..200 {
            let k = rng_range(&mut seed, 4);
            let shape = rotated_shape(kind.shape(), k);
            let ox = rng_range(&mut seed, SITE_W as u32) as i32;
            let oy = rng_range(&mut seed, SITE_H as u32) as i32;
            if let Some(cells) = try_place(&occupied, &shape, ox, oy) {
                placed = Some(cells);
                break;
            }
        }
        // 万一置けなければ決定的な全探索でフォールバック (35マスに対して
        // 宝は合計10マス未満なので実際にはまず到達しない)。
        if placed.is_none() {
            'scan: for k in 0..4 {
                let shape = rotated_shape(kind.shape(), k);
                for oy in 0..SITE_H as i32 {
                    for ox in 0..SITE_W as i32 {
                        if let Some(cells) = try_place(&occupied, &shape, ox, oy) {
                            placed = Some(cells);
                            break 'scan;
                        }
                    }
                }
            }
        }
        if let Some(cells) = placed {
            for &c in &cells {
                occupied[c as usize] = true;
            }
            treasures.push(Treasure { kind, cells });
        }
    }
    treasures
}

/// 指定日の現場をセットし、1日単位の状態をリセットする。
/// 図鑑・コインなどの永続進行は保持する。
pub fn setup_site(state: &mut DigState, day: u64) {
    state.day = day;
    state.treasures = generate_site(day);
    state.dug = [false; SITE_LEN];
    state.scanned = [false; SITE_LEN];
    state.shovels = SHOVELS_PER_DAY;
    state.radar_uses = 0;
    state.radar_armed = false;
    state.perfect_bonus_given = false;
    state.flash = None;
}

// ── 日付リセット ─────────────────────────────────────────────

/// wall-clock ミリ秒から「日インデックス」を求める。UTC 日境界基準。
pub fn day_index(epoch_ms: u64) -> u64 {
    epoch_ms / DAY_MS
}

/// 実際のカレンダー日が進んでいたら新しい現場に切り替える。
/// 「進んでいたら」のみ — 時計の巻き戻し (端末の時刻操作や wall-clock の
/// 取得失敗による 0 フォールバック) でシャベルが再回復しないようにする。
pub fn maybe_reset_day(state: &mut DigState, now_ms: u64) -> bool {
    let today = day_index(now_ms);
    if today <= state.day {
        return false;
    }
    setup_site(state, today);
    state.add_log("本日の発掘現場が公開された！(現場は全員共通)");
    true
}

/// 現場配置のフィンガープリント。セーブに保存しておき、ロード時に
/// `generate_site(day)` の再生成結果と一致するか検証する — 生成ロジックや
/// 宝の形を変更した時に、古い掘削状況が別配置の現場に誤適用されるのを防ぐ。
pub fn site_fingerprint(treasures: &[Treasure]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for t in treasures {
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (t.kind.to_save_id() as u64 + 1);
        for &c in &t.cells {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (c as u64 + 1);
        }
    }
    h
}

// ── ヒント ───────────────────────────────────────────────────

/// `idx` から一番近い「未掘の宝マス」までのマンハッタン距離。
/// 残存宝がなければ `None`。掘った後も毎回これで再計算するので、
/// 表示される数字は常に「残っている宝」への正しい距離になる。
pub fn hint_at(state: &DigState, idx: usize) -> Option<u32> {
    let x0 = (idx % SITE_W) as i32;
    let y0 = (idx / SITE_W) as i32;
    state
        .treasures
        .iter()
        .flat_map(|t| t.cells.iter())
        .filter(|&&c| !state.dug[c as usize])
        .map(|&c| {
            let x1 = (c as usize % SITE_W) as i32;
            let y1 = (c as usize / SITE_W) as i32;
            ((x0 - x1).abs() + (y0 - y1).abs()) as u32
        })
        .min()
}

// ── 発掘 ─────────────────────────────────────────────────────

/// 宝を回収した時の共通処理 (図鑑登録・コイン・セット完成判定)。
fn collect_treasure(state: &mut DigState, kind: ItemKind) {
    let slot = kind.to_save_id() as usize;
    let first_find = state.museum_counts[slot] == 0;
    state.museum_counts[slot] += 1;
    let coins = if first_find {
        kind.first_find_coins()
    } else {
        kind.duplicate_coins()
    };
    state.coins += coins;
    state.total_coins_earned += coins;
    if first_find {
        state.add_log(format!("★ 新発見: {}！ (+{}コイン)", kind.name(), coins));
        maybe_complete_collection(state, kind.collection());
    } else {
        state.add_log(format!("{}を発掘 (+{}コイン)", kind.name(), coins));
    }
}

/// セットの全種類が図鑑に揃っていて未達成ならボーナスを与える。
fn maybe_complete_collection(state: &mut DigState, set: CollectionSet) {
    if state.completed_sets[set.index()] {
        return;
    }
    let has_all = set
        .kinds()
        .iter()
        .all(|k| state.museum_counts[k.to_save_id() as usize] >= 1);
    if !has_all {
        return;
    }
    state.completed_sets[set.index()] = true;
    let bonus = set.bonus_coins();
    state.coins += bonus;
    state.total_coins_earned += bonus;
    state.add_log(format!(
        "★★ 図鑑コンプリート: {} (+{}コイン)",
        set.display_name(),
        bonus
    ));
}

/// その日の宝を全回収していたら一度だけボーナスを与える。
fn maybe_perfect_bonus(state: &mut DigState) {
    if state.perfect_bonus_given || state.treasures.is_empty() {
        return;
    }
    if state.remaining_treasures() > 0 {
        return;
    }
    state.perfect_bonus_given = true;
    state.coins += PERFECT_BONUS;
    state.total_coins_earned += PERFECT_BONUS;
    state.add_log(format!("☆ 完全制覇！ 本日の宝を全回収 (+{PERFECT_BONUS}コイン)"));
}

/// `idx` を掘る。シャベル切れ・範囲外・掘り済みなら何もせず false。
pub fn dig(state: &mut DigState, idx: usize) -> bool {
    if state.shovels == 0 || idx >= SITE_LEN || state.dug[idx] {
        return false;
    }
    state.dug[idx] = true;
    state.shovels -= 1;

    if let Some((t_idx, kind)) = state.treasure_at(idx) {
        if state.treasure_complete(t_idx) {
            // 完掘 → 回収。シャベル1本返却が推理の報酬になる。
            state.flash = Some(Flash {
                cells: state.treasures[t_idx].cells.clone(),
                ttl: FLASH_COMPLETE_TTL,
            });
            collect_treasure(state, kind);
            state.shovels += 1;
            state.add_log("シャベルが1本返ってきた！");
            maybe_perfect_bonus(state);
        } else {
            state.flash = Some(Flash {
                cells: vec![idx as u16],
                ttl: FLASH_HIT_TTL,
            });
            state.add_log(format!("何かの一部だ…！ (残り{}マス)", remaining_cells(state, t_idx)));
        }
    } else {
        match hint_at(state, idx) {
            Some(n) => state.add_log(format!("空振り。近くのお宝まで {n} 歩")),
            None => state.add_log("空振り。もうお宝は残っていない".to_string()),
        }
    }
    true
}

/// 宝 `t_idx` の未掘マス数。
fn remaining_cells(state: &DigState, t_idx: usize) -> usize {
    state.treasures[t_idx]
        .cells
        .iter()
        .filter(|&&c| !state.dug[c as usize])
        .count()
}

// ── 羅盤 ─────────────────────────────────────────────────────

/// 羅盤の次の使用コスト。1日の上限 (`RADAR_MAX_PER_DAY`) に達していたら `None`。
/// コストは使うたびに倍増し、「安い保険」から「高い最後の一手」に変わる。
pub fn radar_cost(uses: u8) -> Option<u64> {
    if uses >= RADAR_MAX_PER_DAY {
        return None;
    }
    Some(30u64 << uses)
}

/// `idx` を羅盤で調べる (掘らずにヒントを見る)。シャベルは消費しない。
/// コイン不足・上限到達・調査済み/掘削済みマス・全回収後には false
/// (全回収後は調べる対象がなく、コインだけ減る誤タップになるため)。
pub fn scan(state: &mut DigState, idx: usize) -> bool {
    if idx >= SITE_LEN || state.dug[idx] || state.scanned[idx] {
        return false;
    }
    if state.remaining_treasures() == 0 {
        return false;
    }
    let Some(cost) = radar_cost(state.radar_uses) else {
        return false;
    };
    if state.coins < cost {
        return false;
    }
    state.coins -= cost;
    state.radar_uses += 1;
    state.scanned[idx] = true;
    state.radar_armed = false;
    match hint_at(state, idx) {
        Some(n) => state.add_log(format!("羅盤: この地点からお宝まで {n} 歩")),
        None => state.add_log("羅盤: 反応なし".to_string()),
    }
    true
}

// ── Tick ─────────────────────────────────────────────────────

/// 演出 state (発掘フラッシュ) を tick 経過分だけ減衰させる。
/// 本ゲームは日付駆動であり、tick ベースでは演出以外の状態は進行しない。
pub fn tick(state: &mut DigState, delta_ticks: u32) {
    if let Some(flash) = &mut state.flash {
        flash.ttl = flash.ttl.saturating_sub(delta_ticks);
        if flash.ttl == 0 {
            state.flash = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::dig::state::KIND_COUNT;

    /// テスト用: ヒント検証をしやすい既知の配置を組む。
    fn fixed_site(state: &mut DigState) {
        // (2,1)-(3,1) に勾玉、(5,3) に鏃。
        state.treasures = vec![
            Treasure {
                kind: ItemKind::Magatama,
                cells: vec![
                    DigState::idx(2, 1) as u16,
                    DigState::idx(3, 1) as u16,
                ],
            },
            Treasure {
                kind: ItemKind::ObsidianArrow,
                cells: vec![DigState::idx(5, 3) as u16],
            },
        ];
    }

    #[test]
    fn day_indexはutc日境界で切り替わる() {
        assert_eq!(day_index(0), 0);
        assert_eq!(day_index(DAY_MS - 1), 0);
        assert_eq!(day_index(DAY_MS), 1);
    }

    #[test]
    fn 現場生成は同じ日なら同一で決定的() {
        for day in [0u64, 1, 42, 20_650] {
            assert_eq!(generate_site(day), generate_site(day), "day={day}");
        }
    }

    #[test]
    fn 現場生成は日によって配置が変わる() {
        // 全日で異なるとまでは言えないが、連続する日で全一致が続くなら
        // seed の混ぜ方が壊れている。
        let differing = (0..20u64)
            .filter(|&d| generate_site(d) != generate_site(d + 1))
            .count();
        assert!(differing >= 18, "現場が日替わりになっていない ({differing}/20)");
    }

    #[test]
    fn 現場生成は1000日分すべて妥当な配置になる() {
        for day in 0..1000u64 {
            let site = generate_site(day);
            assert!(
                (3..=4).contains(&site.len()),
                "day={day}: 宝の数が {}",
                site.len()
            );
            let mut seen = [false; SITE_LEN];
            let mut kinds: Vec<ItemKind> = Vec::new();
            for t in &site {
                assert_eq!(t.cells.len(), t.kind.size(), "day={day}: 形とマス数の不一致");
                assert!(!kinds.contains(&t.kind), "day={day}: 種類の重複");
                kinds.push(t.kind);
                for &c in &t.cells {
                    assert!((c as usize) < SITE_LEN, "day={day}: 境界外 {c}");
                    assert!(!seen[c as usize], "day={day}: マス重複 {c}");
                    seen[c as usize] = true;
                }
            }
            // シャベル収支の保証: 実質消費 (総マス−個数) が上限以下でないと
            // 空振りゼロでも掘り切れない「死に日」が全プレイヤーに発生する。
            let total_cells: usize = site.iter().map(|t| t.cells.len()).sum();
            let net = total_cells - site.len();
            assert!(
                net <= MAX_NET_COST,
                "day={day}: 実質消費 {net} が上限 {MAX_NET_COST} を超え、完全制覇が不可能"
            );
        }
    }

    #[test]
    fn maybe_reset_dayは時計の巻き戻しではリセットしない() {
        let mut s = DigState::new();
        setup_site(&mut s, 100);
        s.shovels = 0;
        s.dug[3] = true;
        // 過去の日付 (wall-clock 0 フォールバック含む) では現場を作り直さない。
        assert!(!maybe_reset_day(&mut s, 0));
        assert!(!maybe_reset_day(&mut s, DAY_MS * 99));
        assert_eq!(s.day, 100);
        assert_eq!(s.shovels, 0, "巻き戻しでシャベルが回復してはいけない");
        assert!(s.dug[3]);
    }

    #[test]
    fn site_fingerprintは配置が変わると変わる() {
        let a = generate_site(0);
        let b = generate_site(1);
        assert_eq!(site_fingerprint(&a), site_fingerprint(&generate_site(0)));
        assert_ne!(site_fingerprint(&a), site_fingerprint(&b));
        assert_ne!(site_fingerprint(&a), 0, "空でない現場のfpは0にならない");
    }

    #[test]
    fn 全回収後はscanできずコインも減らない() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        s.coins = 1000;
        for t in s.treasures.clone() {
            for c in t.cells {
                s.dug[c as usize] = true;
            }
        }
        assert!(!scan(&mut s, 0));
        assert_eq!(s.coins, 1000);
    }

    #[test]
    fn 回転してもマス数と連結オフセットの正規化が保たれる() {
        for kind in ItemKind::all() {
            for k in 0..4 {
                let r = rotated_shape(kind.shape(), k);
                assert_eq!(r.len(), kind.size());
                assert!(r.iter().all(|&(x, y)| x >= 0 && y >= 0), "{kind:?} k={k}");
                assert!(
                    r.iter().any(|&(x, _)| x == 0) && r.iter().any(|&(_, y)| y == 0),
                    "{kind:?} k={k}: 正規化されていない"
                );
            }
        }
    }

    #[test]
    fn hint_atは一番近い未掘宝マスへのマンハッタン距離を返す() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        // (0,0) から: 勾玉(2,1)まで 3歩、鏃(5,3)まで 8歩 → 3
        assert_eq!(hint_at(&s, DigState::idx(0, 0)), Some(3));
        // (5,2) から: 鏃(5,3)まで 1歩
        assert_eq!(hint_at(&s, DigState::idx(5, 2)), Some(1));
        // 勾玉の左マスを掘り済みにすると、そのマスは距離対象から外れる
        s.dug[DigState::idx(2, 1)] = true;
        assert_eq!(hint_at(&s, DigState::idx(1, 1)), Some(2)); // (3,1) まで
    }

    #[test]
    fn hint_atは全回収後noneを返す() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        for t in s.treasures.clone() {
            for c in t.cells {
                s.dug[c as usize] = true;
            }
        }
        assert_eq!(hint_at(&s, 0), None);
    }

    #[test]
    fn digの空振りはシャベルを1本消費してマスを開ける() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        assert!(dig(&mut s, DigState::idx(0, 0)));
        assert_eq!(s.shovels, SHOVELS_PER_DAY - 1);
        assert!(s.dug[DigState::idx(0, 0)]);
        assert!(s.flash.is_none(), "空振りではフラッシュしない");
    }

    #[test]
    fn digはシャベル切れと掘り済みと範囲外で失敗する() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        assert!(!dig(&mut s, SITE_LEN));
        assert!(dig(&mut s, 0));
        assert!(!dig(&mut s, 0), "掘り済みマス");
        s.shovels = 0;
        assert!(!dig(&mut s, 1), "シャベル切れ");
    }

    #[test]
    fn 宝の一部ヒットではまだ回収されない() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        assert!(dig(&mut s, DigState::idx(2, 1)));
        assert_eq!(s.museum_counts[ItemKind::Magatama.to_save_id() as usize], 0);
        assert_eq!(s.shovels, SHOVELS_PER_DAY - 1);
        assert!(s.flash.is_some());
    }

    #[test]
    fn 完掘で図鑑登録とコインとシャベル返却が起きる() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        dig(&mut s, DigState::idx(2, 1));
        dig(&mut s, DigState::idx(3, 1));
        assert_eq!(s.museum_counts[ItemKind::Magatama.to_save_id() as usize], 1);
        assert_eq!(s.coins, ItemKind::Magatama.first_find_coins());
        // 2消費 + 1返却 = 実質1消費
        assert_eq!(s.shovels, SHOVELS_PER_DAY - 1);
        let flash = s.flash.expect("完掘フラッシュ");
        assert_eq!(flash.cells.len(), 2);
        assert_eq!(flash.ttl, FLASH_COMPLETE_TTL);
    }

    #[test]
    fn 同じ宝の再発見はコイン半減で図鑑数だけ増える() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        s.museum_counts[ItemKind::Magatama.to_save_id() as usize] = 1;
        dig(&mut s, DigState::idx(2, 1));
        dig(&mut s, DigState::idx(3, 1));
        assert_eq!(s.museum_counts[ItemKind::Magatama.to_save_id() as usize], 2);
        assert_eq!(s.coins, ItemKind::Magatama.duplicate_coins());
    }

    #[test]
    fn セット全種類が図鑑に揃うとボーナスが一度だけ入る() {
        let mut s = DigState::new();
        // Dragon セットは頭骨+背骨の2種。
        s.museum_counts[ItemKind::DragonSkull.to_save_id() as usize] = 1;
        s.treasures = vec![Treasure {
            kind: ItemKind::DragonSpine,
            cells: vec![0, 1, 2],
        }];
        dig(&mut s, 0);
        dig(&mut s, 1);
        dig(&mut s, 2);
        assert!(s.completed_sets[CollectionSet::Dragon.index()]);
        let expected = ItemKind::DragonSpine.first_find_coins()
            + CollectionSet::Dragon.bonus_coins()
            + PERFECT_BONUS;
        assert_eq!(s.coins, expected);

        // もう一度完成条件を満たしても再加算されない。
        let coins_after = s.coins;
        maybe_complete_collection(&mut s, CollectionSet::Dragon);
        assert_eq!(s.coins, coins_after);
    }

    #[test]
    fn 完全制覇ボーナスは全宝回収時に一度だけ入る() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        dig(&mut s, DigState::idx(2, 1));
        dig(&mut s, DigState::idx(3, 1));
        assert!(!s.perfect_bonus_given, "まだ鏃が残っている");
        dig(&mut s, DigState::idx(5, 3));
        assert!(s.perfect_bonus_given);
        let expected = ItemKind::Magatama.first_find_coins()
            + ItemKind::ObsidianArrow.first_find_coins()
            + PERFECT_BONUS;
        assert_eq!(s.coins, expected);
    }

    #[test]
    fn radar_costは段階的に上がり上限でnoneになる() {
        assert_eq!(radar_cost(0), Some(30));
        assert_eq!(radar_cost(1), Some(60));
        assert_eq!(radar_cost(2), Some(120));
        assert_eq!(radar_cost(RADAR_MAX_PER_DAY), None);
        assert_eq!(radar_cost(200), None);
    }

    #[test]
    fn scanはコインを消費しシャベルは消費しない() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        s.coins = 100;
        s.radar_armed = true;
        assert!(scan(&mut s, DigState::idx(0, 0)));
        assert_eq!(s.coins, 70);
        assert_eq!(s.shovels, SHOVELS_PER_DAY);
        assert!(s.scanned[DigState::idx(0, 0)]);
        assert!(!s.radar_armed, "使用後は羅盤モード解除");
        assert_eq!(s.radar_uses, 1);
    }

    #[test]
    fn scanはコイン不足と調査済みマスと掘削済みマスで失敗する() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        s.coins = 10;
        assert!(!scan(&mut s, 0), "コイン不足");
        s.coins = 1000;
        assert!(scan(&mut s, 0));
        assert!(!scan(&mut s, 0), "調査済み");
        dig(&mut s, 1);
        assert!(!scan(&mut s, 1), "掘削済み");
    }

    #[test]
    fn scanは1日の上限で失敗する() {
        let mut s = DigState::new();
        fixed_site(&mut s);
        s.coins = 10_000;
        assert!(scan(&mut s, 0));
        assert!(scan(&mut s, 1));
        assert!(scan(&mut s, 2));
        assert!(!scan(&mut s, 3), "4回目は上限");
        assert_eq!(s.radar_uses, RADAR_MAX_PER_DAY);
    }

    #[test]
    fn maybe_reset_dayは日付が変わった時だけ新しい現場になる() {
        let mut s = DigState::new();
        setup_site(&mut s, 0);
        s.shovels = 0;
        s.dug[0] = true;
        s.coins = 500;
        s.museum_counts[0] = 3;

        assert!(!maybe_reset_day(&mut s, DAY_MS - 1), "同じ日");
        assert_eq!(s.shovels, 0);

        assert!(maybe_reset_day(&mut s, DAY_MS));
        assert_eq!(s.day, 1);
        assert_eq!(s.shovels, SHOVELS_PER_DAY);
        assert!(s.dug.iter().all(|d| !d));
        assert_eq!(s.treasures, generate_site(1));
        // 永続進行は保持
        assert_eq!(s.coins, 500);
        assert_eq!(s.museum_counts[0], 3);
    }

    #[test]
    fn setup_siteは1日単位の状態をすべてリセットする() {
        let mut s = DigState::new();
        s.scanned[3] = true;
        s.radar_uses = 2;
        s.radar_armed = true;
        s.perfect_bonus_given = true;
        s.flash = Some(Flash { cells: vec![1], ttl: 5 });
        setup_site(&mut s, 7);
        assert!(s.scanned.iter().all(|d| !d));
        assert_eq!(s.radar_uses, 0);
        assert!(!s.radar_armed);
        assert!(!s.perfect_bonus_given);
        assert!(s.flash.is_none());
    }

    #[test]
    fn tickはフラッシュを減衰させ0でnoneになる() {
        let mut s = DigState::new();
        s.flash = Some(Flash { cells: vec![1, 2], ttl: 5 });
        tick(&mut s, 3);
        assert_eq!(s.flash.as_ref().unwrap().ttl, 2);
        tick(&mut s, 10);
        assert!(s.flash.is_none());
    }

    #[test]
    fn museum_countsの配列長は種類数と一致する() {
        let s = DigState::new();
        assert_eq!(s.museum_counts.len(), KIND_COUNT);
    }
}
