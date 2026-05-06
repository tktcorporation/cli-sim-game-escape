//! Idle Metropolis rendering.
//!
//! 視覚デザインは絵文字非依存。block elements / box drawing で構成し、
//! rebels-in-the-sky のように TUI らしい密度のある画面を狙う。
//!
//! Layout intent (wide ≥60 cols):
//!   ┌── Banner (sky + skyline + stats) ───────────────┐
//!   │  ◉ (sun) / ◯ (moon) が水平に往復                 │
//!   │  ▂▃▆▇█▇▆▃ parallax skyline silhouette            │
//!   ├── City grid (2-wide cells = 50col) ─┬── Panels ─┤
//!   │                                      │ Status   │
//!   │  ▟▙ ══ $$ …  + worker overlays      │ Manager  │
//!   │                                      │ AI Log   │
//!   └──────────────────────────────────────┴──────────┘
//!
//! 都市領域がレイアウト幅の 60% 以上を占める設計。クリック対象は side panel と
//! `Clickable` widget 経由で登録 (widgets-only-clicks ルール準拠)。
//!
//! 「みてるだけで楽しい」ための動的演出:
//!   - 建設タイルの進捗フェーズ (`··→░░→▒▒→▓▓`) + シマー
//!   - 完成時のフラッシュ (1.5秒間 REVERSED)
//!   - アクティブ店舗のキラキラ + 給料発生時のハイライト
//!   - 建設タイルに作業員の点滅 (`+`)
//!   - 二重ボーダー (`═` / `║`) で luxury 感
//!   - AI ティア記号 ([I]/[II]/[III]/[IV]) + 思考スピナー
//!   - 太陽 ◉ / 月 ◯ が空を往復し、時間経過を表す

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, TabBar};

use super::logic;
use super::state::{
    city_tier_for, next_tier_threshold, AiTier, Building, City, CityTier, PanelTab, Strategy,
    Tile, GRID_H, GRID_W, PAYOUT_FLASH_TICKS,
};
use super::terrain::Terrain;
use super::{
    ACT_HIRE_WORKER, ACT_STRATEGY_GROWTH, ACT_STRATEGY_INCOME, ACT_STRATEGY_TECH,
    ACT_TAB_EVENTS, ACT_TAB_MANAGER, ACT_TAB_STATUS, ACT_TAB_WORLD, ACT_UPGRADE_AI,
};

pub fn render(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    if is_narrow_layout(area.width) {
        render_narrow(state, f, area, click_state);
    } else {
        render_wide(state, f, area, click_state);
    }
}

// ── Wide layout ─────────────────────────────────────────────

fn render_wide(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    // 上にバナー、下に左右 2 カラム (グリッド | タブパネル)。
    let v = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    render_banner(state, f, v[0], false);

    let grid_w = GRID_W as u16 * 2 + 2; // 2-wide cells + borders
    let h = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Length(grid_w), Constraint::Min(24)])
        .split(v[1]);

    render_grid(state, f, h[0], 2);
    render_tab_panel(state, f, h[1], click_state);
}

// ── Narrow layout (<60 cols) ────────────────────────────────

fn render_narrow(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(4),                 // banner
            Constraint::Length(GRID_H as u16 + 2), // grid 1-wide
            Constraint::Min(8),                    // tab panel
        ])
        .split(area);
    render_banner(state, f, chunks[0], true);
    render_grid(state, f, chunks[1], 1);
    render_tab_panel(state, f, chunks[2], click_state);
}

// ── Banner: sky + skyline + dynamic title ───────────────────

fn render_banner(state: &City, f: &mut Frame, area: Rect, narrow: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(banner_border_color(state)))
        .title(banner_title(state, narrow));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(2);
    lines.push(Line::from(make_sky_line(state.tick, w)));
    if inner.height >= 2 {
        lines.push(Line::from(make_skyline_silhouette(state.tick, w)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn banner_border_color(state: &City) -> Color {
    // ティア進化フラッシュ中は全体を金色に。
    if state.tick < state.tier_flash_until {
        // 6 tick (0.6s) 周期で金/明黄を交互させ、目に止まる。
        if (state.tick / 3).is_multiple_of(2) {
            Color::LightYellow
        } else {
            Color::Yellow
        }
    } else if (state.tick / 10).is_multiple_of(2) {
        Color::Cyan
    } else {
        Color::LightCyan
    }
}

fn banner_title(state: &City, narrow: bool) -> String {
    let cpu = ai_tier_icon(state.ai_tier);
    let strat = strategy_tag(state.strategy);
    let busy = state.active_constructions();
    let pop = state.population();
    let tier = city_tier_for(pop);
    let tier_progress = tier_progress_label(tier, pop);
    if narrow {
        format!(
            " ▙▟ {}  {}  {}  WK {}/{} ",
            tier.jp(),
            cpu,
            strat,
            busy,
            state.workers
        )
    } else {
        format!(
            " ▙▟ {} ({}) {}  ── CPU {} {} ── STRAT {} {} ── WK {}/{} ── ",
            tier.name(),
            tier.jp(),
            tier_progress,
            cpu,
            state.ai_tier.name(),
            strat,
            strategy_label(state.strategy),
            busy,
            state.workers,
        )
    }
}

fn tier_progress_label(t: CityTier, pop: u32) -> String {
    match next_tier_threshold(t) {
        Some(target) => format!("pop {}/{}", pop, target),
        None => format!("pop {} ★MAX", pop),
    }
}

/// AI ティアを Roman 風 ASCII タグで表現 ([I] が dumbest、[IV] が smartest)。
fn ai_tier_icon(t: AiTier) -> &'static str {
    match t {
        AiTier::Random => "[I]",
        AiTier::Greedy => "[II]",
        AiTier::RoadPlanner => "[III]",
        AiTier::DemandAware => "[IV]",
    }
}

/// 戦略を 3 文字タグで表現。
fn strategy_tag(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "[GRW]",
        Strategy::Income => "[CSH]",
        Strategy::Tech => "[TEC]",
    }
}

/// 太陽 / 月 が水平に往復する 1 行 + 固定位置の星。
/// サイクル = `width * 2 * 30` ticks。
fn make_sky_line(tick: u64, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![Span::raw("")];
    }
    let cycle = (width * 2).max(1) as u64;
    let phase = (tick / 30) % cycle;
    let is_day = (phase as usize) < width;
    let pos = (phase as usize) % width;
    let body = if is_day { "◉" } else { "◯" };
    let body_color = if is_day { Color::Yellow } else { Color::LightCyan };

    // 星のチラつき: 固定の素数ステップで「点」を散らし、tick に応じて
    // 一部だけ明るく光らせる。日中は星をほぼ見えなくする。
    let mut chars: Vec<(char, Style)> = (0..width)
        .map(|i| {
            let is_star_slot = i.is_multiple_of(7) || (i + 3).is_multiple_of(11);
            if is_star_slot && !is_day {
                let twinkle = ((tick / 5) as usize + i).is_multiple_of(3);
                let style = if twinkle {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ('·', style)
            } else {
                (' ', Style::default())
            }
        })
        .collect();
    if pos < chars.len() {
        chars[pos] = (
            body.chars().next().unwrap_or('*'),
            Style::default()
                .fg(body_color)
                .add_modifier(Modifier::BOLD),
        );
    }

    // 連続する同スタイル文字を 1 Span にまとめて圧縮 (描画コスト削減)。
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut current_style = chars[0].1;
    for (c, st) in chars {
        if st == current_style {
            buf.push(c);
        } else {
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), current_style));
            }
            current_style = st;
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, current_style));
    }
    spans
}

/// パララックス効果でゆっくり横スクロールするスカイライン。
fn make_skyline_silhouette(tick: u64, width: usize) -> Vec<Span<'static>> {
    let pattern: &[char] = &[
        '▂', '▃', '▅', '▆', '▇', '█', '▇', '▆', '▅', '▆', '▃', '▅', '▆', '▇', '▆', '▅', '▃', ' ',
        '▆', '▇', '▆', ' ', '▅', '▆', '▇', '█', '▇', '▆', ' ', '▃', '▅', '▆', '▅', '▃', ' ',
    ];
    let scroll = ((tick / 60) as usize) % pattern.len();
    let s: String = (0..width)
        .map(|i| pattern[(i + scroll) % pattern.len()])
        .collect();
    vec![Span::styled(s, Style::default().fg(Color::DarkGray))]
}

// ── Grid ────────────────────────────────────────────────────

fn render_grid(state: &City, f: &mut Frame, area: Rect, cell_width: u16) {
    let title = format!(
        " ▟▙ City — POP {}  WIP {} ",
        state.population(),
        state.active_constructions()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(grid_border_color(state)))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let mut lines: Vec<Line> = Vec::with_capacity(GRID_H);
    for y in 0..GRID_H {
        let mut spans: Vec<Span> = Vec::with_capacity(GRID_W * cell_width as usize);
        for x in 0..GRID_W {
            spans.extend(tile_spans(state, x, y, cell_width));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn grid_border_color(state: &City) -> Color {
    // 完成フラッシュが多い時は LightGreen、それ以外は Cyan 系。
    if state.completion_flash_until.iter().flatten().any(|t| *t > state.tick) {
        Color::LightGreen
    } else {
        Color::Cyan
    }
}

fn tile_spans(state: &City, x: usize, y: usize, cell_width: u16) -> Vec<Span<'static>> {
    let tile = state.tile(x, y);
    let completion = state.tick < state.completion_flash_until[y][x];
    let payout = state.tick < state.payout_flash_until[y][x];
    if cell_width == 1 {
        vec![tile_span_1(tile, x, y, state.tick, completion, payout, state)]
    } else {
        tile_spans_2(tile, x, y, state.tick, completion, payout, state)
    }
}

// ── 1-wide cell (narrow) ────────────────────────────────────

fn tile_span_1(
    tile: &Tile,
    x: usize,
    y: usize,
    tick: u64,
    completion: bool,
    payout: bool,
    state: &City,
) -> Span<'static> {
    if completion {
        if let Tile::Built(b) = tile {
            return Span::styled(
                tile_char_1(*b).to_string(),
                Style::default()
                    .fg(Color::White)
                    .bg(built_color(*b))
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
    match tile {
        Tile::Empty => terrain_span_1(state.terrain_at(x, y), x, y, tick),
        Tile::Construction {
            target,
            ticks_remaining,
        } => {
            let total = target.build_ticks().max(1);
            let progress = (total - ticks_remaining) as f32 / total as f32;
            let g = if progress < 0.33 {
                '·'
            } else if progress < 0.67 {
                '░'
            } else {
                '▒'
            };
            let modifier = if (tick / 3).is_multiple_of(2) {
                Modifier::BOLD
            } else {
                Modifier::DIM
            };
            Span::styled(
                g.to_string(),
                Style::default()
                    .fg(construction_color(*target))
                    .add_modifier(modifier),
            )
        }
        Tile::Built(Building::Road) => {
            Span::styled("+".to_string(), Style::default().fg(Color::Gray))
        }
        Tile::Built(Building::House) => {
            let bright = !(tick / 10).is_multiple_of(4);
            let m = if bright { Modifier::BOLD } else { Modifier::empty() };
            let ch = match logic::house_level(state, x, y) {
                logic::HouseLevel::Low => 'h',
                logic::HouseLevel::Mid => 'H',
                logic::HouseLevel::High => '▮',
            };
            Span::styled(
                ch.to_string(),
                Style::default().fg(Color::Green).add_modifier(m),
            )
        }
        Tile::Built(Building::Shop) => {
            let level = logic::shop_level(state, x, y);
            if matches!(level, logic::ShopLevel::Idle) {
                Span::styled("s".to_string(), Style::default().fg(Color::DarkGray))
            } else {
                let style = if payout {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    let bright = (tick / 4).is_multiple_of(2);
                    let m = if bright { Modifier::BOLD } else { Modifier::empty() };
                    let color = match level {
                        logic::ShopLevel::Premium => Color::LightYellow,
                        logic::ShopLevel::Busy => Color::Yellow,
                        _ => Color::Yellow,
                    };
                    Style::default().fg(color).add_modifier(m)
                };
                let ch = match level {
                    logic::ShopLevel::Premium => '★',
                    logic::ShopLevel::Busy => 'S',
                    _ => 's',
                };
                Span::styled(ch.to_string(), style)
            }
        }
    }
}

fn tile_char_1(b: Building) -> char {
    match b {
        Building::Road => '+',
        Building::House => 'H',
        Building::Shop => 'S',
    }
}

// ── 2-wide cell (wide) ──────────────────────────────────────

fn tile_spans_2(
    tile: &Tile,
    x: usize,
    y: usize,
    tick: u64,
    completion: bool,
    payout: bool,
    state: &City,
) -> Vec<Span<'static>> {
    if completion {
        if let Tile::Built(b) = tile {
            return vec![Span::styled(
                built_2wide_glyph(*b).to_string(),
                Style::default()
                    .fg(Color::White)
                    .bg(built_color(*b))
                    .add_modifier(Modifier::BOLD),
            )];
        }
    }
    match tile {
        Tile::Empty => terrain_spans_2(state.terrain_at(x, y), x, y, tick),
        Tile::Construction {
            target,
            ticks_remaining,
        } => {
            let total = target.build_ticks().max(1);
            let progress = (total - ticks_remaining) as f32 / total as f32;
            let phase_pair = if progress < 0.25 {
                ('·', '·')
            } else if progress < 0.5 {
                ('░', '░')
            } else if progress < 0.75 {
                ('▒', '▒')
            } else {
                ('▓', '▓')
            };
            let base_color = construction_color(*target);
            let shimmer = if (tick / 3).is_multiple_of(2) {
                Modifier::BOLD
            } else {
                Modifier::DIM
            };
            // 作業員アイコン: 各 Construction には 1 ワーカーが居る。
            // 右側の文字を一定リズムで '+' に差し替えて「作業中」を可視化。
            let worker_blink = (tick / 2).is_multiple_of(2);
            let right_char = if worker_blink { '+' } else { phase_pair.1 };
            let right_color = if worker_blink {
                Color::LightYellow
            } else {
                base_color
            };
            let right_mod = if worker_blink {
                Modifier::BOLD
            } else {
                shimmer
            };
            vec![
                Span::styled(
                    phase_pair.0.to_string(),
                    Style::default().fg(base_color).add_modifier(shimmer),
                ),
                Span::styled(
                    right_char.to_string(),
                    Style::default().fg(right_color).add_modifier(right_mod),
                ),
            ]
        }
        Tile::Built(Building::Road) => vec![Span::styled(
            "══".to_string(),
            Style::default().fg(Color::Gray),
        )],
        Tile::Built(Building::House) => {
            // 密度レベルでグリフが変わる。低層 ▟▙ → 中層 ▛▜ → 高層 ██。
            let bright = !(tick / 10).is_multiple_of(4);
            let m = if bright { Modifier::BOLD } else { Modifier::empty() };
            let glyph = match logic::house_level(state, x, y) {
                logic::HouseLevel::Low => "▟▙",
                logic::HouseLevel::Mid => "▛▜",
                logic::HouseLevel::High => "██",
            };
            // 高層は LightGreen に明るくして「育った」感を強調。
            let color = match logic::house_level(state, x, y) {
                logic::HouseLevel::High => Color::LightGreen,
                _ => Color::Green,
            };
            vec![Span::styled(
                glyph.to_string(),
                Style::default().fg(color).add_modifier(m),
            )]
        }
        Tile::Built(Building::Shop) => {
            let level = logic::shop_level(state, x, y);
            if matches!(level, logic::ShopLevel::Idle) {
                vec![Span::styled(
                    "$$".to_string(),
                    Style::default().fg(Color::DarkGray),
                )]
            } else {
                let style = if payout {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    let bright = (tick / 4).is_multiple_of(2);
                    let m = if bright { Modifier::BOLD } else { Modifier::DIM };
                    let color = match level {
                        logic::ShopLevel::Premium => Color::LightYellow,
                        _ => Color::Yellow,
                    };
                    Style::default().fg(color).add_modifier(m)
                };
                let glyph = match level {
                    logic::ShopLevel::Premium => "★$",
                    logic::ShopLevel::Busy => "$$",
                    _ => "$·",
                };
                vec![Span::styled(glyph.to_string(), style)]
            }
        }
    }
}

fn construction_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Yellow,
        Building::House => Color::LightGreen,
        Building::Shop => Color::LightCyan,
    }
}

fn built_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Gray,
        Building::House => Color::Green,
        Building::Shop => Color::Yellow,
    }
}

fn built_2wide_glyph(b: Building) -> &'static str {
    match b {
        Building::Road => "══",
        Building::House => "▟▙",
        Building::Shop => "$$",
    }
}

// ── Terrain rendering ───────────────────────────────────────
//
// Empty セル上に地形を描画する。Forest と Water は時間でゆらぎ、
// 「生きているマップ」感を出す。

fn terrain_span_1(t: Terrain, x: usize, y: usize, tick: u64) -> Span<'static> {
    match t {
        Terrain::Plain => {
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "·" } else { " " };
            Span::styled(g.to_string(), Style::default().fg(Color::DarkGray))
        }
        Terrain::Forest => {
            // 微かに揺らぐ緑 (光合成)。
            let sway = ((tick / 8) as usize + x + y).is_multiple_of(3);
            let g = if sway { "♣" } else { "♠" };
            Span::styled(g.to_string(), Style::default().fg(Color::Green))
        }
        Terrain::Water => {
            // 水面のさざ波 (3 フレーム周期)。
            let wave = ((tick / 4) as usize + x + y) % 3;
            let g = match wave {
                0 => "~",
                1 => "≈",
                _ => "˜",
            };
            Span::styled(g.to_string(), Style::default().fg(Color::Blue))
        }
        Terrain::Wasteland => Span::styled(
            ":".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::DIM),
        ),
    }
}

fn terrain_spans_2(t: Terrain, x: usize, y: usize, tick: u64) -> Vec<Span<'static>> {
    match t {
        Terrain::Plain => {
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "· " } else { "  " };
            vec![Span::styled(
                g.to_string(),
                Style::default().fg(Color::DarkGray),
            )]
        }
        Terrain::Forest => {
            let sway = ((tick / 8) as usize + x + y).is_multiple_of(3);
            let g = if sway { "♣♣" } else { "♠♣" };
            vec![Span::styled(g.to_string(), Style::default().fg(Color::Green))]
        }
        Terrain::Water => {
            let wave = ((tick / 4) as usize + x + y) % 3;
            let g = match wave {
                0 => "~~",
                1 => "≈≈",
                _ => "~≈",
            };
            vec![Span::styled(g.to_string(), Style::default().fg(Color::Blue))]
        }
        Terrain::Wasteland => vec![Span::styled(
            "::".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::DIM),
        )],
    }
}

// ── Tab panel (right pane) ──────────────────────────────────
//
// 上に `TabBar`、下に現在タブの内容を描画する。
// `TabBar` は widgets primitive で、自動でクリック対象を登録するため
// disallowed_methods 規約に違反しない。

fn render_tab_panel(
    state: &City,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(format!(
            " {} ",
            state.panel_tab.label()
        )));
    let inner = outer.inner(area);
    f.render_widget(&outer, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let v = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // タブバー (1 行)。狭幅でも収まるよう区切りは "│" のみ、ラベルは短く。
    {
        let mut cs = click_state.borrow_mut();
        let bar = TabBar::new("│")
            .tab(
                format!("1 {}", PanelTab::Status.label()),
                tab_style(state.panel_tab == PanelTab::Status),
                ACT_TAB_STATUS,
            )
            .tab(
                format!("2 {}", PanelTab::Manager.label()),
                tab_style(state.panel_tab == PanelTab::Manager),
                ACT_TAB_MANAGER,
            )
            .tab(
                format!("3 {}", PanelTab::Events.label()),
                tab_style(state.panel_tab == PanelTab::Events),
                ACT_TAB_EVENTS,
            )
            .tab(
                format!("4 {}", PanelTab::World.label()),
                tab_style(state.panel_tab == PanelTab::World),
                ACT_TAB_WORLD,
            );
        bar.render(f, v[0], &mut cs);
    }

    // 内容。
    match state.panel_tab {
        PanelTab::Status => render_status(state, f, v[1]),
        PanelTab::Manager => render_buttons(state, f, v[1], click_state),
        PanelTab::Events => render_log(state, f, v[1]),
        PanelTab::World => render_world(state, f, v[1]),
    }
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

// ── World tab (seed + terrain summary) ──────────────────────
//
// シード値と地形比率を表示。「マイクラ感」を演出する場所で、後で
// 「シード入力 → リジェネ」も加えやすいようここに集約。

fn render_world(state: &City, f: &mut Frame, area: Rect) {
    let mut counts = [0u32; 4];
    for row in &state.terrain {
        for t in row {
            match t {
                Terrain::Plain => counts[0] += 1,
                Terrain::Forest => counts[1] += 1,
                Terrain::Water => counts[2] += 1,
                Terrain::Wasteland => counts[3] += 1,
            }
        }
    }
    let total = (GRID_W * GRID_H).max(1) as u32;
    let pct = |c: u32| (c * 100) / total;

    let lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("SEED ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("0x{:016X}", state.world_seed),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Plain     ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:>3}%", pct(counts[0])), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Forest ♣  ", Style::default().fg(Color::Green)),
            Span::styled(format!("{:>3}%", pct(counts[1])), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Water  ~  ", Style::default().fg(Color::Blue)),
            Span::styled(format!("{:>3}%", pct(counts[2])), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Waste  :  ", Style::default().fg(Color::Yellow)),
            Span::styled(format!("{:>3}%", pct(counts[3])), Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "湖は建設不可。森/荒地は建てられる。",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(Paragraph::new(lines), area);
}

// ── Status panel ────────────────────────────────────────────

/// CASH 行の収入ハイライトを点灯すべきか?
///
/// 起動直後は `last_payout_tick == 0` のため `tick - 0 < PAYOUT_FLASH_TICKS`
/// が真になり、収入が一度も発生していないのに LightYellow に光ってしまう。
/// `last_payout_amount > 0` を gate にすることで「実際の支払いが発生した直後」
/// のみハイライトする。
fn is_payout_flash_active(state: &City) -> bool {
    state.last_payout_amount > 0
        && state.tick.saturating_sub(state.last_payout_tick) < PAYOUT_FLASH_TICKS
}

fn render_status(state: &City, f: &mut Frame, area: Rect) {
    // タブの外側 Block が既に枠を提供するため、ここでは描画のみ。
    let inner = area;

    let income = logic::compute_income_per_sec(state);
    let pop = state.population();
    let active = state.active_constructions();
    let payout_recent = is_payout_flash_active(state);

    let income_color = if payout_recent {
        Color::LightYellow
    } else {
        Color::Green
    };

    let mut lines: Vec<Line> = Vec::new();

    // CASH 行 — 大文字ラベル + 太字数字。
    lines.push(Line::from(vec![
        Span::styled(
            "CASH ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("${}", state.cash),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  +${}/s", income),
            Style::default()
                .fg(income_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // 人口 / 建設中 行
    lines.push(Line::from(vec![
        Span::styled(
            "POP  ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{}", pop),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("    WIP ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", active),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // 作業員バー (busy=▰ / free=▱)
    lines.push(Line::from(worker_bar_spans(state, inner.width)));

    // 累計 + 経過秒
    lines.push(Line::from(vec![
        Span::styled("BLT  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", state.buildings_finished),
            Style::default().fg(Color::White),
        ),
        Span::styled("    TIME ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}s", state.tick / 10),
            Style::default().fg(Color::White),
        ),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

fn worker_bar_spans(state: &City, _max_width: u16) -> Vec<Span<'static>> {
    let busy = state.active_constructions();
    let total = state.workers;
    let free = total.saturating_sub(busy);

    let mut spans: Vec<Span> = vec![Span::styled(
        "WRK  ",
        Style::default().fg(Color::DarkGray),
    )];
    // 作業員のスロットを「働いている」・「待機中」で色分け。
    for _ in 0..busy {
        let busy_pulse = (state.tick / 2).is_multiple_of(2);
        let m = if busy_pulse {
            Modifier::BOLD
        } else {
            Modifier::empty()
        };
        spans.push(Span::styled(
            "▰".to_string(),
            Style::default().fg(Color::LightYellow).add_modifier(m),
        ));
    }
    for _ in 0..free {
        spans.push(Span::styled(
            "▱".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::styled(
        format!(" {}/{}", busy, total),
        Style::default().fg(Color::White),
    ));
    spans
}

// ── Manager panel (buttons) ─────────────────────────────────

fn render_buttons(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    // タブの外側 Block が既に枠を提供するため、ここでは中身のみ。
    let inner_area = area;

    let rows = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner_area);

    let mut cs = click_state.borrow_mut();

    button_row(
        f,
        rows[0],
        &mut cs,
        ACT_STRATEGY_GROWTH,
        "[G] [GRW] 成長重視",
        state.strategy == Strategy::Growth,
        Color::Green,
    );
    button_row(
        f,
        rows[1],
        &mut cs,
        ACT_STRATEGY_INCOME,
        "[I] [CSH] 収入重視",
        state.strategy == Strategy::Income,
        Color::Yellow,
    );
    button_row(
        f,
        rows[2],
        &mut cs,
        ACT_STRATEGY_TECH,
        "[T] [TEC] 技術投資",
        state.strategy == Strategy::Tech,
        Color::Cyan,
    );

    let hire_cost = logic::hire_worker_cost(state.workers);
    let (hire_label, hire_color) = match hire_cost {
        Some(c) if state.cash >= c => (format!("[W] ▰ 作業員雇用 (${})", c), Color::White),
        Some(c) => (format!("[W] ▰ 作業員雇用 (${})", c), Color::DarkGray),
        None => ("[W] ▰ 作業員MAX到達".to_string(), Color::DarkGray),
    };
    let p = Paragraph::new(Span::styled(hire_label, Style::default().fg(hire_color)));
    Clickable::new(p, ACT_HIRE_WORKER).render(f, rows[3], &mut cs);

    if let Some(next) = state.ai_tier.next() {
        let color = if state.cash >= next.upgrade_cost() {
            Color::Magenta
        } else {
            Color::DarkGray
        };
        let label = format!(
            "[U] {} CPU進化 → {} (${})",
            ai_tier_icon(next),
            next.name(),
            next.upgrade_cost()
        );
        let p = Paragraph::new(Span::styled(label, Style::default().fg(color)));
        Clickable::new(p, ACT_UPGRADE_AI).render(f, rows[4], &mut cs);
    } else {
        let p = Paragraph::new(Span::styled(
            "[U] [IV] CPU最大Tier到達",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(p, rows[4]);
    }
}

fn button_row(
    f: &mut Frame,
    area: Rect,
    cs: &mut ClickState,
    action_id: u16,
    label: &str,
    selected: bool,
    accent: Color,
) {
    let style = if selected {
        Style::default()
            .fg(accent)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(accent)
    };
    let p = Paragraph::new(Span::styled(label.to_string(), style));
    Clickable::new(p, action_id).render(f, area, cs);
}

// ── AI activity log ─────────────────────────────────────────

fn render_log(state: &City, f: &mut Frame, area: Rect) {
    // タブの外側 Block が既に枠を提供するため、タイトル風の 1 行を内側に。
    let spinner_chars = ['◐', '◓', '◑', '◒'];
    let spinner = spinner_chars[((state.tick / 2) % spinner_chars.len() as u64) as usize];
    let header = format!("{} AI {} 履歴", spinner, ai_tier_icon(state.ai_tier));

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        header,
        Style::default().fg(Color::Magenta),
    ))];
    for (i, e) in state.events.iter().enumerate() {
        let style = if i == 0 {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else if i == 1 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(e.clone(), style)));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn strategy_label(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "成長",
        Strategy::Income => "収入",
        Strategy::Tech => "技術",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    #[test]
    fn render_does_not_panic_on_empty_city() {
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 100, 30), &cs);
            })
            .unwrap();
    }

    #[test]
    fn render_does_not_panic_on_narrow_layout() {
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(40, 40)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 40, 40), &cs);
            })
            .unwrap();
    }

    #[test]
    fn manager_buttons_register_click_targets() {
        // 既定で Manager タブが選ばれているので、戦略ボタンは出るはず。
        let city = City::new();
        // 32×16 = 2-wide で 66 col 必要。ターミナルもそれに合わせる。
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 100, 30), &cs);
            })
            .unwrap();
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        for id in [
            ACT_STRATEGY_GROWTH,
            ACT_STRATEGY_INCOME,
            ACT_STRATEGY_TECH,
            ACT_HIRE_WORKER,
            ACT_UPGRADE_AI,
            // タブバーも常に登録される。
            ACT_TAB_STATUS,
            ACT_TAB_MANAGER,
            ACT_TAB_EVENTS,
            ACT_TAB_WORLD,
        ] {
            assert!(
                registered.contains(&id),
                "action {} missing from targets {:?}",
                id,
                registered
            );
        }
    }

    /// 都市グリッドが画面幅の半分以上を占めること (wide layout)。
    #[test]
    fn wide_layout_grid_occupies_majority_of_width() {
        // grid = 32*2 + 2 = 66. With area width 100 → 66/100 = 66% ≥ 50%.
        let grid_w = GRID_W as u16 * 2 + 2;
        let area_w = 100u16;
        assert!(
            grid_w * 2 >= area_w,
            "grid_w {} * 2 must be >= area_w {} for >50% coverage",
            grid_w,
            area_w
        );
    }

    /// 起動直後 (last_payout_amount == 0) は payout flash が点灯してはならない。
    /// Codex P2 (#94 r3190426465): tick - 0 < FLASH_TICKS で偽陽性になる回帰防止。
    #[test]
    fn payout_flash_does_not_trigger_on_fresh_city() {
        let city = City::new();
        assert!(
            !is_payout_flash_active(&city),
            "fresh city should not show payout flash"
        );
        // 数 tick 進めても、収入が無ければ点灯しない。
        let mut city = City::new();
        city.tick = 3;
        assert!(!is_payout_flash_active(&city));
    }

    /// 実際に支払いが発生した直後は点灯し、`PAYOUT_FLASH_TICKS` 経過で消える。
    #[test]
    fn payout_flash_lights_after_real_payout() {
        let mut city = City::new();
        city.last_payout_amount = 5;
        city.last_payout_tick = 10;
        city.tick = 11;
        assert!(is_payout_flash_active(&city));
        city.tick = 10 + PAYOUT_FLASH_TICKS;
        assert!(!is_payout_flash_active(&city));
    }

    /// 完成タイルは flash_until が tick より大きい間、特殊スタイルでレンダリングされる。
    #[test]
    fn completion_flash_renders_without_panic() {
        let mut city = City::new();
        // 仮想的にタイルを完成させてフラッシュをセット。
        city.set_tile(3, 3, Tile::Built(Building::House));
        city.completion_flash_until[3][3] = city.tick + 10;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 100, 30), &cs);
            })
            .unwrap();
    }
}
