//! Pure functions for Café game state transitions.

use super::scenario::PROLOGUE_SCENES;
use super::state::{CafeState, CustomerVisit, GamePhase};

/// Advance the story by one line. Returns true if consumed.
pub fn advance_story(state: &mut CafeState) -> bool {
    if state.phase != GamePhase::Story {
        return false;
    }

    let scene_count = PROLOGUE_SCENES.len();
    if state.current_scene_index >= scene_count {
        // All scenes done → transition to business phase
        state.story_complete = true;
        state.phase = GamePhase::Business;
        return true;
    }

    let scene = PROLOGUE_SCENES[state.current_scene_index];
    if state.current_line_index + 1 < scene.lines.len() {
        // More lines in this scene
        state.current_line_index += 1;
    } else {
        // Scene complete → next scene
        state.current_scene_index += 1;
        state.current_line_index = 0;

        if state.current_scene_index >= scene_count {
            state.story_complete = true;
            state.phase = GamePhase::Business;
        }
    }
    true
}

/// Run a simplified day of business. Generates customer visits based on menu.
pub fn run_business_day(state: &mut CafeState) {
    state.today_visits.clear();

    // Simplified: 3-5 customers per day
    let customer_count = 3 + (state.day as usize % 3);
    let customer_names = ["サラリーマン", "OL", "学生", "主婦", "老紳士"];

    for i in 0..customer_count {
        let name = customer_names[i % customer_names.len()];
        let menu_idx = i % state.menu.len();
        let item = &state.menu[menu_idx];

        let visit = CustomerVisit {
            name,
            order: item.name,
            satisfied: true,
            revenue: item.price,
        };
        state.today_visits.push(visit);
    }

    let revenue = state.today_revenue() as i64;
    let cost = state.today_cost() as i64;
    state.money += revenue - cost;
    state.total_customers_served += customer_count as u32;
    state.phase = GamePhase::DayResult;
}

/// End the day results and start the next day.
pub fn next_day(state: &mut CafeState) {
    state.day += 1;
    state.today_visits.clear();
    state.phase = GamePhase::Business;
}
