//! Invariant tests for symbol table structures.

use rustc_hash::FxHashSet;
use trust_hir::db::{Database, FileId, SemanticDatabase, SourceDatabase};
use trust_hir::symbols::SymbolTable;

fn collect_symbols(source: &str) -> std::sync::Arc<SymbolTable> {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(file, source.to_string());
    db.file_symbols(file)
}

#[test]
fn symbol_parent_no_cycles() {
    let source = r#"
PROGRAM Main
VAR
    x : INT;
END_VAR
ACTION Reset
END_ACTION
END_PROGRAM
"#;
    let symbols = collect_symbols(source);
    for sym in symbols.iter() {
        let mut visited = FxHashSet::default();
        let mut current = sym.parent;
        while let Some(parent_id) = current {
            assert!(
                visited.insert(parent_id),
                "cycle detected involving symbol {} (id={})",
                sym.name,
                sym.id.0
            );
            current = symbols.get(parent_id).and_then(|p| p.parent);
        }
    }
}

#[test]
fn symbol_parent_no_cycles_complex() {
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
ACTION Increment
END_ACTION
END_FUNCTION_BLOCK
"#;
    let symbols = collect_symbols(source);
    for sym in symbols.iter() {
        let mut visited = FxHashSet::default();
        let mut current = sym.parent;
        while let Some(parent_id) = current {
            assert!(
                visited.insert(parent_id),
                "cycle detected involving symbol {} (id={})",
                sym.name,
                sym.id.0
            );
            current = symbols.get(parent_id).and_then(|p| p.parent);
        }
    }
}
