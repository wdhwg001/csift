//! The `search --help` after-help body, assembled from one section file per taxonomy
//! family.
//!
//! ONE string used to carry all of it, which meant every family lane editing the same
//! lines. The body is now the byte-JOIN of [`intro::INTRO`], [`outro::OUTRO`] and the six
//! family sections in a fixed order; each family owns exactly one of those files.
//!
//! The SPLIT moved no rendered byte - the six shipped empty and both helps were rendered
//! before and after and compared - but that is a fact about the commit that did it, not a
//! standing property: a family filling its section is exactly what the split is FOR, and
//! the page grows by one column-0 region each time one does. So no test pins this text as
//! a literal, which would go stale the first time a lane wrote a sentence. The frame test
//! pins what the assembler actually owes: that the rendered body IS the sections joined,
//! the ORDER (shared body first, family regions the tail in family order), the two seams
//! (no trailing space where the intro ends, exactly one leading space on the closing
//! notes) and the region shape (a head at column 0, a body indented two, and an EMPTY
//! section rendering nothing at all).
//!
//! A `LazyLock<String>` rather than a const: Rust concatenates string LITERALS at compile
//! time and named consts not at all, so the join happens once on first use of the help.

mod attachments;
mod bookkeeping;
mod injections;
mod intro;
mod keys;
mod outro;
mod system;
mod thinking;

/// The six family sections in the taxonomy's family order - the TAIL of the body, one
/// column-0 region each.
///
/// A family's sentences are a region of the page, not a subsection of the block that
/// happens to be last. They used to land between the JSON schema paragraph and its two
/// closing notes, which put a TEXT render law under the head `JSON SCHEMA (per --format
/// json)` at an indent that read as a subsection of it. Every other head on this page sits
/// at column 0 with its body indented two, and these follow that.
///
/// SHAPE a family writes into its own `SECTION`: `"\n\n"`, then its head line at column
/// zero, then `"\n  "` and the body. An EMPTY section renders NOTHING - no head, no blank
/// line - which is what lets the six ship empty and fill in one at a time.
pub(crate) const FAMILY_SECTIONS: [&str; 6] = [
    bookkeeping::SECTION,
    attachments::SECTION,
    injections::SECTION,
    system::SECTION,
    thinking::SECTION,
    keys::SECTION,
];

/// The SHARED body: the page every family region follows, its own closing notes included.
/// The notes sit directly after the intro, so the JSON schema block reads exactly as it
/// did when one string carried the whole page.
pub(crate) const SHARED: [&str; 2] = [intro::INTRO, outro::OUTRO];

/// The sections in RENDER order: the shared body, then the six family regions.
pub(crate) fn sections() -> Vec<&'static str> {
    [SHARED.as_slice(), FAMILY_SECTIONS.as_slice()].concat()
}

/// The `after_help` body of `csift search --help` (see the module doc).
pub(crate) static SEARCH_AFTER_HELP: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| sections().concat());
