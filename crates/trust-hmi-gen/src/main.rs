//! `trust-hmi-gen` — CLI tool for generating HMI visualization bindings
//! from CODESYS Structured Text projects.
//!
//! This crate replaces the deprecated `trust-lsp.hmiInit` and
//! `trust-lsp.hmiBindings` LSP commands with a standalone binary.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod generator;
mod hmi_bindings;
mod hmi_init;
mod project;

use project::ProjectLoader;

/// CLI entry point for trust-hmi-gen.
#[derive(Parser)]
#[command(name = "trust-hmi-gen")]
#[command(about = "Generate HMI visualization bindings from CODESYS ST projects")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Generate HMI project skeleton
    Init {
        /// Project root directory containing ST sources
        #[arg(long, value_name = "PATH")]
        project: PathBuf,
        /// Output directory for generated HMI files
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
        /// Visual style theme
        #[arg(long, value_name = "STYLE", default_value = "industrial")]
        style: String,
    },
    /// Generate HMI variable bindings from ST symbol table
    Bindings {
        /// Project root directory containing ST sources
        #[arg(long, value_name = "PATH")]
        project: PathBuf,
        /// Path to existing HMI configuration file
        #[arg(long, value_name = "FILE")]
        hmi_config: Option<PathBuf>,
        /// Output directory for generated bindings
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    /// Full HMI generation: init + bindings + build
    Build {
        /// Project root directory containing ST sources
        #[arg(long, value_name = "PATH")]
        project: PathBuf,
        /// Path to existing HMI configuration file
        #[arg(long, value_name = "FILE")]
        hmi_config: Option<PathBuf>,
        /// Output directory for all generated files
        #[arg(long, value_name = "DIR")]
        output: PathBuf,
        /// Visual style theme
        #[arg(long, value_name = "STYLE", default_value = "industrial")]
        style: String,
        /// Output format for bindings
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    /// Validate HMI configuration against ST symbols (no file output)
    Validate {
        /// Project root directory containing ST sources
        #[arg(long, value_name = "PATH")]
        project: PathBuf,
        /// Path to HMI configuration file to validate
        #[arg(long, value_name = "FILE")]
        hmi_config: PathBuf,
    },
}

/// Output format for generated bindings.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
enum OutputFormat {
    /// JSON output
    #[default]
    Json,
    /// TOML output
    Toml,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            project,
            output,
            style,
        } => {
            let loader = ProjectLoader::load(&project)?;
            let out_dir = output.unwrap_or_else(|| project.join("hmi"));
            hmi_init::generate_skeleton(&loader, &out_dir, &style)?;
            println!("HMI skeleton generated at {}", out_dir.display());
        }
        Commands::Bindings {
            project,
            hmi_config,
            output,
            format,
        } => {
            let loader = ProjectLoader::load(&project)?;
            let out_dir = output.unwrap_or_else(|| project.join("hmi"));
            let bindings = hmi_bindings::generate_bindings(&loader, hmi_config.as_deref())?;
            generator::write_bindings(&bindings, &out_dir, format)?;
            println!(
                "HMI bindings generated ({} programs, {} globals)",
                bindings.programs.len(),
                bindings.globals.len()
            );
        }
        Commands::Build {
            project,
            hmi_config,
            output,
            style,
            format,
        } => {
            let loader = ProjectLoader::load(&project)?;
            let hmi_dir = output.join("hmi");

            // Step 1: init skeleton
            hmi_init::generate_skeleton(&loader, &hmi_dir, &style)?;

            // Step 2: generate bindings
            let bindings = hmi_bindings::generate_bindings(&loader, hmi_config.as_deref())?;
            generator::write_bindings(&bindings, &hmi_dir, format)?;

            // Step 3: write combined manifest
            generator::write_manifest(&loader, &bindings, &hmi_dir, &style)?;

            println!("Full HMI build completed at {}", output.display());
        }
        Commands::Validate {
            project,
            hmi_config,
        } => {
            let loader = ProjectLoader::load(&project)?;
            let issues = hmi_bindings::validate_config(&loader, &hmi_config)?;
            if issues.is_empty() {
                println!("HMI configuration is valid.");
            } else {
                println!("HMI configuration has {} issue(s):", issues.len());
                for issue in &issues {
                    println!("  [{}] {}", issue.code, issue.message);
                }
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
