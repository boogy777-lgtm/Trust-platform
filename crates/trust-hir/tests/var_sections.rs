mod common;

use common::*;
use std::sync::Arc;

fn collect_symbols(source: &str) -> trust_hir::symbols::SymbolTable {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(file, source.to_string());
    Arc::try_unwrap(db.file_symbols(file)).unwrap_or_else(|arc| (*arc).clone())
}

#[test]
fn iec_table13() {
    check_no_errors(
        r#"
FUNCTION_BLOCK DemoFb
VAR_INPUT
    i : INT;
END_VAR
VAR_OUTPUT
    o : INT;
END_VAR
VAR_IN_OUT
    io : INT;
END_VAR
VAR_TEMP
    t : INT;
END_VAR
VAR
    v : INT;
END_VAR
END_FUNCTION_BLOCK

PROGRAM Main
VAR_EXTERNAL
    g : INT;
END_VAR
END_PROGRAM

CONFIGURATION Conf
VAR_GLOBAL
    g : INT;
END_VAR
END_CONFIGURATION
"#,
    );
}

#[test]
fn program_var_global_is_accepted() {
    check_no_errors(
        r#"
PROGRAM Main
VAR_GLOBAL
    G : INT;
END_VAR
END_PROGRAM
"#,
    );
}

#[test]
fn file_scope_var_global_is_accepted_across_files() {
    let gvl = r#"
VAR_GLOBAL
    G : INT;
END_VAR
"#;
    let consumer = r#"
PROGRAM Main
VAR_EXTERNAL
    G : INT;
END_VAR
END_PROGRAM
"#;
    check_no_errors_multi(&[gvl, consumer]);
}

#[test]
fn multiple_file_scope_gvls_are_aggregated() {
    let gvl_a = r#"
VAR_GLOBAL
    G_A : INT;
END_VAR
"#;
    let gvl_b = r#"
VAR_GLOBAL
    G_B : INT;
END_VAR
"#;
    let consumer = r#"
PROGRAM Main
VAR_EXTERNAL
    G_A : INT;
    G_B : INT;
END_VAR
END_PROGRAM
    "#;
    check_no_errors_multi(&[gvl_a, gvl_b, consumer]);
}

#[test]
fn duplicate_global_names_across_scopes_are_rejected() {
    check_has_error(
        r#"
VAR_GLOBAL
    G : INT;
END_VAR

PROGRAM Main
VAR_GLOBAL
    G : INT;
END_VAR
END_PROGRAM
"#,
        DiagnosticCode::DuplicateDeclaration,
    );
}

#[test]
fn duplicate_file_scope_global_names_are_rejected_by_collector_path() {
    check_has_error(
        r#"
VAR_GLOBAL
    G : INT;
END_VAR

VAR_GLOBAL
    G : DINT;
END_VAR
"#,
        DiagnosticCode::DuplicateDeclaration,
    );
}

#[test]
fn edge_detection_uses_children_with_tokens() {
    let source = r#"PROGRAM Main VAR x : BOOL R_EDGE; END_VAR END_PROGRAM"#;
    let symbols = collect_symbols(source);
    let x = symbols.iter().find(|sym| sym.name == "x").unwrap().id;
    let sym = symbols.get(x).unwrap();
    assert_eq!(sym.edge.as_deref(), Some("R_EDGE"));
}

#[test]
fn persistence_from_var_block_modifiers() {
    let source = r#"PROGRAM Main VAR_INPUT RETAIN x : INT; END_VAR END_PROGRAM"#;
    let symbols = collect_symbols(source);
    let x = symbols.iter().find(|sym| sym.name == "x").unwrap().id;
    let sym = symbols.get(x).unwrap();
    assert_eq!(sym.persistence.as_deref(), Some("RETAIN"));
}
