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

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::{Clickable, ClickableGrid, ClickableList, ScrollableTab, TabBar};

use super::logic;
use super::save::{format_offline_duration, MAX_OFFLINE_SECS, OFFLINE_EFFICIENCY_PCT};
use super::state::{
    city_tier_for, next_tier_threshold, AiTier, Building, City, CityTier, PanelTab,
    PendingOfflineWelcome, Strategy, Tile, GRID_H, GRID_W, PAYOUT_FLASH_TICKS, VIEW_H, VIEW_W,
};
use super::terrain::Terrain;
use super::{
    ACT_DISMISS_OFFLINE_WELCOME, ACT_HIRE_WORKER, ACT_PANEL_SCROLL_DOWN, ACT_PANEL_SCROLL_UP,
    ACT_STRATEGY_ECO, ACT_STRATEGY_GROWTH, ACT_STRATEGY_INCOME, ACT_STRATEGY_TECH,
    ACT_TAB_CATALOG, ACT_TAB_EVENTS, ACT_TAB_MANAGER, ACT_TAB_STATUS, ACT_TAB_WORLD,
    ACT_UPGRADE_AI,
};

/// Wide layout が必要とする最小幅。
/// 2-wide grid (32*2 + 2 = 66) + tab panel min (24) = 90 col。
/// グローバルの `is_narrow_layout(w < 60)` よりも厳しいしきい値で、
/// 60-89 col の中間幅 (80×N の典型 PC ターミナル含む) で右パネルが
/// 潰れる回帰を防ぐ。Codex review #96 r3192962003 の指摘を反映。
const METROPOLIS_WIDE_MIN_WIDTH: u16 = 90;

fn metropolis_is_narrow(width: u16) -> bool {
    width < METROPOLIS_WIDE_MIN_WIDTH
}

pub fn render(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    if metropolis_is_narrow(area.width) {
        render_narrow(state, f, area, click_state);
    } else {
        render_wide(state, f, area, click_state);
    }
    // 通常レイアウトの上にオーバーレイで重ねる。`Clear` widget で配下を白紙化
    // してから描画するため、背景の建物表示などとは独立した見た目になる。
    if let Some(welcome) = state.pending_offline_welcome.as_ref() {
        render_offline_welcome_overlay(welcome, f, area, click_state);
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

    let grid_w = VIEW_W as u16 * 2 + 2; // 2-wide cells + borders (viewport)
    let h = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Length(grid_w), Constraint::Min(24)])
        .split(v[1]);

    render_grid(state, f, h[0], 2, click_state);
    render_tab_panel(state, f, h[1], click_state);
}

// ── Narrow layout (<60 cols) ────────────────────────────────

fn render_narrow(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(4),                 // banner
            Constraint::Length(VIEW_H as u16 + 2), // grid 1-wide (viewport)
            Constraint::Min(8),                    // tab panel
        ])
        .split(area);
    render_banner(state, f, chunks[0], true);
    render_grid(state, f, chunks[1], 1, click_state);
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
        lines.push(Line::from(make_skyline_silhouette(
            state.tick,
            w,
            state.population(),
        )));
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
        AiTier::Aware => "[III]",
        AiTier::Planner => "[IV]",
        AiTier::Master => "[V]",
    }
}

/// 戦略を 3 文字タグで表現。
fn strategy_tag(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "[GRW]",
        Strategy::Income => "[CSH]",
        Strategy::Tech => "[TEC]",
        Strategy::Eco => "[ECO]",
    }
}

/// 太陽 / 月 が水平に往復する 1 行 + 固定位置の星。
///
/// `logic::day_phase` と同期: Day 中は太陽 ◉ が左→右へ、Night 中は月 ◯ が
/// 左→右へ。Dusk は次の天体が右端付近に固定 (沈みかけ/昇る前)。これで
/// 太陽の位置と建物の窓灯りが一致する (前は別位相で違和感があった)。
fn make_sky_line(tick: u64, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![Span::raw("")];
    }
    let phase = logic::day_phase(tick);
    let progress = logic::day_progress(tick);
    let is_day = matches!(phase, logic::DayPhase::Day | logic::DayPhase::Dusk);
    let pos = ((progress * width.saturating_sub(1) as f32) as usize).min(width - 1);
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
///
/// 人口に応じて全体の高さがリッチ化する: pop 0 では低い丘の連なり、
/// Metropolis 帯では摩天楼が混ざる。`pop / 50` の指数で「街の成熟度」を
/// 表現し、pop が増えるたびにスカイラインが目に見えて変わるので
/// 「街が育っている」実感を画面上部で常に伝えられる。
fn make_skyline_silhouette(tick: u64, width: usize, pop: u32) -> Vec<Span<'static>> {
    // 高さの基本パレット (低 → 高)。pop が増えると右側を多く採用する。
    const HEIGHTS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // pop に応じた成熟度 0..=8。50 pop ごとに 1 段階。
    let maturity = ((pop / 50) as usize).min(HEIGHTS.len() - 1);
    let scroll = (tick / 60) as usize;

    // 各列の高さは「決定論的な疑似ランダム」で安定させる。
    // ノイズ関数 = (i * 11 + scroll * 7) ^ (i / 3) を 0..=8 にマップ。
    let s: String = (0..width)
        .map(|i| {
            let h_seed = (i.wrapping_mul(11).wrapping_add(scroll.wrapping_mul(7))) ^ (i / 3);
            // base height = 0..=maturity の幅でランダムに揺らす。
            let h = h_seed % (maturity + 1);
            HEIGHTS[h]
        })
        .collect();
    vec![Span::styled(s, Style::default().fg(Color::DarkGray))]
}

// ── Grid ────────────────────────────────────────────────────

fn render_grid(
    state: &City,
    f: &mut Frame,
    area: Rect,
    cell_width: u16,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // ビューポート位置を表示。マップが 64×32 の世界を 32×16 で覗く方式。
    // [hjkl] でスクロール可能 (Vim 流) ことをタイトルに併記。
    let title = format!(
        " ▟▙ City — POP {}  WIP {}  ◎({},{})/{}×{}  [hjkl]スクロール ",
        state.population(),
        state.active_constructions(),
        state.cam_x,
        state.cam_y,
        GRID_W,
        GRID_H,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(grid_border_color(state)))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(&block, area);

    // ビューポート範囲だけ描画。`state.cam_x` / `cam_y` が左上セル。
    // VIEW_W / VIEW_H をはみ出す座標は GRID_W / GRID_H で clamp。
    let x0 = state.cam_x;
    let y0 = state.cam_y;
    let x1 = (x0 + VIEW_W).min(GRID_W);
    let y1 = (y0 + VIEW_H).min(GRID_H);

    // edge-connectivity BFS をフレーム冒頭で 1 回だけ計算し、per-tile ループに流す。
    // タイルごとに per-cell BFS を回すと 32*16 = 512 BFS / フレームになり UI 応答性を
    // 大きく損なう。`cached_edge_connected_roads` は tick 境界毎にしか BFS を走らせず、
    // 60 FPS の render から複数回呼ばれても 1 BFS / 10 frames に抑えられる。
    let connected = logic::cached_edge_connected_roads(state);

    let mut lines: Vec<Line> = Vec::with_capacity(VIEW_H);
    for y in y0..y1 {
        let mut spans: Vec<Span> = Vec::with_capacity(VIEW_W * cell_width as usize);
        for x in x0..x1 {
            spans.extend(tile_spans(state, x, y, cell_width, &connected));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);

    // 各セルにクリックターゲットを登録 (施設タップ詳細用)。
    // ClickableGrid はビューポート相対の (col, row) を `base + row*VIEW_W + col` で
    // u16 に詰める。`handle_input` 側で同じ式の逆を使い (cam_x, cam_y) を足して絶対座標に。
    {
        let mut cs = click_state.borrow_mut();
        let grid = ClickableGrid::new(
            VIEW_W,
            VIEW_H,
            super::ACT_GRID_CELL_BASE,
            cell_width,
        );
        // padding_left = 0 (block の inner area がそのまま grid 描画範囲)。
        // block.inner() で borders 分は既に除外されている。
        grid.register_targets(area, &block, &mut cs, 0);
    }
}

fn grid_border_color(state: &City) -> Color {
    // 完成フラッシュが多い時は LightGreen、それ以外は Cyan 系。
    if state.completion_flash_until.iter().flatten().any(|t| *t > state.tick) {
        Color::LightGreen
    } else {
        Color::Cyan
    }
}

fn tile_spans(
    state: &City,
    x: usize,
    y: usize,
    cell_width: u16,
    connected: &[Vec<bool>],
) -> Vec<Span<'static>> {
    let tile = state.tile(x, y);
    let completion = state.tick < state.completion_flash_until[y][x];
    let payout = state.tick < state.payout_flash_until[y][x];
    if cell_width == 1 {
        vec![tile_span_1(tile, x, y, state.tick, completion, payout, state, connected)]
    } else {
        tile_spans_2(tile, x, y, state.tick, completion, payout, state, connected)
    }
}

// ── 1-wide cell (narrow) ────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn tile_span_1(
    tile: &Tile,
    x: usize,
    y: usize,
    tick: u64,
    completion: bool,
    payout: bool,
    state: &City,
    connected: &[Vec<bool>],
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
        Tile::Clearing { .. } => {
            // 整地中: 元の地形 (Forest/Wasteland) の上を斜線で覆う。
            // tick で斜線が回転して「作業中」感を出す。
            let g = ['╳', '╲', '╱', '╳'][((tick / 3) as usize) % 4];
            Span::styled(
                g.to_string(),
                Style::default()
                    .fg(Color::LightYellow)
                    .bg(terrain_bg_at(state.terrain_at(x, y), tick)),
            )
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
            // 1-wide でも自動接続グリフ (狭幅では box-drawing 1 文字)。
            let connections = road_connections(state, x, y);
            let glyph = road_1wide_glyph(connections);
            Span::styled(
                glyph.to_string(),
                Style::default()
                    .fg(Color::Gray)
                    .bg(Color::Rgb(40, 40, 40)),
            )
        }
        Tile::Built(Building::House) => {
            // 1-wide では tier を 1 文字で表現:
            //   Cottage   → 'h' (緑)
            //   Apartment → 'H' (青緑、太字)
            //   Highrise  → '▮' (シアン、太字)
            // BFS 共有版 (`_with`) を使う — render hot path での再計算回避。
            let tier = logic::effective_tier_at_with(state, x, y, connected);
            let bright = !(tick / 10).is_multiple_of(4);
            let m = if bright { Modifier::BOLD } else { Modifier::empty() };
            let (ch, color) = match tier {
                logic::HouseTier::Cottage => ('h', Color::Green),
                logic::HouseTier::Apartment => ('H', Color::LightGreen),
                logic::HouseTier::Highrise => ('▮', Color::LightCyan),
                logic::HouseTier::Tower => ('▌', Color::LightMagenta),
                logic::HouseTier::Arcology => ('◆', Color::Magenta),
            };
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(color)
                    .bg(house_bg(tier, tick))
                    .add_modifier(m),
            )
        }
        Tile::Built(Building::Shop) => {
            // BFS 共有版を使う (Codex review #103 P1)。
            let level = logic::shop_level_with(state, x, y, connected);
            if matches!(level, logic::ShopLevel::Idle) {
                Span::styled(
                    "s".to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(50, 50, 50)),
                )
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
                    let bg = match level {
                        logic::ShopLevel::Premium => Color::Rgb(90, 60, 0),
                        _ => Color::Rgb(60, 40, 0),
                    };
                    Style::default().fg(color).bg(bg).add_modifier(m)
                };
                let ch = match level {
                    logic::ShopLevel::Premium => '★',
                    logic::ShopLevel::Busy => 'S',
                    _ => 's',
                };
                Span::styled(ch.to_string(), style)
            }
        }
        Tile::Built(Building::Workshop) => {
            // BFS 共有版 (Codex review #103 P1)。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            // 煙突アニメ: 3 フレーム周期で `°` `゜` ` ` を切り替えて煙が立つ感じ。
            // 非アクティブは灰色固定で「火が入っていない」を表現。
            let smoke_phase = (tick / 4) as usize % 3;
            let ch = if active {
                ['w', 'W', 'w'][smoke_phase]
            } else {
                'w'
            };
            let (fg, bg) = if active {
                (Color::LightRed, Color::Rgb(60, 30, 30))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg))
        }
        Tile::Built(Building::Factory) => {
            // 工場: Workshop の上位 — 煙が大きく立ち上る `F` 字。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let phase = (tick / 3) as usize % 4;
            let ch = if active {
                ['F', 'f', 'F', '#'][phase]
            } else {
                'F'
            };
            let (fg, bg) = if active {
                (Color::Red, Color::Rgb(80, 25, 20))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD))
        }
        Tile::Built(Building::Mall) => {
            // 商業ビル: ★ + ネオン点滅。
            let active = logic::shop_is_active_with(state, x, y, connected);
            let bright = (tick / 3).is_multiple_of(2);
            let ch = if active {
                if bright { 'M' } else { 'm' }
            } else {
                'M'
            };
            let (fg, bg) = if active {
                (Color::LightYellow, Color::Rgb(110, 70, 0))
            } else {
                (Color::DarkGray, Color::Rgb(50, 50, 50))
            };
            let mods = if bright && active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            };
            Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg).add_modifier(mods))
        }
        Tile::Built(Building::Office) => {
            // オフィス: 高層ガラス。窓灯りが夜に点く。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let night = matches!(logic::day_phase(tick), logic::DayPhase::Night);
            let ch = if night && active { 'O' } else { 'o' };
            let (fg, bg) = if active {
                if night {
                    (Color::LightYellow, Color::Rgb(20, 30, 80))
                } else {
                    (Color::LightCyan, Color::Rgb(30, 40, 70))
                }
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 50))
            };
            Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD))
        }
        Tile::Built(Building::Park) => {
            // 1-wide: 公園は花/蝶を 1 文字でゆらす。夜は蛍。
            let (ch, fg) = park_glyph_1wide(tick);
            let (br, bg, bb) = park_bg_rgb(tick);
            Span::styled(
                ch.to_string(),
                Style::default().fg(fg).bg(Color::Rgb(br, bg, bb)),
            )
        }
        Tile::Built(Building::Outpost) => {
            // 開拓機材: 重機の点滅。1-wide では `⚒` を tick で点滅。
            let blink = (tick / 4).is_multiple_of(2);
            let ch = if blink { '⚒' } else { '⚙' };
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(Color::LightYellow)
                    .bg(Color::Rgb(50, 40, 20))
                    .add_modifier(Modifier::BOLD),
            )
        }
        Tile::Built(Building::Plaza) => {
            // 中央広場: Park より明るい色味で「人が集う」感を出す。
            let phase = (tick / 5) as usize % 3;
            let ch = ['◈', '✦', '◇'][phase];
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(Color::LightMagenta)
                    .bg(Color::Rgb(60, 40, 80))
                    .add_modifier(Modifier::BOLD),
            )
        }
        Tile::Built(Building::Stadium) => {
            // 競技場: 観客のざわめきを点滅で表現。
            let bright = (tick / 3).is_multiple_of(2);
            let ch = if bright { '◎' } else { '○' };
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(Color::LightYellow)
                    .bg(Color::Rgb(70, 40, 0))
                    .add_modifier(Modifier::BOLD),
            )
        }
        Tile::Built(Building::MegaMall) => {
            // メガモール: Mall より眩しいネオン (☆ + 強点滅)。
            let active = logic::shop_is_active_with(state, x, y, connected);
            let bright = (tick / 2).is_multiple_of(2);
            let ch = if active && bright { '☆' } else { '★' };
            let (fg, bg) = if active {
                (Color::LightYellow, Color::Rgb(140, 80, 0))
            } else {
                (Color::DarkGray, Color::Rgb(50, 50, 50))
            };
            Span::styled(
                ch.to_string(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )
        }
        Tile::Built(Building::Headquarters) => {
            // 本社ビル: 高層 + 屋上に赤い航空標識 (夜だけ点滅)。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let night = matches!(logic::day_phase(tick), logic::DayPhase::Night);
            let beacon = night && (tick % 15) < 5;
            let ch = if beacon { '▼' } else { '▣' };
            let (fg, bg) = if active {
                if beacon {
                    (Color::LightRed, Color::Rgb(20, 30, 90))
                } else if night {
                    (Color::LightYellow, Color::Rgb(20, 30, 80))
                } else {
                    (Color::White, Color::Rgb(30, 50, 90))
                }
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 50))
            };
            Span::styled(
                ch.to_string(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )
        }
        Tile::Built(Building::Refinery) => {
            // 製油所: Factory より大きい煙と炎の点滅。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let phase = (tick / 2) as usize % 4;
            let ch = if active {
                ['R', '#', 'R', '▒'][phase]
            } else {
                'R'
            };
            let (fg, bg) = if active {
                (Color::LightRed, Color::Rgb(110, 30, 0))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            Span::styled(
                ch.to_string(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )
        }
    }
}

fn tile_char_1(b: Building) -> char {
    match b {
        Building::Road => '+',
        Building::House => 'H',
        Building::Workshop => 'W',
        Building::Factory => 'F',
        Building::Refinery => 'R',
        Building::Shop => 'S',
        Building::Mall => 'M',
        Building::MegaMall => '★',
        Building::Office => 'O',
        Building::Headquarters => '▣',
        Building::Park => 'P',
        Building::Plaza => '◈',
        Building::Stadium => '◎',
        Building::Outpost => 'X',
    }
}

// ── 2-wide cell (wide) ──────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn tile_spans_2(
    tile: &Tile,
    x: usize,
    y: usize,
    tick: u64,
    completion: bool,
    payout: bool,
    state: &City,
    connected: &[Vec<bool>],
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
        Tile::Clearing { .. } => {
            // 2-wide 整地中: 斧 / 鍬 が動くアニメ + 元の地形背景。
            // 4-frame で `╲╳ ╳╱ ╱╳ ╳╲` を回し、作業員が振ってる感を出す。
            let frame = ((tick / 3) as usize) % 4;
            let pair = ["╲╳", "╳╱", "╱╳", "╳╲"][frame];
            vec![Span::styled(
                pair.to_string(),
                Style::default()
                    .fg(Color::LightYellow)
                    .bg(terrain_bg_at(state.terrain_at(x, y), tick)),
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
        Tile::Built(Building::Road) => {
            // 4-近傍の Road を見て自動接続グリフを選ぶ (rebels-in-the-sky 流)。
            // 道路網が「線として繋がっている」絵が出ることで、ただの灰色マスから
            // 「街路網」へ印象が変わる。1 つだけポツンとある時は十字 (+) を出して
            // 「未接続だが道路として意図されている」ことを示す。
            //
            // **交通フロー演出**: 直線道路 (水平 `══` / 垂直 `║`) は、進行方向に
            // 沿って明るい光点が走るアニメに差し替える。x または y で位相をずらすと
            // 「車のヘッドライトが流れて見える」効果が出る。曲がり角 / 交差点 / 孤立は
            // そのまま静的グリフ (動かすとチラつき増)。
            let connections = road_connections(state, x, y);
            let traffic = road_traffic_glyph(connections, x, y, tick);
            road_spans_2wide(connections, traffic, tick)
        }
        Tile::Built(Building::House) => {
            // 2 軸表現: HouseTier (経済充実度) × HouseLevel (隣接密度)。
            //   - HouseTier がグリフの主軸 (Cottage 屋根 / Apartment 中層 / Highrise 摩天楼)
            //   - HouseLevel が密度ニュアンス (孤立 / 小集団 / 高密集)
            // 夜間 (バナーの月相と同期) になると Apartment/Highrise の窓が灯る。
            // BFS 共有版を使う (Codex review #103 P1)。
            let tier = logic::effective_tier_at_with(state, x, y, connected);
            let level = logic::house_level(state, x, y);
            let glyph = house_glyph_2wide(tier, level);
            let (color, modifier) = house_style_2wide(tier, tick);
            let bg = house_bg(tier, tick);

            // 航空標識: Highrise が密集 (周囲 3 軒以上 Highrise) で、夜間に
            // 1.5 秒周期で右側 1 文字を `*` (赤太字) に差し替える。
            // 都市感の最後のスパイス — Tier 4 経済まで育てたプレイヤーへのご褒美。
            if matches!(tier, logic::HouseTier::Highrise)
                && logic::should_show_aviation_light_with(state, x, y, tick, connected)
            {
                let mut chars = glyph.chars();
                let left = chars.next().unwrap_or(' ');
                vec![
                    Span::styled(
                        left.to_string(),
                        Style::default().fg(color).bg(bg).add_modifier(modifier),
                    ),
                    Span::styled(
                        "*".to_string(),
                        Style::default()
                            .fg(Color::LightRed)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]
            } else {
                vec![Span::styled(
                    glyph.to_string(),
                    Style::default().fg(color).bg(bg).add_modifier(modifier),
                )]
            }
        }
        Tile::Built(Building::Shop) => {
            // BFS 共有版を使う (Codex review #103 P1)。
            let level = logic::shop_level_with(state, x, y, connected);
            if matches!(level, logic::ShopLevel::Idle) {
                // 非アクティブ: 灰背景でくすませる (生気のない店)。
                vec![Span::styled(
                    "$$".to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(50, 50, 50)),
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
                    // アクティブ Shop は黄色のテント感ある背景。Premium はより明るく。
                    let bg = match level {
                        logic::ShopLevel::Premium => Color::Rgb(90, 60, 0),
                        _ => Color::Rgb(60, 40, 0),
                    };
                    Style::default().fg(color).bg(bg).add_modifier(m)
                };
                let glyph = match level {
                    logic::ShopLevel::Premium => "★$",
                    logic::ShopLevel::Busy => "$$",
                    _ => "$·",
                };
                vec![Span::styled(glyph.to_string(), style)]
            }
        }
        Tile::Built(Building::Workshop) => {
            // 工房: 煙突 (左) + 建物本体 (右)。煙突から煙が立ち上る 4 frame アニメ。
            // アクティブで初めて火が入って煙が出る — 非アクティブは煙ゼロの暗い灰。
            // BFS 共有版を使う (Codex review #103 P1)。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let phase = (tick / 4) as usize % 4;
            let smoke = if active {
                ['°', '˚', '·', ' '][phase]
            } else {
                ' '
            };
            let body = if active { '⊞' } else { '⊟' };
            let glyph = format!("{}{}", smoke, body);
            let (fg, bg) = if active {
                (Color::LightRed, Color::Rgb(60, 30, 30))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            vec![Span::styled(glyph, Style::default().fg(fg).bg(bg))]
        }
        Tile::Built(Building::Factory) => {
            // 工場: 大きい煙突 2 本 + 太い本体。Workshop よりダイナミックな煙アニメ。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let phase = (tick / 3) as usize % 4;
            let smoke = if active {
                ['▆', '▅', '▄', '▃'][phase]
            } else {
                '▁'
            };
            let body = if active { '▣' } else { '▢' };
            let glyph = format!("{}{}", smoke, body);
            let (fg, bg) = if active {
                (Color::Red, Color::Rgb(80, 25, 20))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            vec![Span::styled(
                glyph,
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )]
        }
        Tile::Built(Building::Mall) => {
            // 商業ビル: ★ + ネオン。Shop の上位らしい派手な色合い。
            let active = logic::shop_is_active_with(state, x, y, connected);
            let bright = (tick / 3).is_multiple_of(2);
            if !active {
                vec![Span::styled(
                    "★$".to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(50, 50, 50)),
                )]
            } else {
                let glyph = if bright { "★$" } else { "✦$" };
                let bg = Color::Rgb(110, 70, 0);
                let mods = if bright {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                };
                vec![Span::styled(
                    glyph.to_string(),
                    Style::default().fg(Color::LightYellow).bg(bg).add_modifier(mods),
                )]
            }
        }
        Tile::Built(Building::Office) => {
            // オフィス: 高層ガラス窓 — 昼はシアン、夜は黄色 (窓灯り)。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let night = matches!(logic::day_phase(tick), logic::DayPhase::Night);
            let glyph = if active {
                if night { "▮▮" } else { "▭▭" }
            } else {
                "▭▭"
            };
            let (fg, bg) = if active {
                if night {
                    (Color::LightYellow, Color::Rgb(20, 30, 80))
                } else {
                    (Color::LightCyan, Color::Rgb(30, 40, 70))
                }
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 50))
            };
            vec![Span::styled(
                glyph.to_string(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )]
        }
        Tile::Built(Building::Park) => {
            // 公園: 2-wide 右側の文字を「蝶/花/蛍」のアニメで動かす。
            // 昼間は花 + 蝶が舞い、夜は蛍が黄色く点滅。Forest と被らない
            // ように左の文字は固定の `❀` 系、右はフレームアニメ。
            //
            // x/y で位相をずらすと「公園が複数並んだ時に蝶が同時に動かない」
            // 自然な雰囲気が出る。
            let (left_ch, right_ch, fg) = park_glyph_2wide(tick, x, y);
            let (br, bg, bb) = park_bg_rgb(tick);
            let bg_color = Color::Rgb(br, bg, bb);
            vec![
                Span::styled(left_ch.to_string(), Style::default().fg(fg).bg(bg_color)),
                Span::styled(
                    right_ch.to_string(),
                    Style::default()
                        .fg(fg)
                        .bg(bg_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]
        }
        Tile::Built(Building::Outpost) => {
            // 開拓機材: 重機 (左) + 操作パネル (右) の 2-wide 表現。
            // ライトが 4 frame で回転して「稼働中」を示す。
            let phase = (tick / 3) as usize % 4;
            let lamp = ['◐', '◓', '◑', '◒'][phase];
            let body = '⚒';
            vec![
                Span::styled(
                    lamp.to_string(),
                    Style::default()
                        .fg(Color::LightYellow)
                        .bg(Color::Rgb(50, 40, 20))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    body.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(50, 40, 20))
                        .add_modifier(Modifier::BOLD),
                ),
            ]
        }
        Tile::Built(Building::Plaza) => {
            // 中央広場: 噴水 + 花壇の点滅。
            let phase = (tick / 4) as usize % 4;
            let left = ['◈', '✦', '◇', '✧'][phase];
            let right = ['✿', '❀', '✿', '❀'][phase];
            let bg = Color::Rgb(60, 40, 80);
            vec![
                Span::styled(
                    left.to_string(),
                    Style::default()
                        .fg(Color::LightMagenta)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    right.to_string(),
                    Style::default()
                        .fg(Color::LightYellow)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
            ]
        }
        Tile::Built(Building::Stadium) => {
            // 競技場: ◎ + 看板の点滅で「観戦中」を示す。
            let bright = (tick / 3).is_multiple_of(2);
            let glyph = if bright { "◎▣" } else { "○▣" };
            let bg = Color::Rgb(70, 40, 0);
            vec![Span::styled(
                glyph.to_string(),
                Style::default()
                    .fg(Color::LightYellow)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )]
        }
        Tile::Built(Building::MegaMall) => {
            // メガモール: ☆ ★ の眩しいネオン交互点滅。
            let active = logic::shop_is_active_with(state, x, y, connected);
            let bright = (tick / 2).is_multiple_of(2);
            if !active {
                vec![Span::styled(
                    "☆★".to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Rgb(50, 50, 50)),
                )]
            } else {
                let glyph = if bright { "☆★" } else { "★☆" };
                vec![Span::styled(
                    glyph.to_string(),
                    Style::default()
                        .fg(Color::LightYellow)
                        .bg(Color::Rgb(140, 80, 0))
                        .add_modifier(Modifier::BOLD),
                )]
            }
        }
        Tile::Built(Building::Headquarters) => {
            // 本社ビル: 高層 + 屋上に赤い航空標識 (夜だけ点滅)。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let night = matches!(logic::day_phase(tick), logic::DayPhase::Night);
            let beacon = night && (tick % 15) < 5;
            let glyph = if active {
                if beacon { "▼▣" } else if night { "▮▣" } else { "▭▣" }
            } else {
                "▭▢"
            };
            let (fg, bg) = if active {
                if beacon {
                    (Color::LightRed, Color::Rgb(20, 30, 90))
                } else if night {
                    (Color::LightYellow, Color::Rgb(20, 30, 80))
                } else {
                    (Color::White, Color::Rgb(30, 50, 90))
                }
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 50))
            };
            vec![Span::styled(
                glyph.to_string(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )]
        }
        Tile::Built(Building::Refinery) => {
            // 製油所: 大きい煙突 + 炎の点滅。Factory より重厚。
            let active = logic::workshop_is_active_with(state, x, y, connected);
            let phase = (tick / 2) as usize % 4;
            let smoke = if active {
                ['▓', '█', '▓', '▒'][phase]
            } else {
                '▁'
            };
            let body = if active { '▣' } else { '▢' };
            let glyph = format!("{}{}", smoke, body);
            let (fg, bg) = if active {
                (Color::LightRed, Color::Rgb(110, 30, 0))
            } else {
                (Color::DarkGray, Color::Rgb(40, 40, 40))
            };
            vec![Span::styled(
                glyph,
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            )]
        }
    }
}

/// Park の背景 RGB (昼/夜で変化)。Forest より少し明るく開けた緑にして
/// 「人の手が入った緑地」感を出す。
fn park_bg_rgb(tick: u64) -> (u8, u8, u8) {
    let base = (25, 70, 35); // 明るい草地
    let dim = logic::day_phase(tick).dim_factor();
    logic::dim_rgb(base.0, base.1, base.2, dim)
}

/// 1-wide Park: 1 文字で「花/蝶/蛍」のいずれかを tick で循環。
fn park_glyph_1wide(tick: u64) -> (char, Color) {
    let phase = (tick / 5) as usize % 4;
    if matches!(logic::day_phase(tick), logic::DayPhase::Night) {
        // 夜は蛍を点滅。
        let (ch, fg) = [('*', Color::LightYellow), ('·', Color::Yellow)][phase % 2];
        (ch, fg)
    } else {
        // 昼は蝶/花を循環。
        let table: [(char, Color); 4] = [
            ('❀', Color::LightMagenta),
            ('*', Color::White),
            ('❀', Color::LightYellow),
            ('·', Color::LightGreen),
        ];
        table[phase]
    }
}

/// 2-wide Park: 左 = 固定の花 (`❀`)、右 = 蝶/蛍がアニメ。
fn park_glyph_2wide(tick: u64, x: usize, y: usize) -> (char, char, Color) {
    let phase = ((tick / 4) as usize + x + y * 2) % 4;
    if matches!(logic::day_phase(tick), logic::DayPhase::Night) {
        // 夜: 蛍が黄色で揺れる。左側もたまに点く (位相違い)。
        let firefly_right = ['*', '·', '˙', '·'][phase];
        let firefly_left = if (phase + 2).is_multiple_of(4) { '*' } else { ' ' };
        (firefly_left, firefly_right, Color::LightYellow)
    } else {
        // 昼: 左に花 ❀、右に蝶/葉が舞う。
        let butterfly = ['*', '·', '✿', '·'][phase];
        let fg = match phase {
            0 => Color::White,
            2 => Color::LightMagenta,
            _ => Color::LightGreen,
        };
        ('❀', butterfly, fg)
    }
}

/// House の元 RGB (昼間)。Tier ごとに「街区が育つ」色相。
/// Cottage は土の上 (暗茶)、Apartment は舗装地 (灰)、Highrise はガラス張り (青)。
fn house_bg_rgb(tier: logic::HouseTier) -> (u8, u8, u8) {
    match tier {
        logic::HouseTier::Cottage => (40, 25, 15),
        logic::HouseTier::Apartment => (40, 40, 40),
        logic::HouseTier::Highrise => (20, 30, 60),
        // Tower / Arcology は紫がかった背景で「終盤の象徴」感を出す。
        logic::HouseTier::Tower => (40, 20, 60),
        logic::HouseTier::Arcology => (60, 20, 80),
    }
}

/// DayPhase 込みの House bg。夜間は土が黒く沈み、ガラス張りの Highrise も
/// 周囲の闇に溶ける (= 窓の灯りが浮かび上がる対比演出)。
fn house_bg(tier: logic::HouseTier, tick: u64) -> Color {
    let (r, g, b) = house_bg_rgb(tier);
    let dim = logic::day_phase(tick).dim_factor();
    let (r, g, b) = logic::dim_rgb(r, g, b, dim);
    Color::Rgb(r, g, b)
}

// ── Road auto-connect (Phase B) ─────────────────────────────
//
// 4-bit ビットマスク (N|E|S|W) で隣の Road の有無をエンコードし、
// box-drawing 文字を引く。同じテーブルで 1-wide / 2-wide 両方をサポート。

const ROAD_N: u8 = 1 << 0;
const ROAD_E: u8 = 1 << 1;
const ROAD_S: u8 = 1 << 2;
const ROAD_W: u8 = 1 << 3;

/// (x, y) の Road 周囲の Road 接続をビットマスクで返す。
/// 完成 Road / 建設中 Road の両方を「接続済み」とみなす — 建設中も
/// グリフが先に繋がって見えることで「道路網が育っていく」演出になる。
fn road_connections(state: &City, x: usize, y: usize) -> u8 {
    let mut mask = 0u8;
    let dirs: [(i32, i32, u8); 4] = [
        (0, -1, ROAD_N),
        (1, 0, ROAD_E),
        (0, 1, ROAD_S),
        (-1, 0, ROAD_W),
    ];
    for (dx, dy, bit) in dirs {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        match state.tile(nx as usize, ny as usize) {
            Tile::Built(Building::Road)
            | Tile::Construction {
                target: Building::Road,
                ..
            } => mask |= bit,
            _ => {}
        }
    }
    mask
}

/// 1-wide 用の box-drawing 1 文字。
fn road_1wide_glyph(mask: u8) -> char {
    match mask {
        0 => '+', // 孤立: 単発交差点記号
        ROAD_N => '╵',
        ROAD_E => '╶',
        ROAD_S => '╷',
        ROAD_W => '╴',
        m if m == ROAD_N | ROAD_S => '│',
        m if m == ROAD_E | ROAD_W => '─',
        m if m == ROAD_S | ROAD_E => '┌',
        m if m == ROAD_S | ROAD_W => '┐',
        m if m == ROAD_N | ROAD_E => '└',
        m if m == ROAD_N | ROAD_W => '┘',
        m if m == ROAD_N | ROAD_E | ROAD_S => '├',
        m if m == ROAD_N | ROAD_S | ROAD_W => '┤',
        m if m == ROAD_E | ROAD_S | ROAD_W => '┬',
        m if m == ROAD_N | ROAD_E | ROAD_W => '┴',
        _ => '┼', // 全方向
    }
}

/// 直線道路を「2 文字 × 明暗位相」で交通フローを演出する。
///
/// 戻り値は `(left_glyph, right_glyph, left_bright, right_bright)`。
///   - 水平 (`══`): 左右 2 文字のうちどちらが明るいかを tick で振る。
///     横位置 `x` を位相シードに使うと、車が左→右へ流れる錯覚が出る。
///   - 垂直 (`║`): 1 セル幅で左右のないグリフ (左 ` ` / 右 `║`)。
///     右側の明るさだけ y で位相を回す → 縦に光が下へ流れる。
///   - その他 (曲がり角・交差点・孤立): 位相なし固定。
fn road_traffic_glyph(mask: u8, x: usize, y: usize, tick: u64) -> RoadTraffic {
    let horizontal = mask == ROAD_E | ROAD_W || mask == ROAD_E || mask == ROAD_W;
    let vertical = mask == ROAD_N | ROAD_S || mask == ROAD_N || mask == ROAD_S;
    if horizontal {
        // 周期 4 で「光の塊」が x 軸を流れる: 4 ステップで 1 セル進む。
        let phase = ((tick / 2) as usize + x * 2) % 4;
        // 0..2 で左明るい、2..4 で右明るい (左→右の流れ)。
        let left_bright = phase < 2;
        RoadTraffic {
            left_bright,
            right_bright: !left_bright,
        }
    } else if vertical {
        // 縦方向: y 軸で位相、tick で進行方向 (上→下)。
        let phase = ((tick / 2) as usize + y * 2) % 4;
        let bright = phase < 2;
        RoadTraffic {
            left_bright: bright,
            right_bright: bright,
        }
    } else {
        // 曲がり角・交差点・孤立: チラつきを抑えるため固定明るさ。
        RoadTraffic {
            left_bright: false,
            right_bright: false,
        }
    }
}

#[derive(Clone, Copy)]
struct RoadTraffic {
    left_bright: bool,
    right_bright: bool,
}

/// 2-wide 道路を 2 つの Span に分割し、交通フローの明暗を per-character で適用。
fn road_spans_2wide(mask: u8, traffic: RoadTraffic, _tick: u64) -> Vec<Span<'static>> {
    let glyph = road_2wide_glyph(mask);
    let mut chars = glyph.chars();
    let left = chars.next().unwrap_or(' ');
    let right = chars.next().unwrap_or(' ');
    let bg = Color::Rgb(40, 40, 40);
    let make_style = |bright: bool| {
        let m = if bright {
            Modifier::BOLD
        } else {
            Modifier::DIM
        };
        let fg = if bright { Color::White } else { Color::Gray };
        Style::default().fg(fg).bg(bg).add_modifier(m)
    };
    vec![
        Span::styled(left.to_string(), make_style(traffic.left_bright)),
        Span::styled(right.to_string(), make_style(traffic.right_bright)),
    ]
}

/// 2-wide 用 (2 文字)。視覚的に「車線の幅」を持たせるため水平方向は 2 倍ストローク。
fn road_2wide_glyph(mask: u8) -> &'static str {
    match mask {
        0 => "╋╋", // 孤立
        ROAD_N => " ║",
        ROAD_E => "══",
        ROAD_S => " ║",
        ROAD_W => "══",
        m if m == ROAD_N | ROAD_S => " ║",
        m if m == ROAD_E | ROAD_W => "══",
        m if m == ROAD_S | ROAD_E => "╔═",
        m if m == ROAD_S | ROAD_W => "═╗",
        m if m == ROAD_N | ROAD_E => "╚═",
        m if m == ROAD_N | ROAD_W => "═╝",
        m if m == ROAD_N | ROAD_E | ROAD_S => "╠═",
        m if m == ROAD_N | ROAD_S | ROAD_W => "═╣",
        m if m == ROAD_E | ROAD_S | ROAD_W => "═╦",
        m if m == ROAD_N | ROAD_E | ROAD_W => "═╩",
        _ => "═╬",
    }
}

/// 2-wide House glyph: tier × level の組み合わせで 9 バリエーション。
///
/// Cottage は ▟▙ ベース (低い屋根)、Apartment は ▛▜ (中層シルエット)、
/// Highrise は ██ または ▌█ (摩天楼)。Level は窓の有無や形でニュアンスを足す。
fn house_glyph_2wide(tier: logic::HouseTier, level: logic::HouseLevel) -> &'static str {
    use logic::{HouseLevel, HouseTier};
    match (tier, level) {
        // Cottage: 一軒家系。隣接が増えると形が安定する。
        (HouseTier::Cottage, HouseLevel::Low) => "▟▙",
        (HouseTier::Cottage, HouseLevel::Mid) => "▙▟",
        (HouseTier::Cottage, HouseLevel::High) => "▛▜",
        // Apartment: 中層集合住宅。窓のある silhouette。
        (HouseTier::Apartment, HouseLevel::Low) => "▛▜",
        (HouseTier::Apartment, HouseLevel::Mid) => "▛▜",
        (HouseTier::Apartment, HouseLevel::High) => "▜▛",
        // Highrise: 摩天楼。密度が上がるほど隙間なく並ぶ。
        (HouseTier::Highrise, HouseLevel::Low) => "█▌",
        (HouseTier::Highrise, HouseLevel::Mid) => "▐█",
        (HouseTier::Highrise, HouseLevel::High) => "██",
        // Tower: 摩天楼が突き抜ける細い縦シルエット。
        (HouseTier::Tower, _) => "▌█",
        // Arcology: 自己完結都市 — ダイヤとフルブロックの組み合わせで
        // 「ひとつの建物が街区全体」感を出す。
        (HouseTier::Arcology, _) => "◆█",
    }
}

/// 2-wide House の色 + 太字モディファイア。
///
/// Tier ごとに色が明るくなり、夜間 (`tick / 60` で日中/夜間サイクル) は
/// Apartment / Highrise の窓が灯ったように LightYellow に切り替わる。
/// バナーの太陽 ◉/月 ◯ の往復 (周期 GRID_W * 60 ticks) と緩く同期。
fn house_style_2wide(tier: logic::HouseTier, tick: u64) -> (Color, Modifier) {
    use logic::{DayPhase, HouseTier};
    // 夜判定はバナーの太陽/月と同期させるため `day_phase` を使う。
    // Dusk は「灯りが付き始めた」状態として Night と同じ扱い (じわっと点灯)。
    let phase = logic::day_phase(tick);
    let is_lit = matches!(phase, DayPhase::Dusk | DayPhase::Night);
    let bright = !(tick / 10).is_multiple_of(4);
    let modifier = if bright {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };
    let color = match (tier, is_lit) {
        (HouseTier::Cottage, _) => Color::Green,
        (HouseTier::Apartment, false) => Color::LightGreen,
        (HouseTier::Apartment, true) => Color::LightYellow, // 夜の窓灯り
        (HouseTier::Highrise, false) => Color::LightCyan,
        (HouseTier::Highrise, true) => Color::Yellow, // 夜のネオン感
        (HouseTier::Tower, false) => Color::LightMagenta,
        (HouseTier::Tower, true) => Color::White, // 夜のタワー全体ライトアップ
        (HouseTier::Arcology, false) => Color::Magenta,
        (HouseTier::Arcology, true) => Color::LightMagenta,
    };
    (color, modifier)
}

fn construction_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Yellow,
        Building::House => Color::LightGreen,
        Building::Workshop => Color::LightRed,
        Building::Factory => Color::Red,
        Building::Refinery => Color::LightRed,
        Building::Shop => Color::LightCyan,
        Building::Mall => Color::LightYellow,
        Building::MegaMall => Color::LightYellow,
        Building::Office => Color::LightCyan,
        Building::Headquarters => Color::White,
        Building::Park => Color::LightGreen,
        Building::Plaza => Color::LightMagenta,
        Building::Stadium => Color::LightYellow,
        Building::Outpost => Color::LightYellow,
    }
}

fn built_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Gray,
        Building::House => Color::Green,
        Building::Workshop => Color::LightRed,
        Building::Factory => Color::Red,
        Building::Refinery => Color::LightRed,
        Building::Shop => Color::Yellow,
        Building::Mall => Color::LightYellow,
        Building::MegaMall => Color::LightYellow,
        Building::Office => Color::LightCyan,
        Building::Headquarters => Color::White,
        Building::Park => Color::LightGreen,
        Building::Plaza => Color::LightMagenta,
        Building::Stadium => Color::LightYellow,
        Building::Outpost => Color::LightYellow,
    }
}

fn built_2wide_glyph(b: Building) -> &'static str {
    match b {
        Building::Road => "══",
        Building::House => "▟▙",
        Building::Workshop => "˚⊞",
        Building::Factory => "▆▣",
        Building::Refinery => "█▣",
        Building::Shop => "$$",
        Building::Mall => "★$",
        Building::MegaMall => "☆★",
        Building::Office => "▮▮",
        Building::Headquarters => "▮▣",
        Building::Park => "❀✿",
        Building::Plaza => "◈✿",
        Building::Stadium => "◎▣",
        Building::Outpost => "◐⚒",
    }
}

// ── Terrain rendering ───────────────────────────────────────
//
// Empty セル上に地形を描画する。Forest と Water は時間でゆらぎ、
// 「生きているマップ」感を出す。

/// 地形の背景色パレット (rebels-in-the-sky 流の「塊で見せる」表現)。
///
/// 全タイルに `bg(Color)` を入れることで、ASCII グリフの集合が
/// 「色塗りされた地図」に化ける。前景色とコントラストが取れる組み合わせを選ぶ。
/// 地形の元 RGB (昼間)。`terrain_bg` は dim 適用後を返すラッパ。
fn terrain_bg_rgb(t: Terrain) -> (u8, u8, u8) {
    match t {
        // 平地: 暗い緑 (草原)。pure black では味気ないので Rgb 化して
        // 夜間に dim 可能にする。
        Terrain::Plain => (18, 28, 14),
        // 森: 濃い緑のキャンバス + 明るい緑のグリフ。
        Terrain::Forest => (15, 50, 25),
        // 湖: 深い青の水面 + シアンの波。
        Terrain::Water => (15, 35, 80),
        // 荒地: 茶色の砂地 + 暗い黄の点。
        Terrain::Wasteland => (70, 50, 25),
        // 岩盤: 暗い灰色 (花崗岩感)。Wasteland とは違う「硬い」色味で
        // パッと見で「ここは特殊地形」と分かるようにする。
        Terrain::Rock => (60, 55, 50),
    }
}

/// DayPhase を反映した terrain bg。すべての地形描画で使う。
/// 夜間に bg が暗くなる (Plain は暗緑→ほぼ黒、Water は深い藍に変化)。
fn terrain_bg_at(t: Terrain, tick: u64) -> Color {
    let (r, g, b) = terrain_bg_rgb(t);
    let dim = logic::day_phase(tick).dim_factor();
    let (r, g, b) = logic::dim_rgb(r, g, b, dim);
    Color::Rgb(r, g, b)
}

fn terrain_span_1(t: Terrain, x: usize, y: usize, tick: u64) -> Span<'static> {
    let bg = terrain_bg_at(t, tick);
    match t {
        Terrain::Plain => {
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "·" } else { " " };
            Span::styled(
                g.to_string(),
                Style::default().fg(Color::DarkGray).bg(bg),
            )
        }
        Terrain::Forest => {
            let sway = ((tick / 8) as usize + x + y).is_multiple_of(3);
            let g = if sway { "♣" } else { "♠" };
            Span::styled(
                g.to_string(),
                Style::default().fg(Color::LightGreen).bg(bg),
            )
        }
        Terrain::Water => {
            let wave = ((tick / 4) as usize + x + y) % 3;
            let g = match wave {
                0 => "~",
                1 => "≈",
                _ => "˜",
            };
            Span::styled(
                g.to_string(),
                Style::default().fg(Color::LightCyan).bg(bg),
            )
        }
        Terrain::Wasteland => Span::styled(
            ":".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .bg(bg)
                .add_modifier(Modifier::DIM),
        ),
        Terrain::Rock => {
            // 岩盤: 1-wide では座標で固定の凹凸記号を出して「ゴツゴツ」感を表現。
            // 動かさない (チラつき防止)。
            let g = match (x + y * 3) % 4 {
                0 => '▲',
                1 => '◆',
                2 => '▼',
                _ => '■',
            };
            Span::styled(
                g.to_string(),
                Style::default()
                    .fg(Color::Gray)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )
        }
    }
}

fn terrain_spans_2(t: Terrain, x: usize, y: usize, tick: u64) -> Vec<Span<'static>> {
    let bg = terrain_bg_at(t, tick);
    match t {
        Terrain::Plain => {
            let phase = (x + y).is_multiple_of(2);
            let g = if phase { "· " } else { "  " };
            vec![Span::styled(
                g.to_string(),
                Style::default().fg(Color::DarkGray).bg(bg),
            )]
        }
        Terrain::Forest => {
            let sway = ((tick / 8) as usize + x + y).is_multiple_of(3);
            let g = if sway { "♣♣" } else { "♠♣" };
            vec![Span::styled(
                g.to_string(),
                Style::default().fg(Color::LightGreen).bg(bg),
            )]
        }
        Terrain::Water => {
            let wave = ((tick / 4) as usize + x + y) % 3;
            let g = match wave {
                0 => "~~",
                1 => "≈≈",
                _ => "~≈",
            };
            vec![Span::styled(
                g.to_string(),
                Style::default().fg(Color::LightCyan).bg(bg),
            )]
        }
        Terrain::Wasteland => vec![Span::styled(
            "::".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .bg(bg)
                .add_modifier(Modifier::DIM),
        )],
        Terrain::Rock => {
            // 岩盤 2-wide: 凹凸感のあるグリフ 2 文字。座標で固定 (動かさない)。
            // 「壁」感を強くするため Modifier::BOLD でくっきり描画。
            let pattern = match (x + y * 3) % 4 {
                0 => "▲◆",
                1 => "◆▲",
                2 => "▼■",
                _ => "■▼",
            };
            vec![Span::styled(
                pattern.to_string(),
                Style::default()
                    .fg(Color::Gray)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )]
        }
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
            )
            .tab(
                format!("5 {}", PanelTab::Catalog.label()),
                tab_style(state.panel_tab == PanelTab::Catalog),
                ACT_TAB_CATALOG,
            );
        bar.render(f, v[0], &mut cs);
    }

    // 5 タブを共通の `ScrollableTab` primitive に乗せる。Cafe / Abyss と
    // 同じスクロール挙動 (overflow 時のみ ▲▼ 列を予約 / clamp の自動書き戻し)
    // を共有することで、game ごとに scroll 実装を再発明しない。
    let list = match state.panel_tab {
        PanelTab::Status => status_list(state),
        PanelTab::Manager => manager_list(state),
        PanelTab::Events => log_list(state),
        PanelTab::World => world_list(state),
        PanelTab::Catalog => catalog_list(state),
    };
    // スマホでパネル領域内のスワイプを panel scroll (J/K) に振り分けるため、
    // 現在のパネル content 領域を window.metropolisPanelRect に export する。
    // index.html の touch/wheel ハンドラがこの値を見て swipe キーを切り替える。
    export_panel_rect_to_js(v[1]);
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        list,
        &state.panel_scroll,
        ACT_PANEL_SCROLL_UP,
        ACT_PANEL_SCROLL_DOWN,
    )
    .render(f, v[1], &mut cs);
}

/// パネル領域 (cell 座標) と「rect の鮮度」を `window.metropolisPanelRect`
/// に export する。index.html の touch/wheel ハンドラが「タッチ開始位置が
/// この矩形内か」を判定して、パネル内なら J/K (panel scroll)、それ以外なら
/// j/k (viewport scroll) を dispatch する。
///
/// `updatedAt` (= `performance.now()` ミリ秒) を載せることで、JS 側は
/// 「最後の更新から N ms 以上経った rect」を stale とみなし無視できる。
/// これがないと metropolis から他ゲーム (cafe / abyss など) に切り替えた
/// 後も古い rect が残り、その範囲のスワイプが J/K に化けて他ゲームの
/// scrolling を奪う。metropolis 自身は毎フレーム render するので fresh
/// な rect が継続更新される。
#[cfg(target_arch = "wasm32")]
fn export_panel_rect_to_js(area: Rect) {
    use js_sys::{Object, Reflect};
    use web_sys::wasm_bindgen::JsValue;
    let Some(win) = web_sys::window() else { return };
    let now_ms = win
        .performance()
        .map(|p| p.now())
        .unwrap_or(0.0);
    let obj = Object::new();
    let _ = Reflect::set(&obj, &"x".into(), &JsValue::from(area.x));
    let _ = Reflect::set(&obj, &"y".into(), &JsValue::from(area.y));
    let _ = Reflect::set(&obj, &"w".into(), &JsValue::from(area.width));
    let _ = Reflect::set(&obj, &"h".into(), &JsValue::from(area.height));
    let _ = Reflect::set(&obj, &"updatedAt".into(), &JsValue::from(now_ms));
    let _ = Reflect::set(&win, &"metropolisPanelRect".into(), &obj);
}

#[cfg(not(target_arch = "wasm32"))]
fn export_panel_rect_to_js(_area: Rect) {}

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

fn world_list(state: &City) -> ClickableList<'static> {
    let mut counts = [0u32; 5];
    for row in &state.terrain {
        for t in row {
            match t {
                Terrain::Plain => counts[0] += 1,
                Terrain::Forest => counts[1] += 1,
                Terrain::Water => counts[2] += 1,
                Terrain::Wasteland => counts[3] += 1,
                Terrain::Rock => counts[4] += 1,
            }
        }
    }
    let total = (GRID_W * GRID_H).max(1) as u32;
    let pct = |c: u32| (c * 100) / total;

    let lines: Vec<Line<'static>> = vec![
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
        Line::from(vec![
            Span::styled("Rock   ▲  ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:>3}%", pct(counts[4])), Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "湖は建設不可。岩盤は機材で開拓。",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let mut cl = ClickableList::new();
    for line in lines {
        cl.push(line);
    }
    cl
}

// ── Catalog panel (建物図鑑) ─────────────────────────────────
//
// 全 Building 種類と HouseTier を一覧表示する読み取り専用パネル。
// プレイヤーが「次のフェーズで何が建つのか」「今の街区はどの段階か」を
// 1 画面で把握できることが目的。クリックターゲットは登録しない (純粋な
// 情報パネル)。

/// 建物の役割サマリ (1 行)。Catalog タブに直接表示する。
fn building_role(b: Building) -> &'static str {
    match b {
        Building::Road => "接続インフラ。マップ外と物流を繋ぎ、商業/雇用の活性条件を解く。",
        Building::House => "人口供給。周辺の経済充実度で Cottage→Arcology まで段階進化。",
        Building::Workshop => "基礎雇用。隣接 House + Road 接続で活性。Apartment 化の触媒。",
        Building::Factory => "重工業。Workshop の 3.5 倍雇用。隣接 House に煙害デバフ。",
        Building::Refinery => "重工業の頂点。Factory の 2.5 倍雇用。煙害は半径 2 タイルに拡大。",
        Building::Shop => "基礎商業。距離 3 以内の House を客にする小規模店舗。",
        Building::Mall => "大型商業。Shop の 3 倍商業キャパ。Apartment 以上の街区で本領発揮。",
        Building::MegaMall => "商業メガ。Mall の 2.5 倍。Tower 化触媒で終盤の主力商業。",
        Building::Office => "ホワイトカラー雇用。周辺 House を Highrise 化する触媒。",
        Building::Headquarters => "本社ビル。Office の 2.8 倍雇用。Tower 化を直接駆動する終盤触媒。",
        Building::Park => "文化触媒。直接収入なし。Highrise 化の文化需要を担う。道路接続不要。",
        Building::Plaza => "中央広場。Park の 3 倍の文化触媒。Tower 化のサポート条件。",
        Building::Stadium => "競技場。文化メガ施設。Arcology 化の必須条件。最も建設に時間が掛かる。",
        Building::Outpost => "開拓機材。隣接 Rock を整地可能にする。Rock 解禁後は撤去候補。",
    }
}

/// HouseTier の要約 (条件 + 寄与人口 + 家賃)。
fn house_tier_summary(tier: logic::HouseTier) -> (&'static str, &'static str, u32, i64) {
    let (name, cond) = match tier {
        logic::HouseTier::Cottage => ("Cottage", "デフォルト。インフラ未整備でも住める一軒家。"),
        logic::HouseTier::Apartment => (
            "Apartment",
            "Road 接続 + 経済密度 ≥ 1。築 60 sec で昇格可能。",
        ),
        logic::HouseTier::Highrise => (
            "Highrise",
            "Road 2 本以上 + 経済密度 ≥ 2 + 周囲 House ≥ 3。築 5 min。",
        ),
        logic::HouseTier::Tower => (
            "Tower",
            "Highrise 条件 + MegaMall または Headquarters が近接。築 10 min。",
        ),
        logic::HouseTier::Arcology => (
            "Arcology",
            "Tower 条件 + Stadium が近接 + Road 3 本以上。築 15 min — 最終段階。",
        ),
    };
    let cap = logic::house_capacity(tier);
    let rent_cents: i64 = match tier {
        logic::HouseTier::Cottage => 50,
        logic::HouseTier::Apartment => 150,
        logic::HouseTier::Highrise => 300,
        logic::HouseTier::Tower => 600,
        logic::HouseTier::Arcology => 1_200,
    };
    (name, cond, cap, rent_cents)
}

fn catalog_list(_state: &City) -> ClickableList<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(Line::from(Span::styled(
        "建物図鑑 — 全 14 種",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for b in Building::ALL {
        let cost = b.cost();
        let ticks = b.build_ticks();
        let secs = (ticks + 5) / 10; // 10 ticks/sec、四捨五入相当
        // 行1: アイコン + 名前 + コスト + 建設時間
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", building_icon(*b)),
                Style::default(),
            ),
            Span::styled(
                logic::building_display_name(*b).to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ${}", cost),
                Style::default().fg(Color::LightYellow),
            ),
            Span::styled(
                format!("  ⏱{}s", secs),
                Style::default().fg(Color::Gray),
            ),
        ]));
        // 行2: 役割サマリ (薄色)
        lines.push(Line::from(Span::styled(
            format!("  {}", building_role(*b)),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "住宅 Tier — 周辺の経済充実度で自動進化",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let tiers = [
        logic::HouseTier::Cottage,
        logic::HouseTier::Apartment,
        logic::HouseTier::Highrise,
        logic::HouseTier::Tower,
        logic::HouseTier::Arcology,
    ];
    for t in tiers {
        let (name, cond, cap, rent_cents) = house_tier_summary(t);
        lines.push(Line::from(vec![
            Span::styled(
                "  ",
                Style::default(),
            ),
            Span::styled(
                name.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  人口{:>3}", cap),
                Style::default().fg(Color::LightGreen),
            ),
            Span::styled(
                format!("  家賃${:.1}/s", rent_cents as f32 / 100.0),
                Style::default().fg(Color::LightYellow),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("    {}", cond),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let mut cl = ClickableList::new();
    for line in lines {
        cl.push(line);
    }
    cl
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

fn status_list(state: &City) -> ClickableList<'static> {
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

    // 10s ROI 行 — 撤去 / 建設コストも含む実効キャッシュ増減レート。
    // 理論 income (`+$X/s`) との差で「thrash でいくら失っているか」が見える。
    // サンプル不足の起動直後は "—" を出して情報の有無を区別。窓は最大 10 秒で
    // それ未満のサンプルしか無い時はその範囲の平均を出す ("≤10s avg")。
    let roi_cents = state.cash_flow_per_sec_cents(10);
    let income_cents = income.saturating_mul(100);
    let (roi_text, roi_color) = match roi_cents {
        Some(v) if v >= income_cents => (format!("+${}/s", v / 100), Color::LightGreen),
        Some(v) if v >= 0 => (format!("+${}/s", v / 100), Color::Yellow),
        Some(v) => (format!("-${}/s", v.unsigned_abs() / 100), Color::LightRed),
        None => ("—".to_string(), Color::DarkGray),
    };
    lines.push(Line::from(vec![
        Span::styled("ROI  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            roi_text,
            Style::default().fg(roi_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (≤10s avg)",
            Style::default().fg(Color::DarkGray),
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
    lines.push(Line::from(worker_bar_spans(state)));

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

    // 区切り + Strategy 内訳パネル。
    // 「マネージャーが今 CPU に何をやらせているか」を可視化することで、
    // ボタンを切り替えた時の効果が即座に見える。
    lines.push(Line::from(""));
    lines.extend(strategy_status_lines(state));

    // 選択中セルの詳細パネル (タップ / クリックで表示)。
    if let Some((sx, sy)) = state.selected_cell {
        lines.push(Line::from(""));
        lines.extend(selected_cell_lines(state, sx, sy));
    }

    // 区切り + ワーカー稼働状況 (誰が何を建てているか)。
    lines.push(Line::from(""));
    lines.extend(worker_status_lines(state));

    let mut cl = ClickableList::new();
    for line in lines {
        cl.push(line);
    }
    cl
}

/// cents/sec を `$X.XX/s` 形式の文字列にする。
fn format_cents_per_sec(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs() as i64;
    format!("{}${}.{:02}/s", sign, abs / 100, abs % 100)
}

/// (x, y) を中心とした Manhattan 半径 `radius` 内の Built House 数。
fn count_houses_within(state: &City, x: usize, y: usize, radius: i32) -> u32 {
    let mut count = 0u32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            if matches!(
                state.tile(nx as usize, ny as usize),
                Tile::Built(Building::House)
            ) {
                count += 1;
            }
        }
    }
    count
}

/// 商業/雇用建物が停止中の理由を 1 行で出す。
///
/// `is_workshop` が true なら Workshop/Factory/Office (隣接 House + 道路接続)、
/// false なら Shop/Mall (道路接続 + 半径3 House) を判定する。
fn inactive_reason_lines(
    state: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
    is_workshop: bool,
) -> Vec<Line<'static>> {
    let road_ok = logic::is_building_edge_connected(connected, x, y);
    let demand_ok = if is_workshop {
        logic::has_neighbor_kind(state, x, y, Building::House)
    } else {
        count_houses_within(state, x, y, 3) > 0
    };
    let reason = match (road_ok, demand_ok) {
        (false, false) => " ✗ 幹線道路と近隣 House が両方不足",
        (false, true) => " ✗ 幹線道路に未接続",
        (true, false) if is_workshop => " ✗ 隣接 House が無く労働力ゼロ",
        (true, false) => " ✗ 半径3 以内に House が無い",
        (true, true) => return Vec::new(),
    };
    vec![Line::from(vec![Span::styled(
        reason,
        Style::default().fg(Color::LightRed),
    )])]
}

/// 選択中セルの詳細を Status パネルに表示する。
///
/// **見せる情報** (Cookie Factory 等の Pure Logic Pattern を踏襲):
///   - セル種別 (Empty / Construction / Built / Clearing) と建物名 / 地形
///   - 建物の役割説明 (何のためにあるか)
///   - 建物個別の状態 (House の Tier、Shop の賑わい、Workshop の活性 など)
///   - 推定 per-tile 収入 ($/sec) と上限キャパシティ
///   - 停止中なら停止理由 (道路未接続 / 隣接 House なし)
///   - 道路接続状況 / 築年数
///
/// プレイヤーが「なぜここの House が Highrise にならないのか?」「この
/// Workshop がいくら稼いでいるのか?」を読み解く学習の入り口になる。
fn selected_cell_lines(state: &City, x: usize, y: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    out.push(Line::from(vec![Span::styled(
        format!("📍 SELECTED ({},{})", x, y),
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    )]));

    // **edge connectivity を 1 度だけ計算** して下の各 building branch に流す
    // House/Shop/Workshop 系の active 判定で BFS を共有する。tick 境界毎にしか
    // BFS を走らせない `cached_edge_connected_roads` を使うことで、render が
    // 60 FPS で同 BFS を繰り返さずに済む。
    let connected = logic::cached_edge_connected_roads(state);
    // 収入按分の参照テーブル。frame 毎に呼ばれる描画パスなので、収入表示に
    // 必要な branch (House / Shop / Mall / Workshop / Factory / Office) で
    // 初回アクセス時にだけ作る lazy cache。Road/Park/Outpost/空き地選択時は
    // `compute_population_map` のフルグリッドパスをスキップする。
    let mut pop_map_cache: Option<Vec<Vec<u32>>> = None;

    // 1 行目: タイル種別
    let kind_label: String = match state.tile(x, y) {
        Tile::Empty => "空き地".to_string(),
        Tile::Clearing {
            ticks_remaining, ..
        } => format!("整地中 (残 {}s)", ticks_remaining.div_ceil(10)),
        Tile::Construction {
            target,
            ticks_remaining,
        } => format!(
            "{} 建設中 (残 {}s)",
            building_name_for(*target),
            ticks_remaining.div_ceil(10)
        ),
        Tile::Built(b) => building_name_for(*b).to_string(),
    };
    let terrain_label = terrain_name_for(state.terrain[y][x]);
    out.push(Line::from(vec![
        Span::styled(" 種別 ", Style::default().fg(Color::DarkGray)),
        Span::styled(kind_label, Style::default().fg(Color::White)),
        Span::styled(" / 地形 ", Style::default().fg(Color::DarkGray)),
        Span::styled(terrain_label.to_string(), Style::default().fg(Color::Gray)),
    ]));

    // 建物個別の詳細 (BFS 共有版を使う)
    if let Tile::Built(b) = state.tile(x, y) {
        match b {
            Building::House => {
                let tier = logic::effective_tier_at_with(state, x, y, &connected);
                let stats = logic::gather_house_neighborhood_with(state, x, y, &connected);
                let target_tier = logic::house_tier_for(stats);
                let cap = logic::house_capacity(tier);
                out.push(Line::from(vec![
                    Span::styled(" 段階 ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:?}", tier),
                        Style::default().fg(Color::LightGreen),
                    ),
                    Span::styled(
                        format!(" 定員{}人 → 目標 {:?}", cap, target_tier),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                out.push(Line::from(vec![Span::styled(
                    " 効果: 住人を供給 (周辺の Shop / Workshop / Office を活性化)".to_string(),
                    Style::default().fg(Color::DarkGray),
                )]));
                out.push(Line::from(vec![Span::styled(
                    format!(
                        " 道路{}/工房{}/店舗{}/職場{}/家{}/公園{} {}",
                        stats.n_road_adj,
                        stats.n_workshop_within_5,
                        stats.n_shop_within_5,
                        stats.n_office_within_5,
                        stats.n_house_within_3,
                        stats.n_park_within_4,
                        if stats.edge_connected { "🌐" } else { "🚷" },
                    ),
                    Style::default().fg(Color::DarkGray),
                )]));
                if stats.factory_smoke_penalty {
                    out.push(Line::from(vec![Span::styled(
                        " ⚠ 隣接 Factory の煙害で Tier -1",
                        Style::default().fg(Color::LightRed),
                    )]));
                }
                if !stats.edge_connected && matches!(tier, logic::HouseTier::Cottage) {
                    out.push(Line::from(vec![Span::styled(
                        " ⚠ 道路未接続: 家賃が半減",
                        Style::default().fg(Color::LightYellow),
                    )]));
                }
                out.push(Line::from(vec![Span::styled(
                    format!(
                        " 周辺人口 {}人 (需給ゲート閾値 +{})",
                        stats.local_population,
                        stats.local_population / 30,
                    ),
                    Style::default().fg(Color::DarkGray),
                )]));
                let pop_map = pop_map_cache
                    .get_or_insert_with(|| logic::compute_population_map(state, &connected));
                let rent = logic::tile_income_cents_with(state, x, y, pop_map, &connected);
                out.push(Line::from(vec![
                    Span::styled(" 家賃 ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format_cents_per_sec(rent),
                        Style::default().fg(Color::LightGreen),
                    ),
                ]));
            }
            Building::Shop | Building::Mall | Building::MegaMall => {
                let level = logic::shop_level_with(state, x, y, &connected);
                let active = logic::shop_is_active_with(state, x, y, &connected);
                let cap_cents = match b {
                    Building::Shop => logic::SHOP_CAPACITY_CENTS,
                    Building::Mall => logic::MALL_CAPACITY_CENTS,
                    _ => logic::MEGAMALL_CAPACITY_CENTS,
                };
                out.push(Line::from(vec![
                    Span::styled(" 賑わい ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:?}", level),
                        Style::default().fg(if active { Color::Yellow } else { Color::DarkGray }),
                    ),
                    Span::styled(
                        format!(" (上限 {})", format_cents_per_sec(cap_cents)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                let role = match b {
                    Building::Shop => " 効果: 半径3 House の購買力を商品で受ける",
                    Building::Mall => " 効果: 大型商業 (Shop の約3倍キャパ・Apartment/Highrise 向け)",
                    _ => " 効果: 商業メガ (Mall の約2.5倍キャパ・Tower 化触媒)",
                };
                out.push(Line::from(vec![Span::styled(
                    role,
                    Style::default().fg(Color::DarkGray),
                )]));
                if active {
                    let pop_map = pop_map_cache
                        .get_or_insert_with(|| logic::compute_population_map(state, &connected));
                    let income = logic::tile_income_cents_with(state, x, y, pop_map, &connected);
                    let customers = count_houses_within(state, x, y, 3);
                    out.push(Line::from(vec![
                        Span::styled(" 収入 ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format_cents_per_sec(income),
                            Style::default().fg(Color::LightGreen),
                        ),
                        Span::styled(
                            format!(" / 客圏 House {}軒", customers),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                } else {
                    out.extend(inactive_reason_lines(state, x, y, &connected, false));
                }
            }
            Building::Workshop
            | Building::Factory
            | Building::Refinery
            | Building::Office
            | Building::Headquarters => {
                let active = logic::workshop_is_active_with(state, x, y, &connected);
                let (cap_cents, role) = match b {
                    Building::Workshop => (
                        logic::WORKSHOP_CAPACITY_CENTS,
                        " 効果: 工業雇用を供給 (近隣 House の労働需要を吸収)",
                    ),
                    Building::Factory => (
                        logic::FACTORY_CAPACITY_CENTS,
                        " 効果: 工業雇用を大量供給 / 隣接 House の Tier -1 (煙害)",
                    ),
                    Building::Refinery => (
                        logic::REFINERY_CAPACITY_CENTS,
                        " 効果: 重工業の頂点 (Factory の約2.5倍) / 半径2 House の Tier -1",
                    ),
                    Building::Office => (
                        logic::OFFICE_CAPACITY_CENTS,
                        " 効果: ホワイトカラー雇用を供給 / Highrise 化を促進",
                    ),
                    _ => (
                        logic::HEADQUARTERS_CAPACITY_CENTS,
                        " 効果: 本社ビル (Office の約2.8倍) / Tower 化触媒",
                    ),
                };
                out.push(Line::from(vec![
                    Span::styled(" 稼働 ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        if active { "稼働中" } else { "停止中" },
                        Style::default().fg(if active {
                            Color::LightRed
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(
                        format!(" (上限 {})", format_cents_per_sec(cap_cents)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
                out.push(Line::from(vec![Span::styled(
                    role,
                    Style::default().fg(Color::DarkGray),
                )]));
                if active {
                    let pop_map = pop_map_cache
                        .get_or_insert_with(|| logic::compute_population_map(state, &connected));
                    let income = logic::tile_income_cents_with(state, x, y, pop_map, &connected);
                    out.push(Line::from(vec![
                        Span::styled(" 収入 ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format_cents_per_sec(income),
                            Style::default().fg(Color::LightGreen),
                        ),
                    ]));
                } else {
                    out.extend(inactive_reason_lines(state, x, y, &connected, true));
                }
            }
            Building::Park => {
                out.push(Line::from(vec![Span::styled(
                    " 効果: 半径4 の文化触媒 (Highrise 化条件)",
                    Style::default().fg(Color::LightGreen),
                )]));
                out.push(Line::from(vec![Span::styled(
                    " 直接収入なし / 道路接続不要",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            Building::Plaza => {
                out.push(Line::from(vec![Span::styled(
                    " 効果: Park の3倍の文化触媒 / Tower 化サポート条件",
                    Style::default().fg(Color::LightMagenta),
                )]));
                out.push(Line::from(vec![Span::styled(
                    " 直接収入なし / 道路接続不要",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            Building::Stadium => {
                out.push(Line::from(vec![Span::styled(
                    " 効果: 文化メガ施設 / Arcology 化の必須条件",
                    Style::default().fg(Color::LightYellow),
                )]));
                out.push(Line::from(vec![Span::styled(
                    " 直接収入なし / 道路接続不要 / 半径5 で Tier 触媒",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            Building::Road => {
                let edge_connected = connected[y][x];
                out.push(Line::from(vec![
                    Span::styled(" 幹線網 ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        if edge_connected { "接続 ✓" } else { "未接続 ✗" },
                        Style::default().fg(if edge_connected {
                            Color::LightGreen
                        } else {
                            Color::LightYellow
                        }),
                    ),
                ]));
                out.push(Line::from(vec![Span::styled(
                    " 効果: 隣接 Shop / Workshop / Factory / Office を活性化",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
            Building::Outpost => {
                let n_rock = (0..4)
                    .filter(|i| {
                        let (dx, dy) = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)][*i];
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                            return false;
                        }
                        matches!(
                            state.terrain[ny as usize][nx as usize],
                            super::terrain::Terrain::Rock
                        )
                    })
                    .count();
                out.push(Line::from(vec![
                    Span::styled(" 隣接岩盤 ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} 残", n_rock),
                        Style::default().fg(Color::LightYellow),
                    ),
                ]));
                out.push(Line::from(vec![Span::styled(
                    " 効果: 隣接 Rock を整地可能にする (直接収入なし)",
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }
    }

    // 築年数 (Built タイルのみ)
    if matches!(state.tile(x, y), Tile::Built(_)) && state.built_at_tick[y][x] > 0 {
        let age = state.tick.saturating_sub(state.built_at_tick[y][x]);
        out.push(Line::from(vec![
            Span::styled(" 築年 ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}s", age / 10),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    out
}

/// ワーカー一覧を 1 行/人で出力する。
///
/// 各 Construction / Clearing タイルを行優先で列挙し W1, W2, ... と連番を
/// 振る。残りの (workers - busy) は「待機中」として列挙する。
/// プレイヤー視点で「ワーカーが何の作業をやっているか」が一目で分かる。
fn worker_status_lines(state: &City) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    out.push(Line::from(vec![Span::styled(
        format!(
            "WORKERS ({}/{} 稼働)",
            state.active_constructions(),
            state.workers
        ),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));

    let mut idx: u32 = 0;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let line = match state.tile(x, y) {
                Tile::Construction {
                    target,
                    ticks_remaining,
                } => {
                    idx += 1;
                    let icon = building_icon(*target);
                    let secs = (*ticks_remaining).div_ceil(10);
                    Some(Line::from(vec![
                        Span::styled(
                            format!(" W{} ", idx),
                            Style::default()
                                .fg(Color::LightYellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} {}", icon, building_name_for(*target)),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(" ({},{}) ", x, y),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!("⏱{}s", secs),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]))
                }
                Tile::Clearing {
                    ticks_remaining, ..
                } => {
                    idx += 1;
                    let secs = (*ticks_remaining).div_ceil(10);
                    let terrain = state.terrain[y][x];
                    Some(Line::from(vec![
                        Span::styled(
                            format!(" W{} ", idx),
                            Style::default()
                                .fg(Color::LightYellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("⛏ 整地 ({})", terrain_name_for(terrain)),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(" ({},{}) ", x, y),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!("⏱{}s", secs),
                            Style::default().fg(Color::Cyan),
                        ),
                    ]))
                }
                _ => None,
            };
            if let Some(l) = line {
                out.push(l);
            }
        }
    }

    // 待機中ワーカーを idx の続きで列挙。
    let idle = state.workers.saturating_sub(idx);
    for _ in 0..idle {
        idx += 1;
        out.push(Line::from(vec![
            Span::styled(
                format!(" W{} ", idx),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                "待機中",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    out
}

/// 名称は logic.rs の `building_display_name` に集約済み。
/// render 側は薄いラッパーで参照する (重複定義の同期忘れを避けるため)。
fn building_name_for(b: Building) -> &'static str {
    logic::building_display_name(b)
}

fn terrain_name_for(t: Terrain) -> &'static str {
    match t {
        Terrain::Plain => "平地",
        Terrain::Forest => "森",
        Terrain::Wasteland => "荒地",
        Terrain::Water => "湖",
        Terrain::Rock => "岩盤",
    }
}

/// 建物の絵文字アイコン (ワーカー一覧表示用 / 図鑑表示用)。
fn building_icon(b: Building) -> &'static str {
    match b {
        Building::Road => "🛣",
        Building::House => "🏠",
        Building::Workshop => "🔧",
        Building::Factory => "🏭",
        Building::Refinery => "⛽",
        Building::Shop => "🏪",
        Building::Mall => "🏬",
        Building::MegaMall => "🛍",
        Building::Office => "🏢",
        Building::Headquarters => "🏛",
        Building::Park => "🌳",
        Building::Plaza => "🎡",
        Building::Stadium => "🏟",
        Building::Outpost => "⚒",
    }
}

/// 現在の Strategy の内訳を表示する 4 行ブロック。
///   行1: ラベル + 速度/収入修正
///   行2: 建物別の roll 確率
///   行3: 建物別の確率を 1 行のバーで描画 (H██ R▓▓ W░ S██)
///   行4: タグライン (1 行説明)
fn strategy_status_lines(state: &City) -> Vec<Line<'static>> {
    let info = logic::strategy_info(state.strategy);
    let mut out: Vec<Line> = Vec::new();

    // 行1: 戦略ラベル + 副作用。
    let mut head = vec![
        Span::styled("STRAT ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            info.label.to_string(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ];
    if info.speed_bonus_pct != 0 {
        head.push(Span::styled(
            format!("  建設{:+}%", info.speed_bonus_pct),
            Style::default().fg(Color::LightGreen),
        ));
    }
    if info.income_penalty_pct != 0 {
        head.push(Span::styled(
            format!("  収入{:+}%", info.income_penalty_pct),
            Style::default().fg(Color::LightRed),
        ));
    }
    out.push(Line::from(head));

    // 行2: 建物別重みの数値。
    out.push(Line::from(vec![
        Span::styled(
            format!(" 家{:>2}% ", info.house_pct),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!("道{:>2}% ", info.road_pct),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("工{:>2}% ", info.workshop_pct),
            Style::default().fg(Color::LightRed),
        ),
        Span::styled(
            format!("店{:>2}%", info.shop_pct),
            Style::default().fg(Color::Yellow),
        ),
    ]));

    // 行3: 重みを横バーで可視化。色付きの ▰ で 100% を 20 セルに展開。
    out.push(Line::from(strategy_weight_bar(&info)));

    // 行4: 1 行の意図説明 (タグライン)。
    out.push(Line::from(vec![Span::styled(
        format!(" {}", info.tagline),
        Style::default().fg(Color::DarkGray),
    )]));

    out
}

/// 建物別重みをカラフルな 1 行バーに変換。合計 20 セル幅。
/// 各 Strategy の特性が塊として見えるので、切り替え時に即印象が変わる。
fn strategy_weight_bar(info: &logic::StrategyInfo) -> Vec<Span<'static>> {
    const BAR_WIDTH: u32 = 20;
    let segs: [(u32, Color); 4] = [
        (info.house_pct, Color::Green),
        (info.road_pct, Color::Gray),
        (info.workshop_pct, Color::LightRed),
        (info.shop_pct, Color::Yellow),
    ];
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (pct, color) in segs {
        // 整数除算: 0% は 0 セル、99% は 19 セル。表示の都合で 1% 以上なら最低 1 セル
        // 出したいが、合計が 20 を超えないよう四捨五入は避ける。
        let cells = pct * BAR_WIDTH / 100;
        if cells > 0 {
            spans.push(Span::styled(
                "▰".repeat(cells as usize),
                Style::default().fg(color),
            ));
        }
    }
    spans
}

fn worker_bar_spans(state: &City) -> Vec<Span<'static>> {
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

/// Manager タブを `ClickableList` 1 本に詰め込む。各 Strategy ボタン /
/// 雇用 / CPU 進化はクリック可能行 (`push_clickable`)、タグラインや自動
/// 運用ステータスは表示専用の行 (`push`)。共通スクロールレイヤがこの list
/// を `wrap=false` で描画し、行単位でクリック領域を登録するので
/// 「下まで見えない & 押せない」問題が一気に解決する。
fn manager_list(state: &City) -> ClickableList<'static> {
    let mut cl = ClickableList::new();

    cl.push_clickable(
        strategy_button_line("[G] [GRW] 成長重視", state.strategy == Strategy::Growth, Color::Green),
        ACT_STRATEGY_GROWTH,
    );
    cl.push_clickable(
        strategy_button_line("[I] [CSH] 収入重視", state.strategy == Strategy::Income, Color::Yellow),
        ACT_STRATEGY_INCOME,
    );
    cl.push_clickable(
        strategy_button_line("[T] [TEC] 技術投資", state.strategy == Strategy::Tech, Color::Cyan),
        ACT_STRATEGY_TECH,
    );
    cl.push_clickable(
        strategy_button_line("[E] [ECO] 環境配慮", state.strategy == Strategy::Eco, Color::LightGreen),
        ACT_STRATEGY_ECO,
    );

    // 選択中 Strategy のタグライン (1 行)。
    let info = logic::strategy_info(state.strategy);
    let mut tag_spans = vec![
        Span::styled(" → ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            info.tagline.to_string(),
            Style::default().fg(Color::White),
        ),
    ];
    if info.speed_bonus_pct != 0 || info.income_penalty_pct != 0 {
        let mut suffix = String::from(" (");
        if info.speed_bonus_pct != 0 {
            suffix.push_str(&format!("建設{:+}%", info.speed_bonus_pct));
        }
        if info.speed_bonus_pct != 0 && info.income_penalty_pct != 0 {
            suffix.push('/');
        }
        if info.income_penalty_pct != 0 {
            suffix.push_str(&format!("収入{:+}%", info.income_penalty_pct));
        }
        suffix.push(')');
        tag_spans.push(Span::styled(suffix, Style::default().fg(Color::DarkGray)));
    }
    cl.push(Line::from(tag_spans));

    // 雇用ボタン。
    let hire_cost = logic::hire_worker_cost(state.workers);
    let (hire_label, hire_color, hire_clickable) = match hire_cost {
        Some(c) if state.cash >= c => {
            (format!("[W] ▰ 作業員雇用 (${})", c), Color::White, true)
        }
        Some(c) => (format!("[W] ▰ 作業員雇用 (${})", c), Color::DarkGray, true),
        None => ("[W] ▰ 作業員MAX到達".to_string(), Color::DarkGray, false),
    };
    let hire_line = Line::from(Span::styled(hire_label, Style::default().fg(hire_color)));
    if hire_clickable {
        cl.push_clickable(hire_line, ACT_HIRE_WORKER);
    } else {
        cl.push(hire_line);
    }

    // CPU 進化ボタン (or 最大到達表示)。
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
        cl.push_clickable(
            Line::from(Span::styled(label, Style::default().fg(color))),
            ACT_UPGRADE_AI,
        );
    } else {
        cl.push(Line::from(Span::styled(
            "[U] [IV] CPU最大Tier到達",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // 自動運用ステータス — 撤去判断は AI が `evaluate` と
    // `action_value` を比較して即時実行。表示は予備金ガードのみ
    // (= AI が撤去後に手元に残す cash 下限。デフレ螺旋ガード)。
    let policy = logic::automation_policy(state.strategy);
    let auto_label = format!(" 🤖 撤去判断: AI / 予備${}", policy.min_cash_reserve);
    cl.push(Line::from(Span::styled(
        auto_label,
        Style::default().fg(Color::DarkGray),
    )));

    cl
}

fn strategy_button_line(label: &str, selected: bool, accent: Color) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(accent)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(accent)
    };
    Line::from(Span::styled(label.to_string(), style))
}

// ── AI activity log ─────────────────────────────────────────

fn log_list(state: &City) -> ClickableList<'static> {
    let spinner_chars = ['◐', '◓', '◑', '◒'];
    let spinner = spinner_chars[((state.tick / 2) % spinner_chars.len() as u64) as usize];
    let header = format!("{} AI {} 履歴", spinner, ai_tier_icon(state.ai_tier));

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        header,
        Style::default().fg(Color::Magenta),
    )));
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
        cl.push(Line::from(Span::styled(e.clone(), style)));
    }
    cl
}

fn strategy_label(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "成長",
        Strategy::Income => "収入",
        Strategy::Tech => "技術",
        Strategy::Eco => "環境",
    }
}

// ── Offline welcome overlay ────────────────────────────────

/// 中央に重ねる「おかえりなさい」モーダル。
///
/// `Clear` で背景を白紙化してから `Clickable` で wrap した `Paragraph` を
/// 上書き描画することで、配下の grid / panel と独立した見た目になる。
/// クリック対象は box 全体に登録されており、領域内のどこをタップしても
/// `ACT_DISMISS_OFFLINE_WELCOME` が発火する。
fn render_offline_welcome_overlay(
    welcome: &PendingOfflineWelcome,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // モーダルサイズ。横は読みやすさ重視で最大 48 col、画面が狭い時は
    // area.width を上限に縮める。縦 9 行 = ボーダー2 + 余白2 + 本文5。
    let modal_w = area.width.min(48);
    let modal_h: u16 = 9;
    if modal_w < 16 || area.height < modal_h {
        // 画面が極端に狭い時はモーダル省略。Events ログが残るので情報は失われない。
        return;
    }
    let x = area.x + (area.width - modal_w) / 2;
    let y = area.y + (area.height - modal_h) / 2;
    let modal_area = Rect::new(x, y, modal_w, modal_h);

    let duration = format_offline_duration(welcome.elapsed_secs);
    let detail = if welcome.capped {
        format!(
            "上限{}まで回収 ({}%効率)",
            format_offline_duration(MAX_OFFLINE_SECS),
            OFFLINE_EFFICIENCY_PCT
        )
    } else {
        format!("{}%効率", OFFLINE_EFFICIENCY_PCT)
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "🌙 おかえりなさい",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("オフライン {} の収益", duration),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!("+${}", welcome.bonus_cash),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            detail,
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "▶ タップして閉じる",
            Style::default().fg(Color::Cyan),
        )),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" ボーナス受領 "),
    );

    f.render_widget(Clear, modal_area);
    let mut cs = click_state.borrow_mut();
    Clickable::new(para, ACT_DISMISS_OFFLINE_WELCOME).render(f, modal_area, &mut cs);
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

    /// `worker_status_lines`: 建設中タイルが行に変換され、待機中ワーカーが
    /// 残数分追加される (= ワーカー数 = 表示行数 - ヘッダ 1)。
    #[test]
    fn worker_status_lines_lists_active_and_idle() {
        let mut city = City::new();
        city.workers = 3;
        // 1 ワーカーを Construction に、1 ワーカーを Clearing に割り当てる。
        city.set_tile(0, 0, Tile::Construction { target: Building::House, ticks_remaining: 50 });
        city.set_tile(
            1,
            0,
            Tile::Clearing {
                ticks_remaining: 30,
                target: None,
            },
        );

        let lines = worker_status_lines(&city);
        // 1 (header) + 2 (busy) + 1 (idle) = 4 行
        assert_eq!(lines.len(), 4);
        // 行 1 がヘッダ ("WORKERS" を含む)
        let header_text: String = lines[0].iter().map(|s| s.content.as_ref()).collect::<Vec<&str>>().join("");
        assert!(header_text.contains("WORKERS"), "header missing: {:?}", header_text);
        // 最後の行が "待機中"
        let last_text: String = lines[3].iter().map(|s| s.content.as_ref()).collect::<Vec<&str>>().join("");
        assert!(last_text.contains("待機中"), "last line should be idle: {:?}", last_text);
    }

    /// すべてアイドルなら、ヘッダ + ワーカー数行が出る。
    #[test]
    fn worker_status_lines_all_idle() {
        let mut city = City::new();
        city.workers = 4;
        let lines = worker_status_lines(&city);
        assert_eq!(lines.len(), 1 + 4); // header + 4 idle
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

    /// Status タブに切り替えると Strategy のタグラインが画面に出る。
    /// 「マネージャーが今 CPU に何をやらせているか」が UI で読める保証。
    #[test]
    fn status_tab_shows_strategy_tagline() {
        let mut city = City::new();
        city.panel_tab = PanelTab::Status;
        city.strategy = Strategy::Tech;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 100, 30), &cs);
            })
            .unwrap();
        // TestBackend は double-width 文字を 2 セルに分割して 2 セル目を
        // 空白で埋めるため、`concat` 上は "技 術 投 資" のように現れる。
        // 比較前に空白を全て落として「文字の出現」だけをチェックする。
        let concat = screen_compact(&terminal);
        assert!(
            concat.contains("技術投資"),
            "Status tab should display the strategy label; compacted screen:\n{}",
            concat
        );
    }

    /// Manager タブには現在選択中 Strategy のタグラインが矢印付きで出る。
    #[test]
    fn manager_tab_shows_selected_strategy_tagline() {
        let mut city = City::new();
        city.panel_tab = PanelTab::Manager;
        city.strategy = Strategy::Income;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 100, 30), &cs);
            })
            .unwrap();
        // タグラインの先頭 (「工房と店舗」) が出ていれば OK。
        // 末尾は wide layout の右パネル (~32 cells) で折れるため。
        let concat = screen_compact(&terminal);
        assert!(
            concat.contains("工房と店舗"),
            "Manager tab should display the Income tagline beginning; compacted screen:\n{}",
            concat
        );
    }

    /// テスト用: TestBackend のバッファを「空白を抜いた」文字列にして返す。
    /// ratzilla の TestBackend は double-width 文字を 2 セルに分割し、
    /// 2 セル目を空白で埋めるため、そのまま concat すると "技 術" になる。
    /// 検索系 assert では空白を抜いてから比較する。
    fn screen_compact(
        terminal: &Terminal<TestBackend>,
    ) -> String {
        let buffer = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let sym = buffer.cell((x, y)).unwrap().symbol();
                if sym != " " {
                    s.push_str(sym);
                }
            }
        }
        s
    }

    /// 80 col ターミナル (典型的な PC) では narrow layout が選ばれる。
    /// グリッド拡張 (24→32) で wide が ~90 col 必要になったため、80 col は
    /// narrow に振らないと右パネルが潰れる (Codex P2 review #96)。
    #[test]
    fn eighty_col_uses_narrow_layout() {
        assert!(metropolis_is_narrow(60));
        assert!(metropolis_is_narrow(80));
        assert!(metropolis_is_narrow(89));
        assert!(!metropolis_is_narrow(90));
        assert!(!metropolis_is_narrow(120));
    }

    /// 80×30 のような中間幅でもパニックしない (narrow path で描画される)。
    #[test]
    fn render_does_not_panic_on_80col_intermediate() {
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 40), &cs);
            })
            .unwrap();
        // タブのクリック対象が登録されていることも確認 (右パネルが潰れていない)。
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        for id in [ACT_TAB_STATUS, ACT_TAB_MANAGER, ACT_TAB_EVENTS, ACT_TAB_WORLD] {
            assert!(
                registered.contains(&id),
                "tab action {} missing on 80-col layout: targets={:?}",
                id,
                registered
            );
        }
    }

    /// 都市グリッド (= viewport) が画面幅の半分以上を占めること (wide layout)。
    /// マップ全体は 64×32 だが、画面に映る viewport は VIEW_W=32 のままなので
    /// レイアウト幅の関係は不変。
    #[test]
    fn wide_layout_grid_occupies_majority_of_width() {
        // viewport = 32*2 + 2 = 66. With area width 100 → 66/100 = 66% ≥ 50%.
        let grid_w = VIEW_W as u16 * 2 + 2;
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

    /// スマホ想定の浅い縦幅 (Manager の content area が 3 行しか取れない)
    /// で、最初は ▼ ボタンだけが表示される (= 続きがあることを伝える)。
    /// content が見切れてもパニックしない。
    #[test]
    fn narrow_panel_shows_down_arrow_when_overflow() {
        let mut city = City::new();
        city.panel_tab = PanelTab::Manager;
        // 28 col × 28 row の極狭ターミナル。banner 4 + grid 18 = 22 を引くと
        // パネルは最大 6 行 (枠 2 + tab 1 + content 3)。Manager は 8 行あるので
        // content overflow が起きる。
        let mut terminal = Terminal::new(TestBackend::new(28, 28)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 28, 28), &cs);
            })
            .unwrap();
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        assert!(
            registered.contains(&ACT_PANEL_SCROLL_DOWN),
            "▼ scroll target should be registered when content overflows; got {:?}",
            registered
        );
        // まだスクロールしていないので ▲ は出ない。
        assert!(
            !registered.contains(&ACT_PANEL_SCROLL_UP),
            "▲ should not register at scroll=0; got {:?}",
            registered
        );
    }

    /// スクロールダウン後は ▲ も登録され、最下端まで降りると ▼ が消える。
    /// `scroll_panel` で深いオフセットを設定 → `ScrollableTab` 内部の
    /// clamp で max_scroll に揃えられて write-back される連携テスト。
    #[test]
    fn scroll_clamp_and_arrow_visibility() {
        let mut city = City::new();
        city.panel_tab = PanelTab::Manager;
        // 大きめにスクロールを入れて clamp を強制。
        city.panel_scroll.set(99);

        let mut terminal = Terminal::new(TestBackend::new(28, 28)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 28, 28), &cs);
            })
            .unwrap();
        // clamp が走って scroll は max_scroll に揃う。max_scroll は content_h と
        // area_h 次第で具体値はテストで固定しないが、ゼロにはならないはず。
        let scroll = city.panel_scroll.get();
        assert!(scroll > 0, "clamp should keep scroll > 0 when overflow exists");
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        assert!(
            registered.contains(&ACT_PANEL_SCROLL_UP),
            "▲ should appear after scrolling down"
        );
        // 最下端まで来ているので ▼ は消える (clamp 後 scroll == max_scroll)。
        assert!(
            !registered.contains(&ACT_PANEL_SCROLL_DOWN),
            "▼ should disappear at bottom"
        );
    }

    /// Wide layout (= パネル領域が広い) で Manager 全行が収まる時は
    /// スクロールボタンが一切登録されない (= 不要な UI を出さない)。
    #[test]
    fn wide_layout_hides_scroll_arrows_when_no_overflow() {
        let mut city = City::new();
        city.panel_tab = PanelTab::Manager;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 120, 40), &cs);
            })
            .unwrap();
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        assert!(
            !registered.contains(&ACT_PANEL_SCROLL_UP),
            "▲ should not appear when content fits"
        );
        assert!(
            !registered.contains(&ACT_PANEL_SCROLL_DOWN),
            "▼ should not appear when content fits"
        );
    }
}
