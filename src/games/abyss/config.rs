//! 深淵潜行 — 難易度バランス設定 (DI 用)。
//!
//! 数値定数をここに集約することで、本体ゲームとシミュレータで同じ
//! `logic::tick` を共有しつつ、難易度だけを差し替えられるようにする。
//!
//! 値を変えても挙動は `logic.rs` の式によって厳密に決まる ─ つまり
//! sim で観測した結果は本体ゲームでも完全に再現される。
//!
//! 既定値は本体ゲームの現在のバランスを表す (リファクタ前後で挙動不変)。

/// ヒーローの基礎値とアップグレード効果。
#[derive(Clone, Debug)]
pub struct HeroConfig {
    pub base_hp: u64,
    pub base_atk: u64,
    pub base_def: u64,
    /// 1 攻撃あたりの基礎 tick 数 (Speed 強化で短縮)。
    pub atk_period_base: u32,
    /// 攻撃間隔の下限 tick (これより短くしない)。
    pub atk_period_min: u32,

    // upgrade per-level deltas
    pub atk_per_sword_lv: u64,
    pub hp_per_vitality_lv: u64,
    pub def_per_armor_lv: u64,
    pub crit_per_lv: f64,
    pub crit_cap: f64,
    pub speed_per_lv: f64,
    pub regen_per_lv_per_sec: f64,
    pub gold_per_lv: f64,

    // soul perk multipliers
    pub might_per_lv: f64,
    pub endurance_per_lv: f64,
    pub fortune_per_lv: f64,
    pub reaper_per_lv: f64,
}

/// 敵のスケーリングパラメータ。
#[derive(Clone, Debug)]
pub struct EnemyConfig {
    pub hp_base: f64,
    pub hp_growth: f64,
    pub atk_base: f64,
    pub atk_growth: f64,
    pub def_base: f64,
    pub def_per_floor: f64,
    pub gold_base: f64,
    pub gold_growth: f64,

    pub boss_hp_mult: f64,
    pub boss_atk_mult: f64,
    pub boss_def_mult: f64,
    pub boss_gold_mult: f64,

    pub normal_atk_period: u32,
    pub boss_atk_period: u32,
}

/// フロア進行と魂報酬の設定。
#[derive(Clone, Debug)]
pub struct PacingConfig {
    pub enemies_per_floor: u32,
    /// 雑魚撃破時、`floor / normal_souls_div` 切り上げが基礎魂量。
    pub normal_souls_div: u32,
    /// ボス撃破時、`floor * boss_souls_mult` が基礎魂量。
    pub boss_souls_mult: u64,
    /// 死亡時、`floor * death_souls_mult` が基礎魂量。
    pub death_souls_mult: u64,
}

/// ガチャ・鍵ドロップ・フロア種別抽選の設定。
#[derive(Clone, Debug)]
pub struct GachaConfig {
    /// ボス撃破で必ずもらえる基本鍵数。
    pub keys_per_boss: u64,
    /// `floor % deep_floor_step == 0` の階層 (例: 10F毎) のボーナス鍵数。
    pub deep_floor_step: u32,
    pub deep_floor_bonus_keys: u64,

    /// フロア種別の出現確率 (sum != 1 でも内部で正規化)。
    /// `[Normal, Treasure, Elite, Bonanza]` の順。
    pub floor_kind_weights: [u32; 4],
    /// 何階以下を必ず Normal にするか (序盤の把握しやすさ)。
    pub floor_kind_normal_below: u32,

    /// ガチャ tier 確率 (千分率)。`[Common, Rare, Epic, Legendary]`。合計 1000。
    pub gacha_weights_milli: [u32; 4],
    /// 何回引いて Epic+ が出なければ天井で Epic+ 確定にするか (0 で天井無効)。
    pub gacha_pity: u32,

    /// Common ヒット時、`現フロアの基礎雑魚 gold * gain_min..gain_max` を獲得。
    pub common_gold_mult_min: u32,
    pub common_gold_mult_max: u32,
    /// Epic ヒット時の魂量 = `floor * epic_souls_mult` (soul_multiplier 適用後)。
    pub epic_souls_mult: u64,
    /// Legendary ヒット時の鍵数。
    pub legendary_keys: u64,
}

/// 難易度バランスの集約。state に一個保持する。
#[derive(Clone, Debug)]
pub struct BalanceConfig {
    pub hero: HeroConfig,
    pub enemy: EnemyConfig,
    pub pacing: PacingConfig,
    pub gacha: GachaConfig,
}

impl Default for BalanceConfig {
    /// 本体ゲームの既定難易度。値は変えるとゲームバランスが変わる。
    fn default() -> Self {
        Self {
            hero: HeroConfig {
                base_hp: 50,
                base_atk: 5,
                base_def: 2,
                atk_period_base: 12,
                atk_period_min: 3,

                atk_per_sword_lv: 2,
                hp_per_vitality_lv: 10,
                def_per_armor_lv: 1,
                crit_per_lv: 0.01,
                crit_cap: 0.60,
                speed_per_lv: 0.05,
                regen_per_lv_per_sec: 0.2,
                gold_per_lv: 0.05,

                might_per_lv: 0.05,
                endurance_per_lv: 0.05,
                fortune_per_lv: 0.10,
                reaper_per_lv: 0.20,
            },
            enemy: EnemyConfig {
                hp_base: 14.0,
                hp_growth: 1.32,
                atk_base: 4.0,
                atk_growth: 1.22,
                def_base: 1.0,
                def_per_floor: 0.5,
                gold_base: 4.0,
                gold_growth: 1.40,

                boss_hp_mult: 5.0,
                boss_atk_mult: 1.5,
                boss_def_mult: 1.6,
                boss_gold_mult: 8.0,

                normal_atk_period: 18,
                boss_atk_period: 14,
            },
            pacing: PacingConfig {
                enemies_per_floor: 8,
                normal_souls_div: 5,
                boss_souls_mult: 2,
                death_souls_mult: 3,
            },
            gacha: GachaConfig {
                keys_per_boss: 1,
                deep_floor_step: 10,
                deep_floor_bonus_keys: 2,
                // 50% Normal / 25% Treasure / 20% Elite / 5% Bonanza
                floor_kind_weights: [50, 25, 20, 5],
                floor_kind_normal_below: 3,
                // 60% / 28% / 10% / 2%
                gacha_weights_milli: [600, 280, 100, 20],
                gacha_pity: 50,
                common_gold_mult_min: 5,
                common_gold_mult_max: 15,
                epic_souls_mult: 8,
                legendary_keys: 5,
            },
        }
    }
}

// プリセット群はシミュレータ (= test build) でのみ参照される。本体ゲームに
// 「難易度選択」を入れるときに #[cfg(test)] を外して runtime に昇格する。
#[cfg(test)]
impl BalanceConfig {
    /// 「優しめ」プリセット — 敵が弱く、報酬が多め。
    pub fn easy() -> Self {
        let mut c = Self::default();
        c.enemy.hp_growth = 1.25;
        c.enemy.atk_growth = 1.18;
        c.enemy.gold_growth = 1.45;
        c.enemy.boss_hp_mult = 4.0;
        c
    }

    /// 「厳しめ」プリセット — 敵が強く、報酬は据え置き。
    pub fn hard() -> Self {
        let mut c = Self::default();
        c.enemy.hp_growth = 1.40;
        c.enemy.atk_growth = 1.28;
        c.enemy.boss_hp_mult = 6.0;
        c.enemy.boss_atk_mult = 1.8;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_legacy_constants() {
        // 既存コードのリテラル値と一致していることを確認 (リファクタの安全網)。
        let c = BalanceConfig::default();
        assert_eq!(c.hero.base_hp, 50);
        assert_eq!(c.hero.base_atk, 5);
        assert_eq!(c.hero.base_def, 2);
        assert_eq!(c.hero.atk_period_base, 12);
        assert_eq!(c.hero.atk_period_min, 3);
        assert_eq!(c.hero.atk_per_sword_lv, 2);
        assert_eq!(c.hero.hp_per_vitality_lv, 10);
        assert!((c.hero.crit_cap - 0.60).abs() < 1e-9);
        assert_eq!(c.enemy.hp_base, 14.0);
        assert_eq!(c.enemy.hp_growth, 1.32);
        assert_eq!(c.enemy.atk_base, 4.0);
        assert_eq!(c.enemy.atk_growth, 1.22);
        assert_eq!(c.enemy.boss_hp_mult, 5.0);
        assert_eq!(c.enemy.normal_atk_period, 18);
        assert_eq!(c.enemy.boss_atk_period, 14);
        assert_eq!(c.pacing.enemies_per_floor, 8);
        assert_eq!(c.pacing.normal_souls_div, 5);
    }

    #[test]
    fn presets_differ_from_default() {
        let easy = BalanceConfig::easy();
        let hard = BalanceConfig::hard();
        let def = BalanceConfig::default();
        assert!(easy.enemy.hp_growth < def.enemy.hp_growth);
        assert!(hard.enemy.hp_growth > def.enemy.hp_growth);
    }
}
