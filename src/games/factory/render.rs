/// Tiny Factory rendering with animations and help.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};

use super::grid::{Cell, MachineKind, GRID_H, GRID_W};
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
            Constraint::Length(3),  // Header
            Constraint::Min(5),    // Grid
            Constraint::Length(3), // Tool select
            Constraint::Length(5), // Help
        ])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(3)])
        .split(h_chunks[1]);

    render_header(state, f, left_chunks[0], false);
    render_grid(state, f, left_chunks[1]);
    render_tool_bar(state, f, left_chunks[2], click_state);
    render_help(state, f, left_chunks[3]);
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
            Constraint::Length(3),  // Header
            Constraint::Min(5),    // Grid
            Constraint::Length(3), // Tool select
            Constraint::Length(5), // Help
        ])
        .split(area);

    render_header(state, f, chunks[0], true);
    render_grid(state, f, chunks[1]);
    render_tool_bar(state, f, chunks[2], click_state);
    render_help(state, f, chunks[3]);
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

fn tool_short(tool: &PlacementTool) -> &str {
    match tool {
        PlacementTool::None => "---",
        PlacementTool::Miner => "Miner",
        PlacementTool::Smelter => "Smelt",
        PlacementTool::Assembler => "Asm",
        PlacementTool::Exporter => "Exp",
        PlacementTool::Belt => "Belt",
        PlacementTool::Delete => "Del",
    }
}

fn render_grid(state: &FactoryState, f: &mut Frame, area: Rect) {
    let anim_idx = (state.anim_frame / 3) as usize;
    let mut lines: Vec<Line> = Vec::new();

    for y in 0..GRID_H {
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(" ", Style::default()));

        for x in 0..GRID_W {
            let is_cursor = x == state.cursor_x && y == state.cursor_y;

            let (ch, base_style) = match &state.grid[y][x] {
                Cell::Empty => ('.', Style::default().fg(Color::DarkGray)),
                Cell::Machine(m) => {
                    // Animate active machines
                    let ch = if m.progress > 0 || !m.output_buffer.is_empty() {
                        let idx = (anim_idx + x + y) % SPINNER.len();
                        SPINNER[idx]
                    } else {
                        m.kind.symbol()
                    };
                    let color = match m.kind {
                        MachineKind::Miner => Color::Cyan,
                        MachineKind::Smelter => Color::Red,
                        MachineKind::Assembler => Color::Magenta,
                        MachineKind::Exporter => Color::Green,
                    };
                    // Exporter flash effect when export just happened
                    let style = if m.kind == MachineKind::Exporter && state.export_flash > 0 {
                        Style::default()
                            .fg(Color::Yellow)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else if m.progress > 0 || !m.output_buffer.is_empty() {
                        Style::default()
                            .fg(color)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(color)
                    };
                    (ch, style)
                }
                Cell::Belt(b) => {
                    if let Some(item) = &b.item {
                        // Item on belt: show item with bold
                        (
                            item.symbol(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        // Empty belt: animated directional arrow
                        let idx = anim_idx % 4;
                        let ch = match b.direction {
                            super::grid::Direction::Right => BELT_ANIM_R[idx],
                            super::grid::Direction::Left => BELT_ANIM_L[idx],
                            super::grid::Direction::Up => BELT_ANIM_U[idx],
                            super::grid::Direction::Down => BELT_ANIM_D[idx],
                        };
                        (ch, Style::default().fg(Color::White))
                    }
                }
            };

            let style = if is_cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            spans.push(Span::styled(format!("{} ", ch), style));
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

fn render_tool_bar(
    state: &FactoryState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let spans = vec![
        Span::styled("[1]", style_tool_key(&state.tool, &PlacementTool::Miner)),
        Span::styled("M ", Style::default().fg(Color::Cyan)),
        Span::styled("[2]", style_tool_key(&state.tool, &PlacementTool::Smelter)),
        Span::styled("S ", Style::default().fg(Color::Red)),
        Span::styled("[3]", style_tool_key(&state.tool, &PlacementTool::Assembler)),
        Span::styled("A ", Style::default().fg(Color::Magenta)),
        Span::styled("[4]", style_tool_key(&state.tool, &PlacementTool::Exporter)),
        Span::styled("E ", Style::default().fg(Color::Green)),
        Span::styled("[B]", style_tool_key(&state.tool, &PlacementTool::Belt)),
        Span::styled(
            format!("{} ", state.belt_direction.arrow()),
            Style::default().fg(Color::White),
        ),
        Span::styled("[D]", style_tool_key(&state.tool, &PlacementTool::Delete)),
        Span::styled("X ", Style::default().fg(Color::Red)),
    ];

    let widget = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" ツール選択 "),
        )
        .alignment(Alignment::Center);
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    for row in area.y..area.y + area.height {
        cs.add_target(row, ' ');
    }
}

fn render_help(_state: &FactoryState, f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled(
                " [H/J/K/L] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("カーソル移動  ", Style::default().fg(Color::White)),
            Span::styled(
                "[Space] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("設置", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(
                " [R] ",
                Style::default().fg(Color::Cyan),
            ),
            Span::styled("ベルト回転  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Q/Esc] ", Style::default().fg(Color::DarkGray)),
            Span::styled("メニューに戻る", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(Span::styled(
            " 目標: Miner→Belt→Smelter→Belt→Exporter",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" 操作方法 "),
    );
    f.render_widget(widget, area);
}

fn style_tool_key(current: &PlacementTool, this: &PlacementTool) -> Style {
    if std::mem::discriminant(current) == std::mem::discriminant(this) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
