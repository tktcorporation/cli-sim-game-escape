//! First-person 3D dungeon view renderer.
//!
//! Renders a Wizardry-style perspective view using Unicode block characters.
//! The view looks 3 cells ahead from the player's position and facing direction.

// 2D grid rendering uses index-based loops for clarity.
#![allow(clippy::needless_range_loop)]

use ratzilla::ratatui::style::{Color, Style};
use ratzilla::ratatui::text::{Line, Span};

use super::state::{CellType, DungeonMap, Facing, FloorTheme};

pub const VIEW_W: usize = 27;
pub const VIEW_H: usize = 11;

// ── View Data ─────────────────────────────────────────────────

/// What's visible at each depth from the player.
pub struct DepthSlice {
    pub wall_left: bool,
    pub wall_right: bool,
    pub wall_front: bool,
    pub cell_type: CellType,
}

pub struct ViewData {
    pub depths: Vec<DepthSlice>, // 0 = player cell, 1 = one ahead, etc.
}

/// Compute what the player sees from their current position.
pub fn compute_view(map: &DungeonMap) -> ViewData {
    let mut depths = Vec::new();
    let mut x = map.player_x as i32;
    let mut y = map.player_y as i32;
    let facing = map.facing;
    let left = facing.turn_left();
    let right = facing.turn_right();

    for _depth in 0..4 {
        if !map.in_bounds(x, y) {
            break;
        }

        let cell = map.cell(x as usize, y as usize);
        let has_front_wall = cell.wall(facing);
        let has_left_wall = cell.wall(left);
        let has_right_wall = cell.wall(right);

        depths.push(DepthSlice {
            wall_left: has_left_wall,
            wall_right: has_right_wall,
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

// ── Frame Definitions ─────────────────────────────────────────

/// Each depth level has a rectangular "window" that gets smaller toward center.
/// (left_col, right_col, top_row, bottom_row) inclusive.
const FRAMES: [(usize, usize, usize, usize); 5] = [
    (0, 26, 0, 10),  // Frame 0: outermost (viewport edge)
    (3, 23, 1, 9),   // Frame 1: depth 0 inner
    (7, 19, 2, 8),   // Frame 2: depth 1 inner
    (10, 16, 3, 7),  // Frame 3: depth 2 inner
    (12, 14, 4, 6),  // Frame 4: depth 3 inner (vanishing point)
];

// ── 3D Rendering ──────────────────────────────────────────────

/// Render the 3D perspective view as colored Lines.
pub fn render_view(view: &ViewData, theme: FloorTheme) -> Vec<Line<'static>> {
    let mut buf = [[' '; VIEW_W]; VIEW_H];

    let wall_char = '█';
    let side_wall_char = '▓';
    let far_wall_char = '░';

    // Draw from back to front
    let num_depths = view.depths.len().min(4);

    for d in (0..num_depths).rev() {
        let outer = FRAMES[d];
        let inner = FRAMES[d + 1];

        let wchar = match d {
            0 => wall_char,
            1 => side_wall_char,
            _ => far_wall_char,
        };

        // Always draw ceiling strip (top of corridor between outer and inner)
        for row in outer.2..inner.2 {
            for col in inner.0..=inner.1 {
                if col < VIEW_W && row < VIEW_H {
                    buf[row][col] = wchar;
                }
            }
        }

        // Always draw floor strip (bottom of corridor)
        for row in (inner.3 + 1)..=outer.3 {
            for col in inner.0..=inner.1 {
                if col < VIEW_W && row < VIEW_H {
                    buf[row][col] = wchar;
                }
            }
        }

        // Left wall
        if d < view.depths.len() && view.depths[d].wall_left {
            for row in outer.2..=outer.3 {
                for col in outer.0..inner.0 {
                    if col < VIEW_W && row < VIEW_H {
                        buf[row][col] = wchar;
                    }
                }
            }
        } else if d < view.depths.len() {
            // Draw wall edge but show opening
            for row in outer.2..=outer.3 {
                if outer.0 < VIEW_W && row < VIEW_H {
                    buf[row][outer.0] = '│';
                }
            }
            // Draw top/bottom of opening
            for col in outer.0..inner.0 {
                if col < VIEW_W {
                    if outer.2 < VIEW_H {
                        buf[outer.2][col] = '─';
                    }
                    if outer.3 < VIEW_H {
                        buf[outer.3][col] = '─';
                    }
                }
            }
        }

        // Right wall
        if d < view.depths.len() && view.depths[d].wall_right {
            for row in outer.2..=outer.3 {
                for col in (inner.1 + 1)..=outer.1 {
                    if col < VIEW_W && row < VIEW_H {
                        buf[row][col] = wchar;
                    }
                }
            }
        } else if d < view.depths.len() {
            // Show opening
            for row in outer.2..=outer.3 {
                if outer.1 < VIEW_W && row < VIEW_H {
                    buf[row][outer.1] = '│';
                }
            }
            for col in (inner.1 + 1)..=outer.1 {
                if col < VIEW_W {
                    if outer.2 < VIEW_H {
                        buf[outer.2][col] = '─';
                    }
                    if outer.3 < VIEW_H {
                        buf[outer.3][col] = '─';
                    }
                }
            }
        }

        // Front wall (if this depth has a wall blocking forward)
        if d < view.depths.len() && view.depths[d].wall_front {
            for row in inner.2..=inner.3 {
                for col in inner.0..=inner.1 {
                    if col < VIEW_W && row < VIEW_H {
                        buf[row][col] = wchar;
                    }
                }
            }
        }
    }

    // Draw perspective edge lines (diagonals connecting frame corners)
    draw_perspective_edges(&mut buf, num_depths);

    // Add cell-type indicators at the visible end
    add_cell_markers(&mut buf, view);

    // Convert to colored Lines
    buf_to_lines(&buf, theme)
}

/// Draw diagonal perspective lines connecting adjacent frame corners.
fn draw_perspective_edges(buf: &mut [[char; VIEW_W]; VIEW_H], num_depths: usize) {
    for d in 0..num_depths.min(4) {
        let outer = FRAMES[d];
        let inner = FRAMES[d + 1];

        // Top-left diagonal
        draw_diagonal(buf, outer.0, outer.2, inner.0, inner.2, '╲');
        // Top-right diagonal
        draw_diagonal(buf, outer.1, outer.2, inner.1, inner.2, '╱');
        // Bottom-left diagonal
        draw_diagonal(buf, outer.0, outer.3, inner.0, inner.3, '╱');
        // Bottom-right diagonal
        draw_diagonal(buf, outer.1, outer.3, inner.1, inner.3, '╲');
    }
}

fn draw_diagonal(
    buf: &mut [[char; VIEW_W]; VIEW_H],
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
    ch: char,
) {
    let steps = (x2 as i32 - x1 as i32)
        .abs()
        .max((y2 as i32 - y1 as i32).abs()) as usize;
    if steps == 0 {
        return;
    }
    for i in 0..=steps {
        let x = x1 as i32 + ((x2 as i32 - x1 as i32) * i as i32) / steps as i32;
        let y = y1 as i32 + ((y2 as i32 - y1 as i32) * i as i32) / steps as i32;
        if x >= 0 && y >= 0 && (x as usize) < VIEW_W && (y as usize) < VIEW_H {
            let ux = x as usize;
            let uy = y as usize;
            // Only draw on empty cells (don't overwrite walls)
            if buf[uy][ux] == ' ' {
                buf[uy][ux] = ch;
            }
        }
    }
}

/// Add visual markers for special cells at the visible depth.
fn add_cell_markers(buf: &mut [[char; VIEW_W]; VIEW_H], view: &ViewData) {
    // Show marker at the deepest visible cell that has a special type
    let last_depth = if view.depths.len() > 1 {
        view.depths.len() - 1
    } else {
        return;
    };

    let last = &view.depths[last_depth];
    let frame = FRAMES[last_depth.min(3) + 1];
    let center_x = (frame.0 + frame.1) / 2;
    let center_y = (frame.2 + frame.3) / 2;

    if center_x >= VIEW_W || center_y >= VIEW_H {
        return;
    }

    let marker = match last.cell_type {
        CellType::Stairs => '▼',
        CellType::Treasure => '◆',
        CellType::Enemy => '!',
        CellType::Spring => '~',
        CellType::Npc => '?',
        CellType::Lore => '✦',
        CellType::Trap => ' ', // hidden
        _ => return,
    };

    buf[center_y][center_x] = marker;
}

/// Convert character buffer to colored ratatui Lines based on theme.
fn buf_to_lines(buf: &[[char; VIEW_W]; VIEW_H], theme: FloorTheme) -> Vec<Line<'static>> {
    let (wall_color, accent_color, floor_color) = theme_colors(theme);

    buf.iter()
        .map(|row| {
            let spans: Vec<Span<'static>> = row
                .iter()
                .map(|&ch| {
                    let style = match ch {
                        '█' => Style::default().fg(wall_color),
                        '▓' => Style::default().fg(accent_color),
                        '░' => Style::default().fg(floor_color),
                        '│' | '─' | '╲' | '╱' => Style::default().fg(Color::DarkGray),
                        '▼' => Style::default().fg(Color::Green),
                        '◆' => Style::default().fg(Color::Yellow),
                        '!' => Style::default().fg(Color::Red),
                        '~' => Style::default().fg(Color::Cyan),
                        '?' => Style::default().fg(Color::Magenta),
                        '✦' => Style::default().fg(Color::Yellow),
                        _ => Style::default().fg(Color::DarkGray),
                    };
                    Span::styled(ch.to_string(), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn theme_colors(theme: FloorTheme) -> (Color, Color, Color) {
    match theme {
        FloorTheme::MossyRuins => (Color::Rgb(80, 100, 80), Color::Rgb(60, 80, 60), Color::Rgb(40, 60, 40)),
        FloorTheme::Underground => (Color::Rgb(80, 80, 100), Color::Rgb(60, 60, 80), Color::Rgb(40, 40, 60)),
        FloorTheme::AncientTemple => (Color::Rgb(120, 100, 60), Color::Rgb(90, 75, 45), Color::Rgb(60, 50, 30)),
        FloorTheme::VolcanicDepths => (Color::Rgb(130, 50, 30), Color::Rgb(100, 40, 20), Color::Rgb(70, 30, 10)),
        FloorTheme::DemonCastle => (Color::Rgb(80, 40, 100), Color::Rgb(60, 30, 80), Color::Rgb(40, 20, 60)),
    }
}

// ── Minimap Rendering ─────────────────────────────────────────

/// Render the minimap as colored Lines. Shows 9×9 area around the player.
pub fn render_minimap(map: &DungeonMap, theme: FloorTheme) -> Vec<Line<'static>> {
    let radius: i32 = 4; // show 9×9 area
    let map_render_h = (radius * 2 + 1) as usize + 2; // cells + border

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(map_render_h);
    let (wall_color, _, _) = theme_colors(theme);

    let px = map.player_x as i32;
    let py = map.player_y as i32;

    // Render each row of the visible minimap
    for dy in -radius..=radius {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let y = py + dy;

        for dx in -radius..=radius {
            let x = px + dx;

            if x < 0 || y < 0 || x >= map.width as i32 || y >= map.height as i32 {
                spans.push(Span::styled("  ", Style::default()));
                continue;
            }

            let cell = map.cell(x as usize, y as usize);

            if !cell.visited {
                spans.push(Span::styled("··", Style::default().fg(Color::Rgb(30, 30, 30))));
                continue;
            }

            // Player position
            if x == px && y == py {
                let arrow = match map.facing {
                    Facing::North => "▲ ",
                    Facing::East => "▶ ",
                    Facing::South => "▽ ",
                    Facing::West => "◀ ",
                };
                spans.push(Span::styled(
                    arrow.to_string(),
                    Style::default().fg(Color::White),
                ));
                continue;
            }

            // Cell type indicator
            let (ch, color) = match cell.cell_type {
                CellType::Entrance => ("◇ ", Color::Green),
                CellType::Stairs => ("▼ ", Color::Green),
                CellType::Enemy if !cell.event_done => ("! ", Color::Red),
                CellType::Treasure if !cell.event_done => ("◆ ", Color::Yellow),
                CellType::Spring if !cell.event_done => ("~ ", Color::Cyan),
                CellType::Lore if !cell.event_done => ("✦ ", Color::Yellow),
                CellType::Npc if !cell.event_done => ("? ", Color::Magenta),
                _ => ("· ", wall_color),
            };
            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        lines.push(Line::from(spans));
    }

    lines
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
    fn render_view_correct_dimensions() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let view = compute_view(&map);
        let lines = render_view(&view, FloorTheme::MossyRuins);
        assert_eq!(lines.len(), VIEW_H);
    }

    #[test]
    fn minimap_renders() {
        let mut seed = 42u64;
        let mut map = generate_map(1, &mut seed);
        map.grid[map.player_y][map.player_x].visited = true;
        let lines = render_minimap(&map, FloorTheme::MossyRuins);
        assert!(!lines.is_empty());
    }
}
