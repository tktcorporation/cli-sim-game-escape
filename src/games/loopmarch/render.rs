//! 周回討伐 描画。読み取り専用 (state を変更しない)。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{ClickableGrid, ClickableList, ScrollableTab};

use super::actions::*;
use super::logic;
use super::state::{
    CampUpgrades, LoopMarchState, Monster, Phase, Terrain, REFILL_STONE_COST, REFILL_WOOD_COST,
    RING_H, RING_W,
};

pub fn render(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.phase {
        Phase::Camp => render_camp(state, f, area, click_state),
        Phase::Expedition => render_expedition(state, f, area, click_state),
    }
}

// ── 拠点画面 ──

fn render_camp(
    state: &LoopMarchState,
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
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(6)])
        .split(area);

    let title = Paragraph::new(Line::from(Span::styled(
        "周回討伐 - 拠点",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    render_camp_body(state, f, chunks[1], click_state, borders);
    render_log(state, f, chunks[2], borders);
}

fn push_upgrade_row(
    cl: &mut ClickableList,
    name: &str,
    level: u32,
    cost: u32,
    soul: u32,
    action_id: u16,
    detail: String,
) {
    let affordable = soul >= cost;
    let name_color = if affordable { Color::White } else { Color::DarkGray };
    let cost_color = if affordable { Color::LightMagenta } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                format!("{name} Lv.{level} "),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("(魂{cost}) "), Style::default().fg(cost_color)),
            Span::styled(detail, Style::default().fg(Color::DarkGray)),
        ]),
        action_id,
    );
}

fn render_camp_body(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 遠征で魂を集めて、下の強化に使おう。強化は死んでも消えない。",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(
            " 魂: {}   自己ベスト: 第{}周",
            state.soul, state.best_lap
        ),
        Style::default()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    push_upgrade_row(
        &mut cl,
        " 最大HP強化",
        state.camp.max_hp_level,
        state.camp.max_hp_cost(),
        state.soul,
        CAMP_UPGRADE_MAX_HP,
        format!("現在{}", state.camp.hero_max_hp()),
    );
    push_upgrade_row(
        &mut cl,
        " 攻撃力強化",
        state.camp.attack_level,
        state.camp.attack_cost(),
        state.soul,
        CAMP_UPGRADE_ATTACK,
        format!("現在{}", state.camp.hero_attack()),
    );

    if state.camp.extra_card_level >= 1 {
        cl.push(Line::from(Span::styled(
            " 初期手札+1 — 習得済み",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        push_upgrade_row(
            &mut cl,
            " 初期手札+1",
            0,
            CampUpgrades::EXTRA_CARD_COST,
            state.soul,
            CAMP_UPGRADE_EXTRA_CARD,
            "次回遠征から".to_string(),
        );
    }

    cl.push(Line::from(""));

    let label = if state.run_active {
        format!(" ▶ 遠征に戻る (第{}周)", state.lap + 1)
    } else {
        " ▶ 遠征に出発する".to_string()
    };
    cl.push_clickable(
        Line::from(Span::styled(
            label,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        CAMP_START_OR_RESUME,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 拠点強化 ");
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(cl, &state.camp_scroll, CAMP_SCROLL_UP, CAMP_SCROLL_DOWN)
        .block(block)
        .wrap(true)
        .arrow_color(Color::Green)
        .render(f, area, &mut cs);
}

fn render_log(state: &LoopMarchState, f: &mut Frame, area: Rect, borders: Borders) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let log_lines: Vec<Line> = state
        .log
        .iter()
        .rev()
        .take(visible_height)
        .enumerate()
        .map(|(i, entry)| {
            let color = if i == 0 { Color::White } else { Color::DarkGray };
            Line::from(Span::styled(format!(" {entry}"), Style::default().fg(color)))
        })
        .collect();

    let widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Blue))
                .title(" ログ "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

// ── 遠征画面 ──

fn render_expedition(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if is_narrow_layout(area.width) {
        render_expedition_narrow(state, f, area, click_state);
    } else {
        render_expedition_wide(state, f, area, click_state);
    }
}

/// 遠征画面の主要パネルの Rect。演出のトリガー領域を決める
/// `detect_transitions` からも参照するため公開する。
pub struct ExpeditionLayout {
    pub header: Rect,
    pub ring: Rect,
    pub hand: Rect,
    pub log: Rect,
}

/// narrow/wide でチャンク構成が異なるため、両方をここに集約する
/// (render 側とトリガー領域算出側で計算がずれないようにするため)。
pub fn compute_expedition_layout(area: Rect, is_narrow: bool) -> ExpeditionLayout {
    if is_narrow {
        let chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(RING_H as u16 + 2),
                Constraint::Min(8),
                Constraint::Length(3),
            ])
            .split(area);
        ExpeditionLayout {
            header: chunks[0],
            ring: chunks[1],
            hand: chunks[2],
            log: chunks[3],
        }
    } else {
        // ヘッダーは資源の内訳(今回限り/永続)を文章で見せるため幅が要る。
        // 左カラム(20桁、リング用)には収まらないので全幅に置き、
        // その下でリングと手札を左右に分ける。
        let top_chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(20)])
            .split(area);
        let body_chunks = Layout::default()
            .direction(LayoutDir::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(20)])
            .split(top_chunks[1]);
        let right_chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([Constraint::Min(10), Constraint::Min(6)])
            .split(body_chunks[1]);
        ExpeditionLayout {
            header: top_chunks[0],
            ring: body_chunks[0],
            hand: right_chunks[0],
            log: right_chunks[1],
        }
    }
}

fn render_expedition_wide(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layout = compute_expedition_layout(area, false);
    render_header(state, f, layout.header, false);
    render_ring(state, f, layout.ring, click_state);
    render_hand(state, f, layout.hand, click_state);
    render_log(state, f, layout.log, Borders::ALL);
}

fn render_expedition_narrow(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layout = compute_expedition_layout(area, true);
    render_header(state, f, layout.header, true);
    render_ring(state, f, layout.ring, click_state);
    render_hand(state, f, layout.hand, click_state);
    // 「資源が足りない」「そこには既に地形がある」等のフィードバックは
    // ログでしか伝えていないため、狭幅でも省略しない (1行に圧縮)。
    render_log(state, f, layout.log, Borders::TOP);
}

fn render_header(state: &LoopMarchState, f: &mut Frame, area: Rect, is_narrow: bool) {
    let defense = logic::mountain_synergy_defense(&state.path);
    let hp_color = if state.hero_hurt_flash.is_active() {
        theme::DAMAGE_FLASH.color
    } else if state.hero.hp * 3 <= state.hero.max_hp {
        Color::Red
    } else if state.hero.hp * 3 <= state.hero.max_hp * 2 {
        Color::Yellow
    } else {
        Color::Green
    };
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // 交戦中は敵の名前とHPも見せる — さもないと「@が止まってHPが減る」
    // だけでプレイヤーが何と戦っているのか分からない。
    let enemy_color = if state.enemy_hurt_flash.is_active() {
        theme::HIT_FLASH.color
    } else {
        Color::Red
    };
    let enemy_style = Style::default().fg(enemy_color).add_modifier(Modifier::BOLD);
    let stats_text = format!("ATK{} DEF{}", state.hero.attack, defense);

    // 狭幅では横幅が交戦中のモンスター名/HP表示と競合するため、ゲージ表示は
    // 幅に余裕がある wide のみ。narrow は数値のみに留め、ATK/DEF は line2 に
    // 逃がして交戦情報 (敵の名前+HP) の表示幅を確保する
    // (Codexレビュー指摘: 30桁幅だとゲージ込み・ATK/DEF同居だと敵情報が
    // 見切れていた)。それでも HP が育って桁数が増えると幅を圧迫しうるため、
    // 数値側は削らず敵の名前だけを残り幅に合わせて切り詰める。
    let (hp_text, line1_stats, line2_stats, combat_span) = if is_narrow {
        let hp_text = format!(" HP{}/{} ", state.hero.hp, state.hero.max_hp);
        let monster = state.path[state.hero.position].monster.as_ref();
        let combat_text = narrow_combat_text(monster, &hp_text, area.width);
        (hp_text, None, Some(format!(" {stats_text}")), Span::styled(combat_text, enemy_style))
    } else {
        let bar = hp_bar(state.hero.hp, state.hero.max_hp, 8);
        let hp_text = format!(" {} {}/{} ", bar, state.hero.hp, state.hero.max_hp);
        let combat_span = match &state.path[state.hero.position].monster {
            Some(m) => Span::styled(
                format!("  VS {} {}/{}", monster_name(m), m.hp.max(0), m.max_hp),
                enemy_style,
            ),
            None => Span::raw(""),
        };
        (hp_text, Some(stats_text), None, combat_span)
    };

    let mut line1_spans = vec![Span::styled(
        hp_text,
        Style::default().fg(hp_color).add_modifier(Modifier::BOLD),
    )];
    if let Some(stats) = line1_stats {
        line1_spans.push(Span::styled(stats, Style::default().fg(Color::Cyan)));
    }
    line1_spans.push(combat_span);
    let line1 = Line::from(line1_spans);

    let lap_text = format!(" 第{}周 (ベスト{}周)", state.lap + 1, state.best_lap);
    // ATK/DEF (narrowのみ、line1から追い出したもの) は周回数より優先度の
    // 高い戦況情報なので先に置く。ratatui は Span を先頭から順に描画幅を
    // 使い切るまで印字するため、先頭に置いたテキストは常に全て表示され、
    // 溢れた分は後続の (周回数のような相対的に重要度が低い) テキストが
    // 削れる (Codexレビュー指摘: 周回数を先に置くとDEFの方が見切れていた)。
    let mut line2_spans = Vec::new();
    if let Some(stats) = line2_stats {
        line2_spans.push(Span::styled(stats, Style::default().fg(Color::Cyan)));
    }
    line2_spans.push(Span::styled(lap_text, Style::default().fg(Color::White)));
    let line2 = Line::from(line2_spans);

    // 資源は「このラン限りで死ぬと消える」ものと「死んでも残る」ものを
    // 常時ラベルで分けて見せる — 死亡後に初めて気付く設計は分かりにくい
    // というフィードバックを受け、基礎の経済構造は説明することにした
    // (地形シナジーのような「発見させたい」要素とは区別している)。
    let line3 = Line::from(vec![
        Span::styled(" 今回限り:", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" 木材{} 石材{}", state.wood, state.stone),
            Style::default().fg(Color::White),
        ),
        Span::styled("  永続:", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" 魂{}", state.soul),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let widget = Paragraph::new(vec![line1, line2, line3]).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 周回討伐 "),
    );
    f.render_widget(widget, area);
}

/// 表示上の幅 (半角=1/全角=2) を返す。ratatui の Buffer もこの幅で描画を
/// 打ち切るため、切り詰め計算は文字数ではなくこの幅で行う必要がある
/// (文字数で計算すると、全角文字を含む名前が実際の残り幅の2倍まで
/// 書き込まれてしまう)。
fn display_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// 敵名を表示可能な残り幅に収まるよう切り詰める。HP数値 (`fixed_width` に
/// 含まれる) は戦況判断に必須なので絶対に削らず、余白がなくなった時は
/// 名前の方を短くする (最悪 空文字になっても数値は必ず表示される)。
fn fit_monster_name(name: &str, available_width: usize, fixed_width: usize) -> String {
    let width_budget = available_width.saturating_sub(fixed_width);
    let mut name = name.to_string();
    while display_width(&name) > width_budget {
        if name.pop().is_none() {
            break;
        }
    }
    name
}

/// narrow レイアウトの line1 における「VS 敵名 HP/最大HP」部分のテキストを
/// 組み立てる。Span化やBuffer描画から切り離してあるので、幅の予算計算を
/// 単体で (全角文字を含む Buffer 経由の再構成に頼らず) テストできる。
fn narrow_combat_text(monster: Option<&Monster>, hp_text: &str, area_width: u16) -> String {
    match monster {
        Some(m) => {
            let prefix = "VS ";
            let hp_suffix = format!(" {}/{}", m.hp.max(0), m.max_hp);
            let fixed_width = display_width(hp_text) + display_width(prefix) + display_width(&hp_suffix);
            let name = fit_monster_name(monster_name(m), area_width as usize, fixed_width);
            format!("{prefix}{name}{hp_suffix}")
        }
        None => String::new(),
    }
}

fn hp_bar(hp: i32, max: i32, width: usize) -> String {
    if max <= 0 {
        return String::new();
    }
    let filled = ((hp.max(0) as f32 / max as f32) * width as f32).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width);
    for i in 0..width {
        s.push(if i < filled { theme::BAR_FULL } else { theme::BAR_EMPTY });
    }
    s
}

fn monster_name(m: &Monster) -> &'static str {
    match (m.terrain, m.elite) {
        (Terrain::Forest, true) => "強化された狼",
        (Terrain::Forest, false) => "狼",
        (Terrain::Mountain, _) => "ゴーレム",
        (Terrain::Graveyard, _) => "スケルトン",
        (Terrain::Meadow, _) => "?",
    }
}

/// 道の1マス分の表示テキストとスタイルを決める。
/// 優先順位: モンスター > 勇者 > 地形 > 空き道。
///
/// モンスターは湧いた地形マスに勇者が到達して初めて発生し、勇者は倒すまで
/// 同じマスに留まって戦う。つまり「モンスターがいるマス」は常に「勇者が
/// いるマス」でもある。勇者を最優先で表示すると敵が常に隠れてしまうため、
/// 交戦中は敵の姿を勇者色(黄背景)で強調して見せる。
fn cell_visual(state: &LoopMarchState, path_index: usize) -> (String, Style) {
    let (text, style) = cell_visual_base(state, path_index);
    if state.cursor == path_index {
        // キーボード操作用カーソル。地形/モンスター/勇者の表示はそのまま
        // 保ちつつ、下線だけ重ねて現在地を示す。
        (text, style.add_modifier(Modifier::UNDERLINED))
    } else {
        (text, style)
    }
}

fn cell_visual_base(state: &LoopMarchState, path_index: usize) -> (String, Style) {
    let slot = &state.path[path_index];
    let hero_here = state.hero.position == path_index;

    if let Some(m) = &slot.monster {
        let ch = match (m.terrain, m.elite) {
            (Terrain::Forest, true) => 'W',
            (Terrain::Forest, false) => 'w',
            (Terrain::Mountain, _) => 'g',
            (Terrain::Graveyard, _) => 's',
            (Terrain::Meadow, _) => '?',
        };
        let style = if hero_here {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        };
        return (format!("{ch} "), style);
    }

    if hero_here {
        return (
            "@ ".to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }

    match slot.terrain {
        Some(Terrain::Forest) => {
            // 森が2つ以上隣接している (シナジー成立) と見た目を変える。
            // 「なぜ」は説明しない — 気付いたプレイヤーへの発見の余地として残す。
            let clustered = logic::forest_cluster_size(&state.path, path_index) >= 2;
            let style = if clustered {
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            (format!("{} ", Terrain::Forest.symbol()), style)
        }
        Some(t) => (format!("{} ", t.symbol()), Style::default().fg(t.color())),
        None => (". ".to_string(), Style::default().fg(Color::DarkGray)),
    }
}

fn render_ring(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut lines: Vec<Line> = Vec::with_capacity(RING_H);
    for gy in 0..RING_H {
        let mut spans: Vec<Span> = Vec::with_capacity(RING_W);
        for gx in 0..RING_W {
            // 座標⇔道インデックスの変換は logic::ring_index_at を単一の
            // ソースとする (mod.rs のクリック判定と描画をずれさせない)。
            let (text, style) = match logic::ring_index_at(gx, gy) {
                Some(i) => cell_visual(state, i),
                None => ("  ".to_string(), Style::default()),
            };
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(format!(" 道 (第{}周) ", state.lap + 1));

    let grid = ClickableGrid::new(RING_W, RING_H, PATH_CLICK_BASE, 2);
    {
        let mut cs = click_state.borrow_mut();
        grid.register_targets(area, &block, &mut cs, 0);
    }

    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn render_hand(
    state: &LoopMarchState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " カードを選んで→道をタップで配置",
        Style::default().fg(Color::DarkGray),
    )));
    for (i, card) in state.hand.iter().enumerate() {
        match card {
            Some(t) => {
                let selected = state.selected_hand == Some(i);
                let style = if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(t.color())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.color()).add_modifier(Modifier::BOLD)
                };
                let marker = if selected { "▶" } else { " " };
                let hint_style = if selected {
                    Style::default().fg(Color::Black).bg(t.color())
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                cl.push_clickable(
                    Line::from(vec![
                        Span::styled(format!(" {marker}[{}] {} ", i + 1, t.name()), style),
                        Span::styled(t.resource_hint(), hint_style),
                    ]),
                    HAND_CLICK_BASE + i as u16,
                );
            }
            None => {
                cl.push(Line::from(Span::styled(
                    format!("  [{}] (空)", i + 1),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    cl.push(Line::from(""));

    let refill_ready = state.wood >= REFILL_WOOD_COST && state.stone >= REFILL_STONE_COST;
    let refill_color = if refill_ready { Color::Cyan } else { Color::DarkGray };
    cl.push_clickable(
        Line::from(Span::styled(
            format!(" 補充 (木材{REFILL_WOOD_COST}/石材{REFILL_STONE_COST})"),
            Style::default().fg(refill_color),
        )),
        REFILL_HAND,
    );
    cl.push_clickable(
        Line::from(Span::styled(" 拠点へ戻る", Style::default().fg(Color::Gray))),
        GO_TO_CAMP,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 手札 ");
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(cl, &state.hand_scroll, HAND_SCROLL_UP, HAND_SCROLL_DOWN)
        .block(block)
        .arrow_color(Color::DarkGray)
        .render(f, area, &mut cs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    #[test]
    fn hp_bar_lengths_correct() {
        assert_eq!(hp_bar(0, 30, 8), "░░░░░░░░");
        assert_eq!(hp_bar(30, 30, 8), "████████");
        assert_eq!(hp_bar(15, 30, 8), "████░░░░");
        // Negative HP clamps to empty.
        assert_eq!(hp_bar(-5, 30, 8), "░░░░░░░░");
    }

    #[test]
    fn compute_expedition_layout_wide_and_narrow_both_fit_within_area() {
        let area = Rect::new(0, 0, 80, 30);
        let wide = compute_expedition_layout(area, false);
        assert!(wide.header.height > 0);
        assert!(wide.ring.height > 0);
        assert!(wide.hand.height > 0);
        assert!(wide.log.height > 0);

        let narrow = compute_expedition_layout(area, true);
        assert!(narrow.header.height > 0);
        assert!(narrow.ring.height > 0);
        assert!(narrow.hand.height > 0);
        assert!(narrow.log.height > 0);
    }

    /// 回帰テスト (Codexレビュー指摘): "VS" という文字列の有無だけを見る
    /// アサーションだと、名前やHPの桁が実際に見切れていても検出できない
    /// (以前のテストがまさにその穴を突かれた)。narrow (30桁) の通常プレイ
    /// 相当の値なら、敵の名前とHPが両方省略されずに表示されることを、
    /// Buffer 再構成 (全角文字を挟むと空白セルが混ざり文字列比較が壊れる)
    /// を経由せず、実際に描画へ渡すテキストそのもので直接確認する。
    #[test]
    fn narrow_combat_text_shows_full_enemy_name_and_hp_at_typical_stats() {
        let monster = Monster {
            terrain: Terrain::Graveyard, // monster_name() = "スケルトン"
            hp: 15,
            max_hp: 15,
            attack: 1,
            elite: false,
        };
        let hp_text = " HP30/30 ";
        let text = narrow_combat_text(Some(&monster), hp_text, 30);
        assert_eq!(text, "VS スケルトン 15/15", "敵の名前とHPが両方省略されずに表示されるはず");
    }

    /// 回帰テスト (Codexレビュー指摘): 拠点強化でHPが育って桁数が増えても、
    /// 戦況判断に必須の敵HP数値だけは絶対に見切れないことを確認する
    /// (見切れて良いのは残り幅を使い切った時の名前の方のみ)。
    #[test]
    fn narrow_combat_text_never_truncates_enemy_hp_even_with_large_numbers() {
        let monster = Monster {
            terrain: Terrain::Forest,
            hp: 150,
            max_hp: 150,
            attack: 1,
            elite: true, // monster_name() = "強化された狼" (最長級の名前)
        };
        let hp_text = " HP130/130 "; // 勇者のHPも3桁まで育った想定
        let text = narrow_combat_text(Some(&monster), hp_text, 30);
        assert!(
            text.ends_with("150/150"),
            "名前が切り詰められることはあっても、敵のHP数値は見切れないはず: {text:?}"
        );
        assert!(
            display_width(hp_text) + display_width(&text) <= 30,
            "30桁幅に収まらなければならない: hp_text={hp_text:?} combat={text:?}"
        );
    }

    #[test]
    fn fit_monster_name_truncates_by_display_width_not_char_count() {
        // "スケルトン" は全角5文字 = 表示幅10。文字数ではなく表示幅で
        // 予算判定することを確認する (全角文字は1文字で幅2を消費する)。
        assert_eq!(fit_monster_name("スケルトン", 30, 10), "スケルトン", "幅に余裕があれば切り詰めない");
        assert_eq!(fit_monster_name("スケルトン", 12, 10), "ス", "残り幅2 (全角1文字分) だけ表示する");
        assert_eq!(fit_monster_name("スケルトン", 10, 10), "", "残り幅0なら空文字になる");
        assert_eq!(fit_monster_name("スケルトン", 5, 10), "", "予算がマイナスになっても panic しない");
    }

    /// 回帰テスト (Codexレビュー指摘): 周回数テキストは全角文字を含むため
    /// 見た目以上に表示幅を消費する ("第1周 (自己ベスト0周)" だけで22桁)。
    /// 以前は周回数を先に置いていたため、1周目・自己ベスト0周という
    /// 最短の値でも narrow (30桁) で line2 が溢れ、後ろに置いた DEF が
    /// 見切れていた。ATK/DEF を先頭に固定したので、ratatui が Span を
    /// 先頭から順に描画する性質上、桁数がどう増えても必ず全て表示される
    /// (溢れた分は後続の周回数テキストの方が削れる)。
    #[test]
    fn narrow_line2_never_truncates_atk_def_even_with_typical_lap_text() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        let defense = logic::mountain_synergy_defense(&state.path);

        let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
        let completed = terminal
            .draw(|f| {
                render_header(&state, f, Rect::new(0, 0, 30, 5), true);
            })
            .unwrap();

        // line2 は area の3行目 (y=2)。ATK/DEF は line2 の先頭 (全角文字より
        // 前) に置かれる純粋ASCIIなので、Buffer再構成でも安全に検証できる。
        let line: String = (0..30)
            .map(|x| completed.buffer.cell((x, 2)).map(|c| c.symbol()).unwrap_or(" "))
            .collect();
        assert!(
            line.contains(&format!("ATK{} DEF{defense}", state.hero.attack)),
            "ATK/DEFは先頭に置かれるので常に全て表示されるはず: {line:?}"
        );
    }

    #[test]
    fn ring_click_targets_match_rendered_cells() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|f| {
                render_ring(&state, f, Rect::new(0, 0, 20, RING_H as u16 + 2), &click_state);
            })
            .unwrap();

        // Borders::ALL → inner は (1,1) から。cell_display_width=2 なので
        // 先頭セル (リング index 0 = gx0,gy0) は (1,1) にある。
        let cs = click_state.borrow();
        assert_eq!(cs.hit_test(1, 1), Some(PATH_CLICK_BASE));
    }

    #[test]
    fn cell_visual_shows_hero_marker_at_hero_position() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        let (text, _) = cell_visual(&state, state.hero.position);
        assert_eq!(text.trim(), "@");
    }

    #[test]
    fn cell_visual_shows_terrain_symbol_when_no_monster() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.path[5].terrain = Some(Terrain::Forest);
        let (text, _) = cell_visual(&state, 5);
        assert_eq!(text.trim(), Terrain::Forest.symbol().to_string());
    }

    #[test]
    fn cell_visual_highlights_clustered_forest_differently_from_isolated() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.path[5].terrain = Some(Terrain::Forest);
        let (_, isolated_style) = cell_visual(&state, 5);

        state.path[6].terrain = Some(Terrain::Forest);
        let (_, clustered_style) = cell_visual(&state, 5);

        assert_ne!(
            isolated_style, clustered_style,
            "隣接森ができた前後で見た目が変わらないと、プレイヤーがシナジーに気付く手がかりが無い"
        );
    }

    #[test]
    fn cell_visual_shows_monster_glyph_over_terrain() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.path[5].terrain = Some(Terrain::Forest);
        state.path[5].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: false,
        });
        let (text, _) = cell_visual(&state, 5);
        assert_eq!(text.trim(), "w");
    }

    /// 戦闘は勇者が敵のマスに留まって行われる (`resolve_combat_tick` は
    /// `hero.position` のマスしか見ない) — つまり敵がいるマスは常に勇者が
    /// いるマスでもある。勇者を最優先で表示すると敵の姿が実プレイで一度も
    /// 見えなくなるため、この共存ケースの表示を固定する回帰テスト。
    #[test]
    fn cell_visual_shows_monster_glyph_even_when_hero_is_fighting_it() {
        let mut state = LoopMarchState::new();
        logic::start_or_resume_expedition(&mut state);
        state.hero.position = 5;
        state.path[5].monster = Some(Monster {
            terrain: Terrain::Forest,
            hp: 5,
            max_hp: 5,
            attack: 2,
            elite: false,
        });
        let (text, _) = cell_visual(&state, 5);
        assert_eq!(
            text.trim(),
            "w",
            "勇者と同じマスでも敵の姿が見えないと何と戦っているか分からない"
        );
    }
}
