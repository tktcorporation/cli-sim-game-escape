//! Dungeon Dive rendering — single screen, scene-based.
//!
//! Layout: status bar + dungeon progress + scene content + log.
//! Overlays (inventory, shop, status) are full-screen replacements.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::ClickableList;

use super::actions::*;
use super::logic::{
    available_skills, battle_consumables, current_room_kind, dungeon_progress, town_choices,
};
use super::state::{
    enemy_info, item_info, shop_items, skill_info, BattlePhase, Overlay, RoomKind, RpgState, Scene,
};

pub fn render(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // If overlay is active, render it instead
    if let Some(overlay) = state.overlay {
        match overlay {
            Overlay::Inventory => render_inventory(state, f, area, click_state),
            Overlay::Status => render_status(state, f, area, click_state),
            Overlay::Shop => render_shop(state, f, area, click_state),
        }
        return;
    }

    match state.scene {
        Scene::Intro(_) => render_intro(state, f, area, click_state),
        Scene::Town => render_main(state, f, area, click_state),
        Scene::Dungeon | Scene::DungeonResult => render_main(state, f, area, click_state),
        Scene::Battle => render_main(state, f, area, click_state),
        Scene::GameClear => render_game_clear(state, f, area, click_state),
    }
}

// ── Helper: HP bar ──────────────────────────────────────────

fn hp_bar(current: u32, max: u32, width: usize) -> (String, Color) {
    let ratio = if max > 0 {
        current as f64 / max as f64
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(empty);
    let color = if ratio > 0.5 {
        Color::Green
    } else if ratio > 0.25 {
        Color::Yellow
    } else {
        Color::Red
    };
    (bar, color)
}

fn borders_for(area_width: u16) -> Borders {
    if is_narrow_layout(area_width) {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    }
}

// ── Intro ──────────────────────────────────────────────────

fn render_intro(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let step = match state.scene {
        Scene::Intro(s) => s,
        _ => 0,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(8)])
        .split(area);

    let mut lines = Vec::new();
    if step == 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " ……冒険者ギルドの扉を開けた。",
            Style::default().fg(Color::White),
        )));
    } else {
        for text in &state.scene_text {
            if text.is_empty() {
                lines.push(Line::from(""));
            } else {
                lines.push(Line::from(Span::styled(
                    format!(" {}", text),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    let title = " Dungeon Dive ";
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, Style::default().fg(Color::DarkGray)));
    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        chunks[0],
    );

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    match step {
        0 => push_choice(&mut cl, 0, "中に入る"),
        1 => push_choice(&mut cl, 0, "受け取って出発する"),
        _ => push_choice(&mut cl, 0, "続ける"),
    }

    let choice_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[1], &choice_block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines()).block(choice_block),
        chunks[1],
    );
}

// ── Main Screen (Town + Dungeon + Battle) ────────────────────

fn render_main(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let is_narrow = is_narrow_layout(area.width);
    let log_h: u16 = 4;

    // Dungeon progress bar shown when in dungeon
    let has_dungeon_bar = state.dungeon.is_some();
    let dbar_h: u16 = if has_dungeon_bar { 2 } else { 0 };

    let constraints = if has_dungeon_bar {
        vec![
            Constraint::Length(3),   // status bar
            Constraint::Length(dbar_h), // dungeon progress
            Constraint::Min(6),      // content
            Constraint::Length(log_h),
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(log_h),
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_idx = 0;

    // Status bar
    render_status_bar(state, f, chunks[chunk_idx], borders, is_narrow);
    chunk_idx += 1;

    // Dungeon progress
    if has_dungeon_bar {
        render_dungeon_bar(state, f, chunks[chunk_idx], borders);
        chunk_idx += 1;
    }

    // Main content
    render_scene_content(state, f, chunks[chunk_idx], borders, click_state);
    chunk_idx += 1;

    // Log
    render_log(state, f, chunks[chunk_idx], borders);
}

fn render_status_bar(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let hp_w = if is_narrow { 8 } else { 12 };
    let (hp_bar_str, hp_color) = hp_bar(state.hp, state.max_hp, hp_w);

    let mp_w = if is_narrow { 6 } else { 8 };
    let mp_ratio = if state.max_mp > 0 {
        state.mp as f64 / state.max_mp as f64
    } else {
        0.0
    };
    let mp_filled = (mp_ratio * mp_w as f64).round() as usize;
    let mp_empty = mp_w - mp_filled;
    let mp_bar_str = "\u{2588}".repeat(mp_filled) + &"\u{2591}".repeat(mp_empty);

    let line = Line::from(vec![
        Span::styled(
            format!(" Lv.{}", state.level),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" HP", Style::default().fg(Color::Gray)),
        Span::styled(hp_bar_str, Style::default().fg(hp_color)),
        Span::styled(
            format!("{}/{}", state.hp, state.max_hp),
            Style::default().fg(Color::White),
        ),
        Span::styled(" MP", Style::default().fg(Color::Gray)),
        Span::styled(mp_bar_str, Style::default().fg(Color::Blue)),
        Span::styled(
            format!("{}/{}", state.mp, state.max_mp),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!(" {}G", state.gold),
            Style::default().fg(Color::Yellow),
        ),
    ]);

    let title = if is_narrow {
        " Dungeon "
    } else {
        " Dungeon Dive "
    };
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(vec![line]).block(block), area);
}

fn render_dungeon_bar(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    if let Some((floor, room, total)) = dungeon_progress(state) {
        // Build room map: [#][#][?][?][?]
        let dungeon = state.dungeon.as_ref().unwrap();
        let mut map_spans = vec![Span::styled(" ", Style::default())];
        for (i, r) in dungeon.rooms.iter().enumerate() {
            let (symbol, color) = if i + 1 < room {
                // Visited
                let c = match r.kind {
                    RoomKind::Enemy => Color::Red,
                    RoomKind::Treasure => Color::Yellow,
                    RoomKind::Trap => Color::Magenta,
                    RoomKind::Spring => Color::Cyan,
                    RoomKind::Empty => Color::DarkGray,
                    RoomKind::Stairs => Color::Green,
                };
                ("\u{25a0}", c) // filled square
            } else if i + 1 == room {
                ("\u{25c6}", Color::White) // current = diamond
            } else {
                ("\u{25a1}", Color::DarkGray) // unknown = empty square
            };
            map_spans.push(Span::styled(symbol, Style::default().fg(color)));
        }

        let line = Line::from(vec![
            Span::styled(
                format!(" B{}F ", floor),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}/{}", room, total),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(" "),
        ]);
        // Combine into one line with map
        let mut all_spans = line.spans;
        all_spans.extend(map_spans);

        let block = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            Paragraph::new(vec![Line::from(all_spans)]).block(block),
            area,
        );
    }
}

fn render_scene_content(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.scene {
        Scene::Battle => render_battle_content(state, f, area, borders, click_state),
        Scene::Dungeon => render_dungeon_content(state, f, area, borders, click_state),
        Scene::DungeonResult => render_dungeon_result(state, f, area, borders, click_state),
        Scene::Town => render_town_content(state, f, area, borders, click_state),
        _ => {}
    }
}

fn render_town_content(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    for text in &state.scene_text {
        if text.is_empty() {
            cl.push(Line::from(""));
        } else {
            cl.push(Line::from(Span::styled(
                format!(" {}", text),
                Style::default().fg(Color::White),
            )));
        }
    }

    if state.max_floor_reached > 0 {
        cl.push(Line::from(Span::styled(
            format!(
                " 最深到達: B{}F  クリア: {}回",
                state.max_floor_reached, state.total_clears
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    let choices = town_choices(state);
    for (i, choice) in choices.iter().enumerate() {
        push_choice(&mut cl, i, &choice.label);
    }

    // Shortcut hints
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " [I]持ち物  [S]ステータス",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines())
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_dungeon_content(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    // Show what's ahead
    let room_kind = current_room_kind(state);
    let room_hint = match room_kind {
        Some(RoomKind::Stairs) => "階段の気配がする…",
        Some(RoomKind::Enemy) => "殺気を感じる…",
        Some(RoomKind::Treasure) => "何かが光っている…",
        Some(RoomKind::Spring) => "水の音が聞こえる…",
        Some(RoomKind::Trap) => "嫌な予感がする…",
        Some(RoomKind::Empty) | None => "暗い通路が続いている…",
    };

    cl.push(Line::from(Span::styled(
        format!(" {}", room_hint),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    push_choice(&mut cl, 0, "進む");
    push_choice_dim(&mut cl, 1, "引き返す (町に戻る)");

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " [I]持ち物  [S]ステータス",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines())
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_dungeon_result(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    if let Some(result) = &state.room_result {
        for line in &result.description {
            if line.is_empty() {
                cl.push(Line::from(""));
            } else {
                cl.push(Line::from(Span::styled(
                    format!(" {}", line),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    // Check if this is a stairs room
    let is_stairs = state
        .dungeon
        .as_ref()
        .and_then(|d| d.rooms.get(d.current_room))
        .map(|r| r.kind == RoomKind::Stairs)
        .unwrap_or(false);

    let is_dead = state.hp == 0;

    if is_dead {
        push_choice(&mut cl, 0, "町に戻る");
    } else if is_stairs {
        push_choice(&mut cl, 0, "次の階へ進む");
        push_choice_dim(&mut cl, 1, "引き返す (町に戻る)");
    } else {
        push_choice(&mut cl, 0, "先に進む");
        push_choice_dim(&mut cl, 1, "引き返す (町に戻る)");
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " [I]持ち物  [S]ステータス",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines())
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_battle_content(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let battle = match &state.battle {
        Some(b) => b,
        None => return,
    };
    let is_narrow = is_narrow_layout(area.width);
    let einfo = enemy_info(battle.enemy.kind);

    let mut cl = ClickableList::new();

    let boss_str = if battle.is_boss {
        " \u{2605}BOSS\u{2605}"
    } else {
        ""
    };
    cl.push(Line::from(vec![Span::styled(
        format!(" \u{300a}\u{6226}\u{95d8}\u{300b} {}{}", einfo.name, boss_str),
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD),
    )]));

    let ehp_w = if is_narrow { 10 } else { 16 };
    let (ehp_bar, ehp_color) = hp_bar(battle.enemy.hp, battle.enemy.max_hp, ehp_w);
    cl.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::Gray)),
        Span::styled(ehp_bar, Style::default().fg(ehp_color)),
        Span::styled(
            format!(" {}/{}", battle.enemy.hp, battle.enemy.max_hp),
            Style::default().fg(Color::White),
        ),
    ]));

    cl.push(Line::from(""));

    let max_log = 3;
    let start = battle.log.len().saturating_sub(max_log);
    for msg in &battle.log[start..] {
        cl.push(Line::from(Span::styled(
            format!(" {}", msg),
            Style::default().fg(Color::White),
        )));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    match battle.phase {
        BattlePhase::SelectAction => {
            push_choice(&mut cl, 0, "攻撃する");
            if !available_skills(state.level).is_empty() {
                push_choice(&mut cl, 1, "スキル \u{25b8}");
            }
            if !battle_consumables(state).is_empty() {
                push_choice(&mut cl, 2, "アイテム \u{25b8}");
            }
            if !battle.is_boss {
                push_choice_dim(&mut cl, 3, "逃げる");
            }
        }
        BattlePhase::SelectSkill => {
            let skills = available_skills(state.level);
            for (i, &skill) in skills.iter().enumerate() {
                let sinfo = skill_info(skill);
                let can_use = state.mp >= sinfo.mp_cost;
                let label = format!("{} (MP:{})", sinfo.name, sinfo.mp_cost);
                if can_use {
                    cl.push_clickable(
                        Line::from(vec![
                            Span::styled(
                                format!(" [{}] ", i + 1),
                                Style::default()
                                    .fg(Color::Blue)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(label, Style::default().fg(Color::White)),
                        ]),
                        SKILL_BASE + i as u16,
                    );
                } else {
                    cl.push(Line::from(Span::styled(
                        format!(" [{}] {}", i + 1, label),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " [0] 戻る",
                    Style::default().fg(Color::Yellow),
                )),
                BATTLE_BACK,
            );
        }
        BattlePhase::SelectItem => {
            let items = battle_consumables(state);
            for (i, (_, kind, count)) in items.iter().enumerate() {
                let iinfo = item_info(*kind);
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" [{}] ", i + 1),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} x{}", iinfo.name, count),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    BATTLE_ITEM_BASE + i as u16,
                );
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " [0] 戻る",
                    Style::default().fg(Color::Yellow),
                )),
                BATTLE_BACK,
            );
        }
        BattlePhase::Victory => {
            cl.push(Line::from(Span::styled(
                format!(" \u{2605} {}を倒した！", einfo.name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                format!("   EXP+{} {}G", einfo.exp, einfo.gold),
                Style::default().fg(Color::Yellow),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける");
        }
        BattlePhase::Defeat => {
            cl.push(Line::from(Span::styled(
                " \u{2716} 力尽きた...",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                "   町に戻される (所持金半減)",
                Style::default().fg(Color::Red),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける");
        }
        BattlePhase::Fled => {
            cl.push(Line::from(Span::styled(
                " \u{2192} うまく逃げ切った！",
                Style::default().fg(Color::Green),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける");
        }
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines())
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_log(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(max_lines);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|msg| {
            Line::from(Span::styled(
                format!(" > {}", msg),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

// ── Choice Helpers ──────────────────────────────────────────

fn push_choice(cl: &mut ClickableList, index: usize, label: &str) {
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!(" [{}] ", index + 1),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(label.to_string(), Style::default().fg(Color::White)),
        ]),
        CHOICE_BASE + index as u16,
    );
}

fn push_choice_dim(cl: &mut ClickableList, index: usize, label: &str) {
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!(" [{}] ", index + 1),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(label.to_string(), Style::default().fg(Color::DarkGray)),
        ]),
        CHOICE_BASE + index as u16,
    );
}

// ── Overlays ────────────────────────────────────────────────

fn render_inventory(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    let weapon_name = state
        .weapon
        .map(|w| item_info(w).name)
        .unwrap_or("なし");
    let armor_name = state
        .armor
        .map(|a| item_info(a).name)
        .unwrap_or("なし");
    cl.push(Line::from(Span::styled(
        format!(" 武器: {}  防具: {}", weapon_name, armor_name),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    if state.inventory.is_empty() {
        cl.push(Line::from(Span::styled(
            " アイテムなし",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, item) in state.inventory.iter().enumerate() {
            let iinfo = item_info(item.kind);
            if i < 9 {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" [{}] ", i + 1),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            format!("{} x{}", iinfo.name, item.count),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(" - {}", iinfo.description),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                    INV_USE_BASE + i as u16,
                );
            } else {
                cl.push(Line::from(Span::styled(
                    format!("     {} x{} - {}", iinfo.name, item.count, iinfo.description),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            " [0] 閉じる",
            Style::default().fg(Color::Yellow),
        )),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            format!(" 持ち物 ({}G) ", state.gold),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}

fn render_status(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    let weapon_name = state
        .weapon
        .map(|w| item_info(w).name)
        .unwrap_or("なし");
    let armor_name = state
        .armor
        .map(|a| item_info(a).name)
        .unwrap_or("なし");

    cl.push(Line::from(vec![
        Span::styled(
            format!(" Lv.{}", state.level),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  EXP:{}", state.exp),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    cl.push(Line::from(vec![
        Span::styled(
            format!(" HP:{}/{}", state.hp, state.max_hp),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!("  MP:{}/{}", state.mp, state.max_mp),
            Style::default().fg(Color::Blue),
        ),
    ]));
    cl.push(Line::from(vec![
        Span::styled(
            format!(" ATK:{}", state.total_atk()),
            Style::default().fg(Color::Red),
        ),
        Span::styled(
            format!("  DEF:{}", state.total_def()),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("  MAG:{}", state.mag),
            Style::default().fg(Color::Magenta),
        ),
    ]));
    cl.push(Line::from(""));
    cl.push(Line::from(vec![
        Span::styled(" 武器: ", Style::default().fg(Color::Gray)),
        Span::styled(weapon_name, Style::default().fg(Color::White)),
        Span::styled("  防具: ", Style::default().fg(Color::Gray)),
        Span::styled(armor_name, Style::default().fg(Color::White)),
    ]));
    cl.push(Line::from(""));

    // Dungeon progress
    cl.push(Line::from(Span::styled(
        format!(
            " 最深到達: B{}F  クリア: {}回",
            state.max_floor_reached, state.total_clears
        ),
        Style::default().fg(Color::Yellow),
    )));
    cl.push(Line::from(""));

    let skills = available_skills(state.level);
    if !skills.is_empty() {
        cl.push(Line::from(Span::styled(
            " 【スキル】",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )));
        for &skill in &skills {
            let sinfo = skill_info(skill);
            cl.push(Line::from(Span::styled(
                format!(
                    "  {} (MP:{}) - {}",
                    sinfo.name, sinfo.mp_cost, sinfo.description
                ),
                Style::default().fg(Color::White),
            )));
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            " [0] 閉じる",
            Style::default().fg(Color::Yellow),
        )),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " ステータス ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}

fn render_shop(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(Span::styled(
        format!(" 所持金: {}G", state.gold),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let shop = shop_items(state.max_floor_reached);
    for (i, &(kind, _)) in shop.iter().enumerate() {
        let iinfo = item_info(kind);
        let affordable = state.gold >= iinfo.buy_price;
        let color = if affordable {
            Color::White
        } else {
            Color::DarkGray
        };
        if i < 9 {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" [{}] ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{} {}G", iinfo.name, iinfo.buy_price),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        format!(" - {}", iinfo.description),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                SHOP_BUY_BASE + i as u16,
            );
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            " [0] 閉じる",
            Style::default().fg(Color::Yellow),
        )),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " ショップ ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}

// ── Game Clear ──────────────────────────────────────────────

fn render_game_clear(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2605}\u{2605}\u{2605} DUNGEON CLEAR \u{2605}\u{2605}\u{2605}",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 魔王を倒し、ダンジョンを制覇した！",
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(
            " レベル: {}  クリア: {}回  所持金: {}G",
            state.level, state.total_clears, state.gold
        ),
        Style::default().fg(Color::Yellow),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 冒険をありがとう！",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    push_choice(&mut cl, 0, "メニューに戻る");

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " \u{2605} DUNGEON CLEAR \u{2605} ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}
