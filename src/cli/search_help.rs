//! The `search --help` after-help body, assembled from one section file per taxonomy
//! family.
//!
//! ONE string used to carry all of it, which meant every family lane editing the same
//! lines. The body is now the byte-JOIN of [`intro::INTRO`], the six family sections in
//! a fixed order, and [`outro::OUTRO`]; each family owns exactly one of those files. The
//! six ship EMPTY, so the rendered help is byte-identical to what the single const
//! carried - a test pins that, and it is what makes the split provably inert.
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

/// The sections in RENDER order. The six family sections sit between the shared body and
/// its closing notes, so a family's own sentences land inside the JSON schema discussion.
pub(crate) const SECTIONS: [&str; 8] = [
    intro::INTRO,
    bookkeeping::SECTION,
    attachments::SECTION,
    injections::SECTION,
    system::SECTION,
    thinking::SECTION,
    keys::SECTION,
    outro::OUTRO,
];

/// The `after_help` body of `csift search --help` (see the module doc).
pub(crate) static SEARCH_AFTER_HELP: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| SECTIONS.concat());
