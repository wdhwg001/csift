use super::*;
use std::io::Write;

/// A scratch projects-root mimicking the real on-disk layout, removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        // A process-wide atomic sequence guarantees a unique root per instance even
        // when two `Fixture::new()` calls on parallel test threads land on the same
        // PID + nanosecond — otherwise their `Drop` `remove_dir_all` could wipe a
        // sibling test's tree mid-run (a ~8% flake on the default parallel runner).
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("csift-sub-{}-{n}-{seq}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        p
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const SESS: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

/// Build a session jsonl + its sidecar with one built-in, one workflow agent,
/// and a workflow journal (which must be ignored as a transcript).
///
/// The PARENT transcript carries the spawn linkage the topology joins on: an `Agent`
/// tool_use (`toolu_x`, == the built-in meta's `toolUseId`) at 04:59:58 whose paired
/// tool_result is the SYNC returned message, plus a `Workflow` tool_use (`toolu_w`).
/// A top-level `workflows/wf_abc.json` manifest is also written (NOT under
/// `subagents/workflows/`) so the WorkflowRun reader has a manifest to find.
fn layout(fx: &Fixture) -> PathBuf {
    let enc = "-Users-testuser-Projects-foo";
    let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
                // Agent tool_use that spawned aaa111 — its ts is the TRUE trigger time
                // (~2s BEFORE the child-head ts of 05:00:00).
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:59:58.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_x\",\"name\":\"Agent\",\"input\":{\"description\":\"run it\",\"subagent_type\":\"oh-my-claudecode:executor\"}}]}}\n",
                // The SYNC tool_result carrying aaa111's returned message.
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_x\",\"content\":\"SYNC RETURN: the built-in answer\"}]}}\n",
                // Workflow tool_use that launched wf_abc.
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:59:55.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_w\",\"name\":\"Workflow\",\"input\":{\"description\":\"the wf\"}}]}}\n"
            ),
        );

    // (A) built-in agent transcript + meta.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aaa111\",\"sessionId\":\"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"do the thing\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:03:20.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n"
            ),
        );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.meta.json"),
            "{\"agentType\":\"oh-my-claudecode:executor\",\"description\":\"run it\",\"toolUseId\":\"toolu_x\"}",
        );

    // (B) workflow agent transcript + meta + (C) journal with a result event.
    fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"bbb222\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"wf task\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    fx.write(
        &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.meta.json"),
        "{\"agentType\":\"workflow-subagent\"}",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/journal.jsonl"),
            concat!(
                "{\"type\":\"started\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\"}\n",
                "{\"type\":\"result\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\",\"result\":\"WF RETURN: workflow journal payload\"}\n"
            ),
        );

    // Top-level workflow RUN manifest (NOT under subagents/) — the WorkflowRun source.
    fx.write(
            &format!("{enc}/{SESS}/workflows/wf_abc.json"),
            "{\"runId\":\"wf_abc\",\"taskId\":\"t9\",\"workflowName\":\"demo-wf\",\"status\":\"completed\",\"agentCount\":1,\"durationMs\":62000,\"totalTokens\":12345,\"totalToolCalls\":7,\"defaultModel\":\"claude-opus-4-8[1m]\",\"startTime\":\"2026-06-07T05:59:55.000Z\"}",
        );

    session
}

/// A session that spawned ONE teammate (`taskKind:in_process_teammate`) via an `Agent`
/// tool_use carrying `input.name` (the name-join key) + the real `subagent_type`. The
/// teammate meta deliberately overloads `agentType` with the handle (as CC does) and omits
/// `toolUseId`, so only the NAME-join can recover its spawn linkage + real type.
fn teammate_layout(fx: &Fixture) -> PathBuf {
    let enc = "-Users-testuser-Projects-foo";
    let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
                // The Agent tool_use that spawned the teammate: NO paired meta toolUseId on the
                // child, so the topology must join by input.name. Its ts is the TRUE trigger,
                // ~0.5s before the child head (05:00:00.500).
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_team\",\"name\":\"Agent\",\"input\":{\"description\":\"repro the bug\",\"subagent_type\":\"oh-my-claudecode:qa-tester\",\"name\":\"VSRepro\"}}]}}\n"
            ),
        );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aVSRepro-68a2a1661c9390c1.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aVSRepro-68a2a1661c9390c1\",\"sessionId\":\"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\",\"timestamp\":\"2026-06-07T05:00:00.500Z\",\"message\":{\"role\":\"user\",\"content\":\"<teammate-message teammate_id=\\\"team-lead\\\">repro it</teammate-message>\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:10:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"the matrix result\"}]}}\n"
            ),
        );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aVSRepro-68a2a1661c9390c1.meta.json"),
            "{\"agentType\":\"VSRepro\",\"description\":\"repro the bug\",\"name\":\"VSRepro\",\"taskKind\":\"in_process_teammate\",\"teamName\":\"session-25f56dee\",\"color\":\"purple\"}",
        );
    session
}

mod part01;
mod part02;
mod part03;
