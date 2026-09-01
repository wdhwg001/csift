//! Outcome/OpCounts accounting and summary bucketing.

use super::*;

/// The merged + filtered result, ready to render.
pub(crate) struct Outcome {
    pub(crate) detail: FilesDetail,
    pub(crate) mutations: Vec<TaggedMutation>,
    /// Edit-before-Read boundaries (file changed outside the tool stream), sorted by jsonl line.
    pub(crate) boundaries: Vec<TaggedBoundary>,
    pub(crate) skipped_lines: usize,
    /// The raw `--turn` token, kept verbatim for the footer (the range resolves
    /// per-file, so there is no single global `(lo, hi)` to display).
    pub(crate) turn_range: Option<String>,
    pub(crate) time_window_bounded: bool,
    /// SCOPE-span counts of the RESOLVED transcript set (top-level + subagent files),
    /// computed from `resolve_session_files` BEFORE the mutation scan - so a subagent
    /// transcript with zero mutations still counts toward the announced fan-out. Drives the
    /// shared SCOPE banner / JSON `session_header`, suppressed when `scope_sub == 0`.
    pub(crate) scope_top: usize,
    pub(crate) scope_sub: usize,
}

impl Outcome {
    /// Distinct file paths touched (across all mutations).
    pub(crate) fn distinct_files(&self) -> usize {
        let mut set = std::collections::BTreeSet::new();
        for m in &self.mutations {
            set.insert(m.mutation.path.as_str());
        }
        set.len()
    }
}

// ── Aggregation ──

/// Per-op counts for one bucket/dir/file, plus first/last touch + distinct files.
#[derive(Debug, Clone, Default)]
pub(crate) struct OpCounts {
    pub(crate) write: usize,
    pub(crate) edit: usize,
    pub(crate) notebook_edit: usize,
    pub(crate) multi_edit: usize,
    pub(crate) bash: usize,
    /// Snapshot-inferred external writes (settings family; no tool record).
    pub(crate) external_write: usize,
    /// Distinct file paths contributing to this group (for dir/bucket rows).
    pub(crate) files: std::collections::BTreeSet<String>,
    pub(crate) first_ts: Option<String>,
    pub(crate) last_ts: Option<String>,
}

impl OpCounts {
    pub(crate) fn add(&mut self, m: &FileMutation) {
        match m.op {
            FileOp::Write => self.write += 1,
            FileOp::Edit => self.edit += 1,
            FileOp::NotebookEdit => self.notebook_edit += 1,
            FileOp::MultiEdit => self.multi_edit += 1,
            FileOp::BashMutation => self.bash += 1,
            FileOp::ExternalWrite => self.external_write += 1,
        }
        self.files.insert(m.path.clone());
        if let Some(ts) = &m.timestamp_utc {
            // Min/max as raw ISO8601 strings (ISO8601 sorts chronologically as text).
            if self.first_ts.as_deref().is_none_or(|f| ts.as_str() < f) {
                self.first_ts = Some(ts.clone());
            }
            if self.last_ts.as_deref().is_none_or(|l| ts.as_str() > l) {
                self.last_ts = Some(ts.clone());
            }
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.write
            + self.edit
            + self.notebook_edit
            + self.multi_edit
            + self.bash
            + self.external_write
    }

    /// The op-count fragment as `"N write, N edit, …"`, omitting zero counts; Bash is
    /// suffixed `(heuristic)`. Empty groups render `"0"`.
    pub(crate) fn ops_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.write > 0 {
            parts.push(format!("{} write", self.write));
        }
        if self.edit > 0 {
            parts.push(format!("{} edit", self.edit));
        }
        if self.notebook_edit > 0 {
            parts.push(format!("{} notebook-edit", self.notebook_edit));
        }
        if self.multi_edit > 0 {
            parts.push(format!("{} multi-edit", self.multi_edit));
        }
        if self.bash > 0 {
            parts.push(format!("{} bash (heuristic)", self.bash));
        }
        if self.external_write > 0 {
            parts.push(format!("{} external write (inferred)", self.external_write));
        }
        if parts.is_empty() {
            "0".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// How many leading path SEGMENTS the `--summary` rollup keeps. Depth 4 keeps an absolute
/// path's project-root level distinct (`/Users/testuser/Projects/widget_app_prototype`)
/// while COLLAPSING everything deeper into that one bucket - so `--summary` is a genuine
/// coarse rollup, strictly smaller than `--by-dir` (which keys on the full parent dir). A
/// shallower path (e.g. `/tmp/x`) keeps all the segments it has.
pub(crate) const SUMMARY_BUCKET_SEGMENTS: usize = 4;

/// The `--summary` rollup BUCKET key for a path: a COARSE top-level prefix (the first
/// [`SUMMARY_BUCKET_SEGMENTS`] path segments), NOT the full parent dir. This is what makes
/// `--summary` the smallest output and a real rollup - distinct from `--by-dir`, which keys
/// on the full parent. Examples (depth 4): `/Users/testuser/Projects/p/spec/gaps.md` and
/// `/Users/testuser/Projects/p/src/main.rs` BOTH bucket to `/Users/testuser/Projects/p`;
/// `/tmp/x.md` → `/tmp`. A `git:<sub>` pseudo-path keeps its own `git:` bucket (it is not a
/// real file path). A bare relative filename (no `/`) buckets under `./`.
pub(crate) fn bucket_key(path: &str) -> String {
    // The intentional `git:<sub>` coarse pseudo-path is its own bucket, never split as a dir
    // (all `git:add`/`git:commit`/… roll up under one `git:` row, out of the `./` sink).
    if path.starts_with("git:") {
        return "git:".to_string();
    }
    // Roll up the PARENT directory (never the basename) to at most SUMMARY_BUCKET_SEGMENTS
    // segments. A bare relative filename has no parent → the `./` bucket.
    let Some(parent) = parent_dir(path) else {
        return "./".to_string();
    };
    let absolute = parent.starts_with('/');
    let segs: Vec<&str> = parent.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        // Parent is `/` (a top-level file like `/foo`) → the root bucket.
        return "/".to_string();
    }
    let take = segs.len().min(SUMMARY_BUCKET_SEGMENTS);
    let prefix = segs[..take].join("/");
    if absolute {
        format!("/{prefix}")
    } else {
        prefix
    }
}

/// The parent directory of a path string (lexical only - never touches the
/// filesystem). Returns `None` for a bare filename with no `/`.
pub(crate) fn parent_dir(path: &str) -> Option<String> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_string()), // a top-level path like `/foo`
        Some(idx) => Some(trimmed[..idx].to_string()),
        None => None,
    }
}

/// Group mutations by a key function into a deterministic (BTreeMap-sorted) map.
pub(crate) fn group_by<F: Fn(&FileMutation) -> String>(
    mutations: &[TaggedMutation],
    key: F,
) -> BTreeMap<String, OpCounts> {
    let mut map: BTreeMap<String, OpCounts> = BTreeMap::new();
    for m in mutations {
        map.entry(key(&m.mutation)).or_default().add(&m.mutation);
    }
    map
}

// ── Rendering ──
