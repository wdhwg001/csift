//! parse_argv + normalize_argv: the P0 flag-reordering entrypoint (clap #3880 workaround).

use super::*;

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
/// token (`-Users-…` with a SINGLE leading `-`, or the Windows drive shape `C--Users-…`),
/// or `.`. It REJECTS a `--`-leading token: overwhelmingly that is an unknown / typo'd
/// flag the `allow_hyphen_values` positional would otherwise swallow, surfacing the
/// misleading `no project dir named "--xxx"` error instead of clap's clean
/// `unexpected argument '--xxx'` + `did you mean --no-subagents?` suggestion. (The ONE
/// legitimate `--`-leading target — a UNC-encoded `--server-…` dir — is deliberately
/// routed through the `@--server-…` form instead, which the error names.) Returning `Err`
/// here makes clap reconsider the token and emit that standard message uniformly across
/// every scope-operating subcommand (search/whoami already did; the rest did not). A SINGLE
/// `-` token is still accepted (it is a genuine encoded target). The [`normalize_argv`]
/// pre-pass has already routed DECLARED flags away, so a `--`-token reaching here is by
/// construction undeclared.
pub(crate) fn parse_project_target(s: &str) -> Result<PathBuf, String> {
    if s.starts_with("--") {
        return Err(format!(
            "unexpected argument '{s}' — not a project target; did you mistype a flag? \
             (a UNC-encoded dir is targeted as '@{s}')"
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
/// ## Pre-subcommand global flags
///
/// The subcommand token is LOCATED BY SCANNING, not assumed at `argv[1]`: declared root
/// flags (e.g. the global `--claude-home <DIR>`, documented as position-free) and their
/// values are stepped over first. Everything before the subcommand passes through
/// verbatim; only the segment AFTER the subcommand is reordered. Regression context:
/// `csift --claude-home DIR list @x --max-count 3` used to skip normalization entirely
/// (`argv[1]` matched no subcommand), letting the PATH positional swallow `--max-count`.
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
    // argv[0] = program. The subcommand is NOT necessarily argv[1]: a root-level GLOBAL
    // flag may precede it (`csift --claude-home DIR list ...`). Scan forward from argv[1],
    // stepping over DECLARED root flags (and the value token of a value-taking one),
    // until the first non-flag token -- the subcommand candidate. Any token we cannot
    // positively classify (an unknown `--x`, a lone `-`) aborts the scan: argv is
    // returned untouched and clap reports as usual.
    if argv.len() < 2 {
        return argv;
    }
    let cmd = Cli::command();
    let root = root_flag_sets(&cmd);
    let Some(sub_idx) = find_subcommand_index(&argv, &root) else {
        return argv; // no positively-classified subcommand -- leave argv for clap
    };
    let sub_name = &argv[sub_idx];
    let Some(sub) = cmd
        .get_subcommands()
        .find(|s| s.get_name() == sub_name || s.get_all_aliases().any(|a| a == sub_name))
    else {
        // Not a recognized subcommand (a typo) -- leave argv untouched and let clap
        // produce its normal message.
        return argv;
    };
    let sets = sub_flag_sets(sub, &cmd);

    let head = argv[..=sub_idx].to_vec(); // program (+ any root flags) + subcommand
    let rest = &argv[sub_idx + 1..];
    let (flags, positionals, passthrough) = reorder_tail(rest, &sets);

    let mut out = head;
    out.extend(flags);
    out.extend(positionals);
    out.extend(passthrough);
    out
}

/// The ROOT command's declared flags: every long/short spelling, and which take a value.
struct FlagSets {
    value_long: std::collections::HashSet<String>,
    all_long: std::collections::HashSet<String>,
    value_short: std::collections::HashSet<char>,
    all_short: std::collections::HashSet<char>,
}

fn root_flag_sets(cmd: &clap::Command) -> FlagSets {
    let mut root_value_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut root_all_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut root_value_short: std::collections::HashSet<char> = std::collections::HashSet::new();
    let mut root_all_short: std::collections::HashSet<char> = std::collections::HashSet::new();
    for a in cmd.get_arguments() {
        let takes = flag_takes_value(a);
        if let Some(longs) = a.get_long_and_visible_aliases() {
            for l in longs {
                let f = format!("--{l}");
                if takes {
                    root_value_long.insert(f.clone());
                }
                root_all_long.insert(f);
            }
        }
        if let Some(shorts) = a.get_short_and_visible_aliases() {
            for c in shorts {
                root_all_short.insert(c);
                if takes {
                    root_value_short.insert(c);
                }
            }
        }
    }
    FlagSets {
        value_long: root_value_long,
        all_long: root_all_long,
        value_short: root_value_short,
        all_short: root_all_short,
    }
}

/// Scan forward from argv[1] over DECLARED root flags to the first non-flag token — the
/// subcommand candidate. `None` when the scan cannot positively classify a token (an unknown
/// `--x`, a lone `-`, the `--` terminator) or when argv is flags-only: the caller returns
/// argv untouched and clap reports as usual.
fn find_subcommand_index(argv: &[String], root: &FlagSets) -> Option<usize> {
    let mut scan = 1;
    while scan < argv.len() {
        let tok = &argv[scan];
        if let Some(name) = tok.strip_prefix("--") {
            if name.is_empty() {
                return None; // `--` terminator before any subcommand -- nothing to do
            }
            let base = name.split_once('=').map_or(name, |(n, _)| n);
            let flag = format!("--{base}");
            if !root.all_long.contains(&flag) {
                return None; // unknown root flag / typo -- leave for clap to report
            }
            // Inline `--flag=value` and boolean flags span one token; a bare
            // value-taking flag also consumes its following token as the value.
            if name.contains('=') || !root.value_long.contains(&flag) {
                scan += 1;
            } else {
                scan += 2;
            }
        } else if let Some(short) = tok.strip_prefix('-') {
            let first = short.chars().next()?; // a lone `-` is never a root flag
            if !root.all_short.contains(&first) {
                return None;
            }
            if tok.len() == 2 && root.value_short.contains(&first) {
                scan += 2;
            } else {
                scan += 1;
            }
        } else {
            return Some(scan);
        }
    }
    None // flags only, no subcommand -- nothing to normalize
}

/// The SUBCOMMAND's declared flags (its own args PLUS the root's GLOBAL args — e.g.
/// `--claude-home <DIR>` propagates to every subcommand but may not surface in
/// `sub.get_arguments()` at introspection time; without it a `--claude-home /path` placed
/// AFTER the subcommand would mis-sort the path as a positional. HashSet inserts are
/// idempotent, so double-counting is harmless). Short flags matter too: a declared `-x`
/// must be hoisted ahead of the `allow_hyphen_values` positional exactly like a long flag
/// — otherwise `search PATTERN . -t user` lets the positional greedily swallow `-t`. An
/// UNDECLARED `-x` (e.g. an encoded `-Users-...` token) is NOT in these sets and stays a
/// positional.
fn sub_flag_sets(sub: &clap::Command, cmd: &clap::Command) -> FlagSets {
    let mut value_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut short_value: std::collections::HashSet<char> = std::collections::HashSet::new();
    let mut short_all: std::collections::HashSet<char> = std::collections::HashSet::new();
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

    FlagSets {
        value_long,
        all_long,
        value_short: short_value,
        all_short: short_all,
    }
}

/// Re-sort the post-subcommand tokens into (flags-with-values, positionals, passthrough) —
/// the actual clap #3880 workaround: declared flags (and their value tokens) are hoisted
/// ahead of the `allow_hyphen_values` positionals; everything after a `--` terminator is
/// verbatim positional input.
fn reorder_tail(rest: &[String], sets: &FlagSets) -> (Vec<String>, Vec<String>, Vec<String>) {
    let FlagSets {
        value_long,
        all_long,
        value_short: short_value,
        all_short: short_all,
    } = sets;
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
                // A boolean long flag (or an unknown `--x`): no paired value. An unknown
                // long is deliberately hoisted WITH the declared ones: on a command whose
                // first positional refuses hyphen values (`search` PATTERN) clap then
                // reports it as an unknown flag WITH a did-you-mean tip (the old-spelling
                // recovery path, e.g. `--turn-range` → `--turn`), and on every
                // target-taking command the `allow_hyphen_values` PATH/TARGET Vec absorbs
                // it into `parse_project_target`, which rejects it BY NAME ("did you
                // mistype a flag?"). Correct attribution in both regimes — see
                // `ShowArgs::target` for why show's TARGET must be a Vec for this.
                flags.push(tok.clone());
                i += 1;
            }
        } else if let Some(short_c) = declared_short_flag(tok, short_all) {
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

    (flags, positionals, passthrough)
}

/// If `tok` is a `-x…` short-flag token whose first post-dash character is a DECLARED
/// short flag of the active subcommand, return that character; else `None`. Used by
/// [`normalize_argv`] to distinguish a real short flag (`-t`, `-i`, a bundled `-tuser`)
/// — which must be hoisted ahead of an `allow_hyphen_values` positional — from a
/// leading-`-` ENCODED project token (`-Users-…`, whose first char is not a declared
/// short flag) which stays a positional. A bare `-` or `--`-leading token is never a
/// short flag here (the caller handles `--` separately; a lone `-` has no flag char).
pub(crate) fn declared_short_flag(
    tok: &str,
    short_all: &std::collections::HashSet<char>,
) -> Option<char> {
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
pub(crate) fn flag_takes_value(a: &clap::Arg) -> bool {
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
