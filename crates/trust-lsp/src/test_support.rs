//! Test helpers shared across LSP unit tests.

use std::sync::{Arc, Mutex};
use tower_lsp::{Client, LanguageServer, LspService};

pub(crate) fn test_client() -> Client {
    struct DummyServer;

    #[tower_lsp::async_trait]
    impl LanguageServer for DummyServer {
        async fn initialize(
            &self,
            _: tower_lsp::lsp_types::InitializeParams,
        ) -> tower_lsp::jsonrpc::Result<tower_lsp::lsp_types::InitializeResult> {
            Ok(tower_lsp::lsp_types::InitializeResult::default())
        }

        async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
            Ok(())
        }
    }

    let captured = Arc::new(Mutex::new(None));
    let captured_clone = captured.clone();
    let (_service, socket) = LspService::new(move |client| {
        *captured_clone.lock().expect("lock test client") = Some(client.clone());
        DummyServer
    });
    drop(socket);

    let client = captured
        .lock()
        .expect("lock test client")
        .take()
        .expect("test client");
    client
}

use tower_lsp::lsp_types::DocumentSymbol;

#[cfg(test)]
#[derive(Debug)]
pub struct ExpectedNode<'a> {
    pub name: &'a str,
    pub children: Option<Vec<ExpectedNode<'a>>>,
}

#[cfg(test)]
pub fn assert_document_symbol_tree(symbols: &[DocumentSymbol], expected: &[ExpectedNode]) {
    assert_eq!(
        symbols.len(),
        expected.len(),
        "root count mismatch: got {:?}",
        symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
    );
    for (i, (sym, exp)) in symbols.iter().zip(expected.iter()).enumerate() {
        assert_eq!(sym.name, exp.name, "name mismatch at index {}", i);
        if let Some(exp_kids) = &exp.children {
            let children = sym.children.as_ref().expect("expected children");
            assert_document_symbol_tree(children, exp_kids);
        }
    }
}

#[cfg(test)]
pub fn document_symbol_nested(source: &str) -> Vec<DocumentSymbol> {
    let state = crate::state::ServerState::new();
    let uri = tower_lsp::lsp_types::Url::parse("file:///test.st").unwrap();
    state.open_document(uri.clone(), 1, source.to_string());
    let params = tower_lsp::lsp_types::DocumentSymbolParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let response = crate::handlers::document_symbol(&state, params).expect("document symbols");
    match response {
        tower_lsp::lsp_types::DocumentSymbolResponse::Nested(symbols) => symbols,
        _ => panic!("expected nested document symbols"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_symbol_nested_tree() {
        let source = r#"
PROGRAM Main
ACTION Reset
END_ACTION
END_PROGRAM
"#;
        let symbols = document_symbol_nested(source);
        assert_document_symbol_tree(
            &symbols,
            &[ExpectedNode {
                name: "Main",
                children: Some(vec![ExpectedNode {
                    name: "Reset",
                    children: None,
                }]),
            }],
        );
    }

    #[test]
    fn document_symbol_struct_fields_nested() {
        let source = r#"
TYPE Point :
STRUCT
    x : INT;
    y : INT;
END_STRUCT
END_TYPE
"#;
        let symbols = document_symbol_nested(source);
        assert_document_symbol_tree(
            &symbols,
            &[ExpectedNode {
                name: "Point",
                children: Some(vec![
                    ExpectedNode {
                        name: "x",
                        children: None,
                    },
                    ExpectedNode {
                        name: "y",
                        children: None,
                    },
                ]),
            }],
        );
    }

    fn find_symbol<'a>(symbols: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
        for sym in symbols {
            if sym.name == name {
                return Some(sym);
            }
            if let Some(children) = &sym.children {
                if let Some(found) = find_symbol(children, name) {
                    return Some(found);
                }
            }
        }
        None
    }

    #[test]
    fn variable_detail_with_persistence() {
        let source = r#"PROGRAM Main VAR_INPUT RETAIN x : INT; END_VAR END_PROGRAM"#;
        let symbols = document_symbol_nested(source);
        let x = find_symbol(&symbols, "x").expect("x");
        assert_eq!(x.detail.as_deref(), Some("VAR_INPUT RETAIN : INT"));
    }

    #[test]
    fn variable_detail_with_edge() {
        let source = r#"PROGRAM Main VAR_GLOBAL x : BOOL R_EDGE; END_VAR END_PROGRAM"#;
        let symbols = document_symbol_nested(source);
        let x = find_symbol(&symbols, "x").expect("x");
        assert_eq!(x.detail.as_deref(), Some("VAR_GLOBAL R_EDGE : BOOL"));
    }
}
