//! ImageRef + base64 probes, marker zip, per-record image discovery.

use super::*;

/// One image block discovered in a transcript.
#[derive(Debug, Clone)]
pub(crate) struct ImageRef {
    /// Bare hex for a subagent transcript, the uuid for a top-level session.
    pub(crate) session_id: String,
    pub(crate) is_subagent: bool,
    /// Re-feedable owning session uuid (== `session_id` for a top-level hit).
    pub(crate) parent_session_id: String,
    /// 1-based JSONL line of the carrying record.
    pub(crate) line_no: usize,
    /// 1-based ordinal of this image among the record's image blocks.
    pub(crate) img_index: usize,
    /// The session-facing `[Image #N]` number — the SAME handle the model sees and refers to
    /// ("re-share #32"). Extracted by positionally zipping the record's `[Image #N]` text
    /// markers with its image blocks (CC numbers pasted images per-prompt; `history.ts`).
    /// `None` when the marker count doesn't match (then only `L<line>i<n>` addresses it).
    /// NOT globally unique — CC reuses low numbers across prompts, so a `#N` that names >1
    /// DISTINCT image is AMBIGUOUS: `--id #N` then ERRORS with the occurrence list rather than
    /// silently guessing (disambiguate with the locator or `--since`/`--turn`/`--uuid`).
    pub(crate) seq: Option<usize>,
    /// A cheap content fingerprint (`<len>:<head>:<tail>` of the base64) — dedups the SAME
    /// image re-injected across context windows so the listing shows it once.
    pub(crate) fingerprint: String,
    /// `"base64"` | `"url"` | another source kind (kept verbatim).
    pub(crate) source_kind: String,
    pub(crate) media_type: String,
    /// Base64 character length (0 for a non-base64 source).
    pub(crate) b64_len: usize,
    /// Estimated decoded byte size (base64 only; 0 otherwise).
    pub(crate) est_bytes: usize,
    /// The image URL, for a `source.type == "url"` image (no inline bytes to extract).
    pub(crate) url: Option<String>,
    pub(crate) ts_utc: Option<String>,
    pub(crate) record_uuid: Option<String>,
    /// The base64 payload — populated ONLY in extract mode (`--out`), to bound memory in the
    /// common list path.
    pub(crate) data: Option<String>,
}

impl ImageRef {
    /// The exact per-occurrence locator `L<line>i<n>` (always unambiguous).
    pub(crate) fn id(&self) -> String {
        format!("L{}i{}", self.line_no, self.img_index)
    }

    /// The session-facing handle `#N` when known, else the locator — so output leads with the
    /// number the model already uses ("re-share #32" → `--id #32`).
    pub(crate) fn handle(&self) -> String {
        match self.seq {
            Some(n) => format!("#{n}"),
            None => self.id(),
        }
    }

    /// File extension implied by the media type (`bin` when unknown — never fabricated).
    pub(crate) fn ext(&self) -> &'static str {
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
    pub(crate) fn out_filename_with_ext(&self, ext: &str) -> String {
        let short = self.session_id.get(..8).unwrap_or(&self.session_id);
        match self.seq {
            Some(n) => format!("{short}-img{n}-{}.{ext}", self.id()),
            None => format!("{short}-{}.{ext}", self.id()),
        }
    }
}

/// Pre-JSON byte prefilter: a line MIGHT carry an image block. Coarse by design — the
/// structural walk (`record_images`) decides what each line really holds.
pub(crate) fn line_is_image_candidate(line: &[u8]) -> bool {
    // Finders built ONCE (per-line hot path — the stateless form rebuilt its
    // searcher every call).
    static NEEDLES: std::sync::LazyLock<[memmem::Finder<'static>; 3]> =
        std::sync::LazyLock::new(|| {
            [
                memmem::Finder::new(br#""type":"image""#),
                memmem::Finder::new(br#""media_type""#),
                memmem::Finder::new(b"base64"),
            ]
        });
    NEEDLES.iter().any(|f| f.find(line).is_some())
}

/// Human-friendly byte size (`294 KB`, `1.2 MB`).
pub(crate) fn human_bytes(n: usize) -> String {
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
pub(crate) fn est_decoded_len(b64: &str) -> usize {
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
pub(crate) fn decode_base64(s: &str) -> Option<Vec<u8>> {
    pub(crate) fn sextet(c: u8) -> Option<u32> {
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
pub(crate) fn image_ref_from_source(source: &Value, with_data: bool) -> Option<ImageRef> {
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
pub(crate) fn parse_image_markers(text: &str, out: &mut Vec<usize>) {
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
pub(crate) fn record_images(rec: &Record, with_data: bool) -> Vec<ImageRef> {
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
pub(crate) fn images_in_file(path: &Path, with_data: bool) -> Result<(Vec<ImageRef>, usize)> {
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
