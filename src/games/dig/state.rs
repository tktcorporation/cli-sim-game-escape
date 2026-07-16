//! 穴掘り長屋の state — 発掘現場 / 宝 / 図鑑 / コイン。
//!
//! v2: 「ランダム抽選」から「ヒント数字で宝の位置を推理する発掘パズル」へ。
//! 現場は日付から決定的に生成されるため全プレイヤー共通 (Wordle 方式)。

use std::cell::Cell;

pub const SITE_W: usize = 7;
pub const SITE_H: usize = 5;
pub const SITE_LEN: usize = SITE_W * SITE_H;

/// 1日あたりのシャベル本数。宝を完掘すると1本返却されるので、
/// 推理が上手いほど実質の行動回数が増える。
pub const SHOVELS_PER_DAY: u8 = 5;

/// 羅盤 (掘らずにヒントだけ見るツール) の1日の使用上限。
pub const RADAR_MAX_PER_DAY: u8 = 3;

/// 地中に埋まる宝の種類。それぞれ固有の形 (占有マス) を持ち、
/// 全マス掘り当てると回収される。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    JomonPot,
    Magatama,
    ObsidianArrow,
    DragonSkull,
    DragonSpine,
    ManekiNeko,
    CoinJar,
    Senryobako,
}

pub const KIND_COUNT: usize = 8;

impl ItemKind {
    /// この並び順がそのまま `to_save_id`/`from_save_id` のエンコードになる。
    /// 既存セーブとの互換性のため、途中への挿入や並び替えはせず末尾に追記すること。
    pub fn all() -> [ItemKind; KIND_COUNT] {
        [
            ItemKind::JomonPot,
            ItemKind::Magatama,
            ItemKind::ObsidianArrow,
            ItemKind::DragonSkull,
            ItemKind::DragonSpine,
            ItemKind::ManekiNeko,
            ItemKind::CoinJar,
            ItemKind::Senryobako,
        ]
    }

    /// 基本形 (回転前) の占有オフセット。原点 (0,0) を必ず含む。
    /// 多マスの宝は「一部が見えたら残りの位置を形から推理できる」ための設計。
    pub fn shape(self) -> &'static [(i8, i8)] {
        match self {
            ItemKind::JomonPot => &[(0, 0), (1, 0), (0, 1)],
            ItemKind::Magatama => &[(0, 0), (1, 0)],
            ItemKind::ObsidianArrow => &[(0, 0)],
            ItemKind::DragonSkull => &[(0, 0), (1, 0)],
            ItemKind::DragonSpine => &[(0, 0), (1, 0), (2, 0)],
            ItemKind::ManekiNeko => &[(0, 0), (0, 1)],
            ItemKind::CoinJar => &[(0, 0)],
            ItemKind::Senryobako => &[(0, 0), (1, 0), (1, 1)],
        }
    }

    pub fn size(self) -> usize {
        self.shape().len()
    }

    /// 初回発見時のコイン。大きい宝ほど掘り当てる手数がかかるため高額。
    pub fn first_find_coins(self) -> u64 {
        self.size() as u64 * 40
    }

    /// 2回目以降 (図鑑登録済み) の発見時のコイン。
    pub fn duplicate_coins(self) -> u64 {
        self.size() as u64 * 20
    }

    pub fn collection(self) -> CollectionSet {
        match self {
            ItemKind::JomonPot | ItemKind::Magatama | ItemKind::ObsidianArrow => {
                CollectionSet::Jomon
            }
            ItemKind::DragonSkull | ItemKind::DragonSpine => CollectionSet::Dragon,
            ItemKind::ManekiNeko | ItemKind::CoinJar | ItemKind::Senryobako => CollectionSet::Fuku,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::JomonPot => "縄文土器",
            ItemKind::Magatama => "翡翠の勾玉",
            ItemKind::ObsidianArrow => "黒曜石の鏃",
            ItemKind::DragonSkull => "竜の頭骨",
            ItemKind::DragonSpine => "竜の背骨",
            ItemKind::ManekiNeko => "招き猫",
            ItemKind::CoinJar => "古銭の壺",
            ItemKind::Senryobako => "千両箱",
        }
    }

    /// セーブ用 id。
    pub fn to_save_id(self) -> u8 {
        Self::all().iter().position(|k| *k == self).unwrap() as u8
    }

    pub fn from_save_id(id: u8) -> Option<Self> {
        Self::all().get(id as usize).copied()
    }
}

/// 図鑑コレクション。全種類を掘り当てるとボーナスコイン。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionSet {
    Jomon,
    Dragon,
    Fuku,
}

pub const COLLECTION_COUNT: usize = 3;

impl CollectionSet {
    pub fn all() -> [CollectionSet; COLLECTION_COUNT] {
        [CollectionSet::Jomon, CollectionSet::Dragon, CollectionSet::Fuku]
    }

    pub fn index(self) -> usize {
        match self {
            CollectionSet::Jomon => 0,
            CollectionSet::Dragon => 1,
            CollectionSet::Fuku => 2,
        }
    }

    pub fn kinds(self) -> Vec<ItemKind> {
        ItemKind::all()
            .into_iter()
            .filter(|k| k.collection() == self)
            .collect()
    }

    pub fn display_name(self) -> &'static str {
        match self {
            CollectionSet::Jomon => "縄文の宴",
            CollectionSet::Dragon => "竜の伝説",
            CollectionSet::Fuku => "商店街の福",
        }
    }

    pub fn bonus_coins(self) -> u64 {
        match self {
            CollectionSet::Jomon => 300,
            CollectionSet::Dragon => 400,
            CollectionSet::Fuku => 350,
        }
    }
}

/// 現場に埋まっている宝1つ。`cells` は現場グリッドの flat index。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Treasure {
    pub kind: ItemKind,
    pub cells: Vec<u16>,
}

/// 発掘演出。`cells` が REVERSED で光り、`ttl` が tick ごとに減る。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flash {
    pub cells: Vec<u16>,
    pub ttl: u32,
}

/// 宝の一部ヒット時 / 完掘時の演出時間 (tick)。10 ticks/sec。
pub const FLASH_HIT_TTL: u32 = 8;
pub const FLASH_COMPLETE_TTL: u32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigTab {
    Site,
    Museum,
}

pub struct DigState {
    /// 現在の現場の日付インデックス (`logic::day_index`)。現場はこの値から
    /// 決定的に生成されるため、セーブには日付だけ保存すれば復元できる。
    pub day: u64,
    pub treasures: Vec<Treasure>,
    pub dug: [bool; SITE_LEN],
    /// 羅盤で調査済み (掘らずにヒントだけ見えている) のマス。
    pub scanned: [bool; SITE_LEN],
    pub shovels: u8,
    pub radar_uses: u8,
    /// 羅盤モード中 (次のグリッドタップが「掘る」ではなく「調べる」になる)。
    pub radar_armed: bool,
    /// 本日の完全制覇ボーナスを付与済みか。
    pub perfect_bonus_given: bool,
    pub coins: u64,
    pub total_coins_earned: u64,
    /// 種類ごとの累計発見数。0 = 未発見 (図鑑では？？？表示)。
    pub museum_counts: [u32; KIND_COUNT],
    pub completed_sets: [bool; COLLECTION_COUNT],
    pub selected_tab: DigTab,
    /// 図鑑タブのスクロール位置。描画時に `ScrollableTab` が実コンテンツ高で
    /// clamp して書き戻すため `Cell` (render は `&self`)。
    pub museum_scroll: Cell<u16>,
    pub log: Vec<String>,
    pub flash: Option<Flash>,
}

impl DigState {
    pub fn new() -> Self {
        Self {
            day: 0,
            treasures: Vec::new(),
            dug: [false; SITE_LEN],
            scanned: [false; SITE_LEN],
            shovels: SHOVELS_PER_DAY,
            radar_uses: 0,
            radar_armed: false,
            perfect_bonus_given: false,
            coins: 0,
            total_coins_earned: 0,
            museum_counts: [0; KIND_COUNT],
            completed_sets: [false; COLLECTION_COUNT],
            selected_tab: DigTab::Site,
            museum_scroll: Cell::new(0),
            log: vec!["数字は「一番近いお宝までの歩数」。よく狙って掘ろう。".into()],
            flash: None,
        }
    }

    pub fn idx(x: usize, y: usize) -> usize {
        y * SITE_W + x
    }

    /// `idx` を含む宝の (treasures 内 index, 種類) を返す。
    pub fn treasure_at(&self, idx: usize) -> Option<(usize, ItemKind)> {
        self.treasures
            .iter()
            .position(|t| t.cells.contains(&(idx as u16)))
            .map(|i| (i, self.treasures[i].kind))
    }

    /// 宝が完掘 (全マス dug) されているか。
    pub fn treasure_complete(&self, t_idx: usize) -> bool {
        self.treasures[t_idx]
            .cells
            .iter()
            .all(|&c| self.dug[c as usize])
    }

    /// 未回収の宝の数。
    pub fn remaining_treasures(&self) -> usize {
        (0..self.treasures.len())
            .filter(|&i| !self.treasure_complete(i))
            .count()
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 20 {
            self.log.remove(0);
        }
    }
}

impl Default for DigState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 新規stateはシャベル満タンで現場は未掘() {
        let s = DigState::new();
        assert_eq!(s.shovels, SHOVELS_PER_DAY);
        assert!(s.dug.iter().all(|d| !d));
        assert!(s.scanned.iter().all(|d| !d));
        assert_eq!(s.coins, 0);
        assert_eq!(s.radar_uses, 0);
        assert!(!s.radar_armed);
    }

    #[test]
    fn 全種類のshapeは原点を含み空でない() {
        for kind in ItemKind::all() {
            let shape = kind.shape();
            assert!(!shape.is_empty(), "{kind:?} の shape が空");
            assert!(shape.contains(&(0, 0)), "{kind:?} の shape に原点がない");
            assert_eq!(kind.size(), shape.len());
        }
    }

    #[test]
    fn shape内にオフセットの重複がない() {
        for kind in ItemKind::all() {
            let shape = kind.shape();
            for (i, a) in shape.iter().enumerate() {
                for b in &shape[i + 1..] {
                    assert_ne!(a, b, "{kind:?} の shape にマス重複");
                }
            }
        }
    }

    #[test]
    fn 全種類がいずれかのコレクションにちょうど1回属する() {
        let mut counts = [0usize; COLLECTION_COUNT];
        for kind in ItemKind::all() {
            counts[kind.collection().index()] += 1;
        }
        let total: usize = counts.iter().sum();
        assert_eq!(total, KIND_COUNT);
        for set in CollectionSet::all() {
            assert!(
                !set.kinds().is_empty(),
                "{set:?} に属する種類がない"
            );
        }
    }

    #[test]
    fn 初回発見コインは重複発見コインより高い() {
        for kind in ItemKind::all() {
            assert!(kind.first_find_coins() > kind.duplicate_coins());
        }
    }

    #[test]
    fn save_idは往復可能() {
        for kind in ItemKind::all() {
            assert_eq!(ItemKind::from_save_id(kind.to_save_id()), Some(kind));
        }
        assert_eq!(ItemKind::from_save_id(255), None);
    }

    #[test]
    fn treasure_atとtreasure_completeが整合する() {
        let mut s = DigState::new();
        s.treasures = vec![Treasure {
            kind: ItemKind::Magatama,
            cells: vec![3, 4],
        }];
        assert_eq!(s.treasure_at(3), Some((0, ItemKind::Magatama)));
        assert_eq!(s.treasure_at(4), Some((0, ItemKind::Magatama)));
        assert_eq!(s.treasure_at(5), None);
        assert!(!s.treasure_complete(0));
        s.dug[3] = true;
        assert!(!s.treasure_complete(0));
        s.dug[4] = true;
        assert!(s.treasure_complete(0));
        assert_eq!(s.remaining_treasures(), 0);
    }

    #[test]
    fn add_logは20件を超えると古いものから削除される() {
        let mut s = DigState::new();
        for i in 0..30 {
            s.add_log(format!("log {i}"));
        }
        assert_eq!(s.log.len(), 20);
        assert_eq!(s.log.last().unwrap(), "log 29");
    }
}
