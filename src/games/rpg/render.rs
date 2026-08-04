//! Dungeon Dive rendering — single screen, scene-based.
//!
//! Layout: status bar + scene content + log.
//! Inline combat happens on the dungeon explore screen (no separate
//! battle screen). Skill / quest / pray are overlays.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Circle, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{Clickable, ClickableGrid, ClickableList, TabBar};

use super::actions::*;
use super::dungeon_view;
use super::logic::{available_quests, available_skills, return_bonus};
use super::lore::{floor_theme, theme_name};
use super::state::{
    affix_info, element_name, item_info, skill_element, skill_info, DungeonMap, Element,
    Overlay, RpgState, Scene,
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
            Overlay::SkillChoice => render_skill_choice(state, f, area, click_state),
        }
        return;
    }

    match state.scene {
        Scene::Overworld | Scene::DungeonExplore => render_main(state, f, area, click_state),
        Scene::GameClear => render_game_clear(state, f, area, click_state),
    }
}

// ── Helper: HP bar ──────────────────────────────────────────

fn hp_bar(current: u32, max: u32, width: usize) -> (String, Color) {
    let ratio = if max > 0 { current as f64 / max as f64 } else { 0.0 };
    (theme::hp_bar_string(ratio, width), theme::hp_ratio_color(ratio))
}

/// 属性ごとの表示色（弱点表示で使用）。
fn element_color(e: Element) -> Color {
    match e {
        Element::Fire => Color::LightRed,
        Element::Ice => Color::Cyan,
        Element::Thunder => Color::Yellow,
    }
}

/// 階層に連動したアクセント色。深く潜るほど脅威度が上がっていく実感を
/// ステータスバー/フロア表示のボーダーや見出しテキストに持たせる。
/// 村 (floor 0) はCyan固定にする — 色分けの意味は「今どれだけ深く潜っ
/// ているか」なので、潜っていない村では変化させない。
fn floor_color(floor: u32) -> Color {
    match floor {
        0 => Color::Cyan,
        1..=2 => Color::Green,
        3..=4 => Color::Yellow,
        5..=6 => Color::LightRed,
        7..=8 => Color::Magenta,
        _ => Color::Red,
    }
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

/// `render_main` の縦分割 Rect 群。演出 (`RpgEffects`) がどの領域にフラッシュを
/// 掛けるかを決めるのにも使うため、`render_main` の内部でしか使わないローカル
/// 変数ではなく公開関数として切り出してある (mod.rs の detect_transitions から
/// 同じ計算を再利用する)。
pub struct RpgLayout {
    pub status_bar: Rect,
    /// `in_dungeon` が false の時は高さ0 (呼び出し側は演出対象として使わない)。
    pub floor_indicator: Rect,
    pub scene_content: Rect,
    pub log: Rect,
}

pub fn compute_layout(area: Rect, in_dungeon: bool, is_dungeon_explore: bool) -> RpgLayout {
    let is_narrow = is_narrow_layout(area.width);
    let log_h: u16 = if is_narrow && is_dungeon_explore { 2 } else { 4 };
    let dbar_h: u16 = if in_dungeon { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(dbar_h),
            Constraint::Min(6),
            Constraint::Length(log_h),
        ])
        .split(area);

    RpgLayout {
        status_bar: chunks[0],
        floor_indicator: chunks[1],
        scene_content: chunks[2],
        log: chunks[3],
    }
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
    let in_dungeon = state.dungeon.is_some();
    let layout = compute_layout(area, in_dungeon, state.scene == Scene::DungeonExplore);

    render_status_bar(state, f, layout.status_bar, borders, is_narrow);

    if in_dungeon {
        render_floor_indicator(state, f, layout.floor_indicator, borders);
    }

    render_scene_content(state, f, layout.scene_content, borders, click_state);

    render_log(state, f, layout.log, borders);
}

fn render_status_bar(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    is_narrow: bool,
) {
    let hp_w = if is_narrow { 6 } else { 10 };
    let (hp_bar_str, hp_color_by_ratio) = hp_bar(state.hp, state.effective_max_hp(), hp_w);
    // 被弾直後の数tickは残量に関わらずダメージ色を上書きし、「今食らった」ことを
    // 数値の変化を目で追わなくても伝える。
    let hp_color = if state.hero_hurt_flash.is_active() {
        theme::DAMAGE_FLASH.color
    } else {
        hp_color_by_ratio
    };

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
            Style::default().fg(hp_color),
        ),
    ];

    // 被弾直後の短い間だけ実ダメージ量を数値で浮かせる。バーの色変化だけでは
    // 「どれだけ削られたか」までは伝わらないため。HP数値の直後という行の
    // 前寄りの位置に置くのは、ステータスバーが wrap しない Paragraph な
    // ため、後方の桁ほどナロー幅で切り捨てられるから。
    if let Some((dmg, life)) = state.last_hero_damage {
        if life > 0 {
            spans.push(Span::styled(
                format!(" -{}", dmg),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
    }

    spans.extend([
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
    ]);

    if state.buffs.shield_turns > 0 || state.buffs.berserk_turns > 0 || state.buffs.potion_turns > 0 {
        let mut s = String::from(" ");
        if state.buffs.shield_turns > 0 { s.push_str("[盾]"); }
        if state.buffs.berserk_turns > 0 { s.push_str("[狂]"); }
        if state.buffs.potion_turns > 0 { s.push_str("[力]"); }
        spans.push(Span::styled(s, Style::default().fg(Color::Magenta)));
    }

    let floor_num = state.dungeon.as_ref().map(|d| d.floor_num).unwrap_or(0);
    let accent = floor_color(floor_num);

    // グローバル戻るボタン (main.rs, 左上 6 列) が row 0 に重なるため、タイトルは
    // 中央寄せにして先頭が隠れないようにする。
    let title = if is_narrow { " Dungeon " } else { " Dungeon Dive " };
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(accent))
        .title(
            Line::from(Span::styled(
                title,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );
    f.render_widget(Paragraph::new(vec![Line::from(spans)]).block(block), area);
}

fn render_floor_indicator(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    if let Some(map) = &state.dungeon {
        let theme = floor_theme(map.floor_num);

        if map.is_overworld {
            // 階層連動グラデーションは「今どれだけ深く潜っているか」を伝える
            // ための演出なので、潜っていない村では固定の配色にする。
            let block = Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::DarkGray));
            let line = Line::from(vec![
                Span::styled(
                    " 〈村〉 ",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("〈{}〉 ", theme_name(theme)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "R=受付 B=武具 v=村人 $=店 ⚑=掲示板 ⌂=宿 ✴=祭壇 ▼=ダンジョン",
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            f.render_widget(Paragraph::new(vec![line]).block(block), area);
            return;
        }

        let accent = floor_color(map.floor_num);
        let block = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(accent));

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
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
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
        Scene::Overworld | Scene::DungeonExplore => {
            render_dungeon_explore(state, f, area, borders, click_state);
            // Inline event popup — drawn on top of the explore view so the
            // map stays visible while the player picks a choice. See issue
            // #89: events no longer transition to a separate scene.
            if state.active_event.is_some() {
                render_event_popup(state, f, area, click_state);
            }
        }
        _ => {}
    }
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

    // Layout: radar (optional) → info (flexible) → A/B buttons (1 row) → d-pad (3 rows).
    let dpad_h = 3_u16.min(inner.height.saturating_sub(3));
    let ab_h: u16 = if inner.height > dpad_h + 1 { 1 } else { 0 };
    let info_h = inner.height.saturating_sub(dpad_h + ab_h);

    let mut cl = ClickableList::new();
    // Adjacent monster info
    let px = map.player_x as i32;
    let py = map.player_y as i32;
    if let Some(m) = map.monsters.iter().find(|m| {
        m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1
    }) {
        let (hpb, c) = hp_bar(m.hp, m.max_hp, 8);
        // Elite mobs adopt the magenta highlight from the map view.
        let name_color = if m.affix.is_some() { Color::Magenta } else { Color::Red };
        cl.push(Line::from(vec![
            Span::styled(
                format!(" 敵: {}", m.display_name()),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
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
        // 弱点図鑑: 発見済みなら属性を、未発見なら「?」を見せて
        // 「まだ知らない情報がある」ことを示す。
        let weak_span = if state.weakness_known(m.kind) {
            match state.known_weakness(m.kind) {
                Some(w) => Span::styled(
                    element_name(w).to_string(),
                    Style::default().fg(element_color(w)).add_modifier(Modifier::BOLD),
                ),
                None => Span::styled("なし".to_string(), Style::default().fg(Color::Gray)),
            }
        } else {
            Span::styled("?".to_string(), Style::default().fg(Color::DarkGray))
        };
        cl.push(Line::from(vec![
            Span::styled(" 弱点: ", Style::default().fg(Color::Gray)),
            weak_span,
        ]));
        if m.charging {
            cl.push(Line::from(Span::styled(
                " ⚡力を溜めている！",
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                " 防御か回避を！",
                Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
            )));
        }
    }

    // 与ダメージポップアップ。トドメの一撃だと敵は既にリストから消えている
    // (on_player_actionのretainが先に走る) ため、上の「隣接モンスター」
    // ブロックの外に独立させて出す — そうしないと最後の一撃の数字だけ
    // 表示されずに終わってしまう。
    if let Some((dmg, life, crit)) = state.last_enemy_damage {
        if life > 0 {
            let label = if crit { format!(" -{} 会心!", dmg) } else { format!(" -{}", dmg) };
            let color = if crit { Color::LightYellow } else { Color::Yellow };
            cl.push(Line::from(Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
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

    // レーダー有効化の判定は、これから描画する info の内容 (cl) を同じ
    // inner.width で wrap 計算した実測行数を使う。隣接モンスター名の長さ等で
    // 必要行数が変わるため、固定の見積もり値だと折返しで見切れるケースが
    // あった (Codexレビュー指摘)。
    let required_info_rows = cl.visual_height(inner.width);
    let radar_h = radar_height_for(map.is_overworld, info_h, inner.width, required_info_rows);
    let radar_area = Rect::new(inner.x, inner.y, inner.width, radar_h);
    let info_area = Rect::new(inner.x, inner.y + radar_h, inner.width, info_h - radar_h);
    let ab_area = Rect::new(inner.x, inner.y + info_h, inner.width, ab_h);
    let dpad_area = Rect::new(inner.x, inner.y + info_h + ab_h, inner.width, dpad_h);

    if radar_h > 0 {
        render_radar(map, f, radar_area);
    }

    {
        let no_block = Block::default();
        let mut cs = click_state.borrow_mut();
        cl.render(f, info_area, no_block, &mut cs, true, 0);
    }

    if ab_h > 0 {
        render_ab_buttons(state, f, ab_area, click_state);
    }
    render_dpad(map, f, dpad_area, click_state);
}

/// 固定の索敵レーダー高さ (行数)。
const RADAR_H: u16 = 7;

/// 索敵レーダー — 隣接1体の情報だけでは伝わらない周辺の敵配置を常時見せる。
/// `required_info_rows` (呼び出し側が実際に描画する info の内容を
/// `ClickableList::visual_height` で実測した行数) がレーダーの下に収まる
/// 高さがある時だけ確保する。固定の見積もり値ではなく実測値を使うのは、
/// 隣接モンスター名の長さ次第で折返し行数が変わり、固定値だと見切れる
/// ケースがあったため。村 (`is_overworld`) には索敵すべき脅威が無いため、
/// 常に空の円になってしまうので出さない (フロア演出全般が村では変化しない
/// 方針と揃える)。
fn radar_height_for(is_overworld: bool, info_h: u16, width: u16, required_info_rows: u16) -> u16 {
    if !is_overworld && width >= 9 && info_h >= RADAR_H + required_info_rows {
        RADAR_H
    } else {
        0
    }
}

// 部屋の中にいる時は compute_visibility が部屋全体 (10タイル超のことも
// ある) を視界に入れるため、半径を欲張っておかないと部屋内の敵がレーダー
// から漏れてしまう。
const RADAR_DETECT_RADIUS_TILES: f64 = 11.0;
const RADAR_SCALE: f64 = 9.0;

/// 視界内 (お化け同様 `compute_visibility` の判定を流用) かつ awake な
/// モンスターを、レーダー中心 (プレイヤー) からの Canvas 座標 `(x, y, color)`
/// へ変換する。描画から独立させてあるのはユニットテストのため。
fn radar_blips(
    map: &DungeonMap,
    visible: &std::collections::HashSet<(usize, usize)>,
) -> Vec<(f64, f64, Color)> {
    let px = map.player_x as f64;
    let py = map.player_y as f64;
    map.monsters
        .iter()
        .filter(|m| m.hp > 0 && m.awake && visible.contains(&(m.x, m.y)))
        .filter_map(|m| {
            let dx = m.x as f64 - px;
            let dy = m.y as f64 - py;
            if (dx * dx + dy * dy).sqrt() > RADAR_DETECT_RADIUS_TILES {
                return None;
            }
            let color = if m.affix.is_some() {
                Color::Magenta
            } else if m.charging {
                Color::LightRed
            } else {
                Color::Red
            };
            // 画面座標は y が下向きなので、Canvas の数学座標に合わせて反転する。
            Some((
                dx / RADAR_DETECT_RADIUS_TILES * RADAR_SCALE,
                -dy / RADAR_DETECT_RADIUS_TILES * RADAR_SCALE,
                color,
            ))
        })
        .collect()
}

/// braille セル1つは 2(横)×4(縦) の疑似ピクセル。x_bounds/y_bounds を固定
/// のまま横長の `area` にそのまま描くと、疑似ピクセル密度が横方向だけ
/// 上がって円が横に伸びた楕円になる。等密度になる正方形 (幅 = 高さ*2 cell)
/// を `area` から中央寄せで切り出し、円が常に円のまま見えるようにする。
fn square_radar_area(area: Rect) -> Rect {
    let square_w = (area.height.saturating_mul(2)).min(area.width);
    let x_offset = (area.width - square_w) / 2;
    Rect::new(area.x + x_offset, area.y, square_w, area.height)
}

/// 索敵レーダー — プレイヤーを中心に、視界内にいるモンスターを距離・方角で
/// 表示する。隣接1体の情報だけでは伝わらない「周囲に何体いるか」を常時
/// 見せて、探索の緊張感を底上げする。
fn render_radar(map: &DungeonMap, f: &mut Frame, area: Rect) {
    let visible = dungeon_view::compute_visibility(map);
    let blips = radar_blips(map, &visible);

    let canvas = Canvas::default()
        .x_bounds([-10.0, 10.0])
        .y_bounds([-10.0, 10.0])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Circle { x: 0.0, y: 0.0, radius: RADAR_SCALE, color: Color::DarkGray });
            let center = canvas_fx::filled_ellipse_points(0.0, 0.0, 0.6, 0.6, 0.4);
            ctx.draw(&Points { coords: &center, color: Color::Cyan });
            for &(bx, by, color) in &blips {
                let pts = canvas_fx::filled_ellipse_points(bx, by, 0.9, 0.9, 0.45);
                ctx.draw(&Points { coords: &pts, color });
            }
        });
    f.render_widget(canvas, square_radar_area(area));
}

/// Two-button row: A (context-sensitive) and B (open menu).
///
/// A's label changes to hint the contextual action so the player knows
/// what tapping it will do — Elona/Pokémon Mystery Dungeon style.
fn render_ab_buttons(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if area.width < 8 {
        return;
    }
    let half = area.width / 2;
    let a_area = Rect::new(area.x, area.y, half, area.height);
    let b_area = Rect::new(area.x + half, area.y, area.width - half, area.height);

    // Pick A's label based on context.
    let a_label = if state.active_event.is_some() {
        " [A] 決定 "
    } else if let Some(map) = &state.dungeon {
        let px = map.player_x as i32;
        let py = map.player_y as i32;
        let adj = map.monsters.iter().any(|m| {
            m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1
        });
        if adj {
            " [A] スキル "
        } else {
            " [A] 待機 "
        }
    } else {
        " [A] 待機 "
    };

    let a_para = Paragraph::new(Line::from(Span::styled(
        a_label,
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    let b_para = Paragraph::new(Line::from(Span::styled(
        " [B] メニュー ",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);

    let mut cs = click_state.borrow_mut();
    Clickable::new(a_para, AB_A_BUTTON).render(f, a_area, &mut cs);
    Clickable::new(b_para, AB_B_BUTTON).render(f, b_area, &mut cs);
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
    if hp_ratio <= theme::HP_DANGER_RATIO && hp_ratio > 0.0 {
        cl.push(Line::from(Span::styled(
            " ※ 体力が危険！",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    } else if hp_ratio <= theme::HP_CAUTION_RATIO {
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

// ── Dungeon Event Popup ─────────────────────────────────────
//
// Renders the active event as a centred popup over the explore view
// (issue #89). Same `<pre>` is overdrawn — no extra DOM elements
// (CLAUDE.md: "オーバーレイは別 DOM 要素を生やさない").

fn render_event_popup(
    state: &RpgState,
    f: &mut Frame,
    full_area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let event = match &state.active_event {
        Some(e) => e,
        None => return,
    };

    // Estimate popup size from event content.
    let max_choice_label = event
        .choices
        .iter()
        .map(|c| c.label.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let max_desc = event
        .description
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let popup_w = (max_choice_label.max(max_desc) + 12)
        .min(full_area.width.saturating_sub(2))
        .max(28);
    let lines_h = event.description.len() as u16
        + event.choices.len() as u16
        + 6 /* spacing + tip line */;
    let popup_h = lines_h
        .min(full_area.height.saturating_sub(2))
        .max(8);

    let popup_x = full_area.x + (full_area.width.saturating_sub(popup_w)) / 2;
    let popup_y = full_area.y + (full_area.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

    let popup_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " イベント ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let mut cl = ClickableList::new();

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
        " \u{2500}".repeat((popup_w as usize).saturating_sub(2).min(20)),
        Style::default().fg(Color::DarkGray),
    )));

    for (i, choice) in event.choices.iter().enumerate() {
        let selected = i == state.cursor;
        // Cursor marker first so the player sees what A will pick. The
        // older [A]/[B] hardcoded markers were misleading after the cursor
        // unification — A now confirms whichever row is highlighted.
        let prefix = if selected { "▶" } else { " " };
        let label_style = if selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    format!(" {} ", prefix),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}. ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(choice.label.clone(), label_style),
            ]),
            EVENT_CHOICE_BASE + i as u16,
        );
    }

    let mut cs = click_state.borrow_mut();
    // Clear popup_area first so the underlying map doesn't bleed through:
    // Paragraph only overwrites cells it actually draws text into, so an
    // "empty Paragraph" render leaves untouched cells showing whatever was
    // drawn earlier this frame. Clear resets every cell in the area first.
    f.render_widget(Clear, popup_area);
    cl.render(f, popup_area, popup_block, &mut cs, true, 0);
}

/// ログ本文のキーワードから種別を判定して色を返す。rpgのログは絵文字接頭辞
/// を持たない自然文なので、abyssのlog_style (先頭記号判定) とは違い部分
/// 一致で判定する。危険/警告を最優先で拾い、次いで成長関連、最後に成果。
/// どれにも当たらない大半のログ (戦闘の通常ダメージ表記等) は灰色のまま —
/// 全部を色分けすると逆に重要な行が埋もれる。
fn log_style(msg: &str) -> Style {
    const DANGER: &[&str] = &["力尽きた", "飢餓寸前", "飢えで体力が削れる", "神は応えなかった"];
    const GAIN: &[&str] = &[
        "を倒した", "をドロップ", "を落とした", "を授かった", "を受け取った",
        "の加護", "の恵み", "が懐いた",
    ];
    const GROWTH: &[&str] = &["レベルアップ", "を習得", "会心の一撃"];

    if DANGER.iter().any(|kw| msg.contains(kw)) {
        Style::default().fg(Color::Red)
    } else if GROWTH.iter().any(|kw| msg.contains(kw)) {
        Style::default().fg(Color::Yellow)
    } else if GAIN.iter().any(|kw| msg.contains(kw)) {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_log(state: &RpgState, f: &mut Frame, area: Rect, borders: Borders) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(max_lines);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|msg| {
            Line::from(Span::styled(format!(" > {}", msg), log_style(msg)))
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

/// Game Clear screen reuses this for its single "メニューに戻る" button.
fn push_choice(cl: &mut ClickableList, index: usize, label: &str) {
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                "   ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}. ", index + 1),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(label.to_string(), Style::default().fg(Color::White)),
        ]),
        CHOICE_BASE + index as u16,
    );
}

// ── Overlays ────────────────────────────────────────────────

/// Render the unified menu tab bar (持ち物 / スキル / ステータス).
/// Returns the area below the tab bar for the panel content.
fn render_menu_tabs(
    f: &mut Frame,
    area: Rect,
    active: Overlay,
    click_state: &Rc<RefCell<ClickState>>,
) -> Rect {
    if area.height < 3 {
        return area;
    }
    let tab_area = Rect::new(area.x, area.y, area.width, 1);
    // 選択中タブは背景を塗って「押せるボタン」感を出す。地色は各パネルの
    // ボーダー色 (render_inventory=Green / render_skill_menu=Blue /
    // render_status=Cyan) と揃え、タブとその先のパネルが同じ色で繋がって
    // 見えるようにする。
    let style_for = |o: Overlay, base: Color| -> Style {
        if o == active {
            Style::default().fg(Color::Black).bg(base).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base)
        }
    };
    let bar = TabBar::new(" │ ")
        .tab(
            "持ち物",
            style_for(Overlay::Inventory, Color::Green),
            MENU_TAB_INVENTORY,
        )
        .tab("スキル", style_for(Overlay::SkillMenu, Color::Blue), MENU_TAB_SKILL)
        .tab(
            "ステータス",
            style_for(Overlay::Status, Color::Cyan),
            MENU_TAB_STATUS,
        );
    let mut cs = click_state.borrow_mut();
    bar.render(f, tab_area, &mut cs);
    Rect::new(area.x, area.y + 1, area.width, area.height - 1)
}

fn render_inventory(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let area = render_menu_tabs(f, area, Overlay::Inventory, click_state);
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
                let selected = i == state.cursor;
                let prefix = if selected { "▶" } else { " " };
                // Selected and affixed items both deserve yellow; folded
                // into a single condition (clippy complains about the
                // duplicate arm otherwise).
                let label_color = if selected || item.affix.is_some() {
                    Color::Yellow
                } else {
                    Color::White
                };
                let label_mod = if selected { Modifier::BOLD } else { Modifier::empty() };
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" {} ", prefix),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{}. ", i + 1),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            label,
                            Style::default().fg(label_color).add_modifier(label_mod),
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
    let area = render_menu_tabs(f, area, Overlay::Status, click_state);
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

    let skills = available_skills(state);
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
        let selected = i == state.cursor;
        let color = if !affordable {
            Color::DarkGray
        } else if selected {
            Color::Yellow
        } else {
            Color::White
        };
        let prefix = if selected { "▶" } else { " " };
        if i < 9 {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", prefix),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}. ", i + 1),
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
    let area = render_menu_tabs(f, area, Overlay::SkillMenu, click_state);
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(Span::styled(
        format!(" MP:{}/{}", state.mp, state.max_mp),
        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let skills = available_skills(state);
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
            let selected = i == state.cursor;
            let prefix = if selected { "▶" } else { " " };
            if can_use {
                let label_style = if selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(
                            format!(" {} ", prefix),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{}. ", i + 1),
                            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(label, label_style),
                    ]),
                    SKILL_BASE + i as u16,
                );
            } else {
                cl.push(Line::from(Span::styled(
                    format!("   {}. {}", i + 1, label),
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
            let selected = i == state.cursor;
            let prefix = if selected { "▶" } else { " " };
            let label_style = if selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", prefix),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}. ", i + 1),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(q.description(), label_style),
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

// ── Skill Choice (level-up forced pick) ─────────────────────

fn render_skill_choice(
    state: &RpgState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let borders = borders_for(area.width);
    let mut cl = ClickableList::new();
    let Some((left, right)) = state.pending_skill_choice else {
        // Fallback (shouldn't reach here): empty panel.
        let block = Block::default().borders(borders);
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" Lv.{}に到達 — スキルを1つ選んで習得", state.level),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " ※ もう一方は今回の冒険では習得できない",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    for (i, skill) in [left, right].iter().enumerate() {
        let info = skill_info(*skill);
        let bracket = if i == 0 { "[1]" } else { "[2]" };
        let target_id = if i == 0 {
            super::actions::SKILL_CHOICE_LEFT
        } else {
            super::actions::SKILL_CHOICE_RIGHT
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    format!(" {} ", bracket),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    info.name,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" (MP{})", info.mp_cost),
                    Style::default().fg(Color::Blue),
                ),
            ]),
            target_id,
        );
        cl.push(Line::from(Span::styled(
            format!("     {}", info.description),
            Style::default().fg(Color::Gray),
        )));
        cl.push(Line::from(""));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " スキル習得 ",
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
        .title(
            Line::from(Span::styled(
                " \u{2605} DUNGEON CLEAR \u{2605} ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::rpg::dungeon_map::generate_map;
    use crate::games::rpg::state::Monster;

    fn adjacent_monster(map: &DungeonMap, awake: bool) -> Monster {
        Monster {
            kind: super::super::state::EnemyKind::Slime,
            x: map.player_x + 1,
            y: map.player_y,
            hp: 5,
            max_hp: 5,
            awake,
            charging: false,
            affix: None,
        }
    }

    #[test]
    fn radar_height_for_reserves_the_actual_required_info_rows() {
        // 7 (radar) + required_info_rows が info_h を超えるなら出さない。
        assert_eq!(radar_height_for(false, 15, 20, 9), 0, "境界未満は0");
        assert_eq!(radar_height_for(false, 16, 20, 9), 7, "境界ちょうどなら出る");
        // 折返しで必要行数が増えた場合も同じ式で反映される (Codexレビュー指摘)。
        assert_eq!(
            radar_height_for(false, 16, 20, 12), 0,
            "実測の必要行数が増えれば同じinfo_hでも出さなくなる"
        );
        assert_eq!(radar_height_for(false, 19, 20, 12), 7);
    }

    #[test]
    fn radar_height_for_is_zero_in_overworld_or_narrow_width() {
        assert_eq!(radar_height_for(true, 30, 20, 0), 0, "村では出さない");
        assert_eq!(radar_height_for(false, 30, 8, 0), 0, "幅9未満では出さない");
    }

    #[test]
    fn square_radar_area_centers_a_square_in_a_wide_rect() {
        let area = Rect::new(0, 10, 48, 7);
        let squared = square_radar_area(area);

        assert_eq!(squared.height, 7, "高さはそのまま");
        assert_eq!(squared.width, 14, "幅 = 高さ*2 に収まるはず (7*2)");
        assert_eq!(squared.x, 17, "中央寄せ: (48-14)/2 = 17");
        assert_eq!(squared.y, area.y);
    }

    #[test]
    fn square_radar_area_is_noop_when_already_narrow_enough() {
        let area = Rect::new(5, 0, 10, 7);
        let squared = square_radar_area(area);

        assert_eq!(squared, area, "幅が既に高さ*2以下なら切り詰めない");
    }

    #[test]
    fn radar_blips_includes_awake_visible_monster_within_range() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let mut map = map;
        map.monsters = vec![adjacent_monster(&map, true)];
        let visible = dungeon_view::compute_visibility(&map);

        let blips = radar_blips(&map, &visible);

        assert_eq!(blips.len(), 1, "視界内・awakeな隣接モンスターはレーダーに映るはず");
        assert_eq!(blips[0].2, Color::Red);
    }

    #[test]
    fn radar_blips_excludes_sleeping_monster() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let mut map = map;
        map.monsters = vec![adjacent_monster(&map, false)];
        let visible = dungeon_view::compute_visibility(&map);

        let blips = radar_blips(&map, &visible);

        assert!(blips.is_empty(), "まだ気付いていない (awake=false) モンスターは映さない");
    }

    #[test]
    fn radar_blips_excludes_monster_outside_visible_set() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let mut map = map;
        let mut m = adjacent_monster(&map, true);
        // 視界の外 (マップ端の遠方) に置き直す。
        m.x = 0;
        m.y = 0;
        map.monsters = vec![m];
        let visible = dungeon_view::compute_visibility(&map);

        let blips = radar_blips(&map, &visible);

        assert!(blips.is_empty(), "視界外のモンスターは検知半径内でも映さない");
    }

    #[test]
    fn radar_blips_colors_elite_magenta_and_charging_light_red() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let mut map = map;
        let mut elite = adjacent_monster(&map, true);
        elite.affix = Some(super::super::state::EnemyAffix::Swift);
        let mut charging = adjacent_monster(&map, true);
        charging.y = map.player_y.wrapping_sub(1).min(map.height - 1);
        charging.charging = true;
        map.monsters = vec![elite, charging];
        let visible = dungeon_view::compute_visibility(&map);

        let blips = radar_blips(&map, &visible);

        assert!(blips.iter().any(|b| b.2 == Color::Magenta), "affix持ちはマゼンタ");
        assert!(blips.iter().any(|b| b.2 == Color::LightRed), "チャージ中は明赤");
    }

    #[test]
    fn log_style_flags_death_and_danger_as_red() {
        assert_eq!(log_style("力尽きた… 30G失った").fg, Some(Color::Red));
        assert_eq!(log_style("飢餓寸前！何か食べないと…").fg, Some(Color::Red));
        assert_eq!(log_style("…神は応えなかった。心に虚しさが残る…").fg, Some(Color::Red));
    }

    #[test]
    fn log_style_flags_kills_and_gains_as_green() {
        assert_eq!(log_style("スライムを倒した！ EXP+5 +8G").fg, Some(Color::Green));
        assert_eq!(log_style("薬草をドロップ！").fg, Some(Color::Green));
        assert_eq!(log_style("薬草x3 / パンx2 / 50G を受け取った！").fg, Some(Color::Green));
    }

    #[test]
    fn log_style_flags_growth_as_yellow() {
        assert_eq!(log_style("レベルアップ！ Lv.2").fg, Some(Color::Yellow));
        assert_eq!(log_style("スキル「ヒール」を習得！").fg, Some(Color::Yellow));
        assert_eq!(log_style("会心の一撃！ ゴブリンに12ダメージ").fg, Some(Color::Yellow));
    }

    #[test]
    fn log_style_defaults_to_gray_for_ordinary_lines() {
        assert_eq!(log_style("ゴブリンに8ダメージ").fg, Some(Color::DarkGray));
        assert_eq!(log_style("壁だ。進めない。").fg, Some(Color::DarkGray));
    }

    #[test]
    fn floor_color_escalates_with_depth() {
        // 村 (floor 0) は既存の見た目 (Cyan) を維持する。
        assert_eq!(floor_color(0), Color::Cyan);
        // 深く潜るほど色が変わっていく (段階が全て異なることだけ確認する —
        // 具体的な配色はデザイン判断であってテストで固定すべき仕様ではない)。
        let colors: Vec<Color> = (1..=super::super::state::MAX_FLOOR)
            .map(floor_color)
            .collect();
        let unique: std::collections::HashSet<Color> = colors.iter().copied().collect();
        assert!(unique.len() > 1, "階層が進むと色も変わるはず");
    }
}
