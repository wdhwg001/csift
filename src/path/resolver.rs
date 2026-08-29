//! resolve_session_files: the @-grammar resolver + agent-subtree dispatch.

use super::*;

/// Resolve positional targets into the concrete, sorted, de-duplicated list of session
/// `*.jsonl` files to operate on, with the subagent span governed by `scope`.
///
/// This is the SINGLE shared target resolver for `list` / `search` / `agents` / `files` /
/// `recover` / `turns` / `image`. There is NO `--session` flag - a session is targeted by a
/// positional `@<uuid>` / `@<agent-hex>` / `@main` / `@trap:<marker>` token or a `*.jsonl` file; a real
/// path / encoded-dir token / `~/.claude/projects/<enc>` scopes to project dir(s). 0 `paths` ⇒
/// every project under the projects root. The subagent transcripts of a selected session
/// (built-in Task/Agent-tool + workflow / OMC agents under `subagents/**`) are gathered from
/// the already-id-filtered top-level set; workflow `journal.jsonl` event logs are never
/// transcripts and are excluded (see [`crate::subagent::subagent_transcript_files`]). Per
/// [`SubagentScope`]:
/// - `WithSubagents` - top-level session(s) + their subagents (the default).
/// - `TopLevelOnly` - only the top-level `<uuid>.jsonl` session(s).
///
/// Bails (never returns an empty silent result) when a session id was pinned but no matching
/// file exists under the resolved target(s). With no id pin, an empty result is allowed (the
/// caller renders an honest "nothing found").
pub fn resolve_session_files(
    paths: &[std::path::PathBuf],
    scope: SubagentScope,
    caller: Caller,
) -> Result<Vec<PathBuf>> {
    let Targets {
        session_ids,
        session_prefixes,
        mut agent_hexes,
        project_paths,
        explicit_dirs,
        session_target,
    } = collect_targets(paths)?;

    let mut dirs: Vec<ProjectDir> = explicit_dirs;
    if !project_paths.is_empty() {
        for p in &project_paths {
            dirs.push(resolve_target(p)?);
        }
    } else if dirs.is_empty() {
        // No project / encoded / jsonl target: scan every project (a uuid-only or `@main`
        // invocation searches all projects for that session).
        dirs = all_project_dirs()?;
    }

    let _ = caller; // reserved for future subcommand-aware guidance
    let mut files: Vec<PathBuf> = Vec::new();

    // ── SESSION path: the top-level `<uuid>.jsonl` session files (+ subagents per scope).
    // Skipped for an AGENT-ONLY invocation (no session target), so `@<agent-hex>` alone does
    // not list every session - only the agent subtree below runs.
    let session_path_active = session_target || agent_hexes.is_empty();
    if session_path_active {
        let (mut top_level, prefix_hits, prefix_agent_hits) =
            scan_top_level(&dirs, &session_ids, &session_prefixes);

        resolve_prefix_uniqueness(
            &session_prefixes,
            &prefix_hits,
            &prefix_agent_hits,
            &mut agent_hexes,
        )?;

        // Subagent transcripts of each selected top-level session (empty unless scoped in).
        let mut sub_files: Vec<PathBuf> = Vec::new();
        if matches!(scope, SubagentScope::WithSubagents) {
            for sf in &top_level {
                sub_files.extend(crate::subagent::subagent_transcript_files(sf)?);
            }
        }
        let session_files: Vec<PathBuf> = match scope {
            SubagentScope::TopLevelOnly => top_level,
            SubagentScope::WithSubagents => {
                top_level.extend(sub_files);
                top_level
            }
        };
        // A given SESSION id that matched nothing is an honest error (a prefix already bailed).
        if session_files.is_empty() && !session_ids.is_empty() {
            bail!(
                "no session file found for session id [{}] under the resolved target(s)",
                session_ids.join(", ")
            );
        }
        files.extend(session_files);
    }

    // ── AGENT path: each `@<agent-hex>` resolves to the subagent + (unless `--no-subagents`)
    // its TOPOLOGICAL descendants. Errors when no such agent exists in scope.
    for hex in &agent_hexes {
        files.extend(resolve_agent_subtree(&dirs, hex, scope)?);
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// The classified positional targets - one pass over the raw tokens.
///
/// Target grammar (no `--session` flag - a session is an `@<uuid>` / `@main` / `@trap:<marker>`
/// positional, or a `*.jsonl` file). A token is one of: an `@`-prefixed IDENTIFIER (env /
/// session-id / encoded-dir), a `*.jsonl` session file, or a PATH (real cwd, encoded-dir
/// token, or `~/.claude/projects/<enc>`). A BARE uuid is NOT special - it falls to the path
/// branch and fails as "no project dir named <uuid>" (forced-unique: a folder literally named
/// like a uuid would otherwise be ambiguous). The result selects sessions whose basename
/// matches ANY collected id.
struct Targets {
    session_ids: Vec<String>,
    /// Session-UUID PREFIXES (`@13d9645a` - the leading hex of a uuid, e.g. its first
    /// segment): resolved by prefix-match against the enumerated sessions, UNIQUE or an
    /// ambiguity error.
    session_prefixes: Vec<String>,
    /// AGENT targets (`@<agent-hex>` / `@trap:<marker>`→agent / a subagent `*.jsonl`): each
    /// resolves to that subagent + (unless `--no-subagents`) its TOPOLOGICAL descendants.
    agent_hexes: Vec<String>,
    project_paths: Vec<std::path::PathBuf>,
    /// Dirs resolved DIRECTLY from a token (an `@<encoded>` id or a `*.jsonl` file) - kept
    /// apart from `project_paths` so they don't trigger the all-projects scan.
    explicit_dirs: Vec<ProjectDir>,
    /// True once any SESSION/PROJECT target is seen, so the all-session enumeration runs. An
    /// AGENT-ONLY invocation (e.g. just `@<agent-hex>`) leaves it false → only the agent path
    /// runs.
    session_target: bool,
}

fn collect_targets(paths: &[std::path::PathBuf]) -> Result<Targets> {
    let mut session_ids: Vec<String> = Vec::new();
    let mut session_prefixes: Vec<String> = Vec::new();
    let mut agent_hexes: Vec<String> = Vec::new();
    let mut project_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut explicit_dirs: Vec<ProjectDir> = Vec::new();
    let mut session_target = false;
    for p in paths {
        let t = p.to_str().unwrap_or_default();
        // `@`-prefixed IDENTIFIER tokens (never a path): `@main` (env), `@trap:<marker>` (find
        // the CALLER's transcript by a unique literal marker it embedded in this command),
        // `@<uuid>`, `@<agent-hex>` (the agent subtree), `@<uuid-prefix>` (leading hex → unique
        // session), `@<encoded-dir>` (a project dir by its encoded name).
        if let Some(id) = t.strip_prefix('@') {
            match id {
                "main" => {
                    session_ids.push(resolve_env_session()?);
                    session_target = true;
                }
                // `@trap:<marker>` - the SELF identifier. The caller (an in-process subagent
                // whose own id CC withholds from the env) puts a unique, LITERAL marker in this
                // very command; csift finds the transcript whose Bash tool_use carries it. A
                // subagent match → that agent's subtree; a main-thread match → the session.
                _ if id.starts_with("trap:") => {
                    match resolve_trap(id.strip_prefix("trap:").unwrap_or(""))? {
                        TrapSelf::Agent(hex) => agent_hexes.push(hex),
                        TrapSelf::Session(sid) => {
                            session_ids.push(sid);
                            session_target = true;
                        }
                    }
                }
                _ if is_uuid(id) => {
                    session_ids.push(id.to_string());
                    session_target = true;
                }
                _ if is_subagent_id(id) => agent_hexes.push(id.to_string()),
                // A short dashless hex run (4..=11) is a uuid PREFIX (the first segment is 8),
                // never a full uuid (32+dashes) or an agent hex (≥12) - resolve it uniquely.
                _ if is_uuid_prefix(id) => {
                    session_prefixes.push(id.to_string());
                    session_target = true;
                }
                // `@-Users-…` / `@C--Users-…` → an encoded project-dir name (a Unix cwd's
                // leading `/` encodes to `-`; a Windows cwd's `C:\` encodes to `C--`).
                _ if id.starts_with('-') || is_drive_encoded_token(id) => {
                    explicit_dirs.push(resolve_target(Path::new(id))?);
                    session_target = true;
                }
                // Any other `@`-token is an UNRECOGNIZED id shape: fail loud naming the
                // @-grammar. It must NEVER fall through to path resolution - a stripped
                // `@a` used to become the cwd-relative path `a` and report a misleading
                // "no Claude Code project dir", sending the caller down a filesystem
                // debugging trail for what is an ID typo (the one spot the fail-loud
                // targeting law was silently violated).
                _ => {
                    let hexish = !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit());
                    if hexish && id.len() < 4 {
                        bail!(
                            "`@{id}` is too short for a session-uuid prefix — a prefix \
                             needs 4-11 leading (dashless) hex chars; add more characters \
                             (`csift list` shows the full uuids)"
                        );
                    }
                    bail!(
                        "`@{id}` is not a recognized @-target — expected `@<uuid>` | \
                         `@<uuid-prefix>` (4-11 dashless leading hex) | `@<agent-id>` \
                         (what `csift agents` prints) | `@main` | `@trap:<marker>` | \
                         `@-Users-…` / `@C--Users-…` (an encoded project dir). A project \
                         PATH is targeted without `@`."
                    );
                }
            }
            continue;
        }
        // A `*.jsonl` transcript target. A SUBAGENT transcript → that agent (+ its subtree); a
        // top-level `<uuid>.jsonl` → that session. Either way its project dir scopes the search.
        if t.ends_with(".jsonl") {
            // Absolutize FIRST: a bare-basename token (`x.jsonl` with no separator) has
            // `parent() == Some("")`, which used to feed an empty dir into the scan (a
            // tolerated read_dir failure ending in a wrong "no session file found" bail)
            // AND made the `subagents` component test misclassify a bare
            // `agent-<hex>.jsonl` as a top-level session. One canonical form fixes the
            // dir, the classification, and the sidecar sniff together.
            let file = crate::path::absolutize(Path::new(t))?;
            let file = file.as_path();
            // An elicitation SIDECAR (hook-written backfill, csift-elicitation marker records
            // only) is not a Claude Code transcript - it is read AUTOMATICALLY when you target
            // its session and cannot be searched directly. Reject it loudly so a stray target is
            // never silently scanned as a session (the merge is the only supported access).
            if crate::elicitation::is_sidecar_path(file) {
                bail!(
                    "{} is a csift elicitation sidecar (hook-written backfill, \
                     csift-elicitation marker records only), not a Claude Code session \
                     transcript. It is read automatically when you target its session; it \
                     cannot be searched directly.",
                    file.display()
                );
            }
            let is_sub = file
                .components()
                .any(|c| c.as_os_str().to_str() == Some("subagents"));
            let (dir, sid) = session_file_target(file)?;
            explicit_dirs.push(dir);
            if is_sub {
                agent_hexes.push(crate::subagent::session_id_from_path(file));
            } else {
                session_ids.push(sid);
                session_target = true;
            }
            continue;
        }
        // A BARE session/agent id (no `@`) can never be a project path in practice, and
        // the old fall-through error ("no project dir named …") described the WRONG
        // mental model back to the caller. Catch the id SHAPE (unless a real path of
        // that name exists) and say the exact fix.
        if let Some(tok) = p.to_str() {
            if (is_uuid(tok) || is_uuid_prefix(tok) || is_subagent_id(tok)) && !p.exists() {
                bail!(
                    "'{tok}' looks like a session/agent id, not a project path — did you \
                     mean '@{tok}'? (ids are targeted with an `@` prefix: `@<uuid>` | \
                     `@<uuid-prefix>` | `@<agent-id>`)"
                );
            }
        }
        // Everything else is a PATH (real cwd / encoded-dir token / `~/.claude/projects/<enc>`).
        project_paths.push(p.clone());
        session_target = true;
    }
    Ok(Targets {
        session_ids,
        session_prefixes,
        agent_hexes,
        project_paths,
        explicit_dirs,
        session_target,
    })
}

/// Per-prefix hit sets, keyed by the prefix token.
type PrefixHits = std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

/// Enumerate top-level `<uuid>.jsonl` files under `dirs`, honoring the id/prefix filter and
/// the SPEC 2.1 cwd collision guard, and collect the prefix-match hits over the UNION domain
/// (top-level uuids + subagent agent ids - `search` emits an id-prefix header token for
/// subagent exchanges too, and every emitted token must round-trip as an `@` target).
fn scan_top_level(
    dirs: &[ProjectDir],
    session_ids: &[String],
    session_prefixes: &[String],
) -> (Vec<PathBuf>, PrefixHits, PrefixHits) {
    let mut top_level: Vec<PathBuf> = Vec::new();
    let mut prefix_hits: PrefixHits = PrefixHits::new();
    let mut prefix_agent_hits: PrefixHits = PrefixHits::new();
    let have_filter = !session_ids.is_empty() || !session_prefixes.is_empty();
    for pd in dirs {
        let read = match std::fs::read_dir(&pd.dir) {
            Ok(r) => r,
            Err(_) => continue, // tolerate a vanished dir mid-scan
        };
        for entry in read.flatten() {
            let p = entry.path();
            let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
            if is_file && p.extension().is_some_and(|e| e == "jsonl") {
                admit_entry(
                    p,
                    pd,
                    session_ids,
                    session_prefixes,
                    have_filter,
                    &mut top_level,
                    &mut prefix_hits,
                    &mut prefix_agent_hits,
                );
            }
        }
    }
    (top_level, prefix_hits, prefix_agent_hits)
}

/// Admit one candidate `<stem>.jsonl`: the cwd COLLISION GUARD (SPEC 2.1 - a dir resolved
/// from a REAL path may be shared by a DIFFERENT cwd under the lossy encoding, so keep only
/// files whose recorded `cwd` IS this target; a file whose `cwd` is absent is kept), then the
/// UNION-DOMAIN prefix collection (a prefix may name a subagent of a session that itself does
/// NOT match - cost paid only on a prefix-targeted invocation), then the exact-id / prefix
/// keep decision.
#[allow(clippy::too_many_arguments)]
fn admit_entry(
    p: PathBuf,
    pd: &ProjectDir,
    session_ids: &[String],
    session_prefixes: &[String],
    have_filter: bool,
    top_level: &mut Vec<PathBuf>,
    prefix_hits: &mut PrefixHits,
    prefix_agent_hits: &mut PrefixHits,
) {
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    if let Some(want) = &pd.target_cwd {
        if let Some(stored) = read_first_cwd(&p) {
            if !cwd_equivalent(&stored, want) {
                return;
            }
        }
    }
    if !session_prefixes.is_empty() {
        if let Ok(subs) = crate::subagent::subagent_transcript_files(&p) {
            for sp in subs {
                let sid = crate::subagent::session_id_from_path(&sp);
                for pfx in session_prefixes {
                    if sid.starts_with(pfx.as_str()) {
                        prefix_agent_hits
                            .entry(pfx.clone())
                            .or_default()
                            .insert(sid.clone());
                    }
                }
            }
        }
    }
    // An exact id OR a uuid-PREFIX (`@13d9645a…`) keeps the file.
    let matched_prefix = session_prefixes
        .iter()
        .find(|pfx| stem.starts_with(pfx.as_str()))
        .cloned();
    if have_filter {
        let by_id = session_ids.iter().any(|sid| sid == &stem);
        if !by_id && matched_prefix.is_none() {
            return;
        }
    }
    if let Some(pfx) = matched_prefix {
        prefix_hits.entry(pfx).or_default().insert(stem);
    }
    top_level.push(p);
}

/// A PREFIX must resolve to EXACTLY ONE id across the union domain - else error (never
/// silently pick). A unique SUBAGENT match dispatches exactly like a full `@<agent-id>`
/// target; the top-level scan kept no file for it, so only the agent path emits.
fn resolve_prefix_uniqueness(
    session_prefixes: &[String],
    prefix_hits: &PrefixHits,
    prefix_agent_hits: &PrefixHits,
    agent_hexes: &mut Vec<String>,
) -> Result<()> {
    for pfx in session_prefixes {
        let top = prefix_hits.get(pfx);
        let agents = prefix_agent_hits.get(pfx);
        let n_top = top.map_or(0, std::collections::BTreeSet::len);
        let n_agents = agents.map_or(0, std::collections::BTreeSet::len);
        match n_top + n_agents {
            0 => bail!(
                "no session or agent id starts with `{pfx}` under the resolved target(s) — \
                 check the prefix, or widen the scope."
            ),
            1 => {
                if let Some(a) = agents.and_then(|s| s.iter().next()) {
                    agent_hexes.push(a.clone());
                }
            }
            n => {
                let ids: Vec<&str> = top
                    .into_iter()
                    .flatten()
                    .chain(agents.into_iter().flatten())
                    .map(String::as_str)
                    .collect();
                bail!(
                    "`@{pfx}` is AMBIGUOUS: {n} ids start with it ({}). Use more of the id.",
                    ids.join(", ")
                );
            }
        }
    }
    Ok(())
}

/// Resolve an `@<agent-hex>` target: the subagent's OWN transcript plus (unless
/// `--no-subagents`) its TOPOLOGICAL descendants. Scans `dirs` for the session that owns the
/// agent (its hex is globally unique), builds that session's topology, and emits transcripts
/// per `scope`: `TopLevelOnly` = the agent alone; `WithSubagents` = the agent + descendants.
/// Errors when no such agent exists in scope.
pub(crate) fn resolve_agent_subtree(
    dirs: &[ProjectDir],
    hex: &str,
    scope: SubagentScope,
) -> Result<Vec<PathBuf>> {
    for pd in dirs {
        for top in top_level_jsonls(&pd.dir) {
            let subs = crate::subagent::subagent_transcript_files(&top)?;
            // agent_id (bare hex) → its transcript path, for this session.
            let by_id: std::collections::HashMap<String, PathBuf> = subs
                .iter()
                .map(|p| (crate::subagent::session_id_from_path(p), p.clone()))
                .collect();
            if !by_id.contains_key(hex) {
                continue; // not this session's agent
            }
            // Found the owning session. The descendants come from the agent→agent topology
            // (`parent_agent_id`); on flat real data an agent has none, so the result is just
            // the agent itself - correct, and it nests automatically once CC nests subagents.
            let nodes = crate::subagent::build_topology(&top, false)?;
            let descendants = subtree_agent_ids(&nodes, hex);
            let mut out: Vec<PathBuf> = Vec::new();
            // The agent's OWN transcript is always included (both scopes keep the target itself).
            if let Some(p) = by_id.get(hex) {
                out.push(p.clone());
            }
            if !matches!(scope, SubagentScope::TopLevelOnly) {
                for d in &descendants {
                    if let Some(p) = by_id.get(d) {
                        out.push(p.clone());
                    }
                }
            }
            return Ok(out);
        }
    }
    // Exact id matched nothing. A collision-lengthened `search` header token is a PREFIX of a
    // bare-hex agent id (12 hex chars routes here as an exact-id shape), so before giving up,
    // try a unique literal-prefix match over the in-scope agent ids - fail-loud on ambiguity,
    // a unique hit resolves exactly like the full id.
    if hex.len() >= 12 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pd in dirs {
            for top in top_level_jsonls(&pd.dir) {
                for p in crate::subagent::subagent_transcript_files(&top)? {
                    let sid = crate::subagent::session_id_from_path(&p);
                    if sid.starts_with(hex) {
                        hits.insert(sid);
                    }
                }
            }
        }
        match hits.len() {
            1 => {
                let full = hits.iter().next().map(String::as_str).unwrap_or(hex);
                return resolve_agent_subtree(dirs, full, scope);
            }
            n if n > 1 => {
                let ids: Vec<&str> = hits.iter().map(String::as_str).collect();
                bail!(
                    "`@{hex}` is AMBIGUOUS: {n} agent ids start with it ({}). Use more of the id.",
                    ids.join(", ")
                );
            }
            _ => {}
        }
    }
    bail!(
        "no subagent `{hex}` found under the resolved target(s). List ids with \
         `csift agents <session>`, then pass one as `@<agent-hex>`."
    )
}

/// The bare-hex agent ids in `nodes` that DESCEND from `root` (children, grandchildren, …) via
/// the `parent_agent_id` chain. Excludes `root` itself. Cycle-safe (a `visited` set).
pub(crate) fn subtree_agent_ids(
    nodes: &[crate::subagent::SubagentNode],
    root: &str,
) -> Vec<String> {
    let mut children: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for n in nodes {
        if let Some(p) = n.parent_agent_id.as_deref() {
            children.entry(p).or_default().push(n.agent_id.as_str());
        }
    }
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(cur) = stack.pop() {
        if let Some(kids) = children.get(cur) {
            for &k in kids {
                if visited.insert(k) {
                    out.push(k.to_string());
                    stack.push(k);
                }
            }
        }
    }
    out
}

/// The top-level `<uuid>.jsonl` session files directly in `dir` (non-recursive). Tolerates an
/// unreadable/vanished dir (empty result).
pub(crate) fn top_level_jsonls(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let p = entry.path();
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                && p.extension().is_some_and(|e| e == "jsonl")
            {
                out.push(p);
            }
        }
    }
    out
}

/// True when a TARGET token pins a SINGLE transcript/session (so `search`'s empty-pattern
/// warning knows a session filter is present, and `show` can resolve one file): `@main`, a
/// `@trap:<marker>` self-token, an `@<uuid>`/`@<agent-hex>`/`@<uuid-prefix>` id, or a `*.jsonl`
/// file. A plain path or encoded-dir token can span many sessions, so it does NOT pin.
#[must_use]
pub fn pins_single_session(token: &str) -> bool {
    if let Some(id) = token.strip_prefix('@') {
        return id == "main"
            || id.starts_with("trap:")
            || is_uuid(id)
            || is_subagent_id(id)
            || is_uuid_prefix(id);
    }
    token.ends_with(".jsonl")
}
