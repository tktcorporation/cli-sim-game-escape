//! RPG Quest rendering (read-only from state).

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
use super::logic::{available_skills, battle_consumables, visible_quests};
use super::state::{
    enemy_info, item_info, location_info, quest_info, skill_info, shop_inventory,
    BattleAction, ItemCategory, QuestKind, QuestStatus, RpgState, Screen,
};

pub fn render(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.screen {
        Screen::World => render_world(state, f, area, click_state),
        Screen::Battle => render_battle(state, f, area, click_state),
        Screen::Inventory => render_inventory(state, f, area, click_state),
        Screen::QuestLog => render_quest_log(state, f, area, click_state),
        Screen::Shop => render_shop(state, f, area, click_state),
        Screen::Status => render_status(state, f, area, click_state),
        Screen::Dialogue => render_dialogue(state, f, area, click_state),
        Screen::GameClear => render_game_clear(state, f, area, click_state),
    }
}

// ── Helper: HP bar ───────────────────────────────────────────

fn hp_bar(current: u32, max: u32, width: usize) -> (String, Color) {
    let ratio = if max > 0 {
        current as f64 / max as f64
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let color = if ratio > 0.5 {
        Color::Green
    } else if ratio > 0.25 {
        Color::Yellow
    } else {
        Color::Red
    };
    (bar, color)
}

// ── World Screen ─────────────────────────────────────────────

fn render_world(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Header (player stats)
            Constraint::Length(3),  // Location info
            Constraint::Length(if is_narrow { 14 } else { 12 }), // Actions
            Constraint::Min(4),    // Log
        ])
        .split(area);

    render_world_header(state, f, chunks[0], borders, is_narrow);
    render_location(state, f, chunks[1], borders);
    render_world_actions(state, f, chunks[2], borders, is_narrow, click_state);
    render_log(state, f, chunks[3], borders);
}

fn render_world_header(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let (hp_bar_str, hp_color) = hp_bar(state.hp, state.max_hp, if is_narrow { 10 } else { 15 });
    let mp_bar_width: usize = if is_narrow { 8 } else { 12 };
    let mp_ratio = if state.max_mp > 0 {
        state.mp as f64 / state.max_mp as f64
    } else {
        0.0
    };
    let mp_filled = (mp_ratio * mp_bar_width as f64).round() as usize;
    let mp_empty = mp_bar_width.saturating_sub(mp_filled);
    let mp_bar_str = "█".repeat(mp_filled) + &"░".repeat(mp_empty);

    let title = if is_narrow {
        " RPG Quest "
    } else {
        " RPG Quest - 冒険の旅 "
    };

    let lines = vec![
        Line::from(vec![
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
            Span::styled(
                format!("  {}G", state.gold),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled(" HP ", Style::default().fg(Color::Gray)),
            Span::styled(hp_bar_str, Style::default().fg(hp_color)),
            Span::styled(
                format!(" {}/{}", state.hp, state.max_hp),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(" MP ", Style::default().fg(Color::Gray)),
            Span::styled(mp_bar_str, Style::default().fg(Color::Blue)),
            Span::styled(
                format!(" {}/{}", state.mp, state.max_mp),
                Style::default().fg(Color::White),
            ),
        ]),
    ];

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_location(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    let loc = location_info(state.location);
    let lines = vec![Line::from(vec![
        Span::styled(
            format!(" {} ", loc.name),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("- {}", loc.description),
            Style::default().fg(Color::DarkGray),
        ),
    ])];

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 現在地 ");
    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn render_world_actions(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    _is_narrow: bool,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    let loc = location_info(state.location);

    // Explore
    if loc.has_encounters || state.location == LocationId::HiddenLake {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    " ▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("探索する [1]", Style::default().fg(Color::White)),
            ]),
            EXPLORE,
        );
    }

    // Talk to NPC
    if loc.has_npc {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    " ▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("話しかける [2]", Style::default().fg(Color::White)),
            ]),
            TALK_NPC,
        );
    }

    // Shop
    if loc.has_shop {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    " ▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("ショップ [3]", Style::default().fg(Color::White)),
            ]),
            GO_SHOP,
        );
    }

    // Rest
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " ▶ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("休む [4]", Style::default().fg(Color::White)),
        ]),
        REST,
    );

    cl.push(Line::from(""));

    // Navigation: Inventory, QuestLog, Status
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " ▶ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("持ち物 [7]", Style::default().fg(Color::White)),
        ]),
        GO_INVENTORY,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " ▶ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("クエスト [8]", Style::default().fg(Color::White)),
        ]),
        GO_QUEST_LOG,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " ▶ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ステータス [9]", Style::default().fg(Color::White)),
        ]),
        GO_STATUS,
    );

    cl.push(Line::from(""));

    // Travel destinations
    for (i, &dest) in loc.connections.iter().enumerate() {
        let dest_info = location_info(dest);
        let key = match i {
            0 => "A",
            1 => "B",
            2 => "C",
            3 => "D",
            _ => "E",
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    " → ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} [{}]", dest_info.name, key),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            TRAVEL_BASE + i as u16,
        );
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" アクション ");

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
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
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ログ ");
    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

// ── Battle Screen ────────────────────────────────────────────

fn render_battle(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let battle = match &state.battle {
        Some(b) => b,
        None => return,
    };

    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Enemy info
            Constraint::Length(4),  // Player info
            Constraint::Length(8),  // Battle log
            Constraint::Min(6),    // Actions
        ])
        .split(area);

    // Enemy info
    let einfo = enemy_info(battle.enemy.kind);
    let (ehp_bar, ehp_color) = hp_bar(
        battle.enemy.hp,
        battle.enemy.max_hp,
        if is_narrow { 12 } else { 20 },
    );

    let enemy_lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", einfo.name),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            if battle.is_boss {
                Span::styled("★BOSS★", Style::default().fg(Color::Red))
            } else {
                Span::raw("")
            },
        ]),
        Line::from(vec![
            Span::styled(" HP ", Style::default().fg(Color::Gray)),
            Span::styled(ehp_bar, Style::default().fg(ehp_color)),
            Span::styled(
                format!(" {}/{}", battle.enemy.hp, battle.enemy.max_hp),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" ATK:{} DEF:{}", einfo.atk, einfo.def),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    let enemy_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red))
        .title(" 敵 ");
    f.render_widget(
        Paragraph::new(enemy_lines).block(enemy_block),
        chunks[0],
    );

    // Player info
    let (php_bar, php_color) = hp_bar(state.hp, state.max_hp, if is_narrow { 10 } else { 15 });
    let player_lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" Lv.{} ", state.level),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("HP ", Style::default().fg(Color::Gray)),
            Span::styled(php_bar, Style::default().fg(php_color)),
            Span::styled(
                format!(" {}/{}", state.hp, state.max_hp),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" ATK:{} DEF:{}", state.total_atk() + battle.player_atk_boost, state.total_def() + battle.player_def_boost),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  MP:{}/{}", state.mp, state.max_mp),
                Style::default().fg(Color::Blue),
            ),
        ]),
    ];

    let player_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" 冒険者 ");
    f.render_widget(
        Paragraph::new(player_lines).block(player_block),
        chunks[1],
    );

    // Battle log
    let max_log = chunks[2].height.saturating_sub(2) as usize;
    let log_start = battle.battle_log.len().saturating_sub(max_log);
    let log_lines: Vec<Line> = battle.battle_log[log_start..]
        .iter()
        .map(|msg| Line::from(Span::styled(format!(" {}", msg), Style::default().fg(Color::White))))
        .collect();

    let log_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 戦況 ");
    f.render_widget(
        Paragraph::new(log_lines)
            .block(log_block)
            .wrap(Wrap { trim: false }),
        chunks[2],
    );

    // Actions
    render_battle_actions(state, battle, f, chunks[3], borders, click_state);
}

fn render_battle_actions(
    state: &RpgState,
    battle: &super::state::BattleState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    match battle.action {
        BattleAction::SelectAction => {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(" ▶ ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled("攻撃 [1]", Style::default().fg(Color::White)),
                ]),
                BATTLE_ATTACK,
            );
            if !available_skills(state.level).is_empty() {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(" ▶ ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                        Span::styled("スキル [2]", Style::default().fg(Color::White)),
                    ]),
                    BATTLE_SKILL,
                );
            }
            if !battle_consumables(state).is_empty() {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(" ▶ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled("アイテム [3]", Style::default().fg(Color::White)),
                    ]),
                    BATTLE_ITEM,
                );
            }
            if !battle.is_boss {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(" ▶ ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                        Span::styled("逃げる [4]", Style::default().fg(Color::DarkGray)),
                    ]),
                    BATTLE_FLEE,
                );
            }
        }
        BattleAction::SelectSkill => {
            let skills = available_skills(state.level);
            for (i, &skill) in skills.iter().enumerate() {
                let sinfo = skill_info(skill);
                let can_use = state.mp >= sinfo.mp_cost;
                let color = if can_use { Color::White } else { Color::DarkGray };
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" [{}] ", i + 1),
                            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} (MP:{})", sinfo.name, sinfo.mp_cost),
                            Style::default().fg(color),
                        ),
                    ]),
                    SKILL_SELECT_BASE + i as u16,
                );
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " ◀ 戻る [-]",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
                BACK_FROM_SKILL,
            );
        }
        BattleAction::SelectItem => {
            let items = battle_consumables(state);
            for (i, (_idx, kind, count)) in items.iter().enumerate() {
                let iinfo = item_info(*kind);
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" [{}] ", i + 1),
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} x{} - {}", iinfo.name, count, iinfo.description),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    BATTLE_ITEM_BASE + i as u16,
                );
            }
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " ◀ 戻る [-]",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
                BACK_FROM_BATTLE_ITEM,
            );
        }
        BattleAction::Victory => {
            let einfo = enemy_info(battle.enemy.kind);
            cl.push(Line::from(Span::styled(
                format!(" ★ {}を倒した！", einfo.name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                format!("   EXP+{} {}G", einfo.exp, einfo.gold),
                Style::default().fg(Color::Yellow),
            )));
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " ▶ 続ける [0]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                BATTLE_CONTINUE,
            );
        }
        BattleAction::Defeat => {
            cl.push(Line::from(Span::styled(
                " ✖ 力尽きた...",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                "   村で目を覚ます (所持金半減)",
                Style::default().fg(Color::Red),
            )));
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " ▶ 続ける [0]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                BATTLE_CONTINUE,
            );
        }
        BattleAction::Fled => {
            cl.push(Line::from(Span::styled(
                " → うまく逃げ切った！",
                Style::default().fg(Color::Green),
            )));
            cl.push(Line::from(""));
            cl.push_clickable(
                Line::from(Span::styled(
                    " ▶ 続ける [0]",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                BATTLE_CONTINUE,
            );
        }
        BattleAction::EnemyTurn => {
            // Shown briefly, transitions automatically
        }
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" コマンド ");

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(area, &block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(Paragraph::new(cl.into_lines()).block(block), area);
}

// ── Inventory Screen ─────────────────────────────────────────

fn render_inventory(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // Equipment
            Constraint::Min(10),   // Items
            Constraint::Length(4),  // Footer
        ])
        .split(area);

    // Equipment display
    let weapon_name = state
        .weapon
        .map(|w| item_info(w).name)
        .unwrap_or("なし");
    let armor_name = state
        .armor
        .map(|a| item_info(a).name)
        .unwrap_or("なし");

    let equip_lines = vec![
        Line::from(vec![
            Span::styled(" 武器: ", Style::default().fg(Color::Gray)),
            Span::styled(weapon_name, Style::default().fg(Color::White)),
            Span::styled(
                format!(" (ATK:{})", state.total_atk()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 防具: ", Style::default().fg(Color::Gray)),
            Span::styled(armor_name, Style::default().fg(Color::White)),
            Span::styled(
                format!(" (DEF:{})", state.total_def()),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    let equip_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 装備 ");
    f.render_widget(
        Paragraph::new(equip_lines).block(equip_block),
        chunks[0],
    );

    // Items list
    let mut cl = ClickableList::new();
    if state.inventory.is_empty() {
        cl.push(Line::from(Span::styled(
            " アイテムなし",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, item) in state.inventory.iter().enumerate() {
            let iinfo = item_info(item.kind);
            let category_str = match iinfo.category {
                ItemCategory::Consumable => "消",
                ItemCategory::Weapon => "武",
                ItemCategory::Armor => "防",
                ItemCategory::KeyItem => "鍵",
            };
            let key = if i < 9 {
                format!("[{}]", i + 1)
            } else {
                "   ".to_string()
            };
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {} {}", key, category_str),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!(" {} x{}", iinfo.name, item.count),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!(" - {}", iinfo.description),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                INV_USE_BASE + i as u16,
            );
        }
    }

    let items_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(format!(" 持ち物 ({}G) ", state.gold));

    // Footer
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(
        Line::from(Span::styled(
            " ◀ 戻る [-]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        BACK_FROM_INVENTORY,
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[1], &items_block, &mut cs, 0, 0);
    cl_footer.register_targets_with_block(chunks[2], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(
        Paragraph::new(cl.into_lines()).block(items_block),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[2],
    );
}

// ── Quest Log Screen ─────────────────────────────────────────

fn render_quest_log(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),
            Constraint::Length(4),
        ])
        .split(area);

    let quests = visible_quests(state);
    let mut lines = Vec::new();

    if quests.is_empty() {
        lines.push(Line::from(Span::styled(
            " クエストなし",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (quest_id, status) in &quests {
            let info = quest_info(*quest_id);
            let (icon, color) = match status {
                QuestStatus::Completed => ("✓", Color::DarkGray),
                QuestStatus::ReadyToComplete => ("★", Color::Yellow),
                QuestStatus::Active => ("●", Color::Cyan),
                QuestStatus::Available => ("○", Color::White),
            };
            let kind_str = if info.kind == QuestKind::Main {
                "[メイン]"
            } else {
                "[サイド]"
            };
            let status_str = match status {
                QuestStatus::Completed => "完了",
                QuestStatus::ReadyToComplete => "報告可",
                QuestStatus::Active => "進行中",
                QuestStatus::Available => "受注可",
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(color)),
                Span::styled(
                    format!("{} {}", kind_str, info.name),
                    Style::default().fg(color),
                ),
                Span::styled(
                    format!(" ({})", status_str),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            if *status == QuestStatus::Active || *status == QuestStatus::ReadyToComplete {
                lines.push(Line::from(Span::styled(
                    format!("   {}", info.description),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" クエストログ ");
    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Footer
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(
        Line::from(Span::styled(
            " ◀ 戻る [-]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        BACK_FROM_QUEST_LOG,
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl_footer.register_targets_with_block(chunks[1], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Shop Screen ──────────────────────────────────────────────

fn render_shop(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(area);

    let shop = shop_inventory(state.location);
    let mut cl = ClickableList::new();

    cl.push(Line::from(vec![
        Span::styled(
            format!(" 所持金: {}G", state.gold),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    cl.push(Line::from(""));

    for (i, &(kind, _stock)) in shop.iter().enumerate() {
        let iinfo = item_info(kind);
        let affordable = state.gold >= iinfo.buy_price;
        let color = if affordable {
            Color::White
        } else {
            Color::DarkGray
        };
        let key = if i < 9 {
            format!("[{}]", i + 1)
        } else {
            "   ".to_string()
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" {} ", key), Style::default().fg(Color::DarkGray)),
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

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" ショップ ");

    // Footer
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(
        Line::from(Span::styled(
            " ◀ 戻る [-]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        BACK_FROM_SHOP,
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[0], &block, &mut cs, 0, 0);
    cl_footer.register_targets_with_block(chunks[1], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(Paragraph::new(cl.into_lines()).block(block), chunks[0]);
    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Status Screen ────────────────────────────────────────────

fn render_status(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),
            Constraint::Length(4),
        ])
        .split(area);

    let weapon_name = state.weapon.map(|w| item_info(w).name).unwrap_or("なし");
    let armor_name = state.armor.map(|a| item_info(a).name).unwrap_or("なし");
    let skills = available_skills(state.level);

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" レベル: {}", state.level),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  EXP: {}", state.exp),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!(" HP: {}/{}", state.hp, state.max_hp),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("  MP: {}/{}", state.mp, state.max_mp),
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!(" ATK: {}", state.total_atk()),
                Style::default().fg(Color::Red),
            ),
            Span::styled(
                format!("  DEF: {}", state.total_def()),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  MAG: {}", state.mag),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" 武器: ", Style::default().fg(Color::Gray)),
            Span::styled(weapon_name, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" 防具: ", Style::default().fg(Color::Gray)),
            Span::styled(armor_name, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 【スキル】",
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        )),
    ];

    let mut all_lines = lines;
    for &skill in &skills {
        let sinfo = skill_info(skill);
        all_lines.push(Line::from(vec![
            Span::styled(
                format!("  {} (MP:{})", sinfo.name, sinfo.mp_cost),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!(" - {}", sinfo.description),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" ステータス ");
    f.render_widget(Paragraph::new(all_lines).block(block), chunks[0]);

    // Footer
    let mut cl_footer = ClickableList::new();
    cl_footer.push(Line::from(""));
    cl_footer.push_clickable(
        Line::from(Span::styled(
            " ◀ 戻る [-]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        BACK_FROM_STATUS,
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl_footer.register_targets_with_block(chunks[1], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(
        Paragraph::new(cl_footer.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Dialogue Screen ──────────────────────────────────────────

fn render_dialogue(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let dialogue = match &state.dialogue {
        Some(d) => d,
        None => return,
    };

    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let mut lines = Vec::new();
    // Show current line and previous lines
    for (i, line) in dialogue.lines.iter().enumerate() {
        if i <= dialogue.current_line {
            let color = if i == dialogue.current_line {
                Color::White
            } else {
                Color::DarkGray
            };
            lines.push(Line::from(Span::styled(
                format!(" {}", line),
                Style::default().fg(color),
            )));
        }
    }

    lines.push(Line::from(""));

    let remaining = dialogue.lines.len() - dialogue.current_line - 1;
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            " ▼ 次へ [0]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " ▶ 閉じる [0]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }

    let mut cl = ClickableList::new();
    cl.push_clickable(
        Line::from(Span::styled(
            if remaining > 0 {
                " ▼ 次へ [0]"
            } else {
                " ▶ 閉じる [0]"
            },
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        DIALOGUE_NEXT,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::White))
        .title(" 会話 ");

    // Remove the duplicate "next" line from lines since we use ClickableList separately
    lines.pop(); // remove the last "次へ/閉じる" line

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        chunks[0],
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[1], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(
        Paragraph::new(cl.into_lines()).block(footer_block),
        chunks[1],
    );
}

// ── Game Clear Screen ────────────────────────────────────────

fn render_game_clear(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let completed_quests = state
        .quests
        .iter()
        .filter(|q| q.status == QuestStatus::Completed)
        .count();
    let total_quests = state.quests.len();

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " ★★★ GAME CLEAR ★★★",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " 魔王を倒し、世界に平和が戻った！",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" レベル: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", state.level),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled(" クエスト: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}/{}", completed_quests, total_quests),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 所持金: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}G", state.gold),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 冒険をありがとう！",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" ★ GAME CLEAR ★ ");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(12),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    let mut cl = ClickableList::new();
    cl.push_clickable(
        Line::from(Span::styled(
            " ▶ メニューに戻る [0]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        GAME_CLEAR_CONTINUE,
    );

    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut cs = click_state.borrow_mut();
    cl.register_targets_with_block(chunks[1], &footer_block, &mut cs, 0, 0);
    drop(cs);

    f.render_widget(
        Paragraph::new(cl.into_lines()).block(footer_block),
        chunks[1],
    );
}

use super::state::LocationId;
