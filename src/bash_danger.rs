//! Faithful port of Claude Code's `dangerous-rm` bash classifier - the ONE deterministic
//! bash-danger class CC hoists to a human approval prompt EVEN under bypass-permissions (a
//! `classifierApprovable:false` safetyCheck). Used by `agents` to tell a frozen lane that is
//! **escalation-blocked** (a pending Bash tool_use CC would hoist → waiting for a human "Yes")
//! apart from one merely **awaiting-execution** (a slow tool) - a distinction the jsonl alone
//! otherwise can't make (the escalation lives only in CC process memory; see `subagent.rs`).
//!
//! **Extracted 1:1 from the CC 2.1.193 Mach-O** (function `Ywa` + the `egp`/`Zhp` regexes),
//! re-grepped at port time (`Dangerous rm operation` / `possibly-empty variable path` strings +
//! the two regexes verbatim). CC flags `rm`/`rmdir` whose target is a bare `$VAR/…` / `${VAR}/…`.
//! It is PURELY LEXICAL: it does NOT check whether the variable is actually empty - CC knowingly
//! accepts that false-positive rate (it would rather over-prompt on `rm $VAR/…`). We MIRROR that
//! faithfully; do NOT "improve" it to resolve variables, or csift's verdict diverges from CC's
//! real hoist decision and we'd mispredict whether CC actually blocks.
//!
//! **One faithful deviation:** CC's statement splitter `E_` uses a bash tree-sitter parser; rather
//! than pull in tree-sitter, we process the whole command as a single statement (`E_`'s own
//! documented fallback for unparseable/over-long input) and recover compound bodies by splitting
//! on statement separators plus stripping leading shell keywords (`do`/`then`/…). The regex core
//! (egp/Zhp + the preprocessing) is exact; the divergence is limited to exotic compound nesting a
//! full bash AST would isolate differently. Common teardown `rm` - the case that triggered this -
//! is covered. The two preprocessing transforms that use lookbehind/lookahead in the JS
//! (`(?<!\$)\(…\)` and the `&`→`;` rule) are hand-ported, since the `regex` crate has no lookaround.

use regex::Regex;
use std::sync::LazyLock;

/// `egp` (verbatim): a clause head - optional `VAR=val ` assignments, optional backslash, optional
/// `path/` prefix, then `rm`/`rmdir` at a word boundary. Capture group 1 = `rm` | `rmdir`.
static EGP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*\+?=[^\s]*\s+)*\\?(?:[^\s=]*/)?(rm|rmdir)(?:\s|$)")
        .expect("egp")
});

/// `Zhp` (verbatim): a removal TARGET that is a bare `$VAR`/`${VAR}` (optional surrounding `"`)
/// immediately followed by `/` then one of `* $ / " ' <end>` - i.e. `$VAR/…`. This is what makes
/// `"$SCRATCH/$f"` dangerous. Lexical only (no emptiness check).
static ZHP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^"?\$(?:\{[A-Za-z_][A-Za-z0-9_]*\}|[A-Za-z_][A-Za-z0-9_]*)"?/(?:\*|\$|/|["']|$)"#)
        .expect("Zhp")
});

/// The cheap initial guard `/\brm(?:dir)?\b/` - no `rm`/`rmdir` word ⇒ safe.
static RM_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\brm(?:dir)?\b").expect("rm_word"));

/// `/^[\d&]*[<>]/` - a token that STARTS like a redirection.
static REDIR_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\d&]*[<>]").expect("redir"));

/// `/^(?:[0-9]+|&)?(?:>>?[|&]?|<<?<?|<>)$/` - a token that is JUST a redirect operator (no
/// attached target), so the NEXT token is its target and must be skipped.
static REDIR_OP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[0-9]+|&)?(?:>>?[|&]?|<<?<?|<>)$").expect("redir_op"));

/// `/\\\r?\n/` - a line continuation (backslash-newline).
static LINE_CONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\\r?\n").expect("line_cont"));
/// `` /`[^`]*`/ `` - a backtick command substitution.
static BACKTICKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]*`").expect("backticks"));
/// `/\$\([^()]*\)/` - an innermost `$(…)` command substitution (no lookaround).
static DOLLAR_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\([^()]*\)").expect("dpar"));
/// `/\([^()]*\)/` - an innermost `(…)` group (the `(?<!\$)` guard is applied in code).
static PLAIN_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^()]*\)").expect("ppar"));
/// `/[;|\n\r]|&&/` - the clause separator set.
static CLAUSE_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[;|\n\r]|&&").expect("clause"));

/// A removal CC's classifier would flag as dangerous (the `Ywa` return shape `{command,target}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerousRm {
    /// `"rm"` or `"rmdir"`.
    pub command: &'static str,
    /// The matched target token (e.g. `"$SCRATCH/$f"`).
    pub target: String,
}

/// Convenience: would CC flag this command as a dangerous removal?
#[must_use]
pub fn is_dangerous_rm(command: &str) -> bool {
    dangerous_rm(command).is_some()
}

/// Mirror of CC's `Ywa(command)`: the first dangerous `rm`/`rmdir` removal in `command`, or
/// `None`. See the module docs for the fidelity contract + the one deviation.
///
/// STALENESS NOTE (binary evidence, 2026-08-12): CC 2.1.228's classifier (`aLa`) has
/// EVOLVED past the 2.1.x generation this port mirrors - it strips `$(…)` groups to a
/// FIXPOINT (this port is single-pass), and a tree-sitter pass bails to explicit approval
/// when a command carries >64 command substitutions. csift's escalation-blocked prediction
/// can therefore diverge from current CC on those shapes; a port refresh is a recorded
/// follow-up, not silent drift. (CC also ships a separate Windows `PowerShell` tool; this
/// lexical-bash classifier deliberately does NOT run on PowerShell commands - a pending
/// PowerShell lane classifies awaiting-execution.)
#[must_use]
pub fn dangerous_rm(command: &str) -> Option<DangerousRm> {
    // `if(!e.includes("$")||!/\brm(?:dir)?\b/.test(e))return null`
    if !command.contains('$') || !RM_WORD.is_match(command) {
        return None;
    }
    // `let n=t.replace(/\\\r?\n/g," ").replace(/`[^`]*`/g," ").trimStart()`
    let n = LINE_CONT.replace_all(command, " ");
    let n = BACKTICKS.replace_all(&n, " ");
    let mut n = n.trim_start().to_string();
    // `while(n.startsWith("(")||n.startsWith("{"))n=n.slice(1).trimStart()`
    while n.starts_with('(') || n.starts_with('{') {
        n = n[1..].trim_start().to_string();
    }
    // The `$(…)`/`(…)` strip loop, then `&`→`;`.
    n = strip_paren_groups(&n);
    n = amp_to_semicolon(&n);
    // `for(let r of n.split(/[;|\n\r]|&&/))`
    for clause in CLAUSE_SPLIT.split(&n) {
        // `o=r.trimStart()`, plus our leading-keyword strip (the E_ approximation).
        let o = strip_leading_keywords(clause.trim_start());
        let Some(caps) = EGP.captures(o) else {
            continue;
        };
        let command_word: &'static str = if &caps[1] == "rmdir" { "rmdir" } else { "rm" };
        let rest = &o[caps.get(0).map_or(0, |m| m.end())..];
        // `a=o.slice(s[0].length).split(/\s+/)` (empties are skipped below, so split_whitespace
        // is equivalent for the danger logic; the index-based redirect skip is preserved).
        let args: Vec<&str> = rest.split_whitespace().collect();
        let mut l = 0usize;
        while l < args.len() {
            // `c=a[l].replace(/[)\]}]+$/,"")`
            let c = args[l].trim_end_matches([')', ']', '}']);
            // `if(c===""||c.startsWith("-")||c.startsWith("'"))continue`
            if c.is_empty() || c.starts_with('-') || c.starts_with('\'') {
                l += 1;
                continue;
            }
            // `if(/^[\d&]*[<>]/.test(c)){if(/…op…/.test(c))l++;continue}`
            if REDIR_START.is_match(c) {
                if REDIR_OP.is_match(c) {
                    l += 1; // skip the redirect's target token
                }
                l += 1;
                continue;
            }
            // `if(Zhp.test(c))return{command:i,target:c}`
            if ZHP.is_match(c) {
                return Some(DangerousRm {
                    command: command_word,
                    target: c.to_string(),
                });
            }
            l += 1;
        }
    }
    None
}

/// The `$(…)`/`(…)` strip loop:
/// `for(let r="";r!==n;)r=n,n=n.replace(/\$\([^()]*\)/g," ").replace(/(?<!\$)\([^()]*\)/g," ")`.
/// Removes command substitutions and subshell/group parens innermost-first until stable. The
/// `$(…)` pass is a plain regex; the `(?<!\$)(…)` pass needs the lookbehind, hand-applied.
fn strip_paren_groups(s: &str) -> String {
    let mut n = s.to_string();
    loop {
        let prev = n.clone();
        n = DOLLAR_PAREN.replace_all(&n, " ").into_owned();
        n = remove_plain_groups(&n);
        if n == prev {
            break;
        }
    }
    n
}

/// `replace(/(?<!\$)\([^()]*\)/g," ")` - replace each innermost `(…)` whose `(` is NOT preceded by
/// `$` with a space; keep a `$(…)` that the prior pass left (handled there). The `(?<!\$)`
/// lookbehind is applied by checking the byte before the match.
fn remove_plain_groups(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in PLAIN_PAREN.find_iter(s) {
        let start = m.start();
        let preceded_by_dollar = start > 0 && bytes[start - 1] == b'$';
        out.push_str(&s[last..start]);
        if preceded_by_dollar {
            out.push_str(m.as_str()); // keep (it was/will be handled by the `$(…)` pass)
        } else {
            out.push(' ');
        }
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

/// `replace(/(?<![<>&])&(?![<>&])/g,";")` - a lone `&` (backgrounding/`&`-list) becomes `;`, but
/// `&&`, `<&`, `>&`, `&>`, `&<` are preserved. Hand-ported (the JS uses lookbehind+lookahead). The
/// special bytes are all ASCII and never occur inside a UTF-8 multibyte sequence, so a byte scan
/// that copies non-`&` bytes verbatim preserves any multibyte content untouched.
fn amp_to_semicolon(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    for i in 0..b.len() {
        if b[i] == b'&' {
            let prev_bad = i > 0 && matches!(b[i - 1], b'<' | b'>' | b'&');
            let next_bad = i + 1 < b.len() && matches!(b[i + 1], b'<' | b'>' | b'&');
            if !prev_bad && !next_bad {
                out.push(b';');
                continue;
            }
        }
        out.push(b[i]);
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Strip a leading run of shell reserved words that introduce/precede a command, so a clause like
/// `do rm $x/` (a for-loop body) or `then rm $x/` (an if-branch) is recognised - approximating the
/// statement isolation CC's tree-sitter `E_` would do. Conservative: only well-known leading words.
fn strip_leading_keywords(mut s: &str) -> &str {
    const KW: &[&str] = &["do", "then", "else", "elif", "if", "while", "until", "time"];
    s = s.trim_start();
    loop {
        // `!` (negation) can attach with or without a space - strip it directly.
        if let Some(rest) = s.strip_prefix('!') {
            s = rest.trim_start();
            continue;
        }
        let mut stripped = false;
        for kw in KW {
            if let Some(rest) = s.strip_prefix(kw) {
                // A keyword only when followed by whitespace or end (not a prefix of an identifier).
                if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace()) {
                    s = rest.trim_start();
                    stripped = true;
                    break;
                }
            }
        }
        if !stripped {
            return s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mutation-kill pins (cargo-mutants survivors): each case was derived to
    //    DIFFERENTIATE a surviving operator flip from the shipped behavior - a
    //    boundary the ordinary fixtures never reached. ──

    #[test]
    fn redirect_target_token_is_skipped_not_flagged() {
        // `rm -r $X > $TMP/`: the SEPARATED redirect target (`$TMP/`) is Zhp-shaped but
        // belongs to the redirect, not to rm - CC's walker skips it (the extra advance
        // after a bare redirect operator). A broken skip would flag it as the rm target.
        assert!(dangerous_rm("rm -r $X > $TMP/").is_none());
        // The FUSED form (`2>$TMP/`, operator + target in one token) is also a redirect.
        assert!(dangerous_rm("rm -r $X 2>$TMP/").is_none());
    }

    #[test]
    fn amp_to_semicolon_boundary_forms() {
        // Lone `&` converts at EVERY position including the string edges; the two-byte
        // operators (`&&` / `>&` / `<&` / `&>`) are preserved.
        assert_eq!(amp_to_semicolon("a & b"), "a ; b");
        assert_eq!(amp_to_semicolon("a&"), "a;"); // trailing: no next byte to consult
        assert_eq!(amp_to_semicolon("&b"), ";b"); // leading: no previous byte to consult
        assert_eq!(amp_to_semicolon("a&&b"), "a&&b");
        assert_eq!(amp_to_semicolon("2>&1"), "2>&1");
        assert_eq!(amp_to_semicolon("<&0"), "<&0");
        assert_eq!(amp_to_semicolon("a&>x"), "a&>x");
    }

    #[test]
    fn plain_group_removal_edges() {
        // A group at string START is a plain group (there is no preceding byte).
        assert_eq!(remove_plain_groups("(a b) x"), "  x");
        // A `$`-preceded group is KEPT (the `$(…)` pass owns it); a plain one is spaced.
        assert_eq!(remove_plain_groups("a$(b) (c) d"), "a$(b)   d");
    }

    #[test]
    fn flags_the_real_escalation_command() {
        // The exact teardown clause that triggered the 36.7-min hoist (agent-ab8a4c…, L630):
        // `[ -f "$SCRATCH/$f" ] && rm -f "$SCRATCH/$f" && echo …` - the `rm -f "$SCRATCH/$f"`
        // clause is dangerous (bare $VAR/… target).
        let cmd = r#"SCRATCH="/tmp/x"
for f in a.txt b.txt; do
  [ -f "$SCRATCH/$f" ] && rm -f "$SCRATCH/$f" && echo "  shredded $f"
done"#;
        let d = dangerous_rm(cmd).expect("must flag the teardown rm");
        assert_eq!(d.command, "rm");
        assert!(d.target.contains("$SCRATCH/$f"), "target: {}", d.target);
    }

    #[test]
    fn zhp_target_forms_match_cc() {
        // Zhp fires on `$VAR/` followed by one of `* $ / " ' <end>` (a possibly-empty var whose
        // expansion-then-slash is catastrophic, e.g. `rm /*`). Each below is such a target.
        for t in [
            r#"rm -rf "$SCRATCH/$f""#, // $VAR/ then $ (another var)
            r"rm $DIR/*",              // $VAR/ then *
            r"rm ${DIR}/$x",           // ${VAR}/ then $
            r#"rm "$X"/"#,             // "$VAR"/ then end
            r"rmdir $D/",              // $VAR/ then end
        ] {
            assert!(is_dangerous_rm(t), "should flag: {t}");
        }
    }

    #[test]
    fn lexical_boundary_var_slash_literal_is_not_dangerous() {
        // CC requires the post-slash char to be glob/var/slash/quote/end. `$VAR/literal` is NOT
        // flagged - locking the EXACT Zhp boundary (do not broaden it).
        assert!(!is_dangerous_rm("rm $TMP/build"));
        assert!(!is_dangerous_rm("rm ${DIR}/sub"));
        assert!(is_dangerous_rm("rm $TMP/*")); // …but the glob form IS.
    }

    #[test]
    fn keyword_attached_rm_is_recovered() {
        // `do rm $x/…` and `then rm $x/…` (no separator before rm) - caught via keyword stripping
        // (what CC's tree-sitter isolates).
        assert!(is_dangerous_rm("for f in a; do rm -rf $TMP/$f; done"));
        assert!(is_dangerous_rm("if true; then rm $TMP/*; fi"));
    }

    #[test]
    fn safe_commands_are_not_flagged() {
        // No `$` → safe; no rm → safe; rm of a non-$VAR/glob path → safe (mirrors CC).
        assert!(!is_dangerous_rm("rm -rf /tmp/build"));
        assert!(!is_dangerous_rm("echo $HOME/x")); // no rm
        assert!(!is_dangerous_rm("rm -rf ./dist")); // not a bare $VAR/
        assert!(!is_dangerous_rm("rm $FILE")); // $VAR with no trailing slash → not Zhp
                                               // `docker … images rm docker.io/…` - rm is a SUBCOMMAND, clause does not start with rm.
        assert!(!is_dangerous_rm(
            r#"docker exec "$node" ctr -n k8s.io images rm docker.io/library/img 2>&1"#
        ));
    }

    #[test]
    fn empty_var_is_flagged_lexically_like_cc() {
        // Even though $SCRATCH is assigned a non-empty path on the prior line, CC flags `$SCRATCH/*`
        // (it does NOT resolve the variable). We mirror that - do not "fix" it.
        let cmd = "SCRATCH=/tmp/real\nrm -rf $SCRATCH/*";
        assert!(is_dangerous_rm(cmd));
    }

    #[test]
    fn redirects_and_flags_are_skipped_then_target_found() {
        // A redirect operator + target before the dangerous arg must not derail the scan.
        assert!(is_dangerous_rm("rm -f 2>/dev/null $TMP/*"));
        assert!(is_dangerous_rm("rm -f > log $TMP/*"));
    }

    #[test]
    fn amp_to_semicolon_preserves_operators() {
        assert_eq!(amp_to_semicolon("a & b"), "a ; b");
        assert_eq!(amp_to_semicolon("a && b"), "a && b");
        assert_eq!(amp_to_semicolon("x 2>&1"), "x 2>&1");
        assert_eq!(amp_to_semicolon("x >&2"), "x >&2");
    }

    #[test]
    fn paren_groups_stripped() {
        // `(rm $x/)` subshell and `$(...)` cmd-sub are removed before clause matching, but the
        // bare `rm $x/` inside a subshell is still reached (leading `(` stripped at stmt start).
        assert!(is_dangerous_rm("(rm -rf $TMP/*)"));
        assert!(!is_dangerous_rm("echo $(rm $TMP/*)")); // rm only inside $(...) → stripped → safe
    }
}
