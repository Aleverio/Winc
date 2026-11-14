use std::sync::Arc;
use std::time::Duration;

use dscode_lsp::{LspClient, LspClientState};
use lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};
use serde_json::{Value, json};
use tokio::time::timeout;

#[tokio::test]
async fn mock_server_exercises_protocol_and_concurrent_sync() {
    let client = Arc::new(LspClient::new(
        "rust".to_owned(),
        env!("CARGO_BIN_EXE_mock_lsp_server").to_owned(),
        Vec::new(),
    ));
    let mut notifications = client.subscribe_notifications();
    let mut requests = client.subscribe_server_requests();
    client.start().await.unwrap();
    let initialized = client
        .initialize(Url::parse("file:///workspace").unwrap())
        .await
        .unwrap();
    assert_eq!(
        initialized
            .capabilities
            .completion_provider
            .and_then(|provider| provider.trigger_characters),
        Some(vec![".".to_owned()])
    );
    client.initialized().await.unwrap();

    let configuration = timeout(Duration::from_secs(2), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(configuration.method, "workspace/configuration");
    client.respond(configuration.id, json!([null])).await.unwrap();
    let apply_edit = timeout(Duration::from_secs(2), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(apply_edit.method, "workspace/applyEdit");
    client.respond(apply_edit.id, json!({ "applied": true })).await.unwrap();

    let uri = Url::parse("file:///workspace/main.rs").unwrap();
    client
        .did_open(uri.clone(), "rust".to_owned(), 1, "fn main() {}".to_owned())
        .await
        .unwrap();
    assert!(client.document_is_open(&uri).await);
    assert_eq!(
        next_notification(&mut notifications).await["method"],
        "initialized"
    );
    assert_eq!(
        next_notification(&mut notifications).await["method"],
        "textDocument/didOpen"
    );

    let hover_client = client.clone();
    let hover_uri = uri.clone();
    let hover = tokio::spawn(async move { hover_client.hover(hover_uri, 0, 3).await });
    assert_eq!(
        next_notification(&mut notifications).await["method"],
        "textDocument/hover"
    );

    client
        .did_change_with_changes(
            uri.clone(),
            2,
            vec![TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position::new(0, 3),
                    end: Position::new(0, 7),
                }),
                range_length: Some(4),
                text: "start".to_owned(),
            }],
        )
        .await
        .unwrap();
    let change = next_notification(&mut notifications).await;
    assert_eq!(change["method"], "textDocument/didChange");
    assert_eq!(change["params"]["textDocument"]["version"], 2);
    assert_eq!(
        change["params"]["contentChanges"][0]["range"]["start"]["character"],
        3
    );
    assert_eq!(change["params"]["contentChanges"][0]["text"], "start");
    assert!(!hover.is_finished(), "the mock hover response should be delayed");
    assert_eq!(hover.await.unwrap().unwrap(), None);
    let completions = client
        .completion(
            uri.clone(),
            0,
            8,
            lsp_types::CompletionTriggerKind::INVOKED,
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let items = match completions {
        lsp_types::CompletionResponse::Array(items) => items,
        lsp_types::CompletionResponse::List(list) => list.items,
    };
    assert_eq!(items[0].label, "start");
    assert_eq!(
        items[0].insert_text_format,
        Some(lsp_types::InsertTextFormat::SNIPPET)
    );
    let formatting = client
        .formatting(uri.clone(), lsp_types::FormattingOptions::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(formatting.len(), 1);
    assert_eq!(formatting[0].new_text, "fn main() {}");
    let signature_help = client
        .signature_help(uri.clone(), 0, 8)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signature_help.signatures[0].label, "start(name: &str)");
    let definitions = client
        .definition_locations(uri.clone(), 0, 3)
        .await
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].uri, uri);
    let references = client
        .reference_locations(uri.clone(), 0, 3)
        .await
        .unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(
        client
            .declaration_locations(uri.clone(), 0, 3)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        client
            .type_definition_locations(uri.clone(), 0, 3)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        client
            .implementation_locations(uri.clone(), 0, 3)
            .await
            .unwrap()
            .len(),
        1
    );

    client.did_save(uri.clone()).await.unwrap();
    assert_eq!(
        next_notification(&mut notifications).await["method"],
        "textDocument/didSave"
    );
    client
        .execute_command("mock.run".to_owned(), vec![json!("argument")])
        .await
        .unwrap();
    client.did_close(uri.clone()).await.unwrap();
    assert!(!client.document_is_open(&uri).await);
    assert_eq!(
        next_notification(&mut notifications).await["method"],
        "textDocument/didClose"
    );

    client.stop().await.unwrap();
}

#[tokio::test]
async fn clients_are_isolated_by_workspace_and_can_restart() {
    let command = env!("CARGO_BIN_EXE_mock_lsp_server").to_owned();
    let first = Arc::new(LspClient::new(
        "rust".to_owned(),
        command.clone(),
        Vec::new(),
    ));
    let second = LspClient::new("rust".to_owned(), command, Vec::new());

    first.start().await.unwrap();
    first
        .initialize(Url::parse("file:///workspace-a").unwrap())
        .await
        .unwrap();
    first.initialized().await.unwrap();
    second.start().await.unwrap();
    second
        .initialize(Url::parse("file:///workspace-b").unwrap())
        .await
        .unwrap();
    second.initialized().await.unwrap();

    first.stop().await.unwrap();
    second.stop().await.unwrap();
    assert_eq!(first.get_state().await, LspClientState::Stopped);
    assert_eq!(second.get_state().await, LspClientState::Stopped);

    first.start().await.unwrap();
    first
        .initialize(Url::parse("file:///workspace-a").unwrap())
        .await
        .unwrap();
    first.initialized().await.unwrap();
    first.stop().await.unwrap();
}

#[tokio::test]
async fn unexpected_server_exit_rejects_pending_requests_and_marks_crashed() {
    let client = LspClient::new(
        "rust".to_owned(),
        env!("CARGO_BIN_EXE_mock_lsp_server").to_owned(),
        vec!["--crash-on-hover".to_owned()],
    );
    client.start().await.unwrap();
    client
        .initialize(Url::parse("file:///crashing-workspace").unwrap())
        .await
        .unwrap();
    let request_error = client
        .hover(
            Url::parse("file:///crashing-workspace/main.rs").unwrap(),
            0,
            0,
        )
        .await
        .unwrap_err();
    assert!(request_error.contains("connection closed"));

    timeout(Duration::from_secs(2), async {
        while client.get_state().await != LspClientState::Crashed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("client did not report the unexpected server exit");
    client.stop().await.unwrap();
    assert_eq!(client.get_state().await, LspClientState::Stopped);
}

async fn next_notification(
    notifications: &mut tokio::sync::broadcast::Receiver<dscode_lsp::LspNotification>,
) -> Value {
    timeout(Duration::from_secs(2), async {
        loop {
            let notification = notifications.recv().await.unwrap();
            if notification.method == "$/mock/notification" {
                return notification.params;
            }
        }
    })
    .await
    .expect("mock server did not acknowledge the notification")
}
