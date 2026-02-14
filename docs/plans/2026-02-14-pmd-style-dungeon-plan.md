# PMD-Style Dungeon Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** ダンジョンをポケモン不思議のダンジョン風の「部屋+通路」型に全面改修し、視界・操作・マップ生成を一括で改善する。

**Architecture:** タイルベースのマップ（Wall/RoomFloor/Corridor）にセクション分割型生成を適用。部屋認識型の視界システムで「部屋に入ったら全体が見える」体験を実現。facing廃止で直感的な4方向移動に。

**Tech Stack:** Rust, ratzilla (ratatui wrapper), wasm32-unknown-unknown

---

## Task 1: データモデル変更 (state.rs)

**Files:**
- Modify: `src/games/rpg/state.rs:381-453`

**Step 1: `Tile` enum を追加**

`CellType` enum（line 381）の直前に追加:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tile {
    Wall,
    RoomFloor,
    Corridor,
}
```

**Step 2: `MapCell` 構造体を変更**

lines 403-430 を置換:

```rust
#[derive(Clone, Debug)]
pub struct MapCell {
    pub tile: Tile,
    pub cell_type: CellType,
    pub visited: bool,
    /// 視界に入ったことがある（auto-map表示用）
    pub revealed: bool,
    pub event_done: bool,
    /// 所属する部屋ID（Noneなら通路 or 壁）
    pub room_id: Option<u8>,
}

impl MapCell {
    pub fn is_walkable(&self) -> bool {
        self.tile != Tile::Wall
    }
}
```

`wall()` / `set_wall()` メソッドを削除。隣接タイルが Wall かで判定する新方式。

**Step 3: `DungeonMap` 構造体を変更**

lines 432-453 を更新。`facing` を `last_dir` にリネーム:

```rust
#[derive(Clone, Debug)]
pub struct DungeonMap {
    pub floor_num: u32,
    pub width: usize,
    pub height: usize,
    pub grid: Vec<Vec<MapCell>>,
    pub player_x: usize,
    pub player_y: usize,
    /// 最後に移動した方向（テキスト描写で使用）
    pub last_dir: Facing,
    /// 各部屋の情報 (room_id → 部屋の矩形範囲)
    pub rooms: Vec<Room>,
}

#[derive(Clone, Debug)]
pub struct Room {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl DungeonMap {
    pub fn cell(&self, x: usize, y: usize) -> &MapCell {
        &self.grid[y][x]
    }
    pub fn cell_mut(&mut self, x: usize, y: usize) -> &mut MapCell {
        &mut self.grid[y][x]
    }
    pub fn player_cell(&self) -> &MapCell {
        &self.grid[self.player_y][self.player_x]
    }
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }
    /// プレイヤーが部屋の中にいるか
    pub fn player_room_id(&self) -> Option<u8> {
        self.player_cell().room_id
    }
}
```

**Step 4: コンパイルエラーを修正**

`wall()`, `set_wall()`, `facing` への参照が壊れるが、他のタスクで修正する。
この時点では `cargo check` がエラーを出すのは期待通り。

**Step 5: コミット**

```bash
git add src/games/rpg/state.rs
git commit -m "refactor: replace wall-based MapCell with tile-based model for PMD-style dungeon"
```

---

## Task 2: ダンジョン生成 (dungeon_map.rs) — 全面書き換え

**Files:**
- Rewrite: `src/games/rpg/dungeon_map.rs` (全体)

**Step 1: マップサイズを拡大**

```rust
pub fn map_size(floor: u32) -> (usize, usize) {
    match floor {
        1..=2 => (27, 27),
        3..=5 => (33, 33),
        6..=9 => (39, 39),
        _ => (27, 27), // F10: ボス戦
    }
}

fn section_count(_floor: u32) -> usize {
    3 // 常に 3×3 セクション
}
```

**Step 2: セクション分割型生成アルゴリズム**

```rust
pub fn generate_map(floor: u32, rng_seed: &mut u64) -> DungeonMap {
    let (w, h) = map_size(floor);
    let sections = section_count(floor);
    let sec_w = w / sections;
    let sec_h = h / sections;

    // 1. 全てを壁で初期化
    let mut grid = vec![vec![MapCell { tile: Tile::Wall, ... }; w]; h];

    // 2. 各セクションに部屋を配置（一部は空）
    let mut rooms = Vec::new();
    for sy in 0..sections {
        for sx in 0..sections {
            // 7-8/9 のセクションに部屋を配置
            if should_place_room(rng_seed, sx, sy, sections) {
                let room = generate_room_in_section(
                    &mut grid, sx, sy, sec_w, sec_h, rooms.len() as u8, rng_seed
                );
                rooms.push(room);
            }
        }
    }

    // 3. 隣接する部屋を通路で接続
    connect_rooms(&mut grid, &rooms, sections, sec_w, sec_h, rng_seed);

    // 4. 入口（下段中央の部屋）・階段（最遠の部屋）を配置
    // 5. イベント配置（部屋の床タイルに）

    DungeonMap { ..., rooms, last_dir: Facing::North }
}
```

**アルゴリズム詳細:**

**部屋配置**: 各セクション内にマージン1を確保して 4×4～7×7 の矩形を刻む。
部屋内タイルを `Tile::RoomFloor`, `room_id: Some(id)` に設定。

**通路接続**: 隣接セクション間で、部屋の壁からセクション境界まで直線、
境界から隣の部屋の壁まで直線（L字接続）。通路タイルは `Tile::Corridor`。

**入口**: 下段中央セクション (sx=1, sy=2) の部屋中央を Entrance に。
**階段**: BFS で入口から最も遠い部屋の中央を Stairs に。

**イベント配置**: `place_rooms()` を流用。候補を `RoomFloor` タイルに限定。

**Step 3: テスト**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_map_has_rooms() {
        let mut seed = 12345u64;
        let map = generate_map(1, &mut seed);
        assert_eq!(map.width, 27);
        assert_eq!(map.height, 27);
        assert!(!map.rooms.is_empty());
        // 部屋の床タイルが存在する
        let floor_count = map.grid.iter().flatten()
            .filter(|c| c.tile == Tile::RoomFloor).count();
        assert!(floor_count > 30, "部屋の床タイルが少なすぎる: {}", floor_count);
    }

    #[test]
    fn entrance_and_stairs_exist() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let has_entrance = map.grid.iter().flatten()
            .any(|c| c.cell_type == CellType::Entrance);
        let has_stairs = map.grid.iter().flatten()
            .any(|c| c.cell_type == CellType::Stairs);
        assert!(has_entrance);
        assert!(has_stairs);
    }

    #[test]
    fn rooms_are_connected() {
        // BFS from entrance should reach stairs
        let mut seed = 99u64;
        let map = generate_map(1, &mut seed);
        let reachable = bfs_reachable(&map);
        let stairs_pos = map.grid.iter().enumerate().find_map(|(y, row)| {
            row.iter().enumerate().find_map(|(x, c)| {
                if c.cell_type == CellType::Stairs { Some((x, y)) } else { None }
            })
        }).unwrap();
        assert!(reachable[stairs_pos.1][stairs_pos.0],
            "階段に到達できない");
    }

    #[test]
    fn map_size_scales_with_floor() {
        assert_eq!(map_size(1), (27, 27));
        assert_eq!(map_size(5), (33, 33));
        assert_eq!(map_size(8), (39, 39));
        assert_eq!(map_size(10), (27, 27));
    }
}
```

**Step 4: コミット**

```bash
git add src/games/rpg/dungeon_map.rs
git commit -m "feat: section-based PMD-style dungeon generation with rooms and corridors"
```

---

## Task 3: 移動ロジック変更 (logic.rs)

**Files:**
- Modify: `src/games/rpg/logic.rs:150-397`

**Step 1: `enter_dungeon` を新データモデルに対応**

line 161 の `generate_map` 呼び出し周辺を更新。
`map.grid[py][px].visited = true` はそのまま。
`revealed` も `true` に設定。初期 visibility で部屋全体を `revealed = true` に。

**Step 2: `move_forward` → `try_move` に書き換え**

facing ベースの前進を、絶対方向の移動に変更:

```rust
/// 指定方向に1歩移動する。壁なら移動不可。
pub fn try_move(state: &mut RpgState, dir: Facing) -> bool {
    let (can_move, nx, ny) = {
        let map = match &state.dungeon {
            Some(m) => m,
            None => return false,
        };
        let nx = map.player_x as i32 + dir.dx();
        let ny = map.player_y as i32 + dir.dy();
        if !map.in_bounds(nx, ny) {
            (false, 0, 0)
        } else {
            let target = map.cell(nx as usize, ny as usize);
            (target.is_walkable(), nx as usize, ny as usize)
        }
    };

    if !can_move {
        state.add_log("壁だ。");
        return false;
    }

    let map = state.dungeon.as_mut().unwrap();
    map.player_x = nx;
    map.player_y = ny;
    map.last_dir = dir;

    let was_visited = map.grid[ny][nx].visited;
    map.grid[ny][nx].visited = true;
    map.grid[ny][nx].revealed = true;

    // 部屋に入ったら部屋全体を revealed に
    if let Some(rid) = map.grid[ny][nx].room_id {
        reveal_room(map, rid);
    }

    if !was_visited {
        state.run_rooms_explored += 1;
    }

    // イベントトリガー（既存ロジック流用）
    check_cell_event(state, nx, ny);
    true
}

fn reveal_room(map: &mut DungeonMap, room_id: u8) {
    for row in map.grid.iter_mut() {
        for cell in row.iter_mut() {
            if cell.room_id == Some(room_id) {
                cell.revealed = true;
            }
        }
    }
}
```

**Step 3: `turn_left`, `turn_right`, `turn_around` を削除**

facing が無くなるのでターン関数は不要。

**Step 4: `auto_walk_direction` を新モデルに対応**

`cell.wall(dir)` → `!map.cell(nx, ny).is_walkable()` に変更。

**Step 5: テスト**

```rust
#[test]
fn try_move_blocked_by_wall() {
    let mut state = make_test_state();
    // プレイヤーの北が壁の場合
    let map = state.dungeon.as_ref().unwrap();
    let nx = map.player_x as i32 + Facing::North.dx();
    let ny = map.player_y as i32 + Facing::North.dy();
    if map.in_bounds(nx, ny) && !map.cell(nx as usize, ny as usize).is_walkable() {
        assert!(!try_move(&mut state, Facing::North));
    }
}

#[test]
fn entering_room_reveals_all_tiles() {
    let mut state = make_test_state();
    // 部屋に入った後、同じroom_idのタイルが全てrevealedになる
    let map = state.dungeon.as_ref().unwrap();
    if let Some(rid) = map.player_cell().room_id {
        let all_revealed = map.grid.iter().flatten()
            .filter(|c| c.room_id == Some(rid))
            .all(|c| c.revealed);
        assert!(all_revealed);
    }
}
```

**Step 6: コミット**

```bash
git add src/games/rpg/logic.rs
git commit -m "refactor: replace facing-based movement with direct 4-directional movement"
```

---

## Task 4: 視界システム + 描写テキスト (dungeon_view.rs 前半)

**Files:**
- Modify: `src/games/rpg/dungeon_view.rs:1-103`

**Step 1: `compute_view` を新モデルに対応**

`map.facing` → `map.last_dir` に変更。
`cell.wall(facing)` → 前方タイルが `Tile::Wall` かで判定:

```rust
pub fn compute_view(map: &DungeonMap) -> ViewData {
    let mut depths = Vec::new();
    let mut x = map.player_x as i32;
    let mut y = map.player_y as i32;
    let dir = map.last_dir;

    for _depth in 0..4 {
        let nx = x + dir.dx();
        let ny = y + dir.dy();
        if !map.in_bounds(nx, ny) {
            depths.push(DepthSlice {
                wall_front: true,
                cell_type: map.cell(x as usize, y as usize).cell_type,
            });
            break;
        }
        let next = map.cell(nx as usize, ny as usize);
        let blocked = next.tile == Tile::Wall;
        depths.push(DepthSlice {
            wall_front: blocked,
            cell_type: map.cell(x as usize, y as usize).cell_type,
        });
        if blocked { break; }
        x = nx;
        y = ny;
    }
    ViewData { depths }
}
```

**Step 2: `describe_view` はそのまま（変更不要）**

**Step 3: 視界計算関数を追加**

```rust
use std::collections::HashSet;

/// プレイヤーから現在見えるタイル座標のセットを返す
pub fn compute_visibility(map: &DungeonMap) -> HashSet<(usize, usize)> {
    let mut visible = HashSet::new();
    let px = map.player_x;
    let py = map.player_y;

    if let Some(rid) = map.grid[py][px].room_id {
        // 部屋内: 同じ部屋の全タイル + 出口周辺1マス
        for y in 0..map.height {
            for x in 0..map.width {
                if map.grid[y][x].room_id == Some(rid) {
                    visible.insert((x, y));
                    // 隣接マスも1つ見える（出口の先の通路）
                    for &d in &[Facing::North, Facing::East, Facing::South, Facing::West] {
                        let ax = x as i32 + d.dx();
                        let ay = y as i32 + d.dy();
                        if map.in_bounds(ax, ay) {
                            visible.insert((ax as usize, ay as usize));
                        }
                    }
                }
            }
        }
    } else {
        // 通路内: 周囲2マス
        for dy in -2..=2i32 {
            for dx in -2..=2i32 {
                let nx = px as i32 + dx;
                let ny = py as i32 + dy;
                if map.in_bounds(nx, ny) {
                    visible.insert((nx as usize, ny as usize));
                }
            }
        }
    }

    visible
}
```

**Step 4: テスト**

```rust
#[test]
fn visibility_in_room_sees_whole_room() {
    let mut seed = 42u64;
    let map = generate_map(1, &mut seed);
    // プレイヤーは部屋内にいるはず
    let vis = compute_visibility(&map);
    if let Some(rid) = map.player_cell().room_id {
        let room_tiles: Vec<_> = map.grid.iter().enumerate()
            .flat_map(|(y, row)| row.iter().enumerate()
                .filter(|(_, c)| c.room_id == Some(rid))
                .map(move |(x, _)| (x, y)))
            .collect();
        for &(x, y) in &room_tiles {
            assert!(vis.contains(&(x, y)),
                "部屋タイル ({},{}) が見えていない", x, y);
        }
    }
}

#[test]
fn visibility_in_corridor_limited() {
    // 通路にいるとき、遠くは見えない
    let vis_size_limit = 5 * 5; // 2マス radius → 最大25タイル
    // 実際のテストはマップ生成後にプレイヤーを通路に移動して検証
}
```

**Step 5: コミット**

```bash
git add src/games/rpg/dungeon_view.rs
git commit -m "feat: add room-aware visibility system for PMD-style dungeon"
```

---

## Task 5: 2Dマップレンダリング書き換え (dungeon_view.rs 後半)

**Files:**
- Rewrite: `src/games/rpg/dungeon_view.rs:118-283`

**Step 1: `render_map_2d` を全面書き換え**

壁ベース (3col×2row per cell) → タイルベース (2col×1row per tile):

```rust
pub fn render_map_2d(
    map: &DungeonMap,
    theme: FloorTheme,
    max_w: usize,
    max_h: usize,
) -> Vec<Line<'static>> {
    let (wall_color, floor_color) = theme_colors(theme);
    let fog_color = Color::Rgb(25, 25, 25);
    let revealed_color = Color::Rgb(18, 18, 18);
    let corridor_color = Color::Rgb(30, 30, 40);

    // ビューポート: 2文字/タイル幅, 1行/タイル高
    // 利用可能な幅 → タイル数 = max_w / 2
    let tiles_by_w = max_w / 2;
    let tiles_by_h = max_h;
    let mut n = tiles_by_w.min(tiles_by_h);
    if n % 2 == 0 { n = n.saturating_sub(1); }
    n = n.clamp(11, 21);

    let radius = (n / 2) as i32;
    let px = map.player_x as i32;
    let py = map.player_y as i32;

    let visible = compute_visibility(map);

    let mut lines = Vec::new();
    for vy in 0..n {
        let my = py - radius + vy as i32;
        let mut spans = Vec::new();
        for vx in 0..n {
            let mx = px - radius + vx as i32;
            let (text, style) = if !map.in_bounds(mx, my) {
                ("  ", Style::default())
            } else {
                let x = mx as usize;
                let y = my as usize;
                let cell = map.cell(x, y);
                let is_visible = visible.contains(&(x, y));

                if is_visible {
                    tile_to_visible_span(cell, x, y, px as usize, py as usize,
                        wall_color, floor_color, corridor_color)
                } else if cell.revealed {
                    tile_to_revealed_span(cell, revealed_color)
                } else {
                    ("░░", Style::default().fg(fog_color))
                }
            };
            spans.push(Span::styled(text.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    lines
}
```

**可視タイルの描画ルール:**
- `Tile::Wall` → `"██"` テーマ色
- `Tile::RoomFloor` (プレイヤー) → `"＠"` 白
- `Tile::RoomFloor` (Enemy, !done) → `"！"` 赤
- `Tile::RoomFloor` (Treasure, !done) → `"◆ "` 黄
- `Tile::RoomFloor` (Stairs) → `"▽ "` 緑
- `Tile::RoomFloor` / `Tile::Corridor` (空) → `"· "` 床色
- `Tile::Corridor` (プレイヤー) → `"＠"` 白

**探索済み非可視タイルの描画:**
- 壁: `"██"` 非常に暗い色
- 床/通路: `"· "` 非常に暗い色

**Step 2: ビューポート計算を render.rs 側で調整**

`render.rs` の `render_dungeon_explore` 内のビューポートサイズ計算を更新:
- 旧: `n * 3 + 1` (幅), `n * 2 + 1` (高さ)
- 新: `n * 2` (幅), `n` (高さ)

**Step 3: テスト (目視確認 + 基本テスト)**

```rust
#[test]
fn render_map_produces_lines() {
    let mut seed = 42u64;
    let map = generate_map(1, &mut seed);
    let lines = render_map_2d(&map, FloorTheme::MossyRuins, 40, 20);
    assert!(!lines.is_empty());
    // ビューポートの行数が期待通り
    assert!(lines.len() >= 11);
}
```

**Step 4: コミット**

```bash
git add src/games/rpg/dungeon_view.rs src/games/rpg/render.rs
git commit -m "feat: tile-based 2D map rendering with room-aware visibility"
```

---

## Task 6: 入力ハンドリング + D-pad (mod.rs, render.rs)

**Files:**
- Modify: `src/games/rpg/mod.rs:148-227`
- Modify: `src/games/rpg/render.rs:534-625`

**Step 1: `handle_dungeon_explore_key` を簡素化**

turn 関連を削除、全てを `try_move` に統一:

```rust
fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        // WASD (絶対方向)
        'W' | 'w' | 'k' => logic::try_move(state, Facing::North),
        'A' | 'a' | 'h' => logic::try_move(state, Facing::West),
        'S' | 's' | 'j' => logic::try_move(state, Facing::South),
        'D' | 'd' | 'l' => logic::try_move(state, Facing::East),
        _ => handle_overlay_open_key(state, ch),
    }
}
```

**Step 2: `handle_dungeon_explore_click` を簡素化**

`MOVE_FORWARD`, `TURN_LEFT`, `TURN_RIGHT`, `TURN_AROUND` を廃止。
D-pad クリックは直接方向移動:

```rust
fn handle_dungeon_explore_click(state: &mut RpgState, id: u16) -> bool {
    if let Some(dir) = decode_dpad_direction(id) {
        return logic::try_move(state, dir);
    }
    if let Some(dir) = decode_map_tap_direction(id) {
        return logic::move_direction(state, dir);
    }
    handle_overlay_open_click(state, id)
}
```

**Step 3: D-pad レンダリングを更新 (render.rs)**

`dir_style` を新モデルに合わせる（`cell.wall(dir)` → 隣接タイルの `is_walkable()`）:

```rust
let dir_style = |dir: Facing| -> Style {
    let nx = map.player_x as i32 + dir.dx();
    let ny = map.player_y as i32 + dir.dy();
    if !map.in_bounds(nx, ny) || !map.cell(nx as usize, ny as usize).is_walkable() {
        return Style::default().fg(Color::DarkGray); // 壁
    }
    // ... 既存の色分けロジック
};
```

**Step 4: compass line を更新**

`cell.wall(dir)` → 隣接タイルの `is_walkable()` チェックに変更。

**Step 5: テスト**

```rust
#[test]
fn dpad_moves_in_absolute_direction() {
    let mut g = make_game();
    // ... ダンジョン入場後
    let map = g.state.dungeon.as_ref().unwrap();
    let start_x = map.player_x;
    let start_y = map.player_y;

    // 東に歩けるなら、'd' キーで東に移動する
    let east_x = start_x as i32 + Facing::East.dx();
    let east_y = start_y as i32 + Facing::East.dy();
    if map.in_bounds(east_x, east_y)
        && map.cell(east_x as usize, east_y as usize).is_walkable()
    {
        g.handle_input(&InputEvent::Key('d'));
        let map = g.state.dungeon.as_ref().unwrap();
        assert_eq!(map.player_x, east_x as usize);
    }
}
```

**Step 6: 未使用の const (`MOVE_FORWARD`, `TURN_LEFT`, etc.) を削除**

**Step 7: コミット**

```bash
git add src/games/rpg/mod.rs src/games/rpg/render.rs
git commit -m "refactor: simplify input to direct 4-directional movement, remove facing/turn"
```

---

## Task 7: 統合テスト + コンパイル修正 + Clippy

**Files:**
- All `src/games/rpg/*.rs` files
- `src/games/rpg/mod.rs` (tests at bottom)

**Step 1: `cargo check` で残りのコンパイルエラーを全て修正**

特に注意:
- `map.facing` → `map.last_dir` の参照漏れ
- `cell.wall()` → 隣接タイル判定への変換漏れ
- `MapCell` の `walls` フィールド初期化の削除

**Step 2: `cargo test` で既存テストの修正**

mod.rs 末尾のテスト群を新モデルに合わせて更新。
特に `arrow_key_moves_in_absolute_direction` は try_move ベースに。

**Step 3: `cargo clippy -- -W clippy::all` で警告ゼロ確認**

dead_code 警告は `#[cfg(test)]` やフィールド削除で対応。

**Step 4: ブラウザで動作確認**

```bash
trunk serve
```

- F1 ダンジョンに入り、部屋が見えることを確認
- D-pad で東西南北に直接移動できることを確認
- 部屋に入ると全体が見えることを確認
- 通路では周囲2マスのみ見えることを確認

**Step 5: 最終コミット**

```bash
git add -A
git commit -m "fix: resolve compilation errors and update tests for PMD-style dungeon"
```
