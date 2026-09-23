//! The two closing disclosures of a text search: what the survival axis took out of turn
//! numbering, and what a truncated excerpt is not.
//!
//! Split out of `render.rs` when the 600-line cap bound. Each is its own function so its
//! wording lives in ONE place - both are read by a consumer as a contract statement, and a
//! second copy would drift.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use super::super::*;

/// The SURVIVAL AXIS disclosure: what the scan hid from turn numbering and from the bare
/// role selectors, and how to reach it. Never silent - the same law the draft disclosure
/// has always followed - and printed only when there is something to say, so an ordinary
/// transcript's footer is unchanged.
pub(crate) fn write_chain_disclosure<W: std::io::Write>(
    w: &mut W,
    c: &ChainCounts,
) -> std::io::Result<()> {
    if c.drafts > 0 {
        writeln!(
            w,
            "({} superseded draft(s) outside turn numbering — esc-edit resends; searchable \
             as -t user.unsent, addressable via csift show --line/--uuid)",
            c.drafts
        )?;
    }
    if c.rewound_turns > 0 {
        writeln!(
            w,
            "({} rewound turn(s) outside turn numbering — answered, then rewound past; \
             searchable as -t user.rewound)",
            c.rewound_turns
        )?;
    }
    if c.abandoned_records > 0 {
        writeln!(
            w,
            "({} record(s) off the surviving conversation — Claude Code's parentUuid chain \
             no longer reaches them, so a bare -t user/agent/harness skips them; marked \
             [abandoned]/[rewound] and always addressable)",
            c.abandoned_records
        )?;
    }
    if c.replay_copies > 0 {
        writeln!(
            w,
            "({} replay copy line(s) — a compaction re-anchor re-appended these records; the \
             LAST copy is the survivor and the earlier line is marked)",
            c.replay_copies
        )?;
    }
    if let Some(line) = c.boundary_cut_line {
        writeln!(
            w,
            "(the surviving conversation is cut at the compaction boundary L{line}; csift \
             keeps reading above it — Claude Code's own loader stops there)"
        )?;
    }
    for line in c.crossing.lines() {
        writeln!(w, "{line}")?;
    }
    if let Some(src) = c.leaf_source.filter(|_| c.any()) {
        writeln!(w, "(chain leaf: {src})")?;
    }
    Ok(())
}

/// The trailing reader-caution printed when >=1 excerpt was truncated: what the excerpts ARE
/// (clipped fragments, not summaries), why that matters (a fragment can misrepresent the whole),
/// and the exact flags to read the full text. Kept as its own fn so the wording lives in one
/// place (text only - JSON callers read the `excerpts_truncated` summary flag instead).
pub(crate) fn write_truncation_caution<W: std::io::Write>(w: &mut W) -> std::io::Result<()> {
    writeln!(w)?;
    writeln!(
        w,
        "note: matches above are TRUNCATED, match-centered FRAGMENTS — not summaries. A fragment \
         can read very differently from the record's full intent, so do NOT draw conclusions from \
         it alone."
    )?;
    writeln!(w, "  whole records: re-run with --no-truncate")?;
    writeln!(
        w,
        "  one record in full: csift show <@session|@agent-id> --line <N> (the L<n> shown on \
         a row) or --uuid <U>"
    )
}
