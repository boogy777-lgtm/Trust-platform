//! Tests for struct/union field symbol collection.

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
fn collector_creates_field_with_parent_type() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = r#"
TYPE Point :
STRUCT
    x : INT;
    y : INT;
END_STRUCT
END_TYPE
"#;
    db.set_source_text(file, source.to_string());

    let symbols = db.file_symbols(file);
    let field_x = lookup_by_name(&symbols, "x").expect("field x should exist");
    let sym = symbols.get(field_x).unwrap();
    assert!(matches!(sym.kind, SymbolKind::Field { .. }));
    assert!(sym.parent.is_some());
    let parent = symbols.get(sym.parent.unwrap()).unwrap();
    assert!(matches!(parent.kind, SymbolKind::Type));
    assert_eq!(parent.name, "Point");
}

#[test]
fn union_creates_field_with_parent_type() {
    let mut db = Database::new();
    let file = FileId(0);
    let source = r#"
TYPE MyUnion :
UNION
    i : INT;
    r : REAL;
END_UNION
END_TYPE
"#;
    db.set_source_text(file, source.to_string());

    let symbols = db.file_symbols(file);
    let field_i = lookup_by_name(&symbols, "i").expect("union field i should exist");
    let sym = symbols.get(field_i).unwrap();
    assert!(matches!(sym.kind, SymbolKind::Field { .. }));
    assert!(sym.parent.is_some());
    let parent = symbols.get(sym.parent.unwrap()).unwrap();
    assert!(matches!(parent.kind, SymbolKind::Type));
    assert_eq!(parent.name, "MyUnion");
}

#[test]
fn field_declaration_kind_and_role() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"TYPE Point : STRUCT x : INT; END_STRUCT END_TYPE"#.to_string(),
    );

    let symbols = db.file_symbols(file);
    let field_id = lookup_by_name(&symbols, "x").unwrap();
    let catalog = symbols.declaration_catalog(file);
    let record = catalog
        .entries()
        .iter()
        .find(|e| e.symbol_id() == field_id)
        .expect("field should be in declaration catalog");
    assert_eq!(record.kind(), DeclarationKind::Field);
    assert_eq!(record.role(), SemanticRole::Value);
}

#[test]
fn struct_field_has_correct_type_id() {
    let mut db = Database::new();
    let file = FileId(0);
    db.set_source_text(
        file,
        r#"TYPE Point : STRUCT x : INT; y : REAL; END_STRUCT END_TYPE"#.to_string(),
    );

    let symbols = db.file_symbols(file);
    let field_x = lookup_by_name(&symbols, "x").unwrap();
    let sym_x = symbols.get(field_x).unwrap();
    if let SymbolKind::Field { field_type } = sym_x.kind {
        assert_eq!(field_type, trust_hir::TypeId::INT);
    } else {
        panic!("expected Field kind");
    }

    let field_y = lookup_by_name(&symbols, "y").unwrap();
    let sym_y = symbols.get(field_y).unwrap();
    if let SymbolKind::Field { field_type } = sym_y.kind {
        assert_eq!(field_type, trust_hir::TypeId::REAL);
    } else {
        panic!("expected Field kind");
    }
}

// Note: direct_address for struct fields depends on parser support for AT %... inside STRUCT.
// This is not tested here as it's outside PR-B acceptance criteria.
