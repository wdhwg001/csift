//! SparseBuffer: line cells, provenance, edit + structured-patch application.

use super::*;

/// One known file line + the jsonl line that last set it.
#[derive(Debug, Clone)]
pub(crate) struct LineCell {
    pub(crate) text: String,
    pub(crate) last_line_no: usize,
}

/// The "in the LLM's eyes" model: a SPARSE map of known file lines. A line absent from
/// `known` is an EXPLICIT gap (unknown — never fabricated).
#[derive(Debug, Default, Clone)]
pub(crate) struct SparseBuffer {
    pub(crate) known: BTreeMap<usize, LineCell>,
    /// The file length the model last observed (bounds the trailing gap).
    pub(crate) seen_total_lines: Option<usize>,
    /// Whether the last full-content anchor ended in a file-final newline. The Read tool
    /// reports `totalLines` by SEPARATOR count (so a newline-terminated file gets a phantom
    /// empty last line: 12 content lines → totalLines 13), while `split_lines` uses
    /// TERMINATOR count (12). When this is set, a windowed read's `total_lines` is
    /// normalised down by that phantom so the two conventions agree and no spurious trailing
    /// `??? line N+1 unknown` gap is reported for a fully-recovered file.
    pub(crate) content_ends_with_newline: bool,
}

impl SparseBuffer {
    /// Reset to a full snapshot's lines (1..=N). Supersedes all prior state.
    pub(crate) fn reset_to_full(&mut self, content: &str, total_lines: usize, line_no: usize) {
        self.known.clear();
        for (i, text) in split_lines(content).into_iter().enumerate() {
            self.known.insert(
                i + 1,
                LineCell {
                    text,
                    last_line_no: line_no,
                },
            );
        }
        // A full-content anchor is the authority on trailing-newline status (used to
        // normalise later windowed reads' separator-counted totals).
        self.content_ends_with_newline = content.ends_with('\n');
        // CC's Read / file-attachment `totalLines` is a SEPARATOR count: a newline-terminated
        // file reports `split_lines + 1` (a phantom empty last line — e.g. a 96-line file ending
        // in `\n` reports 97). We hold the FULL content here, so `split_lines` (== `known.len()`)
        // is authoritative — normalise the reported total down by that phantom before recording
        // it, else a fully-recovered newline-terminated file is mis-reported as missing its
        // trailing line (restore HARD-FAILS as "partial"; --salvage/--at show a spurious
        // `??? line N+1 unknown`; --coverage shows N/N+1). A Write already passes a terminator
        // count, and the `.max(known.len())` floor keeps every non-phantom case unchanged.
        let normalized_total = self.normalize_total(total_lines);
        self.seen_total_lines = Some(normalized_total.max(self.known.len()));
    }

    /// Convert a tool-reported `total_lines` (SEPARATOR count) to the TERMINATOR count used
    /// by `split_lines`, dropping the phantom empty last line a file-final newline adds.
    /// A no-op until a full-content anchor has confirmed the trailing newline.
    pub(crate) fn normalize_total(&self, total_lines: usize) -> usize {
        if self.content_ends_with_newline {
            total_lines.saturating_sub(1)
        } else {
            total_lines
        }
    }

    /// Splice a windowed read: set `known[start+i]` for each line, leave the rest as-is.
    /// Gaps are NOT padded (padding would fabricate unseen lines).
    pub(crate) fn splice(
        &mut self,
        start_line: usize,
        lines: &[String],
        total_lines: usize,
        line_no: usize,
    ) {
        for (i, text) in lines.iter().enumerate() {
            self.known.insert(
                start_line + i,
                LineCell {
                    text: text.clone(),
                    last_line_no: line_no,
                },
            );
        }
        let norm_total = self.normalize_total(total_lines);
        self.seen_total_lines = Some(norm_total.max(self.seen_total_lines.unwrap_or(0)));
    }

    /// Contiguous runs of currently-known lines, as inclusive `(start, end)` spans.
    pub(crate) fn covered_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for &k in self.known.keys() {
            match ranges.last_mut() {
                Some(last) if last.1 + 1 == k => last.1 = k,
                _ => ranges.push((k, k)),
            }
        }
        ranges
    }

    /// The known lines as a dense `(line_no, text)` vector (gaps omitted), ascending.
    pub(crate) fn known_lines(&self) -> Vec<(usize, String)> {
        self.known
            .iter()
            .map(|(k, c)| (*k, c.text.clone()))
            .collect()
    }

    /// The known lines with provenance: `(file_line, text, jsonl_line_that_set_it)`.
    /// Surfaces `LineCell::last_line_no` so a consumer can `Read` the exact jsonl line a
    /// reconstructed line came from.
    pub(crate) fn known_lines_with_provenance(&self) -> Vec<(usize, String, usize)> {
        self.known
            .iter()
            .map(|(k, c)| (*k, c.text.clone(), c.last_line_no))
            .collect()
    }
}

/// The result of applying an [`EditKind::Edit`] to the buffer: did it anchor cleanly?
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EditOutcome {
    Applied,
    /// The edit could not be anchored (old_string over an unknown gap / not found).
    UnAnchorable,
}

/// Apply an Edit to the buffer. Preferred: structured-patch by exact line position;
/// fallback: string replacement over the contiguous known text. Returns whether it
/// anchored (an un-anchorable edit is a coverage hole, never a fabrication).
pub(crate) fn apply_edit(
    buf: &mut SparseBuffer,
    hunks: &[EditHunk],
    structured_patch: &Option<Vec<PatchHunk>>,
    line_no: usize,
) -> EditOutcome {
    if let Some(patches) = structured_patch {
        if !patches.is_empty() {
            return apply_structured_patch(buf, patches, line_no);
        }
    }
    // Fallback: string replacement over the dense known text.
    apply_string_edit(buf, hunks, line_no)
}

/// Apply structured-patch hunks by exact line position, shifting subsequent keys.
///
/// A patch hunk replaces `oldLines` source lines starting at `oldStart` with `newLines`
/// lines starting at `newStart`. We process hunks high-to-low so earlier hunks' indices
/// stay valid, rebuilding the dense line vector each time (the file is small enough — a
/// single tool result — that an O(n) rebuild per hunk is fine and keeps the logic exact).
pub(crate) fn apply_structured_patch(
    buf: &mut SparseBuffer,
    patches: &[PatchHunk],
    line_no: usize,
) -> EditOutcome {
    // Work on a dense snapshot ONLY if the affected region is fully known; otherwise the
    // edit is un-anchorable (we will not fabricate the missing context).
    // Build the current dense vector over the min..max line span the patches touch.
    let mut applied_any = false;
    // Materialize the whole known buffer as a dense vector indexed from line 1, padding
    // unknown interior lines with a sentinel we refuse to emit (tracked separately).
    let max_line = buf.known.keys().copied().max().unwrap_or(0);
    let patch_max = patches
        .iter()
        .map(|h| h.old_start + h.old_lines)
        .max()
        .unwrap_or(0);
    let span = max_line.max(patch_max);
    // dense[i] = Some(text) if known, None if a gap.
    let mut dense: Vec<Option<String>> = vec![None; span + 1]; // 1-based; index 0 unused
    for (k, c) in &buf.known {
        if *k <= span {
            dense[*k] = Some(c.text.clone());
        }
    }

    // Apply hunks low-to-high but accumulate a running offset (newLines-oldLines) so each
    // subsequent hunk's oldStart maps onto the already-shifted dense vector.
    let mut offset: isize = 0;
    for h in patches {
        // The OLD region content is the patch's context (` `) + removed (`-`) lines, in
        // order. The NEW region content is the context (` `) + added (`+`) lines.
        let old_region: Vec<String> = h
            .lines
            .iter()
            .filter(|l| l.starts_with('-') || l.starts_with(' '))
            .map(|l| l[1.min(l.len())..].to_string())
            .collect();
        let added: Vec<String> = h
            .lines
            .iter()
            .filter(|l| l.starts_with('+') || l.starts_with(' '))
            .map(|l| l[1.min(l.len())..].to_string())
            .collect();

        let start = (h.old_start as isize + offset).max(1) as usize;
        let end = start + h.old_lines; // exclusive
                                       // Defensive grow: `dense` is pre-sized to `span + 1` (≥ every hunk's
                                       // `old_start + old_lines`) and each splice grows it by the running offset, so with
                                       // well-formed ascending hunks `end` never exceeds `dense.len()`. We keep the guard
                                       // anyway because the hunk stream is untrusted transcript data — a pathological
                                       // (e.g. non-ascending) `structuredPatch` must not index out of bounds below.
        if end > dense.len() {
            dense.resize(end, None);
        }
        // ANCHOR CHECK (anti-fabrication): an edit's absolute `oldStart` is only
        // trustworthy if it lands ON or ADJACENT TO currently-known content. A hunk
        // whose entire neighbourhood is an unknown gap is position-drifted — applying it
        // would fabricate island lines at a wrong absolute number (the heavily-edited
        // file built without a clean full anchor is the real-data failure mode). Refuse
        // it as un-anchorable rather than asserting a wrong "known" line.
        let neighbourhood_known = (start.saturating_sub(1)..=end)
            .any(|i| dense.get(i).map(Option::is_some).unwrap_or(false));
        if !neighbourhood_known {
            return EditOutcome::UnAnchorable;
        }
        // Verify the removed region is fully known (anchorable). If any line in the old
        // range is an unknown gap, we cannot safely re-anchor → un-anchorable.
        let region_known = (start..end).all(|i| dense.get(i).map(Option::is_some).unwrap_or(false));
        if h.old_lines > 0 && !region_known {
            return EditOutcome::UnAnchorable;
        }
        // CONTEXT VERIFICATION (anti-fabrication): the patch's old-region lines must
        // match what the buffer currently holds at the anchored position. If they
        // DISAGREE, the edit is mis-anchored (the buffer drifted out of sync with the
        // real file — e.g. an earlier un-anchorable edit), so applying it would corrupt
        // known lines. Refuse: report un-anchorable rather than assert a wrong line.
        if h.old_lines > 0 && old_region.len() == h.old_lines {
            let matches = (0..h.old_lines).all(|k| {
                dense
                    .get(start + k)
                    .and_then(|c| c.as_ref())
                    .map(|t| t == &old_region[k])
                    .unwrap_or(false)
            });
            if !matches {
                return EditOutcome::UnAnchorable;
            }
        }
        // Splice: replace dense[start..end] with the new region content.
        let tail: Vec<Option<String>> = dense.split_off(end.min(dense.len()));
        dense.truncate(start.min(dense.len()));
        for a in &added {
            dense.push(Some(a.clone()));
        }
        dense.extend(tail);
        offset += h.new_lines as isize - h.old_lines as isize;
        applied_any = true;
    }

    // Rebuild the sparse buffer from the dense vector (1-based; skip the unused index 0
    // and any remaining gaps).
    buf.known.clear();
    for (i, cell) in dense.iter().enumerate().skip(1) {
        if let Some(text) = cell {
            buf.known.insert(
                i,
                LineCell {
                    text: text.clone(),
                    last_line_no: line_no,
                },
            );
        }
    }
    let max_known = buf.known.keys().copied().max().unwrap_or(0);
    // A structured patch is AUTHORITATIVE about the file's new length: adjust the
    // previously-seen total by this edit's net line delta (`offset` = added − removed
    // accumulated over the hunks) rather than monotonically maxing it. Maxing left a
    // phantom trailing gap after a net deletion (e.g. an insert that grew the file to N+1
    // followed by a delete back to N would still report N+1, emitting a spurious
    // `??? line N+1 unknown`). Clamp to ≥ the max known line and ≥ 0.
    let prev_total = buf.seen_total_lines.unwrap_or(0) as isize;
    let adjusted = (prev_total + offset).max(max_known as isize).max(0) as usize;
    buf.seen_total_lines = Some(adjusted);

    if applied_any {
        EditOutcome::Applied
    } else {
        EditOutcome::UnAnchorable
    }
}

/// Fallback string-replacement edit over the dense contiguous known text. If the buffer
/// is not a single contiguous run (or `old_string` is not found), the edit is
/// un-anchorable (we never guess across a gap).
pub(crate) fn apply_string_edit(
    buf: &mut SparseBuffer,
    hunks: &[EditHunk],
    line_no: usize,
) -> EditOutcome {
    // Only safe when the known lines are one contiguous run starting at line 1.
    let ranges = buf.covered_ranges();
    let contiguous_from_one = matches!(ranges.first(), Some(&(1, _))) && ranges.len() == 1;
    if !contiguous_from_one {
        return EditOutcome::UnAnchorable;
    }
    let mut text = buf
        .known
        .values()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let mut any = false;
    for h in hunks {
        if h.old_string.is_empty() || !text.contains(&h.old_string) {
            return EditOutcome::UnAnchorable;
        }
        if h.replace_all {
            text = text.replace(&h.old_string, &h.new_string);
        } else {
            text = text.replacen(&h.old_string, &h.new_string, 1);
        }
        any = true;
    }
    if !any {
        return EditOutcome::UnAnchorable;
    }

    buf.known.clear();
    for (i, line) in text.split('\n').enumerate() {
        buf.known.insert(
            i + 1,
            LineCell {
                text: line.to_string(),
                last_line_no: line_no,
            },
        );
    }
    let total = buf.known.len();
    buf.seen_total_lines = Some(total.max(buf.seen_total_lines.unwrap_or(0)));
    EditOutcome::Applied
}

// ─────────────────────────────────────────────────────────────────────────────
// Boundaries + segmentation
// ─────────────────────────────────────────────────────────────────────────────
