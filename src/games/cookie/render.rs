//! Cookie Factory rendering with animations, synergies, golden cookies, and buffs.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;

use super::actions::*;
use super::logic::format_number;
use super::state::{CookieState, ParticleStyle};

/// Compact cookie art — 3 lines, 8 chars wide. Shared across all screen sizes.
const COOKIE_ART: &[&[&str]] = &[
    &["╭━●━●━╮ ", "━●━━●━●━", "╰━●━●━╯ "],
    &["╭━○━○━╮ ", "━○━━○━○━", "╰━○━○━╯ "],
    &["╭━◉━◉━╮ ", "━◉━━◉━◉━", "╰━◉━◉━╯ "],
    &["╭━○━○━╮ ", "━○━━○━○━", "╰━○━○━╯ "],
];

/// Compact cookie art — "pressed" state when clicked.
const COOKIE_CLICK_ART: &[&[&str]] = &[
    &["╭●●●●●╮ ", "●●━━━●●━", "╰●●●●●╯ "],
    &[" ╭━●━╮  ", " ━●●●━  ", " ╰━●━╯  "],
];

/// Sparkline characters for CPS graph (8 levels of height).
const SPARKLINE_CHARS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇'];

/// Spinner characters for production indicator.
const SPINNER: &[char] = &['◐', '◓', '◑', '◒'];

pub fn render(state: &CookieState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let width = area.width;
    let is_narrow = width < 60;

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

    // Calculate dynamic heights for buffs/golden/discount section (unified for all widths)
    let buff_height = {
        let mut n = 0u16;
        if state.golden_event.is_some() { n += 1; }
        n += state.active_buffs.len() as u16;
        if state.active_discount > 0.0 { n += 1; }
        if n > 0 { n.min(4) } else { 0 }
    };

    // Cookie display height — adaptive: compact on narrow screens
    let cookie_height: u16 = if is_narrow { 8 } else { 12 };

    // Tab bar: horizontal (1 row) on all screen sizes
    let tab_rows = 1;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(cookie_height),
            Constraint::Length(buff_height),
            Constraint::Length(tab_rows), // tab bar (horizontal)
            Constraint::Min(5),          // content
        ])
        .split(main_area);

    // Same components for every width — each adapts internally
    render_cookie_display(state, f, chunks[0], click_state);
    if buff_height > 0 {
        render_buffs_and_golden(state, f, chunks[1], click_state);
    }
    render_tab_bar(state, f, chunks[2], click_state);
    if state.show_prestige {
        render_prestige(state, f, chunks[3], click_state);
    } else if state.show_milestones {
        render_milestones(state, f, chunks[3], click_state);
    } else if state.show_research {
        render_research(state, f, chunks[3], click_state);
    } else if state.show_upgrades {
        render_upgrades(state, f, chunks[3], click_state);
    } else {
        render_producers(state, f, chunks[3], click_state);
    }

    if let Some(log_area) = log_area {
        render_log(state, f, log_area);
    }
}

/// Render horizontal tab bar for switching between Producers / Upgrades / Research / Milestones / Prestige.
/// All tabs fit in a single row with short labels; click targets use column regions.
fn render_tab_bar(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let ready_count = state.ready_milestone_count();
    let is_narrow = area.width < 60;

    // Determine active tab index
    let active = if state.show_prestige {
        4
    } else if state.show_milestones {
        3
    } else if state.show_research {
        2
    } else if state.show_upgrades {
        1
    } else {
        0
    };

    let tab_style = |idx: usize, base_color: Color| -> Style {
        if idx == active {
            Style::default()
                .fg(Color::Black)
                .bg(base_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base_color)
        }
    };

    let milestone_color = if ready_count > 0 { Color::Green } else { Color::Cyan };
    let pending_chips = state.pending_heavenly_chips();
    let prestige_color = if pending_chips > 0 { Color::Yellow } else { Color::Blue };

    // Build short labels for horizontal layout
    let milestone_label = if is_narrow {
        if ready_count > 0 { format!("目標{}", ready_count) } else { "目標".to_string() }
    } else if ready_count > 0 {
        format!("目標({})", ready_count)
    } else {
        "目標".to_string()
    };

    let prestige_label = if is_narrow {
        if pending_chips > 0 { format!("転生+{}", pending_chips) } else { "転生".to_string() }
    } else if pending_chips > 0 {
        format!("転生(+{})", pending_chips)
    } else {
        "転生".to_string()
    };

    let tabs: [(String, Style, u16); 5] = [
        ("生産".to_string(), tab_style(0, Color::Green), TAB_PRODUCERS),
        ("強化".to_string(), tab_style(1, Color::Magenta), TAB_UPGRADES),
        ("研究".to_string(), tab_style(2, Color::Cyan), TAB_RESEARCH),
        (milestone_label, tab_style(3, milestone_color), TAB_MILESTONES),
        (prestige_label, tab_style(4, prestige_color), TAB_PRESTIGE),
    ];

    // Render all tabs on a single horizontal row
    let mut spans: Vec<Span> = Vec::new();
    let separator = if is_narrow { "|" } else { " │ " };

    for (i, (label, style, _)) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(separator, Style::default().fg(Color::DarkGray)));
        }
        let padded = format!(" {} ", label);
        spans.push(Span::styled(padded, *style));
    }

    let line = Line::from(spans);
    let widget = Paragraph::new(line);
    f.render_widget(widget, area);

    // Register click targets using equal-width segments across the full row.
    // This is more robust than character-position-based targets because:
    // - No character width calculation needed (CJK width issues eliminated)
    // - No gaps between targets (continuous coverage)
    // - Works regardless of font rendering or CSS scaling differences
    let num_tabs = tabs.len() as u16;
    let mut cs = click_state.borrow_mut();
    for (i, (_, _, action_id)) in tabs.iter().enumerate() {
        let seg_start = area.width * i as u16 / num_tabs;
        let seg_end = area.width * (i as u16 + 1) / num_tabs;
        let rect = Rect::new(
            area.x + seg_start,
            area.y,
            seg_end - seg_start,
            1,
        );
        cs.targets.push(crate::input::ClickTarget {
            rect,
            action_id: *action_id,
        });
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
    let cps = state.total_cps();
    let cps_str = format_number(cps);
    let spinner_idx = (state.anim_frame / 3) as usize % SPINNER.len();
    let spinner = if cps > 0.0 { SPINNER[spinner_idx] } else { ' ' };

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

    let borders = if w >= 60 { Borders::ALL } else { Borders::TOP | Borders::BOTTOM };

    let cookie_color = if state.click_flash > 0 { Color::White } else { Color::Yellow };

    let border_color = if state.purchase_flash > 0 {
        Color::White
    } else if state.combo_count >= 20 {
        if state.anim_frame % 4 < 2 { Color::Yellow } else { Color::White }
    } else if !state.active_buffs.is_empty() {
        Color::Cyan
    } else {
        Color::Yellow
    };

    let title = if state.purchase_flash > 0 {
        " ✦ Cookie Factory ✦ "
    } else if !state.active_buffs.is_empty() {
        " Cookie Factory ⚡ "
    } else {
        " Cookie Factory "
    };

    // --- Unified art selection (same on all screen widths) ---
    let cookie_art = if state.click_flash > 0 {
        let idx = state.click_flash as usize % COOKIE_CLICK_ART.len();
        COOKIE_CLICK_ART[idx]
    } else {
        let idx = (state.anim_frame / 5) as usize % COOKIE_ART.len();
        COOKIE_ART[idx]
    };

    let click_label = if click_power > 1.0 {
        format!("CLICK +{}", format_number(click_power))
    } else {
        "CLICK!".to_string()
    };

    let ready_count = state.ready_milestone_count();

    // CPS delta indicator
    let delta_indicator = if state.cps_delta > 0.1 {
        Span::styled(
            format!(" ▲+{}/s", format_number(state.cps_delta)),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )
    } else if state.cps_delta < -0.1 {
        Span::styled(
            format!(" ▼{}/s", format_number(state.cps_delta)),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ─", Style::default().fg(Color::DarkGray))
    };

    let mut lines: Vec<Line> = Vec::new();

    // --- Row 0: Art[0] + cookie count ---
    lines.push(Line::from(vec![
        Span::styled(cookie_art[0], Style::default().fg(cookie_color)),
        Span::styled(
            format!(" 🍪 {}", cookies_str),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
    ]));

    // --- Row 1: Art[1] + CPS with delta ---
    lines.push(Line::from(vec![
        Span::styled(cookie_art[1], Style::default().fg(cookie_color)),
        Span::styled(
            format!(" {} {}/sec", spinner, cps_str),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        delta_indicator,
    ]));

    // --- Row 2: Art[2] + click button + combo ---
    let combo_span = if state.combo_count >= 5 {
        Span::styled(
            format!(" ×{}", state.combo_count),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("", Style::default())
    };
    lines.push(Line::from(vec![
        Span::styled(cookie_art[2], Style::default().fg(cookie_color)),
        Span::styled(" ", Style::default()),
        Span::styled(&click_label, click_style),
        combo_span,
    ]));

    // --- Row 3: Stats (clicks / milk / kitten / prestige / milestones) ---
    lines.push(Line::from({
        let mut spans = vec![
            Span::styled(
                format!(" 👆{}", state.total_clicks),
                Style::default().fg(Color::Cyan),
            ),
        ];
        if state.milk > 0.0 {
            spans.push(Span::styled(
                format!(" 🥛{:.0}%", state.milk * 100.0),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));
            if state.kitten_multiplier > 1.001 {
                spans.push(Span::styled(
                    format!(" 🐱×{:.2}", state.kitten_multiplier),
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                ));
            }
        }
        if state.prestige_count > 0 {
            spans.push(Span::styled(
                format!(" 👼×{:.2}", state.prestige_multiplier),
                Style::default().fg(Color::Magenta),
            ));
        }
        if ready_count > 0 {
            spans.push(Span::styled(
                format!(" ✨{}個!", ready_count),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }
        spans
    }));

    // --- Wide-only rows: sparkline and production bars ---
    if w >= 60 {
        // --- Row 4: CPS Trend sparkline + best CPS ---
        let sparkline_width = (w as usize).saturating_sub(22).clamp(6, 20);
        let sparkline = build_sparkline(&state.cps_history, sparkline_width);
        let sparkline_color = cycling_color(state.anim_frame, 30);

        lines.push(Line::from({
            let mut spans = vec![
                Span::styled(
                    " ┄┄ CPS ",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(sparkline, Style::default().fg(sparkline_color)),
                Span::styled(
                    format!(" {}/s", cps_str),
                    Style::default().fg(Color::White),
                ),
            ];
            if state.best_cps > 0.0 {
                spans.push(Span::styled(
                    format!(" 最高:{}/s", format_number(state.best_cps)),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            spans
        }));

        // --- Row 5: Production header ---
        lines.push(Line::from(Span::styled(
            " ┄┄ PRODUCTION ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));

        // --- Rows 6+: Producer contribution bars (dynamically sized) ---
        let contributions = state.producer_contributions();
        // Reserve 1 line for status bar; borders take 2 lines
        let max_bar_rows = (h.saturating_sub(2) as usize).saturating_sub(lines.len() + 1).max(1);

        if contributions.is_empty() {
            lines.push(Line::from(Span::styled(
                " (生産者を購入しましょう)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let bar_width = 6usize;
            let entry_approx = 14usize; // "Name:██░░░12% " ≈ 14 chars
            let items_per_row = (w.saturating_sub(2) as usize / entry_approx).max(1);
            let colors = [Color::Cyan, Color::Green, Color::Magenta, Color::Yellow,
                         Color::Blue, Color::Red, Color::White, Color::LightCyan];
            let anim_offset = (state.anim_frame / 2) as usize;

            for (bar_rows, chunk) in contributions.chunks(items_per_row).enumerate() {
                if bar_rows >= max_bar_rows {
                    break;
                }
                let mut row_spans: Vec<Span> = vec![Span::styled(" ", Style::default())];
                for (i, (name, _cps, frac)) in chunk.iter().enumerate() {
                    let filled = ((*frac * bar_width as f64).round() as usize).min(bar_width);
                    let ci = contributions.iter().position(|(n, _, _)| *n == *name).unwrap_or(i);
                    let color = colors[ci % colors.len()];
                    let pulse = if filled > 0 && (anim_offset + ci).is_multiple_of(8) { "█" } else { "▓" };
                    let bar: String = pulse.repeat(filled) + &"░".repeat(bar_width - filled);
                    row_spans.push(Span::styled(
                        format!("{}:", name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                    row_spans.push(Span::styled(bar, Style::default().fg(color)));
                    row_spans.push(Span::styled(
                        format!("{:.0}% ", frac * 100.0),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                lines.push(Line::from(row_spans));
            }
        }
    }

    // --- Status bar (always last line) ---
    // On narrow screens, show only the most important status items
    let mut status_spans: Vec<Span> = Vec::new();

    if w >= 60 {
        let play_secs = state.total_ticks / 10;
        let play_h = play_secs / 3600;
        let play_m = (play_secs % 3600) / 60;
        let play_s = play_secs % 60;
        status_spans.push(Span::styled(
            format!(" ⏱{}h{}m{}s", play_h, play_m, play_s),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if !state.active_buffs.is_empty() {
        let buff_blink = (state.anim_frame / 3).is_multiple_of(2);
        status_spans.push(Span::styled(
            format!(" ⚡×{}", state.active_buffs.len()),
            Style::default().fg(if buff_blink { Color::Yellow } else { Color::Magenta })
                .add_modifier(Modifier::BOLD),
        ));
    }
    if state.golden_event.is_some() {
        let golden_blink = (state.anim_frame / 2).is_multiple_of(2);
        status_spans.push(Span::styled(
            " 🍪G!",
            Style::default().fg(if golden_blink { Color::Yellow } else { Color::White })
                .add_modifier(Modifier::BOLD),
        ));
    }
    if state.active_discount > 0.0 {
        status_spans.push(Span::styled(
            format!(" 💰{:.0}%OFF", state.active_discount * 100.0),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));
    }
    {
        use super::state::MarketPhase;
        let (market_color, market_blink) = match &state.market_phase {
            MarketPhase::Bull => (Color::Red, true),
            MarketPhase::Bear => (Color::Blue, true),
            MarketPhase::Normal => (Color::DarkGray, false),
        };
        let secs_left = state.market_ticks_left / 10;
        let style = if market_blink && (state.anim_frame / 4).is_multiple_of(2) {
            Style::default().fg(market_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(market_color)
        };
        if w >= 60 {
            status_spans.push(Span::styled(
                format!(" {}{}({}s)", state.market_phase.symbol(), state.market_phase.name(), secs_left),
                style,
            ));
        } else {
            // Compact: just symbol + seconds
            status_spans.push(Span::styled(
                format!(" {}{}s", state.market_phase.symbol(), secs_left),
                style,
            ));
        }
    }
    if state.dragon_level > 0 {
        status_spans.push(Span::styled(
            format!(" 🐉{}", state.dragon_level),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if status_spans.is_empty() {
        status_spans.push(Span::styled(" ", Style::default()));
    }
    lines.push(Line::from(status_spans));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(border_color))
            .title(title),
    );
    f.render_widget(widget, area);

    // Particles render on all screen sizes
    render_particles(state, f, area);

    // Register the whole cookie display area as a click target
    let mut cs = click_state.borrow_mut();
    cs.add_click_target(area, CLICK_COOKIE);
}

/// Build a sparkline string from a history of values.
fn build_sparkline(history: &[f64], max_width: usize) -> String {
    if history.is_empty() {
        return "▁".repeat(max_width);
    }
    let data: Vec<f64> = if history.len() > max_width {
        history[history.len() - max_width..].to_vec()
    } else {
        let mut padded = vec![0.0; max_width - history.len()];
        padded.extend_from_slice(history);
        padded
    };
    let max_val = data.iter().cloned().fold(0.0f64, f64::max).max(1.0);
    data.iter()
        .map(|v| {
            let normalized = (v / max_val * 7.0).round() as usize;
            SPARKLINE_CHARS[normalized.min(7)]
        })
        .collect()
}

/// Get a cycling color based on animation frame for visual effects.
fn cycling_color(anim_frame: u32, speed: u32) -> Color {
    let phase = (anim_frame / speed) % 4;
    match phase {
        0 => Color::Cyan,
        1 => Color::Green,
        2 => Color::Blue,
        _ => Color::Magenta,
    }
}

/// Render floating particles as overlays on the cookie display area.
fn render_particles(state: &CookieState, f: &mut Frame, area: Rect) {
    let center_x = area.x + area.width / 2;
    let center_y = area.y + area.height / 2;
    let base_y = area.y + area.height;

    for particle in &state.particles {
        let progress = 1.0 - (particle.life as f32 / particle.max_life as f32);

        let (x, y, color, modifier) = match &particle.style {
            ParticleStyle::Click => {
                let rise = (progress * 5.0) as u16;
                let y = base_y.saturating_sub(1 + rise);
                let x = (center_x as i16 + particle.col_offset).max(area.x as i16) as u16;
                let color = if particle.life > particle.max_life * 2 / 3 {
                    Color::White
                } else if particle.life > particle.max_life / 3 {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                (x, y, color, Modifier::BOLD)
            }
            ParticleStyle::Emoji => {
                let rise = (progress * 5.0) as u16;
                let drift = ((progress * 2.0) as i16).saturating_mul(if particle.col_offset > 0 { 1 } else { -1 });
                let y = base_y.saturating_sub(1 + rise);
                let x = (center_x as i16 + particle.col_offset + drift).max(area.x as i16) as u16;
                // Gold/white palette
                let color = if particle.life > particle.max_life / 2 {
                    Color::Yellow
                } else {
                    Color::White
                };
                (x, y, color, Modifier::empty())
            }
            ParticleStyle::Sparkle => {
                let y = (center_y as i16 + particle.row_offset).max(area.y as i16) as u16;
                let x = (center_x as i16 + particle.col_offset).max(area.x as i16) as u16;
                // Soft twinkle: gold ↔ dim
                let color = if particle.life % 3 == 0 {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                (x, y, color, Modifier::empty())
            }
            ParticleStyle::Celebration => {
                // Gently expand outward from center
                let expand = (progress * 3.0) as i16;
                let dir_x = if particle.col_offset > 0 { expand } else { -expand };
                let dir_y = if particle.row_offset > 0 { expand / 2 } else { -expand / 2 };
                let y = (center_y as i16 + particle.row_offset + dir_y).max(area.y as i16) as u16;
                let x = (center_x as i16 + particle.col_offset + dir_x).max(area.x as i16) as u16;
                // Fade from bright gold to dim
                let color = if particle.life > particle.max_life * 2 / 3 {
                    Color::White
                } else if particle.life > particle.max_life / 3 {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                (x, y, color, Modifier::BOLD)
            }
            ParticleStyle::Combo => {
                let y = (center_y as i16 + particle.row_offset).max(area.y as i16) as u16;
                let x = (center_x as i16 + particle.col_offset
                    - (particle.text.len() as i16 / 2))
                    .max(area.x as i16) as u16;
                // Steady gold — no rainbow cycling
                let color = Color::Yellow;
                (x, y, color, Modifier::BOLD)
            }
        };

        if y >= area.y && y < area.y + area.height && x < area.x + area.width {
            let style = Style::default().fg(color).add_modifier(modifier);
            let text_len = particle.text.chars().count() as u16;
            let available = area.x + area.width - x;
            let display_width = text_len.min(available);
            if display_width > 0 {
                let particle_area = Rect::new(x, y, display_width.max(2), 1);
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
                format!("タップで取得 (残り{:.0}秒)", secs_left),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Register golden cookie click target
        let mut cs = click_state.borrow_mut();
        cs.add_click_target(area, CLAIM_GOLDEN);
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
    let is_narrow = area.width < 60;

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

            if is_narrow {
                // Compact format for narrow screens: "◆Name 2x $15 +0.1/s 30s"
                let best_marker = if is_best_roi { "◆" } else { " " };
                let prod_indicator = if p.count > 0 {
                    let idx = (state.anim_frame as usize / 5 + p.kind.key() as usize) % SPINNER.len();
                    format!("{}", SPINNER[idx])
                } else {
                    " ".to_string()
                };
                let spans = vec![
                    Span::styled(best_marker, key_style),
                    Span::styled(
                        format!("{} {:>2}x", p.kind.name(), p.count),
                        text_style,
                    ),
                    Span::styled(prod_indicator, active_style),
                    Span::styled(format!(" ${}", cost_str), text_style),
                    Span::styled(
                        format!(" +{}/s", format_number(next_cps)),
                        roi_style,
                    ),
                    Span::styled(format!(" {}", payback_str), roi_style),
                ];
                ListItem::new(Line::from(spans))
            } else {
                // Full format for wide screens
                let prod_indicator = if p.count > 0 {
                    let idx = (state.anim_frame as usize / 5 + p.kind.key() as usize) % SPINNER.len();
                    format!("{} ", SPINNER[idx])
                } else {
                    "  ".to_string()
                };

                let syn_str = if total_bonus > 0.001 {
                    format!("+{:.0}%", total_bonus * 100.0)
                } else {
                    String::new()
                };
                let syn_style = Style::default().fg(Color::Magenta);

                let rating = match payback {
                    Some(s) if s <= 60.0 => "★★★",
                    Some(s) if s <= 300.0 => "★★☆",
                    Some(s) if s <= 900.0 => "★☆☆",
                    _ => "☆☆☆",
                };
                let rating_display = if !can_afford { "   " } else { rating };
                let best_marker = if is_best_roi { "◆" } else { " " };

                let mut spans = vec![
                    Span::styled(format!("{}{} ", best_marker, rating_display), key_style),
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
            }
        })
        .collect();

    let producer_border_color = if state.purchase_flash > 0 {
        Color::Yellow
    } else {
        Color::Green
    };
    let title = if is_narrow { " 生産者 ◆=最高効率 " } else { " Producers ◆=最高効率 ★=回収速度 " };
    let widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(producer_border_color))
            .title(title),
    );
    f.render_widget(widget, area);

    // クリックターゲットはコンテンツ領域内（ボーダー除く）のみ登録
    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for (i, p) in state.producers.iter().enumerate() {
        let row = area.y + 1 + i as u16;
        if row < content_end {
            cs.add_row_target(area, row, BUY_PRODUCER_BASE + p.kind.index() as u16);
        }
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

    let mut all_items: Vec<ListItem> = Vec::new();

    // === Upgrade items ===
    for (_, upgrade, unlocked) in &available {
        let can_afford = state.cookies >= upgrade.cost && *unlocked;
        let cost_str = format_number(upgrade.cost);

        if *unlocked {
            let text_style = if can_afford {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            all_items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} - {} ({})", upgrade.name, upgrade.description, cost_str),
                    text_style,
                ),
            ])));
        } else {
            let hint = match &upgrade.unlock_condition {
                Some((kind, count)) => {
                    let current = state.producers[kind.index()].count;
                    format!("🔒 {} {}台必要(現在{}台)", kind.name(), count, current)
                }
                None => "🔒".to_string(),
            };

            all_items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} - {} ", upgrade.name, upgrade.description),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(hint, Style::default().fg(Color::Red)),
            ])));
        }
    }

    let widget = if all_items.is_empty() {
        List::new(vec![ListItem::new(Span::styled(
            " (全て購入済み)",
            Style::default().fg(Color::DarkGray),
        ))])
    } else {
        List::new(all_items.clone())
    }
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" Upgrades "),
    );
    f.render_widget(widget, area);

    // Click targets for items within content area only
    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for i in 0..all_items.len() {
        let row = area.y + 1 + i as u16;
        if row < content_end {
            cs.add_row_target(area, row, BUY_UPGRADE_BASE + i as u16);
        }
    }
}

fn render_research(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    use super::state::ResearchPath;

    let path_name = match &state.research_path {
        ResearchPath::None => "未選択",
        ResearchPath::MassProduction => "量産路線",
        ResearchPath::Quality => "品質路線",
    };

    let mut all_items: Vec<ListItem> = Vec::new();
    let mut key_idx: u8 = 0;

    // Header showing current path
    all_items.push(ListItem::new(Line::from(Span::styled(
        format!(" 🔬 研究パス: {}", path_name),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))));

    let max_tier = state.research_max_tier();
    for node in &state.research_nodes {
        // Skip nodes from the wrong path (if path already chosen)
        if state.research_path != ResearchPath::None && node.path != state.research_path {
            continue;
        }
        if node.purchased {
            all_items.push(ListItem::new(Line::from(vec![
                Span::styled("     ", Style::default()),
                Span::styled(
                    format!("✅ {} - {}", node.name, node.description),
                    Style::default().fg(Color::Green),
                ),
            ])));
            continue;
        }

        let can_buy_tier = node.tier <= max_tier + 1;
        let can_afford = state.cookies >= node.cost && can_buy_tier;
        key_idx += 1;

        let path_icon = match &node.path {
            ResearchPath::MassProduction => "⚙",
            ResearchPath::Quality => "💎",
            ResearchPath::None => "",
        };

        if can_buy_tier {
            let text_style = if can_afford {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            all_items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        " {} T{}: {} - {} ({})",
                        path_icon,
                        node.tier,
                        node.name,
                        node.description,
                        format_number(node.cost)
                    ),
                    text_style,
                ),
            ])));
        } else {
            all_items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        " {} T{}: {} 🔒 前段階の研究が必要",
                        path_icon, node.tier, node.name
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));
        }
    }

    let widget = List::new(all_items.clone()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Research "),
    );
    f.render_widget(widget, area);

    // Click targets for purchasable items (skip header), within content area only
    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for i in 0..key_idx as usize {
        // +1 for border, +1 for header line
        let row = area.y + 2 + i as u16;
        if row < content_end {
            cs.add_row_target(area, row, BUY_RESEARCH_BASE + i as u16);
        }
    }
}

fn render_milestones(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    use super::state::MilestoneStatus;

    let claimed = state.achieved_milestone_count();
    let ready = state.ready_milestone_count();
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
            format!("  🐱×{:.2}", state.kitten_multiplier),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Ready count hint
    let mut bulk_claim_line: Option<u16> = None;
    if ready > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" ✨ {}個が解放可能！", ready),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ▶一括解放",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        bulk_claim_line = Some(lines.len() as u16 - 1);
    }

    // Available height for milestone list (area minus border + header lines + effects section)
    let header_used = if ready > 0 { 3 } else { 2 }; // milk bar + hint? + border
    let effects_lines = 4u16; // effects section estimate
    let avail = area.height.saturating_sub(2 + header_used + effects_lines) as usize;

    // === Ready milestones (show all, top priority) ===
    let mut ready_key_idx: u8 = 0;
    let ready_milestones: Vec<&super::state::Milestone> = state.milestones.iter()
        .filter(|m| m.status == MilestoneStatus::Ready)
        .collect();
    for milestone in &ready_milestones {
        ready_key_idx += 1;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" ✨ {}", milestone.name),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" - {}", milestone.description),
                Style::default().fg(Color::Green),
            ),
        ]));
    }

    // === Locked milestones (show next few goals) ===
    let locked_milestones: Vec<&super::state::Milestone> = state.milestones.iter()
        .filter(|m| m.status == MilestoneStatus::Locked)
        .collect();
    let locked_budget = avail.saturating_sub(ready_milestones.len()).saturating_sub(if claimed > 0 { 1 } else { 0 });
    let locked_show = locked_milestones.len().min(locked_budget.max(2));
    for milestone in locked_milestones.iter().take(locked_show) {
        lines.push(Line::from(vec![
            Span::styled(
                "     🔒 ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                milestone.name.to_string(),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!(" - {}", milestone.description),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    let locked_remaining = locked_milestones.len().saturating_sub(locked_show);
    if locked_remaining > 0 {
        lines.push(Line::from(Span::styled(
            format!("     ...他{}個", locked_remaining),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // === Claimed milestones (compact summary) ===
    if claimed > 0 {
        let claimed_names: Vec<&str> = state.milestones.iter()
            .filter(|m| m.status == MilestoneStatus::Claimed)
            .map(|m| m.name.as_str())
            .collect();
        let summary = if claimed_names.len() <= 3 {
            claimed_names.join(", ")
        } else {
            format!("{}, {} ...他{}個",
                claimed_names[claimed_names.len()-2],
                claimed_names[claimed_names.len()-1],
                claimed_names.len() - 2)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" 🏆 解放済({}): ", claimed),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                summary,
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }

    // === Active effects summary ===
    lines.push(Line::from(Span::styled(
        " ─── 発動中の効果 ────────────────",
        Style::default().fg(Color::DarkGray),
    )));

    // Milk + kitten
    if state.milk > 0.0 {
        let kitten_bonus = (state.kitten_multiplier - 1.0) * 100.0;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" 🥛 ミルク {:.0}%", state.milk * 100.0),
                Style::default().fg(Color::White),
            ),
            if kitten_bonus > 0.01 {
                Span::styled(
                    format!("  → 🐱 CPS +{:.1}%", kitten_bonus),
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    "  (子猫UP購入でCPSに反映)",
                    Style::default().fg(Color::DarkGray),
                )
            },
        ]));
    }

    // Synergy multiplier
    if state.synergy_multiplier > 1.0 {
        lines.push(Line::from(Span::styled(
            format!(" 🔗 シナジー倍率: ×{:.0}", state.synergy_multiplier),
            Style::default().fg(Color::Cyan),
        )));
    }

    // Producer multipliers summary
    let multi_parts: Vec<String> = state.producers.iter()
        .filter(|p| p.multiplier > 1.0)
        .map(|p| format!("{}:×{:.0}", p.kind.name(), p.multiplier))
        .collect();
    if !multi_parts.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" ⚡ 生産倍率: {}", multi_parts.join("  ")),
            Style::default().fg(Color::Yellow),
        )));
    }

    // Active buffs
    for buff in &state.active_buffs {
        let (label, color) = match &buff.effect {
            super::state::GoldenEffect::ProductionFrenzy { multiplier } => {
                (format!("🌟 生産フレンジー ×{:.0} (残{}t)", multiplier, buff.ticks_left), Color::Magenta)
            }
            super::state::GoldenEffect::ClickFrenzy { multiplier } => {
                (format!("👆 クリックフレンジー ×{:.0} (残{}t)", multiplier, buff.ticks_left), Color::Cyan)
            }
            super::state::GoldenEffect::InstantBonus { .. } => continue,
        };
        lines.push(Line::from(Span::styled(
            format!(" {}", label),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
    }

    // Discount
    if state.active_discount > 0.0 {
        lines.push(Line::from(Span::styled(
            format!(" 💰 割引ウェーブ: {:.0}%OFF", state.active_discount * 100.0),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
    }

    // Upgrade count summary
    let purchased_count = state.upgrades.iter().filter(|u| u.purchased).count();
    let total_upgrades = state.upgrades.len();
    lines.push(Line::from(Span::styled(
        format!(" 📦 アップグレード: {}/{}", purchased_count, total_upgrades),
        Style::default().fg(Color::DarkGray),
    )));

    let border_color = if ready > 0 {
        Color::Green
    } else if state.milestone_flash > 0 {
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
                    claimed, total
                )),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);

    // Click targets for ready milestones and bulk claim
    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    // Bulk claim button
    if let Some(line_idx) = bulk_claim_line {
        let row = area.y + 1 + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, CLAIM_ALL_MILESTONES);
        }
    }
    // Individual ready milestones (after header lines)
    let first_ready_row = area.y + 1 + header_used;
    for i in 0..ready_key_idx {
        let row = first_ready_row + i as u16;
        if row < content_end {
            cs.add_row_target(area, row, CLAIM_MILESTONE_BASE + i as u16);
        }
    }
}

fn render_prestige(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let pending = state.pending_heavenly_chips();
    let available = state.available_chips();
    let section = state.prestige_section;

    // Split area: sub-section selector (1 row) + header (2 rows) + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // sub-section tabs
            Constraint::Length(2), // header (chips + reset)
            Constraint::Min(3),   // section content
        ])
        .split(area);

    let border_color = if state.prestige_flash > 0 {
        let phase = state.prestige_flash % 4;
        match phase {
            0 => Color::Yellow,
            1 => Color::Magenta,
            2 => Color::Cyan,
            _ => Color::White,
        }
    } else if pending > 0 {
        Color::Yellow
    } else {
        Color::Blue
    };

    // === Sub-section tab selector (1 row, horizontal) ===
    {
        let sec_style = |idx: u8, color: Color| -> Style {
            if idx == section {
                Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            }
        };

        let sec_tabs: [(& str, Style, u16); 4] = [
            ("転生UP", sec_style(0, Color::Yellow), PRESTIGE_SEC_UPGRADES),
            ("ブースト", sec_style(1, Color::Rgb(255, 182, 193)), PRESTIGE_SEC_BOOSTS),
            ("ドラゴン", sec_style(2, Color::Red), PRESTIGE_SEC_DRAGON),
            ("統計", sec_style(3, Color::White), PRESTIGE_SEC_STATS),
        ];

        let mut spans: Vec<Span> = Vec::new();

        for (i, (label, style, _)) in sec_tabs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("|", Style::default().fg(Color::DarkGray)));
            }
            let padded = format!(" {} ", label);
            spans.push(Span::styled(padded, *style));
        }

        let tab_widget = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color))
                .title(format!(
                    " Prestige{} ",
                    if state.prestige_count > 0 {
                        format!(" {}回目", state.prestige_count)
                    } else {
                        String::new()
                    }
                )),
        );
        f.render_widget(tab_widget, chunks[0]);

        // Equal-width segment click targets (robust against CJK width issues)
        let content_x = chunks[0].x + 1; // +1 for left border
        let content_w = chunks[0].width.saturating_sub(2); // -2 for borders
        let num_secs = sec_tabs.len() as u16;
        let mut cs = click_state.borrow_mut();
        for (i, (_, _, action_id)) in sec_tabs.iter().enumerate() {
            let seg_start = content_w * i as u16 / num_secs;
            let seg_end = content_w * (i as u16 + 1) / num_secs;
            let rect = Rect::new(
                content_x + seg_start,
                chunks[0].y,
                seg_end - seg_start,
                chunks[0].height,
            );
            cs.targets.push(crate::input::ClickTarget {
                rect,
                action_id: *action_id,
            });
        }
    }

    // === Header: chips info + prestige reset (2 rows) ===
    {
        let mut header_lines: Vec<Line> = Vec::new();

        // Row 0: chip info
        header_lines.push(Line::from(vec![
            Span::styled(
                format!(" 👼 {} ", available),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("(計{}) ", state.heavenly_chips),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("CPS×{:.2}", state.prestige_multiplier),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Row 1: reset button or hint
        let mut prestige_reset_line: Option<u16> = None;
        if pending > 0 {
            let blink = (state.anim_frame / 3).is_multiple_of(2);
            let style = if blink {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            };
            prestige_reset_line = Some(1);
            header_lines.push(Line::from(Span::styled(
                format!(" 🌟 ▶転生で +{} チップ獲得！", pending),
                style,
            )));
        } else {
            header_lines.push(Line::from(Span::styled(
                " (10億クッキーで転生可能)",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let header_widget = Paragraph::new(header_lines).block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color)),
        );
        f.render_widget(header_widget, chunks[1]);

        // Click target for prestige reset
        if let Some(line_idx) = prestige_reset_line {
            let mut cs = click_state.borrow_mut();
            let row = chunks[1].y + line_idx;
            if row < chunks[1].y + chunks[1].height {
                cs.add_row_target(chunks[1], row, PRESTIGE_RESET);
            }
        }
    }

    // === Section content ===
    let content_area = chunks[2];
    match section {
        0 => render_prestige_upgrades(state, f, content_area, click_state, available, border_color),
        1 => render_prestige_boosts(state, f, content_area, click_state, border_color),
        2 => render_prestige_dragon(state, f, content_area, click_state, border_color),
        3 => render_prestige_stats(state, f, content_area, border_color),
        _ => render_prestige_upgrades(state, f, content_area, click_state, available, border_color),
    }
}

/// Prestige sub-section: upgrade tree
fn render_prestige_upgrades(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    available: u64,
    border_color: Color,
) {
    let mut lines: Vec<Line> = Vec::new();
    let mut prestige_upgrade_lines: Vec<(usize, u16)> = Vec::new();

    use super::state::PrestigePath;
    let paths = [
        (PrestigePath::Root, "🌟 共通", Color::Yellow),
        (PrestigePath::Production, "🏭 生産パス", Color::Green),
        (PrestigePath::Click, "👆 クリックパス", Color::Cyan),
        (PrestigePath::Luck, "🍀 幸運パス", Color::Magenta),
    ];

    for (path, path_name, path_color) in paths.iter() {
        let path_upgrades: Vec<_> = state
            .prestige_upgrades
            .iter()
            .enumerate()
            .filter(|(_, u)| &u.path == path)
            .collect();

        if path_upgrades.is_empty() {
            continue;
        }

        lines.push(Line::from(Span::styled(
            format!(" {} ", path_name),
            Style::default().fg(*path_color).add_modifier(Modifier::BOLD),
        )));

        for (i, upgrade) in path_upgrades {
            let prereq_met = upgrade.requires.is_none()
                || state
                    .prestige_upgrades
                    .iter()
                    .any(|u| Some(u.id) == upgrade.requires && u.purchased);

            let line_idx = lines.len() as u16;
            prestige_upgrade_lines.push((i, line_idx));

            if upgrade.purchased {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  ✅ {} ", upgrade.name),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        format!("- {}", upgrade.description),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            } else if !prereq_met {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  🔒 {} ", upgrade.name),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        "(前提UP必要)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            } else {
                let can_afford = available >= upgrade.cost;
                let text_style = if can_afford {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", upgrade.name), text_style),
                    Span::styled(format!("- {} ", upgrade.description), text_style),
                    Span::styled(
                        format!("({}チップ)", upgrade.cost),
                        if can_afford {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    ),
                ]));
            }
        }
    }

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for (upgrade_idx, line_idx) in &prestige_upgrade_lines {
        let row = area.y + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, BUY_PRESTIGE_UPGRADE_BASE + *upgrade_idx as u16);
        }
    }
}

/// Prestige sub-section: sugar boosts + auto-clicker
fn render_prestige_boosts(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    border_color: Color,
) {
    let mut lines: Vec<Line> = Vec::new();
    let mut sugar_boost_lines: Vec<(u16, u16)> = Vec::new();
    let mut auto_clicker_line: Option<u16> = None;

    // Sugar header
    lines.push(Line::from(vec![
        Span::styled(
            format!(" 🍬 砂糖: {} ", state.sugar),
            Style::default()
                .fg(Color::Rgb(255, 182, 193))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("(累計: {})", state.sugar_all_time),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Active boost status
    if let Some(ref boost) = state.active_sugar_boost {
        let secs_left = boost.ticks_left as f64 / 10.0;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" ⚡ {} 発動中！ ", boost.kind.name()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK),
            ),
            Span::styled(
                format!(
                    "CPS×{:.1} 残り{:.1}秒",
                    boost.kind.multiplier() * state.prestige_sugar_boost_multiplier(),
                    secs_left
                ),
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }

    // Sugar boost options
    use super::state::SugarBoostKind;
    let boosts = [
        (SugarBoostKind::Rush, SUGAR_RUSH),
        (SugarBoostKind::Fever, SUGAR_FEVER),
        (SugarBoostKind::Frenzy, SUGAR_FRENZY),
    ];
    for (kind, action_id) in boosts.iter() {
        let cost = kind.cost();
        let mult = kind.multiplier() * state.prestige_sugar_boost_multiplier();
        let duration = kind.duration_ticks() as f64 / 10.0;
        let required_prestige = kind.required_prestige();

        let is_unlocked = state.prestige_count >= required_prestige;
        let can_afford = state.sugar >= cost && state.active_sugar_boost.is_none();

        let line_idx = lines.len() as u16;
        sugar_boost_lines.push((*action_id, line_idx));

        if !is_unlocked {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" 🔒 {} ", kind.name()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("(転生{}回で解放)", required_prestige),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else {
            let text_style = if can_afford {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", kind.name()), text_style),
                Span::styled(
                    format!("CPS×{:.1} {:.0}秒 ", mult, duration),
                    text_style,
                ),
                Span::styled(
                    format!("({}砂糖)", cost),
                    if can_afford {
                        Style::default().fg(Color::Rgb(255, 182, 193))
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
            ]));
        }
    }

    // Separator
    lines.push(Line::from(Span::styled(
        " ─── 🤖 オートクリッカー ─────────────",
        Style::default()
            .fg(Color::Rgb(100, 149, 237))
            .add_modifier(Modifier::BOLD),
    )));

    if state.is_auto_clicker_unlocked() {
        let rate = state.auto_clicker_rate();
        let status = if state.auto_clicker_enabled { "ON" } else { "OFF" };
        let status_color = if state.auto_clicker_enabled { Color::Green } else { Color::Red };
        auto_clicker_line = Some(lines.len() as u16);
        lines.push(Line::from(vec![
            Span::styled(
                " 🤖 オートクリッカー ",
                Style::default().fg(Color::Rgb(100, 149, 237)).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}] ", status),
                Style::default().fg(status_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({}回/秒)", rate),
                Style::default().fg(Color::White),
            ),
        ]));
        if state.prestige_count >= 10 {
            lines.push(Line::from(Span::styled(
                "   ⚡ 強化済み！ (5回/秒)",
                Style::default().fg(Color::Yellow),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("   転生10回で強化 (5回/秒) [現在: {}回]", state.prestige_count),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(" 🔒 オートクリッカー ", Style::default().fg(Color::DarkGray)),
            Span::styled("(転生1回で解放)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for (action_id, line_idx) in &sugar_boost_lines {
        let row = area.y + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, *action_id);
        }
    }
    if let Some(line_idx) = auto_clicker_line {
        let row = area.y + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, TOGGLE_AUTO_CLICKER);
        }
    }
}

/// Prestige sub-section: dragon
fn render_prestige_dragon(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    border_color: Color,
) {
    let mut lines: Vec<Line> = Vec::new();
    let mut dragon_feed_lines: Vec<(usize, u16)> = Vec::new();
    let mut dragon_aura_line: Option<u16> = None;

    if state.dragon_level >= 7 {
        lines.push(Line::from(Span::styled(
            " 🐉 ドラゴン Lv.MAX！",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    } else {
        let feed_cost = state.dragon_feed_cost();
        let fed = state.dragon_fed_toward_next();
        let bar_w = 15usize;
        let filled = if feed_cost > 0 {
            ((fed as f64 / feed_cost as f64) * bar_w as f64).round() as usize
        } else {
            0
        };
        let bar: String =
            "█".repeat(filled.min(bar_w)) + &"░".repeat(bar_w - filled.min(bar_w));
        lines.push(Line::from(vec![
            Span::styled(
                format!(" 🐉 Lv.{} ", state.dragon_level),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(bar, Style::default().fg(Color::Red)),
            Span::styled(
                format!(" {}/{}", fed, feed_cost),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(Span::styled(
            " エサ用の生産者をタップ:",
            Style::default().fg(Color::DarkGray),
        )));

        for p in &state.producers {
            if p.count > 0 {
                let line_idx = lines.len() as u16;
                dragon_feed_lines.push((p.kind.index(), line_idx));
                lines.push(Line::from(Span::styled(
                    format!("   ▶{} ({}台)", p.kind.name(), p.count),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    // Dragon aura selection
    if state.dragon_level >= 1 {
        lines.push(Line::from(Span::styled(
            " ─── 🔮 オーラ ─────────────",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )));
        dragon_aura_line = Some(lines.len() as u16);
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", state.dragon_aura.name()),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({}) ", state.dragon_aura.description()),
                Style::default().fg(Color::Magenta),
            ),
            Span::styled("▶切替", Style::default().fg(Color::DarkGray)),
        ]));
        // All aura options
        let mut aura_spans: Vec<Span> = vec![Span::styled("   ", Style::default())];
        for aura in super::state::DragonAura::all().iter() {
            let is_active = *aura == state.dragon_aura;
            let marker = if is_active { "●" } else { "○" };
            let color = if is_active { Color::Magenta } else { Color::DarkGray };
            aura_spans.push(Span::styled(
                format!("{}{} ", marker, aura.name()),
                Style::default().fg(color),
            ));
        }
        lines.push(Line::from(aura_spans));
    } else {
        lines.push(Line::from(Span::styled(
            " 🔒 ドラゴンはまだ目覚めていません",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);

    let mut cs = click_state.borrow_mut();
    let content_end = area.y + area.height.saturating_sub(1);
    for (producer_idx, line_idx) in &dragon_feed_lines {
        let row = area.y + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, DRAGON_FEED_BASE + *producer_idx as u16);
        }
    }
    if let Some(line_idx) = dragon_aura_line {
        let row = area.y + line_idx;
        if row < content_end {
            cs.add_row_target(area, row, DRAGON_CYCLE_AURA);
        }
    }
}

/// Prestige sub-section: statistics
fn render_prestige_stats(
    state: &CookieState,
    f: &mut Frame,
    area: Rect,
    border_color: Color,
) {
    let play_seconds = state.total_ticks / 10;
    let hours = play_seconds / 3600;
    let minutes = (play_seconds % 3600) / 60;
    let secs = play_seconds % 60;

    let lines = vec![
        Line::from(Span::styled(
            format!(" ⏱ プレイ時間: {}h {}m {}s", hours, minutes, secs),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" 🍪 全ラン合計: {}", format_number(state.cookies_all_runs + state.cookies_all_time)),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" 📈 最高CPS: {}/s", format_number(state.best_cps)),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" 🏅 単一ラン最高: {}", format_number(state.best_cookies_single_run)),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" 👆 今回のクリック: {}", state.total_clicks),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" 👼 転生回数: {}", state.prestige_count),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            format!(" 🐉 ドラゴンLv: {}", state.dragon_level),
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::styled(
            format!(" 🍬 砂糖(累計): {}", state.sugar_all_time),
            Style::default().fg(Color::Rgb(255, 182, 193)),
        )),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_log(state: &CookieState, f: &mut Frame, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let total = state.log.len();

    // Show newest entries first (reverse order), limited to visible area
    let log_lines: Vec<Line> = state.log.iter()
        .rev()
        .take(visible_height)
        .enumerate()
        .map(|(i, entry)| {
            let is_recent = i < 3;

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

    let _ = total;

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
