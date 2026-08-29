//! 短時間セッションを回して開口スコア + 動的スコアを集める。

use crate::critique::metrics::{
    evaluate_opening, score_feedback, score_momentum, MetricScore,
};
use crate::critique::probe::Subject;

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub width: u16,
    pub height: u16,
    /// 自動操作の最大回数。0 なら開口だけ。
    pub max_actions: u32,
    /// 1操作あたり進める tick 数。
    pub ticks_per_action: u32,
    /// 進捗サンプリング間隔（action 単位）。
    pub sample_every_actions: u32,
}

impl SessionConfig {
    pub fn ci_default() -> Self {
        Self {
            width: 100,
            height: 40,
            max_actions: 8,
            ticks_per_action: 10,
            sample_every_actions: 1,
        }
    }

    pub fn opening_only() -> Self {
        Self {
            width: 100,
            height: 40,
            max_actions: 0,
            ticks_per_action: 0,
            sample_every_actions: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionReport {
    pub game: String,
    pub phase_at_open: String,
    pub opening: Vec<MetricScore>,
    pub session: Vec<MetricScore>,
}

pub fn run_session(subject: &mut dyn Subject, config: &SessionConfig) -> SessionReport {
    let opening_screen = subject.capture(config.width, config.height);
    let opening_facts = subject.probe();
    let opening = evaluate_opening(&opening_facts, &opening_screen);

    let mut feedback_scores = Vec::new();
    let mut stagnant = 0usize;
    let mut samples = 0usize;
    let mut last_progress = opening_facts.progress.clone();

    for step in 0..config.max_actions {
        let facts = subject.probe();
        let screen = subject.capture(config.width, config.height);
        let Some(action_id) = subject.suggest_action(&facts, &screen) else {
            break;
        };
        let before_text = screen.text.clone();
        let before_progress = facts.progress.clone();
        if !subject.apply_action(action_id) {
            break;
        }
        if config.ticks_per_action > 0 {
            subject.tick(config.ticks_per_action);
        }
        let after_facts = subject.probe();
        let after_screen = subject.capture(config.width, config.height);
        let fb = score_feedback(
            &before_progress,
            &after_facts.progress,
            &before_text,
            &after_screen.text,
            &after_facts.recent_feedback,
        );
        feedback_scores.push(fb.value);

        if config.sample_every_actions > 0 && (step + 1) % config.sample_every_actions == 0 {
            samples += 1;
            if !progress_moved(&last_progress, &after_facts.progress)
                && before_text == after_screen.text
            {
                stagnant += 1;
            }
            last_progress = after_facts.progress;
        }
    }

    let mut session = Vec::new();
    if !feedback_scores.is_empty() {
        let avg = feedback_scores.iter().sum::<f64>() / feedback_scores.len() as f64;
        session.push(MetricScore {
            kind: crate::critique::metrics::MetricKind::FeedbackResponsiveness,
            value: avg,
            note: format!("avg over {} actions", feedback_scores.len()),
        });
        session.push(score_momentum(stagnant, samples.max(1)));
    }

    SessionReport {
        game: subject.name().to_string(),
        phase_at_open: opening_facts.phase,
        opening,
        session,
    }
}

fn progress_moved(before: &[(String, f64)], after: &[(String, f64)]) -> bool {
    for (name, value) in after {
        match before.iter().find(|(n, _)| n == name) {
            Some((_, old)) if (old - value).abs() > f64::EPSILON => return true,
            None => return true,
            _ => {}
        }
    }
    false
}
