//! The `harness.system.*` event kinds, the scheduler's `harness.schedule.event` record and
//! the fallback-served assistant turn: what each renders, which keys each exposes, which
//! record each names as its provenance edge, and the one thing that never reaches an
//! output path - an `api_error`'s response headers.

use crate::harness::*;

mod api_error;
mod edges;
mod events;
mod fallback;
mod gate;
mod schedule;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "3b3b3b3b-4c4c-4d4d-8e8e-5f5f5f5f5f5f";

/// One transcript carrying a specimen of every leaf this file covers.
///
/// L1 the human, L2 refusal-with-fallback, L3 refusal-without, L4 the api_error HTTP
/// shape, the one that carries response headers, L5 informational, L6 agents_killed,
/// L7 the slash wrapper the stdout record wraps, L8 local_command, L9 the CronCreate
/// result that armed the task, L10 the cron fire record, L11 the wakeup fire record,
/// L12 the fallback-served turn, L13 the api_error CONNECTION shape.
///
/// BOTH api_error lines carry a MEASURED key set (claim CLS-062): `{error, headers,
/// requestID, status, type}` on L4, which is the shape of all 5 header-bearing records on
/// each measured root, with the response body it received nested two hops down; and
/// `{connection, formatted, isNetworkDown, message, rateLimits, status}` on L13, the shape
/// of the remaining 24 and 253. A key set neither root holds would pin a render nothing
/// produces.
fn system_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the harbor zzask"}}"#, "\n",
            r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","parentUuid":"u1","level":"warning","timestamp":"2026-06-07T05:00:01.000Z","content":"Switched models to finish this turn","direction":"retry","trigger":"refusal","originalModel":"model-alpha","fallbackModel":"model-beta","requestId":"req-zz1","apiRefusalCategory":"cyber","apiRefusalExplanation":null,"refusedUserMessageUuid":"u1","retractedMessageUuids":["wire-zz1","wire-zz2"]}"#, "\n",
            r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r2","parentUuid":"r1","level":"warning","timestamp":"2026-06-07T05:00:02.000Z","content":"","originalModel":"model-alpha","requestId":"req-zz2","apiRefusalCategory":"bio","apiRefusalExplanation":null,"refusedUserMessageUuid":null}"#, "\n",
            r#"{"type":"system","subtype":"api_error","uuid":"e1","parentUuid":"r2","level":"error","timestamp":"2026-06-07T05:00:03.000Z","error":{"type":"overloaded_error","headers":{"set-cookie":"zzsession=deadbeef","anthropic-organization-id":"org-zzsecret","cf-ray":"zzray-syd"},"requestID":"req-zz3","status":529,"error":{"type":"error","request_id":"req-zz3","error":{"type":"overloaded_error","message":"Overloaded zzoverload"}}},"retryAttempt":3,"maxRetries":20,"retryInMs":1522.5}"#, "\n",
            r#"{"type":"system","subtype":"informational","uuid":"n1","parentUuid":"e1","level":"warning","timestamp":"2026-06-07T05:00:04.000Z","content":"Remote Control disconnected zznotice"}"#, "\n",
            r#"{"type":"system","subtype":"agents_killed","uuid":"k1","parentUuid":"n1","timestamp":"2026-06-07T05:00:05.000Z","isMeta":false}"#, "\n",
            r#"{"type":"user","uuid":"w1","parentUuid":"k1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"<command-name>/status</command-name>\n<command-args>zzargs</command-args>"}}"#, "\n",
            r#"{"type":"system","subtype":"local_command","uuid":"c1","parentUuid":"w1","timestamp":"2026-06-07T05:00:07.000Z","content":"<local-command-stdout>zzstdout ok</local-command-stdout>"}"#, "\n",
            r#"{"type":"user","uuid":"t1","parentUuid":"c1","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_zzcron","content":"Scheduled task task-zz9 created"}]},"toolUseResult":{"id":"task-zz9","cron":"0 5 * * *"}}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s1","parentUuid":"t1","timestamp":"2026-06-07T05:00:09.000Z","content":"Running scheduled task (Jun 7 5:28am)","taskId":"task-zz9","cron":"0 5 * * *","prompt":"zzarmed check the queue"}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:00:10.000Z","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","model":"model-beta","content":[{"type":"fallback","from":{"model":"model-alpha"},"to":{"model":"model-beta"}}]}}"#, "\n",
            r#"{"type":"system","subtype":"api_error","uuid":"e2","parentUuid":"a1","level":"error","timestamp":"2026-06-07T05:00:12.000Z","error":{"connection":{"host":"zzhost"},"formatted":"zzformatted","isNetworkDown":false,"message":"Connection error zzconn","rateLimits":{},"status":429},"retryAttempt":1,"maxRetries":10,"retryInMs":600}"#, "\n",
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
