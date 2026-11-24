use super::*;
use lsp_types::{MarkupContent, MarkupKind};

mod command_tests {
    use super::*;
    use lsp_types::{Diagnostic, Position, Range};

    #[test]
    fn server_command_splits_arguments_without_a_shell() {
        assert_eq!(
            split_server_command(r#"clangd --compile-commands-dir "C:\work space""#).unwrap(),
            ["clangd", "--compile-commands-dir", r"C:\work space"]
        );
    }

    #[test]
    fn server_command_rejects_unclosed_quotes() {
        assert!(split_server_command(r#"clangd --config "oops"#).is_err());
    }

    #[test]
    fn publish_diagnostics_notifications_are_forwarded_to_the_document() {
        let path = std::env::temp_dir().join("winct-lsp-diagnostics.rs");
        let uri = Url::from_file_path(&path).unwrap();
        let notification = LspNotification {
            method: "textDocument/publishDiagnostics".to_owned(),
            params: serde_json::to_value(PublishDiagnosticsParams {
                uri,
                version: None,
                diagnostics: vec![Diagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("test".to_owned()),
                    message: "expected a value".to_owned(),
                    related_information: None,
                    tags: None,
                    data: None,
                }],
            })
            .unwrap(),
        };

        let Some(UserEvent::LspDiagnostics(received_path, diagnostics)) =
            diagnostic_event(notification)
        else {
            panic!("publishDiagnostics must produce a document diagnostics event");
        };
        assert_eq!(received_path, path);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "expected a value");
    }

    #[test]
    fn automatic_servers_are_known_for_the_supported_defaults() {
        assert_eq!(
            auto_server_command("rust"),
            Some(vec!["rust-analyzer".to_owned()])
        );
        assert_eq!(
            auto_server_command("python"),
            Some(vec!["pyright-langserver".to_owned(), "--stdio".to_owned()])
        );
        assert!(auto_server_command("unknown").is_none());
    }

    #[test]
    fn incremental_change_uses_utf16_positions_for_unicode_edits() {
        let old = Rope::from_str("same\nA🦀Z\nunchanged");
        let new = Rope::from_str("same\nA🦀newZ\nunchanged");

        let change = incremental_change(&old, &new, 1).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 3),
                lsp_types::Position::new(1, 3),
            ))
        );
        assert_eq!(change.range_length, Some(0));
        assert_eq!(change.text, "new");
    }

    #[test]
    fn incremental_change_handles_line_insertions_and_deletions() {
        let old = Rope::from_str("first\nlast\n");
        let new = Rope::from_str("first\ninserted\nlast\n");

        let change = incremental_change(&old, &new, 1).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 0),
                lsp_types::Position::new(1, 0),
            ))
        );
        assert_eq!(change.text, "inserted\n");
    }

    #[test]
    fn incremental_change_returns_none_when_the_text_is_unchanged() {
        let text = Rope::from_str("same text\n");
        assert!(incremental_change(&text, &text, 0).is_none());
    }

    #[test]
    fn incremental_change_handles_replacement_without_a_common_prefix_or_suffix() {
        let old = Rope::from_str("abc");
        let new = Rope::from_str("xyz");

        let change = incremental_change(&old, &new, 0).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(0, 0),
                lsp_types::Position::new(0, 3),
            ))
        );
        assert_eq!(change.range_length, Some(3));
        assert_eq!(change.text, "xyz");
    }

    #[test]
    fn incremental_change_handles_insertions_at_the_end() {
        let old = Rope::from_str("existing");
        let new = Rope::from_str("existing text");

        let change = incremental_change(&old, &new, 0).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(0, 8),
                lsp_types::Position::new(0, 8),
            ))
        );
        assert_eq!(change.range_length, Some(0));
        assert_eq!(change.text, " text");
    }

    #[test]
    fn server_configuration_requests_receive_one_result_per_item() {
        let request = LspServerRequest {
            id: serde_json::json!(12),
            method: "workspace/configuration".to_owned(),
            params: serde_json::json!({
                "items": [
                    {"section": "rust-analyzer"},
                    {"section": "python"}
                ]
            }),
        };

        assert_eq!(
            server_request_response(&request, Path::new(".")).unwrap(),
            serde_json::json!([null, null])
        );
    }

    #[test]
    fn unsupported_server_requests_receive_json_rpc_method_not_found() {
        let request = LspServerRequest {
            id: serde_json::json!("req-1"),
            method: "workspace/unknown".to_owned(),
            params: serde_json::Value::Null,
        };

        assert_eq!(
            server_request_response(&request, Path::new(".")).unwrap_err(),
            (
                -32601,
                "Unsupported LSP server request: workspace/unknown".to_owned()
            )
        );
    }
}

#[test]
fn hover_content_fits_the_single_line_hint() {
    let content = Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "```rust\nfn answer() -> u32\n```".to_owned(),
        }),
        range: None,
    };
    assert_eq!(hover_text(content), "```rust fn answer() -> u32 ```");
}

#[test]
fn long_hover_content_is_truncated_at_a_character_boundary() {
    let content = Hover {
        contents: HoverContents::Scalar(MarkedString::String("🦀".repeat(181))),
        range: None,
    };
    let text = hover_text(content);
    assert_eq!(text.chars().count(), 183);
    assert!(text.ends_with("..."));
}
