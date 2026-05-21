//! LSP workspace/executeCommand handlers.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CreateFile, CreateFileOptions, DeleteFile, DeleteFileOptions, DocumentChangeOperation,
    DocumentChanges, ExecuteCommandParams, OptionalVersionedTextDocumentIdentifier, Position,
    Range, ResourceOp, TextDocumentEdit, TextDocumentIdentifier, TextEdit, Url, WorkspaceEdit,
};
use tower_lsp::Client;

use text_size::{TextRange, TextSize};
use trust_ide::refactor::parse_namespace_path;
use trust_ide::rename::{RenameResult, TextEdit as IdeTextEdit};
use trust_syntax::parser::parse;
use trust_syntax::syntax::{SyntaxKind, SyntaxNode};

use crate::handlers::context::ServerContext;
use crate::handlers::lsp_utils::{offset_to_position, position_to_offset};
use crate::library_graph::build_library_graph;
use crate::state::{path_to_uri, uri_to_path, ServerState};

pub const MOVE_NAMESPACE_COMMAND: &str = "trust-lsp.moveNamespace";
pub const PROJECT_INFO_COMMAND: &str = "trust-lsp.projectInfo";
pub const HMI_INIT_COMMAND: &str = "trust-lsp.hmiInit";
pub const HMI_BINDINGS_COMMAND: &str = "trust-lsp.hmiBindings";

#[derive(Debug, Deserialize)]
pub struct MoveNamespaceCommandArgs {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub new_path: String,
    #[serde(default)]
    pub target_uri: Option<Url>,
}

#[derive(Debug, Deserialize)]
struct ProjectInfoCommandArgs {
    #[serde(default)]
    root_uri: Option<Url>,
    #[serde(default)]
    text_document: Option<TextDocumentIdentifier>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct HmiInitCommandArgs {
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    root_uri: Option<Url>,
    #[serde(default)]
    text_document: Option<TextDocumentIdentifier>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct HmiBindingsCommandArgs {
    #[serde(default)]
    root_uri: Option<Url>,
    #[serde(default)]
    text_document: Option<TextDocumentIdentifier>,
}

#[allow(deprecated)]
pub async fn execute_command(
    client: &Client,
    state: &ServerState,
    params: ExecuteCommandParams,
) -> Option<Value> {
    match params.command.as_str() {
        MOVE_NAMESPACE_COMMAND => {
            let args = parse_move_namespace_args(params.arguments)?;
            let edit = namespace_move_workspace_edit(state, args)?;
            let response = client.apply_edit(edit).await.ok()?;
            Some(json!(response.applied))
        }
        PROJECT_INFO_COMMAND => project_info_value(state, params.arguments),
        HMI_INIT_COMMAND => hmi_init_value(state, params.arguments),
        HMI_BINDINGS_COMMAND => hmi_bindings_value(state, params.arguments),
        _ => None,
    }
}

fn parse_move_namespace_args(args: Vec<Value>) -> Option<MoveNamespaceCommandArgs> {
    if args.len() != 1 {
        return None;
    }
    serde_json::from_value(args.into_iter().next()?).ok()
}

pub(crate) fn project_info_value(state: &ServerState, args: Vec<Value>) -> Option<Value> {
    project_info_value_with_context(state, args)
}

fn project_info_value_with_context<C: ServerContext>(
    context: &C,
    args: Vec<Value>,
) -> Option<Value> {
    let mut configs = context.workspace_configs();
    if args.len() == 1 {
        if let Ok(parsed) = serde_json::from_value::<ProjectInfoCommandArgs>(
            args.into_iter().next().unwrap_or(Value::Null),
        ) {
            if let Some(root_uri) = parsed.root_uri {
                configs.retain(|(root, _)| root == &root_uri);
            } else if let Some(text_document) = parsed.text_document {
                if let Some(config) = context.workspace_config_for_uri(&text_document.uri) {
                    let root_uri = path_to_uri(&config.root).unwrap_or(text_document.uri.clone());
                    configs = vec![(root_uri, config)];
                }
            }
        }
    }

    let projects: Vec<Value> = configs
        .into_iter()
        .map(|(root, config)| project_info_for_config(&root, &config))
        .collect();

    Some(json!({ "projects": projects }))
}

fn project_info_for_config(root: &Url, config: &crate::config::ProjectConfig) -> Value {
    let graph = build_library_graph(config);
    let libraries: Vec<Value> = graph
        .nodes
        .into_iter()
        .map(|node| {
            let dependencies: Vec<Value> = node
                .dependencies
                .into_iter()
                .map(|dep| {
                    json!({
                        "name": dep.name,
                        "version": dep.version,
                    })
                })
                .collect();
            json!({
                "name": node.name,
                "version": node.version,
                "path": node.path.display().to_string(),
                "dependencies": dependencies,
            })
        })
        .collect();

    let targets: Vec<Value> = config
        .targets
        .iter()
        .map(|target| {
            json!({
                "name": target.name,
                "profile": target.profile,
                "flags": target.flags,
                "defines": target.defines,
            })
        })
        .collect();

    json!({
        "root": root.to_string(),
        "configPath": config.config_path.as_ref().map(|path| path.display().to_string()),
        "build": {
            "target": config.build.target,
            "profile": config.build.profile,
            "flags": config.build.flags,
            "defines": config.build.defines,
        },
        "targets": targets,
        "libraries": libraries,
    })
}

#[deprecated(note = "HMI commands moved to trust-hmi-gen. Will be removed in v2.1.")]
pub(crate) fn hmi_init_value(_state: &ServerState, _args: Vec<Value>) -> Option<Value> {
    Some(json!({
        "ok": false,
        "error": "DEPRECATED: HMI commands moved to trust-hmi-gen. Will be removed in v2.1."
    }))
}

#[deprecated(note = "HMI commands moved to trust-hmi-gen. Will be removed in v2.1.")]
pub(crate) fn hmi_bindings_value(_state: &ServerState, _args: Vec<Value>) -> Option<Value> {
    Some(json!({
        "ok": false,
        "error": "DEPRECATED: HMI commands moved to trust-hmi-gen. Will be removed in v2.1."
    }))
}

#[deprecated(note = "HMI commands moved to trust-hmi-gen. Will be removed in v2.1.")]
#[allow(dead_code)]
fn hmi_init_value_with_context<C: ServerContext>(_context: &C, _args: Vec<Value>) -> Option<Value> {
    Some(json!({
        "ok": false,
        "error": "DEPRECATED: HMI commands moved to trust-hmi-gen. Will be removed in v2.1."
    }))
}

#[deprecated(note = "HMI commands moved to trust-hmi-gen. Will be removed in v2.1.")]
#[allow(dead_code)]
fn hmi_bindings_value_with_context<C: ServerContext>(
    _context: &C,
    _args: Vec<Value>,
) -> Option<Value> {
    Some(json!({
        "ok": false,
        "error": "DEPRECATED: HMI commands moved to trust-hmi-gen. Will be removed in v2.1."
    }))
}

pub(crate) fn namespace_move_workspace_edit(
    state: &ServerState,
    args: MoveNamespaceCommandArgs,
) -> Option<WorkspaceEdit> {
    namespace_move_workspace_edit_with_context(state, args)
}

include!("commands/hmi_namespace_and_edits.rs");
include!("commands/path_ranges_and_tests.rs");
