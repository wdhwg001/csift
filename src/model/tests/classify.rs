//! The classify engine's own cases, mirroring `model/classify/`'s children.
//!
//! The sibling `classify_*.rs` files beside this one cover the engine from the
//! OUTSIDE - the label a whole record carries, the comm view, the schedule arms -
//! while these four each pin one child of the engine.

use super::*;

mod channel;
mod promoted;
mod resume;
mod support;
