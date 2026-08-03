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
    ENEMY_BULLET_RADIUS, LANE_HALF_WIDTH, LANTERN_Y, SPAWN_Y, WORLD_H, WORLD_W,
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

        // inner.width が COLUMNS (9) で割り切れない時、床除算した cell_w
        // では右端に余りぶんの列が残り、どのレーンのクリック領域にも
        // 属さずタップが無反応になる。最終レーンの領域を、余りぶんも
        // 含めて右端まで確実に覆うよう見えない当たり判定を重ねる
        // (後から登録した方が優先されるルールを利用する)。
        let covered = cell_w * COLUMNS as u16;
        if covered < inner.width {
            let remainder_area = Rect::new(inner.x + covered, inner.y, inner.width - covered, inner.height);
            let last_lane_action = actions::LANE_CLICK_BASE + COLUMNS as u16 - 1;
            Clickable::new(Block::default(), last_lane_action).render(f, remainder_area, &mut cs);
        }
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

    const ENEMY_KINDS: [EnemyKind; 15] = [
        EnemyKind::Wisp,
        EnemyKind::Husk,
        EnemyKind::Swarmling,
        EnemyKind::Elite,
        EnemyKind::Boss,
        EnemyKind::Sniper,
        EnemyKind::Caster,
        EnemyKind::Shielded,
        EnemyKind::Splitter,
        EnemyKind::SprayShielded,
        EnemyKind::AuroraShielded,
        EnemyKind::Charger,
        EnemyKind::ShadowWitch,
        EnemyKind::Serpent,
        EnemyKind::FullMoonBoss,
    ];
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

    let enemy_bullet_pts: Vec<(f64, f64)> = state
        .enemy_bullets
        .iter()
        .flat_map(|b| {
            canvas_fx::filled_ellipse_points(b.x, world_to_canvas_y(b.y), ENEMY_BULLET_RADIUS, ENEMY_BULLET_RADIUS, 0.9)
        })
        .collect();

    // 影の魔女/満月の魔王は2レーン同時に警告するので、線も複数本引く。
    let telegraph_lines: Vec<(f64, f64, f64, f64)> = state
        .boss_telegraph
        .iter()
        .flat_map(|t| t.lane_xs.iter())
        .map(|&x| (x, world_to_canvas_y(SPAWN_Y), x, world_to_canvas_y(BREACH_Y)))
        .collect();

    // 極光は即着弾のヒットスキャンで実体弾を撃たない (弾道が無い) ため、
    // 命中の有無によらず発火した帯を一瞬描かないと「取っても強化しても
    // 何も起きていないように見える」演出の空白になる。`aurora_flash` が
    // 立っている間だけ、現在の判定幅 (`aurora_width_mult`) そのままの帯を描く。
    // 帯の中心は現在の`state.lantern.x`ではなく`aurora_flash_x`(発火時に
    // 実際に判定した位置のスナップショット)を使う — 現在位置だと、発火後に
    // 灯が動いた分だけ実際に判定したレーンとズレて表示されてしまう。
    let aurora_band_pts: Vec<(f64, f64)> = if state.aurora_flash.is_active() {
        state
            .loadout
            .weapon(WeaponKind::Aurora)
            .map(|w| {
                let half_width = LANE_HALF_WIDTH * w.aurora_width_mult();
                let x0 = state.aurora_flash_x - half_width;
                let x1 = state.aurora_flash_x + half_width;
                canvas_fx::filled_rect_points(x0, SPAWN_Y, x1, BREACH_Y, 3.0)
                    .into_iter()
                    .map(|(x, y)| (x, world_to_canvas_y(y)))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // 流星もAurora同様、即着弾のヒットスキャンで実体弾を撃たない。着弾の
    // 事実を見せるため、`meteor_flash` が立っている間だけ現在の判定半径
    // (`meteor_radius`) そのままのリングを `meteor_flash_pos` (発火時の
    // スナップショット位置) に描く。
    let meteor_ring_pts: Vec<(f64, f64)> = if state.meteor_flash.is_active() {
        state
            .loadout
            .weapon(WeaponKind::Meteor)
            .map(|w| {
                let (cx, cy) = state.meteor_flash_pos;
                canvas_fx::ring_points(cx, world_to_canvas_y(cy), w.meteor_radius(), 0.3)
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // 光輪は近接の常時判定武器で、Aurora同様に実体弾を撃たない。判定半径
    // (`halo_radius`) そのままのリングを常時描き、周回する光点で「回転して
    // いる」ことを視覚的に伝える。常時リングは常に現在の`lantern.x`を
    // 追従してよい (発火位置ではなく「今の判定範囲」を示すものなので)。
    //
    // Aurora の `aurora_flash` のような「発火した瞬間だけ光る」演出は
    // 意図的に持たせていない — 光輪のクールダウンは最短5tick (進化・
    // 速射パッシブでさらに短縮可能) で、まとめtick処理での見逃しを防ぐ
    // のに必要な最短表示時間 (5tick、AURORA_FLASH_TICKS参照) を常に
    // 下回るか同じになる。そのタイマーで一瞬だけ光らせる設計にすると、
    // 次の発火が前の発火の表示が消える前後に必ず来て実質常時点灯になり、
    // かつ発火の度に灯の現在位置へワープする不自然な見た目になる。
    // 発火頻度が高い光輪は、この常時リング+回転する光点だけで武器の
    // 存在と強化 (半径の拡大) を伝えるのに十分な視覚フィードバックになる。
    let halo_visual = state.loadout.weapon(WeaponKind::Halo).map(|w| {
        let radius = w.halo_radius();
        let cx = state.lantern.x;
        let cy = world_to_canvas_y(LANTERN_Y);
        let ring = canvas_fx::ring_points(cx, cy, radius, 0.2);
        const SPARK_COUNT: usize = 3;
        let spin_angle = state.elapsed_ticks as f64 * 0.12;
        let sparks: Vec<(f64, f64)> = (0..SPARK_COUNT)
            .flat_map(|i| {
                let a = spin_angle + i as f64 * std::f64::consts::TAU / SPARK_COUNT as f64;
                let sx = cx + a.cos() * radius;
                let sy = cy + a.sin() * radius;
                canvas_fx::filled_ellipse_points(sx, sy, 1.1, 1.1, 0.6)
            })
            .collect();
        (ring, sparks)
    });

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for &(x1, y1, x2, y2) in &telegraph_lines {
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: Color::Red });
            }
            if !aurora_band_pts.is_empty() {
                // 極光の武器色そのまま (LightYellow) だと灯・宝箱の発光と
                // 同色で紛れるため、帯は暗めの Yellow にして区別できるようにする。
                ctx.draw(&Points { coords: &aurora_band_pts, color: Color::Yellow });
            }
            if let Some((ring, _)) = &halo_visual {
                if !ring.is_empty() {
                    ctx.draw(&Points { coords: ring, color: Color::Magenta });
                }
            }
            if !meteor_ring_pts.is_empty() {
                ctx.draw(&Points { coords: &meteor_ring_pts, color: Color::LightRed });
            }
            for (pts, color) in &enemy_groups {
                ctx.draw(&Points { coords: pts, color: *color });
            }
            if !hurt_points.is_empty() {
                ctx.draw(&Points { coords: &hurt_points, color: Color::White });
            }
            if let Some((_, sparks)) = &halo_visual {
                if !sparks.is_empty() {
                    ctx.draw(&Points { coords: sparks, color: Color::LightMagenta });
                }
            }
            for (pts, color) in &projectile_groups {
                ctx.draw(&Points { coords: pts, color: *color });
            }
            if !chest_pts.is_empty() {
                ctx.draw(&Points { coords: &chest_pts, color: Color::LightYellow });
            }
            if !enemy_bullet_pts.is_empty() {
                ctx.draw(&Points { coords: &enemy_bullet_pts, color: Color::Cyan });
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

    push_rank_selector(&mut cl, state);
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

    if state.camp.extra_weapon_slot_level >= 1 {
        cl.push(Line::from(Span::styled(
            " ✓ 武器スロット拡張 (5枠) 解放済み",
            Style::default().fg(Color::Green),
        )));
    } else {
        let affordable = state.ember >= CampUpgrades::EXTRA_WEAPON_SLOT_COST;
        let color = if affordable { Color::LightCyan } else { Color::DarkGray };
        cl.push_clickable(
            Line::from(Span::styled(
                format!(" 武器スロット拡張 (5枠目解放) — {}残光", CampUpgrades::EXTRA_WEAPON_SLOT_COST),
                Style::default().fg(color),
            )),
            actions::CAMP_UPGRADE_EXTRA_WEAPON_SLOT,
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

/// 挑戦ランクの選択行と、選択中ランクの目標 (最終波・最終ボス名) を
/// 積む。「次に何を目指すか」を拠点画面で常に見えるようにする。
fn push_rank_selector(cl: &mut ClickableList, state: &EverlightState) {
    let selected = state.camp.effective_selected_rank();
    let max_unlocked = state.camp.max_unlocked_rank.max(1);

    cl.push(Line::from(Span::styled(
        " 挑戦ランク",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(format!(" 第{selected}夜  (解放済み: 第{max_unlocked}夜まで)")));

    let down_color = if selected > 1 { Color::LightCyan } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(Span::styled(" ◀ 前の夜へ", Style::default().fg(down_color))),
        actions::CAMP_RANK_DOWN,
    );
    let up_color = if selected < max_unlocked { Color::LightCyan } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(Span::styled(" ▶ 次の夜へ", Style::default().fg(up_color))),
        actions::CAMP_RANK_UP,
    );

    let milestone = logic::milestone_wave(selected);
    let boss_name = logic::boss_kind_for(milestone, selected).name();
    cl.push(Line::from(Span::styled(
        format!(" 目標: 第{milestone}波『{boss_name}』を討伐する"),
        Style::default().fg(Color::LightGreen),
    )));
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
    fn vigil_renders_without_panicking_with_aurora_and_halo_active() {
        use super::super::state::OwnedWeapon;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Aurora));
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Halo));
        // 極光の薙ぎ払い帯は`aurora_flash`が立っている間だけ描くコードパス
        // なので、実際の発火tickを計算せずフラグを直接立てて確実に通す
        // (光輪は常時描画なので装備するだけでコードパスを通る)。
        state.aurora_flash.trigger(1);
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn vigil_renders_without_panicking_with_meteor_flash_active() {
        use super::super::state::OwnedWeapon;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Meteor));
        state.meteor_flash.trigger(1);
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn vigil_renders_without_panicking_with_a_caster_and_enemy_bullets() {
        use super::super::state::EnemyBullet;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.enemy_bullets.push(EnemyBullet { x: state.lantern.x, y: 30.0, vx: 0.0, vy: 2.2, damage: 4 });
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

    #[test]
    fn tap_in_remainder_column_still_hits_the_last_lane() {
        // 40幅はCOLUMNS(9)で割り切れない (40/9=4余り4)。床除算した
        // cell_wだけで登録すると右端4列がどのレーンにも属さずタップ
        // 無反応になっていた (すり抜けバグの回帰テスト)。
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
        // 戦場右端ぎりぎり (x=39) は余り列に入るはず。
        let hit = cs.borrow().hit_test(39, 15);
        assert_eq!(
            hit,
            Some(actions::LANE_CLICK_BASE + COLUMNS as u16 - 1),
            "右端の余り列も最終レーンのタップとして扱われるはず"
        );
    }
}
