//! Project loading for HMI generation.
//!
//! Adapts the `trust-hir-cli` project loading pattern to discover and
//! parse ST source files, then expose symbol tables for HMI binding
//! extraction.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use trust_hir::db::{Database, FileId, SemanticDatabase};
use trust_hir::project::{Project, SourceKey};
use trust_hir::symbols::{SymbolKind, SymbolTable, VarQualifier};
use trust_hir::types::Type;

/// Wrapper around a loaded `trust-hir` project with helpers for HMI
/// generation.
pub struct ProjectLoader {
    project: Project,
    project_root: PathBuf,
    file_ids: Vec<FileId>,
}

impl ProjectLoader {
    /// Load all `.st`, `.pou`, and `.gvl` files under `project_root`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read or any source
    /// file fails to load.
    pub fn load(project_root: &Path) -> anyhow::Result<Self> {
        let mut project = Project::new();
        let mut file_ids = Vec::new();
        Self::load_dir(project_root, project_root, &mut project, &mut file_ids)?;

        if file_ids.is_empty() {
            anyhow::bail!("no ST source files found in '{}'", project_root.display());
        }

        Ok(Self {
            project,
            project_root: project_root.to_path_buf(),
            file_ids,
        })
    }

    /// Recursively load source files.
    #[allow(clippy::only_used_in_recursion)]
    fn load_dir(
        dir: &Path,
        project_root: &Path,
        project: &mut Project,
        file_ids: &mut Vec<FileId>,
    ) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::load_dir(&path, project_root, project, file_ids)?;
            } else if path.extension().is_some_and(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                e == "st" || e == "pou" || e == "gvl"
            }) {
                let content = std::fs::read_to_string(&path)?;
                let key = SourceKey::from_path(&path);
                let file_id = project.set_source_text(key, content);
                file_ids.push(file_id);
            }
        }
        Ok(())
    }

    /// Access the project root path.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Access the underlying HIR project.
    #[allow(dead_code)]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Access the HIR database.
    #[allow(dead_code)]
    pub fn database(&self) -> &Database {
        self.project.database()
    }

    /// Iterate over all loaded file IDs.
    #[allow(dead_code)]
    pub fn file_ids(&self) -> &[FileId] {
        &self.file_ids
    }

    /// Collect all symbols across all files.
    pub fn all_symbols(&self) -> Vec<SymbolRef> {
        let mut result = Vec::new();
        for &file_id in &self.file_ids {
            let db = self.project.database();
            let symbols = db.file_symbols(file_id);
            for symbol in symbols.iter() {
                if !symbol.range.is_empty() {
                    result.push(SymbolRef {
                        name: symbol.name.to_string(),
                        id: symbol.id,
                        kind: symbol.kind.clone(),
                        type_id: symbol.type_id,
                        parent: symbol.parent,
                        table: Arc::clone(&symbols),
                        file_id,
                    });
                }
            }
        }
        result
    }

    /// Collect global variables (top-level `VAR_GLOBAL` or standalone
    /// globals in GVL files).
    pub fn global_variables(&self) -> Vec<SymbolRef> {
        self.all_symbols()
            .into_iter()
            .filter(|s| matches!(s.kind, SymbolKind::Variable { .. }) && s.parent.is_none())
            .collect()
    }

    /// Collect program declarations and their variable children.
    pub fn programs_with_variables(&self) -> Vec<ProgramRef> {
        let all = self.all_symbols();
        let programs: Vec<_> = all
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Program))
            .map(|s| s.id)
            .collect();

        programs
            .into_iter()
            .map(|program_id| {
                let program_sym = all
                    .iter()
                    .find(|s| s.id == program_id)
                    .expect("program symbol exists");

                let variables: Vec<_> = all
                    .iter()
                    .filter(|s| {
                        s.parent == Some(program_id)
                            && matches!(s.kind, SymbolKind::Variable { .. })
                    })
                    .cloned()
                    .collect();

                ProgramRef {
                    name: program_sym.name.clone(),
                    id: program_id,
                    variables,
                }
            })
            .collect()
    }
}

/// Reference to a symbol within its symbol table.
#[derive(Clone)]
pub struct SymbolRef {
    /// The symbol name (owned copy).
    pub name: String,
    /// The symbol ID.
    #[allow(dead_code)]
    pub id: trust_hir::symbols::SymbolId,
    /// The symbol kind.
    pub kind: SymbolKind,
    /// The symbol type ID.
    pub type_id: trust_hir::types::TypeId,
    /// Parent symbol ID, if any.
    pub parent: Option<trust_hir::symbols::SymbolId>,
    /// The owning symbol table.
    pub table: Arc<SymbolTable>,
    /// The source file ID.
    #[allow(dead_code)]
    pub file_id: FileId,
}

/// A program and its variables.
pub struct ProgramRef {
    /// The program name.
    pub name: String,
    /// The program symbol ID.
    #[allow(dead_code)]
    pub id: trust_hir::symbols::SymbolId,
    /// Variables declared inside the program.
    pub variables: Vec<SymbolRef>,
}

/// Resolve the display name for a type ID using the symbol table.
pub fn type_name_for_symbol(table: &SymbolTable, type_id: trust_hir::types::TypeId) -> String {
    table
        .type_name(type_id)
        .map_or_else(|| "UNKNOWN".to_string(), |name| name.to_string())
}

/// Determine if a symbol is marked for HMI export via naming convention.
///
/// Current heuristics:
/// - Names prefixed with `hmi_` or `HMI_`
/// - Names ending with `_HMI`
#[allow(dead_code)]
pub fn is_hmi_export_symbol(sym: &trust_hir::symbols::Symbol) -> bool {
    let name = sym.name.as_str();
    let name_upper = name.to_ascii_uppercase();

    // Naming convention markers
    name_upper.starts_with("HMI_") || name_upper.ends_with("_HMI")
}

/// Map a HIR type to an HMI widget type string.
pub fn widget_for_hir_type(ty: &Type, writable: bool) -> &'static str {
    match ty {
        Type::Bool => {
            if writable {
                "toggle"
            } else {
                "indicator"
            }
        }
        Type::Enum { .. } => {
            if writable {
                "selector"
            } else {
                "readout"
            }
        }
        Type::Array { .. } => "table",
        Type::Struct { .. }
        | Type::Union { .. }
        | Type::FunctionBlock { .. }
        | Type::Class { .. }
        | Type::Interface { .. } => "tree",
        ty if ty.is_string() || ty.is_char() => "text",
        ty if (ty.is_numeric() || ty.is_bit_string() || ty.is_time()) && writable => "slider",
        ty if ty.is_numeric() || ty.is_bit_string() || ty.is_time() => "value",
        _ => "value",
    }
}

/// Determine if a variable qualifier implies writability for HMI.
pub fn is_writable_qualifier(qualifier: VarQualifier) -> bool {
    matches!(
        qualifier,
        VarQualifier::Input | VarQualifier::InOut | VarQualifier::Global
    )
}

/// Extract the variable qualifier string from symbol kind.
#[allow(dead_code)]
pub fn qualifier_for_symbol(sym: &trust_hir::symbols::Symbol) -> &'static str {
    match &sym.kind {
        SymbolKind::Variable { qualifier } => match qualifier {
            VarQualifier::Local => "VAR",
            VarQualifier::Input => "VAR_INPUT",
            VarQualifier::Output => "VAR_OUTPUT",
            VarQualifier::InOut => "VAR_IN_OUT",
            VarQualifier::Temp => "VAR_TEMP",
            VarQualifier::Global => "VAR_GLOBAL",
            VarQualifier::External => "VAR_EXTERNAL",
            VarQualifier::Access => "VAR_ACCESS",
            VarQualifier::Static => "VAR_STAT",
        },
        _ => "VAR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text_size::TextRange;
    use trust_hir::symbols::{Symbol, SymbolId};
    use trust_hir::types::TypeId;

    #[test]
    fn widget_mapping_bool() {
        assert_eq!(widget_for_hir_type(&Type::Bool, false), "indicator");
        assert_eq!(widget_for_hir_type(&Type::Bool, true), "toggle");
    }

    #[test]
    fn widget_mapping_numeric() {
        assert_eq!(widget_for_hir_type(&Type::DInt, false), "value");
        assert_eq!(widget_for_hir_type(&Type::DInt, true), "slider");
    }

    #[test]
    fn widget_mapping_string() {
        assert_eq!(
            widget_for_hir_type(&Type::String { max_len: Some(80) }, false),
            "text"
        );
        assert_eq!(
            widget_for_hir_type(&Type::String { max_len: Some(80) }, true),
            "text"
        );
    }

    #[test]
    fn widget_mapping_array() {
        assert_eq!(
            widget_for_hir_type(
                &Type::Array {
                    element: TypeId::VOID,
                    dimensions: vec![]
                },
                false
            ),
            "table"
        );
    }

    #[test]
    fn hmi_naming_convention() {
        let sym = Symbol::new(
            SymbolId::UNKNOWN,
            "hmi_MotorSpeed",
            SymbolKind::Variable {
                qualifier: VarQualifier::Global,
            },
            TypeId::REAL,
            TextRange::empty(0.into()),
        );
        assert!(is_hmi_export_symbol(&sym));

        let sym2 = Symbol::new(
            SymbolId::UNKNOWN,
            "MotorSpeed_HMI",
            SymbolKind::Variable {
                qualifier: VarQualifier::Local,
            },
            TypeId::REAL,
            TextRange::empty(0.into()),
        );
        assert!(is_hmi_export_symbol(&sym2));

        let sym3 = Symbol::new(
            SymbolId::UNKNOWN,
            "InternalTemp",
            SymbolKind::Variable {
                qualifier: VarQualifier::Temp,
            },
            TypeId::REAL,
            TextRange::empty(0.into()),
        );
        assert!(!is_hmi_export_symbol(&sym3));
    }
}
