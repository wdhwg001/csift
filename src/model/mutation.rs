//! FileOp / FileMutation + the Record file-mutation extractors.

use super::*;

/// A file-mutating operation kind, keyed off the tool that performed it. The
/// structured tools (`Write`/`Edit`/`MultiEdit`/`NotebookEdit`) are AUTHORITATIVE —
/// they name an exact `file_path`/`notebook_path`. `BashMutation` is HEURISTIC: it is
/// parsed lexically from a Bash command string (see [`crate::bash_mutations`]), which
/// cannot be a true shell parse, so it is labelled heuristic everywhere it surfaces.
///
/// Write/Edit/NotebookEdit/MultiEdit are kept DISTINCT (not collapsed to "mutation")
/// because the acid-test question — "how many files did it create vs edit" — needs
/// create-vs-edit discrimination, and the per-op counts are a stated output of
/// `csift files --by-file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    /// `Write` tool — writes a file whole (a create when the path was new).
    Write,
    /// `Edit` tool — a single in-place string replacement in an existing file.
    Edit,
    /// `NotebookEdit` tool — edits a Jupyter notebook cell (`notebook_path`).
    NotebookEdit,
    /// `MultiEdit` tool — multiple edits to one file in a single call.
    MultiEdit,
    /// A file mutation inferred HEURISTICALLY from a Bash command string.
    BashMutation,
}

impl FileOp {
    /// Stable lowercase label used in CLI output + JSON (mirrors `SubagentKind::label`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FileOp::Write => "write",
            FileOp::Edit => "edit",
            FileOp::NotebookEdit => "notebook-edit",
            FileOp::MultiEdit => "multi-edit",
            FileOp::BashMutation => "bash",
        }
    }

    /// The JSON-idiomatic token (UNDERSCORE-delimited) for the `--timeline` `op` field, so
    /// the per-mutation `op` value spells a multi-word op the SAME way the grouped
    /// (`--by-file`/`--by-dir`/`--summary`) per-op COUNT keys do (`notebook_edit`,
    /// `multi_edit`). [`label`] keeps the hyphenated form for human-readable TEXT output;
    /// this method is the on-wire spelling so a script normalizing across the two `files`
    /// JSON modes never special-cases the delimiter. Single-word ops coincide either way.
    #[must_use]
    pub fn json_key(self) -> &'static str {
        match self {
            FileOp::Write => "write",
            FileOp::Edit => "edit",
            FileOp::NotebookEdit => "notebook_edit",
            FileOp::MultiEdit => "multi_edit",
            FileOp::BashMutation => "bash",
        }
    }

    /// True only for [`FileOp::BashMutation`] — drives the explicit "heuristic"
    /// labelling in `files` output (Bash mutations are a best-effort lexical parse,
    /// never authoritative).
    #[must_use]
    pub fn is_heuristic(self) -> bool {
        matches!(self, FileOp::BashMutation)
    }
}

/// One extracted file-mutation fact, pure per-record (the turn index is assigned by
/// the `files` module during turn reconstruction, NOT stored here). The `path` is the
/// absolute path exactly as written in the record — never re-encoded or absolutized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMutation {
    /// The path as written in the record (NOT re-encoded / absolutized).
    pub path: String,
    pub op: FileOp,
    /// Raw ISO8601 UTC timestamp from the tool_use record, if present.
    pub timestamp_utc: Option<String>,
    /// `true` when the paired carrier reported `toolUseResult.type == "create"` (a new
    /// file). On a bare tool_use record the carrier field is usually absent, so this
    /// defaults `false` ("unknown / treat as edit"); the joiner in `files` enriches it.
    pub is_create: bool,
}

impl Record {
    /// The NON-HEURISTIC file mutations carried by this record's structured tool_use
    /// blocks (`Write`/`Edit`/`MultiEdit` → `input.file_path`; `NotebookEdit` →
    /// `input.notebook_path`). One [`FileMutation`] per qualifying block.
    ///
    /// MODELLING NOTE: in real data the `file_path` lives on the **tool_use** record
    /// while `toolUseResult.type` (`create`/`update`) lives on the **paired
    /// tool_result carrier**. This function extracts only what is locally present, so
    /// `is_create` here is consulted from THIS record's own `toolUseResult` first (it
    /// is usually absent on a tool_use record, defaulting `is_create` to `false` —
    /// honestly "unknown / treat as edit"); the `files` module (Section 3) joins the
    /// two sides by `tool_use_id` within a turn via [`Record::carrier_create_paths`]
    /// so `is_create` becomes accurate. Keeping this per-record-pure mirrors how
    /// `search` treats a turn as the join unit.
    ///
    /// Blocks whose path is absent/empty are skipped (a defensive arm, tested).
    #[must_use]
    pub fn structured_tool_mutations(&self) -> Vec<FileMutation> {
        let Some(blocks) = self.blocks() else {
            return Vec::new();
        };
        // This record's own carrier `type` (usually absent on a tool_use record).
        let self_is_create = self
            .tur_probe()
            .as_ref()
            .and_then(|p| p.r#type.as_ref())
            .and_then(serde_json::Value::as_str)
            == Some("create");

        let mut out = Vec::new();
        for block in blocks {
            let Block::ToolUse { name, input, .. } = block else {
                continue;
            };
            let Some(name) = name.as_deref() else {
                continue;
            };
            let (op, key) = match name {
                "Write" => (FileOp::Write, "file_path"),
                "Edit" => (FileOp::Edit, "file_path"),
                "MultiEdit" => (FileOp::MultiEdit, "file_path"),
                "NotebookEdit" => (FileOp::NotebookEdit, "notebook_path"),
                _ => continue,
            };
            let path = input
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if path.is_empty() {
                continue; // defensive: a structured tool_use with no/empty path.
            }
            out.push(FileMutation {
                path: path.to_string(),
                op,
                timestamp_utc: self.timestamp.clone(),
                is_create: self_is_create,
            });
        }
        out
    }

    /// The carrier side of a file mutation: when this record's `toolUseResult` is an
    /// object carrying a `filePath`, return `(tool_use_id, filePath, is_create)` for
    /// each `tool_result` block, so the `files` joiner can set `is_create` on the
    /// matching structured mutation (and fall back to this `filePath` if the
    /// tool_use's own path was somehow absent).
    ///
    /// `toolUseResult.type` ∈ {`create`, `update`, `file_unchanged`, `text`, `image`};
    /// only `create` ⇒ `is_create = true`, everything else ⇒ `false`. When there is no
    /// `toolUseResult`, it is not an object, or it has no `filePath`, the result is
    /// empty (defensive arms, each tested).
    #[must_use]
    pub fn carrier_create_paths(&self) -> Vec<(String, String, bool)> {
        let Some(probe) = self.tur_probe() else {
            return Vec::new();
        };
        let Some(file_path) = probe.file_path.as_ref().and_then(serde_json::Value::as_str) else {
            return Vec::new();
        };
        if file_path.is_empty() {
            return Vec::new();
        }
        let is_create = probe.r#type.as_ref().and_then(serde_json::Value::as_str) == Some("create");

        // The carrier rides on a `tool_result` block whose `tool_use_id` joins it back
        // to the structured tool_use. Emit one tuple per tool_result block id found.
        let mut out = Vec::new();
        if let Some(blocks) = self.blocks() {
            for block in blocks {
                if let Block::ToolResult {
                    tool_use_id: Some(id),
                    ..
                } = block
                {
                    out.push((id.clone(), file_path.to_string(), is_create));
                }
            }
        }
        out
    }

    /// The Bash command string for a `Block::ToolUse { name: "Bash", .. }`
    /// (`input.command`). Returns `None` for any other record / a Bash tool_use with
    /// no command. Feeds the heuristic parser in [`crate::bash_mutations`].
    #[must_use]
    pub fn bash_command(&self) -> Option<&str> {
        let blocks = self.blocks()?;
        for block in blocks {
            if let Block::ToolUse { name, input, .. } = block {
                if name.as_deref() == Some("Bash") {
                    if let Some(cmd) = input
                        .as_ref()
                        .and_then(|v| v.get("command"))
                        .and_then(serde_json::Value::as_str)
                    {
                        return Some(cmd);
                    }
                }
            }
        }
        None
    }
}
