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
    CX, FIELD_MARGIN, SHAKE_MAX_Y, SPAWN_Y, TURRET_NEAR_RADIUS, WORLD_H, WORLD_W,
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

/// 購入行の説明文の字下げ。見出しのキー表記 (` [A] `) の下へ揃える。
const BLURB_INDENT: usize = 6;

/// `spaced` を付けた時のかたまり間の空行を含む総行数。
fn sections_height(sections: &[Section], spaced: bool) -> usize {
    let base: usize = sections.iter().map(|s| s.lines.len()).sum();
    if spaced {
        base + sections.len().saturating_sub(1)
    } else {
        base
    }
}

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

/// 描画領域に合わせてかたまりを組み、空行を挟むかどうかを決める。
///
/// `make` は「この幅で説明文を折り返した行」を返す。`ScrollableTab` は内容が
/// 溢れる時だけ右端 1 桁をスクロール列に使うので、溢れる場合は 1 桁狭い幅で
/// 折り直す — 折り返し済みの行はスクロール列の下で切り詰められてしまうため。
/// 幅を狭めれば行数は増えこそすれ減らないので、溢れる判定はそのまま成り立つ。
fn fit_sections<F>(inner: Rect, make: F) -> (Vec<Section>, bool)
where
    F: Fn(u16) -> Vec<Section>,
{
    let sections = make(inner.width);
    let height = inner.height as usize;
    if sections_height(&sections, true) <= height {
        return (sections, true);
    }
    if sections_height(&sections, false) <= height {
        return (sections, false);
    }
    (make(inner.width.saturating_sub(1)), false)
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
    let (sections, spaced) = fit_sections(body, |w| armory_sections(state, w));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(sections, spaced),
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
    let (sections, spaced) = fit_sections(block.inner(area), |w| ring_sections(state, w));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(sections, spaced),
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
    let status = Line::from(Span::styled(
        format!(" 第{}層 {}  {}", layer, Layer::title(layer), bar),
        Style::default()
            .fg(layer_color(layer))
            .add_modifier(Modifier::BOLD),
    ));
    // 撃破進捗と湧き倍率を1行にまとめ、狭い画面で強化行が押し出されないようにする。
    let mults = Line::from(Span::styled(
        match next {
            Some(th) => format!(
                " 撃破{}/{}  湧き×{} HP×{:.1} ✦×{:.1}",
                state.total_kills,
                th,
                Layer::spawn_batch(layer),
                Layer::hp_mult(layer),
                Layer::value_mult(layer)
            ),
            None => format!(
                " 撃破{}  湧き×{} HP×{:.1} ✦×{:.1}",
                state.total_kills,
                Layer::spawn_batch(layer),
                Layer::hp_mult(layer),
                Layer::value_mult(layer)
            ),
        },
        Style::default().fg(Color::DarkGray),
    ));
    sections.push(Section::plain(vec![status, mults]));

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
                    " [!] 第{}層「{}」を開放 ✦{}",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            } else {
                format!(
                    " [!] 第{}層「{}」 要✦{} (不足)",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            };
            let line = vec![Line::from(Span::styled(label, style))];
            sections.push(if can {
                Section::clickable(line, OPEN_LAYER)
            } else {
                Section::plain(line)
            });
        } else {
            sections.push(Section::plain(vec![Line::from(Span::styled(
                format!(
                    " 次層「{}」 撃破{} ✦{}",
                    Layer::title(next_layer),
                    th,
                    format_shards(cost)
                ),
                Style::default().fg(Color::DarkGray),
            ))]));
        }
    }

    sections.push(Section::plain(vec![Line::from(Span::styled(
        " 環の強化",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))]));

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
    let (sections, spaced) = fit_sections(block.inner(area), |_| codex_sections(state));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        build_list(sections, spaced),
        &state.tab_scroll,
        TAB_SCROLL_UP,
        TAB_SCROLL_DOWN,
    )
    .block(block)
    .arrow_color(Color::Yellow)
    .render(f, area, &mut cs);
}

/// 図鑑の行。進捗 / 鉱石 / 累計 / 武装の 4 かたまりに分ける。
fn codex_sections(state: &StarRingState) -> Vec<Section> {
    let unlocked = state.unlocked_ore_kinds();
    let layer = state.layer();

    let progress = Section::plain(vec![Line::from(Span::styled(
        format!(
            " 第{}層  累計撃破 {}  逸失 {}",
            layer, state.total_kills, state.missed_count
        ),
        Style::default().fg(Color::DarkGray),
    ))]);

    let mut ores = Vec::new();
    for kind in OreKind::ALL {
        let open = unlocked.contains(&kind);
        if open {
            ores.push(Line::from(vec![
                Span::styled(" ◆ ", Style::default().fg(ore_color(kind))),
                Span::styled(
                    format!("{} ", kind.label()),
                    Style::default()
                        .fg(ore_color(kind))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "価値{} HP{:.0}",
                        kind.base_value(),
                        kind.base_hp() * Layer::hp_mult(layer)
                    ),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        } else {
            ores.push(Line::from(Span::styled(
                format!(" ？ 第{}層で出現", kind.unlock_layer()),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let earned = Section::plain(vec![Line::from(Span::styled(
        format!(" 獲得累計 ✦{}", format_shards(state.shards_earned)),
        Style::default().fg(Color::DarkGray),
    ))]);

    let mut weapons = vec![Line::from(Span::styled(
        " 武装解放",
        Style::default().fg(Color::Yellow),
    ))];
    for w in WeaponKind::ALL {
        let open = state.is_weapon_unlocked(w);
        if open {
            weapons.push(Line::from(Span::styled(
                format!("  {} {} 解放済", w.glyph(), w.label()),
                Style::default().fg(weapon_color(w)),
            )));
        } else {
            weapons.push(Line::from(Span::styled(
                format!("  ？ {}  第{}層", w.label(), w.unlock_layer()),
                Style::default().fg(Color::DarkGray),
            )));
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

    let shake_x = if state.shake_ticks > 0 {
        (((state.elapsed_ticks % 4) as f64) - 1.5) * 0.4
    } else {
        0.0
    };
    let shake_y = if state.shake_ticks > 0 {
        ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * SHAKE_MAX_Y
    } else {
        0.0
    };

    // 砲台環。コアを中心とする横長の楕円で、砲台の通り道をなぞる。
    let (ring_rx, ring_ry) = state.ring_radii();
    let mut orbit_pts = Vec::new();
    for i in 0..64 {
        let a = i as f64 * std::f64::consts::TAU / 64.0;
        orbit_pts.push((
            CX + a.cos() * ring_rx + shake_x,
            CORE_Y + a.sin() * ring_ry + shake_y,
        ));
    }

    let core_scale = if state.layer_flash_ticks > 0 {
        1.55
    } else if state.layer_ready_flash_ticks > 0 {
        1.30
    } else if state.core_flash_ticks > 0 {
        1.25
    } else if can_unlock_next_layer(state) && state.elapsed_ticks % 20 < 10 {
        1.12
    } else {
        1.0 + (layer.saturating_sub(1) as f64) * 0.04
    };
    let core_pts = filled_circle(
        CX + shake_x,
        CORE_Y + shake_y,
        3.0 * core_scale,
        sample_step,
    );
    let core_ring = canvas_fx::ring_points(CX + shake_x, CORE_Y + shake_y, 4.8 * core_scale, 0.26);

    // 採掘境界。鉱石が湧いてくる高さに水平の点線を引き、そこから上が
    // 今の層の外側だと示す。層が上がるほど点が詰まって濃くなる。
    let mut boundary_pts = Vec::new();
    let boundary_step = (3.4 - (layer.min(7).saturating_sub(1) as f64) * 0.4).max(1.2);
    let mut bx = FIELD_MARGIN;
    while bx <= WORLD_W - FIELD_MARGIN {
        boundary_pts.push((bx + shake_x, SPAWN_Y + shake_y));
        bx += boundary_step;
    }

    // フィールドの左右境界。鉱石はここで跳ね返る。x 表示範囲がワールド幅
    // なので壁はほぼ画面端に来る — 連続した点線だと縁が騒がしくなるだけ
    // なので、端があると分かる程度まで間引いたアクセントに留める。
    let mut wall_pts = Vec::new();
    let mut wy = 0.0;
    while wy <= WORLD_H {
        wall_pts.push((FIELD_MARGIN + shake_x, wy + shake_y));
        wall_pts.push((WORLD_W - FIELD_MARGIN + shake_x, wy + shake_y));
        wy += 12.0;
    }

    // 砲台。環の下半分 (sin < 0) が視点に近い手前側。
    let turrets = turret_positions(state);
    let mut gun_near = Vec::new();
    let mut gun_far = Vec::new();
    for &(gx, gy, depth) in &turrets {
        let near = depth <= 0.0;
        let size = if near { TURRET_NEAR_RADIUS } else { 1.0 };
        let pts = filled_circle(gx + shake_x, gy + shake_y, size, sample_step);
        if near {
            gun_near.extend(pts);
        } else {
            gun_far.extend(pts);
        }
    }

    let mut ore_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut approach_trails: Vec<(f64, f64, f64, f64, Color)> = Vec::new();
    for ore in &state.ores {
        let color = ore_color(ore.kind);
        let pts = filled_circle(ore.x + shake_x, ore.y + shake_y, ore.radius, sample_step);
        if let Some(g) = ore_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            ore_groups.push((pts, color));
        }
        let speed = ore.vx.hypot(ore.vy).max(0.01);
        let trail = ore.radius * 2.8 + speed * 3.0;
        approach_trails.push((
            ore.x + shake_x,
            ore.y + shake_y,
            ore.x - ore.vx / speed * trail + shake_x,
            ore.y - ore.vy / speed * trail + shake_y,
            color,
        ));
    }

    // 飛翔弾を武器色で描画
    let mut proj_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut proj_trails: Vec<(f64, f64, f64, f64, Color)> = Vec::new();
    for p in &state.projectiles {
        let color = weapon_color(p.kind);
        let pts = filled_circle(p.x + shake_x, p.y + shake_y, p.radius, sample_step);
        if let Some(g) = proj_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            proj_groups.push((pts, color));
        }
        let speed = p.vx.hypot(p.vy).max(0.01);
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
        proj_trails.push((
            p.x + shake_x,
            p.y + shake_y,
            p.x - p.vx / speed * len + shake_x,
            p.y - p.vy / speed * len + shake_y,
            color,
        ));
    }

    let mut sparks = Vec::new();
    let mut dust = Vec::new();
    let mut shards = Vec::new();
    let mut embers = Vec::new();
    for p in &state.particles {
        let pt = (p.x + shake_x, p.y + shake_y);
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
        push_pulse_wave_points(
            CX + shake_x,
            CORE_Y + shake_y,
            ring.radius,
            density,
            &mut pulse_ring_pts,
        );
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
        stars.push((x, y));
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
                    color: Color::Indexed(240),
                });
            }
            if !gun_far.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_far,
                    color: Color::Gray,
                });
            }
            for &(x1, y1, x2, y2, color) in &approach_trails {
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
            for &(x1, y1, x2, y2, color) in &proj_trails {
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
            if !gun_near.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_near,
                    color: Color::White,
                });
            }
            if !core_ring.is_empty() {
                ctx.draw(&Points {
                    coords: &core_ring,
                    color: Color::DarkGray,
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
    use crate::games::starringe::logic::unlock_next_layer;
    use crate::games::starringe::state::{Layer, RingUpgrade, Tab};

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
    /// 差し引いた幅で折り返すこと。ここがずれると折り返した行の右端が
    /// スクロール列に隠れる。
    #[test]
    fn fit_sections_leaves_room_for_the_scroll_column() {
        let state = StarRingState::new();
        let inner = Rect::new(0, 0, 33, 6);
        let (fitted, spaced) = fit_sections(inner, |w| armory_sections(&state, w));
        assert!(!spaced, "溢れている時に装飾の空行は挟まない");
        assert_eq!(
            sections_height(&fitted, false),
            sections_height(&armory_sections(&state, inner.width - 1), false),
            "スクロール列の 1 桁を引いた幅で折り返していない"
        );
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

            let tab_area = split_body(split_frame(Rect::new(0, 0, w, h))[2], is_narrow_layout(w)).1;
            let bar_row = rows
                .iter()
                .find(|r| r.contains('弾') && r.contains('威'))
                .expect("強化バーの行が見つからない");
            let cells = bar_cells(tab_area.width);
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

    /// `sections_height` の見積もりと `build_list` の実際の行数が一致すること。
    ///
    /// この 2 つは空行の入れ方を別々に持っている。ずれると `spaced` の判定と
    /// `ScrollableTab` のスクロール上限が同時に狂い、詰めれば入る内容にスクロールを
    /// 生やしたり、逆に末尾へ届かなくなったりする。
    #[test]
    fn section_height_matches_the_list_it_builds() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));

        for (label, make) in [
            (
                "armory",
                armory_sections as fn(&StarRingState, u16) -> Vec<Section>,
            ),
            ("ring", ring_sections),
            ("codex", |s: &StarRingState, _w: u16| codex_sections(s)),
        ] {
            // 折り返しは幅で行数を変えるので、実機で使う幅の両端を含めて見る。
            for width in [20u16, 33, 34, 41, 72] {
                for spaced in [false, true] {
                    let sections = make(&state, width);
                    let expected = sections_height(&sections, spaced);
                    let actual = build_list(sections, spaced).lines().len();
                    assert_eq!(
                        expected, actual,
                        "{label} ({width}桁 spaced={spaced}): 見積もり {expected} 行 / 実際 {actual} 行"
                    );
                }
            }
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
}
