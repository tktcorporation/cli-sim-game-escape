//! RPG Quest rendering — single screen, scene-based.
//!
//! Layout: status bar (optional) + objective (optional) + scene text + choices + log.
//! Overlays (inventory, shop, quest log, status) are full-screen replacements.

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
use super::logic::{available_skills, battle_consumables, visible_quests, world_choices};
use super::state::{
    enemy_info, item_info, quest_info, shop_inventory, skill_info,
    BattlePhase, Overlay, QuestKind, QuestStatus, RpgState, Scene,
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
            Overlay::QuestLog => render_quest_log(state, f, area, click_state),
            Overlay::Status => render_status(state, f, area, click_state),
            Overlay::Shop => render_shop(state, f, area, click_state),
        }
        return;
    }

    // Main scene rendering
    match state.scene {
        Scene::Prologue(_) => render_prologue(state, f, area, click_state),
        Scene::World => render_main(state, f, area, click_state),
        Scene::Battle => render_main(state, f, area, click_state),
        Scene::GameClear => render_game_clear(state, f, area, click_state),
    }
}

// ── Helper: HP bar ──────────────────────────────────────────

fn hp_bar(current: u32, max: u32, width: usize) -> (String, Color) {
    let ratio = if max > 0 { current as f64 / max as f64 } else { 0.0 };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(empty);
    let color = if ratio > 0.5 { Color::Green } else if ratio > 0.25 { Color::Yellow } else { Color::Red };
    (bar, color)
}

fn borders_for(area_width: u16) -> Borders {
    if is_narrow_layout(area_width) { Borders::TOP | Borders::BOTTOM } else { Borders::ALL }
}

// ── Prologue ────────────────────────────────────────────────

fn render_prologue(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let step = match state.scene { Scene::Prologue(s) => s, _ => 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(8)])
        .split(area);

    // Scene text
    let mut lines = Vec::new();
    if step == 0 {
        // Initial screen — minimal, like A Dark Room
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " ……目を覚ますと、見知らぬ村にいた。",
            Style::default().fg(Color::White),
        )));
    } else {
        for text in &state.scene_text.lines {
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

    let title = " RPG Quest ";
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, Style::default().fg(Color::DarkGray)));
    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        chunks[0],
    );

    // Choices
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    match step {
        0 => {
            push_choice(&mut cl, 0, "辺りを見回す", false);
        }
        1 => {
            push_choice(&mut cl, 0, "「話を聞かせてください」", false);
            push_choice(&mut cl, 1, "「…ここはどこですか？」", false);
        }
        2 => {
            push_choice(&mut cl, 0, "冒険に出発する", false);
        }
        _ => {
            push_choice(&mut cl, 0, "続ける", false);
        }
    }

    // Log (only for step 2+)
    if step >= 2 {
        cl.push(Line::from(""));
        let max_log = 2;
        let start = state.log.len().saturating_sub(max_log);
        for msg in &state.log[start..] {
            cl.push(Line::from(Span::styled(
                format!(" > {}", msg), Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let choice_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[1], &choice_block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(choice_block), chunks[1]);
}

// ── Main Screen (World + Battle) ────────────────────────────

fn render_main(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let is_narrow = is_narrow_layout(area.width);

    // Calculate layout constraints
    let has_status = state.unlocks.status_bar;
    let has_objective = state.unlocks.quest_objective && state.current_objective().is_some();

    let status_h = if has_status { 3 } else { 0 };
    let obj_h: u16 = if has_objective { 2 } else { 0 };
    let log_h: u16 = 4;

    let constraints = if has_status && has_objective {
        vec![
            Constraint::Length(status_h),
            Constraint::Length(obj_h),
            Constraint::Min(6),
            Constraint::Length(log_h),
        ]
    } else if has_status {
        vec![
            Constraint::Length(status_h),
            Constraint::Min(6),
            Constraint::Length(log_h),
        ]
    } else {
        vec![Constraint::Min(6), Constraint::Length(log_h)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_idx = 0;

    // Status bar
    if has_status {
        render_status_bar(state, f, chunks[chunk_idx], borders, is_narrow);
        chunk_idx += 1;
    }

    // Objective
    if has_objective {
        render_objective(state, f, chunks[chunk_idx], borders);
        chunk_idx += 1;
    }

    // Main content area: scene text + choices
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
    let mp_ratio = if state.max_mp > 0 { state.mp as f64 / state.max_mp as f64 } else { 0.0 };
    let mp_filled = (mp_ratio * mp_w as f64).round() as usize;
    let mp_empty = mp_w - mp_filled;
    let mp_bar_str = "\u{2588}".repeat(mp_filled) + &"\u{2591}".repeat(mp_empty);

    let line = Line::from(vec![
        Span::styled(
            format!(" Lv.{}", state.level),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" HP", Style::default().fg(Color::Gray)),
        Span::styled(hp_bar_str, Style::default().fg(hp_color)),
        Span::styled(format!("{}/{}", state.hp, state.max_hp), Style::default().fg(Color::White)),
        Span::styled(" MP", Style::default().fg(Color::Gray)),
        Span::styled(mp_bar_str, Style::default().fg(Color::Blue)),
        Span::styled(format!("{}/{}", state.mp, state.max_mp), Style::default().fg(Color::White)),
        Span::styled(format!(" {}G", state.gold), Style::default().fg(Color::Yellow)),
    ]);

    let title = if is_narrow { " RPG " } else { " RPG Quest " };
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    f.render_widget(Paragraph::new(vec![line]).block(block), area);
}

fn render_objective(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    let obj = state.current_objective().unwrap_or_default();
    let line = Line::from(vec![
        Span::styled(" \u{25ce} ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(obj, Style::default().fg(Color::Yellow)),
    ]);
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(vec![line]).block(block), area);
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
        _ => render_world_content(state, f, area, borders, click_state),
    }
}

fn render_world_content(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    // Scene text
    for text in &state.scene_text.lines {
        if text.is_empty() {
            cl.push(Line::from(""));
        } else {
            cl.push(Line::from(Span::styled(
                format!(" {}", text), Style::default().fg(Color::White),
            )));
        }
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    // Choices
    let choices = world_choices(state);
    for (i, choice) in choices.iter().enumerate() {
        push_choice(&mut cl, i, &choice.label, choice.quest_related);
    }

    // Shortcut hints
    let mut hints = Vec::new();
    if state.unlocks.inventory_shortcut { hints.push("[I]持ち物"); }
    if state.unlocks.status_shortcut { hints.push("[S]ステータス"); }
    if state.unlocks.quest_log_shortcut { hints.push("[Q]クエスト"); }
    if !hints.is_empty() {
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            format!(" {}", hints.join("  ")),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let block = Block::default().borders(borders).border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines()).block(block).wrap(Wrap { trim: false }),
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
    let battle = match &state.battle { Some(b) => b, None => return };
    let is_narrow = is_narrow_layout(area.width);
    let einfo = enemy_info(battle.enemy.kind);

    let mut cl = ClickableList::new();

    // Enemy info
    let boss_str = if battle.is_boss { " \u{2605}BOSS\u{2605}" } else { "" };
    cl.push(Line::from(vec![
        Span::styled(
            format!(" \u{300a}\u{6226}\u{95d8}\u{300b} {}{}", einfo.name, boss_str),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    ]));

    let ehp_w = if is_narrow { 10 } else { 16 };
    let (ehp_bar, ehp_color) = hp_bar(battle.enemy.hp, battle.enemy.max_hp, ehp_w);
    cl.push(Line::from(vec![
        Span::styled(" HP ", Style::default().fg(Color::Gray)),
        Span::styled(ehp_bar, Style::default().fg(ehp_color)),
        Span::styled(format!(" {}/{}", battle.enemy.hp, battle.enemy.max_hp), Style::default().fg(Color::White)),
    ]));

    cl.push(Line::from(""));

    // Battle log (last 3 lines)
    let max_log = 3;
    let start = battle.log.len().saturating_sub(max_log);
    for msg in &battle.log[start..] {
        cl.push(Line::from(Span::styled(
            format!(" {}", msg), Style::default().fg(Color::White),
        )));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    // Actions based on phase
    match battle.phase {
        BattlePhase::SelectAction => {
            push_choice(&mut cl, 0, "攻撃する", false);
            if !available_skills(state.level).is_empty() {
                push_choice(&mut cl, 1, "スキル \u{25b8}", false);
            }
            if !battle_consumables(state).is_empty() {
                push_choice(&mut cl, 2, "アイテム \u{25b8}", false);
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
                            Span::styled(format!(" [{}] ", i + 1), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                            Span::styled(label, Style::default().fg(Color::White)),
                        ]),
                        SKILL_BASE + i as u16,
                    );
                } else {
                    cl.push(Line::from(Span::styled(
                        format!(" [{}] {}", i + 1, label), Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(" [0] 戻る", Style::default().fg(Color::Yellow))),
                BATTLE_BACK,
            );
        }
        BattlePhase::SelectItem => {
            let items = battle_consumables(state);
            for (i, (_, kind, count)) in items.iter().enumerate() {
                let iinfo = item_info(*kind);
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(format!(" [{}] ", i + 1), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{} x{}", iinfo.name, count), Style::default().fg(Color::White)),
                    ]),
                    BATTLE_ITEM_BASE + i as u16,
                );
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(" [0] 戻る", Style::default().fg(Color::Yellow))),
                BATTLE_BACK,
            );
        }
        BattlePhase::Victory => {
            cl.push(Line::from(Span::styled(
                format!(" \u{2605} {}を倒した！", einfo.name),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                format!("   EXP+{} {}G", einfo.exp, einfo.gold),
                Style::default().fg(Color::Yellow),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける", false);
        }
        BattlePhase::Defeat => {
            cl.push(Line::from(Span::styled(
                " \u{2716} 力尽きた...",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                "   村で目を覚ます (所持金半減)",
                Style::default().fg(Color::Red),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける", false);
        }
        BattlePhase::Fled => {
            cl.push(Line::from(Span::styled(
                " \u{2192} うまく逃げ切った！",
                Style::default().fg(Color::Green),
            )));
            cl.push(Line::from(""));
            push_choice(&mut cl, 0, "続ける", false);
        }
    }

    let block = Block::default().borders(borders).border_style(Style::default().fg(Color::Red));
    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(
        Paragraph::new(cl.into_lines()).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_log(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(max_lines);
    let lines: Vec<Line> = state.log[start..].iter()
        .map(|msg| Line::from(Span::styled(format!(" > {}", msg), Style::default().fg(Color::DarkGray))))
        .collect();
    let block = Block::default().borders(borders).border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

// ── Choice Helper ───────────────────────────────────────────

fn push_choice(cl: &mut ClickableList, index: usize, label: &str, quest_related: bool) {
    let marker = if quest_related { " \u{2605}" } else { "" };
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!(" [{}] ", index + 1),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}{}", label, marker),
                Style::default().fg(Color::White),
            ),
        ]),
        CHOICE_BASE + index as u16,
    );
}

fn push_choice_dim(cl: &mut ClickableList, index: usize, label: &str) {
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!(" [{}] ", index + 1),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
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

    // Equipment
    let weapon_name = state.weapon.map(|w| item_info(w).name).unwrap_or("なし");
    let armor_name = state.armor.map(|a| item_info(a).name).unwrap_or("なし");
    cl.push(Line::from(Span::styled(
        format!(" 武器: {}  防具: {}", weapon_name, armor_name),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    // Items
    if state.inventory.is_empty() {
        cl.push(Line::from(Span::styled(" アイテムなし", Style::default().fg(Color::DarkGray))));
    } else {
        for (i, item) in state.inventory.iter().enumerate() {
            let iinfo = item_info(item.kind);
            if i < 9 {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(format!(" [{}] ", i + 1), Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{} x{}", iinfo.name, item.count), Style::default().fg(Color::White)),
                        Span::styled(format!(" - {}", iinfo.description), Style::default().fg(Color::DarkGray)),
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
        Line::from(Span::styled(" [0] 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            format!(" 持ち物 ({}G) ", state.gold),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}

fn render_quest_log(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    let quests = visible_quests(state);
    if quests.is_empty() {
        cl.push(Line::from(Span::styled(" クエストなし", Style::default().fg(Color::DarkGray))));
    } else {
        for (quest_id, status) in &quests {
            let info = quest_info(*quest_id);
            let (icon, color) = match status {
                QuestStatus::Completed => ("\u{2713}", Color::DarkGray),
                QuestStatus::ReadyToComplete => ("\u{2605}", Color::Yellow),
                QuestStatus::Active => ("\u{25cf}", Color::Cyan),
                QuestStatus::Available => ("\u{25cb}", Color::White),
            };
            let kind_str = if info.kind == QuestKind::Main { "[M]" } else { "[S]" };
            cl.push(Line::from(vec![
                Span::styled(format!(" {} {} ", icon, kind_str), Style::default().fg(color)),
                Span::styled(info.name, Style::default().fg(color)),
            ]));
            if *status == QuestStatus::Active || *status == QuestStatus::ReadyToComplete {
                cl.push(Line::from(Span::styled(
                    format!("     {}", info.description), Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" [0] 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" クエスト ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));

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

    let weapon_name = state.weapon.map(|w| item_info(w).name).unwrap_or("なし");
    let armor_name = state.armor.map(|a| item_info(a).name).unwrap_or("なし");

    cl.push(Line::from(vec![
        Span::styled(format!(" Lv.{}", state.level), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  EXP:{}", state.exp), Style::default().fg(Color::DarkGray)),
    ]));
    cl.push(Line::from(vec![
        Span::styled(format!(" HP:{}/{}", state.hp, state.max_hp), Style::default().fg(Color::Green)),
        Span::styled(format!("  MP:{}/{}", state.mp, state.max_mp), Style::default().fg(Color::Blue)),
    ]));
    cl.push(Line::from(vec![
        Span::styled(format!(" ATK:{}", state.total_atk()), Style::default().fg(Color::Red)),
        Span::styled(format!("  DEF:{}", state.total_def()), Style::default().fg(Color::Cyan)),
        Span::styled(format!("  MAG:{}", state.mag), Style::default().fg(Color::Magenta)),
    ]));
    cl.push(Line::from(""));
    cl.push(Line::from(vec![
        Span::styled(" 武器: ", Style::default().fg(Color::Gray)),
        Span::styled(weapon_name, Style::default().fg(Color::White)),
        Span::styled("  防具: ", Style::default().fg(Color::Gray)),
        Span::styled(armor_name, Style::default().fg(Color::White)),
    ]));
    cl.push(Line::from(""));

    let skills = available_skills(state.level);
    if !skills.is_empty() {
        cl.push(Line::from(Span::styled(
            " 【スキル】", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        )));
        for &skill in &skills {
            let sinfo = skill_info(skill);
            cl.push(Line::from(Span::styled(
                format!("  {} (MP:{}) - {}", sinfo.name, sinfo.mp_cost, sinfo.description),
                Style::default().fg(Color::White),
            )));
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" [0] 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" ステータス ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));

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
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let shop = shop_inventory(state.location);
    for (i, &(kind, _)) in shop.iter().enumerate() {
        let iinfo = item_info(kind);
        let affordable = state.gold >= iinfo.buy_price;
        let color = if affordable { Color::White } else { Color::DarkGray };
        if i < 9 {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(format!(" [{}] ", i + 1), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{} {}G", iinfo.name, iinfo.buy_price), Style::default().fg(color)),
                    Span::styled(format!(" - {}", iinfo.description), Style::default().fg(Color::DarkGray)),
                ]),
                SHOP_BUY_BASE + i as u16,
            );
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" [0] 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(" ショップ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));

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
    let completed = state.quests.iter().filter(|q| q.status == QuestStatus::Completed).count();
    let total = state.quests.len();

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2605}\u{2605}\u{2605} GAME CLEAR \u{2605}\u{2605}\u{2605}",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 魔王を倒し、世界に平和が戻った！",
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" レベル: {}  クエスト: {}/{}  所持金: {}G", state.level, completed, total, state.gold),
        Style::default().fg(Color::Yellow),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 冒険をありがとう！",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    push_choice(&mut cl, 0, "メニューに戻る", false);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(" \u{2605} GAME CLEAR \u{2605} ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);
    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}
