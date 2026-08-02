//! ゲーム横断の共通テーマ定義。
//!
//! 各ゲームの「ブランドカラー」(メニューでの識別色)、HP/進行度バーの標準
//! 文字セット、フラッシュ演出の標準プリセット (色 + 長さ) をここに集約する。
//! 個々のゲームの `render.rs` / `effects.rs` はこれらのトークンを参照しつつ、
//! シナリオ固有の細かい判断 (どのイベントでどのプリセットを使うか等) は
//! 引き続き各ゲームに委ねる。
//!
//! Duration はミリ秒の `u32` で持つ (tachyonfx への依存を作らないため)。
//! 効果を組み立てる側で `tachyonfx::Duration::from_millis(preset.duration_ms)`
//! に変換する。

use ratzilla::ratatui::style::Color;

use crate::games::GameChoice;

/// メニュー上でゲームを識別する固有色。ゲーム内 UI のアクセントにも流用できる。
/// `const fn` にしているのは、呼び出し側 (`main.rs` の `MENU_ENTRIES`) が
/// `const` 配列の初期化子からそのまま呼びたいため。
pub const fn accent(choice: &GameChoice) -> Color {
    match choice {
        GameChoice::Cookie => Color::LightYellow,
        GameChoice::Factory => Color::Cyan,
        GameChoice::Rpg => Color::LightRed,
        GameChoice::Abyss => Color::LightBlue,
        GameChoice::Godfield => Color::Red,
        GameChoice::Metropolis => Color::LightCyan,
        GameChoice::LoopMarch => Color::LightGreen,
    }
}

/// HP バーの塗りセル。
pub const BAR_FULL: char = '█';
/// HP バーの空セル。
pub const BAR_EMPTY: char = '░';

/// HP 比率 (0.0〜1.0 想定、範囲外はクランプ) から `width` セル幅のバー文字列を作る。
pub fn hp_bar_string(ratio: f64, width: usize) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let mut s = String::with_capacity(width);
    for i in 0..width {
        s.push(if i < filled { BAR_FULL } else { BAR_EMPTY });
    }
    s
}

/// HP 比率から警戒色を 3 段階 (green > 2/3 > yellow > 1/3 > red) で返す。
/// ゲームをまたいで同じ閾値にすることで、プレイヤーが「黄色は危険」を
/// 学習し直さずに済む。
pub fn hp_ratio_color(ratio: f64) -> Color {
    if ratio > 2.0 / 3.0 {
        Color::Green
    } else if ratio > 1.0 / 3.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// クールダウン等の進行度バーの塗りセル。HP バーと形を変え、意味の違いを
/// 視覚的に区別する。
pub const PROGRESS_FULL: char = '▰';
/// 進行度バーの空セル。
pub const PROGRESS_EMPTY: char = '▱';

/// 一時的な色変化 (フラッシュ) 演出の標準プリセット。
///
/// 「何が起きたか」という意味のカテゴリごとに色と長さを固定することで、
/// ゲームが増えても演出のトーンが揃う。値は abyss で実際に使われ調整済みの
/// ものを踏襲している。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashPreset {
    pub color: Color,
    pub duration_ms: u32,
}

/// 自分 (プレイヤー側) がダメージを受けた瞬間。
pub const DAMAGE_FLASH: FlashPreset = FlashPreset { color: Color::Red, duration_ms: 160 };
/// 相手 (敵・対戦相手) にダメージを与えた瞬間。
pub const HIT_FLASH: FlashPreset = FlashPreset { color: Color::Yellow, duration_ms: 120 };
/// 何かを達成・解放した瞬間 (購入確定、装備解放など)。
pub const ACHIEVEMENT_FLASH: FlashPreset = FlashPreset { color: Color::Indexed(220), duration_ms: 600 };
/// 後退・喪失を示す瞬間 (撤退、死亡、敗北など)。
pub const SETBACK_FLASH: FlashPreset = FlashPreset { color: Color::Indexed(52), duration_ms: 650 };
/// 前進・突破を示す瞬間 (フロア到達、ステージクリアなど)。
pub const ADVANCE_FLASH: FlashPreset = FlashPreset { color: Color::Indexed(17), duration_ms: 450 };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_covers_every_game_choice_with_a_distinct_color() {
        let choices = [
            GameChoice::Cookie,
            GameChoice::Factory,
            GameChoice::Rpg,
            GameChoice::Abyss,
            GameChoice::Godfield,
            GameChoice::Metropolis,
            GameChoice::LoopMarch,
        ];
        let colors: Vec<Color> = choices.iter().map(accent).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors[i], colors[j],
                    "{:?} と {:?} のアクセントカラーが衝突している",
                    choices[i], choices[j]
                );
            }
        }
    }

    #[test]
    fn hp_bar_string_fills_proportionally_to_ratio() {
        assert_eq!(hp_bar_string(1.0, 10), "██████████");
        assert_eq!(hp_bar_string(0.0, 10), "░░░░░░░░░░");
        assert_eq!(hp_bar_string(0.5, 10), "█████░░░░░");
    }

    #[test]
    fn hp_bar_string_clamps_out_of_range_ratio() {
        // effective_max_hp のような装備ボーナス込みの上限を下回った直後は
        // hp > max になり得るため、ratio > 1.0 でも width を超えないこと
        // (超えるとステータス行の後続要素を押し出してしまう) を保証する。
        assert_eq!(hp_bar_string(1.5, 10), "██████████");
        assert_eq!(hp_bar_string(-0.5, 10), "░░░░░░░░░░");
    }

    #[test]
    fn hp_bar_string_zero_width_is_empty() {
        assert_eq!(hp_bar_string(0.5, 0), "");
    }

    #[test]
    fn hp_ratio_color_three_tiers_at_thresholds() {
        assert_eq!(hp_ratio_color(1.0), Color::Green);
        assert_eq!(hp_ratio_color(2.0 / 3.0 + 0.01), Color::Green);
        assert_eq!(hp_ratio_color(2.0 / 3.0), Color::Yellow, "境界値ちょうどはgreenに含めない");
        assert_eq!(hp_ratio_color(0.5), Color::Yellow);
        assert_eq!(hp_ratio_color(1.0 / 3.0), Color::Red, "境界値ちょうどはyellowに含めない");
        assert_eq!(hp_ratio_color(0.0), Color::Red);
    }
}
