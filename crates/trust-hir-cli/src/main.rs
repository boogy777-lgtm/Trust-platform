//! trust-hir-cli – JSON-RPC daemon for direct HIR access.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod daemon;
mod handlers;
mod protocol;

#[derive(Parser)]
#[command(name = "trust-hir-cli")]
#[command(about = "JSON-RPC daemon for direct HIR access")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start JSON-RPC daemon on stdin/stdout
    Daemon {
        /// Project root path
        #[arg(long)]
        project: PathBuf,
    },
    /// One-shot HIR snapshot
    Snapshot {
        /// Project root path
        #[arg(long)]
        project: PathBuf,
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { project } => {
            let state = handlers::DaemonState::new();
            state.initialize(project)?;

            // Emit a ready notification on stderr so callers can poll if needed,
            // but keep stdout clean for JSON-RPC responses.
            eprintln!("trust-hir-cli daemon ready");

            daemon::run_daemon(&state).await;
        }
        Commands::Snapshot { project, output } => {
            let state = handlers::DaemonState::new();
            state.initialize(project)?;
            let snapshot = handlers::handle_snapshot(&state, serde_json::json!({}))?;
            let json = serde_json::to_string_pretty(&snapshot)?;
            std::fs::write(output, json)?;
        }
    }

    Ok(())
}
