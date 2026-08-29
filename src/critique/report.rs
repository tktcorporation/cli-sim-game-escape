//! 比較レポートの整形とファイル出力。

use std::fs;
use std::path::Path;

use crate::critique::metrics::MetricKind;
use crate::critique::session::SessionReport;

const KINDS: [MetricKind; 6] = [
    MetricKind::AffordanceCoverage,
    MetricKind::GoalVisibility,
    MetricKind::FeedbackResponsiveness,
    MetricKind::ReadableDensity,
    MetricKind::ChoiceLoad,
    MetricKind::ProgressMomentum,
];

pub fn format_comparative_report(rows: &[SessionReport]) -> String {
    let mut out = String::new();
    out.push_str("=== game critique (text/state, no screenshots) ===\n");
    out.push_str(&format!(
        "{:<12} {:<10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}\n",
        "game", "phase", "afford", "goal", "feedback", "density", "choice", "momentum"
    ));
    for row in rows {
        let mut vals = [f64::NAN; 6];
        for score in row.opening.iter().chain(row.session.iter()) {
            if let Some(idx) = KINDS.iter().position(|k| *k == score.kind) {
                vals[idx] = score.value;
            }
        }
        out.push_str(&format!(
            "{:<12} {:<10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}\n",
            truncate(&row.game, 12),
            truncate(&row.phase_at_open, 10),
            fmt_score(vals[0]),
            fmt_score(vals[1]),
            fmt_score(vals[2]),
            fmt_score(vals[3]),
            fmt_score(vals[4]),
            fmt_score(vals[5]),
        ));
    }

    out.push('\n');
    out.push_str("--- how to read low scores ---\n");
    for kind in KINDS {
        out.push_str(&format!(
            "  {:>8}: {}\n",
            kind.label(),
            kind.low_score_means()
        ));
    }

    out.push('\n');
    out.push_str("--- per-game notes (opening) ---\n");
    for row in rows {
        out.push_str(&format!("[{}]\n", row.game));
        for score in &row.opening {
            if score.value < 0.75 {
                out.push_str(&format!(
                    "  ! {}={:.2} — {}\n",
                    score.kind.label(),
                    score.value,
                    score.note
                ));
            }
        }
        for score in &row.session {
            if score.value < 0.75 {
                out.push_str(&format!(
                    "  ! {}={:.2} — {}\n",
                    score.kind.label(),
                    score.value,
                    score.note
                ));
            }
        }
    }
    out
}

pub fn write_report_file(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, text).unwrap_or_else(|e| panic!("critique report write failed: {e}"));
}

fn fmt_score(v: f64) -> String {
    if v.is_nan() {
        "   n/a".into()
    } else {
        format!("{v:8.2}")
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}
