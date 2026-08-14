//! 玉響 — 描画 (読み取り専用)。
//!
//! 盤面は `ratatui::widgets::canvas::Canvas` + `Marker::Braille` の疑似
//! ピクセルで、logic.rs が持つ連続座標のまま描く。盤面全面は `Clickable` で
//! 1つのタップ対象にし、別 DOM 要素を生やさずに「盤面を触ると打ち出しが
//! 切り替わる」操作を成立させる。
//!
//! ## 何を見せて、何を見せないか
//! 台の回りやすさ (`Machine::nail_spread` / `rail_bias`) は釘の描画位置
//! としてだけ現れ、数値では出さない。スペックの当たり分母も「甘め / 中間 /
//! 荒い」という語に丸める。プレイヤーが盤面を観察して自分で線を引く余地を
//! 残すのがこのゲームの主眼で、答えを数値で配ると判断そのものが消える。
//!
//! 一方、**計測できた事実** — 実測回転率 (`logic::spin_rate`)、大当たり
//! 履歴、収支、投資額 — は隠さずに出す。観察の結果を突き合わせる対象が
//! 無ければ、そもそも線を引けない。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Context, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::games::GameChoice;
use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{Clickable, ClickableList, ScrollableTab, TabBar};

use super::actions;
use super::logic;
use super::state::{
    Digit, InfoTab, JackpotState, Machine, MachineSpec, Mode, PachinkoState, PendingRank, Phase,
    SpinOutcome, StopStyle, ATTACKER_HALF_W, ATTACKER_X, ATTACKER_Y, BALL_LOAN_COUNT,
    BALL_LOAN_YEN, BALL_R, BOARD_H, BOARD_W, LAUNCH_X, LAUNCH_Y, MAX_PENDING, NAIL_R,
    PENDING_PROMOTE_FLASH_TICKS, ROUND_COUNT, SIDE_POCKET_HALF_W, SIDE_POCKET_LEFT_X,
    SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y, START_POCKET_X, START_POCKET_Y,
};

/// 玉響のアクセント色 (銀玉の色)。盤面の枠・選択中タブ・見出しで共有する。
/// メニュー一覧での識別色と同じものを引くことで、ゲーム内外で色がずれない。
const ACCENT: Color = theme::accent(&GameChoice::Pachinko);

pub fn render(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.phase {
        Phase::Hall => render_hall(state, f, area, click_state),
        Phase::Playing => render_playing(state, f, area, click_state),
    }
}

/// 盤面のワールド y (下向きが正、0=天井) を Canvas の y (上向きが正) へ
/// 反転する。x は左右そのままなので変換しない。
fn board_to_canvas_y(world_y: f64) -> f64 {
    BOARD_H - world_y
}

// ── 数値の整形 ─────────────────────────────────────────────────

/// 3桁区切り。玉数も金額も4桁を超えるのが常なので、桁を目で数えずに
/// 大小を比べられるようにする。
fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn format_signed_yen(value: i64) -> String {
    let sign = if value < 0 { '-' } else { '+' };
    format!("{sign}{}円", format_thousands(value.unsigned_abs()))
}

/// 持ち玉を換金したときの金額。収支表示は「今この瞬間に流したら手元に
/// いくら残るか」で見せる。
fn balls_to_yen(balls: u32) -> u64 {
    balls as u64 * BALL_LOAN_YEN as u64 / BALL_LOAN_COUNT as u64
}

/// この来店の収支 (持ち玉の価値 - 投資額)。
fn visit_balance(state: &PachinkoState) -> i64 {
    balls_to_yen(state.balls_held) as i64 - state.invested as i64
}

fn balance_color(value: i64) -> Color {
    if value > 0 {
        Color::LightGreen
    } else if value < 0 {
        Color::LightRed
    } else {
        Color::Gray
    }
}

// ── 台の性格 (数値を出さずに傾向だけ伝える) ────────────────────

/// 当たりの軽さ。分母そのものを出すと「どれが得か」が計算問題になり、
/// 打って確かめる意味が消えるので3段階の語に丸める。
///
/// 語の切れ目は `MACHINE_SPECS` の `normal_odds` の隣り合う値の間に置く。
/// 2機種が同じ側へ入ると当たりやすさの差が表示から消え、ホールでどの台を
/// 選ぶかという判断がラウンド数だけの比較になる。
/// `every_machine_spec_reads_as_a_distinct_odds_word` が切れ目のズレを検知する。
fn odds_flavor(spec: &MachineSpec) -> &'static str {
    match spec.normal_odds {
        0..=59 => "甘め",
        60..=94 => "中間",
        _ => "荒い",
    }
}

/// 一撃の出玉の傾向。ラウンド数も同じ理由で語に丸める。最大ラウンドでは
/// なく重み付き平均で見るのは、大きい方の目だけを拾うと「稀に伸びるが
/// 普段は軽い台」と「常に伸びる台」が同じ語になってしまうため。
fn payout_flavor(spec: &MachineSpec) -> &'static str {
    let total_weight: u32 = spec.round_table.iter().map(|&(_, w)| w).sum();
    if total_weight == 0 {
        return "出玉少";
    }
    let weighted: u32 = spec.round_table.iter().map(|&(r, w)| r * w).sum();
    match weighted / total_weight {
        0..=8 => "出玉少",
        9..=13 => "出玉中",
        _ => "出玉多",
    }
}

/// 実測回転率。標本が足りない間は数字を作らず「計測中…」と出す —
/// 少ない打込数から出した比率を回転率として見せると、観察して確かめる
/// という判断軸そのものを誤らせる。
fn spin_rate_text(machine: Option<&Machine>) -> String {
    match machine {
        Some(m) => match logic::spin_rate(m) {
            Some(rate) => format!("回転率 {rate:.1}回/千円  (打込 {}玉)", m.balls_spent),
            None => format!("回転率 計測中…  (打込 {}玉)", m.balls_spent),
        },
        None => "回転率 —".to_string(),
    }
}

// ── 盤面の静止画 (遊技画面とホールのプレビューで共有) ──────────

/// アウト口へ向かう漏斗の上端。アタッカーから下は入賞判定を持たない
/// 落下区間なので、線だけで「最後にどこへ落ちるか」を見せる。ここに
/// 置くのは描画のためだけの座標で、玉の当たり判定は増やさない。
const FUNNEL_TOP_Y: f64 = ATTACKER_Y + 4.0;
/// アウト口の口元の高さと半幅。
const OUT_MOUTH_Y: f64 = BOARD_H - 3.0;
const OUT_HALF_W: f64 = 5.0;
/// 漏斗の中に落とす山形の目印の数。玉が流れていない間も下向きの流れが
/// 読めるようにするためのもので、段数を増やすほど線が密になる。
const FUNNEL_CHEVRONS: usize = 3;

/// 盤面のうち、玉と大当たり演出を除いた静止部分。
///
/// 遊技画面とホールのプレビューがこの1組を共有することで、プレビューで
/// 読んだヘソの開きが着席後の盤面とそのまま一致する。別々に組むと、
/// 片方だけ直したときに釘読みが嘘になる。
struct BoardStatics {
    guide_lines: Vec<(f64, f64, f64, f64)>,
    nails: Vec<(f64, f64)>,
    side_pockets: Vec<(f64, f64)>,
    start_pocket: Vec<(f64, f64)>,
    attacker: Vec<(f64, f64)>,
    out_mouth: Vec<(f64, f64)>,
}

/// 盤面の静止部分を Canvas 座標の点群として組む。`paint` は move クロージャ
/// なので、描くものは全て所有権付きの Vec で先に組む。
///
/// `aspect` は釘を真円に見せるための y 半径の倍率 (`round_aspect`)、
/// `pocket_half_w` はヘソの受け口半幅 (`logic::pocket_half_w`)。
fn board_statics(
    machine: Option<&Machine>,
    pocket_half_w: f64,
    aspect: f64,
    attacker_open: bool,
) -> BoardStatics {
    let center = BOARD_W / 2.0;

    // 打ち出し口から天井へ回り込むレールと、アタッカーの下からアウト口へ
    // 絞り込む漏斗。玉が飛んでいない時にも玉道の入口と出口が読める。
    let mut guide_lines: Vec<(f64, f64, f64, f64)> = vec![
        (
            LAUNCH_X,
            board_to_canvas_y(LAUNCH_Y),
            LAUNCH_X,
            board_to_canvas_y(1.5),
        ),
        (
            LAUNCH_X,
            board_to_canvas_y(1.5),
            3.0,
            board_to_canvas_y(1.5),
        ),
        (
            2.0,
            board_to_canvas_y(FUNNEL_TOP_Y),
            center - OUT_HALF_W,
            board_to_canvas_y(OUT_MOUTH_Y),
        ),
        (
            BOARD_W - 2.0,
            board_to_canvas_y(FUNNEL_TOP_Y),
            center + OUT_HALF_W,
            board_to_canvas_y(OUT_MOUTH_Y),
        ),
    ];
    for i in 0..FUNNEL_CHEVRONS {
        let t = (i + 1) as f64 / (FUNNEL_CHEVRONS + 1) as f64;
        let y = FUNNEL_TOP_Y + (OUT_MOUTH_Y - FUNNEL_TOP_Y) * t;
        // 漏斗が狭まるのに合わせて山形も縮める。壁の線と交差させないための幅。
        let half = 4.5 - t * 2.0;
        guide_lines.push((
            center - half,
            board_to_canvas_y(y - 1.4),
            center,
            board_to_canvas_y(y + 1.4),
        ));
        guide_lines.push((
            center + half,
            board_to_canvas_y(y - 1.4),
            center,
            board_to_canvas_y(y + 1.4),
        ));
    }

    let nails: Vec<(f64, f64)> = machine
        .map(|m| {
            m.nails
                .iter()
                .flat_map(|n| {
                    canvas_fx::filled_ellipse_points(
                        n.x,
                        board_to_canvas_y(n.y),
                        NAIL_R * 0.6,
                        NAIL_R * 0.6 * aspect,
                        0.2,
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let side_pockets: Vec<(f64, f64)> = [SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X]
        .into_iter()
        .flat_map(|cx| {
            canvas_fx::filled_rect_points(
                cx - SIDE_POCKET_HALF_W,
                board_to_canvas_y(SIDE_POCKET_Y - 0.8),
                cx + SIDE_POCKET_HALF_W,
                board_to_canvas_y(SIDE_POCKET_Y + 0.8),
                0.4,
            )
        })
        .collect();

    // ヘソの受け口は当たり判定と同じ幅で描く。見た目と判定が食い違うと、
    // 釘を読んで台を選ぶという判断そのものが嘘になる。
    let start_pocket = canvas_fx::filled_rect_points(
        START_POCKET_X - pocket_half_w,
        board_to_canvas_y(START_POCKET_Y - 0.9),
        START_POCKET_X + pocket_half_w,
        board_to_canvas_y(START_POCKET_Y + 0.9),
        0.35,
    );

    // アタッカーは開放中だけ厚みを持たせ、閉じている間は細い線にする。
    let attacker_half_h = if attacker_open { 1.4 } else { 0.2 };
    let attacker = canvas_fx::filled_rect_points(
        ATTACKER_X - ATTACKER_HALF_W,
        board_to_canvas_y(ATTACKER_Y - attacker_half_h),
        ATTACKER_X + ATTACKER_HALF_W,
        board_to_canvas_y(ATTACKER_Y + attacker_half_h),
        0.4,
    );

    // アウト口は玉を飲み込む格子として粗い点で描く。塗り潰すと盤面の底が
    // 一枚の板に見え、玉の行き先ではなくなる。
    let out_mouth = canvas_fx::filled_rect_points(
        START_POCKET_X - OUT_HALF_W,
        board_to_canvas_y(OUT_MOUTH_Y),
        START_POCKET_X + OUT_HALF_W,
        board_to_canvas_y(BOARD_H - 0.5),
        0.8,
    );

    BoardStatics {
        guide_lines,
        nails,
        side_pockets,
        start_pocket,
        attacker,
        out_mouth,
    }
}

fn draw_points(ctx: &mut Context, coords: &[(f64, f64)], color: Color) {
    if !coords.is_empty() {
        ctx.draw(&Points { coords, color });
    }
}

/// 静止部分を描く。色だけは遊技中の状態 (ヘソの点灯・アタッカーの開放) で
/// 変わるので、呼び出し側から渡す。
fn draw_board_statics(
    ctx: &mut Context,
    statics: &BoardStatics,
    start_pocket_color: Color,
    attacker_color: Color,
) {
    for &(x1, y1, x2, y2) in &statics.guide_lines {
        ctx.draw(&CanvasLine {
            x1,
            y1,
            x2,
            y2,
            color: Color::DarkGray,
        });
    }
    draw_points(ctx, &statics.out_mouth, Color::DarkGray);
    // 釘は盤面に固定された構造物なので暗く沈める。玉と同じ明るさで描くと、
    // 点描の粒がどちらのものか判別できず、玉が釘の間を落ちていく動きを
    // 目で追えなくなる。
    draw_points(ctx, &statics.nails, Color::DarkGray);
    draw_points(ctx, &statics.side_pockets, Color::Blue);
    draw_points(ctx, &statics.start_pocket, start_pocket_color);
    draw_points(ctx, &statics.attacker, attacker_color);
}

// ── ホール画面 ─────────────────────────────────────────────────

fn render_hall(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let borders = if narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);

    render_hall_header(state, f, chunks[0], borders, narrow);
    if narrow {
        // ナロー幅ではプレビューを出す余地が無いので、リストの可読性を
        // 優先する。釘の手がかりは行内のヘソの開き (`nail_spread_gauge`)
        // だけになる。
        render_hall_list(state, f, chunks[1], borders, click_state);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(24), Constraint::Length(HALL_PREVIEW_W)])
            .split(chunks[1]);
        render_hall_list(state, f, cols[0], borders, click_state);
        render_hall_preview(state, f, cols[1]);
    }
    render_hall_footer(state, f, chunks[2], narrow, click_state);
}

/// 盤面プレビューに割く列数。ヘソ釘2本の間隔は盤面幅 (`BOARD_W`) の 1割にも
/// 満たないので、狭いと台ごとの開きの差が braille の1点差まで潰れて読めなく
/// なる。台名と実測回転率しか出さないリスト側 (`Constraint::Min`) より優先
/// して確保する。
const HALL_PREVIEW_W: u16 = 40;

/// 選択中の台の盤面プレビュー。遊技中の盤面 (`render_board`) と同じ
/// `board_statics` を描くので、ここで読んだヘソの開きは着席後の盤面と
/// そのまま一致する。
///
/// 描くのは釘と入賞口だけの静止画で、玉やアタッカーの開放状態のような
/// 遊技中にしか存在しない要素は持たない。クリック判定も持たない純粋な
/// 装飾なので、別 DOM 要素は生やさず盤面と同じ `<pre>` 上に描く。
fn render_hall_preview(state: &PachinkoState, f: &mut Frame, area: Rect) {
    let machine = state.hall_cursor_machine();
    let title = match machine {
        Some(m) => format!(" {} の釘 ", m.name),
        None => " 釘 ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, Style::default().fg(Color::Gray)));
    let inner = block.inner(area);
    let Some(machine) = machine else {
        f.render_widget(block, area);
        return;
    };

    // ホールから見えるのは通常時の盤面。電サポでヘソが広がった姿を見せると、
    // 座る前に読んだ開きより実際が狭く、釘読みが当てにならなくなる。
    let statics = board_statics(
        Some(machine),
        logic::pocket_half_w(machine.nail_spread, false),
        round_aspect(inner),
        false,
    );
    let canvas = Canvas::default()
        .x_bounds([0.0, BOARD_W])
        .y_bounds([0.0, BOARD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            draw_board_statics(ctx, &statics, Color::Cyan, Color::DarkGray);
        })
        .block(block);
    f.render_widget(canvas, area);
}

fn render_hall_header(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    narrow: bool,
) {
    let record = &state.record;
    // 記録行は幅に合わせて見出しを詰める。溢れた分は右端で切り落とされ、
    // 最後の項目 (最高連チャン) だけが読めなくなる。
    let record_text = if narrow {
        format!(
            "最高 {}玉 / 当り {}回 / 連 {}",
            format_thousands(record.best_balls as u64),
            record.total_jackpots,
            record.best_chain
        )
    } else {
        format!(
            "最高持ち玉 {}玉 / 大当たり {}回 / 最高連チャン {}",
            format_thousands(record.best_balls as u64),
            record.total_jackpots,
            record.best_chain
        )
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("所持金 ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}円", format_thousands(state.cash as u64)),
                Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   持ち玉 ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}玉", format_thousands(state.balls_held as u64)),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            record_text,
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(ACCENT))
            .title(Span::styled(" 玉響ホール ", Style::default().fg(ACCENT))),
    );
    f.render_widget(para, area);
}

/// ホールの台名の色。座っている (直前まで座っていた) 台だけを目立たせる。
/// 着席の有無 (`PachinkoState::has_seated`) を見るのは、`seat` が初期値 0 を
/// 持つため — 見ないと、まだ一度も座っていないプレイヤーにも先頭の台が
/// 着席中として映る。
fn machine_name_color(state: &PachinkoState, index: usize) -> Color {
    if state.has_seated && index == state.seat {
        Color::LightYellow
    } else {
        ACCENT
    }
}

/// ヘソ釘の開きを釘2本の間隔として文字で描く。プレビューを出せない
/// ナロー幅ではこれが唯一の釘の手がかりになり、プレビューを出せる幅でも
/// 並んだ台を1画面で見比べる手がかりとして効く (プレビューが描くのは
/// 選択中の1台だけなので、台をまたいだ比較は記憶に頼ることになる)。
///
/// 数値ではなく間隔で見せるのは、盤面を観察して読むというこのゲームの
/// 判断を、数字の大小比較にすり替えないため。
fn nail_spread_gauge(nail_spread: f64) -> String {
    /// 間隔の下限と上限 (空白の数)。下限を 0 にすると最も渋い台が「釘2本が
    /// くっついた1つの塊」に見えて開きの差が読めなくなる。
    const MIN_GAP: usize = 1;
    const MAX_GAP: usize = 7;

    let (lo, hi) = logic::NAIL_SPREAD_RANGE;
    let t = if hi > lo {
        ((nail_spread - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gap = MIN_GAP + (t * (MAX_GAP - MIN_GAP) as f64).round() as usize;
    format!("│{}│", " ".repeat(gap))
}

fn render_hall_list(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let cursor = state.clamped_hall_cursor();
    let mut cl = ClickableList::new();
    if state.machines.is_empty() {
        cl.push(Line::from(Span::styled(
            " 開いている台がない",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for (index, machine) in state.machines.iter().enumerate() {
        let action_id = actions::machine_select_id(index);
        let key = char::from_digit(index as u32 + 1, 10).unwrap_or('?');
        let name_color = machine_name_color(state, index);
        // 選択中の台は行頭の印で示す。どの台の釘をプレビューが描いているか
        // が分からないと、釘を読み比べても結び付ける先が無い。
        let selected = cursor == Some(index);
        let marker = if selected { "▶" } else { " " };
        // 3行とも同じ台へ結び付ける。台名と実測値のどちらを触っても座れる
        // 方が、指が行を跨いだ時に「押したのに何も起きない」を避けられる。
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    format!("{marker}[{key}] "),
                    Style::default().fg(if selected { ACCENT } else { Color::DarkGray }),
                ),
                Span::styled(
                    machine.name,
                    Style::default().fg(name_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}・{}", odds_flavor(&machine.spec), payout_flavor(&machine.spec)),
                    Style::default().fg(Color::Gray),
                ),
            ]),
            action_id,
        );
        cl.push_clickable(
            Line::from(vec![
                Span::styled("     ヘソ ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    nail_spread_gauge(machine.nail_spread),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            action_id,
        );
        cl.push_clickable(
            Line::from(Span::styled(
                format!("     {}", spin_rate_text(Some(machine))),
                Style::default().fg(Color::DarkGray),
            )),
            action_id,
        );
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" 台を選ぶ ", Style::default().fg(Color::Gray)));
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        cl,
        &state.hall_scroll,
        actions::HALL_SCROLL_UP,
        actions::HALL_SCROLL_DOWN,
    )
        .block(block)
        .arrow_color(ACCENT)
        .render(f, area, &mut cs);
}

fn render_hall_footer(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if area.height == 0 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let buy_label = if narrow { "[!]玉借り" } else { "[!]玉を借りる" };
    let up_label = if narrow { "[K]前" } else { "[K]前の台" };
    let down_label = if narrow { "[J]次" } else { "[J]次の台" };
    {
        let mut cs = click_state.borrow_mut();
        render_key_row(
            f,
            rows[0],
            &mut cs,
            &[
                (up_label, actions::HALL_CURSOR_UP),
                (down_label, actions::HALL_CURSOR_DOWN),
                (buy_label, actions::BUY_BALLS),
            ],
        );
    }

    if rows[1].height > 0 {
        let hint = if state.machines.is_empty() {
            "台を待っている"
        } else if narrow {
            "ヘソの開きで台を選ぶ"
        } else {
            "選んだ台の釘を右で読み、台をタップして着席する"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {hint}"),
                Style::default().fg(Color::DarkGray),
            ))),
            rows[1],
        );
    }
}

// ── 遊技画面 ───────────────────────────────────────────────────

fn render_playing(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(area);

    let body = if narrow {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(chunks[0])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[0])
    };

    render_board(state, f, body[0], click_state);
    render_info_panel(state, f, body[1], click_state);
    render_playing_footer(state, f, chunks[1], narrow, click_state);
}

/// 盤面 Canvas の楕円を真円に見せるための y 半径の倍率。
///
/// Canvas は x を `BOARD_W`、y を `BOARD_H` で割って描画領域へ写すので、
/// 盤面の縦横比と領域の縦横比が一致しない限り rx=ry の楕円は潰れる。
/// braille の疑似ピクセルは 1セル = 横2 × 縦4 なので、セルの高さは幅の
/// 2倍として数える。極端なレイアウトで玉が線に化けないよう倍率は挟む。
fn round_aspect(inner: Rect) -> f64 {
    if inner.width == 0 || inner.height == 0 {
        return 1.0;
    }
    let scale_x = inner.width as f64 / BOARD_W;
    let scale_y = inner.height as f64 * 2.0 / BOARD_H;
    if scale_y <= 0.0 {
        return 1.0;
    }
    (scale_x / scale_y).clamp(0.5, 3.0)
}

/// ラウンドの規定カウントの消化具合。1 tick の間に複数の玉がアタッカーへ
/// 入ると `count` は規定数を超える。賞球は実機と同じくオーバー入賞分も
/// 払うので、表示だけを規定数で止めて「3/2」のような読めない進捗にしない。
fn round_progress(j: JackpotState) -> String {
    format!("{}/{}", j.count.min(ROUND_COUNT), ROUND_COUNT)
}

fn board_title(state: &PachinkoState) -> String {
    let name = state.seated_machine().map(|m| m.name).unwrap_or("空き台");
    match state.mode {
        Mode::Jackpot(j) => format!(
            " {name}  大当たり {}R/{}R  {} ",
            j.round,
            j.total_rounds,
            round_progress(j)
        ),
        Mode::Kakuhen { spins_left: 0 } => format!(" {name}  確変 "),
        Mode::Kakuhen { spins_left } => format!(" {name}  確変 残り{spins_left} "),
        Mode::Jitan { spins_left } => format!(" {name}  時短 残り{spins_left} "),
        Mode::Normal => format!(" {name} "),
    }
}

/// 盤面の枠色。リーチに入った瞬間だけ格の色で縁を光らせ、盤面の玉を目で
/// 追っている間にも「今の回転はリーチだ」と気付けるようにする。大当たり中は
/// リーチより後に来た確定した結果なので、そちらの色で上書きする。
fn board_border_color(state: &PachinkoState) -> Color {
    if matches!(state.mode, Mode::Jackpot(_)) {
        Color::LightRed
    } else if state.reach_flash > 0 {
        state.reach_flash_kind.color()
    } else {
        ACCENT
    }
}

// ── 確定シグナル ───────────────────────────────────────────────

/// 確定シグナルが立っている回転の残り tick。虹が枠を回る速さの位相に使う。
///
/// この演出は当たりでしか立たない (`logic::pick_confirmed`) 唯一のもので、
/// 他のどの演出とも混ざらない見せ方にする。確定が存在すること自体が、
/// 確定ではない他の全ての演出に「まだ分からない」という緊張を与える。
fn confirmed_signal_phase(state: &PachinkoState) -> Option<u32> {
    match &state.digit {
        Digit::Spinning {
            ticks_left,
            outcome,
        } if outcome.confirmed => Some(*ticks_left),
        _ => None,
    }
}

/// 虹が枠を1周する速さ (1 tick あたりの色相の度数)。
const RAINBOW_DEG_PER_TICK: f64 = 26.0;

/// 色相環の1点を彩度・明度いっぱいの RGB にする。虹を「流れている」と
/// 感じさせるには隣り合うセルの色が連続している必要があり、16色のパレット
/// では段が飛んでしまう。
fn hue_color(degrees: f64) -> Color {
    let h = degrees.rem_euclid(360.0) / 60.0;
    let x = ((1.0 - (h % 2.0 - 1.0).abs()) * 255.0) as u8;
    match h as u32 {
        0 => Color::Rgb(255, x, 0),
        1 => Color::Rgb(x, 255, 0),
        2 => Color::Rgb(0, 255, x),
        3 => Color::Rgb(0, x, 255),
        4 => Color::Rgb(x, 0, 255),
        _ => Color::Rgb(255, 0, x),
    }
}

/// 枠セルを外周に沿って一筆書きの順に並べる。並び順がそのまま虹の流れる
/// 向きになるので、時計回りに一周させる。
fn border_cells(area: Rect) -> Vec<(u16, u16)> {
    let (right, bottom) = (area.right() - 1, area.bottom() - 1);
    let mut cells = Vec::new();
    for x in area.x..=right {
        cells.push((x, area.y));
    }
    for y in (area.y + 1)..=bottom {
        cells.push((right, y));
    }
    for x in (area.x..right).rev() {
        cells.push((x, bottom));
    }
    for y in ((area.y + 1)..bottom).rev() {
        cells.push((area.x, y));
    }
    cells
}

/// 盤面の枠へ虹を流す。枠は既に描かれている前提で、色だけを塗り替える。
/// 別 DOM 要素を足さずに済ませるため、同じ `<pre>` のセルを直接染める。
fn paint_rainbow_border(f: &mut Frame, area: Rect, phase: u32) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let cells = border_cells(area);
    let total = cells.len() as f64;
    let buf = f.buffer_mut();
    for (index, position) in cells.into_iter().enumerate() {
        let Some(cell) = buf.cell_mut(position) else {
            continue;
        };
        let hue = index as f64 / total * 360.0 + phase as f64 * RAINBOW_DEG_PER_TICK;
        cell.set_fg(hue_color(hue));
    }
}

// ── 熱い瞬間の画面効果 ─────────────────────────────────────────

/// 盤面をずらして見せる tick 数。
const SHAKE_TICKS: u8 = 3;

/// 盤面を1セル横へずらすか。赤以上の保留が出た瞬間と確定シグナルの発生時に
/// 限る — 揺れは「他と違うことが起きた」という合図なので、頻度が上がるほど
/// 合図としての意味が薄れる。
fn board_shake(state: &PachinkoState) -> bool {
    let hot_pending = state.pending.iter().any(|p| {
        p.rank >= PendingRank::Red && p.promote_flash + SHAKE_TICKS > PENDING_PROMOTE_FLASH_TICKS
    });
    let confirmed_start = match &state.digit {
        Digit::Spinning {
            ticks_left,
            outcome,
        } => {
            outcome.confirmed
                && ticks_left + SHAKE_TICKS as u32
                    > outcome.reach.spin_ticks() + outcome.stop.extra_ticks()
        }
        Digit::Idle => false,
    };
    hot_pending || confirmed_start
}

/// 揺れている間の盤面の描画領域。幅を1つ削って右へ寄せることで、盤面の
/// 中身ごと1セル動く。削れないほど狭い領域では揺らさない。
fn shaken_area(state: &PachinkoState, area: Rect) -> Rect {
    if area.width < 3 || !board_shake(state) {
        return area;
    }
    Rect::new(area.x + 1, area.y, area.width - 1, area.height)
}

/// ヘソの色。玉が入った直後だけ白く光らせる。釘に弾かれた玉
/// (`Ball::hit_glow`) と同じ見せ方にすることで、盤面の白さが一貫して
/// 「今この瞬間に何かが当たった」印になる。
fn start_pocket_color(state: &PachinkoState) -> Color {
    if state.start_flash > 0 {
        Color::White
    } else if state.mode.is_assisted() {
        Color::LightCyan
    } else {
        Color::Cyan
    }
}

fn render_board(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let area = shaken_area(state, area);
    let jackpot = match state.mode {
        Mode::Jackpot(j) => Some(j),
        _ => None,
    };
    let border_color = board_border_color(state);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(board_title(state), Style::default().fg(border_color)));
    let inner = block.inner(area);
    let aspect = round_aspect(inner);

    // ── paint は move クロージャなので、描くものは全て所有権付きの Vec で先に組む ──

    let attacker_open = jackpot.is_some();
    let statics = board_statics(
        state.seated_machine(),
        logic::effective_pocket_half_w(state),
        aspect,
        attacker_open,
    );
    let start_pocket_color = start_pocket_color(state);
    let attacker_color = if attacker_open {
        Color::LightRed
    } else {
        Color::DarkGray
    };

    // 釘に弾かれた直後の玉だけ白く光らせ、どこで跳ねたかを目で追えるようにする。
    let mut ball_pts: Vec<(f64, f64)> = Vec::new();
    let mut glow_pts: Vec<(f64, f64)> = Vec::new();
    for ball in &state.balls {
        let pts = canvas_fx::filled_ellipse_points(
            ball.x,
            board_to_canvas_y(ball.y),
            BALL_R,
            BALL_R * aspect,
            0.25,
        );
        if ball.hit_glow > 0 {
            glow_pts.extend(pts);
        } else {
            ball_pts.extend(pts);
        }
    }

    // 大当たり中は盤面そのものを脈打たせる。ラウンドの残り時間を位相に
    // 使うので、ラウンドが変わるたびに脈の大きさが一度リセットされる。
    let jackpot_ring: Vec<(f64, f64)> = jackpot
        .map(|j| {
            let phase = (j.ticks_left % 20) as f64 / 20.0;
            canvas_fx::ring_points(
                BOARD_W / 2.0,
                BOARD_H / 2.0,
                24.0 + phase * 6.0,
                0.05,
            )
        })
        .unwrap_or_default();

    let canvas = Canvas::default()
        .x_bounds([0.0, BOARD_W])
        .y_bounds([0.0, BOARD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            draw_board_statics(ctx, &statics, start_pocket_color, attacker_color);
            // 玉は盤面で唯一動くものなので、固定物 (釘・入賞口) より明るく
            // 描いて視線を集める。弾かれた瞬間だけさらに白く飛ばすことで、
            // 「今この瞬間に当たった」印はヘソの入賞と同じ白で統一される。
            draw_points(ctx, &ball_pts, Color::LightYellow);
            draw_points(ctx, &glow_pts, Color::White);
            draw_points(ctx, &jackpot_ring, Color::LightRed);
        })
        .block(block);

    Clickable::new(canvas, actions::BOARD_TAP).render(f, area, &mut click_state.borrow_mut());
    if let Some(phase) = confirmed_signal_phase(state) {
        paint_rainbow_border(f, area, phase);
    }
    render_board_banner(state, f, inner);
    if let Some(j) = jackpot {
        render_jackpot_theater(state, f, inner, j);
    }
}

// ── 大当たり中の出玉カウンタ ───────────────────────────────────

/// 出玉カウンタの数字。3×5 のドット絵を行ごとに持ち、各行の下位3ビットが
/// 左から右のドットにあたる。
const BIG_DIGIT_ROWS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// ドット絵の高さ (行数)。
const BIG_DIGIT_ROW_COUNT: usize = 5;
/// 出玉カウンタに割く高さ (セル)。braille は 1 セル = 横2×縦4 の疑似ピクセル
/// なので、2 セルで縦 8 ピクセルが使える。
const BIG_DIGIT_H: u16 = 2;
/// ドット絵1行分の縦のピクセル数。5行を縦 8 ピクセルへ引き伸ばす倍率。
const BIG_DIGIT_ROW_PX: f64 = 1.4;
/// 疑似ピクセル1つを塗るときのサンプリング間隔。braille の 1 点より細かく
/// することで、引き伸ばした数字の縁が欠けない。
const BIG_DIGIT_STEP: f64 = 0.3;

/// 数字列を縦 `BIG_DIGIT_ROW_COUNT` の点灯パターンへ展開する。1要素が
/// 1ピクセル列で、下位ビットから順に上の行にあたる。
///
/// 桁の間と区切りのコンマも列として持つことで、描画側は列を左から並べる
/// だけでよくなり、文字ごとの幅を意識せずに済む。
fn big_number_columns(text: &str) -> Vec<u8> {
    let mut columns: Vec<u8> = Vec::new();
    for ch in text.chars() {
        match ch.to_digit(10) {
            Some(value) => {
                for col in 0..3u32 {
                    let mut bits = 0u8;
                    for (row, pattern) in BIG_DIGIT_ROWS[value as usize].iter().enumerate() {
                        if pattern & (0b100 >> col) != 0 {
                            bits |= 1 << row;
                        }
                    }
                    columns.push(bits);
                }
            }
            // コンマは最下行の1点だけ。桁区切りが数字と同じ大きさで並ぶと、
            // どこが桁の切れ目なのか却って読めなくなる。
            None => columns.push(1 << (BIG_DIGIT_ROW_COUNT - 1)),
        }
        columns.push(0);
    }
    columns.pop();
    columns
}

/// 展開した点灯パターンを Canvas の点群にする。`invert` を立てると点灯と
/// 消灯を入れ替え、数字が塗り潰しから抜けた形になる。
fn big_number_points(columns: &[u8], invert: bool, top_y: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for (col, bits) in columns.iter().enumerate() {
        for row in 0..BIG_DIGIT_ROW_COUNT {
            if (bits & (1 << row) != 0) == invert {
                continue;
            }
            let y = top_y - (row + 1) as f64 * BIG_DIGIT_ROW_PX;
            points.extend(canvas_fx::filled_rect_points(
                col as f64,
                y,
                col as f64 + 1.0,
                y + BIG_DIGIT_ROW_PX,
                BIG_DIGIT_STEP,
            ));
        }
    }
    points
}

/// 桁上がりで数字を反転させる tick 数。
const CARRY_FLASH_TICKS: f64 = 2.0;

/// 桁が1つ増えた直後か。表示値は実際の値へ毎 tick 一定割合ずつ寄る
/// (`logic::PAYOUT_EASE_RATE`) ので、今の桁の最小値からの距離が直近数 tick 分の
/// 伸びに収まっていれば、桁が変わったのはその数 tick の間だと分かる。表示値
/// だけから導けるので、演出のための状態を state 側に増やさずに済む。
fn payout_carry_flash(state: &PachinkoState) -> bool {
    let shown = state.jackpot_payout_shown;
    if shown < 10.0 {
        return false;
    }
    let step = (state.jackpot_payout() as f64 - shown) * logic::PAYOUT_EASE_RATE;
    if step <= 0.0 {
        return false;
    }
    let decade = 10f64.powi(shown.log10().floor() as i32);
    shown - decade < step * CARRY_FLASH_TICKS
}

/// ラウンド内の規定カウントをドットで見せる。1個入賞するたびに埋まるので、
/// アタッカーへ入った瞬間が数字の増加とは別の形でも返ってくる。
fn round_dots(j: JackpotState) -> String {
    (0..ROUND_COUNT)
        .map(|i| if i < j.count { '●' } else { '○' })
        .collect()
}

/// 大当たり中に盤面へ重ねる出玉の劇場。数字はドット絵で大きく描き、その下に
/// ラウンドの進行を置く。別 DOM 要素を足さず同じ `<pre>` へ上書きするので、
/// 下の Canvas に登録済みのタップ判定 (`BOARD_TAP`) は保たれる。
///
/// 描く余地が無い狭さでは1行の文字表示へ落とす。カウンタが盤面を覆って
/// 玉が見えなくなると、出玉が増える理由そのものが画面から消える。
fn render_jackpot_theater(
    state: &PachinkoState,
    f: &mut Frame,
    inner: Rect,
    j: JackpotState,
) {
    if inner.width < 12 || inner.height < 3 {
        return;
    }
    let shown = state.jackpot_payout_shown.round().max(0.0) as u64;
    let columns = big_number_columns(&format_thousands(shown));
    // 疑似ピクセルは 1 セルにつき横 2 つ。奇数列で右端が欠けないよう切り上げる。
    let digits_w = columns.len().div_ceil(2) as u16;

    let round_line = Line::from(vec![
        Span::styled(
            format!(" R {}/{} ", j.round, j.total_rounds),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", round_dots(j)),
            Style::default().fg(Color::LightYellow),
        ),
    ]);

    // 数字を出す余地が無ければ、玉数を文字のまま1行に収める。
    if inner.height < 2 + BIG_DIGIT_H || digits_w + 6 > inner.width {
        let text = format!(" {}玉  R {}/{} ", format_thousands(shown), j.round, j.total_rounds);
        let row = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            row,
        );
        return;
    }

    let invert = payout_carry_flash(state);
    let points = big_number_points(&columns, invert, BIG_DIGIT_H as f64 * 4.0 - 0.5);
    let width_px = columns.len() as f64;
    let digits_x = inner.x + (inner.width - digits_w) / 2;
    let digits_area = Rect::new(
        digits_x,
        inner.bottom() - 1 - BIG_DIGIT_H,
        digits_w,
        BIG_DIGIT_H,
    );
    let canvas = Canvas::default()
        .x_bounds([0.0, width_px])
        .y_bounds([0.0, BIG_DIGIT_H as f64 * 4.0])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            draw_points(ctx, &points, Color::LightYellow);
        });
    f.render_widget(canvas, digits_area);

    // 数字の左に単位を添える。数字だけでは持ち玉との区別が付かない。
    if digits_area.x > inner.x + 4 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "出玉",
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            ))),
            Rect::new(digits_area.x - 5, digits_area.y, 4, 1),
        );
    }
    f.render_widget(
        Paragraph::new(round_line).alignment(Alignment::Center),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}

/// 盤面の最上段に重ねる1行。別 DOM 要素を足さず同じ `<pre>` へ上書きする
/// ので、下の Canvas に登録済みのタップ判定 (`BOARD_TAP`) は保たれる。
fn render_board_banner(state: &PachinkoState, f: &mut Frame, inner: Rect) {
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let (text, style) = match (state.mode, &state.digit) {
        (Mode::Jackpot(j), _) => (
            format!("大当たり {}R", j.total_rounds),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        (_, Digit::Spinning { outcome, .. }) if !outcome.reach.shout().is_empty() => (
            outcome.reach.shout().to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(outcome.reach.color())
                .add_modifier(Modifier::BOLD),
        ),
        _ if state.firing => (
            "打ち出し中".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        _ => (
            "タップで打ち出し".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    };
    let banner = Rect::new(inner.x, inner.y, inner.width, 1);
    let para = Paragraph::new(Line::from(Span::styled(format!(" {text} "), style)))
        .alignment(Alignment::Center);
    f.render_widget(para, banner);
}

// ── 情報パネル ─────────────────────────────────────────────────

fn tab_action(tab: InfoTab) -> u16 {
    match tab {
        InfoTab::Board => actions::TAB_BOARD,
        InfoTab::History => actions::TAB_HISTORY,
        InfoTab::Record => actions::TAB_RECORD,
    }
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_info_panel(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    {
        let mut bar = TabBar::new("│");
        for tab in InfoTab::ALL {
            bar = bar.tab(tab.label(), tab_style(state.tab == tab), tab_action(tab));
        }
        let mut cs = click_state.borrow_mut();
        bar.render(f, chunks[0], &mut cs);
    }

    let list = match state.tab {
        InfoTab::Board => board_tab_list(state),
        InfoTab::History => history_tab_list(state),
        InfoTab::Record => record_tab_list(state),
    };
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        list,
        &state.info_scroll,
        actions::INFO_SCROLL_UP,
        actions::INFO_SCROLL_DOWN,
    )
        .arrow_color(ACCENT)
        .render(f, chunks[1], &mut cs);
}

/// 左のリールが止まってから右が止まるまでの tick 数。左右が先に揃うことで
/// 「リーチになった」ことが中が止まる前に伝わる。
const LEFT_STOP_TICKS: u32 = 4;
const RIGHT_STOP_TICKS: u32 = 8;
/// 中リールが止まる残り tick。左右が揃ってから当落が見えるまでの「間」を
/// 作るため、最後まで引っ張ってから止める。
///
/// 残り tick で測るのは、回転の長さが `ReachKind` ごとに違うため。経過 tick
/// で測ると、最も短い通常回転 (`ReachKind::None`) では右より先に中が止まり、
/// 左→右→中という停止順が崩れる。
const MIDDLE_STOP_REMAINING_TICKS: u32 = 3;

/// テンパイ後、中桁が1コマ進むのに要する tick を停止に近い側から並べたもの。
/// 手前ほど長く留まることで、残り1コマの重みが最大になる。手前から数えて
/// この表を使い切った先は毎 tick 1コマ進む (＝まだ減速していない区間)。
const MIDDLE_SLOW_HOLDS: [u32; 5] = [5, 3, 3, 2, 2];

/// 滑りで中桁が動いた後、出目を見せる tick 数。`StopStyle::Slip` の
/// `extra_ticks` のうち、ここを引いた残りがハズレ位置で止まって見える時間になる。
const SLIP_MOVE_TICKS: u32 = 2;
/// 復活で中桁が戻ってから停止するまでの tick 数。`StopStyle::Revival` の
/// `extra_ticks` のうち、ここを引いた残りが「ハズレで確定した」静止時間になる。
const REVIVAL_RETURN_TICKS: u32 = 8;

fn digit_char(value: u8) -> char {
    char::from_digit(value as u32 % 10, 10).unwrap_or('0')
}

/// 中桁の停止まで残り `ticks_to_stop` tick の時点で、出目から何コマ手前に
/// いるか。`MIDDLE_SLOW_HOLDS` の滞留時間を停止側から積み上げて逆に引く。
fn middle_slow_offset(ticks_to_stop: u32) -> u32 {
    let mut acc = 0;
    for (index, &hold) in MIDDLE_SLOW_HOLDS.iter().enumerate() {
        acc += hold;
        if ticks_to_stop <= acc {
            return index as u32 + 1;
        }
    }
    MIDDLE_SLOW_HOLDS.len() as u32 + (ticks_to_stop - acc)
}

/// 中桁が最後に動く前に見せる目と、その動きを表す印。滑りと復活は
/// 「一度止まったように見せてから動かす」型なので、出目そのものではなく
/// 1コマずれた目で止まったふりをする。
///
/// 滑りは1コマ下がって出目になる位置から始めるが、そこが左と同じ目になる
/// 場合だけ反対側から寄せる — ハズレなのに一度ゾロ目を見せてから崩す形に
/// なると、揃った瞬間の意味そのものが信用できなくなる。
fn middle_pre_stop(outcome: &SpinOutcome) -> (u8, char) {
    let middle = outcome.reels[1] % 10;
    let left = outcome.reels[0] % 10;
    match outcome.stop {
        StopStyle::Slip => {
            let above = (middle + 1) % 10;
            if above == left {
                ((middle + 9) % 10, '↑')
            } else {
                (above, '↓')
            }
        }
        StopStyle::Revival => ((middle + 9) % 10, '←'),
        StopStyle::Plain | StopStyle::NearMiss => (middle, ' '),
    }
}

/// 液晶に出す1コマ分の絵。
struct ReelView {
    faces: [char; 3],
    /// 中桁の動きを説明する印。動いていない間は空白。
    cue: char,
    style: Style,
}

/// 回転中の色。格の色をそのまま使う。
fn spinning_style(outcome: &SpinOutcome) -> Style {
    Style::default()
        .fg(outcome.reach.color())
        .add_modifier(Modifier::BOLD)
}

/// デジタルの1コマ。回転中は左→右→中の順に停止させ、テンパイ後の中桁は
/// 出目へ1コマずつ近づけながら減速する。停止中は最後に止まった出目
/// (`last_reels`) を出し続ける。
///
/// 追加の回転時間 (`StopStyle::extra_ticks`) は末尾に確保されているものとして
/// 扱う。基本の回転で一度出目まで持っていき、そこから滑る・戻るという順序が
/// 「止まったと思わせてから動かす」型の前提になる。
fn reel_view(digit: &Digit, last_reels: [u8; 3]) -> ReelView {
    let Digit::Spinning {
        ticks_left,
        outcome,
    } = digit
    else {
        return ReelView {
            faces: last_reels.map(digit_char),
            cue: ' ',
            style: Style::default().fg(Color::DarkGray),
        };
    };

    let extra = outcome.stop.extra_ticks();
    let base_left = ticks_left.saturating_sub(extra);
    let base_elapsed = outcome.reach.spin_ticks().saturating_sub(base_left);
    let rolling = |slot: u32| digit_char((ticks_left.wrapping_mul(3) + slot * 7) as u8);
    let left = if base_elapsed >= LEFT_STOP_TICKS {
        digit_char(outcome.reels[0])
    } else {
        rolling(0)
    };
    let right = if base_elapsed >= RIGHT_STOP_TICKS {
        digit_char(outcome.reels[2])
    } else {
        rolling(2)
    };
    // 左右が揃って初めて中桁の1コマに意味が生まれる。揃わないうちに減速
    // させても、遅くなった分だけ待ち時間が延びるだけになる。
    let tempai = base_elapsed >= RIGHT_STOP_TICKS && outcome.reels[0] == outcome.reels[2];
    let (pre_stop, move_cue) = middle_pre_stop(outcome);
    let final_middle = outcome.reels[1] % 10;

    let (middle, cue, style) = if *ticks_left > extra {
        let value = if base_left <= MIDDLE_STOP_REMAINING_TICKS {
            pre_stop
        } else if tempai {
            (pre_stop + middle_slow_offset(base_left - MIDDLE_STOP_REMAINING_TICKS) as u8) % 10
        } else {
            // テンパイ前は目を追う相手がいないので、コマ送りではなく
            // 回っていることだけが分かればよい。
            return ReelView {
                faces: [left, rolling(1), right],
                cue: ' ',
                style: spinning_style(outcome),
            };
        };
        (digit_char(value), ' ', spinning_style(outcome))
    } else {
        match outcome.stop {
            // 滑りと復活は基本の回転で1コマ手前に止まっており、ここで動く。
            StopStyle::Slip if *ticks_left > SLIP_MOVE_TICKS => (
                digit_char(pre_stop),
                ' ',
                Style::default().fg(Color::Gray),
            ),
            StopStyle::Slip => (
                digit_char(final_middle),
                move_cue,
                // 動いた瞬間は盤面の「今この瞬間に何かが当たった」印と同じ白に
                // 揃える。まだ当落は分からないので、格の色は出さない。
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            StopStyle::Revival if *ticks_left > REVIVAL_RETURN_TICKS => (
                digit_char(pre_stop),
                ' ',
                Style::default().fg(Color::Gray),
            ),
            StopStyle::Revival => (
                digit_char(final_middle),
                move_cue,
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            ),
            // 惜しいハズレは出目のまま数 tick 留める。次の回転へすぐ移ると、
            // 惜しかったこと自体が画面に残らない。色を落とすことで、
            // 引っ張った末に何も起きなかったという結末を先に伝える。
            StopStyle::NearMiss | StopStyle::Plain => (
                digit_char(final_middle),
                ' ',
                Style::default().fg(Color::Gray),
            ),
        }
    };

    ReelView {
        faces: [left, middle, right],
        cue,
        style,
    }
}

/// デジタルの3桁だけを取り出す。停止順と出目の対応をテストで突き合わせる
/// ためのもので、描画は `reel_view` をそのまま使う。
#[cfg(test)]
fn reel_faces(digit: &Digit, last_reels: [u8; 3]) -> [char; 3] {
    reel_view(digit, last_reels).faces
}

fn mode_text(state: &PachinkoState) -> String {
    match state.mode {
        Mode::Normal => "通常".to_string(),
        Mode::Kakuhen { spins_left: 0 } => "確変 (次の大当たりまで)".to_string(),
        Mode::Kakuhen { spins_left } => format!("確変 残り{spins_left}回転"),
        Mode::Jitan { spins_left } => format!("時短 残り{spins_left}回転"),
        Mode::Jackpot(j) => format!(
            "大当たり {}R/{}R  {}",
            j.round,
            j.total_rounds,
            round_progress(j)
        ),
    }
}

fn mode_color(mode: Mode) -> Color {
    match mode {
        Mode::Normal => Color::Gray,
        Mode::Kakuhen { .. } => Color::LightMagenta,
        Mode::Jitan { .. } => Color::LightCyan,
        Mode::Jackpot(_) => Color::LightRed,
    }
}

/// 空きスロットの印。ランクの記号 (`PendingRank::mark`) と形がぶつからない
/// 点にすることで、白保留と空きが同じ絵にならない。
const PENDING_EMPTY_MARK: char = '·';

/// 保留の列。1スロットを「消化の順番の印 / ランクの記号 / 昇格の印」の3文字で
/// 描く。ランクは色と形の二重符号化 (`PendingRank::mark` / `color`) をそのまま
/// 出し、信頼度は数値にしない — どのランクがどれだけ当たるかを見つけるのは
/// プレイヤー側の領分にする。
fn pending_spans(state: &PachinkoState) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(MAX_PENDING * 3);
    for slot in 0..MAX_PENDING {
        let Some(pending) = state.pending.get(slot) else {
            spans.push(Span::styled(
                format!(" {PENDING_EMPTY_MARK} "),
                Style::default().fg(Color::DarkGray),
            ));
            continue;
        };
        let color = pending.rank.color();
        // 次に消化される保留を指す。どれが今から回るのかが分からないと、
        // 熱い保留を見つけても「あと何回転待つのか」が読めない。
        spans.push(Span::styled(
            if slot == 0 { "▶" } else { " " },
            Style::default().fg(ACCENT),
        ));
        let promoting = pending.promote_flash > 0;
        let mark_style = if promoting {
            // 昇格した瞬間だけ地と図を入れ替える。ランクが上がったことは
            // 記号が変わるだけでは見逃されるので、変わった瞬間そのものを
            // 別の見え方にする。
            Style::default()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        spans.push(Span::styled(pending.rank.mark().to_string(), mark_style));
        spans.push(Span::styled(
            if promoting { "↑" } else { " " },
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

fn power_bar(power: u8) -> String {
    const CELLS: usize = 10;
    let filled = (power as usize * CELLS / 100).min(CELLS);
    (0..CELLS)
        .map(|i| {
            if i < filled {
                theme::PROGRESS_FULL
            } else {
                theme::PROGRESS_EMPTY
            }
        })
        .collect()
}

fn label_value_line(label: &str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label} "), Style::default().fg(Color::DarkGray)),
        Span::styled(value, Style::default().fg(color)),
    ])
}

fn divider_line() -> Line<'static> {
    Line::from(Span::styled(
        " ────────────",
        Style::default().fg(Color::DarkGray),
    ))
}

fn board_tab_list(state: &PachinkoState) -> ClickableList<'static> {
    let mut cl = ClickableList::new();

    let view = reel_view(&state.digit, state.last_reels);
    let frame_style = Style::default().fg(Color::DarkGray);
    cl.push(Line::from(Span::styled(" ┌───────┐", frame_style)));
    cl.push(Line::from(vec![
        Span::styled(" │ ", frame_style),
        Span::styled(
            format!("{} {} {}", view.faces[0], view.faces[1], view.faces[2]),
            view.style,
        ),
        Span::styled(" │", frame_style),
    ]));
    // 中桁の下に印の行を常に置く。動きが無い間も空けておくことで、滑りや
    // 復活で印が出た時に他の行がずれず、動いた1コマだけに目が行く。
    cl.push(Line::from(vec![
        Span::styled(" │ ", frame_style),
        Span::styled(format!("  {}  ", view.cue), view.style),
        Span::styled(" │", frame_style),
    ]));
    cl.push(Line::from(Span::styled(" └───────┘", frame_style)));

    // リーチの格は色と一言でだけ伝える。どの格がどれだけ当たるかは、
    // 打って覚えるプレイヤー側の領分にする。
    if let Digit::Spinning { outcome, .. } = &state.digit {
        let shout = outcome.reach.shout();
        if !shout.is_empty() {
            cl.push(Line::from(Span::styled(
                format!(" {shout}"),
                Style::default()
                    .fg(outcome.reach.color())
                    .add_modifier(Modifier::BOLD),
            )));
        }
    }

    let mut pending_line = vec![Span::styled(" 保留 ", Style::default().fg(Color::DarkGray))];
    pending_line.extend(pending_spans(state));
    cl.push(Line::from(pending_line));
    cl.push(label_value_line(
        "状態",
        mode_text(state),
        mode_color(state.mode),
    ));
    if state.chain > 0 {
        cl.push(label_value_line(
            "連チャン",
            format!("{}連", state.chain),
            Color::LightMagenta,
        ));
    }
    // 大当たりの決算。アタッカーが閉じた時点で出玉が確定するので、
    // 大当たり中は進行中のカウンタ (`render_jackpot_theater`) に譲る。
    if !matches!(state.mode, Mode::Jackpot(_)) && state.last_jackpot_payout > 0 {
        let chain = state.chain.max(1);
        cl.push(label_value_line(
            "前回の当たり",
            format!(
                "{}玉 / {chain}連",
                format_thousands(state.last_jackpot_payout as u64)
            ),
            Color::LightRed,
        ));
    }

    cl.push(divider_line());
    cl.push(label_value_line(
        "持ち玉",
        format!("{}玉", format_thousands(state.balls_held as u64)),
        ACCENT,
    ));
    cl.push(label_value_line(
        "現金",
        format!("{}円", format_thousands(state.cash as u64)),
        Color::LightYellow,
    ));
    cl.push(label_value_line(
        "投資",
        format!("{}円", format_thousands(state.invested as u64)),
        Color::Gray,
    ));
    let balance = visit_balance(state);
    cl.push(label_value_line(
        "収支",
        format_signed_yen(balance),
        balance_color(balance),
    ));

    cl.push(divider_line());
    cl.push(Line::from(vec![
        Span::styled(" 強度 ", Style::default().fg(Color::DarkGray)),
        Span::styled(power_bar(state.power), Style::default().fg(Color::LightYellow)),
        Span::styled(format!(" {}", state.power), Style::default().fg(Color::Gray)),
    ]));
    cl.push(label_value_line(
        "打ち出し",
        if state.firing { "稼働中" } else { "停止中" }.to_string(),
        if state.firing {
            Color::LightGreen
        } else {
            Color::DarkGray
        },
    ));
    cl.push(Line::from(Span::styled(
        format!(" {}", spin_rate_text(state.seated_machine())),
        Style::default().fg(Color::Gray),
    )));

    if !state.log.is_empty() {
        cl.push(divider_line());
        for entry in state.log.iter().take(5) {
            cl.push(Line::from(Span::styled(
                format!(" {entry}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    cl
}

fn history_tab_list(state: &PachinkoState) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    if state.history.is_empty() {
        cl.push(Line::from(Span::styled(
            " まだ大当たりしていない",
            Style::default().fg(Color::DarkGray),
        )));
        return cl;
    }
    cl.push(Line::from(Span::styled(
        " 大当たり履歴 (新しい順)",
        Style::default().fg(Color::Gray),
    )));
    for entry in &state.history {
        let (tail, color) = if entry.kakuhen {
            ("確変", Color::LightMagenta)
        } else {
            ("時短", Color::LightCyan)
        };
        cl.push(Line::from(vec![
            Span::styled(
                format!(" {:>2}R ", entry.rounds),
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            ),
            Span::styled(tail, Style::default().fg(color)),
            Span::styled(
                format!("  {}回転", entry.spins_before),
                Style::default().fg(Color::Gray),
            ),
        ]));
    }
    cl
}

fn record_tab_list(state: &PachinkoState) -> ClickableList<'static> {
    let record = &state.record;
    let mut cl = ClickableList::new();
    cl.push(label_value_line(
        "最高持ち玉",
        format!("{}玉", format_thousands(record.best_balls as u64)),
        ACCENT,
    ));
    cl.push(label_value_line(
        "大当たり",
        format!("{}回", record.total_jackpots),
        Color::LightRed,
    ));
    cl.push(label_value_line(
        "最高連チャン",
        format!("{}連", record.best_chain),
        Color::LightMagenta,
    ));
    cl.push(divider_line());
    cl.push(label_value_line(
        "総投資",
        format!("{}円", format_thousands(record.total_invested as u64)),
        Color::Gray,
    ));
    cl.push(label_value_line(
        "総回収",
        format!("{}円", format_thousands(record.total_returned as u64)),
        Color::Gray,
    ));
    let lifetime = record.total_returned as i64 - record.total_invested as i64;
    cl.push(label_value_line(
        "通算収支",
        format_signed_yen(lifetime),
        balance_color(lifetime),
    ));
    cl
}

// ── フッタ ─────────────────────────────────────────────────────

/// キーヒントを等幅に割り、1項目ずつ `Clickable` で登録する。行を丸ごと
/// 1つのボタンにすると、どのキーを押したのかタップでは選べなくなる。
fn render_key_row(f: &mut Frame, area: Rect, cs: &mut ClickState, items: &[(&str, u16)]) {
    if area.height == 0 || items.is_empty() {
        return;
    }
    let denominator = items.len() as u32;
    let constraints: Vec<Constraint> = items
        .iter()
        .map(|_| Constraint::Ratio(1, denominator))
        .collect();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);
    for (i, &(label, action_id)) in items.iter().enumerate() {
        let para = Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(Color::Gray),
        )));
        Clickable::new(para, action_id).render(f, cols[i], cs);
    }
}

fn render_playing_footer(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if area.height == 0 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let fire_label = if narrow { "[A]打つ" } else { "[A]打ち出し" };
    let buy_label = if narrow { "[!]玉借り" } else { "[!]玉を借りる" };
    let leave_label = if narrow { "[Q]席立つ" } else { "[Q]席を立つ" };
    {
        let mut cs = click_state.borrow_mut();
        render_key_row(
            f,
            rows[0],
            &mut cs,
            &[
                (fire_label, actions::TOGGLE_FIRE),
                (buy_label, actions::BUY_BALLS),
                (leave_label, actions::LEAVE_SEAT),
            ],
        );
    }

    if rows[1].height == 0 {
        return;
    }
    // 強度は両端のボタンで動かし、間に現在値を出す。押した結果がその場で
    // 見えるので、適正値を台ごとに探る操作が1行に収まる。
    let power_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(0),
            Constraint::Length(6),
        ])
        .split(rows[1]);
    {
        let mut cs = click_state.borrow_mut();
        render_key_row(f, power_cols[0], &mut cs, &[("[H]弱", actions::POWER_DOWN)]);
        render_key_row(f, power_cols[2], &mut cs, &[("[L]強", actions::POWER_UP)]);
    }
    let para = Paragraph::new(Line::from(vec![
        Span::styled(power_bar(state.power), Style::default().fg(Color::LightYellow)),
        Span::styled(format!(" {}", state.power), Style::default().fg(Color::Gray)),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(para, power_cols[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    use crate::games::pachinko::state::{
        Ball, Digit, Pending, PendingRank, ReachKind, SpinOutcome, StopStyle, INITIAL_REELS,
        REACH_FLASH_TICKS, START_FLASH_TICKS,
    };

    /// `Game::render` ではなく `render` を直接叩く。前者は `crate::time::now_ms()`
    /// を経由し、native のテストでは panic する。
    fn render_to_test_backend_with_click_state(
        state: &PachinkoState,
        width: u16,
        height: u16,
    ) -> Rc<RefCell<ClickState>> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = width;
        cs.borrow_mut().terminal_rows = height;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                render(state, f, f.area(), &cs);
            })
            .unwrap();
        cs
    }

    /// `click_state` に登録された領域のうち `action_id` を返すセルが
    /// 1つでもあるか。
    fn has_click_target(
        click_state: &Rc<RefCell<ClickState>>,
        cols: u16,
        rows: u16,
        action_id: u16,
    ) -> bool {
        let cs = click_state.borrow();
        (0..rows).any(|row| (0..cols).any(|col| cs.hit_test(col, row) == Some(action_id)))
    }

    fn seated_state() -> PachinkoState {
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        assert!(logic::sit_at(&mut state, 0), "着席できるホールを用意できていない");
        state
    }

    #[test]
    fn hall_renders_without_panicking_narrow_and_wide() {
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
    }

    #[test]
    fn playing_renders_without_panicking_narrow_and_wide() {
        let state = seated_state();
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
    }

    /// 同じ `Rect` へ盤面 (遊技中) とホールのプレビューをそれぞれ描き、
    /// 記号だけを取り出す。色と枠の見出しは画面ごとに違ってよいので、
    /// 比較するのは「どこに何が描かれたか」だけにする。
    fn board_symbols(area_w: u16, area_h: u16, hall_preview: bool) -> Vec<Vec<String>> {
        let state = seated_state();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = area_w;
        cs.borrow_mut().terminal_rows = area_h;
        let mut terminal = Terminal::new(TestBackend::new(area_w, area_h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                if hall_preview {
                    render_hall_preview(&state, f, area);
                } else {
                    render_board(&state, f, area, &cs);
                }
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..area_h)
            .map(|y| {
                (0..area_w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_hall_preview_draws_the_same_board_as_the_seat() {
        // 座る前に読んだ釘と座った後の盤面がずれると、釘読みという判断軸
        // そのものが嘘になる。両者が同じ絵を描いていることを記号単位で
        // 突き合わせて固定する。
        let (w, h) = (34u16, 28u16);
        let seat = board_symbols(w, h, false);
        let preview = board_symbols(w, h, true);
        // 枠の見出し (0行目) と、盤面にだけ重なる案内バナー (1行目) は
        // 画面ごとに違ってよい。
        for y in 2..h as usize {
            assert_eq!(
                seat[y], preview[y],
                "{y} 行目でホールのプレビューと着席後の盤面が食い違っている"
            );
        }
    }

    #[test]
    fn the_hall_preview_shows_the_nails_of_the_selected_machine() {
        // 選ぶ台を変えても同じ絵しか出ないなら、並んだ台を釘で見分けられない。
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        // 開きの上限と下限の台を並べ、最も差が付く2台で見比べる。
        let (lo, hi) = logic::NAIL_SPREAD_RANGE;
        for (index, spread) in [(0usize, lo), (1usize, hi)] {
            let mut seed = 12_345 + index as u32;
            state.machines[index].nail_spread = spread;
            state.machines[index].nails =
                logic::generate_nails(&mut seed, spread, state.machines[index].rail_bias);
        }

        let render_preview = |state: &PachinkoState| -> Vec<String> {
            let (w, h) = (30u16, 26u16);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| render_hall_preview(state, f, f.area()))
                .unwrap();
            let buf = terminal.backend().buffer();
            (0..h)
                .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
                .collect()
        };

        state.hall_cursor = 0;
        let narrow_nails = render_preview(&state);
        state.hall_cursor = 1;
        let wide_nails = render_preview(&state);
        assert_ne!(
            narrow_nails, wide_nails,
            "ヘソ釘の開きが違う台を選んでもプレビューの見た目が変わっていない"
        );
    }

    #[test]
    fn the_hall_preview_registers_no_click_target() {
        // プレビューは純粋な装飾。ここにタップ判定を置くと、台を選ぶ操作が
        // リストとプレビューの2箇所に分かれる。
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        let (w, h) = (100u16, 40u16);
        let cs = render_to_test_backend_with_click_state(&state, w, h);
        assert!(
            !has_click_target(&cs, w, h, actions::BOARD_TAP),
            "ホールに盤面のタップ対象が登録されている"
        );
    }

    #[test]
    fn the_hall_list_shows_the_nail_spread_as_a_gap() {
        // ナロー幅ではプレビューを出せないので、ヘソの開きはこの行だけが
        // 伝える。消えると台を釘で見分ける手がかりが無くなる。
        let (lo, hi) = logic::NAIL_SPREAD_RANGE;
        let tight = nail_spread_gauge(lo);
        let loose = nail_spread_gauge(hi);
        assert!(
            tight.chars().count() < loose.chars().count(),
            "開いた台のヘソが渋い台より広く見えていない: {tight} / {loose}"
        );
        assert!(!tight.chars().any(|c| c.is_ascii_digit()), "開きが数値で出ている");

        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        state.machines[0].nail_spread = hi;
        let (w, h) = (40u16, 30u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();
        let drawn = (0..h).any(|y| {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            row.contains(&loose)
        });
        assert!(drawn, "ナローの台リストにヘソの開きが描かれていない");
    }

    #[test]
    fn hall_registers_a_click_target_for_every_machine() {
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        let (w, h) = (100u16, 40u16);
        let cs = render_to_test_backend_with_click_state(&state, w, h);
        for index in 0..state.machines.len() {
            assert!(
                has_click_target(&cs, w, h, actions::machine_select_id(index)),
                "{index} 番目の台に着席するクリック対象が登録されていない"
            );
        }
    }

    #[test]
    fn playing_registers_board_tap_and_fire_toggle() {
        let state = seated_state();
        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            let cs = render_to_test_backend_with_click_state(&state, w, h);
            assert!(
                has_click_target(&cs, w, h, actions::BOARD_TAP),
                "{w}x{h}: 盤面のタップ対象が登録されていない"
            );
            assert!(
                has_click_target(&cs, w, h, actions::TOGGLE_FIRE),
                "{w}x{h}: 打ち出しボタンのタップ対象が登録されていない"
            );
        }
    }

    #[test]
    fn playing_registers_a_target_for_every_info_tab() {
        let state = seated_state();
        let (w, h) = (100u16, 40u16);
        let cs = render_to_test_backend_with_click_state(&state, w, h);
        for tab in InfoTab::ALL {
            assert!(
                has_click_target(&cs, w, h, tab_action(tab)),
                "{} タブの切り替えがタップできない",
                tab.label()
            );
        }
    }

    #[test]
    fn every_info_tab_renders_without_panicking() {
        let mut state = seated_state();
        state.history.push(crate::games::pachinko::state::HistoryEntry {
            rounds: 16,
            kakuhen: true,
            spins_before: 42,
        });
        for tab in InfoTab::ALL {
            state.tab = tab;
            render_to_test_backend_with_click_state(&state, 100, 40);
            render_to_test_backend_with_click_state(&state, 40, 30);
        }
    }

    #[test]
    fn jackpot_reach_and_balls_render_without_panicking() {
        let mut state = seated_state();
        state.mode = Mode::Jackpot(crate::games::pachinko::state::JackpotState {
            round: 3,
            total_rounds: 16,
            count: 4,
            ticks_left: 120,
            kakuhen: true,
            payout: 0,
        });
        state.digit = Digit::Spinning {
            ticks_left: 20,
            outcome: SpinOutcome {
                hit: true,
                rounds: 16,
                kakuhen: true,
                reach: ReachKind::Super,
                rank: PendingRank::White,
                stop: StopStyle::Plain,
                confirmed: false,
                assisted: false,
                reels: [7, 7, 7],
            },
        };
        state.balls.push(Ball {
            x: 30.0,
            y: 40.0,
            vx: -0.4,
            vy: 0.6,
            hit_glow: 3,
        });
        state.balls.push(Ball {
            x: 12.0,
            y: 70.0,
            vx: 0.2,
            vy: 0.9,
            hit_glow: 0,
        });
        state.pending.push(Pending::new(SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach: ReachKind::None,
            rank: PendingRank::White,
            stop: StopStyle::Plain,
            confirmed: false,
            assisted: false,
            reels: [1, 2, 3],
        }));
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
    }

    #[test]
    fn empty_hall_renders_without_panicking_in_both_phases() {
        // ホール生成前 (`machines` が空) でも描画に入ることがある。台を
        // 前提に添え字を引くと、その1フレームで画面ごと落ちる。
        let mut state = PachinkoState::new();
        assert!(state.machines.is_empty());
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
        state.phase = Phase::Playing;
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
    }

    #[test]
    fn tiny_area_renders_without_panicking() {
        let state = seated_state();
        render_to_test_backend_with_click_state(&state, 8, 4);
        render_to_test_backend_with_click_state(&state, 20, 8);
    }

    #[test]
    fn reels_stop_left_then_right_so_a_reach_reads_before_the_last_stop() {
        // 左右が先に揃うことでリーチだと分かる。中が止まる前に左右が揃って
        // いないと、リーチ演出が「結果と同時にしか分からない」ものになる。
        let outcome = SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach: ReachKind::Super,
            rank: PendingRank::White,
            stop: StopStyle::Plain,
            confirmed: false,
            assisted: false,
            reels: [7, 3, 7],
        };
        let total = ReachKind::Super.spin_ticks();
        let just_started = reel_faces(
            &Digit::Spinning {
                ticks_left: total,
                outcome,
            },
            INITIAL_REELS,
        );
        let after_both_stops = reel_faces(
            &Digit::Spinning {
                ticks_left: total - RIGHT_STOP_TICKS,
                outcome,
            },
            INITIAL_REELS,
        );
        assert_eq!(
            (after_both_stops[0], after_both_stops[2]),
            ('7', '7'),
            "左右が停止後も出目に揃っていない: {after_both_stops:?}"
        );
        assert_ne!(
            just_started, after_both_stops,
            "回転中と停止後で表示が変わらず、回っているように見えない"
        );
    }

    #[test]
    fn every_reel_stops_on_the_outcome_before_the_spin_ends() {
        // 中リールが止まらないと、当たってもゾロ目が出ず、ハズレでもリーチ目に
        // ならない。抽選が決めた出目が画面に一度も現れないまま消える。
        for reach in ReachKind::ALL {
            let outcome = SpinOutcome {
                hit: false,
                rounds: 0,
                kakuhen: false,
                reach,
                rank: PendingRank::White,
                stop: StopStyle::Plain,
                confirmed: false,
                assisted: false,
                reels: [7, 3, 7],
            };
            // 回転中に render が見る最後の tick。`logic::advance_digit` は
            // 0 まで減った時点で `Digit::Idle` へ移すため、1 が下限。
            let faces = reel_faces(&Digit::Spinning { ticks_left: 1, outcome }, INITIAL_REELS);
            assert_eq!(
                faces,
                ['7', '3', '7'],
                "{} の停止間際に出目が揃っていない",
                reach.label()
            );
        }
    }

    #[test]
    fn idle_digit_shows_the_last_stopped_reels() {
        // 停止中の液晶が伏せ字だと、大当たり直後にゾロ目が残らず、
        // 台の当たり状況を液晶から読めなくなる。
        assert_eq!(reel_faces(&Digit::Idle, [7, 7, 7]), ['7', '7', '7']);
        assert_eq!(reel_faces(&Digit::Idle, INITIAL_REELS), ['1', '2', '3']);
    }

    #[test]
    fn a_hit_draws_matching_reels_and_a_reach_draws_only_the_middle_apart() {
        let spinning = |reach: ReachKind, hit: bool, reels: [u8; 3]| Digit::Spinning {
            ticks_left: 1,
            outcome: SpinOutcome {
                hit,
                rounds: if hit { 8 } else { 0 },
                kakuhen: false,
                reach,
                rank: PendingRank::White,
                stop: StopStyle::Plain,
                confirmed: false,
                assisted: false,
                reels,
            },
        };
        for (digit, expected) in [
            (spinning(ReachKind::Super, true, [7, 7, 7]), "7 7 7"),
            (spinning(ReachKind::Super, false, [7, 3, 7]), "7 3 7"),
        ] {
            let mut state = seated_state();
            state.digit = digit;
            let (w, h) = (100u16, 40u16);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();
            let drawn = (0..h).any(|y| {
                let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
                row.contains(expected)
            });
            assert!(drawn, "液晶に {expected} が描かれていない");
        }
    }

    #[test]
    fn the_round_counter_reads_at_most_the_round_limit() {
        // 1 tick の間に複数の玉がアタッカーへ入ると `count` は規定数を超える。
        // 賞球はオーバー入賞分も払うので、表示側だけで丸める。
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 2,
            total_rounds: 4,
            count: ROUND_COUNT + 1,
            ticks_left: 40,
            kakuhen: false,
            payout: 0,
        });
        let capped = format!("{ROUND_COUNT}/{ROUND_COUNT}");
        let overflowed = format!("{}/{ROUND_COUNT}", ROUND_COUNT + 1);
        for text in [board_title(&state), mode_text(&state)] {
            assert!(text.contains(&capped), "規定数まで進んだ表示になっていない: {text}");
            assert!(
                !text.contains(&overflowed),
                "オーバー入賞が規定数を超えた進捗として出ている: {text}"
            );
        }
    }

    #[test]
    fn a_start_pocket_entry_lights_the_pocket_like_a_bounced_ball() {
        let mut state = seated_state();
        assert_eq!(start_pocket_color(&state), Color::Cyan);
        state.start_flash = START_FLASH_TICKS;
        assert_eq!(
            start_pocket_color(&state),
            Color::White,
            "ヘソ入賞の演出トリガが描画に効いていない"
        );
        state.start_flash = 0;
        state.mode = Mode::Jitan { spins_left: 10 };
        assert_eq!(start_pocket_color(&state), Color::LightCyan);
    }

    #[test]
    fn entering_a_reach_recolors_the_board_border() {
        let mut state = seated_state();
        assert_eq!(board_border_color(&state), ACCENT);
        state.reach_flash = REACH_FLASH_TICKS;
        state.reach_flash_kind = ReachKind::Super;
        assert_eq!(
            board_border_color(&state),
            ReachKind::Super.color(),
            "リーチ突入の演出トリガが描画に効いていない"
        );
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 4,
            count: 0,
            ticks_left: 40,
            kakuhen: false,
            payout: 0,
        });
        assert_eq!(
            board_border_color(&state),
            Color::LightRed,
            "大当たり中の枠がリーチの色に上書きされている"
        );
    }

    #[test]
    fn effect_flashes_render_without_panicking() {
        let mut state = seated_state();
        state.start_flash = START_FLASH_TICKS;
        state.reach_flash = REACH_FLASH_TICKS;
        state.reach_flash_kind = ReachKind::Premium;
        render_to_test_backend_with_click_state(&state, 100, 40);
        render_to_test_backend_with_click_state(&state, 40, 30);
    }

    #[test]
    fn no_machine_reads_as_taken_until_the_player_sits_down() {
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        for index in 0..state.machines.len() {
            assert_eq!(
                machine_name_color(&state, index),
                ACCENT,
                "一度も着席していないのに {index} 番目の台が着席中の色になっている"
            );
        }

        assert!(logic::sit_at(&mut state, 1));
        assert!(logic::leave_seat(&mut state));
        assert_eq!(machine_name_color(&state, 1), Color::LightYellow);
        assert_eq!(machine_name_color(&state, 0), ACCENT);
    }

    #[test]
    fn every_machine_spec_reads_as_a_distinct_odds_word() {
        // 分母を出さない代わりに、当たりの軽さの違いはこの1語だけで伝わる
        // 必要がある。2機種が同じ語に潰れると、その差はホールから読めない。
        // 出玉側の語と組にして判定すると、閾値が `MACHINE_SPECS` からずれても
        // 出玉側の違いで通ってしまうので、語ごとに一意性を見る。
        use crate::games::pachinko::state::MACHINE_SPECS;
        let mut seen: Vec<&str> = Vec::new();
        for (name, spec) in MACHINE_SPECS {
            let word = odds_flavor(&spec);
            assert!(
                !seen.contains(&word),
                "{name} が既出の台と同じ語 ({word}) になっている"
            );
            seen.push(word);
        }
    }

    #[test]
    fn every_machine_spec_reads_as_a_distinct_payout_word() {
        use crate::games::pachinko::state::MACHINE_SPECS;
        let mut seen: Vec<&str> = Vec::new();
        for (name, spec) in MACHINE_SPECS {
            let word = payout_flavor(&spec);
            assert!(
                !seen.contains(&word),
                "{name} が既出の台と同じ語 ({word}) になっている"
            );
            seen.push(word);
        }
    }

    #[test]
    fn format_thousands_groups_every_three_digits() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1_000), "1,000");
        assert_eq!(format_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn visit_balance_counts_held_balls_against_the_investment() {
        let mut state = PachinkoState::new();
        state.invested = 1_000;
        state.balls_held = BALL_LOAN_COUNT;
        assert_eq!(visit_balance(&state), 0, "借りた分をそのまま持っていれば収支は 0");
        state.balls_held = BALL_LOAN_COUNT * 2;
        assert_eq!(visit_balance(&state), 1_000);
        assert_eq!(format_signed_yen(visit_balance(&state)), "+1,000円");
    }

    // ── 保留の見せ方 ───────────────────────────────────────────

    /// 抽選結果1件。ランクと止まり方だけを差し替えて使う。
    fn outcome(rank: PendingRank, stop: StopStyle) -> SpinOutcome {
        SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach: ReachKind::None,
            rank,
            stop,
            confirmed: false,
            assisted: false,
            reels: [0, 1, 2],
        }
    }

    /// 描いた画面から記号だけを行ごとに取り出す。
    fn rendered_rows(state: &PachinkoState, w: u16, h: u16) -> Vec<String> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    /// 描いた画面のうち、`symbol` を持つ最初のセルの色。
    fn color_of_symbol(state: &PachinkoState, w: u16, h: u16, symbol: char) -> Option<Style> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();
        for y in 0..h {
            for x in 0..w {
                let cell = &buf[(x, y)];
                if cell.symbol().starts_with(symbol) {
                    return Some(cell.style());
                }
            }
        }
        None
    }

    #[test]
    fn every_pending_rank_draws_its_own_mark_and_color() {
        // ランクが色と形の両方で見分けられないと、保留が情報を運ばない。
        // 信頼度は出さない約束なので、伝える手段はこの2つしかない。
        for rank in PendingRank::ALL {
            let mut state = seated_state();
            state.pending.push(Pending::new(outcome(rank, StopStyle::Plain)));
            state.pending[0].rank = rank;
            for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
                let style = color_of_symbol(&state, w, h, rank.mark());
                let style = style.unwrap_or_else(|| panic!(
                    "{w}x{h}: {} 保留の記号 {} が描かれていない",
                    rank.label(),
                    rank.mark()
                ));
                assert_eq!(
                    style.fg,
                    Some(rank.color()),
                    "{w}x{h}: {} 保留がランクの色で描かれていない",
                    rank.label()
                );
            }
        }
    }

    #[test]
    fn an_empty_pending_slot_reads_apart_from_a_white_one() {
        // 空きスロットと白保留が同じ絵だと、保留が何個溜まっているかすら
        // 読めなくなる。
        let state = seated_state();
        let spans = pending_spans(&state);
        let drawn: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            drawn.chars().filter(|&c| c == PENDING_EMPTY_MARK).count(),
            MAX_PENDING,
            "保留が空なのに空きスロットが {MAX_PENDING} 個並んでいない: {drawn}"
        );
        assert_ne!(PENDING_EMPTY_MARK, PendingRank::White.mark());
        assert!(!drawn.chars().any(|c| c.is_ascii_digit()), "保留に数値が出ている");
    }

    #[test]
    fn the_head_of_the_queue_is_marked_as_the_next_to_spin() {
        let mut state = seated_state();
        for _ in 0..2 {
            state
                .pending
                .push(Pending::new(outcome(PendingRank::White, StopStyle::Plain)));
        }
        let drawn: String = pending_spans(&state)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            drawn.chars().filter(|&c| c == '▶').count(),
            1,
            "次に消化される保留を指す印が1つでない: {drawn}"
        );
    }

    #[test]
    fn a_promoting_pending_swaps_its_foreground_and_background() {
        // 昇格そのものは記号が変わるだけなので見逃される。変わった瞬間を
        // 別の見え方にしないと、段階的に情報を出している意味が消える。
        let mut state = seated_state();
        state
            .pending
            .push(Pending::new(outcome(PendingRank::Red, StopStyle::Plain)));
        state.pending[0].rank = PendingRank::Red;

        let calm: String = pending_spans(&state)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        state.pending[0].promote_flash = PENDING_PROMOTE_FLASH_TICKS;
        let flashing = pending_spans(&state);
        let flashing_text: String = flashing.iter().map(|s| s.content.as_ref()).collect();
        assert_ne!(calm, flashing_text, "昇格した瞬間に描画が変わっていない");
        assert!(flashing_text.contains('↑'), "昇格の向きが出ていない");

        let mark = flashing
            .iter()
            .find(|s| s.content.contains(PendingRank::Red.mark()))
            .expect("ランクの記号が描かれていない");
        assert_eq!(mark.style.bg, Some(PendingRank::Red.color()));
        assert_eq!(mark.style.fg, Some(Color::Black));

        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            assert!(
                rendered_rows(&state, w, h).iter().any(|row| row.contains('↑')),
                "{w}x{h}: 昇格の印が画面に出ていない"
            );
        }
    }

    // ── 図柄の止まり方 ─────────────────────────────────────────

    /// 回転の総 tick 数。追加の回転時間は末尾に確保されている。
    fn total_ticks(outcome: &SpinOutcome) -> u32 {
        outcome.reach.spin_ticks() + outcome.stop.extra_ticks()
    }

    /// 回転開始から停止直前まで、1 tick ずつの見え方を並べる。
    fn spin_frames(outcome: SpinOutcome) -> Vec<ReelView> {
        (1..=total_ticks(&outcome))
            .rev()
            .map(|ticks_left| {
                reel_view(
                    &Digit::Spinning {
                        ticks_left,
                        outcome,
                    },
                    INITIAL_REELS,
                )
            })
            .collect()
    }

    #[test]
    fn every_stop_style_lands_on_the_drawn_outcome() {
        // 抽選が決めた出目が画面に一度も出ないまま消えると、演出から当落を
        // 読むという学習が成り立たない。止まり方を変えても着地は同じになる。
        for stop in StopStyle::ALL {
            for reach in ReachKind::ALL {
                let hit = matches!(stop, StopStyle::Revival);
                let reels = if hit { [7, 7, 7] } else { [7, 3, 7] };
                let mut o = outcome(PendingRank::White, stop);
                o.hit = hit;
                o.reach = reach;
                o.reels = reels;
                let last = spin_frames(o).pop().expect("回転が1 tick も無い");
                assert_eq!(
                    last.faces,
                    reels.map(digit_char),
                    "{} / {} で停止間際に出目が揃っていない",
                    reach.label(),
                    stop.label()
                );
            }
        }
    }

    #[test]
    fn a_slipping_reel_stops_off_the_outcome_before_it_moves() {
        // 一度ハズレ位置で止まって見えなければ、滑りは「ただ長く回っただけ」
        // になる。動く前の目が出目と違うことが演出の前提になる。
        let mut o = outcome(PendingRank::White, StopStyle::Slip);
        o.hit = true;
        o.reach = ReachKind::Super;
        o.reels = [7, 7, 7];
        let frames = spin_frames(o);
        let moved = frames
            .iter()
            .position(|v| v.cue != ' ')
            .expect("滑りの印が一度も出ていない");
        assert_ne!(
            frames[moved - 1].faces[1], '7',
            "滑る直前に既に出目が揃っている"
        );
        assert_eq!(frames[moved].faces[1], '7', "滑った先が出目になっていない");
        assert_eq!(frames[moved].cue, '↓', "滑りの向きが下になっていない");
    }

    #[test]
    fn a_reviving_reel_settles_on_a_miss_before_it_turns_back() {
        // 復活は「完全に停止してハズレが確定した」と思わせる間があって初めて
        // 復活になる。戻る前に出目が揃っていると、ただの遅い停止になる。
        let mut o = outcome(PendingRank::White, StopStyle::Revival);
        o.hit = true;
        o.reach = ReachKind::Super;
        o.reels = [7, 7, 7];
        let frames = spin_frames(o);
        let back = frames
            .iter()
            .position(|v| v.cue == '←')
            .expect("復活の印が一度も出ていない");
        let still: Vec<char> = frames[..back].iter().map(|v| v.faces[1]).collect();
        let held = still.iter().rev().take_while(|&&c| c != '7').count();
        assert!(
            held >= 5,
            "ハズレ位置で静止している時間が短く、復活の間が作れていない: {held} tick"
        );
        assert!(
            frames[back..].iter().all(|v| v.faces[1] == '7'),
            "復活した後に出目が崩れている"
        );
    }

    #[test]
    fn a_near_miss_holds_the_stopped_reels_instead_of_moving_on() {
        // 惜しいハズレは、止まった出目を数 tick 残すことでしか伝わらない。
        let mut o = outcome(PendingRank::Green, StopStyle::NearMiss);
        o.reach = ReachKind::Super;
        o.reels = [7, 3, 7];
        let frames = spin_frames(o);
        let held = frames
            .iter()
            .rev()
            .take_while(|v| v.faces == ['7', '3', '7'])
            .count() as u32;
        assert!(
            held > StopStyle::NearMiss.extra_ticks(),
            "出目が止まってから次へ移るまでの間が確保できていない: {held} tick"
        );
    }

    #[test]
    fn the_middle_reel_slows_down_after_the_sides_match() {
        // 停止に近いコマほど長く画面に留まらないと、1コマの重みが増していく
        // 感覚が出ない。
        let mut o = outcome(PendingRank::White, StopStyle::Plain);
        o.hit = true;
        o.reach = ReachKind::Super;
        o.reels = [7, 7, 7];
        let frames = spin_frames(o);
        // テンパイ (左右が揃った) 以降だけを見る。
        let tempai = frames
            .iter()
            .position(|v| v.faces[0] == '7' && v.faces[2] == '7')
            .expect("左右が揃う瞬間が無い");
        // 同じコマが続いた長さを順に数える。
        let mut holds: Vec<u32> = Vec::new();
        let mut current = '\0';
        for view in &frames[tempai..] {
            if view.faces[1] == current {
                *holds.last_mut().expect("最初のコマが積まれていない") += 1;
            } else {
                current = view.faces[1];
                holds.push(1);
            }
        }
        assert!(holds.len() >= 3, "コマが数えられていない: {holds:?}");
        assert!(
            holds[holds.len() - 1] > holds[0],
            "停止間際のコマが序盤より長く留まっていない: {holds:?}"
        );
    }

    #[test]
    fn every_stop_style_renders_without_panicking() {
        for stop in StopStyle::ALL {
            let mut o = outcome(PendingRank::White, stop);
            o.reach = ReachKind::Super;
            o.reels = [7, 3, 7];
            let total = total_ticks(&o);
            for ticks_left in [total, total / 2, 2, 1] {
                let mut state = seated_state();
                state.digit = Digit::Spinning {
                    ticks_left,
                    outcome: o,
                };
                render_to_test_backend_with_click_state(&state, 100, 40);
                render_to_test_backend_with_click_state(&state, 40, 30);
            }
        }
    }

    // ── 大当たり中の出玉カウンタ ───────────────────────────────

    fn jackpot_state(payout: u32) -> PachinkoState {
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 3,
            total_rounds: 10,
            count: 1,
            ticks_left: 40,
            kakuhen: true,
            payout,
        });
        state.jackpot_payout_shown = payout as f64;
        state
    }

    #[test]
    fn the_payout_counter_follows_the_eased_value() {
        // カウンタが内部値ではなく表示値 (`jackpot_payout_shown`) を読んで
        // いないと、数字は跳ねるだけで「増え続けている」時間が生まれない。
        let low = big_number_columns(&format_thousands(120));
        let high = big_number_columns(&format_thousands(1_480));
        assert_ne!(low, high);

        let mut state = jackpot_state(1_480);
        state.jackpot_payout_shown = 120.0;
        let mid = jackpot_payout_columns(&state);
        assert_eq!(mid, low, "表示値ではなく内部値を描いている");

        state.jackpot_payout_shown = 1_480.0;
        assert_eq!(jackpot_payout_columns(&state), high);
    }

    /// 出玉カウンタが今描いている点灯パターン。
    fn jackpot_payout_columns(state: &PachinkoState) -> Vec<u8> {
        big_number_columns(&format_thousands(
            state.jackpot_payout_shown.round().max(0.0) as u64
        ))
    }

    #[test]
    fn the_payout_counter_is_drawn_wide_and_falls_back_when_narrow() {
        // ドット絵は braille で描くので、文字としては現れない。数字が画面に
        // 「無い」ことではなく、点が描かれていることで確かめる。
        let state = jackpot_state(1_480);
        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            let rows = rendered_rows(&state, w, h);
            let braille = rows
                .iter()
                .any(|row| row.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)));
            let plain = rows.iter().any(|row| row.contains("1,480玉"));
            assert!(
                braille || plain,
                "{w}x{h}: 出玉カウンタが大きな数字としても文字としても出ていない"
            );
            assert!(
                rows.iter().any(|row| row.contains("R 3/10")),
                "{w}x{h}: ラウンドの進行が出ていない"
            );
        }
    }

    #[test]
    fn a_carry_inverts_the_counter_and_a_settled_value_does_not() {
        let mut state = jackpot_state(4_000);
        // 桁が増えた直後 (1000 をまたいだ数 tick 以内)。
        state.jackpot_payout_shown = 1_002.0;
        assert!(payout_carry_flash(&state), "桁上がりの瞬間に反転していない");
        // 同じ桁の中を伸びている間。
        state.jackpot_payout_shown = 2_500.0;
        assert!(!payout_carry_flash(&state), "桁が変わっていないのに反転している");
        // 追いつき終えた後。
        state.jackpot_payout_shown = 4_000.0;
        assert!(!payout_carry_flash(&state), "止まった数字が反転し続けている");

        let columns = big_number_columns("1,002");
        assert_ne!(
            big_number_points(&columns, false, 7.5),
            big_number_points(&columns, true, 7.5),
            "反転しても同じ絵になっている"
        );
    }

    #[test]
    fn the_round_dots_fill_as_balls_enter_the_attacker() {
        let empty = round_dots(JackpotState {
            round: 1,
            total_rounds: 4,
            count: 0,
            ticks_left: 40,
            kakuhen: false,
            payout: 0,
        });
        let full = round_dots(JackpotState {
            round: 1,
            total_rounds: 4,
            count: ROUND_COUNT,
            ticks_left: 40,
            kakuhen: false,
            payout: 0,
        });
        assert_eq!(empty.chars().count(), ROUND_COUNT as usize);
        assert_eq!(empty.chars().filter(|&c| c == '●').count(), 0);
        assert_eq!(full.chars().filter(|&c| c == '●').count(), ROUND_COUNT as usize);
    }

    #[test]
    fn the_summary_of_the_last_jackpot_shows_after_it_ends() {
        let mut state = seated_state();
        state.last_jackpot_payout = 1_536;
        state.chain = 3;
        state.mode = Mode::Kakuhen { spins_left: 0 };
        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            let rows = rendered_rows(&state, w, h);
            assert!(
                rows.iter().any(|row| row.contains("1,536玉")),
                "{w}x{h}: 大当たりの決算が出ていない"
            );
        }
    }

    // ── 確定シグナルと画面効果 ─────────────────────────────────

    fn confirmed_state() -> PachinkoState {
        let mut state = seated_state();
        let mut o = outcome(PendingRank::Rainbow, StopStyle::Plain);
        o.hit = true;
        o.rounds = 16;
        o.reach = ReachKind::Premium;
        o.confirmed = true;
        o.reels = [7, 7, 7];
        state.digit = Digit::Spinning {
            ticks_left: o.reach.spin_ticks() / 2,
            outcome: o,
        };
        state
    }

    /// 盤面の枠が今どう塗られているか。虹は色そのものが情報なので、
    /// 記号ではなく色の並びで見る。
    fn border_colors(state: &PachinkoState, w: u16, h: u16) -> Vec<Option<Color>> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();
        (0..w).map(|x| buf[(x, 0)].style().fg).collect()
    }

    #[test]
    fn the_confirmed_signal_runs_a_rainbow_around_the_board_border() {
        let mut plain = seated_state();
        let mut o = outcome(PendingRank::White, StopStyle::Plain);
        o.reach = ReachKind::Super;
        plain.digit = Digit::Spinning {
            ticks_left: 10,
            outcome: o,
        };
        let confirmed = confirmed_state();
        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            let calm = border_colors(&plain, w, h);
            let hot = border_colors(&confirmed, w, h);
            assert_ne!(calm, hot, "{w}x{h}: 確定シグナルで枠の描画が変わっていない");
            let hues = hot
                .iter()
                .filter(|c| matches!(c, Some(Color::Rgb(_, _, _))))
                .count();
            assert!(hues > 2, "{w}x{h}: 枠に色相が並んでいない");
        }
    }

    #[test]
    fn the_rainbow_travels_as_the_spin_goes_on() {
        // 位相が動かないと「1周流れる」ではなくただの色付きの枠になる。
        let mut state = confirmed_state();
        let first = border_colors(&state, 100, 40);
        if let Digit::Spinning { ticks_left, .. } = &mut state.digit {
            *ticks_left -= 1;
        }
        assert_ne!(first, border_colors(&state, 100, 40), "虹が止まっている");
    }

    #[test]
    fn only_a_hot_moment_shakes_the_board() {
        // 揺れは「他と違うことが起きた」という合図なので、通常時に出ると
        // 合図としての意味が消える。
        let mut state = seated_state();
        assert!(!board_shake(&state), "通常時に揺れている");

        state
            .pending
            .push(Pending::new(outcome(PendingRank::Blue, StopStyle::Plain)));
        state.pending[0].rank = PendingRank::Blue;
        state.pending[0].promote_flash = PENDING_PROMOTE_FLASH_TICKS;
        assert!(!board_shake(&state), "赤に届かない保留で揺れている");

        state.pending[0].rank = PendingRank::Red;
        assert!(board_shake(&state), "赤保留が出た瞬間に揺れていない");
        state.pending[0].promote_flash = 1;
        assert!(!board_shake(&state), "揺れが最初の数 tick で収まっていない");

        let confirmed = confirmed_state();
        assert!(!board_shake(&confirmed), "確定シグナルの揺れが回転中ずっと続いている");
    }

    #[test]
    fn a_shaken_board_draws_one_cell_across() {
        let mut state = seated_state();
        state
            .pending
            .push(Pending::new(outcome(PendingRank::Gold, StopStyle::Plain)));
        state.pending[0].rank = PendingRank::Gold;
        for (w, h) in [(100u16, 40u16), (40u16, 30u16)] {
            let calm = rendered_rows(&state, w, h);
            state.pending[0].promote_flash = PENDING_PROMOTE_FLASH_TICKS;
            let shaken = rendered_rows(&state, w, h);
            state.pending[0].promote_flash = 0;
            assert_ne!(calm, shaken, "{w}x{h}: 揺れが描画に出ていない");
        }
    }

    #[test]
    fn the_pending_row_never_shows_a_reliability_number() {
        // 信頼度を数値で配ると、赤が熱いことをプレイヤー自身が見つける
        // 余地が消える。
        for rank in PendingRank::ALL {
            let mut state = seated_state();
            state.pending.push(Pending::new(outcome(rank, StopStyle::Plain)));
            state.pending[0].rank = rank;
            let drawn: String = pending_spans(&state)
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            assert!(
                !drawn.chars().any(|c| c.is_ascii_digit() || c == '%'),
                "{} 保留に信頼度らしき値が出ている: {drawn}",
                rank.label()
            );
        }
    }
    #[test]
    #[ignore = "目視用"]
    fn dump_screens() {
        let dump = |state: &PachinkoState, w: u16, h: u16, title: &str| {
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render(state, f, f.area(), &cs)).unwrap();
            let buf = t.backend().buffer();
            eprintln!("=== {title} ({w}x{h}) ===");
            for y in 0..h {
                let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
                eprintln!("|{row}|");
            }
        };

        let mut state = seated_state();
        state.balls_held = 400;
        state.pending.push(Pending::new(outcome(PendingRank::White, StopStyle::Plain)));
        state.pending.push(Pending::new(outcome(PendingRank::Red, StopStyle::Plain)));
        state.pending.push(Pending::new(outcome(PendingRank::Gold, StopStyle::Plain)));
        state.pending[0].rank = PendingRank::White;
        state.pending[1].rank = PendingRank::Red;
        state.pending[1].promote_flash = PENDING_PROMOTE_FLASH_TICKS;
        state.pending[2].rank = PendingRank::Gold;
        dump(&state, 100, 40, "保留ワイド");
        dump(&state, 40, 30, "保留ナロー");

        let mut j = jackpot_state(1480);
        j.jackpot_payout_shown = 1237.4;
        dump(&j, 100, 40, "大当たりワイド");
        dump(&j, 40, 30, "大当たりナロー");

        let mut small = jackpot_state(84);
        small.jackpot_payout_shown = 84.0;
        dump(&small, 100, 40, "大当たり2桁");

        let c = confirmed_state();
        dump(&c, 40, 30, "確定ナロー");

        let mut slip = seated_state();
        let mut o = outcome(PendingRank::White, StopStyle::Slip);
        o.hit = true; o.reach = ReachKind::Super; o.reels = [7,7,7];
        for tl in (1..=2).rev() {
            slip.digit = Digit::Spinning { ticks_left: tl, outcome: o };
            dump(&slip, 40, 30, &format!("滑り t={tl}"));
        }
    }
}
