//! Line/uuid/turn addressing: specs, caps, explicit-miss errors.

use super::*;

/// Parsed `--line` tokens: the EXPLICIT singletons (each must resolve - a miss is a
/// hard error) and the inclusive ranges (each clamps, but must yield >=1 record).
#[derive(Debug, Default)]
pub(crate) struct LineSpecs {
    pub(crate) explicit: BTreeSet<usize>,
    pub(crate) ranges: Vec<LineRange>,
    /// Distinct lines a range addressed PAST the file's last line, clamped off at resolution.
    /// A line that does not exist is neither a record nor a non-record line, so it is counted
    /// here and never in `non_record_lines`.
    pub(crate) past_end: usize,
    /// The physical line count the specs were resolved against.
    pub(crate) total_lines: usize,
}

/// One `--line` RANGE resolved against the file, beside the token the reader wrote for it.
#[derive(Debug)]
pub(crate) struct LineRange {
    pub(crate) lo: usize,
    pub(crate) hi: usize,
    /// The token as written (`99999..`, `-3..`), which a miss names when the resolved pair
    /// reads backwards.
    pub(crate) written: String,
}

impl LineRange {
    /// True when the range names no line of the file: its resolved pair reads backwards (a
    /// from-end range whose ends reverse on this file's length, or an open range starting
    /// past the end) or it starts past the last line.
    pub(crate) fn is_empty_on(&self, total_lines: usize) -> bool {
        self.lo > self.hi || self.lo > total_lines
    }
}

impl LineSpecs {
    /// The misses both fetch modes report alike, before either reads a line: every range
    /// that names no line of the file ([`LineRange::is_empty_on`]). An empty range is not a
    /// line that failed to parse, so it takes the plain miss and never the rendered mode's
    /// unparseable-line explanation, and `--raw` refuses it rather than printing nothing.
    pub(crate) fn empty_range_misses(&self) -> Vec<String> {
        self.ranges
            .iter()
            .filter(|r| r.is_empty_on(self.total_lines))
            .map(|r| range_miss_label(r, self.total_lines))
            .collect()
    }

    /// Every addressed line, expanded (ranges included).
    pub(crate) fn all(&self) -> BTreeSet<usize> {
        let mut out = self.explicit.clone();
        for r in &self.ranges {
            out.extend(r.lo..=r.hi);
        }
        out
    }
}

/// Parse `--line` tokens (already comma-split by clap) into `(is_explicit, RangeSpec)` pairs
/// via the shared [`crate::text::parse_range_spec`] grammar (`N`, `A..B`, `N..`, `..N`,
/// `-k` from the end). A single token (no `..`) is an EXPLICIT address (miss = hard error); a
/// `..` token is a clamping range. Open/from-end forms resolve against the file's line count in
/// [`resolve_line_specs`]. No subagent prefix - the TARGET names the transcript.
pub(crate) fn parse_line_specs(
    tokens: &[String],
) -> Result<Vec<(bool, crate::text::RangeSpec, String)>> {
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
            t.to_string(),
        ));
    }
    Ok(out)
}

/// How a miss names a `--line` RANGE, always in a spelling the reader can paste back into
/// `--line`: the grammar's own `A..B` for a range that resolved in order, and the range AS
/// WRITTEN for one that resolved backwards - an open range starting past the end resolves to
/// (start, last line), and `L99999..28` is a range the parser refuses as reversed. `A-B` is
/// refused on input too, which is why the dash spelling went. A range that resolved
/// backwards, or starts past the end, also names the file's length, which is what emptied
/// it: `-1..-3` resolves backwards on any file of three lines or more.
pub(crate) fn range_miss_label(r: &LineRange, total_lines: usize) -> String {
    if r.lo <= r.hi {
        miss_label(format!("L{}..{}", r.lo, r.hi), r.lo, total_lines)
    } else {
        format!("L{} (file has {total_lines} lines)", r.written)
    }
}

/// Resolve parsed `--line` specs against the transcript's line count (1-based), materializing
/// open/from-end forms. A single-token spec becomes an explicit address; a range spec clamps.
pub(crate) fn resolve_line_specs(
    parsed: &[(bool, crate::text::RangeSpec, String)],
    total_lines: usize,
) -> LineSpecs {
    let mut specs = LineSpecs {
        total_lines,
        ..LineSpecs::default()
    };
    for (is_explicit, spec, written) in parsed {
        let (lo, hi) = spec.resolve(total_lines, true);
        if *is_explicit {
            specs.explicit.insert(lo); // lo == hi for a single token
        } else if lo <= total_lines && hi > total_lines {
            // A range that STARTS inside the file keeps its in-file part and the rest is
            // counted. Clamped ranges all extend from the last line, so their union past the
            // end is `last+1 ..= max(hi)`, and the count is the widest overhang, not a sum.
            specs.past_end = specs.past_end.max(hi - total_lines);
            specs.ranges.push(LineRange {
                lo,
                hi: total_lines,
                written: written.clone(),
            });
        } else {
            // Wholly inside, or starting past the end: the latter addresses nothing and is
            // left whole, so the miss names the range the reader wrote.
            specs.ranges.push(LineRange {
                lo,
                hi,
                written: written.clone(),
            });
        }
    }
    specs
}

/// The disclosure for lines a range addressed past the end of the file, shared by the rendered
/// footer and `--raw`'s stderr note so the two cannot word it differently.
pub(crate) fn past_end_note(past_end: usize, total_lines: usize) -> String {
    format!(
        "{past_end} addressed line(s) lie past the end of the file (L{total_lines} is its last \
         line); the range was clamped there"
    )
}

/// How a miss names an address past the end of the file: the address plus the file's length,
/// in both fetch modes.
pub(crate) fn miss_label(label: String, first: usize, total_lines: usize) -> String {
    if first > total_lines {
        format!("{label} (file has {total_lines} lines)")
    } else {
        label
    }
}

/// Count a transcript's physical jsonl lines (newlines + a torn final fragment), the 1-based
/// domain the `--line` from-end / open forms resolve against. An empty/missing file => 0.
pub(crate) fn count_lines(file: &std::path::Path) -> Result<usize> {
    let Some(mmap) = mmap_bytes(file)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    Ok(memchr::memchr_iter(b'\n', bytes).count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n")))
}

/// Resolve the TARGET to exactly ONE transcript file. `@<uuid>` -> that top-level
/// transcript (never spans); `@<agent-id>` -> that subagent transcript alone; a
/// `*.jsonl` path -> that file. Anything resolving to !=1 file is a pointed error.
pub(crate) fn resolve_single_transcript(target: &std::path::Path) -> Result<PathBuf> {
    let resolved = path::resolve_session_files_reported(
        std::slice::from_ref(&target.to_path_buf()),
        SubagentScope::TopLevelOnly,
        path::Caller::Other,
    )?;
    // `show` is a FETCH, so it counts the UNMERGED set: the resolver collapses two copies
    // of one session id, but an address that names two files on disk must not pick one
    // silently. A duplicate group therefore keeps the bail alive and names every copy,
    // because `@<uuid>` cannot separate them - only a path can.
    let dropped: Vec<&PathBuf> = resolved
        .duplicates
        .iter()
        .flat_map(|g| g.dropped.iter())
        .collect();
    match (resolved.files.as_slice(), dropped.as_slice()) {
        ([one], []) => Ok(one.clone()),
        (files, dropped) => {
            let every: Vec<String> = files
                .iter()
                .chain(dropped.iter().copied())
                .map(|p| p.display().to_string())
                .collect();
            // The COPIES case is keyed on the duplicate report, not on what was dropped: a
            // DIVERGENT group drops nothing and leaves both copies in `files`, and a reader
            // facing those two needs the paths exactly as much.
            let how = if resolved.duplicates.is_empty() {
                "Name one: `@<uuid>` (a top-level session) | `@<agent-id>` (a subagent, \
                 ids from `csift agents`) | a `*.jsonl` path."
                    .to_string()
            } else {
                // WHAT was duplicated is the group's own word: an agent target reaches this
                // arm too, and calling its copies a session id sends the reader looking for
                // a uuid that is not in play. `subject()` is the one spelling of that, so
                // this message can never drift from the resolver's own note.
                let subjects: Vec<String> = resolved
                    .duplicates
                    .iter()
                    .map(path::DuplicateGroup::subject)
                    .collect();
                format!(
                    "These are copies of ONE {} at different paths on disk, so no \
                     `@<id>` separates them - name a path: {}",
                    subjects.join(" and one "),
                    every.join(" | ")
                )
            };
            bail!(
                "show targets exactly ONE transcript — '{}' resolves to {}. {how}",
                target.display(),
                every.len()
            )
        }
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

/// One page of a fetch: the units `--skip` passes over, then at most `cap` kept.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Page {
    pub(crate) skip: usize,
    pub(crate) cap: usize,
}

impl Page {
    pub(crate) fn new(skip: Option<usize>, max_count: Option<usize>) -> Self {
        Self {
            skip: skip.unwrap_or(0),
            cap: effective_cap(max_count),
        }
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
