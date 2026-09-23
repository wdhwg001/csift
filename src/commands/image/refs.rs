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
    /// The session-facing `[Image #N]` number - the SAME handle the model sees and refers to
    /// ("re-share #32"). Extracted by positionally zipping the record's `[Image #N]` text
    /// markers with its image blocks (CC numbers pasted images per-prompt; `history.ts`).
    /// `None` when the marker count doesn't match (then only `L<line>i<n>` addresses it).
    /// NOT globally unique - CC reuses low numbers across prompts, so a `#N` that names >1
    /// DISTINCT image is AMBIGUOUS: `--id #N` then ERRORS with the occurrence list rather than
    /// silently guessing (disambiguate with the locator or `--since`/`--turn`/`--uuid`).
    pub(crate) seq: Option<usize>,
    /// A cheap content fingerprint (`<len>:<head>:<tail>` of the base64) - dedups the SAME
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
    /// The DOCUMENT page number this image is, when the carrying record says so: a `Read`
    /// with a `pages` parameter on a `.pdf` returns one image element per page and the
    /// page number lives nowhere on the element, only in the record's own
    /// `toolUseResult.firstPage` plus the element's position ([`record_pdf_pages`]).
    /// `None` for every other image - a screenshot, a pasted picture, a page from a
    /// carrier that names no `firstPage` - because an unknown page is never an assumed 1.
    pub(crate) page: Option<usize>,
    /// The PDF the page came from (`toolUseResult.file.filePath`, verbatim). Set only
    /// beside a `page`, so the pair is never half an answer.
    pub(crate) pdf_path: Option<String>,
    pub(crate) ts_utc: Option<String>,
    pub(crate) record_uuid: Option<String>,
    /// The wire spelling of the carrying record's survival (`live` / `pre-cut` /
    /// `abandoned`). An image on an ABANDONED record - a prompt recalled and re-typed, a
    /// turn the operator rewound past - is still LISTED and still extractable: the bytes are
    /// on disk and `--out` writes the same file either way. It is only MARKED, so a reader
    /// knows the image is not part of the conversation the model now sees.
    pub(crate) survival: &'static str,
    /// The base64 payload - populated ONLY in extract mode (`--out`), to bound memory in the
    /// common list path.
    pub(crate) data: Option<String>,
}

impl ImageRef {
    /// The exact per-occurrence locator `L<line>i<n>` (always unambiguous).
    pub(crate) fn id(&self) -> String {
        format!("L{}i{}", self.line_no, self.img_index)
    }

    /// The session-facing handle `#N` when known, else the locator - so output leads with the
    /// number the model already uses ("re-share #32" → `--id #32`).
    pub(crate) fn handle(&self) -> String {
        match self.seq {
            Some(n) => format!("#{n}"),
            None => self.id(),
        }
    }

    /// File extension implied by the media type (`bin` when unknown - never fabricated).
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

    /// `<session-short>[-img<N>]-L<line>i<n>.<ext>` - carries the `#N` when known (so the file
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

/// Pre-JSON byte prefilter: a line MIGHT carry an image block. Coarse by design - the
/// structural walk (`record_images`) decides what each line really holds.
pub(crate) fn line_is_image_candidate(line: &[u8]) -> bool {
    // Finders built ONCE (per-line hot path - the stateless form rebuilt its
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
/// on an invalid character - so a malformed image is REPORTED, never silently written wrong.
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
        survival: "live",
        page: None,
        pdf_path: None,
        data,
    })
}

/// The PDF page provenance a `Read` with a `pages` parameter leaves on its result record.
///
/// The harness returns ONE `tool_result` block carrying a leading text plus one element
/// per requested page, in page order, and a SUCCESSFUL page element carries no number of
/// any kind - the only page label the writer emits is on the FAILURE branch. So the page
/// number is `firstPage + <position among the elements after the leading text>`, counted
/// over CONTENT POSITIONS and never over image ordinals: a failed page is a TEXT element
/// in the same sequence and counting images would shift every later page by one.
#[derive(Debug, Clone)]
pub(crate) struct PdfPages {
    /// `toolUseResult.firstPage` - the document page number of the FIRST extracted page.
    pub(crate) first: usize,
    /// How many pages the result carries: the content elements after the leading text.
    /// Always >= 1 ([`record_pdf_pages`] refuses a result with none).
    pub(crate) count: usize,
    /// `toolUseResult.file.filePath` - the document the pages came from, verbatim.
    pub(crate) path: Option<String>,
}

impl PdfPages {
    /// The document page of the element at CONTENT POSITION `pos` (0-based, so position 0
    /// is the leading text and carries no page). `None` outside the mapped range.
    pub(crate) fn page_at(&self, pos: usize) -> Option<usize> {
        let i = pos.checked_sub(1)?;
        (i < self.count).then_some(self.first + i)
    }

    /// The document page of the LAST element the result carries - the closing half of the
    /// range a whole-result reader (the `agent.tool.result` unit) prints.
    pub(crate) fn last(&self) -> usize {
        self.first + self.count - 1
    }
}

/// A path's basename. Split on BOTH separators: the path is whatever the harness recorded
/// and a Windows lane records `C:\...`, which a POSIX-only split would hand back whole.
/// A path that is nothing but separators has no name to take, so it renders whole rather
/// than empty.
pub(crate) fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

/// Read the PDF page provenance off ONE record, or `None` when the record does not carry
/// it - a screenshot, a pre-`firstPage` build, a carrier that re-emits page images of its
/// own (the REPL tool flattens inner-`Read` pages into its own result with no `firstPage`,
/// so position does not map to a page there and this must not fire).
///
/// The gate is the record's OWN result object: `toolUseResult.type == "parts"` with a
/// `firstPage`, which is the shape only the Read `pages` branch mints. Plus ONE structural
/// requirement - the record carries exactly one `tool_result` block, because a `parts`
/// result describes one tool call and could not say which of two blocks it belongs to.
///
/// COST, stated as what it is: `Record::tur_probe` returns on its first `?` for a record
/// with no `toolUseResult` at all, but otherwise DESERIALIZES THE WHOLE BLOB to read six
/// small fields - a typed walk that skips the unlisted values without allocating them,
/// never a walk that skips the bytes. Only once that reads `parts` does the full parse of
/// what is by then a six-key object follow. Both are paid once per record and only for a
/// record that carries a nested image, which is where the caller resolves this lazily.
pub(crate) fn record_pdf_pages(rec: &Record) -> Option<PdfPages> {
    // The TYPE half of the gate, and it does two jobs. It is what keeps the derivation off
    // a carrier that re-emits page images with an offset of its own name (IMG-018), and it
    // is the SHORT CIRCUIT: a record whose result is not `parts` never reaches the full
    // `tool_use_result_value()` parse below, which on a screenshot result would walk a
    // base64 payload to find a `firstPage` that a REPL result can also carry.
    let probe = rec.tur_probe()?;
    if probe.r#type.as_ref().and_then(Value::as_str) != Some("parts") {
        return None;
    }
    let tur = rec.tool_use_result_value()?;
    let first = usize::try_from(tur.get("firstPage")?.as_u64()?).ok()?;
    let mut results = rec.blocks()?.iter().filter_map(|b| match b {
        Block::ToolResult {
            content: Some(c), ..
        } => Some(c),
        _ => None,
    });
    let content = results.next()?;
    if results.next().is_some() {
        return None;
    }
    // Element 0 is the leading text, so a result with nothing after it maps no page.
    let count = content
        .as_array()?
        .len()
        .checked_sub(1)
        .filter(|n| *n > 0)?;
    let path = tur
        .get("file")
        .and_then(|f| f.get("filePath"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(PdfPages { first, count, path })
}

/// One `{type:"image"}` element of a tool_result content array as an `ImageRef` skeleton -
/// `None` for every other element kind (the leading text, a failed page's placeholder).
fn image_element_ref(el: &Value, with_data: bool) -> Option<ImageRef> {
    if el.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    image_ref_from_source(el.get("source")?, with_data)
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
/// `Block::ToolResult.content` array (a tool screenshot, or a PDF page). `img_index` is a
/// 1-based running ordinal across both sources, so the id `L<line>i<n>` is stable - and it
/// is deliberately NOT the page index: it counts images across the WHOLE record, while a
/// page is the element's position inside its own tool_result ([`record_pdf_pages`]).
pub(crate) fn record_images(rec: &Record, with_data: bool) -> Vec<ImageRef> {
    let mut out = Vec::new();
    let Some(blocks) = rec.blocks() else {
        return queued_prompt_images(rec, with_data);
    };
    // `[Image #N]` markers across the record's text blocks, in document order.
    let mut markers: Vec<usize> = Vec::new();
    for block in blocks {
        if let Block::Text { text } = block {
            parse_image_markers(text, &mut markers);
        }
    }
    // The PDF page fact, resolved at most ONCE per record and only once a nested image is
    // actually in hand: `record_pdf_pages` probes `toolUseResult`, and a record with no
    // nested image carries no page however its result object is shaped.
    let mut pdf: Option<Option<PdfPages>> = None;
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
                let Some(arr) = content.as_array() else {
                    continue;
                };
                for (pos, el) in arr.iter().enumerate() {
                    let Some(mut r) = image_element_ref(el, with_data) else {
                        continue;
                    };
                    if let Some(pages) = pdf.get_or_insert_with(|| record_pdf_pages(rec)).as_ref() {
                        // The page and the document travel together: naming the PDF for an
                        // element the mapping does not reach would be a document with no
                        // page, which says less than saying nothing.
                        r.page = pages.page_at(pos);
                        r.pdf_path = r.page.and_then(|_| pages.path.clone());
                    }
                    r.img_index = out.len() + 1;
                    out.push(r);
                }
            }
            _ => {}
        }
    }
    // Assign `#N` by NUMBER. The record's `imagePasteIds` lists the ids in the order of
    // its image blocks (the submit path builds both arrays in one ascending-id pass),
    // while the `[Image #N]` markers keep the operator's TEXT order - the two orders
    // diverge on 18 of 662 corpus records, so a positional marker-to-block zip gave a
    // silently wrong `#N` there (ledger IMG-005). Without the array (older
    // records) the positional zip stays, and only when the marker count matches the
    // image count (a mismatch means a back-reference to a compressed-out image, so
    // `seq` stays `None` rather than misassign).
    assign_image_seq(&mut out, rec.image_paste_ids.as_deref(), &markers);
    out
}

/// The `#N` join: by the record's `imagePasteIds` (block order) when it is present and
/// covers every image block, else the positional marker zip under the count guard.
fn assign_image_seq(out: &mut [ImageRef], ids: Option<&[u64]>, markers: &[usize]) {
    if let Some(ids) = ids.filter(|ids| ids.len() == out.len()) {
        for (r, &n) in out.iter_mut().zip(ids.iter()) {
            r.seq = usize::try_from(n).ok();
        }
        return;
    }
    if markers.len() == out.len() {
        for (r, &n) in out.iter_mut().zip(markers.iter()) {
            r.seq = Some(n);
        }
    }
}

/// The third carrier (ledger IMG-002): a `queued_command` attachment record
/// keeps the queued prompt's blocks under `attachment.prompt[]`, image blocks included.
/// A prompt edited or recalled before dispatch never becomes a user record, so its
/// pasted image exists ONLY here (45 of 52 such blocks in the reference corpus had no
/// copy in any message block of their transcript). Same document-order walk and the
/// same positional `[Image #N]` zip as a user record.
fn queued_prompt_images(rec: &Record, with_data: bool) -> Vec<ImageRef> {
    let mut out = Vec::new();
    if rec.attachment_type().as_deref() != Some("queued_command") {
        return out;
    }
    let Some(att) = rec.attachment_value() else {
        return out;
    };
    let Some(prompt) = att.get("prompt").and_then(Value::as_array) else {
        return out;
    };
    let mut markers: Vec<usize> = Vec::new();
    for el in prompt {
        if el.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(text) = el.get("text").and_then(Value::as_str) {
                parse_image_markers(text, &mut markers);
            }
        }
    }
    for el in prompt {
        if el.get("type").and_then(Value::as_str) == Some("image") {
            if let Some(src) = el.get("source") {
                if let Some(mut r) = image_ref_from_source(src, with_data) {
                    r.img_index = out.len() + 1;
                    out.push(r);
                }
            }
        }
    }
    // A queued prompt carries its ids beside the blocks when the attachment has them;
    // the same by-number join applies, with the positional zip as the fallback.
    let ids: Option<Vec<u64>> = att
        .get("imagePasteIds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect());
    assign_image_seq(&mut out, ids.as_deref(), &markers);
    out
}

/// The image HANDLES a record carries - `#N` (the session's own image number) when known,
/// else the `L<line>i<n>` locator - for surfacing in `turns`. Both feed straight back into
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
pub(crate) fn images_in_file(
    path: &Path,
    with_data: bool,
    cross: CrossingBudget,
) -> Result<(Vec<ImageRef>, usize, crate::model::CrossingReport)> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok((Vec::new(), 0, crate::model::CrossingReport::new(cross)));
    };
    // The image prefilter keeps only image-bearing lines, so the SURVIVAL AXIS is fed the
    // structural spine of everything else in the same pass. Nothing here needs turn
    // numbering - what it needs is the pure DAG fact "does the surviving conversation still
    // reach this record", which the spine alone answers.
    let (records, spine, skipped) =
        crate::parse::parse_candidates_with_spine(&mmap, line_is_image_candidate);
    let view = ChainView::build(&records, &spine, cross);

    let mut out = Vec::new();
    for (i, (line_no, rec)) in records.iter().enumerate() {
        for mut r in record_images(rec, with_data) {
            r.session_id = session_id.clone();
            r.is_subagent = is_subagent;
            r.parent_session_id = parent_session_id.clone();
            r.line_no = *line_no;
            r.ts_utc = rec.timestamp.clone();
            r.record_uuid = rec.uuid.clone();
            r.survival = view.survival(i).as_str();
            out.push(r);
        }
    }
    // Stable order: by line, then image ordinal.
    out.sort_by(|a, b| (a.line_no, a.img_index).cmp(&(b.line_no, b.img_index)));
    Ok((out, skipped, view.crossing().clone()))
}
