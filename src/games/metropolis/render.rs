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
use crate::widgets::Clickable;

use super::logic;
use super::state::{
    AiTier, Building, City, Strategy, Tile, GRID_H, GRID_W, PAYOUT_FLASH_TICKS,
};
use super::{
    ACT_HIRE_WORKER, ACT_STRATEGY_BALANCED, ACT_STRATEGY_GROWTH, ACT_STRATEGY_INCOME,
    ACT_UPGRADE_AI,
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
    // 上にバナー、下に左右2カラム。
    let v = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    render_banner(state, f, v[0], false);

    let grid_w = GRID_W as u16 * 2 + 2; // 2-wide cells + borders
    let h = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Length(grid_w), Constraint::Min(20)])
        .split(v[1]);

    render_grid(state, f, h[0], 2);

    let right = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(7),  // status
            Constraint::Length(10), // buttons
            Constraint::Min(4),     // log
        ])
        .split(h[1]);

    render_status(state, f, right[0]);
    render_buttons(state, f, right[1], click_state);
    render_log(state, f, right[2]);
}

// ── Narrow layout (<60 cols) ────────────────────────────────

fn render_narrow(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(4),                 // banner
            Constraint::Length(GRID_H as u16 + 2), // grid 1-wide
            Constraint::Length(7),                 // status
            Constraint::Length(10),                // buttons
            Constraint::Min(4),                    // log
        ])
        .split(area);
    render_banner(state, f, chunks[0], true);
    render_grid(state, f, chunks[1], 1);
    render_status(state, f, chunks[2]);
    render_buttons(state, f, chunks[3], click_state);
    render_log(state, f, chunks[4]);
}

// ── Banner: sky + skyline + dynamic title ───────────────────

fn render_banner(state: &City, f: &mut Frame, area: Rect, narrow: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(banner_border_color(state.tick)))
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

fn banner_border_color(tick: u64) -> Color {
    // 数秒に1度だけ色を切替えるごく弱いパルス。
    if (tick / 10).is_multiple_of(2) {
        Color::Cyan
    } else {
        Color::LightCyan
    }
}

fn banner_title(state: &City, narrow: bool) -> String {
    let cpu = ai_tier_icon(state.ai_tier);
    let strat = strategy_tag(state.strategy);
    let busy = state.active_constructions();
    if narrow {
        format!(
            " ▙▟ METROPOLIS  {}  {}  WK {}/{} ",
            cpu, strat, busy, state.workers
        )
    } else {
        format!(
            " ▙▟ IDLE  METROPOLIS  ── CPU {} {} ── STRAT {} {} ── WORK {}/{} ── ",
            cpu,
            state.ai_tier.name(),
            strat,
            strategy_label(state.strategy),
            busy,
            state.workers,
        )
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
        Strategy::Balanced => "[BAL]",
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
        Tile::Empty => {
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "·" } else { " " };
            Span::styled(g.to_string(), Style::default().fg(Color::DarkGray))
        }
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
            Span::styled(
                "H".to_string(),
                Style::default().fg(Color::Green).add_modifier(m),
            )
        }
        Tile::Built(Building::Shop) => {
            let active = logic::shop_is_active(state, x, y);
            if active {
                let style = if payout {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    let bright = (tick / 4).is_multiple_of(2);
                    let m = if bright { Modifier::BOLD } else { Modifier::empty() };
                    Style::default().fg(Color::Yellow).add_modifier(m)
                };
                Span::styled("S".to_string(), style)
            } else {
                Span::styled("S".to_string(), Style::default().fg(Color::DarkGray))
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
        Tile::Empty => {
            // 軽い「地面」のテクスチャ。チェッカーパターンで薄いドット。
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "· " } else { "  " };
            vec![Span::styled(
                g.to_string(),
                Style::default().fg(Color::DarkGray),
            )]
        }
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
            // 「灯り」の演出: 周期で BOLD と通常を切替えて生活感を出す。
            let bright = !(tick / 10).is_multiple_of(4);
            let m = if bright { Modifier::BOLD } else { Modifier::empty() };
            vec![Span::styled(
                "▟▙".to_string(),
                Style::default().fg(Color::Green).add_modifier(m),
            )]
        }
        Tile::Built(Building::Shop) => {
            let active = logic::shop_is_active(state, x, y);
            if active {
                let style = if payout {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    let bright = (tick / 4).is_multiple_of(2);
                    let m = if bright { Modifier::BOLD } else { Modifier::DIM };
                    Style::default().fg(Color::Yellow).add_modifier(m)
                };
                vec![Span::styled("$$".to_string(), style)]
            } else {
                vec![Span::styled(
                    "$$".to_string(),
                    Style::default().fg(Color::DarkGray),
                )]
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Status ");
    let inner = block.inner(area);
    f.render_widget(&block, area);

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
    let inner = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Manager ")
        .border_style(Style::default().fg(Color::Magenta));
    f.render_widget(&inner, area);
    let inner_area = inner.inner(area);

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
        ACT_STRATEGY_BALANCED,
        "[B] [BAL] バランス",
        state.strategy == Strategy::Balanced,
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
    // AI が「考えている」ことを示すスピナー。block-quad characters。
    let spinner_chars = ['◐', '◓', '◑', '◒'];
    let spinner = spinner_chars[((state.tick / 2) % spinner_chars.len() as u64) as usize];
    let title = format!(" {} AI {} Activity ", spinner, ai_tier_icon(state.ai_tier));

    let lines: Vec<Line> = state
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            // 一番新しいイベント (i==0) は明るく表示して目を引く。
            let style = if i == 0 {
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else if i == 1 {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(e.clone(), style))
        })
        .collect();

    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}

fn strategy_label(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "成長",
        Strategy::Income => "収入",
        Strategy::Balanced => "バランス",
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
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 30), &cs);
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
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 30), &cs);
            })
            .unwrap();
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        for id in [
            ACT_STRATEGY_GROWTH,
            ACT_STRATEGY_INCOME,
            ACT_STRATEGY_BALANCED,
            ACT_HIRE_WORKER,
            ACT_UPGRADE_AI,
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
        // grid = 24*2 + 2 = 50. With area width 80 → 50/80 = 62.5% >= 50%.
        let grid_w = GRID_W as u16 * 2 + 2;
        let area_w = 80u16;
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
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 30), &cs);
            })
            .unwrap();
    }
}
