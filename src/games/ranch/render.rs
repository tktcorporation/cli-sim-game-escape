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
use super::state::{Affinity, RanchState, Species, Tab, CLASH_INTERVAL_TICKS, SPECIES_COUNT};

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
        for line in species_card_lines(state, species) {
            cl.push(line);
        }
        for line in pit_lines(state, species) {
            cl.push(line);
        }
        cl.push(Line::from(""));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 餌やりの方針 (選ぶと解除するまで継続する。何を選ぶかは進化の方向にも影響する)",
        Style::default().fg(Color::DarkGray),
    )));
    for &affinity in Affinity::all() {
        let id = FEED_BASE + affinity.index() as u16;
        let active = state.feed_focus == Some(affinity);
        let marker = if active { "☑" } else { "☐" };
        let style = if active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" [{}] {marker} ", affinity.index() + 1), style),
                Span::styled(format!("{}属性を重点的に育てる", affinity.name()), style),
                Span::styled(
                    format!("  蓄積{}", state.affinity_feed[affinity.index()]),
                    Style::default().fg(Color::DarkGray),
                ),
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

/// 種ごとのアクセント色。進化元が近い種は近い色域にまとめてある
/// (水系統は青系、陽系統は赤系、土系統は緑系)。
fn species_color(species: Species) -> Color {
    match species {
        Species::Tsubu => Color::Gray,
        Species::AquaTsubu => Color::LightBlue,
        Species::FlareTsubu => Color::LightRed,
        Species::EarthTsubu => Color::LightYellow,
        Species::MistPrincess => Color::LightCyan,
        Species::FrostHare => Color::Cyan,
        Species::FireKirin => Color::Red,
        Species::ThunderHawk => Color::Yellow,
        Species::ThornBoar => Color::Green,
        Species::SwampTurtle => Color::LightGreen,
        Species::SeaDragon => Color::Blue,
        Species::FlameWolf => Color::Rgb(255, 140, 0),
        Species::RockBear => Color::Rgb(139, 90, 43),
    }
}

/// ドット絵ポートレートのピクセルキーに割り当てる色。'1' は本体色、'2'/'3' は
/// 目や装飾など種ごとの差し色。定義されていないキーは常に透明として扱う。
fn sprite_palette(species: Species) -> Vec<(char, Color)> {
    let body = species_color(species);
    match species {
        Species::MistPrincess => vec![('1', body), ('2', Color::Black), ('3', Color::Yellow)],
        Species::SwampTurtle => vec![('1', body), ('2', Color::DarkGray), ('3', Color::Black)],
        _ => vec![('1', body), ('2', Color::Black)],
    }
}

/// `sprite_rows` (8行×8列のピクセル形状) を、上下2行を1セルの上半ブロック `▀` の
/// fg/bg に詰めて縦解像度を2倍に見せる half-block トリックで4行のポートレートに描画する。
/// 上下とも透明なピクセルは着色せず空白のまま残し、背景をそのまま透過させる。
fn sprite_lines(species: Species) -> Vec<Line<'static>> {
    let rows = species.sprite_rows();
    let palette = sprite_palette(species);
    let color_of = |ch: char| palette.iter().find(|&&(c, _)| c == ch).map(|&(_, color)| color);

    rows.chunks(2)
        .map(|pair| {
            let (top, bottom) = (pair[0], pair[1]);
            let spans: Vec<Span<'static>> = top
                .chars()
                .zip(bottom.chars())
                .map(|(t, b)| {
                    let fg = color_of(t);
                    let bg = color_of(b);
                    if fg.is_none() && bg.is_none() {
                        Span::raw(" ")
                    } else {
                        Span::styled(
                            "▀",
                            Style::default().fg(fg.unwrap_or(Color::Reset)).bg(bg.unwrap_or(Color::Reset)),
                        )
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

/// 種1体分のポートレート+ステータスをまとめたカード (4行)。
/// ポートレート (4行) とステータス欄 (4行) を横に並べるため、各行の spans を連結する。
fn species_card_lines(state: &RanchState, species: Species) -> Vec<Line<'static>> {
    let portrait = sprite_lines(species);
    let pop = state.population[species.index()].len();
    let mature = state.mature_count(species);
    let avg = state.average_mature_level(species);

    let mut info: Vec<Line<'static>> = Vec::with_capacity(4);
    info.push(Line::from(vec![
        Span::styled(species.name(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" ×{pop}"), Style::default().fg(Color::Cyan)),
    ]));
    info.push(Line::from(Span::styled(
        format!("成熟{mature}"),
        Style::default().fg(Color::LightGreen),
    )));
    info.push(Line::from(Span::styled(
        format!("平均Lv{avg:.1}"),
        Style::default().fg(Color::Yellow),
    )));
    if species.is_final_tier() {
        info.push(Line::from(Span::styled("最終形態", Style::default().fg(Color::LightMagenta))));
    } else {
        info.push(Line::from(Span::styled(
            format!("進化{}/{}", mature, species.evolution_threshold()),
            Style::default().fg(Color::LightMagenta),
        )));
    }

    portrait
        .into_iter()
        .zip(info)
        .map(|(sprite_line, info_line)| {
            let mut spans = sprite_line.spans;
            spans.push(Span::raw("  "));
            spans.extend(info_line.spans);
            Line::from(spans)
        })
        .collect()
}

/// ピット (個体の積み上げ表示) の幅・高さ。テトリス/スイカゲームのように
/// 下の行から埋まっていくのを見せるため、1体=1マスの矩形として扱う。
const PIT_COLS: usize = 8;
const PIT_ROWS: usize = 3;
const PIT_CAPACITY: usize = PIT_COLS * PIT_ROWS;

/// 個体を「下から積み上がるピット」として表示する (最大 `PIT_CAPACITY` 体)。
/// 成熟個体は明るい色で、未成熟はグレーで表示し、群れの成熟度を一目で分かるようにする。
/// 表示上限を超えた分は最終行に "+N" でまとめる。
fn pit_lines(state: &RanchState, species: Species) -> Vec<Line<'static>> {
    let creatures = &state.population[species.index()];
    let glyph = species.glyph();
    let color = species_color(species);

    let mut rows: Vec<Vec<Span<'static>>> = (0..PIT_ROWS).map(|_| Vec::with_capacity(PIT_COLS)).collect();
    for i in 0..PIT_CAPACITY {
        // 一番下の行 (画面上は最終行) から埋まっていくように、行番号を反転して積む。
        let row_from_bottom = i / PIT_COLS;
        let row = PIT_ROWS - 1 - row_from_bottom;
        let span = match creatures.get(i) {
            Some(c) => {
                let style = if c.is_mature() {
                    Style::default().fg(color).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Span::styled(glyph, style)
            }
            None => Span::raw(" "),
        };
        rows[row].push(span);
    }

    let mut lines: Vec<Line<'static>> = rows
        .into_iter()
        .map(|spans| {
            let mut full = vec![Span::raw("   ")];
            full.extend(spans);
            Line::from(full)
        })
        .collect();

    if creatures.len() > PIT_CAPACITY {
        lines.push(Line::from(Span::styled(
            format!("   …+{}", creatures.len() - PIT_CAPACITY),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
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
            Span::styled(format!("{} ", species.glyph()), Style::default().fg(species_color(species))),
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

/// クラッシュ演出のレーン幅 (アリーナの横幅、マス数)。
const ARENA_LANE_WIDTH: usize = 14;

/// 現在のクラッシュサイクル内での経過度合い (0.0=直後, 1.0=次の激突の瞬間)。
///
/// `clash_cooldown` は激突のたびに `CLASH_INTERVAL_TICKS` にリセットされ、以後は
/// tick毎に1ずつ減っていく (詳細は `logic::tick_battle` 参照)。この既存カウンタを
/// そのままアニメーションの時計として再利用するため、演出専用の新しいstateは
/// 増やしていない — render.rs は読み取り専用のまま。
fn clash_progress(state: &RanchState) -> f64 {
    if state.clash_cooldown >= CLASH_INTERVAL_TICKS {
        1.0
    } else {
        (CLASH_INTERVAL_TICKS - state.clash_cooldown) as f64 / CLASH_INTERVAL_TICKS as f64
    }
}

/// 編成中のツブと敵を互いに投げつけ合うアリーナの1行。
/// 自チームの代表個体が右へ、敵が左へ同時に飛び、中央付近ですれ違い、
/// 激突の瞬間だけ両端に閃光を出す。チーム未編成なら `None`。
fn arena_line(state: &RanchState) -> Option<Line<'static>> {
    let ally = state.team.iter().flatten().next().copied()?;
    let enemy = state.enemy_species;
    let progress = clash_progress(state);
    let last = (ARENA_LANE_WIDTH - 1) as f64;

    let ally_pos = (progress * last).round() as usize;
    let enemy_pos = (last - progress * last).round() as usize;
    let ally_color = species_color(ally);
    let enemy_color = species_color(enemy);

    let mut cells: Vec<Span<'static>> = (0..ARENA_LANE_WIDTH).map(|_| Span::raw(" ")).collect();
    if progress >= 1.0 {
        cells[0] = Span::styled("✹", Style::default().fg(ally_color).add_modifier(Modifier::BOLD));
        cells[ARENA_LANE_WIDTH - 1] =
            Span::styled("✹", Style::default().fg(enemy_color).add_modifier(Modifier::BOLD));
    } else if ally_pos == enemy_pos {
        cells[ally_pos] = Span::styled("✹", Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    } else {
        cells[ally_pos] =
            Span::styled(ally.glyph(), Style::default().fg(ally_color).add_modifier(Modifier::BOLD));
        cells[enemy_pos] =
            Span::styled(enemy.glyph(), Style::default().fg(enemy_color).add_modifier(Modifier::BOLD));
    }

    let mut spans = vec![Span::raw(" ")];
    spans.extend(cells);
    Some(Line::from(spans))
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
    if let Some(arena) = arena_line(state) {
        cl.push(Line::from(""));
        cl.push(arena);
    }
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
                Span::styled(format!("{} ", species.glyph()), Style::default().fg(species_color(species))),
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
    use super::super::state::Creature;
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

    /// half-block トリックが期待通り効いていること: 8列ピクセル形状の2行が
    /// 1行の `▀` に畳み込まれ、上段が fg・下段が bg に独立して反映される。
    #[test]
    fn sprite_lines_maps_pixel_rows_to_half_block_fg_bg_pairs() {
        // ツブの sprite_rows は8行なので、half-block化すると4行になる。
        let lines = sprite_lines(Species::Tsubu);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].spans.len(), 8);

        // rows[2]="11111111" (上段), rows[3]="11211211" (下段) の組。
        // 下段の目にあたる '2' の列だけ bg が黒になり、他は本体色のままになるはず。
        let eye_row = &lines[1];
        let body = species_color(Species::Tsubu);
        assert_eq!(eye_row.spans[0].style.fg, Some(body));
        assert_eq!(eye_row.spans[0].style.bg, Some(body));
        assert_eq!(eye_row.spans[2].style.fg, Some(body));
        assert_eq!(eye_row.spans[2].style.bg, Some(Color::Black));
    }

    /// 上下とも透明('.')なピクセルは着色されず、素の空白のままであること。
    #[test]
    fn sprite_lines_leaves_fully_transparent_pixels_unstyled() {
        let lines = sprite_lines(Species::Tsubu);
        // rows[0]="..1111.." / rows[1]=".111111." の組: 左端は上下とも '.'。
        let top_row = &lines[0];
        assert_eq!(top_row.spans[0].content.as_ref(), " ");
        assert_eq!(top_row.spans[0].style, Style::default());
    }

    /// テトリス/スイカゲームのように、個体は下の行から積み上がっていくこと。
    /// 上の行はまだ空 (スペースのまま) で、最下行だけが埋まっているはず。
    #[test]
    fn pit_lines_fills_from_the_bottom_row_first() {
        let mut state = RanchState::new();
        state.population[Species::Tsubu.index()] = vec![Creature::new(); 3];
        let lines = pit_lines(&state, Species::Tsubu);
        assert_eq!(lines.len(), PIT_ROWS);

        let glyph = Species::Tsubu.glyph();
        let bottom_text: String = lines[PIT_ROWS - 1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(bottom_text.matches(glyph).count(), 3, "最下行に3体分のグリフがあるはず");

        let top_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(top_text.matches(glyph).count(), 0, "最上行はまだ空のはず");
    }

    /// ピット容量を超えた分は "+N" 表記で最終行にまとめられること。
    #[test]
    fn pit_lines_overflow_appends_a_plus_n_line() {
        let mut state = RanchState::new();
        state.population[Species::Tsubu.index()] = vec![Creature::new(); PIT_CAPACITY + 5];
        let lines = pit_lines(&state, Species::Tsubu);
        assert_eq!(lines.len(), PIT_ROWS + 1, "オーバーフロー行が1行追加されるはず");
        let overflow_text: String = lines.last().unwrap().spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(overflow_text.contains("+5"));
    }

    /// `clash_cooldown` (既存の対戦タイマー) をそのままアニメーション進捗に
    /// 変換できていること。リセット直後 (>=CLASH_INTERVAL_TICKS) は激突の瞬間 (1.0)、
    /// それ以外は経過tick数に応じた0〜1未満の値になる。
    #[test]
    fn clash_progress_maps_cooldown_to_a_fraction_of_the_cycle() {
        let mut state = RanchState::new();
        state.clash_cooldown = CLASH_INTERVAL_TICKS;
        assert_eq!(clash_progress(&state), 1.0);

        state.clash_cooldown = CLASH_INTERVAL_TICKS - 1;
        assert!((clash_progress(&state) - 0.2).abs() < f64::EPSILON);

        state.clash_cooldown = 1;
        assert!((clash_progress(&state) - 0.8).abs() < f64::EPSILON);
    }

    /// チーム未編成ではアリーナ演出そのものを出さないこと (戦闘が起きていないため)。
    #[test]
    fn arena_line_is_none_without_a_team() {
        let state = RanchState::new();
        assert!(arena_line(&state).is_none());
    }

    /// 飛行中 (激突前) は自チームと敵のグリフが別の位置に表示されること。
    #[test]
    fn arena_line_shows_ally_and_enemy_glyphs_mid_flight() {
        let mut state = RanchState::new();
        state.team[0] = Some(Species::Tsubu);
        state.enemy_species = Species::FireKirin; // ally (Tsubu) と区別できる別種にする
        state.clash_cooldown = CLASH_INTERVAL_TICKS - 1;
        let line = arena_line(&state).unwrap();
        let symbols: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(symbols.iter().filter(|&&s| s == Species::Tsubu.glyph()).count(), 1);
        assert_eq!(symbols.iter().filter(|&&s| s == Species::FireKirin.glyph()).count(), 1);
    }

    /// 激突の瞬間 (進捗1.0) はレーン両端に閃光が出ること。
    #[test]
    fn arena_line_flashes_impact_at_the_edges_when_progress_reaches_one() {
        let mut state = RanchState::new();
        state.team[0] = Some(Species::Tsubu);
        state.clash_cooldown = CLASH_INTERVAL_TICKS;
        let line = arena_line(&state).unwrap();
        let symbols: Vec<&str> = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(symbols.iter().filter(|&&s| s == "✹").count(), 2, "両端に閃光が出るはず");
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
