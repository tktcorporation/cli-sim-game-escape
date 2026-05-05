//! Interactive dungeon events — choices for treasure / spring / lore /
//! npc / trap / stairs / entrance cells. Enemy encounters are handled
//! inline (monster entities on the grid).

use super::state::{
    CellType, DungeonEvent, EnemyKind, EventAction, EventChoice, FloorTheme, ItemKind,
};

// ── RNG ─────────────────────────────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn rng_range(seed: &mut u64, max: u32) -> u32 {
    *seed = next_rng(*seed);
    ((*seed >> 33) % max as u64) as u32
}

// ── Event Generation ────────────────────────────────────────

pub fn generate_event(
    cell_type: CellType,
    floor: u32,
    theme: FloorTheme,
    rng_seed: &mut u64,
) -> Option<DungeonEvent> {
    match cell_type {
        CellType::Treasure => Some(treasure_event(floor, theme, rng_seed)),
        CellType::Trap => Some(trap_event(floor, theme, rng_seed)),
        CellType::Spring => Some(spring_event(theme)),
        CellType::Lore => Some(lore_event(floor, rng_seed)),
        CellType::Npc => Some(npc_event(floor, rng_seed)),
        CellType::Stairs => Some(stairs_event(floor)),
        CellType::Entrance => Some(entrance_event()),
        CellType::Corridor => None,
        CellType::FallenAdventurer => Some(fallen_adventurer_event(rng_seed)),
        CellType::FruitTree => Some(fruit_tree_event(theme)),
        CellType::Well => Some(well_event(theme)),
        CellType::Idol => Some(idol_event(theme)),
        CellType::Peddler => Some(peddler_event(rng_seed)),
        CellType::MonsterEgg => Some(monster_egg_event(rng_seed)),
    }
}

// ── Issue #90: New event generators ─────────────────────────

fn fallen_adventurer_event(rng_seed: &mut u64) -> DungeonEvent {
    let descs = [
        "倒れた冒険者を見つけた。装備が立派だ…",
        "壁にもたれて息絶えた冒険者がいる。",
        "うつ伏せに倒れた冒険者。微かに息がある？",
    ];
    let idx = rng_range(rng_seed, descs.len() as u32) as usize;
    DungeonEvent {
        description: vec![descs[idx].into()],
        choices: vec![
            EventChoice { label: "助け起こす".into(), action: EventAction::ReviveAdventurer },
            EventChoice { label: "装備を奪う".into(), action: EventAction::LootAdventurer },
            EventChoice { label: "見過ごす".into(), action: EventAction::Ignore },
        ],
    }
}

fn fruit_tree_event(theme: FloorTheme) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => "崩れた壁から太い枝が伸びている。実が成っているようだ。",
        FloorTheme::Underground => "地下水脈の傍に奇妙な果樹が育っている。",
        FloorTheme::AncientTemple => "祭壇の脇に古い果樹が残されている。",
        FloorTheme::VolcanicDepths => "熱気の中、紅い実をつけた樹が立っている。",
        FloorTheme::DemonCastle => "歪な果実をつけた黒い樹がある。",
    };
    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice { label: "実を採る (満腹度+リンゴ)".into(), action: EventAction::PickFruit },
            EventChoice { label: "木を揺する (大量+リスク)".into(), action: EventAction::ShakeTree },
            EventChoice { label: "通り過ぎる".into(), action: EventAction::Ignore },
        ],
    }
}

fn well_event(theme: FloorTheme) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => "古い石組みの井戸がある。深く暗い。",
        FloorTheme::Underground => "地下水を汲み上げる井戸が残っている。",
        FloorTheme::AncientTemple => "聖域の井戸。水面が淡く輝く。",
        FloorTheme::VolcanicDepths => "熱湯ではない不思議な冷水井戸。",
        FloorTheme::DemonCastle => "底が見えぬ漆黒の井戸…",
    };
    DungeonEvent {
        description: vec![desc.into(), "水を飲むのは賭けだ。".into()],
        choices: vec![
            EventChoice { label: "水を飲む (運次第)".into(), action: EventAction::DrinkWell },
            EventChoice { label: "瓶に汲む (薬草化)".into(), action: EventAction::BottleWell },
            EventChoice { label: "覗き込む".into(), action: EventAction::PeerWell },
            EventChoice { label: "離れる".into(), action: EventAction::Ignore },
        ],
    }
}

fn idol_event(theme: FloorTheme) -> DungeonEvent {
    let desc = match theme {
        FloorTheme::MossyRuins => "苔むした神像が静かに立っている。",
        FloorTheme::Underground => "地下に佇む朽ちかけた神像。",
        FloorTheme::AncientTemple => "黄金に光る荘厳な神像。",
        FloorTheme::VolcanicDepths => "炎で焦げた神像が残されている。",
        FloorTheme::DemonCastle => "禍々しい彫像。これは…神か？",
    };
    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice { label: "祈りを捧げる (信仰+1)".into(), action: EventAction::PrayIdol },
            EventChoice { label: "薬草を供える (恵み)".into(), action: EventAction::OfferIdol },
            EventChoice { label: "そっと立ち去る".into(), action: EventAction::Ignore },
        ],
    }
}

fn peddler_event(rng_seed: &mut u64) -> DungeonEvent {
    let descs = [
        "怪しい行商人が荷を広げている。",
        "頭巾を被った商人が呼び止めてきた。",
        "深層では珍しい行商人だ。",
    ];
    let idx = rng_range(rng_seed, descs.len() as u32) as usize;
    DungeonEvent {
        description: vec![
            descs[idx].into(),
            "「特別価格でいかがですか？」".into(),
        ],
        choices: vec![
            EventChoice { label: "薬草 (15G)".into(), action: EventAction::PeddlerBuyHerb },
            EventChoice { label: "魔法の水 (40G)".into(), action: EventAction::PeddlerBuyMagicWater },
            EventChoice { label: "パン (12G)".into(), action: EventAction::PeddlerBuyBread },
            EventChoice { label: "立ち去る".into(), action: EventAction::Ignore },
        ],
    }
}

fn monster_egg_event(rng_seed: &mut u64) -> DungeonEvent {
    let descs = [
        "床に大きな卵が転がっている。微かに脈動している。",
        "巣に置き去りの卵。何かが孵りそうだ…",
    ];
    let idx = rng_range(rng_seed, descs.len() as u32) as usize;
    DungeonEvent {
        description: vec![descs[idx].into()],
        choices: vec![
            EventChoice { label: "持ち帰る (ペット化挑戦)".into(), action: EventAction::TakeEgg },
            EventChoice { label: "割る (黄身を食べる)".into(), action: EventAction::BreakEgg },
            EventChoice { label: "そっとしておく".into(), action: EventAction::Ignore },
        ],
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

    let search_hint = if floor >= 4 { "調べる (罠を確認)" } else { "慎重に調べる" };

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice { label: "開ける".into(), action: EventAction::OpenTreasure },
            EventChoice { label: search_hint.into(), action: EventAction::SearchTreasure },
            EventChoice { label: "無視する".into(), action: EventAction::Ignore },
        ],
    }
}

fn trap_event(floor: u32, theme: FloorTheme, rng_seed: &mut u64) -> DungeonEvent {
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

    let _ = floor;

    DungeonEvent {
        description: vec![desc.into(), "嫌な予感がする…".into()],
        choices: vec![
            EventChoice { label: "慎重に進む".into(), action: EventAction::SearchTreasure },
            EventChoice { label: "そのまま通り抜ける".into(), action: EventAction::OpenTreasure },
            EventChoice { label: "引き返す".into(), action: EventAction::Ignore },
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
            EventChoice { label: "水を飲む (HP/MP回復)".into(), action: EventAction::DrinkSpring },
            EventChoice { label: "瓶に汲む (薬草入手)".into(), action: EventAction::FillBottle },
            EventChoice { label: "先に進む".into(), action: EventAction::Ignore },
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
            EventChoice { label: "読む".into(), action: EventAction::ReadLore },
            EventChoice { label: "先に進む".into(), action: EventAction::Ignore },
        ],
    }
}

fn npc_event(floor: u32, rng_seed: &mut u64) -> DungeonEvent {
    let npc_type = rng_range(rng_seed, 3);
    let (desc, talk_label, trade_label) = match npc_type {
        0 => ("傷ついた冒険者がうずくまっている。", "話しかける", "薬草を分ける"),
        1 => ("謎の商人が松明の下で休んでいる。", "話を聞く", "取引する"),
        _ => ("放浪の魔術師がこちらを見ている。", "話しかける", "助けを求める"),
    };
    let _ = floor;

    DungeonEvent {
        description: vec![desc.into()],
        choices: vec![
            EventChoice { label: talk_label.into(), action: EventAction::TalkNpc },
            EventChoice { label: trade_label.into(), action: EventAction::TradeNpc },
            EventChoice { label: "立ち去る".into(), action: EventAction::Ignore },
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
        "扉を開ける (ボスフロアへ)".to_string()
    } else {
        format!("B{}Fへ降りる", floor + 1)
    };

    DungeonEvent {
        description: vec![desc.into(), format!("現在: B{}F", floor)],
        choices: vec![
            EventChoice { label: descend_label, action: EventAction::DescendStairs },
            EventChoice { label: "探索を続ける".into(), action: EventAction::Continue },
        ],
    }
}

fn entrance_event() -> DungeonEvent {
    DungeonEvent {
        description: vec!["入口の階段がある。町へ戻れる。".into()],
        choices: vec![
            EventChoice { label: "町に帰還する".into(), action: EventAction::ReturnToTown },
            EventChoice { label: "探索を続ける".into(), action: EventAction::Continue },
        ],
    }
}

// ── Event Resolution ────────────────────────────────────────

pub struct EventOutcome {
    pub description: Vec<String>,
    /// Net gold change; negative deducts (used by peddler).
    pub gold: i32,
    pub hp_change: i32,
    pub mp_change: i32,
    pub item: Option<(ItemKind, u32)>,
    pub descend: bool,
    pub return_to_town: bool,
    pub lore_id: Option<u32>,
    /// Issue #90: outcome may extend the satiety bar (fruit/peddler bread).
    pub satiety_change: i32,
    /// Issue #90: outcome may grant +faith (idol).
    pub faith_change: u32,
    /// Issue #90: spawn a pet of this kind on success (egg event).
    pub spawn_pet: Option<EnemyKind>,
    /// Issue #90: spawn a hostile monster adjacent to player.
    /// Used by ShakeTree / failed TakeEgg.
    pub spawn_hostile: Option<EnemyKind>,
    /// Issue #90: consume one of these from inventory before applying.
    /// If absent the outcome falls back to a "no offering" message.
    pub require_consume: Option<ItemKind>,
}

impl EventOutcome {
    fn empty() -> Self {
        Self {
            description: Vec::new(),
            gold: 0,
            hp_change: 0,
            mp_change: 0,
            item: None,
            descend: false,
            return_to_town: false,
            lore_id: None,
            satiety_change: 0,
            faith_change: 0,
            spawn_pet: None,
            spawn_hostile: None,
            require_consume: None,
        }
    }
}

pub fn resolve_event(
    action: &EventAction,
    cell_type: CellType,
    floor: u32,
    player_level: u32,
    rng_seed: &mut u64,
) -> EventOutcome {
    match (action, cell_type) {
        (EventAction::OpenTreasure, CellType::Treasure) => {
            let trap_chance = 15 + floor * 2;
            if rng_range(rng_seed, 100) < trap_chance {
                let damage = 5 + floor * 3;
                EventOutcome {
                    description: vec![
                        "罠だ！ 宝箱に仕掛けがあった！".into(),
                        format!("{}ダメージを受けた！", damage),
                    ],
                    hp_change: -(damage as i32),
                    ..EventOutcome::empty()
                }
            } else {
                treasure_reward(floor, rng_seed)
            }
        }
        (EventAction::SearchTreasure, CellType::Treasure) => {
            let mut outcome = treasure_reward(floor, rng_seed);
            outcome.description.insert(0, "慎重に調べた…罠はなかった。".into());
            outcome.gold = (outcome.gold as f32 * 0.8) as i32;
            outcome
        }
        (EventAction::OpenTreasure, CellType::Trap) => {
            let damage = 8 + floor * 3 + rng_range(rng_seed, floor * 2);
            EventOutcome {
                description: vec!["罠が発動した！".into(), format!("{}ダメージ！", damage)],
                hp_change: -(damage as i32),
                ..EventOutcome::empty()
            }
        }
        (EventAction::SearchTreasure, CellType::Trap) => {
            let avoid_chance = 40 + player_level * 5;
            if rng_range(rng_seed, 100) < avoid_chance {
                EventOutcome {
                    description: vec!["罠を見破った！ 慎重に回避した。".into()],
                    ..EventOutcome::empty()
                }
            } else {
                let damage = (5 + floor * 2) / 2;
                EventOutcome {
                    description: vec![
                        "罠を発見したが避けきれなかった！".into(),
                        format!("{}ダメージ (軽減)", damage),
                    ],
                    hp_change: -(damage as i32),
                    ..EventOutcome::empty()
                }
            }
        }
        (EventAction::DrinkSpring, CellType::Spring) => EventOutcome {
            description: vec![
                "澄んだ水で体を癒した。".into(),
                "HP/MPが25%回復した。".into(),
            ],
            hp_change: 9999,
            mp_change: 9999,
            ..EventOutcome::empty()
        },
        (EventAction::FillBottle, CellType::Spring) => EventOutcome {
            description: vec!["泉の水を瓶に汲んだ。".into(), "薬草を1つ入手した。".into()],
            item: Some((ItemKind::Herb, 1)),
            ..EventOutcome::empty()
        },
        (EventAction::ReadLore, CellType::Lore) => {
            let lore_id = floor * 10 + rng_range(rng_seed, 5);
            let text = lore_text(lore_id);
            EventOutcome {
                description: vec!["記録を読んだ：".into(), text.into()],
                lore_id: Some(lore_id),
                ..EventOutcome::empty()
            }
        }
        (EventAction::TalkNpc, CellType::Npc) => {
            let hint = npc_hint(floor, rng_seed);
            EventOutcome {
                description: vec![hint],
                ..EventOutcome::empty()
            }
        }
        (EventAction::TradeNpc, CellType::Npc) => {
            let item = if floor >= 5 { ItemKind::MagicWater } else { ItemKind::Herb };
            EventOutcome {
                description: vec!["お礼にアイテムを受け取った。".into()],
                item: Some((item, 1)),
                ..EventOutcome::empty()
            }
        }
        (EventAction::DescendStairs, CellType::Stairs) => EventOutcome {
            description: vec![format!("B{}Fへ降りる…", floor + 1)],
            descend: true,
            ..EventOutcome::empty()
        },
        (EventAction::ReturnToTown, CellType::Entrance) => EventOutcome {
            description: vec!["町へ帰還する。".into()],
            return_to_town: true,
            ..EventOutcome::empty()
        },
        // ── Issue #90 resolutions ──
        (EventAction::ReviveAdventurer, CellType::FallenAdventurer) => {
            // 25% chance the body was a mimic — bites the player.
            let roll = rng_range(rng_seed, 100);
            if roll < 25 {
                let dmg = 8 + floor * 2;
                EventOutcome {
                    description: vec![
                        "冒険者の死体が突然動き出した！ ミミックだ！".into(),
                        format!("{}ダメージ！", dmg),
                    ],
                    hp_change: -(dmg as i32),
                    spawn_hostile: Some(EnemyKind::Goblin),
                    ..EventOutcome::empty()
                }
            } else {
                EventOutcome {
                    description: vec![
                        "冒険者は息を吹き返した。「礼を…」".into(),
                        "薬草と少しの金を分けてもらった。".into(),
                    ],
                    item: Some((ItemKind::Herb, 2)),
                    gold: 20 + floor as i32 * 10,
                    ..EventOutcome::empty()
                }
            }
        }
        (EventAction::LootAdventurer, CellType::FallenAdventurer) => {
            // High chance of an affixed item drop. Marked via item field so
            // the logic layer can apply the affix; actual affix selection
            // is delegated there because rng on State is owned by logic.rs.
            // For now, drop gold + a guaranteed potion as proxy.
            let gold = 25 + floor * 12 + rng_range(rng_seed, 20);
            EventOutcome {
                description: vec![
                    "冒険者の装備を回収した。".into(),
                    format!("{}Gと装備品の一部を手に入れた。", gold),
                ],
                gold: gold as i32,
                item: Some((ItemKind::StrengthPotion, 1)),
                ..EventOutcome::empty()
            }
        }
        (EventAction::PickFruit, CellType::FruitTree) => {
            let n = 1 + rng_range(rng_seed, 3);
            EventOutcome {
                description: vec![
                    "実をいくつか採った。".into(),
                    format!("リンゴx{}を手に入れた。", n),
                ],
                item: Some((ItemKind::Apple, n)),
                satiety_change: 80,
                ..EventOutcome::empty()
            }
        }
        (EventAction::ShakeTree, CellType::FruitTree) => {
            let big = 3 + rng_range(rng_seed, 3);
            // 35% chance to wake a monster.
            let bad = rng_range(rng_seed, 100) < 35;
            let mut desc = vec![format!("枝を揺すり、リンゴx{}が落ちてきた。", big)];
            let hostile = if bad {
                desc.push("…と同時に何かが樹から飛び降りた！".into());
                Some(EnemyKind::Bat)
            } else {
                None
            };
            EventOutcome {
                description: desc,
                item: Some((ItemKind::Apple, big)),
                satiety_change: 120,
                spawn_hostile: hostile,
                ..EventOutcome::empty()
            }
        }
        (EventAction::DrinkWell, CellType::Well) => {
            let roll = rng_range(rng_seed, 100);
            if roll < 35 {
                EventOutcome {
                    description: vec![
                        "澄んだ水だ。体力が満ちる。".into(),
                    ],
                    hp_change: 9999,
                    mp_change: 9999,
                    ..EventOutcome::empty()
                }
            } else if roll < 65 {
                let dmg = 5 + floor;
                EventOutcome {
                    description: vec![
                        "苦い…毒水だった！".into(),
                        format!("{}ダメージ！", dmg),
                    ],
                    hp_change: -(dmg as i32),
                    ..EventOutcome::empty()
                }
            } else if roll < 85 {
                EventOutcome {
                    description: vec!["何ともなかった。井戸水は冷たい。".into()],
                    ..EventOutcome::empty()
                }
            } else {
                // Lucky: minor blessing +faith
                EventOutcome {
                    description: vec![
                        "水面から光が立ち昇った。神の祝福だ！".into(),
                    ],
                    faith_change: 1,
                    hp_change: 9999,
                    ..EventOutcome::empty()
                }
            }
        }
        (EventAction::BottleWell, CellType::Well) => EventOutcome {
            description: vec!["井戸の水を瓶に汲んだ。薬草代わりになる。".into()],
            item: Some((ItemKind::Herb, 1)),
            ..EventOutcome::empty()
        },
        (EventAction::PeerWell, CellType::Well) => {
            let roll = rng_range(rng_seed, 100);
            if roll < 40 {
                let g = 10 + floor * 5 + rng_range(rng_seed, 20);
                EventOutcome {
                    description: vec![
                        "底に何か光るものを見つけた…".into(),
                        format!("{}Gを拾い上げた！", g),
                    ],
                    gold: g as i32,
                    ..EventOutcome::empty()
                }
            } else {
                EventOutcome {
                    description: vec!["底は暗くて何も見えない。".into()],
                    ..EventOutcome::empty()
                }
            }
        }
        (EventAction::PrayIdol, CellType::Idol) => {
            let roll = rng_range(rng_seed, 100);
            let mut out = EventOutcome {
                description: vec!["神像に祈った。心が落ち着く。".into()],
                faith_change: 1,
                ..EventOutcome::empty()
            };
            if roll < 30 {
                out.description.push("わずかに体力が回復した。".into());
                out.hp_change = 15;
            } else if roll < 50 {
                out.description.push("僅かな魔力が満ちる。".into());
                out.mp_change = 8;
            }
            out
        }
        (EventAction::OfferIdol, CellType::Idol) => EventOutcome {
            description: vec![
                "薬草を供えた。神像が淡く光った！".into(),
                "信仰が大きく深まり、HP/MPが回復した。".into(),
            ],
            faith_change: 3,
            hp_change: 9999,
            mp_change: 9999,
            require_consume: Some(ItemKind::Herb),
            ..EventOutcome::empty()
        },
        (EventAction::PeddlerBuyHerb, CellType::Peddler) => EventOutcome {
            description: vec!["「毎度どうも」".into(), "薬草を買った (-15G)".into()],
            gold: -15,
            item: Some((ItemKind::Herb, 1)),
            ..EventOutcome::empty()
        },
        (EventAction::PeddlerBuyMagicWater, CellType::Peddler) => EventOutcome {
            description: vec!["「お得ですよ」".into(), "魔法の水を買った (-40G)".into()],
            gold: -40,
            item: Some((ItemKind::MagicWater, 1)),
            ..EventOutcome::empty()
        },
        (EventAction::PeddlerBuyBread, CellType::Peddler) => EventOutcome {
            description: vec!["「焼きたてですよ」".into(), "パンを買った (-12G)".into()],
            gold: -12,
            item: Some((ItemKind::Bread, 1)),
            ..EventOutcome::empty()
        },
        (EventAction::TakeEgg, CellType::MonsterEgg) => {
            // 50% chance to gain a Slime/Rat as pet, else hostile hatch.
            let pool = [EnemyKind::Slime, EnemyKind::Rat, EnemyKind::Goblin, EnemyKind::Bat];
            let kind = pool[rng_range(rng_seed, pool.len() as u32) as usize];
            let lucky = rng_range(rng_seed, 100) < 50;
            if lucky {
                let _ = player_level;
                EventOutcome {
                    description: vec![
                        "卵が孵った！ 小さな魔物が懐いた！".into(),
                    ],
                    spawn_pet: Some(kind),
                    ..EventOutcome::empty()
                }
            } else {
                EventOutcome {
                    description: vec![
                        "卵が突然孵化し、敵対的な魔物が現れた！".into(),
                    ],
                    spawn_hostile: Some(kind),
                    ..EventOutcome::empty()
                }
            }
        }
        (EventAction::BreakEgg, CellType::MonsterEgg) => EventOutcome {
            description: vec![
                "卵を割って黄身をすすった。".into(),
                "満腹度が回復した。".into(),
            ],
            satiety_change: 250,
            ..EventOutcome::empty()
        },
        (EventAction::Ignore | EventAction::Continue, _) => EventOutcome {
            description: vec!["先に進むことにした。".into()],
            ..EventOutcome::empty()
        },
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
            description: vec![
                "宝箱を開けた！".into(),
                format!("薬草x{}を手に入れた！", count),
            ],
            item: Some((ItemKind::Herb, count)),
            ..EventOutcome::empty()
        }
    } else {
        let item = if floor >= 6 { ItemKind::StrengthPotion } else { ItemKind::MagicWater };
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
            "「ペットの餌を持っていけば、敵を仲間にできる」",
            "「宝箱には罠があることもある。慎重にな」",
            "「満腹度が0になると体力が削れる。食料を忘れるな」",
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

// ── Tests ─────────────────────────────────────────────────

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
            1, 1,
            &mut seed,
        );
        assert!(!outcome.description.is_empty());
    }

    #[test]
    fn resolve_spring_heals() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::DrinkSpring,
            CellType::Spring,
            1, 1,
            &mut seed,
        );
        assert_eq!(outcome.hp_change, 9999);
    }

    #[test]
    fn fallen_adventurer_event_has_three_choices() {
        let mut seed = 42u64;
        let event = generate_event(CellType::FallenAdventurer, 3, FloorTheme::Underground, &mut seed);
        assert!(event.is_some());
        assert_eq!(event.unwrap().choices.len(), 3);
    }

    #[test]
    fn fruit_tree_pick_grants_apples() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::PickFruit,
            CellType::FruitTree,
            2, 1,
            &mut seed,
        );
        assert!(matches!(outcome.item, Some((ItemKind::Apple, _))));
        assert!(outcome.satiety_change > 0);
    }

    #[test]
    fn peddler_buy_costs_gold() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::PeddlerBuyHerb,
            CellType::Peddler,
            1, 1,
            &mut seed,
        );
        assert_eq!(outcome.gold, -15);
        assert!(matches!(outcome.item, Some((ItemKind::Herb, 1))));
    }

    #[test]
    fn idol_offering_requires_consumable() {
        let mut seed = 42u64;
        let outcome = resolve_event(
            &EventAction::OfferIdol,
            CellType::Idol,
            1, 1,
            &mut seed,
        );
        assert_eq!(outcome.require_consume, Some(ItemKind::Herb));
        assert!(outcome.faith_change > 0);
    }

    #[test]
    fn egg_take_may_spawn_pet_or_hostile() {
        let mut seed = 1u64;
        let outcome = resolve_event(
            &EventAction::TakeEgg,
            CellType::MonsterEgg,
            1, 1,
            &mut seed,
        );
        assert!(outcome.spawn_pet.is_some() || outcome.spawn_hostile.is_some());
    }

    #[test]
    fn resolve_stairs_descends() {
        let mut seed = 42u64;
        let outcome = resolve_event(&EventAction::DescendStairs, CellType::Stairs, 3, 5, &mut seed);
        assert!(outcome.descend);
    }
}
