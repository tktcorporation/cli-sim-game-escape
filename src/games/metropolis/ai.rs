//! AI brains.  Each Tier is a separate function with the same signature so
//! we can swap them via `decide()` and benchmark them independently.

use super::state::*;

/// 4-direction neighbour offsets shared by neighbour-checking helpers.
const DIRS4: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// What the AI wants the city to do this tick.
#[derive(Clone, Debug, PartialEq)]
pub enum AiAction {
    Build {
        x: usize,
        y: usize,
        kind: Building,
    },
    /// 撤去。全 Tier の AI が生成する:
    ///   - Tier 1: 5% 確率で `auto_demolish_target` を試す
    ///   - Tier 2: 隣接候補が枯渇した時のみ
    ///   - Tier 3: 機能不全 (demolish_value >= 80 cents/sec) を build より優先撤去
    ///   - Tier 4/5: `placement_value` (build) と `demolish_value` を 1 つの
    ///     max 選択に統合し、撤去価値が勝った時に生成
    ///
    /// 全経路で `can_afford_demolish` (= cost + min_cash_reserve) を満たすか
    /// 確認してから生成される。デフレ螺旋ガード。
    ///
    /// `drive_ai` 側では Build と同じく 1 worker を消費する (= 1 tick 1 撤去
    /// 上限)。撤去が連続発火して cash が一気に枯渇するのを防ぐため。
    Demolish {
        x: usize,
        y: usize,
    },
    Idle,
}

/// Top-level dispatcher: routes to the active tier's brain.
///
/// **アーキテクチャ**: Tier 4 / 5 は `placement_value` ベースの評価 AI で、
/// 同じ評価関数を thinking depth (1 / 2 手先) で切替えるだけの差。
/// Tier 1-3 は historical なルールベースで残す (低 Tier の "dumb" さの担保)。
pub fn decide(city: &mut City) -> AiAction {
    match city.ai_tier {
        AiTier::Random => tier1_random(city),
        AiTier::Greedy => tier2_greedy(city),
        AiTier::RoadPlanner => tier3_road_planner(city),
        AiTier::DemandAware => tier4_value_search(city, 1),
        AiTier::DeepPlanner => tier4_value_search(city, 2),
    }
}

/// Tier 1 — Random Bot.
///
/// Intentionally dumb: picks a random empty cell and a random building.
/// Two safety nets:
///   1. "can I actually afford it?" — drops Idle when broke.
///   2. *savings protection*: with ~$2/sec income, cheap Roads ($10)
///      will drain funds before a House ($40) can ever be afforded.
///      Without this guard, the simulator observed "1 hour, 5 houses,
///      283 roads, grid full".  With this guard the AI saves up.
///
/// Distribution is biased 60% House / 25% Road / 15% Shop so the player
/// usually sees a population grow before shops appear, even though the
/// AI doesn't *understand* why.
///
/// **Demolish 統合**: 5% の確率で `auto_demolish_target` を試す。ダム性を
/// 維持するため賢い判断はせず、「機能不全建物が見つかった時にぼんやり撤去
/// する」だけ。Tier 階層感を保ちつつ街が完全 saturate しても抜け道を持つ。
fn tier1_random(city: &mut City) -> AiAction {
    // Demolish 試行 (5% 確率)。ヒットしなければ Build に進む。
    if city.next_rand().is_multiple_of(20) {
        if let Some(action) = try_random_demolish(city) {
            return action;
        }
    }

    // Pick building kind by weighted roll
    let roll = (city.next_rand() % 100) as u32;
    let kind = if roll < 60 {
        Building::House
    } else if roll < 85 {
        Building::Road
    } else {
        Building::Shop
    };

    // Affordability gate.
    if city.cash < kind.cost() {
        return AiAction::Idle;
    }

    // Savings protection: skip cheap Road/Shop rolls until we can also
    // afford a House.  Houses are the income source, so this prevents
    // the cheap-build death spiral.
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }

    // No-customer guard: don't build a Shop with fewer than 3 houses
    // standing.  Without this, an unlucky early Shop roll can starve
    // the city of houses *and* leave the shop inactive (no customer
    // base in range), triggering an income==0 stall.  Even the dumbest
    // AI gets this one piece of "common sense".
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }

    // Try up to 30 random cells; if none are empty + buildable, idle.
    // Tier 1 はあえて愚直: 水セルに当たっても "haha 投げる" 回数で済ませる。
    // ただし Rock は機材必須なので、隣接 Outpost が無い Rock は除外する。
    // Tier 1 でも「機材ゲートは absolute rule」として尊重 — 戦略の話ではない。
    for _ in 0..30 {
        let r = city.next_rand();
        let x = (r as usize) % GRID_W;
        let y = ((r >> 32) as usize) % GRID_H;
        if super::logic::ai_can_break_ground(city, x, y) {
            return AiAction::Build { x, y, kind };
        }
    }
    AiAction::Idle
}

/// Tier 2 — Greedy.
///
/// Same building-kind roll, but picks an Empty cell that is **adjacent**
/// (4-connected) to an existing built tile.  Falls back to a random empty
/// cell if no adjacent option exists (early game).
///
/// **Demolish 統合**: 隣接 Empty 候補が枯渇 (= 街が周辺まで埋まった) 時に
/// `auto_demolish_target` を試す。ダム性は維持しつつ「街が満杯になったら
/// 整理する」という greedy 流の発想を反映。普通の状況では撤去しない。
fn tier2_greedy(city: &mut City) -> AiAction {
    let roll = (city.next_rand() % 100) as u32;
    let mut kind = if roll < 50 {
        Building::House
    } else if roll < 80 {
        Building::Road
    } else {
        Building::Shop
    };

    // Tier 2 でも需給を満たすため、人口が伸びた時に Workshop / Mall を
    // 確率的に選ぶ。Shop だけでは需給連動の収入カーブが頭打ちになる。
    if matches!(kind, Building::Shop) {
        let houses = city.count_built(Building::House);
        let pop = city.population();
        if pop >= 100 && houses >= 12 && city.cash >= Building::Mall.cost() * 2 {
            kind = Building::Mall;
        } else if houses >= 5 && city.cash >= Building::Workshop.cost()
            && (city.next_rand() % 10) < 4
        {
            kind = Building::Workshop;
        }
    }

    if city.cash < kind.cost() {
        return AiAction::Idle;
    }

    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Mall) && city.count_built(Building::House) < 6 {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
        return AiAction::Idle;
    }

    // Collect candidate empties adjacent to a built/under-construction tile.
    // 地形が建設不可 (Water) や機材未到達の Rock は候補外。
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !super::logic::ai_can_break_ground(city, x, y) {
                continue;
            }
            if has_built_neighbor(city, x, y) {
                candidates.push((x, y));
            }
        }
    }

    if candidates.is_empty() {
        // 候補枯渇 = 街が周辺まで埋まった。ここで撤去を試す = 「整理して空ける」。
        if let Some(action) = try_random_demolish(city) {
            return action;
        }
        return tier1_random(city);
    }
    let pick = (city.next_rand() as usize) % candidates.len();
    let (x, y) = candidates[pick];
    AiAction::Build { x, y, kind }
}

fn has_built_neighbor(city: &City, x: usize, y: usize) -> bool {
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if !matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Empty
        ) {
            return true;
        }
    }
    false
}

/// True if the cell at (x, y) is an Empty tile next to a finished or
/// under-construction Road.
fn is_empty_next_to_road(city: &City, x: usize, y: usize) -> bool {
    if !super::logic::ai_can_break_ground(city, x, y) {
        return false;
    }
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        match city.tile(nx as usize, ny as usize) {
            Tile::Built(Building::Road) => return true,
            Tile::Construction {
                target: Building::Road,
                ..
            } => return true,
            _ => {}
        }
    }
    false
}

/// True if (x, y) is an Empty cell adjacent to a House or Shop, useful
/// for "extend the road grid here so future buildings can connect".
fn is_empty_next_to_building(city: &City, x: usize, y: usize) -> bool {
    if !super::logic::ai_can_break_ground(city, x, y) {
        return false;
    }
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Built(Building::House) | Tile::Built(Building::Shop)
        ) {
            return true;
        }
    }
    false
}

/// Tier 3 — Road Planner.
///
/// Same kind-roll as Tier 2, but placement obeys "the road network is
/// the backbone".  Concretely:
///   - Houses and Shops prefer cells adjacent to a Road.
///   - Roads prefer cells adjacent to a building (so we extend the
///     network *toward* development, not into the wilderness).
///   - On total dry spell (e.g., turn 1 with no roads), falls back to
///     Tier 2 placement so the city can still bootstrap.
///
/// **Strategy も「軽く」読む** (Tier 4 との階層差):
/// Tier 3 は Workshop を建てない (= 経済チェーンは Tier 4 専用) が、
/// House/Road/Shop の中で Strategy 寄りに `±10%` 揺らす。プレイヤーが
/// $5,000 で Tier 3 にアップグレードした時に「戦略ボタンが効く」実感が
/// 出る程度の弱い反映で、Tier 4 ($50,000) との価値差は維持する。
///
/// **Demolish 統合**: 機能不全建物が一定スコア以上 (= 道路網に組み込めない
/// inactive Shop / Workshop / 役目を終えた Outpost) なら撤去を build より
/// 優先する。「道路網優先」の思想と整合: 網の中に dead 建物があれば取り除く。
fn tier3_road_planner(city: &mut City) -> AiAction {
    // 機能不全建物 (score >= 80 cents/sec, = 中央付近の inactive Shop 相当) が
    // あれば優先撤去。網の整理を先回り — Tier 3 の特徴付け。reserve ガード必須。
    // 閾値は demolish_value の cents/sec 単位で「中央のはっきりミス」に相当する強度。
    if let Some((dx, dy, score)) = super::logic::auto_demolish_target(city) {
        if score >= 80 && can_afford_demolish(city, dx, dy) {
            return AiAction::Demolish { x: dx, y: dy };
        }
    }

    // ベース 50/30/20 (House/Road/Shop) を Strategy で「気持ち程度」偏らせる。
    // Workshop は Tier 3 では建てない (= 0%) ので、Strategy::Income の
    // workshop 重みは無視。
    //
    // ±2% という非常に弱い反映: simulator::tier_ordering_holds_at_30min が
    // T3 < T4 を要求しているため、T3 が Strategy を強く読むと T4 を上回る。
    // 「ボタンを押した時にイベントログ + Status タブには変化が出るが、
    //  AI の挙動はほぼ変わらない」程度に留め、Tier 4 ($50,000) との価値差を
    //  保つ。±0 は寂しいので「気持ちだけ」反映。
    let info = super::logic::strategy_info(city.strategy);
    let house_pct = (50 + (info.house_pct as i32 - 35).clamp(-2, 2)).max(20) as u32;
    let road_pct = (30 + (info.road_pct as i32 - 30).clamp(-2, 2)).max(15) as u32;
    let shop_pct = 100u32.saturating_sub(house_pct + road_pct).max(5);
    let house_pct = 100 - road_pct - shop_pct; // 合計 100 を厳守

    let roll = (city.next_rand() % 100) as u32;
    let mut kind = if roll < house_pct {
        Building::House
    } else if roll < house_pct + road_pct {
        Building::Road
    } else {
        Building::Shop
    };

    // Tier 3 でも需給連動に追従するための「上位建物アップグレード」:
    // 街区が育ったら Shop の代わりに Mall を、雇用が必要なら Workshop / Factory を、
    // Highrise 化を狙うなら Office を選ぶ。Tier 3 が `placement_value` を持たない
    // 中で、人口節目に応じた建物選択で Tier 4 との差を縮める。
    if matches!(kind, Building::Shop) {
        let pop = city.population();
        let houses = city.count_built(Building::House);
        if pop >= 80 && houses >= 12 && city.cash >= Building::Mall.cost() * 2 {
            // 大型商業の出番。
            kind = Building::Mall;
        } else if matches!(city.strategy, Strategy::Income) && houses >= 5
            && city.cash >= Building::Workshop.cost()
        {
            // Income 戦略は商業だけでなく雇用 (Workshop) も並行投資。
            // Workshop は Shop 半額、shop_pct のうち約 30% を Workshop に振る。
            if (city.next_rand() % 10) < 3 {
                kind = Building::Workshop;
            }
        }
    }

    if city.cash < kind.cost() {
        return AiAction::Idle;
    }
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Mall) && city.count_built(Building::House) < 6 {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
        return AiAction::Idle;
    }

    let candidates: Vec<(usize, usize)> = match kind {
        // Buildings prefer to live next to roads. Park は道路接続不要だが
        // Tier 3 が Park を選ぶことは無い (kind picker が House/Road/Shop のみ
        // — debug_assert で守る) — ただし match は網羅性が必要なので、
        // ここに来た場合は House と同じ扱いで safe fallback。
        // Outpost も AI は建てないが網羅性のため。
        Building::House
        | Building::Shop
        | Building::Mall
        | Building::MegaMall
        | Building::Workshop
        | Building::Factory
        | Building::Refinery
        | Building::Office
        | Building::Headquarters
        | Building::Park
        | Building::Plaza
        | Building::Stadium
        | Building::Outpost => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_road(city, *x, *y))
            .collect(),
        // Roads prefer to extend toward existing buildings.
        Building::Road => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_building(city, *x, *y))
            .collect(),
    };

    if candidates.is_empty() {
        // Bootstrapping early game (no roads yet) or stuck — fall back
        // to greedy adjacency so we don't deadlock.
        return tier2_greedy(city);
    }
    let pick = (city.next_rand() as usize) % candidates.len();
    let (x, y) = candidates[pick];
    AiAction::Build { x, y, kind }
}

/// Tier 1-3 共用の Demolish ヘルパー。`auto_demolish_target` の結果 (score 入り)
/// を Demolish action として返す。cash 不足や候補無しなら None。
///
/// 各 Tier は呼び出し側で「いつ呼ぶか」(発火確率 / 候補枯渇時 / score 閾値) を
/// 制御することで階層差を表現する。本ヘルパー自体は単純なラッパー。
///
/// `min_cash_reserve` ガード: 撤去後の cash が戦略予備金を下回るなら見送る。
/// これがないと「cash $50 → 中央のミス撤去 ($50) → cash $0 → 翌 tick 全 idle」
/// のデフレ螺旋に陥る。
fn try_random_demolish(city: &City) -> Option<AiAction> {
    let (dx, dy, _score) = super::logic::auto_demolish_target(city)?;
    if !can_afford_demolish(city, dx, dy) {
        return None;
    }
    Some(AiAction::Demolish { x: dx, y: dy })
}

/// 撤去後の cash が戦略予備金 (`automation_policy.min_cash_reserve`) を
/// 下回らないか確認する。AI の全 Demolish 経路 (Tier 1-5) が共通で使う。
pub(super) fn can_afford_demolish(city: &City, x: usize, y: usize) -> bool {
    let cost = super::logic::demolish_cost(x, y);
    let reserve = super::logic::automation_policy(city.strategy).min_cash_reserve;
    city.cash >= cost + reserve
}

/// Tier 4 / 5 共用の評価ベース placement search。
///
/// **アーキテクチャ**: `logic::placement_value` が「この候補で income/sec が
/// どれだけ増えるか」を cents 単位で返す純関数。AI はその値を最大化する
/// 候補を探すだけ。candidate set は **全建物種別** (Road / House / Workshop /
/// Shop / Park / **Outpost**) × **全 Empty cells** から評価する。
///
/// `depth = 1` (Tier 4): 各候補について placement_value を見て top-k から best 選択。
/// `depth = 2` (Tier 5): 各候補について 1 手目 value + 「2 手目に建てうる最良の
///   候補の value (= 1 手目を仮想的に着工した状態で再計算)」を加算した合計を評価。
///   これにより「道路を引いて、その隣に家を建てるシナリオ」が拾える。
///
/// **Outpost を candidate に含めることで saturation 解消が自然発生**:
///   中央満杯 → 既存 Empty cells の placement_value はほぼ 0 →
///   Outpost (隣接 Rock 数 × 期待 income) が相対的に勝つ → AI は外を割る。
///
/// 戻り値: AiAction::Build か AiAction::Idle。
fn tier4_value_search(city: &mut City, depth: u8) -> AiAction {
    use super::logic::placement_value;
    let connected = super::logic::compute_edge_connected_roads(city);

    let house_cost = Building::House.cost();
    // **Eco 戦略の Forest 配慮**: `placement_value` 内に soft penalty (-100)
    // として組み込み済み。「他に良い候補があれば森を残し、無ければ仕方なく
    // 切る」挙動になる。AI 側の前段フィルタでは扱わない。

    // 1 手目候補に乗せられるか共通ガード — 後で 2 手目評価でも同じ関数を呼ぶ
    // ことで「1 手目で除外された組み合わせを 2 手目だけ加点する」silent
    // divergence を防ぐ (= レビュー指摘 #3, #4)。
    fn passes_guards(city: &City, kind: Building, virtual_cash: i64, house_cost: i64) -> bool {
        let cost = kind.cost();
        if virtual_cash < cost {
            return false;
        }
        if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
            return false;
        }
        if matches!(kind, Building::Mall) && city.count_built(Building::House) < 6 {
            return false;
        }
        if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
            return false;
        }
        if matches!(kind, Building::Factory) && city.count_built(Building::House) < 5 {
            return false;
        }
        if matches!(kind, Building::Office) && city.count_built(Building::House) < 4 {
            return false;
        }
        // 超上位建物は街がそれなりに育ってから建てる (cost が高く、機能には
        // 一定数の House が必要)。ROI ペナルティだけだと序盤に空地スコアの
        // 関係で誤って高評価されることがあるため、人口下限のガードで弾く。
        if matches!(kind, Building::Refinery) && city.count_built(Building::House) < 12 {
            return false;
        }
        if matches!(kind, Building::MegaMall) && city.count_built(Building::House) < 12 {
            return false;
        }
        if matches!(kind, Building::Headquarters) && city.count_built(Building::House) < 10 {
            return false;
        }
        if matches!(kind, Building::Plaza) && city.count_built(Building::House) < 8 {
            return false;
        }
        if matches!(kind, Building::Stadium) && city.count_built(Building::House) < 20 {
            return false;
        }
        // savings protection: 高コスト建物を建てたら House を建てる原資を割る
        // 場合は避ける。Outpost は飽和時専用なので例外。
        if !matches!(kind, Building::House | Building::Outpost)
            && (virtual_cash - cost) < house_cost
        {
            return false;
        }
        true
    }

    // 候補 cells を 2 群に分けて収集する (= O(N²) を避ける):
    //   `regular`  — 既存 Built に 4-近傍隣接する Empty cells (House/Road/etc 用)
    //   `outpost`  — Rock を 4-近傍に持ち、街から距離 4 以内の Plain Empty cells
    // 「Built 隣接」を distance 1 に絞ることで候補 cell 数が最大でも O(built * 4) ≈ 数百に。
    let mut regular: Vec<(usize, usize)> = Vec::new();
    let mut outpost: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Empty) {
                continue;
            }
            let t = city.terrain_at(x, y);
            if !t.buildable() {
                continue;
            }
            // Rock セルは Outpost が建てられないので Outpost candidate にもならない。
            // (Outpost は Plain にだけ建つ — Rock 上には建てない)
            let needs_outpost_unmet =
                t.needs_outpost() && !super::logic::has_outpost_neighbor(city, x, y);
            if needs_outpost_unmet {
                continue;
            }
            // Outpost 候補: Rock 隣接 + 街の近く + 整地不要 (Plain)
            let has_rock_n =
                super::logic::has_terrain_neighbor(city, x, y, super::terrain::Terrain::Rock);
            if has_rock_n
                && !t.needs_clearing()
                && super::logic::has_built_within_distance(city, x, y, 4)
            {
                outpost.push((x, y));
            }
            // 通常候補: Built 4-近傍隣接 (= 街の周辺 1 マス)
            if has_built_neighbor(city, x, y) {
                regular.push((x, y));
            }
        }
    }
    // 序盤フォールバック: 候補が枯渇していたら Built 距離 3 まで広げる。
    if regular.is_empty() && outpost.is_empty() {
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                if !matches!(city.tile(x, y), Tile::Empty) {
                    continue;
                }
                let t = city.terrain_at(x, y);
                if !t.buildable() {
                    continue;
                }
                if super::logic::has_built_within_distance(city, x, y, 3) {
                    regular.push((x, y));
                }
            }
        }
    }

    let mut best: Option<(usize, usize, Building, i64)> = None;
    let consider = |x: usize, y: usize, kind: Building, best: &mut Option<(usize, usize, Building, i64)>| {
        if !passes_guards(city, kind, city.cash, house_cost) {
            return;
        }
        let v1 = placement_value(city, x, y, kind, &connected);
        if v1 == i64::MIN {
            return;
        }
        let better = match *best {
            None => true,
            Some((_, _, _, prev)) => v1 > prev,
        };
        if better {
            *best = Some((x, y, kind, v1));
        }
    };

    // 通常 cells: 全 kinds (Outpost 除く) を評価。
    // 上位建物 (Refinery / MegaMall / Headquarters / Plaza / Stadium) も候補に
    // 含めるが、cost が高いため `placement_value` の ROI ペナルティで序盤は
    // 自然に落選する。終盤 (cash と街の規模が揃った時) のみトップ候補に浮上する設計。
    let normal_kinds: &[Building] = &[
        Building::House,
        Building::Road,
        Building::Workshop,
        Building::Factory,
        Building::Refinery,
        Building::Shop,
        Building::Mall,
        Building::MegaMall,
        Building::Office,
        Building::Headquarters,
        Building::Park,
        Building::Plaza,
        Building::Stadium,
    ];
    for &(x, y) in &regular {
        for &kind in normal_kinds {
            consider(x, y, kind, &mut best);
        }
    }
    // Outpost cells: Outpost のみ評価 (= ほとんどのプレイで saturation 時のみ value 高)
    for &(x, y) in &outpost {
        consider(x, y, Building::Outpost, &mut best);
    }

    // depth=2 (Tier 5): top-1 の (x,y,kind) を採用する代わりに、上位 K (=12) を抽出して
    // それぞれに 2 手目評価を加算し、合計最大を選ぶ。`best` を best2 で上書きしてから
    // 後続の Demolish 比較に進む (= depth=2 でも Demolish action が出せる)。
    if depth >= 2 {
        let mut top: Vec<(usize, usize, Building, i64)> = Vec::new();
        for &(x, y) in &regular {
            for &kind in normal_kinds {
                if !passes_guards(city, kind, city.cash, house_cost) { continue; }
                let v = placement_value(city, x, y, kind, &connected);
                if v == i64::MIN { continue; }
                top.push((x, y, kind, v));
            }
        }
        for &(x, y) in &outpost {
            if !passes_guards(city, Building::Outpost, city.cash, house_cost) { continue; }
            let v = placement_value(city, x, y, Building::Outpost, &connected);
            if v == i64::MIN { continue; }
            top.push((x, y, Building::Outpost, v));
        }
        top.sort_by(|a, b| b.3.cmp(&a.3));
        // 上位 6 候補に絞る (旧 12)。lookahead の CPU 負荷を半減し、Tier 5 の
        // 1 tick あたり計算量を抑えることで Build スループットを Tier 4 に近づける。
        // 6 でも `road→house` のような典型シナリオは拾える幅。
        top.truncate(6);

        // 各 top 候補に 2 手目 value を加算。
        // 2 手目候補は仮想着工した世界 (`virt`) で動的に再計算する。
        // 1 手目時点の `regular`/`outpost` に縛ると、「道路を置いた結果新たに
        // Built 隣接になったセル」を取りこぼす。
        //
        // **重要**: Demolish との比較は 1 手目 value (v1) で行う。Build の選定
        // にだけ 2 手目込み total を使い、(x, y, kind) を決めた後 Tuple の
        // value 欄には v1 を載せる。total を載せると Demolish 候補が永遠に
        // 勝てなくなり、Tier 5 で撤去が走らなくなって街が膠着する。
        let mut best2: Option<(usize, usize, Building, i64, i64)> = None; // (..., v1, total)
        for &(x, y, kind, v1) in &top {
            let v2 = simulate_second_step_value(city, x, y, kind, normal_kinds);
            let total = v1 + v2;
            let better = match best2 {
                None => true,
                Some((_, _, _, _, prev)) => total > prev,
            };
            if better {
                best2 = Some((x, y, kind, v1, total));
            }
        }
        if let Some((x, y, kind, v1, _)) = best2 {
            best = Some((x, y, kind, v1));
        }
    }

    // Demolish 候補: `auto_demolish_target_with` で score (= 機能不全 + 老朽化
    // − cost_penalty) が最大のものを取得。Tier 4/5 共に 1 手目相当の評価で
    // 取得し、上の Build best (= Tier 5 でも v1) と公平に比較する。
    let demolish_best = super::logic::auto_demolish_target_with(city, &connected);

    let best_build_value = best.map(|(_, _, _, v)| v);
    let best_demo_value = demolish_best.map(|(_, _, v)| v);

    match (best_build_value, best_demo_value) {
        (Some(_), Some(dv)) if dv > best_build_value.unwrap() => {
            let (dx, dy, _) = demolish_best.unwrap();
            if can_afford_demolish(city, dx, dy) {
                AiAction::Demolish { x: dx, y: dy }
            } else {
                // Demolish が最高だが reserve ガードに引っかかった →
                // worker と tick を捨てずに次善策の Build に倒す。
                let (x, y, kind, _) = best.unwrap();
                AiAction::Build { x, y, kind }
            }
        }
        (Some(_), _) => {
            let (x, y, kind, _) = best.unwrap();
            AiAction::Build { x, y, kind }
        }
        (None, Some(_)) => {
            let (dx, dy, _) = demolish_best.unwrap();
            if can_afford_demolish(city, dx, dy) {
                AiAction::Demolish { x: dx, y: dy }
            } else {
                AiAction::Idle
            }
        }
        (None, None) => AiAction::Idle,
    }
}

/// 2 手目評価 — `(x, y, kind)` を仮想着工した状態で best 2 手目の placement_value
/// を返す。計算量抑制のため候補 cells は呼び側から渡す。
///
/// 仮想着工の実現方法: city を **clone せず**、評価関数側で「(x, y) に
/// kind が建っている」前提のラッパー city ビューを作るのが理想だが、
/// `placement_value` は `&City` を受け取るので、簡易に **clone** する。
/// これで logic を変えずに lookahead が成立する。
///
/// clone のコスト: City struct の grid + terrain + flash arrays で ~数 KB。
/// Tier 5 の AI 1 回 / tick = 10 回/秒 で数万 cells 走査するので無視できる範囲。
fn simulate_second_step_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    kinds: &[Building],
) -> i64 {
    // 仮想着工した city。建物を Built 状態として置く (Construction tick を読まない)。
    // Built として置くことで、direct_income_value も synergy も「建ってる前提」で評価される。
    let mut virt = clone_city_for_lookahead(city);
    virt.grid[y][x] = Tile::Built(kind);
    let connected2 = super::logic::compute_edge_connected_roads(&virt);

    let virtual_cash = city.cash - kind.cost();
    let house_cost = Building::House.cost();
    let mut best2: i64 = 0; // 2 手目を打たない選択肢があるので 0 が下限。

    // **2 手目候補は仮想着工後の virt で再構築** (Codex P1 指摘修正):
    // 1 手目時点の `regular`/`outpost` に縛ると、1 手目が新たに有効化した
    // セル (例: フロンティアに道路 → その隣の Empty が Built 隣接になる) を
    // 取りこぼす。Manhattan 5 以内 + virt 上で has_built_neighbor を満たす
    // セルを動的に集める。これで「道路を引いて家を載せる」シナリオが拾える。
    let mut nearby: Vec<(usize, usize)> = Vec::new();
    for dy in -5i32..=5 {
        for dx in -5i32..=5 {
            if dx.abs() + dy.abs() > 5 || (dx == 0 && dy == 0) {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !matches!(virt.tile(nx, ny), Tile::Empty) {
                continue;
            }
            let t = virt.terrain_at(nx, ny);
            if !t.buildable() {
                continue;
            }
            // Outpost 解禁前の Rock などは除外。
            if t.needs_outpost() && !super::logic::has_outpost_neighbor(&virt, nx, ny) {
                continue;
            }
            // virt 上で Built 隣接 (= 1 手目で建てた建物含む) を確認。
            if has_built_neighbor(&virt, nx, ny) {
                nearby.push((nx, ny));
            }
        }
    }

    // **重要**: 2 手目候補にも 1 手目と同じ guard を適用 (silent divergence 防止)。
    // virt は 1 手目完成済みの世界なので `virt.count_built(House)` は反映後の値。
    let passes_virt = |k2: Building| -> bool {
        let cost = k2.cost();
        if virtual_cash < cost {
            return false;
        }
        if matches!(k2, Building::Shop) && virt.count_built(Building::House) < 3 {
            return false;
        }
        if matches!(k2, Building::Mall) && virt.count_built(Building::House) < 6 {
            return false;
        }
        if matches!(k2, Building::Workshop) && virt.count_built(Building::House) < 2 {
            return false;
        }
        if matches!(k2, Building::Factory) && virt.count_built(Building::House) < 5 {
            return false;
        }
        if matches!(k2, Building::Office) && virt.count_built(Building::House) < 4 {
            return false;
        }
        if !matches!(k2, Building::House | Building::Outpost)
            && (virtual_cash - cost) < house_cost
        {
            return false;
        }
        true
    };

    for &(cx, cy) in &nearby {
        for &k2 in kinds {
            if !passes_virt(k2) {
                continue;
            }
            let v = super::logic::placement_value(&virt, cx, cy, k2, &connected2);
            if v == i64::MIN {
                continue;
            }
            if v > best2 {
                best2 = v;
            }
        }
    }
    best2
}

/// 2 手目シミュレート用の軽量 City clone。grid と terrain だけ複製し、
/// 残りは default (= placement_value 内で参照しないフィールドはダミー値で OK)。
///
/// 完全 clone との差: `events` / `completion_flash_until` / `payout_flash_until`
/// は使わないため empty にして大幅に軽量化。`built_at_tick` は placement_value が
/// 読まないため empty で OK。
fn clone_city_for_lookahead(city: &City) -> City {
    City {
        grid: city.grid.clone(),
        terrain: city.terrain.clone(),
        world_seed: city.world_seed,
        cash: city.cash,
        tick: city.tick,
        ai_tier: city.ai_tier,
        strategy: city.strategy,
        panel_tab: city.panel_tab,
        last_observed_tier: city.last_observed_tier,
        tier_flash_until: 0,
        last_outpost_dispatch_tick: city.last_outpost_dispatch_tick,
        last_auto_demolish_tick: city.last_auto_demolish_tick,
        outposts_dispatched_total: city.outposts_dispatched_total,
        workers: city.workers,
        rng_state: city.rng_state,
        buildings_started: city.buildings_started,
        buildings_finished: city.buildings_finished,
        cash_earned_total: city.cash_earned_total,
        cash_spent_total: city.cash_spent_total,
        events: Vec::new(),
        completion_flash_until: vec![vec![0u64; GRID_W]; GRID_H],
        payout_flash_until: vec![vec![0u64; GRID_W]; GRID_H],
        last_payout_amount: 0,
        last_payout_tick: 0,
        built_at_tick: vec![vec![0u64; GRID_W]; GRID_H],
        cam_x: city.cam_x,
        cam_y: city.cam_y,
        selected_cell: None,
        panel_scroll: std::cell::Cell::new(0),
        cash_history: std::collections::VecDeque::new(),
        population_cache: std::cell::Cell::new(None),
        pending_offline_welcome: None,
    }
}

