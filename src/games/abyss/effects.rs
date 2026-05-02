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

    // ── 階層遷移系 ──────────────────────────────────────────

    /// **下に潜る** (フロア +1) 演出。ボス撃破などで起きる前進方向。
    /// area は画面全体 (上から流し込みたいので戦闘パネルだけだと弱い)。
    pub fn push_descend(&mut self, area: Rect) {
        // 上から下にスウィープ。色は深い藍色 → 通常表示へ復帰。
        // 「深淵に落ちていく」イメージなので gradient_length を長めにして滑らかに。
        let mut effect = fx::sweep_in(
            Motion::UpToDown,
            14,                       // gradient_length: 大きいほど波が長い
            0,                        // randomness: 0 = 綺麗な水平線
            Color::Indexed(17),       // 深い藍 (xterm 256 color)
            Duration::from_millis(450),
        );
        effect.set_area(area);
        self.manager.add_effect(effect);
    }

    /// **上に戻される** (フロア減少) 演出。撤退・死亡など後退方向。
    /// 違いを出すため: 下から上に逆方向、色は赤系、duration はやや長めで「巻き戻し」感。
    pub fn push_ascend_or_death(&mut self, area: Rect) {
        let mut effect = fx::sweep_in(
            Motion::DownToUp,
            8,                        // gradient_length: 短くして「ザッ」と荒い
            6,                        // randomness: ノイズ混じりで不穏に
            Color::Indexed(52),       // 暗い赤
            Duration::from_millis(650),
        );
        effect.set_area(area);
        self.manager.add_effect(effect);
    }

    // ── 戦闘フィードバック系 ─────────────────────────────────

    /// 敵が被弾した瞬間の演出。area は **enemy_panel_rect だけ** を渡す。
    ///
    /// fade_from_fg: 指定色から元の前景色へフェードイン。被弾の「ピカッ」感。
    /// 短い duration (100ms) で連打にも耐える。重ねがけ前提なので key 管理なし。
    pub fn push_enemy_hit(&mut self, enemy_panel: Rect) {
        let mut effect = fx::fade_from_fg(
            Color::Yellow,
            Duration::from_millis(120),
        );
        effect.set_area(enemy_panel);
        self.manager.add_effect(effect);
    }

    // ── 共通 ────────────────────────────────────────────────

    /// 1 フレーム分の経過時間を進めて、Buffer に effect を適用する。
    /// `elapsed` は前回 render からの wall-clock 差分。
    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.manager.process_effects(elapsed, buf, area);
    }
}
