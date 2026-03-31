use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rug", version, about = "Review Until Green — compact PR state for coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Full PR status: new comments + CI + verdict
    Status {
        /// PR URL, owner/repo#number, or omit for current branch
        pr: Option<String>,
    },
    /// CI/check status only (lightweight poll)
    Checks {
        /// PR URL, owner/repo#number, or omit for current branch
        pr: Option<String>,
    },
    /// Mark comment IDs as addressed in local state
    MarkAddressed {
        /// Comment IDs to mark as addressed
        #[arg(required = true)]
        ids: Vec<u64>,
        /// PR URL (default: current branch)
        #[arg(long)]
        pr: Option<String>,
    },
    /// Clear local state for a PR
    Reset {
        /// PR URL, owner/repo#number, or omit for current branch
        pr: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { pr } => todo!("status {pr:?}"),
        Commands::Checks { pr } => todo!("checks {pr:?}"),
        Commands::MarkAddressed { ids, pr } => todo!("mark {pr:?} {ids:?}"),
        Commands::Reset { pr } => todo!("reset {pr:?}"),
    }
}
