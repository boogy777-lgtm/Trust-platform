fn is_config_uri(uri: &Url) -> bool {
    let Some(path) = uri_to_path(uri) else {
        return false;
    };
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| CONFIG_FILES.iter().any(|candidate| candidate == &name))
        .unwrap_or(false)
}

fn is_hmi_toml_uri(uri: &Url) -> bool {
    let Some(path) = uri_to_path(uri) else {
        return false;
    };
    if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
        return false;
    }
    path.components()
        .any(|component| component.as_os_str() == "hmi")
}

fn collect_hmi_toml_diagnostics(state: &ServerState, uri: &Url, content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = collect_hmi_toml_parse_diagnostics(content);
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let Some(path) = uri_to_path(uri) else {
        return diagnostics;
    };

    let root = state
        .workspace_config_for_uri(uri)
        .map(|config| config.root)
        .or_else(|| infer_hmi_root_from_path(path.as_path()));
    let Some(root) = root else {
        return diagnostics;
    };

    diagnostics.extend(collect_hmi_toml_semantic_diagnostics(
        root.as_path(),
        path.as_path(),
        content,
    ));
    diagnostics
}

fn collect_hmi_toml_parse_diagnostics(content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if let Err(error) = toml::from_str::<toml::Value>(content) {
        let range = if let Some(span) = error.span() {
            Range {
                start: offset_to_position(content, span.start as u32),
                end: offset_to_position(content, span.end as u32),
            }
        } else {
            fallback_range(content)
        };
        diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("HMI_TOML_PARSE".to_string())),
            source: Some("trust-lsp".to_string()),
            message: error.to_string(),
            ..Default::default()
        });
    }
    diagnostics
}

fn infer_hmi_root_from_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    if parent.file_name().and_then(|name| name.to_str()) != Some("hmi") {
        return None;
    }
    parent.parent().map(Path::to_path_buf)
}

fn collect_hmi_toml_semantic_diagnostics(
    _root: &Path,
    _current_file: &Path,
    _content: &str,
) -> Vec<Diagnostic> {
    // HMI semantic diagnostics are deprecated in trust-lsp.
    // Use trust-hmi-gen for HMI validation.
    Vec::new()
}

