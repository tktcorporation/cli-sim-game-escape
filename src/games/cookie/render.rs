//! Cookie Factory rendering with animations, synergies, golden cookies, and buffs.

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

/// Animated cookie frames (cycles every ~2 seconds at 10 ticks/sec).
const COOKIE_FRAMES: &[&[&str]] = &[
    &["  (@@)  ", " (@@@@) ", "  (@@)  "],
    &["  (##)  ", " (####) ", "  (##)  "],
    &["  (**) ", " (****) ", "  (**)  "],
    &["  (@@)  ", " (@@@@) ", "  (@@)  "],
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

    // Calculate left panel height based on active buffs
    let buff_height = if state.active_buffs.is_empty() { 0 } else { 2 + state.active_buffs.len() as u16 };
    let golden_height = if state.golden_event.is_some() { 3 } else { 0 };
    let extra_height = buff_height + golden_height;

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Cookie display + click
            Constraint::Length(extra_height), // Buffs + Golden
            Constraint::Min(5),    // Producers or Upgrades
            Constraint::Length(5), // Controls/help
        ])
        .split(h_chunks[0]);

    render_cookie_display(state, f, left_chunks[0], false, click_state);
    if extra_height > 0 {
        render_buffs_and_golden(state, f, left_chunks[1], click_state);
    }
    if state.show_upgrades {
        render_upgrades(state, f, left_chunks[2], click_state);
    } else {
        render_producers(state, f, left_chunks[2], click_state);
    }
    render_help(state, f, left_chunks[3], click_state);
    render_log(state, f, h_chunks[1]);
}

fn render_narrow(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let buff_height = if state.active_buffs.is_empty() && state.golden_event.is_none() { 0 } else { 3 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Cookie display compact
            Constraint::Length(buff_height),
            Constraint::Min(5),   // Producers or Upgrades
            Constraint::Length(5), // Controls/help
        ])
        .split(area);

    render_cookie_display(state, f, chunks[0], true, click_state);
    if buff_height > 0 {
        render_buffs_and_golden(state, f, chunks[1], click_state);
    }
    if state.show_upgrades {
        render_upgrades(state, f, chunks[2], click_state);
    } else {
        render_producers(state, f, chunks[2], click_state);
    }
    render_help(state, f, chunks[3], click_state);
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

    let click_power = state.effective_click_power();
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

    // Show buff indicator in title if any buff is active
    let title = if !state.active_buffs.is_empty() {
        " Cookie Factory ⚡ "
    } else {
        " Cookie Factory "
    };

    if is_narrow {
        let click_label = if click_power > 1.0 {
            format!(" [C]+{} ", format_number(click_power))
        } else {
            " [C]CLICK ".to_string()
        };
        let line = Line::from(vec![
            Span::styled(
                format!("🍪 {} ", cookies_str),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} {}/s ", spinner, cps_str),
                Style::default().fg(Color::White),
            ),
            Span::styled(click_label, click_style),
        ]);
        let widget = Paragraph::new(line)
            .block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(title),
            )
            .alignment(Alignment::Center);
        f.render_widget(widget, area);
    } else {
        // Animated cookie + stats
        let cookie_frame_idx = (state.anim_frame / 5) as usize % COOKIE_FRAMES.len();
        let cookie_art = COOKIE_FRAMES[cookie_frame_idx];

        let click_label = if click_power > 1.0 {
            format!(">>> [C] +{} <<< ", format_number(click_power))
        } else {
            ">>> [C] CLICK! <<< ".to_string()
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(cookie_art[0], Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(" Cookies: {}", cookies_str),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(cookie_art[1], Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!(" {} {}/sec   Clicks: {}", spinner, cps_str, state.total_clicks),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled(cookie_art[2], Style::default().fg(Color::Yellow)),
                Span::styled("  ", Style::default()),
                Span::styled(click_label, click_style),
            ]),
        ];

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Yellow))
                .title(title),
        );
        f.render_widget(widget, area);
    }

    // Register the whole cookie display area as a click target for 'c'
    let mut cs = click_state.borrow_mut();
    for row in area.y..area.y + area.height {
        cs.add_target(row, 'c');
    }
}

/// Render active buffs and golden cookie indicator.
fn render_buffs_and_golden(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut lines: Vec<Line> = Vec::new();

    // Golden cookie indicator
    if let Some(ref event) = state.golden_event {
        let secs_left = event.appear_ticks_left as f64 / 10.0;
        let blink = (state.anim_frame / 2).is_multiple_of(2);
        let golden_style = if blink {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(vec![
            Span::styled(" 🍪 ゴールデンクッキー！ ", golden_style),
            Span::styled(
                format!("[G]で取得 (残り{:.0}秒)", secs_left),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Register golden cookie click target
        let mut cs = click_state.borrow_mut();
        cs.add_target(area.y, 'g');
        if area.height > 1 {
            cs.add_target(area.y + 1, 'g');
        }
    }

    // Active buffs
    for buff in &state.active_buffs {
        let secs_left = buff.ticks_left as f64 / 10.0;
        let bar_len = 10;
        let max_ticks = match &buff.effect {
            super::state::GoldenEffect::ProductionFrenzy { .. } => 70,
            super::state::GoldenEffect::ClickFrenzy { .. } => 100,
            _ => 70,
        };
        let filled = ((buff.ticks_left as f64 / max_ticks as f64) * bar_len as f64).ceil() as usize;
        let bar: String = "█".repeat(filled.min(bar_len)) + &"░".repeat(bar_len - filled.min(bar_len));

        let buff_color = match &buff.effect {
            super::state::GoldenEffect::ProductionFrenzy { .. } => Color::Magenta,
            super::state::GoldenEffect::ClickFrenzy { .. } => Color::Cyan,
            _ => Color::Yellow,
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" ⚡ {} ", buff.effect.detail()),
                Style::default().fg(buff_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} {:.0}s", bar, secs_left),
                Style::default().fg(buff_color),
            ),
        ]));
    }

    if !lines.is_empty() {
        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(widget, area);
    }
}

fn render_producers(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // Find the best ROI (lowest payback time) among affordable producers, using synergy
    let best_payback = state
        .producers
        .iter()
        .filter(|p| state.cookies >= p.cost())
        .filter_map(|p| {
            let syn = state.synergy_bonus(&p.kind);
            p.payback_seconds_with_synergy(syn)
        })
        .fold(f64::MAX, f64::min);

    let items: Vec<ListItem> = state
        .producers
        .iter()
        .map(|p| {
            let can_afford = state.cookies >= p.cost();
            let cost_str = format_number(p.cost().floor());
            let syn_bonus = state.synergy_bonus(&p.kind);
            let cps = p.cps_with_synergy(syn_bonus);
            let cps_str = format_number(cps);
            let next_cps = p.next_unit_cps_with_synergy(syn_bonus);
            let payback = p.payback_seconds_with_synergy(syn_bonus);

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

            // Synergy indicator
            let syn_str = if syn_bonus > 0.001 {
                format!("+{:.0}%", syn_bonus * 100.0)
            } else {
                String::new()
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
            let syn_style = Style::default().fg(Color::Magenta);

            let best_marker = if is_best_roi { "★" } else { " " };

            let mut spans = vec![
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
            ];

            if !syn_str.is_empty() {
                spans.push(Span::styled(format!(" {}", syn_str), syn_style));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
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
    // Show unpurchased upgrades, distinguishing unlocked vs locked
    let available: Vec<(usize, &super::state::Upgrade, bool)> = state
        .upgrades
        .iter()
        .enumerate()
        .filter(|(_, u)| !u.purchased)
        .map(|(i, u)| (i, u, state.is_upgrade_unlocked(u)))
        .collect();

    let items: Vec<ListItem> = available
        .iter()
        .enumerate()
        .map(|(display_idx, (_, upgrade, unlocked))| {
            let can_afford = state.cookies >= upgrade.cost && *unlocked;
            let key = (b'a' + display_idx as u8) as char;
            let cost_str = format_number(upgrade.cost);

            if *unlocked {
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
            } else {
                // Locked upgrade — show hint about unlock condition
                let hint = match &upgrade.unlock_condition {
                    Some((kind, count)) => {
                        let current = state.producers[kind.index()].count;
                        format!("🔒 {} {}台必要(現在{}台)", kind.name(), count, current)
                    }
                    None => "🔒".to_string(),
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" [{}] ", key), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} - {} ", upgrade.name, upgrade.description),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(hint, Style::default().fg(Color::Red)),
                ]))
            }
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
            .title(" Upgrades [A-Z]で購入 "),
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

    let golden_hint = if state.golden_event.is_some() {
        Span::styled(
            " [G] ゴールデン取得！",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("", Style::default())
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
            golden_hint,
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
