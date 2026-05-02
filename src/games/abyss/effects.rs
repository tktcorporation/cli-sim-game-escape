//! 演出レイヤー (tachyonfx ラッパー)。
//!
//! ratatui 描画後の Buffer に shader 風の post-process を当てる。
//! state.rs / logic.rs を一切変更せず、AbyssGame::render の最後に
//! `process(...)` を呼ぶだけで動く設計。
//!
//! ## なぜ render 内で push するか
//! Effect は area (Rect) を必要とし、area は render 時にしか確定しない。
//! なので state 差分検知 (例: `prev_floor != state.floor`) と effect push
//! の両方を render 冒頭でまとめて行う。

use std::time::Duration;

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::style::Color;
use tachyonfx::fx;
use tachyonfx::{EffectManager, Motion};

/// AbyssGame が保持する演出マネージャ。
///
/// 各 push_* メソッドは 1 つの効果シナリオに対応する。新しい演出を増やす時は
/// 新しい push メソッドを追加して mod.rs の `detect_transitions` から呼ぶ。
pub struct AbyssEffects {
    manager: EffectManager<()>,
}

impl AbyssEffects {
    pub fn new() -> Self {
        Self {
            manager: EffectManager::default(),
        }
    }

    /// 階層遷移時の演出。area は描画される画面全体 (戦闘パネルだけに絞っても可)。
    ///
    /// sweep_in: 指定方向にグラデーション幅を持って文字を流し込む。
    /// gradient_length=10 → 「綺麗な波」、randomness>0 → 「ノイズ混じりの波」。
    pub fn push_floor_transition(&mut self, area: Rect) {
        let mut effect = fx::sweep_in(
            Motion::UpToDown,
            10,                       // gradient_length
            0,                        // randomness
            Color::Black,             // fade-from color
            Duration::from_millis(500),
        );
        effect.set_area(area);
        self.manager.add_effect(effect);
    }

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    /// `elapsed` は前回 render からの wall-clock 差分。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed, buf, area);
    }
}
