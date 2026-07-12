//! 穴掘り長屋の state — 庭グリッド / 隣人 / 図鑑コレクション / コイン。

pub const YARD_W: usize = 5;
pub const YARD_H: usize = 3;
pub const YARD_LEN: usize = YARD_W * YARD_H;

/// 1日あたりの最大行動回数。実際の日付 (localStorage 保存) が変わると全回復する。
pub const MAX_ACTIONS_PER_DAY: u8 = 5;

pub const NEIGHBOR_COUNT: usize = 3;

/// シャベル強化の最大段階。
pub const MAX_SHOVEL_LEVEL: u8 = 3;

/// 掘って出るものの種類。前半5種はその場でコインに変わる「地面の恵み」、
/// 後半7種は図鑑コレクションの「かけら」(所持数を保持し、セット全部揃うと
/// ボーナスコインを獲得する)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Dirt,
    Pebble,
    CopperCoin,
    SilverChunk,
    GoldNugget,
    PotteryTop,
    PotteryBottom,
    DragonSkull,
    DragonSpine,
    DragonTail,
    ManekiRight,
    ManekiLeft,
}

impl ItemKind {
    /// この並び順がそのまま `to_save_id`/`from_save_id` のエンコードになる。
    /// 既存セーブとの互換性のため、途中への挿入や並び替えはせず末尾に追記すること。
    pub fn all() -> [ItemKind; 12] {
        [
            ItemKind::Dirt,
            ItemKind::Pebble,
            ItemKind::CopperCoin,
            ItemKind::SilverChunk,
            ItemKind::GoldNugget,
            ItemKind::PotteryTop,
            ItemKind::PotteryBottom,
            ItemKind::DragonSkull,
            ItemKind::DragonSpine,
            ItemKind::DragonTail,
            ItemKind::ManekiRight,
            ItemKind::ManekiLeft,
        ]
    }

    /// その場でコインに変わる「地面の恵み」ならコイン額を返す。
    /// 図鑑かけらは `None` (別途 `piece_slot` で所持数管理する)。
    pub fn coin_value(self) -> Option<u32> {
        match self {
            ItemKind::Dirt => Some(1),
            ItemKind::Pebble => Some(2),
            ItemKind::CopperCoin => Some(5),
            ItemKind::SilverChunk => Some(15),
            ItemKind::GoldNugget => Some(40),
            _ => None,
        }
    }

    /// 図鑑かけらなら所属する `CollectionSet` を返す。
    pub fn collection(self) -> Option<CollectionSet> {
        match self {
            ItemKind::PotteryTop | ItemKind::PotteryBottom => Some(CollectionSet::Pottery),
            ItemKind::DragonSkull | ItemKind::DragonSpine | ItemKind::DragonTail => {
                Some(CollectionSet::Dragon)
            }
            ItemKind::ManekiRight | ItemKind::ManekiLeft => Some(CollectionSet::Maneki),
            _ => None,
        }
    }

    /// `DigState::piece_counts` のインデックス。図鑑かけらのみ `Some`。
    pub fn piece_slot(self) -> Option<usize> {
        match self {
            ItemKind::PotteryTop => Some(0),
            ItemKind::PotteryBottom => Some(1),
            ItemKind::DragonSkull => Some(2),
            ItemKind::DragonSpine => Some(3),
            ItemKind::DragonTail => Some(4),
            ItemKind::ManekiRight => Some(5),
            ItemKind::ManekiLeft => Some(6),
            _ => None,
        }
    }

    /// 庭グリッドのセル表示用 2 文字コード (ASCII 固定幅、CJK幅ズレを避ける)。
    pub fn glyph(self) -> &'static str {
        match self {
            ItemKind::Dirt => "dt",
            ItemKind::Pebble => "pb",
            ItemKind::CopperCoin => "cu",
            ItemKind::SilverChunk => "si",
            ItemKind::GoldNugget => "gd",
            ItemKind::PotteryTop => "p1",
            ItemKind::PotteryBottom => "p2",
            ItemKind::DragonSkull => "d1",
            ItemKind::DragonSpine => "d2",
            ItemKind::DragonTail => "d3",
            ItemKind::ManekiRight => "m1",
            ItemKind::ManekiLeft => "m2",
        }
    }

    /// ログ・図鑑表示用の日本語名。
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Dirt => "土くれ",
            ItemKind::Pebble => "小石",
            ItemKind::CopperCoin => "古びた銅貨",
            ItemKind::SilverChunk => "銀の欠片",
            ItemKind::GoldNugget => "砂金",
            ItemKind::PotteryTop => "土器のかけら(上)",
            ItemKind::PotteryBottom => "土器のかけら(下)",
            ItemKind::DragonSkull => "竜の頭骨",
            ItemKind::DragonSpine => "竜の背骨",
            ItemKind::DragonTail => "竜の尾骨",
            ItemKind::ManekiRight => "招き猫の右手",
            ItemKind::ManekiLeft => "招き猫の左手",
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

pub const PIECE_COUNT: usize = 7;

/// 図鑑コレクション (かけらを全部揃えるとボーナスコインを獲得)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionSet {
    Pottery,
    Dragon,
    Maneki,
}

pub const COLLECTION_COUNT: usize = 3;

impl CollectionSet {
    pub fn all() -> [CollectionSet; COLLECTION_COUNT] {
        [CollectionSet::Pottery, CollectionSet::Dragon, CollectionSet::Maneki]
    }

    pub fn index(self) -> usize {
        match self {
            CollectionSet::Pottery => 0,
            CollectionSet::Dragon => 1,
            CollectionSet::Maneki => 2,
        }
    }

    pub fn pieces(self) -> &'static [ItemKind] {
        match self {
            CollectionSet::Pottery => &[ItemKind::PotteryTop, ItemKind::PotteryBottom],
            CollectionSet::Dragon => {
                &[ItemKind::DragonSkull, ItemKind::DragonSpine, ItemKind::DragonTail]
            }
            CollectionSet::Maneki => &[ItemKind::ManekiRight, ItemKind::ManekiLeft],
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            CollectionSet::Pottery => "唐草文様の土器",
            CollectionSet::Dragon => "小さな竜の骨格",
            CollectionSet::Maneki => "福を呼ぶ招き猫",
        }
    }

    /// コンプリート時の一括ボーナスコイン。
    pub fn bonus_coins(self) -> u32 {
        match self {
            CollectionSet::Pottery => 150,
            CollectionSet::Dragon => 400,
            CollectionSet::Maneki => 250,
        }
    }
}

/// ご近所さん 1 人分。専門の `CollectionSet` を持ち、そのお福分け穴を
/// 掘らせてもらうとそのセットのかけらが出やすくなる。
pub struct Neighbor {
    pub name: &'static str,
    pub specialty: CollectionSet,
    /// 今日すでにこの人の穴を掘らせてもらったか。日付が変わるとリセット。
    pub dug_today: bool,
    /// 累計で掘らせてもらった回数。`friendship_level` の算出に使う。
    pub total_digs: u32,
}

/// 固定 3 人のご近所さん定義。各人がそれぞれ別のコレクションを専門にすることで、
/// 「今日は誰の庭に行くか」の判断に意味を持たせる。
const NEIGHBOR_DEFS: [(&str, CollectionSet); NEIGHBOR_COUNT] = [
    ("大家さん", CollectionSet::Pottery),
    ("隣のご隠居", CollectionSet::Dragon),
    ("向かいの八百屋さん", CollectionSet::Maneki),
];

/// 図鑑コンプリート演出の表示時間 (tick)。10 ticks/sec → 2 秒。
pub const COLLECTION_FLASH_TTL: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigTab {
    Yard,
    Neighbors,
    Collection,
}

pub struct DigState {
    /// 庭グリッド。`None` = 未掘 (今日はまだ新しい土)、`Some(item)` = 今日掘り当てたもの。
    pub yard: [Option<ItemKind>; YARD_LEN],
    pub actions_remaining: u8,
    /// 直近リセット時点の日付インデックス (`logic::day_index` 参照)。
    /// 実際のカレンダー日付が変わるまで行動回数は回復しない。
    pub last_reset_day: u64,
    pub coins: u64,
    pub total_coins_earned: u64,
    pub piece_counts: [u32; PIECE_COUNT],
    pub completed_sets: [bool; COLLECTION_COUNT],
    pub neighbors: [Neighbor; NEIGHBOR_COUNT],
    pub shovel_level: u8,
    /// 決定的乱数の seed。`localStorage` ロード後も同じ列を続けられる。
    pub rng_state: u64,
    pub log: Vec<String>,
    pub selected_tab: DigTab,
    /// 図鑑コンプリート演出。`(セット, 残り tick)`。
    pub collection_flash: Option<(CollectionSet, u32)>,
}

impl DigState {
    pub fn new() -> Self {
        Self {
            yard: [None; YARD_LEN],
            actions_remaining: MAX_ACTIONS_PER_DAY,
            last_reset_day: 0,
            coins: 0,
            total_coins_earned: 0,
            piece_counts: [0; PIECE_COUNT],
            completed_sets: [false; COLLECTION_COUNT],
            neighbors: NEIGHBOR_DEFS.map(|(name, specialty)| Neighbor {
                name,
                specialty,
                dug_today: false,
                total_digs: 0,
            }),
            shovel_level: 0,
            // 固定 seed: 初プレイの体験を全プレイヤー共通にする (merge と同じ方針)。
            rng_state: 0x9E37_79B9_7F4A_7C15,
            log: vec!["今日の行動力は5。庭を掘るか、ご近所のお福分け穴を掘らせてもらおう。".into()],
            selected_tab: DigTab::Yard,
            collection_flash: None,
        }
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
    fn 新規stateは行動力満タンで庭は未掘() {
        let s = DigState::new();
        assert_eq!(s.actions_remaining, MAX_ACTIONS_PER_DAY);
        assert!(s.yard.iter().all(|c| c.is_none()));
        assert_eq!(s.coins, 0);
        assert_eq!(s.shovel_level, 0);
    }

    #[test]
    fn 新規stateは3人のご近所さんを持つ() {
        let s = DigState::new();
        assert_eq!(s.neighbors.len(), NEIGHBOR_COUNT);
        for n in &s.neighbors {
            assert!(!n.name.is_empty());
            assert!(!n.dug_today);
            assert_eq!(n.total_digs, 0);
        }
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

    #[test]
    fn piece_slotは図鑑かけらのみsomeを返す() {
        for item in ItemKind::all() {
            if item.collection().is_some() {
                assert!(item.piece_slot().is_some(), "{item:?} は piece_slot を持つべき");
            } else {
                assert!(item.piece_slot().is_none(), "{item:?} は piece_slot を持たないべき");
            }
        }
    }

    #[test]
    fn coin_valueは通貨アイテムのみsomeを返す() {
        let currency = [
            ItemKind::Dirt,
            ItemKind::Pebble,
            ItemKind::CopperCoin,
            ItemKind::SilverChunk,
            ItemKind::GoldNugget,
        ];
        for item in ItemKind::all() {
            if currency.contains(&item) {
                assert!(item.coin_value().is_some());
            } else {
                assert!(item.coin_value().is_none());
            }
        }
    }

    #[test]
    fn コレクション定義は全ての図鑑かけらをちょうど1回ずつ含む() {
        let mut covered: Vec<ItemKind> = Vec::new();
        for set in CollectionSet::all() {
            for piece in set.pieces() {
                assert!(
                    !covered.contains(piece),
                    "{piece:?} が複数のセットに重複している"
                );
                covered.push(*piece);
            }
        }
        for item in ItemKind::all() {
            if item.collection().is_some() {
                assert!(covered.contains(&item), "{item:?} がどのセットにも属していない");
            }
        }
        assert_eq!(covered.len(), PIECE_COUNT);
    }

    #[test]
    fn item_kindのsave_idは往復可能() {
        for item in ItemKind::all() {
            assert_eq!(ItemKind::from_save_id(item.to_save_id()), Some(item));
        }
    }

    #[test]
    fn 不正なsave_idはnoneを返す() {
        assert_eq!(ItemKind::from_save_id(255), None);
    }
}
