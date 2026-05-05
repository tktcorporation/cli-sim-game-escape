//! Dungeon Dive rendering — single screen, scene-based.
//!
//! Layout: status bar + scene content + log.
//! Inline combat happens on the dungeon explore screen (no separate
//! battle screen). Skill / quest / pray are overlays.

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
use super::logic::{available_quests, available_skills, return_bonus, town_choices};
use super::lore::{floor_theme, theme_name};
use super::state::{
    affix_info, enemy_info, item_info, skill_element, skill_info, Element, Overlay, RpgState,
    Scene,
};

pub fn render(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if let Some(overlay) = state.overlay {
        match overlay {
            Overlay::Inventory => render_inventory(state, f, area, click_state),
            Overlay::Status => render_status(state, f, area, click_state),
            Overlay::Shop => render_shop(state, f, area, click_state),
            Overlay::SkillMenu => render_skill_menu(state, f, area, click_state),
            Overlay::QuestBoard => render_quest_board(state, f, area, click_state),
            Overlay::PrayMenu => render_pray_menu(state, f, area, click_state),
        }
        return;
    }

    match state.scene {
        Scene::Intro(_) => render_intro(state, f, area, click_state),
        Scene::Town => render_main(state, f, area, click_state),
        Scene::DungeonExplore => render_main(state, f, area, click_state),
        Scene::DungeonEvent => render_main(state, f, area, click_state),
        Scene::GameClear => render_game_clear(state, f, area, click_state),
    }
}

// ── Helper: HP bar ──────────────────────────────────────────

fn hp_bar(current: u32, max: u32, width: usize) -> (String, Color) {
    let ratio = if max > 0 { current as f64 / max as f64 } else { 0.0 };
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

fn satiety_color(s: u32, max: u32) -> Color {
    if max == 0 { return Color::Red; }
    let r = s as f64 / max as f64;
    if r > 0.5 { Color::Green }
    else if r > 0.25 { Color::Yellow }
    else if r > 0.0 { Color::Rgb(220, 100, 50) }
    else { Color::Red }
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

// ── Main Screen ─────────────────────────────────────────────

fn render_main(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let is_narrow = is_narrow_layout(area.width);
    let log_h: u16 = if is_narrow && state.scene == Scene::DungeonExplore { 2 } else { 4 };

    let in_dungeon = state.dungeon.is_some();
    let dbar_h: u16 = if in_dungeon { 1 } else { 0 };

    let constraints = if in_dungeon {
        vec![
            Constraint::Length(3),
            Constraint::Length(dbar_h),
            Constraint::Min(6),
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
    render_status_bar(state, f, chunks[chunk_idx], borders, is_narrow);
    chunk_idx += 1;

    if in_dungeon {
        render_floor_indicator(state, f, chunks[chunk_idx], borders);
        chunk_idx += 1;
    }

    render_scene_content(state, f, chunks[chunk_idx], borders, click_state);
    chunk_idx += 1;

    render_log(state, f, chunks[chunk_idx], borders);
}

fn render_status_bar(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let hp_w = if is_narrow { 6 } else { 10 };
    let (hp_bar_str, hp_color) = hp_bar(state.hp, state.effective_max_hp(), hp_w);

    let mp_w = if is_narrow { 4 } else { 6 };
    let mp_ratio = if state.max_mp > 0 { state.mp as f64 / state.max_mp as f64 } else { 0.0 };
    let mp_filled = (mp_ratio * mp_w as f64).round() as usize;
    let mp_empty = mp_w - mp_filled;
    let mp_bar_str = "\u{2588}".repeat(mp_filled) + &"\u{2591}".repeat(mp_empty);

    // Satiety bar
    let sat_w = if is_narrow { 4 } else { 6 };
    let sat_ratio = if state.satiety_max > 0 {
        state.satiety as f64 / state.satiety_max as f64
    } else { 0.0 };
    let sat_filled = (sat_ratio * sat_w as f64).round() as usize;
    let sat_empty = sat_w - sat_filled;
    let sat_bar_str = "\u{2588}".repeat(sat_filled) + &"\u{2591}".repeat(sat_empty);
    let sat_color = satiety_color(state.satiety, state.satiety_max);

    let mut spans = vec![
        Span::styled(
            format!(" Lv.{}", state.level),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" HP", Style::default().fg(Color::Gray)),
        Span::styled(hp_bar_str, Style::default().fg(hp_color)),
        Span::styled(
            format!("{}/{}", state.hp, state.effective_max_hp()),
            Style::default().fg(Color::White),
        ),
        Span::styled(" MP", Style::default().fg(Color::Gray)),
        Span::styled(mp_bar_str, Style::default().fg(Color::Blue)),
        Span::styled(
            format!("{}/{}", state.mp, state.max_mp),
            Style::default().fg(Color::White),
        ),
        Span::styled(" 食", Style::default().fg(Color::Gray)),
        Span::styled(sat_bar_str, Style::default().fg(sat_color)),
        Span::styled(
            format!(" {}G", state.gold),
            Style::default().fg(Color::Yellow),
        ),
    ];

    if state.buffs.shield_turns > 0 || state.buffs.berserk_turns > 0 || state.buffs.potion_turns > 0 {
        let mut s = String::from(" ");
        if state.buffs.shield_turns > 0 { s.push_str("[盾]"); }
        if state.buffs.berserk_turns > 0 { s.push_str("[狂]"); }
        if state.buffs.potion_turns > 0 { s.push_str("[力]"); }
        spans.push(Span::styled(s, Style::default().fg(Color::Magenta)));
    }

    let title = if is_narrow { " Dungeon " } else { " Dungeon Dive " };
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(vec![Line::from(spans)]).block(block), area);
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

        // Count nearby awake monsters
        let awake_nearby = map.monsters.iter().filter(|m| m.hp > 0 && m.awake).count();
        let monster_span = if awake_nearby > 0 {
            Span::styled(
                format!(" 敵{}", awake_nearby),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" 敵0", Style::default().fg(Color::DarkGray))
        };

        let line = Line::from(vec![
            Span::styled(
                format!(" B{}F ", map.floor_num),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("〈{}〉", theme_name(theme)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!(" 探索:{}", state.run_rooms_explored),
                Style::default().fg(Color::Gray),
            ),
            monster_span,
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
        Scene::DungeonExplore => render_dungeon_explore(state, f, area, borders, click_state),
        Scene::DungeonEvent => render_dungeon_event(state, f, area, borders, click_state),
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
                " 最深到達: B{}F  クリア: {}回  信仰: {}",
                state.max_floor_reached, state.total_clears, state.faith,
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " \u{2500}".repeat(15),
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

// ── Dungeon Explore ──────────────────────────────────────────

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
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(20)])
            .split(area);
        render_2d_map(state, f, h_chunks[0], borders, theme, click_state);
        render_explore_panel(state, f, h_chunks[1], borders, click_state);
    } else {
        let inner_h_total = area.height.saturating_sub(2) as usize;
        let map_max_h = inner_h_total.saturating_sub(9);
        let n = {
            let by_h = map_max_h;
            let mut v = by_h.min(15);
            if v.is_multiple_of(2) { v = v.saturating_sub(1); }
            v.clamp(11, 15)
        };
        let map_h = (n + 2) as u16;
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(map_h), Constraint::Min(8)])
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

    let map_lines = dungeon_view::render_map_2d(map, theme, inner_w, inner_h, state.pet.as_ref());

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    if inner.height >= 3 && inner.width >= 6 {
        let cell_w = inner.width / 3;
        let cell_h = inner.height / 3;
        let grid = ClickableGrid::new(3, 3, MAP_TAP_BASE, cell_w).with_cell_height(cell_h);
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

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 || inner.width < 6 {
        return;
    }

    let dpad_h = 3_u16.min(inner.height.saturating_sub(2));
    let info_h = inner.height.saturating_sub(dpad_h);
    let info_area = Rect::new(inner.x, inner.y, inner.width, info_h);
    let dpad_area = Rect::new(inner.x, inner.y + info_h, inner.width, dpad_h);

    {
        let mut cl = ClickableList::new();
        // Adjacent monster info
        let px = map.player_x as i32;
        let py = map.player_y as i32;
        if let Some(m) = map.monsters.iter().find(|m| {
            m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1
        }) {
            let info = enemy_info(m.kind);
            let (hpb, c) = hp_bar(m.hp, m.max_hp, 8);
            cl.push(Line::from(vec![
                Span::styled(
                    format!(" 敵: {}", info.name),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            cl.push(Line::from(vec![
                Span::styled(" HP", Style::default().fg(Color::Gray)),
                Span::styled(hpb, Style::default().fg(c)),
                Span::styled(
                    format!(" {}/{}", m.hp, m.max_hp),
                    Style::default().fg(Color::White),
                ),
            ]));
            if m.charging {
                cl.push(Line::from(Span::styled(
                    " ⚠ 力を溜めている！",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
            }
        }

        // Pet HP if any
        if let Some(p) = &state.pet {
            let (hpb, c) = hp_bar(p.hp, p.max_hp, 6);
            cl.push(Line::from(vec![
                Span::styled(
                    format!(" {}", p.name),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(" HP", Style::default().fg(Color::Gray)),
                Span::styled(hpb, Style::default().fg(c)),
                Span::styled(
                    format!(" {}/{}", p.hp, p.max_hp),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        render_hp_warning(&mut cl, state);

        // Action buttons (Skill / Pray skipped in dungeon; inventory/status hints)
        cl.push_clickable(
            Line::from(vec![
                Span::styled(" ✦ ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("スキル", Style::default().fg(Color::White)),
            ]),
            OPEN_SKILL_MENU,
        );
        push_overlay_hints(&mut cl);

        let no_block = Block::default();
        let mut cs = click_state.borrow_mut();
        cl.render(f, info_area, no_block, &mut cs, true, 0);
    }

    render_dpad(map, f, dpad_area, click_state);
}

fn render_dpad(
    map: &super::state::DungeonMap,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    use super::state::Facing;

    if area.height < 3 || area.width < 9 {
        return;
    }

    let col_w = area.width / 3;
    let cell_h = (area.height / 3).max(1);

    let dir_style = |dir: Facing| -> Style {
        let nx = map.player_x as i32 + dir.dx();
        let ny = map.player_y as i32 + dir.dy();
        if !map.in_bounds(nx, ny) {
            return Style::default().fg(Color::DarkGray);
        }
        let adj = map.cell(nx as usize, ny as usize);
        if !adj.is_walkable() {
            return Style::default().fg(Color::DarkGray);
        }
        // Monster on this tile?
        if map.monsters.iter().any(|m| m.hp > 0 && m.x == nx as usize && m.y == ny as usize) {
            return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        }
        if !adj.visited {
            return Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        }
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    };

    let center_in = |label: &str, width: usize| -> String {
        let label_len = label.chars().count();
        let pad_left = width.saturating_sub(label_len) / 2;
        let pad_right = width.saturating_sub(pad_left + label_len);
        format!("{}{}{}", " ".repeat(pad_left), label, " ".repeat(pad_right))
    };

    let cw = col_w as usize;
    let blank = " ".repeat(cw);
    let lines = vec![
        Line::from(vec![
            Span::raw(blank.clone()),
            Span::styled(center_in("[ \u{25b2} ]", cw), dir_style(Facing::North)),
            Span::raw(blank.clone()),
        ]),
        Line::from(vec![
            Span::styled(center_in("[ \u{25c0} ]", cw), dir_style(Facing::West)),
            Span::raw(blank.clone()),
            Span::styled(center_in("[ \u{25b6} ]", cw), dir_style(Facing::East)),
        ]),
        Line::from(vec![
            Span::raw(blank.clone()),
            Span::styled(center_in("[ \u{25bc} ]", cw), dir_style(Facing::South)),
            Span::raw(blank),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), area);

    let grid = ClickableGrid::new(3, 3, DPAD_BASE, col_w).with_cell_height(cell_h);
    let no_block = Block::default();
    let mut cs = click_state.borrow_mut();
    grid.register_targets(area, &no_block, &mut cs, 0);
}

fn render_hp_warning(cl: &mut ClickableList, state: &RpgState) {
    let max_hp = state.effective_max_hp();
    let hp_ratio = if max_hp > 0 { state.hp as f64 / max_hp as f64 } else { 1.0 };
    if hp_ratio <= 0.25 && hp_ratio > 0.0 {
        cl.push(Line::from(Span::styled(
            " ※ 体力が危険！",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    } else if hp_ratio <= 0.5 {
        cl.push(Line::from(Span::styled(
            " ※ 傷が痛む…",
            Style::default().fg(Color::Yellow),
        )));
    }
    if state.satiety == 0 {
        cl.push(Line::from(Span::styled(
            " ※ 飢えている！",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    } else if state.satiety < 100 {
        cl.push(Line::from(Span::styled(
            " ※ お腹が空いた…",
            Style::default().fg(Color::Yellow),
        )));
    }
}

// ── Dungeon Event ────────────────────────────────────────────

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
            " \u{2500}".repeat(15),
            Style::default().fg(Color::DarkGray),
        )));
        cl.push(Line::from(""));

        for (i, choice) in event.choices.iter().enumerate() {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {}. ", i + 1),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
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
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

// ── Choice Helpers ──────────────────────────────────────────

fn push_choice(cl: &mut ClickableList, index: usize, label: &str) {
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!(" {}. ", index + 1),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
            Span::styled(" 🎒 ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("持ち物", Style::default().fg(Color::White)),
        ]),
        OPEN_INVENTORY,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" 📊 ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("ステータス", Style::default().fg(Color::White)),
        ]),
        OPEN_STATUS,
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

    let weapon_name = state.weapon().map(|w| w.display_name()).unwrap_or_else(|| "なし".into());
    let armor_name = state.armor().map(|a| a.display_name()).unwrap_or_else(|| "なし".into());
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
            let mut tag = String::new();
            if state.weapon_idx == Some(i) || state.armor_idx == Some(i) {
                tag.push_str("[E]");
            }
            let display = item.display_name();
            let label = if item.affix.is_some() {
                format!("{}{}", tag, display)
            } else {
                format!("{}{} x{}", tag, display, item.count)
            };
            if i < 9 {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" {}. ", i + 1),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            label,
                            Style::default().fg(if item.affix.is_some() { Color::Yellow } else { Color::White }),
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
                    format!("     {} - {}", label, iinfo.description),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            " ✕ 閉じる",
            Style::default().fg(Color::Yellow),
        )),
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

    let weapon_name = state.weapon().map(|w| w.display_name()).unwrap_or_else(|| "なし".into());
    let armor_name = state.armor().map(|a| a.display_name()).unwrap_or_else(|| "なし".into());

    cl.push(Line::from(vec![
        Span::styled(
            format!(" Lv.{}", state.level),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  EXP:{}", state.exp),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    cl.push(Line::from(vec![
        Span::styled(
            format!(" HP:{}/{}", state.hp, state.effective_max_hp()),
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
            format!("  MAG:{}", state.total_mag()),
            Style::default().fg(Color::Magenta),
        ),
    ]));
    cl.push(Line::from(vec![
        Span::styled(
            format!(" 満腹度: {}/{}", state.satiety, state.satiety_max),
            Style::default().fg(satiety_color(state.satiety, state.satiety_max)),
        ),
        Span::styled(
            format!("  信仰: {}", state.faith),
            Style::default().fg(Color::Yellow),
        ),
    ]));
    cl.push(Line::from(""));
    cl.push(Line::from(vec![
        Span::styled(" 武器: ", Style::default().fg(Color::Gray)),
        Span::styled(
            weapon_name,
            Style::default().fg(if state.weapon().and_then(|w| w.affix).is_some() { Color::Yellow } else { Color::White }),
        ),
        Span::styled("  防具: ", Style::default().fg(Color::Gray)),
        Span::styled(
            armor_name,
            Style::default().fg(if state.armor().and_then(|a| a.affix).is_some() { Color::Yellow } else { Color::White }),
        ),
    ]));
    if let Some(w) = state.weapon() {
        if let Some(a) = w.affix {
            cl.push(Line::from(Span::styled(
                format!("  └ 接頭辞: {} (Element: {:?})", affix_info(a).prefix, affix_info(a).element),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    cl.push(Line::from(""));

    if let Some(q) = &state.active_quest {
        cl.push(Line::from(Span::styled(
            format!(" 受託中: {}  (報酬+{}G/+{}EXP)", q.description(), q.reward_gold, q.reward_exp),
            Style::default().fg(Color::Cyan),
        )));
    }
    if let Some(p) = &state.pet {
        cl.push(Line::from(Span::styled(
            format!(" ペット: {} Lv.{} HP:{}/{}", p.name, p.level, p.hp, p.max_hp),
            Style::default().fg(Color::Cyan),
        )));
    }

    cl.push(Line::from(Span::styled(
        format!(
            " 最深到達: B{}F  クリア: {}回  完了依頼: {}",
            state.max_floor_reached, state.total_clears, state.completed_quests,
        ),
        Style::default().fg(Color::Yellow),
    )));

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
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
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
        Line::from(Span::styled(" ✕ 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " ステータス ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let shop = super::state::shop_items(state.max_floor_reached);
    for (i, &(kind, _)) in shop.iter().enumerate() {
        let iinfo = item_info(kind);
        let affordable = state.gold >= iinfo.buy_price;
        let color = if affordable { Color::White } else { Color::DarkGray };
        if i < 9 {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {}. ", i + 1),
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
        Line::from(Span::styled(" ✕ 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " ショップ ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_skill_menu(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(Span::styled(
        format!(" MP:{}/{}", state.mp, state.max_mp),
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let skills = available_skills(state.level);
    if skills.is_empty() {
        cl.push(Line::from(Span::styled(
            " 習得済みスキルなし",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, &skill) in skills.iter().enumerate() {
            let info = skill_info(skill);
            let can_use = state.mp >= info.mp_cost;
            let elem_icon = match skill_element(skill) {
                Some(Element::Fire) => "\u{1f525}",
                Some(Element::Ice) => "\u{2744}",
                Some(Element::Thunder) => "\u{26a1}",
                None => "  ",
            };
            let label = format!("{}{} (MP:{}) - {}", elem_icon, info.name, info.mp_cost, info.description);
            if can_use {
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" {}. ", i + 1),
                            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(label, Style::default().fg(Color::White)),
                    ]),
                    SKILL_BASE + i as u16,
                );
            } else {
                cl.push(Line::from(Span::styled(
                    format!(" {}. {}", i + 1, label),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" ✕ 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Blue))
        .title(Span::styled(
            " スキル ",
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_quest_board(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(Span::styled(
        " 〈冒険者ギルド掲示板〉",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    if let Some(q) = &state.active_quest {
        cl.push(Line::from(Span::styled(
            format!(" 受託中: {}", q.description()),
            Style::default().fg(Color::Cyan),
        )));
        cl.push(Line::from(Span::styled(
            format!("   報酬: {}G / {}EXP", q.reward_gold, q.reward_exp),
            Style::default().fg(Color::DarkGray),
        )));
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(Span::styled(
                " ⌫ 依頼を破棄",
                Style::default().fg(Color::Red),
            )),
            QUEST_ABANDON,
        );
    } else {
        let quests = available_quests(state);
        for (i, q) in quests.iter().enumerate() {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {}. ", i + 1),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(q.description(), Style::default().fg(Color::White)),
                ]),
                QUEST_ACCEPT_BASE + i as u16,
            );
            cl.push(Line::from(Span::styled(
                format!("    報酬: {}G / {}EXP", q.reward_gold, q.reward_exp),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" ✕ 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " 掲示板 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_pray_menu(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 〈祭壇〉",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" 信仰度: {}", state.faith),
        Style::default().fg(Color::Yellow),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 神に祈ると恵み(または試練)が与えられる。",
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(Span::styled(
        " ※ 1冒険につき1回まで",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    if state.prayed_this_run {
        cl.push(Line::from(Span::styled(
            " 今は祈りが届かない…",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(" ✦ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("祈りを捧げる", Style::default().fg(Color::White)),
            ]),
            PRAY_CONFIRM,
        );
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" ✕ 閉じる", Style::default().fg(Color::Yellow))),
        CLOSE_OVERLAY,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " 祭壇 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    push_choice(&mut cl, 0, "メニューに戻る");

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " \u{2605} DUNGEON CLEAR \u{2605} ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}
