use super::*;
use crate::cli::OutputFormat;

fn args(pattern: &str) -> SearchArgs {
    SearchArgs {
        pattern: pattern.to_string(),
        subagents: false,
        paths: Vec::new(),
        sessions_from: None,
        labels: Vec::new(),
        labels_not: Vec::new(),
        raw: false,
        ignore_case: false,
        multiline: false,
        turn_range: None,
        since: None,
        until: None,
        max_count: None,
        count_only: false,
        sessions_with_matches: false,
        count_by: None,
        siblings: false,
        no_truncate: false,
        resolve_persisted: false,
        additional_context: false,
        no_subagents: false,
        format: OutputFormat::Text,
    }
}

fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("valid record")
}

/// A neutral top-level [`ClassifyEnv`] for the `collect_turn_hits` unit tests (no subagent,
/// no spawn lookup) — the per-record ctx degrades to [`ClassifyCtx::top_level`]'s behavior.
fn test_env() -> ClassifyEnv<'static> {
    ClassifyEnv {
        owner_id: "0a1b2c3d-0000-0000-0000-000000000000",
        is_subagent: false,
        parent_id: "0a1b2c3d-0000-0000-0000-000000000000",
        first_opener_line: None,
        spawn: None,
    }
}

/// The synthetic session, one jsonl line per element, in file order.
fn fixture() -> Vec<&'static str> {
    vec![
        // ── turn 0 ──
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"why is the tail-read carry needed?"}}"#,
        r#"{"type":"assistant","uuid":"a0t","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"the carry holds an incomplete line straddling a chunk boundary"}]}}"#,
        r#"{"type":"assistant","uuid":"a0u","parentUuid":"a0t","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Read","input":{"file":"parse.rs"}}]}}"#,
        r#"{"type":"user","uuid":"c0","parentUuid":"a0u","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call0","content":"the carry is the partial line at the low edge"}]}}"#,
        r#"{"type":"assistant","uuid":"a0f","parentUuid":"c0","timestamp":"2026-06-07T05:00:40.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The carry is the partial line at the low-offset edge of each chunk."}]}}"#,
        // An isMeta pseudo-turn — looks human, must NOT open a turn.
        r#"{"type":"user","uuid":"meta","isMeta":true,"parentUuid":"a0f","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#,
        // ── turn 1 ──
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"now explain the panic path"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"There is no panic on a malformed line — it is skipped and counted."}]}}"#,
    ]
}

fn search(lines: &[&str], a: &SearchArgs) -> Vec<Exchange> {
    let matcher = build_matcher(a).unwrap();
    let kept = kept_fixture(lines, &matcher);
    let tr = a
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()
        .unwrap();
    let tw = TimeWindow::from_args(a.since.as_deref(), a.until.as_deref()).unwrap();
    // The fixture path is a non-existent top-level transcript, so its discovery-root resolves
    // no subagents — an empty spawn map (lookup miss ⇒ `None`) reproduces exactly what the
    // former per-file `build_spawn_lookup` returned here.
    let spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
    let (exchanges, _turn_count) = reconstruct_and_match(
        std::path::Path::new("/x/0a1b2c3d-0000-0000-0000-000000000000.jsonl"),
        &kept,
        a,
        &matcher,
        tr,
        &tw,
        None,
        a.siblings,
        &spawn_map,
        false,
    );
    exchanges
}

fn kept_fixture(lines: &[&str], matcher: &Matcher) -> Vec<Kept> {
    lines
        .iter()
        .map(|l| {
            let raw = l.as_bytes();
            Kept {
                rec: serde_json::from_slice(raw).expect("valid fixture record"),
                can_hit: matcher.line_may_match(raw),
                line_no: 1,
                from_sidecar: false,
            }
        })
        .collect()
}

mod part01;
mod part02;
mod part03;
