//! ONE fixture home carrying at least one line per leaf of `Class::ALL`.
//!
//! The taxonomy advertises 73 leaves, and each family's own suite pins the leaves that
//! family owns, which leaves the question none of them answers: is EVERY advertised leaf
//! reachable from a transcript on disk, through the real binary, at once? A leaf nothing
//! can produce is a selector that reports a definitive absence over records that exist,
//! and that is exactly the failure the empty-result diagnosis is built to make legible -
//! so it must never be csift's own doing.
//!
//! Every line here is built from the shape `model::inventory` names for that leaf, never
//! copied from a corpus. The two transcripts split on one fact only: the SUBAGENT file
//! carries the shape whose leaf needs a sidechain lane (the delivered spawn prompt), and
//! the top-level file carries everything else.
//!
//! THE CHAIN IS PART OF THE FIXTURE. Claude Code reconstructs a conversation by walking
//! `parentUuid` from one leaf, and csift mirrors that walk, so a record no walk reaches
//! is abandoned and a turn-opening one is relabelled `user.unsent` whatever it was. The
//! transcript below is therefore ONE unbroken chain from its `compact_boundary` head to
//! its last record, with exactly two deliberate branches off it: the recalled draft and
//! the rewound turn, which are the two leaves that can only exist off the chain.
//!
//! Where a leaf needs more than a line - a draft and its resend, a fire record and the
//! prompt parented to it, a tool_use and its result - the pair sits together with a
//! comment naming what the second line is for.

use super::*;

/// The encoded project directory both reachability transcripts live under.
pub(crate) const REACH_ENC: &str = "-Users-dev-reach-project";
/// The top-level transcript.
pub(crate) const REACH_SESS: &str = "3a1b2c3d-4e5f-4061-8273-849506a7b8c9";
/// Its one subagent lane.
pub(crate) const REACH_SUB: &str = "d00dfeedd00dfeed";

// The two thinking signatures below are hand-emitted protobuf on the field 2 -> 1 -> 8
// tag path `model::narration` decodes: `CAISDAoKQgh0aGlua2luZw==` carries the tag
// `thinking` and `CAISDQoLQgluYXJyYXRpb24=` carries `narration`. They are written out at
// their lines because `concat!` takes literals only.

/// A home whose two transcripts carry every leaf of `Class::ALL` at least once.
pub(crate) fn reachability_home() -> Home {
    let h = Home::new();
    h.write(&format!("{REACH_ENC}/{REACH_SESS}.jsonl"), TOP_LEVEL);
    h.write(
        &format!("{REACH_ENC}/{REACH_SESS}/subagents/agent-{REACH_SUB}.jsonl"),
        SUBAGENT,
    );
    h
}

/// The subagent lane: the delivered spawn prompt is the opener a sidechain transcript
/// starts with, and `agent.communication.inbox` reads it as parent to self.
const SUBAGENT: &str = concat!(
    r#"{"type":"user","isSidechain":true,"agentId":"d00dfeedd00dfeed","uuid":"s1","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"zzreach do the delegated work"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreach on it"}]}}"#,
    "\n",
);

/// The top-level transcript. The `compact_boundary` leads because its own `parentUuid` is
/// null on disk and the walk ends there: putting it first makes the whole file resolvable
/// and leaves nothing in the pre-cut region.
const TOP_LEVEL: &str = concat!(
    // ── harness.compaction (the chain head, then the summary it wrote) ──
    r#"{"type":"system","subtype":"compact_boundary","uuid":"sy1","timestamp":"2026-06-07T05:00:00.000Z","content":"zzreach conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":200,"durationMs":50}}"#,
    "\n",
    r#"{"type":"user","uuid":"ud","parentUuid":"sy1","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"zzreach this session is being continued from a prior context"}}"#,
    "\n",
    // ── user.message ──
    r#"{"type":"user","uuid":"u1","parentUuid":"ud","sessionId":"3a1b2c3d-4e5f-4061-8273-849506a7b8c9","cwd":"/Users/dev/reach-project","version":"2.1.258","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"zzreach the person's own message"}}"#,
    "\n",
    // agent.tool.use - the question half of the AUQ pair below.
    r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_auq","name":"AskUserQuestion","input":{"questions":[{"question":"zzreach which lane?","options":[{"label":"left"},{"label":"right"}]}]}}]}}"#,
    "\n",
    // user.answer - the answered AUQ, carried on the answering tool_result.
    r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:00:04.000Z","toolUseResult":{"answers":{"zzreach which lane?":"left"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_auq","content":"User has answered your questions: \"zzreach which lane?\"=\"left\". You can now continue."}]}}"#,
    "\n",
    // agent.tool.use / agent.tool.result - an ordinary paired call.
    r#"{"type":"assistant","uuid":"ab","parentUuid":"u2","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_read","name":"Read","input":{"file_path":"/Users/dev/reach-project/zzreach.rs"}}]}}"#,
    "\n",
    r#"{"type":"user","uuid":"u6","parentUuid":"ab","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_read","content":"zzreach the file contents"}]}}"#,
    "\n",
    // user.rejection - a refused tool call carrying the operator's typed instruction. The
    // result is an ERROR by construction: a rejection always lands `is_error`.
    r#"{"type":"assistant","uuid":"a2","parentUuid":"u6","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_plan","name":"ExitPlanMode","input":{"plan":"zzreach the plan"}}]}}"#,
    "\n",
    r#"{"type":"user","uuid":"u3","parentUuid":"a2","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_plan","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nzzreach do it the other way"}]}}"#,
    "\n",
    // user.unsent - the DRAFT: it shares `parentUuid` with the opener below and no
    // assistant record hangs beneath it, which is the whole discriminator.
    r#"{"type":"user","uuid":"u4a","parentUuid":"u3","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"user","content":"zzreach a draft the operator recalled"}}"#,
    "\n",
    // ... and the message that replaced it, which the chain does reach.
    r#"{"type":"user","uuid":"u4b","parentUuid":"u3","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"zzreach the message that was sent"}}"#,
    "\n",
    // user.rewound - the same shape WITH an assistant descendant, so the conversation was
    // rewound past an ANSWERED turn rather than a recalled draft.
    r#"{"type":"user","uuid":"u5a","parentUuid":"u4b","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"user","content":"zzreach the turn the rewind left behind"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"a5","parentUuid":"u5a","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreach the answer on the abandoned branch"}]}}"#,
    "\n",
    r#"{"type":"user","uuid":"u5b","parentUuid":"u4b","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"user","content":"zzreach the turn the conversation continued from"}}"#,
    "\n",
    // ── agent.message ──
    r#"{"type":"assistant","uuid":"a6","parentUuid":"u5b","timestamp":"2026-06-07T05:00:14.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreach the assistant's visible reply"}]}}"#,
    "\n",
    // agent.message.structured - the model's own structured answer, on an attachment.
    r#"{"type":"attachment","uuid":"at1","parentUuid":"a6","timestamp":"2026-06-07T05:00:15.000Z","attachment":{"type":"structured_output","output":{"verdict":"zzreach the schema-bound answer"}}}"#,
    "\n",
    // ── agent.thinking (four shapes, two of which render no body at all) ──
    r#"{"type":"assistant","uuid":"a7","parentUuid":"at1","timestamp":"2026-06-07T05:00:16.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"zzreach weighing the options","signature":"CAISDAoKQgh0aGlua2luZw=="}]}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"a8","parentUuid":"a7","timestamp":"2026-06-07T05:00:17.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"zzreach a one-line progress note","signature":"CAISDQoLQgluYXJyYXRpb24="}]}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"a9","parentUuid":"a8","timestamp":"2026-06-07T05:00:18.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"","signature":"CAISDAoKQgh0aGlua2luZw=="}]}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"aa","parentUuid":"a9","timestamp":"2026-06-07T05:00:19.000Z","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"zzreachAAAA"}]}}"#,
    "\n",
    // ── agent.communication ──
    r#"{"type":"user","uuid":"u7","parentUuid":"aa","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\" color=\"blue\">\nzzreach please check the rate limit\n</teammate-message>"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"ac","parentUuid":"u7","timestamp":"2026-06-07T05:00:21.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_sm1","name":"SendMessage","input":{"to":"Reachy","type":"message","message":"zzreach ship the fix"}}]}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"ad","parentUuid":"ac","timestamp":"2026-06-07T05:00:22.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_sm2","name":"SendMessage","input":{"to":"Reachy","type":"shutdown_request","message":{"type":"shutdown_request","reason":"zzreach stop now"}}}]}}"#,
    "\n",
    // agent.communication.channel - a csift-channel delivery, detected on the FIRST
    // content string of a hook_additional_context payload.
    r#"{"type":"attachment","uuid":"at2","parentUuid":"ad","timestamp":"2026-06-07T05:00:23.000Z","attachment":{"type":"hook_additional_context","hookName":"csift-deliver","hookEvent":"UserPromptSubmit","content":["[csift-channel v1 id=00112233445566aa part=1/1 mode=steer from=3a1b2c3d-4e5f-4061-8273-849506a7b8c9]\nzzreach the delivered message"]}}"#,
    "\n",
    // ── harness.notification (five kinds, one pulse each) ──
    r#"{"type":"user","uuid":"u8","parentUuid":"at2","timestamp":"2026-06-07T05:00:24.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf_zz1</task-id>\n<status>completed</status>\n<summary>Workflow \"zzreach pipeline\" completed</summary>\n</task-notification>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"u9","parentUuid":"u8","timestamp":"2026-06-07T05:00:25.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>mon_zz1</task-id>\n<event>tick</event>\n<summary>Monitor \"zzreach liveness\" fired</summary>\n</task-notification>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"ua","parentUuid":"u9","timestamp":"2026-06-07T05:00:26.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>agent_zz1</task-id>\n<status>completed</status>\n<summary>Agent \"zzreach auditor\" completed</summary>\n</task-notification>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"ub","parentUuid":"ua","timestamp":"2026-06-07T05:00:27.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1234567</task-id>\n<status>completed</status>\n<summary>Background command \"zzreach cargo build\" completed</summary>\n</task-notification>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"uc","parentUuid":"ub","timestamp":"2026-06-07T05:00:28.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>task_zz1</task-id>\n<status>completed</status>\n<summary>zzreach something else finished</summary>\n</task-notification>"}}"#,
    "\n",
    // ── harness.command ──
    r#"{"type":"user","uuid":"ue","parentUuid":"uc","timestamp":"2026-06-07T05:00:29.000Z","message":{"role":"user","content":"<command-name>/deploy</command-name>\n<command-message>deploy</command-message>\n<command-args>zzreach to staging</command-args>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"uf","parentUuid":"ue","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":"<local-command-stdout>zzreach deploy succeeded</local-command-stdout>"}}"#,
    "\n",
    // harness.system.command - the `local_command` echo, the one delivered system subtype.
    r#"{"type":"system","subtype":"local_command","uuid":"sya","parentUuid":"uf","timestamp":"2026-06-07T05:00:31.000Z","content":"zzreach the local command's output"}"#,
    "\n",
    r#"{"type":"user","uuid":"ug","parentUuid":"sya","isMeta":true,"timestamp":"2026-06-07T05:00:32.000Z","message":{"role":"user","content":"<local-command-caveat>The messages below were generated by the user while running local commands. zzreach</local-command-caveat>"}}"#,
    "\n",
    // ── harness.interrupt ──
    r#"{"type":"user","uuid":"uh","parentUuid":"ug","timestamp":"2026-06-07T05:00:33.000Z","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
    "\n",
    r#"{"type":"user","uuid":"ui","parentUuid":"uh","timestamp":"2026-06-07T05:00:34.000Z","message":{"role":"user","content":"[Request interrupted by user for tool use]"}}"#,
    "\n",
    // ── harness.schedule ──
    // wakeup: the fired timer tick.
    r##"{"type":"user","uuid":"uj","parentUuid":"ui","isMeta":true,"timestamp":"2026-06-07T05:00:35.000Z","message":{"role":"user","content":"# Autonomous loop check\n\nzzreach the armed check body"}}"##,
    "\n",
    // tick: the loop DRIVER, a different template.
    r##"{"type":"user","uuid":"uk","parentUuid":"uj","isMeta":true,"timestamp":"2026-06-07T05:00:36.000Z","message":{"role":"user","content":"# Autonomous loop tick (dynamic pacing)\nzzreach the driver body"}}"##,
    "\n",
    // event: the scheduler's own fire RECORD ...
    r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"sy2","parentUuid":"uk","timestamp":"2026-06-07T05:00:37.000Z","content":"Running scheduled task (Jun 7 5:00am)","taskId":"cron_zz1","cron":"0 5 * * *","prompt":"zzreach the armed prompt"}"#,
    "\n",
    // ... and fire: the prompt it submitted, parented to that record. The parent link is
    // the whole gate - the armed text is whatever the operator stored.
    r#"{"type":"user","uuid":"ul","parentUuid":"sy2","isMeta":true,"timestamp":"2026-06-07T05:00:38.000Z","message":{"role":"user","content":"zzreach whatever the operator armed"}}"#,
    "\n",
    // ── harness.resume ──
    r#"{"type":"user","uuid":"um","parentUuid":"ul","isMeta":true,"timestamp":"2026-06-07T05:00:39.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"ae","parentUuid":"um","timestamp":"2026-06-07T05:00:40.000Z","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
    "\n",
    // ── harness.hook ──
    r#"{"type":"attachment","uuid":"at3","parentUuid":"ae","timestamp":"2026-06-07T05:00:41.000Z","attachment":{"type":"hook_additional_context","hookName":"SessionStart","hookEvent":"SessionStart","content":["zzreach the context a hook injected"]}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"at4","parentUuid":"at3","timestamp":"2026-06-07T05:00:42.000Z","attachment":{"type":"hook_success","hookName":"SessionStart","command":"zzreach-hook.sh","exitCode":0,"durationMs":12,"stdout":"zzreach the hook run's own output"}}"#,
    "\n",
    r#"{"type":"user","uuid":"un","parentUuid":"at4","isMeta":true,"timestamp":"2026-06-07T05:00:43.000Z","message":{"role":"user","content":"Stop hook feedback:\nzzreach keep going"}}"#,
    "\n",
    // ── harness.context (nine leaves) ──
    r#"{"type":"attachment","uuid":"at5","parentUuid":"un","timestamp":"2026-06-07T05:00:44.000Z","attachment":{"type":"total_tokens_reminder","content":"zzreach the token budget nudge"}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"at6","parentUuid":"at5","timestamp":"2026-06-07T05:00:45.000Z","attachment":{"type":"skill_listing","skills":["zzreach-demo"]}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"at7","parentUuid":"at6","timestamp":"2026-06-07T05:00:46.000Z","attachment":{"type":"environment","content":"zzreach the persisted environment section"}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"at8","parentUuid":"at7","timestamp":"2026-06-07T05:00:47.000Z","attachment":{"type":"edited_text_file","filePath":"/Users/dev/reach-project/zzreach.rs","snippet":"fn zzreach() {}"}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"at9","parentUuid":"at8","timestamp":"2026-06-07T05:00:48.000Z","attachment":{"type":"plan_mode","planPath":"/Users/dev/reach-project/plans/zzreach.md"}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"ata","parentUuid":"at9","timestamp":"2026-06-07T05:00:49.000Z","attachment":{"type":"opened_file_in_ide","filePath":"/Users/dev/reach-project/zzreach.rs"}}"#,
    "\n",
    r#"{"type":"attachment","uuid":"atb","parentUuid":"ata","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"task_status","status":"zzreach in progress"}}"#,
    "\n",
    // harness.context.image - the isMeta text reference the harness writes for a picture.
    r#"{"type":"user","uuid":"uo","parentUuid":"atb","isMeta":true,"timestamp":"2026-06-07T05:00:51.000Z","message":{"role":"user","content":"[Image: 1024x768] zzreach"}}"#,
    "\n",
    // harness.context.skill - a skill body preloaded into a lane's opening array.
    r#"{"type":"user","uuid":"up","parentUuid":"uo","isMeta":true,"timestamp":"2026-06-07T05:00:52.000Z","message":{"role":"user","content":"Base directory for this skill: /Users/dev/skills/zzreach"}}"#,
    "\n",
    // ── harness.enforce ──
    r#"{"type":"user","uuid":"uq","parentUuid":"up","isMeta":true,"timestamp":"2026-06-07T05:00:53.000Z","message":{"role":"user","content":"[structured-output-enforce] zzreach you must call the tool to complete this turn."}}"#,
    "\n",
    r#"{"type":"user","uuid":"ur","parentUuid":"uq","isMeta":true,"timestamp":"2026-06-07T05:00:54.000Z","message":{"role":"user","content":"You ended the turn without calling SendUserMessage. zzreach call it now."}}"#,
    "\n",
    r#"{"type":"user","uuid":"us","parentUuid":"ur","isMeta":true,"timestamp":"2026-06-07T05:00:55.000Z","message":{"role":"user","content":"[handback-send-enforce] zzreach your report has not been delivered."}}"#,
    "\n",
    r#"{"type":"user","uuid":"ut","parentUuid":"us","isMeta":true,"timestamp":"2026-06-07T05:00:56.000Z","message":{"role":"user","content":"The last Edit failed because the target file was modified. zzreach retry."}}"#,
    "\n",
    // ── harness.turn ──
    r#"{"type":"system","subtype":"turn_duration","uuid":"sy3","parentUuid":"ut","timestamp":"2026-06-07T05:00:57.000Z","durationMs":4200}"#,
    "\n",
    r#"{"type":"system","subtype":"stop_hook_summary","uuid":"sy4","parentUuid":"sy3","timestamp":"2026-06-07T05:00:58.000Z","toolUseID":"hook-zzreach0000","content":"zzreach stop hooks ran"}"#,
    "\n",
    r#"{"type":"system","subtype":"away_summary","uuid":"sy5","parentUuid":"sy4","timestamp":"2026-06-07T05:00:59.000Z","content":"zzreach while you were away"}"#,
    "\n",
    // ── harness.system (the four undelivered event kinds) ──
    r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"sy6","parentUuid":"sy5","timestamp":"2026-06-07T05:01:00.000Z","originalModel":"zzreach-opus","fallbackModel":"zzreach-sonnet","apiRefusalCategory":"overloaded","refusedUserMessageUuid":"u1"}"#,
    "\n",
    r#"{"type":"system","subtype":"api_error","uuid":"sy7","parentUuid":"sy6","timestamp":"2026-06-07T05:01:01.000Z","slug":"zzreach","error":{"type":"overloaded_error","message":"zzreach the upstream was overloaded"},"apiErrorStatus":529,"retryAttempt":1,"maxRetries":5,"retryInMs":800}"#,
    "\n",
    r#"{"type":"system","subtype":"informational","uuid":"sy8","parentUuid":"sy7","timestamp":"2026-06-07T05:01:02.000Z","level":"info","content":"zzreach a notice for the terminal"}"#,
    "\n",
    r#"{"type":"system","subtype":"agents_killed","uuid":"sy9","parentUuid":"sy8","timestamp":"2026-06-07T05:01:03.000Z","content":"zzreach agents stopped"}"#,
    "\n",
    // ── user.queued: a queue line whose content is the person's own text ──
    r#"{"type":"queue-operation","timestamp":"2026-06-07T05:01:04.000Z","operation":"enqueue","content":"zzreach the operator typed this while the turn ran"}"#,
    "\n",
    // ── harness.bookkeeping (thirteen leaves; none of these lines carries a uuid, so
    //    none is a chain node and `show --line` is the only address) ──
    r#"{"type":"last-prompt","timestamp":"2026-06-07T05:01:05.000Z","leafUuid":"sy9","explicit":false,"lastPrompt":"zzreach the person's own message"}"#,
    "\n",
    r#"{"type":"mode","mode":"zzreach-default"}"#,
    "\n",
    r#"{"type":"ai-title","title":"zzreach the generated session title"}"#,
    "\n",
    r#"{"type":"atis-latch","atis":"zzreach-latched"}"#,
    "\n",
    r#"{"type":"bridge-session","ownerAccountUuid":"11111111-2222-4333-8444-555555555555","bridgeNote":"zzreach"}"#,
    "\n",
    r#"{"type":"history-suppression","cause":"zzreach-veto","ts":"2026-06-07T05:01:06.000Z"}"#,
    "\n",
    r#"{"type":"pr-link","prUrl":"https://example.invalid/zzreach/1"}"#,
    "\n",
    r#"{"type":"frame-link","frameId":"zzreach-frame-1"}"#,
    "\n",
    r#"{"type":"file-history-delta","trackingPath":"/Users/dev/reach-project/zzreach.rs","backup":"zzreach-v2","messageId":"u1"}"#,
    "\n",
    r#"{"type":"cost-state","sessionId":"3a1b2c3d-4e5f-4061-8273-849506a7b8c9","totalCostUSD":0.25,"totalDuration":1000,"startTime":"2026-06-07T05:00:00.000Z","costNote":"zzreach"}"#,
    "\n",
    r#"{"type":"fork-context-ref","parentSessionId":"99999999-8888-4777-8666-555555555555","parentLastUuid":"u1","forkNote":"zzreach"}"#,
    "\n",
    r#"{"type":"continued-in","timestamp":"2026-06-07T05:01:07.000Z","sessionId":"3a1b2c3d-4e5f-4061-8273-849506a7b8c9","continuedInSessionId":"88888888-7777-4666-8555-444444444444"}"#,
    "\n",
    // harness.bookkeeping.queue - a queue line that is NOT the human's own text.
    r#"{"type":"queue-operation","timestamp":"2026-06-07T05:01:08.000Z","operation":"dequeue","reason":"delivered_as_tool_result"}"#,
    "\n",
);

/// Every `-t` selector the reachability census runs under: one GLOB per role, which is
/// the "every leaf under it, visibility ignored" form. There is no single every-leaf
/// selector, and that is deliberate - a bare role is the LLM-visible face.
pub(crate) const EVERY_LEAF: [&str; 6] = ["-t", "user.*", "-t", "agent.*", "-t", "harness.*"];
