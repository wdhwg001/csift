//! `csift deliver --recipe`: the settings block a user pastes to arm the channel.
//!
//! csift never installs a hook. The whole channel is thin-hook by design - N identical
//! lines `csift deliver --slot k` on the delivery events, with every guarantee living in
//! the binary rather than in what was pasted - so the one honest thing this command can do
//! is PRINT the block and say who owns installing it. The fragment goes to stdout so it
//! can be redirected into a file or piped into an editor; the note goes to stderr so the
//! redirected file stays valid JSON.
//!
//! Slots are positions in the delivery chain, not copies of one hook: slot k emits chunk k
//! of the firing's chunk list, so N slots on an event carry N chunks of context at that
//! event and a message longer than one chunk needs more than one slot to arrive whole.

use anyhow::Result;
use serde_json::{json, Value};

use super::STEER_EVENTS;

/// Which shell form the pasted command entries carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecipeShell {
    /// The default command form, run by the harness's own shell.
    Bash,
    /// The Windows form, which names the PowerShell runner explicitly.
    Powershell,
}

/// The two lines that say who installs the block. Kept beside the renderer so the note can
/// never drift away from the fragment it belongs to.
const NOTE: [&str; 2] = [
    "Paste the block above into the `hooks` object of a settings.json you own (user, \
     project or local scope); the eight events are the delivery points, and slot k emits \
     chunk k of what is waiting there.",
    "csift never writes a settings file: arming the channel is your edit, and `csift \
     deliver` only ever writes its own sidecar directory.",
];

/// Render the fragment for `slots` slots on the eight delivery events.
pub(crate) fn fragment(slots: u32, shell: RecipeShell) -> Value {
    let mut hooks = serde_json::Map::new();
    for event in STEER_EVENTS {
        let entries: Vec<Value> = (1..=slots).map(|k| entry(k, shell)).collect();
        hooks.insert(event.to_string(), json!([{ "hooks": entries }]));
    }
    json!({ "hooks": Value::Object(hooks) })
}

/// One command entry. The PowerShell form names its runner because a Windows session
/// without a Git-for-Windows bash runs the PowerShell tool instead, and a command entry
/// that assumes a POSIX shell there never starts.
fn entry(slot: u32, shell: RecipeShell) -> Value {
    match shell {
        RecipeShell::Bash => json!({
            "type": "command",
            "command": format!("csift deliver --slot {slot}"),
        }),
        RecipeShell::Powershell => json!({
            "type": "command",
            "shell": "powershell",
            "command": format!("csift deliver --slot {slot}"),
        }),
    }
}

/// Print the fragment on stdout and the two-line note on stderr. Writes nothing.
pub(crate) fn run_recipe(slots: u32, shell: RecipeShell) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&fragment(slots, shell))?);
    for line in NOTE {
        eprintln!("{line}");
    }
    Ok(())
}
