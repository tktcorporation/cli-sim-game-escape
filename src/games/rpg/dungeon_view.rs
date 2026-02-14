//! 2D dungeon map renderer with tile-based display and room visibility.
//!
//! Renders a top-down tile map centered on the player. Each tile is 2 chars
//! wide and 1 row tall. Visibility is room-aware: inside a room you see
//! the whole room; in a corridor you see a radius-2 area.

// 2D grid rendering uses index-based loops for clarity.
#![allow(clippy::needless_range_loop)]

use std::collections::HashSet;

use ratzilla::ratatui::style::{Color, Style};
use ratzilla::ratatui::text::{Line, Span};

use super::state::{CellType, DungeonMap, FloorTheme, Tile};

// ── View Data (for describe_view) ────────────────────────────

/// What's visible at each depth from the player (used by describe_view).
pub struct DepthSlice {
    pub wall_front: bool,
    pub cell_type: CellType,
}

pub struct ViewData {
    pub depths: Vec<DepthSlice>, // 0 = player cell, 1 = one ahead, etc.
}

/// Compute what the player sees from their current position (forward line of sight).
pub fn compute_view(map: &DungeonMap) -> ViewData {
    let mut depths = Vec::new();
    let mut x = map.player_x as i32;
    let mut y = map.player_y as i32;
    let facing = map.last_dir;

    for _depth in 0..4 {
        if !map.in_bounds(x, y) {
            break;
        }

        let cell = map.cell(x as usize, y as usize);
        // Check if next tile in facing direction is a wall
        let fx = x + facing.dx();
        let fy = y + facing.dy();
        let has_front_wall = if map.in_bounds(fx, fy) {
            map.cell(fx as usize, fy as usize).tile == Tile::Wall
        } else {
            true
        };

        depths.push(DepthSlice {
            wall_front: has_front_wall,
            cell_type: cell.cell_type,
        });

        if has_front_wall {
            break; // Can't see further
        }

        // Move forward
        x += facing.dx();
        y += facing.dy();
    }

    ViewData { depths }
}

// ── View Description ─────────────────────────────────────────

/// Generate a text description of what the player sees ahead.
pub fn describe_view(view: &ViewData) -> String {
    if view.depths.is_empty() {
        return "目の前は壁だ。".into();
    }

    // How far can we go forward?
    let walkable = view
        .depths
        .iter()
        .take_while(|d| !d.wall_front)
        .count();

    let depth_desc = if walkable == 0 {
        "目の前は壁。".to_string()
    } else {
        format!("前方{}マス進める。", walkable)
    };

    // Find the nearest special cell (skip depth 0 = player's cell)
    let marker_desc = view
        .depths
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(i, d)| {
            let label = match d.cell_type {
                CellType::Stairs => "▼階段",
                CellType::Treasure => "◆宝箱",
                CellType::Enemy => "!敵影",
                CellType::Spring => "~泉",
                CellType::Npc => "?人影",
                CellType::Lore => "\u{2726}碑文",
                _ => return None,
            };
            Some(format!("{}歩先に{}", i, label))
        });

    match marker_desc {
        Some(m) => format!("{} {}", depth_desc, m),
        None => depth_desc,
    }
}

// ── Visibility ───────────────────────────────────────────────

/// Compute the set of (x, y) coordinates that are currently visible.
/// - In a room: all tiles with the same room_id + 1-tile border around room edges
/// - In a corridor: all tiles within radius 2 (5x5 square)
pub fn compute_visibility(map: &DungeonMap) -> HashSet<(usize, usize)> {
    let mut visible = HashSet::new();
    let px = map.player_x;
    let py = map.player_y;
    let cell = map.player_cell();

    if let Some(room_id) = cell.room_id {
        // In a room: find the room and reveal all tiles + 1-tile border
        if let Some(room) = map.rooms.iter().find(|r| {
            let rid = map.grid[r.y + r.h / 2][r.x + r.w / 2].room_id;
            rid == Some(room_id)
        }) {
            let x_start = room.x.saturating_sub(1);
            let y_start = room.y.saturating_sub(1);
            let x_end = (room.x + room.w).min(map.width - 1);
            let y_end = (room.y + room.h).min(map.height - 1);
            for vy in y_start..=y_end {
                for vx in x_start..=x_end {
                    visible.insert((vx, vy));
                }
            }
        }
    } else {
        // In corridor: radius 2
        let radius: i32 = 2;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let vx = px as i32 + dx;
                let vy = py as i32 + dy;
                if map.in_bounds(vx, vy) {
                    visible.insert((vx as usize, vy as usize));
                }
            }
        }
    }

    visible
}

// ── Theme Colors ─────────────────────────────────────────────

fn theme_colors(theme: FloorTheme) -> (Color, Color) {
    // (wall_color, floor_dot_color)
    match theme {
        FloorTheme::MossyRuins => (Color::Rgb(100, 130, 100), Color::Rgb(40, 55, 40)),
        FloorTheme::Underground => (Color::Rgb(100, 100, 140), Color::Rgb(40, 40, 55)),
        FloorTheme::AncientTemple => (Color::Rgb(150, 125, 80), Color::Rgb(55, 45, 30)),
        FloorTheme::VolcanicDepths => (Color::Rgb(160, 70, 40), Color::Rgb(60, 30, 15)),
        FloorTheme::DemonCastle => (Color::Rgb(120, 65, 150), Color::Rgb(50, 25, 60)),
    }
}

// ── 2D Map Rendering ─────────────────────────────────────────

/// Render a 2D top-down map with tile-based rendering, dynamically sized.
///
/// Each tile is rendered as 2 chars wide × 1 row tall.
/// Viewport: `n * 2` columns × `n` rows.
pub fn render_map_2d(
    map: &DungeonMap,
    theme: FloorTheme,
    max_w: usize,
    max_h: usize,
) -> Vec<Line<'static>> {
    let (wall_color, _floor_color) = theme_colors(theme);
    let fog_color = Color::Rgb(25, 25, 25);
    let dark_wall_color = Color::Rgb(35, 35, 35);
    let dark_floor_color = Color::Rgb(20, 20, 20);

    // Compute how many tiles fit
    let tiles_by_w = max_w / 2;
    let tiles_by_h = max_h;
    let mut n = tiles_by_w.min(tiles_by_h);
    if n.is_multiple_of(2) {
        n = n.saturating_sub(1);
    }
    n = n.clamp(11, 21);

    let radius = (n / 2) as i32;
    let gh = n;

    let px = map.player_x as i32;
    let py = map.player_y as i32;

    // Compute visibility
    let visible = compute_visibility(map);

    // Buffer: (2-char string, Color)
    let mut buf: Vec<Vec<(String, Color)>> = Vec::with_capacity(gh);
    for _row in 0..gh {
        let mut row_data = Vec::with_capacity(n);
        for _col in 0..n {
            row_data.push(("  ".to_string(), Color::Reset));
        }
        buf.push(row_data);
    }

    for vy in 0..n {
        let my = py - radius + vy as i32;
        for vx in 0..n {
            let mx = px - radius + vx as i32;

            if !map.in_bounds(mx, my) {
                buf[vy][vx] = ("  ".to_string(), Color::Reset);
                continue;
            }

            let ux = mx as usize;
            let uy = my as usize;
            let cell = map.cell(ux, uy);
            let is_visible = visible.contains(&(ux, uy));
            let is_player = mx == px && my == py;

            if is_player {
                buf[vy][vx] = ("\u{ff20}".to_string(), Color::White); // ＠ (fullwidth @)
            } else if is_visible {
                match cell.tile {
                    Tile::Wall => {
                        buf[vy][vx] = ("\u{2588}\u{2588}".to_string(), wall_color);
                    }
                    Tile::RoomFloor | Tile::Corridor => {
                        let (ch, color) = cell_marker(cell);
                        buf[vy][vx] = (ch, color);
                    }
                }
            } else if cell.revealed {
                // Revealed but not currently visible — very dark
                match cell.tile {
                    Tile::Wall => {
                        buf[vy][vx] = ("\u{2588}\u{2588}".to_string(), dark_wall_color);
                    }
                    Tile::RoomFloor | Tile::Corridor => {
                        if ch_is_floor(cell) {
                            buf[vy][vx] = ("\u{00b7} ".to_string(), dark_floor_color);
                        } else {
                            let (ch, _) = cell_marker(cell);
                            buf[vy][vx] = (ch, dark_floor_color);
                        }
                    }
                }
            } else {
                // Unexplored
                buf[vy][vx] = ("\u{2591}\u{2591}".to_string(), fog_color);
            }
        }
    }

    // Convert to Lines (each tile is a 2-char span)
    buf.iter()
        .map(|row| {
            Line::from(
                row.iter()
                    .map(|(ch, color)| Span::styled(ch.clone(), Style::default().fg(*color)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Map cell type to display string (2 chars wide) and color.
fn cell_marker(cell: &super::state::MapCell) -> (String, Color) {
    if !cell.event_done {
        match cell.cell_type {
            CellType::Entrance => ("\u{25c7} ".to_string(), Color::Green),
            CellType::Stairs => ("\u{25bd} ".to_string(), Color::Green),
            CellType::Enemy => ("\u{ff01}".to_string(), Color::Red),           // ！(fullwidth !)
            CellType::Treasure => ("\u{25c6} ".to_string(), Color::Yellow),
            CellType::Spring => ("~ ".to_string(), Color::Cyan),
            CellType::Lore => ("\u{2726} ".to_string(), Color::Yellow),
            CellType::Npc => ("? ".to_string(), Color::Magenta),
            CellType::Trap => ("\u{00b7} ".to_string(), Color::Reset),  // hidden
            CellType::Corridor => ("\u{00b7} ".to_string(), Color::Reset),
        }
    } else {
        match cell.cell_type {
            CellType::Entrance => ("\u{25c7} ".to_string(), Color::Green),
            CellType::Stairs => ("\u{25bd} ".to_string(), Color::Green),
            _ => ("\u{00b7} ".to_string(), Color::Reset), // resolved event / corridor
        }
    }
}

/// Whether the cell should show as a plain floor dot.
fn ch_is_floor(cell: &super::state::MapCell) -> bool {
    matches!(
        cell.cell_type,
        CellType::Corridor | CellType::Trap
    ) || (cell.event_done
        && cell.cell_type != CellType::Entrance
        && cell.cell_type != CellType::Stairs)
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::rpg::dungeon_map::generate_map;

    #[test]
    fn compute_view_basic() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        map.grid[map.player_y][map.player_x].revealed = true;
        let view = compute_view(&map);
        assert!(!view.depths.is_empty());
    }

    #[test]
    fn map_2d_renders_correct_dimensions() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        map.grid[map.player_y][map.player_x].revealed = true;
        // Request space for 11 tiles (max_w=22, max_h=11)
        let lines = render_map_2d(&map, FloorTheme::MossyRuins, 22, 11);
        // 11 tiles → 11 rows
        assert_eq!(lines.len(), 11);
        // 11 tiles → 11 spans (each 2-char wide)
        assert_eq!(lines[0].spans.len(), 11);
    }

    #[test]
    fn map_2d_shows_player() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        map.grid[map.player_y][map.player_x].revealed = true;
        let lines = render_map_2d(&map, FloorTheme::Underground, 22, 11);
        // Player should be at the center
        let center_row = lines.len() / 2;
        let center_col = lines[0].spans.len() / 2;
        let center_span = &lines[center_row].spans[center_col];
        assert_eq!(center_span.content.as_ref(), "\u{ff20}"); // ＠
    }

    #[test]
    fn describe_view_wall_ahead() {
        let view = ViewData {
            depths: vec![DepthSlice {
                wall_front: true,
                cell_type: CellType::Corridor,
            }],
        };
        let desc = describe_view(&view);
        assert!(desc.contains("壁"));
    }

    #[test]
    fn describe_view_open_ahead() {
        let view = ViewData {
            depths: vec![
                DepthSlice {
                    wall_front: false,
                    cell_type: CellType::Corridor,
                },
                DepthSlice {
                    wall_front: true,
                    cell_type: CellType::Treasure,
                },
            ],
        };
        let desc = describe_view(&view);
        assert!(desc.contains("1マス"));
        assert!(desc.contains("宝箱"));
    }

    #[test]
    fn compute_visibility_room() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        // Player starts in a room
        let vis = compute_visibility(&map);
        // Should see multiple tiles (the room)
        assert!(vis.len() > 4);
        // Player position should be visible
        assert!(vis.contains(&(map.player_x, map.player_y)));
    }
}
