//! The `harness.system.*` event kinds, the scheduler's `harness.schedule.event` record and
//! the fallback-served assistant turn: what each renders, which keys each exposes, which
//! record each names as its provenance edge, and the one thing that never reaches an
//! output path - an `api_error`'s response headers.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "3b3b3b3b-4c4c-4d4d-8e8e-5f5f5f5f5f5f";

/// One transcript carrying a specimen of every leaf this file covers.
///
/// L1 the human · L2 refusal-with-fallback · L3 refusal-without · L4 api_error carrying
/// response headers · L5 informational · L6 agents_killed · L7 the slash wrapper the
/// stdout record wraps · L8 local_command · L9 the CronCreate result that armed the task ·
/// L10 the cron fire record · L11 the wakeup fire record · L12 the fallback-served turn.
fn system_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the harbor zzask"}}"#, "\n",
            r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","parentUuid":"u1","level":"warning","timestamp":"2026-06-07T05:00:01.000Z","content":"Switched models to finish this turn","direction":"retry","trigger":"refusal","originalModel":"model-alpha","fallbackModel":"model-beta","requestId":"req-zz1","apiRefusalCategory":"cyber","apiRefusalExplanation":null,"refusedUserMessageUuid":"u1","retractedMessageUuids":["wire-zz1","wire-zz2"]}"#, "\n",
            r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r2","parentUuid":"r1","level":"warning","timestamp":"2026-06-07T05:00:02.000Z","content":"","originalModel":"model-alpha","requestId":"req-zz2","apiRefusalCategory":"bio","apiRefusalExplanation":null,"refusedUserMessageUuid":null}"#, "\n",
            r#"{"type":"system","subtype":"api_error","uuid":"e1","parentUuid":"r2","level":"error","timestamp":"2026-06-07T05:00:03.000Z","error":{"type":"overloaded_error","message":"Overloaded zzoverload","status":529,"requestID":"req-zz3","headers":{"set-cookie":"zzsession=deadbeef","anthropic-organization-id":"org-zzsecret","cf-ray":"zzray-syd"}},"retryAttempt":3,"maxRetries":20,"retryInMs":1522.5}"#, "\n",
            r#"{"type":"system","subtype":"informational","uuid":"n1","parentUuid":"e1","level":"warning","timestamp":"2026-06-07T05:00:04.000Z","content":"Remote Control disconnected zznotice"}"#, "\n",
            r#"{"type":"system","subtype":"agents_killed","uuid":"k1","parentUuid":"n1","timestamp":"2026-06-07T05:00:05.000Z","isMeta":false}"#, "\n",
            r#"{"type":"user","uuid":"w1","parentUuid":"k1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"<command-name>/status</command-name>\n<command-args>zzargs</command-args>"}}"#, "\n",
            r#"{"type":"system","subtype":"local_command","uuid":"c1","parentUuid":"w1","timestamp":"2026-06-07T05:00:07.000Z","content":"<local-command-stdout>zzstdout ok</local-command-stdout>"}"#, "\n",
            r#"{"type":"user","uuid":"t1","parentUuid":"c1","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_zzcron","content":"Scheduled task task-zz9 created"}]},"toolUseResult":{"id":"task-zz9","cron":"0 5 * * *"}}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s1","parentUuid":"t1","timestamp":"2026-06-07T05:00:09.000Z","content":"Running scheduled task (Jun 7 5:28am)","taskId":"task-zz9","cron":"0 5 * * *","prompt":"zzarmed check the queue"}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:00:10.000Z","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","model":"model-beta","content":[{"type":"fallback","from":{"model":"model-alpha"},"to":{"model":"model-beta"}}]}}"#, "\n",
        ),
    );
    h
}

/// Every hit object of a `--format json` run, flattened out of its exchange rows.
fn hits_of(stdout: &str) -> Vec<serde_json::Value> {
    json_rows(stdout, "exchange")
        .into_iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// Each of the five event kinds renders its own head with the fields that make that event
/// useful - never the one-size subtype-and-level form.
#[test]
fn every_system_event_kind_renders_its_own_fields() {
    let h = system_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.system.*"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for (leaf, text) in [
        (
            "harness.system.refusal",
            "[refusal model_refusal_fallback] originalModel=model-alpha fallbackModel=model-beta \
             apiRefusalCategory=cyber Switched models to finish this turn",
        ),
        (
            "harness.system.refusal",
            "[refusal model_refusal_no_fallback] originalModel=model-alpha fallbackModel=none \
             apiRefusalCategory=bio",
        ),
        (
            "harness.system.api-error",
            "[api-error status=529 retry 3/20 in 1522.5ms] overloaded_error Overloaded zzoverload",
        ),
        (
            "harness.system.notice",
            "[notice warning] Remote Control disconnected zznotice",
        ),
        ("harness.system.kill", "[kill]"),
        (
            "harness.system.command",
            "[command] <local-command-stdout>zzstdout ok</local-command-stdout>",
        ),
    ] {
        assert!(
            out.stdout.contains(leaf) && out.stdout.contains(text),
            "missing {leaf} / {text:?}:\n{}",
            out.stdout
        );
    }
}

/// `--count-by label` names every leaf this family owns, with the two refusal subtypes
/// folded into ONE key: whether a second model was tried is a field, not an event kind.
#[test]
fn the_label_census_names_every_system_event_key() {
    let h = system_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.*",
        "-t",
        "harness.schedule.event",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census");
    let count = |key: &str| {
        rows.iter()
            .find(|r| r["key"] == key)
            .and_then(|r| r["records"].as_u64())
            .unwrap_or(0)
    };
    assert_eq!(count("harness.system.refusal"), 2, "{rows:?}");
    assert_eq!(count("harness.system.api-error"), 1, "{rows:?}");
    assert_eq!(count("harness.system.notice"), 1, "{rows:?}");
    assert_eq!(count("harness.system.kill"), 1, "{rows:?}");
    assert_eq!(count("harness.system.command"), 1, "{rows:?}");
    assert_eq!(count("harness.schedule.event"), 2, "{rows:?}");
}

/// The exposed keys of a refusal hit, and the ONE uuid key that resolves - as an EDGE
/// carrying the line, which `show --line` then fetches. The wire uuids are not exposed.
#[test]
fn a_refusal_hit_exposes_its_models_and_the_rewind_edge() {
    let h = system_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.refusal",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits = hits_of(&out.stdout);
    let fb = hits
        .iter()
        .find(|hh| hh["retried"] == serde_json::json!(true))
        .expect("the fallback refusal hit");
    assert_eq!(fb["original_model"], serde_json::json!("model-alpha"));
    assert_eq!(fb["fallback_model"], serde_json::json!("model-beta"));
    assert_eq!(fb["category"], serde_json::json!("cyber"));
    // The edge names the REWIND TARGET and resolves it to its own line, so the reader
    // has an address rather than an id to hunt for. `for` is an ARRAY, one object per
    // edge, so several edges on one unit accumulate rather than overwrite.
    assert_eq!(fb["for"][0]["kind"], serde_json::json!("refused"));
    assert_eq!(fb["for"][0]["basis"], serde_json::json!("key"));
    assert_eq!(fb["for"][0]["target"]["uuid"], serde_json::json!("u1"));
    assert_eq!(fb["for"][0]["target"]["line"], serde_json::json!(1));
    assert_eq!(fb["for"][0]["reason"], serde_json::Value::Null);
    // The wire uuids never resolve to a line, so they are not offered as a key.
    assert!(fb.get("retracted_message_uuids").is_none(), "{fb}");
    let no_fb = hits
        .iter()
        .find(|hh| hh["retried"] == serde_json::json!(false))
        .expect("the no-fallback refusal hit");
    assert!(no_fb.get("fallback_model").is_none(), "{no_fb}");
    // A null value is an HONEST unknown with its reason, never a missing fact.
    assert_eq!(no_fb["for"][0]["target"], serde_json::Value::Null);
    assert_eq!(
        no_fb["for"][0]["reason"],
        serde_json::json!("the record carries no such key")
    );
    // The text surface renders the same two answers in the label zone.
    let text = h.run(&["search", "", &at(SESS), "-t", "harness.system.refusal"]);
    assert!(text.stdout.contains("[for: refused L1]"), "{}", text.stdout);
    assert!(
        text.stdout
            .contains("[for: refused unknown (the record carries no such key)]"),
        "{}",
        text.stdout
    );
    // The line the edge names is a real address in this transcript.
    let target = h.run(&["show", &at(SESS), "--line", "1"]);
    assert!(
        target.success && target.stdout.contains("chart the harbor zzask"),
        "{}\n{}",
        target.stdout,
        target.stderr
    );
}

/// THE HEADERS LAW. An `api_error`'s response headers carry a session cookie, an
/// organization id and a CDN trace id. They reach no rendered or JSON output path on any
/// surface; `--raw` prints the line and is the one documented way to the bytes.
#[test]
fn api_error_response_headers_never_reach_an_output_path() {
    let h = system_home();
    const SECRETS: [&str; 3] = ["zzsession=deadbeef", "org-zzsecret", "zzray-syd"];
    let t = at(SESS);
    let runs: [&[&str]; 5] = [
        &["search", "", &t, "-t", "harness.system.api-error"],
        &[
            "search",
            "",
            &t,
            "-t",
            "harness.system.api-error",
            "--no-truncate",
        ],
        &[
            "search",
            "",
            &t,
            "-t",
            "harness.system.api-error",
            "--format",
            "json",
        ],
        &["show", &t, "--line", "4"],
        &["show", &t, "--line", "4", "--format", "json"],
    ];
    for argv in runs {
        let out = h.run(argv);
        assert!(out.success, "{argv:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("api-error") || out.stdout.contains("api_error"),
            "{argv:?} must still render the record:\n{}",
            out.stdout
        );
        for s in SECRETS {
            assert!(
                !out.stdout.contains(s),
                "{argv:?} leaked {s:?}:\n{}",
                out.stdout
            );
        }
        assert!(
            !out.stdout.contains("\"headers\""),
            "{argv:?} leaked the headers object:\n{}",
            out.stdout
        );
    }
    // The JSON `error` fact carries the two descriptive fields and nothing else.
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.api-error",
        "--format",
        "json",
    ]);
    let hit = hits_of(&js.stdout).into_iter().next().expect("one hit");
    assert_eq!(hit["status"], serde_json::json!(529));
    assert_eq!(hit["retry_attempt"], serde_json::json!(3));
    assert_eq!(hit["max_retries"], serde_json::json!(20));
    assert_eq!(hit["retry_in_ms"], serde_json::json!(1522.5));
    assert_eq!(
        hit["error"],
        serde_json::json!({"type":"overloaded_error","message":"Overloaded zzoverload"})
    );
    // The DOCUMENTED exception: `--raw` prints the source line, headers and all.
    let raw = h.run(&["show", &at(SESS), "--line", "4", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    for s in SECRETS {
        assert!(
            raw.stdout.contains(s),
            "--raw is the escape hatch and prints the line whole; {s:?} missing:\n{}",
            raw.stdout
        );
    }
}

/// What an api_error SAYS failed is SEARCHABLE. The record carries no `content`, so the
/// error's own type and message are the body; without them the statement of the failure
/// is in the output and reachable by no pattern.
#[test]
fn the_api_error_failure_text_is_searchable() {
    let h = system_home();
    for pattern in ["Overloaded zzoverload", "overloaded_error"] {
        let out = h.run(&[
            "search",
            pattern,
            &at(SESS),
            "-t",
            "harness.system.api-error",
            "-c",
        ]);
        assert!(out.success, "{pattern} stderr: {}", out.stderr);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "{pattern} should reach the api_error record:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
}

/// The delivered command names the slash WRAPPER it is the output of, through the chain
/// link rather than a payload key.
#[test]
fn the_delivered_command_names_the_wrapper_it_wraps() {
    let h = system_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.system.command"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("[for: wraps L7]"), "{}", out.stdout);
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.command",
        "--format",
        "json",
    ]);
    let hit = hits_of(&js.stdout).into_iter().next().expect("one hit");
    assert_eq!(hit["for"][0]["kind"], serde_json::json!("wraps"));
    // POSITION, not key: a run of local commands chains stdout to stdout, so the link
    // names the record ABOVE rather than a wrapper the harness pointed at.
    assert_eq!(hit["for"][0]["basis"], serde_json::json!("position"));
    assert_eq!(hit["for"][0]["target"]["line"], serde_json::json!(7));
    let wrapper = h.run(&["show", &at(SESS), "--line", "7"]);
    assert!(
        wrapper.success && wrapper.stdout.contains("/status"),
        "the line the edge names is the wrapper:\n{}",
        wrapper.stdout
    );
}

/// The fire record's two shapes: the cron arm names its task and schedule and exposes the
/// armed prompt, the `/loop` wakeup arm names only the instant both read from the same
/// parenthesised tail - and says so where the task id would be.
#[test]
fn the_schedule_event_renders_both_fire_shapes() {
    let h = system_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.schedule.event"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "[scheduled Jun 7 5:28am] task=task-zz9 cron=0 5 * * * prompt=zzarmed check the queue"
        ),
        "the cron arm, with every exposed key also IN the matchable text:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "[for: armed_by unknown (the arming call is joined by taskId, not by a record \
             uuid - search the id)]"
        ),
        "the cron arm's edge states the join it cannot make:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[scheduled Jun 7 6:40am] Claude resuming /loop wakeup"),
        "the wakeup arm:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "[for: armed_by unknown (the record carries no taskId - the wakeup arm arms no task)]"
        ),
        "the wakeup arm's honest empty:\n{}",
        out.stdout
    );
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.schedule.event",
        "--format",
        "json",
    ]);
    let hits = hits_of(&js.stdout);
    let cron = hits
        .iter()
        .find(|hh| hh["task_id"] == serde_json::json!("task-zz9"))
        .expect("the cron-arm hit");
    assert_eq!(cron["cron"], serde_json::json!("0 5 * * *"));
    assert_eq!(cron["prompt"], serde_json::json!("zzarmed check the queue"));
    // `scheduled_at` is the ENGINE's field with ONE writer, and that writer fills it for
    // the fired PROMPT. On the event hit it is null and the HEAD carries the instant -
    // a family fact here would be a second writer of one key.
    assert_eq!(cron["scheduled_at"], serde_json::Value::Null);
    // The edge resolves to nothing at all: the join is by id, not by uuid.
    assert_eq!(cron["for"][0]["kind"], serde_json::json!("armed_by"));
    assert_eq!(cron["for"][0]["target"], serde_json::Value::Null);
    let wakeup = hits
        .iter()
        .find(|hh| hh["task_id"].is_null())
        .expect("the wakeup-arm hit");
    assert!(wakeup["cron"].is_null(), "{wakeup}");
    assert!(wakeup["prompt"].is_null(), "{wakeup}");
    assert_eq!(wakeup["for"][0]["target"], serde_json::Value::Null);
}

/// The ARMED-BY join, which is why the task id is worth exposing at all: the SAME
/// pattern reaches the result of the call that armed the task and, under the selector
/// that admits a gated system line, the fire record itself. Two different records on two
/// different lines, so the join is not the fire record finding itself - which is the
/// only thing an id exposed on one record alone could ever prove.
#[test]
fn the_armed_by_id_joins_the_fire_record_to_the_arming_result() {
    let h = system_home();
    // A flagless scan reaches the ARMING call's result, whose own text names the id.
    let armed = h.run(&["search", "task-zz9", &at(SESS), "--format", "json"]);
    assert!(armed.success, "stderr: {}", armed.stderr);
    let armed_lines: Vec<u64> = hits_of(&armed.stdout)
        .iter()
        .filter_map(|hh| hh["line"].as_u64())
        .collect();
    assert_eq!(
        armed_lines,
        vec![9],
        "the CronCreate result that armed the task, and only it:\n{}",
        armed.stdout
    );
    // The gated selector reaches the FIRE record, through the id in its fabricated head.
    let fired = h.run(&[
        "search",
        "task-zz9",
        &at(SESS),
        "-t",
        "harness.schedule.event",
        "--format",
        "json",
    ]);
    assert!(fired.success, "stderr: {}", fired.stderr);
    let fired_hits = hits_of(&fired.stdout);
    assert_eq!(fired_hits.len(), 1, "{}", fired.stdout);
    assert_eq!(fired_hits[0]["line"], serde_json::json!(10));
    assert_eq!(fired_hits[0]["task_id"], serde_json::json!("task-zz9"));
}

/// A turn the API served from a fallback model: an addressed fetch RENDERS it under the
/// plain message leaf with both model ids in the label zone, and the honest banner edge
/// beside them.
#[test]
fn a_fallback_served_turn_is_addressable_and_names_both_models() {
    let h = system_home();
    let show = h.run(&["show", &at(SESS), "--line", "12"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout.contains("agent.message.plain")
            && show
                .stdout
                .contains("[served by model-beta, fell back from model-alpha]"),
        "{}",
        show.stdout
    );
    // The banner edge is the HONEST unknown: the banner names this record, not the
    // reverse, and a uuid map cannot walk down.
    assert!(
        show.stdout.contains(
            "[for: banner unknown (the banner names this record, not the reverse; a \
             one-to-many descent needs the provenance index)]"
        ),
        "{}",
        show.stdout
    );
    let js = h.run(&["show", &at(SESS), "--line", "12", "--format", "json"]);
    let row = json_rows(&js.stdout, "record")
        .into_iter()
        .next()
        .expect("one record row");
    assert_eq!(row["label"], serde_json::json!("agent.message.plain"));
    assert_eq!(row["served_by"], serde_json::json!("model-beta"));
    assert_eq!(row["fell_back_from"], serde_json::json!("model-alpha"));
    assert_eq!(row["for"][0]["kind"], serde_json::json!("banner"));
    assert_eq!(row["for"][0]["target"], serde_json::Value::Null);
    // A pattern SCAN emits nothing for it - the API wrote the block in place of the
    // text, so there is no unit to match or excerpt.
    let scan = h.run(&["search", "model-alpha", &at(SESS), "-t", "agent", "-c"]);
    assert_eq!(scan.stdout.trim(), "0", "{}", scan.stdout);
}

/// A turn that fell back AND SPOKE carries the marker on its scanned unit too.
///
/// The ids come off the parsed BLOCK rather than off the source line, and that is the
/// whole point: a scan hands a family no source line for a role-bearing record, so a
/// marker read off the line appears under `show --line` and vanishes under `search`.
/// This is the address-versus-scan split the one stamping site exists to prevent, pinned
/// from both sides on one record.
#[test]
fn a_fallback_turn_that_spoke_carries_the_marker_on_a_scan_too() {
    let h = Home::new();
    let sess = "6d6d6d6d-7e7e-4f4f-8a8a-9b9b9b9b9b9b";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzask the harbor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","model":"model-beta","content":[{"type":"fallback","from":{"model":"model-alpha"},"to":{"model":"model-beta"}},{"type":"text","text":"zzspoke and fell back"}]}}"#, "\n",
        ),
    );
    let marker = "[served by model-beta, fell back from model-alpha]";
    let scan = h.run(&["search", "zzspoke", &at(sess)]);
    assert!(scan.success, "stderr: {}", scan.stderr);
    assert!(
        scan.stdout.contains(marker),
        "a SCANNED unit carries the marker:\n{}",
        scan.stdout
    );
    let show = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(
        show.success && show.stdout.contains(marker),
        "and the addressed unit renders the same:\n{}",
        show.stdout
    );
    let js = h.run(&["search", "zzspoke", &at(sess), "--format", "json"]);
    let hit = hits_of(&js.stdout).into_iter().next().expect("one hit");
    assert_eq!(hit["served_by"], serde_json::json!("model-beta"));
    assert_eq!(hit["fell_back_from"], serde_json::json!("model-alpha"));
}

/// THE HEADERS LAW HOLDS FOR A SUBTYPE CSIFT MODELS NO LEAF FOR.
///
/// The five modeled event kinds render named fields, so their headers never reach a
/// render by construction. A subtype the taxonomy does not name takes the OTHER road -
/// the 1:1 law's unmodeled unit, whose key=value excerpt walks the line's own values
/// without knowing what they are - and 48 subtype names ship against 11 observed, so a
/// new one is the expected case rather than a hypothetical. Every `headers` member is
/// elided there at every depth, and `--raw` stays the one way to the bytes.
#[test]
fn an_unmodeled_system_subtype_elides_its_headers_too() {
    let h = Home::new();
    let sess = "7e7e7e7e-8f8f-4a4a-8b8b-9c9c9c9c9c9c";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzask"}}"#, "\n",
            r#"{"type":"system","subtype":"api_error_v2","uuid":"x1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","error":{"type":"overloaded_error","headers":{"set-cookie":"zzsession=deadbeef","anthropic-organization-id":"org-zzsecret"}}}"#, "\n",
        ),
    );
    const SECRETS: [&str; 2] = ["zzsession=deadbeef", "org-zzsecret"];
    let t = at(sess);
    let runs: [&[&str]; 2] = [
        &["show", &t, "--line", "2"],
        &["show", &t, "--line", "2", "--format", "json"],
    ];
    for argv in runs {
        let out = h.run(argv);
        assert!(out.success, "{argv:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("api_error_v2"),
            "{argv:?} must still name the shape it cannot model:\n{}",
            out.stdout
        );
        for s in SECRETS {
            assert!(
                !out.stdout.contains(s),
                "{argv:?} leaked {s:?}:\n{}",
                out.stdout
            );
        }
        assert!(
            out.stdout.contains("elided"),
            "{argv:?} states that the set is there rather than dropping it:\n{}",
            out.stdout
        );
    }
    // The DOCUMENTED exception, on this road as on the modeled one.
    let raw = h.run(&["show", &t, "--line", "2", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    for s in SECRETS {
        assert!(
            raw.stdout.contains(s),
            "{s:?} missing from --raw:\n{}",
            raw.stdout
        );
    }
}

/// The whole-file gate must not prune a file whose only hit sits in a FABRICATED head or
/// key word. None of these patterns is a byte substring of the source line.
#[test]
fn a_pattern_matching_only_a_fabricated_word_survives_the_whole_file_gate() {
    let h = system_home();
    for (pattern, selector) in [
        (r"originalModel=model-alpha", "harness.system.refusal"),
        (r"fallbackModel=none", "harness.system.refusal"),
        (r"\[api-error status=529", "harness.system.api-error"),
        (r"retry 3/20", "harness.system.api-error"),
        (r"\[notice warning\]", "harness.system.notice"),
        (r"\[kill\]", "harness.system.kill"),
        (r"\[scheduled Jun 7 5:28am\]", "harness.schedule.event"),
        (r"task=task-zz9", "harness.schedule.event"),
        (r"prompt=zzarmed check the queue", "harness.schedule.event"),
    ] {
        let out = h.run(&["search", pattern, &at(SESS), "-t", selector, "-c"]);
        assert!(out.success, "{pattern} stderr: {}", out.stderr);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "{pattern} lost to the whole-file gate:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
}
