//! 深淵潜行 (Abyss Idle) — game state.
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs に置く。
//!
//! ## 進行軸の三本柱
//!
//! 旧来の「7 種 UpgradeKind を独立に伸ばす」構造は撤去し、進行は以下の 3 本立てに集約した:
//!
//! 1. **装備購入**: 各 lane (武器/防具/装飾) で次の段階を解放する (gold + 前装備)
//! 2. **装備強化**: 所持装備それぞれに per-equipment の `enhancement_level` を持ち、
//!    レベルが上がると `base_bonus + per_level_bonus × Lv` で効果が伸びる
//! 3. **装着スロット**: 各 lane に 1 つだけ装着 (`equipped: [Option<EquipmentId>; 3]`)。
//!    装着中の装備の bonus + soul perks のみが英雄ステに乗る。所持していても
//!    装着しなければ効果は出ない (= 「どれを装着するか」が選択になる)
//!
//! 数値バランス (敵スケーリング・装備強化の per-level 値など) は
//! `config::BalanceConfig` を介して注入される。本体ゲームは既定値を、
//! シミュレータは差し替えた config を渡すことで難易度を変えられる。

use std::cell::Cell;

use super::config::BalanceConfig;

/// 魂の永続強化。死亡しても残り、全体倍率を提供する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoulPerk {
    /// 魂の力: 全攻撃力に +5% / level
    Might,
    /// 鋼の意志: 全HPに +5% / level
    Endurance,
    /// 富の加護: gold +10% / level
    Fortune,
    /// 不死の刻印: 死亡時に魂 +20% / level
    Reaper,
}

impl SoulPerk {
    pub fn all() -> &'static [SoulPerk] {
        &[
            SoulPerk::Might,
            SoulPerk::Endurance,
            SoulPerk::Fortune,
            SoulPerk::Reaper,
        ]
    }

    pub fn index(self) -> usize {
        match self {
            SoulPerk::Might => 0,
            SoulPerk::Endurance => 1,
            SoulPerk::Fortune => 2,
            SoulPerk::Reaper => 3,
        }
    }

    pub fn from_index(idx: usize) -> Option<SoulPerk> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            SoulPerk::Might => "魂の力",
            SoulPerk::Endurance => "鋼の意志",
            SoulPerk::Fortune => "富の加護",
            SoulPerk::Reaper => "不死の刻印",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            SoulPerk::Might => "ATK +5%",
            SoulPerk::Endurance => "HP +5%",
            SoulPerk::Fortune => "金 +10%",
            SoulPerk::Reaper => "死亡時の魂 +20%",
        }
    }

    pub fn base_cost(self) -> u64 {
        match self {
            SoulPerk::Might => 3,
            SoulPerk::Endurance => 3,
            SoulPerk::Fortune => 5,
            SoulPerk::Reaper => 8,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            SoulPerk::Might => 1.45,
            SoulPerk::Endurance => 1.45,
            SoulPerk::Fortune => 1.50,
            SoulPerk::Reaper => 1.60,
        }
    }
}

/// 装備の系統。lane 内では前装備が次の前提になる連鎖構造、かつ装着スロットの
/// 単位でもある (各 lane に 1 つだけ装着 = `equipped[lane.index()]`)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentLane {
    /// 武器系統 (主に ATK)。
    Weapon,
    /// 防具系統 (主に HP / DEF)。
    Armor,
    /// 装飾系統 (Speed / Crit / Regen / Gold の混合)。
    Accessory,
}

/// 装備スロットの総数。`AbyssState::equipped` の配列サイズに使う。
pub const LANE_COUNT: usize = 3;

impl EquipmentLane {
    /// 全 lane を宣言順で返す。装着スロットの並び順 SSOT。
    pub fn all() -> &'static [EquipmentLane] {
        &[
            EquipmentLane::Weapon,
            EquipmentLane::Armor,
            EquipmentLane::Accessory,
        ]
    }

    /// 装着スロット配列のインデックス (`AbyssState::equipped[lane.index()]`)。
    pub fn index(self) -> usize {
        match self {
            EquipmentLane::Weapon => 0,
            EquipmentLane::Armor => 1,
            EquipmentLane::Accessory => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EquipmentLane::Weapon => "武器",
            EquipmentLane::Armor => "防具",
            EquipmentLane::Accessory => "装飾",
        }
    }
}

/// 装備の識別子。各 lane に 6 段階で計 18 種。
///
/// 数値・解放条件・効果は `BalanceConfig::equipment` 経由でデータドリブンに
/// 持つ。enum 自体は識別と `index()` / `lane()` / `lane_index()` の構造情報だけを担う。
///
/// 並びは `all()` の順 (lane ごとに lane_index 昇順)。`index()` は save key として
/// 使うので、新装備の挿入や順序変更は SAVE_VERSION の bump を伴う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentId {
    // 武器 lane (6 段階)
    BronzeSword,
    IronSword,
    SteelSword,
    MithrilSword,
    DragonboneSword,
    GodSword,
    // 防具 lane (6 段階)
    LeatherArmor,
    Chainmail,
    SteelArmor,
    MithrilArmor,
    DragonscaleArmor,
    GodArmor,
    // 装飾 lane (6 段階)
    SwiftBoots,
    WarriorBracelet,
    TwinWolfRing,
    SageRobe,
    PhoenixWings,
    EndingCrown,
}

/// 装備の総数。`AbyssState::owned_equipment` 配列のサイズに使う。
pub const EQUIPMENT_COUNT: usize = 18;

impl EquipmentId {
    /// 全装備を宣言順で返す (save id の SSOT)。
    pub fn all() -> &'static [EquipmentId] {
        &[
            EquipmentId::BronzeSword,
            EquipmentId::IronSword,
            EquipmentId::SteelSword,
            EquipmentId::MithrilSword,
            EquipmentId::DragonboneSword,
            EquipmentId::GodSword,
            EquipmentId::LeatherArmor,
            EquipmentId::Chainmail,
            EquipmentId::SteelArmor,
            EquipmentId::MithrilArmor,
            EquipmentId::DragonscaleArmor,
            EquipmentId::GodArmor,
            EquipmentId::SwiftBoots,
            EquipmentId::WarriorBracelet,
            EquipmentId::TwinWolfRing,
            EquipmentId::SageRobe,
            EquipmentId::PhoenixWings,
            EquipmentId::EndingCrown,
        ]
    }

    pub fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|&e| e == self)
            .expect("EquipmentId variant must appear in EquipmentId::all()")
    }

    pub fn from_index(idx: usize) -> Option<EquipmentId> {
        Self::all().get(idx).copied()
    }

    pub fn lane(self) -> EquipmentLane {
        match self {
            EquipmentId::BronzeSword
            | EquipmentId::IronSword
            | EquipmentId::SteelSword
            | EquipmentId::MithrilSword
            | EquipmentId::DragonboneSword
            | EquipmentId::GodSword => EquipmentLane::Weapon,
            EquipmentId::LeatherArmor
            | EquipmentId::Chainmail
            | EquipmentId::SteelArmor
            | EquipmentId::MithrilArmor
            | EquipmentId::DragonscaleArmor
            | EquipmentId::GodArmor => EquipmentLane::Armor,
            EquipmentId::SwiftBoots
            | EquipmentId::WarriorBracelet
            | EquipmentId::TwinWolfRing
            | EquipmentId::SageRobe
            | EquipmentId::PhoenixWings
            | EquipmentId::EndingCrown => EquipmentLane::Accessory,
        }
    }

    /// lane 内での段階 (0 が最序盤、5 が最上位)。同 lane の前段階が `lane_index() - 1`。
    pub fn lane_index(self) -> usize {
        match self {
            EquipmentId::BronzeSword | EquipmentId::LeatherArmor | EquipmentId::SwiftBoots => 0,
            EquipmentId::IronSword | EquipmentId::Chainmail | EquipmentId::WarriorBracelet => 1,
            EquipmentId::SteelSword | EquipmentId::SteelArmor | EquipmentId::TwinWolfRing => 2,
            EquipmentId::MithrilSword | EquipmentId::MithrilArmor | EquipmentId::SageRobe => 3,
            EquipmentId::DragonboneSword
            | EquipmentId::DragonscaleArmor
            | EquipmentId::PhoenixWings => 4,
            EquipmentId::GodSword | EquipmentId::GodArmor | EquipmentId::EndingCrown => 5,
        }
    }
}

/// 装備が hero ステータスに与える効果の集合。
///
/// 加算 (`*_flat`) と乗算 % (`*_pct`) を分けて持つ。乗算は同種を合算した上で
/// 「(base + flat) × (1 + sum_of_pct)」の形で適用する (soul perk と同じ流儀)。
/// chain 乗算ではなく additive にしている理由は、UI で「合計 +X%」と
/// 一発提示しやすく、バランス調整も線形で予測しやすいため。
#[derive(Clone, Copy, Debug, Default)]
pub struct EquipmentBonus {
    pub atk_pct: f64,
    pub atk_flat: u64,
    pub hp_pct: f64,
    pub hp_flat: u64,
    pub def_flat: u64,
    pub crit_bonus: f64,
    pub speed_pct: f64,
    pub regen_per_sec: f64,
    pub gold_pct: f64,
}

impl EquipmentBonus {
    /// 2 つの bonus を合成する (additive)。
    pub fn merge(&self, other: &EquipmentBonus) -> EquipmentBonus {
        EquipmentBonus {
            atk_pct: self.atk_pct + other.atk_pct,
            atk_flat: self.atk_flat.saturating_add(other.atk_flat),
            hp_pct: self.hp_pct + other.hp_pct,
            hp_flat: self.hp_flat.saturating_add(other.hp_flat),
            def_flat: self.def_flat.saturating_add(other.def_flat),
            crit_bonus: self.crit_bonus + other.crit_bonus,
            speed_pct: self.speed_pct + other.speed_pct,
            regen_per_sec: self.regen_per_sec + other.regen_per_sec,
            gold_pct: self.gold_pct + other.gold_pct,
        }
    }

    /// `base + per_level × level` の合成 (装備の強化レベルから effective bonus を算出)。
    /// 各フィールドが additive にスケールするので、装備ごとに per-level の伸び方を変えられる。
    pub fn scaled(base: &EquipmentBonus, per_level: &EquipmentBonus, level: u32) -> EquipmentBonus {
        let l = level as f64;
        // u64 系は per_level × level の multiplication で over-shoot しないよう u128 経由。
        let scale_u64 = |b: u64, p: u64| -> u64 {
            let total = b as u128 + (p as u128) * (level as u128);
            total.min(u64::MAX as u128) as u64
        };
        EquipmentBonus {
            atk_pct: base.atk_pct + per_level.atk_pct * l,
            atk_flat: scale_u64(base.atk_flat, per_level.atk_flat),
            hp_pct: base.hp_pct + per_level.hp_pct * l,
            hp_flat: scale_u64(base.hp_flat, per_level.hp_flat),
            def_flat: scale_u64(base.def_flat, per_level.def_flat),
            crit_bonus: base.crit_bonus + per_level.crit_bonus * l,
            speed_pct: base.speed_pct + per_level.speed_pct * l,
            regen_per_sec: base.regen_per_sec + per_level.regen_per_sec * l,
            gold_pct: base.gold_pct + per_level.gold_pct * l,
        }
    }
}

/// 現在対峙している敵。スポーンの度に作り直される。
#[derive(Clone, Debug)]
pub struct Enemy {
    pub name: String,
    pub max_hp: u64,
    pub hp: u64,
    pub atk: u64,
    pub def: u64,
    pub gold: u64,
    pub is_boss: bool,
    /// 攻撃クールダウン (tick単位)。0 になると攻撃。
    pub atk_cooldown: u32,
    pub atk_period: u32,
}

/// メイン画面のタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// 強化サブタブ。**装着中の装備** を gold で強化する。
    /// 装備が進行軸の主役になったため、本タブは「いま装着している 3 装備の Lv 上げ」専用。
    Upgrades,
    /// 進捗サブタブ。100F ゴールに対する現在地、最深記録、節目フロアの一覧を表示。
    Roadmap,
    Stats,
    Gacha,
    /// 設定 (自動潜行 ON/OFF など、頻繁に切り替えない項目)。
    Settings,
    /// 装備ショップサブタブ。lane ごとの購入と装着切替。
    Shop,
    /// 魂サブタブ。死亡で蓄積される魂による永続バフ。
    Souls,
}

impl Tab {
    /// 全タブを宣言順に返す。**SSOT**: save id・テストイテレーションの基準。
    #[cfg(any(target_arch = "wasm32", test))]
    pub const fn all() -> &'static [Tab] {
        &[
            Tab::Upgrades,
            Tab::Roadmap,
            Tab::Stats,
            Tab::Gacha,
            Tab::Settings,
            Tab::Shop,
            Tab::Souls,
        ]
    }

    /// セーブデータ用のタブ番号 (`all()` 内のインデックス)。
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        Self::all()
            .iter()
            .position(|&t| t == self)
            .expect("Tab variant must appear in Tab::all()") as u8
    }

    /// セーブデータからタブを復元。未知の id は `Upgrades` にフォールバック。
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn from_save_id(id: u8) -> Self {
        Self::all()
            .get(id as usize)
            .copied()
            .unwrap_or(Tab::Upgrades)
    }
}

/// メインメニューの上位グルーピング (UI only)。
///
/// 6 タブを 4 グループに畳んでメインメニューの認知負荷を下げる:
///
/// | グループ | 内訳 | 役割 |
/// |---|---|---|
/// | 育成 | 強化 + 装備 + 魂 | 「投資して強くなる」系 |
/// | 情報 | 進捗 + 統計 | 「見るだけ」系 |
/// | ガチャ | (単独) | 鍵消費の独自 UX |
/// | 設定 | (単独) | 頻度低、独立性高 |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabGroup {
    Growth,
    Info,
    Gacha,
    Settings,
}

impl TabGroup {
    /// 全グループを宣言順 (= タブバー描画順) で返す。
    /// `Tab::all()` と違い save id 用途は無いので test 限定で OK
    /// (UI 側のタブバー描画は `tabs()` 経由で個別の TabGroup を直接書き出している)。
    #[cfg(test)]
    pub const fn all() -> &'static [TabGroup] {
        &[
            TabGroup::Growth,
            TabGroup::Info,
            TabGroup::Gacha,
            TabGroup::Settings,
        ]
    }

    /// このグループに属する `Tab` をサブタブの並び順で返す。
    pub fn tabs(self) -> &'static [Tab] {
        match self {
            // 育成は 3 サブタブ: 強化 (装着中装備の Lv 上げ) / 装備 (購入と装着) / 魂 (souls)
            TabGroup::Growth => &[Tab::Upgrades, Tab::Shop, Tab::Souls],
            TabGroup::Info => &[Tab::Roadmap, Tab::Stats],
            TabGroup::Gacha => &[Tab::Gacha],
            TabGroup::Settings => &[Tab::Settings],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TabGroup::Growth => "育成",
            TabGroup::Info => "情報",
            TabGroup::Gacha => "ガチャ",
            TabGroup::Settings => "設定",
        }
    }

    pub fn default_tab(self) -> Tab {
        self.tabs()[0]
    }

    pub fn from_tab(tab: Tab) -> TabGroup {
        match tab {
            Tab::Upgrades | Tab::Shop | Tab::Souls => TabGroup::Growth,
            Tab::Roadmap | Tab::Stats => TabGroup::Info,
            Tab::Gacha => TabGroup::Gacha,
            Tab::Settings => TabGroup::Settings,
        }
    }

    pub fn has_subtabs(self) -> bool {
        self.tabs().len() > 1
    }
}

/// 現フロアの種別。フロア降下時に抽選され、敵の数値修正と報酬倍率を変える。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorKind {
    Normal,
    Treasure,
    Elite,
    Bonanza,
}

impl FloorKind {
    #[cfg(any(target_arch = "wasm32", test))]
    pub const fn all() -> &'static [FloorKind] {
        &[
            FloorKind::Normal,
            FloorKind::Treasure,
            FloorKind::Elite,
            FloorKind::Bonanza,
        ]
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        Self::all()
            .iter()
            .position(|&k| k == self)
            .expect("FloorKind variant must appear in FloorKind::all()") as u8
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn from_save_id(id: u8) -> Self {
        Self::all()
            .get(id as usize)
            .copied()
            .unwrap_or(FloorKind::Normal)
    }

    pub fn name(self) -> &'static str {
        match self {
            FloorKind::Normal => "通常",
            FloorKind::Treasure => "宝物",
            FloorKind::Elite => "精鋭",
            FloorKind::Bonanza => "豊穣",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            FloorKind::Normal => "",
            FloorKind::Treasure => "💎",
            FloorKind::Elite => "⚡",
            FloorKind::Bonanza => "🌟",
        }
    }

    pub fn enemy_hp_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 1.0,
            FloorKind::Elite => 1.5,
            FloorKind::Bonanza => 0.5,
        }
    }

    pub fn enemy_atk_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 1.0,
            FloorKind::Elite => 1.5,
            FloorKind::Bonanza => 1.0,
        }
    }

    pub fn gold_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 3.0,
            FloorKind::Elite => 2.0,
            FloorKind::Bonanza => 5.0,
        }
    }

    pub fn bonus_keys_on_boss(self) -> u64 {
        match self {
            FloorKind::Elite => 2,
            _ => 0,
        }
    }
}

/// ガチャの結果カテゴリ。確率と効果は `logic::resolve_gacha_pull` で実装。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GachaTier {
    Common,
    Rare,
    Epic,
    Legendary,
}

/// 直近ガチャ結果のサマリ (UI フラッシュ用)。
#[derive(Clone, Debug)]
pub struct GachaResultSummary {
    pub count: u32,
    pub by_tier: [u32; 4],
    pub gained_gold: u64,
    pub gained_souls: u64,
    pub gained_keys: u64,
    /// 装着中装備の強化 Lv を底上げした累計。
    pub gained_enh_lv: u32,
    pub life_ticks: u32,
}

/// ゲームのルート状態。
pub struct AbyssState {
    /// 難易度バランス。state ごとに 1 つ持ち、tick 内の計算は全てこれを参照する。
    pub config: BalanceConfig,

    // ── 永続強化 (装備系) ──
    /// 解放済み装備フラグ (`EquipmentId::index()` でアクセス)。
    /// 一度購入したら永続。装着しているかは `equipped` 側で持つ。
    pub owned_equipment: [bool; EQUIPMENT_COUNT],
    /// 各装備の強化レベル (gold で 1 ずつ伸ばす)。所持していなくても 0 で常に存在する。
    /// 効果は装着中だけ反映されるが、強化情報そのものは装備に紐付けて保持する
    /// (= 一度伸ばした強化はその装備にずっと残る、付け替えしても消えない)。
    pub equipment_levels: [u32; EQUIPMENT_COUNT],
    /// 各 lane に装着中の装備。所持済み装備からしか選べない。
    /// `equipped[lane.index()]` でアクセス (lane は `EquipmentLane::index()` 経由)。
    pub equipped: [Option<EquipmentId>; LANE_COUNT],

    // ── 永続強化 (魂系) ──
    pub soul_perks: [u32; 4],
    pub souls: u64,

    // ── ラン (1回の冒険) 単位の状態 ──
    pub gold: u64,
    pub floor: u32,
    pub max_floor: u32,
    pub kills_on_floor: u32,
    pub run_kills: u64,
    pub run_gold_earned: u64,

    /// hero の現在 HP。max_hp は装備 + soul_perks から導出する。
    pub hero_hp: u64,
    pub hero_atk_cooldown: u32,
    pub hero_regen_acc_x100: u32,
    pub combat_focus: u32,

    pub current_enemy: Enemy,
    pub floor_kind: FloorKind,

    // ── プレイヤー設定 ──
    pub auto_descend: bool,
    pub tab: Tab,
    /// 現在のタブ本体の縦スクロール量 (visual rows)。
    /// **UI only**: simulator / logic は本質的には触らない、永続化もしない。
    pub tab_scroll: Cell<u16>,

    // ── ガチャ ──
    pub keys: u64,
    pub pulls_since_epic: u32,
    pub total_pulls: u64,
    pub last_gacha: Option<GachaResultSummary>,

    // ── 永続統計 ──
    pub deepest_floor_ever: u32,
    pub total_kills: u64,
    pub deaths: u64,

    // ── 演出 / ログ ──
    pub log: Vec<String>,
    pub hero_hurt_flash: u32,
    pub enemy_hurt_flash: u32,
    pub last_enemy_damage: Option<(u64, u32, bool)>,
    pub last_hero_damage: Option<(u64, u32)>,
    pub descent_flash: u32,

    pub total_ticks: u64,
    pub rng_state: u32,
}

impl AbyssState {
    pub fn new() -> Self {
        Self::with_config(BalanceConfig::default())
    }

    /// 任意の難易度設定で状態を作る。シミュレータ用。
    pub fn with_config(config: BalanceConfig) -> Self {
        let mut s = Self {
            config,
            owned_equipment: [false; EQUIPMENT_COUNT],
            equipment_levels: [0; EQUIPMENT_COUNT],
            equipped: [None; LANE_COUNT],
            soul_perks: [0; 4],
            souls: 0,
            gold: 0,
            floor: 1,
            max_floor: 1,
            kills_on_floor: 0,
            run_kills: 0,
            run_gold_earned: 0,
            hero_hp: 0,
            hero_atk_cooldown: 0,
            hero_regen_acc_x100: 0,
            combat_focus: 0,
            current_enemy: placeholder_enemy(),
            floor_kind: FloorKind::Normal,
            auto_descend: true,
            tab: Tab::Upgrades,
            tab_scroll: Cell::new(0),
            keys: 0,
            pulls_since_epic: 0,
            total_pulls: 0,
            last_gacha: None,
            deepest_floor_ever: 1,
            total_kills: 0,
            deaths: 0,
            log: Vec::new(),
            hero_hurt_flash: 0,
            enemy_hurt_flash: 0,
            last_enemy_damage: None,
            last_hero_damage: None,
            descent_flash: 0,
            total_ticks: 0,
            rng_state: 0xC0FFEE,
        };
        s.hero_hp = s.hero_max_hp();
        s.hero_atk_cooldown = s.hero_atk_period();
        s
    }

    /// 装着中装備の合計効果。各 lane の装着スロットを見て、装備の強化 Lv を
    /// 反映した bonus を additive で合算する。所持しているだけで装着していない
    /// 装備は無視される (= 「装備を選ぶ」というプレイヤー行動が意味を持つ)。
    pub fn equipment_bonus(&self) -> EquipmentBonus {
        let mut total = EquipmentBonus::default();
        for slot in self.equipped.iter().flatten() {
            if let Some(def) = self.config.equipment.get(slot.index()) {
                let lv = self.equipment_levels[slot.index()];
                let scaled = EquipmentBonus::scaled(&def.base_bonus, &def.per_level_bonus, lv);
                total = total.merge(&scaled);
            }
        }
        total
    }

    /// 現在の最大 HP (base + 装着中装備 + soul perks)。
    pub fn hero_max_hp(&self) -> u64 {
        let h = &self.config.hero;
        let eq = self.equipment_bonus();
        let base = h.base_hp + eq.hp_flat;
        let endurance_lv = self.soul_perks[SoulPerk::Endurance.index()];
        let mult = 1.0 + endurance_lv as f64 * h.endurance_per_lv + eq.hp_pct;
        ((base as f64) * mult).round() as u64
    }

    pub fn hero_atk(&self) -> u64 {
        let h = &self.config.hero;
        let eq = self.equipment_bonus();
        let base = h.base_atk + eq.atk_flat;
        let might_lv = self.soul_perks[SoulPerk::Might.index()];
        let mult = 1.0 + might_lv as f64 * h.might_per_lv + eq.atk_pct;
        ((base as f64) * mult).round() as u64
    }

    pub fn hero_def(&self) -> u64 {
        let h = &self.config.hero;
        let eq = self.equipment_bonus();
        h.base_def + eq.def_flat
    }

    /// クリティカル率 (0.0..=`crit_cap`)。装備の crit_bonus が源泉。
    pub fn hero_crit_rate(&self) -> f64 {
        let h = &self.config.hero;
        let eq = self.equipment_bonus();
        eq.crit_bonus.min(h.crit_cap)
    }

    /// 1 攻撃にかかる tick 数。装備の speed_pct と戦闘集中で短縮、`atk_period_min` を下限。
    pub fn hero_atk_period(&self) -> u32 {
        let h = &self.config.hero;
        let eq = self.equipment_bonus();
        let speed_mult = 1.0 + eq.speed_pct;
        let focus_factor = self.focus_factor();
        let period = (h.atk_period_base as f64 * focus_factor / speed_mult).round() as u32;
        period.max(h.atk_period_min)
    }

    /// 戦闘集中による攻撃間隔の倍率 (0..=1)。focus が増えるほど小さくなる。
    pub fn focus_factor(&self) -> f64 {
        let h = &self.config.hero;
        let focus = self.combat_focus.min(h.focus_max) as f64;
        (1.0 - focus * h.focus_reduction_per_point).max(0.1)
    }

    /// HP regen (HP/秒)。装備のみが源泉。
    pub fn hero_regen_per_sec(&self) -> f64 {
        let eq = self.equipment_bonus();
        eq.regen_per_sec
    }

    /// gold 取得倍率 (1.0 + 装備 + soul perks)。
    pub fn gold_multiplier(&self) -> f64 {
        let h = &self.config.hero;
        let fortune_lv = self.soul_perks[SoulPerk::Fortune.index()];
        let eq = self.equipment_bonus();
        1.0 + fortune_lv as f64 * h.fortune_per_lv + eq.gold_pct
    }

    /// 撃破時の魂取得倍率 (Reaper perk 由来)。
    pub fn soul_multiplier(&self) -> f64 {
        let lv = self.soul_perks[SoulPerk::Reaper.index()];
        1.0 + lv as f64 * self.config.hero.reaper_per_lv
    }

    /// 1 階層あたりに倒すべき雑魚数。
    pub fn enemies_per_floor(&self) -> u32 {
        self.config.pacing.enemies_per_floor
    }

    /// ダンジョンの到達ゴールフロア。
    pub fn goal_floor(&self) -> u32 {
        self.config.pacing.goal_floor
    }

    /// 装備 1 段階強化のコスト (Lv L → Lv L+1)。
    /// 所持していなくてもコストだけは計算できる (UI で「次の強化費用」を見せるため)。
    pub fn enhance_cost(&self, id: EquipmentId) -> u64 {
        let def = match self.config.equipment.get(id.index()) {
            Some(d) => d,
            None => return u64::MAX,
        };
        let lv = self.equipment_levels[id.index()] as f64;
        let cost = (def.enh_cost_base as f64) * def.enh_cost_growth.powf(lv);
        cost.round() as u64
    }

    pub fn soul_perk_cost(&self, perk: SoulPerk) -> u64 {
        let lv = self.soul_perks[perk.index()] as f64;
        let cost = (perk.base_cost() as f64) * perk.growth().powf(lv);
        cost.round() as u64
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    /// ボス出現までの残り敵数。
    pub fn enemies_until_boss(&self) -> u32 {
        self.enemies_per_floor().saturating_sub(self.kills_on_floor)
    }

    /// 装着中装備のうち、指定 lane のものを返す (None = その lane は未装着)。
    pub fn equipped_at(&self, lane: EquipmentLane) -> Option<EquipmentId> {
        self.equipped[lane.index()]
    }
}

/// hp/max_hp が 0 だと "次の tick で本物の敵をスポーン" の合図になる。
fn placeholder_enemy() -> Enemy {
    Enemy {
        name: String::new(),
        max_hp: 0,
        hp: 0,
        atk: 1,
        def: 0,
        gold: 0,
        is_boss: false,
        atk_cooldown: 20,
        atk_period: 20,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_save_id_roundtrip() {
        for &tab in Tab::all() {
            let id = tab.to_save_id();
            assert_eq!(Tab::from_save_id(id), tab, "roundtrip mismatch for {:?}", tab);
        }
        assert_eq!(Tab::from_save_id(255), Tab::Upgrades);
    }

    #[test]
    fn tab_group_round_trip_via_tabs_and_from_tab() {
        for &group in TabGroup::all() {
            for &tab in group.tabs() {
                assert_eq!(TabGroup::from_tab(tab), group);
            }
        }
        for &tab in Tab::all() {
            let g = TabGroup::from_tab(tab);
            assert!(g.tabs().contains(&tab));
        }
        for &group in TabGroup::all() {
            assert_eq!(group.default_tab(), group.tabs()[0]);
        }
    }

    #[test]
    fn floor_kind_save_id_roundtrip() {
        for &kind in FloorKind::all() {
            let id = kind.to_save_id();
            assert_eq!(FloorKind::from_save_id(id), kind);
        }
        assert_eq!(FloorKind::from_save_id(255), FloorKind::Normal);
    }

    #[test]
    fn initial_state_sane() {
        let s = AbyssState::new();
        assert_eq!(s.floor, 1);
        assert!(s.hero_hp > 0);
        assert_eq!(s.hero_hp, s.hero_max_hp());
        assert_eq!(s.gold, 0);
        assert_eq!(s.souls, 0);
        // 初期状態では装備未所持 → 装着スロットも全て空。
        assert!(s.equipped.iter().all(|slot| slot.is_none()));
    }

    /// 装着スロットの index がそのまま `EquipmentLane` の宣言順と対応していること。
    /// `equipped[lane.index()]` のアクセスが lane と整合する SSOT 検証。
    #[test]
    fn lane_index_matches_all_order() {
        for (i, &lane) in EquipmentLane::all().iter().enumerate() {
            assert_eq!(lane.index(), i, "lane {:?} index mismatch", lane);
        }
    }

    /// 装着中の装備だけが英雄ステに反映されること (所持しているだけでは効果なし)。
    #[test]
    fn owned_but_not_equipped_does_not_buff_stats() {
        let mut s = AbyssState::new();
        let base_atk = s.hero_atk();
        // BronzeSword を所持するが装着しない。
        s.owned_equipment[EquipmentId::BronzeSword.index()] = true;
        assert_eq!(s.hero_atk(), base_atk, "未装着なら ATK は変わらない");
        // 装着すると変わる。
        s.equipped[EquipmentLane::Weapon.index()] = Some(EquipmentId::BronzeSword);
        assert!(s.hero_atk() > base_atk, "装着で ATK が上がる");
    }

    /// 強化 Lv が上がると装着中装備の効果も伸びること。
    #[test]
    fn enhancement_level_scales_equipped_bonus() {
        let mut s = AbyssState::new();
        s.owned_equipment[EquipmentId::BronzeSword.index()] = true;
        s.equipped[EquipmentLane::Weapon.index()] = Some(EquipmentId::BronzeSword);
        let atk_lv0 = s.hero_atk();
        s.equipment_levels[EquipmentId::BronzeSword.index()] = 10;
        let atk_lv10 = s.hero_atk();
        assert!(atk_lv10 > atk_lv0, "強化 Lv で ATK が伸びる");
    }

    #[test]
    fn enhance_cost_grows() {
        let mut s = AbyssState::new();
        let c0 = s.enhance_cost(EquipmentId::BronzeSword);
        s.equipment_levels[EquipmentId::BronzeSword.index()] = 5;
        let c5 = s.enhance_cost(EquipmentId::BronzeSword);
        assert!(c5 > c0);
    }

    #[test]
    fn enemies_per_floor_default() {
        let s = AbyssState::new();
        assert_eq!(s.enemies_per_floor(), 8);
    }

    #[test]
    fn log_truncates() {
        let mut s = AbyssState::new();
        for i in 0..120 {
            s.add_log(format!("msg {i}"));
        }
        assert!(s.log.len() <= 50);
    }
}
