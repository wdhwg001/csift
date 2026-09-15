//! The SHAPE inventory: every on-disk shape csift has modeled, and what it does with it.
//!
//! One row per shape along the five axes a Claude Code transcript actually splits on -
//! the top-level `type` of a line, the `subtype` of a `type:"system"` line, the payload
//! `type` of a `type:"attachment"` line, the type of a content BLOCK, and the isMeta-user
//! TEMPLATE a record's text opens with. Each row names the leaf that shape carries (or
//! `None` where the leaf is decided on a deeper axis, or where csift models none), the
//! GATE that admits the line to a scan, whether the model receives the content, and
//! whether a human sees it in the terminal.
//!
//! Why a table rather than the code: the classify engine answers "what is THIS record",
//! and no caller can ask it "what shapes exist and which are unmodeled". SPEC section 5
//! renders this table, `SKILL.md`'s transparency tables render it, and the drift tests
//! beside it pin it against [`Class::ALL`] in both directions - so a leaf added without a
//! shape, or a shape added without a treatment, fails here rather than shipping silent.
//!
//! It is a RECORD of the mapping, never a second implementation of it: the classify
//! family tables stay the one place a record's leaf is decided, and a unit test walks
//! this table through them so the two can never disagree.

mod attachments;
mod blocks;
mod lines;
mod subtypes;
mod templates;

#[cfg(test)]
mod tests;

use super::*;

/// Which axis a shape sits on, and its value on that axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// A line's own top-level `type` value.
    TopLevelType(&'static str),
    /// The `subtype` of a `type:"system"` line.
    SystemSubtype(&'static str),
    /// The payload `type` inside a `type:"attachment"` line.
    AttachmentType(&'static str),
    /// A content block's own `type` value.
    Block(&'static str),
    /// An isMeta `type:"user"` record's TEMPLATE family - the harness writing into the
    /// conversation as if the operator had typed it. Named by csift, because the shape
    /// is a text template rather than a field value.
    IsMetaFamily(&'static str),
}

/// What admits a line carrying this shape to a `search` scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// A flagless scan parses it (the role-bearing lines, and the compaction boundary).
    Default,
    /// An EXPLICIT `-t` reaching the leaf, or a `show` address. A bare scan never parses
    /// it, so the leaf costs nothing on a query that cannot want it.
    Selector,
    /// The above, OR `--attachments` / the `--count-by attachment` axis.
    Attachments,
    /// A `show` ADDRESS only: csift models no leaf here, so no selector can name it and
    /// the address renders it as an unmodeled unit.
    Address,
}

/// Whether a person sees this shape in the terminal, and how directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Human {
    /// Rendered in the ordinary transcript view.
    Always,
    /// Rendered only in a verbose / transcript mode.
    Verbose,
    /// Never rendered as itself; its effect is visible in something else.
    Indirect,
    /// Never reaches a person at all.
    Never,
    /// The map csift's taxonomy is built from states no answer for this shape. An honest
    /// hole, not a guess: the family lane that owns the shape fills it with a measured
    /// value.
    Unstated,
}

/// One shape and its treatment.
///
/// Two columns are the DOCUMENTATION surface rather than a binary one: `llm` and
/// `human` answer "who sees this" for SPEC section 5's inventory table and SKILL's
/// transparency tables, and the drift tests beside this file pin them. The binary reads
/// `shape`, `leaf` and `gate`; the allow says so rather than letting a reader think the
/// columns are unfinished.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Entry {
    /// The axis and the value.
    pub(crate) shape: Shape,
    /// The leaf this shape carries. `None` when the leaf is decided on a DEEPER axis (a
    /// `type:"system"` line's subtype, an attachment's payload type, a block's own type)
    /// or when csift models no leaf for it at all.
    pub(crate) leaf: Option<Class>,
    /// What admits the line to a scan.
    pub(crate) gate: Gate,
    /// Whether the harness CONVERTS this shape into content the model receives. Usually
    /// the same answer as the leaf's own default, and deliberately not always: the
    /// never-convert attachment payloads carry leaves that are otherwise visible, and
    /// `agent.message.structured` is visible as a leaf (the model PRODUCED that text)
    /// while the harness converts the attachment back to nothing. Where the two differ,
    /// the record-level [`Record::delivery_override`] is what a bare selector reads.
    pub(crate) llm: bool,
    /// Whether a person sees it.
    pub(crate) human: Human,
}

/// One row, positionally. A `const fn` so each table stays a `const` slice and a row
/// stays one readable statement; the field ORDER is the column order of the rendered
/// SPEC table (shape, leaf, gate, LLM, human).
pub(crate) const fn e(
    shape: Shape,
    leaf: Option<Class>,
    gate: Gate,
    llm: bool,
    human: Human,
) -> Entry {
    Entry {
        shape,
        leaf,
        gate,
        llm,
        human,
    }
}

/// Both accessors serve the same documentation surface as the `llm`/`human` columns
/// (see [`Entry`]): they render a row, and the drift tests assert what they render.
#[allow(dead_code)]
impl Entry {
    /// The on-disk shape string a reader cross-checks a rendered label against with
    /// `show --raw`. Derived from the shape for the four FIELD axes, so it can never
    /// drift from the value it describes; the template axis carries csift's own name for
    /// a text shape that has no field.
    #[must_use]
    pub(crate) fn disk(self) -> String {
        match self.shape {
            Shape::TopLevelType(t) => format!("type:\"{t}\""),
            Shape::SystemSubtype(s) => format!("type:\"system\" subtype:\"{s}\""),
            Shape::AttachmentType(a) => format!("type:\"attachment\" attachment.type:\"{a}\""),
            Shape::Block(b) => format!("message.content[] type:\"{b}\""),
            Shape::IsMetaFamily(f) => format!("type:\"user\" isMeta:true template:{f}"),
        }
    }

    /// The shape's own value, whatever axis it sits on - the census key.
    #[must_use]
    pub(crate) fn value(self) -> &'static str {
        match self.shape {
            Shape::TopLevelType(v)
            | Shape::SystemSubtype(v)
            | Shape::AttachmentType(v)
            | Shape::Block(v)
            | Shape::IsMetaFamily(v) => v,
        }
    }
}

/// The five tables, in axis order. Grouped rather than flattened so each axis lives in
/// its own file at a readable width; [`rows`] is the flat view every consumer takes.
pub(crate) const INVENTORY: &[&[Entry]] = &[
    lines::ROWS,
    subtypes::ROWS,
    attachments::ROWS,
    blocks::ROWS,
    templates::ROWS,
];

/// Every inventory row, in axis order.
pub(crate) fn rows() -> impl Iterator<Item = &'static Entry> {
    INVENTORY.iter().copied().flatten()
}

/// The leaves that have NO shape row, each with the reason. A leaf lands here when its
/// discriminator is not a line-level field: a SECTION inside a record's text, a fact the
/// surrounding DAG carries, or a tool the record names. The drift test asserts this table
/// and the shaped leaves partition [`Class::ALL`] exactly, so neither can grow silently.
///
/// Read by that test and by SPEC section 5, which prints the reasons beside the table;
/// the binary itself never needs it, hence the allow, for the same reason as [`Entry`]'s
/// documentation columns.
#[allow(dead_code)]
pub(crate) const UNSHAPED: &[(Class, &str)] = &[
    (
        Class::UserAnswer,
        "a tool_result carrier's toolUseResult answers/response, not its line type",
    ),
    (
        Class::UserRejection,
        "a tool_result carrier's typed-instruction tail, not its line type",
    ),
    (
        Class::UserUnsent,
        "assigned at the scan layer from the DAG: an opener with a later sibling and no assistant descendant",
    ),
    (
        Class::UserRewound,
        "assigned at the scan layer from the DAG: the same shape WITH an assistant descendant",
    ),
    (
        Class::AgentThinkingNarration,
        "a thinking block whose base64 signature carries the narration tag",
    ),
    (
        Class::AgentThinkingOmitted,
        "a thinking block whose own text is empty",
    ),
    (
        Class::CommSent,
        "a tool_use block naming SendMessage or a spawn tool",
    ),
    (
        Class::CommSignal,
        "a SendMessage whose input type is a control kind, or a peer control payload",
    ),
    (
        Class::CommChannel,
        "a hook_additional_context payload whose FIRST content string opens with the csift envelope header",
    ),
    (
        Class::NotificationWorkflow,
        "a <task-notification> section whose summary names a workflow",
    ),
    (
        Class::NotificationMonitor,
        "a <task-notification> section whose summary opens Monitor/Scheduled/cron",
    ),
    (
        Class::NotificationSubagent,
        "a <task-notification> section for a spawned agent, or the plain-text agents-stopped notice",
    ),
    (
        Class::NotificationBackgroundCommand,
        "a <task-notification> section for a backgrounded shell command",
    ),
    (
        Class::NotificationTask,
        "any other <task-notification> section",
    ),
    (
        Class::CompactionSummary,
        "a type:\"user\" record carrying the isCompactSummary FLAG, not a line type",
    ),
    (
        Class::CommandInvocation,
        "a user record whose text opens with a slash-command wrapper tag",
    ),
    (
        Class::CommandStdout,
        "a user record whose text opens with <local-command-stdout>",
    ),
    (
        Class::InterruptUser,
        "a user record whose text is exactly the interrupt marker",
    ),
    (
        Class::InterruptTool,
        "a user record whose text is exactly the tool-interrupt marker",
    ),
    (
        Class::ResumePlaceholder,
        "an assistant record with the <synthetic> model and exactly the placeholder text",
    ),
    (
        Class::UserQueued,
        "a queue-operation line whose CONTENT is the human's own text, not its line type",
    ),
];

/// The attachment payload types the harness converts to NOTHING - its own drop arms.
/// Nine of them, and the six whose LEAF is otherwise visible are exactly the rows whose
/// `llm` column disagrees with that leaf default; the other three carry
/// `harness.hook.outcome`, which is invisible anyway. The drift test asserts that set
/// equality in both directions, so a row cannot quietly claim a conversion it has not.
pub(crate) const NEVER_CONVERTS: &[&str] = &[
    "batching_reminder_sent",
    "command_permissions",
    "deferred_tools_record",
    "hook_cancelled",
    "hook_non_blocking_error",
    "hook_system_message",
    "prompt_snapshot",
    "structured_output",
    "thinking_stripped",
];
