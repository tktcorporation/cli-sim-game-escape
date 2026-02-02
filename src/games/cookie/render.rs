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
    let width = area.width;
    let is_narrow = is_narrow_layout(width);

    // Horizontal split: show log panel on the right when wide enough (>= 80 cols)
    let (main_area, log_area) = if width >= 80 {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        (h_chunks[0], Some(h_chunks[1]))
    } else {
        (area, None)
    };

    // Calculate dynamic heights for buffs/golden/discount section
    let buff_height = if is_narrow {
        let has_anything = !state.active_buffs.is_empty()
            || state.golden_event.is_some()
            || state.active_discount > 0.0;
        if has_anything { 3 } else { 0 }
    } else {
        let bh = if state.active_buffs.is_empty() { 0 } else { 2 + state.active_buffs.len() as u16 };
        let gh = if state.golden_event.is_some() { 3 } else { 0 };
        let dh: u16 = if state.active_discount > 0.0 { 1 } else { 0 };
        bh + gh + dh
    };

    // Cookie display height adapts to available space
    let cookie_height: u16 = if is_narrow { 3 } else { 5 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(cookie_height),
            Constraint::Length(buff_height),
            Constraint::Min(5),
            Constraint::Length(5),
        ])
        .split(main_area);

    // Same components for every width — each adapts internally
    render_cookie_display(state, f, chunks[0], click_state);
    if buff_height > 0 {
        render_buffs_and_golden(state, f, chunks[1], click_state);
    }
    if state.show_milestones {
        render_milestones(state, f, chunks[2], click_state);
    } else if state.show_upgrades {
        render_upgrades(state, f, chunks[2], click_state);
    } else {
        render_producers(state, f, chunks[2], click_state);
    }
    render_help(state, f, chunks[3], click_state);

    if let Some(log_area) = log_area {
        render_log(state, f, log_area);
    }
}

fn render_cookie_display(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let w = area.width;
    let h = area.height;

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

    // Borders adapt to width: full borders when wide, horizontal-only when narrow
    let borders = if w >= 60 { Borders::ALL } else { Borders::TOP | Borders::BOTTOM };

    // Cookie color changes with click flash
    let cookie_color = if state.click_flash > 0 {
        Color::White
    } else {
        Color::Yellow
    };

    // Border style changes on purchase
    let border_color = if state.purchase_flash > 0 {
        let phase = state.purchase_flash % 3;
        match phase {
            0 => Color::Magenta,
            1 => Color::Cyan,
            _ => Color::Green,
        }
    } else {
        Color::Yellow
    };

    // Show buff/purchase indicator in title
    let title = if state.purchase_flash > 0 {
        " ✨ Cookie Factory ✨ "
    } else if !state.active_buffs.is_empty() {
        " Cookie Factory ⚡ "
    } else {
        " Cookie Factory "
    };

    // Decide whether to show ASCII art based on available space
    let show_art = w >= 40 && h >= 5;

    if show_art {
        // Animated cookie + stats (pressed animation on click)
        let cookie_art = if state.click_flash > 0 {
            let idx = state.click_flash as usize % COOKIE_CLICK_FRAMES.len();
            COOKIE_CLICK_FRAMES[idx]
        } else {
            let idx = (state.anim_frame / 5) as usize % COOKIE_FRAMES.len();
            COOKIE_FRAMES[idx]
        };

        let click_label = if click_power > 1.0 {
            format!(">>> [C] +{} <<< ", format_number(click_power))
        } else {
            ">>> [C] CLICK! <<< ".to_string()
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
            Line::from({
                let mut spans = vec![
                    Span::styled(cookie_art[1], Style::default().fg(cookie_color)),
                    Span::styled(
                        format!(" {} {}/sec   Clicks: {}", spinner, cps_str, state.total_clicks),
                        Style::default().fg(Color::White),
                    ),
                ];
                if state.milk > 0.0 {
                    spans.push(Span::styled(
                        format!("  🥛{:.0}%", state.milk * 100.0),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ));
                    if state.kitten_multiplier > 1.001 {
                        spans.push(Span::styled(
                            format!(" 🐱×{:.2}", state.kitten_multiplier),
                            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                        ));
                    }
                }
                spans
            }),
            Line::from(vec![
                Span::styled(cookie_art[2], Style::default().fg(cookie_color)),
                Span::styled("  ", Style::default()),
                Span::styled(click_label, click_style),
            ]),
        ];

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(border_color))
                .title(title),
        );
        f.render_widget(widget, area);
    } else {
        // Compact single-line display for narrow/short screens
        let click_label = if click_power > 1.0 {
            format!(" [C]+{} ", format_number(click_power))
        } else {
            " [C]CLICK ".to_string()
        };
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
            Span::styled(click_label, click_style),
        ]);
        let widget = Paragraph::new(line)
            .block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(border_color))
                    .title(title),
            )
            .alignment(Alignment::Center);
        f.render_widget(widget, area);
    }

    // Particles render on ALL screen sizes
    render_particles(state, f, area);

    // Register the whole cookie display area as a click target for 'c'
    let mut cs = click_state.borrow_mut();
    for row in area.y..area.y + area.height {
        cs.add_target(row, 'c');
    }
}

/// Render floating particles as overlays on the cookie display area.
fn render_particles(state: &CookieState, f: &mut Frame, area: Rect) {
    let center_x = area.x + area.width / 2;
    let base_y = area.y + area.height;

    for particle in &state.particles {
        let progress = 1.0 - (particle.life as f32 / particle.max_life as f32);
        let rise = (progress * 4.0) as u16;
        let y = base_y.saturating_sub(1 + rise);
        let x = (center_x as i16 + particle.col_offset).max(area.x as i16) as u16;

        if y >= area.y && y < area.y + area.height && x < area.x + area.width {
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

    // Discount indicator
    if state.active_discount > 0.0 {
        lines.push(Line::from(Span::styled(
            format!(" 💰 割引ウェーブ発動中！次の購入{:.0}%OFF", state.active_discount * 100.0),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
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
        .filter(|p| {
            let eff_cost = p.cost() * (1.0 - state.active_discount);
            state.cookies >= eff_cost
        })
        .filter_map(|p| {
            let syn = state.synergy_bonus(&p.kind);
            p.payback_seconds_with_synergy(syn)
        })
        .fold(f64::MAX, f64::min);

    let has_discount = state.active_discount > 0.0;

    let items: Vec<ListItem> = state
        .producers
        .iter()
        .map(|p| {
            let eff_cost = p.cost() * (1.0 - state.active_discount);
            let can_afford = state.cookies >= eff_cost;
            let cost_str = if has_discount {
                format!("{}→{}", format_number(p.cost().floor()), format_number(eff_cost.floor()))
            } else {
                format_number(p.cost().floor())
            };
            let syn_bonus = state.synergy_bonus(&p.kind);
            let cs_bonus = state.count_scaling_bonus(&p.kind);
            let total_bonus = syn_bonus + cs_bonus;
            let cps = p.cps_with_synergy(total_bonus);
            let cps_str = format_number(cps);
            let next_cps = p.next_unit_cps_with_synergy(total_bonus);
            let payback = p.payback_seconds_with_synergy(total_bonus);

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

            // Bonus indicator (synergy + count scaling)
            let syn_str = if total_bonus > 0.001 {
                format!("+{:.0}%", total_bonus * 100.0)
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
    let toggle_label = if state.show_upgrades || state.show_milestones {
        "[U] Producersに戻る"
    } else {
        "[U] Upgradeを見る"
    };

    let milestone_label = if state.show_milestones {
        "[M] Producersに戻る"
    } else {
        "[M] マイルストーン"
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
            Span::styled(
                "[M] ",
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("{}  ", milestone_label.trim_start_matches("[M] ")),
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

fn render_milestones(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    _click_state: &Rc<RefCell<ClickState>>,
) {
    let achieved = state.achieved_milestone_count();
    let total = state.milestones.len();

    // Milk bar
    let milk_pct = state.milk * 100.0;
    let bar_width = 20usize;
    let filled = ((state.milk * bar_width as f64).round() as usize).min(bar_width);
    let milk_bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

    let mut lines: Vec<Line> = Vec::new();

    // Header: milk gauge
    lines.push(Line::from(vec![
        Span::styled(
            format!(" 🥛 ミルク: {:.0}% ", milk_pct),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            milk_bar,
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("  🐱×{:.2}  ", state.kitten_multiplier),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({}/{})", achieved, total),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        " ─────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )));

    // List milestones
    for milestone in &state.milestones {
        let (icon, name_style, desc_style) = if milestone.achieved {
            (
                "🏆",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(Color::White),
            )
        } else {
            (
                "  ",
                Style::default().fg(Color::DarkGray),
                Style::default().fg(Color::DarkGray),
            )
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", icon), name_style),
            Span::styled(format!("{} ", milestone.name), name_style),
            Span::styled(format!("- {}", milestone.description), desc_style),
        ]));
    }

    let border_color = if state.milestone_flash > 0 {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(format!(
                    " マイルストーン ({}/{}) ",
                    achieved, total
                )),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
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
