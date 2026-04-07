//! Character skills — BA-style 3 skills per character.
//!
//! Skill 1: Available from start
//! Skill 2: Unlocks at ★3
//! Skill 3: Unlocks at ★5

use super::CharacterId;

/// A skill definition.
#[derive(Debug, Clone)]
pub struct SkillDef {
    pub name: &'static str,
    pub description: &'static str,
    /// Which star rank unlocks this skill.
    pub unlock_stars: u32,
}

/// Get skill definitions for a character.
pub fn character_skills(id: CharacterId) -> [SkillDef; 3] {
    match id {
        CharacterId::Sakura => [
            SkillDef { name: "レシピ提案", description: "調理力+10%", unlock_stars: 1 },
            SkillDef { name: "常連の目", description: "接客評価+15%", unlock_stars: 3 },
            SkillDef { name: "菓子の記憶", description: "全ステータス+8%", unlock_stars: 5 },
        ],
        CharacterId::Amano => [
            SkillDef { name: "元気な接客", description: "接客力+10%", unlock_stars: 1 },
            SkillDef { name: "商店街の絆", description: "クレジット収入+20%", unlock_stars: 3 },
            SkillDef { name: "仲間の力", description: "全キャラEXP+10%", unlock_stars: 5 },
        ],
        CharacterId::Miyauchi => [
            SkillDef { name: "温かい助言", description: "雰囲気+10%", unlock_stars: 1 },
            SkillDef { name: "店の歴史", description: "特別行動のAP-1", unlock_stars: 3 },
            SkillDef { name: "先代の教え", description: "全行動の親密度+15%", unlock_stars: 5 },
        ],
        CharacterId::Kanzaki => [
            SkillDef { name: "取材力", description: "理解の獲得量+15%", unlock_stars: 1 },
            SkillDef { name: "記事掲載", description: "営業の来客数+1", unlock_stars: 3 },
            SkillDef { name: "真実の筆", description: "全親密度獲得+20%", unlock_stars: 5 },
        ],
        CharacterId::Kiritani => [
            SkillDef { name: "経営分析", description: "営業収入+10%", unlock_stars: 1 },
            SkillDef { name: "効率化", description: "AP回復+1/日", unlock_stars: 3 },
            SkillDef { name: "二つの視点", description: "プロデュース評価+25%", unlock_stars: 5 },
        ],
    }
}

/// Calculate skill effect multiplier for a skill at given level.
/// Returns the multiplier as a percentage bonus (e.g., 10 = +10%).
#[allow(dead_code)] // Phase 2+: skill effect calculation in produce/business
pub fn skill_effect(base_percent: u32, skill_level: u32) -> u32 {
    if skill_level == 0 {
        return 0;
    }
    // Each skill level adds 2% to base effect
    base_percent + (skill_level.saturating_sub(1)) * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_characters_have_skills() {
        for &ch in CharacterId::ALL {
            let skills = character_skills(ch);
            assert_eq!(skills[0].unlock_stars, 1);
            assert_eq!(skills[1].unlock_stars, 3);
            assert_eq!(skills[2].unlock_stars, 5);
        }
    }

    #[test]
    fn skill_effect_scaling() {
        assert_eq!(skill_effect(10, 0), 0);  // locked
        assert_eq!(skill_effect(10, 1), 10); // base
        assert_eq!(skill_effect(10, 3), 14); // base + 2*2
    }
}
