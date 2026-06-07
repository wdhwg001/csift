//! csift — "ripgrep for Claude Code session transcripts".
//!
//! Fast regex `list` / `search` / `whoami` over `~/.claude/projects/**/*.jsonl`.
//! This file is the thin binary entrypoint: parse args, dispatch to a subcommand
//! handler, map errors to a process exit code. All real work lives in the modules.
//!
//! Scaffold status (Phase 1): module skeletons compile; handler bodies are
//! `todo!()` and will be filled in Phase 2 per SPEC.md.

mod cli;
mod model;
mod parse;
mod path;
mod search;
mod session;
mod time_window;
mod whoami;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // No silent failure: surface the full error chain on stderr.
            eprintln!("csift: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::List(args) => session::run_list(&args),
        Command::Search(args) => search::run_search(&args),
        Command::Whoami(args) => whoami::run_whoami(&args),
    }
}
