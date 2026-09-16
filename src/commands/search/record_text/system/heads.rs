//! The per-event-kind HEADS: what each `type:"system"` event renders in place of the
//! one-size `[<subtype> <level>]` head that named the subtype and threw the payload away.
//!
//! Every head is FABRICATED (no byte of it is a substring of the source line), which is
//! sound only because the key-only `"subtype"` synth marker routes these lines to the
//! stage-2 re-render with their own source line - see the module doc of the parent.
//!
//! The `key=` WORDS are the harness's own spellings (`originalModel=`, not `original=`)
//! so a reader can grep a rendered head's value straight back to the record; the JSON
//! keys stay the snake_case names a consumer reads. That is a deliberate split from the
//! short forms the brief proposed, and it is not only taste: the repo's own
//! `docs_do_not_resurrect_removed_tokens` guard denies the substring `category=` in the
//! four documents, a retired 0.x CLI spelling, and renaming the field rather than
//! weakening the guard is the cheaper of the two.

use super::*;

/// The rendered + searchable text of a system EVENT record: a fabricated head naming the
/// event and its meaningful fields, then the record's own `content` when it has one.
/// `None` for every leaf outside the six this family owns.
///
/// `p` is the record's source line, parsed ONCE by the caller (the parent module's
/// `payload`), so no head re-reads the bytes.
pub(crate) fn system_event_text(
    rec: &Record,
    class: Class,
    p: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<String> {
    let head = match class {
        Class::SystemRefusal => refusal_head(rec, p),
        Class::SystemApiError => api_error_head(p),
        Class::SystemNotice => match rec.level.as_deref() {
            Some(l) if !l.is_empty() => format!("[notice {l}]"),
            _ => "[notice]".to_string(),
        },
        // The kill record carries no payload at all on disk - the human's line is
        // fabricated by the harness renderer, not stored - so the head IS the record.
        Class::SystemKill => "[kill]".to_string(),
        Class::SystemCommand => "[command]".to_string(),
        Class::ScheduleEvent => schedule_event_head(rec, p),
        _ => return None,
    };
    let body = match class {
        // An `api_error` line carries no `content` key at all, so the statement of WHAT
        // failed lives in the `error` object - and a reader searching for the failure
        // searches for its wording. The two descriptive fields are rendered as the body;
        // every other key of that object, `headers` above all, stays unrendered.
        Class::SystemApiError => error_text(p),
        _ => content_body(rec),
    };
    Some(if body.is_empty() {
        head
    } else {
        format!("{head} {body}")
    })
}

/// `[refusal <subtype>] originalModel=<v> fallbackModel=<v|none> apiRefusalCategory=<v>`.
///
/// `fallbackModel=` is printed even when the key is ABSENT, as `none`: the absence IS the
/// event for the `model_refusal_no_fallback` subtype (the request was refused and no
/// second model was tried), and eliding it would render the two subtypes alike. That
/// negative is only printable when the line was READ, though - with no source line csift
/// does not know whether the key is there, and a `none` then would be a fabricated
/// negative rather than a reported one, so the head stops at the subtype.
fn refusal_head(rec: &Record, p: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
    let subtype = rec.subtype.as_deref().unwrap_or("refusal");
    let mut parts = vec![format!("[refusal {subtype}]")];
    let Some(m) = p else {
        return parts.join(" ");
    };
    if let Some(v) = str_field(m, "originalModel") {
        parts.push(format!("originalModel={v}"));
    }
    parts.push(format!(
        "fallbackModel={}",
        str_field(m, "fallbackModel").unwrap_or_else(|| "none".to_string())
    ));
    if let Some(v) = str_field(m, "apiRefusalCategory") {
        parts.push(format!("apiRefusalCategory={v}"));
    }
    parts.join(" ")
}

/// `[api-error status=<status> retry <retryAttempt>/<maxRetries> in <retryInMs>ms]`,
/// each part only when the line carries it.
///
/// `status` is read from INSIDE the `error` object (the HTTP shape carries it there, and
/// so does the connection shape when the attempt reached a response); the three retry
/// counters are top-level. Numbers render as their own JSON text - `retryInMs` is a
/// jittered float, and rounding it would report a value the line does not hold.
///
/// A line carrying a MAXIMUM and no attempt renders the attempt slot as `?`: the pair is
/// one counter, and printing `retry 20` for a maximum would read as an attempt number.
/// The placeholder is a marker word, so it is declared in the SKILL shape rows and in
/// `search --help` beside the rest of this head.
fn api_error_head(p: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
    let Some(p) = p else {
        return "[api-error]".to_string();
    };
    let mut parts = vec!["[api-error".to_string()];
    if let Some(s) = p.get("error").and_then(|e| e.get("status")).map(num_text) {
        parts.push(format!("status={s}"));
    }
    let attempt = p.get("retryAttempt").map(num_text);
    let max = p.get("maxRetries").map(num_text);
    match (attempt, max) {
        (Some(a), Some(m)) => parts.push(format!("retry {a}/{m}")),
        (Some(a), None) => parts.push(format!("retry {a}")),
        (None, Some(m)) => parts.push(format!("retry ?/{m}")),
        (None, None) => {}
    }
    if let Some(ms) = p.get("retryInMs").map(num_text) {
        parts.push(format!("in {ms}ms"));
    }
    format!("{}]", parts.join(" "))
}

/// `[scheduled <when>] task=<id> cron=<expr> prompt=<cut>` - the instant from the
/// content's parenthesised tail (the ONE reader of that tail, shared with the fired
/// prompt's join), then the cron arm's own fields when the line carries them.
///
/// Every field here rides the head as well as the hit's JSON, and that is the rule rather
/// than a duplication: a key exposed in JSON alone is unsearchable, and the whole value
/// of a foreign join key is that `csift search '<task id>'` reaches both this record and
/// the call that armed it. The armed `prompt` is cut to [`PROMPT_MAX`] - the same cut the
/// key carries, so the two can never disagree - because it is the operator's whole text
/// and the head is a line a reader scans.
fn schedule_event_head(
    rec: &Record,
    p: Option<&serde_json::Map<String, serde_json::Value>>,
) -> String {
    let mut parts = vec![match rec.scheduled_fire_instant() {
        Some(when) => format!("[scheduled {when}]"),
        None => "[scheduled]".to_string(),
    }];
    if let Some(p) = p {
        for (key, word) in [("taskId", "task"), ("cron", "cron")] {
            if let Some(v) = str_field(p, key) {
                parts.push(format!("{word}={v}"));
            }
        }
        if let Some(v) = str_field(p, "prompt") {
            parts.push(format!(
                "prompt={}",
                crate::text::truncate_excerpt(&v, PROMPT_MAX)
            ));
        }
    }
    parts.join(" ")
}

/// The record's own `content`: a string verbatim (the banner, the command echo, the fire
/// sentence), any other shape as its compact JSON, and the empty string when the line
/// carries none - which is the whole payload of an `agents_killed` record and of a
/// `model_refusal_no_fallback` one.
fn content_body(rec: &Record) -> String {
    match rec.content.as_ref() {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// What an api_error SAYS failed, as the searchable body: the error's `type` and its
/// `message`, in that order, joined by a space. Neither is sensitive - the object's
/// sensitive half is `headers`, which no path but `--raw` prints - and without them the
/// record's statement of the failure is unsearchable: `content` is absent on every
/// observed specimen, so this is the whole body there is. The empty string when the line
/// carries neither field.
fn error_text(p: Option<&serde_json::Map<String, serde_json::Value>>) -> String {
    let Some(obj) = p.and_then(|p| p.get("error")).and_then(|e| e.as_object()) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for key in ["type", "message"] {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    parts.join(" ")
}

/// The api_error `error` object reduced to the two fields that say WHAT failed. Every
/// other key is dropped, `headers` above all: the observed header set carries a session
/// cookie, an organization id and a CDN trace id, none of which belongs in a rendered
/// transcript view. `None` when the error object carries neither field.
pub(crate) fn error_summary(err: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let obj = err?.as_object()?;
    let mut out = serde_json::Map::new();
    for key in ["type", "message"] {
        if let Some(v) = obj.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    (!out.is_empty()).then_some(serde_json::Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        serde_json::from_str(line).expect("fixture parses")
    }

    /// The family's own entry, exercised the way the seam calls it.
    fn text(line: &str) -> Option<String> {
        let r = rec(line);
        let ctx = ClassifyCtx::top_level();
        super::super::render(&r, Some(line), Class::SystemNotice, &ctx)
    }

    /// The same record with NO source line, which is what a caller outside the gated and
    /// addressed paths holds.
    fn text_unread(line: &str) -> Option<String> {
        let r = rec(line);
        let ctx = ClassifyCtx::top_level();
        super::super::render(&r, None, Class::SystemNotice, &ctx)
    }

    /// Every field the head names comes from the line, and `fallbackModel=none` is what
    /// separates the two refusal subtypes: the absence of a second model IS the event.
    #[test]
    fn refusal_head_names_both_models_and_the_category() {
        let line = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","level":"warning","content":"Switched models","originalModel":"model-a","fallbackModel":"model-b","apiRefusalCategory":"cyber"}"#;
        assert_eq!(
            text(line).as_deref(),
            Some(
                "[refusal model_refusal_fallback] originalModel=model-a fallbackModel=model-b \
                 apiRefusalCategory=cyber Switched models"
            )
        );
        let none = r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r2","level":"warning","content":"","originalModel":"model-a","apiRefusalCategory":"bio"}"#;
        assert_eq!(
            text(none).as_deref(),
            Some(
                "[refusal model_refusal_no_fallback] originalModel=model-a fallbackModel=none \
                 apiRefusalCategory=bio"
            )
        );
    }

    /// With NO source line the head names the event and stops. `fallbackModel=none`
    /// reports a key csift READ and did not find; printed with nothing read it would be a
    /// fabricated negative, which is the opposite of what this file is for.
    #[test]
    fn an_unread_refusal_head_names_no_field_it_did_not_read() {
        let none = r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r2","originalModel":"model-a"}"#;
        assert_eq!(
            text_unread(none).as_deref(),
            Some("[refusal model_refusal_no_fallback]")
        );
        // A line the caller HOLDS but cannot read as an object takes the same road.
        let r = rec(none);
        let ctx = ClassifyCtx::top_level();
        assert_eq!(
            super::super::render(&r, Some("[1,2,3]"), Class::SystemNotice, &ctx).as_deref(),
            Some("[refusal model_refusal_no_fallback]")
        );
    }

    /// The retry counters render as the line's own numbers, the status is read from
    /// INSIDE the error object, the jittered float is not rounded, and the error's own
    /// words are the body - while no byte of `headers` reaches the text.
    #[test]
    fn api_error_head_names_the_counters_and_the_failure() {
        let line = r#"{"type":"system","subtype":"api_error","uuid":"e1","level":"error","error":{"type":"overloaded_error","message":"Overloaded zzoverload","status":529,"headers":{"set-cookie":"sess=zz","anthropic-organization-id":"org-zz"}},"retryAttempt":3,"maxRetries":20,"retryInMs":1522.5}"#;
        let t = text(line).expect("renders");
        assert_eq!(
            t,
            "[api-error status=529 retry 3/20 in 1522.5ms] overloaded_error Overloaded zzoverload"
        );
        assert!(!t.contains("set-cookie"), "headers never reach the render");
        assert!(!t.contains("org-zz"), "headers never reach the render");
        assert!(!t.contains("headers"), "not even the key name");
    }

    /// A missing counter leaves its slot out rather than printing a zero, an absent
    /// attempt renders the declared `?` placeholder, and a line the caller cannot read
    /// renders the bare head with no invented field.
    #[test]
    fn api_error_head_elides_what_the_line_does_not_carry() {
        let bare = r#"{"type":"system","subtype":"api_error","uuid":"e2","level":"error"}"#;
        assert_eq!(text(bare).as_deref(), Some("[api-error]"));
        let only_max = r#"{"type":"system","subtype":"api_error","uuid":"e3","maxRetries":10}"#;
        assert_eq!(text(only_max).as_deref(), Some("[api-error retry ?/10]"));
        let only_attempt =
            r#"{"type":"system","subtype":"api_error","uuid":"e4","retryAttempt":2}"#;
        assert_eq!(text(only_attempt).as_deref(), Some("[api-error retry 2]"));
        assert_eq!(text_unread(bare).as_deref(), Some("[api-error]"));
        // An `error` that is not an object, and one with neither descriptive field:
        // the head stands and the body is empty rather than a JSON fragment.
        let odd = r#"{"type":"system","subtype":"api_error","uuid":"e5","error":"boom"}"#;
        assert_eq!(text(odd).as_deref(), Some("[api-error]"));
        let empty = r#"{"type":"system","subtype":"api_error","uuid":"e6","error":{"status":500}}"#;
        assert_eq!(text(empty).as_deref(), Some("[api-error status=500]"));
        assert_eq!(
            error_summary(Some(&serde_json::json!({"status": 500}))),
            None
        );
    }

    /// The three remaining event kinds: the notice keeps its level, the kill has no
    /// payload to print, and the delivered command keeps its content verbatim.
    #[test]
    fn notice_kill_and_command_heads() {
        let notice = r#"{"type":"system","subtype":"informational","uuid":"n1","level":"warning","content":"Remote Control disconnected"}"#;
        assert_eq!(
            text(notice).as_deref(),
            Some("[notice warning] Remote Control disconnected")
        );
        let unlevelled = r#"{"type":"system","subtype":"informational","uuid":"n2","level":"","content":"plain"}"#;
        assert_eq!(text(unlevelled).as_deref(), Some("[notice] plain"));
        let kill = r#"{"type":"system","subtype":"agents_killed","uuid":"k1"}"#;
        assert_eq!(text(kill).as_deref(), Some("[kill]"));
        let cmd = r#"{"type":"system","subtype":"local_command","uuid":"c1","content":"<local-command-stdout>ok</local-command-stdout>"}"#;
        assert_eq!(
            text(cmd).as_deref(),
            Some("[command] <local-command-stdout>ok</local-command-stdout>")
        );
        // A non-string content renders as its own JSON rather than vanishing.
        let odd =
            r#"{"type":"system","subtype":"agents_killed","uuid":"k2","content":{"count":2}}"#;
        assert_eq!(text(odd).as_deref(), Some(r#"[kill] {"count":2}"#));
    }

    /// The fire record's two shapes: the cron arm names its schedule and its armed text,
    /// the `/loop` wakeup arm names none, and both read the instant from the SAME
    /// parenthesised tail.
    #[test]
    fn schedule_event_head_covers_both_fire_shapes() {
        let cron = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s1","content":"Running scheduled task (Jun 7 5:28am)","taskId":"task-zz","cron":"0 5 * * *","prompt":"check the queue"}"#;
        assert_eq!(
            text(cron).as_deref(),
            Some(
                "[scheduled Jun 7 5:28am] task=task-zz cron=0 5 * * * prompt=check the queue \
                 Running scheduled task (Jun 7 5:28am)"
            )
        );
        let wakeup = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s2","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#;
        assert_eq!(
            text(wakeup).as_deref(),
            Some("[scheduled Jun 7 6:40am] Claude resuming /loop wakeup (Jun 7 6:40am)")
        );
        // An unreadable tail is an unknown instant, never a guessed one.
        let bare = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s3","content":"Running scheduled task"}"#;
        assert_eq!(
            text(bare).as_deref(),
            Some("[scheduled] Running scheduled task")
        );
    }

    /// The armed prompt is CUT at exactly [`PROMPT_MAX`] characters, and the head carries
    /// the SAME cut the hit's `prompt` key does. The expectation is written from the
    /// documented budget rather than from the constant, so a change to either side is a
    /// failure rather than a silently agreed pair.
    #[test]
    fn the_armed_prompt_is_cut_at_the_documented_two_hundred_characters() {
        let long = "z".repeat(260);
        let line = format!(
            r#"{{"type":"system","subtype":"scheduled_task_fire","uuid":"s4","content":"Running scheduled task (Jun 7 5:28am)","prompt":"{long}"}}"#
        );
        let t = text(&line).expect("renders");
        // The budget is written as the LITERAL 200 on this side and read as `PROMPT_MAX`
        // on the other, so a change to the constant fails here instead of agreeing with
        // itself; the elision marker comes from the shared helper rather than a second
        // spelling of it.
        let cut = crate::text::truncate_excerpt(&long, 200);
        assert!(
            cut.starts_with(&"z".repeat(200)) && cut.contains("+60 chars"),
            "the shared helper cuts at 200 and says how much it dropped: {cut}"
        );
        assert!(
            t.contains(&format!("prompt={cut}")),
            "the head carries that exact cut: {t}"
        );
    }

    /// A numeric slot holding a STRING renders that string rather than a JSON fragment:
    /// the head reports what the line says, whatever shape the harness put there.
    #[test]
    fn a_string_counter_renders_as_the_string_the_line_holds() {
        let strs = r#"{"type":"system","subtype":"api_error","uuid":"ea","error":{"status":"529"},"retryAttempt":"3","maxRetries":"20","retryInMs":"1522"}"#;
        assert_eq!(
            text(strs).as_deref(),
            Some("[api-error status=529 retry 3/20 in 1522ms]")
        );
    }

    /// A record outside the six leaves renders nothing here - the boundary and the three
    /// telemetry leaves keep their own renders.
    #[test]
    fn a_non_event_system_record_renders_nothing_here() {
        for line in [
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","content":"Conversation compacted"}"#,
            r#"{"type":"system","subtype":"turn_duration","uuid":"d1","durationMs":10}"#,
            r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}"#,
        ] {
            assert_eq!(text(line), None, "{line}");
        }
    }
}
