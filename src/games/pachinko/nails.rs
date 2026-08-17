//! 釘配置。千鳥の正格子と、台ごとの開き・傾きを座標へ落とす。
//!
//! 台の個性 (`nail_spread` / `rail_bias`) はここが釘の座標へ落とし込み、
//! そこから先は物理だけが結果を決める。数値を当落へ直接掛けないことで、
//! 盤面の見た目と実測回転率が食い違わない。

use super::board::{pocket_half_w, Arch, CONTACT_DIST};
use super::rng::rand_range;
use super::state::{
    Nail, ATTACKER_HALF_W, ATTACKER_X, ATTACKER_Y, BALL_R, BOARD_W, NAIL_R, START_POCKET_X,
    START_POCKET_Y,
};

/// 寄り釘の段数と、最上段の y。
///
/// パチンコのゲージも Plinko / Galton board も、釘は**千鳥の正格子**で打つ。
/// 各段の釘は等間隔、次の段は半ピッチずらす。こうすると上の段の隙間の中央に
/// 次の段の釘が来て、玉は段ごとに必ず当たって左右へ割れる。縦に揃えると
/// 隙間を真っ直ぐ落ち、不規則にすると隙間の列ができて同じように落ちる。
///
/// 実機のゲージは 11mm 玉に対して釘間を 11.75〜13.5mm に揃える。ここでの
/// ピッチは盤面幅に合わせたスケールで、玉が隙間を抜けられるが次の段では
/// 釘に当たる大きさにする。
///
/// 中央の扱いだけは格子から外す。全段を空けると縦溝になって出玉率が崩れ、
/// 全段を塞ぐとヘソへ届かなくなる。偶数段は中央を空け、奇数段は中央に
/// ゲートを置く。
///
/// 下側2段は千鳥にしない。隙間を拾う段ではなく、当たった玉を外側へ歩かせる
/// ヘソ前のゲートなので、同じ x に重ねる。ずらすと、一段目で弾かれた玉が
/// 二段目を外してヘソへ落ち、出玉率が 1 を超える。
///
/// 12時から落ちた玉は盤面中央へ来る。偶数段と同じく中央を空けると、中段を
/// 抜けたあと縦溝でヘソへ直行する。ここは奇数段と同じ中央ゲートを重ねて、
/// 一度当たってからヘソ釘の開きで選別されるようにする。
const RAIL_ROWS: usize = 8;
pub(super) const RAIL_TOP_Y: f64 = 16.0;
/// 同じ段の釘の中心間隔。次の段は半分ずらす。
///
/// 半ピッチが `CONTACT_DIST * 2` より大きいと、千鳥でも釘と釘のあいだに
/// 縦の抜け道が残る。実機のゲージは玉が次の段で必ず釘に当たる間隔なので、
/// ここも半ピッチを捕獲幅に近づける。
const RAIL_PITCH: f64 = 6.0;
/// 段の間隔。正三角形 (`pitch * √3/2` ≈ 5.2) より少し詰める。ヘソ前まで
/// 段を足すと正三角形のままでは最下段がヘソ釘と重なり、詰めると跳ねた弧が
/// 次の段へ届いて中段の空落下が消える。
pub(super) const RAIL_ROW_DY: f64 = 4.50;
const RAIL_BOTTOM_Y: f64 = RAIL_TOP_Y + RAIL_ROW_DY * (RAIL_ROWS as f64 - 1.0);
const _: () = assert!(RAIL_BOTTOM_Y > RAIL_TOP_Y);
const _: () = assert!(RAIL_PITCH / 2.0 <= CONTACT_DIST * 2.0 + BALL_R);
const _: () = assert!(Arch::TABLE.b <= RAIL_TOP_Y);
/// 偶数段。中央は空け、右端 (発射側 x≈59) まで届ける。
const RAIL_EVEN_OFFSETS: [f64; 10] = [-27.0, -21.0, -15.0, -9.0, -3.0, 3.0, 9.0, 15.0, 21.0, 27.0];
/// 奇数段。偶数段の隙間の中央に置き、中央にゲートを置く。
const RAIL_ODD_OFFSETS: [f64; 11] = [
    -30.0, -24.0, -18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0, 24.0, 30.0,
];
const _: () = assert!(RAIL_ROWS.is_multiple_of(2));
/// `rail_bias` が最大のときに外側の釘を中央へ寄せる割合。中央の釘は動かず、
/// 端ほど大きく動くので、盤面では「上部の釘が中央へ傾いている」形に見える。
///
/// 千鳥の隙間は半ピッチずつずれている。格子全体を大きく縮めると、ずれが
/// 潰れて隙間がヘソの真上へ揃う `rail_bias` の値でだけ玉道が一本に繋がり、
/// 回転率が跳ね上がる。傾きは玉道を大きく変えない範囲に留める。
const RAIL_BIAS_PULL: f64 = 0.03;
/// 寄り釘1本ごとの位置の揺らぎ。同じ `nail_spread` / `rail_bias` の台でも
/// 盤面が同一にならないようにして、台ごとの見た目の個体差を作る。
///
/// 千鳥の正格子が玉道の本体なので、揺らぎは台差用の仕上げに留める。
/// ここを大きくすると格子が崩れ、隙間の列ができて玉が釘を外して落ちる。
/// 玉道を決めるのが「見えるヘソ釘の開き」ではなく「見えない寄り釘のズレ」
/// にもなる。`simulator::nail_spread_correlates_with_spin_rate` がその退行を
/// 検知する。
const NAIL_JITTER: f64 = 0.22;
const NAIL_Y_JITTER: f64 = 0.18;

/// 下部釘の段数と本数。ヘソより下にあり回転率に効かないので、盤面を疏に
/// 保つ側から間引く。左右の一般入賞口へ玉を振り分ける道が見える本数。
const LOWER_ROWS: usize = 2;
const LOWER_TOP_Y: f64 = 60.0;
const LOWER_BOTTOM_Y: f64 = 72.0;
const LOWER_NAILS_PER_ROW: usize = 2;
const LOWER_INSET: f64 = 10.0;

/// 台の釘配置を seed から生成する。`nail_spread` / `rail_bias` を釘の座標
/// そのものへ反映させることで、プレイヤーは盤面を見て回りやすさを推し量れる
/// (数値としては UI に出さない)。
pub fn generate_nails(seed: &mut u32, nail_spread: f64, rail_bias: f64) -> Vec<Nail> {
    let mut nails = Vec::new();
    let center = BOARD_W / 2.0;

    // 寄り釘。千鳥の正格子。偶数段は中央を空け、奇数段は中央にゲート。
    // 同じ x に全段並べると縦溝か壁かの二択になる。
    for row in 0..RAIL_ROWS {
        let y = RAIL_TOP_Y + row as f64 * RAIL_ROW_DY;
        let offsets: &[f64] = if row + 2 >= RAIL_ROWS {
            &RAIL_ODD_OFFSETS
        } else if row % 2 == 0 {
            &RAIL_EVEN_OFFSETS
        } else {
            &RAIL_ODD_OFFSETS
        };
        for &off in offsets {
            let base_x = center + off;
            let pulled = center + (base_x - center) * (1.0 - rail_bias * RAIL_BIAS_PULL);
            // 中央ゲートは揺らぎで消さない。壁か縦溝かに落ちる。
            let j = if off.abs() < 0.5 {
                NAIL_JITTER * 0.3
            } else {
                NAIL_JITTER
            };
            nails.push(Nail {
                x: pulled + rand_range(seed, -j, j),
                y: y + rand_range(seed, -NAIL_Y_JITTER, NAIL_Y_JITTER),
            });
        }
    }

    // ステージ。ヘソの少し上に浅い受け皿を作り、ここに乗った玉が左右へ
    // 転がって隙間から落ちるかどうかが「入りそう」の本体になる。
    // 内側の隙間は通常時の受け口より広くし、ステージを抜けた玉が
    // その下のヘソ釘で最終的に選別されるようにする。
    let inner = pocket_half_w(nail_spread, false) + NAIL_R + 0.55;
    for side in [-1.0, 1.0] {
        nails.push(Nail {
            x: START_POCKET_X + side * inner,
            y: START_POCKET_Y - 3.8,
        });
    }

    // ヘソ釘。この2本の間隔が「開いて見える」ことが釘読みの手がかりになるので、
    // 通常時の受け口 (`pocket_half_w`) の外側へ釘半径分だけ逃がした位置に
    // 置き、見た目と当たり判定を一致させる。
    let mouth = pocket_half_w(nail_spread, false) + NAIL_R;
    for side in [-1.0, 1.0] {
        nails.push(Nail {
            x: START_POCKET_X + side * mouth,
            y: START_POCKET_Y - 2.0,
        });
    }

    // 下部釘。ヘソを外した玉を一般入賞口とアタッカーへ振り分ける。
    for row in 0..LOWER_ROWS {
        let t = row as f64 / (LOWER_ROWS - 1) as f64;
        let y = LOWER_TOP_Y + (LOWER_BOTTOM_Y - LOWER_TOP_Y) * t;
        let span = BOARD_W - 2.0 * LOWER_INSET;
        for i in 0..LOWER_NAILS_PER_ROW {
            let u = i as f64 / (LOWER_NAILS_PER_ROW - 1) as f64;
            let stagger = if row % 2 == 0 {
                0.0
            } else {
                span / (LOWER_NAILS_PER_ROW as f64 * 2.0)
            };
            let x = LOWER_INSET + span * u + stagger;
            nails.push(Nail {
                x: x + rand_range(seed, -NAIL_JITTER * 0.7, NAIL_JITTER * 0.7),
                y: y + rand_range(seed, -NAIL_Y_JITTER * 0.4, NAIL_Y_JITTER * 0.4),
            });
        }
    }

    // アタッカー脇。開放中の受け口へ玉を導く。
    for side in [-1.0, 1.0] {
        nails.push(Nail {
            x: ATTACKER_X + side * (ATTACKER_HALF_W + 1.5),
            y: ATTACKER_Y - 3.0,
        });
    }

    nails
}

/// 千鳥＋ヘソ＋下部の想定本数。段の配列を足した値で、生成結果と突き合わせる。
#[cfg(test)]
pub(super) fn expected_nail_count() -> usize {
    let mut n = 0usize;
    for row in 0..RAIL_ROWS {
        n += if row + 2 >= RAIL_ROWS || row % 2 == 1 {
            RAIL_ODD_OFFSETS.len()
        } else {
            RAIL_EVEN_OFFSETS.len()
        };
    }
    n + 2 + 2 + LOWER_ROWS * LOWER_NAILS_PER_ROW + 2
}

/// ホールに並ぶ台のヘソ釘の開きの範囲。
///
/// 下限は「全く回らない台」を並べないための足切り。上限は出玉率 (賞球総数 ÷
/// 打ち込み玉数) の天井を決める。開くほど回り、回るほど当たるので、ここを
/// 上げすぎると打つほど玉が増える台がホールに並び、有限の軍資金という前提が
/// 崩れる。`simulator::payout_ratio_stays_below_break_even` がこの上限の台を
/// 実際に打って確かめる。
pub const NAIL_SPREAD_RANGE: (f64, f64) = (0.25, 0.80);
/// ホールに並ぶ台の寄り釘の傾きの範囲。効き方は `RAIL_BIAS_PULL` を参照。
pub const RAIL_BIAS_RANGE: (f64, f64) = (-0.6, 0.9);

#[cfg(test)]
mod tests {
    use super::*;

    fn start_pocket_gap(nail_spread: f64) -> f64 {
        let mut seed = 0x1234_5678;
        let nails = generate_nails(&mut seed, nail_spread, 0.0);
        let mouth: Vec<f64> = nails
            .iter()
            .filter(|n| (n.y - (START_POCKET_Y - 2.0)).abs() < 1e-9)
            .map(|n| n.x)
            .collect();
        assert_eq!(mouth.len(), 2, "ヘソ釘が2本ではない: {mouth:?}");
        (mouth[0] - mouth[1]).abs()
    }

    #[test]
    fn nail_spread_widens_the_start_pocket_gap() {
        // 釘読みは「ヘソ釘の開きが見た目で分かる」ことが前提。開きが
        // `nail_spread` に連動しないと、盤面から回りやすさを読む軸が消える。
        let narrow = start_pocket_gap(0.15);
        let wide = start_pocket_gap(0.95);
        assert!(
            wide > narrow + 1.0,
            "nail_spread を上げてもヘソ釘が開いていない (狭={narrow:.2} 広={wide:.2})"
        );
    }

    #[test]
    fn rail_bias_pulls_upper_nails_toward_center() {
        // 寄り釘の傾きも釘読みの手がかり。bias を上げたら上部の釘が中央へ
        // 寄る、という対応が崩れると盤面の見た目が何も語らなくなる。
        let center = BOARD_W / 2.0;
        let spread_of = |bias: f64| {
            let mut seed = 0xABCD_1234;
            let nails = generate_nails(&mut seed, 0.5, bias);
            let upper: Vec<&Nail> = nails
                .iter()
                .filter(|n| n.y < START_POCKET_Y - 4.0)
                .collect();
            let sum: f64 = upper.iter().map(|n| (n.x - center).abs()).sum();
            sum / upper.len() as f64
        };
        assert!(
            spread_of(0.9) < spread_of(-0.6),
            "rail_bias を上げても上部釘が中央へ寄っていない (寄={:.2} 逃={:.2})",
            spread_of(0.9),
            spread_of(-0.6)
        );
    }

    #[test]
    fn generate_nails_keeps_the_galton_count() {
        // 密すぎると玉が毎コマ釘に当たり、弧を描いて跳ねる絵が残らない。
        // 本数が段×列から外れると、千鳥のどこかが欠けて抜け道になる。
        let expected = expected_nail_count();
        for seed in [1u32, 0xABCD, 0x5EED_1234, 99] {
            let mut s = seed;
            let nails = generate_nails(&mut s, 0.55, 0.0);
            assert_eq!(
                nails.len(),
                expected,
                "釘の本数が千鳥格子の想定から外れている (seed={seed})"
            );
        }
    }

    #[test]
    fn rail_pitch_is_uniform_in_a_row() {
        // ゲージは同じ段の間隔を揃える。不揃いだと隙間の列ができて落ちる。
        for w in RAIL_EVEN_OFFSETS.windows(2) {
            assert!(
                (w[1] - w[0] - RAIL_PITCH).abs() < 1e-9,
                "偶数段の間隔がピッチから外れている ({:?})",
                w
            );
        }
        for w in RAIL_ODD_OFFSETS.windows(2) {
            assert!(
                (w[1] - w[0] - RAIL_PITCH).abs() < 1e-9,
                "奇数段の間隔がピッチから外れている ({:?})",
                w
            );
        }
    }

    #[test]
    fn rail_rows_stagger_like_a_galton_board() {
        // 次の段の釘が上の段の隙間の中央に来る。縦に揃えると隙間を真っ直ぐ落ちる。
        for w in RAIL_EVEN_OFFSETS.windows(2) {
            let mid = (w[0] + w[1]) / 2.0;
            let nearest = RAIL_ODD_OFFSETS
                .iter()
                .map(|x| (x - mid).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 1e-9, "偶数段の隙間中央 {mid} に奇数段の釘が無い");
        }
    }

    #[test]
    fn rail_heso_gate_rows_stack_on_the_same_x() {
        // ヘソ前は千鳥で隙間を拾う場所ではない。同じ x に中央ゲートを重ねて、
        // 12時から落ちた玉を一度当ててからヘソ釘へ渡す。
        let mut seed = 0xA11C_E5EDu32;
        let nails = generate_nails(&mut seed, 0.55, 0.0);
        let y4 = RAIL_TOP_Y + RAIL_ROW_DY * (RAIL_ROWS as f64 - 2.0);
        let y5 = RAIL_TOP_Y + RAIL_ROW_DY * (RAIL_ROWS as f64 - 1.0);
        let xs_at = |y_target: f64| -> Vec<f64> {
            let mut xs: Vec<f64> = nails
                .iter()
                .filter(|n| (n.y - y_target).abs() < 2.0)
                .map(|n| n.x)
                .collect();
            xs.sort_by(|a, b| a.partial_cmp(b).expect("x が NaN"));
            xs
        };
        let upper = xs_at(y4);
        let lower = xs_at(y5);
        assert_eq!(
            upper.len(),
            lower.len(),
            "ヘソ前の2段で本数が違う ({upper:?} / {lower:?})"
        );
        for (a, b) in upper.iter().zip(lower.iter()) {
            assert!(
                (a - b).abs() < NAIL_JITTER * 2.0 + 0.05,
                "ヘソ前の2段が同じ x に重なっていない ({a:.2} vs {b:.2})"
            );
        }
    }

    #[test]
    fn rail_nails_alternate_a_center_gate() {
        // 偶数段が中央を開け、奇数段も中央を開けると縦溝になる。
        // 逆に全段が中央を塞ぐとヘソへ届かない。段ごとにゲートと隙間が
        // 入れ替わることを、最上段 (偶数) と次の段 (奇数) で見る。
        let mut seed = 0xA11C_E5EDu32;
        let nails = generate_nails(&mut seed, 0.55, 0.0);
        let center = BOARD_W / 2.0;
        let nearest = |y_target: f64| {
            nails
                .iter()
                .filter(|n| (n.y - y_target).abs() < 2.8)
                .map(|n| (n.x - center).abs())
                .fold(f64::INFINITY, f64::min)
        };
        let even_y = RAIL_TOP_Y;
        let odd_y = RAIL_TOP_Y + RAIL_ROW_DY;
        let even_gap = nearest(even_y);
        let odd_gap = nearest(odd_y);
        assert!(
            even_gap > CONTACT_DIST + 1.0,
            "偶数段の中央が塞がっている (最近={even_gap:.2})"
        );
        assert!(
            odd_gap < CONTACT_DIST,
            "奇数段に中央ゲートが無い (最近={odd_gap:.2})"
        );
        let last_y = RAIL_TOP_Y + RAIL_ROW_DY * (RAIL_ROWS as f64 - 1.0);
        assert!(
            nearest(last_y) < CONTACT_DIST,
            "ヘソ前の段に中央ゲートが無く、12時から落ちた玉が縦溝で抜ける (最近={:.2})",
            nearest(last_y)
        );
    }
}
