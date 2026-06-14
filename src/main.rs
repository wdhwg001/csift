//! csift — "ripgrep for Claude Code session transcripts".
//!
//! Fast regex `list` / `search` / `agents` / `whoami` / `files` / `recover` over
//! `~/.claude/projects/**/*.jsonl`. This file is the thin binary entrypoint: parse
//! args, dispatch to a subcommand handler, map errors to a process exit code. All real
//! work lives in the modules.

mod agents;
mod bash_mutations;
mod cli;
mod files;
mod model;
mod parse;
mod path;
mod plan;
mod recover;
mod search;
mod session;
mod subagent;
mod text;
mod time_window;
mod timez;
mod turns;
mod whoami;

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::{parse_argv, Cli, Command};

fn main() -> ExitCode {
    // `parse_argv` wraps clap with an argv-normalization pass so a `--format`/`--kind`
    // flag works in ANY position relative to a leading-`-` encoded project target
    // (see cli::normalize_argv — fixes the allow_hyphen_values greedy-absorb bug).
    let cli = parse_argv();
    // Install the `--claude-home` override (if any) BEFORE dispatch, so every subcommand's
    // path resolution honors it. `$CLAUDE_CONFIG_DIR` is read directly by `path::claude_home`
    // and needs no wiring here.
    if let Some(dir) = cli.claude_home.clone() {
        path::set_claude_home_override(dir);
    }
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
        Command::Agents(args) => agents::run_agents(&args),
        Command::Files(args) => files::run_files(&args),
        Command::Recover(args) => recover::run_recover(&args),
        Command::Plan(args) => plan::run_plan(&args),
        Command::Turns(args) => turns::run_turns(&args),
    }
}
