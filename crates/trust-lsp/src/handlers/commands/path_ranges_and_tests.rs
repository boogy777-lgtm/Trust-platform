#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        BuildConfig, DiagnosticSettings, IndexingConfig, ProjectConfig, RuntimeConfig,
        StdlibSettings, TelemetryConfig, WorkspaceSettings,
    };
    use crate::state::Document;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use trust_hir::db::FileId;

    #[derive(Clone, Default)]
    struct MockContext {
        workspace_configs: Vec<(Url, ProjectConfig)>,
        workspace_config_by_uri: HashMap<Url, ProjectConfig>,
        workspace_folders: Vec<Url>,
        documents_by_uri: HashMap<Url, Document>,
        documents_by_file_id: HashMap<FileId, Document>,
        rename_result: Option<RenameResult>,
    }

    impl MockContext {
        fn insert_document(&mut self, document: Document) {
            self.documents_by_file_id
                .insert(document.file_id, document.clone());
            self.documents_by_uri.insert(document.uri.clone(), document);
        }
    }

    impl ServerContext for MockContext {
        fn workspace_configs(&self) -> Vec<(Url, ProjectConfig)> {
            self.workspace_configs.clone()
        }

        fn workspace_config_for_uri(&self, uri: &Url) -> Option<ProjectConfig> {
            self.workspace_config_by_uri.get(uri).cloned()
        }

        fn workspace_folders(&self) -> Vec<Url> {
            self.workspace_folders.clone()
        }

        fn get_document(&self, uri: &Url) -> Option<Document> {
            self.documents_by_uri.get(uri).cloned()
        }

        fn document_for_file_id(&self, file_id: FileId) -> Option<Document> {
            self.documents_by_file_id.get(&file_id).cloned()
        }

        fn rename(
            &self,
            _file_id: FileId,
            _offset: TextSize,
            _new_name: &str,
        ) -> Option<RenameResult> {
            self.rename_result.clone()
        }
    }

    fn test_project_config(root: &str, target: &str) -> ProjectConfig {
        ProjectConfig {
            root: PathBuf::from(root),
            config_path: None,
            include_paths: Vec::new(),
            vendor_profile: None,
            stdlib: StdlibSettings::default(),
            libraries: Vec::new(),
            dependencies: Vec::new(),
            dependency_resolution_issues: Vec::new(),
            diagnostic_external_paths: Vec::new(),
            build: BuildConfig {
                target: Some(target.to_string()),
                ..BuildConfig::default()
            },
            targets: Vec::new(),
            indexing: IndexingConfig::default(),
            diagnostics: DiagnosticSettings::default(),
            runtime: RuntimeConfig::default(),
            workspace: WorkspaceSettings::default(),
            telemetry: TelemetryConfig::default(),
        }
    }

    fn test_document(uri: &str, file_id: u32, content: &str) -> Document {
        Document::new(
            Url::parse(uri).expect("test uri"),
            1,
            content.to_string(),
            FileId(file_id),
            true,
            1,
        )
    }

    #[test]
    fn project_info_with_mock_context_uses_uri_mapping() {
        let root_a = Url::parse("file:///workspace/a/").expect("root a");
        let root_b = Url::parse("file:///workspace/b/").expect("root b");
        let config_a = test_project_config("/workspace/a", "x86_64");
        let config_b = test_project_config("/workspace/b", "armv7");
        let main_uri = Url::parse("file:///workspace/a/src/main.st").expect("main uri");

        let mut context = MockContext {
            workspace_configs: vec![
                (root_a.clone(), config_a.clone()),
                (root_b.clone(), config_b.clone()),
            ],
            ..MockContext::default()
        };
        context
            .workspace_config_by_uri
            .insert(main_uri.clone(), config_a);

        let info = project_info_value_with_context(
            &context,
            vec![json!({
                "text_document": {
                    "uri": main_uri,
                }
            })],
        )
        .expect("project info");
        let projects = info
            .get("projects")
            .and_then(Value::as_array)
            .expect("projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(
            projects[0]
                .get("build")
                .and_then(|build| build.get("target"))
                .and_then(Value::as_str),
            Some("x86_64")
        );
    }

    #[test]
    fn namespace_move_with_mock_context_generates_expected_operations() {
        let source = r#"
NAMESPACE LibA
TYPE Foo : INT;
END_TYPE
END_NAMESPACE
"#;
        let main = r#"
PROGRAM Main
VAR
    x : LibA.Foo;
END_VAR
END_PROGRAM
"#;
        let source_uri = Url::parse("file:///workspace/liba.st").expect("source uri");
        let main_uri = Url::parse("file:///workspace/main.st").expect("main uri");
        let target_uri = Url::parse("file:///workspace/Company/LibA.st").expect("target uri");

        let source_doc = test_document(source_uri.as_str(), 1, source);
        let main_doc = test_document(main_uri.as_str(), 2, main);

        let mut rename_result = RenameResult::new();
        let ns_start = source.find("LibA").expect("namespace name start");
        rename_result.add_edit(
            source_doc.file_id,
            IdeTextEdit {
                range: TextRange::new(
                    TextSize::from(ns_start as u32),
                    TextSize::from((ns_start + "LibA".len()) as u32),
                ),
                new_text: "Company.LibA".to_string(),
            },
        );
        let main_ref_start = main.find("LibA").expect("main namespace reference");
        rename_result.add_edit(
            main_doc.file_id,
            IdeTextEdit {
                range: TextRange::new(
                    TextSize::from(main_ref_start as u32),
                    TextSize::from((main_ref_start + "LibA".len()) as u32),
                ),
                new_text: "Company.LibA".to_string(),
            },
        );

        let mut context = MockContext {
            workspace_folders: vec![Url::parse("file:///workspace/").expect("root uri")],
            rename_result: Some(rename_result),
            ..MockContext::default()
        };
        context.insert_document(source_doc);
        context.insert_document(main_doc);

        let args = MoveNamespaceCommandArgs {
            text_document: TextDocumentIdentifier {
                uri: source_uri.clone(),
            },
            position: offset_to_position(source, source.find("LibA").expect("position") as u32),
            new_path: "Company.LibA".to_string(),
            target_uri: Some(target_uri.clone()),
        };
        let edit = namespace_move_workspace_edit_with_context(&context, args).expect("edit");
        let ops = match edit.document_changes.expect("document changes") {
            DocumentChanges::Operations(ops) => ops,
            DocumentChanges::Edits(_) => panic!("expected operation list"),
        };

        assert!(
            ops.iter().any(|op| matches!(
                op,
                DocumentChangeOperation::Op(ResourceOp::Create(create)) if create.uri == target_uri
            )),
            "expected target file create operation"
        );
        assert!(
            ops.iter().any(|op| matches!(
                op,
                DocumentChangeOperation::Op(ResourceOp::Delete(delete)) if delete.uri == source_uri
            )),
            "expected source file delete operation"
        );

        let target_edit = ops.iter().find_map(|op| match op {
            DocumentChangeOperation::Edit(edit) if edit.text_document.uri == target_uri => {
                Some(edit)
            }
            _ => None,
        });
        let target_edit = target_edit.expect("target edit");
        let target_contains_renamed_namespace = target_edit.edits.iter().any(|edit| match edit {
            tower_lsp::lsp_types::OneOf::Left(edit) => {
                edit.new_text.contains("NAMESPACE Company.LibA")
            }
            tower_lsp::lsp_types::OneOf::Right(_) => false,
        });
        assert!(
            target_contains_renamed_namespace,
            "target insertion should include renamed namespace"
        );

        let main_edit = ops.iter().find_map(|op| match op {
            DocumentChangeOperation::Edit(edit) if edit.text_document.uri == main_uri => Some(edit),
            _ => None,
        });
        let main_edit = main_edit.expect("main edit");
        let main_updated = main_edit.edits.iter().any(|edit| match edit {
            tower_lsp::lsp_types::OneOf::Left(edit) => edit.new_text.contains("Company.LibA"),
            tower_lsp::lsp_types::OneOf::Right(_) => false,
        });
        assert!(main_updated, "main file should include renamed namespace");
    }

    #[test]
    fn project_info_server_state_and_context_paths_match() {
        let state = ServerState::new();
        let root = Url::parse("file:///workspace/").expect("root");
        state.set_workspace_folders(vec![root.clone()]);
        state.set_workspace_config(root, test_project_config("/workspace", "x86_64"));

        let from_wrapper = project_info_value(&state, Vec::new()).expect("wrapper value");
        let from_context =
            project_info_value_with_context(&state, Vec::new()).expect("context value");
        assert_eq!(from_wrapper, from_context);
    }

    #[test]
    #[allow(deprecated)]
    fn hmi_init_command_returns_deprecation_notice() {
        let context = MockContext::default();
        let result = hmi_init_value_with_context(&context, vec![json!({ "style": "mint" })])
            .expect("hmi init response");
        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        let error = result.get("error").and_then(Value::as_str).unwrap_or("");
        assert!(
            error.contains("DEPRECATED"),
            "expected deprecation notice, got: {error}"
        );
    }

    #[test]
    #[allow(deprecated)]
    fn hmi_bindings_command_returns_deprecation_notice() {
        let context = MockContext::default();
        let result =
            hmi_bindings_value_with_context(&context, Vec::new()).expect("hmi bindings response");
        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        let error = result.get("error").and_then(Value::as_str).unwrap_or("");
        assert!(
            error.contains("DEPRECATED"),
            "expected deprecation notice, got: {error}"
        );
    }
}
