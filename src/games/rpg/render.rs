//! Dungeon Dive rendering — single screen, scene-based.
//!
//! Layout: status bar + scene content + log.
//! DungeonExplore uses a split layout: 2D map (left) + controls (right).
//! Overlays (inventory, shop, status) are full-screen replacements.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{ClickableGrid, ClickableList};

use super::actions::*;
use super::dungeon_view;
use super::logic::{available_skills, battle_consumables, return_bonus, town_choices};
use super::lore::{floor_theme, theme_name};
use super::state::{
    enemy_info, item_info, skill_element, skill_info, BattlePhase, Element, Overlay, RpgState,
    Scene,
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
        Scene::DungeonExplore => render_main(state, f, area, click_state),
        Scene::DungeonEvent => render_main(state, f, area, click_state),
        Scene::DungeonResult => render_main(state, f, area, click_state),
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
    cl.render(f, chunks[1], choice_block, &mut cs, false, 0);
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

    // Dungeon floor info shown when in dungeon
    let in_dungeon = state.dungeon.is_some();
    let dbar_h: u16 = if in_dungeon { 1 } else { 0 };

    let constraints = if in_dungeon {
        vec![
            Constraint::Length(3),    // status bar
            Constraint::Length(dbar_h), // floor info
            Constraint::Min(6),       // content
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

    // Dungeon floor indicator
    if in_dungeon {
        render_floor_indicator(state, f, chunks[chunk_idx], borders);
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

fn render_floor_indicator(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    if let Some(map) = &state.dungeon {
        let theme = floor_theme(map.floor_num);
        let bonus = return_bonus(map.floor_num, state.run_rooms_explored);
        let bonus_span = if bonus > 0 {
            Span::styled(
                format!(" 帰還+{}G", bonus),
                Style::default().fg(Color::Green),
            )
        } else {
            Span::styled(" 帰還+0G", Style::default().fg(Color::DarkGray))
        };

        let line = Line::from(vec![
            Span::styled(
                format!(" B{}F ", map.floor_num),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("〈{}〉", theme_name(theme)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!(" 探索:{}", state.run_rooms_explored),
                Style::default().fg(Color::Gray),
            ),
            bonus_span,
        ]);

        let block = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(Paragraph::new(vec![line]).block(block), area);
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
        Scene::DungeonExplore => render_dungeon_explore(state, f, area, borders, click_state),
        Scene::DungeonEvent => render_dungeon_event(state, f, area, borders, click_state),
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

    push_overlay_hints(&mut cl);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, true, 0);
}

// ── Dungeon Explore (3D View + Minimap) ──────────────────────

fn render_dungeon_explore(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return,
    };
    let theme = floor_theme(map.floor_num);
    let is_narrow = is_narrow_layout(area.width);

    if !is_narrow && area.width >= 40 {
        // Wide layout: 2D map (left) + controls panel (right)
        // Compute map cell count based on available height
        let inner_h = area.height.saturating_sub(2) as usize; // minus borders
        let n = {
            let by_h = if inner_h >= 5 { (inner_h - 1) / 2 } else { 3 };
            let mut v = by_h.min(11);
            if v % 2 == 0 { v = v.saturating_sub(1); }
            v.clamp(5, 11)
        };
        let map_w = (n * 3 + 1 + 2) as u16; // +2 for borders

        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(map_w), Constraint::Min(18)])
            .split(area);

        render_2d_map(state, f, h_chunks[0], borders, theme, click_state);
        render_explore_panel(state, f, h_chunks[1], borders, click_state);
    } else {
        // Narrow layout: 2D map (top) + controls (bottom)
        let inner_w = area.width.saturating_sub(2) as usize;
        let inner_h_total = area.height.saturating_sub(2) as usize;
        // Reserve at least 7 rows for controls
        let map_max_h = inner_h_total.saturating_sub(7);
        let n = {
            let by_w = if inner_w >= 7 { (inner_w - 1) / 3 } else { 3 };
            let by_h = if map_max_h >= 5 { (map_max_h - 1) / 2 } else { 3 };
            let mut v = by_w.min(by_h).min(9);
            if v % 2 == 0 { v = v.saturating_sub(1); }
            v.clamp(5, 9)
        };
        let map_h = (n * 2 + 1 + 2) as u16; // +2 for borders

        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(map_h), Constraint::Min(6)])
            .split(area);

        render_2d_map(state, f, v_chunks[0], borders, theme, click_state);
        render_explore_panel(state, f, v_chunks[1], borders, click_state);
    }
}

fn render_2d_map(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    theme: super::state::FloorTheme,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return,
    };

    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut map_lines = dungeon_view::render_map_2d(map, theme, inner_w, inner_h);

    // Add description line if there's room
    let view = dungeon_view::compute_view(map);
    let desc = dungeon_view::describe_view(&view);
    if map_lines.len() < inner_h {
        map_lines.push(Line::from(Span::styled(
            format!(" {}", desc),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    // Register directional tap zones (3×3 grid over the map area)
    let inner = block.inner(area);
    if inner.height >= 3 && inner.width >= 6 {
        let cell_w = inner.width / 3;
        let cell_h = inner.height / 3;
        let grid = ClickableGrid::new(3, 3, MAP_TAP_BASE, cell_w)
            .with_cell_height(cell_h);
        let mut cs = click_state.borrow_mut();
        grid.register_targets(area, &block, &mut cs, 0);
    }

    f.render_widget(Paragraph::new(map_lines).block(block), area);
}

fn render_explore_panel(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let map = match &state.dungeon {
        Some(m) => m,
        None => return,
    };

    let mut cl = ClickableList::new();

    // Tap / arrow key hint
    cl.push(Line::from(Span::styled(
        " マップタップ / 矢印キーで移動",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    // Compass: show what's in each cardinal direction
    render_compass_line(&mut cl, map);

    render_hp_warning(&mut cl, state);

    // Scene text (atmosphere)
    for text in &state.scene_text {
        if !text.is_empty() {
            cl.push(Line::from(Span::styled(
                format!(" {}", text),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    cl.push(Line::from(""));
    render_movement_controls(&mut cl, map);
    push_overlay_hints(&mut cl);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, true, 0);
}

fn render_movement_controls(cl: &mut ClickableList, map: &super::state::DungeonMap) {
    let cell = map.player_cell();
    let can_fwd = !cell.wall(map.facing);

    // Compact: forward + turn buttons (keyboard shortcuts shown for desktop)
    let fwd_style = if can_fwd { Color::Cyan } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " [W] ",
                Style::default()
                    .fg(fwd_style)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if can_fwd { "▲ 前進" } else { "× 壁" },
                Style::default().fg(if can_fwd { Color::White } else { Color::DarkGray }),
            ),
        ]),
        MOVE_FORWARD,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" [A] ", Style::default().fg(Color::Cyan)),
            Span::styled("◀ 左", Style::default().fg(Color::White)),
        ]),
        TURN_LEFT,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" [D] ", Style::default().fg(Color::Cyan)),
            Span::styled("▶ 右", Style::default().fg(Color::White)),
        ]),
        TURN_RIGHT,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" [X] ", Style::default().fg(Color::DarkGray)),
            Span::styled("▼ 転回", Style::default().fg(Color::DarkGray)),
        ]),
        TURN_AROUND,
    );
}

/// Render a compass line showing what's in each cardinal direction.
fn render_compass_line(cl: &mut ClickableList, map: &super::state::DungeonMap) {
    use super::state::{CellType, Facing, MapCell};

    fn compass_marker(cell: &MapCell) -> (&'static str, Color) {
        if !cell.visited {
            return ("?", Color::DarkGray);
        }
        if !cell.event_done {
            match cell.cell_type {
                CellType::Enemy => ("!", Color::Red),
                CellType::Treasure => ("\u{25c6}", Color::Yellow),
                CellType::Spring => ("~", Color::Cyan),
                CellType::Lore => ("\u{2726}", Color::Yellow),
                CellType::Npc => ("?", Color::Magenta),
                CellType::Stairs => ("\u{25bc}", Color::Green),
                CellType::Entrance => ("\u{25c7}", Color::Green),
                _ => ("\u{00b7}", Color::White),
            }
        } else {
            ("\u{00b7}", Color::White)
        }
    }

    let cell = map.player_cell();
    let mut spans: Vec<Span> = Vec::new();

    for &(dir, arrow) in &[
        (Facing::North, "\u{2191}"),
        (Facing::East, "\u{2192}"),
        (Facing::South, "\u{2193}"),
        (Facing::West, "\u{2190}"),
    ] {
        if cell.wall(dir) {
            spans.push(Span::styled(
                format!(" {}\u{58c1}", arrow),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            let nx = map.player_x as i32 + dir.dx();
            let ny = map.player_y as i32 + dir.dy();
            if map.in_bounds(nx, ny) {
                let adj = map.cell(nx as usize, ny as usize);
                let (marker, color) = compass_marker(adj);
                spans.push(Span::styled(
                    format!(" {}{}", arrow, marker),
                    Style::default().fg(color),
                ));
            } else {
                spans.push(Span::styled(
                    format!(" {}\u{58c1}", arrow),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
    }

    cl.push(Line::from(spans));
}

fn render_hp_warning(cl: &mut ClickableList, state: &RpgState) {
    let hp_ratio = if state.max_hp > 0 {
        state.hp as f64 / state.max_hp as f64
    } else {
        1.0
    };
    if hp_ratio <= 0.25 && hp_ratio > 0.0 {
        cl.push(Line::from(Span::styled(
            " ※ 体力が危険！",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
    } else if hp_ratio <= 0.5 {
        cl.push(Line::from(Span::styled(
            " ※ 傷が痛む…",
            Style::default().fg(Color::Yellow),
        )));
    }
}

// ── Dungeon Event (Interactive Choices) ──────────────────────

fn render_dungeon_event(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    if let Some(event) = &state.active_event {
        for line in &event.description {
            if line.is_empty() {
                cl.push(Line::from(""));
            } else {
                cl.push(Line::from(Span::styled(
                    format!(" {}", line),
                    Style::default().fg(Color::White),
                )));
            }
        }

        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            Style::default().fg(Color::DarkGray),
        )));
        cl.push(Line::from(""));

        for (i, choice) in event.choices.iter().enumerate() {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" [{}] ", i + 1),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(choice.label.clone(), Style::default().fg(Color::White)),
                ]),
                EVENT_CHOICE_BASE + i as u16,
            );
        }
    }

    push_overlay_hints(&mut cl);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " イベント ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, true, 0);
}

// ── Dungeon Result ──────────────────────────────────────────

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

    push_choice(&mut cl, 0, "探索を続ける");

    push_overlay_hints(&mut cl);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, true, 0);
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
    // Weakness indicator
    let weak_str = match einfo.weakness {
        Some(Element::Fire) => " [炎弱点]",
        Some(Element::Ice) => " [氷弱点]",
        Some(Element::Thunder) => " [雷弱点]",
        None => "",
    };
    cl.push(Line::from(vec![
        Span::styled(
            format!(" \u{300a}\u{6226}\u{95d8}\u{300b} {}{}", einfo.name, boss_str),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(weak_str, Style::default().fg(Color::Cyan)),
    ]));

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

    // Charge warning
    if battle.enemy_charging {
        cl.push(Line::from(Span::styled(
            " \u{26a0} 力を溜めている！（次のターン大ダメージ）",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        cl.push(Line::from(""));
    }

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
                // Element icon for skills with elements
                let elem_icon = match skill_element(skill) {
                    Some(Element::Fire) => "\u{1f525}",
                    Some(Element::Ice) => "\u{2744}",
                    Some(Element::Thunder) => "\u{26a1}",
                    None => "",
                };
                let label = if elem_icon.is_empty() {
                    format!("{} (MP:{})", sinfo.name, sinfo.mp_cost)
                } else {
                    format!("{}{} (MP:{})", elem_icon, sinfo.name, sinfo.mp_cost)
                };
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
            let lost_run = state.run_gold_earned;
            let pre_run = state.gold.saturating_sub(lost_run);
            let extra = pre_run / 5;
            cl.push(Line::from(Span::styled(
                format!(
                    "   探索報酬{}G + ペナルティ{}G 失う",
                    lost_run, extra
                ),
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
    cl.render(f, area, block, &mut cs, true, 0);
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

fn push_overlay_hints(cl: &mut ClickableList) {
    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " [I] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("持ち物", Style::default().fg(Color::White)),
        ]),
        OPEN_INVENTORY,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " [S] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ステータス", Style::default().fg(Color::White)),
        ]),
        OPEN_STATUS,
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
    cl.render(f, area, block, &mut cs, false, 0);
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

    // Lore progress
    if !state.lore_found.is_empty() {
        cl.push(Line::from(Span::styled(
            format!(" 発見した記録: {}件", state.lore_found.len()),
            Style::default().fg(Color::Cyan),
        )));
    }
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
    cl.render(f, area, block, &mut cs, false, 0);
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

    let shop = super::state::shop_items(state.max_floor_reached);
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
    cl.render(f, area, block, &mut cs, false, 0);
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
    cl.render(f, area, block, &mut cs, false, 0);
}
