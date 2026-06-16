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
    /// The stable, surfaceable id `L<line>i<n>`.
    fn id(&self) -> String {
        format!("L{}i{}", self.line_no, self.img_index)
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

    /// `<session-short>-L<line>i<n>.<ext>` — unique across sessions/lines/indices, so a
    /// multi-transcript `--out` extraction never collides.
    fn out_filename(&self) -> String {
        let short = self.session_id.get(..8).unwrap_or(&self.session_id);
        format!("{short}-{}.{}", self.id(), self.ext())
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
    let (b64_len, est_bytes, url, data) = if source_kind == "url" {
        let url = source
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        (0, 0, url, None)
    } else {
        let data = source.get("data").and_then(Value::as_str)?;
        (
            data.len(),
            est_decoded_len(data),
            None,
            with_data.then(|| data.to_string()),
        )
    };
    Some(ImageRef {
        session_id: String::new(),
        is_subagent: false,
        parent_session_id: String::new(),
        line_no: 0,
        img_index: 0,
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

/// Collect every image carried by one record, in document order: a direct `Block::Image`
/// (the user-sent / assistant case) OR an `{type:"image"}` element nested in a
/// `Block::ToolResult.content` array (a tool screenshot). `img_index` is a 1-based running
/// ordinal across both sources, so the id `L<line>i<n>` is stable.
fn record_images(rec: &Record, with_data: bool) -> Vec<ImageRef> {
    let mut out = Vec::new();
    let Some(blocks) = rec.blocks() else {
        return out;
    };
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
    out
}

/// The stable `L<line>i<n>` image ids carried by one record — for surfacing in `turns`
/// (and any other view) without the full extraction machinery. Empty when the record has
/// no images.
pub(crate) fn image_ids_for_record(rec: &Record, line_no: usize) -> Vec<String> {
    record_images(rec, false)
        .into_iter()
        .map(|mut r| {
            r.line_no = line_no;
            r.id()
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

/// Parse the `--id` selection: `L<line>i<n>` tokens (the leading `L` optional), repeatable
/// and comma-delimited. Returns the `(line, index)` set. A malformed token is a hard error.
fn parse_id_selection(ids: &[String]) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    for raw in ids {
        for tok in raw.split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            let body = tok
                .strip_prefix('L')
                .or_else(|| tok.strip_prefix('l'))
                .unwrap_or(tok);
            let (line, idx) = body
                .split_once('i')
                .with_context(|| format!("--id `{tok}` is not `L<line>i<n>` (e.g. `L6812i2`)"))?;
            let line: usize = line
                .parse()
                .with_context(|| format!("--id `{tok}`: bad line number `{line}`"))?;
            let idx: usize = idx
                .parse()
                .with_context(|| format!("--id `{tok}`: bad image index `{idx}`"))?;
            out.push((line, idx));
        }
    }
    Ok(out)
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

    // Apply `--id` selection (if any). Selection is per-(line,index); when the scope spans
    // more than one transcript the `L<line>i<n>` ids are ambiguous, so require a single
    // transcript (same rule/spirit as `search --line`).
    let selected: Vec<&ImageRef> = if selection.is_empty() {
        images.iter().collect()
    } else {
        if distinct_transcripts > 1 {
            bail!(
                "--id addresses one transcript by line, but the scope resolves to {} — pin it \
                 with `--session <uuid> --no-subagents`",
                distinct_transcripts
            );
        }
        let mut sel = Vec::new();
        let mut unresolved = Vec::new();
        for (line, idx) in &selection {
            match images
                .iter()
                .find(|i| i.line_no == *line && i.img_index == *idx)
            {
                Some(i) => sel.push(i),
                None => unresolved.push(format!("L{line}i{idx}")),
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
                        "id": img.id(), "path": path.to_string_lossy(),
                        "bytes": bytes.len(), "media_type": img.media_type,
                        "session_id": img.session_id, "is_subagent": img.is_subagent,
                        "parent_session_id": img.parent_session_id,
                    })
                );
            }
            OutputFormat::Text => {
                println!(
                    "wrote {}  ({}, {})",
                    path.display(),
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
        println!(
            "{}  {}  {}{}",
            img.id(),
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
        assert_eq!(
            parse_id_selection(&["L6812i2,L99i1".into(), "L1i1".into()]).unwrap(),
            vec![(6812, 2), (99, 1), (1, 1)]
        );
        assert!(parse_id_selection(&["nope".into()]).is_err());
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
