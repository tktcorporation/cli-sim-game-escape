//! つぶ牧場 (Tsubu Ranch) — rendering.
//!
//! 読み取り専用。`RanchState` を書き換えない。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{ClickableList, ScrollableTab, TabBar};

use super::actions::{FEED_BASE, SCROLL_DOWN, SCROLL_UP, TAB_BATTLE, TAB_DEX, TAB_HABITAT, TOGGLE_TEAM_BASE, UPGRADE_CAPACITY};
use super::state::{Affinity, RanchState, Species, Tab, SPECIES_COUNT};

pub fn render(state: &RanchState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    render_header(state, f, chunks[0], click_state, borders);

    let mut cs = click_state.borrow_mut();
    match state.tab {
        Tab::Habitat => render_habitat(state, f, chunks[1], &mut cs, borders),
        Tab::Dex => render_dex(state, f, chunks[1], &mut cs, borders),
        Tab::Battle => render_battle(state, f, chunks[1], &mut cs, borders),
    }
}

fn render_header(
    state: &RanchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(14), Constraint::Min(10)])
        .split(area);

    let food_widget = Paragraph::new(Line::from(vec![
        Span::styled("餌 ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", state.food),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Green)),
    );
    f.render_widget(food_widget, chunks[0]);

    let tab_style = |t: Tab| {
        if state.tab == t {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };

    let mut cs = click_state.borrow_mut();
    TabBar::new(" │ ")
        .tab("牧場", tab_style(Tab::Habitat), TAB_HABITAT)
        .tab("図鑑", tab_style(Tab::Dex), TAB_DEX)
        .tab("対戦", tab_style(Tab::Battle), TAB_BATTLE)
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Green)),
        )
        .render(f, chunks[1], &mut cs);
}

fn push_log_section(cl: &mut ClickableList, state: &RanchState) {
    if state.log.is_empty() {
        return;
    }
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(" ── ログ ──", Style::default().fg(Color::DarkGray))));
    for msg in state.log.iter().rev().take(5) {
        cl.push(Line::from(Span::styled(format!(" {msg}"), Style::default().fg(Color::Gray))));
    }
}

fn render_habitat(state: &RanchState, f: &mut Frame, area: Rect, cs: &mut ClickState, borders: Borders) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" 収容数: {}/{}", state.total_population(), state.capacity()),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    for &species in Species::all() {
        let pop = state.population[species.index()].len();
        if pop == 0 {
            continue;
        }
        let mature = state.mature_count(species);
        let avg = state.average_mature_level(species);
        let mut spans = vec![
            Span::styled(format!(" {:<8}", species.name()), Style::default().fg(Color::White)),
            Span::styled(format!("×{pop:<3}"), Style::default().fg(Color::Cyan)),
            Span::styled(format!(" 成熟{mature:<2}"), Style::default().fg(Color::LightGreen)),
            Span::styled(format!(" 平均Lv{avg:.1}"), Style::default().fg(Color::Yellow)),
        ];
        if !species.is_final_tier() {
            spans.push(Span::styled(
                format!(" 進化{}/{}", mature, species.evolution_threshold()),
                Style::default().fg(Color::LightMagenta),
            ));
        }
        cl.push(Line::from(spans));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 餌やり (成長を早める。何を与えるかは進化の方向にも影響する)",
        Style::default().fg(Color::DarkGray),
    )));
    for &affinity in Affinity::all() {
        let id = FEED_BASE + affinity.index() as u16;
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" [{}] ", affinity.index() + 1), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{}属性の餌をやる", affinity.name()), Style::default().fg(Color::White)),
                Span::styled(format!(" (餌-{})", state.feed_cost()), Style::default().fg(Color::DarkGray)),
            ]),
            id,
        );
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" [C] ", Style::default().fg(Color::Yellow)),
            Span::styled("収容数を拡張する", Style::default().fg(Color::White)),
            Span::styled(
                format!(" (餌-{})", state.capacity_upgrade_cost()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        UPGRADE_CAPACITY,
    );

    push_log_section(&mut cl, state);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 牧場 ");
    ScrollableTab::new(cl, &state.tab_scroll, SCROLL_UP, SCROLL_DOWN)
        .block(block)
        .arrow_color(Color::LightGreen)
        .render(f, area, cs);
}

fn tier_marker(tier: u8) -> &'static str {
    match tier {
        0 => "☆",
        1 => "★",
        _ => "★★",
    }
}

fn render_dex(state: &RanchState, f: &mut Frame, area: Rect, cs: &mut ClickState, borders: Borders) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    let discovered_count = state.discovered.iter().filter(|&&d| d).count();
    cl.push(Line::from(Span::styled(
        format!(" 発見数: {discovered_count}/{SPECIES_COUNT}"),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    for &species in Species::all() {
        let idx = species.index();
        if !state.discovered[idx] {
            cl.push(Line::from(Span::styled(
                format!(" {} ？？？", tier_marker(species.tier())),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }
        let pop = state.population[idx].len();
        let (owned_marker, owned_color) = if pop > 0 {
            ("●", Color::LightGreen)
        } else {
            ("○", Color::DarkGray)
        };
        cl.push(Line::from(vec![
            Span::styled(format!(" {} ", tier_marker(species.tier())), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{owned_marker} "), Style::default().fg(owned_color)),
            Span::styled(species.name(), Style::default().fg(Color::White)),
            Span::styled(format!("  所持{pop}"), Style::default().fg(Color::Cyan)),
        ]));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 図鑑 ");
    ScrollableTab::new(cl, &state.tab_scroll, SCROLL_UP, SCROLL_DOWN)
        .block(block)
        .arrow_color(Color::LightGreen)
        .render(f, area, cs);
}

fn render_battle(state: &RanchState, f: &mut Frame, area: Rect, cs: &mut ClickState, borders: Borders) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" ステージ {} (クリア{}回)", state.stage, state.stage_clears),
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(vec![
        Span::styled(" 敵: ", Style::default().fg(Color::DarkGray)),
        Span::styled(state.enemy_species.name(), Style::default().fg(Color::LightRed)),
        Span::styled(
            format!("  HP {}/{}", state.enemy_hp, state.enemy_max_hp),
            Style::default().fg(Color::White),
        ),
    ]));
    cl.push(Line::from(vec![
        Span::styled(" 味方チーム HP ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}/{}", state.team_hp(), state.team_max_hp()),
            Style::default().fg(Color::LightGreen),
        ),
    ]));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 編成 (タップで追加/解除、最大3体)",
        Style::default().fg(Color::DarkGray),
    )));

    for &species in Species::all() {
        let pop = state.population[species.index()].len();
        let in_team = state.team.contains(&Some(species));
        // 進化で個体数が0になった種でも、編成中なら表示し続ける。
        // ここで単純に pop==0 を弾くと、絶滅した種を編成解除する手段が
        // 二度と描画されずチーム枠が永久にロックされてしまう。
        if pop == 0 && !in_team {
            continue;
        }
        let marker = if in_team { "☑" } else { "☐" };
        let style = if in_team {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let stats = if pop == 0 {
            Span::styled(" (絶滅 — 編成解除できます)", Style::default().fg(Color::Red))
        } else {
            let strongest_lv = state.strongest(species).map(|c| c.level).unwrap_or(0);
            Span::styled(
                format!(
                    "Lv{strongest_lv} (ATK{} HP{})",
                    species.atk_at_level(strongest_lv),
                    species.hp_at_level(strongest_lv)
                ),
                Style::default().fg(Color::DarkGray),
            )
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" {marker} "), style),
                Span::styled(format!("{} ", species.name()), style),
                stats,
            ]),
            TOGGLE_TEAM_BASE + species.index() as u16,
        );
    }

    push_log_section(&mut cl, state);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red))
        .title(" 対戦 ");
    ScrollableTab::new(cl, &state.tab_scroll, SCROLL_UP, SCROLL_DOWN)
        .block(block)
        .arrow_color(Color::LightRed)
        .render(f, area, cs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    fn find(cs: &ClickState, width: u16, height: u16, target: u16) -> bool {
        for y in 0..height {
            for x in 0..width {
                if cs.hit_test(x, y) == Some(target) {
                    return true;
                }
            }
        }
        false
    }

    /// 3タブすべてが、狭い/広いレイアウトの両方で panic せずに描画できること。
    #[test]
    fn renders_all_tabs_without_panicking_narrow_and_wide() {
        for &tab in &[Tab::Habitat, Tab::Dex, Tab::Battle] {
            for &(w, h) in &[(40u16, 24u16), (100u16, 30u16)] {
                let mut state = RanchState::new();
                state.tab = tab;
                let cs = Rc::new(RefCell::new(ClickState::new()));
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            }
        }
    }

    #[test]
    fn tab_bar_registers_all_three_tabs() {
        let state = RanchState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        assert!(find(&cs, 80, 30, TAB_HABITAT));
        assert!(find(&cs, 80, 30, TAB_DEX));
        assert!(find(&cs, 80, 30, TAB_BATTLE));
    }

    #[test]
    fn habitat_tab_registers_feed_and_capacity_targets() {
        let state = RanchState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        for &affinity in Affinity::all() {
            assert!(find(&cs, 80, 30, FEED_BASE + affinity.index() as u16));
        }
        assert!(find(&cs, 80, 30, UPGRADE_CAPACITY));
    }

    #[test]
    fn battle_tab_registers_toggle_target_for_owned_species() {
        let mut state = RanchState::new();
        state.tab = Tab::Battle;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        assert!(find(&cs, 80, 30, TOGGLE_TEAM_BASE + Species::Tsubu.index() as u16));
    }

    /// 進化で個体数が0になった種でも、編成中なら解除ボタンが描画され続けること。
    /// (個体数0を理由に一覧から消すと、チーム枠が永久にロックされてしまう回帰防止)
    #[test]
    fn battle_tab_keeps_toggle_target_for_extinct_team_member() {
        let mut state = RanchState::new();
        state.tab = Tab::Battle;
        state.team[0] = Some(Species::FireKirin);
        assert!(state.population[Species::FireKirin.index()].is_empty());
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        assert!(find(&cs, 80, 30, TOGGLE_TEAM_BASE + Species::FireKirin.index() as u16));
    }

    /// 未発見の種は「？？？」で伏せられ、名前が漏れないこと。
    #[test]
    fn dex_tab_hides_undiscovered_species_name() {
        let mut state = RanchState::new();
        state.tab = Tab::Dex;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| {
                render(&state, f, f.area(), &cs);
                let buf = f.buffer_mut();
                // 全角文字は2セル分を占有し、継続セルの symbol は空白になる
                // (ratatui の TestBackend の仕様)。そのため連続した「？？？」を
                // そのまま部分文字列として探すと継続セルの空白で分断されて
                // 見つからない。1文字でも「？」が出ていれば伏せ表示は機能している。
                let text: String = buf.content().iter().map(|c| c.symbol()).collect();
                assert!(!text.contains(Species::FireKirin.name()), "未発見の種名が漏れている");
                assert!(text.contains('？'), "未発見の種は？で伏せられるはず");
            })
            .unwrap();
    }
}
