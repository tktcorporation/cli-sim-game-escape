/// Tiny Factory rendering with animations and help.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};

use super::grid::{anchor_of, machine_at, Cell, MachineKind, GRID_H, GRID_W};
use super::state::{FactoryState, PlacementTool};

/// Spinner for active machines.
const SPINNER: &[char] = &['◐', '◓', '◑', '◒'];
/// Belt animation frames.
const BELT_ANIM_R: &[char] = &['>', '≫', '»', '›'];
const BELT_ANIM_L: &[char] = &['<', '≪', '«', '‹'];
const BELT_ANIM_U: &[char] = &['^', '⌃', '˄', '↑'];
const BELT_ANIM_D: &[char] = &['v', '⌄', '˅', '↓'];

pub fn render(
    state: &FactoryState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);

    if is_narrow {
        render_narrow(state, f, area, click_state);
    } else {
        render_wide(state, f, area, click_state);
    }
}

fn render_wide(
    state: &FactoryState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let h_chunks = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(3),                       // Header
            Constraint::Length(GRID_H as u16 + 2),       // Grid (fixed to grid height + border)
            Constraint::Length(11),                       // Tool panel (selection + description)
        ])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(3)])
        .split(h_chunks[1]);

    render_header(state, f, left_chunks[0], false);
    render_grid(state, f, left_chunks[1]);
    render_tool_panel(state, f, left_chunks[2], click_state);
    render_stats(state, f, right_chunks[0]);
    render_log(state, f, right_chunks[1]);
}

fn render_narrow(
    state: &FactoryState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(3),                       // Header
            Constraint::Length(GRID_H as u16 + 2),       // Grid (fixed)
            Constraint::Length(11),                       // Tool panel
        ])
        .split(area);

    render_header(state, f, chunks[0], true);
    render_grid(state, f, chunks[1]);
    render_tool_panel(state, f, chunks[2], click_state);
}

fn render_header(state: &FactoryState, f: &mut Frame, area: Rect, is_narrow: bool) {
    // Animated money indicator
    let money_anim = if state.total_exported > 0 {
        let idx = (state.anim_frame / 3) as usize % SPINNER.len();
        format!("{} ", SPINNER[idx])
    } else {
        "  ".to_string()
    };

    // Income rate ($/sec) based on total earnings and time elapsed
    let income_str = if state.total_ticks > 0 && state.total_money_earned > 0 {
        let seconds = state.total_ticks as f64 / 10.0;
        let rate = state.total_money_earned as f64 / seconds;
        if rate >= 1.0 {
            format!(" ${:.1}/s", rate)
        } else {
            format!(" ${:.2}/s", rate)
        }
    } else {
        String::new()
    };

    // Export flash: show earned amount
    let flash_str = if state.export_flash > 0 {
        format!(" +${}", state.last_export_value)
    } else {
        String::new()
    };

    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // Build header with flash effect
    let money_style = if state.export_flash > 0 {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };

    let flash_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);

    let income_style = Style::default().fg(Color::Cyan);

    let spans = if is_narrow {
        vec![
            Span::styled(
                format!("{}${} Exp:{}", money_anim, state.money, state.total_exported),
                money_style,
            ),
            Span::styled(flash_str, flash_style),
            Span::styled(income_str, income_style),
        ]
    } else {
        vec![
            Span::styled(
                format!(
                    "{} $: {}    Exported: {}",
                    money_anim, state.money, state.total_exported,
                ),
                money_style,
            ),
            Span::styled(flash_str, flash_style),
            Span::styled(income_str, income_style),
            Span::styled(
                format!("    Tool: {}", tool_name(&state.tool, &state.belt_direction)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]
    };

    let widget = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Tiny Factory "),
        )
        .alignment(Alignment::Center);

    f.render_widget(widget, area);
}

fn tool_name(tool: &PlacementTool, belt_dir: &super::grid::Direction) -> String {
    match tool {
        PlacementTool::None => "None (数字キーで選択)".into(),
        PlacementTool::Miner => "Miner ($10)".into(),
        PlacementTool::Smelter => "Smelter ($25)".into(),
        PlacementTool::Assembler => "Assembler ($50)".into(),
        PlacementTool::Exporter => "Exporter ($15)".into(),
        PlacementTool::Belt => format!("Belt {} ($2)", belt_dir.arrow()),
        PlacementTool::Delete => "Delete".into(),
    }
}


/// Check if a 2×2 machine anchored at (ax,ay) has output buffer full and no adjacent belt.
fn is_output_blocked(grid: &[Vec<Cell>], ax: usize, ay: usize, m: &super::grid::Machine) -> bool {
    if m.output_buffer.len() < m.max_buffer {
        return false;
    }
    if m.kind == MachineKind::Exporter {
        return false; // Exporters don't output
    }
    // Check 2×2 perimeter for any belt
    for (px, py) in perimeter_2x2(ax, ay) {
        if matches!(grid[py][px], Cell::Belt(_)) {
            return false;
        }
    }
    true
}

/// Collect perimeter cells around a 2×2 block anchored at (ax, ay).
fn perimeter_2x2(ax: usize, ay: usize) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    if ay > 0 {
        for dx in 0..2 { cells.push((ax + dx, ay - 1)); }
    }
    if ay + 2 < GRID_H {
        for dx in 0..2 { cells.push((ax + dx, ay + 2)); }
    }
    if ax > 0 {
        for dy in 0..2 { cells.push((ax - 1, ay + dy)); }
    }
    if ax + 2 < GRID_W {
        for dy in 0..2 { cells.push((ax + 2, ay + dy)); }
    }
    if ay > 0 && ax > 0 { cells.push((ax - 1, ay - 1)); }
    if ay > 0 && ax + 2 < GRID_W { cells.push((ax + 2, ay - 1)); }
    if ay + 2 < GRID_H && ax > 0 { cells.push((ax - 1, ay + 2)); }
    if ay + 2 < GRID_H && ax + 2 < GRID_W { cells.push((ax + 2, ay + 2)); }
    cells
}

/// Compute I/O hint arrows for cells adjacent to the cursor's machine (2×2 perimeter).
/// Returns vec of (x, y, char, color) for each hint to display.
fn compute_io_hints(state: &FactoryState) -> Vec<(usize, usize, char, Color)> {
    let cx = state.cursor_x;
    let cy = state.cursor_y;

    // Resolve anchor if cursor is on any part of a machine
    let (ax, ay) = match anchor_of(&state.grid, cx, cy) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let m = match machine_at(&state.grid, ax, ay) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let has_output = m.kind.output().is_some();
    let has_input = m.kind.input().is_some() || m.kind == MachineKind::Exporter;

    let mut hints = Vec::new();
    for (px, py) in perimeter_2x2(ax, ay) {
        match &state.grid[py][px] {
            Cell::Empty => {
                // Pick an arrow based on relative position to the 2×2 block
                let arrow = if px < ax { '←' }
                    else if px > ax + 1 { '→' }
                    else if py < ay { '↑' }
                    else if py > ay + 1 { '↓' }
                    else { '·' };

                if has_output && has_input {
                    hints.push((px, py, arrow, Color::DarkGray));
                } else if has_output {
                    hints.push((px, py, arrow, Color::Green));
                } else if has_input {
                    hints.push((px, py, arrow, Color::Yellow));
                }
            }
            _ => {}
        }
    }
    hints
}

/// Get the 2-char string for a machine cell at position (dx, dy) relative to anchor.
/// dx, dy are 0 or 1.
fn machine_cell_chars(kind: MachineKind, dx: usize, dy: usize, m: &super::grid::Machine) -> &'static str {
    // Progress indicator for top-left second char (TL[1])
    let progress_char = if !m.output_buffer.is_empty() && m.progress == 0 {
        '█' // output ready
    } else if m.progress > 0 {
        let ratio = m.progress as f32 / kind.recipe_time() as f32;
        if ratio < 0.25 { '░' }
        else if ratio < 0.5 { '▒' }
        else if ratio < 0.75 { '▓' }
        else { '█' }
    } else {
        '\0' // use default char
    };

    // Position key: (dx, dy)
    match (kind, dx, dy) {
        // Miner: ╔═╗  / ║M║
        (MachineKind::Miner, 0, 0) => if progress_char != '\0' { match progress_char {
            '░' => "╔░", '▒' => "╔▒", '▓' => "╔▓", '█' => "╔█", _ => "╔═" }
        } else { "╔═" },
        (MachineKind::Miner, 1, 0) => "╗ ",
        (MachineKind::Miner, 0, 1) => "║M",
        (MachineKind::Miner, 1, 1) => "║ ",

        // Smelter: ▄▄▄ / █S█
        (MachineKind::Smelter, 0, 0) => if progress_char != '\0' { match progress_char {
            '░' => "▄░", '▒' => "▄▒", '▓' => "▄▓", '█' => "▄█", _ => "▄▄" }
        } else { "▄▄" },
        (MachineKind::Smelter, 1, 0) => "▄ ",
        (MachineKind::Smelter, 0, 1) => "█S",
        (MachineKind::Smelter, 1, 1) => "█ ",

        // Assembler: ╭─╮ / │A│
        (MachineKind::Assembler, 0, 0) => if progress_char != '\0' { match progress_char {
            '░' => "╭░", '▒' => "╭▒", '▓' => "╭▓", '█' => "╭█", _ => "╭─" }
        } else { "╭─" },
        (MachineKind::Assembler, 1, 0) => "╮ ",
        (MachineKind::Assembler, 0, 1) => "│A",
        (MachineKind::Assembler, 1, 1) => "│ ",

        // Exporter: ┌$┐ / └E┘
        (MachineKind::Exporter, 0, 0) => if progress_char != '\0' { match progress_char {
            '░' => "┌░", '▒' => "┌▒", '▓' => "┌▓", '█' => "┌█", _ => "┌$" }
        } else { "┌$" },
        (MachineKind::Exporter, 1, 0) => "┐ ",
        (MachineKind::Exporter, 0, 1) => "└E",
        (MachineKind::Exporter, 1, 1) => "┘ ",

        _ => "  ",
    }
}

/// Blocked indicator: replace TL's second char with '!'
fn machine_cell_chars_blocked(kind: MachineKind, dx: usize, dy: usize) -> &'static str {
    match (kind, dx, dy) {
        (MachineKind::Miner, 0, 0) => "╔!",
        (MachineKind::Smelter, 0, 0) => "▄!",
        (MachineKind::Assembler, 0, 0) => "╭!",
        (MachineKind::Exporter, 0, 0) => "┌!",
        _ => machine_cell_chars(kind, dx, dy, &super::grid::Machine::new(kind)),
    }
}

fn machine_color(kind: MachineKind) -> Color {
    match kind {
        MachineKind::Miner => Color::Cyan,
        MachineKind::Smelter => Color::Red,
        MachineKind::Assembler => Color::Magenta,
        MachineKind::Exporter => Color::Green,
    }
}

/// Check if cursor is on any part of the 2×2 machine anchored at (ax, ay).
fn cursor_on_machine(state: &FactoryState, ax: usize, ay: usize) -> bool {
    let cx = state.cursor_x;
    let cy = state.cursor_y;
    cx >= ax && cx <= ax + 1 && cy >= ay && cy <= ay + 1
}

fn render_grid(state: &FactoryState, f: &mut Frame, area: Rect) {
    let anim_idx = (state.anim_frame / 3) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Pre-compute I/O hints for adjacent cells when cursor is on a machine
    let io_hints = compute_io_hints(state);

    for y in 0..GRID_H {
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(" ", Style::default()));

        for x in 0..GRID_W {
            let (text, base_style) = match &state.grid[y][x] {
                Cell::Empty => {
                    // Check for I/O hints on empty cells
                    if let Some((_, _, hint_ch, hint_color)) = io_hints.iter().find(|(hx, hy, _, _)| *hx == x && *hy == y) {
                        (format!("{} ", hint_ch), Style::default().fg(*hint_color))
                    } else {
                        (". ".to_string(), Style::default().fg(Color::DarkGray))
                    }
                }
                Cell::Machine(m) => {
                    // This is the anchor (TL) of a 2×2 machine
                    let blocked = is_output_blocked(&state.grid, x, y, m);
                    let color = machine_color(m.kind);
                    let chars = if blocked {
                        machine_cell_chars_blocked(m.kind, 0, 0).to_string()
                    } else {
                        machine_cell_chars(m.kind, 0, 0, m).to_string()
                    };
                    let style = if m.kind == MachineKind::Exporter && state.export_flash > 0 {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else if blocked {
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD)
                    } else if m.progress > 0 || !m.output_buffer.is_empty() {
                        Style::default()
                            .fg(color)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(color)
                    };
                    (chars, style)
                }
                Cell::MachinePart { anchor_x, anchor_y } => {
                    let ax = *anchor_x;
                    let ay = *anchor_y;
                    let dx = x - ax;
                    let dy = y - ay;
                    // Get machine data from anchor
                    let (chars, style) = if let Some(m) = machine_at(&state.grid, ax, ay) {
                        let blocked = is_output_blocked(&state.grid, ax, ay, m);
                        let color = machine_color(m.kind);
                        let chars = if blocked && dx == 0 && dy == 0 {
                            machine_cell_chars_blocked(m.kind, dx, dy).to_string()
                        } else {
                            machine_cell_chars(m.kind, dx, dy, m).to_string()
                        };
                        let style = if m.kind == MachineKind::Exporter && state.export_flash > 0 {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else if blocked {
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD)
                        } else if m.progress > 0 || !m.output_buffer.is_empty() {
                            Style::default()
                                .fg(color)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(color)
                        };
                        (chars, style)
                    } else {
                        ("  ".to_string(), Style::default())
                    };
                    (chars, style)
                }
                Cell::Belt(b) => {
                    if let Some(item) = &b.item {
                        (
                            format!("{} ", item.symbol()),
                            Style::default()
                                .fg(item.color())
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        let idx = anim_idx % 4;
                        let ch = match b.direction {
                            super::grid::Direction::Right => BELT_ANIM_R[idx],
                            super::grid::Direction::Left => BELT_ANIM_L[idx],
                            super::grid::Direction::Up => BELT_ANIM_U[idx],
                            super::grid::Direction::Down => BELT_ANIM_D[idx],
                        };
                        (format!("{} ", ch), Style::default().fg(Color::White))
                    }
                }
            };

            // Cursor highlighting: highlight 2×2 block if cursor is on any part of a machine
            let is_highlighted = if let Some((ax, ay)) = anchor_of(&state.grid, x, y) {
                cursor_on_machine(state, ax, ay)
            } else {
                x == state.cursor_x && y == state.cursor_y
            };

            let style = if is_highlighted {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            spans.push(Span::styled(text, style));
        }

        lines.push(Line::from(spans));
    }

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" Grid (H/J/K/Lで移動, Spaceで設置) "),
    );
    f.render_widget(widget, area);
}

fn render_stats(state: &FactoryState, f: &mut Frame, area: Rect) {
    let anim_idx = (state.anim_frame / 3) as usize;

    // Count active machines
    let mut miners = 0u32;
    let mut smelters = 0u32;
    let mut assemblers = 0u32;
    let mut exporters = 0u32;
    for row in &state.grid {
        for cell in row {
            if let Cell::Machine(m) = cell {
                match m.kind {
                    MachineKind::Miner => miners += 1,
                    MachineKind::Smelter => smelters += 1,
                    MachineKind::Assembler => assemblers += 1,
                    MachineKind::Exporter => exporters += 1,
                }
            }
        }
    }

    let s = |count: u32| -> String {
        if count > 0 {
            let idx = (anim_idx + count as usize) % SPINNER.len();
            format!("{} ", SPINNER[idx])
        } else {
            "  ".to_string()
        }
    };

    let lines = vec![
        Line::from(format!(
            " {}Miner x{}    Iron Ore: {}",
            s(miners),
            miners,
            state.produced_count[0]
        )),
        Line::from(format!(
            " {}Smelter x{}  Iron Plate: {}",
            s(smelters),
            smelters,
            state.produced_count[1]
        )),
        Line::from(format!(
            " {}Assembler x{}  Gear: {}",
            s(assemblers),
            assemblers,
            state.produced_count[2]
        )),
        Line::from(format!(
            " {}Exporter x{}",
            s(exporters),
            exporters
        )),
        Line::from(""),
        Line::from(format!(
            " Exported: {}   Money: ${}",
            state.total_exported, state.money
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Miner→Belt→Smelter→Belt→Exporter",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(lines)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Stats "),
        );
    f.render_widget(widget, area);
}

fn render_log(state: &FactoryState, f: &mut Frame, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let start = if state.log.len() > visible_height {
        state.log.len() - visible_height
    } else {
        0
    };

    let log_lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|entry| {
            Line::from(Span::styled(
                format!(" {}", entry),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();

    let widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                .title(" Log "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

/// Tool descriptions for each placement tool.
fn tool_description(tool: &PlacementTool) -> &'static str {
    match tool {
        PlacementTool::None => "↑キーまたはクリックでツールを選択してください",
        PlacementTool::Miner => "鉱石(o)を自動生産。隣のベルトに出力します",
        PlacementTool::Smelter => "鉱石(o)→鉄板(=)に精錬。入力:鉱石",
        PlacementTool::Assembler => "鉄板(=)→歯車(*)を組立。入力:鉄板",
        PlacementTool::Exporter => "アイテムを売却して$に変換。何でも受付",
        PlacementTool::Belt => "アイテムを運ぶベルトコンベア [R]で回転",
        PlacementTool::Delete => "設置済みの機械やベルトを撤去します",
    }
}

/// Tool color for each placement tool.
fn tool_color(tool: &PlacementTool) -> Color {
    match tool {
        PlacementTool::None => Color::DarkGray,
        PlacementTool::Miner => Color::Cyan,
        PlacementTool::Smelter => Color::Red,
        PlacementTool::Assembler => Color::Magenta,
        PlacementTool::Exporter => Color::Green,
        PlacementTool::Belt => Color::White,
        PlacementTool::Delete => Color::Red,
    }
}

fn render_tool_panel(
    state: &FactoryState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // Tool definitions: (key, tool variant, label, cost_str)
    let tools: Vec<(char, PlacementTool, &str, String)> = vec![
        ('1', PlacementTool::Miner, "Miner", "$10".into()),
        ('2', PlacementTool::Smelter, "Smelter", "$25".into()),
        ('3', PlacementTool::Assembler, "Assembler", "$50".into()),
        ('4', PlacementTool::Exporter, "Exporter", "$15".into()),
        ('b', PlacementTool::Belt, "Belt", format!("$2 {}", state.belt_direction.arrow())),
        ('d', PlacementTool::Delete, "Delete", "---".into()),
    ];

    let mut lines: Vec<Line> = Vec::new();

    // Tool selection rows
    for (key, tool, label, cost) in &tools {
        let is_selected = std::mem::discriminant(&state.tool) == std::mem::discriminant(tool);
        let color = tool_color(tool);

        let marker = if is_selected { "▶" } else { " " };

        let key_style = if is_selected {
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label_style = if is_selected {
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {}[{}] ", marker, key), key_style),
            Span::styled(format!("{:<10}", label), label_style),
            Span::styled(format!("{:<6}", cost), label_style),
        ]));
    }

    // Description of selected tool
    let desc = tool_description(&state.tool);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" {}", desc),
        Style::default()
            .fg(tool_color(&state.tool))
            .add_modifier(Modifier::BOLD),
    )));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" ツール [↑↓/Space設置/R回転/Q戻る] "),
    );
    f.render_widget(widget, area);

    // Register click targets for each tool row
    let mut cs = click_state.borrow_mut();
    for (i, (key, _, _, _)) in tools.iter().enumerate() {
        cs.add_target(area.y + 1 + i as u16, *key);
    }
}

