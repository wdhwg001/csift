//! csift - "ripgrep for Claude Code session transcripts".
//!
//! Fast regex `list` / `search` / `agents` / `whoami` / `files` / `recover` over
//! `~/.claude/projects/**/*.jsonl`. This file is the thin binary entrypoint: parse
//! args, dispatch to a subcommand handler, map errors to a process exit code. All real
//! work lives in the modules.

mod agents;
// The dangerous-removal port is one concern in seven focused files. It is NOT a
// path-named directory module like every other family here: `src/` sits AT the
// sixteen-subfolder structure limit, and a seventeenth would fail the gate, so the
// children are siblings of the root that declares them.
mod bash_danger;
mod bash_danger_argv;
mod bash_danger_census;
mod bash_danger_lexical;
mod bash_danger_out;
mod bash_danger_removal;
mod bash_danger_shape;
mod bash_mutations;
mod chardiff;
mod cli;
mod elicitation;
mod files;
mod image;
mod live;
mod model;
mod parse;
mod path;
mod plan;
mod recover;
mod search;
mod session;
mod show;
mod stats;
mod subagent;
mod text;
mod time_window;
mod timez;
mod turns;
mod whoami;

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
    match cli.command {
        Command::List(args) => session::run_list(&args),
        Command::Search(args) => search::run_search(&args),
        Command::Show(args) => show::run_show(&args),
        Command::Stats(args) => stats::run_stats(&args),
        Command::Whoami(args) => whoami::run_whoami(&args),
        Command::Agents(args) => agents::run_agents(&args),
        Command::Files(args) => files::run_files(&args),
        Command::Recover(args) => recover::run_recover(&args),
        Command::Plan(args) => plan::run_plan(&args),
        Command::Verbatim(args) => turns::run_verbatim(&args),
        Command::Image(args) => image::run_image(&args),
        Command::Status(args) => live::run_status(&args),
        Command::Wait(args) => live::run_wait(&args),
        Command::Send(args) => live::channel::run_send(&args),
        Command::Msg(args) => live::channel::run_msg(&args),
        Command::Ack(args) => live::channel::run_ack(&args),
        // The hidden rename tombstone (cli.rs): always the pointed error, never a run.
        Command::Turns(_) => anyhow::bail!(
            "`csift turns` was RENAMED to `csift verbatim` in v0.5 — same command, same \
             flags (reconstruct the VERBATIM turns a compaction summary clipped): re-run \
             as `csift verbatim …`. To simply READ a session's recent turns (no compaction \
             involved), that is `csift show <target> --turn -3..`."
        ),
        Command::Deliver(args) => live::channel::run_deliver(&args),
    }
}
