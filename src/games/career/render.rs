//! Career Simulator rendering (read-only from state).

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};

use super::actions::*;

use super::logic::{can_apply, format_money, format_money_exact, income_per_tick, next_available_job};
use super::state::{
    invest_info, job_info, CareerState, InvestKind, Screen, ALL_JOBS, SKILL_CAP, TRAININGS,
};

pub fn render(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.screen {
        Screen::Main => render_main(state, f, area, click_state),
        Screen::JobMarket => render_job_market(state, f, area, click_state),
        Screen::Invest => render_invest(state, f, area, click_state),
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
            Constraint::Length(5), // Header
            Constraint::Length(6), // Skills
            Constraint::Length(if is_narrow { 10 } else { 11 }), // Actions
            Constraint::Min(4),   // Log
        ])
        .split(area);

    render_header(state, f, chunks[0], borders, is_narrow);
    render_skills(state, f, chunks[1], borders, is_narrow);
    render_actions(state, f, chunks[2], borders, is_narrow, click_state);
    render_log(state, f, chunks[3], borders);
}

fn render_header(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let info = job_info(state.job);
    let income = income_per_tick(state);

    let title = if is_narrow {
        " Career Sim "
    } else {
        " Career Simulator - キャリアシミュレーター "
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(" 所持金: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("¥{}", format_money_exact(state.money)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  (¥{}/tick)", format_money(income)),
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
                format!("  給料: ¥{}/tick", info.salary as u64),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 日数: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}日目", state.day()),
                Style::default().fg(Color::White),
            ),
            Span::styled("  評判: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_reputation(state.reputation),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ];

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

fn render_actions(
    state: &CareerState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut lines = Vec::new();
    let mut cs = click_state.borrow_mut();

    // Training options [1]-[5]
    for (i, t) in TRAININGS.iter().enumerate() {
        let key = (b'1' + i as u8) as char;
        let affordable = state.money >= t.cost;
        let cost_str = if t.cost > 0.0 {
            format!("¥{}", format_money(t.cost))
        } else {
            "無料".to_string()
        };

        let effect = training_effect_str(t);
        let label = if is_narrow {
            format!(" [{}] {} {}", key, t.name, cost_str)
        } else {
            format!(" [{}] {:　<9} {:　<7} {}", key, t.name, effect, cost_str)
        };

        let color = if affordable { Color::White } else { Color::DarkGray };
        lines.push(Line::from(Span::styled(label, Style::default().fg(color))));
        cs.add_row_target(area, area.y + 1 + i as u16, TRAINING_BASE + i as u16);
    }

    // Spacer + navigation
    lines.push(Line::from(""));

    // [6] Job Market
    let job_row = area.y + 1 + TRAININGS.len() as u16 + 1;
    lines.push(Line::from(vec![
        Span::styled(
            " [6] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("転職する", Style::default().fg(Color::White)),
        next_job_hint(state),
    ]));
    cs.add_row_target(area, job_row, GO_JOB_MARKET);

    // [7] Invest
    let invest_row = area.y + 1 + TRAININGS.len() as u16 + 2;
    lines.push(Line::from(vec![
        Span::styled(
            " [7] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("投資する", Style::default().fg(Color::White)),
    ]));
    cs.add_row_target(area, invest_row, GO_INVEST);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" アクション ");
    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn training_effect_str(t: &super::state::TrainingInfo) -> String {
    let mut parts = Vec::new();
    if t.technical > 0.0 {
        parts.push(format!("技+{}", t.technical as u32));
    }
    if t.social > 0.0 {
        parts.push(format!("営+{}", t.social as u32));
    }
    if t.management > 0.0 {
        parts.push(format!("管+{}", t.management as u32));
    }
    if t.knowledge > 0.0 {
        parts.push(format!("知+{}", t.knowledge as u32));
    }
    if t.reputation > 0.0 {
        parts.push(format!("評+{}", t.reputation as u32));
    }
    parts.join(",")
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

    let mut lines = Vec::new();
    let mut cs = click_state.borrow_mut();

    for (i, &kind) in ALL_JOBS.iter().enumerate() {
        let info = job_info(kind);
        let available = can_apply(state, kind);
        let is_current = kind == state.job;
        let key = if i < 9 {
            (b'1' + i as u8) as char
        } else {
            '0'
        };

        let (fg, marker) = if is_current {
            (Color::Cyan, " * ")
        } else if available {
            (Color::Green, "   ")
        } else {
            (Color::DarkGray, "   ")
        };

        let req_str = requirement_str(&info, state, is_narrow);
        let label = if is_narrow {
            format!(
                " [{}]{}{} ¥{}",
                key,
                marker,
                info.name,
                info.salary as u64
            )
        } else {
            format!(
                " [{}]{}{:　<8} ¥{}/tick  {}",
                key,
                marker,
                info.name,
                info.salary as u64,
                req_str
            )
        };

        lines.push(Line::from(Span::styled(label, Style::default().fg(fg))));
        cs.add_row_target(chunks[0], chunks[0].y + 1 + i as u16, APPLY_JOB_BASE + i as u16);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if is_narrow {
            " 緑=応募可 灰=条件未達 *=現職"
        } else {
            " (緑=応募可能  灰=条件未達  *=現在の職業)"
        },
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 求人情報 ");
    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, chunks[0]);

    // Footer: back button
    let footer_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [-] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("戻る", Style::default().fg(Color::White)),
        ]),
    ];
    cs.add_row_target(chunks[1], chunks[1].y + 2, BACK_FROM_JOBS);
    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let footer_widget = Paragraph::new(footer_lines).block(footer_block);
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
            Constraint::Length(8),  // Current investments
            Constraint::Length(8),  // Investment actions
            Constraint::Min(4),    // Footer + back
        ])
        .split(area);

    // Current investments
    let sav_info = invest_info(InvestKind::Savings);
    let stk_info = invest_info(InvestKind::Stocks);
    let re_info = invest_info(InvestKind::RealEstate);

    let sav_ret = state.savings * sav_info.return_rate;
    let stk_ret = state.stocks * stk_info.return_rate;
    let re_ret = state.real_estate * re_info.return_rate;
    let total_ret = sav_ret + stk_ret + re_ret;

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
                format!("  (+¥{}/tick)", format_money(sav_ret)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 株式:   ¥{}", format_money_exact(state.stocks)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  (+¥{}/tick)", format_money(stk_ret)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 不動産: ¥{}", format_money_exact(state.real_estate)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("  (+¥{}/tick)", format_money(re_ret)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" 投資収入合計: +¥{}/tick", format_money(total_ret)),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let current_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 投資状況 ");
    f.render_widget(Paragraph::new(current_lines).block(current_block), chunks[0]);

    // Investment actions
    let mut action_lines = Vec::new();
    let mut cs = click_state.borrow_mut();

    let investments = [
        ('1', InvestKind::Savings, "低リスク低利回り"),
        ('2', InvestKind::Stocks, "中リスク中利回り"),
        ('3', InvestKind::RealEstate, "高リスク高利回り"),
    ];

    for (key, kind, desc) in &investments {
        let info = invest_info(*kind);
        let affordable = state.money >= info.increment;
        let color = if affordable { Color::White } else { Color::DarkGray };

        let label = if is_narrow {
            format!(
                " [{}] {} +¥{} {}",
                key,
                info.name,
                format_money(info.increment),
                desc
            )
        } else {
            format!(
                " [{}] {:　<5} +¥{}  ({})",
                key,
                info.name,
                format_money(info.increment),
                desc
            )
        };

        action_lines.push(Line::from(Span::styled(label, Style::default().fg(color))));
    }

    // Register click targets for investment actions
    let invest_action_ids = [INVEST_SAVINGS, INVEST_STOCKS, INVEST_REAL_ESTATE];
    for (i, &action_id) in invest_action_ids.iter().enumerate() {
        cs.add_row_target(chunks[1], chunks[1].y + 1 + i as u16, action_id);
    }

    let action_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 投資する ");
    f.render_widget(
        Paragraph::new(action_lines).block(action_block),
        chunks[1],
    );

    // Footer: back button
    let footer_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [-] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("戻る", Style::default().fg(Color::White)),
        ]),
    ];
    cs.add_row_target(chunks[2], chunks[2].y + 2, BACK_FROM_INVEST);
    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(footer_lines).block(footer_block), chunks[2]);
}

// ── Helpers ────────────────────────────────────────────────────────────

fn format_reputation(rep: f64) -> String {
    let stars = (rep / 20.0).floor() as usize;
    let filled = stars.min(5);
    let empty = 5 - filled;
    let bar: String = "★".repeat(filled) + &"☆".repeat(empty);
    format!("{} ({})", bar, rep as u32)
}
