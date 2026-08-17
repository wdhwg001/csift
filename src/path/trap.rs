//! @trap self-identification: marker grammar, transcript scan, ancestry walk.

use super::*;

/// The identity `@trap:<marker>` resolved to: a specific subagent (its bare hex), or the
/// main-thread session itself (when the marker was issued from the top-level conversation).
pub(crate) enum TrapSelf {
    Agent(String),
    Session(String),
}

/// Resolve `@trap:<marker>` to the CALLING agent/session by finding the transcript whose Bash
/// `tool_use` command carries the (unique, literal) marker AND the literal `csift` - i.e. the
/// very command that launched this run. Mechanism + TIMING (both verified live 2026-07-12): a
/// SUBAGENT's transcript records the launching tool_use eagerly - its own Bash can grep the
/// marker mid-execution, so a first try resolves; the MAIN conversation's record is flushed only
/// AFTER the current Bash call completes (a 3s in-command sleep still saw 0 on disk), so a
/// top-level FIRST use ALWAYS misses and only a re-run of the SAME marker (now in the previous,
/// flushed record) resolves. @trap therefore earns its keep for subagents (which cannot name
/// themselves); a main thread should prefer `@main` (env-based, no race) - the no-match error
/// routes both. It searches the calling session (`CLAUDE_CODE_SESSION_ID`) + its subagent
/// transcripts; a subagent match → that agent (then its subtree, per scope); only the main
/// transcript → the session. The marker grammar is enforced strictly (see
/// [`validate_trap_marker`]) precisely so the discipline cannot be shortcut: it must be a fresh,
/// one-shot, imaginative token the model invents on the spot - never script-generated (a
/// generator would itself be a `csift`-ish Bash call carrying the marker → ambiguity). Errors on
/// a malformed marker, no match (marker not literal / mistyped / not yet flushed, or the command
/// did not actually run `csift`), or >1 subagent (use a fresher random marker).
pub(crate) fn resolve_trap(marker: &str) -> Result<TrapSelf> {
    let marker = marker.trim();
    validate_trap_marker(marker)?;
    let session_id = resolve_env_session()?;
    let main_jsonl = locate_session_jsonl(&session_id).ok_or_else(|| {
        anyhow!(
            "@trap: cannot locate the calling session {session_id}.jsonl under the projects root"
        )
    })?;

    let main_hit = bash_command_carries_trap(&main_jsonl, marker);
    let mut subagent_hits: Vec<String> = Vec::new();
    for sub in crate::subagent::subagent_transcript_files(&main_jsonl)? {
        if bash_command_carries_trap(&sub, marker) {
            subagent_hits.push(crate::subagent::session_id_from_path(&sub));
        }
    }

    match subagent_hits.len() {
        1 => Ok(TrapSelf::Agent(subagent_hits.remove(0))),
        0 if main_hit => Ok(TrapSelf::Session(session_id)),
        0 => bail!(
            "@trap: marker `{marker}` not found in a `csift` shell command (Bash / PowerShell) of \
             the calling session. \
             It must appear LITERALLY in THIS csift invocation (no shell variable / concatenation, \
             and the command must actually run `csift`). TIMING: a SUBAGENT's transcript already \
             carries its command mid-run (a first try resolves), but the MAIN conversation's own \
             record is only flushed AFTER the current command finishes — a top-level FIRST use \
             always misses. If you are the top-level thread: use `@main` (env-based, no race), or \
             re-run this EXACT command with the SAME marker as a NEW, SEPARATE shell invocation — \
             a second attempt inside the SAME shell script does NOT count (the whole script is ONE \
             still-in-flight command; nothing flushes until it exits), and a fresh marker restarts \
             the race and misses again."
        ),
        n => bail!(
            "@trap: marker `{marker}` is AMBIGUOUS — it matched {n} subagents. Use a fresher, \
             more random marker (it must be unique within the conversation)."
        ),
    }
}

/// One node in the `whoami @trap` UPSTREAM ancestry chain - a subagent (or the top-level session)
/// the caller belongs to. The chain runs SELF → ancestors → top-level root, so a subagent learns
/// its own bare hex AND the whole re-feedable session lineage above it: `agents` walks the topology
/// DOWN, `whoami` walks it UP.
pub struct WhoNode {
    /// Bare-hex agent id for a subagent node, or the uuid for the top-level session.
    pub session_id: String,
    pub is_subagent: bool,
    /// The always-re-feedable owning top-level uuid (== `session_id` on the root node).
    pub parent_session_id: String,
    /// tool_use-graph nesting depth for a subagent (0 = a direct child of the session); `None` for
    /// the top-level root.
    pub depth: Option<usize>,
    pub path: Option<PathBuf>,
}

/// Resolve `@trap:<marker>` for `whoami` into the caller's UPSTREAM ancestry chain: the marker
/// carrier FIRST (a subagent, or the top-level session itself), then each parent walked via the
/// topology's `parent_agent_id`, ending at the top-level session. Reuses [`resolve_trap`]'s strict
/// grammar + marker scan and [`crate::subagent::build_topology`] for the walk. Env-independent -
/// reliable for a built-in Task AND a workflow subagent (whose env id is the PARENT, not itself).
/// The topology is flat today (every subagent is depth 0), so the chain is `subagent → top-level`;
/// the walk is future-proof for real nesting (depth > 0).
pub fn resolve_trap_who(marker: &str) -> Result<Vec<WhoNode>> {
    let root = resolve_env_session()?;
    let main_jsonl = locate_session_jsonl(&root);

    match resolve_trap(marker)? {
        // The marker was in the MAIN transcript → the caller IS the top-level session (no ancestry).
        TrapSelf::Session(session_id) => {
            let path = locate_session_jsonl(&session_id);
            Ok(vec![WhoNode {
                parent_session_id: session_id.clone(),
                session_id,
                is_subagent: false,
                depth: None,
                path,
            }])
        }
        // A subagent carried it → walk UP from that hex to the top-level session.
        TrapSelf::Agent(agent_id) => {
            let nodes = main_jsonl
                .as_deref()
                .and_then(|m| crate::subagent::build_topology(m, false).ok())
                .unwrap_or_default();
            let sub_files = main_jsonl
                .as_deref()
                .and_then(|m| crate::subagent::subagent_transcript_files(m).ok())
                .unwrap_or_default();
            let locate_sub = |hex: &str| {
                sub_files
                    .iter()
                    .find(|p| crate::subagent::session_id_from_path(p) == hex)
                    .cloned()
            };

            let mut chain: Vec<WhoNode> = Vec::new();
            let mut cur = Some(agent_id);
            // Walk `parent_agent_id` up; the dedup guard makes any malformed cycle terminate.
            while let Some(hex) = cur {
                if chain.iter().any(|n| n.session_id == hex) {
                    break;
                }
                let node = nodes.iter().find(|n| n.agent_id == hex);
                let depth = node.map(|n| n.depth);
                let next = node.and_then(|n| n.parent_agent_id.clone());
                let path = locate_sub(&hex);
                chain.push(WhoNode {
                    session_id: hex,
                    is_subagent: true,
                    parent_session_id: root.clone(),
                    depth,
                    path,
                });
                cur = next;
            }
            // Append the top-level session as the chain root.
            chain.push(WhoNode {
                parent_session_id: root.clone(),
                session_id: root,
                is_subagent: false,
                depth: None,
                path: main_jsonl,
            });
            Ok(chain)
        }
    }
}

/// Markers that appear as the DOCUMENTED EXAMPLE in the SKILL / `--help` / SPEC. Because the doc
/// text prints them right next to the literal `csift`, any command that quotes or greps that doc
/// satisfies the marker-AND-`csift` scan; worse, every agent that lazily copies the example uses
/// the SAME token, so it can never resolve to ONE transcript (it self-collides into ambiguity).
/// They are RESERVED - [`validate_trap_marker`] always refuses them, forcing a fresh hand-invented
/// marker. Keep this in lockstep with the example literal shown in every doc.
pub(crate) const RESERVED_EXAMPLE_MARKERS: &[&str] = &["JollyShinyBrook4283"];

/// Enforce the STRICT `@trap` marker grammar, rejecting every lazy shortcut at the source so the
/// only way to satisfy it is to invent a fresh, imaginative token by hand. The marker must be
/// EXACTLY 3 CamelCase words (each an uppercase letter + at least two lowercase letters - no single
/// letters, no ALLCAPS acronyms like `HTML` / `USB`) followed by EXACTLY four digits, and those four
/// digits must NOT form a trivial run (all-equal / consecutive / simple odd / simple even - e.g.
/// `0000` / `1234` / `9876` / `1357` / `2468`). The grammar-valid SHAPE looks like
/// `JollyShinyBrook4283`, but that exact literal is the RESERVED doc example (see
/// `RESERVED_EXAMPLE_MARKERS`) and is always refused, so nobody ships a copy-pasted token. The
/// strictness IS
/// the point: it makes a hand-invented, imaginative, CONTEXT-INDEPENDENT literary token the path of
/// least resistance and a scripted or boilerplate token fail loudly.
pub(crate) fn validate_trap_marker(marker: &str) -> Result<()> {
    let guidance = "@trap needs a marker you INVENT one-shot, right now, by hand: EXACTLY 3 \
                    imaginative, CONTEXT-INDEPENDENT CamelCase words (each 1 uppercase + >=2 \
                    lowercase) + 4 non-trivial digits. The shape looks like `JollyShinyBrook4283`, \
                    but that literal is the RESERVED doc example and is itself refused — pick your \
                    OWN. Put it VERBATIM in this csift command (no shell variable / concatenation), \
                    and never generate it with a script. Rejected: not exactly 3 words, \
                    single-letter or ALLCAPS \"words\", missing or !=4 trailing digits, trivial \
                    digits (1111 / 1234 / 9876 / 1357 / 2468 ...), or the reserved doc example.";
    if marker.is_empty() {
        bail!("{guidance}");
    }
    if RESERVED_EXAMPLE_MARKERS.contains(&marker) {
        bail!(
            "@trap: marker `{marker}` is the RESERVED documentation example — the SKILL / --help / \
             SPEC print it next to `csift`, so quoting the doc self-matches it and every agent that \
             copies it clashes into ambiguity. csift always refuses it. {guidance}"
        );
    }
    if !marker.is_ascii() || marker.len() < 13 {
        bail!("@trap: marker `{marker}` is malformed. {guidance}");
    }
    let (words, digits) = marker.split_at(marker.len() - 4);
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        bail!("@trap: marker `{marker}` must END with exactly 4 digits. {guidance}");
    }
    match camel_words(words) {
        Some(w) if w.len() == 3 => {}
        _ => bail!(
            "@trap: marker `{marker}` must be EXACTLY 3 CamelCase words (each: 1 uppercase + >=2 \
             lowercase) before the 4 digits. {guidance}"
        ),
    }
    if is_trivial_4_digits(digits) {
        bail!(
            "@trap: the 4 digits `{digits}` are a trivial run — pick non-sequential random \
             digits. {guidance}"
        );
    }
    Ok(())
}

/// Split a CamelCase run into its words, requiring each to be one uppercase letter followed by
/// `>=2` lowercase letters; returns `None` the moment that shape is violated (a digit, a
/// lone/2-char "word", an ALLCAPS acronym, or any non-ASCII-letter). Used only by
/// [`validate_trap_marker`].
pub(crate) fn camel_words(s: &str) -> Option<Vec<&str>> {
    let b = s.as_bytes();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_uppercase() {
            return None;
        }
        let start = i;
        i += 1;
        let mut lower = 0;
        while i < b.len() && b[i].is_ascii_lowercase() {
            i += 1;
            lower += 1;
        }
        if lower < 2 {
            return None;
        }
        words.push(&s[start..i]);
    }
    (!words.is_empty()).then_some(words)
}

/// True when four ASCII digits form a trivial arithmetic run - a constant step in `-2..=2`
/// (all-equal `0` / consecutive `±1` / simple odd-or-even `±2`), e.g. `0000` `1234` `9876` `1357`
/// `2468`. Such markers are too guessable / boilerplate to make a unique trap. Caller guarantees
/// exactly four ASCII digits.
pub(crate) fn is_trivial_4_digits(d: &str) -> bool {
    let v: Vec<i32> = d.bytes().map(|b| i32::from(b - b'0')).collect();
    let s1 = v[1] - v[0];
    let s2 = v[2] - v[1];
    let s3 = v[3] - v[2];
    s1 == s2 && s2 == s3 && (-2..=2).contains(&s1)
}

/// Locate a session's top-level `<id>.jsonl` under the projects root: try the cwd-encoded dir
/// first, then scan every project dir. `None` if absent. (Mirrors `whoami`'s locate logic.)
pub(crate) fn locate_session_jsonl(id: &str) -> Option<PathBuf> {
    let root = projects_root().ok()?;
    let fname = format!("{id}.jsonl");
    if let Ok(cwd) = std::env::current_dir() {
        let c = root.join(encode_cwd(&cwd)).join(&fname);
        if c.is_file() {
            return Some(c);
        }
    }
    for pd in all_project_dirs().ok()? {
        let c = pd.dir.join(&fname);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

/// True when `path`'s transcript contains a Bash `tool_use` whose `input.command` includes BOTH
/// the `marker` AND the literal `csift` - i.e. the actual csift invocation that embedded the
/// trap, not some unrelated command that merely echoed the token. A byte prefilter (`memmem` on
/// the rare marker) skips a transcript that never mentions it without parsing - so a giant main
/// transcript is mmap-scanned, not deserialized, unless the (unique) marker is present. Matching
/// the SHELL tool_use INPUT (`Bash`, or Windows' `PowerShell` tool - not anywhere) avoids a
/// false hit on a tool_result that echoed it.
pub(crate) fn bash_command_carries_trap(path: &Path, marker: &str) -> bool {
    let Ok(Some(mmap)) = crate::parse::mmap_bytes(path) else {
        return false;
    };
    let bytes: &[u8] = &mmap;
    if memchr::memmem::find(bytes, marker.as_bytes()).is_none() {
        return false;
    }
    let mut found = false;
    let _ = crate::parse::scan_lines_bytes(bytes, |line| {
        if found {
            return;
        }
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    if let crate::model::Block::ToolUse {
                        name: Some(n),
                        input: Some(inp),
                        ..
                    } = b
                    {
                        // Both SHELL tools carry the invocation in `input.command`: `Bash`
                        // everywhere, and Windows' SEPARATE `PowerShell` tool (CC 2.1.228:
                        // the fallback when Git-for-Windows bash is absent, or the gated
                        // preference - same `command` field, verbatim from the binary's
                        // tool registry). A Bash-only gate left @trap blind exactly on the
                        // mandatory Windows fallback.
                        if (n == "Bash" || n == "PowerShell")
                            && inp
                                .get("command")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|c| c.contains(marker) && c.contains("csift"))
                        {
                            found = true;
                        }
                    }
                }
            }
        }
    });
    found
}
