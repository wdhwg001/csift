//! What the watched session did WHILE `wait` waited: a census of the records that
//! landed after the baseline, classified through the same engine `search -t` uses.
//! Rendered on every exit (a fired condition or the timeout) so a caller learns what
//! the session was busy with, not just that the bound elapsed.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub(crate) struct Activity {
    pub(crate) records: usize,
    pub(crate) lanes: BTreeSet<String>,
    /// tool name -> tool_use count.
    pub(crate) tools: BTreeMap<String, usize>,
    pub(crate) thinking: usize,
    pub(crate) agent_messages: usize,
    pub(crate) user_prompts: usize,
    pub(crate) notifications: usize,
    /// One entry per SHRINK seen while waiting (`<lane>: -<bytes>`): the harness rewrote
    /// the transcript in place, so the baseline moved to the new end (v0.10.4).
    pub(crate) shrinks: Vec<String>,
}

impl Activity {
    /// Record that `lane`'s transcript lost `bytes` bytes between two polls.
    pub(crate) fn note_shrink(&mut self, lane: &str, bytes: u64) {
        self.shrinks.push(format!("{lane}: -{bytes} bytes"));
    }

    /// Fold one post-baseline record (any lane).
    pub(crate) fn fold(&mut self, rec: &Record, lane: &str) {
        self.records += 1;
        self.lanes.insert(lane.to_string());
        // A pulse absorbed mid-turn is a queue enqueue line / a queued_command
        // attachment, never a labeled record (v0.10.2): count those deliveries too, so
        // the census agrees with what `--until notification` can fire on.
        if rec.is_type("queue-operation") || rec.attachment_type().is_some() {
            self.notifications += crate::live::delivered_pulse_labels(rec).len();
            return;
        }
        let labels = rec.classify(&crate::model::ClassifyCtx::top_level());
        for c in &labels {
            match c {
                crate::model::Class::AgentToolUse => {
                    for b in rec.blocks().unwrap_or_default() {
                        if let Block::ToolUse { name, .. } = b {
                            let n = name.clone().unwrap_or_else(|| "(unnamed)".to_string());
                            *self.tools.entry(n).or_insert(0) += 1;
                        }
                    }
                }
                crate::model::Class::AgentThinking
                | crate::model::Class::AgentThinkingNarration => self.thinking += 1,
                crate::model::Class::AgentMessage => self.agent_messages += 1,
                crate::model::Class::UserMessage => self.user_prompts += 1,
                c if c.path().starts_with("harness.notification") => self.notifications += 1,
                _ => {}
            }
        }
    }

    /// `12 record(s) in 2 lane(s): tools Bash x7 Read x3 · thinking 4 · messages 2 ·
    /// prompts 0 · notifications 1`; `nothing landed` when the baseline never moved.
    pub(crate) fn summary_line(&self) -> String {
        let shrank = if self.shrinks.is_empty() {
            String::new()
        } else {
            format!(
                " · transcript shrank {} time(s) ({}): rewritten in place, baseline moved",
                self.shrinks.len(),
                self.shrinks.join(", ")
            )
        };
        if self.records == 0 {
            return format!("nothing landed after the baseline{shrank}");
        }
        let tools = if self.tools.is_empty() {
            "no tools".to_string()
        } else {
            let mut by_count: Vec<(&String, &usize)> = self.tools.iter().collect();
            by_count.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            format!(
                "tools {}",
                by_count
                    .iter()
                    .map(|(n, c)| format!("{n} x{c}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        format!(
            "{} record(s) in {} lane(s): {tools} · thinking {} · messages {} · prompts {} · \
             notifications {}{shrank}",
            self.records,
            self.lanes.len(),
            self.thinking,
            self.agent_messages,
            self.user_prompts,
            self.notifications
        )
    }

    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "records": self.records,
            "lanes": self.lanes.len(),
            "tools": self.tools,
            "thinking": self.thinking,
            "agent_messages": self.agent_messages,
            "user_prompts": self.user_prompts,
            "notifications": self.notifications,
            "shrinks": self.shrinks,
        })
    }
}
