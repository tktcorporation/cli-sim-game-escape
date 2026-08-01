//! ゲーム横断の演出基盤 (tachyonfx ラッパー)。
//!
//! ratatui 描画後の `Buffer` に shader 風の post-process を当てる仕組みを
//! 共通化する。各ゲームはこれをそのまま使うか、`EffectHost` をフィールドに
//! 持つ薄いラッパー (例: `games::abyss::effects::AbyssEffects`) を作り、
//! シナリオ別の `push_*` メソッドから演出を積む。
//!
//! ## なぜ render 内で push するか
//! Effect は area (Rect) を必要とし、area は render 時にしか確定しない。
//! なので state 差分検知 (例: `prev_floor != state.floor`) と effect push
//! の両方を render 冒頭でまとめて行う設計になる (各ゲームの `detect_transitions`
//! 相当のメソッドを参照)。

use std::cell::Cell;

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use tachyonfx::{Duration, Effect, EffectManager};

/// tachyonfx の `EffectManager` を保持し、`Buffer` への演出適用を担う。
///
/// ゲーム固有の `push_*` シナリオメソッドは、effect を組み立てて
/// [`EffectHost::push`] に渡すだけでよい。
pub struct EffectHost {
    manager: EffectManager<()>,
}

impl EffectHost {
    pub fn new() -> Self {
        Self { manager: EffectManager::default() }
    }

    /// 指定領域に演出を 1 つ積む。
    pub fn push(&mut self, mut effect: Effect, area: Rect) {
        effect.set_area(area);
        self.manager.add_effect(effect);
    }

    /// 1 フレーム分の経過時間を進めて、`Buffer` に演出を適用する。
    /// `elapsed` は前回 render からの wall-clock 差分 ([`FrameClock::elapsed`])。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed, buf, area);
    }
}

impl Default for EffectHost {
    fn default() -> Self {
        Self::new()
    }
}

/// `render()` 呼び出し間の wall-clock 経過時間を計測する。
///
/// ゲームロジックの tick は固定 10/sec だが、`render()` はブラウザの実際の
/// フレームレートで呼ばれるため、演出を滑らかに進めるにはこの実時間差分が要る。
pub struct FrameClock {
    last_render_ms: Cell<f64>,
}

impl FrameClock {
    pub fn new() -> Self {
        Self { last_render_ms: Cell::new(0.0) }
    }

    /// 前回の呼び出しからの経過時間。初回呼び出しは基準点が無いので `Duration::ZERO`。
    /// ブラウザ外 (native cargo test) では wall clock が取れないため常に `Duration::ZERO`。
    pub fn elapsed(&self) -> Duration {
        let now = now_ms().unwrap_or(0.0);
        let prev = self.last_render_ms.get();
        self.last_render_ms.set(now);
        if prev == 0.0 {
            Duration::ZERO
        } else {
            let delta_ms = (now - prev).clamp(0.0, 100.0);
            if !delta_ms.is_finite() {
                return Duration::ZERO;
            }
            Duration::from_millis(delta_ms as u32)
        }
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> Option<f64> {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now())
}

/// 一時的な視覚フラッシュ (被弾時に名前を赤くする等) の残り tick 数。
///
/// `logic::tick` から毎フレーム [`FlashTimer::tick`] を呼んでカウントダウンし、
/// `render` 側は [`FlashTimer::is_active`] を見て色を切り替える。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlashTimer(u32);

impl FlashTimer {
    pub const fn new() -> Self {
        Self(0)
    }

    /// `ticks` 分のフラッシュを (再) 開始する。既存のカウントは上書きする。
    pub fn trigger(&mut self, ticks: u32) {
        self.0 = ticks;
    }

    /// 論理フレーム分カウントダウンする。
    pub fn tick(&mut self, delta_ticks: u32) {
        self.0 = self.0.saturating_sub(delta_ticks);
    }

    pub fn is_active(&self) -> bool {
        self.0 > 0
    }

    pub fn ticks_left(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_timer_starts_inactive() {
        let t = FlashTimer::new();
        assert!(!t.is_active());
        assert_eq!(t.ticks_left(), 0);
    }

    #[test]
    fn flash_timer_active_after_trigger() {
        let mut t = FlashTimer::new();
        t.trigger(3);
        assert!(t.is_active());
        assert_eq!(t.ticks_left(), 3);
    }

    #[test]
    fn flash_timer_counts_down_and_deactivates() {
        let mut t = FlashTimer::new();
        t.trigger(3);
        t.tick(2);
        assert!(t.is_active());
        assert_eq!(t.ticks_left(), 1);
        t.tick(1);
        assert!(!t.is_active());
    }

    #[test]
    fn flash_timer_tick_never_underflows() {
        let mut t = FlashTimer::new();
        t.trigger(2);
        t.tick(10);
        assert!(!t.is_active());
        assert_eq!(t.ticks_left(), 0);
    }

    #[test]
    fn flash_timer_trigger_overwrites_existing_countdown() {
        let mut t = FlashTimer::new();
        t.trigger(5);
        t.tick(3);
        assert_eq!(t.ticks_left(), 2);
        t.trigger(10);
        assert_eq!(t.ticks_left(), 10);
    }

    // FrameClock::elapsed() は web_sys::window() を経由するため、native
    // (cargo test) 上で呼ぶと wasm-bindgen が "cannot access imported statics
    // on non-wasm targets" で panic する。ブラウザ (wasm32) からのみ到達可能な
    // 経路であり、Game::render() を経由しないユニットテストではこの経路に
    // 到達しない。ここでは意図的にテストを書かない。
}
