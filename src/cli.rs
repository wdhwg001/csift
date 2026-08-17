//! Command-line surface (clap derive).
//!
//! Eleven subcommands: `list`, `search`, `show`, `stats`, `agents`, `whoami`, `files`,
//! `recover`, `plan`, `verbatim`, `image`. Each carries example-rich help (`--help`) keyed off
//! the SPEC §6 baseline invocations. `list`/`search`/`stats`/`files`/`recover`/`plan`/`image`
//! span each session's subagent transcripts by default (`--no-subagents` opts out); `verbatim`
//! is the exception - a single-thread recovery tool whose per-session budget MULTIPLIES, so
//! it defaults to the TOP-LEVEL thread only, opts INTO spanning via `--subagents`, and
//! REQUIRES a target. `agents` reports a session's subagent lifecycle (it lists subagents as
//! targets, so it rejects both span flags). `show` FETCHES the records of exactly ONE
//! transcript by `--line`/`--uuid` (rendered full, or `--raw` verbatim bytes); `stats` is
//! the one-scan per-session aggregate view. `plan` resolves the plan file BOUND to a session
//! (its `plan_mode` attachment); `recover --file @plan` reconstructs that bound plan's
//! content.
//! The session-operating subcommands
//! (`list`/`search`/`show`/`stats`/`agents`/`files`/`recover`/`plan`/`verbatim`/`image`)
//! resolve their target through ONE shared resolver
//! ([`crate::path::resolve_session_files`]): a positional `[PATH]...` that is a cwd / encoded
//! dir, an `@<uuid>` / `@<agent-hex>` / `@main` / `@trap:<marker>` session token, or a `*.jsonl` file.
//! A BARE uuid (no `@`) is not special - prefix it `@<uuid>`. (For `search` the
//! first positional is PATTERN, so a session is targeted by an `@<uuid>` PATH positional - see
//! [`SearchArgs::pattern`].) `whoami` is the exception (no target - it reads
//! `$CLAUDE_CODE_SESSION_ID`).
//!
//! ## argv normalization (flag-ordering fix)
//!
//! The real entrypoint is [`parse_argv`], NOT `Cli::parse` - it runs [`normalize_argv`]
//! first so a `--format`/`--shape`/… flag works in ANY position relative to a
//! leading-`-` encoded project target (clap's `allow_hyphen_values` otherwise lets a
//! `Vec` positional greedily swallow the trailing flag). See [`normalize_argv`].

use std::path::PathBuf;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::model::Class;

mod agents_args;
mod argv;
mod files_recover_args;
mod formats;
mod image_plan_args;
mod list_args;
mod root;
mod search_args;
mod selectors;
mod show_stats_args;
mod verbatim_whoami_args;

pub(crate) use agents_args::*;
pub(crate) use argv::*;
pub(crate) use files_recover_args::*;
pub(crate) use formats::*;
pub(crate) use image_plan_args::*;
pub(crate) use list_args::*;
pub(crate) use root::*;
pub(crate) use search_args::*;
pub(crate) use selectors::*;
pub(crate) use show_stats_args::*;
pub(crate) use verbatim_whoami_args::*;

#[cfg(test)]
mod tests;
