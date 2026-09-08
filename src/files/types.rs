//! Tagged mutation/boundary rows + the path filter.

use super::*;

/// One extracted mutation tagged with the turn it belongs to + its owning session.
#[derive(Debug, Clone)]
pub(crate) struct TaggedMutation {
    /// The transcript's own id: a top-level session uuid, OR a bare SUBAGENT hex when the
    /// mutation came from a subagent transcript. A subagent hex is NOT a re-feedable `@<uuid>`
    /// target; use `parent_session_id` to re-feed. `is_subagent` discriminates the id-domain.
    pub(crate) session_id: String,
    /// True when this mutation came from a subagent transcript (so `session_id` is a bare
    /// hex, not a re-feedable uuid). Defaults false; set per-file in `scan_one_file`.
    pub(crate) is_subagent: bool,
    /// The re-feedable PARENT session uuid (the owning top-level session). Equals
    /// `session_id` for a top-level mutation. Defaults to `session_id`; set in `scan_one_file`.
    pub(crate) parent_session_id: String,
    /// The live turn this mutation physically sits in. For a mutation on a record the
    /// surviving conversation no longer reaches it is the nearest PRECEDING live turn - an
    /// ordering key for the `--turn` window only, never printed (see `stamp`).
    pub(crate) turn_index: usize,
    /// Where the mutating record sits in live turn numbering: `Live(n)`, or `Abandoned`
    /// naming the branch head's line. Disk truth is FILE ORDER, so an abandoned mutation is
    /// kept and marked, never dropped - the write happened.
    pub(crate) stamp: crate::model::TurnStamp,
    /// The wire spelling of the record's survival (`live` / `pre-cut` / `abandoned`).
    pub(crate) survival: &'static str,
    /// The JSONL physical line number of the mutating record (1-based), so a `files` row joins
    /// back to the raw transcript exactly like `recover`/`search`/`turns` do.
    pub(crate) line_no: usize,
    pub(crate) mutation: FileMutation,
}

/// An Edit-before-Read boundary `files` detected: the file changed OUTSIDE the Read/Write/Edit
/// stream (a formatter, husky/pre-commit, git, an external editor) and the harness rejected an
/// Edit/Write with `File has been modified since read`, forcing a fresh Read. Attributed to its
/// file via the failed op's `tool_use_id` ↔ that op's `file_path` join, carrying the jsonl line.
#[derive(Debug, Clone)]
pub(crate) struct TaggedBoundary {
    pub(crate) session_id: String,
    pub(crate) is_subagent: bool,
    pub(crate) parent_session_id: String,
    pub(crate) path: String,
    pub(crate) line_no: usize,
    /// The ordering turn (see [`TaggedMutation::turn_index`]).
    pub(crate) turn_index: usize,
    pub(crate) stamp: crate::model::TurnStamp,
    pub(crate) survival: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) timestamp_utc: Option<String>,
}

/// Per-file scan result before global aggregation.
pub(crate) struct FileResult {
    pub(crate) mutations: Vec<TaggedMutation>,
    pub(crate) boundaries: Vec<TaggedBoundary>,
    pub(crate) skipped_lines: usize,
    /// This transcript's genuine-user turn count - so a `--turn` spec resolves its
    /// open/from-end forms (`N..`, `-3..`) against THIS file's turns, not a global count.
    pub(crate) turn_count: usize,
}

/// The compiled `--regex` / `--glob` path predicates. Both are OPTIONAL and ANDed: a path is
/// kept iff it satisfies EVERY supplied filter, tested against the FULL absolute path string.
/// Applied to mutations AND Edit-before-Read boundaries BEFORE the `--by` rollup, so all views
/// reflect the filtered set. With neither supplied, [`Self::keeps`] keeps everything.
pub(crate) struct PathFilter {
    /// `--regex <RE>`: keep iff the pattern matches ANYWHERE in the full path (used as-is).
    pub(crate) regex: Option<Regex>,
    /// `--glob <PAT>`: keep iff the glob matches the full path (`**` crosses `/`).
    pub(crate) glob: Option<GlobMatcher>,
}

impl PathFilter {
    /// Compile the optional `--regex` / `--glob` patterns. An invalid pattern is a HARD error
    /// (named in the message), surfaced before any scan so the failure is fast.
    pub(crate) fn from_args(regex: Option<&str>, glob: Option<&str>) -> Result<Self> {
        let regex = regex
            .map(|re| Regex::new(re).with_context(|| format!("invalid --regex pattern: {re}")))
            .transpose()?;
        let glob = glob
            .map(|pat| {
                Glob::new(pat)
                    .map(|g| g.compile_matcher())
                    .with_context(|| format!("invalid --glob pattern: {pat}"))
            })
            .transpose()?;
        Ok(Self { regex, glob })
    }

    /// Whether `path` survives every supplied filter (vacuously true when none was supplied).
    pub(crate) fn keeps(&self, path: &str) -> bool {
        if let Some(re) = &self.regex {
            if !re.is_match(path) {
                return false;
            }
        }
        if let Some(g) = &self.glob {
            if !g.is_match(path) {
                return false;
            }
        }
        true
    }
}
