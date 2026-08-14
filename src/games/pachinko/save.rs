//! 玉響 セーブ/ロード。
//!
//! 永続対象は「来店をまたいで持ち越すもの」だけに絞る。
//!
//! - `record` — 自己記録。来店の外側にある値。
//! - `cash` / `balls_held` — 財布と持ち玉。台を替えても持ち歩くもの。
//! - `power` — ハンドル強度。プレイヤーの好みの設定なので引き継ぐ。
//! - `rng_state` — 保存しないとリロードのたびに同じ乱数列を再生し、
//!   出目も釘の振り分けも毎回同じ並びになる。
//!
//! 保存しないもの:
//!
//! - `machines` (釘配置) — 来店ごとにホールの並びが変わる方が「今日はどの台が
//!   回るか」を毎回読む体験になる。読み込み後に `logic::generate_hall` が作り直す。
//! - `balls` / `digit` / `pending` / `mode` / `history` / `chain` — 台に着いている
//!   間だけの一時状態。席を立てば消えるものをリロードで残さない。
//! - `invested` — この来店での投資額。来店ごとに 0 から数え直す
//!   (累計は `record.total_invested` が持つ)。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{PachinkoState, Record};

/// セーブデータのフォーマットバージョン。フィールド追加時にインクリメントすること。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

/// 互換性を維持できる最小バージョン。これ未満のセーブは破棄する。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "pachinko_save_v1";

/// オートセーブの間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
pub const AUTOSAVE_INTERVAL: u32 = 300;

/// `rng_state` が 0 で読み込まれたときの差し替え値。xorshift32 は 0 が
/// 不動点で、そのまま使うと乱数が固定値のまま止まる。
#[cfg(any(target_arch = "wasm32", test))]
const RNG_FALLBACK_SEED: u32 = 0xDEAD_BEEF;

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    best_balls: u32,
    total_jackpots: u32,
    best_chain: u32,
    total_invested: u32,
    total_returned: u32,
    cash: u32,
    balls_held: u32,
    /// ハンドル強度 0〜100。手書き編集や範囲外の値が来ても打ち出しが壊れないよう
    /// `apply_save` 側でクランプする。
    power: u8,
    rng_state: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &PachinkoState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            best_balls: state.record.best_balls,
            total_jackpots: state.record.total_jackpots,
            best_chain: state.record.best_chain,
            total_invested: state.record.total_invested,
            total_returned: state.record.total_returned,
            cash: state.cash,
            balls_held: state.balls_held,
            power: state.power,
            rng_state: state.rng_state,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut PachinkoState, save: &GameSave) {
    state.record = Record {
        best_balls: save.best_balls,
        total_jackpots: save.total_jackpots,
        best_chain: save.best_chain,
        total_invested: save.total_invested,
        total_returned: save.total_returned,
    };
    state.cash = save.cash;
    state.balls_held = save.balls_held;
    state.power = save.power.min(100);
    state.rng_state = if save.rng_state == 0 {
        RNG_FALLBACK_SEED
    } else {
        save.rng_state
    };
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &PachinkoState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("玉響: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(&format!("玉響: localStorage への保存に失敗: {e:?}").into());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut PachinkoState) -> bool {
    let storage = match get_storage() {
        Some(s) => s,
        None => return false,
    };
    let json = match storage.get_item(STORAGE_KEY) {
        Ok(Some(j)) => j,
        _ => return false,
    };
    let save_data: SaveData = match serde_json::from_str(&json) {
        Ok(d) => d,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("玉響: セーブデータのパースに失敗（破棄します）: {e}").into(),
            );
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };
    if save_data.version < MIN_COMPATIBLE_VERSION {
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    apply_save(state, &save_data.game);
    true
}

#[cfg(target_arch = "wasm32")]
pub fn delete_save() {
    if let Some(storage) = get_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::pachinko::state::{Digit, InfoTab, Mode, Phase};

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = PachinkoState::new();
        original.record = Record {
            best_balls: 4200,
            total_jackpots: 17,
            best_chain: 6,
            total_invested: 32_000,
            total_returned: 51_500,
        };
        original.cash = 7_000;
        original.balls_held = 830;
        original.power = 74;
        original.rng_state = 123_456;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = PachinkoState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.record, original.record);
        assert_eq!(restored.cash, 7_000);
        assert_eq!(restored.balls_held, 830);
        assert_eq!(restored.power, 74, "ハンドル強度は好みの設定なので引き継ぐ");
        assert_eq!(
            restored.rng_state, 123_456,
            "rng_state を保存しないとリロードのたびに同じ乱数列を再生してしまう"
        );
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = PachinkoState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = PachinkoState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.record, Record::default());
        assert_eq!(restored.cash, state.cash);
        assert_eq!(restored.balls_held, 0);
        assert_eq!(restored.rng_state, state.rng_state);
    }

    #[test]
    fn zero_rng_state_is_replaced() {
        // xorshift32 の不動点をそのまま読み込むと、以降の抽選・釘生成が
        // 同じ値を返し続ける。
        let save = GameSave {
            rng_state: 0,
            ..GameSave::default()
        };
        let mut restored = PachinkoState::new();
        apply_save(&mut restored, &save);

        assert_ne!(restored.rng_state, 0);
    }

    #[test]
    fn power_out_of_range_is_clamped() {
        let save = GameSave {
            power: 200,
            ..GameSave::default()
        };
        let mut restored = PachinkoState::new();
        apply_save(&mut restored, &save);

        assert_eq!(restored.power, 100);
    }

    #[test]
    fn transient_play_state_is_untouched() {
        // 遊技中の一時状態を復元すると、席を立って戻ったときと
        // リロードしたときで挙動が食い違う。
        let mut restored = PachinkoState::new();
        crate::games::pachinko::logic::generate_hall(&mut restored);
        let machines_before = restored.machines.len();
        let nails_before = restored.machines[0].nails.clone();
        restored.phase = Phase::Playing;
        restored.mode = Mode::Jitan { spins_left: 30 };
        restored.chain = 3;
        restored.invested = 5_000;
        restored.tab = InfoTab::History;

        apply_save(
            &mut restored,
            &GameSave {
                cash: 2_000,
                ..GameSave::default()
            },
        );

        assert_eq!(restored.machines.len(), machines_before);
        assert_eq!(restored.machines[0].nails, nails_before);
        assert!(restored.balls.is_empty());
        assert_eq!(restored.digit, Digit::Idle);
        assert!(restored.pending.is_empty());
        assert_eq!(restored.mode, Mode::Jitan { spins_left: 30 });
        assert_eq!(restored.chain, 3);
        assert_eq!(restored.invested, 5_000);
        assert_eq!(restored.phase, Phase::Playing);
        assert_eq!(restored.tab, InfoTab::History);
        assert_eq!(restored.cash, 2_000);
    }

    #[test]
    fn save_without_new_fields_loads_with_defaults() {
        // フィールドが欠けたセーブ (フォーマット拡張の前後をまたぐ読み込み) でも
        // panic せず、欠けた分はデフォルト値として扱えることを保証する。
        let json = r#"{"version":1,"game":{"cash":3000,"total_jackpots":2}}"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();

        let mut restored = PachinkoState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.cash, 3_000);
        assert_eq!(restored.record.total_jackpots, 2);
        assert_eq!(restored.record.best_balls, 0);
        assert_eq!(restored.balls_held, 0);
        assert_ne!(restored.rng_state, 0, "欠けた rng_state は不動点にしない");
    }

    #[test]
    fn save_version_is_compatible() {
        const { assert!(SAVE_VERSION >= MIN_COMPATIBLE_VERSION) };
    }
}
