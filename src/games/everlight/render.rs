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
use ratzilla::ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::games::GameChoice;
use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{Clickable, ClickableGrid, ClickableList, ScrollableTab, TabBar};

use super::actions;
use super::logic;
use super::state::{
    BoonKind, CampTab, CampUpgrades, EnemyKind, EverlightState, LanternType, Phase, WeaponKind,
    BREACH_Y, COLUMNS, ENEMY_BULLET_RADIUS, KILL_EFFECT_TICKS, LANE_HALF_WIDTH, LANTERN_Y, SPAWN_Y,
    WORLD_H, WORLD_W,
};

/// 燠火の色。情景パネルの熾火 (`render_camp_ambience`) と拠点画面の点描
/// 区切り (`ember_divider_line`) で共有し、「地の炎」の質感を両方に一貫させる。
const EMBER_COLOR: Color = Color::Rgb(120, 90, 40);

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

/// 戦場に描画する敵種の一覧。`EnemyKind::all()` と食い違うと、新種を
/// 追加した時に「湧いているのに一切描画されない」不具合になる —
/// `enemy_kinds_render_list_covers_every_enemy_kind` テストで一致を検証する。
const ENEMY_KINDS: [EnemyKind; 17] = [
    EnemyKind::Wisp,
    EnemyKind::Husk,
    EnemyKind::Swarmling,
    EnemyKind::Elite,
    EnemyKind::Boss,
    EnemyKind::Sniper,
    EnemyKind::Caster,
    EnemyKind::Wraith,
    EnemyKind::Shielded,
    EnemyKind::Splitter,
    EnemyKind::SprayShielded,
    EnemyKind::AuroraShielded,
    EnemyKind::Charger,
    EnemyKind::ShadowWitch,
    EnemyKind::Serpent,
    EnemyKind::FullMoonBoss,
    EnemyKind::Brute,
];

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

    // 敵を討った位置に一瞬だけ残す小さな爆破演出。`ticks_left`が
    // `KILL_EFFECT_TICKS`から0へ減るのに合わせてリングを広げ、消える瞬間に
    // 最大サイズになる「弾けた」見た目にする。
    const KILL_EFFECT_MIN_RADIUS: f64 = 0.6;
    const KILL_EFFECT_MAX_RADIUS: f64 = 2.6;
    let kill_effect_pts: Vec<(f64, f64)> = state
        .kill_effects
        .iter()
        .flat_map(|e| {
            let progress = 1.0 - (e.ticks_left as f64 / KILL_EFFECT_TICKS as f64);
            let radius = KILL_EFFECT_MIN_RADIUS + (KILL_EFFECT_MAX_RADIUS - KILL_EFFECT_MIN_RADIUS) * progress;
            canvas_fx::ring_points(e.x, world_to_canvas_y(e.y), radius, 0.6)
        })
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

    // 雷光もAurora/流星同様、連鎖の命中そのものはヒットスキャンで弾道が
    // 無い。`chain_flash` が立っている間だけ、実際に連鎖が通過した経路
    // (`chain_flash_points`、起点→末端の順) を線で結んで見せる。
    let chain_lines: Vec<(f64, f64, f64, f64)> = if state.chain_flash.is_active() {
        state
            .chain_flash_points
            .windows(2)
            .map(|w| {
                let (x1, y1) = w[0];
                let (x2, y2) = w[1];
                (x1, world_to_canvas_y(y1), x2, world_to_canvas_y(y2))
            })
            .collect()
    } else {
        Vec::new()
    };
    // 連鎖が1体で終わると`windows(2)`は何も返さず線が引けない。極光/流星の
    // 「発火した事実は必ず見せる」という前提が崩れないよう、単体ヒットの
    // 場合はその1点に光点を描く。
    let chain_single_hit_pts: Vec<(f64, f64)> = if state.chain_flash.is_active() && state.chain_flash_points.len() == 1 {
        let (x, y) = state.chain_flash_points[0];
        canvas_fx::filled_ellipse_points(x, world_to_canvas_y(y), 1.6, 1.6, 0.5)
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
            for &(x1, y1, x2, y2) in &chain_lines {
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: WeaponKind::Chain.color() });
            }
            if !chain_single_hit_pts.is_empty() {
                ctx.draw(&Points { coords: &chain_single_hit_pts, color: WeaponKind::Chain.color() });
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
            if !kill_effect_pts.is_empty() {
                ctx.draw(&Points { coords: &kill_effect_pts, color: Color::Red });
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
    // Clear で modal_area を白紙化してから描く。Paragraph はテキストのある
    // セルしか書き換えないため、Clear を挟まないと配下の戦場描画がモーダルの
    // 余白から透けて見える。
    f.render_widget(Clear, modal_area);
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

    let title = Paragraph::new(Line::from(vec![
        Span::styled("⠐⠂ ", Style::default().fg(EMBER_COLOR)),
        Span::styled(
            "拠点 — 常夜灯",
            Style::default().fg(theme::accent(&GameChoice::Everlight)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ⠂⠐", Style::default().fg(EMBER_COLOR)),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(theme::accent(&GameChoice::Everlight))),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    if is_narrow {
        render_camp_body(state, f, chunks[1], click_state, borders);
    } else {
        // ワイドレイアウトのみ右側に情景パネルを置く。ナロー(モバイル)では
        // 画面幅が足りず、リストの可読性を犠牲にしてまで確保する価値が
        // 無いため素通りする。
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(22)])
            .split(chunks[1]);
        render_camp_body(state, f, hchunks[0], click_state, borders);
        render_camp_ambience(state, f, hchunks[1], borders);
    }
    if state.weapon_detail_modal.is_some() {
        render_weapon_detail_modal(state, f, chunks[1], click_state);
    }
}

/// 灯の残光量をこの値で割った比率で情景パネルの灯の大きさを決める。
/// 最も高価な武器解放 (`WeaponKind::Wave` = 200) を賄えるくらい貯まった
/// 時点で「満ちている」と感じられるよう、ゲーム内の実際の価格帯に合わせた。
const EMBER_SCALE_CAP: u32 = 200;

/// 灯のタイプごとの情景パネルの色味。`halo`は輪郭、`glow`はにじみ。
/// 疾風は涼やかな白寄り、守灯は深く赤い熾火、常灯は素の暖色のまま —
/// 拠点で選んだタイプが情景にも表れることで、選択が反映されている実感を持たせる。
fn lantern_ambience_colors(t: LanternType) -> (Color, Color) {
    match t {
        LanternType::Steady => (Color::Rgb(255, 200, 80), Color::Rgb(200, 140, 40)),
        LanternType::Swift => (Color::Rgb(215, 235, 255), Color::Rgb(140, 190, 210)),
        LanternType::Warden => (Color::Rgb(255, 150, 90), Color::Rgb(200, 80, 40)),
    }
}

/// 拠点画面ワイドレイアウトの右側に置く、点々表現の情景パネル。夜番の
/// 戦闘画面と同じ Canvas+Braille の質感を待機画面にも持ち込む。クリック
/// 判定は持たない (常にBlockの背景として完結する) が、残光量で灯の大きさ、
/// 選択中の灯タイプで色味が変わるため、単なる背景装飾ではなく拠点の
/// 現在地を映す一部になっている。
fn render_camp_ambience(state: &EverlightState, f: &mut Frame, area: Rect, borders: Borders) {
    const W: f64 = 40.0;
    const H: f64 = 60.0;
    let cx = W / 2.0;
    let cy = H * 0.42;

    let ember_ratio = state.ember.min(EMBER_SCALE_CAP) as f64 / EMBER_SCALE_CAP as f64;
    let scale = 1.0 + ember_ratio * 0.6;
    let (halo_color, glow_color) = lantern_ambience_colors(state.camp.lantern_type);

    // 灯本体: 中心の明るい核 + 外側ににじむ淡い光の2層 + 輪郭のリング。
    let core = canvas_fx::filled_ellipse_points(cx, cy, 2.2 * scale, 2.2 * scale, 0.5);
    let glow = canvas_fx::filled_ellipse_points(cx, cy, 5.5 * scale, 5.5 * scale, 0.6);
    let halo = canvas_fx::ring_points(cx, cy, 8.0 * scale, 0.15);

    // 立ち上る残り火。座標は毎フレーム同じ (アニメーションはしない) が、
    // 周波数の異なる複数のsin波を重ねることで手作業で散らしたような
    // 自然な配置にする — 単純な等間隔グリッドだと機械的に見えるため。
    let embers: Vec<(f64, f64)> = (0..26)
        .map(|i| {
            let t = i as f64;
            let x = cx + (t * 2.3).sin() * (4.0 + (t * 0.7).cos() * 10.0);
            let y = H - (t * 7.3 + (t * 1.9).sin() * 6.0) % (H - 4.0) - 2.0;
            (x.clamp(1.0, W - 1.0), y)
        })
        .collect();

    let canvas = Canvas::default()
        .x_bounds([0.0, W])
        .y_bounds([0.0, H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points { coords: &embers, color: EMBER_COLOR });
            ctx.draw(&Points { coords: &halo, color: halo_color });
            ctx.draw(&Points { coords: &glow, color: glow_color });
            ctx.draw(&Points { coords: &core, color: Color::LightYellow });
        })
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" 灯 "),
        );
    f.render_widget(canvas, area);
}

/// 武器解放欄でタップした武器の詳細モーダル。`render_boon_modal` と同じ
/// 「選択→モーダル」の作法 (同じ`<pre>`上への上書き描画、別DOM要素は
/// 生やさない) を拠点画面にも揃える。
fn render_weapon_detail_modal(state: &EverlightState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let Some(kind) = state.weapon_detail_modal else {
        return;
    };

    let unlocked = state.camp.is_weapon_unlocked(kind);
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" {}", kind.name()),
        Style::default().fg(kind.color()).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(Span::styled(format!(" {}", kind.summary()), Style::default().fg(Color::Gray))));
    cl.push(Line::from(""));

    if unlocked {
        cl.push(Line::from(Span::styled(" ✓ 解放済み", Style::default().fg(Color::Green))));
    } else if let Some(cost) = kind.unlock_cost() {
        let affordable = state.ember >= cost;
        let color = if affordable { Color::LightGreen } else { Color::DarkGray };
        cl.push_clickable(
            Line::from(Span::styled(format!(" ▶ 解放する — {cost}残光", cost = cost), Style::default().fg(color))),
            actions::CAMP_WEAPON_DETAIL_CONFIRM,
        );
        if !affordable {
            cl.push(Line::from(Span::styled(
                format!("    残光が足りない (所持 {})", state.ember),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" 閉じる", Style::default().fg(Color::Gray))),
        actions::CAMP_WEAPON_DETAIL_CLOSE,
    );

    // 固定行数だと内容 (残光不足の注記等) が増えた時に「閉じる」がborder外へ
    // 押し出され、タップできなくなる (モーダルに詰む) 回帰を招く。下で
    // wrap=trueで render するため、論理行数 (`cl.len()`) ではなく実際に
    // 折り返された後の行数 (`visual_height`) から高さを決める必要がある —
    // 狭い端末幅では新武器3種の長い説明文が折り返され、論理行数のままだと
    // 依然として閉じるボタンが押し出される。
    let modal_w = area.width.saturating_sub(4).clamp(1, 48);
    let modal_h = (cl.visual_height(modal_w.saturating_sub(2)) + 2).min(area.height);
    let modal_area = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(kind.color()))
        .title(" 武器詳細 ");
    // Clear で modal_area を白紙化してから描く (render_boon_modal と同じ理由)。
    f.render_widget(Clear, modal_area);
    let mut cs = click_state.borrow_mut();
    cl.render(f, modal_area, block, &mut cs, true, 0);
}

/// 拠点画面の本体。目的別の4タブ (`CampTab`) に分け、選択中のタブだけを
/// スクロール一覧として描く。以前は全項目を1本の長いリストに詰め込んで
/// いたため「毎回選ぶもの」「残光で払うもの」「振り返るだけのもの」が
/// 混在して見づらかった — タブで区切ることで、画面には常に1つの目的の
/// 項目だけが載るようにする。
///
/// `TabBar`/`ScrollableTab` の組み合わせは `metropolis::render_tab_panel`
/// と同じ構成 (外枠のBlockを手動で描き、内側を [タブバー1行 / 内容] に
/// 分割する) を踏襲している。
fn render_camp_body(
    state: &EverlightState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 拠点 ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let vchunks =
        Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Min(0)]).split(inner);

    {
        let mut cs = click_state.borrow_mut();
        TabBar::new("│")
            .tab(CampTab::Prepare.label(), camp_tab_style(state.camp_tab == CampTab::Prepare), actions::CAMP_TAB_PREPARE)
            .tab(CampTab::Upgrades.label(), camp_tab_style(state.camp_tab == CampTab::Upgrades), actions::CAMP_TAB_UPGRADES)
            .tab(CampTab::Weapons.label(), camp_tab_style(state.camp_tab == CampTab::Weapons), actions::CAMP_TAB_WEAPONS)
            .tab(CampTab::Stats.label(), camp_tab_style(state.camp_tab == CampTab::Stats), actions::CAMP_TAB_STATS)
            .render(f, vchunks[0], &mut cs);
    }

    let list = match state.camp_tab {
        CampTab::Prepare => camp_prepare_list(state),
        CampTab::Upgrades => camp_upgrades_list(state),
        CampTab::Weapons => camp_weapons_list(state, vchunks[1].width),
        CampTab::Stats => camp_stats_list(state),
    };
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(list, &state.camp_scroll, actions::CAMP_SCROLL_UP, actions::CAMP_SCROLL_DOWN)
        .wrap(true)
        .arrow_color(Color::Green)
        .render(f, vchunks[1], &mut cs);
}

/// 選択中のタブは Everlight のブランドカラー (`theme::accent`) を反転背景
/// で強調し、それ以外は暗く沈める — `render_camp_body` の CTA ボタンと
/// 同じ「反転色 = 今アクティブ/主役」という配色言語を、タブの選択状態にも揃える。
fn camp_tab_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Black).bg(theme::accent(&GameChoice::Everlight)).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// 「出撃」タブ: 夜番へ出る前に毎回見直すもの (残光・挑戦ランク・初期武器・
/// 灯のタイプ) と、出撃ボタンそのもの。
fn camp_prepare_list(state: &EverlightState) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(vec![
        Span::styled("⠄⠂ ", Style::default().fg(EMBER_COLOR)),
        Span::styled("残光 ", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{}", state.ember), Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
    ]));
    cl.push(ember_divider_line());

    push_rank_selector(&mut cl, state);
    cl.push(ember_divider_line());

    push_starting_weapon_selector(&mut cl, state);
    cl.push(ember_divider_line());
    push_lantern_type_selector(&mut cl, state);
    cl.push(ember_divider_line());

    let cta_style = Style::default().fg(Color::Black).bg(Color::LightGreen).add_modifier(Modifier::BOLD);
    cl.push_clickable(Line::from(Span::styled("  ▶ 夜番へ出る  ", cta_style)), actions::CAMP_START_VIGIL);
    cl.push(Line::from(Span::styled(
        "    灯を持って魔物の群れを迎え撃つ",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));
    cl
}

/// 「強化」タブ: 残光で払う恒久強化 (灯心/光力/スロット拡張)。
fn camp_upgrades_list(state: &EverlightState) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));

    push_upgrade_row(
        &mut cl,
        "灯心",
        &format!("最大灯 +{} (現在 {})", state.camp.light_increment(), state.camp.light_max()),
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
    cl
}

/// 「武器」タブ: 残光で解放する武器の一覧。
fn camp_weapons_list(state: &EverlightState, area_width: u16) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    push_weapon_unlock_section(&mut cl, state, area_width);
    cl.push(Line::from(""));
    cl
}

/// 「戦績」タブ: 自己ベストの振り返り。読み取り専用でタップ操作は無い。
fn camp_stats_list(state: &EverlightState) -> ClickableList<'static> {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(format!(
        " 自己最高: 第{}波 / 生存 {}",
        state.best_wave,
        format_survival(state.best_survival_ticks)
    )));
    cl.push(Line::from(""));
    cl
}

/// 拠点画面の各セクション見出し。太字ラベルの下に Braille の点罫線を
/// 敷き、戦場 (Canvas+Braille) と同じ点描の質感を待機中の一覧にも
/// 持ち込む。プレーンテキストが並ぶだけだったセクションの境目を、
/// 色と点描の両方でひと目でわかるようにする。
fn push_section_header(cl: &mut ClickableList, title: &str, color: Color) {
    cl.push(Line::from(Span::styled(format!(" {title}"), Style::default().fg(color).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(Span::styled(" ⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒", Style::default().fg(color))));
}

/// セクション間に挟む区切り。空行の代わりに、8方向いずれか1点だけが
/// 立った braille を並べ、`render_camp_ambience` の熾火と同じ質感の
/// 「燠火が散っている」ような一行にする。
fn ember_divider_line() -> Line<'static> {
    const DOTS: [char; 8] = ['⠁', '⠐', '⠂', '⠠', '⠄', '⢀', '⡀', '⠈'];
    let dots: String = DOTS.iter().cycle().take(22).collect();
    Line::from(Span::styled(format!(" {dots}"), Style::default().fg(EMBER_COLOR)))
}

/// 直近 [`RANK_PIP_WINDOW`] 件だけを表示するウィンドウ幅。
/// `max_unlocked_rank` に上限が無いため、ランクが進むほど行が際限なく
/// 伸びないよう選択中のランクを含む範囲だけを切り出す。
const RANK_PIP_WINDOW: u32 = 10;
/// 解放済みランクの先に、未解放のランクが控えていることを示す予告分。
const RANK_PIP_LOOKAHEAD: u32 = 2;

/// 挑戦ランクの解放状況を dot で可視化する行。選択中 (◆)・解放済み (●)・
/// まだ先にある未解放 (○) を横に並べ、「あと何夜で次のランクか」を
/// 文章を読まず一目で把握できるようにする。
fn rank_pip_line(selected: u32, max_unlocked: u32) -> Line<'static> {
    let end = max_unlocked + RANK_PIP_LOOKAHEAD;
    let start = end.saturating_sub(RANK_PIP_WINDOW - 1).max(1);

    let mut spans = vec![if start > 1 {
        Span::styled(" …", Style::default().fg(Color::DarkGray))
    } else {
        Span::raw(" ")
    }];
    for rank in start..=end {
        let (glyph, color) = if rank == selected {
            ("◆", Color::LightYellow)
        } else if rank <= max_unlocked {
            ("●", Color::LightGreen)
        } else {
            ("○", Color::DarkGray)
        };
        spans.push(Span::styled(glyph, Style::default().fg(color)));
    }
    Line::from(spans)
}

/// 挑戦ランクの選択行と、選択中ランクの目標 (最終波・最終ボス名) を
/// 積む。「次に何を目指すか」を拠点画面で常に見えるようにする。
fn push_rank_selector(cl: &mut ClickableList, state: &EverlightState) {
    let selected = state.camp.effective_selected_rank();
    let max_unlocked = state.camp.max_unlocked_rank.max(1);

    push_section_header(cl, "挑戦ランク", theme::accent(&GameChoice::Everlight));
    cl.push(Line::from(format!(" 第{selected}夜  (解放済み: 第{max_unlocked}夜まで)")));
    cl.push(rank_pip_line(selected, max_unlocked));

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

/// 夜番開始時に持つ武器の選択。解放済みの武器のみを巡回する
/// (`logic::cycle_starting_weapon`)。
fn push_starting_weapon_selector(cl: &mut ClickableList, state: &EverlightState) {
    let selected = state.camp.effective_starting_weapon();
    push_section_header(cl, "初期武器", selected.color());
    cl.push(Line::from(Span::styled(
        format!(" {} — {}", selected.name(), selected.summary()),
        Style::default().fg(selected.color()),
    )));
    let multi = WeaponKind::all().iter().filter(|&&k| state.camp.is_weapon_unlocked(k)).count() > 1;
    let color = if multi { Color::LightCyan } else { Color::DarkGray };
    cl.push_clickable(Line::from(Span::styled(" ◀ 前の武器", Style::default().fg(color))), actions::CAMP_STARTING_WEAPON_PREV);
    cl.push_clickable(Line::from(Span::styled(" ▶ 次の武器", Style::default().fg(color))), actions::CAMP_STARTING_WEAPON_NEXT);
}

/// 灯のタイプの選択。武器と違い常に3種すべてから自由に選べる
/// (`logic::cycle_lantern_type`)。
fn push_lantern_type_selector(cl: &mut ClickableList, state: &EverlightState) {
    let selected = state.camp.lantern_type;
    // 見出しの色は `render_camp_ambience` の情景パネルと同じ配色を使う —
    // 「このタイプを選ぶと灯の色味がこう変わる」を、選ぶ前から予告する。
    let (accent, _) = lantern_ambience_colors(selected);
    push_section_header(cl, "灯のタイプ", accent);
    cl.push(Line::from(format!(" {} — {}", selected.name(), selected.summary())));
    cl.push_clickable(
        Line::from(Span::styled(" ◀ 前のタイプ", Style::default().fg(Color::LightCyan))),
        actions::CAMP_LANTERN_TYPE_PREV,
    );
    cl.push_clickable(
        Line::from(Span::styled(" ▶ 次のタイプ", Style::default().fg(Color::LightCyan))),
        actions::CAMP_LANTERN_TYPE_NEXT,
    );
}

/// 説明文を、先頭に4スペースのインデントを付けた複数行へ手動で折り返す。
///
/// `ClickableList` の自動 wrap は「継続行に元のインデントを引き継がない」
/// (先頭行だけが `"    "` を保持し、2行目以降は左端に張り付く) ため、
/// 長い説明文がwrapされると `push_weapon_unlock_section` が作る「見出し+
/// インデントした説明」というブロックの見た目が崩れる。ここで先に短い
/// 行へ割ってしまえば自動 wrap 自体が発生せず、崩れを防げる。
///
/// `area_width` は `render_camp_body` がタブ内容へ渡す内側 (border控除後)
/// の `Rect::width` だが、`ScrollableTab` が予約する矢印列ぶんはここでは
/// 引いていない。厳密な最終幅を計算する代わりに大きめの余白
/// (`SAFE_WIDTH_MARGIN`) を引いて必ず実際の描画幅以下になるようにしている
/// — 実際より狭く見積もる分には、折り返しがやや増えるだけで崩れない (安全側)。
const SAFE_WIDTH_MARGIN: u16 = 5;
const DESCRIPTION_INDENT: &str = "    ";

fn wrap_indented_description(text: &str, area_width: u16) -> Vec<Line<'static>> {
    let indent_width = Span::raw(DESCRIPTION_INDENT).width() as u16;
    let budget = area_width.saturating_sub(SAFE_WIDTH_MARGIN).saturating_sub(indent_width).max(1);

    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0u16;
    for ch in text.chars() {
        let ch_width = Span::raw(ch.to_string()).width() as u16;
        if current_width + ch_width > budget && !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows.into_iter()
        .map(|row| {
            Line::from(Span::styled(format!("{DESCRIPTION_INDENT}{row}"), Style::default().fg(Color::DarkGray)))
        })
        .collect()
}

/// 武器解放の一覧。タップすると `render_weapon_detail_modal` の詳細
/// モーダルが開く (解放済みは確認、未解放は解放ボタン付き)。
///
/// 名前・状態・コスト・説明を1行に詰め込むと、8種が隙間なく並んで
/// どこからどこまでが1つのボタンなのか読み取りにくくなる。`push_upgrade_row`
/// と同じ「太字の見出し行 + インデントした説明行」の2行構成に揃え、
/// 行間の空行で1件ずつ独立したボタンとして読めるようにする — 装飾を
/// 足すのではなく、既存のレイアウト言語をここにも適用する。
fn push_weapon_unlock_section(cl: &mut ClickableList, state: &EverlightState, area_width: u16) {
    let kinds = WeaponKind::all();
    for (i, &kind) in kinds.iter().enumerate() {
        if i > 0 {
            cl.push(Line::from(""));
        }
        let action_id = actions::CAMP_UNLOCK_WEAPON_BASE + kind.save_id() as u16;
        if state.camp.is_weapon_unlocked(kind) {
            cl.push_clickable(
                Line::from(Span::styled(
                    format!(" ✓ {}  解放済み", kind.name()),
                    Style::default().fg(kind.color()).add_modifier(Modifier::BOLD),
                )),
                action_id,
            );
            for line in wrap_indented_description(kind.summary(), area_width) {
                cl.push(line);
            }
            continue;
        }
        let Some(cost) = kind.unlock_cost() else {
            continue;
        };
        let affordable = state.ember >= cost;
        let color = if affordable { Color::LightCyan } else { Color::DarkGray };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" ▶ {}", kind.name()), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {cost}残光"), Style::default().fg(color)),
            ]),
            action_id,
        );
        for line in wrap_indented_description(kind.summary(), area_width) {
            cl.push(line);
        }
    }
}

/// 恒久強化のレベル表示に添える進行度バー (`theme::PROGRESS_FULL`/
/// `PROGRESS_EMPTY`)。HPバーと同じ「塗り具合」の見た目言語を、上限の
/// 無いレベルにも流用する。無制限に伸びるレベルをそのまま点で埋めると
/// 行が際限なく伸びるため、`cap` 件で頭打ちにして超過分は "+N" で表す。
const UPGRADE_PIP_CAP: u32 = 6;

fn level_pip_string(level: u32, cap: u32) -> String {
    let filled = level.min(cap);
    let mut s: String = (0..cap).map(|i| if i < filled { theme::PROGRESS_FULL } else { theme::PROGRESS_EMPTY }).collect();
    if level > cap {
        s.push_str(&format!("+{}", level - cap));
    }
    s
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
        Span::styled(format!(" {}", level_pip_string(level, UPGRADE_PIP_CAP)), Style::default().fg(color)),
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
        render_to_test_backend_with_click_state(state, width, height);
    }

    /// `Line` の表示テキストだけを取り出す (装飾スタイルを無視して内容を検査したいテスト用)。
    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn rank_pip_line_marks_the_only_rank_as_selected_with_two_upcoming_locked_pips() {
        let text = line_text(&rank_pip_line(1, 1));
        assert_eq!(text.matches('◆').count(), 1, "選択中ランクはちょうど1つ表示されるはず: {text:?}");
        assert_eq!(
            text.matches('○').count(),
            RANK_PIP_LOOKAHEAD as usize,
            "解放済みランクの先に、未解放の予告pipがLOOKAHEAD件表示されるはず: {text:?}"
        );
        assert_eq!(text.matches('●').count(), 0);
    }

    #[test]
    fn rank_pip_line_marks_unlocked_ranks_other_than_selected_as_filled() {
        let text = line_text(&rank_pip_line(5, 8));
        assert_eq!(text.matches('◆').count(), 1, "選択中ランクはちょうど1つ表示されるはず: {text:?}");
        // 1..=8 (解放済み) のうち selected(5) 以外の7つが●、8+1..=8+2 の2つが未解放の予告○。
        assert_eq!(text.matches('●').count(), 7);
        assert_eq!(text.matches('○').count(), 2);
    }

    #[test]
    fn rank_pip_line_windows_to_the_recent_ranks_and_flags_truncation_with_an_ellipsis() {
        let text = line_text(&rank_pip_line(20, 20));
        assert!(text.starts_with(" …"), "ウィンドウ外に切り捨てた先頭は省略記号を示すはず: {text:?}");
        let pip_count = text.chars().filter(|c| matches!(c, '◆' | '●' | '○')).count();
        assert_eq!(pip_count, RANK_PIP_WINDOW as usize, "表示されるpip数はウィンドウ幅に固定されるはず: {text:?}");
    }

    #[test]
    fn level_pip_string_fills_proportionally_and_marks_overflow_with_plus_n() {
        assert_eq!(level_pip_string(0, 6), "▱▱▱▱▱▱");
        assert_eq!(level_pip_string(3, 6), "▰▰▰▱▱▱");
        assert_eq!(level_pip_string(6, 6), "▰▰▰▰▰▰");
        assert_eq!(level_pip_string(9, 6), "▰▰▰▰▰▰+3", "cap超過分は数字で表すはず");
    }

    #[test]
    fn camp_body_renders_without_panicking_across_a_range_of_narrow_widths() {
        // 実機のスマホ幅相当 (20〜)からデスクトップ幅までを一通りスイープし、
        // 新しく追加した装飾行 (点罫線・pip・区切り) が特定の幅でだけ panic
        // する回帰を防ぐ。
        let state = EverlightState::new();
        for w in 20u16..=100 {
            render_to_test_backend(&state, w, 34);
        }
    }

    #[test]
    fn camp_body_renders_without_panicking_with_heavily_progressed_state() {
        // pip表示 (`rank_pip_line`/`level_pip_string`) は「無制限に伸びる値」を
        // 固定幅の点描に丸め込む設計。丸め込みが機能せず行が際限なく伸びて
        // いないか、実際の進行では滅多に起きないくらい極端な値でも確認する。
        let mut state = EverlightState::new();
        state.camp.light_level = 500;
        state.camp.power_level = 500;
        state.camp.max_unlocked_rank = 500;
        state.camp.selected_rank = 500;
        state.ember = 100_000;
        state.camp.unlocked_weapons = WeaponKind::all().to_vec();
        render_to_test_backend(&state, 40, 34);
        render_to_test_backend(&state, 100, 34);
    }

    #[test]
    fn every_weapon_unlock_row_is_individually_clickable() {
        // 武器解放欄を「見出し行+説明行+空行」の3行構成に組み替えた際、
        // 空行の挿入タイミング (`i > 0` の位置) を間違えると特定の武器だけ
        // クリック対象がずれる/消える回帰が起こり得る。全武器種について
        // 個別に action_id が登録されていることを確認する。
        let mut state = EverlightState::new();
        state.camp_tab = CampTab::Weapons;
        let (w, h) = (80u16, 200u16);
        let cs = render_to_test_backend_with_click_state(&state, w, h);
        for &kind in WeaponKind::all() {
            let action_id = actions::CAMP_UNLOCK_WEAPON_BASE + kind.save_id() as u16;
            assert!(has_click_target(&cs, w, h, action_id), "{kind:?} の武器解放行がクリック対象として登録されていない");
        }
    }

    #[test]
    fn every_camp_tab_renders_without_panicking_narrow_and_wide() {
        let mut state = EverlightState::new();
        for tab in [CampTab::Prepare, CampTab::Upgrades, CampTab::Weapons, CampTab::Stats] {
            state.camp_tab = tab;
            render_to_test_backend(&state, 40, 30);
            render_to_test_backend(&state, 100, 30);
        }
    }

    #[test]
    fn scroll_indicator_never_appears_on_the_tab_bar_row() {
        // タブバー (1行) とスクロール矢印列は `render_camp_body` の
        // `Layout::split` で別々の Rect (vchunks[0]/vchunks[1]) に分けている。
        // 分割を間違えると矢印がタブバー行に描かれてしまいかねないので、
        // 武器タブを全解放してスクロールが確実に発生する高さで検証する。
        let mut state = EverlightState::new();
        state.camp_tab = CampTab::Weapons;
        state.camp.unlocked_weapons = WeaponKind::all().to_vec();
        let (w, h) = (40u16, 20u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let buf = terminal.backend().buffer();

        let row_text = |y: u16| -> String { (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect() };
        let tab_row = (0..h)
            .find(|&y| {
                let compact: String = row_text(y).chars().filter(|c| !c.is_whitespace()).collect();
                compact.contains("出撃") && compact.contains("戦績")
            })
            .expect("タブバー行が見つからない");

        let tab_row_text = row_text(tab_row);
        assert!(
            !tab_row_text.contains('▲') && !tab_row_text.contains('▼'),
            "タブバー行に矢印が描かれている: {tab_row_text:?}"
        );

        // このassertだけだと「そもそも矢印が無い」だけでも通ってしまうため、
        // 矢印自体はどこかの行に実際に出ていることも確認する。
        let has_scroll_indicator = (0..h).any(|y| row_text(y).contains('▼') || row_text(y).contains('▲'));
        assert!(has_scroll_indicator, "この高さでは全武器を並べきれずスクロールが発生するはず");
    }

    #[test]
    fn camp_tabs_show_only_their_own_content() {
        // 「出撃」タブにしか無い夜番開始ボタンが、他のタブでも誤って
        // 表示され続ける (=タブの切り替えがコンテンツに反映されていない)
        // 回帰を検知する。
        let mut state = EverlightState::new();

        state.camp_tab = CampTab::Prepare;
        let cs = render_to_test_backend_with_click_state(&state, 80, 40);
        assert!(has_click_target(&cs, 80, 40, actions::CAMP_START_VIGIL), "出撃タブには夜番へ出るボタンがあるはず");

        for tab in [CampTab::Upgrades, CampTab::Weapons, CampTab::Stats] {
            state.camp_tab = tab;
            let cs = render_to_test_backend_with_click_state(&state, 80, 40);
            assert!(!has_click_target(&cs, 80, 40, actions::CAMP_START_VIGIL), "{tab:?}タブに夜番へ出るボタンが漏れている");
        }
    }

    #[test]
    fn wrap_indented_description_keeps_short_text_on_one_indented_line() {
        let lines = wrap_indented_description("短い説明", 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "    短い説明");
    }

    #[test]
    fn wrap_indented_description_splits_long_text_and_indents_every_row() {
        // 氷華の説明文で実機再現した回帰: 折り返しが起きた継続行が左端に
        // 張り付き、見出し行とのインデントが揃わなくなっていた。
        let lines = wrap_indented_description("命中した敵を減速させる自動照準弾", 40);
        assert!(lines.len() > 1, "40幅では1行に収まらないはず");
        for line in &lines {
            let text = line_text(line);
            assert!(text.starts_with(DESCRIPTION_INDENT), "継続行も含め全行がインデントされるはず: {text:?}");
        }
        // 分割前後で文字が失われていないこと。
        let rejoined: String = lines.iter().map(|l| line_text(l).trim_start().to_string()).collect();
        assert_eq!(rejoined, "命中した敵を減速させる自動照準弾");
    }

    #[test]
    fn wrap_indented_description_each_row_fits_within_the_conservative_budget() {
        for &kind in WeaponKind::all() {
            for area_width in [20u16, 31, 45, 100] {
                for line in wrap_indented_description(kind.summary(), area_width) {
                    let width = line.width() as u16;
                    assert!(
                        width <= area_width,
                        "{kind:?} at area_width={area_width}: 行幅{width}が実際の描画幅を超えている: {line:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn wrap_indented_description_never_panics_on_zero_width() {
        assert!(!wrap_indented_description("何かの説明文", 0).is_empty());
    }

    /// `render_to_test_backend` の戻り値ありバージョン。描画後に実際に
    /// 登録されたクリック対象を検査したいテスト用 (モーダルのボタンが
    /// border外へ押し出されて登録されない、といった回帰を検知するため)。
    fn render_to_test_backend_with_click_state(state: &EverlightState, width: u16, height: u16) -> Rc<RefCell<ClickState>> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = width;
        cs.borrow_mut().terminal_rows = height;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                render(state, f, f.area(), &cs);
            })
            .unwrap();
        cs
    }

    /// `click_state` に登録された領域のうち `action_id` を返すセルが
    /// 1つでもあるか。
    fn has_click_target(click_state: &Rc<RefCell<ClickState>>, cols: u16, rows: u16, action_id: u16) -> bool {
        let cs = click_state.borrow();
        (0..rows).any(|row| (0..cols).any(|col| cs.hit_test(col, row) == Some(action_id)))
    }

    #[test]
    fn enemy_kinds_render_list_covers_every_enemy_kind() {
        // ENEMY_KINDS が EnemyKind::all() を取りこぼすと、その敵種は湧いて
        // いるのに一切描画されない (過去に実際に踏んだ不具合パターン)。
        let all: std::collections::HashSet<_> = EnemyKind::all().iter().collect();
        let rendered: std::collections::HashSet<_> = ENEMY_KINDS.iter().collect();
        assert_eq!(all, rendered, "ENEMY_KINDSがEnemyKind::all()と食い違っている");
    }

    #[test]
    fn camp_renders_without_panicking_narrow_and_wide() {
        let state = EverlightState::new();
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    /// `render_weapon_detail_modal` は拠点画面(本体リスト + 情景パネル)の
    /// 上に、同じフレーム内で重ね描みされる。`render_boon_modal` と同じ
    /// `Clear` 漏れの回帰を検証する — ただしこちらは表示行数が「解放済み
    /// か」「残光が足りるか」で変わる (`modal_h` が `cl.visual_height` に
    /// 依存する) ため、その計算式をテスト側で複製すると本体の分岐が増えた
    /// 時にテストだけ追従し忘れて誤った領域を検査するドリフトの危険がある。
    /// 代わりに、実際に描画された枠線 (" 武器詳細 " というタイトルを持つ
    /// border) をバッファから探して modal の実座標を特定する — 本体の
    /// 行数が変わっても常に正しい領域を検査できる。
    #[test]
    fn weapon_detail_modal_clears_background_glyphs_underneath() {
        let mut state = EverlightState::new();
        state.weapon_detail_modal = Some(WeaponKind::Meteor);

        let (w, h) = (100u16, 30u16);
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = w;
        cs.borrow_mut().terminal_rows = h;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let bg = Paragraph::new(vec![Line::from("#".repeat(w as usize)); h as usize]);
                f.render_widget(bg, f.area());
                render(&state, f, f.area(), &cs);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let row_symbols = |y: u16| -> Vec<String> {
            (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect()
        };

        // タイトル " 武器詳細 " を含む上端の border 行を探し、同じ列にある
        // "┌"/"┐" を左右端、その列で "└"/"┘" が現れる行を下端とする。
        let mut modal_left = None;
        let mut modal_right = None;
        let mut top_row = None;
        for y in 0..h {
            let cells = row_symbols(y);
            for (x, symbol) in cells.iter().enumerate() {
                if symbol != "┌" {
                    continue;
                }
                // 全角文字はセルごとに空白の継続セルを伴うため、素の
                // concat だと "武 器 詳 細" のように分断される。空白/空
                // セルを除いてから連結することで、幅を気にせず部分一致
                // 判定できるようにする。
                let ahead: String = cells[x..(x + 20).min(cells.len())]
                    .iter()
                    .filter(|s| !s.trim().is_empty())
                    .cloned()
                    .collect();
                if ahead.contains("武器詳細") {
                    modal_left = Some(x as u16);
                    top_row = Some(y);
                    modal_right = cells[x + 1..]
                        .iter()
                        .position(|s| s == "┐")
                        .map(|off| x as u16 + 1 + off as u16);
                    break;
                }
            }
            if top_row.is_some() {
                break;
            }
        }
        let (left, right, top) = (
            modal_left.expect("武器詳細モーダルの左上が見つからない"),
            modal_right.expect("武器詳細モーダルの右上が見つからない"),
            top_row.unwrap(),
        );
        let mut bottom = None;
        for y in (top + 1)..h {
            if row_symbols(y)[left as usize] == "└" {
                bottom = Some(y);
                break;
            }
        }
        let bottom = bottom.expect("武器詳細モーダルの下端が見つからない");

        for y in (top + 1)..bottom {
            for x in (left + 1)..right {
                assert_ne!(
                    buf[(x, y)].symbol(),
                    "#",
                    "modal cell ({x},{y}) still shows the background glyph underneath — Clear is missing"
                );
            }
        }
    }

    #[test]
    fn camp_renders_without_panicking_with_weapon_detail_modal_open() {
        let mut state = EverlightState::new();
        state.weapon_detail_modal = Some(WeaponKind::Meteor);
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);

        state.ember = 1000;
        state.camp.unlocked_weapons.push(WeaponKind::Meteor);
        state.weapon_detail_modal = Some(WeaponKind::Meteor);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn weapon_detail_modal_close_button_is_always_reachable() {
        // 「残光不足」の注記が出る最長の内容 (未解放+購入不可) でも、
        // 閉じるボタンがborder外へ押し出されてタップ不能にならないことを
        // 確認する回帰テスト。modal_h を固定値や論理行数 (`cl.len()`) から
        // 決めると、狭い端末幅で説明文が折り返された時に壊れる —
        // 実機のスマホ幅相当 (index.html の targetCols 計算で31〜45列
        // 程度になる) を含む範囲を、説明文が最も長い新武器3種を含む
        // 全武器種でスイープする。
        let h = 30u16;
        for &kind in WeaponKind::all() {
            let mut state = EverlightState::new();
            state.ember = 0;
            state.weapon_detail_modal = Some(kind);
            for w in 20u16..=50 {
                let cs = render_to_test_backend_with_click_state(&state, w, h);
                assert!(
                    has_click_target(&cs, w, h, actions::CAMP_WEAPON_DETAIL_CLOSE),
                    "{kind:?} at {w}x{h}: 閉じるボタンがクリック対象として登録されていない"
                );
            }
        }

        // タイトル帯を除いた本体が薄い、縦にも狭いケース。
        let mut state = EverlightState::new();
        state.ember = 0;
        state.weapon_detail_modal = Some(WeaponKind::Chain);
        let cs = render_to_test_backend_with_click_state(&state, 40, 16);
        assert!(has_click_target(&cs, 40, 16, actions::CAMP_WEAPON_DETAIL_CLOSE));
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
    fn vigil_renders_without_panicking_with_kill_effects() {
        use super::super::state::KillEffect;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.kill_effects.push(KillEffect { x: state.lantern.x, y: 30.0, ticks_left: KILL_EFFECT_TICKS });
        render_to_test_backend(&state, 40, 30);
        render_to_test_backend(&state, 100, 30);
    }

    #[test]
    fn vigil_renders_without_panicking_with_a_caster_and_enemy_bullets() {
        use super::super::state::EnemyBullet;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        state.enemy_bullets.push(EnemyBullet {
            x: state.lantern.x,
            y: 30.0,
            vx: 0.0,
            vy: 2.2,
            damage: 4,
            source: EnemyKind::Caster,
        });
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

    /// `render_boon_modal` は `render_battlefield` の後、同じフレーム内で
    /// battlefield の上に重ね描きされる。`Clear` widget を挟まずに描くと、
    /// モーダルの余白セル (テキストが無いセル) には battlefield の Braille
    /// 点描がそのまま残る (`Paragraph` はテキストのあるセルしか書き換えない
    /// ため) — これが「モーダル表示時に背面が透ける」不具合の実体。この
    /// テストは、中央に置いた敵の Braille 点がモーダル領域内から一切
    /// 見えなくなることを検証する回帰テスト。
    #[test]
    fn boon_modal_clears_battlefield_braille_underneath() {
        use super::super::state::{BoonKind, BoonOption, Enemy};
        use crate::effects::FlashTimer;

        let mut state = EverlightState::new();
        logic::start_vigil(&mut state);
        // ワールド中心に敵を置く。world_to_canvas_y(WORLD_H/2) == WORLD_H/2
        // なので、Canvas の x_bounds/y_bounds ([0,WORLD_W]/[0,WORLD_H]) 上でも
        // battlefield Rect のちょうど中央に描かれ、中央に配置されるモーダルと
        // 確実に重なる。
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x: WORLD_W / 2.0,
            y: WORLD_H / 2.0,
            hp: 10,
            max_hp: 10,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
            slow_ticks: 0,
        });
        state.pending_boons = Some([
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Spray) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Aurora) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Halo) },
        ]);

        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = 60;
        cs.borrow_mut().terminal_rows = 30;
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|f| {
                render(&state, f, f.area(), &cs);
            })
            .unwrap();

        let layout = compute_vigil_layout(Rect::new(0, 0, 60, 30));
        let modal_w = layout.battlefield.width.saturating_sub(2).max(1);
        let modal_h = layout.battlefield.height.min(3 * 3 + 5);
        let modal_area = Rect::new(
            layout.battlefield.x + (layout.battlefield.width.saturating_sub(modal_w)) / 2,
            layout.battlefield.y + (layout.battlefield.height.saturating_sub(modal_h)) / 2,
            modal_w,
            modal_h,
        );

        let buffer = terminal.backend().buffer();
        for y in modal_area.y..modal_area.y + modal_area.height {
            for x in modal_area.x..modal_area.x + modal_area.width {
                let symbol = buffer[(x, y)].symbol();
                let is_braille = symbol.chars().next().is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
                assert!(
                    !is_braille,
                    "modal cell ({x},{y}) still shows a battlefield Braille glyph ({symbol:?}) — Clear is missing"
                );
            }
        }
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
