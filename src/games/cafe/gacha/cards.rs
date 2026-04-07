//! Card definitions — static data for all collectible cards.

use serde::{Deserialize, Serialize};
use super::super::characters::CharacterId;

// ── Rarity ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rarity {
    Star1,
    Star2,
    Star3,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Star1 => "★",
            Self::Star2 => "★★",
            Self::Star3 => "★★★",
        }
    }

    pub fn base_multiplier(self) -> f64 {
        match self {
            Self::Star1 => 1.0,
            Self::Star2 => 1.3,
            Self::Star3 => 1.8,
        }
    }

    /// Character shards given on duplicate pull.
    #[allow(dead_code)] // Phase 2+: dupe→shard conversion
    pub fn dupe_shards(self) -> u32 {
        match self {
            Self::Star1 => 1,
            Self::Star2 => 5,
            Self::Star3 => 30,
        }
    }
}

// ── Bonus Axis ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BonusAxis {
    Trust,
    Understanding,
    Empathy,
    Balanced,
}

// ── Card Definition ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CardDef {
    pub id: u32,
    pub name: &'static str,
    #[allow(dead_code)] // Phase 2+: character-specific card filtering
    pub character: Option<CharacterId>,
    pub rarity: Rarity,
    #[allow(dead_code)] // Phase 2+: card detail UI
    pub description: &'static str,
    pub bonus_axis: BonusAxis,
}

pub fn all_cards() -> &'static [CardDef] {
    static CARDS: &[CardDef] = &[
        // ── ★1 Common ──
        CardDef { id: 1, name: "朝のブレンド", character: None, rarity: Rarity::Star1, description: "基本のドリップコーヒー", bonus_axis: BonusAxis::Trust },
        CardDef { id: 2, name: "窓辺の読書", character: None, rarity: Rarity::Star1, description: "静かなひととき", bonus_axis: BonusAxis::Understanding },
        CardDef { id: 3, name: "雨の日の来客", character: None, rarity: Rarity::Star1, description: "傘を忘れた客との会話", bonus_axis: BonusAxis::Empathy },
        CardDef { id: 4, name: "エスプレッソ練習", character: None, rarity: Rarity::Star1, description: "マシンの使い方を覚える", bonus_axis: BonusAxis::Balanced },
        CardDef { id: 5, name: "仕入れの朝", character: None, rarity: Rarity::Star1, description: "市場で食材を選ぶ", bonus_axis: BonusAxis::Trust },
        CardDef { id: 6, name: "掃除の時間", character: None, rarity: Rarity::Star1, description: "店を綺麗に保つ", bonus_axis: BonusAxis::Understanding },
        // ── ★2 Uncommon ──
        CardDef { id: 10, name: "佐倉の定位置", character: Some(CharacterId::Sakura), rarity: Rarity::Star2, description: "カウンター端の指定席", bonus_axis: BonusAxis::Understanding },
        CardDef { id: 11, name: "蓮のバイト申請", character: Some(CharacterId::Amano), rarity: Rarity::Star2, description: "元気な助っ人登場", bonus_axis: BonusAxis::Empathy },
        CardDef { id: 12, name: "宮内の昔話", character: Some(CharacterId::Miyauchi), rarity: Rarity::Star2, description: "この店の昔のこと", bonus_axis: BonusAxis::Trust },
        CardDef { id: 13, name: "凛の取材ノート", character: Some(CharacterId::Kanzaki), rarity: Rarity::Star2, description: "小さな記事が繋ぐ縁", bonus_axis: BonusAxis::Understanding },
        CardDef { id: 14, name: "楓の視察", character: Some(CharacterId::Kiritani), rarity: Rarity::Star2, description: "チェーン店マネージャーの目", bonus_axis: BonusAxis::Balanced },
        CardDef { id: 15, name: "手作りスコーン", character: None, rarity: Rarity::Star2, description: "新メニュー開発の第一歩", bonus_axis: BonusAxis::Trust },
        // ── ★3 Rare ──
        CardDef { id: 20, name: "月灯りの記憶", character: Some(CharacterId::Sakura), rarity: Rarity::Star3, description: "佐倉が語る、あの日の味", bonus_axis: BonusAxis::Empathy },
        CardDef { id: 21, name: "商店街の絆", character: Some(CharacterId::Amano), rarity: Rarity::Star3, description: "蓮と商店街を歩く午後", bonus_axis: BonusAxis::Trust },
        CardDef { id: 22, name: "レシピノート", character: Some(CharacterId::Miyauchi), rarity: Rarity::Star3, description: "前の店主が残したもの", bonus_axis: BonusAxis::Understanding },
        CardDef { id: 23, name: "書きたい記事", character: Some(CharacterId::Kanzaki), rarity: Rarity::Star3, description: "凛が本当に伝えたいこと", bonus_axis: BonusAxis::Empathy },
        CardDef { id: 24, name: "二つのカフェ", character: Some(CharacterId::Kiritani), rarity: Rarity::Star3, description: "効率じゃない何か", bonus_axis: BonusAxis::Balanced },
    ];
    CARDS
}

pub fn card_def(id: u32) -> Option<&'static CardDef> {
    all_cards().iter().find(|c| c.id == id)
}
