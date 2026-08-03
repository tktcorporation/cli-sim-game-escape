//! 常夜灯 — 描画 (読み取り専用)。
//!
//! 戦場は `ratatui::widgets::canvas::Canvas` + `Marker::Braille` の疑似
//! ピクセルで連続座標のまま描く (`rpg` のレーダー・`loopmarch` の推移
//! グラフと同じ手法)。タップ移動は同じ領域に `ClickableGrid` を重ねて
//! 実現する — 別DOM要素を生やさず、同じ `<pre>` 上に描画とヒット判定を
//! 両立させる規約 (ARCHITECTURE.md) に従う。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::games::GameChoice;
use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{Clickable, ClickableGrid, ClickableList, ScrollableTab};

use super::actions;
use super::logic;
use super::state::{
    BoonKind, CampUpgrades, EnemyKind, EverlightState, Phase, WeaponKind, BREACH_Y, COLUMNS,
    LANTERN_Y, SPAWN_Y, WORLD_H, WORLD_W,
};

pub fn render(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    match state.phase {
        Phase::Camp => render_camp(state, f, area, click_state),
        Phase::Vigil => render_vigil(state, f, area, click_state),
    }
}

fn format_survival(ticks: u64) -> String {
    let secs = ticks / 10;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// ワールドy座標 (0=湧き出し端/上, WORLD_H=防衛線/下) を、Canvas の数学的
/// y座標 (上が正) に変換する。x座標は反転不要 (左右はそのまま)。
fn world_to_canvas_y(world_y: f64) -> f64 {
    WORLD_H - world_y
}

// ── 夜番 (Vigil) 画面 ────────────────────────────────────────────

struct VigilLayout {
    header: Rect,
    battlefield: Rect,
    side: Option<Rect>,
}

fn compute_vigil_layout(area: Rect) -> VigilLayout {
    let is_narrow = is_narrow_layout(area.width);
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(10)])
        .split(area);
    let header = vchunks[0];
    if is_narrow {
        VigilLayout { header, battlefield: vchunks[1], side: None }
    } else {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(24)])
            .split(vchunks[1]);
        VigilLayout { header, battlefield: hchunks[0], side: Some(hchunks[1]) }
    }
}

fn render_vigil(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let layout = compute_vigil_layout(area);
    render_header(state, f, layout.header, click_state);
    render_battlefield(state, f, layout.battlefield, click_state);
    if let Some(side) = layout.side {
        render_side_panel(state, f, side);
    }
    if state.pending_boons.is_some() {
        render_boon_modal(state, f, layout.battlefield, click_state);
    }
}

fn render_header(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let borders = if is_narrow_layout(area.width) { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };
    let ratio = if state.lantern.light_max > 0 {
        state.lantern.light as f64 / state.lantern.light_max as f64
    } else {
        0.0
    };
    let bar = theme::hp_bar_string(ratio, 16);
    let bar_color = theme::hp_ratio_color(ratio);

    let mut line1 = vec![
        Span::styled("灯 ", Style::default().fg(Color::LightYellow)),
        Span::styled(bar, Style::default().fg(bar_color)),
        Span::styled(
            format!(" {}/{}", state.lantern.light.max(0), state.lantern.light_max),
            Style::default().fg(Color::White),
        ),
    ];
    if let Some((dmg, _)) = state.last_light_damage {
        line1.push(Span::styled(
            format!(" -{dmg}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    let line2 = Line::from(vec![
        Span::styled(format!("第{}波", state.wave), Style::default().fg(Color::LightMagenta)),
        Span::raw("  "),
        Span::styled(format!("生存 {}", format_survival(state.elapsed_ticks)), Style::default().fg(Color::Gray)),
        Span::raw("  "),
        Span::styled(format!("残光 {}", state.ember), Style::default().fg(Color::LightYellow)),
    ]);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(theme::accent(&GameChoice::Everlight)))
        .title(" 常夜灯 ");
    let inner = block.inner(area);
    // 撤退ボタンは灯/波数などの可変長テキストと同じ行に重ね描きしない。
    // 専用の3行目を空けておき、そこへオーバーレイすることで、灯の残量が
    // 3桁になったりダメージポップアップが出た時に文字が欠けるのを防ぐ。
    let widget = Paragraph::new(vec![Line::from(line1), line2, Line::from("")]).block(block);
    f.render_widget(widget, area);

    if inner.width >= 4 && inner.height >= 3 {
        let retreat_area = Rect::new(inner.x + inner.width - 4, inner.y + 2, 4, 1);
        let para = Paragraph::new(Span::styled("撤退", Style::default().fg(Color::DarkGray)));
        let mut cs = click_state.borrow_mut();
        Clickable::new(para, actions::RETREAT_TO_CAMP).render(f, retreat_area, &mut cs);
    }
}

fn battlefield_block(is_narrow: bool) -> Block<'static> {
    let borders = if is_narrow { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };
    Block::default()
        .borders(borders)
        .border_style(Style::default().fg(theme::accent(&GameChoice::Everlight)))
        .title(" 戦場 ")
}

fn render_battlefield(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let block = battlefield_block(is_narrow);
    let inner = block.inner(area);

    if inner.width > 0 && inner.height > 0 {
        let cell_w = (inner.width / COLUMNS as u16).max(1);
        let mut cs = click_state.borrow_mut();
        ClickableGrid::new(COLUMNS, 1, actions::LANE_CLICK_BASE, cell_w)
            .with_cell_height(inner.height)
            .register_targets(area, &block, &mut cs, 0);
    }

    // ── ワールド座標 → Canvas 描画データを事前に組み立てる (paintクロージャに move する) ──
    let light_ratio = if state.lantern.light_max > 0 {
        (state.lantern.light as f64 / state.lantern.light_max as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lantern_radius = 2.0 + light_ratio * 1.6;
    let lantern_color = if state.lantern_hurt_flash.is_active() { Color::White } else { Color::LightYellow };
    let lantern_glow = canvas_fx::filled_ellipse_points(
        state.lantern.x,
        world_to_canvas_y(LANTERN_Y),
        lantern_radius,
        lantern_radius,
        0.8,
    );

    const ENEMY_KINDS: [EnemyKind; 5] =
        [EnemyKind::Wisp, EnemyKind::Husk, EnemyKind::Swarmling, EnemyKind::Elite, EnemyKind::Boss];
    let mut enemy_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    for kind in ENEMY_KINDS {
        let pts: Vec<(f64, f64)> = state
            .enemies
            .iter()
            .filter(|e| e.kind == kind)
            .flat_map(|e| {
                let r = (e.kind.radius() * 0.55).max(0.5);
                canvas_fx::filled_ellipse_points(e.x, world_to_canvas_y(e.y), r, r, 0.9)
            })
            .collect();
        if !pts.is_empty() {
            enemy_groups.push((pts, kind.color()));
        }
    }
    let hurt_points: Vec<(f64, f64)> = state
        .enemies
        .iter()
        .filter(|e| e.hurt_flash.is_active())
        .map(|e| (e.x, world_to_canvas_y(e.y)))
        .collect();

    let mut projectile_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    for &weapon_kind in WeaponKind::all() {
        let color = weapon_kind.color();
        let pts: Vec<(f64, f64)> = state
            .projectiles
            .iter()
            .filter(|p| p.color == color)
            .map(|p| (p.x, world_to_canvas_y(p.y)))
            .collect();
        if !pts.is_empty() {
            projectile_groups.push((pts, color));
        }
    }

    let chest_pts: Vec<(f64, f64)> = state
        .chests
        .iter()
        .flat_map(|c| canvas_fx::filled_ellipse_points(c.x, world_to_canvas_y(c.y), 1.6, 1.6, 0.9))
        .collect();

    let telegraph_line = state
        .boss_telegraph
        .map(|(x, _)| (x, world_to_canvas_y(SPAWN_Y), x, world_to_canvas_y(BREACH_Y)));

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if let Some((x1, y1, x2, y2)) = telegraph_line {
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: Color::Red });
            }
            for (pts, color) in &enemy_groups {
                ctx.draw(&Points { coords: pts, color: *color });
            }
            if !hurt_points.is_empty() {
                ctx.draw(&Points { coords: &hurt_points, color: Color::White });
            }
            for (pts, color) in &projectile_groups {
                ctx.draw(&Points { coords: pts, color: *color });
            }
            if !chest_pts.is_empty() {
                ctx.draw(&Points { coords: &chest_pts, color: Color::LightYellow });
            }
            if !lantern_glow.is_empty() {
                ctx.draw(&Points { coords: &lantern_glow, color: lantern_color });
            }
        })
        .block(block);
    f.render_widget(canvas, area);

    // ログのポップ表示 (無い間は操作ヒントを常設表示する)。
    if inner.height > 0 {
        let toast_area = Rect::new(inner.x, inner.y, inner.width, 1);
        let (text, style) = match state.visible_log() {
            Some(msg) => (msg.to_string(), Style::default().fg(Color::Black).bg(Color::LightYellow)),
            None => ("タップで灯を移動".to_string(), Style::default().fg(Color::DarkGray)),
        };
        let para = Paragraph::new(Line::from(Span::styled(format!(" {text} "), style))).alignment(Alignment::Center);
        f.render_widget(para, toast_area);
    }

    // 装備アイコン行 (最下段)。
    if inner.height > 1 {
        let loadout_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        let text = loadout_summary_text(state);
        if !text.is_empty() {
            let para = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(Color::DarkGray))))
                .alignment(Alignment::Center);
            f.render_widget(para, loadout_area);
        }
    }
}

fn loadout_summary_text(state: &EverlightState) -> String {
    state
        .loadout
        .weapons
        .iter()
        .map(|w| {
            let name = if w.evolved { w.kind.evolved_name() } else { w.kind.name() };
            format!("{name}Lv{}", w.level)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_side_panel(state: &EverlightState, f: &mut Frame, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        " 装備",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ))];
    if state.loadout.weapons.is_empty() {
        lines.push(Line::from(" (なし)"));
    }
    for w in &state.loadout.weapons {
        let name = if w.evolved { w.kind.evolved_name() } else { w.kind.name() };
        lines.push(Line::from(Span::styled(
            format!(" {name} Lv{}", w.level),
            Style::default().fg(w.kind.color()),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 効果",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    if state.loadout.passives.is_empty() {
        lines.push(Line::from(" (なし)"));
    }
    // 受動効果の色はそれと組み合う武器と同じ色にしてある — 「なぜこの2つが
    // 同じ色なんだろう」から進化レシピへ気付いてもらうための伏線。
    for p in &state.loadout.passives {
        lines.push(Line::from(Span::styled(
            format!(" {} Lv{}", p.kind.name(), p.level),
            Style::default().fg(p.kind.color()),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" 撃破 {}", state.kill_count),
        Style::default().fg(Color::Gray),
    )));
    lines.push(Line::from(Span::styled(
        format!(" 自己最高 第{}波", state.best_wave),
        Style::default().fg(Color::Gray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 状況 ");
    let widget = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_boon_modal(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let Some(options) = state.pending_boons else {
        return;
    };

    let modal_w = area.width.saturating_sub(2).max(1);
    let modal_h = area.height.min(3 * options.len() as u16 + 5);
    let modal_area = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 宝箱を見つけた！強化を選ぼう",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    for (i, opt) in options.iter().enumerate() {
        let (title, detail) = logic::boon_option_text(state, opt.kind);
        let action_id = actions::BOON_OPTION_BASE + i as u16;
        cl.push_clickable(
            Line::from(Span::styled(
                format!(" ▶ {title}"),
                Style::default().fg(boon_accent_color(opt.kind)).add_modifier(Modifier::BOLD),
            )),
            action_id,
        );
        cl.push_clickable(
            Line::from(Span::styled(format!("    {detail}"), Style::default().fg(Color::DarkGray))),
            action_id,
        );
        cl.push(Line::from(""));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightYellow))
        .title(" 灯を強化する ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, modal_area, block, &mut cs, true, 0);
}

/// 選択肢に紐づく武器/受動効果の色。武器とその進化相方の受動効果は
/// 同じ色を返す (`WeaponKind::color` / `PassiveKind::color` 参照) ので、
/// モーダル上で並んだ時に色の一致が視覚的なヒントになる。
fn boon_accent_color(kind: BoonKind) -> Color {
    match kind {
        BoonKind::NewWeapon(k) | BoonKind::LevelWeapon(k) | BoonKind::Evolve(k) => k.color(),
        BoonKind::NewPassive(k) | BoonKind::LevelPassive(k) => k.color(),
        BoonKind::InstantHeal | BoonKind::EmberWindfall => Color::LightYellow,
    }
}

// ── 拠点 (Camp) 画面 ────────────────────────────────────────────

fn render_camp(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    let title = Paragraph::new(Line::from(Span::styled(
        "拠点 — 常夜灯",
        Style::default().fg(theme::accent(&GameChoice::Everlight)).add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(theme::accent(&GameChoice::Everlight))),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    render_camp_body(state, f, chunks[1], click_state, borders);
}

fn render_camp_body(
    state: &EverlightState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" 残光 {}", state.ember),
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    cl.push_clickable(
        Line::from(Span::styled(
            " ▶ 夜番へ出る",
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
        )),
        actions::CAMP_START_VIGIL,
    );
    cl.push(Line::from(Span::styled(
        "    灯を持って魔物の群れを迎え撃つ",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    cl.push(Line::from(Span::styled(
        " 恒久強化",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    push_upgrade_row(
        &mut cl,
        "灯心",
        &format!("最大灯 +12 (現在 {})", state.camp.light_max()),
        state.camp.light_level,
        state.camp.light_cost(),
        state.ember,
        actions::CAMP_UPGRADE_LIGHT,
        state.camp.light_cost_per_point(),
    );
    push_upgrade_row(
        &mut cl,
        "光力",
        "夜番開始時の全武器威力 +5%",
        state.camp.power_level,
        state.camp.power_cost(),
        state.ember,
        actions::CAMP_UPGRADE_POWER,
        state.camp.power_cost_per_point(),
    );

    cl.push(Line::from(""));
    if state.camp.extra_slot_level >= 1 {
        cl.push(Line::from(Span::styled(
            " ✓ 受動効果スロット拡張 (5枠) 解放済み",
            Style::default().fg(Color::Green),
        )));
    } else {
        let affordable = state.ember >= CampUpgrades::EXTRA_SLOT_COST;
        let color = if affordable { Color::LightCyan } else { Color::DarkGray };
        cl.push_clickable(
            Line::from(Span::styled(
                format!(" 受動効果スロット拡張 (5枠目解放) — {}残光", CampUpgrades::EXTRA_SLOT_COST),
                Style::default().fg(color),
            )),
            actions::CAMP_UPGRADE_EXTRA_SLOT,
        );
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 戦績",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(format!(
        " 自己最高: 第{}波 / 生存 {}",
        state.best_wave,
        format_survival(state.best_survival_ticks)
    )));
    cl.push(Line::from(""));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 拠点 ");
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(cl, &state.camp_scroll, actions::CAMP_SCROLL_UP, actions::CAMP_SCROLL_DOWN)
        .block(block)
        .wrap(true)
        .arrow_color(Color::Green)
        .render(f, area, &mut cs);
}

#[allow(clippy::too_many_arguments)]
fn push_upgrade_row(
    cl: &mut ClickableList,
    name: &str,
    effect: &str,
    level: u32,
    cost: u32,
    ember: u32,
    action_id: u16,
    cost_per_point: f64,
) {
    let affordable = ember >= cost;
    let color = if affordable { Color::LightCyan } else { Color::DarkGray };
    let spans = vec![
        Span::styled(
            format!(" {name} Lv{level}→{}", level + 1),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  {cost}残光"), Style::default().fg(color)),
        Span::styled(format!(" ({cost_per_point:.1}/pt)"), Style::default().fg(Color::DarkGray)),
    ];
    cl.push_clickable(Line::from(spans), action_id);
    cl.push(Line::from(Span::styled(format!("    {effect}"), Style::default().fg(Color::DarkGray))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    fn render_to_test_backend(state: &EverlightState, width: u16, height: u16) {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = width;
        cs.borrow_mut().terminal_rows = height;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                render(state, f, f.area(), &cs);
            })
            .unwrap();
    }

    #[test]
    fn camp_renders_without_panicking_narrow_and_wide() {
        let state = EverlightState::new();
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn vigil_renders_without_panicking_with_enemies_and_projectiles() {
        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        logic::tick_n(&mut state, 100);
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn boon_modal_renders_without_panicking() {
        use super::super::state::{BoonKind, BoonOption};

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.pending_boons = Some([
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Spray) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Aurora) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Halo) },
        ]);
        render_to_test_backend(&state, 40, 30);
    }

    #[test]
    fn tap_on_lane_registers_move_target() {
        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = 40;
        cs.borrow_mut().terminal_rows = 30;
        let mut terminal = Terminal::new(TestBackend::new(40, 30)).unwrap();
        terminal
            .draw(|f| {
                render(&state, f, f.area(), &cs);
            })
            .unwrap();
        // 戦場内のどこかは必ずレーンのクリックターゲットとしてヒットするはず。
        let hit = cs.borrow().hit_test(20, 15);
        assert!(hit.is_some());
    }
}
