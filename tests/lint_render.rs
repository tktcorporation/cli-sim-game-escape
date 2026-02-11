//! Lint: detect bracket-key text (`[X]`) rendered without click registration.
//!
//! The TEA Builder pattern requires that any `[X]`-style button text displayed
//! in a `render.rs` is registered as a click target via `push_clickable()`.
//!
//! Using `cl.push(Line::from(... "[I]..." ...))` renders the text but makes it
//! un-clickable — a common source of tap/click bugs on mobile.
//!
//! This test scans all `render.rs` files under `src/games/` and flags
//! `push(` calls whose string arguments contain bracket-key patterns.

use std::fs;
use std::path::Path;

/// Check if a string literal contains a bracket-key pattern like `[I]`, `[S]`, `[1]`.
fn contains_bracket_key(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    for i in 0..bytes.len() - 2 {
        if bytes[i] == b'[' && bytes[i + 2] == b']' {
            let ch = bytes[i + 1];
            if ch.is_ascii_alphanumeric() || b"-=!~{}|\\".contains(&ch) {
                return true;
            }
        }
    }
    false
}

/// Scan source for `push(` calls (non-clickable) containing bracket-key patterns.
fn find_bracket_key_in_push(source: &str) -> Vec<(usize, String)> {
    let mut violations = Vec::new();

    for (line_num_0, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("///") {
            continue;
        }

        // Must contain a bracket-key pattern
        if !contains_bracket_key(line) {
            continue;
        }

        // Check: is this inside a non-clickable `push(` call?
        let has_push = line.contains(".push(");
        let has_clickable = line.contains("push_clickable(")
            || line.contains("push_choice(")
            || line.contains("push_choice_dim(")
            || line.contains("push_overlay_hints(");

        if has_push && !has_clickable {
            violations.push((line_num_0 + 1, trimmed.to_string()));
        }
    }

    violations
}

#[test]
fn no_bracket_keys_in_non_clickable_push() {
    let games_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/games");
    let mut all_violations = Vec::new();

    visit_render_files(&games_dir, &mut all_violations);

    if !all_violations.is_empty() {
        let mut msg = String::from(
            "Found bracket-key text [X] in non-clickable cl.push() calls.\n\
             These should use push_clickable() or a helper like push_overlay_hints().\n\
             See ARCHITECTURE.md Rule 1.\n\n",
        );
        for (file, line_num, line) in &all_violations {
            msg.push_str(&format!("  {}:{}: {}\n", file, line_num, line));
        }
        panic!("{}", msg);
    }
}

fn visit_render_files(dir: &Path, violations: &mut Vec<(String, usize, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_render_files(&path, violations);
        } else if path.file_name().map(|n| n == "render.rs").unwrap_or(false) {
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            let file_violations = find_bracket_key_in_push(&source);
            let display_path = path.display().to_string();
            for (line_num, line) in file_violations {
                violations.push((display_path.clone(), line_num, line));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bracket_key_in_push() {
        let source = r#"cl.push(Line::from(" [I]持ち物  [S]ステータス"));"#;
        let violations = find_bracket_key_in_push(source);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn allows_push_clickable() {
        let source = r#"cl.push_clickable(Line::from(" [I] 持ち物"), OPEN_INVENTORY);"#;
        let violations = find_bracket_key_in_push(source);
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_push_choice() {
        let source = r#"push_choice(&mut cl, 0, "中に入る");"#;
        let violations = find_bracket_key_in_push(source);
        assert!(violations.is_empty());
    }

    #[test]
    fn ignores_comments() {
        let source = r#"// cl.push(Line::from(" [I]持ち物"));"#;
        let violations = find_bracket_key_in_push(source);
        assert!(violations.is_empty());
    }

    #[test]
    fn bracket_key_detection() {
        assert!(contains_bracket_key("[I]"));
        assert!(contains_bracket_key("[S]"));
        assert!(contains_bracket_key("[1]"));
        assert!(contains_bracket_key("[0]"));
        assert!(contains_bracket_key("[-]"));
        assert!(!contains_bracket_key("[]"));
        assert!(!contains_bracket_key("[II]"));
        assert!(!contains_bracket_key("abc"));
    }
}
