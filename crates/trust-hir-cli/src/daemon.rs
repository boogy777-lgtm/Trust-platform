//! JSON-RPC daemon loop over stdin/stdout.

use std::io::Write;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::handlers::{DaemonState, RpcError};
use crate::protocol::{Response, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};

/// Run the JSON-RPC daemon reading from stdin and writing to stdout.
pub async fn run_daemon(state: &DaemonState) {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let response = process_line(state, &line).await;
        match serde_json::to_string(&response) {
            Ok(json) => {
                println!("{json}");
                let _ = std::io::stdout().flush();
            }
            Err(e) => {
                eprintln!("Failed to serialize response: {e}");
            }
        }
    }
}

async fn process_line(state: &DaemonState, line: &str) -> Response {
    let request = match serde_json::from_str::<crate::protocol::Request>(line) {
        Ok(req) => req,
        Err(e) => {
            return Response::error(None, PARSE_ERROR, format!("Parse error: {e}"));
        }
    };

    if request.jsonrpc != "2.0" {
        return Response::error(request.id, INVALID_REQUEST, "Invalid jsonrpc version");
    }

    if request.method.is_empty() {
        return Response::error(request.id, INVALID_REQUEST, "Missing method");
    }

    let result = match request.method.as_str() {
        "hir/initialize" => crate::handlers::handle_initialize(state, request.params),
        "hir/getSymbols" => crate::handlers::handle_get_symbols(state, request.params),
        "hir/getTypes" => crate::handlers::handle_get_types(state, request.params),
        "hir/getReferences" => crate::handlers::handle_get_references(state, request.params),
        "hir/getDiagnostics" => crate::handlers::handle_get_diagnostics(state, request.params),
        "hir/index" => crate::handlers::handle_index(state, request.params),
        "hir/snapshot" => crate::handlers::handle_snapshot(state, request.params),
        "hir/updateFile" => crate::handlers::handle_update_file(state, request.params),
        "hir/shutdown" => crate::handlers::handle_shutdown(state, request.params),
        _ => Err(RpcError {
            code: METHOD_NOT_FOUND,
            message: format!("Method not found: {}", request.method),
        }),
    };

    match result {
        Ok(value) => Response::success(request.id, value),
        Err(err) => Response::error(request.id, err.code, err.message),
    }
}
