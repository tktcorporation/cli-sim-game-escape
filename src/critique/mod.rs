//! ゲームの「わかりにくさ / 手応え」を、画面の文字と抽象状態から測る。
//!
//! スクリーンショットは使わない。端末セルを `tui_inspect` で文字列化し、
//! 各ゲームが返す [`ProbeFacts`]（今の局面・次の目標・可能な操作・進捗）と
//! 突き合わせてスコアにする。CI のログと artifact で比較できる。
//!
//! 使い方:
//! - 通常: `cargo test --lib critique -- --nocapture`
//! - 画面全文も見る: `cargo test --lib critique::tests::dump_ -- --ignored --nocapture`
//!
//! スコアの読み方は `.claude/rules/project/game-critique.md`。

mod frame;
mod metrics;
mod probe;
mod report;
mod session;
mod subjects;

pub use frame::{capture_frame, ScreenSnapshot};
pub use metrics::{evaluate_opening, MetricKind, MetricScore};
pub use probe::{ActionFact, ProbeFacts, Subject};
pub use report::{format_comparative_report, write_report_file};
pub use session::{run_session, SessionConfig, SessionReport};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn all_subjects() -> Vec<Box<dyn Subject>> {
        subjects::all()
    }

    #[test]
    fn critique_scores_stay_in_unit_interval_and_write_report() {
        let mut rows = Vec::new();
        for mut subject in all_subjects() {
            let report = run_session(subject.as_mut(), &SessionConfig::ci_default());
            for score in report.opening.iter().chain(report.session.iter()) {
                assert!(
                    (0.0..=1.0).contains(&score.value),
                    "{} {:?} = {}",
                    report.game,
                    score.kind,
                    score.value
                );
            }
            rows.push(report);
        }

        let text = format_comparative_report(&rows);
        eprintln!("{text}");

        let out = report_path();
        write_report_file(&out, &text);
        assert!(
            out.is_file(),
            "critique report が書けていない: {}",
            out.display()
        );
    }

    #[test]
    fn everlight_camp_puts_the_stated_goal_on_screen() {
        // 拠点に目標行を常設している設計なので、goal_visibility が開口で
        // 落ちるのは退行。
        let mut subject = subjects::everlight();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let goal = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::GoalVisibility)
            .expect("GoalVisibility が無い");
        assert!(
            goal.value >= 0.99,
            "常夜灯の拠点で目標が見えていない: {} ({})",
            goal.value,
            goal.note
        );
    }

    #[test]
    fn cookie_opening_exposes_next_purchase_goal() {
        let mut subject = subjects::cookie();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let goal = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::GoalVisibility)
            .expect("GoalVisibility が無い");
        assert!(
            goal.value >= 0.99,
            "Cookie の次購入目標が見えていない: {} ({})",
            goal.value,
            goal.note
        );
    }

    #[test]
    fn cookie_opening_exposes_a_primary_click_target() {
        let mut subject = subjects::cookie();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let afford = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::AffordanceCoverage)
            .expect("AffordanceCoverage が無い");
        assert!(
            afford.value >= 0.5,
            "Cookie 開口で主操作の手がかりが薄い: {} ({})",
            afford.value,
            afford.note
        );
    }

    #[test]
    fn pachinko_hall_tells_the_player_to_pick_a_machine() {
        let mut subject = subjects::pachinko();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let goal = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::GoalVisibility)
            .expect("GoalVisibility が無い");
        assert!(
            goal.value >= 0.99,
            "玉響のホールで台選びの目標が見えていない: {} ({})",
            goal.value,
            goal.note
        );
    }

    #[test]
    fn loopmarch_camp_shows_the_expedition_cta() {
        let mut subject = subjects::loopmarch();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let goal = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::GoalVisibility)
            .expect("GoalVisibility が無い");
        assert!(
            goal.value >= 0.99,
            "周回討伐の拠点で出発 CTA が見えていない: {} ({})",
            goal.value,
            goal.note
        );
        let afford = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::AffordanceCoverage)
            .expect("AffordanceCoverage が無い");
        assert!(
            afford.value >= 0.99,
            "周回討伐の出発ボタンが辿れない: {} ({})",
            afford.value,
            afford.note
        );
    }

    #[test]
    fn cookie_opening_density_is_not_a_sparse_cavern() {
        // 投資ヒントとレイアウト契約で開口の空洞を埋めた設計なので、
        // density が帯の下限 (0.25 occupancy → 1.0) 付近まで落ちるのは退行。
        let mut subject = subjects::cookie();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let density = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::ReadableDensity)
            .expect("ReadableDensity が無い");
        assert!(
            density.value >= 0.85,
            "Cookie 開口がスカスカ: {} ({})",
            density.value,
            density.note
        );
    }

    #[test]
    fn factory_opening_shows_miner_build_goal() {
        let mut subject = subjects::factory();
        let report = run_session(subject.as_mut(), &SessionConfig::opening_only());
        let goal = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::GoalVisibility)
            .expect("GoalVisibility が無い");
        assert!(
            goal.value >= 0.99,
            "Factory 開口で次の設置目標が見えていない: {} ({})",
            goal.value,
            goal.note
        );
        let afford = report
            .opening
            .iter()
            .find(|s| s.kind == MetricKind::AffordanceCoverage)
            .expect("AffordanceCoverage が無い");
        assert!(
            afford.value >= 0.99,
            "Factory 開口で Miner 選択が辿れない: {} ({})",
            afford.value,
            afford.note
        );
    }

    #[test]
    fn everlight_and_loopmarch_session_feedback_stays_responsive() {
        // レーン移動・手札選択にログ/進捗を足した設計の退行検知。
        for mut subject in [subjects::everlight(), subjects::loopmarch()] {
            let name = subject.name();
            let report = run_session(subject.as_mut(), &SessionConfig::ci_default());
            let feedback = report
                .session
                .iter()
                .find(|s| s.kind == MetricKind::FeedbackResponsiveness)
                .expect("FeedbackResponsiveness が無い");
            assert!(
                feedback.value >= 0.9,
                "{name} の操作フィードバックが薄い: {} ({})",
                feedback.value,
                feedback.note
            );
        }
    }

    /// 画面全文 + スコア内訳。人間が「なんで低い？」を追う出口。
    ///
    /// `cargo test --lib critique::tests::dump_critique_frames -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_critique_frames() {
        for mut subject in all_subjects() {
            let name = subject.name();
            let snap = subject.capture(100, 40);
            let facts = subject.probe();
            eprintln!("=== {name} probe ===\n{facts:#?}");
            eprintln!("=== {name} screen 100x40 ===\n{}", snap.text);
            let report = run_session(subject.as_mut(), &SessionConfig::ci_default());
            eprintln!(
                "=== {name} scores ===\n{}",
                format_comparative_report(std::slice::from_ref(&report))
            );
        }
    }

    fn report_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/critique-report.txt")
    }
}
