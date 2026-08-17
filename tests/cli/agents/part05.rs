use crate::harness::*;

#[test]
fn mcp_pending_is_merged_into_turns() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            mcp_pending_line(
                "el-9",
                "2026-06-27T01:10:00.000Z",
                "gdrive",
                "Authorize Google Drive access"
            )
        ),
    );
    let out = h.run(&["verbatim", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("mcp-elicitation: [gdrive]"),
        "turns must include the pending MCP elicitation:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );
}

#[test]
fn acceptance_communication_sent_spawn_and_subagent_opener() {
    // §C8 a Task/Agent spawn tool_use → `agent.communication.sent` (self ⇨ child); §C6 SendMessage
    // message → sent; §C9 the subagent transcript opener → `agent.communication.inbox` (parent ⇨ self).
    let h = acceptance_home();

    let spawn = acc(&h, "zzspawn", "agent.communication.sent");
    assert!(spawn.success, "C8: stderr {}", spawn.stderr);
    assert!(
        spawn.stdout.contains("agent.communication.sent")
            && spawn.stdout.contains("self ⇨ audit-x"),
        "C8 spawn → comm.sent (self ⇨ audit-x):\n{}",
        spawn.stdout
    );

    let sent = acc(&h, "zzsent", "agent.communication.sent");
    assert!(
        sent.stdout.contains("agent.communication.sent")
            && sent.stdout.contains("self ⇨ GraftBoard"),
        "C6 SendMessage message → comm.sent (self ⇨ GraftBoard):\n{}",
        sent.stdout
    );

    // C9 needs subagent span (the opener lives in the subagent transcript).
    let opener = h.run(&[
        "search",
        "zzopener",
        "-t",
        "agent.communication.inbox",
        &at(ACC_SESS),
    ]);
    assert!(opener.success, "C9: stderr {}", opener.stderr);
    assert!(
        opener.stdout.contains("agent.communication.inbox") && opener.stdout.contains("⇨ self"),
        "C9 subagent opener → comm.inbox (parent ⇨ self):\n{}",
        opener.stdout
    );
}
