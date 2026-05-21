use super::*;
use rustc_hash::FxHashMap;
use trust_hir::symbols::{Symbol, VarQualifier};
use trust_hir::Type;

pub fn document_symbol(
    state: &ServerState,
    params: DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let doc = state.get_document(&params.text_document.uri)?;
    let symbols = state.with_database(|db| db.file_symbols(doc.file_id));

    let hir_symbols: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| is_outline_symbol_kind(&s.kind) && !s.range.is_empty())
        .collect();

    // parent → children map
    let mut children_map: FxHashMap<Option<trust_hir::symbols::SymbolId>, Vec<&Symbol>> =
        FxHashMap::default();
    for sym in &hir_symbols {
        children_map.entry(sym.parent).or_default().push(sym);
    }
    for children in children_map.values_mut() {
        children.sort_by_key(|s| s.range.start());
    }

    let roots = children_map.get(&None).cloned().unwrap_or_default();
    let tree: Vec<DocumentSymbol> = roots
        .iter()
        .map(|s| build_document_symbol(s, &children_map, &doc, &symbols))
        .collect();

    Some(DocumentSymbolResponse::Nested(tree))
}

fn symbol_detail_string(symbols: &SymbolTable, symbol: &Symbol) -> Option<String> {
    match &symbol.kind {
        HirSymbolKind::Variable { qualifier } => {
            let type_name = symbols.type_name(symbol.type_id).unwrap_or_default();
            let qual = qualifier_label(qualifier);
            let pers = symbol.persistence.as_deref();
            let edge = symbol.edge.as_deref();
            let parts: Vec<&str> = [Some(qual), pers, edge].iter().filter_map(|s| *s).collect();
            let prefix = parts.join(" ");
            if type_name.is_empty() {
                if prefix.is_empty() {
                    None
                } else {
                    Some(prefix)
                }
            } else {
                Some(format!("{} : {}", prefix, type_name))
            }
        }
        HirSymbolKind::Parameter { direction } => {
            let type_name = symbols.type_name(symbol.type_id).unwrap_or_default();
            let qual = match direction {
                ParamDirection::In => "VAR_INPUT",
                ParamDirection::Out => "VAR_OUTPUT",
                ParamDirection::InOut => "VAR_IN_OUT",
            };
            let pers = symbol.persistence.as_deref();
            let edge = symbol.edge.as_deref();
            let parts: Vec<&str> = [Some(qual), pers, edge].iter().filter_map(|s| *s).collect();
            let prefix = parts.join(" ");
            if type_name.is_empty() {
                if prefix.is_empty() {
                    None
                } else {
                    Some(prefix)
                }
            } else {
                Some(format!("{} : {}", prefix, type_name))
            }
        }
        HirSymbolKind::Field { field_type } => {
            let type_name = symbols.type_name(*field_type).unwrap_or_default();
            if type_name.is_empty() {
                None
            } else {
                Some(format!(": {}", type_name))
            }
        }
        HirSymbolKind::Action => Some("ACTION".to_string()),
        HirSymbolKind::Function { .. } => {
            let tn = symbols.type_name(symbol.type_id).unwrap_or_default();
            if tn.is_empty() {
                None
            } else {
                Some(format!(": {}", tn))
            }
        }
        HirSymbolKind::Method { .. } => {
            let tn = symbols.type_name(symbol.type_id).unwrap_or_default();
            if tn.is_empty() {
                None
            } else {
                Some(format!(": {}", tn))
            }
        }
        HirSymbolKind::Property { .. } => {
            let tn = symbols.type_name(symbol.type_id).unwrap_or_default();
            if tn.is_empty() {
                None
            } else {
                Some(format!(": {}", tn))
            }
        }
        _ => None,
    }
}

fn qualifier_label(q: &VarQualifier) -> &'static str {
    match q {
        VarQualifier::Input => "VAR_INPUT",
        VarQualifier::Output => "VAR_OUTPUT",
        VarQualifier::InOut => "VAR_IN_OUT",
        VarQualifier::Global => "VAR_GLOBAL",
        VarQualifier::Temp => "VAR_TEMP",
        VarQualifier::Static => "VAR_STAT",
        VarQualifier::External => "VAR_EXTERNAL",
        VarQualifier::Access => "VAR_ACCESS",
        VarQualifier::Local => "VAR",
    }
}

fn build_document_symbol(
    sym: &Symbol,
    children_map: &FxHashMap<Option<trust_hir::symbols::SymbolId>, Vec<&Symbol>>,
    doc: &crate::state::Document,
    symbols: &trust_hir::symbols::SymbolTable,
) -> DocumentSymbol {
    let mut children: Option<Vec<DocumentSymbol>> = children_map.get(&Some(sym.id)).map(|kids| {
        kids.iter()
            .map(|c| build_document_symbol(c, children_map, doc, symbols))
            .collect()
    });

    // If Variable is of Enum type, add EnumValue as synthetic children.
    if let HirSymbolKind::Variable { .. } = &sym.kind {
        if let Some(enum_children) = enum_value_children(symbols, sym.type_id, doc) {
            children.get_or_insert_with(Vec::new).extend(enum_children);
        }
    }

    let name = sym.name.to_string();
    let kind = lsp_symbol_kind(symbols, sym);
    let detail = symbol_detail_string(symbols, sym);

    let selection_range = Range {
        start: offset_to_position(&doc.content, sym.range.start().into()),
        end: offset_to_position(&doc.content, sym.range.end().into()),
    };

    let full_range = selection_range;

    #[allow(deprecated)]
    DocumentSymbol {
        name,
        detail,
        kind,
        range: full_range,
        selection_range,
        children,
        deprecated: None,
        tags: None,
    }
}

/// Creates synthetic DocumentSymbol children from EnumValue for a Variable of Enum type.
fn enum_value_children(
    symbols: &trust_hir::symbols::SymbolTable,
    type_id: trust_hir::TypeId,
    _doc: &crate::state::Document,
) -> Option<Vec<DocumentSymbol>> {
    let ty = symbols.type_by_id(type_id)?;

    let values = match ty {
        Type::Enum { values, .. } => values,
        _ => return None,
    };

    let children: Vec<DocumentSymbol> = values
        .iter()
        .map(|(name, _value)| {
            #[allow(deprecated)]
            DocumentSymbol {
                name: name.to_string(),
                kind: SymbolKind::ENUM_MEMBER,
                detail: None,
                range: Range {
                    start: Position::default(),
                    end: Position::default(),
                },
                selection_range: Range {
                    start: Position::default(),
                    end: Position::default(),
                },
                children: None,
                deprecated: None,
                tags: None,
            }
        })
        .collect();

    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

fn is_outline_symbol_kind(kind: &HirSymbolKind) -> bool {
    matches!(
        kind,
        HirSymbolKind::Program
            | HirSymbolKind::Configuration
            | HirSymbolKind::Resource
            | HirSymbolKind::Task
            | HirSymbolKind::ProgramInstance
            | HirSymbolKind::Namespace
            | HirSymbolKind::Function { .. }
            | HirSymbolKind::FunctionBlock
            | HirSymbolKind::Class
            | HirSymbolKind::Interface
            | HirSymbolKind::Type
            | HirSymbolKind::EnumValue { .. }
            | HirSymbolKind::Method { .. }
            | HirSymbolKind::Property { .. }
            | HirSymbolKind::Action
            | HirSymbolKind::Field { .. }
            | HirSymbolKind::Variable { .. }
            | HirSymbolKind::Parameter { .. }
            | HirSymbolKind::Constant
    )
}

pub fn workspace_symbol(
    state: &ServerState,
    params: WorkspaceSymbolParams,
) -> Option<Vec<SymbolInformation>> {
    let request_ticket = state.begin_semantic_request();
    workspace_symbol_with_ticket(state, params, request_ticket)
}

fn workspace_symbol_with_ticket(
    state: &ServerState,
    params: WorkspaceSymbolParams,
    request_ticket: u64,
) -> Option<Vec<SymbolInformation>> {
    if state.semantic_request_cancelled(request_ticket) {
        return None;
    }

    let query = params.query.trim().to_lowercase();
    let query_empty = query.is_empty();

    let file_ids = state.with_database(|db| db.file_ids());
    let mut result = Vec::new();

    for file_id in file_ids {
        if state.semantic_request_cancelled(request_ticket) {
            return None;
        }

        let doc = match state.document_for_file_id(file_id) {
            Some(doc) => doc,
            None => continue,
        };

        let config = state.workspace_config_for_uri(&doc.uri);
        let (priority, visibility) = config
            .map(|config| (config.workspace.priority, config.workspace.visibility))
            .unwrap_or((0, WorkspaceVisibility::default()));
        if !visibility.allows_query(query_empty) {
            continue;
        }

        let symbols = state.with_database(|db| db.file_symbols(file_id));
        for symbol in symbols.iter() {
            if state.semantic_request_cancelled(request_ticket) {
                return None;
            }

            let name = display_symbol_name(&symbols, symbol);
            if !query_empty && !name.to_lowercase().contains(&query) {
                continue;
            }

            let kind = lsp_symbol_kind(&symbols, symbol);
            let range = Range {
                start: offset_to_position(&doc.content, symbol.range.start().into()),
                end: offset_to_position(&doc.content, symbol.range.end().into()),
            };
            let container_name = symbol_container_name(&symbols, symbol);

            #[allow(deprecated)]
            result.push((
                priority,
                SymbolInformation {
                    name,
                    kind,
                    location: Location {
                        uri: doc.uri.clone(),
                        range,
                    },
                    container_name,
                    tags: None,
                    deprecated: None,
                },
            ));
        }
    }

    result.sort_by(|(prio_a, sym_a), (prio_b, sym_b)| {
        prio_b
            .cmp(prio_a)
            .then_with(|| sym_a.name.cmp(&sym_b.name))
            .then_with(|| sym_a.location.uri.as_str().cmp(sym_b.location.uri.as_str()))
            // Keep snapshot order deterministic when multiple symbols share name+uri.
            .then_with(|| sym_a.container_name.cmp(&sym_b.container_name))
            .then_with(|| {
                sym_a
                    .location
                    .range
                    .start
                    .line
                    .cmp(&sym_b.location.range.start.line)
            })
            .then_with(|| {
                sym_a
                    .location
                    .range
                    .start
                    .character
                    .cmp(&sym_b.location.range.start.character)
            })
            .then_with(|| {
                sym_a
                    .location
                    .range
                    .end
                    .line
                    .cmp(&sym_b.location.range.end.line)
            })
            .then_with(|| {
                sym_a
                    .location
                    .range
                    .end
                    .character
                    .cmp(&sym_b.location.range.end.character)
            })
    });
    let result = result.into_iter().map(|(_, symbol)| symbol).collect();
    Some(result)
}

pub async fn workspace_symbol_with_progress(
    client: &Client,
    state: &ServerState,
    params: WorkspaceSymbolParams,
) -> Option<Vec<SymbolInformation>> {
    let work_done_token = params.work_done_progress_params.work_done_token.clone();
    let partial_token = params.partial_result_params.partial_result_token.clone();
    let message = if params.query.trim().is_empty() {
        None
    } else {
        Some(format!("Query: {}", params.query))
    };
    send_work_done_begin(
        client,
        &work_done_token,
        "Searching workspace symbols",
        message,
    )
    .await;

    let result = workspace_symbol(state, params);

    if let Some(symbols) = result.as_ref() {
        if partial_token.is_some() {
            let total = symbols.len().max(1);
            let mut emitted = 0usize;
            for chunk in symbols.chunks(PARTIAL_CHUNK_SIZE) {
                send_partial_result(client, &partial_token, chunk.to_vec()).await;
                emitted = emitted.saturating_add(chunk.len());
                let percentage = ((emitted as f64 / total as f64) * 100.0).round() as u32;
                send_work_done_report(
                    client,
                    &work_done_token,
                    Some(format!("Symbols: {emitted}/{total}")),
                    Some(percentage.min(100)),
                )
                .await;
            }
        }
    }

    let count = result.as_ref().map(|items| items.len()).unwrap_or(0);
    send_work_done_end(
        client,
        &work_done_token,
        Some(format!("Found {count} symbol(s)")),
    )
    .await;
    result
}
