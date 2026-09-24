//! The `search --help` section for the INJECTIONS family: the isMeta templates -
//! `harness.enforce.*`, `harness.command.caveat`, `harness.hook.feedback` and the isMeta
//! forms of `harness.context.{reminder,image,skill}` - the records the harness wrote into
//! a turn wearing a user record's clothes, and the three that are a PERSON or another
//! SESSION underneath.
//!
//! Information parity with SKILL is the contract (AGENTS section 1): every marker word,
//! JSON key and render form the family ships is named here too, in the same commit
//! series as the surface it documents.
//!
//! The HEAD names every selector whose records this section explains, not only the
//! four categories the family is named for: a reader who selects `user.message` or
//! `agent.communication.inbox` and lands on a relayed record has to be able to find the
//! paragraph that says so.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and closes
//! without one: `"\n\n"`, the head line at column zero, `"\n  "`, the body at indent
//! two. The regions come AFTER the JSON schema block and its closing notes: a head
//! indented under that block would read as a subsection of the JSON schema rather than
//! as a section of the page.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str = "\n\nINJECTION LEAVES (harness.enforce, harness.hook.feedback, harness.command.caveat, harness.context, harness.schedule, harness.resume, harness.notification, and the isMeta user.message / agent.communication.inbox)\n  \
     Text the harness writes into a turn as an `isMeta` user record, shaped exactly like \
     the operator's own message. Every arm matches at CONTENT START after one left trim \
     and every arm needs the `isMeta` flag, so a person who QUOTES a template keeps \
     `user.message`. `.hook.feedback` answers to all four names one formatter writes \
     (Stop, TeammateIdle, TaskCreated, TaskCompleted); `.enforce.retry` is the harness \
     CONTINUING a turn the model did not finish, over NINE whole messages matched by \
     exact equality - the four redo nudges, the two classifier-stop forms opening `Your \
     response above was stopped by a safety classifier`, the output-limit resume opening \
     `Output token limit hit. Resume directly`, and the two cut-off forms opening `Your \
     response above was cut off mid-stream` - plus the edit-failed notice `The last Edit \
     failed because the target file was modified` at content start; five of the nine \
     texts carry an em dash, so a text retyped with an ASCII hyphen matches no record; `.command.caveat` is the slash-command \
     executor's `<local-command-caveat>` wrapper, not a hook's. THREE records here are \
     not the harness talking to itself. A person's message the harness relayed into a \
     running lane (`origin.kind` `human`) is `user.message` with the marker `[relayed]` \
     (JSON `relayed`) and opens NO turn, so it is searchable without moving the turn \
     numbering. The other two are `agent.communication.inbox` and are told apart by the \
     same field: a COORDINATOR relay's `origin` carries only its kind, while a PEER \
     relay names the lane that sent it - `[peer lane <id>]`, JSON `peer_sender_task_id` - \
     and a peer relay DOES open a turn, like any other delivered message. An \
     `harness.context.image` record whose content is image blocks and no text renders \
     `[context image-only: N image block(s), M document block(s)]` (JSON `image_only`), \
     because it has no text of its own to show; the same shape WITHOUT `isMeta` is a \
     person's record and carries no leaf at all. Most units of the family carry one `for` \
     edge (JSON `for`, an array of `{kind, basis, target:{line,uuid,session,note}, \
     reason}`): `redoes` the assistant turn an enforce nudge sends the model back to, \
     `blocked` the turn a hook's feedback stopped, `fires` the scheduler event a fired \
     prompt came from, `reports` the `<tool-use-id>` a notification frame's pulse names, \
     and `attached_to` the record everything else was injected under. `basis` is \
     `position` for the five read off the parent link and `key` for `reports`. The three \
     answers stay distinct: a target this transcript carries renders `[for: <kind> L<n>]`, \
     one it does not keeps the id as `[for: <kind> <id> (not in this file)]`, and a record \
     that names no such key renders `[for: <kind> unknown (<reason>)]` - never a silently \
     absent edge. `harness.schedule.wakeup`, `harness.schedule.tick` and \
     `harness.resume.prompt` carry NO edge, because no key on those records names one. An \
     isMeta record matching no template carries no leaf: `show --line N` renders it as \
     `? unmodeled: user/isMeta` and a scan never emits it.";
