//! 2D dungeon map renderer with wall display and auto-mapping.
//!
//! Renders a top-down grid map centered on the player. Visited cells show
//! walls, cell-type markers, and the player's facing direction. Unvisited
//! cells appear as fog. The map auto-sizes based on available terminal area.

// 2D grid rendering uses index-based loops for clarity.
#![allow(clippy::needless_range_loop)]

use ratzilla::ratatui::style::{Color, Style};
use ratzilla::ratatui::text::{Line, Span};

use super::state::{CellType, DungeonMap, Facing, FloorTheme};

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
    let facing = map.facing;

    for _depth in 0..4 {
        if !map.in_bounds(x, y) {
            break;
        }

        let cell = map.cell(x as usize, y as usize);
        let has_front_wall = cell.wall(facing);

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
                CellType::Lore => "✦碑文",
                _ => return None,
            };
            Some(format!("{}歩先に{}", i, label))
        });

    match marker_desc {
        Some(m) => format!("{} {}", depth_desc, m),
        None => depth_desc,
    }
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

/// Render a 2D top-down map with walls, dynamically sized to fill the area.
///
/// Grid encoding per cell (3 cols × 2 rows):
/// ```text
/// row 0: [corner(1)] [h-wall(2)]
/// row 1: [v-wall(1)] [content(2)]
/// ```
/// Plus trailing wall column/row for the last cells.
/// Total: `(3n+1)` columns × `(2n+1)` rows for n×n cells.
pub fn render_map_2d(
    map: &DungeonMap,
    theme: FloorTheme,
    max_w: usize,
    max_h: usize,
) -> Vec<Line<'static>> {
    let (wall_color, floor_color) = theme_colors(theme);
    let fog_color = Color::Rgb(25, 25, 25);

    // Compute how many cells fit
    let cells_by_w = if max_w >= 7 { (max_w - 1) / 3 } else { 3 };
    let cells_by_h = if max_h >= 5 { (max_h - 1) / 2 } else { 3 };
    let mut n = cells_by_w.min(cells_by_h);
    if n % 2 == 0 {
        n = n.saturating_sub(1);
    }
    n = n.clamp(5, 13);

    let radius = (n / 2) as i32;
    let gw = n * 3 + 1;
    let gh = n * 2 + 1;

    let px = map.player_x as i32;
    let py = map.player_y as i32;

    // Buffer: (char, Color)
    let mut buf: Vec<Vec<(char, Color)>> = vec![vec![(' ', Color::Reset); gw]; gh];

    // Pass 1: fog for unvisited in-bounds cells
    for cy in 0..n {
        let my = py - radius + cy as i32;
        for cx in 0..n {
            let mx = px - radius + cx as i32;

            if !map.in_bounds(mx, my) {
                continue;
            }

            let cell = map.cell(mx as usize, my as usize);
            if cell.visited {
                continue;
            }

            // Fog content area
            let crow = cy * 2 + 1;
            let ccol = cx * 3 + 1;
            buf[crow][ccol] = ('░', fog_color);
            buf[crow][ccol + 1] = ('░', fog_color);
        }
    }

    // Pass 2: draw visited cells — walls, corners, content
    for cy in 0..n {
        let my = py - radius + cy as i32;
        for cx in 0..n {
            let mx = px - radius + cx as i32;

            if !map.in_bounds(mx, my) {
                continue;
            }

            let cell = map.cell(mx as usize, my as usize);
            if !cell.visited {
                continue;
            }

            let crow = cy * 2 + 1;
            let ccol = cx * 3 + 1;
            let wrow_top = cy * 2;
            let wrow_bot = (cy + 1) * 2;
            let wcol_left = cx * 3;
            let wcol_right = (cx + 1) * 3;

            // ── Walls ──
            // North wall
            if cell.wall(Facing::North) {
                buf[wrow_top][ccol] = ('─', wall_color);
                buf[wrow_top][ccol + 1] = ('─', wall_color);
            }
            // South wall
            if wrow_bot < gh && cell.wall(Facing::South) {
                buf[wrow_bot][ccol] = ('─', wall_color);
                buf[wrow_bot][ccol + 1] = ('─', wall_color);
            }
            // West wall
            if cell.wall(Facing::West) {
                buf[crow][wcol_left] = ('│', wall_color);
            }
            // East wall
            if wcol_right < gw && cell.wall(Facing::East) {
                buf[crow][wcol_right] = ('│', wall_color);
            }

            // ── Corners (always draw for visited cells) ──
            buf[wrow_top][wcol_left] = ('·', Color::DarkGray);
            if wcol_right < gw {
                buf[wrow_top][wcol_right] = ('·', Color::DarkGray);
            }
            if wrow_bot < gh {
                buf[wrow_bot][wcol_left] = ('·', Color::DarkGray);
                if wcol_right < gw {
                    buf[wrow_bot][wcol_right] = ('·', Color::DarkGray);
                }
            }

            // ── Cell content ──
            if mx == px && my == py {
                let arrow = match map.facing {
                    Facing::North => '▲',
                    Facing::East => '▶',
                    Facing::South => '▽',
                    Facing::West => '◀',
                };
                buf[crow][ccol] = (arrow, Color::White);
                buf[crow][ccol + 1] = (' ', Color::Reset);
            } else {
                let (ch, color) = cell_marker(cell);
                buf[crow][ccol] = (ch, color);
                buf[crow][ccol + 1] = (' ', Color::Reset);
            }

            // Floor dot for visited corridors (no event / event done)
            if !(mx == px && my == py) && ch_is_floor(cell) {
                buf[crow][ccol] = ('·', floor_color);
                buf[crow][ccol + 1] = (' ', Color::Reset);
            }
        }
    }

    // Convert to Lines
    buf.iter()
        .map(|row| {
            Line::from(
                row.iter()
                    .map(|&(ch, color)| Span::styled(ch.to_string(), Style::default().fg(color)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Map cell type to display character and color.
fn cell_marker(cell: &super::state::MapCell) -> (char, Color) {
    match cell.cell_type {
        CellType::Entrance => ('◇', Color::Green),
        CellType::Stairs => ('▼', Color::Green),
        CellType::Enemy if !cell.event_done => ('!', Color::Red),
        CellType::Treasure if !cell.event_done => ('◆', Color::Yellow),
        CellType::Spring if !cell.event_done => ('~', Color::Cyan),
        CellType::Lore if !cell.event_done => ('✦', Color::Yellow),
        CellType::Npc if !cell.event_done => ('?', Color::Magenta),
        CellType::Trap if !cell.event_done => (' ', Color::Reset), // hidden
        _ => (' ', Color::Reset), // corridor / resolved event
    }
}

/// Whether the cell should show as a plain floor dot.
fn ch_is_floor(cell: &super::state::MapCell) -> bool {
    matches!(
        cell.cell_type,
        CellType::Corridor | CellType::Trap
    ) || cell.event_done
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
        let view = compute_view(&map);
        assert!(!view.depths.is_empty());
    }

    #[test]
    fn map_2d_renders_correct_dimensions() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        // Request 7×7 cell area (max_w=22, max_h=15)
        let lines = render_map_2d(&map, FloorTheme::MossyRuins, 22, 15);
        // 7 cells → 2*7+1 = 15 rows
        assert_eq!(lines.len(), 15);
        // 7 cells → 3*7+1 = 22 cols
        assert!(lines[0].spans.len() == 22);
    }

    #[test]
    fn map_2d_shows_player() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        map.facing = Facing::North;
        let lines = render_map_2d(&map, FloorTheme::Underground, 22, 15);
        // Player should be at the center cell
        // Center cell row = radius*2 + 1, col = radius*3 + 1
        // For 7×7: radius=3, row=7, col=10
        let center_row = &lines[7];
        let center_char = &center_row.spans[10];
        assert_eq!(center_char.content.as_ref(), "▲");
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
}
