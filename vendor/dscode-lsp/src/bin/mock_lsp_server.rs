use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() -> io::Result<()> {
    let crash_on_hover = std::env::args().any(|arg| arg == "--crash-on-hover");
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let output = Arc::new(Mutex::new(io::stdout()));
    while let Some(message) = read_message(&mut input)? {
        let method = message.get("method").and_then(serde_json::Value::as_str);
        match (method, message.get("id")) {
            (Some("initialize"), Some(id)) => {
                let snippet_support = message["params"]["capabilities"]["textDocument"]
                    ["completion"]["completionItem"]["snippetSupport"]
                    == true;
                let apply_edit = message["params"]["capabilities"]["workspace"]["applyEdit"]
                    == true;
                let formatting = message["params"]["capabilities"]["textDocument"]
                    ["formatting"]["dynamicRegistration"]
                    == false;
                let signature_help = message["params"]["capabilities"]["textDocument"]
                    ["signatureHelp"]
                    .is_object();
                let references = message["params"]["capabilities"]["textDocument"]["references"]
                    .is_object();
                let navigation_capabilities = [
                    "declaration",
                    "typeDefinition",
                    "implementation",
                ]
                .iter()
                .all(|name| {
                    message["params"]["capabilities"]["textDocument"][name].is_object()
                });
                if !snippet_support
                    || !apply_edit
                    || !formatting
                    || !signature_help
                    || !references
                    || !navigation_capabilities
                {
                    write_message(
                        &output,
                        &serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32602, "message": "Required client capabilities missing" }
                        }),
                    )?;
                    continue;
                }
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "capabilities": {
                                "textDocumentSync": { "change": 2, "save": true },
                                "completionProvider": { "triggerCharacters": ["."] },
                                "documentFormattingProvider": true
                            },
                            "serverInfo": { "name": "mock-lsp", "version": "1" }
                        }
                    }),
                )?;
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1001,
                        "method": "workspace/configuration",
                        "params": { "items": [{}] }
                    }),
                )?;
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1002,
                        "method": "workspace/applyEdit",
                        "params": { "edit": { "changes": {} } }
                    }),
                )?;
            }
            (Some("textDocument/hover"), Some(id)) => {
                if crash_on_hover {
                    return Ok(());
                }
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "$/mock/notification",
                        "params": { "method": "textDocument/hover" }
                    }),
                )?;
                let id = id.clone();
                let output = output.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(300));
                    let _ = write_message(
                        &output,
                        &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                    );
                });
            }
            (Some("textDocument/completion"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "isIncomplete": false,
                            "items": [{
                                "label": "start",
                                "sortText": "1",
                                "insertText": "start",
                                "insertTextFormat": 2
                            }]
                        }
                    }),
                )?;
            }
            (Some("textDocument/formatting"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [{
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": 0, "character": 12 }
                            },
                            "newText": "fn main() {}"
                        }]
                    }),
                )?;
            }
            (Some("textDocument/signatureHelp"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "signatures": [{
                                "label": "start(name: &str)",
                                "parameters": [{ "label": "name: &str" }]
                            }],
                            "activeSignature": 0,
                            "activeParameter": 0
                        }
                    }),
                )?;
            }
            (Some("textDocument/definition"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "uri": "file:///workspace/main.rs",
                            "range": {
                                "start": { "line": 0, "character": 3 },
                                "end": { "line": 0, "character": 7 }
                            }
                        }
                    }),
                )?;
            }
            (Some("textDocument/declaration"), Some(id))
            | (Some("textDocument/typeDefinition"), Some(id))
            | (Some("textDocument/implementation"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "uri": "file:///workspace/main.rs",
                            "range": {
                                "start": { "line": 0, "character": 3 },
                                "end": { "line": 0, "character": 7 }
                            }
                        }
                    }),
                )?;
            }
            (Some("textDocument/references"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [{
                            "uri": "file:///workspace/main.rs",
                            "range": {
                                "start": { "line": 0, "character": 3 },
                                "end": { "line": 0, "character": 7 }
                            }
                        }]
                    }),
                )?;
            }
            (Some("workspace/executeCommand"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )?;
            }
            (Some("shutdown"), Some(id)) => {
                write_message(
                    &output,
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )?;
            }
            (Some("exit"), _) => break,
            (Some(method), None) => {
                write_message(
                    &output,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "$/mock/notification",
                        "params": {
                            "method": method,
                            "params": message.get("params").cloned().unwrap_or_default()
                        }
                    }),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<serde_json::Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(content_length) = content_length else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"));
    };
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(io::Error::other)
}

fn write_message(output: &Arc<Mutex<io::Stdout>>, message: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    let mut output = output.lock().expect("mock LSP stdout lock poisoned");
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(&body)?;
    output.flush()
}
