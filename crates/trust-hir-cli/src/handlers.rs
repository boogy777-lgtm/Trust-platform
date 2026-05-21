//! JSON-RPC method handlers.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::{json, Value};
use smol_str::SmolStr;
use trust_hir::db::{FileId, SemanticDatabase, SourceDatabase};
use trust_hir::project::{Project, SourceKey};
use trust_hir::symbols::{SymbolId, SymbolKind, SymbolTable, VarQualifier};
use trust_hir::types::Type;
use trust_ide::call_hierarchy::{collect_project_call_edges, SimpleCallEdge};
use trust_ide::references::{find_references, FindReferencesOptions};

use crate::protocol::{INVALID_PARAMS, NOT_INITIALIZED, PATH_ESCAPE};

// ---------------------------------------------------------------------------
// Daemon state
// ---------------------------------------------------------------------------

/// Daemon-wide state that may hold a loaded project.
pub struct DaemonState {
    project_state: RwLock<Option<Arc<ProjectState>>>,
}

impl DaemonState {
    /// Create uninitialized daemon state.
    pub fn new() -> Self {
        Self {
            project_state: RwLock::new(None),
        }
    }

    /// Initialize (or re-initialize) with a project root path.
    pub fn initialize(&self, project_root: PathBuf) -> anyhow::Result<()> {
        let state = ProjectState::new(project_root)?;
        *self.project_state.write() = Some(Arc::new(state));
        Ok(())
    }

    /// Run a function with the project state, or return a not-initialized error.
    pub fn with_state<F, R>(&self, f: F) -> Result<R, RpcError>
    where
        F: FnOnce(&ProjectState) -> Result<R, RpcError>,
    {
        let guard = self.project_state.read();
        match guard.as_ref() {
            Some(state) => f(state),
            None => Err(RpcError {
                code: NOT_INITIALIZED,
                message: "Project not initialized".to_string(),
            }),
        }
    }
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Project state
// ---------------------------------------------------------------------------

/// Per-project state wrapping the HIR project and root path.
pub struct ProjectState {
    project: RwLock<Project>,
    project_root: PathBuf,
}

impl ProjectState {
    /// Load a project from a directory tree.
    pub fn new(project_root: PathBuf) -> anyhow::Result<Self> {
        let mut project = Project::new();
        load_project_files(&project_root, &mut project)?;
        Ok(Self {
            project: RwLock::new(project),
            project_root,
        })
    }

    /// Access the project root.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Read-only access to the project.
    pub fn with_project<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Project) -> R,
    {
        f(&self.project.read())
    }

    /// Mutable access to the project.
    pub fn with_project_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Project) -> R,
    {
        f(&mut self.project.write())
    }
}

fn load_project_files(project_root: &Path, project: &mut Project) -> anyhow::Result<()> {
    load_dir(project_root, project_root, project)?;
    Ok(())
}

fn load_dir(dir: &Path, _project_root: &Path, project: &mut Project) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            load_dir(&path, _project_root, project)?;
        } else if path.extension().is_some_and(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "st" || e == "pou" || e == "gvl"
        }) {
            let content = std::fs::read_to_string(&path)?;
            let key = SourceKey::from_path(&path);
            project.set_source_text(key, content);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Error type for handler failures.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// Manually normalize a path without requiring it to exist.
fn manual_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(p) => result.push(p.as_os_str()),
            std::path::Component::RootDir => result.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::Normal(name) => result.push(name),
        }
    }
    result
}

/// Validate that a user-supplied path stays within the project root.
///
/// Works for both existing and non-existing paths.
pub fn validate_path(input_path: &str, project_root: &Path) -> Result<PathBuf, RpcError> {
    if input_path.contains('\0') {
        return Err(RpcError::new(INVALID_PARAMS, "Path contains null bytes"));
    }

    let path = Path::new(input_path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };

    let normalized = manual_normalize(&absolute);
    let root_normalized = manual_normalize(project_root);

    if !normalized.starts_with(&root_normalized) {
        return Err(RpcError::new(PATH_ESCAPE, "Path escapes workspace root"));
    }

    Ok(normalized)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `hir/initialize` – load or re-load a project.
pub fn handle_initialize(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    let project_path = params
        .get("projectPath")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing projectPath"))?;

    let path = PathBuf::from(project_path);
    if !path.exists() {
        return Err(RpcError::new(
            NOT_INITIALIZED,
            format!("Project path does not exist: {project_path}"),
        ));
    }

    state.initialize(path).map_err(|e| {
        RpcError::new(
            NOT_INITIALIZED,
            format!("Failed to initialize project: {e}"),
        )
    })?;

    Ok(json!({"status":"ready"}))
}

/// `hir/getSymbols` – query symbols.
pub fn handle_get_symbols(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let file_path = params.get("filePath").and_then(Value::as_str);
        let kind_filter = params.get("kind").and_then(Value::as_str);
        let scope_filter = params.get("scope").and_then(Value::as_str);

        let symbols = if let Some(file_path) = file_path {
            let path = validate_path(file_path, project_state.project_root())?;
            let key = SourceKey::from_path(&path);
            let file_id = project_state
                .with_project(|p| p.file_id_for_key(&key))
                .ok_or_else(|| RpcError::new(NOT_INITIALIZED, "File not found in project"))?;
            collect_file_symbols(project_state, file_id, kind_filter, scope_filter)?
        } else {
            collect_all_symbols(project_state, kind_filter, scope_filter)?
        };

        Ok(json!({ "symbols": symbols }))
    })
}

/// `hir/getTypes` – query type definitions.
pub fn handle_get_types(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let file_path = params.get("filePath").and_then(Value::as_str);
        let name_filter = params.get("name").and_then(Value::as_str);

        let types = if let Some(file_path) = file_path {
            let path = validate_path(file_path, project_state.project_root())?;
            let key = SourceKey::from_path(&path);
            let file_id = project_state
                .with_project(|p| p.file_id_for_key(&key))
                .ok_or_else(|| RpcError::new(NOT_INITIALIZED, "File not found in project"))?;
            collect_file_types(project_state, file_id, name_filter)?
        } else {
            collect_all_types(project_state, name_filter)?
        };

        Ok(json!({ "types": types }))
    })
}

/// `hir/getReferences` – find references to a symbol.
pub fn handle_get_references(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let symbol_id = params
            .get("symbolId")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing symbolId"))?;

        let include_declaration = params
            .get("includeDeclaration")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let references = find_symbol_references(project_state, symbol_id, include_declaration)?;
        Ok(json!({ "references": references }))
    })
}

/// `hir/getDiagnostics` – collect diagnostics.
pub fn handle_get_diagnostics(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let file_path = params.get("filePath").and_then(Value::as_str);

        let diagnostics = if let Some(file_path) = file_path {
            let path = validate_path(file_path, project_state.project_root())?;
            let key = SourceKey::from_path(&path);
            let file_id = project_state
                .with_project(|p| p.file_id_for_key(&key))
                .ok_or_else(|| RpcError::new(NOT_INITIALIZED, "File not found in project"))?;
            collect_file_diagnostics(project_state, file_id)?
        } else {
            collect_all_diagnostics(project_state)?
        };

        Ok(json!({ "diagnostics": diagnostics }))
    })
}

/// `hir/snapshot` – export full HIR snapshot.
pub fn handle_snapshot(state: &DaemonState, _params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let snapshot = build_snapshot(project_state)?;
        Ok(json!({ "snapshot": snapshot }))
    })
}

/// `hir/updateFile` – update or create a source file.
pub fn handle_update_file(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    state.with_state(|project_state| {
        let file_path = params
            .get("filePath")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing filePath"))?;
        let content = params
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing content"))?;

        let path = validate_path(file_path, project_state.project_root())?;
        let key = SourceKey::from_path(&path);
        project_state.with_project_mut(|p| {
            p.set_source_text(key, content.to_string());
        });

        Ok(json!({ "status": "ok" }))
    })
}

/// `hir/shutdown` – graceful shutdown.
pub fn handle_shutdown(_state: &DaemonState, _params: Value) -> Result<Value, RpcError> {
    Ok(json!({}))
}

/// `hir/index` – index project files into graph-friendly entities.
pub fn handle_index(state: &DaemonState, params: Value) -> Result<Value, RpcError> {
    let start = std::time::Instant::now();
    state.with_state(|project_state| {
        let file_paths = params.get("filePaths");
        let (file_ids, warnings) = match file_paths {
            Some(Value::Null) | None => {
                let ids = project_state.with_project(|p| p.database().file_ids());
                (ids, Vec::new())
            }
            Some(Value::Array(arr)) if arr.is_empty() => {
                let ids = project_state.with_project(|p| p.database().file_ids());
                (ids, Vec::new())
            }
            Some(Value::Array(arr)) => {
                let mut ids = Vec::new();
                let mut warnings = Vec::new();
                for path_val in arr {
                    if let Some(path_str) = path_val.as_str() {
                        match validate_path(path_str, project_state.project_root()) {
                            Ok(path) => {
                                let key = SourceKey::from_path(&path);
                                if let Some(file_id) =
                                    project_state.with_project(|p| p.file_id_for_key(&key))
                                {
                                    ids.push(file_id);
                                } else {
                                    warnings.push(format!(
                                        "File not found in project: {path_str}"
                                    ));
                                }
                            }
                            Err(e) => {
                                warnings.push(format!(
                                    "Invalid path '{path_str}': {}",
                                    e.message
                                ));
                            }
                        }
                    } else {
                        warnings.push("Non-string entry in filePaths array".to_string());
                    }
                }
                (ids, warnings)
            }
            _ => {
                let ids = project_state.with_project(|p| p.database().file_ids());
                (
                    ids,
                    vec!["Invalid filePaths parameter; indexing all files".to_string()],
                )
            }
        };

        let total_files = file_ids.len();
        let mut indexed_files = 0usize;
        let mut all_diagnostics: Vec<DiagnosticOut> = Vec::new();
        let mut file_results = Vec::new();

        // Pre-collect call edges for the selected files.
        let call_edges = project_state.with_project(|p| {
            collect_project_call_edges(p.database(), file_ids.clone())
        });

        // Group call edges by caller file for efficient lookup.
        let mut edges_by_caller: HashMap<FileId, Vec<&SimpleCallEdge>> = HashMap::new();
        for edge in &call_edges {
            edges_by_caller
                .entry(edge.caller_file_id)
                .or_default()
                .push(edge);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for file_id in &file_ids {
            let mut file_result = json!({
                "filePath": null,
                "pous": [],
                "variables": [],
                "types": [],
                "fields": [],
                "relationships": [],
            });

            let success = project_state.with_project(|project| {
                let db = project.database();
                let symbols = db.file_symbols(*file_id);
                let content = db.source_text(*file_id);
                let path_str = match file_id_to_path(project, *file_id) {
                    Some(p) => p,
                    None => return false,
                };

                let mut pous = Vec::new();
                let mut variables = Vec::new();
                let mut types = Vec::new();
                let mut fields = Vec::new();
                let mut relationships = Vec::new();

                // Build name maps for symbols in this file.
                let mut symbol_names: HashMap<SymbolId, String> = HashMap::new();
                let mut pou_ids: HashMap<SymbolId, String> = HashMap::new();

                for symbol in symbols.iter() {
                    if symbol.range.is_empty() {
                        continue;
                    }
                    symbol_names.insert(symbol.id, symbol.name.to_string());

                    if is_pou_kind(&symbol.kind) {
                        let pou_id = format!("st:{}:{}", path_str, symbol.name);
                        pou_ids.insert(symbol.id, pou_id.clone());
                        let line = offset_to_line_col(&content, symbol.range.start().into()).0;
                        pous.push(json!({
                            "id": pou_id,
                            "name": symbol.name.to_string(),
                            "kind": pou_kind_to_string(&symbol.kind),
                            "line": line,
                            "created_at": now,
                            "updated_at": now,
                        }));
                    }
                }

                // Variables and Types
                for symbol in symbols.iter() {
                    if symbol.range.is_empty() {
                        continue;
                    }

                    match &symbol.kind {
                        SymbolKind::Variable { qualifier } => {
                            if let Some(pou_id) =
                                symbol.parent.and_then(|pid| pou_ids.get(&pid))
                            {
                                let line =
                                    offset_to_line_col(&content, symbol.range.start().into()).0;
                                variables.push(json!({
                                    "id": format!("st:var:{}:{}", pou_id, symbol.name),
                                    "name": symbol.name.to_string(),
                                    "direction": var_qualifier_to_string(qualifier),
                                    "pouId": pou_id,
                                    "line": line,
                                    "created_at": now,
                                    "updated_at": now,
                                }));
                            }
                        }
                        SymbolKind::Type => {
                            let type_id_str = format!("st:{}:type:{}", path_str, symbol.name);
                            let line =
                                offset_to_line_col(&content, symbol.range.start().into()).0;
                            let type_info = symbols.type_by_id(symbol.type_id);
                            let type_kind = type_to_kind_string(type_info);

                            types.push(json!({
                                "id": type_id_str,
                                "name": symbol.name.to_string(),
                                "type_kind": type_kind,
                                "line": line,
                                "created_at": now,
                                "updated_at": now,
                            }));

                            if let Some(Type::Struct { fields: struct_fields, .. }) = type_info {
                                for field in struct_fields {
                                    fields.push(json!({
                                        "id": format!("st:{}:field:{}", type_id_str, field.name),
                                        "name": field.name.to_string(),
                                        "typeId": type_id_str,
                                        "created_at": now,
                                        "updated_at": now,
                                    }));
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Relationships: EXTENDS, IMPLEMENTS, CONTAINS
                for symbol in symbols.iter() {
                    if symbol.range.is_empty() || !is_pou_kind(&symbol.kind) {
                        continue;
                    }

                    let from_id = format!("st:{}:{}", path_str, symbol.name);
                    let line = offset_to_line_col(&content, symbol.range.start().into()).0;

                    if let Some(extends_name) = symbols.extends_name(symbol.id) {
                        relationships.push(json!({
                            "id": format!("st:rel:{}:{}:{}:EXTENDS:{}", path_str, symbol.name, extends_name, line),
                            "from": from_id.clone(),
                            "to": format!("st:{}:{}", path_str, extends_name),
                            "kind": "EXTENDS",
                            "line": line,
                            "created_at": now,
                            "updated_at": now,
                        }));
                    }

                    if let Some(iface_names) = symbols.implements_names(symbol.id) {
                        for iface_name in iface_names {
                            relationships.push(json!({
                                "id": format!("st:rel:{}:{}:{}:IMPLEMENTS:{}", path_str, symbol.name, iface_name, line),
                                "from": from_id.clone(),
                                "to": format!("st:{}:{}", path_str, iface_name),
                                "kind": "IMPLEMENTS",
                                "line": line,
                                "created_at": now,
                                "updated_at": now,
                            }));
                        }
                    }

                    if let Some(parent_id) = symbol.parent {
                        if let Some(parent_name) = symbol_names.get(&parent_id) {
                            relationships.push(json!({
                                "id": format!("st:rel:{}:{}:{}:CONTAINS:{}", path_str, parent_name, symbol.name, line),
                                "from": format!("st:{}:{}", path_str, parent_name),
                                "to": from_id.clone(),
                                "kind": "CONTAINS",
                                "line": line,
                                "created_at": now,
                                "updated_at": now,
                            }));
                        }
                    }
                }

                // Relationships: CALLS
                if let Some(edges) = edges_by_caller.get(file_id) {
                    for edge in edges {
                        let caller_name = symbol_names
                            .get(&edge.caller_symbol_id)
                            .cloned()
                            .unwrap_or_default();
                        let callee_name = if edge.callee_file_id == *file_id {
                            symbol_names
                                .get(&edge.callee_symbol_id)
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            let callee_symbols = db.file_symbols(edge.callee_file_id);
                            callee_symbols
                                .get(edge.callee_symbol_id)
                                .map(|s| s.name.to_string())
                                .unwrap_or_default()
                        };
                        if !caller_name.is_empty() && !callee_name.is_empty() {
                            relationships.push(json!({
                                "id": format!("st:rel:{}:{}:{}:CALLS:{}", path_str, caller_name, callee_name, edge.line),
                                "from": format!("st:{}:{}", path_str, caller_name),
                                "to": format!("st:{}:{}", path_str, callee_name),
                                "kind": "CALLS",
                                "line": edge.line,
                                "created_at": now,
                                "updated_at": now,
                            }));
                        }
                    }
                }

                // Diagnostics
                let diagnostics = trust_ide::diagnostics::collect_diagnostics(db, *file_id);
                for diag in diagnostics {
                    let (line, col) = offset_to_line_col(&content, diag.range.start().into());
                    all_diagnostics.push(DiagnosticOut {
                        path: path_str.clone(),
                        severity: match diag.severity {
                            trust_hir::DiagnosticSeverity::Error => "error",
                            trust_hir::DiagnosticSeverity::Warning => "warning",
                            trust_hir::DiagnosticSeverity::Info => "info",
                            trust_hir::DiagnosticSeverity::Hint => "hint",
                        }
                        .to_string(),
                        line,
                        col,
                        message: diag.message,
                        code: Some(diag.code.code().to_string()),
                    });
                }

                file_result["filePath"] = json!(path_str);
                file_result["pous"] = json!(pous);
                file_result["variables"] = json!(variables);
                file_result["types"] = json!(types);
                file_result["fields"] = json!(fields);
                file_result["relationships"] = json!(relationships);
                true
            });

            if success {
                indexed_files += 1;
                file_results.push(file_result);
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(json!({
            "files": file_results,
            "diagnostics": all_diagnostics,
            "warnings": warnings,
            "stats": {
                "totalFiles": total_files,
                "indexedFiles": indexed_files,
                "totalTimeMs": elapsed,
            }
        }))
    })
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
struct SymbolOut {
    id: String,
    name: String,
    kind: String,
    file_path: String,
    range: RangeOut,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<SymbolOut>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RangeOut {
    start: PositionOut,
    end: PositionOut,
}

#[derive(Debug, Clone, serde::Serialize)]
struct PositionOut {
    line: u32,
    col: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
struct TypeOut {
    name: String,
    kind: String,
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<TypeFieldOut>>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct TypeFieldOut {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DiagnosticOut {
    path: String,
    severity: String,
    line: u32,
    col: u32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ReferenceOut {
    file_path: String,
    range: RangeOut,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SnapshotOut {
    metadata: SnapshotMetadata,
    symbols: Vec<SymbolOut>,
    types: Vec<TypeOut>,
    diagnostics: Vec<DiagnosticOut>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SnapshotMetadata {
    project_path: String,
    export_time: String,
    entity_count: usize,
    edge_count: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn symbol_kind_to_string(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Program => "program",
        SymbolKind::Configuration => "configuration",
        SymbolKind::Resource => "resource",
        SymbolKind::Task => "task",
        SymbolKind::ProgramInstance => "program_instance",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Function { .. } => "function",
        SymbolKind::FunctionBlock => "function_block",
        SymbolKind::Class => "class",
        SymbolKind::Method { .. } => "method",
        SymbolKind::Property { .. } => "property",
        SymbolKind::Interface => "interface",
        SymbolKind::Variable { .. } => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::Type => "type",
        SymbolKind::EnumValue { .. } => "enum_value",
        SymbolKind::Parameter { .. } => "parameter",
        SymbolKind::Action => "action",
        SymbolKind::Field { .. } => "field",
    }
}

fn is_pou_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Program
            | SymbolKind::Function { .. }
            | SymbolKind::FunctionBlock
            | SymbolKind::Class
            | SymbolKind::Method { .. }
            | SymbolKind::Interface
    )
}

fn pou_kind_to_string(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Program => "program",
        SymbolKind::Function { .. } => "function",
        SymbolKind::FunctionBlock => "function_block",
        SymbolKind::Class => "class",
        SymbolKind::Method { .. } => "method",
        SymbolKind::Interface => "interface",
        _ => "unknown",
    }
}

fn var_qualifier_to_string(qual: &VarQualifier) -> &'static str {
    match qual {
        VarQualifier::Local => "VAR",
        VarQualifier::Input => "VAR_INPUT",
        VarQualifier::Output => "VAR_OUTPUT",
        VarQualifier::InOut => "VAR_IN_OUT",
        VarQualifier::Temp => "VAR_TEMP",
        VarQualifier::Global => "VAR_GLOBAL",
        VarQualifier::External => "VAR_EXTERNAL",
        VarQualifier::Access => "VAR_ACCESS",
        VarQualifier::Static => "VAR_STAT",
    }
}

fn type_to_kind_string(type_info: Option<&Type>) -> &'static str {
    match type_info {
        Some(Type::Struct { .. }) => "struct",
        Some(Type::Enum { .. }) => "enum",
        Some(Type::Alias { .. }) => "alias",
        Some(Type::Array { .. }) => "array",
        Some(Type::Union { .. }) => "union",
        Some(Type::Pointer { .. }) => "pointer",
        Some(Type::Subrange { .. }) => "subrange",
        _ => "unknown",
    }
}

fn offset_to_line_col(content: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in content.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn build_range(content: &str, range: text_size::TextRange) -> RangeOut {
    let (start_line, start_col) = offset_to_line_col(content, range.start().into());
    let (end_line, end_col) = offset_to_line_col(content, range.end().into());
    RangeOut {
        start: PositionOut {
            line: start_line,
            col: start_col,
        },
        end: PositionOut {
            line: end_line,
            col: end_col,
        },
    }
}

fn file_id_to_path(project: &Project, file_id: FileId) -> Option<String> {
    project.key_for_file_id(file_id).map(|key| key.display())
}

fn collect_file_symbols(
    project_state: &ProjectState,
    file_id: FileId,
    kind_filter: Option<&str>,
    scope_filter: Option<&str>,
) -> Result<Vec<SymbolOut>, RpcError> {
    let (symbols, content, path_str) = project_state.with_project(|project| {
        let db = project.database();
        let symbols = db.file_symbols(file_id);
        let content = db.source_text(file_id);
        let path_str = file_id_to_path(project, file_id).unwrap_or_default();
        (symbols, content, path_str)
    });

    let symbol_table = symbols.as_ref();
    build_symbol_tree(symbol_table, &content, &path_str, kind_filter, scope_filter)
}

fn collect_all_symbols(
    project_state: &ProjectState,
    kind_filter: Option<&str>,
    scope_filter: Option<&str>,
) -> Result<Vec<SymbolOut>, RpcError> {
    let mut all_symbols = Vec::new();
    let file_ids = project_state.with_project(|p| {
        let db = p.database();
        db.file_ids()
    });

    for file_id in file_ids {
        let file_symbols = collect_file_symbols(project_state, file_id, kind_filter, scope_filter)?;
        all_symbols.extend(file_symbols);
    }

    Ok(all_symbols)
}

fn build_symbol_tree(
    symbols: &SymbolTable,
    content: &str,
    path_str: &str,
    kind_filter: Option<&str>,
    scope_filter: Option<&str>,
) -> Result<Vec<SymbolOut>, RpcError> {
    // Skip built-in symbols that have empty ranges.
    let mut candidates: Vec<_> = symbols.iter().filter(|s| !s.range.is_empty()).collect();
    candidates.sort_by_key(|s| s.range.start());

    // If scope filter, narrow to scope symbol and its descendants.
    if let Some(scope_name) = scope_filter {
        let scope_ids: Vec<_> = candidates
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(scope_name))
            .map(|s| s.id)
            .collect();

        let mut allowed = HashSet::new();
        for scope_id in &scope_ids {
            allowed.insert(*scope_id);
            collect_descendants(symbols, *scope_id, &mut allowed);
        }

        if allowed.is_empty() {
            return Ok(Vec::new());
        }
        candidates.retain(|s| allowed.contains(&s.id));
    }

    // Apply kind filter.
    if let Some(filter) = kind_filter {
        candidates.retain(|s| symbol_kind_to_string(&s.kind).eq_ignore_ascii_case(filter));
    }

    let candidate_ids: HashSet<SymbolId> = candidates.iter().map(|s| s.id).collect();

    // Build parent -> children map, but only for parents inside the candidate set.
    let mut children_map: HashMap<Option<SymbolId>, Vec<SymbolId>> = HashMap::new();
    for &id in &candidate_ids {
        if let Some(sym) = symbols.get(id) {
            let parent = sym
                .parent
                .filter(|pid| candidate_ids.contains(pid))
                .or(None);
            children_map.entry(parent).or_default().push(id);
        }
    }

    // Sort children by source position.
    for children in children_map.values_mut() {
        children.sort_by_key(|id| symbols.get(*id).map(|s| s.range.start()));
    }

    let root_ids = children_map.get(&None).cloned().unwrap_or_default();
    let tree: Vec<SymbolOut> = root_ids
        .iter()
        .map(|id| build_symbol_recursive(*id, symbols, content, path_str, &children_map))
        .collect();

    Ok(tree)
}

fn collect_descendants(symbols: &SymbolTable, parent_id: SymbolId, out: &mut HashSet<SymbolId>) {
    for sym in symbols.iter() {
        if sym.parent == Some(parent_id) {
            out.insert(sym.id);
            collect_descendants(symbols, sym.id, out);
        }
    }
}

fn build_symbol_recursive(
    id: SymbolId,
    symbols: &SymbolTable,
    content: &str,
    path_str: &str,
    children_map: &HashMap<Option<SymbolId>, Vec<SymbolId>>,
) -> SymbolOut {
    let sym = symbols.get(id).expect("symbol must exist");
    let children: Vec<SymbolOut> = children_map
        .get(&Some(id))
        .map(|kids| {
            kids.iter()
                .map(|cid| build_symbol_recursive(*cid, symbols, content, path_str, children_map))
                .collect()
        })
        .unwrap_or_default();

    SymbolOut {
        id: format!("{}:{}", path_str, u32::from(sym.range.start())),
        name: sym.name.to_string(),
        kind: symbol_kind_to_string(&sym.kind).to_string(),
        file_path: path_str.to_string(),
        range: build_range(content, sym.range),
        children,
    }
}

fn collect_file_types(
    project_state: &ProjectState,
    file_id: FileId,
    name_filter: Option<&str>,
) -> Result<Vec<TypeOut>, RpcError> {
    let (symbols, path_str) = project_state.with_project(|project| {
        let db = project.database();
        let symbols = db.file_symbols(file_id);
        let path_str = file_id_to_path(project, file_id).unwrap_or_default();
        (symbols, path_str)
    });

    let mut types = Vec::new();
    for symbol in symbols.iter() {
        if !matches!(symbol.kind, SymbolKind::Type) {
            continue;
        }
        if let Some(filter) = name_filter {
            if !symbol.name.eq_ignore_ascii_case(filter) {
                continue;
            }
        }

        let type_info = symbols.type_by_id(symbol.type_id);
        let (kind_str, fields) = match type_info {
            Some(Type::Struct { fields, .. }) => {
                let field_outs: Vec<TypeFieldOut> = fields
                    .iter()
                    .map(|f| TypeFieldOut {
                        name: f.name.to_string(),
                        type_name: symbols.type_name(f.type_id).unwrap_or_default().to_string(),
                    })
                    .collect();
                ("struct", Some(field_outs))
            }
            Some(Type::Enum { values, .. }) => {
                let field_outs: Vec<TypeFieldOut> = values
                    .iter()
                    .map(|(n, _)| TypeFieldOut {
                        name: n.to_string(),
                        type_name: "INT".to_string(),
                    })
                    .collect();
                ("enum", Some(field_outs))
            }
            Some(Type::Union { .. }) => ("union", None),
            Some(Type::Alias { .. }) => ("alias", None),
            Some(Type::Array { .. }) => ("array", None),
            Some(Type::Pointer { .. }) => ("pointer", None),
            _ => ("type", None),
        };

        types.push(TypeOut {
            name: symbol.name.to_string(),
            kind: kind_str.to_string(),
            file_path: path_str.clone(),
            fields,
        });
    }

    Ok(types)
}

fn collect_all_types(
    project_state: &ProjectState,
    name_filter: Option<&str>,
) -> Result<Vec<TypeOut>, RpcError> {
    let mut all_types = Vec::new();
    let file_ids = project_state.with_project(|p| {
        let db = p.database();
        db.file_ids()
    });

    for file_id in file_ids {
        let file_types = collect_file_types(project_state, file_id, name_filter)?;
        all_types.extend(file_types);
    }

    Ok(all_types)
}

fn find_symbol_references(
    project_state: &ProjectState,
    symbol_id: &str,
    include_declaration: bool,
) -> Result<Vec<ReferenceOut>, RpcError> {
    let parts: Vec<_> = symbol_id.split('.').collect();

    let file_ids = project_state.with_project(|p| {
        let db = p.database();
        db.file_ids()
    });

    // Find the symbol in any file (using project-augmented symbol tables).
    let mut target_file_id = None;
    let mut target_position = None;

    for file_id in file_ids {
        let found = project_state.with_project(|project| {
            let db = project.database();
            let symbols = db.file_symbols_with_project(file_id);
            let resolved = if parts.len() > 1 {
                let smol_parts: Vec<SmolStr> = parts.iter().map(|&p| SmolStr::new(p)).collect();
                symbols.resolve_qualified(&smol_parts)
            } else {
                symbols.lookup(parts[0])
            };

            resolved.and_then(|sid| {
                let sym = symbols.get(sid)?;
                let actual_file_id = sym.origin.map(|o| o.file_id).unwrap_or(file_id);
                Some((actual_file_id, sym.range.start()))
            })
        });

        if let Some((fid, pos)) = found {
            target_file_id = Some(fid);
            target_position = Some(pos);
            break;
        }
    }

    let (file_id, position) = match (target_file_id, target_position) {
        (Some(fid), Some(pos)) => (fid, pos),
        _ => {
            return Err(RpcError::new(
                NOT_INITIALIZED,
                format!("Symbol not found: {symbol_id}"),
            ))
        }
    };

    let options = FindReferencesOptions {
        include_declaration,
    };

    let references = project_state.with_project(|project| {
        let db = project.database();
        find_references(db, file_id, position, options)
    });

    let mut result = Vec::new();
    for reference in references {
        let ref_path = project_state.with_project(|project| {
            file_id_to_path(project, reference.file_id).unwrap_or_default()
        });
        let content = project_state.with_project(|project| {
            let db = project.database();
            db.source_text(reference.file_id)
        });

        result.push(ReferenceOut {
            file_path: ref_path,
            range: build_range(&content, reference.range),
        });
    }

    Ok(result)
}

fn collect_file_diagnostics(
    project_state: &ProjectState,
    file_id: FileId,
) -> Result<Vec<DiagnosticOut>, RpcError> {
    let (diagnostics, content, path_str) = project_state.with_project(|project| {
        let db = project.database();
        let diagnostics = trust_ide::diagnostics::collect_diagnostics(db, file_id);
        let content = db.source_text(file_id);
        let path_str = file_id_to_path(project, file_id).unwrap_or_default();
        (diagnostics, content, path_str)
    });

    let mut result = Vec::new();
    for diag in diagnostics {
        let (line, col) = offset_to_line_col(&content, diag.range.start().into());
        result.push(DiagnosticOut {
            path: path_str.clone(),
            severity: match diag.severity {
                trust_hir::DiagnosticSeverity::Error => "error",
                trust_hir::DiagnosticSeverity::Warning => "warning",
                trust_hir::DiagnosticSeverity::Info => "info",
                trust_hir::DiagnosticSeverity::Hint => "hint",
            }
            .to_string(),
            line,
            col,
            message: diag.message.clone(),
            code: Some(diag.code.code().to_string()),
        });
    }

    Ok(result)
}

fn collect_all_diagnostics(project_state: &ProjectState) -> Result<Vec<DiagnosticOut>, RpcError> {
    let mut all = Vec::new();
    let file_ids = project_state.with_project(|p| {
        let db = p.database();
        db.file_ids()
    });

    for file_id in file_ids {
        let file_diagnostics = collect_file_diagnostics(project_state, file_id)?;
        all.extend(file_diagnostics);
    }

    Ok(all)
}

fn build_snapshot(project_state: &ProjectState) -> Result<SnapshotOut, RpcError> {
    let symbols = collect_all_symbols(project_state, None, None)?;
    let types = collect_all_types(project_state, None)?;
    let diagnostics = collect_all_diagnostics(project_state)?;

    let entity_count = symbols.len() + types.len();
    let edge_count = 0; // Deep edge analysis is future work.

    let export_time = {
        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}Z", since_epoch.as_secs())
    };

    let metadata = SnapshotMetadata {
        project_path: project_state.project_root().to_string_lossy().to_string(),
        export_time,
        entity_count,
        edge_count,
    };

    Ok(SnapshotOut {
        metadata,
        symbols,
        types,
        diagnostics,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_escape_detected() {
        let root =
            std::env::temp_dir().join(format!("trust-hir-cli-test-escape-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let result = validate_path("../../etc/passwd", &root);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, PATH_ESCAPE);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn normalized_path_within_workspace_passes() {
        let root =
            std::env::temp_dir().join(format!("trust-hir-cli-test-valid-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        let result = validate_path("src/main.st", &root);
        assert!(result.is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absolute_path_within_workspace_passes() {
        let root =
            std::env::temp_dir().join(format!("trust-hir-cli-test-abs-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        let abs = root.join("src/main.st");
        let result = validate_path(abs.to_str().unwrap(), &root);
        assert!(result.is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn null_bytes_in_path_rejected() {
        let root =
            std::env::temp_dir().join(format!("trust-hir-cli-test-null-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let result = validate_path("foo\0bar", &root);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, INVALID_PARAMS);
        fs::remove_dir_all(&root).ok();
    }
}
