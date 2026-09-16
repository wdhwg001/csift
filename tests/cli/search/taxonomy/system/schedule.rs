//! The scheduler's own fire record: its two arms, and the id join that is why the
//! task id is worth exposing at all.

use super::*;

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
    // `scheduled_at` is the ENGINE's field with ONE writer, and it means one thing on
    // both halves of the pair: the instant the schedule fired. On the fired PROMPT the
    // writer reads the per-file index; on the EVENT record it reads the record's own
    // parenthesised tail, which is the line the instant is written on. It read null here,
    // which said the scheduler's own fire record does not know when it fired.
    assert_eq!(cron["scheduled_at"], serde_json::json!("Jun 7 5:28am"));
    // The head still carries it too, because every exposed key of this family is also IN
    // the rendered text - a key that reached JSON alone would be unsearchable.
    assert!(
        out.stdout.contains("[scheduled Jun 7 5:28am]"),
        "{}",
        out.stdout
    );
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
