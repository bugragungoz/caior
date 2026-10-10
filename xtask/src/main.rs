//! Developer tasks. Run with `cargo xtask <command>`.

mod catalog_cmd;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "CAIOR developer tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Catalog tools.
    #[command(subcommand)]
    Catalog(CatalogCommand),
}

#[derive(Subcommand)]
enum CatalogCommand {
    /// Parse and validate every catalog file.
    Check {
        #[command(flatten)]
        sel: Selection,
        /// Require hashes and sizes (release mode).
        #[arg(long)]
        strict: bool,
    },
    /// Check that each source URL answers, and show where it redirects.
    Probe {
        #[command(flatten)]
        sel: Selection,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 6)]
        jobs: usize,
    },
    /// Download sources into the cache, then write size, sha256 and signer back into the TOML files.
    Refresh {
        #[command(flatten)]
        sel: Selection,
        /// Download again even when size and hash are already filled.
        #[arg(long)]
        force: bool,
        #[arg(long, default_value_t = 4)]
        jobs: usize,
        #[arg(long, default_value = "cache")]
        cache: PathBuf,
        /// Also print the full report (signature, file version, MSI codes) as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the catalog as a markdown table.
    Summary {
        #[command(flatten)]
        sel: Selection,
    },
}

/// Which packages a command works on. Empty means all.
#[derive(Args, Clone, Default)]
pub struct Selection {
    /// Catalog directory.
    #[arg(long, default_value = "catalog")]
    pub dir: PathBuf,
    /// Only these catalog files (file name, e.g. vcredist.toml). Repeatable.
    #[arg(long = "file")]
    pub files: Vec<String>,
    /// Only these package ids; a trailing * matches a prefix. Repeatable.
    #[arg(long = "id")]
    pub ids: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Catalog(cmd) => match cmd {
            CatalogCommand::Check { sel, strict } => catalog_cmd::check(&sel, strict),
            CatalogCommand::Probe { sel, json, jobs } => catalog_cmd::probe(&sel, json, jobs),
            CatalogCommand::Refresh { sel, force, jobs, cache, json } => {
                catalog_cmd::refresh(&sel, force, jobs, &cache, json)
            }
            CatalogCommand::Summary { sel } => catalog_cmd::summary(&sel),
        },
    }
}
