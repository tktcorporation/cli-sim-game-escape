//! 演出レイヤー (共通 `crate::effects::EffectHost` の薄いラッパー)。
//!
//! ratatui 描画後の Buffer に shader 風の post-process を当てる。
//! state.rs / logic.rs を一切変更せず、AbyssGame::render の最後に
//! `process(...)` を呼ぶだけで動く設計。
//!
//! ## なぜ render 内で push するか
//! Effect は area (Rect) を必要とし、area は render 時にしか確定しない。
//! なので state 差分検知 (例: `prev_floor != state.floor`) と effect push
//! の両方を render 冒頭でまとめて行う。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::style::Color;
use tachyonfx::fx::{self, Glitch, RepeatMode};
// NOTE: `Duration` は tachyonfx の独自型を使う (std-duration feature は wasm と
// 排他のため有効化できない)。API は std::time::Duration と同じ from_millis 系を持つ。
use tachyonfx::{Duration, IntoEffect, Motion, SimpleRng};

use crate::effects::EffectHost;
use crate::theme;

/// AbyssGame が保持する演出マネージャ。
///
/// 各 push_* メソッドは 1 つの効果シナリオに対応する。新しい演出を増やす時は
/// 新しい push メソッドを追加して mod.rs の `detect_transitions` から呼ぶ。
pub struct AbyssEffects {
    host: EffectHost,
}

impl AbyssEffects {
    pub fn new() -> Self {
        Self { host: EffectHost::new() }
    }

    // ── 階層遷移系 ──────────────────────────────────────────

    /// **下に潜る** (フロア +1) 演出。ボス撃破などで起きる前進方向。
    /// area は画面全体 (上から流し込みたいので戦闘パネルだけだと弱い)。
    pub fn push_descend(&mut self, area: Rect) {
        let preset = theme::ADVANCE_FLASH;
        let effect = fx::sweep_in(
            Motion::UpToDown,
            14,                       // gradient_length: 大きいほど波が長い
            0,                        // randomness: 0 = 綺麗な水平線
            preset.color,
            Duration::from_millis(preset.duration_ms),
        );
        self.host.push(effect, area);
    }

    /// **上に戻される** (フロア減少) 演出。撤退・死亡など後退方向。
    /// 違いを出すため: 下から上に逆方向、色は赤系、duration はやや長めで「巻き戻し」感。
    pub fn push_ascend_or_death(&mut self, area: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::sweep_in(
            Motion::DownToUp,
            8,                        // gradient_length: 短くして「ザッ」と荒い
            6,                        // randomness: ノイズ混じりで不穏に
            preset.color,
            Duration::from_millis(preset.duration_ms),
        );
        self.host.push(effect, area);
    }

    // ── 戦闘フィードバック系 ─────────────────────────────────

    /// 敵が被弾した瞬間の演出。area は **enemy_panel_rect だけ** を渡す。
    ///
    /// fade_from_fg: 指定色から元の前景色へフェードイン。被弾の「ピカッ」感。
    /// 短い duration で連打にも耐える。重ねがけ前提なので key 管理なし。
    pub fn push_enemy_hit(&mut self, enemy_panel: Rect) {
        let preset = theme::HIT_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, enemy_panel);
    }

    /// 勇者が被弾した瞬間の演出 (敵被弾の対称、色を赤に)。
    /// duration を少し長めにして「痛い」感を出す。
    pub fn push_hero_hit(&mut self, hero_panel: Rect) {
        let preset = theme::DAMAGE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, hero_panel);
    }

    /// クリティカル演出。戦闘エリア全体を一瞬グリッチさせる。
    ///
    /// Glitch::builder で「揺らぎ範囲」と「対象セル比率」を細かく指定。
    /// fx::with_duration で全体時間を制限しないと glitch は無限に続く。
    pub fn push_critical(&mut self, combat: Rect) {
        let glitch = Glitch::builder()
            .rng(SimpleRng::default())
            .action_ms(30..120)               // 各セルが「化ける」時間
            .action_start_delay_ms(0..80)     // セル毎の発動オフセット
            .cell_glitch_ratio(0.45)          // 戦闘領域の 45% が化ける
            .build()
            .into_effect();
        let effect = fx::with_duration(Duration::from_millis(280), glitch);
        self.host.push(effect, combat);
    }

    /// ボス出現演出。戦闘エリアに左から右へオレンジ色のスウィープ。
    pub fn push_boss_appearance(&mut self, combat: Rect) {
        let effect = fx::sweep_in(
            Motion::LeftToRight,
            12,
            2,                               // 少しだけノイズ
            Color::Indexed(208),             // オレンジ (警戒色)
            Duration::from_millis(700),
        );
        self.host.push(effect, combat);
    }

    /// ボス撃破演出。敵パネルが dissolve で散って消える。
    /// dissolve: ランダムなセルから順に背景色化していく。
    pub fn push_boss_defeated(&mut self, enemy_panel: Rect) {
        let effect = fx::dissolve(Duration::from_millis(550));
        self.host.push(effect, enemy_panel);
    }

    // ── ガチャ系 ────────────────────────────────────────────

    /// Legendary 当選演出。タブコンテンツ領域に
    /// 1) coalesce で文字を組み上げる (出現)
    /// 2) hsl_shift_fg を 2 周回して虹色循環
    ///    を順番に流す。豪華さの演出。
    pub fn push_gacha_legendary(&mut self, body: Rect) {
        let coalesce_in = fx::coalesce(Duration::from_millis(700));
        let rainbow_loop = fx::repeat(
            // [360°, 0%, 0%] = 色相だけ 1 周
            fx::hsl_shift_fg([360.0, 0.0, 0.0], Duration::from_millis(900)),
            RepeatMode::Times(2),
        );
        let effect = fx::sequence(&[coalesce_in, rainbow_loop]);
        self.host.push(effect, body);
    }

    // ── 装備系 ──────────────────────────────────────────────

    /// 装備解放演出。タブコンテンツ領域を黄色フェードで包み、所有感を強調する。
    /// duration はガチャ Legendary より控えめだが、強化購入のログ流れより
    /// 確実に視認できる長さ。
    pub fn push_equipment_unlock(&mut self, body: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(
            Motion::LeftToRight,
            10,
            3,
            preset.color,
            Duration::from_millis(preset.duration_ms),
        );
        self.host.push(effect, body);
    }

    // ── 共通 ────────────────────────────────────────────────

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    /// `elapsed` は前回 render からの wall-clock 差分。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.host.process(elapsed, buf, area);
    }
}
