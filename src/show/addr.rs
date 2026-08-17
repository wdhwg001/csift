//! Line/uuid/turn addressing: specs, caps, explicit-miss errors.

use super::*;

/// Parsed `--line` tokens: the EXPLICIT singletons (each must resolve - a miss is a
/// hard error) and the inclusive ranges (each clamps, but must yield ≥1 record).
#[derive(Debug, Default)]
pub(crate) struct LineSpecs {
    pub(crate) explicit: BTreeSet<usize>,
    pub(crate) ranges: Vec<(usize, usize)>,
}

impl LineSpecs {
    /// Every addressed line, expanded (ranges included).
    pub(crate) fn all(&self) -> BTreeSet<usize> {
        let mut out = self.explicit.clone();
        for &(a, b) in &self.ranges {
            out.extend(a..=b);
        }
        out
    }
}

/// Parse `--line` tokens (already comma-split by clap) into `(is_explicit, RangeSpec)` pairs
/// via the shared [`crate::text::parse_range_spec`] grammar (`N` · `A..B` · `N..` · `..N` ·
/// `-k` from the end). A single token (no `..`) is an EXPLICIT address (miss = hard error); a
/// `..` token is a clamping range. Open/from-end forms resolve against the file's line count in
/// [`resolve_line_specs`]. No subagent prefix - the TARGET names the transcript.
pub(crate) fn parse_line_specs(tokens: &[String]) -> Result<Vec<(bool, crate::text::RangeSpec)>> {
    let mut out = Vec::new();
    for tok in tokens {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let is_explicit = !t.contains("..");
        out.push((
            is_explicit,
            crate::text::parse_range_spec(t, "--line", true)?,
        ));
    }
    Ok(out)
}

/// Resolve parsed `--line` specs against the transcript's line count (1-based), materializing
/// open/from-end forms. A single-token spec becomes an explicit address; a range spec clamps.
pub(crate) fn resolve_line_specs(
    parsed: &[(bool, crate::text::RangeSpec)],
    total_lines: usize,
) -> LineSpecs {
    let mut specs = LineSpecs::default();
    for (is_explicit, spec) in parsed {
        let (lo, hi) = spec.resolve(total_lines, true);
        if *is_explicit {
            specs.explicit.insert(lo); // lo == hi for a single token
        } else {
            specs.ranges.push((lo, hi));
        }
    }
    specs
}

/// Count a transcript's physical jsonl lines (newlines + a torn final fragment), the 1-based
/// domain the `--line` from-end / open forms resolve against. An empty/missing file ⇒ 0.
pub(crate) fn count_lines(file: &std::path::Path) -> Result<usize> {
    let Some(mmap) = mmap_bytes(file)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    Ok(memchr::memchr_iter(b'\n', bytes).count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n")))
}

/// Resolve the TARGET to exactly ONE transcript file. `@<uuid>` → that top-level
/// transcript (never spans); `@<agent-id>` → that subagent transcript alone; a
/// `*.jsonl` path → that file. Anything resolving to ≠1 file is a pointed error.
pub(crate) fn resolve_single_transcript(target: &std::path::Path) -> Result<PathBuf> {
    let files = path::resolve_session_files(
        std::slice::from_ref(&target.to_path_buf()),
        SubagentScope::TopLevelOnly,
        path::Caller::Other,
    )?;
    match files.as_slice() {
        [one] => Ok(one.clone()),
        many => bail!(
            "show targets exactly ONE transcript — '{}' resolves to {}. Name one: \
             `@<uuid>` (a top-level session) | `@<agent-id>` (a subagent, ids from \
             `csift agents`) | a `*.jsonl` path.",
            target.display(),
            many.len()
        ),
    }
}

/// The default cap on emitted record units - `show`'s context-flood guard (law 4: any
/// cap reports its drop, here with the exact continuation command). `--max-count N`
/// overrides; `0` = uncapped (the crate-wide `--max-count 0` convention).
pub(crate) const DEFAULT_SHOW_CAP: usize = 200;

/// The effective unit cap: explicit N wins, `0` lifts the cap entirely, absent = default.
pub(crate) fn effective_cap(max_count: Option<usize>) -> usize {
    match max_count {
        Some(0) => usize::MAX,
        Some(n) => n,
        None => DEFAULT_SHOW_CAP,
    }
}

/// True when a `--turn` spec is fully EXPLICIT (`N` / `A..B` - both endpoints written as
/// absolute indices): an ADDRESS by law 1, so resolving to zero records is a miss (hard
/// error), never an honest-empty. Open / from-the-end forms (`N..`, `..N`, `-k`, `..`)
/// clamp - the tail-peek (`--turn -3..`) must stay robust on short sessions.
pub(crate) fn turn_spec_is_explicit(spec: &crate::text::RangeSpec) -> bool {
    matches!(spec.start, crate::text::Endpoint::At(_))
        && matches!(spec.end, crate::text::Endpoint::At(_))
}

/// The turn address-miss error - names the missed turn(s) AND the transcript's actual
/// turn domain, mirroring the `--line` miss's self-teaching shape.
pub(crate) fn turn_miss_error(spec: &crate::text::RangeSpec, turn_count: usize) -> anyhow::Error {
    use crate::text::Endpoint;
    let shown = match (spec.start, spec.end) {
        (Endpoint::At(a), Endpoint::At(b)) if a == b => format!("t{a}"),
        (Endpoint::At(a), Endpoint::At(b)) => format!("t{a}..t{b}"),
        _ => "the requested turn range".to_string(),
    };
    let domain = if turn_count == 0 {
        "the transcript has 0 turns".to_string()
    } else {
        format!(
            "the transcript has {turn_count} turn(s) (t0..t{})",
            turn_count - 1
        )
    };
    anyhow::anyhow!(
        "no such turn(s): {shown} — {domain}; turn indices are 0-based (the `tN` search \
         prints), and the last k turns are `--turn -k..`"
    )
}
