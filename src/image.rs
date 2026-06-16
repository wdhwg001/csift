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
//! Default action is to LIST. Pass `--out <DIR>` to EXTRACT (decode → write `<DIR>/<file>`).

use std::path::Path;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use serde_json::{json, Value};

use crate::cli::{ImageArgs, OutputFormat};
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
    /// NOT globally unique — CC reuses low numbers across prompts, so a `#N` lookup resolves
    /// to the LATEST occurrence (what the live session currently means by it).
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
    /// multi-image / multi-transcript `--out` extraction never collides.
    fn out_filename(&self) -> String {
        let short = self.session_id.get(..8).unwrap_or(&self.session_id);
        match self.seq {
            Some(n) => format!("{short}-img{n}-{}.{}", self.id(), self.ext()),
            None => format!("{short}-{}.{}", self.id(), self.ext()),
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
/// Hand-rolled to keep csift dependency-free (the repo gates on a zero-vuln `npm`/`cargo`
/// audit; a new crate is audit surface we don't need for ~30 lines).
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
    /// `#N` / bare `N` — the `[Image #N]` handle the model uses. Resolves to the LATEST
    /// occurrence (CC reuses low numbers across prompts; latest = what the live session means).
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

pub fn run_image(args: &ImageArgs) -> Result<()> {
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }
    let extracting = args.out.is_some();
    let selection = parse_id_selection(&args.id)?;

    let session_files = crate::path::resolve_session_files(
        &args.paths,
        args.session.as_deref(),
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

    let distinct_transcripts = {
        let mut ids: Vec<&str> = images.iter().map(|i| i.session_id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    };

    // Apply the `--id` selection (if any). `#N` and `L<line>i<n>` are both per-transcript, so
    // a multi-transcript scope is ambiguous → require a single transcript (like `search --line`).
    let selected: Vec<&ImageRef> = if selection.is_empty() {
        // List/extract ALL → dedup the SAME image re-injected across context windows (by
        // content fingerprint), keeping the LATEST occurrence (its CURRENT `#N`), then order
        // by `#N` so a reader scanning for "#32" finds it in sequence.
        dedup_latest(&images)
    } else {
        if distinct_transcripts > 1 {
            bail!(
                "--id is per-transcript (line numbers and `#N` are), but the scope resolves to \
                 {} transcripts — pin it with `--session <uuid> --no-subagents`",
                distinct_transcripts
            );
        }
        let mut sel = Vec::new();
        let mut unresolved = Vec::new();
        for s in &selection {
            // `#N` → the LATEST occurrence with that number (CC reuses low numbers across
            // prompts; `images` is sorted ascending, so the last match is the current one).
            let found = match s {
                Sel::Seq(n) => images.iter().filter(|i| i.seq == Some(*n)).next_back(),
                Sel::Loc(line, idx) => images
                    .iter()
                    .find(|i| i.line_no == *line && i.img_index == *idx),
            };
            match found {
                Some(i) => sel.push(i),
                None => unresolved.push(match s {
                    Sel::Seq(n) => format!("#{n}"),
                    Sel::Loc(l, i) => format!("L{l}i{i}"),
                }),
            }
        }
        if !unresolved.is_empty() {
            bail!("--id matched no image: {}", unresolved.join(", "));
        }
        sel
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

/// Decode + write each selected image to `out_dir`. Reports every written path + byte count;
/// a decode/IO failure is surfaced (never a silent or partial-wrong file).
fn extract(
    selected: &[&ImageRef],
    out_dir: &Path,
    args: &ImageArgs,
    skipped_lines: usize,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create output dir {}", out_dir.display()))?;

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
        let path = out_dir.join(img.out_filename());
        std::fs::write(&path, &bytes)
            .with_context(|| format!("cannot write {}", path.display()))?;
        match args.format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    json!({
                        "handle": img.handle(), "seq": img.seq, "id": img.id(),
                        "path": path.to_string_lossy(),
                        "bytes": bytes.len(), "media_type": img.media_type,
                        "session_id": img.session_id, "is_subagent": img.is_subagent,
                        "parent_session_id": img.parent_session_id,
                    })
                );
            }
            OutputFormat::Text => {
                println!(
                    "wrote {}  ({} {}, {})",
                    path.display(),
                    img.handle(),
                    img.media_type,
                    human_bytes(bytes.len())
                );
            }
        }
        written += 1;
    }
    if matches!(args.format, OutputFormat::Text) {
        let mut tail = format!("extracted {written} image(s) to {}", out_dir.display());
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
        assert_eq!(r.out_filename(), "0a1b2c3d-L6812i2.png");
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
}
