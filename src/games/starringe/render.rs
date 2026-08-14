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
    CX, FIELD_MARGIN, INNER_RADIUS, SHAKE_MAX_X, SHAKE_MAX_Y, SPAWN_Y, TURRET_NEAR_RADIUS,
    VISIBLE_Y_LO, WORLD_H, WORLD_W,
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

/// 核と砲台の塗り潰しに使う、`fill_step` から詰める割合。
///
/// `fill_step` は「同じドットを塗り直さない」ところで間隔を止めるので、円の縁は
/// 標本の位相しだいでドットを取りこぼす。モバイル幅の核は 90 点ほどしか無く、
/// 縁の欠けがそのまま輪郭の粗さになって塊に見えない。核と砲台は画面のどこを
/// 見るかを決める 2 つなので、この 2 つだけ縁が丸く出るところまで詰める。
/// どの行でも点が途切れないことは
/// `the_core_and_the_turret_are_drawn_as_solid_blobs` が実描画で見張る。
const SOLID_FILL_REFINE: f64 = 0.7;

/// 核と砲台の塗り潰しに渡すサンプリング間隔。
fn solid_fill_step(inner: Rect) -> f64 {
    fill_step(inner) * SOLID_FILL_REFINE
}

/// 円を塗り潰した点列。間隔は `fill_step` が領域の解像度から決めるが、半径より
/// 粗い間隔を渡すと `canvas_fx::filled_ellipse_points` は 1 点も返さない。
/// 砲台や小さい弾のように半径がドット間隔を下回る円が消えないよう、間隔は
/// 半径で頭打ちにする。
fn filled_circle(cx: f64, cy: f64, r: f64, step: f64) -> Vec<(f64, f64)> {
    canvas_fx::filled_ellipse_points(cx, cy, r, r, step.min(r))
}

// ステージの描画物は「核 → 砲台 → 環」の順に大きく明るく描く。守る拠点と
// 自分の武装が、降ってくる鉱石や軌道の装飾と同じ粒度で並ぶと、画面のどこを
// 見ればよいかが決まらない。以下の半径はその序列を作る値。

/// 核の膨らみが取りうる最大倍率。層のぶんも合図のぶんもここで頭打ちにする —
/// 層に上限が無い (`Layer::title` の「無限輪」) ため、青天井のまま掛けると
/// いずれ核が Canvas の下端を割る。
const CORE_MAX_SCALE: f64 = 1.55;

/// 核本体の描画半径。層でも合図でも動かさない。
///
/// 鉱石が消える到達半径 (`INNER_RADIUS`) を包む大きさに取る。到達半径より
/// 小さく描くと、鉱石が核の縁へ触れる手前で消えて吸い込まれたように見えない。
/// 最大の鉱石 (`OreKind::Nova`) より一回り大きいので、拠点と的は色より先に
/// 大きさで読み分けられる。
///
/// 固定にするのは、核本体が広げてよい幅が点グリッドの分解能より狭いから。
/// 核本体は「触れた鉱石が消える」判定そのものを見せる図形なので、環の最下点に
/// いる手前側の砲台 (`StarRingState::ring_radii` の縦半径) と、まだ到達半径の
/// 外にいる最小の鉱石 (`OreKind::Dust`、半径 3.0) のどちらも覆わないところまで
/// しか塗れない。そこまでの 1〜2 ワールド単位を合図ごとの段階へ割っても、1 段が
/// `fill_step` のドット間隔 (モバイルで約 1.5、デスクトップで約 0.8) を下回り、
/// 描かれる点は変わらないまま判定との対応だけが緩む。核が合図を返す役は、
/// 色 (`core_color`) と面を塗らない暈 (`CORE_HALO_RADIUS`) が持つ。
const CORE_RADIUS: f64 = INNER_RADIUS + 1.0;

/// 核を囲む暈の基準半径。核本体との差が、核がまとう光の厚みになる。
///
/// 輪郭だけの点列なので、膨らみの倍率はこちらへ掛ける。上限は「`CORE_MAX_SCALE`
/// 倍まで膨らんでも、下端が `VISIBLE_Y_LO` (画面シェイクで下がっても Canvas に
/// 残る高さ) を割らない」ことで決まる。核は `CORE_Y` に座っているので、下へ
/// 使える余裕はその値しかない。
const CORE_HALO_RADIUS: f64 = 9.0;

const _: () = assert!(
    CORE_Y - CORE_HALO_RADIUS * CORE_MAX_SCALE >= VISIBLE_Y_LO,
    "膨らみしきった核の暈が Canvas の下端を割る"
);
const _: () = assert!(
    INNER_RADIUS <= CORE_RADIUS && CORE_RADIUS <= CORE_HALO_RADIUS,
    "核本体が到達半径を包まないか、暈より大きくて暈が輪郭に見えない"
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

/// 層が 1 つ上がるごとに核が増す常時の膨らみ。
const CORE_LAYER_SWELL_PER_LAYER: f64 = 0.04;

/// 常時の膨らみの上限。層に上限が無いので、ここで止めないと合図を乗せる余地が
/// 無くなる。
const CORE_LAYER_SWELL_MAX: f64 = 0.20;

/// 核脈動が波を撃った拍に上乗せする膨らみ。
const CORE_CUE_PULSE: f64 = 0.06;
/// 開放待ちの点滅で上乗せする膨らみ。
const CORE_CUE_UNLOCK_READY: f64 = 0.10;
/// 撃破条件を満たした瞬間に上乗せする膨らみ。
const CORE_CUE_LAYER_READY: f64 = 0.20;
/// 層を開放した瞬間に上乗せする膨らみ。合図の中でいちばん大きい。
const CORE_CUE_LAYER_OPEN: f64 = 0.35;

const _: () = assert!(
    0.0 < CORE_CUE_PULSE
        && CORE_CUE_PULSE < CORE_CUE_UNLOCK_READY
        && CORE_CUE_UNLOCK_READY < CORE_CUE_LAYER_READY
        && CORE_CUE_LAYER_READY < CORE_CUE_LAYER_OPEN,
    "合図の重さと膨らみの大きさが対応していない"
);
const _: () = assert!(
    1.0 + CORE_LAYER_SWELL_MAX + CORE_CUE_LAYER_OPEN <= CORE_MAX_SCALE,
    "層を重ねきった核では、いちばん大きい合図が上限で削られる"
);

/// 層の深さだけで決まる、核の常時の膨らみ。
fn core_layer_swell(state: &StarRingState) -> f64 {
    ((state.layer().saturating_sub(1)) as f64 * CORE_LAYER_SWELL_PER_LAYER)
        .min(CORE_LAYER_SWELL_MAX)
}

/// 層開放・開放待ち・核脈動の合図が上乗せする膨らみ。合図が無ければ 0。
///
/// 重い合図から順に見て、最初に当たった 1 つだけを返す。分岐の順は定数の
/// 大きさの順と揃える——軽い合図を先に見ると、重い合図が出ている間だけ核が
/// 小さく描かれる。
///
/// 重さを決めるのは、プレイヤーに次の一手を促す度合い。層開放と撃破条件の
/// 達成は進行が動いた報せ、開放待ちの点滅は `[!]` を押させるための催促で、
/// どれも操作へ結び付く。いちばん軽い核脈動の拍だけは押させるものが無い
/// 装飾なので、催促を覆い隠さないところに置く。
fn core_cue_swell(state: &StarRingState) -> f64 {
    if state.layer_flash_ticks > 0 {
        CORE_CUE_LAYER_OPEN
    } else if state.layer_ready_flash_ticks > 0 {
        CORE_CUE_LAYER_READY
    } else if can_unlock_next_layer(state) && state.elapsed_ticks % 20 < 10 {
        CORE_CUE_UNLOCK_READY
    } else if state.core_pulse_flash_ticks > 0 {
        CORE_CUE_PULSE
    } else {
        0.0
    }
}

/// 核の膨らみ倍率。層の深さぶんの常時の膨らみへ、合図の膨らみを上乗せする。
///
/// 常時と合図を 1 本の倍率の排他分岐にすると、層が伸びたぶんだけ常時側が
/// 大きくなり、いずれ合図側を追い越して「合図が出た瞬間に核が縮む」。上乗せ
/// なら、どの層でも合図は必ずその層の常時より大きい。
fn core_scale(state: &StarRingState) -> f64 {
    (1.0 + core_layer_swell(state) + core_cue_swell(state)).min(CORE_MAX_SCALE)
}

/// 核を囲む暈の描画半径。核の膨らみを引き受けるのはこちらだけ。
fn core_halo_radius(state: &StarRingState) -> f64 {
    CORE_HALO_RADIUS * core_scale(state)
}

// ステージへ同時に出る色は、`ctx.draw` の呼び出しへ直に書かず、この節の定数と
// 関数だけに持たせる。呼び出し側へ散らすとステージに出る色をコードから列挙
// できなくなり、同色で潰れ合う組み合わせを検査にかけられない。列挙が閉じて
// いることは `stage_colours_live_in_the_palette` が見張る。

/// 手前側の砲台の色。
///
/// 砲台は塗り潰した円で、描画半径 (`turret_radius`) は光線弾
/// (`logic::RAY_PROJECTILE_RADIUS`) とほとんど変わらない。しかも弾は砲台の
/// 位置から出るので、色まで揃うと環の周りで弾と砲台の区別が付かない。鉱石
/// (`ore_color`) とも弾 (`weapon_color`) とも重ならない色を砲台だけに与える。
const TURRET_NEAR_COLOR: Color = Color::LightGreen;

/// 奥側の砲台の色。手前側と同系の暗い色で、同じ物が遠くにあると読ませる。
const TURRET_FAR_COLOR: Color = Color::Green;

/// 砲台環の色。
///
/// 砲台がどの経路を通るかを示す線なので、背景の星 (`star_color`) より明るい側に
/// 置く。星より暗くすると、控えめを通り越して経路そのものが背景へ沈む。砲台
/// (`TURRET_NEAR_COLOR`) より明るくしないのは、通り道が通る物より目立たない
/// ため。
const ORBIT_COLOR: Color = Color::Indexed(246);

/// フィールド左右の壁の色。端があると分かる以上に主張させない。
const FIELD_WALL_COLOR: Color = Color::Indexed(236);

/// 核を囲む暈の色。核本体 (`core_color`) のどの色とも重ならない無彩色で、
/// 本体の輪郭として読めるようにする。
const CORE_HALO_COLOR: Color = Color::DarkGray;

/// 着弾の火花の色。
const SPARK_COLOR: Color = Color::White;
/// 砕けた鉱石の粉の色。
const DUST_PARTICLE_COLOR: Color = Color::Gray;
/// 砕けた鉱石の破片の色。
const SHARD_PARTICLE_COLOR: Color = Color::LightMagenta;
/// 燃え残りの色。
const EMBER_PARTICLE_COLOR: Color = Color::LightRed;

/// ステージ枠の見出しの文字色。枠の上に載る文字で、点描の描画物ではないため
/// 同色の衝突検査 (`things_that_share_the_stage_never_share_a_colour`) の対象に
/// しない。
const STAGE_TITLE_COLOR: Color = Color::Yellow;

/// 核脈動の波面の色。
///
/// 波面は `StarRingState::pulse_reach` ぶん広がってフィールドのほぼ全域を毎周期
/// 舐めるので、鉱石・弾・層のどの色とも重ねられない。境界は
/// `things_that_share_the_stage_never_share_a_colour` が持つ。
const PULSE_WAVE_COLOR: Color = Color::LightBlue;

/// 核本体の色。大きさを固定した核が合図を返す手段はこちら。
fn core_color(state: &StarRingState) -> Color {
    if state.layer_flash_ticks > 0 {
        layer_color(state.layer())
    } else if state.layer_ready_flash_ticks > 0 || can_unlock_next_layer(state) {
        Color::LightMagenta
    } else if state.boost_ticks > 0 {
        Color::LightYellow
    } else {
        Color::Yellow
    }
}

/// 背景星の色。層が深いほど色味を持たせ、同じ盤面でも空気が変わって見える
/// ようにする。
///
/// 無彩色の層 (第1層・第2層) は核の暈 (`CORE_HALO_COLOR`) より暗い側へ置く。
/// 暈と同じ明るさだと、暈が核のまとう光ではなく「星が密な領域」に見える。
fn star_color(layer: u32) -> Color {
    match layer {
        1 => Color::Indexed(240),
        2 => Color::Indexed(242),
        3 => Color::Indexed(81),
        4 => Color::Indexed(177),
        _ => Color::Indexed(210),
    }
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
            dx: ((((state.elapsed_ticks % 4) as f64) - 1.5) / 1.5) * SHAKE_MAX_X,
            dy: ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * SHAKE_MAX_Y,
        }
    }

    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (x + self.dx, y + self.dy)
    }

    /// 半径 `r` の円を塗り潰した点列。
    fn circle(self, x: f64, y: f64, r: f64, step: f64) -> Vec<(f64, f64)> {
        let (cx, cy) = self.point(x, y);
        filled_circle(cx, cy, r, step)
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

/// 寿命が半分を切った波面の弧長間隔に掛ける倍率。点を間引いて、消えかけの波を
/// 薄く見せる。
const PULSE_FADED_ARC_SCALE: f64 = 1.7;

/// 核脈動の波面の点を `out` へ積む。`arc_scale` は点の弧長間隔に掛かるので、
/// 大きいほど波面は疎になる。
///
/// 角度を一定間隔で刻むと、波が広がるほど点の間隔が円周に比例して開き、
/// 上空へ届く頃には点がばらけて波に見えなくなる。弧長で刻んで密度を保ち、
/// フィールドの外へ出た点は捨てる — 核は下端にあるので、円周の下半分は
/// ほとんど画面の外に落ちる。
fn push_pulse_wave_points(
    cx: f64,
    cy: f64,
    radius: f64,
    arc_scale: f64,
    out: &mut Vec<(f64, f64)>,
) {
    if radius <= 0.0 || arc_scale <= 0.0 {
        return;
    }
    let step = (PULSE_ARC_STEP * arc_scale / radius).clamp(0.02, 0.5);
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

/// 横並びで左のタブペインへ渡す割合。残りがステージ。
const WIDE_TAB_PERCENT: u16 = 34;

/// タブペインが枠とスクロール列へ使う桁数。左右の枠が 2 桁、内容が溢れた
/// ときに `ScrollableTab` がスクロール列へ回す 1 桁。
const TAB_CHROME_COLS: u16 = 3;

/// タブ本文が要る内側の桁数。
///
/// タブ本文の行は説明文だけが折り返す。強化行の「ラベル + レベル + コスト」
/// (`│ [A] 弾数  Lv.0  ✦30.0`) と武器の要約 (`  威力1.60  間隔18  斉射×1`)
/// は 1 行に収める前提で組むので、この桁を割ると右端から黙って切り落ちる。
/// 省略記号も出ないので、切れたこと自体が画面から読み取れない。
///
/// いちばん長いのが武器の要約で 26 桁。威力の桁が 1 つ伸びるぶんを足して
/// 27 桁とる。行が実際に収まることは `no_tab_row_is_cut_off_at_any_width` が
/// 幅を掃引して見張る。
const TAB_MIN_INNER_COLS: u16 = 27;

/// 本体を縦積みにするか。
///
/// 判定は「横に並べたらタブペインがタブ本文を収められるか」だけで決める。
/// 横並びのタブペインは画面幅の 3 割ほどしか取らないので、画面のほうが先に
/// 足りなくなることは無く、狭い端末 (`is_narrow_layout` が見る 60 桁) は
/// この判定に含まれる。
///
/// 縦積みならタブ本文もステージも画面幅を丸ごと使えるので、横並びを諦めた
/// 幅帯ではステージの横幅もむしろ広がる。削れるのはステージの高さのほう。
fn is_stacked_layout(width: u16) -> bool {
    wide_tab_width(width) < TAB_MIN_INNER_COLS + TAB_CHROME_COLS
}

/// 横並びにしたときのタブペインの桁数。分割そのものを走らせて測るので、
/// 割合の刻み方が `split_body` とずれない。
fn wide_tab_width(width: u16) -> u16 {
    split_body(Rect::new(0, 0, width, 1), false).1.width
}

/// 本体を (ステージ, タブ内容) へ分ける。
///
/// ステージ側を過半にするのは、鉱石が降ってきて砕ける様子が主役だから。
/// タブ内容が溢れる分は `ScrollableTab` のスクロールで拾う。
fn split_body(body: Rect, is_stacked: bool) -> (Rect, Rect) {
    // 縦積みは上がステージ、横並びは左がタブ内容で右がステージ。
    // 横並びの左パネルは、説明文を折り返して読ませる前提で幅を詰める。
    let (dir, first, second) = if is_stacked {
        (Direction::Vertical, 58, 42)
    } else {
        (Direction::Horizontal, WIDE_TAB_PERCENT, 100 - WIDE_TAB_PERCENT)
    };
    let parts = Layout::default()
        .direction(dir)
        .constraints([
            Constraint::Percentage(first),
            Constraint::Percentage(second),
        ])
        .split(body);
    if is_stacked {
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
    let is_stacked = is_stacked_layout(area.width);
    let borders = if is_stacked {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let [header, tabs, body, footer] = split_frame(area);

    render_header(state, f, header);
    render_tabs(state, f, tabs, click_state);

    let (stage_area, tab_area) = split_body(body, is_stacked);
    render_stage(state, f, stage_area, borders, click_state);
    match state.tab {
        Tab::Armory => render_armory(state, f, tab_area, borders, click_state),
        Tab::Ring => render_ring(state, f, tab_area, borders, click_state),
        Tab::Codex => render_codex(state, f, tab_area, borders, click_state),
    }

    render_footer(state, f, footer);
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

/// ヘッダーに出す層の合図。無ければ空文字。
///
/// 見る順は、達成した瞬間の祝いを先に、居座る状態の告知を後に置く。
/// 撃破条件を満たしている間 `kills_ready_for_next_layer` はずっと真なので、
/// これを `layer_ready_flash_ticks` より先に見ると、条件を満たした瞬間の
/// 「◆条件達成」が一度も出ないまま「◆星屑不足」に吸われる。ステージ枠の
/// 見出し (`render_stage`) も同じ順で合図を選ぶ。
fn layer_cue(state: &StarRingState) -> &'static str {
    if state.layer_flash_ticks > 0 {
        " ◆層開放"
    } else if can_unlock_next_layer(state) {
        " ◆開放可[!]"
    } else if state.layer_ready_flash_ticks > 0 {
        " ◆条件達成"
    } else if state.kills_ready_for_next_layer() {
        " ◆星屑不足"
    } else {
        ""
    }
}

/// ヘッダー。枠を持たず 2 行で、1 行へ畳めるだけの幅があれば畳んで残り 1 行を
/// 本体との区切り罫にする。畳めない幅では 2 行へ折り返す。
///
/// 畳むかどうかは画面幅だけで決まる。ヘッダーは本体の分割と関係なく画面幅を
/// まるごと使う 1 本の帯なので、本体を縦積みにしたかどうかとは無関係。
fn render_header(state: &StarRingState, f: &mut Frame, area: Rect) {
    let is_narrow = is_narrow_layout(area.width);
    let sps = state.shards_per_sec();
    let layer = state.layer();
    let boost = if state.boost_ticks > 0 {
        " ⚡ブースト"
    } else {
        ""
    };
    let layer_fx = layer_cue(state);

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
                Span::styled(
                    format!("  {} ", w.glyph()),
                    Style::default().fg(weapon_color(w)),
                ),
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
        .title(Span::styled(title, Style::default().fg(STAGE_TITLE_COLOR)));

    let inner = block.inner(area);
    if inner.width < 2 || inner.height < 2 {
        // 点描が成立しない狭さ。枠だけ描き、タップ面だけは維持する。
        Clickable::new(block, TAP_STRIKE).render(f, area, &mut click_state.borrow_mut());
        return;
    }

    let sample_step = fill_step(inner);
    let solid_step = solid_fill_step(inner);
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

    let core_pts = shake.circle(CX, CORE_Y, CORE_RADIUS, solid_step);
    let core_halo = canvas_fx::ring_points(core_x, core_y, core_halo_radius(state), 0.26);

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
        let pts = shake.circle(gx, gy, r, solid_step);
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
        let pts = shake.circle(ore.x, ore.y, ore.radius(), sample_step);
        if let Some(g) = ore_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            ore_groups.push((pts, color));
        }
        let len = ore.radius() * 2.8 + ore.vx.hypot(ore.vy).max(0.01) * 3.0;
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
        // 畳まれた波 (`PulseRing::folded`) の `life` は残寿命ではなく、一息に
        // 削った半径を一度だけ描かせる猶予。残寿命として読むと、その波が
        // 削った範囲を示すただ 1 tick がいちばん薄く描かれてしまう。
        let faded = !ring.folded && ring.life * 2 <= ring.max_life;
        let arc_scale = if faded { PULSE_FADED_ARC_SCALE } else { 1.0 };
        push_pulse_wave_points(core_x, core_y, ring.radius, arc_scale, &mut pulse_ring_pts);
    }

    // 背景星は上から下へ流れ、フィールド内に「降ってくる場」の向きを与える。
    // 壁の外へ散らすと鉱石が動ける範囲が曖昧になるので左右の壁で挟む。
    // 横に狭いステージでは星が団子になって鉱石と紛れるので数を抑える。
    let star_count = if is_narrow_layout(area.width) {
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

    let core_color = core_color(state);
    let star_color = star_color(layer);
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
                    color: FIELD_WALL_COLOR,
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
                    color: ORBIT_COLOR,
                });
            }
            if !gun_far.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_far,
                    color: TURRET_FAR_COLOR,
                });
            }
            // 核の暈は背景側の装飾なので、鉱石や砲台より先に置く。1 セルへ同居
            // した点は後から描いた色を取るので、暈を手前へ回すと、暈をかすめた
            // 鉱石や砲台がそのぶん虫食いになる — 何基あるか・どこに鉱石がいるか
            // という、盤面を読むのに要る手がかりの側が削れる。
            if !core_halo.is_empty() {
                ctx.draw(&Points {
                    coords: &core_halo,
                    color: CORE_HALO_COLOR,
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
            if !gun_near.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_near,
                    color: TURRET_NEAR_COLOR,
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
                    color: DUST_PARTICLE_COLOR,
                });
            }
            if !shards.is_empty() {
                ctx.draw(&Points {
                    coords: &shards,
                    color: SHARD_PARTICLE_COLOR,
                });
            }
            if !embers.is_empty() {
                ctx.draw(&Points {
                    coords: &embers,
                    color: EMBER_PARTICLE_COLOR,
                });
            }
            if !sparks.is_empty() {
                ctx.draw(&Points {
                    coords: &sparks,
                    color: SPARK_COLOR,
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

/// フッター。案内の詳しさは画面幅だけで決まる。ヘッダーと同じく、本体の
/// 分割と関係なく画面幅をまるごと使う 1 本の帯。
fn render_footer(state: &StarRingState, f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer_hint(state.tab, is_narrow_layout(area.width), area.width),
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
    use crate::games::starringe::logic::{
        purchase_ring_upgrade, purchase_weapon_stat, unlock_next_layer, ARC_PROJECTILE_RADIUS,
        NOVA_PROJECTILE_RADIUS, PULSE_PROJECTILE_RADIUS, RAY_PROJECTILE_RADIUS,
        SCATTER_PROJECTILE_RADIUS,
    };
    use crate::games::starringe::state::{
        Layer, Ore, OreMotion, Projectile, PulseRing, RingUpgrade, Tab, LAYER_FLASH_TICKS,
        VISIBLE_X_HI, VISIBLE_X_LO, VISIBLE_Y_HI,
    };

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

    /// 端末サイズ `w`x`h` で `state.tab` の本文を組む `(桁数, 空行を挟むか)`。
    /// レイアウトの分岐も枠とスクロール列の引き方も `render` と同じ手順を通す。
    fn tab_layout_at(state: &StarRingState, w: u16, h: u16) -> (u16, bool) {
        let stacked = is_stacked_layout(w);
        let borders = if stacked {
            Borders::TOP | Borders::BOTTOM
        } else {
            Borders::ALL
        };
        let tab_area = split_body(split_frame(Rect::new(0, 0, w, h))[2], stacked).1;
        let inner = Block::default().borders(borders).inner(tab_area);
        match state.tab {
            // 武装タブだけは先頭 1 行を武器ピッカーへ渡し、残りを本文へ回す。
            Tab::Armory => {
                let body = Rect::new(
                    inner.x,
                    inner.y + 1,
                    inner.width,
                    inner.height.saturating_sub(1),
                );
                fit_tab_layout(body, |cols| armory_sections(state, cols))
            }
            Tab::Ring => fit_tab_layout(inner, |cols| ring_sections(state, cols)),
            Tab::Codex => fit_tab_layout(inner, |cols| codex_sections(state, cols)),
        }
    }

    /// 層と強化がある程度伸びた state。数字は桁が育つ側で行を押し広げるので、
    /// 幅の検査は新規 state ではなくこちらで見る。星屑は購入のたびに戻して、
    /// 資金ではなくレベルの上限だけが伸び方を決めるようにする。
    fn mid_game_state(tab: Tab) -> StarRingState {
        let mut state = StarRingState::new();
        state.tab = tab;
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        state.layer_flash_ticks = 0;
        for weapon in WeaponKind::ALL {
            for stat in WeaponStat::ALL {
                for _ in 0..6 {
                    state.shards = 1e9;
                    purchase_weapon_stat(&mut state, weapon, stat);
                }
            }
        }
        for kind in RingUpgrade::ALL {
            for _ in 0..6 {
                state.shards = 1e9;
                purchase_ring_upgrade(&mut state, kind);
            }
        }
        state.shards = 1e9;
        state
    }

    /// 幅を 1 桁ずつ動かして、どの幅でもタブ本文の行が切り落ちないこと。
    ///
    /// 折り返さない行 (強化行のラベル + レベル + コスト、武器の要約) は、
    /// パネルが狭いと右端から黙って消える。省略記号も出ないので、代表的な
    /// 数サイズを個別に見るだけでは間の幅帯にできた穴に気付けない。
    #[test]
    fn no_tab_row_is_cut_off_at_any_width() {
        for w in 30u16..=120 {
            for h in [22u16, 30, 36] {
                for tab in [Tab::Armory, Tab::Ring, Tab::Codex] {
                    let state = mid_game_state(tab);
                    let (wrap_w, spaced) = tab_layout_at(&state, w, h);
                    let sections = match tab {
                        Tab::Armory => armory_sections(&state, wrap_w),
                        Tab::Ring => ring_sections(&state, wrap_w),
                        Tab::Codex => codex_sections(&state, wrap_w),
                    };
                    for line in build_list(sections, spaced).lines() {
                        assert!(
                            line.width() <= wrap_w as usize,
                            "{w}x{h} {tab:?}: {wrap_w} 桁のパネルに {} 桁の行がある {}",
                            line.width(),
                            line.spans
                                .iter()
                                .map(|s| s.content.as_ref())
                                .collect::<String>()
                        );
                    }
                }
            }
        }
    }

    /// 幅を 1 桁ずつ動かして、どの幅でも武器ピッカーが選択中の武器名を出すこと。
    ///
    /// ピッカーは 5 つのチップを横に割るので、パネルが狭いとチップ 1 つが
    /// 全角 2 文字を置けなくなり、武器名が読めないまま選択だけができる。
    #[test]
    fn weapon_picker_keeps_the_selected_name_at_any_width() {
        for w in 30u16..=120 {
            let state = mid_game_state(Tab::Armory);
            let mut terminal = Terminal::new(TestBackend::new(w, 30)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = 30;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();
            let picker = (0..30)
                .map(|y| row_text(buf, y, w))
                .find(|r| r.contains('▶'))
                .expect("武器ピッカーの行が見つからない");
            assert!(
                picker.contains(state.selected_weapon.label()),
                "{w}桁: 選択中の武器名が読めない {picker}"
            );
        }
    }

    /// ヘッダーの層の合図が、どの枝も実際に出せる state を持つこと。
    ///
    /// 撃破条件を満たしている間 `kills_ready_for_next_layer` は真のままなので、
    /// 「◆条件達成」と「◆星屑不足」の順を取り違えると前者が一度も出ない。
    /// 出ない枝はコードだけ読んでも動いているように見えるので、5 枝すべてを
    /// 実際に踏んで押さえる。
    #[test]
    fn every_layer_cue_has_a_state_that_shows_it() {
        use crate::games::starringe::state::LAYER_READY_FLASH_TICKS;

        let fresh = StarRingState::new();
        assert_eq!(layer_cue(&fresh), "", "条件を満たす前は合図を出さない");

        let ready_state = |flash: u32, shards: f64| {
            let mut state = StarRingState::new();
            state.total_kills = Layer::THRESHOLDS[1];
            state.layer_ready_flash_ticks = flash;
            state.shards = shards;
            state
        };

        // 条件を満たした瞬間の 18 tick。星屑が足りていなくてもここが優先される。
        let just_reached = ready_state(LAYER_READY_FLASH_TICKS, 0.0);
        assert_eq!(layer_cue(&just_reached), " ◆条件達成");

        // 祝いが切れた後、星屑が貯まるまで居座る告知。
        let waiting = ready_state(0, 0.0);
        assert_eq!(layer_cue(&waiting), " ◆星屑不足");

        // 星屑が揃えば、祝いの最中でも開放を促す側が勝つ。
        let affordable = ready_state(LAYER_READY_FLASH_TICKS, 1e9);
        assert_eq!(layer_cue(&affordable), " ◆開放可[!]");

        let mut opened = ready_state(0, 1e9);
        assert!(unlock_next_layer(&mut opened));
        assert_eq!(layer_cue(&opened), " ◆層開放");
    }

    /// 条件達成の合図が、ヘッダーとステージ枠の見出しに同時に出ること。
    /// 片方だけが出る状態は、同じ出来事に 2 つの読み方を与えてしまう。
    #[test]
    fn the_reached_cue_shows_in_the_header_and_the_stage_title() {
        use crate::games::starringe::state::LAYER_READY_FLASH_TICKS;

        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.layer_ready_flash_ticks = LAYER_READY_FLASH_TICKS;
        state.shards = 0.0;

        let (w, h) = (108u16, 36u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = (0..h).map(|y| row_text(buf, y, w)).collect();

        assert!(screen.contains("◆条件達成"), "ヘッダーに合図が出ていない");
        assert!(
            screen.contains("撃破条件達成"),
            "ステージ枠の見出しに合図が出ていない"
        );
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

            // バーの升目数は、実際に行を組んだ桁数から決まる。
            let (wrap_w, _) = tab_layout_at(&state, w, h);

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
                motion: OreMotion::Spiral,
                sway: 0.05,
                age: 10,
            });
        }

        for (w, h) in [(100u16, 30u16), (40, 30), (38, 20)] {
            // レイアウトの分岐は render と同じ判定から引く。閾値が動いたときに
            // 実描画と別の Rect を検査したまま通ることがないようにする。
            let stacked = is_stacked_layout(w);
            let area = Rect::new(0, 0, w, h);
            let (stage, _) = split_body(split_frame(area)[2], stacked);
            let borders = if stacked {
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
        // 縦積みはタブ内容へ回る高さが本体の 4 割ほどしか無い。画面が横に
        // 広くても、行が高さで落ちる事情はモバイルと変わらないので、縦積みへ
        // 倒れる幅帯の上端まで見る。
        for (w, h) in [(38u16, 20u16), (60, 20), (80, 22), (86, 20)] {
            short_viewport_reaches_every_purchase_row(w, h);
        }
    }

    fn short_viewport_reaches_every_purchase_row(w: u16, h: u16) {
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
                split_body(split_frame(Rect::new(0, 0, w, h))[2], is_stacked_layout(w)).1;
            let filled = (tab_area.y..tab_area.y + tab_area.height)
                .filter(|&y| {
                    (tab_area.x..tab_area.x + tab_area.width)
                        .any(|x| buf[(x, y)].symbol().trim() != "")
                })
                .count();
            assert!(
                filled >= 3,
                "{w}x{h} {tab:?}: タブ内側が空欄になっている (中身のある行 {filled})"
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
                    "{w}x{h} {tab:?}: 購入行が出ていないのに送る手段が無い"
                );
                state.scroll_tab(3);
                cs.borrow_mut().targets.clear();
                terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
                reached = has_action(&cs, w, h, wanted);
            }
            assert!(reached, "{w}x{h} {tab:?}: スクロールしても購入行へ届かない");
        }
    }

    /// 上空の鉱石と、画面下部のコアが縦に分離して描かれること。
    ///
    /// 物体の同定は色で行うので、fixture 側で「その色が他の要素へ割り当たって
    /// いない」ことを先に固定する。採掘境界は `layer_color` で描かれるため、層が
    /// 進んだ state に差し替えると同じ色が上空に現れ、この検査は無関係な理由で
    /// 落ちる。
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
        assert_ne!(PULSE_WAVE_COLOR, ore, "核脈動の波面が鉱石と同色");

        for (x, y) in [(20.0, 92.0), (52.0, 84.0), (78.0, 90.0)] {
            state.ores.push(Ore {
                x,
                y,
                vx: 0.0,
                vy: -0.3,
                hp: 5.0,
                kind: OreKind::Crystal,
                motion: OreMotion::Spiral,
                sway: 0.05,
                age: 10,
            });
        }

        for (w, h) in [(100u16, 30u16), (40, 30)] {
            let stacked = is_stacked_layout(w);
            let area = Rect::new(0, 0, w, h);
            let (stage, _) = split_body(split_frame(area)[2], stacked);
            let borders = if stacked {
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

    /// 点の集合を上下左右に繋がった塊へ分ける。塊の数がそのまま「いくつの物と
    /// して見えるか」になる。
    fn blob_groups(pts: &[(usize, usize)]) -> Vec<Vec<(usize, usize)>> {
        use std::collections::HashSet;
        let mut left: HashSet<(usize, usize)> = pts.iter().copied().collect();
        let mut blobs = Vec::new();
        while let Some(&seed) = left.iter().next() {
            left.remove(&seed);
            let mut stack = vec![seed];
            let mut blob = Vec::new();
            while let Some((x, y)) = stack.pop() {
                blob.push((x, y));
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
            blobs.push(blob);
        }
        blobs
    }

    /// 塊ごとの点数。
    fn blob_sizes(pts: &[(usize, usize)]) -> Vec<usize> {
        blob_groups(pts).iter().map(|b| b.len()).collect()
    }

    /// 塊の中で点が途切れている行があれば、その行を返す。
    ///
    /// 塗り潰した円はどの行も 1 続きの点として並ぶ。途切れは、標本が点を
    /// 取りこぼして内側に穴が空いたということ。
    fn row_with_a_gap(blob: &[(usize, usize)]) -> Option<usize> {
        use std::collections::BTreeMap;
        let mut rows: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(x, y) in blob {
            rows.entry(y).or_default().push(x);
        }
        rows.into_iter().find_map(|(y, xs)| {
            let lo = *xs.iter().min().unwrap();
            let hi = *xs.iter().max().unwrap();
            (hi - lo + 1 != xs.len()).then_some(y)
        })
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
    /// 速度を 0 にすると尾 (`proj_trails`) が 1 点へ潰れるので、測るのは円その
    /// ものの占有だけになる。
    ///
    /// 円は飛翔弾として置く。鉱石の半径は種から決まる (`Ore::radius`) のに対し
    /// 弾は個体ごとに半径を持つので、鉱石の大きさも弾の大きさも 1 つの経路で
    /// 測れる。塗り潰しは `Shake::circle` が両者で共通なので、どちらとして置いて
    /// も点の落ち方は変わらない。
    fn circle_footprint(radius: f64, x: f64, y: f64, w: u16, h: u16) -> (usize, usize, usize) {
        let empty = StarRingState::new();
        let mut placed = empty.clone();
        placed.projectiles.push(Projectile {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            damage: 1.0,
            life: 10,
            radius,
            pierce: 0,
            splash: 0.0,
            kind: WeaponKind::Pulse,
            spin: 0.0,
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
                    let (n, bw, bh) = circle_footprint(
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
        let stacked = is_stacked_layout(w);
        let (stage, _) = split_body(split_frame(Rect::new(0, 0, w, h))[2], stacked);
        let borders = if stacked {
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
        const ORBIT: Color = ORBIT_COLOR;
        const TURRET: Color = TURRET_NEAR_COLOR;

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

    /// 砲台が、環のどこにいても Canvas の四辺からはみ出さないこと。欠けると、
    /// 武装がフレームに削られた形で描かれる。
    ///
    /// 突き合わせるのは中心ではなく円の端で、半径は実際に描く
    /// `turret_radius`。奥側の砲台は手前側から `TURRET_FAR_SCALE` で縮むだけ
    /// なので、手前側の半径で見れば四辺とも上限になる。砲台は核を中心とする
    /// 楕円 (`StarRingState::ring_radii`) を回るので、四辺へいちばん寄るのは
    /// その楕円の端 — 位相をなめる代わりに端を直接見る。
    ///
    /// 突き合わせる範囲は画面シェイクの振れ込みで残る領域
    /// (`VISIBLE_X_LO`/`VISIBLE_X_HI`/`VISIBLE_Y_LO`/`VISIBLE_Y_HI`)。核側の
    /// 同じ不変条件は定数の隣の `const` アサートが持つ。
    #[test]
    fn turrets_never_leave_the_field() {
        // 余裕を使い切る砲台数では辺へちょうど接するので、丸め誤差ぶんを許す。
        const EPS: f64 = 1e-9;

        let mut state = StarRingState::new();
        for count in 0..=7u32 {
            state.weapon_levels[0][0] = count;
            let (ring_rx, ring_ry) = state.ring_radii();
            let r = turret_radius(ring_ry);
            let guns = state.turret_count();

            let bottom = CORE_Y - ring_ry - r;
            assert!(
                bottom >= VISIBLE_Y_LO - EPS,
                "砲台 {guns} 基で環の最下点の砲台が下端を割る (y={bottom})"
            );
            let top = CORE_Y + ring_ry + r;
            assert!(
                top <= VISIBLE_Y_HI + EPS,
                "砲台 {guns} 基で環の最上点の砲台が上端を越える (y={top})"
            );
            let left = CX - ring_rx - r;
            assert!(
                left >= VISIBLE_X_LO - EPS,
                "砲台 {guns} 基で環の左端の砲台が左辺を越える (x={left})"
            );
            let right = CX + ring_rx + r;
            assert!(
                right <= VISIBLE_X_HI + EPS,
                "砲台 {guns} 基で環の右端の砲台が右辺を越える (x={right})"
            );
        }
    }

    /// 核は最大の鉱石より大きく描かれること。拠点と的を、色より先に大きさで
    /// 読み分けられる差を持たせる。
    #[test]
    fn the_core_outgrows_every_ore() {
        let widest = OreKind::ALL
            .iter()
            .map(|k| k.radius())
            .fold(0.0f64, f64::max);
        assert!(
            CORE_RADIUS >= widest * SIZE_TELL_RATIO,
            "核 {CORE_RADIUS} が最大の鉱石 {widest} と同格に見える"
        );
    }

    /// どの層でも、合図が出た核はその層の平常時より必ず大きく膨らむこと。
    ///
    /// 層の伸びぶんの膨らみと合図の膨らみを同じ 1 本の倍率で奪い合わせると、
    /// 層が深いほど平常時が大きくなり、いずれ合図の膨らみを追い越して
    /// 「合図が出た瞬間に核が縮む」「層を重ねきると合図の差が消える」。層は
    /// 上限を持たないので、深い層まで見る。
    #[test]
    fn every_cue_swells_the_core_past_the_layer_it_sits_in() {
        let mut state = StarRingState::new();
        for layer in [1u32, 2, 6, 8, 15, 40, 400] {
            state.current_layer = layer;
            state.total_kills = 0;
            state.shards = 0.0;
            state.core_pulse_flash_ticks = 0;
            state.layer_ready_flash_ticks = 0;
            state.layer_flash_ticks = 0;
            let calm = core_halo_radius(&state);

            // 開放待ちの点滅は、灯っている位相だけ平常時より大きい。
            state.total_kills = u64::MAX;
            state.shards = f64::MAX;
            state.elapsed_ticks = 0;
            assert!(
                can_unlock_next_layer(&state),
                "第{layer}層で開放待ちの状態を作れていない"
            );
            let blink_on = core_halo_radius(&state);

            // 核脈動は開放待ちの間も鳴り続ける。軽い側が重い側を覆えば、
            // 「開放できる」を報せる点滅が脈動の周期に飲まれる。
            state.core_pulse_flash_ticks = 6;
            let blink_on_while_pulsing = core_halo_radius(&state);
            state.core_pulse_flash_ticks = 0;
            assert_eq!(
                blink_on_while_pulsing, blink_on,
                "第{layer}層: 核脈動の拍が開放待ちの点滅を上書きしている"
            );

            state.elapsed_ticks = 10;
            let blink_off = core_halo_radius(&state);
            state.total_kills = 0;
            state.shards = 0.0;
            state.elapsed_ticks = 0;
            assert_eq!(
                blink_off, calm,
                "第{layer}層: 点滅の消灯側が平常時と違う大きさで描かれる"
            );

            state.core_pulse_flash_ticks = 6;
            let pulse = core_halo_radius(&state);
            state.core_pulse_flash_ticks = 0;

            state.layer_ready_flash_ticks = LAYER_FLASH_TICKS;
            let ready = core_halo_radius(&state);
            state.layer_ready_flash_ticks = 0;

            state.layer_flash_ticks = LAYER_FLASH_TICKS;
            let open = core_halo_radius(&state);
            state.layer_flash_ticks = 0;

            for (before, after, what) in [
                (calm, pulse, "平常時 → 核脈動の拍"),
                (pulse, blink_on, "核脈動の拍 → 開放待ちの点滅"),
                (blink_on, ready, "開放待ちの点滅 → 撃破条件の達成"),
                (ready, open, "撃破条件の達成 → 層開放"),
            ] {
                assert!(
                    after > before,
                    "第{layer}層: {what} で核が {before} から {after} へ縮む/変わらない"
                );
            }
        }
    }

    /// いちばん軽い合図 (核脈動の拍) も、実際に描かれる点として太ること。
    ///
    /// 合図の膨らみは点の解像度より小さくなると絵に出ない。単発の合図より
    /// 軽くする以上、下限は「描いて増えるか」で見る。
    #[test]
    fn the_core_pulse_beat_shows_as_drawn_dots() {
        let mut state = StarRingState::new();
        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let halo_span = |st: &StarRingState| {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let cs = Rc::new(RefCell::new(ClickState::new()));
                cs.borrow_mut().terminal_cols = w;
                cs.borrow_mut().terminal_rows = h;
                terminal.draw(|f| render(st, f, f.area(), &cs)).unwrap();
                dot_span_w(&colored_dots(terminal.backend().buffer(), inner, CORE_HALO_COLOR))
            };

            let calm = halo_span(&state);
            assert!(calm > 0, "{w}x{h}: 平常時に暈が見えていない");
            state.core_pulse_flash_ticks = 3;
            let pulsing = halo_span(&state);
            state.core_pulse_flash_ticks = 0;
            assert!(
                pulsing > calm,
                "{w}x{h}: 核脈動の拍で暈が {calm}点幅 のまま太らない"
            );
        }
    }

    /// 層を重ねきった核でも、層開放の合図が実際に描かれる点として太ること。
    ///
    /// 合図の膨らみが上限で削られていないかは、倍率ではなく描いた点で見る。
    /// 暈の色 (`CORE_HALO_COLOR`) は、鉱石も粒子も置かない第15層のステージでは
    /// 暈だけが使う。
    #[test]
    fn the_layer_flash_still_shows_in_a_deep_layer() {
        let mut state = StarRingState::new();
        state.current_layer = 15;
        assert_ne!(
            star_color(state.layer()),
            CORE_HALO_COLOR,
            "背景星が暈と同色では、暈だけを測れない"
        );

        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let halo_span = |st: &StarRingState| {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let cs = Rc::new(RefCell::new(ClickState::new()));
                cs.borrow_mut().terminal_cols = w;
                cs.borrow_mut().terminal_rows = h;
                terminal.draw(|f| render(st, f, f.area(), &cs)).unwrap();
                dot_span_w(&colored_dots(terminal.backend().buffer(), inner, CORE_HALO_COLOR))
            };

            let calm = halo_span(&state);
            assert!(calm > 0, "{w}x{h}: 平常時に暈が見えていない");
            state.layer_flash_ticks = LAYER_FLASH_TICKS;
            let flashing = halo_span(&state);
            state.layer_flash_ticks = 0;
            assert!(
                flashing > calm,
                "{w}x{h}: 第15層の層開放で暈が {calm}点幅 から {flashing}点幅 にしかならない"
            );
        }
    }

    /// 核が、環の最下点にいる手前側の砲台へ食い込まないこと。
    ///
    /// 環は砲台が少ないほど核へ寄る (`StarRingState::ring_radii`) ので、砲台数を
    /// なめて最も狭くなるところを見る。弾数強化を 1 度も買っていない状態が
    /// いちばん狭い。
    #[test]
    fn the_core_never_swallows_the_lowest_turret() {
        let mut state = StarRingState::new();
        let core = CORE_RADIUS;
        for count in 0..=7u32 {
            state.weapon_levels[0][0] = count;
            let (_, ring_ry) = state.ring_radii();
            let gap = ring_ry - turret_radius(ring_ry) - core;
            assert!(
                gap > 0.0,
                "砲台 {} 基のとき、核 (半径 {core}) が最下点の砲台へ {gap} 食い込む",
                state.turret_count()
            );
        }
    }

    /// 層開放フラッシュの最中でも、環の最下点にいる砲台が痩せないこと。
    ///
    /// 核と暈は砲台より後に描かれるので、膨らんだ暈と砲台が 1 セルへ同居すると、
    /// そのセルは暈の色になって砲台の側から点が減る。ワールド座標で離れて
    /// いることを確かめるだけでは足りないので、実際に描いた点を数える。
    #[test]
    fn the_layer_flash_keeps_the_lowest_turret_intact() {
        const TURRET: Color = TURRET_NEAR_COLOR;

        let mut state = StarRingState::new();
        // 砲台 1 基だけの環が核にいちばん近い。その 1 基が最下点へ来る位相を選ぶ。
        state.elapsed_ticks = 168;
        let (_, ring_ry) = state.ring_radii();
        let lowest = turret_positions(&state)[0];
        assert!(
            (lowest.1 - (CORE_Y - ring_ry)).abs() < 0.1,
            "位相 {} tick で砲台が環の最下点にいない (y={})",
            state.elapsed_ticks,
            lowest.1
        );

        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let turret_dots = |st: &StarRingState| {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let cs = Rc::new(RefCell::new(ClickState::new()));
                cs.borrow_mut().terminal_cols = w;
                cs.borrow_mut().terminal_rows = h;
                terminal.draw(|f| render(st, f, f.area(), &cs)).unwrap();
                colored_dots(terminal.backend().buffer(), inner, TURRET).len()
            };

            let calm = turret_dots(&state);
            assert!(calm > 0, "{w}x{h}: 平常時に砲台が見えていない");
            state.layer_flash_ticks = LAYER_FLASH_TICKS;
            let flashing = turret_dots(&state);
            state.layer_flash_ticks = 0;
            // 膨らんだ暈が砲台のセルへ点を足すことはあるので、下限だけを見る。
            assert!(
                flashing >= calm,
                "{w}x{h}: 層開放フラッシュ中に砲台が {calm} 点から {flashing} 点へ痩せる"
            );
        }
    }

    /// 到達半径の外にいる鉱石が、核と暈の裏へ完全に隠れないこと。
    ///
    /// 鉱石は核より先に描かれるので、核が到達半径から遠く離れて塗り広がると、
    /// まだ消えていない鉱石が数 tick まるごと見えなくなる。
    #[test]
    fn the_core_never_hides_an_ore_that_has_not_reached_it() {
        let smallest = OreKind::ALL
            .iter()
            .map(|k| k.radius())
            .fold(f64::MAX, f64::min);
        assert!(
            CORE_RADIUS < INNER_RADIUS + smallest,
            "核 {CORE_RADIUS} が、到達半径 {INNER_RADIUS} の外にいる最小の鉱石 (半径 {smallest}) を丸ごと覆う"
        );

        // 暈が膨らみきる 2 通り — 層を重ねた定常状態と、層開放フラッシュ中。
        // 層は、採掘境界の色 (`layer_color`) が鉱石と重ならないものを選ぶ。
        for (layer, flash) in [(15u32, 0u32), (3, LAYER_FLASH_TICKS)] {
            let mut state = StarRingState::new();
            state.current_layer = layer;
            state.layer_flash_ticks = flash;
            let ore_hue = ore_color(OreKind::Dust);
            assert_ne!(
                ore_hue,
                layer_color(layer),
                "第{layer}層の採掘境界が鉱石と同色"
            );
            state.ores.push(Ore {
                x: CX,
                // 到達半径のすぐ外。ここで隠れるなら、どの距離でも隠れる。
                y: CORE_Y + INNER_RADIUS + 0.5,
                vx: 0.0,
                vy: 0.0,
                hp: 5.0,
                kind: OreKind::Dust,
                motion: OreMotion::Spiral,
                sway: 0.0,
                age: 0,
            });

            for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
                let inner = stage_inner(w, h);
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let cs = Rc::new(RefCell::new(ClickState::new()));
                cs.borrow_mut().terminal_cols = w;
                cs.borrow_mut().terminal_rows = h;
                terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
                let visible = colored_dots(terminal.backend().buffer(), inner, ore_hue).len();
                assert!(
                    visible > 0,
                    "{w}x{h}: 第{layer}層 (フラッシュ {flash} tick) で、到達前の鉱石が核の裏へ隠れる"
                );
            }
        }
    }

    /// 同色でも別物として読める、面を塗った円どうしの描画半径の比。
    ///
    /// 半径が 1.25 倍あれば占有する点数はおよそ 1.5 倍になり、隣り合っても
    /// 大きさで分けられる。核が最大の鉱石より大きいと言える比
    /// (`the_core_outgrows_every_ore`) と同じ値を使う。
    const SIZE_TELL_RATIO: f64 = 1.25;

    /// 同色の衝突検査にかける、ステージの描画物 1 つ。
    struct StageMark {
        /// 検査が落ちたときに、どれとどれが潰れ合うかを示す名前。
        what: String,
        /// 同じ物の別の見え方 (合図ごとの核の色・環の広さで変わる砲台の半径) を
        /// まとめる名前。同じ物どうしは色が揃っていて構わない。
        group: &'static str,
        color: Color,
        /// 面を塗った円の描画半径 (ワールド単位)。点・線・輪郭は `None`。
        blob_radius: Option<f64>,
        /// フィールドへ散る常設の点描か。散った点どうしは大きさも形も同じ 1 点
        /// なので、色でしか分けられない。
        field_furniture: bool,
    }

    fn blob(group: &'static str, what: impl Into<String>, color: Color, radius: f64) -> StageMark {
        StageMark {
            what: what.into(),
            group,
            color,
            blob_radius: Some(radius),
            field_furniture: false,
        }
    }

    fn thin(group: &'static str, what: impl Into<String>, color: Color) -> StageMark {
        StageMark {
            what: what.into(),
            group,
            color,
            blob_radius: None,
            field_furniture: false,
        }
    }

    /// 毎フレーム同じ場所に出て、フィールド全体へ点として散る装飾。
    fn furniture(group: &'static str, what: impl Into<String>, color: Color) -> StageMark {
        StageMark {
            what: what.into(),
            group,
            color,
            blob_radius: None,
            field_furniture: true,
        }
    }

    /// 弾の描画半径。
    fn projectile_radius(kind: WeaponKind) -> f64 {
        match kind {
            WeaponKind::Pulse => PULSE_PROJECTILE_RADIUS,
            WeaponKind::Ray => RAY_PROJECTILE_RADIUS,
            WeaponKind::Scatter => SCATTER_PROJECTILE_RADIUS,
            WeaponKind::Arc => ARC_PROJECTILE_RADIUS,
            WeaponKind::Nova => NOVA_PROJECTILE_RADIUS,
        }
    }

    /// 核本体が取りうる色。合図ごとの分岐を実際に `core_color` へ通して集める。
    fn core_colors() -> Vec<(String, Color)> {
        let mut out = Vec::new();
        let calm = StarRingState::new();
        out.push(("核 (平常)".to_string(), core_color(&calm)));

        let mut boosting = StarRingState::new();
        boosting.boost_ticks = 10;
        out.push(("核 (ブースト)".to_string(), core_color(&boosting)));

        let mut ready = StarRingState::new();
        ready.layer_ready_flash_ticks = LAYER_FLASH_TICKS;
        out.push(("核 (条件達成)".to_string(), core_color(&ready)));

        // 層開放中は層の色をまとう。層に上限は無いので、色が一巡するところまで。
        for layer in 1..=9u32 {
            let mut opening = StarRingState::new();
            opening.current_layer = layer;
            opening.layer_flash_ticks = LAYER_FLASH_TICKS;
            out.push((format!("核 (第{layer}層の開放)"), core_color(&opening)));
        }
        out
    }

    /// ステージへ同時に出る描画物と、その色。
    ///
    /// 色の衝突を検査できるのは、ステージに出る色をここへ列挙し切れている間
    /// だけ。列挙が閉じていることは `stage_colours_live_in_the_palette` が
    /// 見張る。
    fn stage_marks() -> Vec<StageMark> {
        let mut marks = Vec::new();

        for kind in OreKind::ALL {
            marks.push(blob("鉱石", format!("鉱石 {kind:?}"), ore_color(kind), kind.radius()));
        }
        for kind in WeaponKind::ALL {
            marks.push(blob(
                "弾",
                format!("弾 {kind:?}"),
                weapon_color(kind),
                projectile_radius(kind),
            ));
        }
        // 砲台の半径は環の広さで動く (`turret_radius`)。帯の両端を置いて、どこに
        // 居ても他の円と潰れ合わないことを見る。
        for r in [TURRET_NEAR_RADIUS, TURRET_MAX_RADIUS] {
            marks.push(blob("手前の砲台", "手前の砲台", TURRET_NEAR_COLOR, r));
            marks.push(blob(
                "奥の砲台",
                "奥の砲台",
                TURRET_FAR_COLOR,
                r * TURRET_FAR_SCALE,
            ));
        }
        for (what, color) in core_colors() {
            marks.push(blob("核", what, color, CORE_RADIUS));
        }

        marks.push(furniture("核の暈", "核の暈", CORE_HALO_COLOR));
        marks.push(furniture("砲台環", "砲台環", ORBIT_COLOR));
        marks.push(furniture("壁", "フィールドの壁", FIELD_WALL_COLOR));
        marks.push(thin("波面", "核脈動の波面", PULSE_WAVE_COLOR));
        marks.push(thin("火花", "火花", SPARK_COLOR));
        marks.push(thin("粉", "粉", DUST_PARTICLE_COLOR));
        marks.push(thin("破片", "破片", SHARD_PARTICLE_COLOR));
        marks.push(thin("燃え残り", "燃え残り", EMBER_PARTICLE_COLOR));
        for layer in 1..=9u32 {
            marks.push(thin(
                "採掘境界",
                format!("第{layer}層の採掘境界"),
                layer_color(layer),
            ));
            marks.push(furniture(
                "背景星",
                format!("第{layer}層の背景星"),
                star_color(layer),
            ));
        }
        // 尾は本体と同色。どちらの物に付いた尾かを色で示すため、あえて揃える。
        marks
    }

    /// 同時に画面へ出て、大きさでも形でも分けられないものが同色にならないこと。
    ///
    /// ステージの物を見分ける手がかりは大きさ・形・色の 3 つで、16 色を全員で
    /// 分け合っているのは色だけ。面を塗った円どうしは形が同じなので、同色なら
    /// 描画半径が `SIZE_TELL_RATIO` 倍以上離れていること。手前側の砲台と光線弾の
    /// ように、同じ形で描画半径まで並ぶものは色でしか分けられない。
    ///
    /// 点・線・輪郭は面を持たないので、円とは形で分かれる。同じ色を円へ回して
    /// よいのはそのため。ただし、毎フレーム画面へ散る常設の点描 (背景星・環・
    /// 壁・核の暈) どうしは大きさも形も同じ 1 点でしかないので、互いに色を
    /// 分ける。採掘境界は横一列に連なる線として、粒子は着弾の瞬間だけ出る点と
    /// して、それぞれ並び方と寿命で常設の点描から分かれる。
    ///
    /// 核脈動の波面だけは扱いが違う。`StarRingState::pulse_reach` ぶん広がって
    /// フィールドのほぼ全域を毎周期舐めるので、どの円の上も層の色の上も通る。
    #[test]
    fn things_that_share_the_stage_never_share_a_colour() {
        let marks = stage_marks();

        for (i, a) in marks.iter().enumerate() {
            let Some(ra) = a.blob_radius else { continue };
            for b in &marks[i + 1..] {
                let Some(rb) = b.blob_radius else { continue };
                if a.color != b.color || a.group == b.group {
                    continue;
                }
                let (small, large) = if ra <= rb { (ra, rb) } else { (rb, ra) };
                assert!(
                    large >= small * SIZE_TELL_RATIO,
                    "{} (半径 {ra}) と {} (半径 {rb}) が同色で同じ大きさに見える",
                    a.what,
                    b.what
                );
            }
        }

        for mark in &marks {
            if mark.blob_radius.is_none() {
                continue;
            }
            assert_ne!(
                PULSE_WAVE_COLOR, mark.color,
                "核脈動の波面が {} と同色",
                mark.what
            );
        }
        for layer in 1..=9u32 {
            assert_ne!(
                PULSE_WAVE_COLOR,
                layer_color(layer),
                "核脈動の波面が第{layer}層の色と同色"
            );
        }

        for (what, color) in core_colors() {
            assert_ne!(
                CORE_HALO_COLOR, color,
                "核の暈が {what} と同色では、暈が本体の輪郭に見えない"
            );
        }

        let furniture: Vec<&StageMark> = marks.iter().filter(|m| m.field_furniture).collect();
        for (i, a) in furniture.iter().enumerate() {
            for b in &furniture[i + 1..] {
                if a.group == b.group {
                    continue;
                }
                assert_ne!(
                    a.color, b.color,
                    "{} と {} が同色で、どちらの点か決められない",
                    a.what, b.what
                );
            }
        }
    }

    /// ステージの描画色が `ctx.draw` の呼び出しへ直に書かれていないこと。
    ///
    /// 色を呼び出し側へ散らすと、同時に画面へ出る色をコードから列挙できなくなり、
    /// `things_that_share_the_stage_never_share_a_colour` はその色を検査しないまま
    /// 通る。
    #[test]
    fn stage_colours_live_in_the_palette() {
        let src = include_str!("render.rs");
        let head = src
            .find("\nfn render_stage(")
            .expect("render_stage の定義が見つからない");
        let body = &src[head + 1..];
        let tail = body
            .find("\n}\n")
            .expect("render_stage の終わりが見つからない");
        assert!(
            !body[..tail].contains("Color::"),
            "render_stage が色を直に書いている — 色はステージの色の節へ移す"
        );
    }

    /// 畳んだ波が、広がっている途中の波と同じ濃さで描かれること。
    ///
    /// 畳んだ波に残る 1tick (`PulseRing::folded`) は、一息に削った半径を一度だけ
    /// 描かせるためのもの。ここを寿命の残りとして薄く描くと、その波が削った
    /// 範囲はどのフレームにも出てこない。
    #[test]
    fn a_folded_wave_is_drawn_as_solidly_as_one_still_expanding() {
        const RADIUS: f64 = 40.0;
        let wave = |folded: bool, life: u32, reach: f64| PulseRing {
            radius: RADIUS,
            reach,
            life,
            max_life: 10,
            damage: 1.0,
            folded,
        };

        // 畳まれた波は `reach` まで跳ねた先で 1tick だけ残る。
        let mut folded = StarRingState::new();
        folded.pulse_rings.push(wave(true, 1, RADIUS));
        // 比較対象は、同じ半径をまだ広がっている途中で通っている波。
        let mut expanding = StarRingState::new();
        expanding.pulse_rings.push(wave(false, 10, RADIUS + 20.0));

        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let wave_dots = |st: &StarRingState| {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let cs = Rc::new(RefCell::new(ClickState::new()));
                cs.borrow_mut().terminal_cols = w;
                cs.borrow_mut().terminal_rows = h;
                terminal.draw(|f| render(st, f, f.area(), &cs)).unwrap();
                colored_dots(terminal.backend().buffer(), inner, PULSE_WAVE_COLOR).len()
            };

            let still_expanding = wave_dots(&expanding);
            assert!(still_expanding > 0, "{w}x{h}: 波面が描かれていない");
            let folded_dots = wave_dots(&folded);
            assert!(
                folded_dots >= still_expanding,
                "{w}x{h}: 畳んだ波が {folded_dots} 点で、広がっている途中の波 {still_expanding} 点より薄い"
            );
        }
    }

    /// 無彩色で描く点の明るさ (0〜255)。xterm のグレースケール ramp と、端末が
    /// 無彩色として出す既定色だけを扱う。
    fn grey_level(color: Color) -> Option<u16> {
        match color {
            Color::Indexed(n @ 232..=255) => Some(8 + (n as u16 - 232) * 10),
            Color::DarkGray => Some(128),
            Color::Gray => Some(192),
            Color::White => Some(255),
            _ => None,
        }
    }

    /// 常設の点描が「背景ほど暗い」順に並ぶこと。
    ///
    /// 環は砲台がどの経路を通るかを示す線なので、背景の星より暗いと経路が
    /// 背景へ沈む。無彩色どうしは明度でしか順序を付けられないので、壁・星・
    /// 暈・環の 4 つを暗い順の 1 本の並びとして固定する。層が深い側の星は
    /// 色味を持ち、明度ではなく色相で分かれるのでこの並びには入らない。
    #[test]
    fn the_field_furniture_gets_brighter_towards_the_front() {
        let chain = [
            ("フィールドの壁", FIELD_WALL_COLOR),
            ("第1層の背景星", star_color(1)),
            ("第2層の背景星", star_color(2)),
            ("核の暈", CORE_HALO_COLOR),
            ("砲台環", ORBIT_COLOR),
        ];
        let mut prev: Option<(&str, u16)> = None;
        for (what, color) in chain {
            let level = grey_level(color)
                .unwrap_or_else(|| panic!("{what} が無彩色として明るさを比べられない"));
            if let Some((prev_what, prev_level)) = prev {
                assert!(
                    level > prev_level,
                    "{what} (明るさ {level}) が {prev_what} (明るさ {prev_level}) より暗い"
                );
            }
            prev = Some((what, level));
        }
    }

    /// 核と手前側の砲台が、内側に穴の無い塊として描かれること。
    ///
    /// 塗り潰しの標本間隔が点の間隔と噛み合わないと、円の内側でドットを
    /// 取りこぼして市松に見える。画面の主役 2 つは、いちばん粗いモバイル幅でも
    /// 塊として読めること。
    #[test]
    fn the_core_and_the_turret_are_drawn_as_solid_blobs() {
        let mut state = StarRingState::new();
        state.weapon_levels[0][0] = 3;
        // 核に重なる位置の砲台は、後から描く核に上書きされて塗り潰しの検査に
        // ならない。環の最下点から外れた位相を選び、実際に離れていることを見る。
        state.elapsed_ticks = 23;
        for &(gx, gy, depth) in turret_positions(&state).iter() {
            if depth > 0.0 {
                continue;
            }
            let d = (gx - CX).hypot(gy - CORE_Y);
            assert!(
                d > CORE_RADIUS + TURRET_MAX_RADIUS + 4.0,
                "位相 {} tick では砲台が核へ寄りすぎている (距離 {d})",
                state.elapsed_ticks
            );
        }

        for (w, h) in [DESKTOP_STAGE, PHONE_STAGE] {
            let inner = stage_inner(w, h);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cs = Rc::new(RefCell::new(ClickState::new()));
            cs.borrow_mut().terminal_cols = w;
            cs.borrow_mut().terminal_rows = h;
            terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
            let buf = terminal.backend().buffer();

            for (what, color) in [("核", core_color(&state)), ("砲台", TURRET_NEAR_COLOR)] {
                let blobs = blob_groups(&colored_dots(buf, inner, color));
                assert!(!blobs.is_empty(), "{w}x{h}: {what} が描かれていない");
                for blob in blobs {
                    if let Some(row) = row_with_a_gap(&blob) {
                        panic!(
                            "{w}x{h}: {what} の塊 ({}点) の {row} 行目で点が途切れている",
                            blob.len()
                        );
                    }
                }
            }
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

