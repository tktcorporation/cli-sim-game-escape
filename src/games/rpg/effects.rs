//! 演出レイヤー (共通 `crate::effects::EffectHost` の薄いラッパー)。
//!
//! ratatui 描画後の Buffer に shader 風の post-process を当てる。ここでの
//! 演出 (push_* メソッド) を増やす分には state.rs / logic.rs の変更は不要で、
//! RpgGame::render の最後に `process(...)` を呼ぶだけで動く設計 (被弾フラッシュ
//! の残り時間自体は state.rs 側で `crate::effects::FlashTimer` を使って管理
//! しており、これとは別レイヤー)。abyss/effects.rs と同じ構成。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use tachyonfx::fx::{self, Glitch};
use tachyonfx::{Duration, IntoEffect, Motion, SimpleRng};

use crate::effects::EffectHost;
use crate::theme;

pub struct RpgEffects {
    host: EffectHost,
}

impl RpgEffects {
    pub fn new() -> Self {
        Self { host: EffectHost::new() }
    }

    // ── 戦闘フィードバック系 ─────────────────────────────────

    /// 勇者が被弾した瞬間の演出。area はステータスバー全体。
    pub fn push_hero_hit(&mut self, status_bar: Rect) {
        let preset = theme::DAMAGE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, status_bar);
    }

    /// 敵が被弾した瞬間の演出。area はダンジョン画面全体
    /// (グリッド上のどのマスの敵かまでは追わない、abyssの enemy_panel と同じ粒度)。
    pub fn push_enemy_hit(&mut self, scene: Rect) {
        let preset = theme::HIT_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, scene);
    }

    /// クリティカルヒット演出。ダンジョン画面全体を一瞬グリッチさせる。
    pub fn push_critical(&mut self, scene: Rect) {
        let glitch = Glitch::builder()
            .rng(SimpleRng::default())
            .action_ms(30..120)
            .action_start_delay_ms(0..80)
            .cell_glitch_ratio(0.35)
            .build()
            .into_effect();
        let effect = fx::with_duration(Duration::from_millis(260), glitch);
        self.host.push(effect, scene);
    }

    /// 敵がチャージ攻撃を溜め始めた瞬間の予告演出。「今シールドを使うべきか」
    /// の読み合いを見落とさせないよう、通常の被弾より目立つ警戒色で
    /// ダンジョン画面全体を強めに煽る。
    pub fn push_charge_warning(&mut self, scene: Rect) {
        let effect = fx::sweep_in(
            Motion::LeftToRight,
            10,
            4,
            ratzilla::ratatui::style::Color::Indexed(208), // オレンジ (警戒色、abyssのボス出現と同色)
            Duration::from_millis(500),
        );
        self.host.push(effect, scene);
    }

    /// 敵の弱点を発見した瞬間の演出。
    pub fn push_weakness_discovered(&mut self, scene: Rect) {
        let preset = theme::ADVANCE_FLASH;
        let effect = fx::coalesce(Duration::from_millis(preset.duration_ms));
        self.host.push(effect, scene);
    }

    // ── 階層遷移系 ──────────────────────────────────────────

    /// フロア到達 (階層 +1) 演出。
    pub fn push_descend(&mut self, area: Rect) {
        let preset = theme::ADVANCE_FLASH;
        let effect = fx::sweep_in(Motion::UpToDown, 14, 0, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 階段で1つ浅い階層へ戻る演出。村へ帰還する退却とは違い、道中の
    /// 単なる後戻りなので push_descend より短く控えめにする。他の階層遷移
    /// 演出と違い効果音は意図的に付けない (頻度が高いわりに感情的な重みが
    /// 軽い操作のため、視覚のみの控えめなフィードバックに留める)。
    pub fn push_ascend(&mut self, area: Rect) {
        let effect = fx::sweep_in(
            Motion::DownToUp,
            14,
            0,
            ratzilla::ratatui::style::Color::DarkGray,
            Duration::from_millis(300),
        );
        self.host.push(effect, area);
    }

    /// 帰還 (村へ生還) 演出。撤退そのものは前向きな結果 (帰還ボーナス) なので、
    /// 死亡の SETBACK_FLASH とは色・方向を変えて区別する。
    pub fn push_return_to_town(&mut self, area: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::LeftToRight, 12, 2, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 死亡して村へ撤退させられた演出。
    pub fn push_death(&mut self, area: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::sweep_in(Motion::DownToUp, 8, 6, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 最深到達フロアの自己ベスト更新演出。
    pub fn push_new_record(&mut self, area: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::LeftToRight, 10, 3, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    /// 魔王撃破 (ゲームクリア) 演出。detect_transitions → render → process の
    /// 順で呼ばれるため、この時点で Buffer には既に新しい GameClear 画面が
    /// 描画済み。dissolve (消えていく演出) を使うと「今描画されたクリア画面」
    /// が一瞬消えてから元に戻るという逆効果になるため、coalesce (組み上がって
    /// 現れる演出) でクリア画面自体が出現するように見せる。
    pub fn push_boss_defeated(&mut self, scene: Rect) {
        let effect = fx::coalesce(Duration::from_millis(600));
        self.host.push(effect, scene);
    }

    // ── 成長系 ──────────────────────────────────────────────

    /// レベルアップ演出。ステータスバーを黄色フェードで包む。
    pub fn push_level_up(&mut self, status_bar: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::LeftToRight, 10, 3, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, status_bar);
    }

    // ── 共通 ────────────────────────────────────────────────

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.host.process(elapsed, buf, area);
    }

    /// 現在進行中の演出があるか。
    pub fn is_running(&self) -> bool {
        self.host.is_running()
    }
}

impl Default for RpgEffects {
    fn default() -> Self {
        Self::new()
    }
}
