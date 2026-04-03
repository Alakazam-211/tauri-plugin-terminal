#[cfg(test)]
mod tests {
    use crate::backend::{detect_shell, resolve_cwd, TerminalManager};
    use crate::grid_types::*;

    // ── Helper function tests ─────────────────────────────────────────

    #[test]
    fn test_detect_shell_returns_valid_path() {
        let shell = detect_shell();
        assert!(!shell.is_empty(), "Shell should not be empty");
        assert!(
            std::path::Path::new(&shell).exists(),
            "Shell path should exist: {}",
            shell
        );
    }

    #[test]
    fn test_detect_shell_is_known() {
        let shell = detect_shell();
        let known = ["/bin/zsh", "/bin/bash", "/bin/sh", "/usr/bin/zsh", "/usr/bin/bash"];
        assert!(
            known.iter().any(|k| shell.ends_with(k) || shell == *k),
            "Shell should be a known shell, got: {}",
            shell
        );
    }

    #[test]
    fn test_resolve_cwd_home_tilde() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let resolved = resolve_cwd("~");
        assert_eq!(resolved, home);
    }

    #[test]
    fn test_resolve_cwd_home_tilde_subpath() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let resolved = resolve_cwd("~/Documents");
        assert!(
            resolved.starts_with(&home),
            "Should start with home dir: {}",
            resolved
        );
        assert!(
            resolved.ends_with("Documents"),
            "Should end with Documents: {}",
            resolved
        );
    }

    #[test]
    fn test_resolve_cwd_nonexistent_falls_back_to_home() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let resolved = resolve_cwd("/this/path/definitely/does/not/exist/xyz123");
        assert_eq!(resolved, home, "Nonexistent path should fall back to home");
    }

    #[test]
    fn test_resolve_cwd_absolute_existing() {
        let resolved = resolve_cwd("/tmp");
        assert_eq!(resolved, "/tmp");
    }

    // ── Grid types tests ──────────────────────────────────────────────

    #[test]
    fn test_style_span_serialization() {
        let span = StyleSpan {
            s: 0,
            e: 10,
            fg: Some(0xFF0000),
            bg: None,
            fl: None,
        };
        let json = serde_json::to_string(&span).unwrap();
        assert!(json.contains("\"s\":0"));
        assert!(json.contains("\"e\":10"));
        assert!(json.contains("\"fg\":16711680")); // 0xFF0000
        assert!(!json.contains("\"bg\""), "bg should be skipped when None");
        assert!(!json.contains("\"fl\""), "fl should be skipped when None");
    }

    #[test]
    fn test_style_span_all_fields() {
        let span = StyleSpan {
            s: 5,
            e: 20,
            fg: Some(0x00FF00),
            bg: Some(0x000000),
            fl: Some(ATTR_BOLD | ATTR_ITALIC),
        };
        let json = serde_json::to_string(&span).unwrap();
        assert!(json.contains("\"fg\""));
        assert!(json.contains("\"bg\""));
        assert!(json.contains("\"fl\":3")); // BOLD(1) | ITALIC(2) = 3
    }

    #[test]
    fn test_compact_line_empty_spans_skipped() {
        let line = CompactLine {
            row: 0,
            text: "hello world".to_string(),
            spans: vec![],
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(!json.contains("spans"), "Empty spans should be skipped");
        assert!(json.contains("\"text\":\"hello world\""));
    }

    #[test]
    fn test_compact_line_with_spans() {
        let line = CompactLine {
            row: 3,
            text: "colored text".to_string(),
            spans: vec![StyleSpan {
                s: 0,
                e: 6,
                fg: Some(0xFF0000),
                bg: None,
                fl: None,
            }],
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(json.contains("\"spans\""));
        assert!(json.contains("\"row\":3"));
    }

    #[test]
    fn test_grid_update_serialization() {
        let update = GridUpdate {
            cols: 80,
            rows: 24,
            cursor_col: 0,
            cursor_row: 0,
            cursor_visible: true,
            cursor_shape: "block".to_string(),
            lines: vec![CompactLine {
                row: 0,
                text: "$ ".to_string(),
                spans: vec![],
            }],
            full: true,
            mode: 0,
            display_offset: 0,
            selection: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"cols\":80"));
        assert!(json.contains("\"rows\":24"));
        assert!(json.contains("\"cursor_visible\":true"));
        assert!(json.contains("\"full\":true"));
        assert!(!json.contains("\"selection\""), "None selection should be skipped");
    }

    #[test]
    fn test_grid_update_with_selection() {
        let update = GridUpdate {
            cols: 80,
            rows: 24,
            cursor_col: 5,
            cursor_row: 2,
            cursor_visible: true,
            cursor_shape: "beam".to_string(),
            lines: vec![],
            full: false,
            mode: 0,
            display_offset: 0,
            selection: Some([0, 1, 10, 1]),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"selection\":[0,1,10,1]"));
    }

    // ── Attribute flag tests ──────────────────────────────────────────

    #[test]
    fn test_attribute_flags_values() {
        assert_eq!(ATTR_BOLD, 1);
        assert_eq!(ATTR_ITALIC, 2);
        assert_eq!(ATTR_UNDERLINE, 4);
        assert_eq!(ATTR_STRIKETHROUGH, 8);
        assert_eq!(ATTR_INVERSE, 16);
        assert_eq!(ATTR_DIM, 32);
        assert_eq!(ATTR_HIDDEN, 64);
        assert_eq!(ATTR_WIDE, 128);
    }

    #[test]
    fn test_attribute_flags_combine() {
        let flags = ATTR_BOLD | ATTR_UNDERLINE | ATTR_DIM;
        assert_eq!(flags, 37); // 1 + 4 + 32
        assert!(flags & ATTR_BOLD != 0);
        assert!(flags & ATTR_UNDERLINE != 0);
        assert!(flags & ATTR_DIM != 0);
        assert!(flags & ATTR_ITALIC == 0);
    }

    // ── Terminal Manager tests ────────────────────────────────────────

    #[test]
    fn test_terminal_manager_new() {
        let manager = TerminalManager::new();
        assert_eq!(manager.get_active_count(), 0);
    }

    #[test]
    fn test_terminal_manager_exists_nonexistent() {
        let manager = TerminalManager::new();
        assert!(!manager.exists("nonexistent-id"));
    }

    #[test]
    fn test_terminal_manager_kill_nonexistent_is_ok() {
        let mut manager = TerminalManager::new();
        // Killing a nonexistent terminal should not error
        let result = manager.kill("nonexistent-id");
        assert!(result.is_ok());
    }

    #[test]
    fn test_terminal_manager_write_nonexistent_errors() {
        let manager = TerminalManager::new();
        let result = manager.write("nonexistent-id", "hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_terminal_manager_resize_nonexistent_is_ok() {
        let manager = TerminalManager::new();
        // Resizing a nonexistent terminal should not error (returns Ok(()))
        let result = manager.resize("nonexistent-id", 80, 24);
        assert!(result.is_ok());
    }

    #[test]
    fn test_terminal_manager_get_grid_nonexistent_errors() {
        let manager = TerminalManager::new();
        let result = manager.get_grid("nonexistent-id");
        assert!(result.is_err());
    }

    #[test]
    fn test_terminal_manager_count_for_nonexistent_path() {
        let manager = TerminalManager::new();
        assert_eq!(manager.get_count_for_path("/nonexistent"), 0);
    }

    #[test]
    fn test_terminal_manager_kill_all_empty() {
        let mut manager = TerminalManager::new();
        manager.kill_all(); // Should not panic
        assert_eq!(manager.get_active_count(), 0);
    }

    // ── Selection action deserialization ───────────────────────────────

    #[test]
    fn test_selection_action_deserialize() {
        let start: SelectionAction = serde_json::from_str("\"start\"").unwrap();
        assert!(matches!(start, SelectionAction::Start));

        let update: SelectionAction = serde_json::from_str("\"update\"").unwrap();
        assert!(matches!(update, SelectionAction::Update));

        let end: SelectionAction = serde_json::from_str("\"end\"").unwrap();
        assert!(matches!(end, SelectionAction::End));
    }

    #[test]
    fn test_selection_request_deserialize() {
        let json = r#"{"action":"start","col":5,"row":10}"#;
        let req: SelectionRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.action, SelectionAction::Start));
        assert_eq!(req.col, 5);
        assert_eq!(req.row, 10);
    }
}
