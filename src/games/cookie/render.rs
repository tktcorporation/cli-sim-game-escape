/// Cookie Factory rendering with animations and help.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};

use super::logic::format_number;
use super::state::CookieState;

/// Animated cookie frames — normal state (cycles every ~2 seconds at 10 ticks/sec).
const COOKIE_FRAMES: &[&[&str]] = &[
    &["   ╭━●━╮  ", "  ━●━━━●━ ", "   ╰━●━╯  "],
    &["   ╭━○━╮  ", "  ━○━━━○━ ", "   ╰━○━╯  "],
    &["   ╭━◉━╮  ", "  ━◉━━━◉━ ", "   ╰━◉━╯  "],
    &["   ╭━○━╮  ", "  ━○━━━○━ ", "   ╰━○━╯  "],
];

/// Cookie frames — "pressed" state when clicked.
const COOKIE_CLICK_FRAMES: &[&[&str]] = &[
    &["  ╭━━●━━╮ ", " ━●━━━━━●━", "  ╰━━●━━╯ "],
    &["    ╭●╮   ", "   ━●●●━  ", "    ╰●╯   "],
];

/// Spinner characters for production indicator.
const SPINNER: &[char] = &['◐', '◓', '◑', '◒'];

pub fn render(state: &CookieState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);

    if is_narrow {
        render_narrow(state, f, area, click_state);
    } else {
        render_wide(state, f, area, click_state);
    }
}

fn render_wide(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Cookie display + click
            Constraint::Min(5),    // Producers or Upgrades
            Constraint::Length(5), // Controls/help
        ])
        .split(h_chunks[0]);

    render_cookie_display(state, f, left_chunks[0], false, click_state);
    if state.show_upgrades {
        render_upgrades(state, f, left_chunks[1], click_state);
    } else {
        render_producers(state, f, left_chunks[1], click_state);
    }
    render_help(state, f, left_chunks[2], click_state);
    render_log(state, f, h_chunks[1]);
}

fn render_narrow(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Cookie display compact
            Constraint::Min(5),   // Producers or Upgrades
            Constraint::Length(5), // Controls/help
        ])
        .split(area);

    render_cookie_display(state, f, chunks[0], true, click_state);
    if state.show_upgrades {
        render_upgrades(state, f, chunks[1], click_state);
    } else {
        render_producers(state, f, chunks[1], click_state);
    }
    render_help(state, f, chunks[2], click_state);
}

fn render_cookie_display(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    is_narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let cookies_str = format_number(state.cookies.floor());
    let cps_str = format_number(state.total_cps());
    let spinner_idx = (state.anim_frame / 3) as usize % SPINNER.len();
    let spinner = if state.total_cps() > 0.0 {
        SPINNER[spinner_idx]
    } else {
        ' '
    };

    let click_style = if state.click_flash > 0 {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };

    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // Cookie color changes with click flash
    let cookie_color = if state.click_flash > 0 {
        Color::White
    } else {
        Color::Yellow
    };

    // Border style changes on purchase
    let border_color = if state.purchase_flash > 0 {
        // Cycle through celebration colors
        let phase = state.purchase_flash % 3;
        match phase {
            0 => Color::Magenta,
            1 => Color::Cyan,
            _ => Color::Green,
        }
    } else {
        Color::Yellow
    };

    let title_str = if state.purchase_flash > 0 {
        " ✨ Cookie Factory ✨ "
    } else {
        " Cookie Factory "
    };

    if is_narrow {
        let line = Line::from(vec![
            Span::styled(
                format!("🍪 {} ", cookies_str),
                Style::default()
                    .fg(cookie_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} {}/s ", spinner, cps_str),
                Style::default().fg(Color::White),
            ),
            Span::styled(" [C]CLICK ", click_style),
        ]);
        let widget = Paragraph::new(line)
            .block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(border_color))
                    .title(title_str),
            )
            .alignment(Alignment::Center);
        f.render_widget(widget, area);
    } else {
        // Animated cookie + stats
        let cookie_art = if state.click_flash > 0 {
            // "Pressed" animation — cookie squishes
            let idx = state.click_flash as usize % COOKIE_CLICK_FRAMES.len();
            COOKIE_CLICK_FRAMES[idx]
        } else {
            let idx = (state.anim_frame / 5) as usize % COOKIE_FRAMES.len();
            COOKIE_FRAMES[idx]
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(cookie_art[0], Style::default().fg(cookie_color)),
                Span::styled(
                    format!(" Cookies: {}", cookies_str),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(cookie_art[1], Style::default().fg(cookie_color)),
                Span::styled(
                    format!(" {} {}/sec   Clicks: {}", spinner, cps_str, state.total_clicks),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled(cookie_art[2], Style::default().fg(cookie_color)),
                Span::styled("  ", Style::default()),
                Span::styled(">>> [C] CLICK! <<< ", click_style),
            ]),
        ];

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(border_color))
                .title(title_str),
        );
        f.render_widget(widget, area);

        // Render floating particles over the cookie area
        render_particles(state, f, area);
    }

    // Register the whole cookie display area as a click target for 'c'
    let mut cs = click_state.borrow_mut();
    for row in area.y..area.y + area.height {
        cs.add_target(row, 'c');
    }
}

/// Render floating particles as overlays on the cookie display area.
fn render_particles(state: &CookieState, f: &mut Frame, area: Rect) {
    let center_x = area.x + area.width / 2;
    let base_y = area.y + area.height; // particles float up from bottom

    for particle in &state.particles {
        // Calculate vertical offset: rises as life decreases
        let progress = 1.0 - (particle.life as f32 / particle.max_life as f32);
        let rise = (progress * 4.0) as u16; // rise up to 4 rows
        let y = base_y.saturating_sub(1 + rise);
        let x = (center_x as i16 + particle.col_offset).max(area.x as i16) as u16;

        if y >= area.y && y < area.y + area.height && x < area.x + area.width {
            // Color fades from bright to dim
            let color = if particle.life > particle.max_life * 2 / 3 {
                Color::White
            } else if particle.life > particle.max_life / 3 {
                Color::Yellow
            } else {
                Color::DarkGray
            };
            let style = Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD);

            let text_len = particle.text.len() as u16;
            let available = area.x + area.width - x;
            if text_len <= available {
                let particle_area = Rect::new(x, y, text_len, 1);
                let widget = Paragraph::new(Span::styled(&particle.text, style));
                f.render_widget(widget, particle_area);
            }
        }
    }
}

fn render_producers(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // Find the best ROI (lowest payback time) among affordable producers
    let best_payback = state
        .producers
        .iter()
        .filter(|p| state.cookies >= p.cost())
        .filter_map(|p| p.payback_seconds())
        .fold(f64::MAX, f64::min);

    let items: Vec<ListItem> = state
        .producers
        .iter()
        .map(|p| {
            let can_afford = state.cookies >= p.cost();
            let cost_str = format_number(p.cost().floor());
            let cps_str = format_number(p.cps());
            let next_cps = p.next_unit_cps();
            let payback = p.payback_seconds();

            // Check if this is the best ROI among affordable options
            let is_best_roi = can_afford
                && payback
                    .map(|pb| (pb - best_payback).abs() < 0.01)
                    .unwrap_or(false);

            // Production indicator animation
            let prod_indicator = if p.count > 0 {
                let idx = (state.anim_frame as usize / 5 + p.kind.key() as usize) % SPINNER.len();
                format!("{} ", SPINNER[idx])
            } else {
                "  ".to_string()
            };

            // Format payback time
            let payback_str = match payback {
                Some(s) if s < 60.0 => format!("{}s", s.round() as u32),
                Some(s) if s < 3600.0 => format!("{}m", (s / 60.0).round() as u32),
                Some(s) => format!("{}h", (s / 3600.0).round() as u32),
                None => "---".to_string(),
            };

            let key_style = if is_best_roi {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if can_afford {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let text_style = if can_afford {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let active_style = if p.count > 0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let roi_style = if is_best_roi {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if can_afford {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let best_marker = if is_best_roi { "★" } else { " " };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{}[{}] ", best_marker, p.kind.key()), key_style),
                Span::styled(
                    format!("{:<8} {:>2}x ", p.kind.name(), p.count),
                    text_style,
                ),
                Span::styled(prod_indicator, active_style),
                Span::styled(format!("{}/s ", cps_str), active_style),
                Span::styled(format!("${} ", cost_str), text_style),
                Span::styled(
                    format!("+{}/s ", format_number(next_cps)),
                    roi_style,
                ),
                Span::styled(format!("回収{}", payback_str), roi_style),
            ]))
        })
        .collect();

    let producer_border_color = if state.purchase_flash > 0 {
        Color::Yellow
    } else {
        Color::Green
    };
    let widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(producer_border_color))
            .title(" Producers [1-5]で購入 ★=最高効率 "),
    );
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    for (i, p) in state.producers.iter().enumerate() {
        cs.add_target(area.y + 1 + i as u16, p.kind.key());
    }
}

fn render_upgrades(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let available: Vec<(usize, &super::state::Upgrade)> = state
        .upgrades
        .iter()
        .enumerate()
        .filter(|(_, u)| !u.purchased)
        .collect();

    let items: Vec<ListItem> = available
        .iter()
        .enumerate()
        .map(|(display_idx, (_, upgrade))| {
            let can_afford = state.cookies >= upgrade.cost;
            let key = (b'a' + display_idx as u8) as char;
            let cost_str = format_number(upgrade.cost);

            let key_style = if can_afford {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let text_style = if can_afford {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" [{}] ", key), key_style),
                Span::styled(
                    format!("{} - {} ({})", upgrade.name, upgrade.description, cost_str),
                    text_style,
                ),
            ]))
        })
        .collect();

    let widget = if items.is_empty() {
        List::new(vec![ListItem::new(Span::styled(
            " (全て購入済み)",
            Style::default().fg(Color::DarkGray),
        ))])
    } else {
        List::new(items)
    }
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" Upgrades [A-F]で購入 "),
    );
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    for (display_idx, _) in available.iter().enumerate() {
        let key = (b'a' + display_idx as u8) as char;
        cs.add_target(area.y + 1 + display_idx as u16, key);
    }
}

fn render_help(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let toggle_label = if state.show_upgrades {
        "[U] Producersに戻る"
    } else {
        "[U] Upgradeを見る"
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                " [C] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("クリックで+1  ", Style::default().fg(Color::White)),
            Span::styled(
                "[1-5] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Producer購入", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(
                " [U] ",
                Style::default().fg(Color::Magenta),
            ),
            Span::styled(
                format!("{}  ", toggle_label.trim_start_matches("[U] ")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("[Q/Esc] ", Style::default().fg(Color::DarkGray)),
            Span::styled("メニューに戻る", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(Span::styled(
            " タップ/クリックでも操作できます",
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

    // 'U' toggle click target
    let mut cs = click_state.borrow_mut();
    cs.add_target(area.y + 2, 'u');
}

fn render_log(state: &CookieState, f: &mut Frame, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let start = if state.log.len() > visible_height {
        state.log.len() - visible_height
    } else {
        0
    };

    let total = state.log.len();
    let log_lines: Vec<Line> = state.log[start..]
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let global_idx = start + i;
            let is_recent = total > 0 && global_idx >= total.saturating_sub(3);

            if entry.is_important {
                let style = if is_recent {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                Line::from(Span::styled(&entry.text, style))
            } else if is_recent {
                Line::from(Span::styled(
                    &entry.text,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    &entry.text,
                    Style::default().fg(Color::DarkGray),
                ))
            }
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
