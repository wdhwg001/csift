//! `csift image` — list and EXTRACT the images a session carries.
//!
//! A user-pasted/attached image (and a tool-result screenshot) rides INLINE on a record as
//! an `{type:"image", source:{type:"base64", media_type:"image/png", data:"<base64>"}}`
//! block — verified against real `~/.claude/projects` data (2026-06-16). The bytes live in
//! the jsonl, so `image` decodes them straight back to files; nothing is externalised.
//!
//! Stable image id = `L<line>i<n>`: the 1-based JSONL line of the carrying record plus the
//! 1-based ordinal of the image among that record's image blocks. It is stable because the
//! transcript is append-only, and it is consistent with the `Lnnnnn` line references used
//! across `recover` / `turns` / `search` (so an id surfaced there feeds straight back here).
//!
//! Default action is to LIST. Pass `--out <PATH>` to EXTRACT — a DIRECTORY keeps each image's
//! source format; a FILE path's extension converts to that format (the `convert in out.jpg` idiom).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use memchr::memmem;
use serde_json::{json, Value};

use crate::cli::{ImageArgs, ImageOutFormat, OutputFormat};
use crate::model::{Block, Record};
use crate::parse::{mmap_bytes, parse_candidates_parallel};
use crate::timez::{format_timestamp, local_iso};

/// One image block discovered in a transcript.
#[derive(Debug, Clone)]
struct ImageRef {
    /// Bare hex for a subagent transcript, the uuid for a top-level session.
    session_id: String,
    is_subagent: bool,
    /// Re-feedable owning session uuid (== `session_id` for a top-level hit).
    parent_session_id: String,
    /// 1-based JSONL line of the carrying record.
    line_no: usize,
    /// 1-based ordinal of this image among the record's image blocks.
    img_index: usize,
    /// The session-facing `[Image #N]` number — the SAME handle the model sees and refers to
    /// ("re-share #32"). Extracted by positionally zipping the record's `[Image #N]` text
    /// markers with its image blocks (CC numbers pasted images per-prompt; `history.ts`).
    /// `None` when the marker count doesn't match (then only `L<line>i<n>` addresses it).
    /// NOT globally unique — CC reuses low numbers across prompts, so a `#N` that names >1
    /// DISTINCT image is AMBIGUOUS: `--id #N` then ERRORS with the occurrence list rather than
    /// silently guessing (disambiguate with the locator or `--since`/`--turn-range`/`--uuid`).
    seq: Option<usize>,
    /// A cheap content fingerprint (`<len>:<head>:<tail>` of the base64) — dedups the SAME
    /// image re-injected across context windows so the listing shows it once.
    fingerprint: String,
    /// `"base64"` | `"url"` | another source kind (kept verbatim).
    source_kind: String,
    media_type: String,
    /// Base64 character length (0 for a non-base64 source).
    b64_len: usize,
    /// Estimated decoded byte size (base64 only; 0 otherwise).
    est_bytes: usize,
    /// The image URL, for a `source.type == "url"` image (no inline bytes to extract).
    url: Option<String>,
    ts_utc: Option<String>,
    record_uuid: Option<String>,
    /// The base64 payload — populated ONLY in extract mode (`--out`), to bound memory in the
    /// common list path.
    data: Option<String>,
}

impl ImageRef {
    /// The exact per-occurrence locator `L<line>i<n>` (always unambiguous).
    fn id(&self) -> String {
        format!("L{}i{}", self.line_no, self.img_index)
    }

    /// The session-facing handle `#N` when known, else the locator — so output leads with the
    /// number the model already uses ("re-share #32" → `--id #32`).
    fn handle(&self) -> String {
        match self.seq {
            Some(n) => format!("#{n}"),
            None => self.id(),
        }
    }

    /// File extension implied by the media type (`bin` when unknown — never fabricated).
    fn ext(&self) -> &'static str {
        match self.media_type.as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "image/bmp" => "bmp",
            "image/tiff" => "tiff",
            "image/heic" => "heic",
            "image/avif" => "avif",
            _ => "bin",
        }
    }

    /// `<session-short>[-img<N>]-L<line>i<n>.<ext>` — carries the `#N` when known (so the file
    /// is recognizable as "image #32") and stays unique via the `L<line>i<n>` locator, so a
    /// multi-image / multi-transcript `--out` extraction never collides. `ext` is the source
    /// extension by default, or the `--out` file path's when its extension converts the format.
    fn out_filename_with_ext(&self, ext: &str) -> String {
        let short = self.session_id.get(..8).unwrap_or(&self.session_id);
        match self.seq {
            Some(n) => format!("{short}-img{n}-{}.{ext}", self.id()),
            None => format!("{short}-{}.{ext}", self.id()),
        }
    }
}

/// Pre-JSON byte prefilter: a line MIGHT carry an image block. Coarse by design — the
/// structural walk (`record_images`) decides what each line really holds.
fn line_is_image_candidate(line: &[u8]) -> bool {
    memmem::find(line, br#""type":"image""#).is_some()
        || memmem::find(line, br#""media_type""#).is_some()
        || memmem::find(line, b"base64").is_some()
}

/// Human-friendly byte size (`294 KB`, `1.2 MB`).
fn human_bytes(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{} KB", n / 1024)
    } else {
        format!("{n} B")
    }
}

/// Estimated decoded size of a base64 string (4 chars → 3 bytes, minus `=` padding). Exact
/// enough for a listing without decoding the (large) payload.
fn est_decoded_len(b64: &str) -> usize {
    let pad = b64
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&c| c == b'=')
        .count();
    (b64.len() / 4) * 3 - pad.min(2)
}

/// Decode standard-alphabet base64 (padding + embedded whitespace tolerated). Returns `None`
/// on an invalid character — so a malformed image is REPORTED, never silently written wrong.
/// Hand-rolled (~30 lines) to avoid pulling a base64 crate just for this; the `image` crate is
/// the one heavyweight dependency, justified by the format transcoding it alone can do.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    fn sextet(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        buf = (buf << 6) | sextet(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Pull `{type, media_type, data|url}` out of an image block's `source` value into an
/// `ImageRef` skeleton (line/index/session filled by the caller). `None` when the source is
/// absent or shapeless.
fn image_ref_from_source(source: &Value, with_data: bool) -> Option<ImageRef> {
    let source_kind = source
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("base64")
        .to_string();
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let (b64_len, est_bytes, url, fingerprint, data) = if source_kind == "url" {
        let url = source
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        let fp = format!("url:{}", url.as_deref().unwrap_or(""));
        (0, 0, url, fp, None)
    } else {
        let d = source.get("data").and_then(Value::as_str)?;
        // Cheap content fingerprint: length + base64 head/tail. Dedups the SAME image
        // re-injected across context windows without holding/hashing the full payload.
        let head: String = d.chars().take(32).collect();
        let tail: String = d.chars().rev().take(32).collect();
        let fp = format!("{}:{head}:{tail}", d.len());
        (
            d.len(),
            est_decoded_len(d),
            None,
            fp,
            with_data.then(|| d.to_string()),
        )
    };
    Some(ImageRef {
        session_id: String::new(),
        is_subagent: false,
        parent_session_id: String::new(),
        line_no: 0,
        img_index: 0,
        seq: None,
        fingerprint,
        source_kind,
        media_type,
        b64_len,
        est_bytes,
        url,
        ts_utc: None,
        record_uuid: None,
        data,
    })
}

/// Extract the `[Image #N]` numbers from a text block, in order of appearance. CC writes
/// these per-prompt as the model's image handles (`history.ts`).
fn parse_image_markers(text: &str, out: &mut Vec<usize>) {
    let mut rest = text;
    const PAT: &str = "[Image #";
    while let Some(i) = rest.find(PAT) {
        let after = &rest[i + PAT.len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && after[digits.len()..].starts_with(']') {
            if let Ok(n) = digits.parse::<usize>() {
                out.push(n);
            }
        }
        rest = &after[digits.len()..];
    }
}

/// Collect every image carried by one record, in document order: a direct `Block::Image`
/// (the user-sent / assistant case) OR an `{type:"image"}` element nested in a
/// `Block::ToolResult.content` array (a tool screenshot). `img_index` is a 1-based running
/// ordinal across both sources, so the id `L<line>i<n>` is stable.
fn record_images(rec: &Record, with_data: bool) -> Vec<ImageRef> {
    let mut out = Vec::new();
    let Some(blocks) = rec.blocks() else {
        return out;
    };
    // `[Image #N]` markers across the record's text blocks, in document order.
    let mut markers: Vec<usize> = Vec::new();
    for block in blocks {
        if let Block::Text { text } = block {
            parse_image_markers(text, &mut markers);
        }
    }
    for block in blocks {
        match block {
            Block::Image { source: Some(src) } => {
                if let Some(mut r) = image_ref_from_source(src, with_data) {
                    r.img_index = out.len() + 1;
                    out.push(r);
                }
            }
            Block::ToolResult {
                content: Some(content),
                ..
            } => {
                if let Some(arr) = content.as_array() {
                    for el in arr {
                        if el.get("type").and_then(Value::as_str) == Some("image") {
                            if let Some(src) = el.get("source") {
                                if let Some(mut r) = image_ref_from_source(src, with_data) {
                                    r.img_index = out.len() + 1;
                                    out.push(r);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Assign `#N` by POSITIONAL zip — only when the marker count matches the image count
    // (CC guarantees `[Image #N]` is unique within a prompt; a mismatch means a back-
    // reference to a compressed-out image, so we leave `seq = None` rather than misassign).
    if markers.len() == out.len() {
        for (r, &n) in out.iter_mut().zip(markers.iter()) {
            r.seq = Some(n);
        }
    }
    out
}

/// The image HANDLES a record carries — `#N` (the session's own image number) when known,
/// else the `L<line>i<n>` locator — for surfacing in `turns`. Both feed straight back into
/// `csift image --id`. Empty when the record has no images.
pub(crate) fn image_ids_for_record(rec: &Record, line_no: usize) -> Vec<String> {
    record_images(rec, false)
        .into_iter()
        .map(|mut r| {
            r.line_no = line_no;
            r.handle()
        })
        .collect()
}

/// Scan ONE transcript for images. Mirrors `recover`/`files`: mmap + a pre-JSON byte
/// prefilter + parse only candidate lines (line-numbered, 1:1 with the file). Returns the
/// images and the malformed-line count (surfaced, never hidden).
fn images_in_file(path: &Path, with_data: bool) -> Result<(Vec<ImageRef>, usize)> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok((Vec::new(), 0));
    };
    let (records, skipped) = parse_candidates_parallel(&mmap, line_is_image_candidate);

    let mut out = Vec::new();
    for (line_no, rec) in &records {
        for mut r in record_images(rec, with_data) {
            r.session_id = session_id.clone();
            r.is_subagent = is_subagent;
            r.parent_session_id = parent_session_id.clone();
            r.line_no = *line_no;
            r.ts_utc = rec.timestamp.clone();
            r.record_uuid = rec.uuid.clone();
            out.push(r);
        }
    }
    // Stable order: by line, then image ordinal.
    out.sort_by(|a, b| (a.line_no, a.img_index).cmp(&(b.line_no, b.img_index)));
    Ok((out, skipped))
}

/// One `--id` selector: either the session's `#N` image number, or an exact `L<line>i<n>`
/// locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sel {
    /// `#N` / bare `N` — the `[Image #N]` handle the model uses. Resolves to the unique image
    /// with that handle in scope; if it names >1 DISTINCT image (CC reuses `#N` across prompts)
    /// it is AMBIGUOUS and ERRORS with the occurrence list rather than silently picking one.
    Seq(usize),
    /// `L<line>i<n>` — the exact per-occurrence locator.
    Loc(usize, usize),
}

/// Parse the `--id` selection (repeatable + comma-delimited). Accepts `#32` / `32` (a `#N`
/// handle) and `L6812i2` / `6812i2` (a locator). A malformed token is a hard error.
fn parse_id_selection(ids: &[String]) -> Result<Vec<Sel>> {
    let mut out = Vec::new();
    for raw in ids {
        for tok in raw.split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            if let Some(n) = tok.strip_prefix('#') {
                let n = n
                    .parse()
                    .with_context(|| format!("--id `{tok}`: bad image number `{n}`"))?;
                out.push(Sel::Seq(n));
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
fn dedup_latest(images: &[ImageRef]) -> Vec<&ImageRef> {
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
/// for `--id` / `--turn-range` (line numbers + `#N` + turn indices are all per-transcript).
fn count_distinct_transcripts(images: &[ImageRef]) -> usize {
    let mut ids: Vec<&str> = images.iter().map(|i| i.session_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

/// The transcript file backing a single-transcript image set (by matching session id) — used to
/// re-parse it for turn indices / excerpts on the `--turn-range` and ambiguity paths.
fn pinned_path<'a>(session_files: &'a [PathBuf], images: &[ImageRef]) -> Option<&'a Path> {
    let sid = images.first().map(|i| i.session_id.as_str())?;
    session_files
        .iter()
        .map(PathBuf::as_path)
        .find(|p| crate::subagent::session_id_from_path(p) == sid)
}

/// The DISTINCT images among `cands` (same `#N`): dedup by content fingerprint keeping the
/// latest occurrence, ordered by line. Length 1 ⇒ `#N` is unambiguous; >1 ⇒ genuine reuse.
fn distinct_by_fingerprint<'a>(cands: &[&'a ImageRef]) -> Vec<&'a ImageRef> {
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
fn resolve_selection<'a>(
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
        bail!("--id matched no image: {}", unresolved.join(", "));
    }
    Ok(sel)
}

/// Per-line turn index + concatenated text for one transcript — a full parse (the `--id`/
/// `--turn-range` paths already pin a single transcript, so this is one file) used to attach
/// `t<turn>` + an excerpt to each occurrence in an ambiguity error.
struct LineInfo {
    turn_of: HashMap<usize, usize>,
    text: HashMap<usize, String>,
}

fn transcript_line_info(path: &Path) -> Result<LineInfo> {
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
fn short_uuid(u: &str) -> String {
    u.chars().take(8).collect()
}

/// A whitespace-normalized excerpt of `text` centered on `needle` (`[Image #N]`), `radius` chars
/// each side — char-boundary safe. Falls back to the head when the needle isn't found.
fn excerpt_around(text: &str, needle: &str, radius: usize) -> String {
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
/// everything the consumer needs to disambiguate (by locator, or `--since`/`--turn-range`/`--uuid`).
fn ambiguity_error(ambiguous: &[(usize, Vec<&ImageRef>)], path: Option<&Path>) -> anyhow::Error {
    let info = path.and_then(|p| transcript_line_info(p).ok());
    let mut msg = String::new();
    for (n, occs) in ambiguous {
        if !msg.is_empty() {
            msg.push('\n');
        }
        msg.push_str(&format!(
            "--id #{n} is ambiguous: it names {} different images in this transcript (Claude Code \
             reuses `#N` across prompts). Pick one by its exact `--id L<line>i<n>`, or narrow the \
             scope with --since/--until (a time window) / --turn-range / --uuid:",
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

pub fn run_image(args: &ImageArgs) -> Result<()> {
    let extracting = args.out.is_some();
    let selection = parse_id_selection(&args.id)?;

    let session_files = crate::path::resolve_session_files(
        &args.paths,
        args.want_subagents().into(),
        crate::path::Caller::Other,
    )?;

    // Scan across files (rayon — each is an independent mmap + prefilter scan).
    use rayon::prelude::*;
    let per_file: Vec<(Vec<ImageRef>, usize)> = session_files
        .par_iter()
        .map(|p| images_in_file(p, extracting))
        .collect::<Result<Vec<_>>>()?;

    let mut images: Vec<ImageRef> = Vec::new();
    let mut skipped_lines = 0usize;
    for (refs, skipped) in per_file {
        images.extend(refs);
        skipped_lines += skipped;
    }
    // Combined stable order: chronological by record ts, then line/index; ts-less last.
    images.sort_by(|a, b| {
        match (a.ts_utc.as_deref(), b.ts_utc.as_deref()) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then((a.line_no, a.img_index).cmp(&(b.line_no, b.img_index)))
    });

    // ── Scope filters: each NARROWS the image set, so an ambiguous `#N` can resolve in a
    //    window / turn / uuid where it is unique — the disambiguators the ambiguity error
    //    names, and pre-applyable up front (`--since 1h` so `#N` is unique in the last hour). ──
    let time_window =
        crate::time_window::TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    images.retain(|i| time_window.contains(i.ts_utc.as_deref()));
    if let Some(prefix) = args.uuid.as_deref() {
        images.retain(|i| {
            i.record_uuid
                .as_deref()
                .is_some_and(|u| u.starts_with(prefix))
        });
    }
    let mut distinct_transcripts = count_distinct_transcripts(&images);

    // `--turn-range` is per-transcript (turn indices are), so it needs a single transcript.
    if let Some(spec) = args.turn_range.as_deref() {
        let (lo, hi) = crate::text::parse_range(spec, "--turn-range", false)?;
        if distinct_transcripts > 1 {
            bail!(
                "--turn-range is per-transcript (turn indices are), but the scope resolves to \
                 {distinct_transcripts} transcripts — pin it with `@<uuid> --no-subagents`"
            );
        }
        if let Some(path) = pinned_path(&session_files, &images) {
            let turn_of = transcript_line_info(path)?.turn_of;
            images.retain(|i| {
                turn_of
                    .get(&i.line_no)
                    .is_some_and(|t| *t >= lo && *t <= hi)
            });
        } else {
            images.clear();
        }
        distinct_transcripts = count_distinct_transcripts(&images);
    }

    // Apply the `--id` selection (if any). `#N` and `L<line>i<n>` are both per-transcript, so
    // a multi-transcript scope is ambiguous → require a single transcript (like `search --line`).
    let selected: Vec<&ImageRef> = if selection.is_empty() {
        // List/extract ALL → dedup the SAME image re-injected across context windows (by
        // content fingerprint). Two DISTINCT-content images that share a `#N` both survive, so
        // the listing shows the reuse — and an `--id #N` against it ERRORS (never silent-picks).
        dedup_latest(&images)
    } else {
        if distinct_transcripts > 1 {
            bail!(
                "--id is per-transcript (line numbers and `#N` are), but the scope resolves to \
                 {distinct_transcripts} transcripts — pin it with `@<uuid> --no-subagents`"
            );
        }
        resolve_selection(&selection, &images, &session_files)?
    };

    if let Some(out_dir) = args.out.as_deref() {
        extract(&selected, out_dir, args, skipped_lines)
    } else {
        match args.format {
            OutputFormat::Json => render_json(&selected, distinct_transcripts, skipped_lines),
            OutputFormat::Text => render_text(&selected, distinct_transcripts, skipped_lines),
        }
        Ok(())
    }
}

/// A source `media_type` → an [`ImageOutFormat`]. `None` for a media type outside the four
/// Claude-API image types (shouldn't occur — CC only stores those).
fn format_of_media_type(mt: &str) -> Option<ImageOutFormat> {
    match mt {
        "image/png" => Some(ImageOutFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageOutFormat::Jpeg),
        "image/gif" => Some(ImageOutFormat::Gif),
        "image/webp" => Some(ImageOutFormat::Webp),
        _ => None,
    }
}

/// Decode `bytes` and RE-ENCODE to `target` (caller handles the same-format passthrough). An
/// animated GIF flattened to a still target yields its FIRST frame (with a warning note);
/// →jpeg is quality-90 lossy, →gif is palette-quantized, →webp is pure-Rust lossless. Returns
/// the encoded bytes plus any short human notes. A decode/encode failure is surfaced, never a
/// wrong file.
fn convert_image(bytes: &[u8], target: ImageOutFormat) -> Result<(Vec<u8>, Vec<String>)> {
    let mut notes = Vec::new();
    let src_is_gif = image::guess_format(bytes).ok() == Some(image::ImageFormat::Gif);

    // Decode to a single still image. An animated GIF → still target keeps only the first frame.
    let img: image::DynamicImage = if src_is_gif && target != ImageOutFormat::Gif {
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
            .context("decode gif")?;
        let frames = image::AnimationDecoder::into_frames(decoder)
            .collect_frames()
            .context("read gif frames")?;
        let n = frames.len();
        let total_s: f64 = frames
            .iter()
            .map(|f| {
                let (num, den) = f.delay().numer_denom_ms();
                if den == 0 {
                    0.0
                } else {
                    f64::from(num) / f64::from(den)
                }
            })
            .sum::<f64>()
            / 1000.0;
        let first = frames
            .into_iter()
            .next()
            .context("animated gif has no frames")?;
        if n > 1 {
            notes.push(format!(
                "animated GIF ({n} frames, {total_s:.1}s) — extracted the first frame only"
            ));
        }
        image::DynamicImage::ImageRgba8(first.into_buffer())
    } else {
        image::load_from_memory(bytes).context("decode source image")?
    };

    let mut out = Vec::new();
    match target {
        ImageOutFormat::Png => {
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .context("encode png")?;
        }
        ImageOutFormat::Jpeg => {
            // JPEG has no alpha → flatten to RGB; quality fixed at 90.
            let rgb = img.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut std::io::Cursor::new(&mut out),
                90,
            )
            .encode_image(&rgb)
            .context("encode jpeg")?;
            notes.push("lossy re-encode (jpeg quality 90)".to_string());
        }
        ImageOutFormat::Gif => {
            // Build a ≤256-color palette (NeuQuant) and Floyd-Steinberg dither INTO it before
            // encoding, so a photographic source doesn't band. `image`'s dither diffuses error to
            // x+1 unconditionally → it panics on a 1px-wide buffer; a sub-2px image has nothing to
            // dither anyway, so skip it there (the encoder still quantizes) and never crash.
            let mut rgba = img.to_rgba8();
            let dithered = rgba.width() >= 2 && rgba.height() >= 2;
            if dithered {
                let nq = color_quant::NeuQuant::new(10, 256, rgba.as_raw());
                image::imageops::colorops::dither(&mut rgba, &nq);
            }
            image::codecs::gif::GifEncoder::new(&mut std::io::Cursor::new(&mut out))
                .encode_frame(image::Frame::new(rgba))
                .context("encode gif")?;
            notes.push(
                if dithered {
                    "dithered to a ≤256-color palette"
                } else {
                    "quantized to a ≤256-color palette"
                }
                .to_string(),
            );
        }
        ImageOutFormat::Webp => {
            // libwebp (the `webp` crate) — proper lossy quality-90, not a lossless-only fallback.
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            out = webp::Encoder::from_rgba(rgba.as_raw(), w, h)
                .encode(90.0)
                .to_vec();
            notes.push("lossy re-encode (webp quality 90)".to_string());
        }
    }
    Ok((out, notes))
}

/// Decode + write the selected image(s) to `out_path`. The path's EXTENSION drives the format
/// (the `convert in out.jpg` idiom): a path WITH a `png`/`jpg`/`jpeg`/`gif`/`webp` extension is a
/// single-FILE target in that format (the one selected image is CONVERTED to it if it differs);
/// any other path is a DIRECTORY, each image auto-named in its SOURCE format. A decode/IO failure
/// is surfaced (never a silent or partial-wrong file).
fn extract(
    selected: &[&ImageRef],
    out_path: &Path,
    args: &ImageArgs,
    skipped_lines: usize,
) -> Result<()> {
    let file_target = out_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(ImageOutFormat::from_ext);

    if let Some(t) = file_target {
        // One file ⇒ exactly one image; create the parent dir (the path itself is the file).
        if selected.len() != 1 {
            bail!(
                "--out {} is a single {} file, but {} image(s) are selected — give a directory \
                 (a path with no image extension) to extract many, or narrow `--id` to one",
                out_path.display(),
                t.ext(),
                selected.len()
            );
        }
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
        }
    } else {
        std::fs::create_dir_all(out_path)
            .with_context(|| format!("cannot create output dir {}", out_path.display()))?;
    }

    let mut written = 0usize;
    let mut skipped_url = 0usize;
    for img in selected {
        if img.source_kind == "url" {
            // No inline bytes to write — report the URL, never fabricate a file.
            eprintln!(
                "csift: note: {} is a URL image ({}) — not extracted: {}",
                img.id(),
                img.media_type,
                img.url.as_deref().unwrap_or("<no url>")
            );
            skipped_url += 1;
            continue;
        }
        let b64 = img
            .data
            .as_deref()
            .with_context(|| format!("{}: image data missing (internal)", img.id()))?;
        let bytes = decode_base64(b64)
            .with_context(|| format!("{}: base64 decode failed (corrupt data)", img.id()))?;

        let src_fmt = format_of_media_type(&img.media_type);
        // FILE mode → convert to the path's format (raw if it already matches, keeping an animated
        // GIF animated); DIR mode → keep the source format (the "auto"/no-conversion path).
        let (out_bytes, out_ext, out_media, notes): (Vec<u8>, String, String, Vec<String>) =
            match file_target {
                None => (
                    bytes,
                    img.ext().to_string(),
                    img.media_type.clone(),
                    Vec::new(),
                ),
                Some(t) if src_fmt == Some(t) => (
                    bytes,
                    t.ext().to_string(),
                    t.media_type().to_string(),
                    Vec::new(),
                ),
                Some(t) => {
                    let (conv, notes) = convert_image(&bytes, t).with_context(|| {
                        format!(
                            "{}: convert {} → {}",
                            img.id(),
                            img.media_type,
                            t.media_type()
                        )
                    })?;
                    (conv, t.ext().to_string(), t.media_type().to_string(), notes)
                }
            };

        let path = if file_target.is_some() {
            out_path.to_path_buf()
        } else {
            out_path.join(img.out_filename_with_ext(&out_ext))
        };
        std::fs::write(&path, &out_bytes)
            .with_context(|| format!("cannot write {}", path.display()))?;
        match args.format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    json!({
                        "handle": img.handle(), "seq": img.seq, "id": img.id(),
                        "path": path.to_string_lossy(),
                        "bytes": out_bytes.len(),
                        "media_type": out_media,
                        "source_media_type": img.media_type,
                        "converted": file_target.is_some() && src_fmt != file_target,
                        "notes": notes,
                        "session_id": img.session_id, "is_subagent": img.is_subagent,
                        "parent_session_id": img.parent_session_id,
                    })
                );
            }
            OutputFormat::Text => {
                let kind = if out_media == img.media_type {
                    out_media.clone()
                } else {
                    format!("{}→{out_media}", img.media_type)
                };
                let note = if notes.is_empty() {
                    String::new()
                } else {
                    format!("  — {}", notes.join("; "))
                };
                println!(
                    "wrote {}  ({} {}, {}){}",
                    path.display(),
                    img.handle(),
                    kind,
                    human_bytes(out_bytes.len()),
                    note
                );
            }
        }
        written += 1;
    }
    if matches!(args.format, OutputFormat::Text) {
        let dest = if file_target.is_some() {
            String::new()
        } else {
            format!(" to {}", out_path.display())
        };
        let mut tail = format!("extracted {written} image(s){dest}");
        if skipped_url > 0 {
            tail.push_str(&format!(" · {skipped_url} url image(s) skipped"));
        }
        if skipped_lines > 0 {
            tail.push_str(&format!(
                " · ({})",
                crate::text::malformed_note(skipped_lines)
            ));
        }
        println!("{tail}");
    }
    Ok(())
}

/// Text listing: a session-label table (declared once), then one row per image.
fn render_text(selected: &[&ImageRef], transcripts: usize, skipped_lines: usize) {
    if selected.is_empty() {
        println!("no images found");
        if skipped_lines > 0 {
            println!("({})", crate::text::malformed_note(skipped_lines));
        }
        return;
    }
    for img in selected {
        let tag = if img.is_subagent {
            format!(
                "  SUBAGENT {} · parent {}",
                img.session_id, img.parent_session_id
            )
        } else {
            String::new()
        };
        let size = if img.source_kind == "url" {
            format!("url {}", img.url.as_deref().unwrap_or("<no url>"))
        } else {
            format!("{} ~{}", img.media_type, human_bytes(img.est_bytes))
        };
        // Lead with the session's `#N` handle (what the model says), then the exact locator.
        let label = match img.seq {
            Some(_) => format!("{:<5} {}", img.handle(), img.id()),
            None => img.id(),
        };
        println!(
            "{}  {}  {}{}",
            label,
            size,
            format_timestamp(img.ts_utc.as_deref()),
            tag
        );
    }
    let mut tail = format!(
        "{} image(s) · {} transcript(s)",
        selected.len(),
        transcripts
    );
    if skipped_lines > 0 {
        tail.push_str(&format!(
            " · ({})",
            crate::text::malformed_note(skipped_lines)
        ));
    }
    println!("{tail}");
}

/// JSON listing: one object per image, then a trailing summary object.
fn render_json(selected: &[&ImageRef], transcripts: usize, skipped_lines: usize) {
    for img in selected {
        println!(
            "{}",
            json!({
                "handle": img.handle(),
                "seq": img.seq,
                "id": img.id(),
                "line_no": img.line_no,
                "img_index": img.img_index,
                "session_id": img.session_id,
                "is_subagent": img.is_subagent,
                "parent_session_id": img.parent_session_id,
                "source_kind": img.source_kind,
                "media_type": img.media_type,
                "b64_len": img.b64_len,
                "est_bytes": img.est_bytes,
                "url": img.url,
                "record_uuid": img.record_uuid,
                "ts_utc": img.ts_utc,
                "ts_local": img.ts_utc.as_deref().and_then(local_iso),
            })
        );
    }
    println!(
        "{}",
        json!({ "images": selected.len(), "transcripts": transcripts, "skipped_lines": skipped_lines })
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip_standard_and_padded() {
        // "hi" → "aGk=", "caf\u{e9}" bytes → padded; whitespace tolerated.
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("a G V s\nbG8=").unwrap(), b"hello");
        // A 1x1 transparent PNG header decodes to the PNG magic bytes.
        let png = decode_base64("iVBORw0KGgo=").unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    }

    #[test]
    fn base64_rejects_invalid_char() {
        assert!(decode_base64("not base64 !!!@").is_none());
    }

    #[test]
    fn est_decoded_len_matches_real_decode() {
        for s in ["aGk=", "aGVsbG8=", "iVBORw0KGgo=", "Zm9vYmFy"] {
            assert_eq!(est_decoded_len(s), decode_base64(s).unwrap().len(), "{s}");
        }
    }

    #[test]
    fn id_and_filename_are_stable() {
        let mut r = image_ref_from_source(
            &json!({"type":"base64","media_type":"image/png","data":"aGk="}),
            false,
        )
        .unwrap();
        r.session_id = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into();
        r.line_no = 6812;
        r.img_index = 2;
        assert_eq!(r.id(), "L6812i2");
        assert_eq!(r.out_filename_with_ext(r.ext()), "0a1b2c3d-L6812i2.png");
        // A converted target overrides only the extension, keeping the unique locator.
        assert_eq!(r.out_filename_with_ext("jpg"), "0a1b2c3d-L6812i2.jpg");
        assert_eq!(r.ext(), "png");
    }

    #[test]
    fn parse_id_selection_forms() {
        // Locators (L<line>i<n>) AND the session `#N` handle (with `#` or bare number).
        assert_eq!(
            parse_id_selection(&["L6812i2,L99i1".into(), "L1i1".into()]).unwrap(),
            vec![Sel::Loc(6812, 2), Sel::Loc(99, 1), Sel::Loc(1, 1)]
        );
        assert_eq!(
            parse_id_selection(&["#32,#33".into(), "34".into()]).unwrap(),
            vec![Sel::Seq(32), Sel::Seq(33), Sel::Seq(34)]
        );
        // A bare number is a `#N` handle, not a locator (no `i`).
        assert_eq!(
            parse_id_selection(&["7".into()]).unwrap(),
            vec![Sel::Seq(7)]
        );
        assert!(parse_id_selection(&["nope".into()]).is_err());
        assert!(parse_id_selection(&["L6812".into()]).is_err()); // missing `i<n>`
    }

    #[test]
    fn seq_extracted_by_positional_zip_of_image_markers() {
        // Text lists `[Image #2] [Image #3]` and the record has 2 image blocks → positional.
        let r: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"see [Image #2] and [Image #3] please"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}}
            ]}}"#,
        )
        .unwrap();
        let imgs = record_images(&r, false);
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].seq, Some(2));
        assert_eq!(imgs[1].seq, Some(3));
        assert_eq!(imgs[1].media_type, "image/jpeg"); // NOT assumed PNG
                                                      // A count mismatch (1 marker, 2 images) leaves seq unassigned — no misattribution.
        let r2: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"only [Image #9] referenced"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}
            ]}}"#,
        )
        .unwrap();
        assert!(record_images(&r2, false).iter().all(|i| i.seq.is_none()));
    }

    #[test]
    fn url_source_has_no_bytes() {
        let r = image_ref_from_source(
            &json!({"type":"url","url":"https://example.test/x.png","media_type":"image/png"}),
            true,
        )
        .unwrap();
        assert_eq!(r.source_kind, "url");
        assert_eq!(r.b64_len, 0);
        assert_eq!(r.url.as_deref(), Some("https://example.test/x.png"));
        assert!(r.data.is_none());
    }

    #[test]
    fn excerpt_around_centers_on_marker_and_is_char_safe() {
        // Centers on the marker, normalizes whitespace, and never slices a multi-byte char.
        let text = "café münster ☕ [Image #7] dolor sit amet consectetur";
        let ex = excerpt_around(text, "[Image #7]", 8);
        assert!(ex.contains("[Image #7]"), "centered on the marker: {ex}");
        assert!(
            ex.contains('☕') || ex.contains("dolor"),
            "window around it: {ex}"
        );
        // No marker → head fallback, still char-safe (no panic on the multi-byte head).
        let head = excerpt_around("résumé piñata ☕ über no marker here", "[Image #9]", 4);
        assert!(!head.is_empty());
    }

    #[test]
    fn format_of_media_type_maps_the_four_api_types() {
        assert_eq!(format_of_media_type("image/png"), Some(ImageOutFormat::Png));
        assert_eq!(
            format_of_media_type("image/jpeg"),
            Some(ImageOutFormat::Jpeg)
        );
        assert_eq!(
            format_of_media_type("image/jpg"),
            Some(ImageOutFormat::Jpeg)
        );
        assert_eq!(format_of_media_type("image/gif"), Some(ImageOutFormat::Gif));
        assert_eq!(
            format_of_media_type("image/webp"),
            Some(ImageOutFormat::Webp)
        );
        assert_eq!(format_of_media_type("image/heic"), None);
    }
}
