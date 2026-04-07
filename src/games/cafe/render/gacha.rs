//! Card screen and gacha result rendering.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::super::actions::*;
use super::super::gacha::{self, card_def};
use super::super::state::CafeState;

pub(super) fn render_card_screen(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    // Delegate to hub cards view
    super::hub::render_hub_cards(state, f, area, click_state);
}

pub(super) fn render_gacha_result(f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>, card_ids: &[u32]) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(" ガチャ結果！", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(""));

    for &id in card_ids {
        if let Some(def) = card_def(id) {
            let color = match def.rarity {
                gacha::Rarity::Star3 => Color::Yellow,
                gacha::Rarity::Star2 => Color::Cyan,
                gacha::Rarity::Star1 => Color::White,
            };
            cl.push(Line::from(vec![
                Span::styled(format!(" {} ", def.rarity.label()), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(def.name, Style::default().fg(color)),
                Span::styled(format!(" — {}", def.description), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(Span::styled(" ▶ OK", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))), GACHA_RESULT_OK);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).title(" ガチャ ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}
