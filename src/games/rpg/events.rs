//! Interactive dungeon events — choices that shape the exploration experience.
//!
//! Each cell type can trigger an event with multiple choices.
//! Events create the "exploration feel" that was missing from the linear system.

use super::state::{
    CellType, DungeonEvent, EventAction, EventChoice, FloorTheme, ItemKind,
};

// ── RNG (same LCG) ──────────────────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn rng_range(seed: &mut u64, max: u32) -> u32 {
    *seed = next_rng(*seed);
    ((*seed >> 33) % max as u64) as u32
}

// ── Event Generation ──────────────────────────────────────────

/// Generate an interactive event based on cell type and floor theme.
pub fn generate_event(
    cell_type: CellType,
    floor: u32,
    theme: FloorTheme,
    rng_seed: &mut u64,
) -> Option<DungeonEvent> {
    match cell_type {
        CellType::Treasure => Some(treasure_event(floor, theme, rng_seed)),
        CellType::Enemy => Some(enemy_event(floor, theme, rng_seed)),
        CellType::Trap => Some(trap_event(floor, theme, rng_seed)),
        CellType::Spring => Some(spring_event(theme)),
        CellType::Lore => Some(lore_event(floor, rng_seed)),
        CellType::Npc => Some(npc_event(floor, rng_seed)),
        CellType::Stairs => Some(stairs_event(floor)),
        CellType::Entrance => Some(entrance_event()),
        CellType::Corridor => None,
    }
}

fn treasure_event(floor: u32, theme: FloorTheme, rng_seed: &mut u64) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => {
            let texts = ["苔に覆われた宝箱を見つけた。", "壁の窪みに朽ちた箱がある。"];
            texts[rng_range(rng_seed, texts.len() as u32) as usize]
        }
        FloorTheme::Underground => "地下水に半分沈んだ宝箱がある。",
        FloorTheme::AncientTemple => "祭壇の上に装飾された箱が置かれている。",
        FloorTheme::VolcanicDepths => "溶岩の縁に耐熱の箱が残されている。",
        FloorTheme::DemonCastle => "禍々しい紋章が刻まれた箱がある。",
    };

    let search_hint = if floor >= 4 {
        "調べる (罠を確認)"
    } else {
        "慎重に調べる"
    };

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice {
                label: "開ける".into(),
                action: EventAction::OpenTreasure,
            },
            EventChoice {
                label: search_hint.into(),
                action: EventAction::SearchTreasure,
            },
            EventChoice {
                label: "無視する".into(),
                action: EventAction::Ignore,
            },
        ],
    }
}

fn enemy_event(floor: u32, theme: FloorTheme, rng_seed: &mut u64) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => {
            let texts = [
                "物陰に何かが蠢いている…！",
                "暗がりから敵意を感じる。",
            ];
            texts[rng_range(rng_seed, texts.len() as u32) as usize]
        }
        FloorTheme::Underground => "水面に映る影…何かいる。",
        FloorTheme::AncientTemple => "神殿の守護者が立ちはだかる。",
        FloorTheme::VolcanicDepths => "灼熱の中に蠢く影。",
        FloorTheme::DemonCastle => "闇の中から殺気が迸る。",
    };

    let sneak_label = if floor >= 3 {
        "忍び足で通り抜ける"
    } else {
        "そっと通り過ぎる"
    };

    DungeonEvent {
        description: vec![desc.into(), "敵はまだこちらに気づいていない。".into()],
        choices: vec![
            EventChoice {
                label: "奇襲する".into(),
                action: EventAction::Ambush,
            },
            EventChoice {
                label: sneak_label.into(),
                action: EventAction::SneakPast,
            },
            EventChoice {
                label: "正面から戦う".into(),
                action: EventAction::FightNormally,
            },
        ],
    }
}

fn trap_event(floor: u32, theme: FloorTheme, rng_seed: &mut u64) -> DungeonEvent {
    // Traps are disguised — the description hints but doesn't reveal
    let desc = match theme {
        FloorTheme::MossyRuins => {
            let texts = [
                "足元の石畳が微妙に浮いている。",
                "壁から何かが突き出ている…仕掛け？",
            ];
            texts[rng_range(rng_seed, texts.len() as u32) as usize]
        }
        FloorTheme::Underground => "通路の床が不自然に光っている。",
        FloorTheme::AncientTemple => "床に複雑な紋様が描かれている。",
        FloorTheme::VolcanicDepths => "足元から微かな振動を感じる。",
        FloorTheme::DemonCastle => "魔法陣のような痕跡が床に残る。",
    };

    let _ = floor; // may use later for trap difficulty hints

    DungeonEvent {
        description: vec![desc.into(), "嫌な予感がする…".into()],
        choices: vec![
            EventChoice {
                label: "慎重に進む".into(),
                action: EventAction::SearchTreasure, // reuse: search = careful
            },
            EventChoice {
                label: "そのまま通り抜ける".into(),
                action: EventAction::OpenTreasure, // reuse: open = direct
            },
            EventChoice {
                label: "引き返す".into(),
                action: EventAction::Ignore,
            },
        ],
    }
}

fn spring_event(theme: FloorTheme) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => "苔の間から澄んだ水が湧き出ている。",
        FloorTheme::Underground => "地底湖の端に清水が流れている。",
        FloorTheme::AncientTemple => "聖なる泉が淡い光を放っている。",
        FloorTheme::VolcanicDepths => "溶岩の中に不思議な冷泉がある。",
        FloorTheme::DemonCastle => "闇の中に癒しの力を持つ泉が。",
    };

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice {
                label: "水を飲む (HP/MP回復)".into(),
                action: EventAction::DrinkSpring,
            },
            EventChoice {
                label: "瓶に汲む (薬草入手)".into(),
                action: EventAction::FillBottle,
            },
            EventChoice {
                label: "先に進む".into(),
                action: EventAction::Ignore,
            },
        ],
    }
}

fn lore_event(floor: u32, rng_seed: &mut u64) -> DungeonEvent {
    let lore_id = floor * 10 + rng_range(rng_seed, 3);
    let desc = match lore_id % 8 {
        0 => "壁に古い文字が刻まれている。前の冒険者の記録のようだ。",
        1 => "朽ちた書物が床に落ちている。まだ読める部分がある。",
        2 => "石碑に何かが彫られている。古代の言葉のようだ。",
        3 => "壁画が残っている。このダンジョンの歴史を描いているらしい。",
        4 => "冒険者の骸が壁にもたれている。手記を握りしめている。",
        5 => "祭壇の裏に隠された文書がある。",
        6 => "水晶に映像が浮かんでいる。過去の出来事が再生されている。",
        _ => "奇妙な紋章が壁に刻まれている。魔力を帯びているようだ。",
    };

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice {
                label: "読む".into(),
                action: EventAction::ReadLore,
            },
            EventChoice {
                label: "先に進む".into(),
                action: EventAction::Ignore,
            },
        ],
    }
}

fn npc_event(floor: u32, rng_seed: &mut u64) -> DungeonEvent {
    let npc_type = rng_range(rng_seed, 3);
    let (desc, talk_label, trade_label) = match npc_type {
        0 => (
            "傷ついた冒険者がうずくまっている。",
            "話しかける",
            "薬草を分ける",
        ),
        1 => (
            "謎の商人が松明の下で休んでいる。",
            "話を聞く",
            "取引する",
        ),
        _ => (
            "放浪の魔術師がこちらを見ている。",
            "話しかける",
            "助けを求める",
        ),
    };

    let _ = floor; // may use for NPC inventory scaling

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice {
                label: talk_label.into(),
                action: EventAction::TalkNpc,
            },
            EventChoice {
                label: trade_label.into(),
                action: EventAction::TradeNpc,
            },
            EventChoice {
                label: "立ち去る".into(),
                action: EventAction::Ignore,
            },
        ],
    }
}

fn stairs_event(floor: u32) -> DungeonEvent {
    let desc = if floor >= 10 {
        "巨大な扉の前に立っている。向こう側から圧倒的な魔力を感じる。"
    } else {
        "下へ続く階段を見つけた。より深い闇が待っている。"
    };

    let descend_label = if floor >= 10 {
        "扉を開ける (ボス戦)"
    } else {
        &format!("B{}Fへ降りる", floor + 1)
    };

    DungeonEvent {
        description: vec![
            desc.into(),
            format!("現在: B{}F", floor),
        ],
        choices: vec![
            EventChoice {
                label: descend_label.to_string(),
                action: EventAction::DescendStairs,
            },
            EventChoice {
                label: "探索を続ける".into(),
                action: EventAction::Continue,
            },
        ],
    }
}

fn entrance_event() -> DungeonEvent {
    DungeonEvent {
        description: vec!["入口の階段がある。町へ戻れる。".into()],
        choices: vec![
            EventChoice {
                label: "町に帰還する".into(),
                action: EventAction::ReturnToTown,
            },
            EventChoice {
                label: "探索を続ける".into(),
                action: EventAction::Continue,
            },
        ],
    }
}

// ── Event Resolution ──────────────────────────────────────────

/// Result of resolving an event choice.
pub struct EventOutcome {
    pub description: Vec<String>,
    pub gold: i32,
    pub hp_change: i32,
    pub mp_change: i32,
    pub item: Option<(ItemKind, u32)>,
    pub start_battle: bool,
    pub first_strike: bool,
    pub descend: bool,
    pub return_to_town: bool,
    pub lore_id: Option<u32>,
}

impl EventOutcome {
    fn empty() -> Self {
        Self {
            description: Vec::new(),
            gold: 0,
            hp_change: 0,
            mp_change: 0,
            item: None,
            start_battle: false,
            first_strike: false,
            descend: false,
            return_to_town: false,
            lore_id: None,
        }
    }
}

/// Resolve an event choice and produce an outcome.
pub fn resolve_event(
    action: &EventAction,
    cell_type: CellType,
    floor: u32,
    player_level: u32,
    rng_seed: &mut u64,
) -> EventOutcome {
    match (action, cell_type) {
        // Treasure: Open directly
        (EventAction::OpenTreasure, CellType::Treasure) => {
            let trap_chance = 15 + floor * 2; // 17% at F1, 35% at F10
            if rng_range(rng_seed, 100) < trap_chance {
                // Trapped!
                let damage = 5 + floor * 3;
                EventOutcome {
                    description: vec!["罠だ！ 宝箱に仕掛けがあった！".into(), format!("{}ダメージを受けた！", damage)],
                    hp_change: -(damage as i32),
                    ..EventOutcome::empty()
                }
            } else {
                treasure_reward(floor, rng_seed)
            }
        }
        // Treasure: Search carefully
        (EventAction::SearchTreasure, CellType::Treasure) => {
            // Searching avoids traps but takes time (could attract enemies later)
            let mut outcome = treasure_reward(floor, rng_seed);
            outcome.description.insert(0, "慎重に調べた…罠はなかった。".into());
            // Slightly less gold as "cost" of being careful
            outcome.gold = (outcome.gold as f32 * 0.8) as i32;
            outcome
        }
        // Trap: Rush through
        (EventAction::OpenTreasure, CellType::Trap) => {
            let damage = 8 + floor * 3 + rng_range(rng_seed, floor * 2);
            EventOutcome {
                description: vec!["罠が発動した！".into(), format!("{}ダメージ！", damage)],
                hp_change: -(damage as i32),
                ..EventOutcome::empty()
            }
        }
        // Trap: Move carefully
        (EventAction::SearchTreasure, CellType::Trap) => {
            let avoid_chance = 40 + player_level * 5; // 45% at lv1, 90% at lv10
            if rng_range(rng_seed, 100) < avoid_chance {
                EventOutcome {
                    description: vec!["罠を見破った！ 慎重に回避した。".into()],
                    ..EventOutcome::empty()
                }
            } else {
                let damage = (5 + floor * 2) / 2; // half damage
                EventOutcome {
                    description: vec!["罠を発見したが避けきれなかった！".into(), format!("{}ダメージ (軽減)", damage)],
                    hp_change: -(damage as i32),
                    ..EventOutcome::empty()
                }
            }
        }
        // Enemy: Ambush
        (EventAction::Ambush, CellType::Enemy) => {
            EventOutcome {
                description: vec!["不意を突いた！ 先制攻撃！".into()],
                start_battle: true,
                first_strike: true,
                ..EventOutcome::empty()
            }
        }
        // Enemy: Sneak past
        (EventAction::SneakPast, CellType::Enemy) => {
            let sneak_chance = 30 + player_level * 5; // 35% at lv1, 80% at lv10
            if rng_range(rng_seed, 100) < sneak_chance {
                EventOutcome {
                    description: vec!["気づかれずに通り抜けた！".into()],
                    ..EventOutcome::empty()
                }
            } else {
                EventOutcome {
                    description: vec!["見つかった！ 不意打ちされた！".into()],
                    start_battle: true,
                    first_strike: false,
                    hp_change: -(floor as i32 * 2), // surprise damage
                    ..EventOutcome::empty()
                }
            }
        }
        // Enemy: Fight normally
        (EventAction::FightNormally, CellType::Enemy) => {
            EventOutcome {
                description: vec!["正面から立ち向かった！".into()],
                start_battle: true,
                first_strike: false,
                ..EventOutcome::empty()
            }
        }
        // Spring: Drink
        (EventAction::DrinkSpring, CellType::Spring) => {
            EventOutcome {
                description: vec!["澄んだ水で体を癒した。".into(), "HP/MPが25%回復した。".into()],
                hp_change: 9999, // special: means 25% heal (handled by caller)
                mp_change: 9999,
                ..EventOutcome::empty()
            }
        }
        // Spring: Fill bottle
        (EventAction::FillBottle, CellType::Spring) => {
            EventOutcome {
                description: vec!["泉の水を瓶に汲んだ。".into(), "薬草を1つ入手した。".into()],
                item: Some((ItemKind::Herb, 1)),
                ..EventOutcome::empty()
            }
        }
        // Lore: Read
        (EventAction::ReadLore, CellType::Lore) => {
            let lore_id = floor * 10 + rng_range(rng_seed, 5);
            let text = lore_text(lore_id);
            EventOutcome {
                description: vec!["記録を読んだ：".into(), text.into()],
                lore_id: Some(lore_id),
                ..EventOutcome::empty()
            }
        }
        // NPC: Talk
        (EventAction::TalkNpc, CellType::Npc) => {
            let hint = npc_hint(floor, rng_seed);
            EventOutcome {
                description: vec![hint],
                ..EventOutcome::empty()
            }
        }
        // NPC: Trade
        (EventAction::TradeNpc, CellType::Npc) => {
            let item = if floor >= 5 {
                ItemKind::MagicWater
            } else {
                ItemKind::Herb
            };
            EventOutcome {
                description: vec!["お礼にアイテムを受け取った。".into()],
                item: Some((item, 1)),
                ..EventOutcome::empty()
            }
        }
        // Stairs: Descend
        (EventAction::DescendStairs, CellType::Stairs) => {
            EventOutcome {
                description: vec![format!("B{}Fへ降りる…", floor + 1)],
                descend: true,
                ..EventOutcome::empty()
            }
        }
        // Entrance: Return to town
        (EventAction::ReturnToTown, CellType::Entrance) => {
            EventOutcome {
                description: vec!["町へ帰還する。".into()],
                return_to_town: true,
                ..EventOutcome::empty()
            }
        }
        // Ignore / Continue
        (EventAction::Ignore | EventAction::Continue, _) => {
            EventOutcome {
                description: vec!["先に進むことにした。".into()],
                ..EventOutcome::empty()
            }
        }
        // Default
        _ => EventOutcome {
            description: vec!["何も起こらなかった。".into()],
            ..EventOutcome::empty()
        },
    }
}

fn treasure_reward(floor: u32, rng_seed: &mut u64) -> EventOutcome {
    let roll = rng_range(rng_seed, 100);
    if roll < 50 {
        let gold = 15 + floor * 10 + rng_range(rng_seed, floor * 5);
        EventOutcome {
            description: vec!["宝箱を開けた！".into(), format!("{}Gを手に入れた！", gold)],
            gold: gold as i32,
            ..EventOutcome::empty()
        }
    } else if roll < 80 {
        let count = 1 + rng_range(rng_seed, 2);
        EventOutcome {
            description: vec!["宝箱を開けた！".into(), format!("薬草x{}を手に入れた！", count)],
            item: Some((ItemKind::Herb, count)),
            ..EventOutcome::empty()
        }
    } else {
        let item = if floor >= 6 {
            ItemKind::StrengthPotion
        } else {
            ItemKind::MagicWater
        };
        let name = match item {
            ItemKind::MagicWater => "魔法の水",
            ItemKind::StrengthPotion => "力の薬",
            _ => "アイテム",
        };
        EventOutcome {
            description: vec!["宝箱を開けた！".into(), format!("{}を手に入れた！", name)],
            item: Some((item, 1)),
            ..EventOutcome::empty()
        }
    }
}

fn lore_text(lore_id: u32) -> &'static str {
    match lore_id % 15 {
        0 => "「この先にある泉は…癒しの力を持つ。覚えておけ」— ある冒険者の手記",
        1 => "「魔王は…かつて人間だった。力に呑まれた哀れな存在だ」",
        2 => "「B5Fから先は別世界だ。空気すら変わる。準備を怠るな」",
        3 => "「炎の魔物には氷が効く。雷の魔物には…覚えていない」",
        4 => "この碑文は古代の祈りが刻まれている。読むと少し心が落ち着く。",
        5 => "「ゴーレムは力を溜めてから攻撃する。その隙にシールドを」",
        6 => "「闇の騎士は雷に弱い。かつての同胞が残した情報だ」",
        7 => "「このダンジョンは千年前に封印された禁忌の地だ」",
        8 => "「ドラゴンのブレスは凄まじい。氷の刃で怯ませろ」",
        9 => "「魔王の闇の波動…弱点はない。ただ力で押すしかない」",
        10 => "「最深部に辿り着いた者は片手で数えるほどだ」",
        11 => "壁画には豊かだった頃のこの地が描かれている。魔王が現れる前の世界…",
        12 => "「引き返す勇気も大切だ。命あっての物種」",
        13 => "「泉で瓶に水を汲めば薬草代わりになる。覚えておけ」",
        _ => "「帰還できたら儲けもの。欲張り過ぎるな」— 生き残った冒険者より",
    }
}

fn npc_hint(floor: u32, rng_seed: &mut u64) -> String {
    let hints = match floor {
        1..=3 => vec![
            "「この先は行き止まりが多い。地図を頭に入れておけ」",
            "「宝箱には罠があることもある。慎重にな」",
            "「敵に気づかれる前に奇襲するのが一番だ」",
        ],
        4..=6 => vec![
            "「ゴーレムが力を溜めたらシールドを使え」",
            "「この階層から先は罠が増える。注意しろ」",
            "「鋼の剣があれば中層は楽になるぞ」",
        ],
        7..=9 => vec![
            "「ドラゴンにはアイスブレードが効く」",
            "「魔王の居城が近い…覚悟はいいか」",
            "「聖剣があれば魔王にも対抗できる」",
        ],
        _ => vec![
            "「ここが最後だ。全力で行け」",
            "「魔王に弱点はない。持てる全てを注ぎ込め」",
        ],
    };
    let idx = rng_range(rng_seed, hints.len() as u32) as usize;
    hints[idx].to_string()
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treasure_event_has_choices() {
        let mut seed = 42u64;
        let event = generate_event(CellType::Treasure, 1, FloorTheme::MossyRuins, &mut seed);
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.choices.len(), 3);
    }

    #[test]
    fn enemy_event_has_choices() {
        let mut seed = 42u64;
        let event = generate_event(CellType::Enemy, 1, FloorTheme::MossyRuins, &mut seed);
        assert!(event.is_some());
        let event = event.unwrap();
        assert!(event.choices.len() >= 3);
    }

    #[test]
    fn corridor_has_no_event() {
        let mut seed = 42u64;
        let event = generate_event(CellType::Corridor, 1, FloorTheme::MossyRuins, &mut seed);
        assert!(event.is_none());
    }

    #[test]
    fn resolve_treasure_open() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::OpenTreasure,
            CellType::Treasure,
            1,
            1,
            &mut seed,
        );
        assert!(!outcome.description.is_empty());
        // Either got treasure or got trapped
        assert!(outcome.gold > 0 || outcome.hp_change < 0 || outcome.item.is_some());
    }

    #[test]
    fn resolve_enemy_ambush_starts_battle() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::Ambush,
            CellType::Enemy,
            1,
            1,
            &mut seed,
        );
        assert!(outcome.start_battle);
        assert!(outcome.first_strike);
    }

    #[test]
    fn resolve_spring_heals() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::DrinkSpring,
            CellType::Spring,
            1,
            1,
            &mut seed,
        );
        assert_eq!(outcome.hp_change, 9999); // sentinel for 25% heal
    }

    #[test]
    fn resolve_stairs_descends() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::DescendStairs,
            CellType::Stairs,
            3,
            5,
            &mut seed,
        );
        assert!(outcome.descend);
    }
}
