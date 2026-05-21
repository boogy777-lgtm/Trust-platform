//! Property-based tests, invariants, edge cases and performance tests for DocumentSymbol.

use crate::test_support::{assert_document_symbol_tree, document_symbol_nested, ExpectedNode};
use tower_lsp::lsp_types::DocumentSymbol;

// ---------------------------------------------------------------------------
// Invariant helpers
// ---------------------------------------------------------------------------

fn assert_selection_range_invariant(symbols: &[DocumentSymbol]) {
    for sym in symbols {
        assert!(
            sym.selection_range.start.line >= sym.range.start.line,
            "selection_range.start.line < range.start.line for {}",
            sym.name
        );
        assert!(
            sym.selection_range.end.line <= sym.range.end.line,
            "selection_range.end.line > range.end.line for {}",
            sym.name
        );
        if let Some(children) = &sym.children {
            assert_selection_range_invariant(children);
        }
    }
}

fn assert_children_sorted(symbols: &[DocumentSymbol]) {
    for sym in symbols {
        if let Some(children) = &sym.children {
            for i in 1..children.len() {
                assert!(
                    children[i].range.start.line >= children[i - 1].range.start.line,
                    "children not sorted by position for {}",
                    sym.name
                );
            }
            assert_children_sorted(children);
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based / invariant tests
// ---------------------------------------------------------------------------

#[test]
fn document_symbol_selection_range_inside_range() {
    let cases = vec![
        ("PROGRAM Main END_PROGRAM", vec![("Main", vec![])]),
        (
            "TYPE Color : (Red, Green, Blue); END_TYPE",
            vec![("Color", vec!["Red", "Green", "Blue"])],
        ),
        (
            "PROGRAM Main\nACTION Reset\nEND_ACTION\nEND_PROGRAM",
            vec![("Main", vec!["Reset"])],
        ),
    ];

    for (source, _expected) in cases {
        let symbols = document_symbol_nested(source);
        assert_selection_range_invariant(&symbols);
        assert_children_sorted(&symbols);
    }
}

#[test]
fn document_symbol_invariants_for_complex_source() {
    let source = r#"
TYPE Color : (Red, Green, Blue); END_TYPE
PROGRAM Main
VAR_INPUT
    x : INT;
END_VAR
ACTION Reset
END_ACTION
END_PROGRAM
FUNCTION_BLOCK Counter
VAR_OUTPUT
    count : INT;
END_VAR
END_FUNCTION_BLOCK
"#;
    let symbols = document_symbol_nested(source);
    assert_selection_range_invariant(&symbols);
    assert_children_sorted(&symbols);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_file_returns_empty() {
    let symbols = document_symbol_nested("");
    assert!(symbols.is_empty());
}

#[test]
fn only_globals_returns_flat() {
    let source = r#"VAR_GLOBAL x : INT; y : REAL; END_VAR"#;
    let symbols = document_symbol_nested(source);
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().all(|s| s.children.is_none()));
}

#[test]
fn mixed_file_order_preserved() {
    let source = r#"
VAR_GLOBAL gX : INT; END_VAR
PROGRAM Main VAR_INPUT x : BOOL; END_VAR END_PROGRAM
VAR_GLOBAL gY : REAL; END_VAR
"#;
    let symbols = document_symbol_nested(source);
    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["gX", "Main", "gY"]);
}

#[test]
fn nested_struct_with_fields() {
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

#[test]
fn program_with_action_and_variables() {
    let source = r#"
PROGRAM Main
VAR_INPUT
    x : INT;
END_VAR
ACTION Reset
END_ACTION
END_PROGRAM
"#;
    let symbols = document_symbol_nested(source);
    assert_document_symbol_tree(
        &symbols,
        &[ExpectedNode {
            name: "Main",
            children: Some(vec![
                ExpectedNode {
                    name: "x",
                    children: None,
                },
                ExpectedNode {
                    name: "Reset",
                    children: None,
                },
            ]),
        }],
    );
}

// ---------------------------------------------------------------------------
// Performance test (no criterion in workspace, use plain test)
// ---------------------------------------------------------------------------

fn generate_large_source(n: usize) -> String {
    let mut parts = Vec::with_capacity(n);
    for i in 0..n {
        parts.push(format!(
            "PROGRAM P{i}\nVAR_INPUT v{i} : INT; END_VAR\nACTION A{i}\nEND_ACTION\nEND_PROGRAM\n"
        ));
    }
    parts.join("")
}

#[test]
fn document_symbol_performance_100_symbols() {
    let source = generate_large_source(100);
    let state = crate::state::ServerState::new();
    let uri = tower_lsp::lsp_types::Url::parse("file:///bench.st").unwrap();
    state.open_document(uri.clone(), 1, source);

    let params = tower_lsp::lsp_types::DocumentSymbolParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let start = std::time::Instant::now();
    for _ in 0..10 {
        let _ = crate::handlers::document_symbol(&state, params.clone());
    }
    let elapsed = start.elapsed();
    println!("10 calls over 100 POUs: {:?}", elapsed);
    assert!(elapsed.as_secs() < 30, "too slow: {:?}", elapsed);
}

// ---------------------------------------------------------------------------
// Variable → EnumValue synthetic children
// ---------------------------------------------------------------------------

#[test]
fn document_symbol_variable_enum_shows_values() {
    let source = r#"
TYPE State : (Idle, Running, Error); END_TYPE
PROGRAM Main
    VAR state : State; END_VAR
END_PROGRAM
"#;
    let symbols = document_symbol_nested(source);

    let main = symbols.iter().find(|s| s.name == "Main").expect("Main");
    let state_var = main
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name == "state")
        .expect("state variable");

    let children = state_var.children.as_ref().expect("enum children");
    assert_eq!(children.len(), 3);
    assert!(children.iter().any(|c| c.name == "Idle"));
    assert!(children.iter().any(|c| c.name == "Running"));
    assert!(children.iter().any(|c| c.name == "Error"));
    assert!(children
        .iter()
        .all(|c| c.kind == tower_lsp::lsp_types::SymbolKind::ENUM_MEMBER));
}

#[test]
fn document_symbol_variable_int_no_enum_children() {
    let source = r#"PROGRAM Main VAR x : INT; END_VAR END_PROGRAM"#;
    let symbols = document_symbol_nested(source);
    let main = symbols.iter().find(|s| s.name == "Main").unwrap();
    let x = main
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name == "x")
        .unwrap();
    assert!(x.children.is_none() || x.children.as_ref().unwrap().is_empty());
}

#[test]
fn document_symbol_variable_struct_no_enum_children() {
    let source = r#"
TYPE Point : STRUCT x : INT; y : INT; END_STRUCT END_TYPE
PROGRAM Main VAR p : Point; END_VAR END_PROGRAM
"#;
    let symbols = document_symbol_nested(source);
    let main = symbols.iter().find(|s| s.name == "Main").unwrap();
    let p = main
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name == "p")
        .unwrap();
    assert!(p.children.is_none() || p.children.as_ref().unwrap().is_empty());
}

#[test]
fn document_symbol_variable_empty_enum_no_children() {
    let source = r#"
TYPE Empty : (); END_TYPE
PROGRAM Main VAR e : Empty; END_VAR END_PROGRAM
"#;
    let symbols = document_symbol_nested(source);
    let main = symbols.iter().find(|s| s.name == "Main").unwrap();
    let e = main
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name == "e")
        .unwrap();
    assert!(e.children.is_none() || e.children.as_ref().unwrap().is_empty());
}
