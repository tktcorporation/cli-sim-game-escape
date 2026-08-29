//! 「なんかよくわからん」を数値に落とすメトリクス。
//!
//! どれも 0.0..=1.0。低いほど「わかりにくい / 手応えが薄い」側。
//! 絶対値の完璧さより、ゲーム間比較と退行検知が目的。

use crate::critique::frame::ScreenSnapshot;
use crate::critique::probe::ProbeFacts;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MetricKind {
    /// 主操作・提示操作が画面上で辿れるか。
    AffordanceCoverage,
    /// `next_goal` が画面に出ているか。目標未設定ならスコア 1.0 とし note で明示。
    GoalVisibility,
    /// 情報密度が読みやすい帯にいるか。
    ReadableDensity,
    /// 同時に見える操作が多すぎないか。
    ChoiceLoad,
    /// 主操作のあと進捗 or 画面が動いたか（セッション側で埋める）。
    FeedbackResponsiveness,
    /// セッション中に進捗が止まった割合の逆。
    ProgressMomentum,
}

#[derive(Clone, Debug)]
pub struct MetricScore {
    pub kind: MetricKind,
    pub value: f64,
    pub note: String,
}

impl MetricKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AffordanceCoverage => "affordance",
            Self::GoalVisibility => "goal",
            Self::ReadableDensity => "density",
            Self::ChoiceLoad => "choice",
            Self::FeedbackResponsiveness => "feedback",
            Self::ProgressMomentum => "momentum",
        }
    }

    /// 低いときにプレイヤーが抱きやすい感想。
    pub fn low_score_means(self) -> &'static str {
        match self {
            Self::AffordanceCoverage => "何を押せばいいかわからん",
            Self::GoalVisibility => "何を目指せばいいかわからん",
            Self::ReadableDensity => "画面がスカスカか情報過多",
            Self::ChoiceLoad => "選択肢が多すぎて迷う",
            Self::FeedbackResponsiveness => "押しても何も起きてる感じがしない",
            Self::ProgressMomentum => "進んでる実感が無い",
        }
    }
}

/// 開口（操作前）で測れる指標。
pub fn evaluate_opening(facts: &ProbeFacts, screen: &ScreenSnapshot) -> Vec<MetricScore> {
    vec![
        score_affordance(facts, screen),
        score_goal_visibility(facts, screen),
        score_readable_density(screen),
        score_choice_load(screen),
    ]
}

fn score_affordance(facts: &ProbeFacts, screen: &ScreenSnapshot) -> MetricScore {
    let relevant: Vec<_> = facts
        .actions
        .iter()
        .filter(|a| a.primary || facts.actions.iter().filter(|x| x.primary).count() == 0)
        .collect();
    if relevant.is_empty() {
        return MetricScore {
            kind: MetricKind::AffordanceCoverage,
            value: 0.0,
            note: "probe に操作が無い".into(),
        };
    }

    let mut hit = 0usize;
    let mut details = Vec::new();
    for action in &relevant {
        let label_ok = screen.contains_needle(&action.label);
        let hint_ok = action
            .hint
            .map(|c| screen.contains_needle(&format!("[{c}]")))
            .unwrap_or(false);
        let target_ok = screen.has_action(action.id);
        let ok = (label_ok || hint_ok) && target_ok;
        if ok {
            hit += 1;
        } else {
            details.push(format!(
                "{}(label={} hint={} target={})",
                action.label, label_ok, hint_ok, target_ok
            ));
        }
    }
    let value = hit as f64 / relevant.len() as f64;
    let note = if details.is_empty() {
        format!("{hit}/{} primary actions reachable", relevant.len())
    } else {
        format!(
            "{hit}/{} ok; missing: {}",
            relevant.len(),
            details.join(", ")
        )
    };
    MetricScore {
        kind: MetricKind::AffordanceCoverage,
        value,
        note,
    }
}

fn score_goal_visibility(facts: &ProbeFacts, screen: &ScreenSnapshot) -> MetricScore {
    let Some(goal) = facts.next_goal.as_deref() else {
        return MetricScore {
            kind: MetricKind::GoalVisibility,
            value: 1.0,
            note: "probe に next_goal が無い（未設計扱い）".into(),
        };
    };
    // 全文一致を優先。折り返しで壊れる場合だけ、キーワードの過半数一致を
    // 認める（1語ヒットでは HUD の別行に騙されない）。
    let visible = screen.contains_needle(goal) || majority_keywords_visible(goal, screen);
    MetricScore {
        kind: MetricKind::GoalVisibility,
        value: if visible { 1.0 } else { 0.0 },
        note: if visible {
            format!("画面に目標の手がかりあり: {goal}")
        } else {
            format!("画面に目標が見えない: {goal}")
        },
    }
}

fn majority_keywords_visible(goal: &str, screen: &ScreenSnapshot) -> bool {
    let keys: Vec<&str> = goal_keywords(goal)
        .into_iter()
        .filter(|k| k.chars().count() >= 2)
        .collect();
    if keys.is_empty() {
        return false;
    }
    let hits = keys.iter().filter(|k| screen.contains_needle(k)).count();
    hits * 2 >= keys.len()
}

fn goal_keywords(goal: &str) -> Vec<&str> {
    // 日本語の助詞で雑に切る。精密な形態素解析はしない。
    goal.split([' ', '　', 'を', 'に', 'へ', 'が', 'は', 'と', 'の', '『', '』', '「', '」'])
        .filter(|s| !s.is_empty())
        .collect()
}

fn score_readable_density(screen: &ScreenSnapshot) -> MetricScore {
    let occ = screen.occupancy();
    // 読みやすい帯: 25%..=70%。スカスカも詰まりも減点。
    let value = if occ < 0.25 {
        (occ / 0.25).clamp(0.0, 1.0)
    } else if occ > 0.70 {
        ((1.0 - occ) / 0.30).clamp(0.0, 1.0)
    } else {
        1.0
    };
    MetricScore {
        kind: MetricKind::ReadableDensity,
        value,
        note: format!("occupancy={occ:.2}"),
    }
}

fn score_choice_load(screen: &ScreenSnapshot) -> MetricScore {
    // probe の列挙数ではなく、画面に登録されたクリック対象数を見る。
    // subject 作者が actions を絞っても、実画面の迷い度は変わらない。
    let n = screen.distinct_action_count();
    let value = if n <= 8 {
        1.0
    } else if n >= 24 {
        0.0
    } else {
        1.0 - (n - 8) as f64 / 16.0
    };
    MetricScore {
        kind: MetricKind::ChoiceLoad,
        value,
        note: format!("{n} click targets on screen"),
    }
}

/// 操作前後の progress / 画面 / ログ差分からフィードバックを採点する。
pub fn score_feedback(
    before_progress: &[(String, f64)],
    after_progress: &[(String, f64)],
    before_text: &str,
    after_text: &str,
    before_feedback: &[String],
    after_feedback: &[String],
) -> MetricScore {
    let progress_moved = progress_changed(before_progress, after_progress);
    let screen_moved = before_text != after_text;
    let log_moved = before_feedback != after_feedback;
    let value = if progress_moved {
        1.0
    } else if screen_moved && log_moved {
        0.85
    } else if screen_moved || log_moved {
        0.6
    } else {
        0.0
    };
    MetricScore {
        kind: MetricKind::FeedbackResponsiveness,
        value,
        note: format!(
            "progress={progress_moved} screen={screen_moved} log={log_moved}"
        ),
    }
}

pub fn score_momentum(stagnant_samples: usize, total_samples: usize) -> MetricScore {
    if total_samples == 0 {
        return MetricScore {
            kind: MetricKind::ProgressMomentum,
            value: 1.0,
            note: "no samples".into(),
        };
    }
    let stagnant_ratio = stagnant_samples as f64 / total_samples as f64;
    MetricScore {
        kind: MetricKind::ProgressMomentum,
        value: (1.0 - stagnant_ratio).clamp(0.0, 1.0),
        note: format!("{stagnant_samples}/{total_samples} stagnant samples"),
    }
}

pub fn progress_changed(before: &[(String, f64)], after: &[(String, f64)]) -> bool {
    for (name, value) in after {
        match before.iter().find(|(n, _)| n == name) {
            Some((_, old)) if (old - value).abs() > f64::EPSILON => return true,
            None => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::critique::probe::ActionFact;

    fn empty_screen() -> ScreenSnapshot {
        ScreenSnapshot {
            width: 10,
            height: 2,
            text: "          \n          ".into(),
            action_ids: vec![],
            occupancy: 0.0,
        }
    }

    #[test]
    fn goal_visibility_is_perfect_when_goal_is_unset() {
        let facts = ProbeFacts {
            phase: "x".into(),
            next_goal: None,
            actions: vec![],
            progress: vec![],
            recent_feedback: vec![],
        };
        let score = score_goal_visibility(&facts, &empty_screen());
        assert_eq!(score.value, 1.0);
    }

    #[test]
    fn goal_visibility_rejects_a_single_stray_keyword() {
        let facts = ProbeFacts {
            phase: "x".into(),
            next_goal: Some("第15波『満月の魔王』を討伐する".into()),
            actions: vec![],
            progress: vec![],
            recent_feedback: vec![],
        };
        let mut screen = empty_screen();
        screen.text = "魔王だけがどこかにある画面".into();
        let score = score_goal_visibility(&facts, &screen);
        assert_eq!(score.value, 0.0, "1語ヒットだけで満点になってはいけない");
    }

    #[test]
    fn affordance_requires_label_and_click_target() {
        let facts = ProbeFacts {
            phase: "x".into(),
            next_goal: None,
            actions: vec![ActionFact {
                id: 1,
                label: "出発".into(),
                hint: None,
                primary: true,
            }],
            progress: vec![],
            recent_feedback: vec![],
        };
        let mut screen = empty_screen();
        screen.text = " ▶ 出発する     \n                ".into();
        screen.action_ids = vec![1];
        let score = score_affordance(&facts, &screen);
        assert_eq!(score.value, 1.0);
    }

    #[test]
    fn feedback_ignores_stale_welcome_logs() {
        let before_log = vec!["ようこそ".into()];
        let after_log = vec!["ようこそ".into()];
        let score = score_feedback(&[], &[], "a", "a", &before_log, &after_log);
        assert_eq!(score.value, 0.0);
    }

    #[test]
    fn feedback_counts_new_log_lines() {
        let before_log = vec!["ようこそ".into()];
        let after_log = vec!["買った".into(), "ようこそ".into()];
        let score = score_feedback(&[], &[], "a", "a", &before_log, &after_log);
        assert_eq!(score.value, 0.6);
    }

    #[test]
    fn choice_load_uses_on_screen_targets() {
        let mut screen = empty_screen();
        screen.action_ids = (1..=20).collect();
        let score = score_choice_load(&screen);
        assert!(score.value < 1.0, "画面上の対象が多いのに choice=1.0");
        assert!(score.note.contains("20 click targets"));
    }
}
