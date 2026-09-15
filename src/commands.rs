//! One module per subcommand family: the handlers `main.rs` dispatches into.
//!
//! Every `Command::Foo(args)` arm resolves to a `run_<subcommand>` in here, and
//! the variant-to-module mapping is deliberately not 1:1 - `list` lives in
//! [`session`], `verbatim` in [`turns`], and the channel four in
//! [`live::channel`]. One member carries no subcommand of its own:
//! [`elicitation`] is the sidecar merge every record-reading surface performs
//! transparently, and it sits here because that merge is a command-layer
//! concern rather than a parsing one.
//!
//! What is NOT here is the base layer these build on: `cli`, `model`, `parse`,
//! `path`, `subagent`, `bash`, `text`, `timez`, `time_window` and `chardiff`
//! stay at the crate root. The split is by LAYER, not by direction - a handful
//! of base modules do call into a command module (`cli` builds `turns` config
//! types, `path` asks `elicitation` whether a target is a sidecar, `subagent`
//! reuses `files`' mutation extractor, `model::narration` reuses `image`'s
//! base64 decoder), and each of those is a deliberate reuse of one helper
//! rather than a layer inversion.

pub(crate) mod agents;
pub(crate) mod elicitation;
pub(crate) mod files;
pub(crate) mod image;
pub(crate) mod live;
pub(crate) mod plan;
pub(crate) mod recover;
pub(crate) mod search;
pub(crate) mod session;
pub(crate) mod show;
pub(crate) mod stats;
pub(crate) mod turns;
pub(crate) mod whoami;
