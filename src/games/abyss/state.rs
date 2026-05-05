//! 深淵潜行 (Abyss Idle) — game state.
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs に置く。
//!
//! 数値バランス (敵スケーリング・ヒーローのレベル係数など) は
//! `config::BalanceConfig` を介して注入される。本体ゲームは既定値を、
//! シミュレータは差し替えた config を渡すことで難易度を変えられる。

use std::cell::Cell;

use super::config::BalanceConfig;

/// 強化の種類。各強化は累積購入数 (level) を持ち、レベルが上がるほどコストも上昇する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    /// 剣術: ATK +2 / level
    Sword,
    /// 体力: Max HP +10 / level
    Vitality,
    /// 鎧: DEF +1 / level
    Armor,
    /// 必殺: CRIT +1% / level (cap 60%)
    Crit,
    /// 俊敏: ATK speed +5% / level
    Speed,
    /// 回復: HP regen +0.2/sec / level
    Regen,
    /// 金運: gold gain +5% / level
    Gold,
}

impl UpgradeKind {
    pub fn all() -> &'static [UpgradeKind] {
        &[
            UpgradeKind::Sword,
            UpgradeKind::Vitality,
            UpgradeKind::Armor,
            UpgradeKind::Crit,
            UpgradeKind::Speed,
            UpgradeKind::Regen,
            UpgradeKind::Gold,
        ]
    }

    pub fn index(self) -> usize {
        match self {
            UpgradeKind::Sword => 0,
            UpgradeKind::Vitality => 1,
            UpgradeKind::Armor => 2,
            UpgradeKind::Crit => 3,
            UpgradeKind::Speed => 4,
            UpgradeKind::Regen => 5,
            UpgradeKind::Gold => 6,
        }
    }

    pub fn from_index(idx: usize) -> Option<UpgradeKind> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            UpgradeKind::Sword => "剣術",
            UpgradeKind::Vitality => "体力",
            UpgradeKind::Armor => "鎧",
            UpgradeKind::Crit => "必殺",
            UpgradeKind::Speed => "俊敏",
            UpgradeKind::Regen => "回復",
            UpgradeKind::Gold => "金運",
        }
    }

    /// 1レベル分の効果説明 (UI用)。
    pub fn effect(self) -> &'static str {
        match self {
            UpgradeKind::Sword => "ATK+2",
            UpgradeKind::Vitality => "HP+10",
            UpgradeKind::Armor => "DEF+1",
            UpgradeKind::Crit => "CRIT+1%",
            UpgradeKind::Speed => "速度+5%",
            UpgradeKind::Regen => "回復+0.2/秒",
            UpgradeKind::Gold => "金+5%",
        }
    }

    pub fn base_cost(self) -> u64 {
        match self {
            UpgradeKind::Sword => 10,
            UpgradeKind::Vitality => 12,
            UpgradeKind::Armor => 18,
            UpgradeKind::Crit => 50,
            UpgradeKind::Speed => 80,
            UpgradeKind::Regen => 30,
            UpgradeKind::Gold => 60,
        }
    }

    /// レベル毎のコスト成長率。
    pub fn growth(self) -> f64 {
        match self {
            UpgradeKind::Sword => 1.15,
            UpgradeKind::Vitality => 1.14,
            UpgradeKind::Armor => 1.18,
            UpgradeKind::Crit => 1.25,
            UpgradeKind::Speed => 1.30,
            UpgradeKind::Regen => 1.20,
            UpgradeKind::Gold => 1.22,
        }
    }
}

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

/// 装備の系統。lane 内では前装備が次の前提になる連鎖構造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentLane {
    /// 武器系統 (Sword 強化と連動して伸びる)。
    Weapon,
    /// 防具系統 (Armor + Vitality 強化と連動)。
    Armor,
    /// 装飾系統 (Speed/Crit/Regen/Gold と複合的に絡む)。
    Accessory,
}

impl EquipmentLane {
    pub fn name(self) -> &'static str {
        match self {
            EquipmentLane::Weapon => "武器",
            EquipmentLane::Armor => "防具",
            EquipmentLane::Accessory => "装飾",
        }
    }
}

/// 装備の識別子。各 lane に 4 段階で計 12 種。
///
/// 数値・解放条件・効果は `BalanceConfig::equipment` 経由でデータドリブンに
/// 持つ (UpgradeKind が `base_cost`/`growth` を enum 内に持つのと違い、
/// 装備はバランス調整頻度が高いと予想されるため)。enum 自体は識別と
/// `index()` / `lane()` / `lane_index()` の構造情報だけを担う。
///
/// 並びは `all()` の順。**save id は宣言順固定**: 末尾追加なら旧 save 互換。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentId {
    // 武器 lane
    BronzeSword,
    SteelSword,
    MithrilSword,
    GodSword,
    // 防具 lane
    LeatherArmor,
    SteelArmor,
    MithrilArmor,
    GodArmor,
    // 装飾 lane
    SwiftBoots,
    TwinWolfRing,
    SageRobe,
    EndingCrown,
}

/// 装備の総数。`AbyssState::owned_equipment` 配列のサイズに使う。
pub const EQUIPMENT_COUNT: usize = 12;

impl EquipmentId {
    /// 全装備を宣言順で返す (save id の SSOT)。
    pub fn all() -> &'static [EquipmentId] {
        &[
            EquipmentId::BronzeSword,
            EquipmentId::SteelSword,
            EquipmentId::MithrilSword,
            EquipmentId::GodSword,
            EquipmentId::LeatherArmor,
            EquipmentId::SteelArmor,
            EquipmentId::MithrilArmor,
            EquipmentId::GodArmor,
            EquipmentId::SwiftBoots,
            EquipmentId::TwinWolfRing,
            EquipmentId::SageRobe,
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
            | EquipmentId::SteelSword
            | EquipmentId::MithrilSword
            | EquipmentId::GodSword => EquipmentLane::Weapon,
            EquipmentId::LeatherArmor
            | EquipmentId::SteelArmor
            | EquipmentId::MithrilArmor
            | EquipmentId::GodArmor => EquipmentLane::Armor,
            EquipmentId::SwiftBoots
            | EquipmentId::TwinWolfRing
            | EquipmentId::SageRobe
            | EquipmentId::EndingCrown => EquipmentLane::Accessory,
        }
    }

    /// lane 内での段階 (0 が最序盤)。同 lane の前段階が `lane_index() - 1`。
    pub fn lane_index(self) -> usize {
        match self {
            EquipmentId::BronzeSword | EquipmentId::LeatherArmor | EquipmentId::SwiftBoots => 0,
            EquipmentId::SteelSword | EquipmentId::SteelArmor | EquipmentId::TwinWolfRing => 1,
            EquipmentId::MithrilSword | EquipmentId::MithrilArmor | EquipmentId::SageRobe => 2,
            EquipmentId::GodSword | EquipmentId::GodArmor | EquipmentId::EndingCrown => 3,
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
    /// 強化サブタブ。gold で買う通常強化のみ (魂は `Tab::Souls` に分離)。
    /// 装備が「段階的な達成感」を担うようになったため、本タブは線形な数値投資に
    /// 専念する: 段階バッジ・次段階プレビューは廃止し、Lv + コスト + per-Lv 効果のみ表示。
    Upgrades,
    /// 進捗サブタブ。100F ゴールに対する現在地、最深記録、節目フロアの一覧を表示。
    /// `Tab::all()` 内で旧 `Souls` の位置 (save id 1) を継承し、
    /// 既存セーブの Stats/Gacha/Settings の id を維持する。
    Roadmap,
    Stats,
    Gacha,
    /// 設定 (自動潜行 ON/OFF など、頻繁に切り替えない項目)。
    /// idle 系はメイン画面の縦領域を強化リストに優先したいので、設定系は別タブへ追い出す。
    Settings,
    /// 装備ショップサブタブ。lane 連鎖型の解放ツリー。「次は見える / その先は ???」。
    /// 普段は開かなくて良い (HUD には出さない)。気が向いた時だけ訪問するメニュー。
    Shop,
    /// 魂サブタブ。死亡で蓄積される魂による永続バフ (旧 `Tab::Upgrades` 内の魂セクションを分離)。
    /// 操作頻度が `Tab::Upgrades` と異なる (魂はラン死亡後に集中) ため、サブタブ独立で UX 整理。
    Souls,
}

impl Tab {
    /// 全タブを宣言順に返す。**SSOT**: save id・テストイテレーションの基準。
    /// グループ階層化後は UI のタブバー描画順は `TabGroup::all()` に移ったため、
    /// このリストは save id 専用 SSOT になっている (= save / test だけが利用)。
    /// `to_save_id` / `from_save_id` と同じく cfg gate でデフォルト build の
    /// dead_code を避ける。
    #[cfg(any(target_arch = "wasm32", test))]
    pub const fn all() -> &'static [Tab] {
        &[
            Tab::Upgrades,
            Tab::Roadmap,
            Tab::Stats,
            Tab::Gacha,
            Tab::Settings,
            // 末尾追加: 既存セーブの save id (Upgrades=0..Settings=4) を維持するため
            // 新タブは末尾に置く。UI 上の表示順は `TabGroup` が制御する。
            Tab::Shop,
            Tab::Souls,
        ]
    }

    /// セーブデータ用のタブ番号 (`all()` 内のインデックス)。
    /// 利用箇所が cfg-gated な save.rs / test だけなので、デフォルトビルドでの
    /// dead_code を避けるため同じ gate を付与。
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
/// | 育成 | 強化 + 装備 | 「投資して強くなる」系。アクティブな操作が中心 |
/// | 情報 | 進捗 + 統計 | 「見るだけ」系。たまに開く |
/// | ガチャ | (単独) | 鍵消費の独自 UX があり統合先がない |
/// | 設定 | (単独) | 頻度低、独立性高 |
///
/// **save には保存しない**: 永続化対象は `Tab` のまま。`TabGroup` は
/// `Tab` の関数 (`from_tab`) なので、save から `Tab` を復元すれば group は
/// 一意に決まる。これにより既存セーブとの互換が崩れない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabGroup {
    Growth,   // 強化 + 装備
    Info,     // 進捗 + 統計
    Gacha,
    Settings,
}

impl TabGroup {
    /// 全グループを宣言順 (= タブバー描画順) で返す。
    /// 利用は SSOT 整合性 test だけなので、デフォルトビルドの dead_code を
    /// 避けるため `Tab::all` と同じ cfg-gate を当てる。
    #[cfg(any(target_arch = "wasm32", test))]
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
            // 育成は 3 サブタブ: 強化 (gold) / 装備 (gold + 強化Lv連鎖) / 魂 (souls)
            // 強化と装備は毎分操作する系、魂はラン死亡後に集中する系で頻度差があるため
            // 同じグループ内のサブタブとして並列に並べる。
            TabGroup::Growth => &[Tab::Upgrades, Tab::Shop, Tab::Souls],
            TabGroup::Info => &[Tab::Roadmap, Tab::Stats],
            TabGroup::Gacha => &[Tab::Gacha],
            TabGroup::Settings => &[Tab::Settings],
        }
    }

    /// グループ表示名 (タブバーに出す)。
    pub fn name(self) -> &'static str {
        match self {
            TabGroup::Growth => "育成",
            TabGroup::Info => "情報",
            TabGroup::Gacha => "ガチャ",
            TabGroup::Settings => "設定",
        }
    }

    /// グループ切替時に最初に表示するサブタブ。
    pub fn default_tab(self) -> Tab {
        self.tabs()[0]
    }

    /// 任意の `Tab` から所属グループを引き当てる (逆引き)。
    /// `tabs()` の構成と必ず一致するように `match` で網羅する。
    pub fn from_tab(tab: Tab) -> TabGroup {
        match tab {
            Tab::Upgrades | Tab::Shop | Tab::Souls => TabGroup::Growth,
            Tab::Roadmap | Tab::Stats => TabGroup::Info,
            Tab::Gacha => TabGroup::Gacha,
            Tab::Settings => TabGroup::Settings,
        }
    }

    /// このグループにサブタブが複数あるか (= サブタブバーを描く必要があるか)。
    pub fn has_subtabs(self) -> bool {
        self.tabs().len() > 1
    }
}

/// 現フロアの種別。フロア降下時に抽選され、敵の数値修正と報酬倍率を変える。
/// 1〜2 階は必ず Normal にする (序盤は把握しやすさを優先)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorKind {
    /// 通常フロア。
    Normal,
    /// 宝物フロア。雑魚も含めて gold ×3、敵 HP は通常通り。
    Treasure,
    /// 精鋭フロア。敵 HP/ATK ×1.5、gold ×2、ボス撃破時に鍵 +2 (合計 +3)。
    Elite,
    /// 豊穣フロア。敵 HP -50%、gold ×5。レアな当たり階。
    Bonanza,
}

impl FloorKind {
    /// 全フロア種別を宣言順に返す。**SSOT**: save id はこの順で振られる。
    /// 抽選確率テーブル等とは別概念なので注意 (確率は別途 config 側で持つ)。
    /// 現状 save 系統からのみ利用するので cfg-gate しているが、将来 UI 等で
    /// イテレーションしたくなったら gate を外す。
    #[cfg(any(target_arch = "wasm32", test))]
    pub const fn all() -> &'static [FloorKind] {
        &[
            FloorKind::Normal,
            FloorKind::Treasure,
            FloorKind::Elite,
            FloorKind::Bonanza,
        ]
    }

    /// セーブデータ用の番号 (`all()` 内のインデックス)。
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        Self::all()
            .iter()
            .position(|&k| k == self)
            .expect("FloorKind variant must appear in FloorKind::all()") as u8
    }

    /// セーブデータから復元。未知の id は `Normal` フォールバック。
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

    /// 敵 HP の倍率。
    pub fn enemy_hp_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 1.0,
            FloorKind::Elite => 1.5,
            FloorKind::Bonanza => 0.5,
        }
    }

    /// 敵 ATK の倍率。
    pub fn enemy_atk_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 1.0,
            FloorKind::Elite => 1.5,
            FloorKind::Bonanza => 1.0,
        }
    }

    /// gold ドロップの倍率。
    pub fn gold_mult(self) -> f64 {
        match self {
            FloorKind::Normal => 1.0,
            FloorKind::Treasure => 3.0,
            FloorKind::Elite => 2.0,
            FloorKind::Bonanza => 5.0,
        }
    }

    /// このフロアのボス撃破時の追加鍵数。
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
    /// 引いた回数。
    pub count: u32,
    /// 等級ごとの当選数 [Common, Rare, Epic, Legendary]。
    pub by_tier: [u32; 4],
    /// 累計の獲得 gold / souls / keys / upgrade level / soul_perk level。
    pub gained_gold: u64,
    pub gained_souls: u64,
    pub gained_keys: u64,
    pub gained_upgrade_lv: u32,
    /// 演出用残 tick。
    pub life_ticks: u32,
}

/// ゲームのルート状態。
pub struct AbyssState {
    /// 難易度バランス。state ごとに 1 つ持ち、tick 内の計算は全てこれを参照する。
    /// 本体ゲームは `BalanceConfig::default()`、シミュレータはカスタムを注入。
    pub config: BalanceConfig,

    // ── プレイヤー (ラン中もリセットされない、永続強化分のレベル) ──
    pub upgrades: [u32; 7],
    pub soul_perks: [u32; 4],
    pub souls: u64,
    /// 解放済み装備フラグ (`EquipmentId::index()` でアクセス)。
    /// 一度解放したら永続。付け替え無し、効果は累積加算。
    pub owned_equipment: [bool; EQUIPMENT_COUNT],

    // ── ラン (1回の冒険) 単位の状態 ──
    pub gold: u64,
    pub floor: u32,
    pub max_floor: u32,
    pub kills_on_floor: u32,
    /// このラン中に撃破した敵の総数。
    pub run_kills: u64,
    /// このランで稼いだ gold の総量 (統計用)。
    pub run_gold_earned: u64,

    /// hero の現在 HP。max_hp は upgrades と soul_perks から導出する。
    pub hero_hp: u64,
    /// hero の攻撃進捗 (tick 単位)。`hero_atk_period` 経過するごとに攻撃。
    pub hero_atk_cooldown: u32,
    /// hero の HP regen の小数累積 (1.0 ごとに 1 HP 回復)。
    pub hero_regen_acc_x100: u32,
    /// 戦闘集中 (combat focus)。攻撃成功で +1 (focus_max まで)、被弾で減少、
    /// 死亡や撤退で 0 にリセット。攻撃間隔を微量ずつ短縮していく。
    pub combat_focus: u32,

    pub current_enemy: Enemy,
    /// 現フロアの種別。
    pub floor_kind: FloorKind,

    // ── プレイヤー設定 ──
    pub auto_descend: bool,
    pub tab: Tab,
    /// 現在のタブ本体の縦スクロール量 (visual rows)。
    ///
    /// **UI only**: simulator / logic は本質的には触らない、永続化もしない
    /// (セッション限定)。タブ切替で 0 にリセット
    /// (`logic::apply_action::SetTab` で処理)。
    ///
    /// `Cell<u16>` なのは、Game::render が `&self` のため、上限 clamp を
    /// render 直前に行うために interior mutability が必要なため。
    /// (Game trait シグネチャ変更を避ける目的。logic からは set/get で書ける)
    pub tab_scroll: Cell<u16>,

    // ── ガチャ ──
    /// 蓄積した深淵の鍵 (ガチャ通貨)。永続。
    pub keys: u64,
    /// 直近の Epic+ 以降に引いた回数。50 で Epic+ 確定 (天井)。
    pub pulls_since_epic: u32,
    /// このセッションの累計引き回数。
    pub total_pulls: u64,
    /// 直近のガチャ結果 (UI 表示用)。
    pub last_gacha: Option<GachaResultSummary>,

    // ── 永続統計 ──
    pub deepest_floor_ever: u32,
    pub total_kills: u64,
    pub deaths: u64,

    // ── 演出 / ログ ──
    pub log: Vec<String>,
    /// hero が攻撃を受けた直後 (赤フラッシュ用 tick)。
    pub hero_hurt_flash: u32,
    /// 敵が攻撃を受けた直後 (黄フラッシュ用 tick)。
    pub enemy_hurt_flash: u32,
    /// 直近の hero ダメージ表示 (タプル: amount, life_ticks)。
    pub last_enemy_damage: Option<(u64, u32, bool)>, // (amount, ticks_left, is_crit)
    pub last_hero_damage: Option<(u64, u32)>,
    /// 階層遷移の演出 tick 残り (0 なら通常表示)。
    pub descent_flash: u32,

    /// ラン中の総tick (デバッグ・統計用)。
    pub total_ticks: u64,

    /// シンプルRNG。
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
            upgrades: [0; 7],
            soul_perks: [0; 4],
            souls: 0,
            owned_equipment: [false; EQUIPMENT_COUNT],
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

    /// 装備の合計効果。所持中の `EquipmentDef::bonus` を additive で合算する。
    /// 毎フレーム呼ばれうるが装備数は最大 12 なので O(N) で十分。
    pub fn equipment_bonus(&self) -> EquipmentBonus {
        let mut total = EquipmentBonus::default();
        for def in self.config.equipment.iter() {
            if self.owned_equipment[def.id.index()] {
                total = total.merge(&def.bonus);
            }
        }
        total
    }

    /// 現在の最大 HP (upgrades + soul perks + 装備の合算)。
    ///
    /// 計算式: `(base + curve_cumul + equipment_flat) × (1 + endurance_pct + equipment_pct)`
    /// pct は additive で合算してから掛ける (chain 乗算ではない)。
    pub fn hero_max_hp(&self) -> u64 {
        let h = &self.config.hero;
        let lv = self.upgrades[UpgradeKind::Vitality.index()];
        let upgrade_hp = match &h.vitality_curve {
            Some(curve) => curve.cumulative(lv).round() as u64,
            None => lv as u64 * h.hp_per_vitality_lv,
        };
        let eq = self.equipment_bonus();
        let base = h.base_hp + upgrade_hp + eq.hp_flat;
        let endurance_lv = self.soul_perks[SoulPerk::Endurance.index()];
        let mult = 1.0 + endurance_lv as f64 * h.endurance_per_lv + eq.hp_pct;
        ((base as f64) * mult).round() as u64
    }

    pub fn hero_atk(&self) -> u64 {
        let h = &self.config.hero;
        let lv = self.upgrades[UpgradeKind::Sword.index()];
        let upgrade_atk = match &h.sword_curve {
            Some(curve) => curve.cumulative(lv).round() as u64,
            None => lv as u64 * h.atk_per_sword_lv,
        };
        let eq = self.equipment_bonus();
        let base = h.base_atk + upgrade_atk + eq.atk_flat;
        let might_lv = self.soul_perks[SoulPerk::Might.index()];
        let mult = 1.0 + might_lv as f64 * h.might_per_lv + eq.atk_pct;
        ((base as f64) * mult).round() as u64
    }

    /// 指定強化のカーブ参照を返す (curve 未定義なら None)。
    /// 内部の hero stat 計算 (`hero_atk` / `hero_max_hp` 等) で `cumulative()` を使うために保持。
    /// UI からは段階表示を撤去した (装備が段階の役目を担うため) ので、UI で
    /// このメソッドを呼ぶ箇所は無い。
    pub fn upgrade_curve(&self, kind: UpgradeKind) -> Option<&super::config::TierCurve> {
        let h = &self.config.hero;
        match kind {
            UpgradeKind::Sword => h.sword_curve.as_ref(),
            UpgradeKind::Vitality => h.vitality_curve.as_ref(),
            UpgradeKind::Armor => h.armor_curve.as_ref(),
            UpgradeKind::Speed => h.speed_curve.as_ref(),
            // Crit/Regen/Gold は段階制未対応 (cap や倍率の都合で別設計が必要)
            _ => None,
        }
    }

    pub fn hero_def(&self) -> u64 {
        let h = &self.config.hero;
        let lv = self.upgrades[UpgradeKind::Armor.index()];
        let upgrade_def = match &h.armor_curve {
            Some(curve) => curve.cumulative(lv).round() as u64,
            None => lv as u64 * h.def_per_armor_lv,
        };
        let eq = self.equipment_bonus();
        h.base_def + upgrade_def + eq.def_flat
    }

    /// クリティカル率 (0.0..=`crit_cap`)。装備の crit_bonus も加算。
    pub fn hero_crit_rate(&self) -> f64 {
        let h = &self.config.hero;
        let lv = self.upgrades[UpgradeKind::Crit.index()] as f64;
        let eq = self.equipment_bonus();
        (lv * h.crit_per_lv + eq.crit_bonus).min(h.crit_cap)
    }

    /// 1 攻撃にかかる tick 数。SPD upgrade と戦闘集中で短縮、`atk_period_min` を下限。
    pub fn hero_atk_period(&self) -> u32 {
        let h = &self.config.hero;
        let lv = self.upgrades[UpgradeKind::Speed.index()];
        let speed_bonus = match &h.speed_curve {
            Some(curve) => curve.cumulative(lv),
            None => lv as f64 * h.speed_per_lv,
        };
        let eq = self.equipment_bonus();
        let speed_mult = 1.0 + speed_bonus + eq.speed_pct;
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

    /// HP regen (HP/秒)。装備の regen_per_sec も加算。
    pub fn hero_regen_per_sec(&self) -> f64 {
        let eq = self.equipment_bonus();
        self.upgrades[UpgradeKind::Regen.index()] as f64 * self.config.hero.regen_per_lv_per_sec
            + eq.regen_per_sec
    }

    /// gold 取得倍率 (1.0 + upgrades + soul perks + equipment)。
    pub fn gold_multiplier(&self) -> f64 {
        let h = &self.config.hero;
        let upgrade_lv = self.upgrades[UpgradeKind::Gold.index()];
        let fortune_lv = self.soul_perks[SoulPerk::Fortune.index()];
        let eq = self.equipment_bonus();
        1.0 + upgrade_lv as f64 * h.gold_per_lv
            + fortune_lv as f64 * h.fortune_per_lv
            + eq.gold_pct
    }

    /// 撃破時の魂取得倍率 (Reaper perk 由来)。
    pub fn soul_multiplier(&self) -> f64 {
        let lv = self.soul_perks[SoulPerk::Reaper.index()];
        1.0 + lv as f64 * self.config.hero.reaper_per_lv
    }

    /// 1 階層あたりに倒すべき雑魚数。config 経由。
    pub fn enemies_per_floor(&self) -> u32 {
        self.config.pacing.enemies_per_floor
    }

    /// ダンジョンの到達ゴールフロア。進捗バーの分母。
    pub fn goal_floor(&self) -> u32 {
        self.config.pacing.goal_floor
    }

    /// 強化 1 段階のコスト。
    pub fn upgrade_cost(&self, kind: UpgradeKind) -> u64 {
        let lv = self.upgrades[kind.index()] as f64;
        let cost = (kind.base_cost() as f64) * kind.growth().powf(lv);
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

    /// ボス出現までの残り敵数。0 なら現在のフロアにはボスが出現中。
    pub fn enemies_until_boss(&self) -> u32 {
        self.enemies_per_floor().saturating_sub(self.kills_on_floor)
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

    /// `Tab::all()` の全要素が to_save_id → from_save_id で元に戻る (双方向同型)。
    /// 新タブ追加時に save id の重複や抜け穴を検知する SSOT 保護網。
    #[test]
    fn tab_save_id_roundtrip() {
        for &tab in Tab::all() {
            let id = tab.to_save_id();
            assert_eq!(Tab::from_save_id(id), tab, "roundtrip mismatch for {:?}", tab);
        }
        // 範囲外 id は既定値にフォールバック
        assert_eq!(Tab::from_save_id(255), Tab::Upgrades);
    }

    /// `TabGroup::tabs()` と `from_tab()` が双方向で一貫していること。
    /// 新グループ追加 / 既存グループへの Tab 追加で抜け漏れたら検出する。
    #[test]
    fn tab_group_round_trip_via_tabs_and_from_tab() {
        for &group in TabGroup::all() {
            for &tab in group.tabs() {
                assert_eq!(
                    TabGroup::from_tab(tab),
                    group,
                    "group {:?} declares tab {:?} but from_tab says {:?}",
                    group,
                    tab,
                    TabGroup::from_tab(tab),
                );
            }
        }
        // 全 Tab がいずれかのグループに属すること (網羅性)。
        for &tab in Tab::all() {
            let g = TabGroup::from_tab(tab);
            assert!(
                g.tabs().contains(&tab),
                "tab {:?} is mapped to {:?} but {:?}.tabs() doesn't contain it",
                tab,
                g,
                g
            );
        }
        // default_tab が tabs() の先頭と一致 (UI 約束)。
        for &group in TabGroup::all() {
            assert_eq!(group.default_tab(), group.tabs()[0]);
        }
    }

    /// 同様に FloorKind も双方向 SSOT。
    #[test]
    fn floor_kind_save_id_roundtrip() {
        for &kind in FloorKind::all() {
            let id = kind.to_save_id();
            assert_eq!(FloorKind::from_save_id(id), kind, "roundtrip mismatch for {:?}", kind);
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
    }

    #[test]
    fn upgrades_change_stats() {
        let mut s = AbyssState::new();
        let base_atk = s.hero_atk();
        s.upgrades[UpgradeKind::Sword.index()] = 5;
        assert_eq!(s.hero_atk(), base_atk + 10);

        let base_hp = s.hero_max_hp();
        s.upgrades[UpgradeKind::Vitality.index()] = 3;
        assert_eq!(s.hero_max_hp(), base_hp + 30);
    }

    #[test]
    fn crit_capped_at_60() {
        let mut s = AbyssState::new();
        s.upgrades[UpgradeKind::Crit.index()] = 200;
        assert!((s.hero_crit_rate() - 0.60).abs() < 1e-9);
    }

    #[test]
    fn upgrade_cost_grows() {
        let mut s = AbyssState::new();
        let c0 = s.upgrade_cost(UpgradeKind::Sword);
        s.upgrades[UpgradeKind::Sword.index()] = 5;
        let c5 = s.upgrade_cost(UpgradeKind::Sword);
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
