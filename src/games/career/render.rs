//! Career Simulator rendering (read-only from state).

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState, ClickableList};

use super::actions::*;

use super::logic::{
    can_apply, format_money, format_money_exact, freedom_progress, is_game_over, monthly_expenses,
    monthly_passive, monthly_salary, months_remaining, next_available_job,
    training_cost_multiplier,
};
use super::state::{
    event_description, event_name, invest_info, job_info, lifestyle_info, CareerState, InvestKind,
    LifestyleLevel, Screen, ALL_JOBS, ALL_LIFESTYLES, SKILL_CAP, TRAININGS,
};

pub fn render(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.screen {
        Screen::Main => render_main(state, f, area, click_state),
        Screen::Training => render_training(state, f, area, click_state),
        Screen::JobMarket => render_job_market(state, f, area, click_state),
        Screen::Invest => render_invest(state, f, area, click_state),
        Screen::Budget => render_budget(state, f, area, click_state),
        Screen::Lifestyle => render_lifestyle(state, f, area, click_state),
        Screen::Report => render_report(state, f, area, click_state),
    }
}

// ── Main Screen ────────────────────────────────────────────────────────

fn render_main(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // Header (expanded for freedom progress)
            Constraint::Length(6),  // Skills
            Constraint::Length(3),  // Event (always reserved)
            Constraint::Length(if is_narrow { 12 } else { 13 }), // Actions
            Constraint::Min(4),    // Log
        ])
        .split(area);

    render_header(state, f, chunks[0], borders, is_narrow);
    render_skills(state, f, chunks[1], borders, is_narrow);
    render_event(state, f, chunks[2], borders);
    render_actions(state, f, chunks[3], borders, is_narrow, click_state);
    render_log(state, f, chunks[4], borders);
}

fn render_header(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let info = job_info(state.job);
    let m_salary = monthly_salary(state);
    let m_passive = monthly_passive(state);
    let m_expenses = monthly_expenses(state);
    let progress = freedom_progress(state);

    let title = if is_narrow {
        " Career Sim "
    } else {
        " Career Simulator - キャリアシミュレーター "
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" 所持金: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money_exact(state.money)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  月給: ¥{}", format_money(m_salary)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 職業: ", Style::default().fg(Color::Gray)),
            Span::styled(
                info.name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}ヶ月目", state.months_elapsed + 1),
                Style::default().fg(Color::White),
            ),
            {
                let remaining = months_remaining(state);
                let color = if remaining <= 12 {
                    Color::Red
                } else if remaining <= 36 {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                Span::styled(
                    format!("  残{}月", remaining),
                    Style::default().fg(color),
                )
            },
            Span::styled("  評判: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_reputation(state.reputation),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 生活: Lv.{} {}", lifestyle_info(state.lifestyle).level, lifestyle_info(state.lifestyle).name),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("  不労所得: ¥{}/月", format_money(m_passive)),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("  AP: {}/{}", state.ap, state.ap_max),
                Style::default()
                    .fg(if state.ap > 0 { Color::Cyan } else { Color::DarkGray })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    // Economic freedom progress bar
    let bar_width = if is_narrow { 15 } else { 25 };
    let filled = ((progress * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let pct = (progress * 100.0) as u32;

    if state.won {
        lines.push(Line::from(vec![
            Span::styled(
                " ★ 経済的自由達成！ ★",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(" 自由: ", Style::default().fg(Color::Gray)),
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::styled(
                format!(" {}% (¥{}/¥{})", pct, format_money(m_passive), format_money(m_expenses)),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn render_skills(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let bar_width = if is_narrow { 12 } else { 20 };
    let lines = vec![
        skill_line("技術力", state.technical, bar_width, Color::Blue),
        skill_line("営業力", state.social, bar_width, Color::Green),
        skill_line("管理力", state.management, bar_width, Color::Yellow),
        skill_line("知識  ", state.knowledge, bar_width, Color::Magenta),
    ];

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" スキル ");
    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn skill_line(label: &str, value: f64, bar_width: usize, color: Color) -> Line<'static> {
    let filled = ((value / SKILL_CAP) * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar: String = "█".repeat(filled) + &"░".repeat(empty);

    Line::from(vec![
        Span::styled(format!(" {} ", label), Style::default().fg(Color::Gray)),
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(
            format!(" {}", value as u32),
            Style::default().fg(Color::White),
        ),
    ])
}

fn render_event(state: &CareerState, f: &mut Frame, area: Rect, borders: Borders) {
    if let Some(event) = state.current_event {
        let (icon, color) = match event {
            super::state::MonthEvent::TrainingSale
            | super::state::MonthEvent::BullMarket
            | super::state::MonthEvent::SkillBoom
            | super::state::MonthEvent::WindfallBonus
            | super::state::MonthEvent::TaxRefund => ("▲", Color::Green),
            super::state::MonthEvent::Recession
            | super::state::MonthEvent::MarketCrash
            | super::state::MonthEvent::ExpenseSpike => ("▼", Color::Red),
        };

        let lines = vec![Line::from(vec![
            Span::styled(
                format!(" {} {} - ", icon, event_name(event)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                event_description(event),
                Style::default().fg(color),
            ),
        ])];

        let block = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(color))
            .title(" イベント ");
        f.render_widget(Paragraph::new(lines).block(block), area);
    } else {
        let block = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" イベント ");
        f.render_widget(Paragraph::new(vec![Line::from(Span::styled(
            " なし",
            Style::default().fg(Color::DarkGray),
        ))]).block(block), area);
    }
}

fn render_actions(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    let has_ap = state.ap > 0;
    let ap_color = if has_ap { Color::Cyan } else { Color::DarkGray };

    // AP-consuming actions (clickable)
    let training_label = if is_narrow { "研修" } else { "研修する" };
    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(ap_color).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{} [1]", training_label),
            Style::default().fg(if has_ap { Color::White } else { Color::DarkGray }),
        ),
        Span::styled(" (1AP)", Style::default().fg(Color::DarkGray)),
    ]), GO_TRAINING);

    let net_label = if is_narrow { "人脈作り" } else { "ネットワーキング" };
    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(ap_color).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{} [2]", net_label),
            Style::default().fg(if has_ap { Color::White } else { Color::DarkGray }),
        ),
        Span::styled(" (1AP) 営業+2 評判+3", Style::default().fg(Color::DarkGray)),
    ]), DO_NETWORKING);

    let side_label = if is_narrow { "副業" } else { "副業する" };
    let best_skill = state.technical
        .max(state.social)
        .max(state.management)
        .max(state.knowledge);
    let side_available = has_ap && best_skill >= 5.0;
    cl.push_clickable(Line::from(vec![
        Span::styled(
            " ▶ ",
            Style::default()
                .fg(if side_available { Color::Cyan } else { Color::DarkGray })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} [3]", side_label),
            Style::default().fg(if side_available { Color::White } else { Color::DarkGray }),
        ),
        Span::styled(
            if best_skill >= 5.0 {
                format!(" (1AP) ¥{}", format_money(best_skill * 100.0))
            } else {
                " (スキル5以上必要)".to_string()
            },
            Style::default().fg(Color::DarkGray),
        ),
    ]), DO_SIDE_JOB);

    // Spacer
    cl.push(Line::from(""));

    // Navigation (clickable)
    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("転職する [6]", Style::default().fg(Color::White)),
        next_job_hint(state),
    ]), GO_JOB_MARKET);

    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("投資する [7]", Style::default().fg(Color::White)),
    ]), GO_INVEST);

    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("家計簿 [8]", Style::default().fg(Color::White)),
    ]), GO_BUDGET);

    cl.push_clickable(Line::from(vec![
        Span::styled(" ▶ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                "生活水準 [9] (Lv.{} {})",
                lifestyle_info(state.lifestyle).level,
                lifestyle_info(state.lifestyle).name
            ),
            Style::default().fg(Color::White),
        ),
    ]), GO_LIFESTYLE);

    // Spacer + Advance Month
    cl.push(Line::from(""));

    if state.won {
        cl.push(Line::from(Span::styled(
            " ★ 経済的自由を達成しました！",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(""));
    } else if is_game_over(state) {
        cl.push(Line::from(Span::styled(
            " ✖ 120ヶ月経過 - GAME OVER",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(Span::styled(
            "   経済的自由は達成できませんでした…",
            Style::default().fg(Color::Red),
        )));
    } else {
        cl.push_clickable(Line::from(vec![
            Span::styled(
                " ▶▶ 次の月へ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("[0]", Style::default().fg(Color::DarkGray)),
        ]), ADVANCE_MONTH);
        cl.push(Line::from(""));
    }

    // Determine top_offset based on border style
    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };

    let mut cs = click_state.borrow_mut();
    cl.register_targets(area, &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(format!(" アクション (AP: {}/{}) ", state.ap, state.ap_max));
    let widget = Paragraph::new(cl.into_lines()).block(block);
    f.render_widget(widget, area);
}

fn next_job_hint(state: &CareerState) -> Span<'static> {
    if let Some((_kind, name)) = next_available_job(state) {
        Span::styled(
            format!(" (次: {})", name),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::raw("")
    }
}

fn render_log(state: &CareerState, f: &mut Frame, area: Rect, borders: Borders) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(max_lines);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|msg| {
            Line::from(Span::styled(
                format!(" > {}", msg),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ログ ");
    let widget = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

// ── Training Screen ───────────────────────────────────────────────────

fn render_training(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10), // Training list
            Constraint::Length(4), // Footer
        ])
        .split(area);

    let mut cl = ClickableList::new();
    let has_ap = state.ap > 0;
    let cost_mult = training_cost_multiplier(state.current_event);
    let is_sale = cost_mult < 1.0;

    cl.push(Line::from(vec![
        Span::styled(
            format!(" AP: {}/{}", state.ap, state.ap_max),
            Style::default()
                .fg(if has_ap { Color::Cyan } else { Color::DarkGray })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  各研修: 1AP消費", Style::default().fg(Color::DarkGray)),
        if is_sale {
            Span::styled("  ★セール中！50%オフ★", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("")
        },
    ]));
    cl.push(Line::from(""));

    for (i, t) in TRAININGS.iter().enumerate() {
        let cost = t.cost * cost_mult;
        let affordable = state.money >= cost && has_ap;
        let cost_str = if cost > 0.0 {
            if is_sale && t.cost > 0.0 {
                format!("¥{} (¥{})", format_money(cost), format_money(t.cost))
            } else {
                format!("¥{}", format_money(cost))
            }
        } else {
            "無料".to_string()
        };

        let effect = training_effect_str(t, state.lifestyle, state.current_event);
        let label = if is_narrow {
            format!(" [{}] {} {}", i + 1, t.name, cost_str)
        } else {
            format!(
                " [{}] {:　<9} {:　<10} {}",
                i + 1, t.name, effect, cost_str
            )
        };

        let color = if affordable {
            Color::White
        } else {
            Color::DarkGray
        };
        cl.push_clickable(Line::from(Span::styled(label, Style::default().fg(color))), TRAINING_BASE + i as u16);
    }

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl.register_targets(chunks[0], &mut cs, top_offset, bottom_offset, 0);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 研修 ");
    let widget = Paragraph::new(cl.into_lines()).block(block);
    f.render_widget(widget, chunks[0]);

    // Footer: back button
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(Line::from(Span::styled(
        " ◀ 戻る [-]",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )), BACK_FROM_TRAINING);
    cl_footer.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let footer_widget = Paragraph::new(cl_footer.into_lines()).block(footer_block);
    f.render_widget(footer_widget, chunks[1]);
}

fn training_effect_str(
    t: &super::state::TrainingInfo,
    lifestyle: LifestyleLevel,
    event: Option<super::state::MonthEvent>,
) -> String {
    let ls = lifestyle_info(lifestyle);
    let eff = 1.0 + ls.skill_efficiency;
    let s_mult = super::logic::training_cost_multiplier(event);
    // s_mult is cost mult; for skill we use skill_multiplier logic
    let skill_mult = match event {
        Some(super::state::MonthEvent::SkillBoom) => 2.0,
        _ => 1.0,
    };
    let total_mult = eff * skill_mult;
    let mut parts = Vec::new();
    if t.technical > 0.0 {
        parts.push(format!("技+{}", (t.technical * total_mult) as u32));
    }
    if t.social > 0.0 {
        parts.push(format!("営+{}", (t.social * total_mult) as u32));
    }
    if t.management > 0.0 {
        parts.push(format!("管+{}", (t.management * total_mult) as u32));
    }
    if t.knowledge > 0.0 {
        parts.push(format!("知+{}", (t.knowledge * total_mult) as u32));
    }
    if t.reputation > 0.0 {
        parts.push(format!("評+{}", t.reputation as u32));
    }
    let _ = s_mult; // cost multiplier used elsewhere
    parts.join(",")
}

// ── Job Market Screen ──────────────────────────────────────────────────

fn render_job_market(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),  // Job list
            Constraint::Length(4), // Footer
        ])
        .split(area);

    let mut cl = ClickableList::new();

    for (i, &kind) in ALL_JOBS.iter().enumerate() {
        let info = job_info(kind);
        let available = can_apply(state, kind);
        let is_current = kind == state.job;

        let (fg, marker) = if is_current {
            (Color::Cyan, " ●")
        } else if available {
            (Color::Green, " ▶")
        } else {
            (Color::DarkGray, "  ")
        };

        let monthly = info.salary * 300.0;
        let req_str = requirement_str(&info, state, is_narrow);
        let label = if is_narrow {
            format!(" {}{} ¥{}/月", marker, info.name, format_money(monthly))
        } else {
            format!(
                " {}{:　<8} ¥{}/月  {}",
                marker,
                info.name,
                format_money(monthly),
                req_str
            )
        };

        cl.push_clickable(Line::from(Span::styled(label, Style::default().fg(fg))), APPLY_JOB_BASE + i as u16);
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        if is_narrow {
            " ▶=応募可 ●=現職 (1AP)"
        } else {
            " (▶=応募可能  ●=現在の職業  転職: 1AP)"
        },
        Style::default().fg(Color::DarkGray),
    )));

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl.register_targets(chunks[0], &mut cs, top_offset, bottom_offset, 0);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(format!(" 求人情報 (AP: {}/{}) ", state.ap, state.ap_max));
    let widget = Paragraph::new(cl.into_lines()).block(block);
    f.render_widget(widget, chunks[0]);

    // Footer: back button
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(Line::from(Span::styled(
        " ◀ 戻る [-]",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )), BACK_FROM_JOBS);
    cl_footer.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let footer_widget = Paragraph::new(cl_footer.into_lines()).block(footer_block);
    f.render_widget(footer_widget, chunks[1]);
}

fn requirement_str(
    info: &super::state::JobInfo,
    state: &CareerState,
    is_narrow: bool,
) -> String {
    let mut parts = Vec::new();
    if info.req_technical > 0.0 {
        let met = state.technical >= info.req_technical;
        let label = if is_narrow { "技" } else { "技術" };
        parts.push(format_req(label, info.req_technical, met));
    }
    if info.req_social > 0.0 {
        let met = state.social >= info.req_social;
        let label = if is_narrow { "営" } else { "営業" };
        parts.push(format_req(label, info.req_social, met));
    }
    if info.req_management > 0.0 {
        let met = state.management >= info.req_management;
        let label = if is_narrow { "管" } else { "管理" };
        parts.push(format_req(label, info.req_management, met));
    }
    if info.req_knowledge > 0.0 {
        let met = state.knowledge >= info.req_knowledge;
        let label = if is_narrow { "知" } else { "知識" };
        parts.push(format_req(label, info.req_knowledge, met));
    }
    if info.req_reputation > 0.0 {
        let met = state.reputation >= info.req_reputation;
        let label = if is_narrow { "評" } else { "評判" };
        parts.push(format_req(label, info.req_reputation, met));
    }
    if info.req_money > 0.0 {
        let met = state.money >= info.req_money;
        parts.push(format!(
            "¥{}{}",
            format_money(info.req_money),
            if met { "" } else { "!" }
        ));
    }
    if parts.is_empty() {
        "条件なし".to_string()
    } else {
        parts.join(" ")
    }
}

fn format_req(label: &str, required: f64, met: bool) -> String {
    if met {
        format!("{}>={}", label, required as u32)
    } else {
        format!("{}>={}!", label, required as u32)
    }
}

// ── Investment Screen ──────────────────────────────────────────────────

fn render_invest(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Current investments
            Constraint::Length(8), // Investment actions
            Constraint::Min(4),   // Footer + back
        ])
        .split(area);

    // Current investments
    let sav_info = invest_info(InvestKind::Savings);
    let stk_info = invest_info(InvestKind::Stocks);
    let re_info = invest_info(InvestKind::RealEstate);

    let sav_monthly = state.savings * sav_info.return_rate * 300.0;
    let stk_monthly = state.stocks * stk_info.return_rate * 300.0;
    let re_monthly = state.real_estate * re_info.return_rate * 300.0;
    let total_monthly = sav_monthly + stk_monthly + re_monthly;

    let current_lines = vec![
        Line::from(vec![
            Span::styled(" 所持金: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money_exact(state.money)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!(" 貯金:   ¥{}", format_money_exact(state.savings)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  (+¥{}/月)", format_money(sav_monthly)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 株式:   ¥{}", format_money_exact(state.stocks)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  (+¥{}/月)", format_money(stk_monthly)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 不動産: ¥{}", format_money_exact(state.real_estate)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  (+¥{}/月)", format_money(re_monthly)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![Span::styled(
            format!(" 投資収入合計: +¥{}/月", format_money(total_monthly)),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]),
    ];

    let current_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 投資状況 ");
    f.render_widget(
        Paragraph::new(current_lines).block(current_block),
        chunks[0],
    );

    // Investment actions
    let mut cl = ClickableList::new();

    let investments = [
        (InvestKind::Savings, "低リスク 月利0.05%"),
        (InvestKind::Stocks, "中リスク 月利0.5%"),
        (InvestKind::RealEstate, "高リスク 月利1.5%"),
    ];

    let invest_action_ids = [INVEST_SAVINGS, INVEST_STOCKS, INVEST_REAL_ESTATE];
    for ((kind, desc), &action_id) in investments.iter().zip(invest_action_ids.iter()) {
        let info = invest_info(*kind);
        let affordable = state.money >= info.increment;
        let color = if affordable {
            Color::White
        } else {
            Color::DarkGray
        };

        let label = if is_narrow {
            format!(" ▶{} +¥{} {}", info.name, format_money(info.increment), desc)
        } else {
            format!(
                " ▶{:　<5} +¥{}  ({})",
                info.name,
                format_money(info.increment),
                desc
            )
        };

        cl.push_clickable(Line::from(Span::styled(label, Style::default().fg(color))), action_id);
    }

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);

    let action_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 投資する (APなし) ");
    f.render_widget(
        Paragraph::new(cl.into_lines()).block(action_block),
        chunks[1],
    );

    // Footer: back button
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(Line::from(Span::styled(
        " ◀ 戻る [-]",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )), BACK_FROM_INVEST);
    cl_footer.register_targets(chunks[2], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[2],
    );
}

// ── Budget Screen (Income Statement + Balance Sheet) ───────────────────

fn render_budget(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(16),  // Content
            Constraint::Length(6), // Freedom progress + back
        ])
        .split(area);

    let r = &state.last_report;
    let ls = lifestyle_info(state.lifestyle);
    let m_passive = monthly_passive(state);
    let m_expenses = monthly_expenses(state);

    if is_narrow {
        render_budget_narrow(state, f, chunks[0], borders, r, ls.name);
    } else {
        render_budget_wide(state, f, chunks[0], borders, r, ls.name);
    }

    // Freedom progress + back button
    let progress = freedom_progress(state);
    let bar_width: usize = if is_narrow { 15 } else { 25 };
    let filled = ((progress * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let pct = (progress * 100.0) as u32;

    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));

    if state.won {
        cl_footer.push(Line::from(Span::styled(
            " ★★★ 経済的自由達成！ ★★★",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }

    cl_footer.push(Line::from(vec![
        Span::styled(" 不労所得 ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("¥{}", format_money(m_passive)),
            Style::default().fg(Color::Green),
        ),
        Span::styled(" / 支出 ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("¥{}", format_money(m_expenses)),
            Style::default().fg(Color::Red),
        ),
    ]));
    cl_footer.push(Line::from(vec![
        Span::styled(" 経済的自由: ", Style::default().fg(Color::Gray)),
        Span::styled(bar, Style::default().fg(Color::Green)),
        Span::styled(format!(" {}%", pct), Style::default().fg(Color::White)),
    ]));
    cl_footer.push_clickable(Line::from(Span::styled(
        " ◀ 戻る [-]",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )), BACK_FROM_BUDGET);

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl_footer.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

fn render_budget_narrow(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    r: &super::state::MonthlyReport,
    lifestyle_name: &str,
) {
    let total_assets = state.savings + state.stocks + state.real_estate;
    let lines = vec![
        Line::from(Span::styled(
            " 【収入】",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  給与       ¥{}", format_money(r.gross_salary))),
        Line::from(Span::styled(
            " 【天引き】",
            Style::default().fg(Color::Red),
        )),
        Line::from(format!("  所得税等  ▲¥{}", format_money(r.tax))),
        Line::from(format!("  社会保険  ▲¥{}", format_money(r.insurance))),
        Line::from(format!("  手取り     ¥{}", format_money(r.net_salary))),
        Line::from(Span::styled(
            " 【不労所得】",
            Style::default().fg(Color::Green),
        )),
        Line::from(format!("  投資収入   ¥{}", format_money(r.passive_income))),
        Line::from(Span::styled(
            format!(" 【支出】 ({})", lifestyle_name),
            Style::default().fg(Color::Red),
        )),
        Line::from(format!("  生活費    ▲¥{}", format_money(r.living_cost))),
        Line::from(format!("  家賃      ▲¥{}", format_money(r.rent))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" 月間CF ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money(r.cashflow)),
                Style::default()
                    .fg(if r.cashflow >= 0.0 { Color::Green } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!(" 総資産  ¥{}", format_money(total_assets + state.money))),
    ];

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 家計簿 ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_budget_wide(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    r: &super::state::MonthlyReport,
    lifestyle_name: &str,
) {
    let sav_info = invest_info(InvestKind::Savings);
    let stk_info = invest_info(InvestKind::Stocks);
    let re_info = invest_info(InvestKind::RealEstate);
    let sav_monthly = state.savings * sav_info.return_rate * 300.0;
    let stk_monthly = state.stocks * stk_info.return_rate * 300.0;
    let re_monthly = state.real_estate * re_info.return_rate * 300.0;
    let total_assets = state.savings + state.stocks + state.real_estate + state.money;

    // Two-column layout: Income Statement | Balance Sheet
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: Income Statement
    let is_lines = vec![
        Line::from(Span::styled(
            " 【収入】",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  給与        ¥{}", format_money(r.gross_salary))),
        Line::from(Span::styled(
            " 【天引き】",
            Style::default().fg(Color::Red),
        )),
        Line::from(format!("  所得税等   ▲¥{}", format_money(r.tax))),
        Line::from(format!("  社会保険   ▲¥{}", format_money(r.insurance))),
        Line::from(format!("  手取り      ¥{}", format_money(r.net_salary))),
        Line::from(""),
        Line::from(Span::styled(
            " 【不労所得】",
            Style::default().fg(Color::Green),
        )),
        Line::from(format!("  投資収入    ¥{}", format_money(r.passive_income))),
        Line::from(""),
        Line::from(Span::styled(
            format!(" 【支出】 ({})", lifestyle_name),
            Style::default().fg(Color::Red),
        )),
        Line::from(format!("  生活費     ▲¥{}", format_money(r.living_cost))),
        Line::from(format!("  家賃       ▲¥{}", format_money(r.rent))),
        Line::from(vec![
            Span::styled(" 月間CF ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money(r.cashflow)),
                Style::default()
                    .fg(if r.cashflow >= 0.0 {
                        Color::Green
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let is_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 月次収支 ");
    f.render_widget(Paragraph::new(is_lines).block(is_block), cols[0]);

    // Right: Balance Sheet
    let bs_lines = vec![
        Line::from(Span::styled(
            " 【資産】",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "  現金    ¥{}",
            format_money_exact(state.money)
        )),
        Line::from(format!(
            "  貯金    ¥{}  +¥{}/月",
            format_money_exact(state.savings),
            format_money(sav_monthly)
        )),
        Line::from(format!(
            "  株式    ¥{}  +¥{}/月",
            format_money_exact(state.stocks),
            format_money(stk_monthly)
        )),
        Line::from(format!(
            "  不動産  ¥{}  +¥{}/月",
            format_money_exact(state.real_estate),
            format_money(re_monthly)
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" 資産合計 ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money(total_assets)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 【不労所得】",
            Style::default().fg(Color::Green),
        )),
        Line::from(vec![
            Span::styled(
                format!(
                    "  合計 ¥{}/月 (税引後)",
                    format_money(r.passive_income)
                ),
                Style::default().fg(Color::Green),
            ),
        ]),
    ];

    let bs_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" バランスシート ");
    f.render_widget(Paragraph::new(bs_lines).block(bs_block), cols[1]);
}

// ── Report Screen (after advancing a month) ────────────────────────────

fn render_report(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(16), // Report content
            Constraint::Length(4), // Footer
        ])
        .split(area);

    let r = &state.last_report;
    let info = job_info(state.job);
    let m_passive = monthly_passive(state);
    let m_expenses = monthly_expenses(state);

    let mut lines = Vec::new();

    // Month header
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {}ヶ月目 完了", state.months_elapsed),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  (残{}月)", months_remaining(state)),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    // Income
    lines.push(Line::from(Span::styled(
        " 【収入】",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "  給与 ({})    ¥{}",
        info.name,
        format_money(r.gross_salary)
    )));
    lines.push(Line::from(format!(
        "  所得税      ▲¥{}",
        format_money(r.tax)
    )));
    lines.push(Line::from(format!(
        "  社会保険    ▲¥{}",
        format_money(r.insurance)
    )));
    lines.push(Line::from(vec![
        Span::styled("  手取り       ", Style::default().fg(Color::White)),
        Span::styled(
            format!("¥{}", format_money(r.net_salary)),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    // Passive income
    if r.passive_income > 0.0 {
        lines.push(Line::from(Span::styled(
            " 【不労所得】",
            Style::default().fg(Color::Green),
        )));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  投資収入     ¥{}", format_money(r.passive_income)),
                Style::default().fg(Color::Green),
            ),
        ]));
        lines.push(Line::from(""));
    }

    // Expenses
    lines.push(Line::from(Span::styled(
        format!(" 【支出】 ({})", lifestyle_info(state.lifestyle).name),
        Style::default().fg(Color::Red),
    )));
    lines.push(Line::from(format!(
        "  生活費      ▲¥{}",
        format_money(r.living_cost)
    )));
    lines.push(Line::from(format!(
        "  家賃        ▲¥{}",
        format_money(r.rent)
    )));
    lines.push(Line::from(""));

    // Cash flow
    let cf_color = if r.cashflow >= 0.0 {
        Color::Green
    } else {
        Color::Red
    };
    lines.push(Line::from(vec![
        Span::styled(" 月間キャッシュフロー ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("¥{}", format_money(r.cashflow)),
            Style::default().fg(cf_color).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Current balance
    lines.push(Line::from(vec![
        Span::styled(" 所持金 ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("¥{}", format_money_exact(state.money)),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Freedom progress
    let progress = freedom_progress(state);
    let bar_width = if is_narrow { 15 } else { 25 };
    let filled = ((progress * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let pct = (progress * 100.0) as u32;

    lines.push(Line::from(vec![
        Span::styled(" 自由: ", Style::default().fg(Color::Gray)),
        Span::styled(bar, Style::default().fg(Color::Green)),
        Span::styled(
            format!(" {}% (¥{}/¥{})", pct, format_money(m_passive), format_money(m_expenses)),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Win/game over message
    if state.won {
        lines.push(Line::from(Span::styled(
            " ★ 経済的自由を達成しました！ ★",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    } else if is_game_over(state) {
        lines.push(Line::from(Span::styled(
            " ✖ 120ヶ月経過 - GAME OVER",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Event info
    if let Some(event) = state.current_event {
        let (icon, color) = match event {
            super::state::MonthEvent::TrainingSale
            | super::state::MonthEvent::BullMarket
            | super::state::MonthEvent::SkillBoom
            | super::state::MonthEvent::WindfallBonus
            | super::state::MonthEvent::TaxRefund => ("▲", Color::Green),
            super::state::MonthEvent::Recession
            | super::state::MonthEvent::MarketCrash
            | super::state::MonthEvent::ExpenseSpike => ("▼", Color::Red),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} 来月イベント: {} ", icon, event_name(event)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 月次レポート ");
    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Footer: continue button
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(Line::from(vec![
        Span::styled(
            " ▶ 続ける ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("[0]", Style::default().fg(Color::DarkGray)),
    ]), BACK_FROM_REPORT);

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl_footer.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Lifestyle Screen ───────────────────────────────────────────────────

fn render_lifestyle(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14), // Lifestyle list
            Constraint::Length(4), // Footer
        ])
        .split(area);

    let mut cl = ClickableList::new();

    cl.push(Line::from(vec![
        Span::styled(
            format!(
                " 現在: Lv.{} {}",
                lifestyle_info(state.lifestyle).level,
                lifestyle_info(state.lifestyle).name
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    cl.push(Line::from(""));

    for (i, &level) in ALL_LIFESTYLES.iter().enumerate() {
        let info = lifestyle_info(level);
        let is_current = level == state.lifestyle;
        let total = info.living_cost + info.rent;

        let fg = if is_current {
            Color::Cyan
        } else {
            Color::White
        };
        let marker = if is_current { "●" } else { "▶" };

        let eff_str = if info.skill_efficiency > 0.0 {
            format!(" 効率+{}%", (info.skill_efficiency * 100.0) as u32)
        } else {
            String::new()
        };

        let rep_str = if info.rep_bonus > 0.0 {
            format!(" 評判+{:.3}/t", info.rep_bonus)
        } else {
            String::new()
        };

        let label = if is_narrow {
            format!(
                " {} Lv.{} {} ¥{}/月{}",
                marker, info.level, info.name, format_money(total), eff_str
            )
        } else {
            format!(
                " {} Lv.{} {:　<3} 生活費¥{} 家賃¥{} (計¥{}/月){}{}",
                marker,
                info.level,
                info.name,
                format_money(info.living_cost),
                format_money(info.rent),
                format_money(total),
                eff_str,
                rep_str,
            )
        };

        cl.push_clickable(Line::from(Span::styled(label, Style::default().fg(fg))), LIFESTYLE_BASE + i as u16);
    }

    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    let mut cs = click_state.borrow_mut();
    cl.register_targets(chunks[0], &mut cs, top_offset, bottom_offset, 0);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" 生活水準 ");
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), chunks[0]);

    // Footer
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(Line::from(Span::styled(
        " ◀ 戻る [-]",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )), BACK_FROM_LIFESTYLE);
    cl_footer.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    drop(cs);

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Helpers ────────────────────────────────────────────────────────────

fn format_reputation(rep: f64) -> String {
    let stars = (rep / 20.0).floor() as usize;
    let filled = stars.min(5);
    let empty = 5 - filled;
    let bar: String = "★".repeat(filled) + &"☆".repeat(empty);
    format!("{} ({})", bar, rep as u32)
}
