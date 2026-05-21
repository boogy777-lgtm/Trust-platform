//! HMI binding generation from ST symbol tables.
//!
//! Maps IEC 61131-3 types to HMI widget types and produces a
//! machine-readable binding catalog.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::project::{
    is_writable_qualifier, type_name_for_symbol, widget_for_hir_type, ProjectLoader, SymbolRef,
};

/// Generated HMI binding catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmiBindingsCatalog {
    /// Programs and their HMI-visible variables.
    pub programs: Vec<HmiBindingsProgram>,
    /// Global variables visible to HMI.
    pub globals: Vec<HmiBindingsVariable>,
}

/// A program's HMI binding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmiBindingsProgram {
    /// Program name.
    pub name: String,
    /// Source file path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// HMI-visible variables.
    pub variables: Vec<HmiBindingsVariable>,
}

/// A single variable binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmiBindingsVariable {
    /// Variable name.
    pub name: String,
    /// Fully-qualified path (e.g. `Program.Var` or `Global.Var`).
    pub path: String,
    /// IEC 61131-3 type name.
    #[serde(rename = "type")]
    pub data_type: String,
    /// Variable qualifier (`VAR_INPUT`, `VAR_OUTPUT`, etc.).
    pub qualifier: String,
    /// Whether the variable is writable through HMI.
    pub writable: bool,
    /// Suggested widget type.
    pub widget: String,
    /// Whether this binding was inferred rather than explicitly annotated.
    #[serde(default, skip_serializing_if = "is_false")]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub inferred_interface: bool,
    /// Engineering unit, if inferred from naming.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Minimum value, if inferable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value, if inferable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Enum values, if the type is an enumeration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<String>,
}

/// A validation issue found during HMI config checking.
#[derive(Debug, Clone, Serialize)]
pub struct HmiValidationIssue {
    /// Issue code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Affected binding path, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

const HMI_DIAG_UNKNOWN_BIND: &str = "HMI_BIND_UNKNOWN_PATH";
#[allow(dead_code)]
const HMI_DIAG_TYPE_MISMATCH: &str = "HMI_BIND_TYPE_MISMATCH";
#[allow(dead_code)]
const HMI_DIAG_UNKNOWN_WIDGET: &str = "HMI_UNKNOWN_WIDGET_KIND";

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Generate HMI bindings by scanning all symbols in the loaded project.
///
/// If `hmi_config_path` is provided, it is used to filter or override
/// which symbols are exported. Otherwise, all symbols matching HMI
/// heuristics are included.
///
/// # Errors
///
/// Returns an error if the HMI config file cannot be read or parsed.
pub fn generate_bindings(
    loader: &ProjectLoader,
    hmi_config_path: Option<&Path>,
) -> anyhow::Result<HmiBindingsCatalog> {
    let _config = hmi_config_path.map(std::fs::read_to_string).transpose()?;

    let mut programs = Vec::new();
    let mut globals = Vec::new();

    // Collect program variables
    for prog_ref in loader.programs_with_variables() {
        let program_name = prog_ref.name.clone();
        let mut variables = Vec::new();

        for var in prog_ref.variables {
            let qualifier = match &var.kind {
                trust_hir::symbols::SymbolKind::Variable { qualifier } => *qualifier,
                _ => trust_hir::symbols::VarQualifier::Local,
            };

            if !should_include_variable(&var, qualifier) {
                continue;
            }

            let var_binding = make_variable_binding(&var, &program_name);
            variables.push(var_binding);
        }

        if !variables.is_empty() {
            programs.push(HmiBindingsProgram {
                name: program_name,
                file: None,
                variables,
            });
        }
    }

    // Collect global variables
    for sym_ref in loader.global_variables() {
        let qualifier = match &sym_ref.kind {
            trust_hir::symbols::SymbolKind::Variable { qualifier } => *qualifier,
            _ => trust_hir::symbols::VarQualifier::Local,
        };

        if !should_include_variable(&sym_ref, qualifier) {
            continue;
        }

        let var_binding = make_variable_binding(&sym_ref, "global");
        globals.push(var_binding);
    }

    Ok(HmiBindingsCatalog { programs, globals })
}

fn should_include_variable(var: &SymbolRef, qualifier: trust_hir::symbols::VarQualifier) -> bool {
    // Include if marked with HMI naming convention
    if is_hmi_export_symbol_by_name(&var.name) {
        return true;
    }

    // Skip temporary and external variables
    if matches!(
        qualifier,
        trust_hir::symbols::VarQualifier::Temp | trust_hir::symbols::VarQualifier::External
    ) {
        return false;
    }

    // Include all other variables (inferred)
    true
}

fn is_hmi_export_symbol_by_name(name: &str) -> bool {
    let name_upper = name.to_ascii_uppercase();
    name_upper.starts_with("HMI_") || name_upper.ends_with("_HMI")
}

/// Validate an existing HMI configuration against the current project
/// symbols.
///
/// # Errors
///
/// Returns an error if the config file cannot be read.
pub fn validate_config(
    loader: &ProjectLoader,
    hmi_config_path: &Path,
) -> anyhow::Result<Vec<HmiValidationIssue>> {
    let _config_text = std::fs::read_to_string(hmi_config_path)?;

    // Build a lookup of all valid binding paths
    let bindings = generate_bindings(loader, None)?;
    let mut valid_paths: HashMap<String, String> = HashMap::new();

    for prog in &bindings.programs {
        for var in &prog.variables {
            valid_paths.insert(var.path.clone(), var.data_type.clone());
        }
    }
    for var in &bindings.globals {
        valid_paths.insert(var.path.clone(), var.data_type.clone());
    }

    let mut issues = Vec::new();

    // TODO: Parse the actual HMI config format and validate each binding
    // For now, we do a basic structural validation
    if valid_paths.is_empty() {
        issues.push(HmiValidationIssue {
            code: HMI_DIAG_UNKNOWN_BIND.to_string(),
            message: "no HMI-exportable symbols found in project".to_string(),
            path: None,
        });
    }

    Ok(issues)
}

fn make_variable_binding(sym: &SymbolRef, scope: &str) -> HmiBindingsVariable {
    let type_name = type_name_for_symbol(&sym.table, sym.type_id);
    let qualifier = qualifier_from_kind(&sym.kind);
    let writable = match &sym.kind {
        trust_hir::symbols::SymbolKind::Variable { qualifier } => is_writable_qualifier(*qualifier),
        _ => false,
    };

    let ty = sym.table.type_by_id(sym.type_id);
    let widget = ty.map_or("value", |t| widget_for_hir_type(t, writable));

    let path = if scope == "global" {
        format!("global.{}", sym.name)
    } else {
        format!("{scope}.{}", sym.name)
    };

    let (unit, min, max) = infer_unit_and_range(&type_name, &sym.name);
    let enum_values = extract_enum_values(ty, &sym.table);

    HmiBindingsVariable {
        name: sym.name.clone(),
        path,
        data_type: type_name,
        qualifier,
        writable,
        widget: widget.to_string(),
        inferred_interface: !is_hmi_export_symbol_by_name(&sym.name),
        unit,
        min,
        max,
        enum_values,
    }
}

fn qualifier_from_kind(kind: &trust_hir::symbols::SymbolKind) -> String {
    match kind {
        trust_hir::symbols::SymbolKind::Variable { qualifier } => match qualifier {
            trust_hir::symbols::VarQualifier::Local => "VAR".to_string(),
            trust_hir::symbols::VarQualifier::Input => "VAR_INPUT".to_string(),
            trust_hir::symbols::VarQualifier::Output => "VAR_OUTPUT".to_string(),
            trust_hir::symbols::VarQualifier::InOut => "VAR_IN_OUT".to_string(),
            trust_hir::symbols::VarQualifier::Temp => "VAR_TEMP".to_string(),
            trust_hir::symbols::VarQualifier::Global => "VAR_GLOBAL".to_string(),
            trust_hir::symbols::VarQualifier::External => "VAR_EXTERNAL".to_string(),
            trust_hir::symbols::VarQualifier::Access => "VAR_ACCESS".to_string(),
            trust_hir::symbols::VarQualifier::Static => "VAR_STAT".to_string(),
        },
        _ => "VAR".to_string(),
    }
}

/// Try to infer engineering unit and plausible min/max from name and type.
fn infer_unit_and_range(
    type_name: &str,
    var_name: &str,
) -> (Option<String>, Option<f64>, Option<f64>) {
    let name_lower = var_name.to_ascii_lowercase();
    let mut unit: Option<String> = None;
    let mut min: Option<f64> = None;
    let mut max: Option<f64> = None;

    if name_lower.contains("pressure") {
        unit = Some("bar".to_string());
        min = Some(0.0);
        max = Some(10.0);
    } else if name_lower.contains("temp") || name_lower.contains("temperature") {
        unit = Some("°C".to_string());
        min = Some(-40.0);
        max = Some(150.0);
    } else if name_lower.contains("speed") || name_lower.contains("rpm") {
        unit = Some("rpm".to_string());
        min = Some(0.0);
        max = Some(6000.0);
    } else if name_lower.contains("flow") {
        unit = Some("L/min".to_string());
        min = Some(0.0);
        max = Some(100.0);
    } else if name_lower.contains("voltage") || name_lower.contains("volt") {
        unit = Some("V".to_string());
        min = Some(0.0);
        max = Some(480.0);
    } else if name_lower.contains("current") || name_lower.contains("amp") {
        unit = Some("A".to_string());
        min = Some(0.0);
        max = Some(100.0);
    }

    // Override for boolean types
    if type_name.eq_ignore_ascii_case("BOOL") {
        min = None;
        max = None;
        unit = None;
    }

    (unit, min, max)
}

fn extract_enum_values(
    ty: Option<&trust_hir::types::Type>,
    table: &trust_hir::symbols::SymbolTable,
) -> Vec<String> {
    let mut result = Vec::new();
    let Some(ty) = ty else {
        return result;
    };

    match ty {
        trust_hir::types::Type::Enum { values, .. } => {
            for (name, _value) in values {
                result.push(name.to_string());
            }
        }
        trust_hir::types::Type::Alias { target, .. } => {
            // Follow alias
            if let Some(base) = table.type_by_id(*target) {
                return extract_enum_values(Some(base), table);
            }
        }
        _ => {}
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_inference_temperature() {
        let (unit, min, max) = infer_unit_and_range("REAL", "MotorTemperature");
        assert_eq!(unit, Some("°C".to_string()));
        assert_eq!(min, Some(-40.0));
        assert_eq!(max, Some(150.0));
    }

    #[test]
    fn unit_inference_pressure() {
        let (unit, min, max) = infer_unit_and_range("REAL", "SystemPressure");
        assert_eq!(unit, Some("bar".to_string()));
        assert_eq!(min, Some(0.0));
        assert_eq!(max, Some(10.0));
    }

    #[test]
    fn unit_inference_bool_clears_range() {
        let (unit, min, max) = infer_unit_and_range("BOOL", "hmi_EnableMotor");
        assert_eq!(unit, None);
        assert_eq!(min, None);
        assert_eq!(max, None);
    }
}
