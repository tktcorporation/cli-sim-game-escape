//! Hub (main screen with tabs) rendering.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{ClickableList, TabBar};

use super::super::actions::*;
use super::super::characters::CharacterId;
use super::super::gacha::{card_def, GACHA_SINGLE_COST, GACHA_TEN_COST};
use super::super::produce::PRODUCE_STAMINA_COST;
use super::super::scenario;
use super::super::social_sys::{self, STAMINA_MAX};
use super::super::state::{CafeState, HubTab, AP_MAX};

pub(super) fn render_hub(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let header_h = if is_narrow { 4 } else { 3 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h), // header (narrow: 2 行 + borders)
            Constraint::Length(2),        // tab bar
            Constraint::Min(8),           // content
        ])
        .split(area);

    // Header — narrow ではステータスを 2 行に折り返す
    let header_lines = if is_narrow {
        vec![
            Line::from(vec![
                Span::styled(format!(" Rank {} ", state.player_rank.level), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(format!("│ ¥{}", state.money), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled(format!(" 💎{} ", state.card_state.gems), Style::default().fg(Color::Cyan)),
                Span::styled(format!("│ AP {}/{}", state.ap_current, AP_MAX), Style::default().fg(Color::Green)),
            ]),
        ]
    } else {
        vec![Line::from(vec![
            Span::styled(format!(" Rank {} ", state.player_rank.level), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(format!("│ ¥{} ", state.money), Style::default().fg(Color::White)),
            Span::styled(format!("│ 💎{} ", state.card_state.gems), Style::default().fg(Color::Cyan)),
            Span::styled(format!("│ AP {}/{}", state.ap_current, AP_MAX), Style::default().fg(Color::Green)),
        ])]
    };
    let header_borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let header = Paragraph::new(header_lines).block(
        Block::default()
            .borders(header_borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 月灯り "),
    );
    f.render_widget(header, chunks[0]);

    // Tab bar
    let tabs = [
        (HubTab::Home, "ホーム", TAB_HOME),
        (HubTab::Characters, "常連", TAB_CHARACTERS),
        (HubTab::Cards, "カード", TAB_CARDS),
        (HubTab::Produce, "P営業", TAB_PRODUCE),
        (HubTab::Missions, "任務", TAB_MISSIONS),
    ];

    // Build the tab bar via the shared `TabBar` widget so that click rects
    // are computed from real CJK-aware label widths instead of `area.width
    // / tabs.len()`.  The label is wrapped in `[...]` to preserve the
    // existing visual exactly (TabBar pads each label with a single space
    // on each side, so passing `[ホーム]` renders as ` [ホーム] `).
    {
        let mut bar = TabBar::new("").block(
            Block::default().borders(Borders::BOTTOM),
        );
        for (tab, name, id) in &tabs {
            let style = if state.hub_tab == *tab {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            bar = bar.tab(format!("[{name}]"), style, *id);
        }
        bar.render(f, chunks[1], &mut click_state.borrow_mut());
    }

    // Content area
    match state.hub_tab {
        HubTab::Home => render_hub_home(state, f, chunks[2], click_state),
        HubTab::Characters => render_hub_characters(state, f, chunks[2], click_state),
        HubTab::Cards => render_hub_cards(state, f, chunks[2], click_state),
        HubTab::Produce => render_hub_produce(state, f, chunks[2], click_state),
        HubTab::Missions => render_hub_missions(state, f, chunks[2], click_state),
    }
}

fn render_hub_home(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" Day {} │ Rank {} (EXP {}/{})", state.day, state.player_rank.level, state.player_rank.exp, state.player_rank.exp_to_next()),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(Span::styled(
        format!(" 所持金 ¥{} │ 💎{} │ 🪙{}", state.money, state.card_state.gems, state.card_state.coins),
        Style::default().fg(Color::Cyan),
    )));
    cl.push(Line::from(""));

    // Story
    let next_ch = super::super::logic::next_available_chapter(state);
    if let Some(ch) = next_ch {
        let title = scenario::chapter_title(ch);
        cl.push_clickable(Line::from(Span::styled(
            format!(" ▶ Ch.{ch} 「{title}」を読む"),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )), HUB_STORY);
    } else {
        cl.push(Line::from(Span::styled(" (次のチャプターはまだ解放されていません)", Style::default().fg(Color::DarkGray))));
    }
    cl.push(Line::from(""));

    // Character interaction
    cl.push_clickable(Line::from(Span::styled(" ▶ 常連客と交流する", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))), CHARACTER_BASE);
    cl.push(Line::from(""));

    // Business
    let enough = state.stamina.current >= super::super::social_sys::BUSINESS_DAY_COST;
    if enough {
        cl.push_clickable(Line::from(Span::styled(
            format!(" ▶ 営業する (予算-{})", super::super::social_sys::BUSINESS_DAY_COST),
            Style::default().fg(Color::Green),
        )), HUB_BUSINESS);
    } else {
        cl.push(Line::from(Span::styled(
            format!(" × 予算不足 ({}/{})", state.stamina.current, super::super::social_sys::BUSINESS_DAY_COST),
            Style::default().fg(Color::DarkGray),
        )));
    }
    cl.push(Line::from(""));

    // Produce shortcut
    let produce_enough = state.stamina.current >= PRODUCE_STAMINA_COST;
    if produce_enough {
        cl.push_clickable(Line::from(Span::styled(
            format!(" ▶ プロデュース営業 (予算-{PRODUCE_STAMINA_COST})"),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )), HUB_PRODUCE);
    } else {
        cl.push(Line::from(Span::styled(
            format!(" × プロデュース予算不足 ({}/{PRODUCE_STAMINA_COST})", state.stamina.current),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Memories
    if !state.memories.is_empty() {
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(format!(" 思い出: {}個獲得", state.memories.len()), Style::default().fg(Color::Magenta))));
    }

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)).title(" ホーム ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn render_hub_characters(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));

    let unlocked = state.unlocked_characters();
    for (i, ch) in unlocked.iter().enumerate() {
        let data = state.character_data.get(ch);
        let aff = state.affinities.get(ch);
        let (level, stars) = data.map(|d| (d.level, d.stars)).unwrap_or((1, 1));
        let aff_level = aff.map(|a| a.axes.level()).unwrap_or(0);
        let star_str = "★".repeat(stars as usize);
        cl.push_clickable(Line::from(vec![
            Span::styled(format!(" {}. ", i + 1), Style::default().fg(Color::Yellow)),
            Span::styled(ch.name(), Style::default().fg(Color::White)),
            Span::styled(format!("  {star_str}"), Style::default().fg(Color::Yellow)),
            Span::styled(format!(" Lv.{level}"), Style::default().fg(Color::Cyan)),
            Span::styled(format!(" 好感度Lv.{aff_level}"), Style::default().fg(Color::Magenta)),
        ]), CHARACTER_BASE + i as u16);

        // Show shards for promotion
        if let Some(d) = data {
            if let Some(cost) = d.shards_to_promote() {
                cl.push(Line::from(Span::styled(
                    format!("     欠片: {}/{} (★昇格)", d.shards, cost),
                    Style::default().fg(if d.shards >= cost { Color::Green } else { Color::DarkGray }),
                )));
            }
        }
    }

    for ch in CharacterId::ALL {
        if !unlocked.contains(ch) {
            cl.push(Line::from(Span::styled(
                format!("   ??? (Ch.{}で解放)", ch.unlock_chapter()),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)).title(" 常連客 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_hub_cards(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        format!(" 💎{} │ 🪙{}", state.card_state.gems, state.card_state.coins),
        Style::default().fg(Color::Cyan),
    )));

    // Spark progress
    cl.push(Line::from(Span::styled(
        format!(" 天井: {}/200", state.card_state.banner_pulls),
        Style::default().fg(Color::Magenta),
    )));
    cl.push(Line::from(""));

    // Equipped card — narrow では 2 行に分けて溢れないようにする
    if let Some(idx) = state.card_state.equipped_card {
        if let Some(owned) = state.card_state.cards.get(idx) {
            if let Some(def) = card_def(owned.card_id) {
                if is_narrow {
                    cl.push(Line::from(Span::styled(
                        format!(" 装備中: {} {}", def.rarity.label(), def.name),
                        Style::default().fg(Color::Yellow),
                    )));
                    cl.push(Line::from(Span::styled(
                        format!("   Lv.{} (x{:.2})", owned.level, owned.multiplier()),
                        Style::default().fg(Color::Yellow),
                    )));
                } else {
                    cl.push(Line::from(Span::styled(
                        format!(" 装備中: {} {} Lv.{} (x{:.2})", def.rarity.label(), def.name, owned.level, owned.multiplier()),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
        }
    } else {
        cl.push(Line::from(Span::styled(" 装備中: なし", Style::default().fg(Color::DarkGray))));
    }
    cl.push(Line::from(""));

    // Gacha buttons
    if !state.card_state.daily_draw_used {
        cl.push_clickable(Line::from(Span::styled(" ▶ デイリードロー (無料)", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))), CARD_DAILY_DRAW);
    } else {
        cl.push(Line::from(Span::styled(" ✓ デイリードロー済み", Style::default().fg(Color::DarkGray))));
    }
    if state.card_state.gems >= GACHA_SINGLE_COST {
        cl.push_clickable(Line::from(Span::styled(
            format!(" ▶ ガチャ単発 (💎{})", GACHA_SINGLE_COST),
            Style::default().fg(Color::Cyan),
        )), CARD_GACHA_SINGLE);
    }
    if state.card_state.gems >= GACHA_TEN_COST {
        cl.push_clickable(Line::from(Span::styled(
            format!(" ▶ ガチャ10連 (💎{})", GACHA_TEN_COST),
            Style::default().fg(Color::Cyan),
        )), CARD_GACHA_TEN);
    }
    cl.push(Line::from(""));

    // Card list ( ▶ ★★★ 名前 Lv.10 形式 — 最長 ~28 セルなので 32 列 narrow に収まる)
    for (i, owned) in state.card_state.cards.iter().enumerate().take(15) {
        if let Some(def) = card_def(owned.card_id) {
            let equipped = state.card_state.equipped_card == Some(i);
            let marker = if equipped { "▶" } else { " " };
            cl.push_clickable(Line::from(vec![
                Span::styled(format!(" {marker} "), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{} ", def.rarity.label()), Style::default().fg(Color::Magenta)),
                Span::styled(def.name, Style::default().fg(Color::White)),
                Span::styled(format!(" Lv.{}", owned.level), Style::default().fg(Color::Cyan)),
            ]), CARD_EQUIP_BASE + i as u16);
        }
    }

    let borders = if is_narrow { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };
    let block = Block::default().borders(borders).border_style(Style::default().fg(Color::Magenta)).title(" カード ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn render_hub_produce(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(" プロデュース営業", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(Span::styled(" 常連客を選んで5ターンの特訓！", Style::default().fg(Color::White))));
    cl.push(Line::from(Span::styled(
        format!(" 予算消費: {} │ 現在: {}/{}", PRODUCE_STAMINA_COST, state.stamina.current, STAMINA_MAX),
        Style::default().fg(Color::Cyan),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" 累計プロデュース: {}回", state.total_produce_completions),
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    let enough = state.stamina.current >= PRODUCE_STAMINA_COST;
    if enough {
        cl.push_clickable(Line::from(Span::styled(
            " ▶ プロデュース開始",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )), HUB_PRODUCE);
    } else {
        cl.push(Line::from(Span::styled(" × 予算不足", Style::default().fg(Color::DarkGray))));
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(" ランク目安:", Style::default().fg(Color::Yellow))));
    cl.push(Line::from(Span::styled("  C(~49) B(50~) A(100~) S(150~) SS(200~)", Style::default().fg(Color::DarkGray))));

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)).title(" プロデュース ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn render_hub_missions(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let mut cl = ClickableList::new();
    let now = social_sys::now_ms();
    let stamina = &state.stamina;

    // Stamina gauge
    let gauge_w = 20u32;
    let filled = (stamina.current as f64 / STAMINA_MAX as f64 * gauge_w as f64) as u32;
    let empty = gauge_w - filled;
    let gauge: String = "\u{2588}".repeat(filled as usize) + &"\u{2591}".repeat(empty as usize);
    let stam_color = if stamina.current >= 40 { Color::Green } else if stamina.current >= 20 { Color::Yellow } else { Color::Red };
    let recovery = if stamina.current < STAMINA_MAX { format!(" (全回復: {}分)", stamina.minutes_to_full(now)) } else { " (MAX)".into() };
    cl.push(Line::from(vec![
        Span::styled(" 予算: ", Style::default().fg(Color::Cyan)),
        Span::styled(gauge, Style::default().fg(stam_color)),
        Span::styled(format!(" {}/{}{}", stamina.current, STAMINA_MAX, recovery), Style::default().fg(stam_color)),
    ]));
    cl.push(Line::from(""));

    // Daily Missions
    cl.push(Line::from(Span::styled(" デイリーミッション", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    for m in &state.daily_missions.missions {
        let check = if m.completed { "✓" } else { " " };
        let style = if m.completed { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
        let gem_str = if m.reward_gems > 0 { format!(" 💎{}", m.reward_gems) } else { String::new() };
        cl.push(Line::from(vec![
            Span::styled(format!(" [{check}] "), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{} {}/{}", m.name, m.progress, m.target), style),
            Span::styled(format!("  ¥{}{}", m.reward_money, gem_str), if m.completed { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Green) }),
        ]));
    }
    if state.daily_missions.all_complete() && !state.daily_missions.all_clear_claimed {
        cl.push(Line::from(Span::styled(" ★ 全達成ボーナス ¥500 + 💎100", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    }

    cl.push(Line::from(""));

    // Weekly Missions
    cl.push(Line::from(Span::styled(" ウィークリーミッション", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    for m in &state.weekly_missions.missions {
        let check = if m.completed { "✓" } else { " " };
        let style = if m.completed { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::White) };
        let gem_str = if m.reward_gems > 0 { format!(" 💎{}", m.reward_gems) } else { String::new() };
        cl.push(Line::from(vec![
            Span::styled(format!(" [{check}] "), Style::default().fg(Color::Cyan)),
            Span::styled(format!("{} {}/{}", m.name, m.progress, m.target), style),
            Span::styled(format!("  ¥{}{}", m.reward_money, gem_str), if m.completed { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Green) }),
        ]));
    }

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)).title(" 任務・ステータス ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;
    use super::super::super::gacha::cards::Rarity;
    use super::super::super::gacha::state::OwnedCard;
    use super::super::super::state::HubTab;

    /// 32 列幅でカードタブを描画したとき、ガチャボタン (デイリードロー、
    /// 単発、10連) がクリック登録され、装備中カード行が幅を超えないことを確認。
    /// (もばいるでガチャの表示が崩れるリグレッション防止)
    #[test]
    fn hub_cards_renders_at_narrow_width() {
        let mut state = CafeState::new();
        state.phase = super::super::super::state::GamePhase::Hub;
        state.hub_tab = HubTab::Cards;
        state.card_state.gems = 5000;
        state.player_rank.level = 99;
        state.money = 9_999_999;
        state.ap_current = 100;
        // 装備中カードに最長 description のもの (id=20 月灯りの記憶) をセット
        state.card_state.cards.push(OwnedCard {
            card_id: 20,
            level: 10,
            rank_ups: 0,
            duplicates: 0,
        });
        state.card_state.equipped_card = Some(0);

        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(32, 40)).unwrap();
        terminal
            .draw(|f| render_hub(&state, f, f.area(), &cs))
            .unwrap();

        // ガチャ系ボタンのクリック登録
        let cs = cs.borrow();
        let mut found_daily = false;
        let mut found_single = false;
        let mut found_ten = false;
        for y in 0..40 {
            for x in 0..32 {
                match cs.hit_test(x, y) {
                    Some(CARD_DAILY_DRAW) => found_daily = true,
                    Some(CARD_GACHA_SINGLE) => found_single = true,
                    Some(CARD_GACHA_TEN) => found_ten = true,
                    _ => {}
                }
            }
        }
        assert!(found_daily, "CARD_DAILY_DRAW button missing at narrow width");
        assert!(found_single, "CARD_GACHA_SINGLE button missing at narrow width");
        assert!(found_ten, "CARD_GACHA_TEN button missing at narrow width");

        // 装備中カードの行 (rarity + name) が 32 列に収まる
        let def = super::super::super::gacha::card_def(20).unwrap();
        let equipped_line = format!(" 装備中: {} {}", def.rarity.label(), def.name);
        assert!(
            ratzilla::ratatui::text::Line::from(equipped_line.as_str()).width() <= 32,
            "equipped card name+rarity overflows 32 cells"
        );
        // Rarity が Star3 であることも確認 (テストデータの妥当性)
        assert_eq!(def.rarity, Rarity::Star3);
    }

    /// ヘッダーが 32 列幅で 2 行レイアウトに切り替わり、
    /// 各行が 32 列に収まることを確認。
    #[test]
    fn hub_header_fits_at_narrow_width() {
        // 最悪値 (Rank 99 / ¥9999999 / 💎99999 / AP 100/100)
        let line1 = " Rank 99 │ ¥9999999".to_string();
        let line2 = " 💎99999 │ AP 100/100".to_string();
        let w1 = ratzilla::ratatui::text::Line::from(line1.as_str()).width();
        let w2 = ratzilla::ratatui::text::Line::from(line2.as_str()).width();
        assert!(w1 <= 32, "header line1 width = {w1}");
        assert!(w2 <= 32, "header line2 width = {w2}");
    }

    /// 32 列幅では narrow レイアウトが選択されることを境界値で確認 (input::is_narrow_layout が `< 60`)。
    #[test]
    fn narrow_threshold_applies_to_mobile_widths() {
        assert!(crate::input::is_narrow_layout(32));
        assert!(crate::input::is_narrow_layout(40));
        assert!(!crate::input::is_narrow_layout(60));
    }
}
