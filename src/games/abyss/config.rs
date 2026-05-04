//! 深淵潜行 — 難易度バランス設定 (DI 用)。
//!
//! 数値定数をここに集約することで、本体ゲームとシミュレータで同じ
//! `logic::tick` を共有しつつ、難易度だけを差し替えられるようにする。
//!
//! 値を変えても挙動は `logic.rs` の式によって厳密に決まる ─ つまり
//! sim で観測した結果は本体ゲームでも完全に再現される。
//!
//! 既定値は本体ゲームの現在のバランスを表す (リファクタ前後で挙動不変)。

/// 段階制成長カーブ。一定 level ごとに per-level 増分が変わる区分線形関数。
///
/// 例: Sword の `[(0,2,"剣士"), (10,3,"剣豪"), (25,5,"剣聖")]` は
///   Lv 1〜10  : +2/Lv → 累積 ATK = 2*Lv
///   Lv 11〜25 : +3/Lv → 累積 ATK = 20 + 3*(Lv - 10)
///   Lv 26〜   : +5/Lv → 累積 ATK = 20 + 45 + 5*(Lv - 25)
///
/// 「段階の名前」は UI に出して未到達層が "次の目的地" として見える。
///
/// 不正な状態 (空 tiers / start_level が 0 から始まらない / 昇順でない) は
/// 構築時に panic させ、ランタイムの index アクセスで panic しないようにする。
#[derive(Clone, Debug)]
pub struct TierCurve {
    /// `(start_level, per_level_delta, tier_name)` の昇順配列。空不可。
    /// 必ず最初の要素は `start_level == 0`。
    tiers: Vec<(u32, f64, &'static str)>,
}

impl TierCurve {
    /// バリデーション付きコンストラクタ。空 tiers / 不正な順序を拒否する。
    pub fn new(tiers: Vec<(u32, f64, &'static str)>) -> Self {
        assert!(!tiers.is_empty(), "TierCurve requires at least one tier");
        assert_eq!(
            tiers[0].0, 0,
            "TierCurve first tier must start at level 0 (got {})",
            tiers[0].0
        );
        for w in tiers.windows(2) {
            assert!(
                w[0].0 < w[1].0,
                "TierCurve start_level must be strictly ascending (got {} -> {})",
                w[0].0,
                w[1].0
            );
        }
        Self { tiers }
    }

    /// 段階数。常に >= 1。
    pub fn len(&self) -> usize {
        self.tiers.len()
    }

    /// インデックス指定で段階を取り出す。境界外は None。
    pub fn tier(&self, idx: usize) -> Option<(u32, f64, &'static str)> {
        self.tiers.get(idx).copied()
    }

    /// Lv `level` までの累積効果値を返す (合算)。
    pub fn cumulative(&self, level: u32) -> f64 {
        let mut total = 0.0;
        for window in self.tiers.windows(2) {
            let (start, delta, _) = window[0];
            let (next_start, _, _) = window[1];
            if level <= start {
                break;
            }
            let span = (level.min(next_start) - start) as f64;
            total += span * delta;
        }
        // 最後の段階 (上限なし)
        if let Some(&(start, delta, _)) = self.tiers.last() {
            if level > start {
                let span = (level - start) as f64;
                total += span * delta;
            }
        }
        total
    }

    /// `level` が属する段階のインデックスと名前を返す。
    ///
    /// 境界の解釈は `cumulative` と一致させる: `start_level` は
    /// 「**次の段階に入るために超えるべき Lv**」であり、`level == start_level` は
    /// まだ前段階に属する。例えば curve `[(0,…), (10,…), (25,…)]` で:
    ///   Lv 0..=10  → 段階 0 (旧スロープ継続)
    ///   Lv 11..=25 → 段階 1
    ///   Lv 26..    → 段階 2
    ///
    /// この扱いでないと「Lv 10 で段階突破ログが出るのに伸びは Lv 11 から」
    /// というズレが生じるため、両者の境界判定を一致させている。
    ///
    /// `new()` で空を弾いているので `tiers[idx]` は常に有効。
    pub fn tier_at(&self, level: u32) -> (usize, &'static str) {
        let mut idx = 0;
        for (i, &(start, _, _)) in self.tiers.iter().enumerate() {
            if level > start {
                idx = i;
            } else {
                break;
            }
        }
        (idx, self.tiers[idx].2)
    }

    /// 次の段階 (start_level, name)。最終段なら None。
    pub fn next_tier(&self, level: u32) -> Option<(u32, &'static str)> {
        let (idx, _) = self.tier_at(level);
        self.tiers.get(idx + 1).map(|&(s, _, n)| (s, n))
    }
}

/// 既定の Sword (ATK) 段階制カーブ。
///
/// 設計判断ポイント:
/// - **段階の境界 (start_level)**: どの Lv で「段階が変わった！」を体感させるか
/// - **per-level delta**: 段階内でどれだけ伸びるか (旧線形は一律 2.0)
/// - **段階名**: 未到達段階が UI に見えると「先がある」感が出る
///
/// 序盤 (Lv 1〜10) は旧バランスと一致するスロープを維持し、
/// Lv 11 以降で段階突破ごとにスロープが加速する。
fn default_sword_curve() -> TierCurve {
    TierCurve::new(vec![
        (0, 2.0, "剣士"),
        (10, 3.0, "剣豪"),
        (25, 5.0, "剣聖"),
        (50, 8.0, "剣神"),
        (100, 12.0, "剣の化身"),
    ])
}

/// 既定の Vitality (HP) 段階制カーブ。Lv 1〜10 は旧線形 (+10/Lv) と一致。
fn default_vitality_curve() -> TierCurve {
    TierCurve::new(vec![
        (0, 10.0, "凡体"),
        (10, 15.0, "屈強"),
        (25, 25.0, "鋼体"),
        (50, 40.0, "不撓"),
        (100, 60.0, "不死身"),
    ])
}

/// 既定の Armor (DEF) 段階制カーブ。Lv 1〜10 は旧線形 (+1/Lv)。
fn default_armor_curve() -> TierCurve {
    TierCurve::new(vec![
        (0, 1.0, "軽装"),
        (10, 2.0, "重装"),
        (25, 3.0, "鉄壁"),
        (50, 5.0, "神鎧"),
        (100, 8.0, "不落"),
    ])
}

/// 既定の Speed 段階制カーブ (倍率増分)。Lv 1〜10 は旧線形 (+0.05/Lv = 5%)。
fn default_speed_curve() -> TierCurve {
    TierCurve::new(vec![
        (0, 0.05, "軽歩"),
        (10, 0.07, "疾風"),
        (25, 0.10, "神速"),
        (50, 0.15, "瞬光"),
        (100, 0.20, "残影"),
    ])
}

/// ヒーローの基礎値とアップグレード効果。
#[derive(Clone, Debug)]
pub struct HeroConfig {
    pub base_hp: u64,
    pub base_atk: u64,
    pub base_def: u64,
    /// 1 攻撃あたりの基礎 tick 数 (Speed 強化と戦闘集中で短縮)。
    pub atk_period_base: u32,
    /// 攻撃間隔の下限 tick (これより短くしない)。
    pub atk_period_min: u32,

    /// 戦闘集中 (combat focus) の上限。攻撃成功で +1、死亡や撤退で 0 にリセット。
    pub focus_max: u32,
    /// focus 1 ポイントごとに攻撃間隔を短縮する係数 (0.0..=1.0)。
    /// 例: 0.01 なら 1 ポイントで 1% 短縮、focus_max が 50 なら最大 50% 短縮。
    pub focus_reduction_per_point: f64,

    // upgrade per-level deltas
    pub atk_per_sword_lv: u64,
    /// 段階制 ATK カーブ。設定時は `atk_per_sword_lv` を上書きする。
    /// `None` の場合は旧線形 (`atk_per_sword_lv * level`) を使う。
    pub sword_curve: Option<TierCurve>,
    pub hp_per_vitality_lv: u64,
    /// 段階制 HP カーブ。設定時は `hp_per_vitality_lv` を上書き。
    pub vitality_curve: Option<TierCurve>,
    pub def_per_armor_lv: u64,
    /// 段階制 DEF カーブ。設定時は `def_per_armor_lv` を上書き。
    pub armor_curve: Option<TierCurve>,
    pub crit_per_lv: f64,
    pub crit_cap: f64,
    pub speed_per_lv: f64,
    /// 段階制 Speed カーブ (倍率増分)。設定時は `speed_per_lv` を上書き。
    pub speed_curve: Option<TierCurve>,
    pub regen_per_lv_per_sec: f64,
    pub gold_per_lv: f64,

    // soul perk multipliers
    pub might_per_lv: f64,
    pub endurance_per_lv: f64,
    pub fortune_per_lv: f64,
    pub reaper_per_lv: f64,
}

/// 敵のスケーリングパラメータ。
///
/// HP / ATK / gold の階層成長は `BalanceConfig::enemy_hp_schedule` 等の
/// piecewise schedule で表現する (旧 `hp_growth: f64` 等の単一指数は
/// B100 到達不能になるため撤去)。`def_per_floor` は線形なのでここに残す。
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
    /// 雑魚撃破時、`floor / normal_souls_div` 切り上げが基礎魂量。
    pub normal_souls_div: u32,
    /// ボス撃破時、`floor * boss_souls_mult` が基礎魂量。
    pub boss_souls_mult: u64,
    /// 死亡時、`floor * death_souls_mult` が基礎魂量。
    pub death_souls_mult: u64,
    /// ダンジョンの到達ゴールフロア。進捗タブの分母として使う。
    /// 将来「短編 (50F)」「長編 (200F)」など難度差し替えで切り替えるため
    /// BalanceConfig 経由で注入する。
    pub goal_floor: u32,
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

/// 装備の解放条件。すべて AND で満たす必要がある。
///
/// 設計判断: フロア到達条件は入れない (gold + 強化 Lv + 前装備のみ)。
/// 「フロア降下のリスクを取らないと装備が解放されない」型のゲームではなく、
/// 「ゴールドファームしながらでも装備計画を進められる」型にする (=
/// idle ゲーム本来の自由度を保つ)。
#[derive(Clone, Debug, Default)]
pub struct EquipmentRequirement {
    /// 必要 gold (購入時に消費される)。
    pub gold_cost: u64,
    /// 必要強化レベル。`(UpgradeKind, min_level)` の AND リスト。
    /// 同じ UpgradeKind を複数入れる必要は無い (最大値だけ書けば十分)。
    pub upgrade_levels: Vec<(super::state::UpgradeKind, u32)>,
    /// 前提装備 (この装備を解放済みであること)。lane 連鎖の表現。
    /// 通常は 1 個 (同 lane の前段階)。空なら lane 入り口の装備。
    pub prerequisite: Option<super::state::EquipmentId>,
}

/// 装備 1 個の定義 (id・名前・lane 帰属・解放条件・効果)。
///
/// Vec で持つので並びは `EquipmentId::all()` と必ず一致させる
/// (`lookup` は index アクセスする想定)。テストで一致を保証する。
#[derive(Clone, Debug)]
pub struct EquipmentDef {
    pub id: super::state::EquipmentId,
    pub name: &'static str,
    pub effect_label: &'static str,
    pub requirement: EquipmentRequirement,
    pub bonus: super::state::EquipmentBonus,
}

/// 既定の装備テーブル (12 個 / 3 lane × 4 段階)。
///
/// バランス設計:
/// - 武器 lane: ATK 系。各段階で +5% / +20% / +60% / +200% (additive 合計 +285%)
/// - 防具 lane: HP 系 + DEF flat。HP +5% / +20% / +60% / +200%、DEF +5/+20/+50/+150
/// - 装飾 lane: Speed/Crit/Regen/Gold 各種 + 終焉の冠で全方位ブースト
///
/// gold コストは「全装備解放までに ~30M gold が必要」になるよう調整。
/// 強化 Lv 条件は既存 TierCurve の境界 (10/25/50/100) と整合させる。
fn default_equipment_table() -> Vec<EquipmentDef> {
    use super::state::{EquipmentBonus, EquipmentId, UpgradeKind};

    vec![
        // ── 武器 lane ──
        EquipmentDef {
            id: EquipmentId::BronzeSword,
            name: "銅の剣",
            effect_label: "ATK +5%",
            requirement: EquipmentRequirement {
                gold_cost: 100,
                upgrade_levels: vec![(UpgradeKind::Sword, 5)],
                prerequisite: None,
            },
            bonus: EquipmentBonus {
                atk_pct: 0.05,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::SteelSword,
            name: "鋼鉄の剣",
            effect_label: "ATK +20%",
            requirement: EquipmentRequirement {
                gold_cost: 5_000,
                upgrade_levels: vec![(UpgradeKind::Sword, 20)],
                prerequisite: Some(EquipmentId::BronzeSword),
            },
            bonus: EquipmentBonus {
                atk_pct: 0.20,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::MithrilSword,
            name: "ミスリルの剣",
            effect_label: "ATK +60%",
            requirement: EquipmentRequirement {
                gold_cost: 200_000,
                upgrade_levels: vec![(UpgradeKind::Sword, 40)],
                prerequisite: Some(EquipmentId::SteelSword),
            },
            bonus: EquipmentBonus {
                atk_pct: 0.60,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::GodSword,
            name: "神剣エクスカリバー",
            effect_label: "ATK +400%",
            requirement: EquipmentRequirement {
                gold_cost: 5_000_000,
                upgrade_levels: vec![(UpgradeKind::Sword, 80)],
                prerequisite: Some(EquipmentId::MithrilSword),
            },
            bonus: EquipmentBonus {
                atk_pct: 4.00,
                ..Default::default()
            },
        },
        // ── 防具 lane ──
        EquipmentDef {
            id: EquipmentId::LeatherArmor,
            name: "革鎧",
            effect_label: "HP +5% / DEF +5",
            requirement: EquipmentRequirement {
                gold_cost: 150,
                upgrade_levels: vec![(UpgradeKind::Armor, 5)],
                prerequisite: None,
            },
            bonus: EquipmentBonus {
                hp_pct: 0.05,
                def_flat: 5,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::SteelArmor,
            name: "鋼鉄の鎧",
            effect_label: "HP +20% / DEF +20",
            requirement: EquipmentRequirement {
                gold_cost: 7_500,
                upgrade_levels: vec![(UpgradeKind::Armor, 15), (UpgradeKind::Vitality, 10)],
                prerequisite: Some(EquipmentId::LeatherArmor),
            },
            bonus: EquipmentBonus {
                hp_pct: 0.20,
                def_flat: 20,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::MithrilArmor,
            name: "ミスリルの鎧",
            effect_label: "HP +60% / DEF +50",
            requirement: EquipmentRequirement {
                gold_cost: 250_000,
                upgrade_levels: vec![(UpgradeKind::Armor, 30), (UpgradeKind::Vitality, 30)],
                prerequisite: Some(EquipmentId::SteelArmor),
            },
            bonus: EquipmentBonus {
                hp_pct: 0.60,
                def_flat: 50,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::GodArmor,
            name: "神鎧アイギス",
            effect_label: "HP +600% / DEF +800",
            requirement: EquipmentRequirement {
                gold_cost: 6_000_000,
                upgrade_levels: vec![(UpgradeKind::Armor, 60), (UpgradeKind::Vitality, 60)],
                prerequisite: Some(EquipmentId::MithrilArmor),
            },
            bonus: EquipmentBonus {
                hp_pct: 6.00,
                def_flat: 800,
                ..Default::default()
            },
        },
        // ── 装飾 lane ──
        EquipmentDef {
            id: EquipmentId::SwiftBoots,
            name: "速攻のブーツ",
            effect_label: "攻撃速度 +20%",
            requirement: EquipmentRequirement {
                gold_cost: 200,
                upgrade_levels: vec![(UpgradeKind::Speed, 5)],
                prerequisite: None,
            },
            bonus: EquipmentBonus {
                speed_pct: 0.20,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::TwinWolfRing,
            name: "双狼の指輪",
            effect_label: "CRIT +10%",
            requirement: EquipmentRequirement {
                gold_cost: 8_000,
                upgrade_levels: vec![(UpgradeKind::Crit, 10)],
                prerequisite: Some(EquipmentId::SwiftBoots),
            },
            bonus: EquipmentBonus {
                crit_bonus: 0.10,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::SageRobe,
            name: "賢者のローブ",
            effect_label: "回復 +1.5/s / 金 +30%",
            requirement: EquipmentRequirement {
                gold_cost: 300_000,
                upgrade_levels: vec![(UpgradeKind::Regen, 20), (UpgradeKind::Gold, 15)],
                prerequisite: Some(EquipmentId::TwinWolfRing),
            },
            bonus: EquipmentBonus {
                regen_per_sec: 1.5,
                gold_pct: 0.30,
                ..Default::default()
            },
        },
        EquipmentDef {
            id: EquipmentId::EndingCrown,
            name: "終焉の冠",
            effect_label: "ATK +150% / HP +150%",
            requirement: EquipmentRequirement {
                gold_cost: 8_000_000,
                upgrade_levels: vec![
                    (UpgradeKind::Speed, 50),
                    (UpgradeKind::Crit, 30),
                    (UpgradeKind::Regen, 30),
                    (UpgradeKind::Gold, 30),
                ],
                prerequisite: Some(EquipmentId::SageRobe),
            },
            bonus: EquipmentBonus {
                atk_pct: 1.50,
                hp_pct: 1.50,
                ..Default::default()
            },
        },
    ]
}

/// 敵成長の階層帯ごとの倍率テーブル。
///
/// 既定の指数成長 (`hp_growth.powf(F-1)`) では B100 の HP が天文学的になり
/// 「全装備でもクリア不能」になるため、フロア帯ごとに growth rate を変える
/// piecewise schedule を導入する。
///
/// `steps` は `(start_floor, growth_rate)` の昇順配列。
/// 例: `[(1, 1.32), (10, 1.20), (25, 1.12), (50, 1.10)]` のとき
///   B 1- 9 区間: 1.32 で 9 段成長 (約 ×11)
///   B10-24 区間: 1.20 で 15 段成長 (約 ×15)
///   B25-49 区間: 1.12 で 25 段成長 (約 ×17)
///   B50+ 区間  : 1.10 で 残り段成長
///
/// `start_floor` は「この階で次の rate に切り替わる」境界。リストの最初は
/// 必ず `start_floor=1` で始まる。
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
    ///
    /// 意味: 各遷移 B(f-1) → Bf で「Bf に当てはまる segment の rate」を 1 回掛ける。
    /// `multiplier(F) = ∏_{f=2..=F} rate_at(f)`
    /// 例: steps=[(1, 1.32), (10, 1.20)] のとき
    ///   rate_at(2..=9) = 1.32, rate_at(10..) = 1.20
    ///   multiplier(10) = 1.32^8 × 1.20^1
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
            // この segment が支配する遷移 = `f in [max(seg_start, 2), min(floor+1, seg_end))`
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
    /// 敵 HP の階層帯成長スケジュール (B100 到達可能性のため piecewise 化)。
    pub enemy_hp_schedule: EnemyGrowthSchedule,
    /// 敵 ATK の階層帯成長スケジュール (HP と同じ理由)。
    pub enemy_atk_schedule: EnemyGrowthSchedule,
    /// 敵 gold ドロップの階層帯成長スケジュール (gold rate もスローダウンしないと
    /// 装備コストが届かなくなるが、深層では gold rate を残しておくと装備購入動機になる)。
    pub enemy_gold_schedule: EnemyGrowthSchedule,
}

impl Default for BalanceConfig {
    /// 本体ゲームの既定難易度。値は変えるとゲームバランスが変わる。
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

                atk_per_sword_lv: 2,
                sword_curve: Some(default_sword_curve()),
                hp_per_vitality_lv: 10,
                vitality_curve: Some(default_vitality_curve()),
                def_per_armor_lv: 1,
                armor_curve: Some(default_armor_curve()),
                crit_per_lv: 0.01,
                crit_cap: 0.60,
                speed_per_lv: 0.05,
                speed_curve: Some(default_speed_curve()),
                regen_per_lv_per_sec: 0.2,
                gold_per_lv: 0.05,

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
            equipment: default_equipment_table(),
            // piecewise growth: 早期は現状 (1.32) の手応えを保ち、深層は装備で
            // 押し切れるレートまで段階的に緩める。
            // B 1- 25: 旧バランス相当 (急峻、毎フロアの達成感)
            // B26- 50: 装備中盤を活かす (中緩やか)
            // B51- 75: 装備上位を必要とする (緩やか)
            // B76+   : 神装備込みで届くレベル (極緩やか)
            // B100 想定 HP = 14 × 1.32^9 × 1.20^15 × 1.10^25 × 1.04^25 × 1.015^25 ≈ 8k
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
            // gold rate は早期 1.40 → 深層 1.18。装備購入が成立する高単価を維持。
            enemy_gold_schedule: EnemyGrowthSchedule::new(vec![
                (1, 1.40),
                (10, 1.30),
                (25, 1.22),
                (50, 1.18),
            ]),
        }
    }
}

// プリセット群はシミュレータ (= test build) でのみ参照される。本体ゲームに
// 「難易度選択」を入れるときに #[cfg(test)] を外して runtime に昇格する。
#[cfg(test)]
impl BalanceConfig {
    /// 「優しめ」プリセット — 各 segment を default より低く設定 (5 段階構造を維持)。
    #[allow(clippy::field_reassign_with_default)] // schedule 群を書き直す方が冗長
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

    /// 「厳しめ」プリセット — 各 segment を default より高く設定 (5 段階構造を維持)。
    #[allow(clippy::field_reassign_with_default)] // schedule 群を書き直す方が冗長
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
        // 既存コードのリテラル値と一致していることを確認 (リファクタの安全網)。
        let c = BalanceConfig::default();
        assert_eq!(c.hero.base_hp, 50);
        assert_eq!(c.hero.base_atk, 5);
        assert_eq!(c.hero.base_def, 2);
        assert_eq!(c.hero.atk_period_base, 18);
        assert_eq!(c.hero.atk_period_min, 3);
        assert_eq!(c.hero.focus_max, 50);
        assert!((c.hero.focus_reduction_per_point - 0.012).abs() < 1e-9);
        assert_eq!(c.hero.atk_per_sword_lv, 2);
        assert_eq!(c.hero.hp_per_vitality_lv, 10);
        assert!((c.hero.crit_cap - 0.60).abs() < 1e-9);
        assert_eq!(c.enemy.hp_base, 14.0);
        assert_eq!(c.enemy.atk_base, 4.0);
        // boss_hp_mult / boss_atk_mult は 40h B100 設計に合わせて緩和済 (旧 5.0 → 4.0)。
        assert_eq!(c.enemy.boss_hp_mult, 4.0);
        assert!((c.enemy.boss_atk_mult - 1.3).abs() < 1e-9);
        assert_eq!(c.enemy.normal_atk_period, 18);
        assert_eq!(c.enemy.boss_atk_period, 14);
        assert_eq!(c.pacing.enemies_per_floor, 8);
        assert_eq!(c.pacing.normal_souls_div, 5);
        assert_eq!(c.pacing.goal_floor, 100);
        // piecewise schedule の早期 rate が旧定数と一致 (体感維持)。
        assert!((c.enemy_hp_schedule.multiplier(2) - 1.32).abs() < 1e-9);
        assert!((c.enemy_atk_schedule.multiplier(2) - 1.22).abs() < 1e-9);
        // 装備テーブルは 12 個。
        assert_eq!(c.equipment.len(), 12);
    }

    #[test]
    fn enemy_growth_schedule_piecewise() {
        let s = EnemyGrowthSchedule::new(vec![(1, 2.0), (5, 3.0)]);
        // multiplier(1) = 1.0、multiplier(2..=4) は 2.0 系で乗算
        assert!((s.multiplier(1) - 1.0).abs() < 1e-9);
        assert!((s.multiplier(2) - 2.0).abs() < 1e-9);
        assert!((s.multiplier(4) - 8.0).abs() < 1e-9);
        // floor 5 で 3.0 segment に切り替わる。multiplier(5) = 8.0 × 3.0 = 24.0
        assert!((s.multiplier(5) - 24.0).abs() < 1e-9);
        assert!((s.multiplier(6) - 72.0).abs() < 1e-9);
    }

    #[test]
    fn equipment_table_matches_id_order() {
        // EquipmentDef::id 並びが EquipmentId::all() と一致する SSOT 保護網。
        let c = BalanceConfig::default();
        for (i, def) in c.equipment.iter().enumerate() {
            assert_eq!(def.id.index(), i, "equipment[{i}] id mismatch");
        }
    }

    #[test]
    fn tier_curve_single_tier_is_linear() {
        // 1 段階のみのカーブは線形 (旧バランスと一致)
        let c = TierCurve::new(vec![(0, 2.0, "剣士")]);
        assert_eq!(c.cumulative(0), 0.0);
        assert_eq!(c.cumulative(10), 20.0);
        assert_eq!(c.cumulative(50), 100.0);
        assert_eq!(c.tier_at(0).1, "剣士");
        assert_eq!(c.tier_at(99).1, "剣士");
        assert!(c.next_tier(50).is_none());
    }

    #[test]
    #[should_panic(expected = "at least one tier")]
    fn tier_curve_rejects_empty() {
        let _ = TierCurve::new(vec![]);
    }

    #[test]
    #[should_panic(expected = "must start at level 0")]
    fn tier_curve_rejects_non_zero_start() {
        let _ = TierCurve::new(vec![(1, 2.0, "x")]);
    }

    #[test]
    #[should_panic(expected = "strictly ascending")]
    fn tier_curve_rejects_non_ascending_starts() {
        let _ = TierCurve::new(vec![(0, 2.0, "a"), (10, 3.0, "b"), (10, 5.0, "c")]);
    }

    #[test]
    fn tier_curve_multi_tier_accumulates_per_segment() {
        let c = TierCurve::new(vec![
            (0, 2.0, "剣士"),
            (10, 3.0, "剣豪"),
            (25, 5.0, "剣聖"),
        ]);
        // Lv 0..=10 は線形 (剣士)
        assert_eq!(c.cumulative(10), 20.0);
        // Lv 11..=25 は 20 + 3*(Lv-10)
        assert_eq!(c.cumulative(15), 20.0 + 3.0 * 5.0);
        assert_eq!(c.cumulative(25), 20.0 + 3.0 * 15.0); // 65
        // Lv 26+ は 65 + 5*(Lv-25)
        assert_eq!(c.cumulative(30), 65.0 + 5.0 * 5.0); // 90

        // tier_at は cumulative と境界を共有する:
        //   start_level は「次段階に入るために超える Lv」なので、
        //   level == start_level はまだ前段階。
        assert_eq!(c.tier_at(10).1, "剣士"); // 10 まで剣士の +2 が効く
        assert_eq!(c.tier_at(11).1, "剣豪"); // 11 から +3 に切り替わる
        assert_eq!(c.tier_at(25).1, "剣豪"); // 25 まで剣豪
        assert_eq!(c.tier_at(26).1, "剣聖"); // 26 から剣聖
        assert_eq!(c.next_tier(5), Some((10, "剣豪")));
        assert_eq!(c.next_tier(10), Some((10, "剣豪"))); // Lv10 ではまだ剣士、次が剣豪
        assert_eq!(c.next_tier(11), Some((25, "剣聖"))); // Lv11 で剣豪入り、次が剣聖
        assert!(c.next_tier(30).is_none());
    }

    #[test]
    fn presets_differ_from_default() {
        let easy = BalanceConfig::easy();
        let hard = BalanceConfig::hard();
        let def = BalanceConfig::default();
        // 同じフロア (B100) における累積 HP 倍率で難易度差を比較。
        let f = 100u32;
        assert!(easy.enemy_hp_schedule.multiplier(f) < def.enemy_hp_schedule.multiplier(f));
        assert!(hard.enemy_hp_schedule.multiplier(f) > def.enemy_hp_schedule.multiplier(f));
    }
}
