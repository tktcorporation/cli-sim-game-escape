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
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
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
    Digit, InfoTab, JackpotState, Machine, MachineSpec, Mode, PachinkoState, Phase,
    ATTACKER_HALF_W, ATTACKER_X, ATTACKER_Y, BALL_LOAN_COUNT, BALL_LOAN_YEN, BALL_R, BOARD_H,
    BOARD_W, LAUNCH_X, LAUNCH_Y, MAX_PENDING, NAIL_R, ROUND_COUNT, SIDE_POCKET_HALF_W,
    SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y, START_POCKET_X, START_POCKET_Y,
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
    render_hall_list(state, f, chunks[1], borders, click_state);
    render_hall_footer(state, f, chunks[2], narrow, click_state);
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

fn render_hall_list(
    state: &PachinkoState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
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
        // 2行とも同じ台へ結び付ける。台名と実測値のどちらを触っても座れる
        // 方が、指が行を跨いだ時に「押したのに何も起きない」を避けられる。
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" [{key}] "), Style::default().fg(Color::DarkGray)),
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
    {
        let mut cs = click_state.borrow_mut();
        render_key_row(f, rows[0], &mut cs, &[(buy_label, actions::BUY_BALLS)]);
    }

    if rows[1].height > 0 {
        let hint = if state.machines.is_empty() {
            "台を待っている"
        } else {
            "台をタップして着席する"
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

    // 打ち出し口から天井、そしてアウト口までの導線。玉がどこから入って
    // どこへ消えるのかを、玉が飛んでいない時にも読めるようにする。
    let guide_lines: Vec<(f64, f64, f64, f64)> = vec![
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
            board_to_canvas_y(BOARD_H - 1.0),
            BOARD_W - 2.0,
            board_to_canvas_y(BOARD_H - 1.0),
        ),
    ];

    let nail_pts: Vec<(f64, f64)> = state
        .seated_machine()
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

    let side_pocket_pts: Vec<(f64, f64)> = [SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X]
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
    let pocket_half_w = logic::effective_pocket_half_w(state);
    let start_pocket_pts = canvas_fx::filled_rect_points(
        START_POCKET_X - pocket_half_w,
        board_to_canvas_y(START_POCKET_Y - 0.9),
        START_POCKET_X + pocket_half_w,
        board_to_canvas_y(START_POCKET_Y + 0.9),
        0.35,
    );
    let start_pocket_color = start_pocket_color(state);

    // アタッカーは開放中だけ厚みを持たせ、閉じている間は細い線にする。
    let attacker_open = jackpot.is_some();
    let attacker_half_h = if attacker_open { 1.4 } else { 0.2 };
    let attacker_pts = canvas_fx::filled_rect_points(
        ATTACKER_X - ATTACKER_HALF_W,
        board_to_canvas_y(ATTACKER_Y - attacker_half_h),
        ATTACKER_X + ATTACKER_HALF_W,
        board_to_canvas_y(ATTACKER_Y + attacker_half_h),
        0.4,
    );
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
            for &(x1, y1, x2, y2) in &guide_lines {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: Color::DarkGray,
                });
            }
            if !nail_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &nail_pts,
                    color: Color::Gray,
                });
            }
            if !side_pocket_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &side_pocket_pts,
                    color: Color::Blue,
                });
            }
            if !start_pocket_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &start_pocket_pts,
                    color: start_pocket_color,
                });
            }
            if !attacker_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &attacker_pts,
                    color: attacker_color,
                });
            }
            if !ball_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &ball_pts,
                    color: Color::Gray,
                });
            }
            if !glow_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &glow_pts,
                    color: Color::White,
                });
            }
            if !jackpot_ring.is_empty() {
                ctx.draw(&Points {
                    coords: &jackpot_ring,
                    color: Color::LightRed,
                });
            }
        })
        .block(block);

    Clickable::new(canvas, actions::BOARD_TAP).render(f, area, &mut click_state.borrow_mut());
    render_board_banner(state, f, inner);
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

fn digit_char(value: u8) -> char {
    char::from_digit(value as u32 % 10, 10).unwrap_or('0')
}

/// デジタルの3桁。回転中のリールは擬似的な出目を tick ごとに差し替え、
/// 左→右→中の順に停止させる。停止中は最後に止まった出目 (`last_reels`) を
/// 出し続ける。
fn reel_faces(digit: &Digit, last_reels: [u8; 3]) -> [char; 3] {
    match digit {
        Digit::Idle => last_reels.map(digit_char),
        Digit::Spinning {
            ticks_left,
            outcome,
        } => {
            let elapsed = outcome.reach.spin_ticks().saturating_sub(*ticks_left);
            let rolling = |slot: u32| digit_char((ticks_left.wrapping_mul(3) + slot * 7) as u8);
            let left = if elapsed >= LEFT_STOP_TICKS {
                digit_char(outcome.reels[0])
            } else {
                rolling(0)
            };
            let right = if elapsed >= RIGHT_STOP_TICKS {
                digit_char(outcome.reels[2])
            } else {
                rolling(2)
            };
            let middle = if *ticks_left <= MIDDLE_STOP_REMAINING_TICKS {
                digit_char(outcome.reels[1])
            } else {
                rolling(1)
            };
            [left, middle, right]
        }
    }
}

fn digit_style(digit: &Digit) -> Style {
    match digit {
        Digit::Idle => Style::default().fg(Color::DarkGray),
        Digit::Spinning { outcome, .. } => Style::default()
            .fg(outcome.reach.color())
            .add_modifier(Modifier::BOLD),
    }
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

fn pending_text(state: &PachinkoState) -> String {
    let filled = state.pending.len().min(MAX_PENDING);
    (0..MAX_PENDING)
        .map(|i| if i < filled { '●' } else { '○' })
        .collect()
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

    let faces = reel_faces(&state.digit, state.last_reels);
    let style = digit_style(&state.digit);
    cl.push(Line::from(Span::styled(
        " ┌───────┐",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(vec![
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} {} {}", faces[0], faces[1], faces[2]),
            style,
        ),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]));
    cl.push(Line::from(Span::styled(
        " └───────┘",
        Style::default().fg(Color::DarkGray),
    )));

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

    cl.push(label_value_line("保留", pending_text(state), ACCENT));
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
        Ball, Digit, ReachKind, SpinOutcome, INITIAL_REELS, REACH_FLASH_TICKS, START_FLASH_TICKS,
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
        });
        state.digit = Digit::Spinning {
            ticks_left: 20,
            outcome: SpinOutcome {
                hit: true,
                rounds: 16,
                kakuhen: true,
                reach: ReachKind::Super,
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
        state.pending.push(SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach: ReachKind::None,
            reels: [1, 2, 3],
        });
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

    #[test]
    fn pending_text_shows_one_mark_per_slot() {
        let mut state = PachinkoState::new();
        assert_eq!(pending_text(&state).chars().count(), MAX_PENDING);
        state.pending.push(SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach: ReachKind::None,
            reels: [0, 1, 2],
        });
        let text = pending_text(&state);
        assert_eq!(text.chars().filter(|&c| c == '●').count(), 1);
        assert_eq!(text.chars().count(), MAX_PENDING);
    }
}
