//! The Bash lexical layers: what csift can read off a shell command string alone.
//!
//! Two independent families live here, both purely lexical and both fed by a
//! `Bash` tool_use record's `input.command`:
//!
//! - [`danger`] predicts the dangerous-removal decision Claude Code's permission
//!   path would make, so a frozen lane can be called `escalation-blocked` instead
//!   of merely pending.
//! - [`mutations`] infers which files a command touched, because a Bash result
//!   carries no path field the way `Write`/`Edit` do.
//!
//! Neither touches the filesystem and neither expands a variable: where a decision
//! would need the state at the time, the answer is an explicit unresolved verdict,
//! never a guess.

pub(crate) mod danger;
pub(crate) mod mutations;
