//! `wait --until` condition grammar: parse + per-event matching.

use super::*;

/// One parsed `--until` condition.
#[derive(Debug)]
pub(crate) enum Cond {
    /// Verdict becomes idle-eot or stale-dead.
    Stop,
    /// Verdict becomes waiting-hitl.
    Hitl,
    /// The elicitation sidecar gains an unanswered question (or a native AUQ ask lands).
    Auq,
    /// A task-notification lands in the MAIN transcript (they never land in children),
    /// optionally payload-matched.
    Notification(Option<regex::Regex>),
    /// A `tool_use` of NAME appears whose serialized input matches (any watched lane).
    Tool {
        name: String,
        input_re: Option<regex::Regex>,
    },
    /// A Write/Edit/MultiEdit/NotebookEdit whose path matches (and whose written content
    /// contains a line matching, when given).
    Write {
        path_re: regex::Regex,
        line_re: Option<regex::Regex>,
    },
    /// Any verdict from the status table.
    VerdictIs(Verdict),
}

impl Cond {
    /// True when this condition needs a fresh VERDICT evaluation each poll (vs a
    /// record-event match on appended lines).
    #[must_use]
    pub(crate) fn needs_verdict(&self) -> bool {
        matches!(self, Cond::Stop | Cond::Hitl | Cond::VerdictIs(_))
    }
}

/// Parse one `--until` token. The grammar is closed; an unknown head is a hard error
/// naming the set (never a silent no-op condition).
pub(crate) fn parse_condition(s: &str) -> Result<Cond> {
    let mk_re = |p: &str, what: &str| -> Result<regex::Regex> {
        regex::Regex::new(p).map_err(|e| anyhow::anyhow!("--until {what}: bad regex `{p}`: {e}"))
    };
    if let Some(rest) = s.strip_prefix("notification") {
        return Ok(match rest.strip_prefix(':') {
            Some(re) => Cond::Notification(Some(mk_re(re, "notification")?)),
            None if rest.is_empty() => Cond::Notification(None),
            _ => bail!("--until: unknown condition `{s}` (did you mean `notification:{rest}`?)"),
        });
    }
    if let Some(rest) = s.strip_prefix("tool:") {
        let mut it = rest.splitn(2, ':');
        let name = it.next().unwrap_or_default();
        if name.is_empty() {
            bail!("--until tool: needs a tool NAME (`tool:NAME[:REGEX]`)");
        }
        let input_re = it.next().map(|re| mk_re(re, "tool")).transpose()?;
        return Ok(Cond::Tool {
            name: name.to_string(),
            input_re,
        });
    }
    if let Some(rest) = s.strip_prefix("write:") {
        let mut it = rest.splitn(2, ':');
        let path = it.next().unwrap_or_default();
        if path.is_empty() {
            bail!("--until write: needs a path regex (`write:PATH_RE[:LINE_RE]`)");
        }
        let line_re = it.next().map(|re| mk_re(re, "write")).transpose()?;
        return Ok(Cond::Write {
            path_re: mk_re(path, "write")?,
            line_re,
        });
    }
    if let Some(v) = s.strip_prefix("verdict:") {
        let verdict = match v {
            "running" => Verdict::Running,
            "waiting-children" => Verdict::WaitingChildren,
            "waiting-hitl" => Verdict::WaitingHitl,
            "idle-eot" => Verdict::IdleEot,
            "stale-dead" => Verdict::StaleDead,
            "unknown" => Verdict::Unknown,
            other => bail!(
                "--until verdict: unknown verdict `{other}` (running | waiting-children | \
                 waiting-hitl | idle-eot | stale-dead | unknown)"
            ),
        };
        return Ok(Cond::VerdictIs(verdict));
    }
    match s {
        "stop" => Ok(Cond::Stop),
        "hitl" => Ok(Cond::Hitl),
        "auq" => Ok(Cond::Auq),
        other => bail!(
            "--until: unknown condition `{other}`. The set: stop | hitl | auq | \
             notification[:REGEX] | tool:NAME[:REGEX] | write:PATH_RE[:LINE_RE] | \
             verdict:V"
        ),
    }
}

/// Match a freshly appended RECORD line against the record-event conditions. `is_main`
/// scopes the notification carrier (they persist only in the main transcript).
pub(crate) fn record_matches(cond: &Cond, rec: &crate::model::Record, is_main: bool) -> bool {
    match cond {
        Cond::Notification(re) => {
            if !is_main {
                return false;
            }
            let Some(label) = rec.automation_label() else {
                return false;
            };
            re.as_ref().is_none_or(|r| {
                r.is_match(&label)
                    || rec
                        .reconstructed_user_text(None)
                        .as_deref()
                        .is_some_and(|t| r.is_match(t))
            })
        }
        Cond::Auq => {
            // The sidecar ask (real-time) OR a native AskUserQuestion tool_use landing
            // (an answered AUQ's buffered turn - post-hoc but still the ask on disk).
            if rec.is_elicitation_marker() {
                return rec.csift_phase.as_deref() == Some("pending");
            }
            rec.blocks().is_some_and(|bs| {
                bs.iter().any(|b| {
                    matches!(b, crate::model::Block::ToolUse { name: Some(n), .. }
                        if n == "AskUserQuestion")
                })
            })
        }
        Cond::Tool { name, input_re } => rec.blocks().is_some_and(|bs| {
            bs.iter().any(|b| match b {
                crate::model::Block::ToolUse {
                    name: Some(n),
                    input,
                    ..
                } if n == name => input_re.as_ref().is_none_or(|re| {
                    input
                        .as_ref()
                        .map(|i| re.is_match(&i.to_string()))
                        .unwrap_or(false)
                }),
                _ => false,
            })
        }),
        Cond::Write { path_re, line_re } => rec.blocks().is_some_and(|bs| {
            bs.iter().any(|b| match b {
                crate::model::Block::ToolUse {
                    name: Some(n),
                    input: Some(input),
                    ..
                } if matches!(n.as_str(), "Write" | "Edit" | "MultiEdit" | "NotebookEdit") => {
                    let path = input
                        .get("file_path")
                        .or_else(|| input.get("notebook_path"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if !path_re.is_match(path) {
                        return false;
                    }
                    line_re.as_ref().is_none_or(|re| {
                        ["content", "new_string", "new_source"].iter().any(|k| {
                            input
                                .get(*k)
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|c| c.lines().any(|l| re.is_match(l)))
                        })
                    })
                }
                _ => false,
            })
        }),
        // Verdict-class conditions are evaluated on the assessment, not per record.
        Cond::Stop | Cond::Hitl | Cond::VerdictIs(_) => false,
    }
}

/// Match a fresh assessment against the verdict-class conditions.
pub(crate) fn verdict_matches(cond: &Cond, verdict: Verdict) -> bool {
    match cond {
        Cond::Stop => matches!(verdict, Verdict::IdleEot | Verdict::StaleDead),
        Cond::Hitl => verdict == Verdict::WaitingHitl,
        Cond::VerdictIs(v) => verdict == *v,
        _ => false,
    }
}
