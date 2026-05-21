//! Tests for ACTION block semantic collection.

use trust_hir::db::{Database, FileId, SemanticDatabase, SourceDatabase};
use trust_hir::semantic::{DeclarationKind, SemanticRole};
use trust_hir::symbols::SymbolKind;

fn lookup_by_name(
    symbols: &trust_hir::symbols::SymbolTable,
    name: &str,
) -> Option<trust_hir::symbols::SymbolId> {
    symbols
        .iter()
        .find(|sym| sym.name.eq_ignore_ascii_case(name))
        .map(|sym| sym.id)
}

#[test]
fn collector_creates_action_with_parent_pou() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = r#"
PROGRAM Main
ACTION Reset
END_ACTION
END_PROGRAM
"#;
    db.set_source_text(file, source.to_string());

    let symbols = db.file_symbols(file);
    let action_id = lookup_by_name(&symbols, "Reset").expect("action symbol should exist");
    let action_sym = symbols.get(action_id).unwrap();
    assert!(matches!(action_sym.kind, SymbolKind::Action));
    assert!(action_sym.parent.is_some());
    let parent = symbols.get(action_sym.parent.unwrap()).unwrap();
    assert!(matches!(parent.kind, SymbolKind::Program));
}

#[test]
fn action_has_declaration_kind_action() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"PROGRAM Main ACTION Reset END_ACTION END_PROGRAM"#.to_string(),
    );

    let symbols = db.file_symbols(file);
    let action_id = lookup_by_name(&symbols, "Reset").unwrap();
    let catalog = symbols.declaration_catalog(file);
    let record = catalog
        .entries()
        .iter()
        .find(|e| e.symbol_id() == action_id)
        .expect("action should be in declaration catalog");
    assert_eq!(record.kind(), DeclarationKind::Action);
    assert_eq!(record.role(), SemanticRole::ScopeOwner);
}

#[test]
fn action_in_function_block_has_parent_fb() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"
FUNCTION_BLOCK Counter
ACTION Increment
END_ACTION
END_FUNCTION_BLOCK
"#
        .to_string(),
    );

    let symbols = db.file_symbols(file);
    let action_id = lookup_by_name(&symbols, "Increment").expect("action symbol should exist");
    let action_sym = symbols.get(action_id).unwrap();
    assert!(matches!(action_sym.kind, SymbolKind::Action));
    assert!(action_sym.parent.is_some());
    let parent = symbols.get(action_sym.parent.unwrap()).unwrap();
    assert!(matches!(parent.kind, SymbolKind::FunctionBlock));
}
