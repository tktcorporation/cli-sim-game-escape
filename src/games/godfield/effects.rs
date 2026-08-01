//! 演出レイヤー (共通 `crate::effects::EffectHost` の薄いラッパー)。
//!
//! ratatui 描画後の Buffer に shader 風の post-process を当てる。ここでの
//! 演出 (push_* メソッド) を増やす分には state.rs / logic.rs の変更は不要で、
//! GodFieldGame::render の最後に `process(...)` を呼ぶだけで動く設計
//! (被弾フラッシュの残り時間自体は state.rs 側で `crate::effects::FlashTimer`
//! を使って管理しており、これとは別レイヤー)。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use tachyonfx::fx;
use tachyonfx::{Duration, Motion};

use crate::effects::EffectHost;
use crate::theme;

pub struct GfEffects {
    host: EffectHost,
}

impl GfEffects {
    pub fn new() -> Self {
        Self { host: EffectHost::new() }
    }

    /// プレイヤーが被弾した瞬間の演出。area は status パネル全体。
    pub fn push_player_hit(&mut self, status_panel: Rect) {
        let preset = theme::DAMAGE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, status_panel);
    }

    /// プレイヤーが撃破された瞬間の演出。
    pub fn push_death(&mut self, status_panel: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, status_panel);
    }

    /// 勝利画面が出た瞬間の演出。画面全体に金色のスウィープ。
    pub fn push_victory(&mut self, area: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::LeftToRight, 10, 3, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 敗北画面が出た瞬間の演出。画面全体に暗い赤のスウィープ (下から上へ、abyss の撤退演出と同じ「巻き戻し」方向)。
    pub fn push_defeat(&mut self, area: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::sweep_in(Motion::DownToUp, 8, 6, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.host.process(elapsed, buf, area);
    }
}

impl Default for GfEffects {
    fn default() -> Self {
        Self::new()
    }
}
