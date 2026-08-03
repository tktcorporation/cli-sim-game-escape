//! 常夜灯 — 演出 (tachyonfx ラッパー)。
//!
//! `crate::effects::EffectHost` に、このゲーム固有の「何が起きたら
//! どう光らせるか」だけを積む薄いラッパー。render.rs の
//! `detect_transitions` から呼ばれる。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::layout::Rect;
use tachyonfx::fx::{self, Glitch};
use tachyonfx::{Duration, IntoEffect, Motion, SimpleRng};

use crate::effects::EffectHost;
use crate::theme;

pub struct EverlightEffects {
    host: EffectHost,
}

impl EverlightEffects {
    pub fn new() -> Self {
        Self { host: EffectHost::new() }
    }

    /// 灯がダメージを受けた瞬間 (漏れ・ボスの一撃どちらも共通)。
    pub fn push_light_hit(&mut self, header: Rect) {
        let preset = theme::DAMAGE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, header);
    }

    /// 敵が防衛線を突破した瞬間、戦場側にも軽い警告フラッシュを重ねる。
    pub fn push_breach(&mut self, battlefield: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, battlefield);
    }

    /// 宝箱を捕まえた瞬間。
    pub fn push_chest_caught(&mut self, battlefield: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::UpToDown, 10, 2, preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, battlefield);
    }

    /// 波が進んだ瞬間。
    pub fn push_wave_advance(&mut self, header: Rect) {
        let preset = theme::ADVANCE_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, header);
    }

    /// 魔王が出現した瞬間。威圧感のあるグリッチで異物感を出す。
    pub fn push_boss_appear(&mut self, battlefield: Rect) {
        let glitch = Glitch::builder()
            .rng(SimpleRng::default())
            .action_ms(30..120)
            .action_start_delay_ms(0..80)
            .cell_glitch_ratio(0.3)
            .build()
            .into_effect();
        let effect = fx::with_duration(Duration::from_millis(280), glitch);
        self.host.push(effect, battlefield);
    }

    /// Dawn (夜のマイルストーン最終ボス撃破) を達成した瞬間。他の演出より
    /// 長く・画面全体に金色のsweepをかけて「大きな節目」だと分からせる。
    pub fn push_dawn(&mut self, area: Rect) {
        let preset = theme::ACHIEVEMENT_FLASH;
        let effect = fx::sweep_in(Motion::UpToDown, 14, 4, preset.color, Duration::from_millis(900));
        self.host.push(effect, area);
    }

    /// 夜番が終わった瞬間 (灯が消えた/自ら撤退した)。
    pub fn push_vigil_end(&mut self, area: Rect) {
        let preset = theme::SETBACK_FLASH;
        let effect = fx::fade_from_fg(preset.color, Duration::from_millis(preset.duration_ms));
        self.host.push(effect, area);
    }

    pub fn process(&mut self, elapsed: Duration, buf: &mut Buffer, area: Rect) {
        self.host.process(elapsed, buf, area);
    }

    pub fn is_running(&self) -> bool {
        self.host.is_running()
    }
}

impl Default for EverlightEffects {
    fn default() -> Self {
        Self::new()
    }
}
