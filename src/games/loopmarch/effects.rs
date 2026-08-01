//! 演出レイヤー (共通 `crate::effects::EffectHost` の薄いラッパー)。
//!
//! ratatui 描画後の Buffer に shader 風の post-process を当てる。ここでの
//! 演出 (push_* メソッド) を増やす分には state.rs / logic.rs の変更は不要で、
//! LoopMarchGame::render の最後に `process(...)` を呼ぶだけで動く設計
//! (被弾フラッシュの残り時間自体は state.rs 側で `crate::effects::FlashTimer`
//! を使って管理しており、これとは別レイヤー)。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use tachyonfx::fx;
use tachyonfx::{Duration, Motion};

use crate::effects::EffectHost;
use crate::theme;

pub struct LoopMarchEffects {
    host: EffectHost,
}

impl LoopMarchEffects {
    pub fn new() -> Self {
        Self { host: EffectHost::new() }
    }

    /// 勇者が被弾した瞬間の演出。area はヘッダー全体。
    pub fn push_hero_hit(&mut self, header: Rect) {
        let preset = theme::DAMAGE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, header);
    }

    /// モンスターが被弾した瞬間の演出。area はリング全体。
    pub fn push_enemy_hit(&mut self, ring: Rect) {
        let preset = theme::HIT_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, ring);
    }

    /// 周回達成の瞬間の演出。
    pub fn push_lap_complete(&mut self, ring: Rect) {
        let preset = theme::ADVANCE_FLASH;
        let effect = fx::sweep_in(Motion::UpToDown, 10, 2, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, ring);
    }

    /// 自己ベスト更新の瞬間の演出。通常の周回達成より豪華にして差別化する。
    pub fn push_best_lap_achievement(&mut self, area: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::LeftToRight, 10, 3, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 死亡し拠点へ撤退した瞬間の演出。
    pub fn push_death(&mut self, area: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::sweep_in(Motion::DownToUp, 8, 6, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.host.process(elapsed, buf, area);
    }

    /// 現在進行中の演出があるか。
    pub fn is_running(&self) -> bool {
        self.host.is_running()
    }
}

impl Default for LoopMarchEffects {
    fn default() -> Self {
        Self::new()
    }
}
