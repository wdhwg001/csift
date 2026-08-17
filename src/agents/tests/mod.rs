use super::*;
use crate::subagent::{ReturnedMsgSource, SubagentStatus};

fn node(
    trigger: Option<&str>,
    start: Option<&str>,
    complete: Option<&str>,
    kind: SubagentKind,
) -> SubagentNode {
    SubagentNode {
        agent_id: "abc123".to_string(),
        kind,
        parent_session_id: "sess".to_string(),
        parent_agent_id: None,
        spawn_tool_use_id: Some("toolu_x".to_string()),
        spawn_tool: Some("Agent".to_string()),
        workflow_id: None,
        agent_type: None,
        name: None,
        team_name: None,
        description: None,
        trigger_utc: trigger.map(str::to_string),
        started_utc: start.map(str::to_string),
        completed_utc: complete.map(str::to_string),
        last_activity_utc: complete.map(str::to_string),
        returned_message: None,
        returned_message_source: None,
        status: SubagentStatus::Completed,
        pending_tool_use_id: None,
        pending_tool_name: None,
        pending_classification: None,
        pending_since_utc: None,
        files_changed: Vec::new(),
        depth: 0,
        children: Vec::new(),
        skipped_lines: 0,
    }
}

mod part01;
