//! Random events during produce — commu, bonus stats, special encounters.

use serde::{Deserialize, Serialize};

/// A produce event that can trigger during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProduceEvent {
    pub name: String,
    pub description: String,
    pub bonus_service: u32,
    pub bonus_cooking: u32,
    pub bonus_atmosphere: u32,
}

/// Event definitions.
static EVENTS: &[(&str, &str, u32, u32, u32)] = &[
    ("常連のリクエスト", "常連さんが新メニューを提案してくれた", 5, 10, 0),
    ("材料の特売", "市場で良い素材が安く手に入った", 0, 15, 0),
    ("インスタ映え", "お客さんがSNSに写真を投稿した", 0, 0, 15),
    ("クレーム対応", "お客さんの不満を丁寧に解決した", 10, 0, 5),
    ("近所のお裾分け", "商店街の仲間から差し入れがあった", 5, 5, 5),
    ("雨の日の混雑", "雨宿りのお客さんが沢山来た", 10, 5, 0),
    ("フラワーアレンジ", "お花を飾って店の雰囲気が良くなった", 0, 0, 12),
    ("料理雑誌の取材", "小さな雑誌に紹介された", 5, 5, 10),
    ("子供のお客さん", "ファミリー客への対応を学んだ", 8, 0, 8),
    ("仕込みの工夫", "効率的な仕込み方法を発見した", 0, 12, 3),
];

/// Roll for an event based on turn and seed.
/// ~40% chance of event per turn.
pub fn roll_event(turn: u32, seed: u32) -> Option<ProduceEvent> {
    let roll = seed.wrapping_mul(2654435761).wrapping_add(turn * 37);

    // 40% chance of event
    if roll % 100 >= 40 {
        return None;
    }

    let idx = (roll / 100) as usize % EVENTS.len();
    let (name, desc, s, c, a) = EVENTS[idx];

    Some(ProduceEvent {
        name: name.to_string(),
        description: desc.to_string(),
        bonus_service: s,
        bonus_cooking: c,
        bonus_atmosphere: a,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_can_trigger() {
        let mut triggered = false;
        for seed in 0..100u32 {
            if roll_event(1, seed).is_some() {
                triggered = true;
                break;
            }
        }
        assert!(triggered);
    }

    #[test]
    fn events_have_content() {
        // Find a seed that triggers an event
        for seed in 0..1000u32 {
            if let Some(event) = roll_event(1, seed) {
                assert!(!event.name.is_empty());
                assert!(!event.description.is_empty());
                let total = event.bonus_service + event.bonus_cooking + event.bonus_atmosphere;
                assert!(total > 0);
                return;
            }
        }
        panic!("No event triggered in 1000 tries");
    }

    #[test]
    fn events_vary_by_seed() {
        let mut names = std::collections::HashSet::new();
        for seed in 0..1000u32 {
            if let Some(event) = roll_event(1, seed) {
                names.insert(event.name);
            }
        }
        assert!(names.len() > 1); // Multiple different events
    }
}
