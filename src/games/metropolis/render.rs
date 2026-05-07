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

use crate::input::ClickState;
use crate::widgets::{Clickable, TabBar};

use super::logic;
use super::state::{
    city_tier_for, next_tier_threshold, AiTier, Building, City, CityTier, PanelTab, Strategy,
    Tile, GRID_H, GRID_W, PAYOUT_FLASH_TICKS,
};
use super::terrain::Terrain;
use super::{
    ACT_HIRE_WORKER, ACT_STRATEGY_ECO, ACT_STRATEGY_GROWTH, ACT_STRATEGY_INCOME, ACT_STRATEGY_TECH,
    ACT_TAB_EVENTS, ACT_TAB_MANAGER, ACT_TAB_STATUS, ACT_TAB_WORLD, ACT_UPGRADE_AI,
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

    render_grid(state, f, h[0], 2, click_state);
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
    _click_state: &Rc<RefCell<ClickState>>,
) {
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
            let tier = logic::effective_tier_at(state, x, y);
            let bright = !(tick / 10).is_multiple_of(4);
            let m = if bright { Modifier::BOLD } else { Modifier::empty() };
            let (ch, color) = match tier {
                logic::HouseTier::Cottage => ('h', Color::Green),
                logic::HouseTier::Apartment => ('H', Color::LightGreen),
                logic::HouseTier::Highrise => ('▮', Color::LightCyan),
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
            let level = logic::shop_level(state, x, y);
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
            let active = logic::workshop_is_active(state, x, y);
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
    }
}

fn tile_char_1(b: Building) -> char {
    match b {
        Building::Road => '+',
        Building::House => 'H',
        Building::Workshop => 'W',
        Building::Shop => 'S',
        Building::Park => 'P',
        Building::Outpost => 'O',
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
            let tier = logic::effective_tier_at(state, x, y);
            let level = logic::house_level(state, x, y);
            let glyph = house_glyph_2wide(tier, level);
            let (color, modifier) = house_style_2wide(tier, tick);
            let bg = house_bg(tier, tick);

            // 航空標識: Highrise が密集 (周囲 3 軒以上 Highrise) で、夜間に
            // 1.5 秒周期で右側 1 文字を `*` (赤太字) に差し替える。
            // 都市感の最後のスパイス — Tier 4 経済まで育てたプレイヤーへのご褒美。
            if matches!(tier, logic::HouseTier::Highrise)
                && logic::should_show_aviation_light(state, x, y, tick)
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
            let level = logic::shop_level(state, x, y);
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
            let active = logic::workshop_is_active(state, x, y);
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
    };
    (color, modifier)
}

fn construction_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Yellow,
        Building::House => Color::LightGreen,
        Building::Workshop => Color::LightRed,
        Building::Shop => Color::LightCyan,
        Building::Park => Color::LightGreen,
        Building::Outpost => Color::LightYellow,
    }
}

fn built_color(b: Building) -> Color {
    match b {
        Building::Road => Color::Gray,
        Building::House => Color::Green,
        Building::Workshop => Color::LightRed,
        Building::Shop => Color::Yellow,
        Building::Park => Color::LightGreen,
        Building::Outpost => Color::LightYellow,
    }
}

fn built_2wide_glyph(b: Building) -> &'static str {
    match b {
        Building::Road => "══",
        Building::House => "▟▙",
        Building::Workshop => "˚⊞",
        Building::Shop => "$$",
        Building::Park => "❀✿",
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

    // 区切り + Strategy 内訳パネル。
    // 「マネージャーが今 CPU に何をやらせているか」を可視化することで、
    // ボタンを切り替えた時の効果が即座に見える。
    lines.push(Line::from(""));
    lines.extend(strategy_status_lines(state));

    // 区切り + ワーカー稼働状況 (誰が何を建てているか)。
    lines.push(Line::from(""));
    lines.extend(worker_status_lines(state));

    f.render_widget(Paragraph::new(lines), inner);
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
                Tile::Clearing { ticks_remaining } => {
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

/// `building_name` は logic.rs にあるが pub では無いので render 側で複製。
/// 種類が増えた時に両方更新する必要がある (clippy で気付くのは難しいので注意)。
fn building_name_for(b: Building) -> &'static str {
    match b {
        Building::Road => "道路",
        Building::House => "住宅",
        Building::Workshop => "工房",
        Building::Shop => "店舗",
        Building::Park => "公園",
        Building::Outpost => "開拓機材",
    }
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

/// 建物の絵文字アイコン (ワーカー一覧表示用)。
fn building_icon(b: Building) -> &'static str {
    match b {
        Building::Road => "🛣",
        Building::House => "🏠",
        Building::Workshop => "🔧",
        Building::Shop => "🏪",
        Building::Park => "🌳",
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

    // 行構成: [GRW][CSH][TEC][ECO] 4 ボタン → タグライン 1 行 →
    // 雇用 → AI Upgrade。Eco 追加で 4 行になったが、タブパネル下端が
    // 余っているので問題ない。
    let rows = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(1), // [GRW]
            Constraint::Length(1), // [CSH]
            Constraint::Length(1), // [TEC]
            Constraint::Length(1), // [ECO]
            Constraint::Length(1), // 選択中タグライン
            Constraint::Length(1), // 雇用
            Constraint::Length(1), // AI 進化
            Constraint::Length(1), // 自動運用ステータス
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
    button_row(
        f,
        rows[3],
        &mut cs,
        ACT_STRATEGY_ECO,
        "[E] [ECO] 環境配慮",
        state.strategy == Strategy::Eco,
        Color::LightGreen,
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
    f.render_widget(Paragraph::new(Line::from(tag_spans)), rows[4]);

    let hire_cost = logic::hire_worker_cost(state.workers);
    let (hire_label, hire_color) = match hire_cost {
        Some(c) if state.cash >= c => (format!("[W] ▰ 作業員雇用 (${})", c), Color::White),
        Some(c) => (format!("[W] ▰ 作業員雇用 (${})", c), Color::DarkGray),
        None => ("[W] ▰ 作業員MAX到達".to_string(), Color::DarkGray),
    };
    let p = Paragraph::new(Span::styled(hire_label, Style::default().fg(hire_color)));
    Clickable::new(p, ACT_HIRE_WORKER).render(f, rows[5], &mut cs);

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
        Clickable::new(p, ACT_UPGRADE_AI).render(f, rows[6], &mut cs);
    } else {
        let p = Paragraph::new(Span::styled(
            "[U] [IV] CPU最大Tier到達",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(p, rows[6]);
    }

    // 自動運用ステータス — 戦略に応じた撤去 / 開拓の周期を表示。
    // 旧 `[O]` `[D]` `[X]` ボタンは廃止され、すべて tick 駆動で自動発火する。
    let policy = logic::automation_policy(state.strategy);
    let outpost_txt = policy
        .outpost_dispatch_period_ticks
        .map(|p| format!("拡張{}s", p / 10))
        .unwrap_or_else(|| "拡張なし".to_string());
    let demolish_txt = policy
        .auto_demolish_period_ticks
        .map(|p| format!("撤去{}s", p / 10))
        .unwrap_or_else(|| "撤去なし".to_string());
    let auto_label = format!(
        " 🤖 自動運用: {} / {} / 予備${}",
        outpost_txt, demolish_txt, policy.min_cash_reserve
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            auto_label,
            Style::default().fg(Color::DarkGray),
        )),
        rows[7],
    );
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
        Strategy::Eco => "環境",
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

    /// `worker_status_lines`: 建設中タイルが行に変換され、待機中ワーカーが
    /// 残数分追加される (= ワーカー数 = 表示行数 - ヘッダ 1)。
    #[test]
    fn worker_status_lines_lists_active_and_idle() {
        let mut city = City::new();
        city.workers = 3;
        // 1 ワーカーを Construction に、1 ワーカーを Clearing に割り当てる。
        city.set_tile(0, 0, Tile::Construction { target: Building::House, ticks_remaining: 50 });
        city.set_tile(1, 0, Tile::Clearing { ticks_remaining: 30 });

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
