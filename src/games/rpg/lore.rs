//! Floor themes, atmosphere text, and story fragments.
//!
//! Each dungeon floor has a visual theme that affects:
//! - 3D view wall colors
//! - Atmospheric descriptions during exploration
//! - Event flavor text

use super::state::FloorTheme;

/// Get the theme for a given floor number.
pub fn floor_theme(floor: u32) -> FloorTheme {
    match floor {
        1..=2 => FloorTheme::MossyRuins,
        3..=4 => FloorTheme::Underground,
        5..=6 => FloorTheme::AncientTemple,
        7..=8 => FloorTheme::VolcanicDepths,
        _ => FloorTheme::DemonCastle,
    }
}

/// Theme display name.
pub fn theme_name(theme: FloorTheme) -> &'static str {
    match theme {
        FloorTheme::MossyRuins => "苔むした遺跡",
        FloorTheme::Underground => "地下水脈",
        FloorTheme::AncientTemple => "古代神殿",
        FloorTheme::VolcanicDepths => "灼熱の坑道",
        FloorTheme::DemonCastle => "魔王の居城",
    }
}

/// Get atmospheric flavor text for the current movement.
pub fn atmosphere_text(theme: FloorTheme, rng_val: u32) -> &'static str {
    match theme {
        FloorTheme::MossyRuins => match rng_val % 8 {
            0 => "苔むした壁が続いている。水滴が落ちる音がする。",
            1 => "古い松明の跡がある。かつて誰かが通った道だ。",
            2 => "足元に小さなキノコが群生している。",
            3 => "壁の隙間から冷たい風が吹いている。",
            4 => "天井から根が垂れ下がっている。",
            5 => "苔の匂いが鼻をつく。",
            6 => "遠くで何かが崩れる音がした。",
            _ => "石畳が湿っている。足音が響く。",
        },
        FloorTheme::Underground => match rng_val % 8 {
            0 => "水の流れる音が反響している。",
            1 => "天井から雫が落ちてくる。",
            2 => "地底の冷たい空気が肌を刺す。",
            3 => "岩壁に鍾乳石が輝いている。",
            4 => "足元に水たまりがある。何か光っている。",
            5 => "地下河の音が近い。",
            6 => "壁が湿って滑りやすい。",
            _ => "暗い水面に自分の姿が映っている。",
        },
        FloorTheme::AncientTemple => match rng_val % 8 {
            0 => "崩れかけた柱が並ぶ通路。かつての荘厳さが偲ばれる。",
            1 => "壁に古代の壁画が残っている。色あせているが美しい。",
            2 => "祈りの場だったのか、石の長椅子が並んでいる。",
            3 => "天井に星のような模様が描かれている。",
            4 => "古い香の残り香がかすかに漂う。",
            5 => "床に礼拝の跡が見える。膝の痕が石に刻まれている。",
            6 => "壊れた祭壇がある。供物は既にない。",
            _ => "静寂が支配する空間。時が止まったようだ。",
        },
        FloorTheme::VolcanicDepths => match rng_val % 8 {
            0 => "空気が熱い。壁の裂け目から赤い光が漏れる。",
            1 => "足元の岩が温かい。溶岩が近い証拠だ。",
            2 => "硫黄の匂いが鼻を突く。",
            3 => "遠くで地鳴りがした。",
            4 => "壁面が赤熱している。触れたら火傷する。",
            5 => "蒸気が噴き出している。視界が悪い。",
            6 => "溶岩の流れが見える。美しいが恐ろしい。",
            _ => "汗が止まらない。水の消費が激しい。",
        },
        FloorTheme::DemonCastle => match rng_val % 8 {
            0 => "闇が濃い。松明の光が吸い込まれていく。",
            1 => "壁面が赤黒く脈動している。生きているようだ。",
            2 => "遠くで何者かの嘲笑が聞こえる。",
            3 => "空気が重い。魔力の気配が充満している。",
            4 => "黒い霧が足元を這っている。",
            5 => "壁に爪痕がある。巨大な何かが通った跡だ。",
            6 => "不気味な静けさ。嵐の前の静けさに似ている。",
            _ => "魔王の気配が近い。全身が震える。",
        },
    }
}

/// Get dungeon entry flavor text.
pub fn floor_entry_text(floor: u32, theme: FloorTheme) -> Vec<String> {
    let theme_desc = match theme {
        FloorTheme::MossyRuins => "苔と湿気に覆われた古い遺跡。微かに光る苔が道を照らす。",
        FloorTheme::Underground => "水の流れる音が響く地下世界。暗い水面が足元に広がる。",
        FloorTheme::AncientTemple => "荘厳な古代神殿の遺構。崩れた柱の間から光が差し込む。",
        FloorTheme::VolcanicDepths => "灼熱の空気が肌を焼く坑道。壁の裂け目から溶岩の光。",
        FloorTheme::DemonCastle => "闇に沈む魔王の居城。壁が赤黒く脈動している。",
    };

    vec![
        format!("── B{}F ──", floor),
        format!("〈{}〉", theme_name(theme)),
        String::new(),
        theme_desc.into(),
    ]
}

/// Story fragments found in lore cells. These reveal the dungeon's backstory.
#[cfg(test)]
pub fn story_fragment(floor: u32, fragment_id: u32) -> &'static str {
    match (floor, fragment_id % 3) {
        (1..=2, 0) => "手記:「このダンジョンは千年前、魔術師たちの研究施設だった。彼らは禁忌の力を求めてここに潜った…」",
        (1..=2, 1) => "碑文:「封印の門を開く者よ、覚悟せよ。この先にあるのは知識と…狂気だ」",
        (1..=2, _) => "壁画: 繁栄する街と、その地下に広がる迷宮。二つは共存していたようだ。",
        (3..=4, 0) => "手記:「地下水脈に辿り着いた。ここの水には不思議な力がある。傷が癒える…だが長居は危険だ」",
        (3..=4, 1) => "碑文:「水の守護者は外敵を許さない。この地を荒らす者には罰が下る」",
        (3..=4, _) => "壁画: 地底の湖で祈りを捧げる人々。彼らは水の精霊を信仰していたらしい。",
        (5..=6, 0) => "手記:「古代神殿に到達した。ここにはかつて神官たちが暮らしていた。魔王が現れるまでは…」",
        (5..=6, 1) => "碑文:「神の加護はとうに失われた。残るは形骸だけだ」",
        (5..=6, _) => "壁画: 光に包まれた神官と、闇から這い出る影。二つの力の戦いが描かれている。",
        (7..=8, 0) => "手記:「灼熱の坑道…かつてここでドワーフたちが聖なる武器を鍛えていた。彼らの技術があれば…」",
        (7..=8, 1) => "碑文:「炎の試練を超えし者のみ、王の間への道を開く」",
        (7..=8, _) => "壁画: 溶岩の中で剣を鍛える職人たち。彼らの作品は今も最強の武器として語り継がれる。",
        (_, 0) => "手記:「魔王の正体を知った…彼はかつてこのダンジョンの主席研究者だった。力を求め過ぎた…」",
        (_, 1) => "碑文:「全ての始まりにして終わり。扉の向こうに真実がある」",
        (_, _) => "壁画: 人間が闇の力に飲まれ、魔王へと変貌する過程が克明に描かれている。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_assigned_correctly() {
        assert_eq!(floor_theme(1), FloorTheme::MossyRuins);
        assert_eq!(floor_theme(3), FloorTheme::Underground);
        assert_eq!(floor_theme(5), FloorTheme::AncientTemple);
        assert_eq!(floor_theme(7), FloorTheme::VolcanicDepths);
        assert_eq!(floor_theme(10), FloorTheme::DemonCastle);
    }

    #[test]
    fn atmosphere_text_varies() {
        let t1 = atmosphere_text(FloorTheme::MossyRuins, 0);
        let t2 = atmosphere_text(FloorTheme::MossyRuins, 1);
        assert_ne!(t1, t2);
    }

    #[test]
    fn story_fragments_exist_for_all_floors() {
        for floor in 1..=10 {
            for fid in 0..3 {
                let text = story_fragment(floor, fid);
                assert!(!text.is_empty());
            }
        }
    }

    #[test]
    fn entry_text_not_empty() {
        let text = floor_entry_text(1, FloorTheme::MossyRuins);
        assert!(!text.is_empty());
    }
}
