//! Seed-based terrain generation for Idle Metropolis.
//!
//! 「マイクラのシード値生成」感を出すための地形レイヤー。建物とは独立に
//! 持ち、`grid[y][x] == Empty` の時だけ画面に見える。建物撤去等で再露出
//! する将来拡張も視野に入れて分離。
//!
//! ## 生成アルゴリズム
//!
//! 1. **Forest**: 初期 ~33% を森に塗布 → 4-近傍多数決 CA を 3 回 →
//!    有機的な塊が現れる (Conway 系)。
//! 2. **Lake**: 種点 1〜3 個をランダム配置し、各種点から BFS で
//!    膨らませて湖にする。Forest を上書きする (湖が森より優先)。
//! 3. **Wasteland**: 残った Plain のうち、孤立 (近傍に Forest/Lake が無い)
//!    セルの一部を荒地化。Plain と荒地が市松状に出ない自然なテクスチャ。
//!
//! 全て deterministic — 同じ seed なら毎回同じ地形。

use super::state::{GRID_H, GRID_W};

/// 何もない更地に対する自然な地表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    /// 平地 — 普通に建設できる。
    Plain,
    /// 森 — 緑地。建設可能 (将来は満足度 +)。
    Forest,
    /// 湖 — 建設不可。Shop 隣接で +1 収入 (将来拡張)。
    Water,
    /// 荒地 — 建設可能。コスト ↓ / 視覚的にひび割れ (将来は収入 -)。
    Wasteland,
}

impl Terrain {
    /// 建物がここに建てられるか。Water だけが false。
    pub fn buildable(self) -> bool {
        !matches!(self, Terrain::Water)
    }

    /// この地形は着工前に整地が必要か。Plain は不要、Forest/Wasteland は要整地、
    /// Water は buildable=false なので整地以前の問題。
    pub fn needs_clearing(self) -> bool {
        matches!(self, Terrain::Forest | Terrain::Wasteland)
    }

    /// 整地に必要な tick 数。Wasteland (荒地) は短く、Forest (伐採) は長め。
    /// `0` = 整地不要 (既に Plain)。
    pub fn clearing_ticks(self) -> u32 {
        match self {
            Terrain::Wasteland => 30,    // 3 sec — 表土を均すだけ
            Terrain::Forest => 60,       // 6 sec — 木を切り倒す
            Terrain::Plain | Terrain::Water => 0,
        }
    }

    /// 整地コスト (cash)。Wasteland は安く、Forest は高い (人手がかかる)。
    pub fn clearing_cost(self) -> i64 {
        match self {
            Terrain::Wasteland => 5,
            Terrain::Forest => 15,
            Terrain::Plain | Terrain::Water => 0,
        }
    }
}

/// 地形レイヤー。`generate(seed)` で初期化。
pub type TerrainLayer = Vec<Vec<Terrain>>;

/// Seed から決定論的に地形を生成。
pub fn generate(seed: u64) -> TerrainLayer {
    let mut rng = SmallRng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));

    // Phase 1: Forest を CA で生成。
    let forest = generate_forest(&mut rng);

    // Phase 2: Lake を BFS で生成。
    let lake = generate_lakes(&mut rng);

    // Phase 3: Wasteland を散布 (Forest/Lake が無い孤立セル中心)。
    let mut layer: TerrainLayer = vec![vec![Terrain::Plain; GRID_W]; GRID_H];
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if lake[y][x] {
                layer[y][x] = Terrain::Water;
            } else if forest[y][x] {
                layer[y][x] = Terrain::Forest;
            }
        }
    }
    sprinkle_wasteland(&mut layer, &mut rng);
    layer
}

/// 32-bit splitmix 風 — 軽量で予測可能、コードが短い。
struct SmallRng {
    state: u64,
}
impl SmallRng {
    fn new(seed: u64) -> Self {
        // 0 シードの zero-state を避けるため定数を OR。
        Self {
            state: seed | 0xA5A5_A5A5_0000_0001,
        }
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.state = x;
        x ^= x >> 30;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        x
    }
    fn next_pct(&mut self) -> u32 {
        (self.next_u64() % 100) as u32
    }
    fn range(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() as usize) % n
        }
    }
}

// ── Forest (CA) ─────────────────────────────────────────────

const FOREST_INITIAL_PCT: u32 = 33;
const FOREST_CA_ITERATIONS: u32 = 3;

// CA の近傍参照は (y, x) と (ny, nx) の二重 index アクセスが本質的。
// iterator/enumerate に書き換えるとむしろ可読性が下がるため抑制。
#[allow(clippy::needless_range_loop)]
fn generate_forest(rng: &mut SmallRng) -> Vec<Vec<bool>> {
    let mut grid: Vec<Vec<bool>> = (0..GRID_H)
        .map(|_| (0..GRID_W).map(|_| rng.next_pct() < FOREST_INITIAL_PCT).collect())
        .collect();

    // 多数決 CA。境界はクランプ (端は森扱いせず、cell 自身でカウント)。
    for _ in 0..FOREST_CA_ITERATIONS {
        let mut next = grid.clone();
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                let mut n = 0;
                let mut total = 0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                            continue;
                        }
                        total += 1;
                        if grid[ny as usize][nx as usize] {
                            n += 1;
                        }
                    }
                }
                // 多数決: 半分超なら森。8 近傍だと閾値 5、3-corner で 3。
                next[y][x] = n * 2 > total;
            }
        }
        grid = next;
    }
    grid
}

// ── Lake (BFS flood) ────────────────────────────────────────

/// マップ内に湖を 1〜3 個配置。各湖は 6〜14 セル程度。
fn generate_lakes(rng: &mut SmallRng) -> Vec<Vec<bool>> {
    let mut lake = vec![vec![false; GRID_W]; GRID_H];
    let lake_count = 1 + (rng.next_u64() % 3) as usize; // 1..=3
    for _ in 0..lake_count {
        let cx = rng.range(GRID_W);
        let cy = rng.range(GRID_H);
        // 湖は中心に近いほど水になりやすい確率場で広げる。
        let target_size = 6 + rng.range(9); // 6..=14
        flood_lake(&mut lake, cx, cy, target_size, rng);
    }
    lake
}

fn flood_lake(
    lake: &mut [Vec<bool>],
    cx: usize,
    cy: usize,
    target: usize,
    rng: &mut SmallRng,
) {
    let mut frontier: Vec<(usize, usize)> = vec![(cx, cy)];
    lake[cy][cx] = true;
    let mut placed = 1;
    while placed < target && !frontier.is_empty() {
        let idx = rng.range(frontier.len());
        let (x, y) = frontier.swap_remove(idx);
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            let (nxu, nyu) = (nx as usize, ny as usize);
            if lake[nyu][nxu] {
                continue;
            }
            // 中心から離れるほど水になりにくい (確率減衰)。
            let dist = (nx - cx as i32).abs() + (ny - cy as i32).abs();
            let p = 80u32.saturating_sub((dist as u32) * 12);
            if rng.next_pct() < p {
                lake[nyu][nxu] = true;
                frontier.push((nxu, nyu));
                placed += 1;
                if placed >= target {
                    break;
                }
            }
        }
    }
}

// ── Wasteland sprinkle ──────────────────────────────────────

/// Plain かつ近傍 1 セルに Forest/Water が「ない」セルを ~12% で荒地に。
fn sprinkle_wasteland(layer: &mut TerrainLayer, rng: &mut SmallRng) {
    let snapshot = layer.clone();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if snapshot[y][x] != Terrain::Plain {
                continue;
            }
            let lonely = !has_neighbor(&snapshot, x, y, |t| {
                matches!(t, Terrain::Forest | Terrain::Water)
            });
            if lonely && rng.next_pct() < 12 {
                layer[y][x] = Terrain::Wasteland;
            }
        }
    }
}

fn has_neighbor(layer: &TerrainLayer, x: usize, y: usize, pred: impl Fn(Terrain) -> bool) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if pred(layer[ny as usize][nx as usize]) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同じ seed は同じ地形を返す (deterministic)。
    #[test]
    fn deterministic_for_same_seed() {
        let a = generate(0xC1A5_5EED);
        let b = generate(0xC1A5_5EED);
        assert_eq!(a, b);
    }

    /// 異なる seed は (ほぼ確実に) 異なる地形を返す。
    #[test]
    fn varies_with_seed() {
        let a = generate(0xC1A5_5EED);
        let b = generate(0xDEAD_BEEF);
        assert_ne!(a, b);
    }

    /// 4 種すべてが平均的に出現する (極端な seed では取れないので、
    /// 16 種のシードを試して合算する)。
    #[test]
    fn all_terrains_appear_across_seeds() {
        let mut counts = [0usize; 4];
        for s in 0..16u64 {
            let layer = generate(s.wrapping_mul(0xABCD_1234));
            for row in &layer {
                for t in row {
                    match t {
                        Terrain::Plain => counts[0] += 1,
                        Terrain::Forest => counts[1] += 1,
                        Terrain::Water => counts[2] += 1,
                        Terrain::Wasteland => counts[3] += 1,
                    }
                }
            }
        }
        for (i, c) in counts.iter().enumerate() {
            assert!(*c > 0, "terrain index {} never appeared across 16 seeds", i);
        }
    }

    /// Buildable は Water 以外すべて。
    #[test]
    fn water_is_only_unbuildable() {
        assert!(!Terrain::Water.buildable());
        assert!(Terrain::Plain.buildable());
        assert!(Terrain::Forest.buildable());
        assert!(Terrain::Wasteland.buildable());
    }
}
