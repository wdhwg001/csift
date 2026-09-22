//! The writers BELOW an exchange header: one record line, and the image annotations that
//! ride it.
//!
//! Split out of `render.rs` (which keeps what a row SAYS about itself - the glyph, the
//! decorated label, the header token) when the 600-line cap bound. `show` renders the same
//! record line, so it lives here once rather than twice.

use super::super::*;

/// One hit/sibling line: `<marker> <label>[ <tool>]  L<line>  <excerpt>` (excerpt inline; its
/// newlines are already collapsed to single spaces). `marker` is the role glyph for a match or a
/// dim `·` for a `--siblings` context record; `<label>` is the dotted path with the GOLD §4/§7
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
    // locator instead of a fabricated `Lnnnn` (§3.10).
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

/// ` [N image(s): …]` suffix when the hit's record carries images - the SAME ids `turns` shows,
/// feedable straight to `csift image <session> --id <ID>`. Empty string when there are none.
pub(crate) fn image_suffix(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let noun = if ids.len() == 1 { "image" } else { "images" };
    format!("  [{} {}: {}]", ids.len(), noun, ids.join(", "))
}

/// The paste-ready extraction command for a row's images, printed ONCE per run under the
/// first image-bearing row (C-11: the inline `[N image(s): #7]` annotation taught nothing
/// about extraction, and fleets classified images as unreadable without ever testing).
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
