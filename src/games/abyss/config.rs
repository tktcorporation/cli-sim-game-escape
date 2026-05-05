//! 深淵潜行 — 難易度バランス設定 (DI 用)。
//!
//! 数値定数をここに集約することで、本体ゲームとシミュレータで同じ
//! `logic::tick` を共有しつつ、難易度だけを差し替えられるようにする。
//!
//! 値を変えても挙動は `logic.rs` の式によって厳密に決まる ─ つまり
//! sim で観測した結果は本体ゲームでも完全に再現される。
//!
//! 既定値は本体ゲームの現在のバランスを表す (リファクタ前後で挙動不変)。

/// ヒーローの基礎値と soul perk 倍率。
///
/// 旧来の per-Sword/Vitality/etc Lv 加算は **全廃** された。英雄ステは:
///   `base_* + 装着中装備.scaled_bonus(Lv) × (1 + soul_perks)`
/// に集約される。stat ごとの per-Lv 値は装備自体が
/// `EquipmentDef::per_level_bonus` で持つ (装備ごとに伸び方が違って良い)。
#[derive(Clone, Debug)]
pub struct HeroConfig {
    pub base_hp: u64,
    pub base_atk: u64,
    pub base_def: u64,
    /// 1 攻撃あたりの基礎 tick 数 (装備の speed_pct と戦闘集中で短縮)。
    pub atk_period_base: u32,
    /// 攻撃間隔の下限 tick (これより短くしない)。
    pub atk_period_min: u32,

    /// 戦闘集中 (combat focus) の上限。攻撃成功で +1、死亡や撤退で 0 にリセット。
    pub focus_max: u32,
    /// focus 1 ポイントごとに攻撃間隔を短縮する係数 (0.0..=1.0)。
    pub focus_reduction_per_point: f64,

    /// クリティカル率の上限。装備の crit_bonus 合計がこれを超えても切り詰められる。
    pub crit_cap: f64,

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
    pub atk_base: f64,
    pub def_base: f64,
    pub def_per_floor: f64,
    pub gold_base: f64,

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
    pub normal_souls_div: u32,
    pub boss_souls_mult: u64,
    pub death_souls_mult: u64,
    pub goal_floor: u32,
}

/// ガチャ・鍵ドロップ・フロア種別抽選の設定。
#[derive(Clone, Debug)]
pub struct GachaConfig {
    pub keys_per_boss: u64,
    pub deep_floor_step: u32,
    pub deep_floor_bonus_keys: u64,

    pub floor_kind_weights: [u32; 4],
    pub floor_kind_normal_below: u32,

    pub gacha_weights_milli: [u32; 4],
    pub gacha_pity: u32,

    pub common_gold_mult_min: u32,
    pub common_gold_mult_max: u32,
    pub epic_souls_mult: u64,
    pub legendary_keys: u64,
}

/// 装備 1 個の定義 (id・名前・lane 帰属・購入条件・効果カーブ)。
///
/// 効果は **base + per_level × Lv** の形で表現される。これにより:
/// - 銅剣は base 控えめ / per_level も控えめ → 序盤の足場
/// - 神剣は base 爆発 / per_level も爆発 → 後期に強化を注ぎ込む対象
///
/// と装備ごとに伸び方の質を変えられる。
///
/// Vec で持つので並びは `EquipmentId::all()` と必ず一致させる。テストで保証。
#[derive(Clone, Debug)]
pub struct EquipmentDef {
    /// 自分の id。`config.equipment[i].id == EquipmentId::all()[i]` を SSOT 整合性
    /// テスト (`equipment_table_matches_id_order`) で検証する用途のみで保持する。
    /// コード側はインデックスアクセス (`config.equipment.get(id.index())`) で引くので、
    /// 非 test ビルドでは読まれず dead_code 警告が出るのを抑制する。
    #[cfg_attr(not(test), allow(dead_code))]
    pub id: super::state::EquipmentId,
    pub name: &'static str,
    /// UI 用の効果ラベル (Lv 0 時点の主要数値)。
    pub effect_label: &'static str,
    /// Lv 0 (購入直後) の bonus。
    pub base_bonus: super::state::EquipmentBonus,
    /// 強化 Lv 1 ごとに加算される bonus。
    pub per_level_bonus: super::state::EquipmentBonus,

    /// 購入コスト (gold)。
    pub gold_cost: u64,
    /// 前提装備 (この装備を所持していること)。lane 連鎖の表現。
    /// 通常は 1 個 (同 lane の前段階)。空なら lane 入り口の装備。
    pub prerequisite: Option<super::state::EquipmentId>,

    /// 強化 Lv 0 → 1 のコスト (gold)。
    pub enh_cost_base: u64,
    /// 強化コストの geometric 成長率 (典型値 1.15..1.20)。
    pub enh_cost_growth: f64,
}

/// 既定の装備テーブル (12 個 / 3 lane × 4 段階)。
///
/// バランス設計:
/// - 武器 lane: ATK 系 (base + per-Lv で flat と % 両方が伸びる)
/// - 防具 lane: HP 系 + DEF flat
/// - 装飾 lane: Speed/Crit/Regen/Gold の混合 + 終焉の冠で全方位ブースト
///
/// 「強化 Lv を伸ばすのが進行軸」なので、各装備とも per_level_bonus が
/// 旧 UpgradeKind の per-Lv に相当する役目を持つ。
fn default_equipment_table() -> Vec<EquipmentDef> {
    use super::state::{EquipmentBonus, EquipmentId};

    vec![
        // ── 武器 lane ──
        // 銅: 序盤、強化で線形に伸びる足場。
        EquipmentDef {
            id: EquipmentId::BronzeSword,
            name: "銅の剣",
            effect_label: "ATK +5% / +1 (Lv毎 +1%/+1)",
            base_bonus: EquipmentBonus { atk_pct: 0.05, atk_flat: 1, ..Default::default() },
            per_level_bonus: EquipmentBonus { atk_pct: 0.01, atk_flat: 1, ..Default::default() },
            gold_cost: 100,
            prerequisite: None,
            enh_cost_base: 15,
            enh_cost_growth: 1.16,
        },
        EquipmentDef {
            id: EquipmentId::SteelSword,
            name: "鋼鉄の剣",
            effect_label: "ATK +20% / +5 (Lv毎 +2%/+3)",
            base_bonus: EquipmentBonus { atk_pct: 0.20, atk_flat: 5, ..Default::default() },
            per_level_bonus: EquipmentBonus { atk_pct: 0.02, atk_flat: 3, ..Default::default() },
            gold_cost: 5_000,
            prerequisite: Some(EquipmentId::BronzeSword),
            enh_cost_base: 600,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::MithrilSword,
            name: "ミスリルの剣",
            effect_label: "ATK +60% / +20 (Lv毎 +5%/+10)",
            base_bonus: EquipmentBonus { atk_pct: 0.60, atk_flat: 20, ..Default::default() },
            per_level_bonus: EquipmentBonus { atk_pct: 0.05, atk_flat: 10, ..Default::default() },
            gold_cost: 200_000,
            prerequisite: Some(EquipmentId::SteelSword),
            enh_cost_base: 25_000,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::GodSword,
            name: "神剣エクスカリバー",
            effect_label: "ATK +400% / +100 (Lv毎 +15%/+50)",
            base_bonus: EquipmentBonus { atk_pct: 4.00, atk_flat: 100, ..Default::default() },
            per_level_bonus: EquipmentBonus { atk_pct: 0.15, atk_flat: 50, ..Default::default() },
            gold_cost: 5_000_000,
            prerequisite: Some(EquipmentId::MithrilSword),
            enh_cost_base: 800_000,
            enh_cost_growth: 1.20,
        },
        // ── 防具 lane ──
        EquipmentDef {
            id: EquipmentId::LeatherArmor,
            name: "革鎧",
            effect_label: "HP +5% / +5 / DEF +1 (Lv毎 +1%/+5/+1)",
            base_bonus: EquipmentBonus {
                hp_pct: 0.05,
                hp_flat: 5,
                def_flat: 1,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                hp_pct: 0.01,
                hp_flat: 5,
                def_flat: 1,
                ..Default::default()
            },
            gold_cost: 150,
            prerequisite: None,
            enh_cost_base: 20,
            enh_cost_growth: 1.16,
        },
        EquipmentDef {
            id: EquipmentId::SteelArmor,
            name: "鋼鉄の鎧",
            effect_label: "HP +20% / +25 / DEF +5 (Lv毎 +2%/+15/+2)",
            base_bonus: EquipmentBonus {
                hp_pct: 0.20,
                hp_flat: 25,
                def_flat: 5,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                hp_pct: 0.02,
                hp_flat: 15,
                def_flat: 2,
                ..Default::default()
            },
            gold_cost: 7_500,
            prerequisite: Some(EquipmentId::LeatherArmor),
            enh_cost_base: 900,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::MithrilArmor,
            name: "ミスリルの鎧",
            effect_label: "HP +60% / +120 / DEF +20 (Lv毎 +5%/+50/+5)",
            base_bonus: EquipmentBonus {
                hp_pct: 0.60,
                hp_flat: 120,
                def_flat: 20,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                hp_pct: 0.05,
                hp_flat: 50,
                def_flat: 5,
                ..Default::default()
            },
            gold_cost: 250_000,
            prerequisite: Some(EquipmentId::SteelArmor),
            enh_cost_base: 30_000,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::GodArmor,
            name: "神鎧アイギス",
            effect_label: "HP +600% / +800 / DEF +100 (Lv毎 +15%/+200/+20)",
            base_bonus: EquipmentBonus {
                hp_pct: 6.00,
                hp_flat: 800,
                def_flat: 100,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                hp_pct: 0.15,
                hp_flat: 200,
                def_flat: 20,
                ..Default::default()
            },
            gold_cost: 6_000_000,
            prerequisite: Some(EquipmentId::MithrilArmor),
            enh_cost_base: 900_000,
            enh_cost_growth: 1.20,
        },
        // ── 装飾 lane ──
        EquipmentDef {
            id: EquipmentId::SwiftBoots,
            name: "速攻のブーツ",
            effect_label: "速度+20% / CRIT+2% (Lv毎 +1%/+0.2%)",
            base_bonus: EquipmentBonus {
                speed_pct: 0.20,
                crit_bonus: 0.02,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                speed_pct: 0.01,
                crit_bonus: 0.002,
                ..Default::default()
            },
            gold_cost: 200,
            prerequisite: None,
            enh_cost_base: 25,
            enh_cost_growth: 1.16,
        },
        EquipmentDef {
            id: EquipmentId::TwinWolfRing,
            name: "双狼の指輪",
            effect_label: "CRIT +10% / 速度+10% (Lv毎 +0.5%/+1%)",
            base_bonus: EquipmentBonus {
                crit_bonus: 0.10,
                speed_pct: 0.10,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                crit_bonus: 0.005,
                speed_pct: 0.01,
                ..Default::default()
            },
            gold_cost: 8_000,
            prerequisite: Some(EquipmentId::SwiftBoots),
            enh_cost_base: 1_000,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::SageRobe,
            name: "賢者のローブ",
            effect_label: "回復+1.5/s / 金+30% (Lv毎 +0.1/s/+2%)",
            base_bonus: EquipmentBonus {
                regen_per_sec: 1.5,
                gold_pct: 0.30,
                crit_bonus: 0.05,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                regen_per_sec: 0.10,
                gold_pct: 0.02,
                crit_bonus: 0.002,
                ..Default::default()
            },
            gold_cost: 300_000,
            prerequisite: Some(EquipmentId::TwinWolfRing),
            enh_cost_base: 40_000,
            enh_cost_growth: 1.18,
        },
        EquipmentDef {
            id: EquipmentId::EndingCrown,
            name: "終焉の冠",
            effect_label: "ATK+150% / HP+150% / 全方位 (Lv毎 +5%系)",
            base_bonus: EquipmentBonus {
                atk_pct: 1.50,
                hp_pct: 1.50,
                speed_pct: 0.30,
                crit_bonus: 0.20,
                regen_per_sec: 3.0,
                gold_pct: 0.50,
                ..Default::default()
            },
            per_level_bonus: EquipmentBonus {
                atk_pct: 0.05,
                hp_pct: 0.05,
                speed_pct: 0.01,
                crit_bonus: 0.005,
                regen_per_sec: 0.20,
                gold_pct: 0.01,
                ..Default::default()
            },
            gold_cost: 8_000_000,
            prerequisite: Some(EquipmentId::SageRobe),
            enh_cost_base: 1_200_000,
            enh_cost_growth: 1.20,
        },
    ]
}

/// 敵成長の階層帯ごとの倍率テーブル。
///
/// `steps` は `(start_floor, growth_rate)` の昇順配列。
/// 例: `[(1, 1.32), (10, 1.20), (25, 1.12), (50, 1.10)]` のとき
///   B 1- 9 区間: 1.32 で 9 段成長 (約 ×11)
///   B10-24 区間: 1.20 で 15 段成長 (約 ×15)
///   B25-49 区間: 1.12 で 25 段成長 (約 ×17)
///   B50+ 区間  : 1.10 で 残り段成長
#[derive(Clone, Debug)]
pub struct EnemyGrowthSchedule {
    steps: Vec<(u32, f64)>,
}

impl EnemyGrowthSchedule {
    pub fn new(steps: Vec<(u32, f64)>) -> Self {
        assert!(!steps.is_empty(), "EnemyGrowthSchedule requires at least one step");
        assert_eq!(
            steps[0].0, 1,
            "EnemyGrowthSchedule first step must start at floor 1 (got {})",
            steps[0].0
        );
        for w in steps.windows(2) {
            assert!(
                w[0].0 < w[1].0,
                "EnemyGrowthSchedule start_floor must be strictly ascending (got {} -> {})",
                w[0].0,
                w[1].0
            );
        }
        Self { steps }
    }

    /// `floor` の累積倍率を返す (B1 = 1.0)。
    pub fn multiplier(&self, floor: u32) -> f64 {
        if floor <= 1 {
            return 1.0;
        }
        let mut total = 1.0;
        let n = self.steps.len();
        for i in 0..n {
            let (seg_start, rate) = self.steps[i];
            let seg_end = if i + 1 < n {
                self.steps[i + 1].0
            } else {
                u32::MAX
            };
            let lo = seg_start.max(2);
            let hi = (floor + 1).min(seg_end);
            if hi > lo {
                total *= rate.powi((hi - lo) as i32);
            }
            if seg_end > floor {
                break;
            }
        }
        total.max(1.0)
    }
}

/// 難易度バランスの集約。state に一個保持する。
#[derive(Clone, Debug)]
pub struct BalanceConfig {
    pub hero: HeroConfig,
    pub enemy: EnemyConfig,
    pub pacing: PacingConfig,
    pub gacha: GachaConfig,
    /// 装備の解放テーブル (12 個 = `EquipmentId::all()`)。順序は `EquipmentId::all()` と一致。
    pub equipment: Vec<EquipmentDef>,
    pub enemy_hp_schedule: EnemyGrowthSchedule,
    pub enemy_atk_schedule: EnemyGrowthSchedule,
    pub enemy_gold_schedule: EnemyGrowthSchedule,
}

impl Default for BalanceConfig {
    /// 本体ゲームの既定難易度。
    fn default() -> Self {
        Self {
            hero: HeroConfig {
                base_hp: 50,
                base_atk: 5,
                base_def: 2,
                atk_period_base: 18,
                atk_period_min: 3,

                focus_max: 50,
                focus_reduction_per_point: 0.012,

                crit_cap: 0.60,

                might_per_lv: 0.05,
                endurance_per_lv: 0.05,
                fortune_per_lv: 0.10,
                reaper_per_lv: 0.20,
            },
            enemy: EnemyConfig {
                hp_base: 14.0,
                atk_base: 4.0,
                def_base: 1.0,
                def_per_floor: 0.5,
                gold_base: 4.0,

                boss_hp_mult: 4.0,
                boss_atk_mult: 1.3,
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
                goal_floor: 100,
            },
            gacha: GachaConfig {
                keys_per_boss: 1,
                deep_floor_step: 10,
                deep_floor_bonus_keys: 2,
                floor_kind_weights: [50, 25, 20, 5],
                floor_kind_normal_below: 3,
                gacha_weights_milli: [600, 280, 100, 20],
                gacha_pity: 50,
                common_gold_mult_min: 5,
                common_gold_mult_max: 15,
                epic_souls_mult: 8,
                legendary_keys: 5,
            },
            equipment: default_equipment_table(),
            enemy_hp_schedule: EnemyGrowthSchedule::new(vec![
                (1, 1.32),
                (10, 1.20),
                (25, 1.10),
                (50, 1.04),
                (75, 1.015),
            ]),
            enemy_atk_schedule: EnemyGrowthSchedule::new(vec![
                (1, 1.22),
                (10, 1.15),
                (25, 1.08),
                (50, 1.03),
                (75, 1.01),
            ]),
            enemy_gold_schedule: EnemyGrowthSchedule::new(vec![
                (1, 1.40),
                (10, 1.30),
                (25, 1.22),
                (50, 1.18),
            ]),
        }
    }
}

#[cfg(test)]
impl BalanceConfig {
    /// 「優しめ」プリセット — 各 segment を default より低く設定。
    #[allow(clippy::field_reassign_with_default)]
    pub fn easy() -> Self {
        let mut c = Self::default();
        c.enemy_hp_schedule = EnemyGrowthSchedule::new(vec![
            (1, 1.25),
            (10, 1.15),
            (25, 1.08),
            (50, 1.03),
            (75, 1.010),
        ]);
        c.enemy_atk_schedule = EnemyGrowthSchedule::new(vec![
            (1, 1.18),
            (10, 1.12),
            (25, 1.06),
            (50, 1.02),
            (75, 1.008),
        ]);
        c.enemy_gold_schedule = EnemyGrowthSchedule::new(vec![
            (1, 1.45),
            (10, 1.35),
            (25, 1.25),
            (50, 1.20),
        ]);
        c.enemy.boss_hp_mult = 3.0;
        c
    }

    /// 「厳しめ」プリセット — 各 segment を default より高く設定。
    #[allow(clippy::field_reassign_with_default)]
    pub fn hard() -> Self {
        let mut c = Self::default();
        c.enemy_hp_schedule = EnemyGrowthSchedule::new(vec![
            (1, 1.40),
            (10, 1.25),
            (25, 1.15),
            (50, 1.06),
            (75, 1.025),
        ]);
        c.enemy_atk_schedule = EnemyGrowthSchedule::new(vec![
            (1, 1.28),
            (10, 1.18),
            (25, 1.12),
            (50, 1.05),
            (75, 1.02),
        ]);
        c.enemy.boss_hp_mult = 5.0;
        c.enemy.boss_atk_mult = 1.6;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_legacy_constants() {
        let c = BalanceConfig::default();
        assert_eq!(c.hero.base_hp, 50);
        assert_eq!(c.hero.base_atk, 5);
        assert_eq!(c.hero.base_def, 2);
        assert_eq!(c.hero.atk_period_base, 18);
        assert_eq!(c.hero.atk_period_min, 3);
        assert_eq!(c.hero.focus_max, 50);
        assert!((c.hero.focus_reduction_per_point - 0.012).abs() < 1e-9);
        assert!((c.hero.crit_cap - 0.60).abs() < 1e-9);
        assert_eq!(c.enemy.hp_base, 14.0);
        assert_eq!(c.enemy.atk_base, 4.0);
        assert_eq!(c.enemy.boss_hp_mult, 4.0);
        assert!((c.enemy.boss_atk_mult - 1.3).abs() < 1e-9);
        assert_eq!(c.pacing.enemies_per_floor, 8);
        assert_eq!(c.pacing.goal_floor, 100);
        assert!((c.enemy_hp_schedule.multiplier(2) - 1.32).abs() < 1e-9);
        assert!((c.enemy_atk_schedule.multiplier(2) - 1.22).abs() < 1e-9);
        assert_eq!(c.equipment.len(), 12);
    }

    #[test]
    fn enemy_growth_schedule_piecewise() {
        let s = EnemyGrowthSchedule::new(vec![(1, 2.0), (5, 3.0)]);
        assert!((s.multiplier(1) - 1.0).abs() < 1e-9);
        assert!((s.multiplier(2) - 2.0).abs() < 1e-9);
        assert!((s.multiplier(4) - 8.0).abs() < 1e-9);
        assert!((s.multiplier(5) - 24.0).abs() < 1e-9);
        assert!((s.multiplier(6) - 72.0).abs() < 1e-9);
    }

    #[test]
    fn equipment_table_matches_id_order() {
        let c = BalanceConfig::default();
        for (i, def) in c.equipment.iter().enumerate() {
            assert_eq!(def.id.index(), i, "equipment[{i}] id mismatch");
        }
    }

    #[test]
    fn presets_differ_from_default() {
        let easy = BalanceConfig::easy();
        let hard = BalanceConfig::hard();
        let def = BalanceConfig::default();
        let f = 100u32;
        assert!(easy.enemy_hp_schedule.multiplier(f) < def.enemy_hp_schedule.multiplier(f));
        assert!(hard.enemy_hp_schedule.multiplier(f) > def.enemy_hp_schedule.multiplier(f));
    }
}
