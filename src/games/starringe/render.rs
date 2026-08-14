//! 星環の描画。読み取り専用。クリック登録は widgets 経由のみ。
//!
//! フィールドは縦型。画面下部にコアと砲台の環が座り、上空の広い範囲から
//! 鉱石が降ってくる。ステージは Canvas + braille の点描で、ワールド座標を
//! そのまま渡す。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableList, ScrollableTab, TabBar};

use super::actions::{
    buy_ring_id, buy_weapon_stat_id, select_weapon_id, OPEN_LAYER, TAB_ARMORY, TAB_CODEX, TAB_RING,
    TAB_SCROLL_DOWN, TAB_SCROLL_UP, TAP_STRIKE, WEAPON_NEXT, WEAPON_PREV,
};
use super::logic::{
    can_unlock_next_layer, can_upgrade_ring, can_upgrade_weapon_stat, layer_unlock_cost,
    ring_upgrade_cost, turret_positions, weapon_stat_cost,
};
use super::state::{
    Layer, OreKind, ParticleKind, RingUpgrade, StarRingState, Tab, WeaponKind, WeaponStat, CORE_Y,
    CX, FIELD_MARGIN, INNER_RADIUS, SHAKE_MAX_Y, SPAWN_Y, TURRET_NEAR_RADIUS, VISIBLE_Y_LO,
    WORLD_H, WORLD_W,
};

/// 画面全体の縦分割。ヘッダー / タブ / 本体 / フッターの順に返す。
///
/// 固定消費を 4 行に抑え、残りをすべて本体へ回す。端末が 30 行しかない
/// モバイルでは、枠に 1 行使うたびにステージの情報量がそのまま削れる。
fn split_frame(area: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2], chunks[3]]
}

/// 円を塗り潰すサンプリング間隔をドット間隔ちょうどから少しだけ詰める割合。
/// 端の丸みが標本の位相で欠けないぶんの余裕。
const FILL_STEP_MARGIN: f64 = 0.85;

/// 円の塗り潰し (`canvas_fx::filled_ellipse_points`) に渡すサンプリング間隔。
///
/// Braille は 1 セルを 2×4 のドットへ割るので、1 ドットが受け持つワールド距離は
/// 描画領域の広さで決まる。ドットより細かく刻んでも同じドットを塗り直すだけで
/// 見た目は変わらず点数だけが増える — 非力な端末ほどステージが狭く、そこで
/// 過剰サンプリングが一番効いてしまうので、間隔は領域の解像度から導く。
fn fill_step(inner: Rect) -> f64 {
    let dot_x = WORLD_W / (inner.width.max(1) as f64 * 2.0);
    let dot_y = WORLD_H / (inner.height.max(1) as f64 * 4.0);
    dot_x.min(dot_y) * FILL_STEP_MARGIN
}

/// 円を塗り潰した点列。間隔は `fill_step` が領域の解像度から決めるが、半径より
/// 粗い間隔を渡すと `canvas_fx::filled_ellipse_points` は 1 点も返さない。
/// 砲台や小さい弾のように半径がドット間隔を下回る円が消えないよう、間隔は
/// 半径で頭打ちにする。
fn filled_circle(cx: f64, cy: f64, r: f64, step: f64) -> Vec<(f64, f64)> {
    canvas_fx::filled_ellipse_points(cx, cy, r, r, step.min(r))
}

/// ステージの描画物は「核 → 砲台 → 環」の順に大きく明るく描く。守る拠点と
/// 自分の武装が、降ってくる鉱石や軌道の装飾と同じ粒度で並ぶと、画面のどこを
/// 見ればよいかが決まらない。
///
/// 核の脈動が取りうる最大倍率。層が上がるぶんの膨らみもここで頭打ちにする —
/// 層に上限が無い (`Layer::title` の「無限輪」) ため、青天井のまま掛けると
/// いずれ核が Canvas の下端を割る。
const CORE_MAX_SCALE: f64 = 1.55;

/// 核本体の描画半径。
///
/// 鉱石が消える到達半径 (`INNER_RADIUS`) を包む大きさに取る。到達半径より
/// 小さく描くと、鉱石が核の縁へ触れる手前で消えて吸い込まれたように見えない。
/// 最大の鉱石 (`OreKind::Nova` の半径 5.1) より一回り大きいので、拠点と的は
/// 色より先に大きさで読み分けられる。
const CORE_RADIUS: f64 = INNER_RADIUS + 1.0;

/// 核を囲む暈の半径。核本体との差が、核がまとう光の厚みになる。
///
/// 上限は「脈動で `CORE_MAX_SCALE` 倍まで膨らんでも、下端が `VISIBLE_Y_LO`
/// (画面シェイクで下がっても Canvas に残る高さ) を割らない」ことで決まる。
/// 核は `CORE_Y` に座っているので、下へ使える余裕はその値しかない。
const CORE_HALO_RADIUS: f64 = 9.0;

const _: () = assert!(
    CORE_Y - CORE_HALO_RADIUS * CORE_MAX_SCALE >= VISIBLE_Y_LO,
    "脈動しきった核の暈が Canvas の下端を割る"
);
const _: () = assert!(
    CORE_RADIUS <= CORE_HALO_RADIUS,
    "核本体が暈より大きいと暈が輪郭に見えない"
);

/// 手前側の砲台を描く半径の上限。環の点より明らかに太い塊として読めて、
/// なお核より小さい大きさ。
const TURRET_MAX_RADIUS: f64 = 2.6;

/// 奥側の砲台を手前側から縮める割合。遠近を大きさで描き分ける。
const TURRET_FAR_SCALE: f64 = 0.62;

/// 環の点を打つ弧長間隔 (ワールド単位)。砲台の直径より広く取り、環が砲台と
/// 同じ粒度で並んだ帯にならないようにする。
const ORBIT_DOT_SPACING: f64 = 6.0;

/// 手前側の砲台の描画半径。
///
/// 環の最下点にいる砲台の下端が `VISIBLE_Y_LO` を割らない範囲へ収める。砲台が
/// 増えると環は下がるので、使える余裕はそのぶん減る。下限の
/// `TURRET_NEAR_RADIUS` は `StarRingState::ring_radii` が環の縦半径を頭打ちに
/// するのに使う値で、環が下がりきった時に残る余裕と一致する。
fn turret_radius(ring_ry: f64) -> f64 {
    (CORE_Y - ring_ry - VISIBLE_Y_LO).clamp(TURRET_NEAR_RADIUS, TURRET_MAX_RADIUS)
}

/// 核の脈動倍率。層開放・被弾・開放待ちの合図を大きさへ変える。
fn core_scale(state: &StarRingState) -> f64 {
    let raw = if state.layer_flash_ticks > 0 {
        CORE_MAX_SCALE
    } else if state.layer_ready_flash_ticks > 0 {
        1.30
    } else if state.core_flash_ticks > 0 {
        1.25
    } else if can_unlock_next_layer(state) && state.elapsed_ticks % 20 < 10 {
        1.12
    } else {
        1.0 + (state.layer().saturating_sub(1) as f64) * 0.04
    };
    raw.min(CORE_MAX_SCALE)
}

/// ステージの描画物をワールド座標から画面座標へ移す平行移動。
///
/// 振れを描画物ごとに手で足すと、足し忘れた物だけが揺れずに取り残されたり、
/// 画面外へはみ出さない上限の計算だけが振れを勘定し損ねたりする。ステージの
/// 描画物は例外なくこの変換を通す。
#[derive(Clone, Copy)]
struct Shake {
    dx: f64,
    dy: f64,
}

impl Shake {
    /// 衝撃の残っている間だけ振れる。横は tick ごと、縦は 2 tick ごとに位相を
    /// 変えて同じ向きへ流れないようにし、縦の振れ幅は `SHAKE_MAX_Y` に収める。
    fn new(state: &StarRingState) -> Self {
        if state.shake_ticks == 0 {
            return Self { dx: 0.0, dy: 0.0 };
        }
        Self {
            dx: (((state.elapsed_ticks % 4) as f64) - 1.5) * 0.4,
            dy: ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * SHAKE_MAX_Y,
        }
    }

    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (x + self.dx, y + self.dy)
    }

    /// 半径 `r` の円を塗り潰した点列。
    fn circle(self, x: f64, y: f64, r: f64, step: f64) -> Vec<(f64, f64)> {
        filled_circle(x + self.dx, y + self.dy, r, step)
    }

    /// 進行方向 `(vx, vy)` の反対側へ `len` 伸ばした尾の線分 `(先端, 末尾)`。
    fn trail(self, x: f64, y: f64, vx: f64, vy: f64, len: f64) -> (f64, f64, f64, f64) {
        let speed = vx.hypot(vy).max(0.01);
        let (hx, hy) = self.point(x, y);
        let (tx, ty) = self.point(x - vx / speed * len, y - vy / speed * len);
        (hx, hy, tx, ty)
    }
}

/// 核脈動の波面を打つ点の弧長間隔 (ワールド単位)。
const PULSE_ARC_STEP: f64 = 1.0;

/// 核脈動の波面の色。
const PULSE_WAVE_COLOR: Color = Color::LightCyan;

/// 核脈動の波面の点を `out` へ積む。
///
/// 角度を一定間隔で刻むと、波が広がるほど点の間隔が円周に比例して開き、
/// 上空へ届く頃には点がばらけて波に見えなくなる。弧長で刻んで密度を保ち、
/// フィールドの外へ出た点は捨てる — 核は下端にあるので、円周の下半分は
/// ほとんど画面の外に落ちる。
fn push_pulse_wave_points(cx: f64, cy: f64, radius: f64, density: f64, out: &mut Vec<(f64, f64)>) {
    if radius <= 0.0 || density <= 0.0 {
        return;
    }
    let step = (PULSE_ARC_STEP * density / radius).clamp(0.02, 0.5);
    let mut angle = 0.0;
    while angle < std::f64::consts::TAU {
        let (sin, cos) = angle.sin_cos();
        let (x, y) = (cx + cos * radius, cy + sin * radius);
        if (0.0..=WORLD_W).contains(&x) && (0.0..=WORLD_H).contains(&y) {
            out.push((x, y));
        }
        angle += step;
    }
}

/// 本体を (ステージ, タブ内容) へ分ける。
///
/// ステージ側を過半にするのは、鉱石が降ってきて砕ける様子が主役だから。
/// タブ内容が溢れる分は `ScrollableTab` のスクロールで拾う。
fn split_body(body: Rect, is_narrow: bool) -> (Rect, Rect) {
    // ナローは上がステージ、ワイドは左がタブ内容で右がステージ。
    // ワイドの左パネルは、説明文を折り返して読ませる前提で幅を詰める。
    // 折り返さない行 (強化バー・武器ピッカー・コスト表示) が読める下限が
    // 34% で、それ以上をステージへ回す。
    let (dir, first, second) = if is_narrow {
        (Direction::Vertical, 58, 42)
    } else {
        (Direction::Horizontal, 34, 66)
    };
    let parts = Layout::default()
        .direction(dir)
        .constraints([
            Constraint::Percentage(first),
            Constraint::Percentage(second),
        ])
        .split(body);
    if is_narrow {
        (parts[0], parts[1])
    } else {
        (parts[1], parts[0])
    }
}

pub fn render(
    state: &StarRingState,
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

    let [header, tabs, body, footer] = split_frame(area);

    render_header(state, f, header, is_narrow);
    render_tabs(state, f, tabs, click_state);

    let (stage_area, tab_area) = split_body(body, is_narrow);
    render_stage(state, f, stage_area, borders, is_narrow, click_state);
    match state.tab {
        Tab::Armory => render_armory(state, f, tab_area, borders, click_state),
        Tab::Ring => render_ring(state, f, tab_area, borders, click_state),
        Tab::Codex => render_codex(state, f, tab_area, borders, click_state),
    }

    render_footer(state, f, footer, is_narrow);
}

fn format_shards(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.2}M", n / 1_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else if n >= 100.0 {
        format!("{:.0}", n)
    } else {
        format!("{:.1}", n)
    }
}

/// ヘッダー。枠を持たず 2 行で、ワイドは 1 行に畳んで残り 1 行を
/// 本体との区切り罫にする。ナローは幅が足りないので 2 行へ折り返す。
fn render_header(state: &StarRingState, f: &mut Frame, area: Rect, is_narrow: bool) {
    let sps = state.shards_per_sec();
    let layer = state.layer();
    let boost = if state.boost_ticks > 0 {
        " ⚡ブースト"
    } else {
        ""
    };
    let layer_fx = if state.layer_flash_ticks > 0 {
        " ◆層開放"
    } else if can_unlock_next_layer(state) {
        " ◆開放可[!]"
    } else if state.kills_ready_for_next_layer() {
        " ◆星屑不足"
    } else if state.layer_ready_flash_ticks > 0 {
        " ◆条件達成"
    } else {
        ""
    };

    let ident = vec![
        Span::styled(
            "星環",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("第{layer}層 {}", Layer::title(layer)),
            Style::default()
                .fg(layer_color(layer))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(layer_fx, Style::default().fg(Color::LightMagenta)),
    ];
    let stats = vec![
        Span::styled(
            format!("✦{}", format_shards(state.shards)),
            Style::default().fg(Color::LightYellow),
        ),
        Span::raw("  "),
        Span::styled(format!("{:.1}/秒", sps), Style::default().fg(Color::Cyan)),
        Span::styled(boost, Style::default().fg(Color::LightRed)),
    ];

    let lines = if is_narrow {
        vec![Line::from(ident), Line::from(stats)]
    } else {
        let mut single = ident;
        single.push(Span::raw("   "));
        single.extend(stats);
        vec![
            Line::from(single),
            Line::from(Span::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };
    f.render_widget(Paragraph::new(lines), area);
}

fn layer_color(layer: u32) -> Color {
    match layer {
        1 => Color::Gray,
        2 => Color::Yellow,
        3 => Color::LightCyan,
        4 => Color::LightMagenta,
        5 => Color::LightRed,
        6 => Color::Cyan,
        7 => Color::Magenta,
        _ => Color::White,
    }
}

/// タブ帯を占める背景色。枠線を持たない 1 行のタブバーが、ヘッダーとも
/// 本体とも別の帯として読めるようにする。
const TAB_BAND: Color = Color::Indexed(236);

/// タブバー。枠を外して 1 行に収める。`Block` は borders を持たないので
/// `TabBar` が計算する内側領域は area と一致し、クリック判定は行全体に載る。
fn render_tabs(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let sel = |active: bool| {
        if active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray).bg(TAB_BAND)
        }
    };
    TabBar::new("│")
        .block(Block::default().style(Style::default().bg(TAB_BAND)))
        .tab("武装", sel(state.tab == Tab::Armory), TAB_ARMORY)
        .tab("環", sel(state.tab == Tab::Ring), TAB_RING)
        .tab("図鑑", sel(state.tab == Tab::Codex), TAB_CODEX)
        .render(f, area, &mut cs);
}

/// タブ本文の 1 かたまり。行の集合と、その全行に割り当てるクリック先。
///
/// かたまり単位で持つのは、購入項目が「見出し行 + 説明行」の 2 行組で、
/// どちらを叩いても同じ購入が走ってほしいため。
struct Section {
    lines: Vec<Line<'static>>,
    action: Option<u16>,
}

impl Section {
    fn plain(lines: Vec<Line<'static>>) -> Self {
        Self {
            lines,
            action: None,
        }
    }

    fn clickable(lines: Vec<Line<'static>>, action: u16) -> Self {
        Self {
            lines,
            action: Some(action),
        }
    }
}

/// 表示幅 (半角=1 / 全角=2)。ratatui の Buffer もこの幅でセルを埋めるため、
/// 折り返しの計算は文字数ではなくこの幅で行う。
fn display_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// `text` を表示幅 `budget` に収まる断片へ切り分ける。日本語は語の切れ目に
/// 空白を持たないので、単語単位ではなく文字単位で折る。
fn wrap_by_width(text: &str, budget: usize) -> Vec<String> {
    if budget == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut used = 0usize;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let cw = display_width(&*ch.encode_utf8(&mut buf));
        if used + cw > budget && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(ch);
        used += cw;
    }
    if cur.is_empty() && out.is_empty() {
        out.push(String::new());
    } else if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 説明文を `width` 桁に折り返した行。2 行目以降も `indent` で字下げを揃え、
/// `rail` を渡した時は各行の先頭へ同じ縦棒を立てる。
///
/// 折り返した行はどれも `Section` へまとめて渡され、購入の当たり判定を
/// 見出し行と共有する。
fn blurb_lines(
    rail: Option<Style>,
    indent: usize,
    text: &str,
    width: u16,
    style: Style,
) -> Vec<Line<'static>> {
    let rail_w = usize::from(rail.is_some());
    // 全角 1 文字も置けない幅では折り返しても読めないので、下限を設けて
    // はみ出しは描画側の切り詰めに任せる。
    let budget = (width as usize).saturating_sub(rail_w + indent).max(2);
    wrap_by_width(text, budget)
        .into_iter()
        .map(|chunk| {
            let mut spans = Vec::new();
            if let Some(st) = rail {
                spans.push(Span::styled("│", st));
            }
            spans.push(Span::styled(
                format!("{}{}", " ".repeat(indent), chunk),
                style,
            ));
            Line::from(spans)
        })
        .collect()
}

/// 先頭に飾り (`lead`) を置き、続く本文を `width` 桁へ折り返した行。
/// 2 行目以降は飾りの幅ぶん字下げして、本文の左端を揃える。
///
/// 飾りと本文で色を変えたい 1 行もの (図鑑の一覧など) はこれで組む。折り
/// 返しを持たせておかないと、桁数の少ない端末で行の右端が黙って切り落ちる。
fn wrapped_row(lead: Span<'static>, text: String, style: Style, width: u16) -> Vec<Line<'static>> {
    let indent = lead.width();
    // 全角 1 文字も置けない幅では折り返しても読めないので、下限を設けて
    // はみ出しは描画側の切り詰めに任せる。
    let budget = (width as usize).saturating_sub(indent).max(2);
    wrap_by_width(&text, budget)
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let head = if i == 0 {
                lead.clone()
            } else {
                Span::raw(" ".repeat(indent))
            };
            Line::from(vec![head, Span::styled(chunk, style)])
        })
        .collect()
}

/// 購入行の説明文の字下げ。見出しのキー表記 (` [A] `) の下へ揃える。
const BLURB_INDENT: usize = 6;

/// かたまりを `ClickableList` へ流し込む。`spaced` の時だけ間に空行を挟む。
fn build_list(sections: Vec<Section>, spaced: bool) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    for (i, section) in sections.into_iter().enumerate() {
        if spaced && i > 0 {
            cl.push(Line::from(""));
        }
        let Section { lines, action } = section;
        for line in lines {
            match action {
                Some(id) => cl.push_clickable(line, id),
                None => cl.push(line),
            }
        }
    }
    cl
}

/// 描画領域に合わせて決めた `(行を組む桁数, かたまり間に空行を挟むか)`。
///
/// `make` は「渡した桁数に収まる行」を返す契約で、桁数を狭めれば行数は
/// 増えこそすれ減らない。`ScrollableTab` は内容が溢れる時だけ右端 1 桁を
/// スクロール列に使うので、溢れる場合は 1 桁狭い桁数を返す — その幅で組み
/// 直さないと、右端まで使った行がスクロール列の下で切り詰められる。
///
/// 行数は `build_list` が組んだ結果をそのまま数える。空行の入れ方をここへ
/// 別に書くと、`ScrollableTab` が見る実際の行数と食い違ったときに、詰めれば
/// 入る内容へスクロールを生やしたり末尾へ届かなくなったりする。
fn fit_tab_layout<F>(inner: Rect, make: F) -> (u16, bool)
where
    F: Fn(u16) -> Vec<Section>,
{
    let height = inner.height as usize;
    let rows = |width: u16, spaced: bool| build_list(make(width), spaced).lines().len();
    if rows(inner.width, true) <= height {
        (inner.width, true)
    } else if rows(inner.width, false) <= height {
        (inner.width, false)
    } else {
        (inner.width.saturating_sub(1), false)
    }
}

/// 武装タブ: 先頭 1 行の武器ピッカー + 説明と強化のスクロール領域。
///
/// ピッカーだけを固定行として外へ出すのは、内側が数行しかない端末でも
/// 武器の切り替えを常に手の届く位置へ置くため。残りは `ScrollableTab` が
/// 引き受けるので、高さが足りなければ先頭から入るだけ描いて後続はスクロール
/// で拾える。
fn render_armory(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 武装 ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < 4 {
        return;
    }

    render_weapon_picker(
        state,
        f,
        Rect::new(inner.x, inner.y, inner.width, 1),
        click_state,
    );
    if inner.height <= 1 {
        return;
    }

    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
    let (wrap_w, spaced) = fit_tab_layout(body, |w| armory_sections(state, w));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(armory_sections(state, wrap_w), spaced),
        &state.tab_scroll,
        TAB_SCROLL_UP,
        TAB_SCROLL_DOWN,
    )
    .arrow_color(Color::Yellow)
    .render(f, body, &mut cs);
}

fn render_weapon_picker(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let prev = Paragraph::new(Line::from(Span::styled(
        "◀",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::NONE));
    Clickable::new(prev, WEAPON_PREV).render(f, chunks[0], &mut click_state.borrow_mut());

    let next = Paragraph::new(Line::from(Span::styled(
        "▶",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    Clickable::new(next, WEAPON_NEXT).render(f, chunks[2], &mut click_state.borrow_mut());

    // 中央: 解放済み武器を横並びで選択
    let n = WeaponKind::ALL.len().max(1) as u16;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n as u32); WeaponKind::ALL.len()])
        .split(chunks[1]);

    for (i, w) in WeaponKind::ALL.into_iter().enumerate() {
        let unlocked = state.is_weapon_unlocked(w);
        let selected = state.selected_weapon == w;
        let style = if !unlocked {
            Style::default().fg(Color::DarkGray)
        } else if selected {
            Style::default()
                .fg(Color::Black)
                .bg(weapon_color(w))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(weapon_color(w))
        };
        // チップに割り当たった桁へ収まる表記を広い順に選ぶ。幅を無視して
        // 長い表記を渡すと右端から切り詰められ、武器名が途中で消える。
        let candidates = if unlocked {
            [
                format!(" {}{} ", w.glyph(), w.label()),
                format!(" {} ", w.label()),
                w.label().to_string(),
            ]
        } else {
            [
                format!(" ？L{} ", w.unlock_layer()),
                format!("？L{}", w.unlock_layer()),
                format!("L{}", w.unlock_layer()),
            ]
        };
        let chip_w = cols[i].width as usize;
        let label = candidates
            .iter()
            .find(|s| display_width(s) <= chip_w)
            .unwrap_or(&candidates[2])
            .clone();
        let p = Paragraph::new(Line::from(Span::styled(label, style))).alignment(Alignment::Center);
        if unlocked {
            Clickable::new(p, select_weapon_id(w)).render(
                f,
                cols[i],
                &mut click_state.borrow_mut(),
            );
        } else {
            f.render_widget(p, cols[i]);
        }
    }
}

/// 武装タブ本文のかたまり: 選択中武器の説明 + 強化 3 種。
/// `width` は説明文を折り返す桁数。
fn armory_sections(state: &StarRingState, width: u16) -> Vec<Section> {
    let w = state.selected_weapon;
    let unlocked = state.is_weapon_unlocked(w);
    let mut sections = vec![Section::plain(weapon_showcase_lines(
        state, w, unlocked, width,
    ))];
    if !unlocked {
        sections.push(Section::plain(vec![Line::from(Span::styled(
            "  解放後に強化できます",
            Style::default().fg(Color::DarkGray),
        ))]));
        return sections;
    }

    let keys = ['A', 'S', 'D'];
    for (i, stat) in WeaponStat::ALL.iter().copied().enumerate() {
        let lv = state.weapon_stat(w, stat);
        let maxed = !can_upgrade_weapon_stat(state, w, stat);
        let cost = weapon_stat_cost(state, w, stat);
        let can = !maxed && state.shards + 1e-9 >= cost;
        let cost_label = if maxed {
            "MAX".to_string()
        } else {
            format!("✦{}", format_shards(cost))
        };
        let style = if maxed {
            Style::default().fg(Color::DarkGray)
        } else if can {
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        // 行頭の縦棒は買える時だけ武器色に灯す。一覧の中で「今払えるもの」を
        // 目線だけで拾えるようにする。
        let rail = Style::default().fg(if can {
            weapon_color(w)
        } else {
            Color::DarkGray
        });
        let key = keys.get(i).copied().unwrap_or('?');
        let mut lines = vec![Line::from(vec![
            Span::styled("│", rail),
            Span::styled(format!(" [{key}] "), Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("{}  Lv.{}", stat.label(), lv),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(cost_label, Style::default().fg(Color::Cyan)),
        ])];
        lines.extend(blurb_lines(
            Some(rail),
            BLURB_INDENT,
            stat.blurb(),
            width,
            Style::default().fg(Color::DarkGray),
        ));
        sections.push(Section::clickable(lines, buy_weapon_stat_id(w, stat)));
    }
    sections
}

fn weapon_showcase_lines(
    state: &StarRingState,
    w: WeaponKind,
    unlocked: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let art = weapon_art(w);
    let dmg = state.weapon_damage(w);
    let interval = state.fire_interval(w);
    let volley = state.volley_count(w);

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(" {}  {}", w.glyph(), w.label()),
            Style::default()
                .fg(weapon_color(w))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(art, Style::default().fg(weapon_color(w))),
    ])];
    let intro = if unlocked {
        w.blurb().to_string()
    } else {
        format!("第{}層で解放", w.unlock_layer())
    };
    lines.extend(blurb_lines(
        None,
        2,
        &intro,
        width,
        Style::default().fg(Color::Gray),
    ));
    if unlocked {
        lines.push(Line::from(Span::styled(
            format!("  威力{dmg:.2}  間隔{interval}  斉射×{volley}"),
            Style::default().fg(Color::DarkGray),
        )));
        // 簡易ステータスバー
        let power_lv = state.weapon_stat(w, WeaponStat::Power);
        let rate_lv = state.weapon_stat(w, WeaponStat::Rate);
        let count_lv = state.weapon_stat(w, WeaponStat::Count);
        let cells = bar_cells(width);
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("弾{}", bar(count_lv, 7, cells)),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" "),
            Span::styled(
                format!("連{}", bar(rate_lv, 8, cells)),
                Style::default().fg(Color::LightYellow),
            ),
            Span::raw(" "),
            Span::styled(
                format!("威{}", bar(power_lv, 8, cells)),
                Style::default().fg(Color::LightRed),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "  次層を開放して手札を増やそう",
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}

/// 強化バー 1 本の升目数。行頭の字下げ 2 桁・全角ラベル 3 つ・バーの間の
/// 空白 2 桁を除いた残りを 3 本で分ける。
fn bar_cells(width: u16) -> usize {
    const FIXED: usize = 2 + 3 * 2 + 2;
    ((width as usize).saturating_sub(FIXED) / 3).clamp(2, 8)
}

/// レベルを `cells` 個の升目で表す進捗バー。
///
/// 升目数は幅に合わせて縮むので、満たす割合は上限レベル `max_lv` との比で
/// 決める — 狭い画面でも「どこまで伸ばしたか」の読み取りが変わらない。
fn bar(lv: u32, max_lv: u32, cells: usize) -> String {
    let max_lv = max_lv.max(1) as usize;
    let filled = (lv.min(max_lv as u32) as usize * cells).div_ceil(max_lv);
    format!("{}{}", "█".repeat(filled), "░".repeat(cells - filled))
}

fn weapon_art(w: WeaponKind) -> &'static str {
    match w {
        WeaponKind::Pulse => "· › · › · ›",
        WeaponKind::Ray => "════════▷",
        WeaponKind::Scatter => "  ※ ※ ※",
        WeaponKind::Arc => "  ☾  ～▷",
        WeaponKind::Nova => "  ·→✸←·",
    }
}

fn weapon_color(w: WeaponKind) -> Color {
    match w {
        WeaponKind::Pulse => Color::Cyan,
        WeaponKind::Ray => Color::White,
        WeaponKind::Scatter => Color::Yellow,
        WeaponKind::Arc => Color::LightMagenta,
        WeaponKind::Nova => Color::LightRed,
    }
}

fn render_ring(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 環 ");
    let (wrap_w, spaced) = fit_tab_layout(block.inner(area), |w| ring_sections(state, w));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(ring_sections(state, wrap_w), spaced),
        &state.tab_scroll,
        TAB_SCROLL_UP,
        TAB_SCROLL_DOWN,
    )
    .block(block)
    .arrow_color(Color::Yellow)
    .render(f, area, &mut cs);
}

/// 環タブの行。層の進捗 / 次層開放 / 見出し / 強化項目のかたまりに分ける。
/// `width` は説明文を折り返す桁数。
fn ring_sections(state: &StarRingState, width: u16) -> Vec<Section> {
    let layer = state.layer();
    let next = Layer::next_threshold(layer);
    let progress = match next {
        Some(th) => {
            let prev = Layer::entry_threshold(layer);
            let span = th.saturating_sub(prev).max(1);
            let done = state.total_kills.saturating_sub(prev);
            ((done * 10) / span).min(10)
        }
        None => 10,
    };
    let bar = format!(
        "{}{}",
        "█".repeat(progress as usize),
        "░".repeat(10 - progress as usize)
    );

    let mut sections = Vec::new();
    let mut head = wrapped_row(
        Span::raw(" "),
        format!("第{}層 {}  {}", layer, Layer::title(layer), bar),
        Style::default()
            .fg(layer_color(layer))
            .add_modifier(Modifier::BOLD),
        width,
    );
    // 撃破進捗と湧き倍率を1行にまとめ、狭い画面で強化行が押し出されないようにする。
    head.extend(wrapped_row(
        Span::raw(" "),
        match next {
            Some(th) => format!(
                "撃破{}/{}  湧き×{} HP×{:.1} ✦×{:.1}",
                state.total_kills,
                th,
                Layer::spawn_batch(layer),
                Layer::hp_mult(layer),
                Layer::value_mult(layer)
            ),
            None => format!(
                "撃破{}  湧き×{} HP×{:.1} ✦×{:.1}",
                state.total_kills,
                Layer::spawn_batch(layer),
                Layer::hp_mult(layer),
                Layer::value_mult(layer)
            ),
        },
        Style::default().fg(Color::DarkGray),
        width,
    ));
    sections.push(Section::plain(head));

    if let Some(th) = next {
        let next_layer = layer + 1;
        let cost = layer_unlock_cost(state);
        let kills_ready = state.total_kills >= th;
        let can = can_unlock_next_layer(state);
        if kills_ready {
            let style = if can {
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label = if can {
                format!(
                    "[!] 第{}層「{}」を開放 ✦{}",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            } else {
                format!(
                    "[!] 第{}層「{}」 要✦{} (不足)",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            };
            let line = wrapped_row(Span::raw(" "), label, style, width);
            sections.push(if can {
                Section::clickable(line, OPEN_LAYER)
            } else {
                Section::plain(line)
            });
        } else {
            sections.push(Section::plain(wrapped_row(
                Span::raw(" "),
                format!(
                    "次層「{}」 撃破{} ✦{}",
                    Layer::title(next_layer),
                    th,
                    format_shards(cost)
                ),
                Style::default().fg(Color::DarkGray),
                width,
            )));
        }
    }

    sections.push(Section::plain(wrapped_row(
        Span::raw(" "),
        "環の強化".to_string(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        width,
    )));

    let keys = ['1', '2'];
    for (i, kind) in RingUpgrade::ALL.iter().copied().enumerate() {
        let unlocked = state.is_ring_unlocked(kind);
        let lv = state.ring_level(kind);
        let maxed = unlocked && !can_upgrade_ring(state, kind);
        let cost = ring_upgrade_cost(state, kind);
        let can = unlocked && !maxed && state.shards + 1e-9 >= cost;
        let key = keys.get(i).copied().unwrap_or('?');
        let style = if !unlocked {
            Style::default().fg(Color::DarkGray)
        } else if can {
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let cost_label = if !unlocked {
            format!("L{}", kind.unlock_layer())
        } else if maxed {
            "MAX".to_string()
        } else {
            format!("✦{}", format_shards(cost))
        };
        let head = if unlocked {
            Line::from(vec![
                Span::styled(format!(" [{key}] "), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{} Lv.{} ", kind.label(), lv), style),
                Span::styled(cost_label, Style::default().fg(Color::Cyan)),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!(" [{key}] "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}  ", kind.label()), style),
                Span::styled(cost_label, Style::default().fg(Color::DarkGray)),
            ])
        };
        let mut lines = vec![head];
        lines.extend(blurb_lines(
            None,
            BLURB_INDENT,
            kind.blurb(),
            width,
            Style::default().fg(Color::DarkGray),
        ));
        sections.push(if unlocked {
            Section::clickable(lines, buy_ring_id(kind))
        } else {
            Section::plain(lines)
        });
    }

    sections
}

fn render_codex(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 図鑑 ");
    let (wrap_w, spaced) = fit_tab_layout(block.inner(area), |w| codex_sections(state, w));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(codex_sections(state, wrap_w), spaced),
        &state.tab_scroll,
        TAB_SCROLL_UP,
        TAB_SCROLL_DOWN,
    )
    .block(block)
    .arrow_color(Color::Yellow)
    .render(f, area, &mut cs);
}

/// 図鑑の行。進捗 / 鉱石 / 累計 / 武装の 4 かたまりに分ける。
/// `width` は行を折り返す桁数。
fn codex_sections(state: &StarRingState, width: u16) -> Vec<Section> {
    let unlocked = state.unlocked_ore_kinds();
    let layer = state.layer();
    let dim = Style::default().fg(Color::DarkGray);

    let progress = Section::plain(wrapped_row(
        Span::raw(" "),
        format!(
            "第{}層  累計撃破 {}  逸失 {}",
            layer, state.total_kills, state.missed_count
        ),
        dim,
        width,
    ));

    let mut ores = Vec::new();
    for kind in OreKind::ALL {
        if unlocked.contains(&kind) {
            ores.extend(wrapped_row(
                Span::styled(
                    format!(" ◆ {} ", kind.label()),
                    Style::default()
                        .fg(ore_color(kind))
                        .add_modifier(Modifier::BOLD),
                ),
                format!(
                    "価値{} HP{:.0}",
                    kind.base_value(),
                    kind.base_hp() * Layer::hp_mult(layer)
                ),
                Style::default().fg(Color::Gray),
                width,
            ));
        } else {
            ores.extend(wrapped_row(
                Span::styled(" ？ ", dim),
                format!("第{}層で出現", kind.unlock_layer()),
                dim,
                width,
            ));
        }
    }

    let earned = Section::plain(wrapped_row(
        Span::raw(" "),
        format!("獲得累計 ✦{}", format_shards(state.shards_earned)),
        dim,
        width,
    ));

    let mut weapons = wrapped_row(
        Span::raw(" "),
        "武装解放".to_string(),
        Style::default().fg(Color::Yellow),
        width,
    );
    for w in WeaponKind::ALL {
        if state.is_weapon_unlocked(w) {
            weapons.extend(wrapped_row(
                Span::styled(format!("  {} ", w.glyph()), Style::default().fg(weapon_color(w))),
                format!("{} 解放済", w.label()),
                Style::default().fg(weapon_color(w)),
                width,
            ));
        } else {
            weapons.extend(wrapped_row(
                Span::styled("  ？ ", dim),
                format!("{}  第{}層", w.label(), w.unlock_layer()),
                dim,
                width,
            ));
        }
    }

    vec![
        progress,
        Section::plain(ores),
        earned,
        Section::plain(weapons),
    ]
}

fn ore_color(kind: OreKind) -> Color {
    match kind {
        OreKind::Dust => Color::Gray,
        OreKind::Rock => Color::Yellow,
        OreKind::Crystal => Color::LightCyan,
        OreKind::Wisp => Color::White,
        OreKind::Prism => Color::LightMagenta,
        OreKind::Shell => Color::DarkGray,
        OreKind::Splitter => Color::LightYellow,
        OreKind::Nova => Color::LightRed,
    }
}

/// ステージ。ワールド座標をそのまま Canvas へ渡し、braille の点描で描く。
///
/// x 表示範囲はワールド幅に固定し、フィールドが常に画面幅いっぱいへ広がる
/// ようにする。braille は 1 セル = 横2×縦4 ドットなので、Rect の 列:行 が
/// 2:1 に近いほど円が真円に近づく。狭い端末で左右へ余白を作って等方性を
/// 取りにいくと、鉱石を見分けられる横解像度そのものが減ってしまう。
fn render_stage(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layer = state.layer();
    let title = if state.layer_flash_ticks > 0 {
        format!(" ◆開放 第{}層 {} ", layer, Layer::title(layer))
    } else if can_unlock_next_layer(state) {
        format!(" 次層開放可「{}」[!]", Layer::title(layer + 1))
    } else if state.layer_ready_flash_ticks > 0 {
        " 撃破条件達成 — 星屑で開放 ".to_string()
    } else {
        format!(" 情景 砲×{} ", state.turret_count())
    };
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(layer_color(layer)))
        .title(Span::styled(title, Style::default().fg(Color::Yellow)));

    let inner = block.inner(area);
    if inner.width < 2 || inner.height < 2 {
        // 点描が成立しない狭さ。枠だけ描き、タップ面だけは維持する。
        Clickable::new(block, TAP_STRIKE).render(f, area, &mut click_state.borrow_mut());
        return;
    }

    let sample_step = fill_step(inner);
    let shake = Shake::new(state);
    let (core_x, core_y) = shake.point(CX, CORE_Y);

    // 砲台環。コアを中心とする横長の楕円で、砲台の通り道をなぞる。点は弧長
    // 一定で打つ。角度を一定に刻むと横長の楕円では上下だけ点が開き、砲台が
    // 通る帯で環の粒度が変わってしまう。
    let (ring_rx, ring_ry) = state.ring_radii();
    let mut orbit_pts = Vec::new();
    let mut orbit_a = 0.0;
    while orbit_a < std::f64::consts::TAU {
        let (sin, cos) = orbit_a.sin_cos();
        orbit_pts.push(shake.point(CX + cos * ring_rx, CORE_Y + sin * ring_ry));
        orbit_a += ORBIT_DOT_SPACING / (ring_rx * sin).hypot(ring_ry * cos).max(0.01);
    }

    let core_scale = core_scale(state);
    let core_pts = shake.circle(CX, CORE_Y, CORE_RADIUS * core_scale, sample_step);
    let core_halo = canvas_fx::ring_points(core_x, core_y, CORE_HALO_RADIUS * core_scale, 0.26);

    // 採掘境界。鉱石が湧いてくる高さに水平の点線を引き、そこから上が
    // 今の層の外側だと示す。層が上がるほど点が詰まって濃くなる。
    let mut boundary_pts = Vec::new();
    let boundary_step = (3.4 - (layer.min(7).saturating_sub(1) as f64) * 0.4).max(1.2);
    let mut bx = FIELD_MARGIN;
    while bx <= WORLD_W - FIELD_MARGIN {
        boundary_pts.push(shake.point(bx, SPAWN_Y));
        bx += boundary_step;
    }

    // フィールドの左右境界。鉱石はここで跳ね返る。x 表示範囲がワールド幅
    // なので壁はほぼ画面端に来る — 連続した点線だと縁が騒がしくなるだけ
    // なので、端があると分かる程度まで間引いたアクセントに留める。
    let mut wall_pts = Vec::new();
    let mut wy = 0.0;
    while wy <= WORLD_H {
        wall_pts.push(shake.point(FIELD_MARGIN, wy));
        wall_pts.push(shake.point(WORLD_W - FIELD_MARGIN, wy));
        wy += 12.0;
    }

    // 砲台。環の下半分 (sin < 0) が視点に近い手前側。
    let near_radius = turret_radius(ring_ry);
    let mut gun_near = Vec::new();
    let mut gun_far = Vec::new();
    for &(gx, gy, depth) in &turret_positions(state) {
        let near = depth <= 0.0;
        let r = if near {
            near_radius
        } else {
            near_radius * TURRET_FAR_SCALE
        };
        let pts = shake.circle(gx, gy, r, sample_step);
        if near {
            gun_near.extend(pts);
        } else {
            gun_far.extend(pts);
        }
    }

    let mut ore_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut approach_trails: Vec<((f64, f64, f64, f64), Color)> = Vec::new();
    for ore in &state.ores {
        let color = ore_color(ore.kind);
        let pts = shake.circle(ore.x, ore.y, ore.radius, sample_step);
        if let Some(g) = ore_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            ore_groups.push((pts, color));
        }
        let len = ore.radius * 2.8 + ore.vx.hypot(ore.vy).max(0.01) * 3.0;
        approach_trails.push((shake.trail(ore.x, ore.y, ore.vx, ore.vy, len), color));
    }

    // 飛翔弾を武器色で描画
    let mut proj_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut proj_trails: Vec<((f64, f64, f64, f64), Color)> = Vec::new();
    for p in &state.projectiles {
        let color = weapon_color(p.kind);
        let pts = shake.circle(p.x, p.y, p.radius, sample_step);
        if let Some(g) = proj_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            proj_groups.push((pts, color));
        }
        // 尾の長さは弾半径の倍数で持つ。尾は「弾本体より何倍長いか」で見え方が
        // 決まるので、ワールド単位の固定長にすると弾の大きさを変えた途端に尾が
        // 本体へ飲まれ、飛んでいる向きが読めなくなる。
        let len = p.radius
            * match p.kind {
                WeaponKind::Ray => 6.4,
                WeaponKind::Scatter => 4.8,
                WeaponKind::Arc => 3.7,
                WeaponKind::Pulse => 3.3,
                WeaponKind::Nova => 2.2,
            };
        proj_trails.push((shake.trail(p.x, p.y, p.vx, p.vy, len), color));
    }

    let mut sparks = Vec::new();
    let mut dust = Vec::new();
    let mut shards = Vec::new();
    let mut embers = Vec::new();
    for p in &state.particles {
        let pt = shake.point(p.x, p.y);
        match p.kind {
            ParticleKind::Spark => sparks.push(pt),
            ParticleKind::Dust => dust.push(pt),
            ParticleKind::Shard => shards.push(pt),
            ParticleKind::Ember => embers.push(pt),
        }
    }

    // 核脈動の波面。判定 (`logic::pulse_wave_damage`) はコアからの等方距離なので
    // 描画も真円で、削る半径そのものを描く。
    let mut pulse_ring_pts: Vec<(f64, f64)> = Vec::new();
    for ring in &state.pulse_rings {
        let alpha = ring.life as f64 / ring.max_life.max(1) as f64;
        let density = if alpha > 0.5 { 1.0 } else { 1.7 };
        push_pulse_wave_points(core_x, core_y, ring.radius, density, &mut pulse_ring_pts);
    }

    // 背景星は上から下へ流れ、フィールド内に「降ってくる場」の向きを与える。
    // 壁の外へ散らすと鉱石が動ける範囲が曖昧になるので左右の壁で挟む。
    // ナローは点が潰れるので数を抑える。
    let star_count = if is_narrow {
        10
    } else {
        16 + (layer.min(6) as usize) * 3
    };
    let star_lo = FIELD_MARGIN;
    let star_hi = WORLD_W - FIELD_MARGIN;
    let drift = state.elapsed_ticks as f64 * 0.06;
    let mut stars = Vec::with_capacity(star_count);
    for i in 0..star_count {
        let seed = i as f64 * 7.13;
        let fx = (seed * 11.0).sin().abs();
        let x = star_lo + (star_hi - star_lo) * fx;
        let y = (seed * 17.3 - drift).rem_euclid(WORLD_H);
        stars.push(shake.point(x, y));
    }

    let core_color = if state.layer_flash_ticks > 0 {
        layer_color(layer)
    } else if state.layer_ready_flash_ticks > 0 || can_unlock_next_layer(state) {
        Color::LightMagenta
    } else if state.boost_ticks > 0 {
        Color::LightYellow
    } else {
        Color::Yellow
    };
    let star_color = match layer {
        1 => Color::DarkGray,
        2 => Color::Indexed(240),
        3 => Color::Indexed(81),
        4 => Color::Indexed(177),
        _ => Color::Indexed(210),
    };
    let boundary_color = layer_color(layer);

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if !stars.is_empty() {
                ctx.draw(&Points {
                    coords: &stars,
                    color: star_color,
                });
            }
            if !boundary_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &boundary_pts,
                    color: boundary_color,
                });
            }
            if !wall_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &wall_pts,
                    color: Color::Indexed(236),
                });
            }
            if !pulse_ring_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &pulse_ring_pts,
                    color: PULSE_WAVE_COLOR,
                });
            }
            if !orbit_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &orbit_pts,
                    color: Color::Indexed(238),
                });
            }
            if !gun_far.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_far,
                    color: Color::Indexed(250),
                });
            }
            for &((x1, y1, x2, y2), color) in &approach_trails {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                });
            }
            for (pts, color) in &ore_groups {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            for &((x1, y1, x2, y2), color) in &proj_trails {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                });
            }
            for (pts, color) in &proj_groups {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            // 核の暈は背景側の装飾なので砲台より先に置く。手前を通る砲台が
            // 暈の点で虫食いになると、何基あるかを数える手がかりが濁る。
            if !core_halo.is_empty() {
                ctx.draw(&Points {
                    coords: &core_halo,
                    color: Color::DarkGray,
                });
            }
            if !gun_near.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_near,
                    color: Color::White,
                });
            }
            if !core_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &core_pts,
                    color: core_color,
                });
            }
            if !dust.is_empty() {
                ctx.draw(&Points {
                    coords: &dust,
                    color: Color::Gray,
                });
            }
            if !shards.is_empty() {
                ctx.draw(&Points {
                    coords: &shards,
                    color: Color::LightMagenta,
                });
            }
            if !embers.is_empty() {
                ctx.draw(&Points {
                    coords: &embers,
                    color: Color::LightRed,
                });
            }
            if !sparks.is_empty() {
                ctx.draw(&Points {
                    coords: &sparks,
                    color: Color::White,
                });
            }
        })
        .block(block);

    Clickable::new(canvas, TAP_STRIKE).render(f, area, &mut click_state.borrow_mut());
}

/// フッターの案内は幅に入るぶんだけ前から採る。ここで区切りに使う空白。
const FOOTER_GAP: &str = "  ";
/// 幅がいくら狭くても残す案内。ここが切り落とされるとゲームから出られなくなる。
const FOOTER_BACK: &str = "[Q]戻る";

/// フッター 1 行の文言。優先度の高い順に並べた案内を、`width` 桁に収まるところ
/// まで採用して連結する。
///
/// 端末幅は 38 桁ほどまで下がる一方、日本語は 1 文字 2 桁を食う。全部を並べると
/// 末尾から溢れるので、落とす順序をこちらで決めて `[Q]戻る` を必ず残す。
/// タブ送りは 3 タブとも `StarRingState::scroll_tab` で共通なので、どのタブでも
/// 案内する。
fn footer_hint(tab: Tab, is_narrow: bool, width: u16) -> String {
    let parts: &[&str] = match (tab, is_narrow) {
        (Tab::Armory, false) => &[
            "[A/S/D]弾数/連射/威力",
            "[J/K]送り",
            "[T]/情景タップでブースト",
            "[◀▶]武装",
        ],
        (Tab::Armory, true) => &["[A/S/D]強化", "[J/K]送り", "[T]ブースト"],
        (Tab::Ring, false) => &["[!]次層開放", "[1-2]収率/核脈動", "[J/K]送り"],
        (Tab::Ring, true) => &["[!]開放", "[1-2]強化", "[J/K]送り"],
        (Tab::Codex, false) => &["[J/K]送り", "層開放で鉱石と武装が増える"],
        (Tab::Codex, true) => &["[J/K]送り", "層開放で鉱石が増える"],
    };
    let cells = |s: &str| Span::raw(s).width();
    let gap = cells(FOOTER_GAP);
    let mut used = cells(FOOTER_BACK);
    let mut out = String::new();
    for part in parts {
        let cost = cells(part) + gap;
        if used + cost > width as usize {
            break;
        }
        used += cost;
        out.push_str(part);
        out.push_str(FOOTER_GAP);
    }
    out.push_str(FOOTER_BACK);
    out
}

fn render_footer(state: &StarRingState, f: &mut Frame, area: Rect, is_narrow: bool) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer_hint(state.tab, is_narrow, area.width),
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    use crate::games::starringe::actions::buy_ring_id;
    use crate::games::starringe::logic::{unlock_next_layer, NOVA_PROJECTILE_RADIUS};
    use crate::games::starringe::state::{Layer, Ore, OreMotion, RingUpgrade, Tab};

    fn render_frame(state: &StarRingState, width: u16, height: u16) -> Rc<RefCell<ClickState>> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = width;
        cs.borrow_mut().terminal_rows = height;
        terminal
            .draw(|f| render(state, f, f.area(), &cs))
            .unwrap();
        cs
    }

    fn has_action(cs: &Rc<RefCell<ClickState>>, width: u16, height: u16, action_id: u16) -> bool {
        let guard = cs.borrow();
        for y in 0..height {
            for x in 0..width {
                if guard.hit_test(x, y) == Some(action_id) {
                    return true;
                }
            }
        }
        false
    }

    /// バッファの 1 行を文字列へ戻す。全角文字は継続セルを伴うので、
    /// セルを素直に連結すると文字の間に空白が挟まる。
    fn row_text(buf: &ratzilla::ratatui::buffer::Buffer, y: u16, width: u16) -> String {
        let mut out = String::new();
        let mut x = 0u16;
        while x < width {
            let sym = buf[(x, y)].symbol();
            out.push_str(if sym.is_empty() { " " } else { sym });
            x += Span::raw(sym).width().max(1) as u16;
        }
        out
    }

    #[test]
    fn narrow_ring_tab_exposes_scroll_when_layer_unlock_rows_are_present() {
        // 40×30 の狭い画面では環ペインが短く、層開放行を足すと核脈動が
        // はみ出す。ScrollableTab により ▼ が出てスクロールできることを見る。
        let mut state = StarRingState::new();
        state.tab = Tab::Ring;
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        state.layer_flash_ticks = 0;

        let (w, h) = (40u16, 30u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let buf = terminal.backend().buffer();
        let row_text = |y: u16| -> String {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        };
        let has_scroll = (0..h).any(|y| {
            let t = row_text(y);
            t.contains('▼') || t.contains('▲')
        });
        assert!(
            has_scroll
                || has_action(&cs, w, h, TAB_SCROLL_DOWN)
                || has_action(&cs, w, h, buy_ring_id(RingUpgrade::CorePulse)),
            "狭い画面でも核脈動へ届く手段 (スクロール or 直接表示) があるはず"
        );
    }

    #[test]
    fn scrolled_ring_tab_keeps_core_pulse_clickable_on_short_viewport() {
        let mut state = StarRingState::new();
        state.tab = Tab::Ring;
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        state.layer_flash_ticks = 0;
        // 先頭の層情報を送って核脈動行を可視領域へ入れる。
        state.tab_scroll.set(4);

        let cs = render_frame(&state, 40, 30);
        assert!(
            has_action(&cs, 40, 30, buy_ring_id(RingUpgrade::CorePulse)),
            "スクロール後は核脈動の購入行がクリックできるはず"
        );
    }

    /// 端末に近い幅で、説明文が全文残ったまま幅に収まること。
    ///
    /// 折り返しは行を増やすかわりに、1 行が幅を超えないことで初めて意味を持つ。
    /// 幅を超えた行は描画側が右端で捨ててしまい、省略記号も出ない。
    #[test]
    fn blurb_lines_keep_the_whole_text_within_the_width() {
        let texts: Vec<&str> = WeaponStat::ALL
            .iter()
            .map(|s| s.blurb())
            .chain(RingUpgrade::ALL.iter().map(|k| k.blurb()))
            .chain(WeaponKind::ALL.iter().map(|w| w.blurb()))
            .collect();

        for width in [20u16, 32, 33, 35, 41, 72] {
            for rail in [None, Some(Style::default())] {
                for indent in [2usize, BLURB_INDENT] {
                    for text in &texts {
                        let lines =
                            blurb_lines(rail, indent, text, width, Style::default());
                        let mut joined = String::new();
                        for line in &lines {
                            assert!(
                                line.width() <= width as usize,
                                "{width}桁: 折り返した行が幅を超えている ({}桁) {line:?}",
                                line.width()
                            );
                            for span in line.spans.iter() {
                                joined.push_str(span.content.as_ref());
                            }
                        }
                        let restored: String = joined
                            .chars()
                            .filter(|c| *c != ' ' && *c != '│')
                            .collect();
                        let want: String = text.chars().filter(|c| *c != ' ').collect();
                        assert_eq!(
                            restored, want,
                            "{width}桁 (indent={indent}): 説明文が欠けている"
                        );
                    }
                }
            }
        }
    }

    /// 内容が溢れる領域では、`ScrollableTab` がスクロール列へ回す 1 桁を
    /// 差し引いた幅で行を組むこと。ここがずれると行の右端がスクロール列に隠れる。
    #[test]
    fn fit_tab_layout_leaves_room_for_the_scroll_column() {
        let state = StarRingState::new();
        let inner = Rect::new(0, 0, 33, 6);
        let (wrap_w, spaced) = fit_tab_layout(inner, |w| armory_sections(&state, w));
        assert!(!spaced, "溢れている時に装飾の空行は挟まない");
        assert_eq!(
            wrap_w,
            inner.width - 1,
            "スクロール列の 1 桁を引いた幅で組んでいない"
        );
    }

    /// 3 タブとも、`fit_tab_layout` に渡した桁数へ収まる行を返すこと。
    ///
    /// この契約が崩れると、溢れた時の「1 桁狭い幅で組み直す」が同じ結果を
    /// 作り直すだけの空振りになり、右端がスクロール列の下へ隠れる。
    ///
    /// 桁数はタブペインが実際に取る値の範囲で見る。下限の 32 桁は、いちばん
    /// 狭いモバイル (幅 33) でスクロール列へ 1 桁を回した時の幅。数字は
    /// 桁が伸びる側で効くので、累計が大きく育った state で見る。
    #[test]
    fn every_tab_builds_rows_that_fit_the_width_it_is_given() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        state.total_kills = 123_456;
        state.missed_count = 98_765;
        state.shards_earned = 1.2e7;

        for (label, make) in [
            (
                "armory",
                armory_sections as fn(&StarRingState, u16) -> Vec<Section>,
            ),
            ("ring", ring_sections),
            ("codex", codex_sections),
        ] {
            for width in [32u16, 33, 34, 41, 72] {
                for line in build_list(make(&state, width), false).lines() {
                    assert!(
                        line.width() <= width as usize,
                        "{label} ({width}桁): 行が幅を超えている ({}桁) {line:?}",
                        line.width()
                    );
                }
            }
        }
    }

    /// 折り返して増えた説明行も、見出し行と同じ購入のクリック領域に入ること。
    #[test]
    fn wrapped_blurb_rows_stay_clickable() {
        let (w, h) = (108u16, 36u16);
        let mut state = StarRingState::new();
        state.tab = Tab::Ring;
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        state.layer_flash_ticks = 0;

        let wanted = buy_ring_id(RingUpgrade::CorePulse);
        let inner_w = split_body(split_frame(Rect::new(0, 0, w, h))[2], false)
            .1
            .width
            - 2;
        let blurb_rows = blurb_lines(
            None,
            BLURB_INDENT,
            RingUpgrade::CorePulse.blurb(),
            inner_w,
            Style::default(),
        )
        .len();
        assert!(blurb_rows >= 2, "この幅では説明文が折り返る前提の検査");

        let cs = render_frame(&state, w, h);
        let guard = cs.borrow();
        let rows = (0..h)
            .filter(|&y| (0..w).any(|x| guard.hit_test(x, y) == Some(wanted)))
            .count();
        assert!(
            rows > blurb_rows,
            "折り返した説明行が当たり判定から漏れている ({rows} 行)"
        );
    }

    /// モバイル幅でも購入項目が 2 つ以上見えること。折り返しで行が増えるほど
    /// 一度に見える項目は減るが、比べる相手が無い画面は投資の判断に使えない。
    #[test]
    fn mobile_shows_at_least_two_purchase_rows() {
        let (w, h) = (33u16, 38u16);
        for tab in [Tab::Armory, Tab::Ring] {
            let mut state = StarRingState::new();
            state.tab = tab;
            state.total_kills = Layer::THRESHOLDS[1];
            state.shards = 1e9;
            assert!(unlock_next_layer(&mut state));
            state.layer_flash_ticks = 0;

            let wanted: Vec<u16> = match tab {
                Tab::Armory => WeaponStat::ALL
                    .iter()
                    .map(|s| buy_weapon_stat_id(state.selected_weapon, *s))
                    .collect(),
                _ => RingUpgrade::ALL.iter().map(|k| buy_ring_id(*k)).collect(),
            };
            let cs = render_frame(&state, w, h);
            let visible = wanted
                .iter()
                .filter(|id| has_action(&cs, w, h, **id))
                .count();
            assert!(
                visible >= 2,
                "{tab:?}: モバイルで見えている購入項目が {visible} 個しかない"
            );
        }
    }

    /// 折り返さない情報 (武器名・強化バー) が、実機幅のどれでも切り詰められずに
    /// 出ること。折り返すのは説明文だけで、1 行に収める情報はパネルの幅が
    /// 下限を割ると読めなくなる。
    #[test]
    fn single_line_info_survives_every_device_width() {
        for (w, h) in [(108u16, 36u16), (33, 38), (40, 30), (38, 20)] {
            let mut state = StarRingState::new();
            state.tab = Tab::Armory;

            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();

            let rows: Vec<String> = (0..h).map(|y| row_text(buf, y, w)).collect();
            let picker = rows
                .iter()
                .find(|r| r.contains('▶'))
                .expect("武器ピッカーの行が見つからない");
            assert!(
                picker.contains(state.selected_weapon.label()),
                "{w}x{h}: 選択中の武器名が読めない {picker}"
            );

            // バーの升目数は、実際に行を組んだ桁数から決まる。枠の内側 (ワイドは
            // 左右 2 桁ぶん狭い) と、内容が溢れた時にスクロール列へ回る 1 桁の
            // どちらも `render_armory` と同じ手順で引く。
            let narrow = is_narrow_layout(w);
            let borders = if narrow {
                Borders::TOP | Borders::BOTTOM
            } else {
                Borders::ALL
            };
            let tab_area = split_body(split_frame(Rect::new(0, 0, w, h))[2], narrow).1;
            let inner = Block::default().borders(borders).inner(tab_area);
            let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1);
            let (wrap_w, _) = fit_tab_layout(body, |cols| armory_sections(&state, cols));

            let bar_row = rows
                .iter()
                .find(|r| r.contains('弾') && r.contains('威'))
                .expect("強化バーの行が見つからない");
            let cells = bar_cells(wrap_w);
            assert_eq!(
                bar_row.matches('░').count(),
                cells * 3,
                "{w}x{h}: 強化バーの升目が欠けている {bar_row}"
            );
        }
    }

    /// バーは升目数が変わっても、伸ばした割合が同じに読めること。
    #[test]
    fn bar_scales_to_the_cells_it_is_given() {
        for cells in [3usize, 5, 8] {
            assert_eq!(bar(0, 8, cells).matches('█').count(), 0);
            assert_eq!(bar(8, 8, cells).matches('█').count(), cells);
            assert_eq!(bar(99, 8, cells).matches('█').count(), cells);
            assert_eq!(bar(0, 8, cells).chars().count(), cells);
        }
        assert_eq!(bar(4, 8, 8).matches('█').count(), 4);
        assert_eq!(bar(4, 8, 4).matches('█').count(), 2);
        // 升目より上限レベルが大きくても、1 レベル目で必ず升目が 1 つ灯る。
        assert_eq!(bar(1, 8, 4).matches('█').count(), 1);
    }

    /// 購入項目は見出し行と説明行のどちらを叩いても同じ購入が走ること。
    /// 指の当たる面が広いほどモバイルで押しやすく、武装タブと環タブで
    /// 当たり方が違うと「押せる行」を探させることになる。
    #[test]
    fn purchase_rows_take_taps_on_their_blurb_line_too() {
        let (w, h) = (100u16, 30u16);
        for (tab, wanted) in [
            (
                Tab::Armory,
                buy_weapon_stat_id(WeaponKind::Pulse, WeaponStat::Count),
            ),
            (Tab::Ring, buy_ring_id(RingUpgrade::Yield)),
        ] {
            let mut state = StarRingState::new();
            state.tab = tab;
            let cs = render_frame(&state, w, h);
            let guard = cs.borrow();
            let rows = (0..h)
                .filter(|&y| (0..w).any(|x| guard.hit_test(x, y) == Some(wanted)))
                .count();
            assert!(
                rows >= 2,
                "{tab:?}: 購入項目の当たり判定が {rows} 行しかない (見出し+説明の2行を想定)"
            );
        }
    }

    /// フッターは幅が狭くても `[Q]戻る` を切り落とさず、スクロールできる 3 タブ
    /// すべてで送りの案内を出すこと。
    #[test]
    fn footer_keeps_back_and_scroll_hints_within_width() {
        for w in [38u16, 40, 60, 80, 100] {
            for tab in [Tab::Armory, Tab::Ring, Tab::Codex] {
                let hint = footer_hint(tab, is_narrow_layout(w), w);
                let cells = Span::raw(hint.as_str()).width();
                assert!(
                    cells <= w as usize,
                    "{w}桁 {tab:?}: フッターが幅を超えている ({cells}桁) {hint}"
                );
                assert!(
                    hint.ends_with(FOOTER_BACK),
                    "{w}桁 {tab:?}: 戻る案内が残っていない {hint}"
                );
                assert!(
                    hint.contains("[J/K]"),
                    "{w}桁 {tab:?}: 送りの案内が出ていない {hint}"
                );
            }
        }
    }

    /// ヘッダー・タブ・フッターの固定消費が 4 行に収まり、残りがすべて
    /// 本体へ回ること。30 行しかないモバイルでは、ここが 1 行増えるだけで
    /// ステージの情報量が直接削れる。
    #[test]
    fn frame_chrome_costs_four_rows() {
        let area = Rect::new(0, 0, 100, 30);
        let [header, tabs, body, footer] = split_frame(area);
        assert_eq!((header.height, tabs.height, footer.height), (2, 1, 1));
        assert_eq!(body.height, 26);
    }

    /// ステージが本体の過半を取ること。ワイドは横幅、ナローは高さで見る。
    #[test]
    fn stage_takes_the_larger_share_of_the_body() {
        let body = Rect::new(0, 4, 100, 26);
        let (stage, tab) = split_body(body, false);
        assert!(
            stage.width > tab.width,
            "ワイドではステージが左パネルより広いはず ({} vs {})",
            stage.width,
            tab.width
        );
        // 左パネルは説明文を折り返して幅を詰める前提なので、ステージが本体の
        // 2/3 近くを取る。ここが痩せると鉱石の落下を追う面が削れる。
        assert!(
            stage.width * 100 >= body.width * 64,
            "ワイドのステージが本体の 64% に届いていない ({} / {})",
            stage.width,
            body.width
        );
        assert_eq!(stage.height, body.height);

        let narrow_body = Rect::new(0, 4, 40, 26);
        let (stage, tab) = split_body(narrow_body, true);
        assert!(
            stage.height > tab.height,
            "ナローではステージがタブ内容より高いはず ({} vs {})",
            stage.height,
            tab.height
        );
        assert!(
            stage.height >= 15,
            "ナローのステージは 15 行以上ないと落下が追えない (実際 {})",
            stage.height
        );
    }

    /// フィールドが画面幅を使い切ること。左右の壁ぎわに置いた鉱石が、
    /// ステージ内側の両端 2 列以内へ届くかで見る。
    #[test]
    fn stage_field_reaches_both_screen_edges() {
        use crate::games::starringe::state::{Ore, OreMotion};

        let mut state = StarRingState::new();
        for x in [FIELD_MARGIN, WORLD_W - FIELD_MARGIN] {
            state.ores.push(Ore {
                x,
                y: 60.0,
                vx: 0.0,
                vy: -0.3,
                hp: 5.0,
                kind: OreKind::Crystal,
                radius: OreKind::Crystal.radius(),
                motion: OreMotion::Spiral,
                sway: 0.05,
                age: 10,
            });
        }

        for (w, h) in [(100u16, 30u16), (40, 30), (38, 20)] {
            // レイアウトの分岐は render と同じ判定から引く。閾値が動いたときに
            // 実描画と別の Rect を検査したまま通ることがないようにする。
            let narrow = is_narrow_layout(w);
            let area = Rect::new(0, 0, w, h);
            let (stage, _) = split_body(split_frame(area)[2], narrow);
            let borders = if narrow {
                Borders::TOP | Borders::BOTTOM
            } else {
                Borders::ALL
            };
            let inner = Block::default().borders(borders).inner(stage);

            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();

            let ore = ore_color(OreKind::Crystal);
            let column_has_ore = |x: u16| -> bool {
                (inner.y..inner.y + inner.height).any(|y| buf[(x, y)].fg == ore)
            };
            let left = inner.x..inner.x + 2;
            let right = inner.x + inner.width - 2..inner.x + inner.width;
            assert!(
                left.clone().any(column_has_ore),
                "{w}x{h}: 左端の鉱石がステージ左端 2 列に届いていない"
            );
            assert!(
                right.clone().any(column_has_ore),
                "{w}x{h}: 右端の鉱石がステージ右端 2 列に届いていない"
            );
        }
    }

    /// 潰れた領域を渡しても描画が壊れないこと。ステージは braille が
    /// 成立しない狭さでは点描を諦めるが、その判定より手前で panic しない。
    #[test]
    fn render_survives_degenerate_areas() {
        for (w, h) in [(1u16, 1u16), (2, 3), (4, 5), (12, 6), (20, 4), (38, 8)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            for tab in [Tab::Armory, Tab::Ring, Tab::Codex] {
                let mut state = StarRingState::new();
                state.tab = tab;
                terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            }
        }
    }

    /// 内側が数行しかない端末でも、3 タブとも先頭から描画され、購入行を持つタブは
    /// そこへ届く手段 (直接表示 or スクロール) が残ること。
    #[test]
    fn short_viewport_keeps_tab_content_visible() {
        let (w, h) = (38u16, 20u16);
        for (tab, wanted) in [
            (
                Tab::Armory,
                Some(buy_weapon_stat_id(WeaponKind::Pulse, WeaponStat::Count)),
            ),
            (Tab::Ring, Some(buy_ring_id(RingUpgrade::Yield))),
            // 図鑑は購入行を持たないので、描画されていることだけを見る。
            (Tab::Codex, None),
        ] {
            let mut state = StarRingState::new();
            state.tab = tab;

            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();

            let buf = terminal.backend().buffer();
            let tab_area =
                split_body(split_frame(Rect::new(0, 0, w, h))[2], is_narrow_layout(w)).1;
            let filled = (tab_area.y..tab_area.y + tab_area.height)
                .filter(|&y| {
                    (tab_area.x..tab_area.x + tab_area.width)
                        .any(|x| buf[(x, y)].symbol().trim() != "")
                })
                .count();
            assert!(
                filled >= 3,
                "{tab:?}: タブ内側が空欄になっている (中身のある行 {filled})"
            );
            let Some(wanted) = wanted else {
                continue;
            };
            // 先頭に出ていないなら、送り切った先で必ずクリックできること。
            let mut reached = has_action(&cs, w, h, wanted);
            for _ in 0..12 {
                if reached {
                    break;
                }
                assert!(
                    has_action(&cs, w, h, TAB_SCROLL_DOWN),
                    "{tab:?}: 購入行が出ていないのに送る手段が無い"
                );
                state.scroll_tab(3);
                cs.borrow_mut().targets.clear();
                terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
                reached = has_action(&cs, w, h, wanted);
            }
            assert!(reached, "{tab:?}: スクロールしても購入行へ届かない");
        }
    }

    /// 上空の鉱石と、画面下部のコアが縦に分離して描かれること。
    ///
    /// 物体の同定は色で行うので、fixture 側で「その色が他の要素へ割り当たって
    /// いない」ことを先に固定する。採掘境界は `layer_color`、核脈動の波面は
    /// `PULSE_WAVE_COLOR` で描かれるため、層が進んだ state や脈動を積んだ state に
    /// 差し替えると同じ色が別の場所に現れ、この検査は無関係な理由で落ちる。
    #[test]
    fn stage_separates_falling_ores_from_the_core() {
        use crate::games::starringe::state::{Ore, OreMotion};
        use ratzilla::ratatui::style::Color;

        let mut state = StarRingState::new();
        const CORE: Color = Color::Yellow;
        let ore = ore_color(OreKind::Crystal);
        assert_ne!(layer_color(state.layer()), CORE, "採掘境界がコアと同色");
        assert_ne!(layer_color(state.layer()), ore, "採掘境界が鉱石と同色");
        assert_ne!(PULSE_WAVE_COLOR, CORE, "核脈動の波面がコアと同色");
        assert!(
            state.pulse_rings.is_empty(),
            "波面は鉱石と同色 ({PULSE_WAVE_COLOR:?}) なので、波の出ていない state で見る"
        );

        for (x, y) in [(20.0, 92.0), (52.0, 84.0), (78.0, 90.0)] {
            state.ores.push(Ore {
                x,
                y,
                vx: 0.0,
                vy: -0.3,
                hp: 5.0,
                kind: OreKind::Crystal,
                radius: OreKind::Crystal.radius(),
                motion: OreMotion::Spiral,
                sway: 0.05,
                age: 10,
            });
        }

        for (w, h) in [(100u16, 30u16), (40, 30)] {
            let narrow = is_narrow_layout(w);
            let area = Rect::new(0, 0, w, h);
            let (stage, _) = split_body(split_frame(area)[2], narrow);
            let borders = if narrow {
                Borders::TOP | Borders::BOTTOM
            } else {
                Borders::ALL
            };
            let inner = Block::default().borders(borders).inner(stage);

            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();

            let quarter = (inner.height / 4).max(1);
            let count = |y0: u16, y1: u16, want: Color| -> usize {
                let mut n = 0;
                for y in y0..y1 {
                    for x in inner.x..inner.x + inner.width {
                        if buf[(x, y)].fg == want {
                            n += 1;
                        }
                    }
                }
                n
            };
            let top = (inner.y, inner.y + quarter);
            let bottom = (inner.y + inner.height - quarter, inner.y + inner.height);

            assert!(
                count(top.0, top.1, ore) > 0,
                "{w}x{h}: 鉱石はステージ上部に見えるはず"
            );
            assert_eq!(
                count(bottom.0, bottom.1, ore),
                0,
                "{w}x{h}: 上空の鉱石が下部へ描かれてはいけない"
            );
            assert!(
                count(bottom.0, bottom.1, CORE) > 0,
                "{w}x{h}: コアはステージ下部に見えるはず"
            );
            assert_eq!(
                count(top.0, top.1, CORE),
                0,
                "{w}x{h}: コアが上部へ描かれてはいけない"
            );
        }
    }

    /// braille 1 セルが灯している点の `(列, 行)` オフセット。
    ///
    /// `Marker::Braille` は 1 セルへ 2×4 の点を詰めるので、セル単位で数えると
    /// 「見かけの大きさ」が 4 分の 1 の粗さでしか測れない。点の単位まで開くと、
    /// 描画物がどれだけの点を占めているかをそのまま数えられる。
    fn cell_dots(symbol: &str) -> Vec<(usize, usize)> {
        // Unicode の braille は「左列を上から 1・2・3、右列を上から 4・5・6、
        // 最下段を左 7・右 8」の順にビットが並ぶ。行×列の位置へ並べ替える。
        const DOT_BITS: [[u16; 2]; 4] = [
            [0x0001, 0x0008],
            [0x0002, 0x0010],
            [0x0004, 0x0020],
            [0x0040, 0x0080],
        ];
        const BRAILLE_BASE: u32 = 0x2800;
        let Some(c) = symbol.chars().next() else {
            return Vec::new();
        };
        let cp = c as u32;
        if !(BRAILLE_BASE..BRAILLE_BASE + 0x100).contains(&cp) {
            return Vec::new();
        }
        let bits = (cp - BRAILLE_BASE) as u16;
        let mut out = Vec::new();
        for (row, cols) in DOT_BITS.iter().enumerate() {
            for (col, bit) in cols.iter().enumerate() {
                if bits & bit != 0 {
                    out.push((col, row));
                }
            }
        }
        out
    }

    /// バッファ全体を覆う点のオン/オフ表。
    fn braille_dots(buf: &ratzilla::ratatui::buffer::Buffer, w: u16, h: u16) -> Vec<bool> {
        let mut grid = vec![false; (w as usize * 2) * (h as usize * 4)];
        for y in 0..h {
            for x in 0..w {
                for (col, row) in cell_dots(buf[(x, y)].symbol()) {
                    grid[(y as usize * 4 + row) * (w as usize * 2) + x as usize * 2 + col] = true;
                }
            }
        }
        grid
    }

    /// `inner` の中で `color` に塗られたセルが灯している点の座標。
    ///
    /// ステージの描画物は色で塗り分かれているので、色で絞ってから点へ開くと
    /// 物ごとの占有を測れる。重なった部分は後から描いた物の色になるため、
    /// 手前の物が奥の物を隠した結果がそのまま出る。
    fn colored_dots(
        buf: &ratzilla::ratatui::buffer::Buffer,
        inner: Rect,
        color: Color,
    ) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for y in inner.y..inner.y + inner.height {
            for x in inner.x..inner.x + inner.width {
                if buf[(x, y)].fg != color {
                    continue;
                }
                for (col, row) in cell_dots(buf[(x, y)].symbol()) {
                    out.push((x as usize * 2 + col, y as usize * 4 + row));
                }
            }
        }
        out
    }

    /// 点の集合を上下左右に繋がった塊へ分け、塊ごとの点数を返す。
    /// 塊の数がそのまま「いくつの物として見えるか」になる。
    fn blob_sizes(pts: &[(usize, usize)]) -> Vec<usize> {
        use std::collections::HashSet;
        let mut left: HashSet<(usize, usize)> = pts.iter().copied().collect();
        let mut sizes = Vec::new();
        while let Some(&seed) = left.iter().next() {
            left.remove(&seed);
            let mut stack = vec![seed];
            let mut n = 0usize;
            while let Some((x, y)) = stack.pop() {
                n += 1;
                for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let nb = (
                        (x as i64 + dx).max(0) as usize,
                        (y as i64 + dy).max(0) as usize,
                    );
                    if left.remove(&nb) {
                        stack.push(nb);
                    }
                }
            }
            sizes.push(n);
        }
        sizes
    }

    /// 点の集合の外接幅 (点の単位)。
    fn dot_span_w(pts: &[(usize, usize)]) -> usize {
        match (pts.iter().map(|p| p.0).min(), pts.iter().map(|p| p.0).max()) {
            (Some(lo), Some(hi)) => hi - lo + 1,
            _ => 0,
        }
    }

    /// 半径 `radius` の円を `(x, y)` へ 1 つ置いたときに増える点を数え、
    /// `(点数, 外接する幅, 外接する高さ)` を点の単位で返す。
    ///
    /// 置いた前後の差を取るのは、背景星や境界線と重なった点まで数えないため。
    /// 速度を 0 にすると尾 (`approach_trails`) が 1 点へ潰れるので、測るのは
    /// 円そのものの占有だけになる。
    fn ore_footprint(radius: f64, x: f64, y: f64, w: u16, h: u16) -> (usize, usize, usize) {
        let empty = StarRingState::new();
        let mut placed = empty.clone();
        placed.ores.push(Ore {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            hp: 1.0,
            kind: OreKind::Dust,
            radius,
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
        });
        let dots_of = |st: &StarRingState| {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(st, f, f.area(), &cs)).unwrap();
            braille_dots(terminal.backend().buffer(), w, h)
        };
        let before = dots_of(&empty);
        let after = dots_of(&placed);
        let grid_w = w as usize * 2;
        let (mut count, mut x0, mut y0) = (0usize, usize::MAX, usize::MAX);
        let (mut x1, mut y1) = (0usize, 0usize);
        for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
            if *a && !*b {
                count += 1;
                x0 = x0.min(i % grid_w);
                x1 = x1.max(i % grid_w);
                y0 = y0.min(i / grid_w);
                y1 = y1.max(i / grid_w);
            }
        }
        if count == 0 {
            return (0, 0, 0);
        }
        (count, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// 円の中心をずらしながら繰り返し測り、まとめた `FootprintStats` を返す。
    ///
    /// 円が点グリッドのどこに落ちるかは中心の端数で変わる。位相をなめて初めて
    /// 「降下するあいだずっとこの大きさに見える」と言える。
    fn footprint_stats(radius: f64, w: u16, h: u16) -> FootprintStats {
        let mut total = 0usize;
        let mut samples = 0usize;
        let (mut min_w, mut min_h, mut max_w, mut max_h) = (usize::MAX, usize::MAX, 0, 0);
        for (ox, oy) in SAMPLE_ORIGINS {
            for i in 0..6 {
                for j in 0..6 {
                    let (n, bw, bh) = ore_footprint(
                        radius,
                        ox + i as f64 * PHASE_STEP,
                        oy + j as f64 * PHASE_STEP,
                        w,
                        h,
                    );
                    total += n;
                    samples += 1;
                    min_w = min_w.min(bw);
                    min_h = min_h.min(bh);
                    max_w = max_w.max(bw);
                    max_h = max_h.max(bh);
                }
            }
        }
        FootprintStats {
            // 点数の平均は 1/100 点まで刻んで持つ。整数へ丸めると、隣り合う
            // 種の差がまるごと丸め誤差に飲まれてしまう。
            mean_dots_centi: total * 100 / samples,
            min_w,
            min_h,
            max_w,
            max_h,
        }
    }

    /// 円を点の単位で測った結果。`mean_dots_centi` は占有点数の平均を 100 倍
    /// した整数、残りは外接矩形の振れ幅。
    struct FootprintStats {
        mean_dots_centi: usize,
        min_w: usize,
        min_h: usize,
        max_w: usize,
        max_h: usize,
    }

    /// 位相をなめる刻み。ステージの点間隔 (モバイルで約 1.5、デスクトップで
    /// 約 0.7 ワールド単位) のどちらとも割り切れない幅を選び、少ない標本でも
    /// 位相が同じところへ偏らないようにする。
    const PHASE_STEP: f64 = 0.31;
    /// 測る場所。円が点へ落ちる位相は中心の端数だけでなく、描画領域のどこに
    /// いるかでも変わる。離れた 2 点で測って、片方に都合の良い位置で判定が
    /// 通ってしまうのを避ける。
    const SAMPLE_ORIGINS: [(f64, f64); 2] = [(30.0, 68.0), (52.5, 41.5)];

    /// 電話幅のステージ。ワールド 100 幅が braille 66 点しかない最小構成で、
    /// 点グリッドの粗さが一番効く。
    const PHONE_STAGE: (u16, u16) = (33, 38);
    /// デスクトップ幅のステージ。
    const DESKTOP_STAGE: (u16, u16) = (108, 36);

    /// 端末サイズ `w`x`h` でステージの枠の内側に当たる Rect。
    /// レイアウトの分岐は `render` と同じ判定から引く。
    fn stage_inner(w: u16, h: u16) -> Rect {
        let narrow = is_narrow_layout(w);
        let (stage, _) = split_body(split_frame(Rect::new(0, 0, w, h))[2], narrow);
        let borders = if narrow {
            Borders::TOP | Borders::BOTTOM
        } else {
            Borders::ALL
        };
        Block::default().borders(borders).inner(stage)
    }

    /// ステージが「核 > 砲台 > 環」の順に読めること。
    ///
    /// 守る拠点も自分の武装も、降ってくる鉱石や軌道の装飾と同じ粒度で並ぶと、
    /// 画面のどこを見ればよいかが決まらない。核は最大の鉱石より太く、砲台は
    /// 環の点より太い塊で、しかも基数を数えられるだけ離れていること。
    #[test]
    fn the_stage_reads_as_core_over_turret_over_orbit() {
        const CORE: Color = Color::Yellow;
        const ORBIT: Color = Color::Indexed(238);
        const TURRET: Color = Color::White;

        let big = *kinds_by_radius().last().unwrap();
        let ore_hue = ore_color(big);
        assert!(
            ![CORE, ORBIT, TURRET].contains(&ore_hue),
            "鉱石の色が核・環・砲台のどれかと同じでは、色で物を分けられない"
        );

        let mut state = StarRingState::new();
        state.weapon_levels[0][0] = 3;
        state.ores.push(Ore {
            x: 30.0,
            y: 78.0,
            // 速度 0 なら尾が 1 点へ潰れ、測るのは鉱石そのものの占有だけになる。
            vx: 0.0,
            vy: 0.0,
            hp: 5.0,
            kind: big,
            radius: big.radius(),
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
        });
        let near_guns = turret_positions(&state)
            .iter()
            .filter(|(_, _, depth)| *depth <= 0.0)
            .count();
        assert!(
            near_guns >= 2,
            "手前側の砲台が {near_guns} 基では基数を数える検査にならない"
        );

        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();

            let core = dot_span_w(&colored_dots(buf, inner, CORE));
            let ore = dot_span_w(&colored_dots(buf, inner, ore_hue));
            assert!(
                core >= ore + 2,
                "{w}x{h}: 核 {core}点幅 と最大の鉱石 {ore}点幅 が同じ大きさに見える"
            );

            let guns = blob_sizes(&colored_dots(buf, inner, TURRET));
            assert_eq!(
                guns.len(),
                near_guns,
                "{w}x{h}: 手前側の砲台 {near_guns} 基が {} 個の塊にしか見えない",
                guns.len()
            );
            let thinnest_gun = guns.iter().copied().min().unwrap_or(0);
            let thickest_orbit = blob_sizes(&colored_dots(buf, inner, ORBIT))
                .iter()
                .copied()
                .max()
                .unwrap_or(0);
            assert!(
                thinnest_gun >= thickest_orbit * 3,
                "{w}x{h}: 砲台 {thinnest_gun}点 が環の点 {thickest_orbit}点 に埋もれる"
            );
        }
    }

    /// 砲台の下端が、環がどれだけ下がっても `VISIBLE_Y_LO` を割らないこと。
    /// 欠けると、武装がフレームに削られた形で描かれる。
    ///
    /// 核側の同じ不変条件は定数の隣の `const` アサートが持つ。
    #[test]
    fn turrets_never_reach_below_the_field() {
        let mut state = StarRingState::new();
        for count in 0..=7u32 {
            state.weapon_levels[0][0] = count;
            let (_, ring_ry) = state.ring_radii();
            let bottom = CORE_Y - ring_ry - turret_radius(ring_ry);
            // 余裕を使い切る砲台数では下端ちょうどに接するので、丸め誤差ぶんを許す。
            assert!(
                bottom >= VISIBLE_Y_LO - 1e-9,
                "砲台 {} 基で環の最下点の砲台が下端を割る (y={bottom})",
                state.turret_count()
            );
        }
    }

    /// 核は最大の鉱石より大きく、層をいくら重ねても脈動の上限を超えないこと。
    #[test]
    fn the_core_outgrows_every_ore_at_any_layer() {
        let widest = OreKind::ALL
            .iter()
            .map(|k| k.radius())
            .fold(0.0f64, f64::max);
        assert!(
            CORE_RADIUS >= widest * 1.25,
            "核 {CORE_RADIUS} が最大の鉱石 {widest} と同格に見える"
        );

        let mut state = StarRingState::new();
        for layer in [1u32, 8, 40, 400] {
            state.current_layer = layer;
            assert!(
                core_scale(&state) <= CORE_MAX_SCALE,
                "第{layer}層で核の脈動が上限を超える"
            );
        }
    }

    /// 半径の小さい順に並べた鉱石。大きさの比較はこの順で見る。
    fn kinds_by_radius() -> Vec<OreKind> {
        let mut kinds = OreKind::ALL.to_vec();
        kinds.sort_by(|a, b| a.radius().partial_cmp(&b.radius()).unwrap());
        kinds
    }

    /// 最小の鉱石は、いちばん大きい弾より確実に大きく描かれる。
    ///
    /// 星塵と弾は色と尾の向きでも違うが、降ってくる的と自分の撃った弾を
    /// 見分ける最初の手がかりは大きさになる。端数をどうずらしても外接矩形が
    /// 2 点ぶん離れていれば、弾と的が同じ塊に見えることはない。
    #[test]
    fn the_smallest_ore_outgrows_a_shot_on_a_phone_sized_stage() {
        let (w, h) = PHONE_STAGE;
        let ore = footprint_stats(OreKind::Dust.radius(), w, h);
        let shot = footprint_stats(NOVA_PROJECTILE_RADIUS, w, h);
        assert!(
            ore.min_w >= shot.max_w + 2 && ore.min_h >= shot.max_h + 2,
            "星塵 {}x{}点 と最大の弾 (新星弾) {}x{}点 が紛らわしい",
            ore.min_w,
            ore.min_h,
            shot.max_w,
            shot.max_h
        );
    }

    /// 最小の鉱石は、端数がどこに落ちても 3×3 点を割らない。
    ///
    /// 直径が 3 点を下回ると、円が点へ落ちる位相しだいで見かけの大きさが
    /// 1 点ぶん揺れる。降下するあいだ端数は毎 tick 変わるので、その揺れは
    /// 脈打つ明滅として出てしまう。
    #[test]
    fn the_smallest_ore_keeps_a_steady_size_while_it_falls() {
        for (w, h) in [PHONE_STAGE, DESKTOP_STAGE] {
            let s = footprint_stats(OreKind::Dust.radius(), w, h);
            assert!(
                s.min_w >= 3 && s.min_h >= 3,
                "{w}x{h}: 星塵が {}x{}点まで痩せる",
                s.min_w,
                s.min_h
            );
            assert!(
                s.max_w - s.min_w <= 1 && s.max_h - s.min_h <= 1,
                "{w}x{h}: 星塵の大きさが {}x{}〜{}x{}点 で暴れる",
                s.min_w,
                s.min_h,
                s.max_w,
                s.max_h
            );
        }
    }

    /// 隣り合う大きさの鉱石どうしが、点グリッドの上で同じ塊に潰れない。
    ///
    /// 半径の差が点の間隔を下回ると、ラスタライズ後の占有点数がほぼ同じに
    /// なり、種の違いが色だけになる。8 種を `OreKind::Dust` から
    /// `OreKind::Nova` までの幅へ詰め込む以上ここが一番狭くなるので、順序と
    /// 最小の差を数値で押さえる。閾値は実測の最小差より一段低く取ってあり、
    /// 半径をわずかに動かしただけでは鳴らない。
    #[test]
    fn neighbouring_ore_kinds_stay_distinguishable_by_size() {
        const MIN_GROWTH_PERCENT: usize = 108;
        for (w, h) in [PHONE_STAGE, DESKTOP_STAGE] {
            let mut prev: Option<(OreKind, usize)> = None;
            for kind in kinds_by_radius() {
                let dots = footprint_stats(kind.radius(), w, h).mean_dots_centi;
                if let Some((prev_kind, prev_dots)) = prev {
                    assert!(
                        dots * 100 >= prev_dots * MIN_GROWTH_PERCENT,
                        "{w}x{h}: {prev_kind:?} 平均{:.1}点 と {kind:?} 平均{:.1}点 が同じ大きさに見える",
                        prev_dots as f64 / 100.0,
                        dots as f64 / 100.0
                    );
                }
                prev = Some((kind, dots));
            }
        }
    }
}
