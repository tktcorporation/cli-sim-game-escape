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

    /// 現在進行中の演出があるか。
    pub fn is_running(&self) -> bool {
        self.manager.is_running()
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
///
/// [`crate::time::GameTime`] と同じく、タイムスタンプは呼び出し側が注入する
/// (内部で `web_sys` を呼ばない)。これにより native (cargo test) でも
/// wasm 依存なしに `elapsed` の計算ロジックを検証できる。
pub struct FrameClock {
    last_render_ms: Cell<Option<f64>>,
}

impl FrameClock {
    pub fn new() -> Self {
        Self { last_render_ms: Cell::new(None) }
    }

    /// 前回の呼び出しからの経過時間。`now_ms` は [`crate::time::now_ms`] 等で
    /// 呼び出し側が計測した wall-clock タイムスタンプ。初回呼び出しは基準点が
    /// 無いので `Duration::ZERO`。
    pub fn elapsed(&self, now_ms: f64) -> Duration {
        let prev = self.last_render_ms.replace(Some(now_ms));
        let Some(prev) = prev else {
            return Duration::ZERO;
        };
        let delta_ms = (now_ms - prev).clamp(0.0, 100.0);
        if !delta_ms.is_finite() {
            return Duration::ZERO;
        }
        Duration::from_millis(delta_ms as u32)
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
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

    #[test]
    fn frame_clock_first_call_returns_zero() {
        let clock = FrameClock::new();
        assert_eq!(clock.elapsed(1_000.0), Duration::ZERO);
    }

    #[test]
    fn frame_clock_computes_delta_between_calls() {
        let clock = FrameClock::new();
        clock.elapsed(1_000.0);
        assert_eq!(clock.elapsed(1_050.0), Duration::from_millis(50));
    }

    #[test]
    fn frame_clock_clamps_large_gaps_to_100ms() {
        let clock = FrameClock::new();
        clock.elapsed(0.0);
        // タブが非アクティブ化された後の復帰等、大きな gap は tachyonfx
        // の演出が一気に完了しないよう 100ms に丸める。
        assert_eq!(clock.elapsed(10_000.0), Duration::from_millis(100));
    }

    #[test]
    fn frame_clock_never_returns_negative_delta() {
        let clock = FrameClock::new();
        clock.elapsed(1_000.0);
        // タイムスタンプが逆行する (通常は起きないが) ケースでも 0 に丸める。
        assert_eq!(clock.elapsed(900.0), Duration::ZERO);
    }

    #[test]
    fn effect_host_reports_running_between_push_and_completion() {
        use ratzilla::ratatui::style::Color;
        use tachyonfx::fx;

        let area = Rect::new(0, 0, 10, 3);
        let mut host = EffectHost::new();
        assert!(!host.is_running(), "push 前は running ではない");

        host.push(fx::fade_from_fg(Color::Red, Duration::from_millis(100)), area);
        assert!(host.is_running(), "push した直後は running");

        let mut buf = Buffer::empty(area);
        host.process(Duration::from_millis(50), &mut buf, area);
        assert!(host.is_running(), "duration 未満の経過では running のまま");

        host.process(Duration::from_millis(100), &mut buf, area);
        assert!(!host.is_running(), "duration を超えた経過で完了し running でなくなる");
    }
}
