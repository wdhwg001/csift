//! Text + JSON projections.

use super::*;

/// Text listing: a session-label table (declared once), then one row per image.
pub(crate) fn render_text(selected: &[&ImageRef], transcripts: usize, skipped_lines: usize) {
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
pub(crate) fn render_json(selected: &[&ImageRef], transcripts: usize, skipped_lines: usize) {
    println!("{}", crate::text::envelope_header("image", json!({})));
    for img in selected {
        println!(
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
                "record_uuid": img.record_uuid,
                "ts_utc": img.ts_utc,
                "ts_local": img.ts_utc.as_deref().and_then(local_iso),
            })
        );
    }
    println!(
        "{}",
        crate::text::envelope_summary(
            json!({ "images": selected.len(), "transcripts": transcripts, "skipped_lines": skipped_lines })
        )
    );
}
