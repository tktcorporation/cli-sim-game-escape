//! 周回討伐 描画。読み取り専用 (state を変更しない)。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{ClickableGrid, ClickableList, ScrollableTab};

use super::actions::*;
use super::logic;
use super::state::{
    CampUpgrades, LoopMarchState, Terrain, Phase, REFILL_STONE_COST, REFILL_WOOD_COST, RING_H,
    RING_W,
};

pub fn render(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.phase {
        Phase::Camp => render_camp(state, f, area, click_state),
        Phase::Expedition => render_expedition(state, f, area, click_state),
    }
}

// ── 拠点画面 ──

fn render_camp(
    state: &LoopMarchState,
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
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(6)])
        .split(area);

    let title = Paragraph::new(Line::from(Span::styled(
        "周回討伐 - 拠点",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    render_camp_body(state, f, chunks[1], click_state, borders);
    render_log(state, f, chunks[2], borders);
}

fn push_upgrade_row(
    cl: &mut ClickableList,
    name: &str,
    level: u32,
    cost: u32,
    soul: u32,
    action_id: u16,
    detail: String,
) {
    let affordable = soul >= cost;
    let name_color = if affordable { Color::White } else { Color::DarkGray };
    let cost_color = if affordable { Color::LightMagenta } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!("{name} Lv.{level} "),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("(魂{cost}) "), Style::default().fg(cost_color)),
            Span::styled(detail, Style::default().fg(Color::DarkGray)),
        ]),
        action_id,
    );
}

fn render_camp_body(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(
            " 魂: {}   自己ベスト: 第{}周",
            state.soul, state.best_lap
        ),
        Style::default()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    push_upgrade_row(
        &mut cl,
        " 最大HP強化",
        state.camp.max_hp_level,
        state.camp.max_hp_cost(),
        state.soul,
        CAMP_UPGRADE_MAX_HP,
        format!("現在{}", state.camp.hero_max_hp()),
    );
    push_upgrade_row(
        &mut cl,
        " 攻撃力強化",
        state.camp.attack_level,
        state.camp.attack_cost(),
        state.soul,
        CAMP_UPGRADE_ATTACK,
        format!("現在{}", state.camp.hero_attack()),
    );

    if state.camp.extra_card_level >= 1 {
        cl.push(Line::from(Span::styled(
            " 初期手札+1 — 習得済み",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        push_upgrade_row(
            &mut cl,
            " 初期手札+1",
            0,
            CampUpgrades::EXTRA_CARD_COST,
            state.soul,
            CAMP_UPGRADE_EXTRA_CARD,
            "次回遠征から".to_string(),
        );
    }

    cl.push(Line::from(""));

    let label = if state.run_active {
        format!(" ▶ 遠征に戻る (第{}周)", state.lap + 1)
    } else {
        " ▶ 遠征に出発する".to_string()
    };
    cl.push_clickable(
        Line::from(Span::styled(
            label,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        CAMP_START_OR_RESUME,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 拠点強化 ");
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(cl, &state.camp_scroll, CAMP_SCROLL_UP, CAMP_SCROLL_DOWN)
        .block(block)
        .wrap(true)
        .arrow_color(Color::Green)
        .render(f, area, &mut cs);
}

fn render_log(state: &LoopMarchState, f: &mut Frame, area: Rect, borders: Borders) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let log_lines: Vec<Line> = state
        .log
        .iter()
        .rev()
        .take(visible_height)
        .enumerate()
        .map(|(i, entry)| {
            let color = if i == 0 { Color::White } else { Color::DarkGray };
            Line::from(Span::styled(format!(" {entry}"), Style::default().fg(color)))
        })
        .collect();

    let widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Blue))
                .title(" ログ "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

// ── 遠征画面 ──

fn render_expedition(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if is_narrow_layout(area.width) {
        render_expedition_narrow(state, f, area, click_state);
    } else {
        render_expedition_wide(state, f, area, click_state);
    }
}

fn render_expedition_wide(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let h_chunks = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(20)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(RING_H as u16 + 2),
            Constraint::Min(3),
        ])
        .split(h_chunks[0]);

    let right_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Min(10), Constraint::Min(6)])
        .split(h_chunks[1]);

    render_header(state, f, left_chunks[0], false);
    render_ring(state, f, left_chunks[1], click_state);
    render_hint(f, left_chunks[2]);
    render_hand(state, f, right_chunks[0], click_state);
    render_log(state, f, right_chunks[1], Borders::ALL);
}

fn render_expedition_narrow(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(RING_H as u16 + 2),
            Constraint::Min(8),
        ])
        .split(area);

    render_header(state, f, chunks[0], true);
    render_ring(state, f, chunks[1], click_state);
    render_hand(state, f, chunks[2], click_state);
}

fn render_header(state: &LoopMarchState, f: &mut Frame, area: Rect, is_narrow: bool) {
    let defense = logic::mountain_synergy_defense(&state.path);
    let hp_color = if state.hero.hp * 3 <= state.hero.max_hp {
        Color::Red
    } else if state.hero.hp * 3 <= state.hero.max_hp * 2 {
        Color::Yellow
    } else {
        Color::Green
    };
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let line1 = Line::from(vec![
        Span::styled(
            format!(" HP {}/{} ", state.hero.hp, state.hero.max_hp),
            Style::default().fg(hp_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("ATK{} DEF{}", state.hero.attack, defense),
            Style::default().fg(Color::Cyan),
        ),
    ]);
    let line2 = Line::from(Span::styled(
        format!(
            " 第{}周(自己ベスト{})  木材{} 石材{} 魂{}",
            state.lap + 1,
            state.best_lap,
            state.wood,
            state.stone,
            state.soul
        ),
        Style::default().fg(Color::White),
    ));

    let widget = Paragraph::new(vec![line1, line2]).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 周回討伐 "),
    );
    f.render_widget(widget, area);
}

fn render_hint(f: &mut Frame, area: Rect) {
    let widget = Paragraph::new(Line::from(Span::styled(
        " カードを選んで道をタップで配置",
        Style::default().fg(Color::DarkGray),
    )))
    .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

/// 道の1マス分の表示テキストとスタイルを決める。
/// 優先順位: 勇者 > モンスター > 地形 > 空き道。
fn cell_visual(state: &LoopMarchState, path_index: usize) -> (String, Style) {
    if state.hero.position == path_index {
        return (
            "@ ".to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }

    let slot = &state.path[path_index];
    if let Some(m) = &slot.monster {
        let ch = match (m.terrain, m.elite) {
            (Terrain::Forest, true) => 'W',
            (Terrain::Forest, false) => 'w',
            (Terrain::Mountain, _) => 'g',
            (Terrain::Graveyard, _) => 's',
            (Terrain::Meadow, _) => '?',
        };
        return (
            format!("{ch} "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        );
    }

    match slot.terrain {
        Some(t) => (format!("{} ", t.symbol()), Style::default().fg(t.color())),
        None => (". ".to_string(), Style::default().fg(Color::DarkGray)),
    }
}

fn render_ring(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let positions = logic::ring_positions();
    let mut lines: Vec<Line> = Vec::with_capacity(RING_H);
    for gy in 0..RING_H {
        let mut spans: Vec<Span> = Vec::with_capacity(RING_W);
        for gx in 0..RING_W {
            let idx = positions.iter().position(|&(x, y)| x == gx && y == gy);
            let (text, style) = match idx {
                Some(i) => cell_visual(state, i),
                None => ("  ".to_string(), Style::default()),
            };
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(format!(" 道 (第{}周) ", state.lap + 1));

    let grid = ClickableGrid::new(RING_W, RING_H, PATH_CLICK_BASE, 2);
    {
        let mut cs = click_state.borrow_mut();
        grid.register_targets(area, &block, &mut cs, 0);
    }

    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn render_hand(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    for (i, card) in state.hand.iter().enumerate() {
        match card {
            Some(t) => {
                let selected = state.selected_hand == Some(i);
                let style = if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(t.color())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.color()).add_modifier(Modifier::BOLD)
                };
                let marker = if selected { "▶" } else { " " };
                cl.push_clickable(
                    Line::from(Span::styled(
                        format!(" {marker}[{}] {}", i + 1, t.name()),
                        style,
                    )),
                    HAND_CLICK_BASE + i as u16,
                );
            }
            None => {
                cl.push(Line::from(Span::styled(
                    format!("  [{}] (空)", i + 1),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    cl.push(Line::from(""));

    let refill_ready = state.wood >= REFILL_WOOD_COST && state.stone >= REFILL_STONE_COST;
    let refill_color = if refill_ready { Color::Cyan } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(Span::styled(
            format!(" 補充 (木材{REFILL_WOOD_COST}/石材{REFILL_STONE_COST})"),
            Style::default().fg(refill_color),
        )),
        REFILL_HAND,
    );
    cl.push_clickable(
        Line::from(Span::styled(" 拠点へ戻る", Style::default().fg(Color::Gray))),
        GO_TO_CAMP,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 手札 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    #[test]
    fn ring_click_targets_match_rendered_cells() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|f| {
                render_ring(&state, f, Rect::new(0, 0, 20, RING_H as u16 + 2), &click_state);
            })
            .unwrap();

        // Borders::ALL → inner は (1,1) から。cell_display_width=2 なので
        // 先頭セル (リング index 0 = gx0,gy0) は (1,1) にある。
        let cs = click_state.borrow();
        assert_eq!(cs.hit_test(1, 1), Some(PATH_CLICK_BASE));
    }

    #[test]
    fn cell_visual_shows_hero_marker_at_hero_position() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        let (text, _) = cell_visual(&state, state.hero.position);
        assert_eq!(text.trim(), "@");
    }

    #[test]
    fn cell_visual_shows_terrain_symbol_when_no_monster() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.path[5].terrain = Some(Terrain::Forest);
        let (text, _) = cell_visual(&state, 5);
        assert_eq!(text.trim(), Terrain::Forest.symbol().to_string());
    }

    #[test]
    fn cell_visual_shows_monster_glyph_over_terrain() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.path[5].terrain = Some(Terrain::Forest);
        state.path[5].monster = Some(super::super::state::Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: false,
        });
        let (text, _) = cell_visual(&state, 5);
        assert_eq!(text.trim(), "w");
    }
}
