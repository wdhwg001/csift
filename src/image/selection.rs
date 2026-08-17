//! #N / locator selection, dedup, ambiguity errors.

use super::*;

/// One `--id` selector: either the session's `#N` image number, or an exact `L<line>i<n>`
/// locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sel {
    /// `#N` / bare `N` — the `[Image #N]` handle the model uses. Resolves to the unique image
    /// with that handle in scope; if it names >1 DISTINCT image (CC reuses `#N` across prompts)
    /// it is AMBIGUOUS and ERRORS with the occurrence list rather than silently picking one.
    Seq(usize),
    /// `L<line>i<n>` — the exact per-occurrence locator.
    Loc(usize, usize),
}

/// Parse the `--id` selection (repeatable + comma-delimited). Accepts `#32` / `32` (a `#N`
/// handle) and `L6812i2` / `6812i2` (a locator). A malformed token is a hard error.
pub(crate) fn parse_id_selection(ids: &[String]) -> Result<Vec<Sel>> {
    let mut out = Vec::new();
    for raw in ids {
        for tok in raw.split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            if let Some(n) = tok.strip_prefix('#') {
                // A '#'-form would need shell quoting (`--id #32` unquoted becomes a shell
                // comment and silently drops the argument) — bare digits are the ONE input
                // form; the DISPLAY stays `#N` (what the model sees in `[Image #N]`).
                bail!(
                    "--id: drop the '#' — pass the bare number (`--id {n}`); the listing \
                     displays `#{n}` but input takes digits (or the `L<line>i<n>` locator)"
                );
            } else if let Some((line, idx)) = tok
                .strip_prefix(['L', 'l'])
                .unwrap_or(tok)
                .split_once('i')
                .filter(|(l, i)| {
                    !l.is_empty()
                        && l.bytes().all(|b| b.is_ascii_digit())
                        && !i.is_empty()
                        && i.bytes().all(|b| b.is_ascii_digit())
                })
            {
                out.push(Sel::Loc(line.parse()?, idx.parse()?));
            } else if !tok.is_empty() && tok.bytes().all(|b| b.is_ascii_digit()) {
                out.push(Sel::Seq(tok.parse()?));
            } else {
                bail!("--id `{tok}` is not `#N` or `L<line>i<n>` (e.g. `#32` or `L6812i2`)");
            }
        }
    }
    Ok(out)
}

/// Dedup the SAME image re-injected across context windows (by content fingerprint), keeping
/// the LATEST occurrence (its current `#N`). `images` is sorted ascending, so walking in
/// reverse and keeping first-seen yields the latest; then order by `#N` (then line) for the
/// listing so a reader scanning for "#32" finds it in sequence.
pub(crate) fn dedup_latest(images: &[ImageRef]) -> Vec<&ImageRef> {
    let mut seen = std::collections::HashSet::new();
    let mut v: Vec<&ImageRef> = Vec::new();
    for i in images.iter().rev() {
        if seen.insert(i.fingerprint.as_str()) {
            v.push(i);
        }
    }
    v.sort_by(|a, b| {
        (a.seq.unwrap_or(usize::MAX), a.line_no, a.img_index).cmp(&(
            b.seq.unwrap_or(usize::MAX),
            b.line_no,
            b.img_index,
        ))
    });
    v
}

/// Number of distinct transcripts (session ids) the image set spans — the per-transcript guard
/// for `--id` / `--turn` (line numbers + `#N` + turn indices are all per-transcript).
pub(crate) fn count_distinct_transcripts(images: &[ImageRef]) -> usize {
    let mut ids: Vec<&str> = images.iter().map(|i| i.session_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

/// The transcript file backing a single-transcript image set (by matching session id) — used to
/// re-parse it for turn indices / excerpts on the `--turn` and ambiguity paths.
pub(crate) fn pinned_path<'a>(
    session_files: &'a [PathBuf],
    images: &[ImageRef],
) -> Option<&'a Path> {
    let sid = images.first().map(|i| i.session_id.as_str())?;
    session_files
        .iter()
        .map(PathBuf::as_path)
        .find(|p| crate::subagent::session_id_from_path(p) == sid)
}

/// The DISTINCT images among `cands` (same `#N`): dedup by content fingerprint keeping the
/// latest occurrence, ordered by line. Length 1 ⇒ `#N` is unambiguous; >1 ⇒ genuine reuse.
pub(crate) fn distinct_by_fingerprint<'a>(cands: &[&'a ImageRef]) -> Vec<&'a ImageRef> {
    let mut seen = std::collections::HashSet::new();
    let mut v: Vec<&ImageRef> = Vec::new();
    // `cands` are ascending by line; walk reverse + keep first-seen ⇒ latest per content.
    for i in cands.iter().rev() {
        if seen.insert(i.fingerprint.as_str()) {
            v.push(i);
        }
    }
    v.sort_by(|a, b| (a.line_no, a.img_index).cmp(&(b.line_no, b.img_index)));
    v
}

/// Resolve the `--id` selectors against the (scope-filtered, single-transcript) image set. A
/// `#N` resolves only when it names ONE distinct image; if it names several it ERRORS with the
/// occurrence list (turn / locator / uuid / time / excerpt) — never silently picks one.
pub(crate) fn resolve_selection<'a>(
    selection: &[Sel],
    images: &'a [ImageRef],
    session_files: &[PathBuf],
) -> Result<Vec<&'a ImageRef>> {
    let mut sel = Vec::new();
    let mut unresolved = Vec::new();
    let mut ambiguous: Vec<(usize, Vec<&ImageRef>)> = Vec::new();
    for s in selection {
        match s {
            Sel::Seq(n) => {
                let cands: Vec<&ImageRef> = images.iter().filter(|i| i.seq == Some(*n)).collect();
                let distinct = distinct_by_fingerprint(&cands);
                match distinct.len() {
                    0 => unresolved.push(format!("#{n}")),
                    1 => sel.push(distinct[0]),
                    _ => ambiguous.push((*n, distinct)),
                }
            }
            Sel::Loc(line, idx) => match images
                .iter()
                .find(|i| i.line_no == *line && i.img_index == *idx)
            {
                Some(i) => sel.push(i),
                None => unresolved.push(format!("L{line}i{idx}")),
            },
        }
    }
    if !ambiguous.is_empty() {
        return Err(ambiguity_error(
            &ambiguous,
            pinned_path(session_files, images),
        ));
    }
    if !unresolved.is_empty() {
        // Name the handles that DO exist: `#N` is inherited from CC's paste-time
        // `[Image #N]` numbering, so a transcript's handles can start past #1 and carry
        // holes — a bare "matched no image" reads like a csift drop when it is a source gap.
        let mut present: Vec<usize> = images.iter().filter_map(|i| i.seq).collect();
        present.sort_unstable();
        present.dedup();
        let unnumbered = images.iter().filter(|i| i.seq.is_none()).count();
        let inventory = if present.is_empty() && unnumbered == 0 {
            "the scope carries no images at all".to_string()
        } else {
            const HANDLE_CAP: usize = 24;
            let mut s: String = present
                .iter()
                .take(HANDLE_CAP)
                .map(|n| format!("#{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            if present.len() > HANDLE_CAP {
                s.push_str(&format!(" (+{} more)", present.len() - HANDLE_CAP));
            }
            if unnumbered > 0 {
                if !s.is_empty() {
                    s.push_str(" + ");
                }
                s.push_str(&format!(
                    "{unnumbered} unnumbered image(s) (address by the L<line>i<n> locator)"
                ));
            }
            format!("present here: {s}")
        };
        bail!(
            "--id matched no image: {} — {inventory}. `#N` handles are inherited from Claude \
             Code's paste-time `[Image #N]` numbering, NOT a dense 1..N index csift assigns, \
             so a transcript's handles can start past #1 and carry holes; a missing number is \
             a source gap (that number's image never landed in this transcript), not a \
             dropped image. Run `csift image` on the same target to see the full listing.",
            unresolved.join(", ")
        );
    }
    Ok(sel)
}

/// Per-line turn index + concatenated text for one transcript — a full parse (the `--id`/
/// `--turn` paths already pin a single transcript, so this is one file) used to attach
/// `t<turn>` + an excerpt to each occurrence in an ambiguity error.
pub(crate) struct LineInfo {
    pub(crate) turn_of: HashMap<usize, usize>,
    pub(crate) text: HashMap<usize, String>,
}

pub(crate) fn transcript_line_info(path: &Path) -> Result<LineInfo> {
    let mut info = LineInfo {
        turn_of: HashMap::new(),
        text: HashMap::new(),
    };
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(info);
    };
    let (mut records, _skipped) = parse_candidates_parallel(&mmap, |_| true);
    records.sort_by_key(|(ln, _)| *ln);
    for (ln, rec) in &records {
        if let Some(blocks) = rec.blocks() {
            let mut t = String::new();
            for b in blocks {
                if let Block::Text { text } = b {
                    if !t.is_empty() {
                        t.push(' ');
                    }
                    t.push_str(text);
                }
            }
            if !t.is_empty() {
                info.text.insert(*ln, t);
            }
        }
    }
    // Turn index per line via the shared §6.4 delimiter (byte-consistent with turns/search).
    for (ti, group) in crate::model::group_turn_indices_deduped(&records, |(_, r)| r)
        .iter()
        .enumerate()
    {
        for &ri in group {
            info.turn_of.insert(records[ri].0, ti);
        }
    }
    Ok(info)
}

/// First 8 chars of a uuid (its short, still-unique-in-practice prefix) for the ambiguity list.
pub(crate) fn short_uuid(u: &str) -> String {
    u.chars().take(8).collect()
}

/// A whitespace-normalized excerpt of `text` centered on `needle` (`[Image #N]`), `radius` chars
/// each side — char-boundary safe. Falls back to the head when the needle isn't found.
pub(crate) fn excerpt_around(text: &str, needle: &str, radius: usize) -> String {
    let norm: Vec<char> = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    let nd: Vec<char> = needle.chars().collect();
    let pos = (0..norm.len()).find(|&i| norm[i..].starts_with(&nd[..]));
    let (s, e) = match pos {
        Some(p) => (
            p.saturating_sub(radius),
            (p + nd.len() + radius).min(norm.len()),
        ),
        None => (0, (radius * 2).min(norm.len())),
    };
    norm[s..e].iter().collect()
}

/// Build the `#N is ambiguous` error: for each reused `#N`, list every distinct occurrence with
/// its turn, `L<line>i<n>` locator, uuid, time, and an excerpt around the `[Image #N]` marker —
/// everything the consumer needs to disambiguate (by locator, or `--since`/`--turn`/`--uuid`).
pub(crate) fn ambiguity_error(
    ambiguous: &[(usize, Vec<&ImageRef>)],
    path: Option<&Path>,
) -> anyhow::Error {
    let info = path.and_then(|p| transcript_line_info(p).ok());
    let mut msg = String::new();
    for (n, occs) in ambiguous {
        if !msg.is_empty() {
            msg.push('\n');
        }
        msg.push_str(&format!(
            "--id #{n} is ambiguous: it names {} different images in this transcript (Claude Code \
             reuses `#N` across prompts). Pick one by its exact `--id L<line>i<n>`, or narrow the \
             scope with --since/--until (a time window) / --turn / --uuid:",
            occs.len()
        ));
        for o in occs {
            let turn = info
                .as_ref()
                .and_then(|i| i.turn_of.get(&o.line_no))
                .map(|t| format!("t{t}"))
                .unwrap_or_else(|| "t?".to_string());
            let when = format_timestamp(o.ts_utc.as_deref());
            let uuid = o
                .record_uuid
                .as_deref()
                .map(short_uuid)
                .unwrap_or_else(|| "--------".to_string());
            let excerpt = info
                .as_ref()
                .and_then(|i| i.text.get(&o.line_no))
                .map(|t| excerpt_around(t, &format!("[Image #{n}]"), 48))
                .unwrap_or_default();
            msg.push_str(&format!(
                "\n  #{n}  {}  {turn}  {when}  uuid {uuid}  \"…{excerpt}…\"",
                o.id()
            ));
        }
    }
    anyhow::anyhow!(msg)
}
