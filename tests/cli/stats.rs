use crate::harness::*;

#[test]
fn stats_spans_subagents_by_default_and_restricts() {
    // Mutation pin on the span contract (§ subcommand spanning default): `stats` spans the
    // session's subagent transcripts by default; `--no-subagents` restricts to the top level.
    let h = Home::new();
    subagents_only_scenario(&h);
    let span = h.run(&["stats", at(SESS).as_str()]);
    assert!(span.success, "stderr: {}", span.stderr);
    assert!(
        span.stdout.contains("sub111"),
        "stats spans subagents by default: {}",
        span.stdout
    );
    let top = h.run(&["stats", at(SESS).as_str(), "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(
        !top.stdout.contains("sub111"),
        "--no-subagents restricts stats to the top level: {}",
        top.stdout
    );
}
