//! csift - "ripgrep for Claude Code session transcripts".
//!
//! Fast regex `list` / `search` / `agents` / `whoami` / `files` / `recover` over
//! `~/.claude/projects/**/*.jsonl`. This file is the thin binary entrypoint: parse
//! args, dispatch to a subcommand handler, map errors to a process exit code. All real
//! work lives in the modules.

mod bash;
mod chardiff;
mod cli;
mod commands;
mod model;
mod parse;
mod path;
mod subagent;
/// Scratch-tree fixtures for the crate's unit tests (the e2e harness's rule, mirrored).
#[cfg(test)]
mod testfs;
mod text;
mod time_window;
mod timez;

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::{parse_argv, Cli, Command};

/// Global allocator: mimalloc. The scan paths (search/turns/files/recover) allocate
/// per-record Strings from many rayon workers at once; macOS's default libmalloc
/// serializes under that load (nanov2/tiny-malloc lock contention shows up directly
/// in profiles). mimalloc's per-thread heaps remove the contention - a measured
/// multi-subcommand win with zero behaviour change (SPEC §7 performance contract).
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    // `parse_argv` wraps clap with an argv-normalization pass so a `--format`/`--kind`
    // flag works in ANY position relative to a leading-`-` encoded project target
    // (see cli::normalize_argv - fixes the allow_hyphen_values greedy-absorb bug).
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
    // The global crossing budget, handed to the SEVEN chain-walking subcommands and to
    // nobody else. It is threaded rather than installed as a process-global (the way
    // `--claude-home` is) so every `Chain::build` / `ChainView::build` site states the
    // reading it runs under and a hard-coded one is visible where it sits.
    let cross = cli.cross_compactions;
    match cli.command {
        Command::List(args) => commands::session::run_list(&args),
        Command::Search(args) => commands::search::run_search(&args, cross),
        Command::Show(args) => commands::show::run_show(&args, cross),
        Command::Stats(args) => commands::stats::run_stats(&args, cross),
        Command::Whoami(args) => commands::whoami::run_whoami(&args),
        Command::Agents(args) => commands::agents::run_agents(&args),
        Command::Files(args) => commands::files::run_files(&args, cross),
        Command::Recover(args) => commands::recover::run_recover(&args, cross),
        Command::Plan(args) => commands::plan::run_plan(&args),
        Command::Verbatim(args) => commands::turns::run_verbatim(&args, cross),
        Command::Image(args) => commands::image::run_image(&args, cross),
        Command::Status(args) => commands::live::run_status(&args),
        Command::Wait(args) => commands::live::run_wait(&args),
        Command::Send(args) => commands::live::channel::run_send(&args),
        Command::Msg(args) => commands::live::channel::run_msg(&args),
        Command::Ack(args) => commands::live::channel::run_ack(&args),
        // The hidden rename tombstone (cli.rs): always the pointed error, never a run.
        Command::Turns(_) => anyhow::bail!(
            "`csift turns` was RENAMED to `csift verbatim` in v0.5 — same command, same \
             flags (reconstruct the VERBATIM turns a compaction summary clipped): re-run \
             as `csift verbatim …`. To simply READ a session's recent turns (no compaction \
             involved), that is `csift show <target> --turn -3..`."
        ),
        Command::Deliver(args) => commands::live::channel::run_deliver(&args),
    }
}
