//! C-34: the resume repair pair, `harness.resume.prompt` + `harness.resume.placeholder`.
//!
//! Both leaves are reachable by their own path, by the `harness.resume` prefix and by the
//! bare `harness` role (no gate); neither is a `user` or an `agent` record; the placeholder
//! says whether it closes a pair; and the retired `harness.schedule.continuation` spelling
//! errors with its successor.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "5c4b3a29-1e0d-4c3b-9a87-6f5e4d3c2b1a";

/// The live receipt's shape: a prompt the human sent, its reply, then a DANGLING prompt
/// (L3) with its submit attachment (L4) - the tail the loader repairs. On the next resume
/// the loader appends the repair PROMPT (L5) and splices the PLACEHOLDER (L6) in after it,
/// both with one identical timestamp; the human's next real prompt (L7) chains after them.
/// L8 is a SECOND placeholder from the same splice with an interrupt marker (L9's partner)
/// as its parent - the unpaired form.
fn resume_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef pass please"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"the reef pass is charted"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"now sound the shoal margin"}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","parentUuid":"u2","timestamp":"2026-06-07T05:02:00.100Z","attachment":{"type":"file_snapshot","content":"soundings.csv"}}"#, "\n",
            r#"{"type":"user","uuid":"p1","parentUuid":"att1","isMeta":true,"promptId":"pr-9","userType":"external","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"h1","parentUuid":"p1","isApiErrorMessage":false,"timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"h1","timestamp":"2026-06-07T06:01:00.000Z","message":{"role":"user","content":"resume the sounding run"}}"#, "\n",
            r#"{"type":"user","uuid":"int1","parentUuid":"u3","timestamp":"2026-06-07T06:05:00.000Z","message":{"role":"user","content":"[Request interrupted by user]"}}"#, "\n",
            r#"{"type":"assistant","uuid":"h2","parentUuid":"int1","isApiErrorMessage":false,"timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn both_halves_carry_their_own_leaf_under_the_prefix() {
    let h = resume_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.resume"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.resume.prompt"),
        "the prefix must reach the prompt:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("harness.resume.placeholder"),
        "the prefix must reach the placeholder:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("Continue from where you left off.")
            && out.stdout.contains("No response requested."),
        "both render VERBATIM:\n{}",
        out.stdout
    );
    // Each full path reaches exactly its own half.
    let prompt = h.run(&["search", "", &at(SESS), "-t", "harness.resume.prompt"]);
    assert!(
        prompt.stdout.contains("Continue from where"),
        "{}",
        prompt.stdout
    );
    assert!(
        !prompt.stdout.contains("No response requested"),
        "{}",
        prompt.stdout
    );
    let ph = h.run(&["search", "", &at(SESS), "-t", "harness.resume.placeholder"]);
    assert!(ph.stdout.contains("No response requested"), "{}", ph.stdout);
    assert!(!ph.stdout.contains("Continue from where"), "{}", ph.stdout);
}

#[test]
fn the_bare_harness_role_reaches_both_and_user_and_agent_reach_neither() {
    let h = resume_home();
    // No gate: both leaves are delivered records the role keep already admits, so a bare
    // `-t harness` finds them with no extra flag.
    let harness = h.run(&["search", "", &at(SESS), "-t", "harness"]);
    assert!(harness.success, "stderr: {}", harness.stderr);
    assert!(
        harness.stdout.contains("harness.resume.prompt")
            && harness.stdout.contains("harness.resume.placeholder"),
        "a bare harness role reaches both:\n{}",
        harness.stdout
    );
    // The prompt is the LOADER's text, not the operator's; the placeholder is fabricated,
    // not the model's. Neither may appear under the conversational roles.
    let user = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        !user.stdout.contains("Continue from where")
            && !user.stdout.contains("No response requested"),
        "the pair must not read as the human:\n{}",
        user.stdout
    );
    let agent = h.run(&["search", "", &at(SESS), "-t", "agent"]);
    assert!(
        !agent.stdout.contains("No response requested")
            && !agent.stdout.contains("Continue from where"),
        "the pair must not read as the assistant:\n{}",
        agent.stdout
    );
    // The human's own messages are still there under `user` - the filter is not just empty.
    assert!(user.stdout.contains("reef pass"), "{}", user.stdout);
}

#[test]
fn the_placeholder_says_whether_it_closes_a_pair() {
    let h = resume_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.resume.placeholder"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // One writer produces both forms: the splice fires after ANY trailing user record, and
    // the prompt is pushed only when the tail classifier called the turn interrupted.
    assert!(
        out.stdout.contains("harness.resume.placeholder [paired]"),
        "the L6 placeholder is parented on the prompt:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("harness.resume.placeholder [unpaired]"),
        "the L9 placeholder is parented on an interrupt marker:\n{}",
        out.stdout
    );
    // The same verdict rides JSON, per hit.
    let json = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.resume.placeholder",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    assert!(
        json.stdout.contains(r#""resume_paired":true"#)
            && json.stdout.contains(r#""resume_paired":false"#),
        "both verdicts ride JSON:\n{}",
        json.stdout
    );
    // A hit that is not a placeholder carries no verdict at all (never a guessed false).
    let any = h.run(&["search", "reef pass", &at(SESS), "--format", "json"]);
    assert!(
        any.stdout.contains(r#""resume_paired":null"#),
        "a non-placeholder hit carries null:\n{}",
        any.stdout
    );
}

#[test]
fn the_census_counts_both_leaves_and_the_json_labels_agree() {
    let h = resume_home();
    let census = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census.stdout.contains("harness.resume.prompt"),
        "{}",
        census.stdout
    );
    assert!(
        census.stdout.contains("harness.resume.placeholder"),
        "{}",
        census.stdout
    );
    // The placeholder census key counts BOTH forms - the pairing is a fact, not a leaf.
    let rows = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(
        rows.stdout
            .contains(r#""key":"harness.resume.placeholder","kind":"census","records":2"#),
        "both placeholders count under the one leaf:\n{}",
        rows.stdout
    );
    assert!(
        rows.stdout
            .contains(r#""key":"harness.resume.prompt","kind":"census","records":1"#),
        "{}",
        rows.stdout
    );
    // Each record carries exactly its own leaf - single-label, no agent.message twin.
    let json = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.resume",
        "--format",
        "json",
    ]);
    assert!(
        json.stdout.contains(
            r#""label":"harness.resume.placeholder","labels":["harness.resume.placeholder"]"#
        ),
        "the placeholder is single-labelled:\n{}",
        json.stdout
    );
    assert!(
        json.stdout
            .contains(r#""label":"harness.resume.prompt","labels":["harness.resume.prompt"]"#),
        "the prompt is single-labelled:\n{}",
        json.stdout
    );
}

#[test]
fn neither_half_opens_a_turn() {
    let h = resume_home();
    // Three human prompts open three turns; the repair pair sits INSIDE the third and adds
    // none of its own, so a resumed session's turn numbering is untouched by the repair.
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#""distinct_keys":3"#),
        "the pair opens no turn:\n{}",
        out.stdout
    );
}

#[test]
fn show_renders_each_half_by_address() {
    let h = resume_home();
    let prompt = h.run(&["show", &at(SESS), "--line", "5"]);
    assert!(prompt.success, "stderr: {}", prompt.stderr);
    assert!(
        prompt.stdout.contains("harness.resume.prompt")
            && prompt.stdout.contains("Continue from where you left off."),
        "{}",
        prompt.stdout
    );
    let ph = h.run(&["show", &at(SESS), "--line", "6"]);
    assert!(ph.success, "stderr: {}", ph.stderr);
    assert!(
        ph.stdout.contains("harness.resume.placeholder [paired]")
            && ph.stdout.contains("No response requested."),
        "{}",
        ph.stdout
    );
    let unpaired = h.run(&["show", &at(SESS), "--line", "9"]);
    assert!(unpaired.success, "stderr: {}", unpaired.stderr);
    assert!(
        unpaired
            .stdout
            .contains("harness.resume.placeholder [unpaired]"),
        "{}",
        unpaired.stdout
    );
}

#[test]
fn the_prompt_index_is_per_file() {
    // The pairing index is built per TRANSCRIPT. A placeholder whose parentUuid happens to
    // equal a repair prompt's uuid in a DIFFERENT file of the same scan is unpaired: uuids
    // are unique in practice, but a fork copies them, so the join must never reach across.
    let h = resume_home();
    let other = "8d7c6b5a-4e3f-4210-9876-5a4b3c2d1e0f";
    h.write(
        &format!("{ENC}/{other}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"z1","timestamp":"2026-06-08T05:00:00.000Z","message":{"role":"user","content":"a separate session entirely"}}"#, "\n",
            // parentUuid `p1` names the OTHER transcript's repair prompt; no prompt here.
            r#"{"type":"assistant","uuid":"z2","parentUuid":"p1","isApiErrorMessage":false,"timestamp":"2026-06-08T05:00:01.000Z","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        &at(other),
        "-t",
        "harness.resume.placeholder",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#""resume_paired":false"#),
        "a cross-file parent must not pair:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains(r#""resume_paired":true"#),
        "{}",
        out.stdout
    );
    // Scanning BOTH files together does not change the verdict either.
    let both = h.run(&[
        "search",
        "",
        "-t",
        "harness.resume.placeholder",
        "--format",
        "json",
    ]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert_eq!(
        both.stdout.matches(r#""resume_paired":true"#).count(),
        1,
        "only the same-file pair counts:\n{}",
        both.stdout
    );
    assert_eq!(
        both.stdout.matches(r#""resume_paired":false"#).count(),
        2,
        "the interrupt-parented one and the cross-file one:\n{}",
        both.stdout
    );
}

#[test]
fn a_human_message_opening_with_the_sentence_stays_the_human() {
    // The isMeta gate: a person can begin a real prompt with the loader's sentence, and that
    // message must stay theirs - labelled user.message, opening its own turn.
    let h = Home::new();
    let sess = "2b1a0908-7f6e-4d5c-8b4a-3928170f6e5d";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"Continue from where you left off. and also chart the reef pass"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"charting now"}]}}"#, "\n",
        ),
    );
    let user = h.run(&["search", "", &at(sess), "-t", "user"]);
    assert!(user.success, "stderr: {}", user.stderr);
    assert!(
        user.stdout.contains("user.message") && user.stdout.contains("also chart the reef pass"),
        "a typed message opening with the sentence is the human:\n{}",
        user.stdout
    );
    let resume = h.run(&["search", "", &at(sess), "-t", "harness.resume"]);
    assert!(resume.success, "stderr: {}", resume.stderr);
    assert!(
        !resume.stdout.contains("also chart the reef pass"),
        "and it is not the loader's:\n{}",
        resume.stdout
    );
    // It opens its own turn, so turn numbering still counts it.
    let turns = h.run(&[
        "search",
        "",
        &at(sess),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    assert!(
        turns.stdout.contains(r#""distinct_keys":1"#),
        "{}",
        turns.stdout
    );
}

#[test]
fn the_retired_continuation_selector_names_its_successor() {
    let h = resume_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.schedule.continuation",
    ]);
    assert!(
        !out.success,
        "a retired selector is a HARD error, not a shim"
    );
    assert!(
        out.stderr.contains("harness.resume.prompt"),
        "the error must name the successor:\n{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("pre-v0.12.0 name"),
        "and say it is a retired spelling:\n{}",
        out.stderr
    );
    // `harness.schedule` itself is still a valid prefix - only `wakeup` lives under it now.
    let sched = h.run(&["search", "", &at(SESS), "-t", "harness.schedule"]);
    assert!(sched.success, "stderr: {}", sched.stderr);
    assert!(
        !sched.stdout.contains("Continue from where"),
        "the resume prompt left the schedule family:\n{}",
        sched.stdout
    );
}
