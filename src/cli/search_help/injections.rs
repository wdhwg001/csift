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
//! The section's FIRST LINE names every selector whose records it explains, not only the
//! four categories the family is named for: a reader who selects `user.message` or
//! `agent.communication.inbox` and lands on a relayed record has to be able to find the
//! paragraph that says so. The list sits in the body rather than in the head, because a
//! head is one column-0 line and the list does not fit one.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and closes
//! without one: `"\n\n"`, the head line at column zero, `"\n  "`, the body at indent
//! two. The regions come AFTER the JSON schema block and its closing notes: a head
//! indented under that block would read as a subsection of the JSON schema rather than
//! as a section of the page.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str = "\n\nINJECTION LEAVES (isMeta templates and the relays beside them)\n  \
     This section covers harness.enforce, harness.hook.feedback, harness.command.caveat,\n  \
     harness.context, harness.schedule, harness.resume, harness.notification, and the\n  \
     isMeta user.message and agent.communication.inbox.\n\n  \
     The harness writes text into a turn as a user record, shaped exactly like the\n  \
     operator's own message. Each template matches only at CONTENT START, after leading\n  \
     whitespace is trimmed, and only on a record carrying an AUTHORSHIP STAMP, either\n  \
     `isMeta` or `turnCompanion` on a record the harness wrote to accompany a turn. A person\n  \
     writes neither, so a person who QUOTES a template keeps `user.message`.\n    \
     .hook.feedback    answers to all four names one formatter writes (Stop, TeammateIdle,\n                      \
     TaskCreated, TaskCompleted)\n    \
     .enforce.retry    the harness CONTINUING a turn the model did not finish. It matches\n                      \
     NINE whole messages by exact equality.\n                        \
     the four redo nudges\n                        \
     the two classifier-stop forms, opening\n                          \
     `Your response above was stopped by a safety classifier`\n                        \
     the output-limit resume, opening\n                          \
     `Output token limit hit. Resume directly`\n                        \
     the two cut-off forms, opening\n                          \
     `Your response above was cut off mid-stream`\n                      \
     It also matches the edit-failed notice at content start,\n                        \
     `The last Edit failed because the target file was modified`\n                      \
     Five of the nine texts carry an em dash, so a text retyped with an\n                      \
     ASCII hyphen matches no record.\n    \
     .command.caveat   the slash-command executor's `<local-command-caveat>` wrapper, never\n                      \
     a hook's\n    \
     .context.skill    answers to TWO heads. `Base directory for this skill:` is what the\n                      \
     registrar prepends, only for a skill that ships FILES.\n                      \
     `# Workflow authoring reference` heads the one bundled skill body\n                      \
     with no files that the leaf recognises. Other file-less bodies\n                      \
     share no head and carry no leaf.\n\n  \
     THREE records here are not the harness talking to itself.\n    \
     relayed message     A person's message the harness relayed into a running lane\n                        \
     (`origin.kind` `human`) is `user.message` with the marker\n                        \
     `[relayed]` (JSON `relayed`). It opens NO turn, so it is\n                        \
     searchable without moving the turn numbering.\n    \
     coordinator relay   `agent.communication.inbox`. Its `origin` carries only its kind.\n    \
     peer relay          `agent.communication.inbox` as well. Its `origin` names the lane\n                        \
     that sent it, as `[peer lane <id>]` (JSON `peer_sender_task_id`).\n                        \
     A peer relay DOES open a turn, like any other delivered message.\n  \
     The two inbox relays are told apart by that same `origin` field.\n\n  \
     A `harness.context.image` record whose content is image blocks and no text renders\n  \
     `[context image-only: N image block(s), M document block(s)]` (JSON `image_only`),\n  \
     because it has no text of its own to show. The same shape WITHOUT `isMeta` is a\n  \
     person's record and carries no leaf at all.\n\n  \
     Most of these units carry one `for` edge (JSON `for`, an array of\n  \
     `{kind, basis, target:{line,uuid,session,note}, reason}`).\n    \
     redoes        the assistant turn an enforce nudge sends the model back to\n    \
     blocked       the turn a hook's feedback stopped\n    \
     fires         the scheduler event a fired prompt came from\n    \
     reports       the `<tool-use-id>` a notification frame's pulse names\n    \
     attached_to   the record everything else was injected under\n  \
     `basis` is `position` for the four read off the parent link and `key` for `reports`.\n  \
     The three answers stay distinct, and an edge is never silently absent.\n    \
     [for: <kind> L<n>]                      a target this transcript carries\n    \
     [for: <kind> <id> (not in this file)]   a target it does not, keeping the id\n    \
     [for: <kind> unknown (<reason>)]        a record that names no such key\n  \
     `harness.schedule.wakeup` and `harness.schedule.tick` carry the `fires` edge exactly\n  \
     when a fire record is their parent. Otherwise they carry NO edge, like\n  \
     `harness.resume.prompt`, because no key on those records names one.\n\n  \
     A record matching no template carries no leaf, whichever stamp it bears.\n  \
     `show --line N` renders it as `? unmodeled: user/isMeta` or\n  \
     `? unmodeled: user/turnCompanion`, and a scan never emits it. The AUTHORSHIP names the\n  \
     residue ahead of the content shape, so a companion whose content is a text-block\n  \
     array is named by its stamp and not by the block. csift's test for the person's own\n  \
     message reads `turnCompanion` too, so a record carrying it is never the person and\n  \
     never opens a turn, whatever `isMeta` says.";
