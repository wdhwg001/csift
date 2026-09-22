//! Text + JSON projections.
//!
//! Both go through ONE [`crate::text::Out`]: a listing is one row per image and a
//! transcript can carry thousands. See the `text::out` module docs for what holding the
//! lock obliges - the duplicate banner stays `println!` because it is already flushed
//! before the writer exists.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::io::Write as _;

use super::*;

/// Text listing: a session-label table (declared once), then one row per image.
pub(crate) fn render_text(
    selected: &[&ImageRef],
    transcripts: usize,
    skipped_lines: usize,
    duplicates: &[crate::path::DuplicateGroup],
    crossing: &crate::model::CrossingReport,
) -> Result<()> {
    crate::path::emit_duplicate_banner(duplicates);
    let mut out = crate::text::Out::stdout();
    if selected.is_empty() {
        writeln!(out, "no images found")?;
        if skipped_lines > 0 {
            writeln!(out, "({})", crate::text::malformed_note(skipped_lines))?;
        }
        out.flush()?;
        return Ok(());
    }
    // The document basename rides the FIRST row of each RECORD GROUP, not every row: five
    // pages of one PDF would repeat one name five times.
    //
    // A group is the whole coordinate of the record - TRANSCRIPT, line, document - and the
    // memo is a SET rather than the previous row, because neither half is optional. The
    // listing is sorted by `(seq, line, ordinal)` alone (`dedup_latest`), with no transcript
    // and no timestamp in the key, so rows from two sessions whose results sit at the same
    // line number ALTERNATE row by row. Without the transcript in the key, two such records
    // naming the same path print the name on the first session's rows only; without the
    // set, two naming different paths alternate keys and print it on every row.
    let mut named: std::collections::HashSet<(&str, bool, usize, &str)> =
        std::collections::HashSet::new();
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
            match img.page {
                Some(p) => format!("{} p{p} ~{}", img.media_type, human_bytes(img.est_bytes)),
                None => format!("{} ~{}", img.media_type, human_bytes(img.est_bytes)),
            }
        };
        // Never the whole path in the text listing (JSON carries it verbatim, like every
        // other path field); the basename is what tells two documents apart on one line.
        let doc = match img.page.and(img.pdf_path.as_deref()) {
            Some(path)
                if named.insert((img.session_id.as_str(), img.is_subagent, img.line_no, path)) =>
            {
                format!("  pdf {}", basename(path))
            }
            _ => String::new(),
        };
        // Lead with the session's `#N` handle (what the model says), then the exact locator.
        let label = match img.seq {
            Some(_) => format!("{:<5} {}", img.handle(), img.id()),
            None => img.id(),
        };
        // The survival marker is the ONLY difference an off-chain image gets: it is listed
        // and extractable like any other, because the bytes are on disk.
        let survival = match img.survival {
            "live" => String::new(),
            other => format!("  [{other}]"),
        };
        writeln!(
            out,
            "{}  {}  {}{}{}{}",
            label,
            size,
            format_timestamp(img.ts_utc.as_deref()),
            doc,
            survival,
            tag
        )?;
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
    writeln!(out, "{tail}")?;
    // Each row's survival marker is read off the chain, so how far that chain climbed is
    // part of what the marker means.
    for line in crossing.lines() {
        writeln!(out, "{line}")?;
    }
    out.flush()?;
    Ok(())
}

/// JSON listing: one object per image, then a trailing summary object.
pub(crate) fn render_json(
    selected: &[&ImageRef],
    transcripts: usize,
    skipped_lines: usize,
    duplicates: &[crate::path::DuplicateGroup],
    crossing: &crate::model::CrossingReport,
) -> Result<()> {
    let mut out = crate::text::Out::stdout();
    writeln!(
        out,
        "{}",
        crate::text::envelope_header("image", crate::path::duplicate_header_fields(duplicates))
    )?;
    for img in selected {
        writeln!(
            out,
            "{}",
            json!({
                "kind": "image",
                "handle": img.handle(),
                "seq": img.seq,
                "id": img.id(),
                "line": img.line_no,
                "img_index": img.img_index,
                "session_id": img.session_id,
                "is_subagent": img.is_subagent,
                "parent_session_id": img.parent_session_id,
                "source_kind": img.source_kind,
                "media_type": img.media_type,
                "b64_len": img.b64_len,
                "est_bytes": img.est_bytes,
                "url": img.url,
                "page": img.page,
                "pdf_path": img.pdf_path,
                "record_uuid": img.record_uuid,
                "survival": img.survival,
                "ts_utc": img.ts_utc,
                "ts_local": img.ts_utc.as_deref().and_then(local_iso),
            })
        )?;
    }
    let mut summary_fields = json!({ "images": selected.len(), "transcripts": transcripts, "skipped_lines": skipped_lines });
    for (k, v) in crossing.json_fields() {
        summary_fields[k] = v;
    }
    writeln!(out, "{}", crate::text::envelope_summary(summary_fields))?;
    out.flush()?;
    Ok(())
}
