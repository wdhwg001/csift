//! ONE exchange's text block - its header, its hit rows, its `--siblings` context rows -
//! plus the writers each row is made of.
//!
//! Split out of `render.rs` (which keeps what a row SAYS about itself - the glyph, the
//! decorated label, the header token) when the 600-line cap bound. `show` renders the same
//! record line, so it lives here once rather than twice.
//!
//! [`write_exchange`] is the UNIT the ordered-parallel pipeline renders: it reads the
//! exchange, the header-token map and one precomputed decision, and touches nothing else -
//! which is what lets several exchanges render at once and still reach stdout in file
//! order.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use super::super::*;

/// WHERE the one image-extraction hint goes.
///
/// The hint is printed ONCE per run, under the first row whose record carries image ids -
/// a cross-row decision, and the only one the text render makes. A serial loop carried it
/// in a `bool` it flipped as it went; the parallel render cannot, because the chunk holding
/// the first such row does not know what earlier chunks found. So the decision is made
/// ONCE, up front, by [`first_image_row`], and the renderer of that one exchange is handed
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageHint {
    /// The exchange's index in the emitted stream.
    pub(crate) exchange: usize,
    /// True when the row is a `--siblings` context row rather than a hit - the two lists
    /// are searched in that order, so an index alone would be ambiguous.
    pub(crate) sibling: bool,
    /// The row's index within its own list.
    pub(crate) row: usize,
}

/// The first row of the emitted stream whose record carries image ids, searching hits then
/// siblings within each exchange - exactly the order the rows are printed in. `None` when
/// no emitted record carries an image, which is when no hint is printed at all.
pub(crate) fn first_image_row(exchanges: &[Exchange]) -> Option<ImageHint> {
    for (i, ex) in exchanges.iter().enumerate() {
        if let Some(row) = ex.hits.iter().position(|h| !h.image_ids.is_empty()) {
            return Some(ImageHint {
                exchange: i,
                sibling: false,
                row,
            });
        }
        if let Some(row) = ex.siblings.iter().position(|h| !h.image_ids.is_empty()) {
            return Some(ImageHint {
                exchange: i,
                sibling: true,
                row,
            });
        }
    }
    None
}

/// Write ONE exchange's block: the blank separator, the self-resolving header, the draft
/// distance line, every hit row, the `--siblings` context rows, and the capped-away
/// remainder pointer. `hint` is `Some` only for the exchange that owns the run's single
/// image-extraction hint (see [`ImageHint`]).
pub(crate) fn write_exchange<W: std::io::Write>(
    w: &mut W,
    ex: &Exchange,
    tok: &HashMap<&str, String>,
    hint: Option<ImageHint>,
) -> Result<()> {
    writeln!(w)?;
    // `<tok>·t6` - the id-prefix token + 0-based turn index (the SAME numbering
    // `show --turn` addresses) + the single compact local instant (offset already pins it;
    // no second UTC copy). Per-hit timestamps are omitted in text (this turn time covers
    // them); the JSON envelope still carries each hit's `ts_utc`. A subagent exchange
    // carries its parent on EVERY header (a tail-truncated read must still resolve); the
    // parent token is the plain first-8 of the owning top-level uuid - no collision
    // machinery (the resolver's fail-loud ambiguity check is the backstop).
    let t = &tok[ex.session_id.as_str()];
    // An abandoned unit sits OUTSIDE turn numbering - name it, never fabricate a t<N>.
    let turn_tag = match ex.turn_index {
        Some(t) => format!("t{t}"),
        None => abandoned_tag(ex),
    };
    if ex.is_subagent {
        writeln!(
            w,
            "{t}·{turn_tag} (parent {})  {}",
            id_prefix(&ex.parent_session_id, 8),
            format_local_compact(ex.started_utc.as_deref())
        )?;
    } else {
        writeln!(
            w,
            "{t}·{turn_tag}  {}",
            format_local_compact(ex.started_utc.as_deref())
        )?;
    }
    // What the user changed before sending. Printed once per draft unit,
    // under the header, so the reader weighs the draft against what replaced it
    // instead of reading an abandoned text as the message.
    if let Some(d) = &ex.draft_diff {
        writeln!(w, "      ↳ {}", d.text_line_for(ex.abandoned_kind))?;
    }
    for (i, hit) in ex.hits.iter().enumerate() {
        write_record_line(w, role_glyph(hit.class), hit)?;
        // Close the text-mode ID-law hole for SUBAGENT hits: a line number is per-FILE, so
        // it must be fetched with the SUBAGENT's own id (`session_id`), NEVER the parent
        // uuid (that would silently fetch the WRONG record). Top-level hits are safe (the
        // header token IS an `@`-targetable prefix of the fetch id), so only the hazard
        // rows carry the explicit ready-to-run command (with the FULL id - zero ambiguity
        // risk) - the same address JSON's per-hit `refetch` gives.
        if ex.is_subagent && !hit.from_sidecar && hit.line > 0 {
            writeln!(
                w,
                "      ↳ csift show @{} --line {}",
                ex.session_id, hit.line
            )?;
        }
        write_image_hint(w, ex, hint, false, i)?;
    }
    // `--siblings`: the turn's non-matched records, under a dim `·` context marker so
    // they read as surrounding back-and-forth, not as matches.
    for (i, sib) in ex.siblings.iter().enumerate() {
        write_record_line(w, '·', sib)?;
        write_image_hint(w, ex, hint, true, i)?;
    }
    // The FIXED policy's capped-away remainder - explicit, with the exact fetch
    // command (self-healing escape hatch; `@<session_id>` round-trips for both a
    // top-level uuid and a subagent id).
    if ex.siblings_hidden > 0 && ex.turn_lines.0 > 0 {
        writeln!(
            w,
            "  · (+{} more · csift show @{} --line {}..{})",
            ex.siblings_hidden, ex.session_id, ex.turn_lines.0, ex.turn_lines.1
        )?;
    }
    Ok(())
}

/// Emit the run's single image-extraction hint when THIS row is the one that owns it.
/// The ids are read off the row rather than off the hint, so the printed command is the
/// row's own.
fn write_image_hint<W: std::io::Write>(
    w: &mut W,
    ex: &Exchange,
    hint: Option<ImageHint>,
    sibling: bool,
    row: usize,
) -> Result<()> {
    let Some(h) = hint.filter(|h| h.sibling == sibling && h.row == row) else {
        return Ok(());
    };
    let ids = if h.sibling {
        ex.siblings.get(h.row).map(|s| &s.image_ids)
    } else {
        ex.hits.get(h.row).map(|s| &s.image_ids)
    };
    if let Some(l) = ids.and_then(|ids| image_hint_line(&ex.session_id, ids)) {
        writeln!(w, "{l}")?;
    }
    Ok(())
}

/// One hit/sibling line: `<marker> <label>[ <tool>]  L<line>  <excerpt>` (excerpt inline; its
/// newlines are already collapsed to single spaces). `marker` is the role glyph for a match or a
/// dim `·` for a `--siblings` context record; `<label>` is the dotted path with its
/// `⇨`/`▹` decorations ([`render_label`]).
pub(crate) fn write_record_line<W: std::io::Write>(
    w: &mut W,
    marker: char,
    h: &Hit,
) -> std::io::Result<()> {
    let label = render_label(h);
    let name = h
        .tool_name
        .as_deref()
        .map(|n| format!(" {n}"))
        .unwrap_or_default();
    let images = image_suffix(&h.image_ids);
    // A merged elicitation-sidecar hit has no physical jsonl line - render the provenance
    // locator instead of a fabricated `Lnnnn` (section 3.10).
    let locator = if h.from_sidecar {
        "(elicitation sidecar)".to_string()
    } else {
        format!("L{}", h.line)
    };
    writeln!(
        w,
        "  {marker} {label}{name}  {locator}  {}{}",
        h.excerpt, images
    )
}

/// ` [N image(s): ...]` suffix when the hit's record carries images - the SAME ids `turns` shows,
/// feedable straight to `csift image <session> --id <ID>`. Empty string when there are none.
pub(crate) fn image_suffix(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let noun = if ids.len() == 1 { "image" } else { "images" };
    format!("  [{} {}: {}]", ids.len(), noun, ids.join(", "))
}

/// The paste-ready extraction command for a row's images, printed ONCE per run under the
/// first image-bearing row: the inline `[N image(s): #7]` annotation alone says nothing
/// about extraction, and a reader who does not know the command takes images for
/// unreadable without ever trying.
/// INPUT forms only: `#7` is display, `--id` takes the bare number; `L<line>i<n>` is
/// identical in both. Addressed at the row's OWNING transcript id (the per-FILE ID law).
pub(crate) fn image_hint_line(session_id: &str, ids: &[String]) -> Option<String> {
    let first = ids.first()?;
    let input = first.strip_prefix('#').unwrap_or(first);
    Some(format!(
        "      ↳ read the image(s): csift image @{session_id} --id {input} --out DIR           (decodes to a file you can then Read - an image-bearing turn is evidence, not a blank)"
    ))
}

/// True when any hit or sibling row carries image ids - gates the capability footer note.
pub(crate) fn any_image_ids(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| !h.image_ids.is_empty())
    })
}
