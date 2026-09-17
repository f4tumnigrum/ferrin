//! Workspace automation entry point (`cargo xtask <command>`).
//!
//! Commands are described in `docs/03-engineering/02-workspace-layout.md` §6.

// A command-line tool prints to stdout by design.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

mod api_snapshot;
mod check_versions;
mod module_size;
mod publish_order;
mod record_fixture;
mod workspace;

#[cfg(test)]
mod tests;

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Ferrin workspace automation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print publishable workspace crates in dependency order.
    PublishOrder,
    /// Warn about non-test source files above the module size threshold.
    CheckModuleSize {
        /// Line threshold above which a warning is printed.
        #[arg(long, default_value_t = 800)]
        threshold: usize,
    },
    /// Compare the resolved versions of direct external dependencies against
    /// the latest stable releases on crates.io (exit code 1 when outdated).
    CheckVersions,
    /// Record a provider fixture from a live request described by
    /// `tests/fixtures/<case>.scenario.json`.
    RecordFixture {
        /// Provider crate short name (`openai`, `anthropic`, `google`,
        /// `openai-compatible`).
        #[arg(long)]
        provider: String,
        /// Fixture case (`<area>/<name>`, e.g. `responses/tool-call`).
        #[arg(long)]
        case: String,
    },
    /// Write the public API summary of every publishable crate to
    /// `docs/api/<crate>.json` (rustdoc JSON, all features).
    ApiSnapshot {
        /// Compare against the committed files instead of writing them.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::PublishOrder => publish_order::run(),
        Command::CheckModuleSize { threshold } => module_size::run(threshold),
        Command::CheckVersions => check_versions::run(),
        Command::RecordFixture { provider, case } => record_fixture::run(&provider, &case),
        Command::ApiSnapshot { check } => api_snapshot::run(check),
    }
}
