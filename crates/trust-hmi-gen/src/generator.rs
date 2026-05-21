//! Output file generation for HMI bindings.
//!
//! Writes JSON, TOML, and manifest files to the HMI output directory.

use std::path::Path;

use crate::hmi_bindings::HmiBindingsCatalog;
use crate::project::ProjectLoader;
use crate::OutputFormat;

/// Write the binding catalog to the output directory.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file
/// cannot be written.
pub fn write_bindings(
    catalog: &HmiBindingsCatalog,
    out_dir: &Path,
    format: OutputFormat,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;

    match format {
        OutputFormat::Json => {
            let path = out_dir.join("hmi-bindings.json");
            let json = serde_json::to_string_pretty(catalog)?;
            std::fs::write(&path, json)?;
        }
        OutputFormat::Toml => {
            let path = out_dir.join("hmi-bindings.toml");
            let toml = toml::to_string_pretty(catalog)?;
            std::fs::write(&path, toml)?;
        }
    }

    Ok(())
}

/// Write a combined HMI manifest describing the full generation result.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_manifest(
    loader: &ProjectLoader,
    catalog: &HmiBindingsCatalog,
    out_dir: &Path,
    style: &str,
) -> anyhow::Result<()> {
    let manifest = serde_json::json!({
        "generator": "trust-hmi-gen",
        "project_root": loader.project_root().to_string_lossy(),
        "style": style,
        "bindings": {
            "program_count": catalog.programs.len(),
            "global_count": catalog.globals.len(),
            "total_variables": catalog.programs.iter().map(|p| p.variables.len()).sum::<usize>()
                + catalog.globals.len(),
        },
        "files": [
            "_config.toml",
            "overview.toml",
            "trends.toml",
            "alarms.toml",
            "control.toml",
            "hmi-bindings.json"
        ]
    });

    let path = out_dir.join("hmi-manifest.json");
    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&path, json)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hmi_bindings::{HmiBindingsProgram, HmiBindingsVariable};

    fn make_test_catalog() -> HmiBindingsCatalog {
        HmiBindingsCatalog {
            programs: vec![HmiBindingsProgram {
                name: "Main".to_string(),
                file: None,
                variables: vec![HmiBindingsVariable {
                    name: "MotorSpeed".to_string(),
                    path: "Main.MotorSpeed".to_string(),
                    data_type: "REAL".to_string(),
                    qualifier: "VAR_INPUT".to_string(),
                    writable: true,
                    widget: "slider".to_string(),
                    inferred_interface: true,
                    unit: Some("rpm".to_string()),
                    min: Some(0.0),
                    max: Some(6000.0),
                    enum_values: vec![],
                }],
            }],
            globals: vec![HmiBindingsVariable {
                name: "hmi_SystemEnable".to_string(),
                path: "global.hmi_SystemEnable".to_string(),
                data_type: "BOOL".to_string(),
                qualifier: "VAR_GLOBAL".to_string(),
                writable: true,
                widget: "toggle".to_string(),
                inferred_interface: false,
                unit: None,
                min: None,
                max: None,
                enum_values: vec![],
            }],
        }
    }

    #[test]
    fn json_roundtrip() {
        let catalog = make_test_catalog();
        let json = serde_json::to_string_pretty(&catalog).unwrap();
        let parsed: HmiBindingsCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.programs.len(), 1);
        assert_eq!(parsed.globals.len(), 1);
        assert_eq!(parsed.programs[0].variables[0].widget, "slider");
    }

    #[test]
    fn toml_roundtrip() {
        let catalog = make_test_catalog();
        let toml_str = toml::to_string_pretty(&catalog).unwrap();
        let parsed: HmiBindingsCatalog = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.programs.len(), 1);
        assert_eq!(parsed.globals.len(), 1);
    }
}
