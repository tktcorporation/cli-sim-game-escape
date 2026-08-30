//! Tiny Factory — 開口の「次: Minerを設置」とツール選択 CTA。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::factory::actions::{GRID_CLICK_BASE, SELECT_BELT, SELECT_MINER};
use crate::games::factory::grid::{Cell, MachineKind, VIEW_W};
use crate::games::factory::logic;
use crate::games::factory::render;
use crate::games::factory::state::{FactoryState, PlacementTool};
use crate::widgets::ClickableGrid;

pub struct FactorySubject {
    state: FactoryState,
}

impl FactorySubject {
    pub fn new() -> Self {
        Self {
            state: FactoryState::new(),
        }
    }

    fn machine_count(&self, kind: MachineKind) -> u32 {
        self.state
            .grid
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| matches!(c, Cell::Machine(m) if m.kind == kind))
            .count() as u32
    }

    fn belt_count(&self) -> u32 {
        self.state
            .grid
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| matches!(c, Cell::Belt(_)))
            .count() as u32
    }
}

impl Subject for FactorySubject {
    fn name(&self) -> &'static str {
        "factory"
    }

    fn probe(&self) -> ProbeFacts {
        let goal = logic::next_build_goal(&self.state);
        let (primary_id, primary_label) = match &self.state.tool {
            PlacementTool::None if self.machine_count(MachineKind::Miner) == 0 => {
                (SELECT_MINER, "Miner")
            }
            PlacementTool::Miner if self.machine_count(MachineKind::Miner) == 0 => {
                // 設置そのものはグリッド。ラベルはヘッダー目標と揃える。
                (GRID_CLICK_BASE, "Miner")
            }
            PlacementTool::None | PlacementTool::Belt
                if self.machine_count(MachineKind::Miner) > 0 && self.belt_count() == 0 =>
            {
                if matches!(self.state.tool, PlacementTool::Belt) {
                    // Miner(0,0) の右隣 — suggest_action と同じマス
                    (GRID_CLICK_BASE + 2, "Belt")
                } else {
                    (SELECT_BELT, "Belt")
                }
            }
            _ => (SELECT_MINER, "Miner"),
        };
        ProbeFacts {
            phase: "floor".into(),
            next_goal: Some(goal),
            actions: vec![ActionFact {
                id: primary_id,
                label: primary_label.into(),
                hint: None,
                primary: true,
            }],
            progress: vec![
                ("money".into(), self.state.money as f64),
                ("exported".into(), self.state.total_exported as f64),
                (
                    "miners".into(),
                    self.machine_count(MachineKind::Miner) as f64,
                ),
                ("belts".into(), self.belt_count() as f64),
                (
                    "tool".into(),
                    match self.state.tool {
                        PlacementTool::None => 0.0,
                        PlacementTool::Miner => 1.0,
                        PlacementTool::Belt => 2.0,
                        PlacementTool::Smelter => 3.0,
                        PlacementTool::Exporter => 4.0,
                        PlacementTool::Assembler => 5.0,
                        PlacementTool::Fabricator => 6.0,
                        PlacementTool::Delete => 7.0,
                    },
                ),
            ],
            recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
        }
    }

    fn capture(&self, width: u16, height: u16) -> ScreenSnapshot {
        capture_frame(width, height, |f, cs| {
            render::render(&self.state, f, f.area(), cs);
        })
    }

    fn tick(&mut self, n: u32) {
        logic::tick_n(&mut self.state, n);
    }

    fn apply_action(&mut self, action_id: u16) -> bool {
        match action_id {
            SELECT_MINER => {
                logic::select_tool(&mut self.state, PlacementTool::Miner);
                true
            }
            SELECT_BELT => {
                logic::select_tool(&mut self.state, PlacementTool::Belt);
                true
            }
            id if id >= GRID_CLICK_BASE => {
                if let Some((vx, vy)) = ClickableGrid::decode(GRID_CLICK_BASE, VIEW_W, id) {
                    self.state.cursor_x = self.state.viewport_x + vx;
                    self.state.cursor_y = self.state.viewport_y + vy;
                    logic::place(&mut self.state)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        let miners = self.machine_count(MachineKind::Miner);
        let belts = self.belt_count();
        if miners == 0 {
            if !matches!(self.state.tool, PlacementTool::Miner) {
                return Some(SELECT_MINER);
            }
            // 空きマスへ Miner (2×2) を置く
            return Some(GRID_CLICK_BASE);
        }
        if belts == 0 {
            if !matches!(self.state.tool, PlacementTool::Belt) {
                return Some(SELECT_BELT);
            }
            // Miner の右隣 (x=2, y=0) — viewport 原点想定
            let col = 2usize.min(VIEW_W - 1);
            return Some(GRID_CLICK_BASE + col as u16);
        }
        None
    }
}
