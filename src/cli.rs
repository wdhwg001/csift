//! Command-line surface (clap derive).
//!
//! Nine subcommands: `list`, `search`, `agents`, `whoami`, `files`, `recover`, `plan`,
//! `turns`, `image`. Each carries example-rich help (`--help`) keyed off the SPEC §6.1–§6.9
//! baseline invocations. `list`/`search`/`files`/`recover`/`plan`/`image` span each session's
//! subagent transcripts by default (`--no-subagents` opts out); `turns` is the exception — a
//! single-thread recovery tool whose per-session budget MULTIPLIES, so it defaults to the
//! TOP-LEVEL thread only and opts INTO spanning via `--include-subagents`. `agents` reports a
//! session's subagent lifecycle (it lists subagents as targets, so it has no subagent-span
//! flag). `plan` resolves the plan file BOUND to a session (its `plan_mode` attachment);
//! `recover --file @plan` reconstructs that bound plan's content. `search` doubles as the
//! message-fetcher: `--line`/`--uuid` address specific records (rendered full) — the
//! in-permission alternative to `Read`-ing the raw jsonl.
//! The session-operating subcommands
//! (`list`/`search`/`agents`/`files`/`recover`/`plan`/`turns`/`image`)
//! resolve their target through ONE shared resolver
//! ([`crate::path::resolve_session_files`]): a positional `[PATH]...` that is a cwd / encoded
//! dir, an `@<uuid>` / `@<agent-hex>` / `@main` / `@trap:<marker>` session token, or a `*.jsonl` file.
//! A BARE uuid (no `@`) is not special — prefix it `@<uuid>`. (For `search` the
//! first positional is PATTERN, so a session is targeted by an `@<uuid>` PATH positional — see
//! [`SearchArgs::pattern`].) `whoami` is the exception (no target — it reads
//! `$CLAUDE_CODE_SESSION_ID`).
//!
//! ## argv normalization (flag-ordering fix)
//!
//! The real entrypoint is [`parse_argv`], NOT `Cli::parse` — it runs [`normalize_argv`]
//! first so a `--format`/`--kind`/… flag works in ANY position relative to a
//! leading-`-` encoded project target (clap's `allow_hyphen_values` otherwise lets a
//! `Vec` positional greedily swallow the trailing flag). See [`normalize_argv`].

use std::path::PathBuf;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};

/// Parse + normalize the process argv into a [`Cli`] (the real entrypoint).
///
/// This wraps [`Cli::parse`] with an argv NORMALIZATION pass that fixes the
/// flag-ordering bug described on [`normalize_argv`]: a project-target positional must
/// tolerate a leading `-` (every encoded projects-dir token starts with `-`), which
/// requires clap's `allow_hyphen_values`; but that setting makes a multi-value
/// positional GREEDILY absorb every following token — including declared long flags
/// like `--format json` — so `csift list <ENCODED> --format json` used to fail with
/// `no project dir named "--format"`. We reorder declared flags ahead of the
/// leading-dash positionals BEFORE clap parses, so `--format` works in any position.
#[must_use]
pub fn parse_argv() -> Cli {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_argv(raw);
    Cli::parse_from(normalized)
}

/// Value parser for a project-target positional/option. Accepts a real cwd, an encoded
/// `-Users-…` token (SINGLE leading `-`), or `.`. It REJECTS a `--`-leading token: an
/// encoded projects-dir basename always starts with a SINGLE `-` (an absolute cwd's leading
/// `/` encodes to one `-`), so a DOUBLE-dash token can never be a real target — it is an
/// unknown / typo'd flag the `allow_hyphen_values` positional would otherwise swallow,
/// surfacing the misleading `no project dir named "--xxx"` error instead of clap's clean
/// `unexpected argument '--xxx'` + `did you mean --no-subagents?` suggestion. Returning `Err`
/// here makes clap reconsider the token and emit that standard message uniformly across
/// every scope-operating subcommand (search/whoami already did; the rest did not). A SINGLE
/// `-` token is still accepted (it is a genuine encoded target). The [`normalize_argv`]
/// pre-pass has already routed DECLARED flags away, so a `--`-token reaching here is by
/// construction undeclared.
fn parse_project_target(s: &str) -> Result<PathBuf, String> {
    if s.starts_with("--") {
        return Err(format!(
            "unexpected argument '{s}' — not a project target (encoded dirs start with a \
             single '-', never '--'); did you mistype a flag?"
        ));
    }
    Ok(PathBuf::from(s))
}

/// Reorder argv so declared flags (and the value of value-taking ones) precede the
/// leading-`-` project-target positionals, defeating clap's greedy `allow_hyphen_values`
/// var-arg absorption WITHOUT a hardcoded flag table.
///
/// ## Why a pre-pass (vs a value parser / `allow_hyphen_values` tweak)
///
/// clap issue #3880 + empirical probing (clap 4.6): a `Vec` positional with
/// `allow_hyphen_values=true` swallows every following token — including KNOWN long
/// flags — once it starts consuming, and `num_args`/value parsers cannot undo that
/// (a value-parser `Err` hard-aborts rather than letting clap retry the token as a
/// flag). The robust, well-understood fix is to normalize argv first.
///
/// ## Zero-drift flag discovery
///
/// We introspect csift's OWN [`Cli::command`] for the active subcommand to learn its
/// long flags and which take a value (action `Set`/`Append` take a value; `SetTrue`/
/// `SetFalse`/`Count`/help/version do not). So the flag set is never duplicated — it
/// follows the derive definitions automatically.
///
/// ## Algorithm (per subcommand args, after the subcommand name)
///
/// - `--` terminator: everything after it is passed through verbatim (clap's escape).
/// - A `--flag=value` token → a flag token (its value is inline).
/// - A bare `--flag` token → a flag token; if that flag takes a value AND the next
///   token does not itself start with `-`, the next token is its value (kept paired).
/// - A `-x` token whose `x` is a DECLARED short flag is hoisted ahead of the positionals
///   exactly like a long flag (`-t user` keeps its paired value if the flag takes one,
///   `-tuser`/`-i` are emitted as-is). clap does NOT resolve a trailing declared short
///   flag ahead of an `allow_hyphen_values` positional — the positional swallows it — so
///   we reorder it here too. A leading-dash ENCODED token (a single `-` followed by
///   alphanumerics/dashes, whose first char is NOT a declared short flag, e.g.
///   `-Users-…`) is NOT a flag and stays a positional.
/// - Everything else is a positional.
///
/// Flags (with paired values) are emitted first, then positionals, preserving each
/// group's relative order. clap then validates the result and errors clearly on any
/// genuine misuse.
#[must_use]
pub fn normalize_argv(argv: Vec<String>) -> Vec<String> {
    // argv[0] = program; argv[1] = subcommand (if any). Find the subcommand and the
    // index where its args start.
    if argv.len() < 2 {
        return argv;
    }
    let cmd = Cli::command();
    let sub_name = &argv[1];
    let Some(sub) = cmd
        .get_subcommands()
        .find(|s| s.get_name() == sub_name || s.get_all_aliases().any(|a| a == sub_name))
    else {
        // Not a recognized subcommand (e.g. `--help`, `--version`, a typo) — leave
        // argv untouched and let clap produce its normal message.
        return argv;
    };

    // Long flags of this subcommand that TAKE a value (need their following token),
    // and the full set of declared long flags (to detect a value that is actually the
    // NEXT flag, e.g. a user typo `--max-count --format`).
    let mut value_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The declared SHORT flags of this subcommand, and which take a value. A `-x` short
    // token that is a DECLARED short flag must be hoisted ahead of the `allow_hyphen_values`
    // positional EXACTLY like a long flag — otherwise `search PATTERN . -t user` lets the
    // positional greedily swallow `-t` (and `-i`), surfacing the misleading "no project dir
    // named -t" error. An UNDECLARED `-x` (e.g. an encoded `-Users-…` token, which is a
    // single dash + alnum/dash) is NOT in this set, so it stays a positional.
    let mut short_value: std::collections::HashSet<char> = std::collections::HashSet::new();
    let mut short_all: std::collections::HashSet<char> = std::collections::HashSet::new();
    // Enumerate the subcommand's own args PLUS the root command's GLOBAL args (e.g.
    // `--claude-home <DIR>`): a global value-flag propagates to every subcommand but may
    // not surface in `sub.get_arguments()` at introspection time, and if it isn't known to
    // take a value, a `--claude-home /path` placed AFTER the subcommand would mis-sort the
    // path as a positional. HashSet inserts are idempotent, so double-counting is harmless.
    for a in sub
        .get_arguments()
        .chain(cmd.get_arguments().filter(|a| a.is_global_set()))
    {
        if let Some(longs) = a.get_long_and_visible_aliases() {
            let takes = flag_takes_value(a);
            for l in longs {
                let f = format!("--{l}");
                if takes {
                    value_long.insert(f.clone());
                }
                all_long.insert(f);
            }
        }
        if let Some(shorts) = a.get_short_and_visible_aliases() {
            let takes = flag_takes_value(a);
            for c in shorts {
                short_all.insert(c);
                if takes {
                    short_value.insert(c);
                }
            }
        }
    }

    let head = argv[..2].to_vec(); // program + subcommand
    let rest = &argv[2..];

    let mut flags: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut passthrough: Vec<String> = Vec::new();

    let mut i = 0;
    let mut after_terminator = false;
    while i < rest.len() {
        let tok = &rest[i];
        if after_terminator {
            passthrough.push(tok.clone());
            i += 1;
            continue;
        }
        if tok == "--" {
            // Everything after the explicit terminator is verbatim positional input.
            after_terminator = true;
            passthrough.push(tok.clone());
            i += 1;
            continue;
        }
        if tok.starts_with("--") {
            // `--flag=value` carries its own value inline; a bare `--flag` may need
            // the next token if it is a value-taking flag.
            if let Some((name, _)) = tok.split_once('=') {
                let _ = name;
                flags.push(tok.clone());
                i += 1;
            } else if value_long.contains(tok) {
                flags.push(tok.clone());
                // A value-taking flag (arity 1) consumes its NEXT token as the value —
                // INCLUDING a leading-`-` token when that flag carries `allow_hyphen_values`.
                // The only tokens that are NOT its value: the `--` terminator, or another
                // DECLARED long flag (a user typo such as `--format --kind`, which we leave
                // for clap to report). A bare short `-x` or an encoded `-Users-…` IS consumed.
                if i + 1 < rest.len() && rest[i + 1] != "--" && !all_long.contains(&rest[i + 1]) {
                    flags.push(rest[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            } else {
                // A boolean long flag (or an unknown `--x`): no paired value.
                flags.push(tok.clone());
                i += 1;
            }
        } else if let Some(short_c) = declared_short_flag(tok, &short_all) {
            // A `-x…` token whose first post-dash char `x` is a DECLARED short flag. Hoist
            // it ahead of the positionals like a long flag. A BARE `-x` (exactly two chars)
            // that takes a value consumes the NEXT token as its value (the same pairing
            // rule as a value-taking long flag); a bundled `-xVALUE` (`-tuser`) or a boolean
            // `-i` carries everything inline and is emitted as one token. A leading-`-`
            // ENCODED token (`-Users-…`) never reaches here — its first char is not a
            // declared short flag, so `declared_short_flag` returns `None` and it falls to
            // the positional arm below.
            flags.push(tok.clone());
            if tok.len() == 2 && short_value.contains(&short_c) {
                if i + 1 < rest.len() && rest[i + 1] != "--" && !rest[i + 1].starts_with('-') {
                    flags.push(rest[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            // Positional: a real path, `.`, or a leading-`-` encoded token (which is a
            // SINGLE dash; it did not match `--` above and its first char is not a declared
            // short flag).
            positionals.push(tok.clone());
            i += 1;
        }
    }

    let mut out = head;
    out.extend(flags);
    out.extend(positionals);
    out.extend(passthrough);
    out
}

/// If `tok` is a `-x…` short-flag token whose first post-dash character is a DECLARED
/// short flag of the active subcommand, return that character; else `None`. Used by
/// [`normalize_argv`] to distinguish a real short flag (`-t`, `-i`, a bundled `-tuser`)
/// — which must be hoisted ahead of an `allow_hyphen_values` positional — from a
/// leading-`-` ENCODED project token (`-Users-…`, whose first char is not a declared
/// short flag) which stays a positional. A bare `-` or `--`-leading token is never a
/// short flag here (the caller handles `--` separately; a lone `-` has no flag char).
fn declared_short_flag(tok: &str, short_all: &std::collections::HashSet<char>) -> Option<char> {
    let rest = tok.strip_prefix('-')?;
    if rest.starts_with('-') {
        return None; // a `--long` token, handled by the long-flag path.
    }
    let first = rest.chars().next()?;
    if short_all.contains(&first) {
        Some(first)
    } else {
        None
    }
}

/// True when an arg consumes a value (so its following token is its value, not a
/// separate positional). `Set`/`Append` take a value; `SetTrue`/`SetFalse`/`Count`/
/// help/version do not.
fn flag_takes_value(a: &clap::Arg) -> bool {
    !matches!(
        a.get_action(),
        ArgAction::SetTrue
            | ArgAction::SetFalse
            | ArgAction::Count
            | ArgAction::Help
            | ArgAction::HelpShort
            | ArgAction::HelpLong
            | ArgAction::Version
    )
}

/// ripgrep for Claude Code session transcripts.
#[derive(Debug, Parser)]
#[command(
    name = "csift",
    version,
    about = "ripgrep for Claude Code session transcripts",
    long_about = "csift — \"ripgrep for Claude Code session transcripts\".\n\n\
        A fast Rust CLI to list and regex-search Claude Code session `.jsonl` files \
        under ~/.claude/projects/<encoded>/. Built for an LLM consumer (a CC agent \
        searching or recovering its own / a peer session): output is clean, \
        token-efficient and regex-driven. Human/LLM-readable text by default; pass \
        `--format json` for machine use.\n\n\
        Pure regex/ripgrep only — explicitly NO BM25 / embeddings / semantic search.\n\n\
        SUBCOMMANDS\n  \
          list     fast \"which session is this?\" index (first/last user + last agent)\n  \
          search   regex over transcripts, returning the complete round-trip exchange per hit\n  \
          agents   list a session's subagents (kind, start/completion, status) + time-window filter\n  \
          whoami   identify the calling CC session via $CLAUDE_CODE_SESSION_ID\n  \
          files    which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash)\n  \
          recover  reconstruct a file's history from the transcript — segmented diff-patches,\n           \
                   point-in-time partial snapshot, or coverage scoping\n  \
          plan     locate the Plan-Mode plan file bound to a session (recover --file @plan dumps it)\n  \
          turns    turn-fidelity reconstruction — restore the verbatim user/assistant\n           \
                   back-and-forth a compaction summary clipped, within a char/token budget\n  \
          image    list + extract the inline images a session carries (pastes/screenshots)\n\n\
        (`search` also FETCHES: `--line`/`--uuid` address specific records, rendered full — \
        the in-permission alternative to `Read`-ing the raw jsonl.)\n\n\
        list/search/files/recover/plan/image span each session's subagent transcripts by \
        default (built-in Task/Agent-tool, OMC, and Workflow agents); pass `--no-subagents` \
        to restrict to top-level sessions. `turns` is the exception among the file-operating \
        commands — it defaults to the TOP-LEVEL thread only (single-thread recovery) and \
        opts INTO spanning via `--include-subagents`. (`agents` LISTS subagents as its \
        targets rather than spanning them as inputs, so it has no subagent flag.)\n\n\
        A target is EITHER a real filesystem cwd (it gets path-encoded) OR an \
        already-encoded `-Users-...` projects-dir token; with no target, every \
        project under ~/.claude/projects is scanned.\n\n\
        TARGETING a session/agent (always positional): `@<uuid>` (or its \
        leading-hex prefix, e.g. `@13d9645a`) scopes to one session; `@<agent-hex>` to one \
        subagent + its subtree; `@main` is the CALLING top-level session (read from \
        $CLAUDE_CODE_SESSION_ID); a `*.jsonl` path scopes to that one transcript. A BARE uuid \
        (no `@`) is NOT special.\n\n\
        @trap:<marker> — \"which subagent am I?\", on EVERY subcommand (including `whoami`, which \
        reports your bare hex + parent uuid). CC withholds a subagent's OWN id from its Bash \
        environment, so a running subagent cannot name itself via env. Instead you INVENT a marker \
        and embed it LITERALLY in this very csift command; csift finds the transcript whose Bash \
        `csift` command carries that marker (CC records the tool_use to disk before it runs) and \
        scopes to that agent's subtree — or to the session itself, if you are the main thread. \
        DISCIPLINE (enforced): invent the marker ONE-SHOT, by you, right now — an imaginative, \
        literary, random, CONTEXT-INDEPENDENT token of EXACTLY 3 CamelCase words + 4 random digits, \
        shaped like `@trap:JollyShinyBrook4283` — but that exact literal is the RESERVED doc \
        example, refused by csift (a copied example self-collides into ambiguity), so invent your \
        OWN. NEVER generate it with a script (a generator would be \
        another `csift`-ish command carrying the marker → ambiguity); never build it from a \
        shell variable or string concatenation (it must appear VERBATIM in the recorded \
        command); never reuse a previous one. csift REJECTS lazy markers: not EXACTLY 3 words, \
        single-letter or ALLCAPS \"words\" (`HTML`/`USB`…), not exactly 4 trailing digits, or \
        trivial digits (1111 / 1234 / 9876 / 1357 / 2468).",
    after_help = "EXAMPLES\n  \
          csift list                                  # index every session, all projects\n  \
          csift list .                                # just this cwd's project\n  \
          csift search \"carry\"                        # smart-case regex, all projects\n  \
          csift search \"\" -t user --since 2h .        # pure filter: user turns, last 2h, here\n  \
          csift agents @<uuid> --since 2h             # subagents TRIGGERED in the last 2h\n  \
          csift whoami                                # who am I (this CC session)?\n  \
          csift files @<uuid> --by file               # which files this session modified, when\n  \
          csift recover @<uuid> --file /abs/app.py    # segmented diff-patch history of a file\n  \
          csift recover . --file /abs/app.py --at @turn:42  # partial snapshot as the LLM saw it at turn 42\n  \
          csift turns . --budget 40000                # restore the verbatim back-and-forth a summary clipped\n  \
          csift plan @<uuid>                          # locate the plan file bound to a session\n  \
          csift image @<uuid> --out /tmp/imgs         # extract every pasted image to a dir\n  \
          csift search \"\" @<uuid> --no-subagents --line 46550   # fetch the exact message a hit reported, in full\n  \
          csift search \"\" @<uuid> --no-subagents --line 100-140 # fetch a contiguous span of records\n\n\
        Run `csift <subcommand> --help` for per-subcommand flags + examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override the Claude Code config dir — the `~/.claude` directory, or wherever it has
    /// been relocated. Applies to EVERY subcommand (it determines where session
    /// transcripts and plan files are read from). Highest priority; the `$CLAUDE_CONFIG_DIR`
    /// env var (Claude Code's own relocation mechanism) is honored too, and a bare
    /// `~/.claude` under `$HOME` is the default. Point this at the dir that IS the `.claude`
    /// equivalent — transcripts are read from `<DIR>/projects/<encoded>/*.jsonl`.
    #[arg(long = "claude-home", value_name = "DIR", global = true)]
    pub claude_home: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List sessions with first/last genuine-user + last-agent message and timestamps.
    List(ListArgs),
    /// Regex-search sessions, returning complete request/response round-trip exchanges.
    Search(SearchArgs),
    /// Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID, falling back
    /// to CODEX_COMPANION_SESSION_ID).
    Whoami(WhoamiArgs),
    /// List a session's subagents with kind, start/completion timestamps + status.
    Agents(AgentsArgs),
    /// Which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash).
    Files(FilesArgs),
    /// Reconstruct a file's history from the transcript — segmented diff-patches,
    /// point-in-time partial snapshot, or coverage scoping.
    Recover(RecoverArgs),
    /// Locate the Plan-Mode plan file BOUND to a session (via its `plan_mode` attachment).
    Plan(PlanArgs),
    /// Turn-fidelity reconstruction — restore the verbatim user/assistant
    /// back-and-forth a compaction summary clipped, within a char/token budget.
    Turns(TurnsArgs),
    /// List + extract the images a session carries (inline base64 blocks → files).
    Image(ImageArgs),
}

/// How to interpret `--budget`: as raw characters (default) or as tokens (estimated
/// at [`crate::turns::TOKEN_CHARS`] characters/token, a documented heuristic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum BudgetUnit {
    /// Interpret `--budget` as a character count (default).
    #[default]
    Chars,
    /// Interpret `--budget` as a token count (≈4 chars/token heuristic).
    Tokens,
}

/// A transcript content category, used by `search --category/-t` (repeatable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Category {
    /// Assistant thinking blocks.
    Thinking,
    /// GENUINE human input + AskUserQuestion answers + machine AUTOMATION-TRIGGER openers
    /// (`<task-notification>`, rendered as the parsed `[<kind> <id> <status>] <summary>`
    /// attribution label, where `<kind>` is background-command / workflow / agent / monitor /
    /// task) — NOT tool_result carriers. The automation openers DO open a turn, so they surface
    /// under `user`; recognize them by the `[<kind> …]` prefix.
    User,
    /// `tool_use` blocks (agent calling a tool); AskUserQuestion is a tool_use.
    Tool,
    /// `tool_result` blocks (the tool's response).
    #[value(name = "tool-response")]
    ToolResponse,
    /// Assistant visible end-of-turn text (the agent message).
    Agent,
}

/// How to render matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human/LLM-readable with headers (default).
    #[default]
    Text,
    /// One JSON object per emitted unit, for machine consumption.
    Json,
}

/// The four image formats the Claude API accepts — the only `image --out <file.ext>` conversion
/// targets, and the only formats a transcript's inline images are ever stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOutFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageOutFormat {
    /// The output format implied by an `--out` path EXTENSION (case-insensitive), or `None` when
    /// the extension isn't one of the four image types (⇒ the path is treated as a directory).
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(ImageOutFormat::Png),
            "jpg" | "jpeg" => Some(ImageOutFormat::Jpeg),
            "gif" => Some(ImageOutFormat::Gif),
            "webp" => Some(ImageOutFormat::Webp),
            _ => None,
        }
    }

    /// The lower-case file extension for this format.
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            ImageOutFormat::Png => "png",
            ImageOutFormat::Jpeg => "jpg",
            ImageOutFormat::Gif => "gif",
            ImageOutFormat::Webp => "webp",
        }
    }

    /// The canonical `image/*` media type.
    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            ImageOutFormat::Png => "image/png",
            ImageOutFormat::Jpeg => "image/jpeg",
            ImageOutFormat::Gif => "image/gif",
            ImageOutFormat::Webp => "image/webp",
        }
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "List sessions with a fast quick-identity tuple per session — WITHOUT \
        parsing the whole file. For each session jsonl under the target(s) it emits: \
        session id, the FIRST genuine-user message (+ time), the LAST genuine-user \
        message (+ time), the LAST agent message (+ time), plus the decoded cwd / git \
        branch / CC version. A forward HEAD read finds the first user; a backward \
        TAIL read finds the last user/agent — neither parses the full file, so it \
        stays fast on 200 MB+ transcripts. Files are scanned in parallel across the \
        corpus, then sorted for deterministic output.\n\n\
        A PATH is either a real cwd (path-encoded for you) or an already-encoded \
        `-Users-...` token; with no PATH, every project is listed. Note: genuine-user \
        excludes tool_result carriers, isMeta pseudo-turns (\"Continue from where you \
        left off.\"), and compaction summaries.",
    after_help = "EXAMPLES\n  \
          csift list                                                  # every session, all projects\n  \
          csift list .                                                # sessions for the current cwd's project\n  \
          csift list /Users/testuser/Projects/widget_app_prototype  # a real path (gets encoded)\n  \
          csift list -Users-testuser-Projects-widget-app-prototype  # a pre-encoded dir token\n  \
          csift list ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype\n  \
          csift list @<uuid> --no-subagents                          # JUST the one top-level session row\n  \
          csift list --format json .                                  # machine-readable index\n\n\
        SCOPE: because the default SPANS subagents, a `csift list @<uuid>` can return 1 \
        top-level + N subagent rows. The text output then leads with a `scope  N sessions in \
        scope (1 top-level + M subagent)` banner, brands each subagent row \
        `SUBAGENT <hex> · parent SESSION <uuid>` (a bare hex is NOT a re-feedable target — \
        re-feed the parent uuid), and a top-level row keeps the plain `SESSION <uuid>` header.\n\n\
        JSON SCHEMA (per --format json)\n  \
          One BARE object per session (JSONL — one record per line, no envelope): \
        {session_id, is_subagent, parent_session_id, path, cwd, git_branch, version, \
        first_user, last_user, last_agent, skipped_lines}. `is_subagent` flags a bare-hex \
        subagent row; `parent_session_id` is the re-feedable owning uuid (= session_id for a \
        top-level row) — never re-feed a subagent `session_id`. The \
        `first_user`/`last_user`/`last_agent` fields are {excerpt, ts_utc, ts_local} \
        sub-objects (or null when absent). UNLIKE search/files/recover/turns, this stream has \
        NO trailing terminator object — it is pure JSONL, end-of-stream = EOF."
)]
pub struct ListArgs {
    /// One or more targets: an actual filesystem cwd, a direct
    /// `~/.claude/projects/<encoded>` path / bare `<encoded>` dir, or an `@`-prefixed session
    /// token. Repeatable. Defaults to all projects. `@<uuid>` (8-4-4-4-12 hex) scopes to that
    /// one top-level session (searched across all projects when no project path is given), so
    /// `csift list @<uuid>` identifies it — the SAME positional surface `files`/`recover`/`turns`
    /// use; `@main`/`@trap:<marker>` resolve the calling session from the environment; a `*.jsonl` file
    /// path scopes to that transcript. NOTE: the default still SPANS that session's subagents, so
    /// a fan-out session lists 1 + N rows; add `--no-subagents` for just the single top-level row.
    /// A bare uuid WITHOUT `@` is NOT a session here — prefix it (`@<uuid>`). To inspect one
    /// subagent use `csift agents --agent <hex>`, or pass the PARENT session uuid as `@<uuid>`.
    ///
    /// `allow_hyphen_values` is REQUIRED: every encoded dir starts with `-`
    /// (an absolute cwd's leading `/` encodes to `-`), e.g.
    /// `csift list -Users-testuser-Projects-foo`. Without this clap would reject the
    /// leading `-` token as an unknown flag (SPEC §6.1 baseline invocation). The
    /// `parse_project_target` value parser NARROWS that tolerance so a real flag
    /// (`--format json`) is parsed as a flag in any position, never swallowed as a PATH value.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Exclude subagent transcripts — list only the top-level `<uuid>.jsonl` sessions (the
    /// pre-subagent behavior). Subagent transcripts are spanned by default; this is the only
    /// span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl ListArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }
}

#[derive(Debug, Clone, Args)]
#[command(
    long_about = "Regex-search transcripts, returning the COMPLETE round-trip exchange \
        containing each hit — never a bare fragment. A turn is delimited by a genuine \
        user message; the emitted exchange is the whole turn (the opening user record \
        plus every assistant/thinking/tool_use/tool_result record chained under it \
        until the next genuine user). So a matched tool_use comes WITH its \
        tool_result, a matched user turn WITH the agent's response, etc.\n\n\
        The PATTERN is ripgrep-like and defaults to SMART-CASE: case-insensitive \
        unless it contains an uppercase letter; `-i` forces case-insensitive and \
        always wins. `--multiline` lets `.` cross newlines. An EMPTY pattern is a \
        pure filter — it matches every category-eligible record, so combine it with \
        `--category` / `--since` / `--turn-range` (a bare empty pattern with no other \
        filter warns that it will emit a lot).\n\n\
        CATEGORIES (`-t`, repeatable): thinking | user | tool | tool-response | agent. \
        With none given, all five are eligible. `user` is the genuine human turn PLUS \
        the answer to an AskUserQuestion (the full Q+options+answer unit) PLUS a \
        plan-rejection-with-message (+ a [plan: …] pointer) PLUS a machine AUTOMATION \
        trigger; it excludes plain tool_result carriers, interrupts, and slash-command \
        wrappers. `tool` includes AskUserQuestion tool_use calls.\n\n\
        AUTOMATION TRIGGERS: a `<task-notification>` (a background-command / workflow / \
        spawned-agent COMPLETION pulse Claude Code injects as a `type:\"user\"` record) \
        OPENS a turn like a human message, so it surfaces under `-t user`. It renders as a \
        parsed attribution label `[<kind> <task-id> <status>] <summary>` (kind = \
        background-command | workflow | agent | monitor | task, read from the summary) — \
        never the raw `<task-id>`/`<output-file>` XML. Match it like any other text (e.g. \
        `search 'background-command' -t user`).\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based on turn-boundary \
        order) is mutually exclusive with `--since`/`--until`. Time bounds accept \
        ISO8601 (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative form (`2h`, \
        `3d`, `90m`, `45s`, `1w`) meaning \"that long ago\" in the system local timezone.\n\n\
        `--max-count` caps emitted exchanges but reports the dropped count (default: \
        unlimited — no cap) — there is NO silent truncation anywhere.",
    after_help = "EXAMPLES\n  \
          csift search \"carry\"                                  # all projects, smart-case\n  \
          csift search \"carry\" .                                # this project (positional PATH, like every sibling)\n  \
          csift search -i \"askuserquestion\" -t tool             # tool_use blocks naming AUQ\n  \
          csift search \"\" -t user --since 2h .                  # user turns, last 2h, this project\n  \
          csift search \"tail.read\" --multiline @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\n  \
          csift search \"panic\" -t agent -t thinking --turn-range 10..20 --max-count 50\n  \
          csift search \"persisted-output\" --resolve-persisted --format json\n  \
          csift search \"refactor\" -c                            # COUNT matches only (ripgrep -c idiom)\n  \
          csift search \"\" @<uuid> --no-subagents --line 88     # fetch a record by line (FULL)\n  \
          csift search \"\" @<parent> --line 7f3c9e21:88,495-500 # …in a SUBAGENT transcript (hex-prefixed --line)\n  \
          csift search \"let's chat\" -t user --siblings 3        # the match WITH up to 3 sibling records\n  \
          csift search \"let's chat\" -t user --siblings agent:1  # …only the agent-side sibling (cap 1)\n  \
          csift search \"let's chat\" -t user --siblings agent:1 --no-truncate  # …and READ that reply end-to-end\n\n\
        SIBLINGS (`--siblings <SPEC>`)\n  \
          A match renders only the records that MATCHED. `--siblings <SPEC>` additionally renders \
        the OTHER records of the same turn (the back-and-forth around the hit) under a `·` marker, \
        so a matched user question surfaces WITH the agent's reply — no need to drop to the raw \
        jsonl. Each SPEC token (repeatable / comma-joined) is a CAP: a bare `N` caps the TOTAL \
        siblings (any category) at N; a `<cat>:N` (cat ∈ thinking|user|tool|tool-response|agent, \
        ≥1) caps THAT category at N. When both forms are given, each typed cap governs its own \
        category and the bare `N` caps every OTHER category; a category with no typed spec and no \
        bare-`N` fallback is not shown. A record that itself matched is never duplicated as a \
        sibling.\n\n\
        COUNT (`-c` / `--count-only`)\n  \
          `-c`/`--count-only` prints just the integer match total (ripgrep `-c`), honoring every \
        filter. That total is ALSO always in the normal output's footer (alongside the \
        distinct-session total); `--count-only` just isolates that ONE integer for a pipe. To \
        list WHICH sessions matched, pipe `--format json` through \
        `jq -r .session_id | sort -u`.\n\n\
        REGEX DIALECT — linear-time (RE2-class)\n  \
          The pattern is the Rust `regex` crate (regex::bytes), which GUARANTEES \
        linear-time matching in the input length: NO catastrophic backtracking, ever.\n  \
          Supported: literals; character classes [...] / [^...] / \\d \\w \\s and \
        Unicode classes \\p{...}; alternation |; groups (...) and non-capturing \
        (?:...); quantifiers * + ? {m,n} (greedy + lazy *?); anchors ^ $ \\b \\B; \
        dot . (use --multiline to let it cross newlines); inline flags (?i)(?m)(?s)(?x); \
        Unicode-aware by default.\n  \
          NOT supported (these need non-linear engines): backreferences \\1; \
        lookahead/lookbehind (?=) (?!) (?<=) (?<!); atomic groups / possessive \
        quantifiers (?>...) / a*+. A pattern using these fails to COMPILE with a clear \
        error — by design, not a bug.\n  \
          Case: smart-case by default (insensitive unless the pattern has an uppercase \
        letter); -i forces insensitive. --multiline lives in the SAME dialect (it sets \
        the (?s)(?m) flags).\n\n\
        AUTOMATION TRIGGERS (under `-t user`)\n  \
          A machine `<task-notification>` (a background-command / workflow / spawned-agent / \
        monitor-tick COMPLETION pulse) OPENS a user turn, so it surfaces under `-t user`. It \
        renders as a PARSED attribution label `[<kind> <task-id> <status>] <summary>` (kind = \
        background-command | workflow | agent | monitor | task, read from the summary) — \
        never the raw XML. Match it like any text, e.g. `csift search 'background-command' -t \
        user`. The `<kind>` prefix distinguishes a machine opener from a genuine human \
        message.\n\n\
        CATEGORY DEFAULT\n  \
          With NO `-t`/`--category`, ALL FIVE categories are searched (thinking, user, tool, \
        tool-response, agent) — a zero-hit result then means the pattern truly matched \
        nothing, not that a category was excluded.\n\n\
        JSON SCHEMA (per --format json)\n  \
          One ENVELOPE object PER matched exchange (NOT one bare record per line): \
        {session_id, is_subagent, parent_session_id, turn_index, ts_utc, ts_local, \
        record_uuids:[…], hits:[{category, excerpt, tool_name, ts_utc, ts_local}, …]} — the \
        per-hit objects carry no session_id; it lives on the envelope. With `--siblings`, the \
        envelope also carries a `siblings:[…]` array (same per-hit shape) for the turn's \
        non-matched records. Envelopes stream in \
        a COMBINED STABLE CHRONOLOGICAL order (subagent exchanges interleaved with top-level \
        by `ts_utc`, the turn-opening timestamp; timestamp-less exchanges sort last); the \
        per-hit `ts_utc` may be later than the envelope's for a deep tool_use match. \
        `session_id` is the transcript's own id: a re-feedable top-level uuid, OR a bare \
        SUBAGENT hex when `is_subagent` is true (that hex is NOT a re-feedable `@<uuid>` target — \
        re-feed `parent_session_id`, which is always the owning top-level uuid). \
        `record_uuids` lists every record stitched into the round-trip (§6.4 completeness \
        evidence). A trailing footer object {matched, dropped_by_cap, skipped_lines} closes \
        the stream. (Whole-document `json.load` fails — parse line-by-line as JSONL: N \
        envelopes then the footer.)"
)]
pub struct SearchArgs {
    /// Regex pattern (ripgrep-like, default smart-case). MAY be empty for a
    /// pure filter (use `--category` / time / turn filters alone).
    ///
    /// search's FIRST positional is PATTERN (unlike files/turns/list/agents/recover, whose
    /// first positional is the PATH target), so a bare uuid here is a LITERAL pattern, searched
    /// verbatim. To SCOPE to a session, pass it as an `@<uuid>` POSITIONAL (a PATH target),
    /// exactly like every sibling: `csift search PATTERN @<uuid>`.
    #[arg(value_name = "PATTERN", default_value = "")]
    pub pattern: String,

    /// Project target(s) as a POSITIONAL `[PATH]...` — the SAME scope-target surface every
    /// sibling subcommand uses (`csift search PATTERN .` now works, matching
    /// `csift files .`). An actual cwd or an encoded `-Users-…` dir token; repeatable.
    /// With none, every project is scanned. A target may ALSO be an `@<uuid>` (8-4-4-4-12 hex)
    /// session token (so `csift search PATTERN @<uuid>` scopes to that one session),
    /// `@main`/`@trap:<marker>` (the calling session), or a `*.jsonl` file. A bare SUBAGENT hex is NOT
    /// accepted here — inspect one subagent with `csift agents --agent <hex>`.
    ///
    /// `allow_hyphen_values` is REQUIRED: every encoded dir starts with `-`; the
    /// `normalize_argv` pre-pass routes declared flags — LONG and the short `-t`/`-i` —
    /// away from the positional, so a trailing flag (`… <path> --format json` or
    /// `… <path> -t user`) is never swallowed.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Exclude subagent transcripts — search only the top-level `<uuid>.jsonl` sessions.
    /// Subagent transcripts are searched by default; this is the only span flag. Workflow
    /// `journal.jsonl` event logs are not transcripts and are never searched.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Filter to one or more categories. Repeatable.
    #[arg(short = 't', long = "category", value_enum)]
    pub categories: Vec<Category>,

    /// Case-insensitive match (overrides smart-case).
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Allow `.` to match newlines / multiline patterns.
    #[arg(long)]
    pub multiline: bool,

    /// Inclusive turn-index range `START..END`, 0-BASED — turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `s·t<n>` header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Mutually exclusive with `--since` /
    /// `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 datetime (`2026-06-01T05:00:00Z`); or a bare date (`2026-06-01`) = LOCAL
    /// MIDNIGHT that day. Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Cap emitted exchanges (default: unlimited — no cap). NO silent truncation — the drop
    /// count is reported.
    #[arg(long, value_name = "N")]
    pub max_count: Option<usize>,

    /// Print ONLY the total number of matching exchanges (one integer) — the ripgrep
    /// `-c` idiom for "how many times X?". Honors every filter (`-t`, time window,
    /// session/path scope) and reports the TRUE total even if `--max-count` would cap
    /// the listing. With `--format json`, prints `{"matched":N}` instead. (You rarely need
    /// it: the normal output's footer ALWAYS carries this same match total plus the
    /// distinct-session total — `--count-only` just isolates that ONE integer for a pipe,
    /// hence "only".)
    #[arg(long = "count-only", short = 'c')]
    pub count_only: bool,

    /// Also render the SIBLING records of every matched turn — the rest of the
    /// back-and-forth, not only the matched line — so a matched USER question surfaces
    /// WITH the agent's reply (answers "I said X, what did you say back?"). Repeatable /
    /// comma-joined SPEC tokens turn sibling rendering ON and CAP how many show:
    /// a bare `N` caps the TOTAL siblings (any category) at N; a `<category>:N`
    /// (category ∈ thinking|user|tool|tool-response|agent, the same set as `-t`) caps
    /// THAT category at N (must be ≥1). Mix them — e.g. `--siblings tool:1 --siblings
    /// thinking:2` or `--siblings 3`. When both a bare `N` and typed `<cat>:N` specs are
    /// given, each typed cap governs its own category and the bare `N` caps every OTHER
    /// category (the rest); a category with no typed spec and no bare-`N` fallback is NOT
    /// shown. A record that itself matched is never repeated as a sibling. No effect under
    /// `--count-only`.
    #[arg(long, value_name = "SPEC", value_delimiter = ',')]
    pub siblings: Vec<String>,

    /// Emit each matched (and `--siblings`) record's FULL text instead of the ~400-char
    /// excerpt — so you can READ a found message end-to-end (e.g. the question at the tail
    /// of a long reply) without dropping to the raw jsonl. Newlines are still collapsed to
    /// single spaces (one line per record). The default excerpt stays centered on the match
    /// with an explicit `… (+N chars)` marker; `--no-truncate` removes the cap entirely.
    #[arg(long)]
    pub no_truncate: bool,

    /// ADDRESS by physical line(s): fetch the record(s) at these 1-based line numbers / ranges
    /// instead of (or as well as) pattern-matching — the permission-friendly alternative to
    /// `Read`-ing the raw jsonl, built for BATCH. Repeatable AND comma-delimited, each token
    /// `N` or `A-B` (inclusive, ascending): `--line 87,495-500,992`. Addressed records render
    /// FULL. Lines are per-file, so `--line` needs the scope to pin a SINGLE transcript
    /// (`@<uuid>` [`--no-subagents`]); to address a SUBAGENT transcript, prefix the FIRST token
    /// with its bare hex `<hex>:<spec>` (e.g. `--line 7f3c9e21:88,495-500` → subagent `7f3c9e21`,
    /// lines 88,495-500). All hex-bearing tokens must name the SAME transcript (lines address
    /// ONE file). A range CLAMPS to the file; an EXPLICIT line that resolves to nothing is
    /// reported as `unresolved`.
    #[arg(long, value_name = "SPEC", value_delimiter = ',')]
    pub line: Vec<String>,

    /// ADDRESS by record `uuid`(s) (globally unique) — fetch those exact records, FULL.
    /// Repeatable AND comma-delimited (`--uuid a,b` or `--uuid a --uuid b`). Scope is optional
    /// (uuid is global) but an `@<uuid>`/PATH scope makes the scan fast. A uuid that resolves
    /// to nothing is reported as `unresolved`.
    #[arg(long, value_name = "UUID", value_delimiter = ',')]
    pub uuid: Vec<String>,

    /// Resolve `<persisted-output>` pointers to their `tool-results/<id>.txt` file.
    #[arg(long)]
    pub resolve_persisted: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl SearchArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }

    /// The project targets to scope to: the positional `[PATH]...`. Empty ⇒ the shared
    /// resolver scans every project.
    #[must_use]
    pub fn targets(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }
}

/// Which subagent kinds to surface in `agents`. Mirrors the on-disk discriminator
/// (path location), with `agentType` retained as a descriptive sub-label per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentKindFilter {
    /// Built-in Task/Agent-tool subagents (`subagents/agent-<hex>.jsonl`).
    #[value(name = "builtin-task")]
    BuiltinTask,
    /// Workflow / OMC workflow-subagent transcripts
    /// (`subagents/workflows/wf_*/agent-<hex>.jsonl`).
    Workflow,
    /// "Teammate" agents (`taskKind:"in_process_teammate"`) — Claude Code's persistent,
    /// directly-addressable team members. They share the built-in on-disk location
    /// (`subagents/agent-<id>.jsonl`); the meta.json `taskKind` is the discriminator.
    Teammate,
}

#[derive(Debug, Args)]
#[command(
    long_about = "List a session's SUBAGENT lifecycle: every subagent transcript the \
        session spawned, with its id, KIND, start + completion timestamps, duration, \
        and a determinable status. Three on-disk shapes are discovered under \
        `<session>/subagents/**` (verified empirically against ~/.claude/projects):\n  \
          • builtin-task  subagents/agent-<hex>.jsonl                 (Task/Agent tool)\n  \
          • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl (OMC workflows)\n  \
          • teammate      subagents/agent-a<Name>-<hex>.jsonl         (in_process_teammate / FleetView; meta taskKind)\n\
        Workflow `journal.jsonl` event logs are NOT transcripts — they are read only \
        to corroborate completion status, never listed as agents.\n\n\
        The TARGET selects the parent session: pass `@<uuid>` for one \
        session, or a project PATH/encoded-dir to cover every session under it (each \
        session's subagents are grouped under it). Start/completion come from the \
        subagent transcript's first/last record timestamp; status is `completed` when \
        a workflow journal carries a `result` event for the agent (or the transcript \
        terminates cleanly), else `running`/`unknown`.\n\n\
        `--since`/`--until` (ISO8601 or relative `2h`/`3d`/…, in the system local \
        timezone) filter to subagents whose TRIGGER time (the parent tool_use ts — the \
        true spawn instant) falls in the window by default; `--order-by start` uses the \
        transcript's first-record ts, `--order-by completion` the last.\n\n\
        TOPOLOGY: the output is ALWAYS the parent→child tree — workflow runs as parent \
        nodes of their agents, and a nested sub-subagent under its spawning agent (text \
        indents by depth; JSON nests `children`). Count by traversing the rendered tree / \
        nested JSON. `--agent <hex>` grabs ONE subagent with its returned message; \
        `--returned-message` adds the 3-way-resolved returned message to every row; \
        `--with-files` attaches each node's files-changed list.",
    after_help = "TARGET / TOPOLOGY (scope guidance)\n  \
          The TARGET selects the PARENT session whose subagents to list: pass `@<uuid>` \
        (or `@main`/`@trap:<marker>`) for ONE session, or a project PATH/encoded-dir \
        to cover every session under it (each session's subagents grouped under it). \
        Subagent shapes discovered under `<session>/subagents/**`:\n    \
            • builtin-task  subagents/agent-<hex>.jsonl                       (Task/Agent tool)\n    \
            • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl     (OMC workflows)\n    \
            • teammate      subagents/agent-a<Name>-<hex>.jsonl              (in_process_teammate; meta taskKind)\n  \
          Workflow `journal.jsonl` event logs are NOT transcripts (read only to corroborate \
        status, never listed). Status is `completed` when a workflow journal carries a \
        `result` event (or the transcript terminates cleanly), else `running`/`unknown`. \
        The output is ALWAYS the parent→child topology tree (workflow runs as parents of \
        their agents; a nested sub-subagent under its spawning agent) — there is no flat \
        mode; count by traversing the tree / nested JSON. `--agent <hex>` grabs ONE subagent \
        by its bare-hex id (full node + returned \
        message); it is a DIRECT lookup that IGNORES --since/--until/--order-by/--kind and \
        renders just that node (a tree of one), and \
        a non-matching hex is a hard error (run a plain listing first to discover ids). NOTE: \
        `--kind` here is the TRANSCRIPT-SHAPE filter (builtin-task | workflow), a DIFFERENT \
        axis from the automation-trigger `kind` (background-command/agent/monitor/task) used \
        by `turns`/`search -t user` — they overlap only on the token `workflow`.\n\n\
        EXAMPLES\n  \
          csift agents @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d              # one session's subagent tree\n  \
          csift agents .                                                   # every session under this project\n  \
          csift agents . --kind workflow                                   # only workflow-shape agents (NOT the automation kind)\n  \
          csift agents @<uuid> --since 2h                                  # subagents TRIGGERED in the last 2h\n  \
          csift agents @<uuid> --since 6h --order-by completion            # COMPLETED in the last 6h\n  \
          csift agents @<uuid> --order-by completion                       # order/window on the completion axis\n  \
          csift agents @<uuid> --since 2026-06-01T09:00:00Z --order-by completion  # COMPLETED since an ISO bound\n  \
          csift agents @<uuid>                                             # discover ids, then grab one:\n  \
          csift agents --agent <hex>                                       #   grab ONE subagent (direct lookup; ignores time/kind)\n  \
          csift agents @<uuid> --with-files                                # each node + its files-changed list\n  \
          csift agents @<uuid> --returned-message                          # add the 3-way-resolved returned message to every row\n  \
          csift agents . --format json                                     # machine-readable per-session tree\n\n\
        JSON SCHEMA (per --format json)\n  \
          One object PER SESSION: {session_id, workflow_runs:[run], agents:[node]} — the \
        nested tree shape (always). A node's sub-subagents nest under its `children` \
        array: {agent_id, agent_type, kind, status, parent_session_id, parent_agent_id, \
        workflow_id, depth, description, spawn_tool, spawn_tool_use_id, trigger_utc, \
        trigger_local, started_utc, started_local, completed_utc, completed_local, duration, \
        skipped_lines}. `agent_type` is the semantic agent ROLE / subagent-type string (e.g. \
        `Explore`, `general-purpose`, `oh-my-claudecode:critic`, `workflow-subagent`) — \
        DISTINCT from `kind`, which is the on-disk transcript SHAPE (builtin-task | workflow). \
        There is no role filter today (only `--kind` for shape). ID-DOMAIN: `agent_id` IS this \
        record's transcript-own id — the SAME concept other commands call `session_id` (a bare \
        SUBAGENT hex here, since `agents` only lists subagents, so an `is_subagent` flag is \
        implied-true and omitted); re-feed `parent_session_id`, never the bare `agent_id`. \
        `--with-files` adds a \
        `files_changed` array; `--returned-message` \
        (implied by a single `--agent`) adds `returned_message` + `returned_message_source`. \
        A single `--agent <hex>` grab is the ONE exception to the per-session envelope: it \
        emits just the matched node as a bare object (no enclosing {session_id, …} wrapper). \
        A workflow RUN parent object is {run_id, task_id, workflow_name, \
        status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, \
        started_utc, started_local, children}. Every `_utc` field carries a paired `_local` \
        (system-local ISO). The malformed-line count rides on each node's `skipped_lines`. \
        UNLIKE search/files/recover/turns, this stream has NO trailing terminator object — it \
        is pure JSONL, end-of-stream = EOF."
)]
pub struct AgentsArgs {
    /// Project target (actual cwd or encoded dir) whose sessions' subagents to list, OR an
    /// `@<uuid>` session token. Repeatable; with none, every project is scanned. `@<uuid>`
    /// (8-4-4-4-12 hex) scopes to one session (so `csift agents @<uuid>` lists its subagents),
    /// `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to that transcript.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// HIDDEN no-op subagent-span flag. `agents` has NO subagent-span control: it DISCOVERS
    /// subagents as its primary output, so `--no-subagents` is meaningless here (unlike
    /// list/search/files/recover, which span their session's subagent transcripts). It is
    /// accepted only to emit a pointed error instead of letting `allow_hyphen_values` swallow
    /// it as a bogus PATH value (the misleading `invalid value '--no-subagents' for
    /// '[PATH]...'`). See [`AgentsArgs::span_flag_error`].
    #[arg(long = "no-subagents", hide = true)]
    pub no_subagents: bool,

    /// Only show subagents of this kind (repeatable). Default: all kinds. This is the
    /// subagent TRANSCRIPT-SHAPE filter — its values are `builtin-task` | `workflow`. It is
    /// NOT the automation-TRIGGER `kind` taxonomy (`background-command`/`agent`/`monitor`/
    /// `task`/`workflow`) that `turns`/`search -t user` use; the two axes share only the
    /// literal token `workflow` (different meaning). `csift agents --kind monitor` is a
    /// parse error for this reason. Ignored when `--agent <hex>` is given (a direct grab).
    #[arg(long = "kind", value_enum)]
    pub kinds: Vec<AgentKindFilter>,

    /// Lower time bound. WHEN grammar (system-local tz): relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that long AGO (`45s`,`90m`,`2h`,`3d`,`1w`); an ISO8601 datetime
    /// (`2026-06-01T05:00:00Z`); or a bare date (`2026-06-01`) = LOCAL MIDNIGHT. Filters by
    /// TRIGGER time by default (`--order-by start|completion` switches axis).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Same axis as `--since`.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// The ORDERING axis: which timestamp sorts the tree AND bounds `--since`/`--until`.
    /// `trigger` (DEFAULT — the true parent-tool_use spawn instant), `start` (the
    /// subagent's first transcript record / child-head ts, which LAGS the trigger by
    /// seconds), or `completion` (the last record). Named `--order-by` (not `--by`, which
    /// reads like a projection) because it names the sort axis. (Sibling note: `files` uses
    /// `--by` for a PROJECTION — a different meaning on a different subcommand.)
    #[arg(long = "order-by", value_enum, default_value_t = AgentTimeAxis::Trigger)]
    pub order_by: AgentTimeAxis,

    /// Grab ONE subagent by its bare-hex id: prints its full node incl. the returned
    /// message (implies `--returned-message`) and, with `--with-files`, its files-changed.
    /// This is a DIRECT id lookup: it BYPASSES `--since`/`--until`/`--order-by` and `--kind`
    /// (a known id resolves regardless of when it ran or its shape), and just the matched
    /// node is rendered (a tree of one — not the whole workflow tree). If the hex matches
    /// nothing in scope, it is a hard ERROR with discovery guidance — not the ambiguous
    /// `no subagents found`. Discover ids first with `csift agents @<uuid>` (the `agent_id`
    /// column / JSON field).
    #[arg(long, value_name = "HEX")]
    pub agent: Option<String>,

    /// Attach each node's files-changed list (reuses the `files` extractors over the
    /// subagent's own transcript). Off by default — it re-scans each transcript.
    #[arg(long = "with-files")]
    pub with_files: bool,

    /// Include each subagent's RETURNED MESSAGE (3-way resolved). Omitted by default
    /// (a returned message can be large); always included for a single `--agent` grab.
    #[arg(long = "returned-message")]
    pub returned_message: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl AgentsArgs {
    /// The error string when the (no-op) `--no-subagents` span flag is passed to `agents`, or
    /// `None` when it was not. `agents` has no subagent-span control (it lists subagents AS its
    /// output), so the flag is meaningless — but accepting + rejecting it gives a pointed
    /// message instead of the misleading `allow_hyphen_values` PATH-swallow error.
    #[must_use]
    pub fn span_flag_error(&self) -> Option<&'static str> {
        if self.no_subagents {
            Some(
                "`agents` has no subagent-span flag: it DISCOVERS a session's subagents as its \
                 primary output (there is nothing to span over). Drop `--no-subagents`. To list \
                 a session's subagents run `csift agents @<uuid>`; to scope another subcommand's \
                 subagent span use that subcommand's flag (e.g. `csift files --no-subagents \
                 @<uuid>`).",
            )
        } else {
            None
        }
    }
}

/// Which lifecycle timestamp the `agents` ordering axis uses (`--order-by`) — it sorts the
/// tree AND bounds the `--since`/`--until` window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AgentTimeAxis {
    /// Filter on the TRUE TRIGGER instant — the parent `Task`/`Agent` tool_use timestamp
    /// (the correct "when was it triggered" axis). The DEFAULT. Falls back to the start
    /// timestamp for a subagent whose spawn could not be located.
    #[default]
    Trigger,
    /// Filter on the subagent's START timestamp (first transcript record; lags the
    /// trigger by seconds).
    Start,
    /// Filter on the subagent's COMPLETION timestamp (last transcript record).
    Completion,
}

/// The aggregation detail level for `files`, selected by `--by <summary|dir|file|timeline>`
/// (exactly one is active; default `summary`). Doubles as the clap `ValueEnum` for `--by`,
/// so the value spellings (`summary`/`dir`/`file`/`timeline`) ARE the variants' value names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum FilesDetail {
    /// Coarse TOP-LEVEL-prefix op rollup — buckets each path on its first few directory
    /// segments (so a whole project tree collapses to one row), the smallest output and the
    /// DEFAULT. Strictly coarser than `--by dir` (which keys on the FULL parent dir).
    #[default]
    #[value(name = "summary")]
    Summary,
    /// One row per distinct directory (the FULL parent path) with per-op + distinct-file
    /// counts — finer than `--by summary`'s top-level-prefix rollup.
    #[value(name = "dir")]
    ByDir,
    /// One row per distinct file with per-op counts + first/last touch timestamps.
    #[value(name = "file")]
    ByFile,
    /// Full chronological list, one line per mutation (the verbose, opt-in mode).
    #[value(name = "timeline")]
    Timeline,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Show which FILES and DIRECTORIES a session modified, and when. csift \
        extracts file mutations from a session's transcript (spanning its subagents by \
        default, since OMC fan-out edits happen in subagents):\n  \
          • AUTHORITATIVE  Edit / Write / MultiEdit (input.file_path) + NotebookEdit \
        (input.notebook_path), with create-vs-edit resolved from the paired \
        tool_result (`type:\"create\"` = a new file).\n  \
          • HEURISTIC      Bash file mutations, parsed LEXICALLY from the command \
        string (rm/mv/cp/mkdir/touch/tee/sed -i/git/redirection). Bash carries no path \
        field in its result, so these are best-effort and ALWAYS labelled `(heuristic)`.\n\n\
        DETAIL LEVEL — `--by <summary|dir|file|timeline>` (DEFAULT `summary`; STRICTLY \
        coarsening):\n  \
          --by summary   (DEFAULT) coarse TOP-LEVEL-PREFIX rollup (first few dir segments — a\n              \
                         whole project tree collapses to one row); the smallest output\n  \
          --by dir       one row per distinct directory (the FULL parent path — finer than\n              \
                         summary) with per-op + distinct-file counts + first/last\n  \
          --by file      one row per distinct file (per-op counts + first/last touch)\n  \
          --by timeline  full chronological list, one line per mutation (HEAVY — opt-in only)\n\n\
        PATH FILTERS (optional, combinable with each other AND with --by; matched against \
        each mutation's FULL absolute path):\n  \
          --regex <RE>   keep a path iff the Rust `regex` pattern matches ANYWHERE in it\n  \
          --glob <PAT>   keep a path iff the glob matches the full path (`**` crosses `/`)\n  \
        Both filters AND together (a path must satisfy EVERY supplied filter) and apply \
        BEFORE the --by rollup, so summary/dir/file/timeline AND the Edit-before-Read \
        boundary section all reflect the filtered set; an invalid pattern is a hard error \
        and a filter that removes everything yields the normal empty output.\n\n\
        The TARGET selects the session(s): `@<uuid>` for one, or a project \
        PATH/encoded-dir for every session under it; with neither, all projects are \
        scanned. SUBAGENT SCOPE (default spans subagents, since OMC fan-out edits happen \
        in subagents): `--no-subagents` restricts to the TOP-LEVEL session only.\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based on genuine-user order) \
        is mutually exclusive with `--since`/`--until`. Time bounds accept ISO8601 \
        (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative form (`2h`, `3d`, `90m`, \
        `45s`, `1w`) meaning \"that long ago\" in the system-local timezone; a mutation \
        with no timestamp never falls inside a bounded window.\n\n\
        No silent truncation: skipped malformed lines are counted and surfaced.",
    after_help = "DETAIL LEVEL — `--by <summary|dir|file|timeline>` (default summary, strictly coarsening)\n  \
          --by summary (coarse top-level-prefix rollup) < --by dir (full parent dir) < --by file \
        (per file) < --by timeline (per mutation). PATH FILTERS: --regex <RE> (Rust regex, \
        matches anywhere in the full path) and --glob <PAT> (full-path glob, `**` crosses `/`) \
        are optional, combinable with each other and with --by, and AND together over the \
        full absolute path BEFORE the rollup. SUBAGENT SCOPE: --no-subagents restricts to the \
        top-level session (default spans subagents).\n\n\
        EXAMPLES\n  \
          csift files @<uuid>                          # default summary: coarse top-level-prefix op rollup\n  \
          csift files @<uuid> --by file                # per-file op counts + first/last touch\n  \
          csift files @<uuid> --by file --regex '\\.rs$'   # ONLY .rs paths, per-file\n  \
          csift files @<uuid> --by timeline --glob '**/src/**'  # mutations anywhere under a src/ dir\n  \
          csift files @<uuid> --by timeline --since 2h # full chronological, last 2h (heavy)\n  \
          csift files . --format json --by dir         # machine-readable per-dir rollup\n\n\
        ACID TEST: \"how many distinct gap docs touched / how many /tmp docs created?\"\n  \
          csift files @<uuid> --by file                       # count rows ending in gaps-style docs\n  \
          csift files @<uuid> --by timeline --format json     # filter is_create==true (path under /tmp)\n  \
          (there is NO `op` value `create` — `op` is one of {bash,edit,write,multi_edit,\n   \
           notebook_edit}; create-vs-edit is the SEPARATE `is_create` boolean, so a /tmp-doc\n   \
           CREATE test filters `is_create==true` [optionally AND op in {write,multi_edit,\n   \
           notebook_edit}], never `op==\"create\"`.)\n  \
        (NOTE: BOTH the per-mutation `op` AND `is_create` keys live ONLY in `--by timeline`\n   \
         JSON; a `--by file` row carries per-op COUNT fields (write/edit/bash/multi_edit/\n   \
         notebook_edit/total) + first/last timestamps, NOT `op`/`is_create` — so use\n   \
         `--by timeline` to test create-vs-edit or filter by op.)\n\n\
        JSON SCHEMAS (per --format json)\n  \
          --by timeline : one object per mutation — {session_id, is_subagent, parent_session_id,\n             \
                       path, op, ts_utc, ts_local, turn_index, is_create, heuristic} + a\n             \
                       trailing summary object. (session_id is the transcript's own id — a\n             \
                       top-level uuid, or a bare SUBAGENT hex when is_subagent=true, which is\n             \
                       NOT a re-feedable @<uuid> target; re-feed parent_session_id, always the\n             \
                       owning top-level uuid. heuristic=true ONLY for a bash-derived mutation — a\n             \
                       guessed path/op lexically parsed from a shell command, lower confidence;\n             \
                       false = a definitive Edit/Write/Notebook/MultiEdit tool call with an\n             \
                       exact file_path. Filter heuristic==false for confirmed mutations only.)\n  \
          --by file  : one object per file — {session_id, is_subagent, parent_session_id,\n             \
                       file, write, edit, bash, multi_edit, notebook_edit, total,\n             \
                       distinct_files, first_utc, first_local, last_utc, last_local} + a\n             \
                       trailing summary object. (is_subagent + parent_session_id discriminate\n             \
                       the id-domain on EVERY grouped view, same as --by timeline — a subagent\n             \
                       row's session_id is a bare hex; re-feed parent_session_id.)\n  \
          --by dir / --by summary : the same per-op count keys + the same {session_id,\n             \
                       is_subagent, parent_session_id} discriminators, grouped under a\n             \
                       `dir`/`bucket` key, + a trailing summary {distinct_files,\n             \
                       total_mutations, skipped_lines, detail_level}."
)]
pub struct FilesArgs {
    /// Project target(s) (actual cwd or encoded dir) whose sessions' file mutations to
    /// report, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session (searched across all projects
    /// when no project path is given), so `csift files @<uuid>` works as the EXAMPLES show;
    /// `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to that
    /// transcript. A bare SUBAGENT hex is NOT accepted here — inspect one subagent with
    /// `csift agents --agent <hex>`, or pass the PARENT session uuid as `@<uuid>` (its
    /// subagents are spanned by default).
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Exclude subagent transcripts — report only the top-level `<uuid>.jsonl` session's
    /// mutations. Subagent mutations are attributed by default (built-in Task/Agent-tool, OMC,
    /// and Workflow agents — important because OMC fan-out edits happen in subagents); this is
    /// the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Detail level — `--by <summary|dir|file|timeline>` (DEFAULT `summary`, strictly
    /// coarsening): `summary` = coarse top-level-prefix op rollup (smallest output);
    /// `dir` = one row per distinct directory (full parent path) with per-op +
    /// distinct-file counts + first/last; `file` = one row per distinct file (per-op
    /// counts + first/last touch); `timeline` = full chronological list, one line per
    /// mutation (HEAVY — opt-in only).
    #[arg(long = "by", value_enum, default_value_t = FilesDetail::Summary)]
    pub by: FilesDetail,

    /// Keep only mutated paths whose FULL absolute path matches this Rust `regex` pattern
    /// ANYWHERE (used as-is, no smart-case). Combinable with `--glob` (ANDed) and `--by`;
    /// applied BEFORE the rollup so every view + the boundary section reflect the filtered
    /// set. An invalid pattern is a hard error.
    #[arg(long = "regex", value_name = "RE")]
    pub regex: Option<String>,

    /// Keep only mutated paths whose FULL absolute path matches this glob (`**` crosses
    /// `/`). Combinable with `--regex` (ANDed) and `--by`; applied BEFORE the rollup so
    /// every view + the boundary section reflect the filtered set. An invalid pattern is a
    /// hard error.
    #[arg(long = "glob", value_name = "PAT")]
    pub glob: Option<String>,

    /// Inclusive turn-index range `START..END`, 0-BASED — turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `s·t<n>` header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Mutually exclusive with `--since` /
    /// `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 datetime (`2026-06-01T05:00:00Z`); or a bare date (`2026-06-01`) = LOCAL
    /// MIDNIGHT that day. Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl FilesArgs {
    /// Whether subagent transcripts are spanned (the default). `files` now matches every
    /// other default-on command: `--no-subagents` restricts to the top-level session;
    /// otherwise subagents are spanned. Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }

    /// The active [`FilesDetail`], selected directly by `--by` (clap validates the value
    /// against the `ValueEnum`, defaulting to `Summary`).
    #[must_use]
    pub fn detail(&self) -> FilesDetail {
        self.by
    }
}

/// The reconstruction mode for `recover` (exactly one is active; default restore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverMode {
    /// DEFAULT (no mode flag): hand back the file's FINAL content as RAW restorable bytes —
    /// but ONLY when it is fully recoverable. If the session saw just PART of the file (a
    /// windowed read + a few edits), restore ERRORS rather than emit a holey file, naming what
    /// it can/can't recover and pointing at `--salvage`.
    Restore,
    /// Best-effort, line-numbered FINAL-state fragment of `--file`. Explicit (`--salvage`).
    /// Restore's never-fails sibling: when the session only ever saw PART of the file, this
    /// dumps what DID survive (each known line numbered) with the unrecoverable lines left as
    /// explicit `??? lines A..B unknown` gaps — for a file that is gone, only-partially-read,
    /// and barely-edited, where rewinding isn't the goal and salvaging the surviving proportion
    /// is. Same output as `--at @latest`, framed as the dead-file salvage front door.
    Salvage,
    /// One or more unified-diff patches over the range, segmented at integrity boundaries.
    /// Explicit (`--patches`). The diff/rewind view: shows the changes a session made (compose
    /// with `--since`/`--until`/`--turn-range` to extract a time window — to rewind a file you
    /// still have to an older state, even when the session only partially read it).
    Patches,
    /// The partial, line-numbered point-in-time snapshot as of `--at <WHEN>` (gaps are explicit).
    At,
    /// Coverage / scoping summary — recoverable ranges + boundaries + counts, no dump.
    Coverage,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Reconstruct a single file's history from a session transcript. Unlike \
        `files` (which only ROLLS UP that a file was touched), `recover` rebuilds the \
        file's CONTENT line-by-line from the transcript's Reads / Writes / Edits, in \
        transcript order, with every output line carrying the JSONL LINE NUMBER so an \
        LLM can `Read` the raw jsonl directly.\n\n\
        FIVE MUTUALLY-EXCLUSIVE MODES (exactly one; default = restore):\n  \
          (default, no mode flag) RESTORE the file's FINAL content as RAW restorable \
        bytes — what you'd `> file` to put it back. Restore SUCCEEDS only when the \
        session saw the WHOLE file (a full Read, or it authored the file outright); it \
        then prints the reconstructed content with NO line numbers / banners (clean for \
        piping). When the session observed just PART of the file (a windowed read + a few \
        edits), restore FAILS LOUDLY — it never hands back a holey file — naming the \
        line ranges it CAN and CANNOT recover and pointing at `--salvage`.\n  \
          --salvage   restore's never-fails sibling: the best-effort, line-numbered \
        FINAL-state fragment. Dumps whatever survived (each known line numbered) with the \
        unrecoverable lines left as explicit `??? lines A..B unknown` gaps. For a file \
        that is GONE, only-partially-read, and barely-edited — where rewinding isn't the \
        goal and salvaging the surviving proportion is. Identical output to `--at @latest`, \
        framed as the dead-file salvage front door. (Content invalidated by a \
        `modified since read` boundary is dropped, not shown stale.)\n  \
          --patches   segmented unified-diff history of `--file`. The range \
        is split at INTEGRITY BOUNDARIES — points where reconstruction across them is \
        invalid (a `File has been modified since read` harness error, an `originalFile` \
        that disagrees with the replayed buffer, an external `edited_text_file`, or a \
        heuristic Bash mutation). Each segment + boundary carries its jsonl line / turn \
        / timestamp. This is the CHANGES view: rewind a still-present file to an older \
        state over a `--since`/`--until` window — most useful when ONLY this session \
        touched the file and it did NOT read it in full.\n  \
          --at <WHEN> the PARTIAL, line-numbered \"in the LLM's eyes\" snapshot of \
        `--file` as of <WHEN> (the SAME relative/ISO/bare-date grammar as --since — \
        `45s`/`90m`/`2h`/`3d`/`1w`, ISO8601, bare-date=local-midnight — PLUS the \
        recover-only `@turn:<N>` / `@line:<N>` / `@latest`). Unlike restore, `--at` will \
        dump a PARTIAL snapshot: known lines carry their number; unknown regions are \
        marked `??? lines A..B unknown` — gaps are NEVER fabricated. `--at @latest` is \
        the partial-tolerant sibling of the default restore (final state, holes shown).\n  \
          --coverage  (alias --dry-run) scope a recovery WITHOUT dumping content: which \
        line ranges are recoverable, where the integrity boundaries sit, and per-op \
        counts (reads / edits / writes / bash / external-edits).\n\n\
        The TARGET selects the session(s): `@<uuid>` for one, or a project \
        PATH/encoded-dir for every session under it. `--no-subagents` restricts to the \
        top-level session (OMC fan-out edits happen in subagents, so default ON).\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based genuine-user order) is \
        mutually exclusive with `--since`/`--until` (ISO8601 / relative). `--line-range \
        START..END` further restricts to a 1-based file-line span. `--out <PATH>` writes \
        the reconstructed artifact (restored content / snapshot / concatenated patches) \
        verbatim to a file; in restore mode stdout then stays empty (just a stderr note), \
        in the other modes the summary still prints to stdout.\n\n\
        Reconstruction is NECESSARILY PARTIAL and NEVER fabricates: an unseen line is \
        an explicit gap, an un-anchorable edit is a coverage hole, a Bash touch is a \
        heuristic (not authoritative) boundary. No silent truncation.",
    after_help = "MODE (choose AT MOST ONE; default = restore)\n  \
          --salvage / --patches / --at <WHEN> / --coverage are MUTUALLY EXCLUSIVE — \
        passing two (e.g. `--coverage --patches`) is a parse error. With NONE, the default \
        RESTORE mode applies: it hands back the file's final content, or FAILS (never a \
        partial file) when the session saw only part of it — reach for `--salvage` then. \
        `--file` is REQUIRED for every mode; its value is an absolute path OR the magic \
        `@plan` (the session-bound plan file).\n\n\
        EXAMPLES\n  \
          csift recover @<uuid> --file /abs/app.py                  # DEFAULT: restore final content (or fail if only partial)\n  \
          csift recover @<uuid> --file /abs/app.py --out /abs/app.py # restore straight back onto disk (raw bytes, no banners)\n  \
          csift recover @<uuid> --file /abs/gone.py --salvage       # file is gone + only partly seen: dump what survived, gaps explicit\n  \
          csift recover . --file /abs/PLAN.md --coverage            # scope first: covered ranges + boundaries, no dump\n  \
          csift recover @<uuid> --file /abs/app.py --patches        # segmented unified diffs over the whole session\n  \
          csift recover @<uuid> --file /abs/app.py --since 2h       # patches for the last 2h only (rewind a window)\n  \
          csift recover @<uuid> --file /abs/app.py --at @latest     # partial-tolerant final snapshot (holes shown)\n  \
          csift recover @<uuid> --file /abs/app.py --at @turn:42    # partial snapshot as the LLM saw it at turn 42\n  \
          csift recover @<uuid> --file @plan --out /tmp/plan.md     # reconstruct the session's bound plan (even if deleted)\n  \
          csift recover @<uuid> --file /abs/x.rs --line-range 100..200 --patches   # only patches touching lines 100-200\n\n\
        JSON SCHEMA (per --format json)\n  \
          The default RESTORE mode emits a SINGLE object and no trailer — \
        `{file, complete:true, lines, content}` on success (or, with `--out`, \
        `{file, complete:true, lines, path, wrote}`); a partial file is an ERROR (a stderr \
        message + non-zero exit), never a JSON record. The other four modes emit per-MODE \
        record objects (each tagged by a `type`/`mode`-specific shape — \
        `--patches` emits `{type:\"segment\",…}` + `{type:\"boundary\",…}`; `--coverage` emits \
        a `{covered_ranges, boundaries, events, fragments, recoverable_lines, …}` object; \
        `--at` and `--salvage` emit line/gap records). For those four, EVERY per-record \
        object carries the id-domain discriminators `{session_id, is_subagent, \
        parent_session_id}` (is_subagent flags a bare-hex subagent record; re-feed \
        parent_session_id, never the bare session_id). Records are followed by a UNIFORM \
        trailer `{summary:{file, mode, sessions, skipped_lines}}` that closes every run \
        regardless of mode."
)]
pub struct RecoverArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to reconstruct
    /// from, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, so `csift recover @<uuid>
    /// --file …` works as the EXAMPLES show; `@main`/`@trap:<marker>` resolve the calling session, and a
    /// `*.jsonl` file scopes to that transcript. A bare SUBAGENT hex is NOT accepted here —
    /// inspect one subagent with `csift agents --agent <hex>`.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// The ABSOLUTE file path whose history to reconstruct, matched against the path
    /// exactly as written in the transcript (with a basename-suffix fallback). REQUIRED
    /// for every mode (`--patches` / `--at` / `--coverage`). The MAGIC value `@plan`
    /// (bash-safe, no escaping) instead reconstructs the session-BOUND plan file — the
    /// path from the session's `plan_mode` attachment — so a deleted plan is recoverable
    /// from the transcript alone; locate it without dumping via `csift plan`.
    #[arg(long, value_name = "ABS_PATH")]
    pub file: Option<String>,

    /// Exclude subagent transcripts — reconstruct only from the top-level session. Subagent
    /// transcripts are spanned by default; this is the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Best-effort line-numbered FINAL-state fragment — restore's never-fails sibling. Where
    /// the default restore REFUSES a partial file, `--salvage` dumps whatever survived (known
    /// lines numbered, unrecoverable lines left as explicit `??? lines A..B unknown` gaps). For
    /// a file that is gone, only-partially-read, and barely-edited: salvage the surviving
    /// proportion instead of rewinding. Identical output to `--at @latest`.
    #[arg(long, group = "mode")]
    pub salvage: bool,

    /// Segmented unified-diff history of `--file` — the CHANGES view (rewind a still-present
    /// file over a `--since`/`--until` window; best when only this session touched it and did
    /// not read it whole). NOT the default — with no mode flag, `recover` RESTOREs the final
    /// content instead (and fails loudly rather than emit a partial file).
    #[arg(long, group = "mode")]
    pub patches: bool,

    /// Point-in-time partial snapshot of `--file` as of `<WHEN>`. WHEN uses the SAME
    /// grammar as `--since` — a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw` (`45s`, `90m`, `2h`, `3d`,
    /// `1w`) = that long ago, an ISO8601 datetime (`2026-06-01T05:00:00Z`), or a bare date
    /// (`2026-06-01`) = LOCAL MIDNIGHT — PLUS the recover-only forms `@turn:<N>` (snapshot as
    /// of the first line after genuine-user turn N — discover N from the `s·t<n>` header in
    /// `csift search` text, or `turn_index` in any `--format json` record) and `@line:<N>`
    /// (snapshot as of JSONL TRANSCRIPT line N — the `line_no` shown in this tool's output,
    /// NOT a file line of `--file`; for a 1-based FILE-line span of `--file` use `--line-range`
    /// instead), and `@latest` (the file's FINAL reconstructed state — no cutoff; the clean way
    /// to ask for "its last form" without guessing a timestamp past the last write). A datetime
    /// bound is INCLUSIVE of events AT that instant.
    /// Setting this both selects the mode AND supplies its cutoff.
    #[arg(long, value_name = "WHEN", group = "mode")]
    pub at: Option<String>,

    /// Coverage / scoping summary (no content dump). Alias: `--dry-run`.
    #[arg(long, visible_alias = "dry-run", group = "mode")]
    pub coverage: bool,

    /// Inclusive turn-index range `START..END`, 0-BASED — turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `s·t<n>` header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Mutually exclusive with `--since` /
    /// `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 datetime (`2026-06-01T05:00:00Z`); or a bare date (`2026-06-01`) = LOCAL
    /// MIDNIGHT that day. Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Restrict to a 1-based, inclusive file-line span of `--file` (filters the reconstructed
    /// line space, independent of the turn/time window). Applies in `--patches` / `--at` /
    /// `--coverage`.
    #[arg(long, value_name = "START..END")]
    pub line_range: Option<String>,

    /// Write the reconstructed artifact (snapshot / concatenated patches)
    /// verbatim to this file; the summary still prints to stdout. The DIFFERENCE from stdout:
    /// stdout shortens each over-long line/body to a ~400-char excerpt (a `… (+N chars)`
    /// marker) for readability, whereas `--out` writes every line in full. IGNORED in
    /// `--coverage` mode (a scoping summary — no artifact to write, so no file is created and
    /// a stderr note is printed); it writes for `--patches` / `--at`.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// BATCH MODE: reconstruct MANY files in a single corpus scan. Path to a manifest listing
    /// the target absolute file paths (one per line; blank lines and `#` comments ignored).
    /// Each transcript is parsed ONCE and every listed file it touched is extracted from it —
    /// turning N separate `recover --file` runs (N whole-corpus parses) into one. Requires
    /// `--out-dir`; mutually exclusive with `--file`. Honors `--at`/`--since`/`--until` (default
    /// = the file's final reconstructed state). Each file is written under `--out-dir` mirroring
    /// its absolute path; a `recovery-report.tsv` summarizes per-file status.
    #[arg(long, value_name = "MANIFEST")]
    pub files_from: Option<PathBuf>,

    /// BATCH MODE output directory (required with `--files-from`). Each recovered file is written
    /// to `<DIR>/<abs-path-without-leading-slash>` (created as needed); existing files are not
    /// clobbered unless `--force`.
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,

    /// In batch mode, overwrite an already-present output file (default: skip + report it).
    #[arg(long)]
    pub force: bool,
}

impl RecoverArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }

    /// Resolve the mode flags into the active [`RecoverMode`]. clap's `group = "mode"`
    /// (multiple=false) rejects more than one at parse time, so at most one is set;
    /// none set ⇒ the `Restore` default (full content, or an error if only partial).
    #[must_use]
    pub fn mode(&self) -> RecoverMode {
        if self.at.is_some() {
            RecoverMode::At
        } else if self.coverage {
            RecoverMode::Coverage
        } else if self.patches {
            RecoverMode::Patches
        } else if self.salvage {
            RecoverMode::Salvage
        } else {
            RecoverMode::Restore
        }
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "List and EXTRACT the images a session carries. A pasted/attached image (and a \
        tool-result screenshot) is stored INLINE on a record as a base64 image block, so `image` \
        decodes it straight back to a file — nothing was externalised.\n\n\
        TWO ADDRESSES: `#N` — the session's own `[Image #N]` handle (`turns`/`search` show it \
        inline; an ambiguous `#N` errors with the occurrence list, disambiguate via `--since`/\
        `--turn-range`/`--uuid`); and `L<line>i<n>` — the exact locator (carrying record's JSONL \
        line + ordinal within it).\n\n\
        Default action is to LIST (id · media-type · ~size · time). Pass `--out <PATH>` to EXTRACT: \
        a DIRECTORY keeps each image's SOURCE format (auto-named); a FILE path's extension CONVERTS \
        the single image to that format (`convert in.png out.jpg` idiom). A URL-source image has no \
        inline bytes — it is reported, never fabricated.",
    after_help = "EXAMPLES\n  \
          csift image @<uuid>                             # list every image (deduped)\n  \
          csift image . --format json                     # machine-readable listing\n  \
          csift image @<uuid> --no-subagents --id '#32,#33,#34,#36' --out /tmp/imgs  # re-share by handle\n  \
          csift image @<uuid> --no-subagents --id '#1' --since 1h --out /tmp/imgs    # disambiguate a reused #1\n  \
          csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg        # one image -> a file, convert"
)]
pub struct ImageArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to scan for images,
    /// OR an `@<uuid>` session token. With none, every project is scanned. `@<uuid>` scopes to
    /// one session, `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to
    /// that transcript.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// ADDRESS specific images by the `#N` session handle (`--id #32,#33`) or the exact
    /// `L<line>i<n>` locator (`--id L6812i1`). Repeatable + comma-delimited. Without `--out`,
    /// filters the LISTING to these; with `--out`, extracts only these. Both forms are
    /// per-transcript, so `--id` needs a single transcript in scope (pin with `@<uuid>
    /// --no-subagents`). If a `#N` is AMBIGUOUS (CC reuses `#N` across prompts, so it names >1
    /// distinct image), `image` ERRORS with the occurrence list — disambiguate with the exact
    /// `L<line>i<n>`, or narrow scope via `--since`/`--until` / `--turn-range` / `--uuid`.
    #[arg(long, value_name = "ID", value_delimiter = ',')]
    pub id: Vec<String>,

    /// Lower time bound (ISO8601 or relative `2h`/`3d`/…, system-local) — narrows the image set
    /// so an ambiguous `#N` can resolve in a window where it is unique. A `#N` disambiguator.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as `--since`). A `#N` disambiguator.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Restrict to images in this turn range (`START..END`, 0-based inclusive). A per-transcript
    /// `#N` disambiguator — needs a single transcript in scope.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Restrict to images carried by the record whose uuid starts with this (a `#N`
    /// disambiguator — the uuid shown in the ambiguity error / `--format json`).
    #[arg(long, value_name = "UUID")]
    pub uuid: Option<String>,

    /// EXTRACT — decode the selected image(s) to this PATH. The path's EXTENSION drives the
    /// format (the `convert in.png out.jpg` idiom): a **directory** (or any path WITHOUT an
    /// `png`/`jpg`/`jpeg`/`gif`/`webp` extension) writes each image auto-named
    /// `<session>[-img<N>]-L<line>i<n>.<ext>` in its SOURCE format; a path WITH one of those
    /// extensions writes the (single) selected image to exactly that file, CONVERTING to that
    /// format if it differs from the source (→jpeg lossy q90, →gif dithered palette, →webp lossy
    /// q90; an animated GIF → a still format yields its first frame + a warning). Without
    /// `--out`, `image` only LISTS.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Exclude subagent transcripts — scan only the top-level session. Subagent transcripts are
    /// scanned by default (a tool screenshot may live in one); this is the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Emit JSON (one object per image + a trailing summary) instead of the text listing.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl ImageArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "Locate the Plan-Mode PLAN FILE bound to a session. Claude Code stores plans \
        flat under `~/.claude/plans/<three-words>.md` (a subagent's gets an `-agent-<hex>` \
        suffix); the random name is bound to the session by the `plan_mode` ATTACHMENT the \
        transcript writes on entering Plan Mode. That attachment is the authoritative binding \
        — a session may also Edit/Write OTHER sessions' plan files, but those are not its own \
        plan, so this never path-guesses.\n\n\
        TARGET: a project PATH / encoded-dir (positional), or an `@<uuid>` session token. \
        With NO target, the CALLING session is resolved from `CLAUDE_CODE_SESSION_ID` \
        (like `whoami`) — `csift plan` answers \"what is MY plan file\". Subagents are spanned \
        by default (their own plans surface, flagged); `--no-subagents` restricts to the \
        top-level session.\n\n\
        To DUMP the plan's content (even after it was deleted), feed it to recover: \
        `csift recover @<uuid> --file @plan` reconstructs the bound plan from the \
        transcript's Writes/Edits.",
    after_help = "EXAMPLES\n  \
          csift plan                                  # the calling session's bound plan file\n  \
          csift plan @<uuid>                          # a specific session's plan file\n  \
          csift plan . --format json                  # every session under this project (NDJSON)\n  \
          csift recover @<uuid> --file @plan          # DUMP the bound plan's reconstructed content"
)]
pub struct PlanArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to resolve the bound
    /// plan file for, OR an `@<uuid>` session token (`@main`/`@trap:<marker>` resolve the calling
    /// session; a `*.jsonl` file scopes to that transcript). With no target, the calling
    /// session is resolved from the environment.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// REVERSE lookup: given a PLAN FILE, find the session(s) BOUND to it (the inverse of the
    /// default session→plan direction). Scans the resolved scope — default every project, or
    /// narrow with a PATH target — for transcripts whose `plan_mode` attachment names this exact
    /// plan file, and prints the bound session/subagent id(s). Useful when you have a plan file
    /// (e.g. from `~/.claude/plans/`) and need to know which conversation owns it. The path is
    /// matched by absolute identity (relative / `~` inputs are absolutized first).
    #[arg(long, value_name = "PLAN_FILE")]
    pub reverse: Option<PathBuf>,

    /// Exclude subagent transcripts — resolve only the top-level session's bound plan (forward),
    /// or only top-level bindings (reverse).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Emit NDJSON (one object per resolved plan) instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl PlanArgs {
    /// Subagent span is ON by default; `--no-subagents` restricts to the top-level session.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "Turn-fidelity reconstruction: restore the verbatim user/assistant \
        back-and-forth that a Claude Code COMPACTION SUMMARY clipped. A summary preserves \
        TASK STATE (the 9-section synthesis: intent, file ledger, errors+fixes, plan, next \
        step) in high fidelity but PROVABLY LOSES turn fidelity — its \"All user messages\" \
        section clips ~22 real prose turns to ~17 `...`-truncated bullets, and the assistant \
        side collapses to a SINGLE verbatim quote. `turns` supplements (never replaces) the \
        summary: it re-emits the clipped user phrasings + discarded assistant end-of-turn \
        replies, IN ORIGINAL ORDER, each line carrying the jsonl LINE NUMBER so a consumer \
        can `Read` the raw transcript at the cited line.\n\n\
        SELECTION is recency-first (most-recent turns win the budget, what a resumed agent \
        most needs); the EMITTED document is sorted ascending so it reads as a forward \
        transcript. The backward walk is TRANSPARENT to compaction boundaries — a summary \
        record is a turn MEMBER, never a delimiter — so a 40K-char budget reaches back \
        across multiple boundaries by default (verified: 3 on one real sample, 2 on \
        another). `--max-compactions` only caps how far.\n\n\
        BUDGET (`--budget`, default 40000) bounds EACH session's reconstruction in chars \
        (or tokens via `--budget-unit tokens`, ≈4 chars/token) — it is applied PER session \
        in scope. `turns` defaults to the TOP-LEVEL thread only, so a bare-uuid run realizes \
        just `budget` chars; with `--include-subagents` a target that spans S subagents \
        realizes up to `budget × (1 + S)` chars total (a scope banner surfaces the \
        multiplier). `--round-trip-fraction` \
        (default 0.5) is a HARD FLOOR: that fraction of the budget can ONLY be spent on \
        COMPLETE round-trips (user → [N tool calls] → assistant EOT), never on user-only / \
        assistant-only fragments — without it an assistant-heavy tail recovers ZERO human \
        turns. Over-cap units are MIDDLE-truncated (head+tail kept) with an explicit \
        `… [+K chars, L lines elided] …` marker; the assistant head is larger than the \
        user head (its prose front-loads context, back-loads the decision). Nothing is \
        ever fabricated or silently dropped.\n\n\
        AGENT MESSAGES (`--agent-msgs`, default `eot-only` = non-breaking): a single \
        genuine-user turn can own a LONG run of agent messages (a debugging/build chain \
        the model narrates) that the summary clips to one §9 quote. `eot-only` restores \
        just the last (today's behavior). `rich` ALSO restores the first/middle messages \
        that carry important info — a count, a commit hash, a `file.rs:NNN` ref, backtick \
        code, or a finding/decision lexeme, or that are clearly long — collapsing pure \
        \"let me look into this\" declarations into a `△ L… [X agent messages, Y tool \
        calls, Z failed]` placeholder (only on runs longer than `--agent-run-threshold`, \
        default 6). `all` keeps every agent message. `--profile heavy|light` bundles the \
        thresholds; explicit threshold flags override the profile.\n\n\
        DEDUP: a turn the NEWEST summary already quotes verbatim is flagged `(also in \
        summary)` and DEMOTED (selected only after non-dup turns) — never silently dropped \
        (a false positive must not lose a real turn).\n\n\
        AUTOMATION TRIGGERS: a machine `<task-notification>` (a background-command / \
        workflow / spawned-agent / monitor-tick COMPLETION pulse) OPENS a turn just like a \
        human message, and is rendered as a parsed attribution label `[<kind> <task-id> \
        <status>] <summary>` (kind = background-command | workflow | agent | monitor | task) \
        instead of the raw XML. The header reports the human/automation split (`selected N \
        user (M automation triggers) + …`). These pulses are EXCLUDED from the \
        `--round-trip-fraction` HARD FLOOR (that lane is reserved for human exchanges) but \
        can still be selected as Phase-2 fill.\n\n\
        BUDGET FAN-OUT: `--budget` is applied PER session in scope. `turns` defaults to the \
        top-level thread only, so a bare-uuid run is a single session at `budget` chars. With \
        `--include-subagents` the target also spans that session's subagents, so the realized \
        output is `budget × (sessions in scope)`; a top-of-output scope banner then names the \
        TRUE scope (all discovered top-level + subagent sessions), how many rendered within \
        budget, and the realized multiplier.\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based genuine-user order) is \
        mutually exclusive with `--since`/`--until` (ISO8601 / relative `2h`,`3d`,…). NOTE \
        `turns` TEXT prints `L<line>` per unit, NOT a turn marker — to pick a value for \
        `--turn-range` read the index from `csift search` text (`s·t<n>` header) or this \
        command's own `--format json` (`turn_index`). \
        `--out <PATH>` captures the SAME rendered reconstruction that prints to stdout into a \
        file (byte-identical — turns does NOT line-truncate stdout, so `--out` differs only in \
        going to a file rather than the terminal; over-cap units are middle-truncated with an \
        explicit `… [+K chars, L lines elided] …` marker in BOTH). For UN-truncated unit \
        bodies use `--format json`, which emits one VERBATIM object per unit (full `text`, no \
        per-unit cap) plus interleaved compaction-boundary records.",
    after_help = "EXAMPLES\n  \
          csift turns .                                     # default 40K-char reconstruction, top-level thread\n  \
          csift turns @<uuid> --budget 12000                # recover JUST my thread (~10-15K, no fan-out)\n  \
          csift turns @<uuid> --include-subagents --format json  # ALSO span subagents (budget × N, line-numbered)\n  \
          csift turns @<uuid> --round-trip-fraction 0.6     # weight harder toward complete round-trips\n  \
          csift turns . --budget 40000 --out /tmp/turns.md  # full reconstruction to a file\n  \
          csift turns . --budget 36000 --window 9000 --slice 1   # 1st ≤9000-char chunk for a SessionStart hook (fan slices 1..4)\n  \
          csift turns @<uuid> --budget 8000 --max-compactions 1  # stay within one compaction boundary\n  \
          csift turns @<uuid> --agent-msgs eot-only         # force the old single-EOT (last-message-only) output\n  \
          csift turns @<uuid> --agent-rich-min-chars 200    # default mode, lower bar → keep more first/middle messages\n  \
          csift turns @<uuid> --profile heavy               # lower thresholds (max fidelity)\n  \
          csift turns @<uuid> --agent-msgs all              # every agent message, no filtering\n\n\
        AUTOMATION TRIGGERS\n  \
          A machine `<task-notification>` (a background-command / workflow / spawned-agent /\n  \
          monitor-tick COMPLETION pulse Claude Code injects as a `type:\"user\"` record) OPENS\n  \
          a turn like a human message, so it appears in the reconstruction. It renders as a\n  \
          PARSED attribution label `[<kind> <task-id> <status>] <summary>` (kind =\n  \
          background-command | workflow | agent | monitor | task, read from the summary) —\n  \
          never the raw `<task-id>` / `<output-file>` XML. The header reports the\n  \
          human/automation split, e.g. `selected 16 user (3 automation triggers) + 58\n  \
          assistant units`. These pulses are EXCLUDED from the `--round-trip-fraction` HARD\n  \
          FLOOR (reserved for human exchanges) but can still be selected as Phase-2 fill.\n  \
          LIMITATION: only `<task-notification>` COMPLETION pulses are segmented + attributed.\n  \
          The isMeta ScheduleWakeup WAKEUP-TICK *prompts* (a monitor/cron tick FIRING, e.g.\n  \
          `MONITOR TICK`) bypass this — they are isMeta records that do NOT open a\n  \
          turn, so the agent run a tick triggers currently groups under the PRECEDING\n  \
          genuine-user turn (not yet split into per-tick segments). In an automation-heavy\n  \
          monitor session this lumps the dominant tick-driven work onto one human turn.\n\n\
        BUDGET FAN-OUT\n  \
          `--budget` is PER session in scope. `turns` defaults to the TOP-LEVEL thread only, so\n  \
          a bare-uuid run is one session at `budget` chars. Add `--include-subagents` to also\n  \
          span that session's subagents — the realized output is then budget × (sessions in\n  \
          scope), and a top-of-output `scope` line names the TRUE scope (all top-level +\n  \
          subagent sessions discovered), how many rendered within budget, and the multiplier.\n  \
          A targeted top-level session that does not fit `budget` is reported with an explicit\n  \
          `skipped — needs ≥ N chars` note, never silently dropped.\n\n\
        JSON SCHEMA (per --format json)\n  \
          A leading `{kind:\"session_header\", sessions_in_scope, sessions_rendered,\n  \
          top_level_sessions, subagent_sessions, budget_chars, budget_is_per_session,\n  \
          max_total_chars, selected_user, automation_triggers, automation_by_kind,\n  \
          automation_in_scope_by_kind}` object —\n  \
          `sessions_in_scope` is the TRUE scope (every discovered session), `sessions_rendered`\n  \
          is how many fit the budget, the top_level/subagent split is over ALL in scope,\n  \
          `budget_chars`/`max_total_chars` are ALWAYS in CHARS (a `--budget-unit tokens` budget\n  \
          is pre-multiplied ×4, so they read 4× `--budget` under tokens mode),\n  \
          `automation_by_kind` breaks the SELECTED `automation_triggers` total down per class\n  \
          ({background-command,agent,workflow,monitor,task}), and `automation_in_scope_by_kind`\n  \
          is the SAME breakdown over EVERY in-scope automation pulse REGARDLESS of budget\n  \
          selection — so a monitor-heavy session is never read as `monitor:0` just because the\n  \
          recency window selected none of its deep pulses (compare the two to see how much\n  \
          automation exists vs was rendered). Then one object PER emitted unit:\n  \
          {session_id, is_subagent, parent_session_id, turn_index, line_no, role, ts_utc,\n  \
          ts_local, tool_calls, full_chars, rendered_chars, truncated, elided_chars,\n  \
          elided_lines, also_in_summary, compactions_before, text, is_automation} (is_subagent\n  \
          flags a bare-hex subagent unit; re-feed parent_session_id, never the bare session_id);\n  \
          an automation USER unit additionally\n  \
          carries {trigger_kind, task_id, status, event} (event = the Monitor/ScheduleWakeup\n  \
          outcome tag, null on non-monitor pulses). Boundary objects are tagged\n  \
          {kind:\"compaction_boundary\",…} / {kind:\"collapsed_agents\",…}; a trailing\n  \
          {kind:\"skipped_lines\", skipped_lines:N} ALWAYS closes the stream (even when N=0),\n  \
          so a JSONL consumer can detect end-of-stream. NOTE the terminator SHAPE is NOT yet\n  \
          uniform tool-wide: turns is the ONLY command whose terminator is `kind`-tagged.\n  \
          search closes with `{matched,dropped_by_cap,skipped_lines}`, files with\n  \
          `{total_mutations,distinct_files,skipped_lines,detail_level}`, recover with\n  \
          `{summary:{…}}` (nested) — all THREE untagged — and list/agents emit NO terminator\n  \
          (end-of-stream = EOF). Key any portable EOF check on the per-command shape, not on a\n  \
          shared `kind:\"skipped_lines\"` (a tool-wide `{kind:\"end\",…}` is a planned change)."
)]
pub struct TurnsArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to reconstruct
    /// turns from, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, so `csift turns @<uuid>` works
    /// as the EXAMPLES show; `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file
    /// scopes to that transcript. A bare SUBAGENT hex is NOT accepted here — inspect one subagent
    /// with `csift agents --agent <hex>`. NOTE: `turns` defaults to the TOP-LEVEL thread only (the
    /// single-thread recovery use case), so a bare `csift turns @<uuid>` reconstructs just that
    /// conversation; add `--include-subagents` for the rare cross-fan-out reconstruction
    /// (`--budget` is then applied PER session in scope — see `--budget`).
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Character (default) or token budget applied PER session in scope. Each session's
    /// reconstruction is bounded by this; `turns` defaults to the top-level thread only, so a
    /// bare-uuid run realizes just `budget` chars. With `--include-subagents` the realized
    /// total is `budget × (sessions in scope)` and a scope banner surfaces the multiplier.
    /// Default 40000.
    #[arg(long, value_name = "N", default_value_t = 40000)]
    pub budget: usize,

    /// Interpret `--budget` as chars (default) or tokens (≈4 chars/token heuristic).
    /// NOTE: the DEFAULT `--budget 40000` is read as 40000 TOKENS ≈ 160000 CHARS the moment
    /// you pass `--budget-unit tokens` WITHOUT also lowering `--budget` — a 4× larger output.
    /// Pass an explicit `--budget` when flipping to tokens (e.g. `--budget 10000 --budget-unit
    /// tokens` ≈ 40000 chars). The JSON `budget_chars`/`max_total_chars` are ALWAYS in CHARS
    /// (a token budget is pre-multiplied ×4), so under tokens mode they read 4× the `--budget`.
    #[arg(long = "budget-unit", value_enum, default_value_t = BudgetUnit::Chars)]
    pub budget_unit: BudgetUnit,

    /// Fraction of the budget RESERVED to guarantee complete round-trips (user →
    /// [N tool calls] → assistant EOT), not user messages alone. A hard floor.
    /// Default 0.5; must be in the open interval (0.0, 1.0).
    #[arg(long = "round-trip-fraction", value_name = "F", default_value_t = 0.5)]
    pub round_trip_fraction: f64,

    /// Also span each in-scope session's SUBAGENT transcripts (built-in Task/Agent-tool,
    /// OMC, and Workflow agents) under `subagents/**`. Default OFF for `turns` (UNLIKE
    /// files/search): `turns` is a SINGLE-THREAD recovery tool and `--budget` MULTIPLIES per
    /// session in scope, so spanning hundreds of unrelated fan-out subagents by default would
    /// bury the thread you asked to restore under megabytes of noise. Opt in with
    /// `--include-subagents` only for the rare cross-fan-out reconstruction.
    #[arg(
        long = "include-subagents",
        overrides_with = "no_subagents",
        default_value_t = false
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts — reconstruct only from the top-level session. This is
    /// already the `turns` DEFAULT; the flag is kept for symmetry with the other subcommands
    /// and to explicitly cancel an earlier `--include-subagents` (last flag wins).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Stop walking back after crossing N compaction boundaries (0 = unlimited;
    /// default 0). A guard, not a target.
    #[arg(long = "max-compactions", value_name = "N", default_value_t = 0)]
    pub max_compactions: usize,

    /// Per-turn agent-message policy (MASTER switch for the multi-agent-message model).
    /// `longest` (DEFAULT) keeps the LONGEST agent message — the substantive Rich Response,
    /// which is frequently a MIDDLE message, not the last ~50-char throwaway wrap-up that
    /// the old `agents.last()` default silently kept — PLUS the first message when it is
    /// substantive (`>= --agent-rich-min-chars`) PLUS each rich middle (a number, commit
    /// hash, file:line, backtick code, or finding/decision lexeme, or clearly long),
    /// collapsing the rest into a placeholder. `eot-only` forces the old single-EOT
    /// behavior (only each turn's last agent message — byte-identical to the pre-feature
    /// output). `rich` keeps the last always + the first by position privilege + each
    /// non-droppable middle, only on a long run (`> --agent-run-threshold` agent messages).
    /// `all` keeps every agent message.
    #[arg(long = "agent-msgs", value_enum, default_value_t = crate::turns::AgentMsgMode::Longest)]
    pub agent_msgs: crate::turns::AgentMsgMode,

    /// Only richness-filter a turn whose agent-message count EXCEEDS this (default 6;
    /// short runs keep every message verbatim). Only consulted in `--agent-msgs rich`.
    #[arg(long = "agent-run-threshold", value_name = "N", default_value_t = 6)]
    pub agent_run_threshold: usize,

    /// An agent message at least this many chars is kept on length alone (default 280 ≈
    /// 1.5× the measured 184-char median middle). Consulted in `--agent-msgs longest`
    /// (gates the "keep the first if substantive" + the rich middles) AND `rich`.
    #[arg(long = "agent-rich-min-chars", value_name = "N", default_value_t = 280)]
    pub agent_rich_min_chars: usize,

    /// A signal-less intent-verb-opening agent message SHORTER than this is droppable;
    /// at/above it is kept (default 200 — the pure-declaration band). Only consulted in
    /// `--agent-msgs rich`.
    #[arg(
        long = "agent-declaration-max-chars",
        value_name = "N",
        default_value_t = 200
    )]
    pub agent_declaration_max_chars: usize,

    /// Keep a turn's FIRST agent message by position privilege (first-matters — the
    /// opening message often states the plan or an early finding). `--no-keep-first` drops
    /// the privilege and decides the first as a MIDDLE (kept unless it is a proven pure
    /// declaration; a rich first still survives). ONLY TAKES EFFECT in `--agent-msgs rich`.
    /// In the DEFAULT `longest` mode, first-message retention is governed by
    /// `--agent-rich-min-chars` (keep-the-first-if-substantive), NOT this flag — so
    /// `--keep-first`/`--no-keep-first` are no-ops there. (The `default_value_t = true`
    /// only sets the privilege value consulted by `rich`; it does not make the flag active
    /// in `longest`.)
    #[arg(
        long = "keep-first",
        overrides_with = "no_keep_first",
        default_value_t = true
    )]
    pub keep_first: bool,

    /// Drop the first-message position privilege (see `--keep-first`). Overrides it.
    #[arg(long = "no-keep-first")]
    pub no_keep_first: bool,

    /// Convenience threshold bundle, applied BEFORE the individual flags (so an explicit
    /// flag overrides the profile). `heavy` = maximal fidelity (threshold 4, rich-min 200,
    /// declaration-max 140); `light` = lean (threshold 8, rich-min 360, declaration-max
    /// 240). Neither changes the master `--agent-msgs` mode — with no `--agent-msgs` the
    /// mode stays the default `longest` (with the profile's thresholds); add `--agent-msgs
    /// rich` to also switch the keep-set.
    #[arg(long = "profile", value_enum)]
    pub profile: Option<crate::turns::Profile>,

    /// Inclusive turn-index range `START..END`, 0-BASED — turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `s·t<n>` header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Mutually exclusive with `--since` /
    /// `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 datetime (`2026-06-01T05:00:00Z`); or a bare date (`2026-06-01`) = LOCAL
    /// MIDNIGHT that day. Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Capture the SAME rendered reconstruction that prints to stdout into this file
    /// (byte-identical — turns does NOT truncate stdout; over-cap units are middle-truncated
    /// with a `… [+K chars, L lines elided] …` marker in BOTH). The summary still prints to
    /// stdout. For UN-truncated unit bodies use `--format json` (full verbatim `text`).
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// CHUNKED OUTPUT: paginate the recovered DOCUMENT (the verbatim turns — the same content
    /// `--out` writes) into ≤`--window`-CHARACTER chunks and print ONLY the Nth chunk (1-based)
    /// to stdout, with NO operational chrome (scope banner / SESSION header / footer). Built for
    /// fanning a >10K reconstruction across several SessionStart hooks: Claude Code caps EACH
    /// hook's `additionalContext` at 10,000 CHARACTERS (over-cap is replaced by a file-path +
    /// short preview — i.e. the body is effectively LOST to the model), so one hook per slice
    /// keeps every injected chunk under the wall. Slicing is DETERMINISTIC (same session +
    /// budget ⇒ identical chunk boundaries), so N independent hooks can each request their own
    /// slice; the lock/ordering lives in the hook shell. An out-of-range N prints nothing (exit
    /// 0) — surplus hooks simply inject nothing. Text format only; not combinable with `--out`.
    #[arg(long, value_name = "N")]
    pub slice: Option<usize>,

    /// Chunk size for `--slice`, in CHARACTERS (Unicode scalars — the unit Claude Code's
    /// 10,000-char `additionalContext` cap counts, so a CJK-heavy document is NOT 3×
    /// over-counted the way a byte budget would). Default 10000 = the cap; pass a little under
    /// (e.g. `--window 9000`) to leave headroom for any wrapper text the hook adds around the
    /// chunk. Lines are packed greedily up to the window at LINE boundaries; a single line
    /// longer than the window is hard-split on a char boundary so no chunk ever exceeds it.
    /// Ignored without `--slice`.
    #[arg(long, value_name = "N", default_value_t = 10000)]
    pub window: usize,

    /// FIXED-FLEET mode: pin the reconstruction to AT MOST N slices of `--window` chars each,
    /// instead of letting `--budget` decide a VARIABLE number of chunks. A hook fleet is a fixed
    /// set of registered `SessionStart` hooks, so the slice COUNT — not the char budget — is the
    /// hard constraint: it must NOT drift to 5/6/7 as the turns grow. With `--slices N`, csift
    /// fills the N newest-first slices with WHOLE turns (the per-role 600/900 caps are dropped; a
    /// turn is ellipsized ONLY if it alone exceeds one window), and DISCARDS the oldest turns that
    /// don't fit — so the emitted count is ALWAYS ≤N no matter how big the turns are. Requires
    /// `--slice i` to pick which chunk; the budget becomes N×`--window`, so `--budget` is ignored.
    /// Without `--slices`, `--slice` keeps its legacy budget-driven, variable-chunk-count behavior.
    #[arg(long, value_name = "N")]
    pub slices: Option<usize>,
}

impl TurnsArgs {
    /// Resolve the include/exclude flags into a single decision. UNLIKE the other
    /// subcommands, `turns` defaults to TOP-LEVEL-ONLY (`include_subagents` defaults false):
    /// spanning is opt-in via `--include-subagents`, and a trailing `--no-subagents` still
    /// forces it off. So a `csift turns @<uuid>` reconstructs just that one thread.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.include_subagents && !self.no_subagents
    }

    /// Resolve the agent-message policy into a [`crate::turns::RichnessCfg`]. A `--profile`
    /// (if given) seeds the threshold baseline FIRST; then every EXPLICITLY-passed flag
    /// overrides the profile (clap exposes whether a flag was user-set via its default
    /// matching, so we apply the explicit value unconditionally — a flag left at its
    /// documented default coincides with no-override, the intended behavior). The master
    /// `--agent-msgs` mode is honored as-is; the default `eot-only` reproduces today's
    /// single-EOT output regardless of the thresholds.
    #[must_use]
    pub fn richness_cfg(&self) -> crate::turns::RichnessCfg {
        use crate::turns::{Profile, RichnessCfg};
        // Profile baseline (or the plain defaults when no profile is given).
        let mut cfg = match self.profile {
            Some(Profile::Heavy) => RichnessCfg {
                mode: self.agent_msgs,
                run_threshold: 4,
                rich_min_chars: 200,
                declaration_max_chars: 140,
                keep_first: self.keep_first(),
            },
            Some(Profile::Light) => RichnessCfg {
                mode: self.agent_msgs,
                run_threshold: 8,
                rich_min_chars: 360,
                declaration_max_chars: 240,
                keep_first: self.keep_first(),
            },
            None => RichnessCfg {
                mode: self.agent_msgs,
                run_threshold: self.agent_run_threshold,
                rich_min_chars: self.agent_rich_min_chars,
                declaration_max_chars: self.agent_declaration_max_chars,
                keep_first: self.keep_first(),
            },
        };
        // Explicit flags WIN over the profile. clap fills these with their declared
        // defaults when the user omits them; we treat a value that differs from the
        // documented default as an explicit override (a user who passes the default value
        // gets the same result, which is correct). This keeps "profile sets baseline,
        // flag wins" without needing clap's raw ArgMatches.
        if self.profile.is_some() {
            if self.agent_run_threshold != 6 {
                cfg.run_threshold = self.agent_run_threshold;
            }
            if self.agent_rich_min_chars != 280 {
                cfg.rich_min_chars = self.agent_rich_min_chars;
            }
            if self.agent_declaration_max_chars != 200 {
                cfg.declaration_max_chars = self.agent_declaration_max_chars;
            }
        }
        cfg
    }

    /// Resolve the `--keep-first` / `--no-keep-first` pair (default keep-first).
    #[must_use]
    fn keep_first(&self) -> bool {
        !self.no_keep_first
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "Identify the CALLING Claude Code session, false-positive-safe.\n\n\
        Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash-tool environment, \
        and its value equals the calling session's own jsonl basename exactly. That \
        canonical var is the signal csift trusts: per-session, version-independent, \
        survives bash nesting, zero false positives. When it is absent, csift falls back \
        to `CODEX_COMPANION_SESSION_ID` (the alias the Codex companion plugin sets) before \
        giving up. (The exact var names are matched — never a loose /session/i regex, which \
        would false-positive on macOS's SECURITYSESSIONID.)\n\n\
        When NEITHER var is set (an old CC build, or running outside CC/Codex), whoami \
        does NOT guess — it errors with guidance to pass an explicit `@<uuid>` target. \
        Most-recent-mtime and process-tree walking are FORBIDDEN: many CC sessions \
        may be live at once, so mtime is almost always wrong. It is acceptable for \
        whoami to often say \"ambiguous, pass `@<uuid>`\".\n\n\
        SUBAGENT CAVEAT: which session $CLAUDE_CODE_SESSION_ID names depends on HOW the \
        subagent was spawned. In a built-in Task/Agent SUBAGENT it is the SUBAGENT's OWN id \
        (so `whoami` identifies the subagent, not the main session). In an \
        ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) it is the PARENT \
        session's id (so `whoami` resolves the ROOT, not a subagent). Do NOT assume which — \
        disambiguate via the recovery: feed the resolved id to `csift agents --agent <id> \
        --format json`; if it returns the node, read `parent_session_id` for the ROOT (one \
        call); if it errors `no subagent matched`, the id is ALREADY top-level — use it \
        directly. (Or scan `csift agents .` / `csift list .` on the project PATH and find the \
        parent uuid.) The ENV form's JSON carries `{session_id, path}`; the `@trap:<marker>` form \
        instead returns the UPSTREAM CHAIN `{chain:[{session_id, is_subagent, parent_session_id, \
        depth, path}, …]}` (self first, top-level root last) — so a subagent reads is_subagent / \
        parent_session_id directly, no `agents` round-trip.\n\n\
        FLAG NOTE: whoami's only positional is the optional SELF token `@trap:<marker>` / `@main`; \
        the `path` line is ALWAYS printed. Every OTHER session-operating subcommand takes a \
        general POSITIONAL `[PATH]...` / `@`-token; there is no target FLAG.",
    after_help = "SESSION-ID SOURCE\n  \
          The canonical env var CLAUDE_CODE_SESSION_ID (CC sets it per Bash-tool process; \
        its value IS the calling session's jsonl basename). If absent, csift falls back to \
        CODEX_COMPANION_SESSION_ID (the Codex companion plugin's alias). If NEITHER is set, \
        whoami errors with guidance to pass an `@<uuid>` target — it never guesses by mtime.\n\n\
        SUBAGENT CAVEAT\n  \
          What the env var names depends on HOW the subagent was spawned. In a built-in \
        Task/Agent SUBAGENT it holds the SUBAGENT's OWN id (so `whoami` identifies the \
        subagent). In an ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) it \
        holds the PARENT session id (so `whoami` resolves the ROOT instead). For a DEFINITIVE, \
        env-INDEPENDENT answer use `whoami @trap:<marker>`: you embed a one-shot literal marker in \
        that very command and csift maps it to your subagent hex and walks the UPSTREAM CHAIN up to \
        the top-level root (the @trap form's JSON is `{chain:[{session_id, is_subagent, \
        parent_session_id, depth, path}, …]}`). \
        WITHOUT @trap, don't assume which id the env gave — feed it to `csift agents --agent <id> \
        --format json`; a returned node → read `parent_session_id` for the ROOT; `no subagent \
        matched` → the id is ALREADY top-level. Plain `whoami` JSON carries only {session_id, \
        path} — from the env alone it canNOT know is_subagent / parent_session_id (that is exactly \
        what `@trap` resolves).\n\n\
        FLAG NOTE\n  \
          whoami takes an OPTIONAL positional SELF target — `@trap:<marker>` or `@main` ONLY; the \
        `path` line is ALWAYS printed. Every OTHER session-operating subcommand takes a general \
        POSITIONAL [PATH]... / `@`-token. There is no target flag anywhere.\n\n\
        EXAMPLES\n  \
          csift whoami                  # the calling session's uuid + its jsonl path\n  \
          csift whoami --format json    # {\"session_id\":\"…\",\"path\":\"…\"}\n  \
          csift whoami @trap:<invent-a-fresh-3word-4digit-marker>   # which SUBAGENT am I? -> upstream chain (self -> ... -> top-level root); the marker is YOURS to invent, never a copied literal\n  \
          # FALLBACK (no @trap) — map this subagent's bare hex to its ROOT (read parent_session_id):\n  \
          csift agents --agent \"$(csift whoami --format json | jq -r .session_id)\" --format json\n  \
          # …then scope the whole conversation with that parent uuid:\n  \
          csift search \"<pattern>\" \"@$(csift agents --agent \"$(csift whoami --format json | jq -r .session_id)\" --format json | jq -r .parent_session_id)\""
)]
pub struct WhoamiArgs {
    /// Optional SELF target — `@trap:<marker>` or `@main` (nothing else). With NO target, identify
    /// the calling session from `$CLAUDE_CODE_SESSION_ID` (the historical behavior; `@main` is its
    /// explicit spelling). `@trap:<marker>` answers "which SUBAGENT am I?": a running subagent
    /// (whose own id CC withholds from the env) embeds a unique, literal, one-shot marker in THIS
    /// very csift command and csift maps it to the subagent's bare hex and walks the UPSTREAM
    /// ancestry CHAIN up to the top-level root (the walk-UP mirror of `agents`' walk-DOWN) —
    /// env-INDEPENDENT, so it is reliable for a built-in Task AND a workflow subagent (whose env id
    /// is the PARENT). To inspect a DIFFERENT session, use `list`/`agents`, not `whoami`.
    #[arg(value_name = "SELF")]
    pub self_target: Option<String>,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// A canonical 8-4-4-4-12 session uuid for the bare-uuid-positional routing tests.
    const SESS_UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    /// clap's own internal consistency check — catches duplicate flags, bad
    /// `overrides_with` targets, malformed value parsers at build time.
    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn whoami_accepts_trap_and_main_self_target_else_none() {
        // `@trap:<marker>` and `@main` land in whoami's optional positional; bare whoami = None.
        let cli =
            parse(&["csift", "whoami", "@trap:CrimsonWillowFen5180"]).expect("whoami @trap parses");
        match cli.command {
            Command::Whoami(a) => {
                assert_eq!(a.self_target.as_deref(), Some("@trap:CrimsonWillowFen5180"));
            }
            _ => panic!("expected whoami"),
        }
        let cli = parse(&["csift", "whoami", "@main"]).expect("whoami @main parses");
        match cli.command {
            Command::Whoami(a) => assert_eq!(a.self_target.as_deref(), Some("@main")),
            _ => panic!("expected whoami"),
        }
        let cli = parse(&["csift", "whoami"]).expect("bare whoami parses");
        match cli.command {
            Command::Whoami(a) => assert!(a.self_target.is_none()),
            _ => panic!("expected whoami"),
        }
    }

    /// Parse through the SAME normalization pass the real entrypoint uses
    /// (`parse_argv` → `normalize_argv` → clap), so the flag-ordering fix is what we
    /// actually test, not bare clap.
    fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
        Cli::try_parse_from(normalize_argv(owned))
    }

    // ── The reported flag-ordering bug (CONFIRMED against target/debug/csift) ──

    #[test]
    fn list_format_json_after_encoded_positional_now_works() {
        // The exact repro: `--format json` AFTER a leading-`-` encoded positional
        // used to be swallowed as two extra PATH values. It must now parse as the
        // format flag, with the positional intact.
        let cli = parse(&[
            "csift",
            "list",
            "-Users-testuser-Projects-widget-app-prototype",
            "--format",
            "json",
        ])
        .expect("flag after encoded positional must parse");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(
                    a.paths[0].to_string_lossy(),
                    "-Users-testuser-Projects-widget-app-prototype"
                );
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn list_format_json_before_positional_still_works() {
        // The old workaround ordering must keep working (backward-compatible).
        let cli = parse(&[
            "csift",
            "list",
            "--format",
            "json",
            "-Users-testuser-Projects-foo",
        ])
        .expect("flag before positional still parses");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn search_flag_after_encoded_target_parses() {
        // A `--format json` flag is hoisted ahead of a leading-`-` encoded POSITIONAL target
        // in any position (the allow_hyphen_values greedy-absorb fix).
        let cli = parse(&[
            "csift",
            "search",
            "carry",
            "-Users-testuser-Projects-foo",
            "--format",
            "json",
        ])
        .expect("search <encoded> then --format must parse");
        match cli.command {
            Command::Search(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1, "the encoded target is a positional");
                assert_eq!(a.targets().len(), 1);
                assert_eq!(a.pattern, "carry");
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn search_positional_path_like_siblings() {
        // The fix: `csift search PATTERN .` — a POSITIONAL path, the SAME surface every
        // sibling subcommand uses.
        let cli = parse(&["csift", "search", "carry", "."]).expect("positional PATH must parse");
        match cli.command {
            Command::Search(a) => {
                assert_eq!(a.pattern, "carry");
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.paths[0].to_string_lossy(), ".");
                assert_eq!(a.targets().len(), 1);
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn bare_encoded_token_still_accepted_as_target() {
        // The leading-`-` tolerance must remain for genuine encoded tokens.
        let cli = parse(&["csift", "list", "-a--claude-b"]).expect("encoded token parses");
        match cli.command {
            Command::List(a) => assert_eq!(a.paths[0].to_string_lossy(), "-a--claude-b"),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn real_path_target_still_accepted() {
        let cli =
            parse(&["csift", "list", "/Users/testuser/Projects/foo"]).expect("real path parses");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.paths[0].to_string_lossy(), "/Users/testuser/Projects/foo")
            }
            _ => panic!("expected list"),
        }
    }

    // ── Value parser accepts real targets, REJECTS a `--`-leading unknown flag ──

    #[test]
    fn parse_project_target_accepts_real_targets_rejects_double_dash() {
        // Genuine targets (single-dash encoded token, real path, `.`) parse.
        assert!(parse_project_target("-Users-testuser-Projects-foo").is_ok());
        assert!(parse_project_target("/Users/testuser/Projects/foo").is_ok());
        assert!(parse_project_target(".").is_ok());
        assert!(parse_project_target("-a--claude-b").is_ok());
        // A `--`-leading token can NEVER be a real encoded dir (those start with ONE `-`),
        // so it is rejected → clap reports "unexpected argument" instead of the misleading
        // "no project dir named --xxx". A single `-` token still parses (encoded target).
        let err = parse_project_target("--by-fil").unwrap_err();
        assert!(err.contains("unexpected argument"), "got: {err}");
        assert!(parse_project_target("-singledash").is_ok());
    }

    #[test]
    fn unknown_flag_is_rejected_not_swallowed_as_target() {
        // The unknown-flag-masking fix: a `--`-prefixed unknown/typo token reaching the
        // positional is rejected so clap surfaces it, on EVERY scope-operating subcommand.
        for argv in [
            ["csift", "files", "--by-fil"].as_slice(),
            ["csift", "turns", "--budgett"].as_slice(),
            ["csift", "list", "--by-fil"].as_slice(),
            ["csift", "agents", "--bogus"].as_slice(),
        ] {
            let err = parse(argv).expect_err("unknown flag must error");
            let msg = err.to_string();
            assert!(
                !msg.contains("no Claude Code project dir named"),
                "must not mask as project-dir error for {argv:?}: {msg}"
            );
        }
    }

    #[test]
    fn list_routes_at_uuid_and_bare_uuid_as_positional() {
        // A session is an `@<uuid>` POSITIONAL token, which the parser lands in `paths`
        // (the resolver does the routing, not the parser).
        let at = format!("@{SESS_UUID}");
        let cli = parse(&["csift", "list", at.as_str()]).unwrap();
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.paths.len(), 1, "the @<uuid> token is a positional target");
                assert_eq!(a.paths[0].to_string_lossy(), at);
            }
            _ => panic!("expected list"),
        }
        // A BARE uuid (no `@`) is NOT special — it is just a positional (the
        // resolver later fails it as "no project dir named <uuid>", by design).
        let cli = parse(&["csift", "list", SESS_UUID]).unwrap();
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.paths.len(), 1, "the bare uuid is a positional target");
                assert_eq!(a.paths[0].to_string_lossy(), SESS_UUID);
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn removed_session_flag_no_longer_parses() {
        // The old session-pin flag was hard-removed; clap must reject it as an unknown argument
        // (never silently accept it) on a session-operating subcommand. The flag spelling is
        // assembled at runtime (not a source literal) so it stays a NEGATIVE regression guard,
        // not a lingering reference to the dead flag.
        let dead_flag = concat!("--", "session");
        assert!(
            parse(&["csift", "list", dead_flag, SESS_UUID]).is_err(),
            "a bare --session token must not parse"
        );
    }

    // ── argv normalizer (the actual fix mechanism) ──

    #[test]
    fn normalize_moves_long_flag_with_value_ahead_of_positional() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--format", "json"]
                .map(String::from)
                .to_vec(),
        );
        // program + subcommand stay first; the flag+value are pulled ahead of the
        // leading-`-` positional.
        assert_eq!(out, vec!["csift", "list", "--format", "json", "-Users-foo"]);
    }

    #[test]
    fn normalize_keeps_boolean_flag_without_grabbing_a_value() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--no-subagents"]
                .map(String::from)
                .to_vec(),
        );
        // `--no-subagents` is a SetTrue flag → it must NOT consume the positional.
        assert_eq!(out, vec!["csift", "list", "--no-subagents", "-Users-foo"]);
    }

    #[test]
    fn normalize_passes_through_after_double_dash() {
        let out = normalize_argv(
            ["csift", "list", "--", "-weird-token", "--not-a-flag"]
                .map(String::from)
                .to_vec(),
        );
        // After `--`, tokens are verbatim positionals (clap's escape) — untouched.
        assert_eq!(
            out,
            vec!["csift", "list", "--", "-weird-token", "--not-a-flag"]
        );
    }

    #[test]
    fn normalize_leaves_unknown_subcommand_untouched() {
        let out = normalize_argv(["csift", "--help"].map(String::from).to_vec());
        assert_eq!(out, vec!["csift", "--help"]);
    }

    #[test]
    fn normalize_handles_equals_form() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--format=json"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "list", "--format=json", "-Users-foo"]);
    }

    #[test]
    fn normalize_value_flag_at_end_has_no_following_value() {
        // A value-taking flag with NOTHING after it → the `i+1 < rest.len()` FALSE arm
        // (no value consumed). clap will later report the missing value; normalize
        // just leaves the flag in place.
        let out = normalize_argv(
            ["csift", "search", "x", "--since"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "--since", "x"]);
    }

    #[test]
    fn normalize_value_flag_followed_by_another_flag_does_not_consume_it() {
        // `--since --format json`: `--since` is value-taking but the NEXT token is a
        // declared long flag → it must NOT be consumed as the value (the
        // `!all_long.contains(next)` FALSE arm). Both flags are emitted; clap reports
        // the missing `--since` value.
        let out = normalize_argv(
            ["csift", "search", "x", "--since", "--format", "json"]
                .map(String::from)
                .to_vec(),
        );
        // --since stays unpaired; --format json stay paired; positional `x` last.
        assert_eq!(
            out,
            vec!["csift", "search", "--since", "--format", "json", "x"]
        );
    }

    #[test]
    fn normalize_hoists_short_value_flag_after_positional() {
        // The reported critical bug: `search PATTERN <path> -t user` let the
        // `allow_hyphen_values` positional swallow `-t` ("no project dir named -t").
        // The pre-pass must now hoist the declared short flag `-t` AND pair its value.
        let out = normalize_argv(
            ["csift", "search", "spec", ".", "-t", "user"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "-t", "user", "spec", "."]);
    }

    #[test]
    fn normalize_hoists_short_bool_flag_after_positional() {
        // `-i` is a boolean short flag → hoisted but NOT paired with a value.
        let out = normalize_argv(
            ["csift", "search", "spec", ".", "-i"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "-i", "spec", "."]);
    }

    #[test]
    fn normalize_hoists_bundled_short_flag_after_positional() {
        // A bundled `-tuser` carries its value inline (len != 2) → emitted as one token,
        // never pairing the next positional.
        let out = normalize_argv(
            ["csift", "search", "spec", ".", "-tuser"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "-tuser", "spec", "."]);
    }

    #[test]
    fn normalize_short_value_flag_at_end_has_no_following_value() {
        // `-t` with nothing after it → the no-value arm (clap reports the missing value).
        let out = normalize_argv(
            ["csift", "search", "spec", ".", "-t"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "-t", "spec", "."]);
    }

    #[test]
    fn normalize_short_value_flag_followed_by_flag_does_not_consume_it() {
        // `-t -i`: `-t` is value-taking but the next token starts with `-` → not consumed.
        let out = normalize_argv(
            ["csift", "search", "spec", ".", "-t", "-i"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "-t", "-i", "spec", "."]);
    }

    #[test]
    fn normalize_leaves_encoded_token_with_letter_short_collision_as_positional() {
        // An encoded `-Users-…` token's first char `U` is NOT a declared short flag, so it
        // stays a positional even though `-t`/`-i` exist. The short-flag set is per-char.
        let out = normalize_argv(
            [
                "csift",
                "search",
                "spec",
                "-Users-testuser-Projects-foo",
                "-i",
            ]
            .map(String::from)
            .to_vec(),
        );
        assert_eq!(
            out,
            vec![
                "csift",
                "search",
                "-i",
                "spec",
                "-Users-testuser-Projects-foo"
            ]
        );
    }

    #[test]
    fn parse_search_short_t_after_positional_sets_category() {
        // End-to-end through clap: the hoisted `-t user` lands as a category, positional intact.
        let cli =
            parse(&["csift", "search", "spec", ".", "-t", "user"]).expect("short flag after path");
        match cli.command {
            Command::Search(a) => {
                assert_eq!(a.categories, vec![Category::User]);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.pattern, "spec");
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn parse_search_short_i_after_positional_sets_ignore_case() {
        let cli = parse(&["csift", "search", "SPEC", ".", "-i"]).expect("short -i after path");
        match cli.command {
            Command::Search(a) => {
                assert!(a.ignore_case);
                assert_eq!(a.paths.len(), 1);
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn normalize_value_flag_before_double_dash_does_not_consume_terminator() {
        // `--since -- rest`: the `--` terminator must NOT be eaten as --since's value
        // (the `rest[i+1] != "--"` FALSE arm).
        let out = normalize_argv(
            ["csift", "search", "--since", "--", "-weird"]
                .map(String::from)
                .to_vec(),
        );
        // --since left unpaired; everything after `--` passes through verbatim.
        assert_eq!(out, vec!["csift", "search", "--since", "--", "-weird"]);
    }

    // ── Subagent inclusion flags (default include) ──

    #[test]
    fn list_includes_subagents_by_default() {
        let cli = parse(&["csift", "list"]).unwrap();
        match cli.command {
            Command::List(a) => assert!(a.want_subagents(), "default must include subagents"),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn list_no_subagents_excludes() {
        let cli = parse(&["csift", "list", "--no-subagents"]).unwrap();
        match cli.command {
            Command::List(a) => assert!(!a.want_subagents()),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn search_no_subagents_excludes() {
        let cli = parse(&["csift", "search", "x", "--no-subagents"]).unwrap();
        match cli.command {
            Command::Search(a) => assert!(!a.want_subagents()),
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn include_subagents_is_gone_from_the_default_on_commands() {
        // `--include-subagents` was a no-op on the default-ON spanning commands (the mode it
        // requested was already the default) and is REMOVED there. Only `turns` — which
        // defaults to top-level-only — keeps it as a meaningful opt-in.
        for argv in [
            vec!["csift", "list", SESS_UUID, "--include-subagents"],
            vec!["csift", "search", "x", SESS_UUID, "--include-subagents"],
            vec!["csift", "files", SESS_UUID, "--include-subagents"],
            vec!["csift", "recover", SESS_UUID, "--include-subagents"],
            vec!["csift", "image", SESS_UUID, "--include-subagents"],
        ] {
            assert!(
                parse(&argv).is_err(),
                "{argv:?}: --include-subagents must be an unknown argument now"
            );
        }
        // turns KEEPS it (the opt-in).
        let cli = parse(&["csift", "turns", SESS_UUID, "--include-subagents"]).unwrap();
        match cli.command {
            Command::Turns(a) => assert!(a.want_subagents(), "turns --include-subagents opts in"),
            _ => panic!("expected turns"),
        }
    }

    #[test]
    fn no_subagents_excludes_on_every_default_on_command() {
        // On the default-ON spanning commands the only span flag is `--no-subagents`; it
        // restricts to the top-level session regardless of position.
        let cli = parse(&["csift", "list", SESS_UUID, "--no-subagents"]).unwrap();
        match cli.command {
            Command::List(a) => assert!(!a.want_subagents(), "list: no-subagents excludes"),
            _ => panic!("expected list"),
        }
        let cli = parse(&["csift", "search", "x", SESS_UUID, "--no-subagents"]).unwrap();
        match cli.command {
            Command::Search(a) => assert!(!a.want_subagents(), "search: no-subagents excludes"),
            _ => panic!("expected search"),
        }
        let cli = parse(&["csift", "recover", SESS_UUID, "--no-subagents"]).unwrap();
        match cli.command {
            Command::Recover(a) => assert!(!a.want_subagents(), "recover: no-subagents excludes"),
            _ => panic!("expected recover"),
        }
        let cli = parse(&["csift", "files", SESS_UUID, "--no-subagents"]).unwrap();
        match cli.command {
            Command::Files(a) => assert!(!a.want_subagents(), "files: no-subagents excludes"),
            _ => panic!("expected files"),
        }
        let cli = parse(&["csift", "image", SESS_UUID, "--no-subagents"]).unwrap();
        match cli.command {
            Command::Image(a) => assert!(!a.want_subagents(), "image: no-subagents excludes"),
            _ => panic!("expected image"),
        }
    }

    #[test]
    fn turns_include_then_no_subagents_cancels_the_opt_in() {
        // `turns` defaults to top-level-only; `--include-subagents` opts in. A LATER
        // `--no-subagents` cancels it — the field's `overrides_with` makes the last flag win,
        // and `want_subagents()` ANDs `include && !no_subagents`. (Bare opt-in spans; the
        // trailing `--no-subagents` here suppresses it.)
        let cli = parse(&[
            "csift",
            "turns",
            SESS_UUID,
            "--include-subagents",
            "--no-subagents",
        ])
        .unwrap();
        match cli.command {
            Command::Turns(a) => assert!(
                !a.want_subagents(),
                "turns: a trailing --no-subagents must cancel --include-subagents"
            ),
            _ => panic!("expected turns"),
        }
        // Default (no span flag) is top-level only.
        let cli = parse(&["csift", "turns", SESS_UUID]).unwrap();
        match cli.command {
            Command::Turns(a) => assert!(
                !a.want_subagents(),
                "turns defaults to top-level only (no opt-in)"
            ),
            _ => panic!("expected turns"),
        }
    }

    // ── agents subcommand parsing ──

    #[test]
    fn agents_session_target_and_window() {
        // An `@<uuid>` session token is a POSITIONAL: it lands in `paths`.
        let at = format!("@{SESS_UUID}");
        let cli = parse(&[
            "csift",
            "agents",
            at.as_str(),
            "--since",
            "2h",
            "--order-by",
            "completion",
        ])
        .unwrap();
        match cli.command {
            Command::Agents(a) => {
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.paths[0].to_string_lossy(), at);
                assert_eq!(a.since.as_deref(), Some("2h"));
                assert_eq!(a.order_by, AgentTimeAxis::Completion);
            }
            _ => panic!("expected agents"),
        }
    }

    #[test]
    fn agents_old_by_and_tree_flags_are_gone() {
        // `--by` was renamed to `--order-by`, and `--tree` was removed (tree is always on);
        // both now error as unknown arguments.
        let by = parse(&["csift", "agents", ".", "--by", "trigger"]);
        assert!(by.is_err(), "--by must be an unknown argument now");
        let tree = parse(&["csift", "agents", ".", "--tree"]);
        assert!(tree.is_err(), "--tree must be an unknown argument now");
    }

    #[test]
    fn agents_kind_filter_and_default_axis() {
        let cli = parse(&["csift", "agents", ".", "--kind", "workflow"]).unwrap();
        match cli.command {
            Command::Agents(a) => {
                assert_eq!(a.kinds, vec![AgentKindFilter::Workflow]);
                assert_eq!(
                    a.order_by,
                    AgentTimeAxis::Trigger,
                    "default axis is trigger (the true spawn instant)"
                );
                assert!(a.agent.is_none());
                assert!(!a.with_files);
                assert!(!a.returned_message);
            }
            _ => panic!("expected agents"),
        }
    }

    // ── files subcommand parsing ──

    #[test]
    fn files_default_detail_is_summary() {
        let at = format!("@{SESS_UUID}");
        let cli = parse(&["csift", "files", at.as_str()]).unwrap();
        match cli.command {
            Command::Files(a) => {
                assert_eq!(a.detail(), FilesDetail::Summary, "default is summary");
                assert!(a.want_subagents(), "subagents spanned by default");
                assert_eq!(a.regex, None);
                assert_eq!(a.glob, None);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.paths[0].to_string_lossy(), at);
            }
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_by_file_selects_by_file() {
        let at = format!("@{SESS_UUID}");
        let cli = parse(&["csift", "files", at.as_str(), "--by", "file"]).unwrap();
        match cli.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByFile),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_by_dir_and_timeline_select_levels() {
        let by_dir = parse(&["csift", "files", ".", "--by", "dir"]).unwrap();
        match by_dir.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByDir),
            _ => panic!("expected files"),
        }
        let timeline =
            parse(&["csift", "files", ".", "--by", "timeline", "--since", "2h"]).unwrap();
        match timeline.command {
            Command::Files(a) => {
                assert_eq!(a.detail(), FilesDetail::Timeline);
                assert_eq!(a.since.as_deref(), Some("2h"));
            }
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_explicit_summary_value_is_summary() {
        let cli = parse(&["csift", "files", ".", "--by", "summary"]).unwrap();
        match cli.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::Summary),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_by_rejects_unknown_value() {
        // The clap `ValueEnum` for `--by` rejects a spelling outside the four allowed values
        // (in particular the OLD `--by-dir`-style value name is NOT accepted).
        let err = parse(&["csift", "files", ".", "--by", "by-dir"]);
        assert!(err.is_err(), "an unknown --by value must be a parse error");
        let err2 = parse(&["csift", "files", ".", "--by", "files"]);
        assert!(err2.is_err(), "an unknown --by value must be a parse error");
    }

    #[test]
    fn files_old_detail_flags_are_gone() {
        // The 4 old boolean detail flags were replaced by `--by <value>`; passing one now
        // errors as an unknown argument.
        for flag in ["--summary", "--by-dir", "--by-file", "--timeline"] {
            let err = parse(&["csift", "files", ".", flag]);
            assert!(err.is_err(), "{flag} must be an unknown argument now");
        }
    }

    #[test]
    fn files_no_subagents_excludes() {
        let cli = parse(&["csift", "files", ".", "--no-subagents"]).unwrap();
        match cli.command {
            Command::Files(a) => assert!(!a.want_subagents()),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_subagents_only_flag_is_gone() {
        // `--subagents-only` was removed from `files`; it is now an unknown argument.
        let err = parse(&["csift", "files", ".", "--subagents-only"]);
        assert!(err.is_err(), "--subagents-only must be gone from files");
    }

    #[test]
    fn files_regex_and_glob_parse() {
        let cli = parse(&[
            "csift",
            "files",
            ".",
            "--by",
            "file",
            "--regex",
            r"\.rs$",
            "--glob",
            "**/src/**",
        ])
        .unwrap();
        match cli.command {
            Command::Files(a) => {
                assert_eq!(a.detail(), FilesDetail::ByFile);
                assert_eq!(a.regex.as_deref(), Some(r"\.rs$"));
                assert_eq!(a.glob.as_deref(), Some("**/src/**"));
            }
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_encoded_token_then_flag_ordering() {
        // normalize_argv must route a trailing flag ahead of a leading-`-` token.
        let cli = parse(&["csift", "files", "-Users-foo", "--format", "json"]).unwrap();
        match cli.command {
            Command::Files(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.paths[0].to_string_lossy(), "-Users-foo");
            }
            _ => panic!("expected files"),
        }
    }
}
