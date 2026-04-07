//! Bond story episodes per character.
//! Phase 3+: episode viewing UI.

#![allow(dead_code)]

use super::CharacterId;
use super::affinity::CharacterAffinity;

/// An episode definition.
#[derive(Debug, Clone)]
pub struct Episode {
    pub id: u32,
    pub title: &'static str,
    pub required_level: u32,
}

/// Get available (unlocked but not viewed) episodes.
pub fn available_episodes(character: CharacterId, affinity: &CharacterAffinity) -> Vec<Episode> {
    let level = affinity.axes.level();
    episode_definitions(character)
        .into_iter()
        .filter(|ep| level >= ep.required_level && !affinity.viewed_episodes.contains(&ep.id))
        .collect()
}

/// All episode definitions for a character.
pub fn episode_definitions(character: CharacterId) -> Vec<Episode> {
    match character {
        CharacterId::Sakura => vec![
            Episode { id: 1, title: "静かな読書", required_level: 2 },
            Episode { id: 2, title: "昔のお店", required_level: 5 },
            Episode { id: 3, title: "もう一度、菓子を", required_level: 8 },
            Episode { id: 4, title: "佐倉の選択", required_level: 12 },
        ],
        CharacterId::Amano => vec![
            Episode { id: 1, title: "バイト志願", required_level: 2 },
            Episode { id: 2, title: "実家の金物屋", required_level: 5 },
            Episode { id: 3, title: "商店街の未来", required_level: 8 },
            Episode { id: 4, title: "蓮の答え", required_level: 12 },
        ],
        CharacterId::Miyauchi => vec![
            Episode { id: 1, title: "古い常連", required_level: 2 },
            Episode { id: 2, title: "前の店主のこと", required_level: 5 },
            Episode { id: 3, title: "秘密の手紙", required_level: 8 },
            Episode { id: 4, title: "宮内の後悔", required_level: 12 },
        ],
        CharacterId::Kanzaki => vec![
            Episode { id: 1, title: "取材申し込み", required_level: 2 },
            Episode { id: 2, title: "バズった記事", required_level: 5 },
            Episode { id: 3, title: "書けない記事", required_level: 8 },
            Episode { id: 4, title: "凛の矜持", required_level: 12 },
        ],
        CharacterId::Kiritani => vec![
            Episode { id: 1, title: "偵察", required_level: 2 },
            Episode { id: 2, title: "効率と非効率", required_level: 5 },
            Episode { id: 3, title: "本部の圧力", required_level: 8 },
            Episode { id: 4, title: "楓の本音", required_level: 12 },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::affinity::{AffinityAxes, CharacterAffinity};

    #[test]
    fn episode_availability() {
        let affinity = CharacterAffinity {
            axes: AffinityAxes { trust: 30, understanding: 10, empathy: 5 },
            viewed_episodes: vec![1],
        };
        let eps = available_episodes(CharacterId::Sakura, &affinity);
        assert!(eps.is_empty()); // level 3, ep2 needs level 5
    }

    #[test]
    fn all_characters_have_episodes() {
        for &ch in CharacterId::ALL {
            let eps = episode_definitions(ch);
            assert_eq!(eps.len(), 4);
        }
    }
}
