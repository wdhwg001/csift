# Changelog

All notable changes to csift are documented in this file, newest first — one
entry per released version, written in that version's release commit. Pre-1.0
SemVer: a BREAKING surface change bumps the MINOR version; a non-breaking
surface change bumps the PATCH.

## [0.12.2] - unreleased

### Added

- **`harness.schedule.fire`, the prompt a scheduled task fires.** When a cron entry or a
  `ScheduleWakeup` timer comes due, Claude Code submits the armed text as its own `isMeta`
  record stamped `promptSource:"system"`. The text reaches that record verbatim - the fire path
  applies exactly one rewrite, the autonomous-loop sentinel resolution, and every other prompt
  falls through it unchanged - so there was no marker to match and the record fell into the
  isMeta exclusion: unsearchable on a flagless scan, absent from every census. The leaf keys on
  the record's own fields instead, and a `-t harness.schedule.fire` now enumerates a session's
  scheduled work. The label zone names the instant the fire happened, read off the
  `system`/`scheduled_task_fire` record the prompt is parented to (`[scheduled fire <when>]`,
  JSON `scheduled_at`); a transcript that holds no such record reports null rather than a
  guessed time. A tick that DOES carry a loop marker keeps `harness.schedule.wakeup` or
  `harness.meta.loop` by arm order, and an inbound peer message, which carries the identical
  stamp, is refused by its relay framing and its `origin` object. `Class::ALL` grows to 38
  leaves; every search hit's JSON gains a `scheduled_at` field.
- **`list` joins a cleared session to the transcript it was cleared from.** A `/clear`
  mints a new session id inside the same process and writes no lineage anywhere on disk,
  so the row infers the predecessor and says so. A transcript is `minted_by: "clear"` when
  its first non-isMeta user record is the `/clear` slash wrapper, which lands in the file
  the command created rather than the one it ended; the predecessor is the sibling in the
  same project directory whose cost-ledger checkpoint (`startTime + totalDuration` on a
  `cost-state` line) closes within 2000 ms of that wrapper. The nearest line wins, and a
  tie between two different files is reported and joined to neither. New JSON fields
  `minted_by`, `cleared_from`, `cleared_from_distance_ms` and `cleared_from_candidates`,
  and one `cleared` text row; never a file mtime, never the time adjacency of ordinary
  records.
- **`status` and `wait` report a cost-ledger checkpoint at the tail as evidence.** When a
  transcript's last line is a `cost-state` line, a `checkpoint` evidence row names it and
  JSON carries `last_checkpoint: {line, kind}`. No verdict was added: the harness writes a
  checkpoint at a clear, a background handover, an in-app resume and at exit, most of them
  mid-file, so a tail one says only that nothing was appended after it, and the registry
  row keeps deciding liveness. With no registry row and a tail checkpoint, that note gains
  a clause saying the session closed or was handed over.
- **`status` finds the task store a clear left behind.** The store is named after the id
  the process started with, so the session's own id is now the first candidate rather than
  the only one: then the root of the `cleared_from` chain, then the team file written
  within five seconds of the registry row's `startedAt`. A directory matching no candidate
  is never read, and every store that answered prints as `tasks store: <dir> (via
  <candidate>)`, with the same pairs in JSON `tasks_stores`; an empty store prints too,
  marked `- empty`. This is the one place this release changes a pre-existing JSON key
  rather than adding one: a session whose store is found through the `cleared_from` root
  or the team file now reports its tasks where it reported none, so `tasks` moves from
  null to an array and `tasks_completed` from null to a count.
- **`agents` gains `pending_reason` and `pending_checker`** on a frozen lane, in JSON and as two
  lines under the text tree's `PENDING` line: the harness's own reason tail for a predicted ask,
  and one disclosure line naming the path, the checker and the generation
  (`path: structured · checker: structured · gen2`), with `assumed` when the record carries no
  `version`. Both are `null` for a pending call these lexical layers do not model, a Windows
  `PowerShell` call included.
- **`toolDenialKind`, the harness's own reason a tool call did not run.** Modeled on the record,
  rendered on a tool-result hit as `[denied: user-rejected]` in the label zone, and carried as
  `denial_kind` in hit JSON. Four values are written (`user-rejected`, `interrupted`,
  `cancelled`, `permission-rule`); the classifier's reason sentence is rendered into the prompt
  only and never reaches disk, so this field is the whole on-disk answer to why a call was
  refused. `--count-by result` is unchanged: a denial is an `error`.

### Changed

- **A frozen lane's `pending_classification` is now decided by the checker the harness would
  actually have run, for the Claude Code generation that recorded it.** Claude Code decides a
  dangerous removal with one of TWO checkers and the tree-sitter decomposition picks which: a
  too-complex parse reaches the lexical classifier, a cleanly parsed removal reaches the
  structured one, and both produce the same bypass-immune ask. csift ran the lexical classifier
  on everything, so it attributed structured verdicts to the wrong grammar. It also stripped
  leading shell keywords, which the real clause head does not do, and that was a pure false
  positive on `do rm` / `then rm` / `else rm`. The port now branches on the parse shape, runs
  the generation's chain (`< 2.1.208`, `2.1.208..=2.1.260`, `>= 2.1.261`), and reports every
  arm's reason tail verbatim. Over 162,118 distinct Bash commands in a local corpus, 12 lanes
  flip: 8 up to `escalation-blocked`, 4 down to `awaiting-execution` (all four the removed
  keyword false positive). No other command's output changes.
- **An arm csift cannot decide now says so instead of guessing.** The structured checker
  resolves the operand against the shell cwd, realpaths both, and compares against the
  working-directory set; a transcript carries none of that. Those two arms answer
  `awaiting-execution` with the reason `removal target needs the filesystem state at the time`.

### Fixed

- **The autonomous-loop markers now anchor at the start of a record's content.**
  `harness.schedule.wakeup` and `harness.meta.loop` used to match their markers anywhere in a
  body, so a record that merely QUOTED a fired tick was read as one: a skill's instruction text
  that embeds the loop preamble to explain it, a peer message relaying a tick, a compaction
  summary recapping one. Each lost the leaf it had earned. The resolver builds the whole
  delivered prompt and the fire writes it as its own record, so a real tick carries its marker
  at offset 0; the arms now test exactly that, after the same leading-whitespace trim the other
  harness marker arms use. A genuine tick classifies as before.

## [0.12.1] - 2026-09-11

### Changed

- **The compaction-boundary prefilter keep now requires the `"subtype"` key beside the
  literal.** A `compact_boundary` record always carries that key and a payload that merely
  writes the word in prose never does, so a flagless scan stops parsing attachment and
  `queue-operation` lines it had no leaf to render them under. Every real boundary is still
  admitted, a refused line is still a node of the survival chain, and an explicit address still
  fetches it whole. The visible fix is a queued prompt naming the literal, which used to be
  emitted and censused as `user.queued` on a flagless scan while the same run reported that
  leaf as unscanned.
- **The survival axis got most of its cost back, and no output byte moved.** 0.12.0 put each
  line's chain-structural spine row in the same vector as the records the command actually
  wanted, so every element was sized to a full record. The two kinds now ride separate output
  streams and the spine row is its own narrow type, 72 bytes against a record's 1184. Medians
  of 21 reps on a 397 MiB transcript, this release against 0.12.0: `image` 29.2% faster,
  `verbatim` 14.6%, `show --branch-points` 14.2%, `files` 8.1%, a heavy `-t user.unsent`
  search 6.8%, `stats` 6.7%, `recover` 6.1%. The same-binary control arm drifted at most 1.1%
  over those seven shapes, so the smallest win is five times the drift. A second, independent
  run of the same driver read the same direction with larger margins (`image` 31.8%,
  `verbatim` 16.0%). What remains against 0.11.1 is the axis itself: `image` 1.233x where
  0.12.0 read 1.74x, `verbatim` 1.153x where it read 1.35x, and `files` 0.776x, still faster
  than 0.11.1 because one chain build replaced four. Nothing a consumer reads changed, and
  that is the checked part: stdout, stderr and exit code are byte-identical to the 0.12.0
  binary on 851 (command, file) pairs over the 76 top-level transcripts of one corpus, and on
  a further 1368 from a supplementary sweep of 18 more command shapes over the same files.

### Fixed

- **An orphan reconciliation pulse now renders every task id it closes, and names the kind.**
  At the next session start Claude Code reconciles the tasks a previous session left open with
  one `<task-notification>` carrying several `<task-id>` tags plus its own scan marker. csift
  joined every id already, but the render read only the first, so a pulse closing five tasks
  showed one and hid four, and the marker could take the id slot outright. The real ids now
  render comma-joined in the label's id slot, the marker's kind renders as a trailing
  `(orphan reconciliation: <kind>)`, and a `search` hit carries both as JSON `task_ids` (an
  array) and `orphan_kind`. One helper is behind `search`, `show`, `list` and `verbatim`, so
  every record surface follows. Rare and worth knowing: of 3076 notification sections on the
  top-level transcripts of one corpus, 2 carry more than one `<task-id>`.

### Documentation

- **The compaction step-over is a guard, not the path the walk takes.** The maintenance
  manual, the spec, the chain module comments, the `verbatim --help` text and four ledger
  claims said csift steps over a compaction cut through the boundary's own
  `logicalParentUuid`, but a `compact_boundary` carries a null `parentUuid` on every corpus
  specimen (260 of 260), so the walk ends on the boundary and never consults that field,
  and everything physically above the terminating record reads `pre-cut` from the floor
  rule alone. Renaming the field on five real compacted transcripts of 9 to 21 MB leaves
  the per-hit `survival` census, the `stats` chain totals and the search-summary
  disclosures byte-identical, which is how the corrected reading was checked and why no
  output moved. The coverage figure that justified the step (a median 9.4% against 46.5%)
  measured a walk that consults the field at a null-parent boundary rather than the shipped
  one, so it is out of the maintenance docs and recorded as a residue on the claim.

## [0.12.0] - 2026-09-09

### Changed (breaking)

- **BREAKING: the SURVIVAL AXIS replaces the three opener heuristics.** csift now asks Claude
  Code's own question - which records does the conversation chain still reach? - instead of
  three special cases that approximated it. A record the chain no longer reaches is ABANDONED:
  outside turn numbering, skipped by a bare `-t user`/`-t agent`/`-t harness`, reached by a glob
  or an exact leaf with an `[abandoned]` / `[rewound]` marker. Turn numbering shifts on any
  transcript carrying a rewind or a replayed block.
- **`-t user`/`-t agent`/`-t harness` gained a third exclusion axis.** Delivery and leaf
  visibility already narrowed a bare role; survival is the third. Every other selector form is
  unchanged.
- **`user.unsent` narrows to drafts, and `user.rewound` is one of the two leaves this release
  adds, bringing the taxonomy to 37** (`harness.resume.placeholder` is the other). A draft is a prompt
  that was recalled and never drew a reply. A turn the operator rewound past was sent and it
  DID draw one, and that reply is the whole discriminator. Both sit outside turn numbering and
  outside a bare `-t user`; both are reached by `-t user.unsent` / `-t user.rewound` or a glob.
  A rewound opener's diff line leads `rewound: the conversation continued from L<n> instead`.
- **A replay copy is now the EARLIER line.** The loader's uuid map keeps the last line carrying
  a uuid, so that line is the survivor for opener treatment and for addressing by uuid; the
  earlier line renders `[replay copy of L<n>]`. Both stay searchable and counted.
- **A turn number names the same turn on every command.** `verbatim` turn 7, a
  `search --count-by turn` row `t7`, `show --turn 7`, `files --turn 7` and `recover --at
  @turn:7` all mean the live turn 7 that Claude Code's own conversation chain still reaches,
  and `image --turn` joins them (it had stayed on the older, more conservative grouper). That
  agreement had to be re-established and not assumed: splitting one numbering rule into
  two, which is what wiring the axis into only some surfaces does, put 2,085 turn stamps out of
  step on a 19-transcript `files`-to-`search` join, 23.1% of the paired rows. 0.12.0 reads 0
  there, as 0.11.1 did, and `stats` `turns` equals the `search --count-by turn` row count on
  75 of 75 top-level transcripts.
- **Census effect of the axis, so you can reconcile your own numbers.** Over this corpus the
  opener count is conserved (3650 turn openers before, 3650 + 7 after): seven records that used
  to be counted as turn-opening `agent.communication.signal` are now `user.rewound`, so that
  leaf reads 397 where it read 404. Nothing was dropped; the seven moved leaf. The whole of
  `user.rewound` is a re-labelling and the arithmetic closes. Run the two binaries back to back
  over one corpus, `search "" --count-by label --max-count 0`, and `user.message` drops 6
  (2,781 to 2,775), `user.unsent` drops 30 (909 to 879), `agent.communication.signal` drops 7
  (404 to 397), while `user.rewound` reads 43: 6 + 30 + 7 = 43, every record accounted for.
- **Records above a compaction cut are `pre-cut`, and that is where the fail-open guard
  applies.** Claude Code's walk stops at a `compact_boundary` and never reads its
  `logicalParentUuid`; csift takes exactly that step so an archive can still show what the model
  had read. The step can fail: 52 of 245 boundary records in this corpus, across 14 of 75
  transcripts, point at a record that is no longer on disk, and on 13 files that is where the
  walk ends. Above such a point csift does not know what the chain reached, so nothing there is
  ever called abandoned — it is `pre-cut`, every selector still reaches it, and the same-parent
  draft rule still marks a recalled draft inside it. The step is worth taking anyway: coverage
  of a compacted transcript's conversation records is a median 9.4% without it and 46.5% with
  it. Recorded as claim MISC-043.
- **`verbatim` replays LIVE turns only.** A turn the operator rewound past, or a prompt recalled
  and re-typed, is counted in a per-session note with the `csift show --line` that fetches it,
  and never re-emitted. The compaction summariser reads the in-memory message array, so it never
  saw that turn either - replaying it would put text into a "what the compaction clipped"
  reconstruction that no model ever read. Reconstruction still crosses a compaction cut, where
  Claude Code's own loader stops; those units are flagged `pre-cut` rather than dropped.
- **`files` and `recover` KEEP every abandoned mutation and mark it.** Disk truth is file order:
  an Edit on a rewound branch landed. The row stays, its turn slot reads
  `turn abandoned (root L<n>)`, and it carries an `[abandoned]` marker. A `--turn` window admits
  it by the live turn it physically follows, so a window over a span never hides a write that
  happened inside it.
- **`image` lists an abandoned image, marked.** The bytes are on disk and `--out` writes the same
  file; only `--turn`, which windows on numbered turns, leaves it out.
- **`recover`'s external-write inference widened to the `--file` target.** The file-history
  instrument now reports a version jump with no tool write on ANY path a recover run names, as
  `external write (inferred, snapshot vN->vM, no tool record since L<line>)`. `files` still
  reports it for the settings family only: the tracked set spans every written path, and a global
  timeline row would flood the output (58,115 such jumps over 1,121 non-settings paths in one
  measured corpus). Both `file-history-snapshot` and the per-write `file-history-delta` now feed
  the version sequence.

- **`harness.schedule.continuation` is now `harness.resume.prompt`.** Same predicate, same
  records, new path. That leaf never came from the scheduler: the record is written by the
  resume LOADER when the transcript it is loading ends on a dangling user prompt, and it
  needed a family its new sibling could share. `harness.schedule` keeps only `wakeup`. The
  old spelling is a hard parse error that names its successor, the same way the pre-v0.5
  flat label values do. Update any script or saved query that keys on the old path.

- **The axis costs time, and the number is here, not in a footnote.** Every full-scan
  command now reads the lines its own prefilter drops, keeping the five structural fields the
  chain needs and none of the payload. On a 397 MiB transcript that is 100,387 structural rows
  against as few as 183 records the command itself wanted. Measured against 0.11.1 on that
  transcript, medians of 21 runs per arm on an interleaved CPU-time driver with rotated cells
  and a same-binary control drifting under 2%, the ratios quoted being wall clock: `image`
  1.74 to 1.77x, `verbatim` 1.35x,
  `show --branch-points` 1.54x, `recover` 1.19x, `stats` 1.17x, and a heavy `-t user.unsent`
  search on a 721 MB transcript 1.17x. `files` got FASTER, 0.84x, because four chain builds
  became one. The byte walk itself is nearly free in wall time because it runs inside the
  parallel scan; what lands on the clock is the serial carry of a wider row vector plus the
  chain build. Four candidate savings were measured and rejected, and the one that remains
  needs the structural rows to stop sharing a collection with full records, which is a later
  patch and not a last-minute change to the seam this release just settled.

### Added

- **The resume repair pair has its own two leaves.** Resume a session whose last record is a
  prompt nobody answered — an esc-recalled draft, an interrupted turn — and Claude Code
  repairs the tail at load time: it appends `Continue from where you left off.` as an
  `isMeta` user record (`harness.resume.prompt`) and splices a stand-in reply reading `No
  response requested.` in after it (`harness.resume.placeholder`, new). **The model receives
  both**, so they shape the resumed turn while belonging to neither side of the conversation:
  `-t user` and `-t agent` hide them, `-t harness` and `-t harness.resume` show them, and
  neither opens a turn. The prompt leaf requires `isMeta`, which is the flag the injecting
  call site stamps and the one Claude Code's own recogniser tests — so a message *you* begin
  with that sentence is still yours, labelled `user.message` and opening its turn.
- **`resume_paired` on a placeholder hit**, with `[paired]` / `[unpaired]` in the label zone.
  One writer produces every placeholder and it fires whenever the loaded tail is a user
  record, while the prompt beside it needs an interrupted-turn verdict — so the same record
  also lands alone after an interrupt marker or a slash-command wrapper (measured: 8 of 18
  in this corpus are paired, 10 are not). Both forms carry the leaf; the pairing is a fact
  on the hit, read from the record's `parentUuid`, rather than a second label.

- **JSON**: `survival`, `abandoned_root_line` and `replay_copy_of` per hit and per `show` record;
  `abandoned_records`, `rewound_turns`, `replay_copies`, `boundary_cut_line` and `leaf_source` in
  the `search` summary. The text footer states every one of them.
- **`show --branch-points` names the live child.** Each fork prints `live child: L<n>` from the
  chain and labels every other child `rewound`, `draft`, `abandoned` or `pre-cut`. The fork parent
  is now located over every line the loader admits, so a fork parented to a hook's attachment
  record prints that line and its type; `parent uuid not in this file` replaces the old
  `(parent line not located)` and means what it says. JSON branch-point rows gain `parent_line`,
  `parent_type`, `live_child_line` and, per child, `survival` and `verdict`.
- **`stats` counts LIVE turns**, the same numbering `search` prints, and adds three whole-file
  chain totals per session and in the scope TOTAL: `abandoned_turns`, `rewound_turns` and
  `replay_copies`. They never window - an abandoned opener has no turn index to window on - and
  the line-type census stays the exact whole-file corruption authority.
- **JSON on the file-oriented commands**: `survival` + `abandoned_root_line` on a `files` mutation
  and boundary row (with a null `turn_index` when abandoned) and on a `recover` boundary;
  `abandoned` on every grouped `files` row, a SHARE of `total` rather than a subtraction;
  `abandoned_events` on a `recover` coverage/snapshot row; `abandoned_turns` in the `verbatim`
  summary and `survival` on each of its units; `survival` on an `image` listing row.

- **The compaction boundary shows what the compaction KEPT, and the two `/rewind` summarize
  modes are finally distinguishable.** A boundary record always carried more than the four
  numbers csift printed. Its excerpt now names every field the record actually has, in a fixed
  order and only when present: `messagesSummarized`, `cumulativeDroppedTokens` (the running
  total of context tokens every compaction of this session has dropped), `preserved=<N uuids,
  M allUuids, anchor abc12345>` and `segment=<head>..<tail>`. The `preserved` pair is the set
  the compaction kept — `uuids` is what reached the disk, `allUuids` the in-memory superset,
  which is why an `allUuids` entry sometimes resolves to no line. The excerpt counts those
  lists and prints eight-character handles so a boundary line stays scannable; JSON
  `compact_metadata` carries the whole object verbatim, uuid lists included. A boundary with
  only the four original scalars renders exactly as it did before, byte for byte.
- **`mode` on both compaction records: `compact`, `summarize-from-here`, `summarize-up-to-here`.**
  Claude Code compacts three ways and writes the same two records for all three, so a `/rewind`
  "Summarize up to here" used to look like an ordinary auto-compact. One key separates them,
  and it sits on the summary: `summarizeMetadata:{direction, messagesSummarized}` is written
  INSTEAD of `isVisibleInTranscriptOnly`, and `direction` says which half was summarised —
  `up_to` summarised everything before the message you picked, `from` everything after it.
  csift prints that as `mode` in JSON on the search hit, the show record and the verbatim
  boundary row, as a `[summarize up_to]` tag in the summary's label zone, and as a
  `· summarize up_to ·` segment in the verbatim banner. The boundary carries no direction of
  its own, so it takes its mode from the summary that follows it; a boundary your query never
  paired with one reads null rather than guessing `compact`. And a summarize is a compaction
  like any other: `verbatim` still restores the turns it clipped.

### Fixed

- **A turn you rewound past used to read as a recalled draft.** The old rule grouped turn
  openers by their shared `parentUuid` and called every earlier one a superseded draft, so a
  rewind that landed back on the same parent came out as `user.unsent` with a character diff
  against the "resend", and a rewind that landed anywhere else came out as an ordinary live
  turn. Over 910 shared-parent opener pairs in one corpus, 858 have no assistant record below
  them and are genuine drafts, 52 have one and were rewinds wearing a draft's label, and 0 of
  the 910 earlier openers are on the surviving conversation. Do not read that 52 and the leaf's
  own count as one number: they are different censuses. The pair census needs a LATER opener
  under the same parent; the leaf needs the chain to have resolved that region and then covers
  every abandoned opener that drew a reply, whatever sits beside it. On this corpus at this
  release's census `user.rewound` reads 43 over 10 sessions. The likeliest source of the
  difference is the blind region above a dangling compaction boundary, where csift declines to
  call anything abandoned at all; that explanation is plausible and UNTESTED.
- **An abandoned record used to count as part of the live conversation.** Not just the opener:
  every reply, tool call and tool result under a rewound prompt was numbered into a turn,
  selected by a bare `-t agent`, replayed by `verbatim` and counted by `stats`. All of that was
  the model's context on a branch the conversation left. Such a record is now outside turn
  numbering and outside a bare role, reached by a glob or an exact leaf with an `[abandoned]` /
  `[rewound]` marker, and every count that changed is disclosed in the footer and the JSON
  summary. `files`, `recover` and `image` deliberately keep their rows and mark them, because
  those three answer about the disk.
- **A shell backgrounded by ctrl+b, a timeout or a delivered message is a background task.**
  `run_in_background` is the only door the model asks for. Claude Code opens three more on a
  command the model ran in the foreground: you press ctrl+b on the in-flight call, it hits its
  timeout, or it is moved aside so a queued message can reach the model. The launching tool
  call is never rewritten, so the receipt sentence is the only trace — and `status` was
  calling such a session `idle-eot` while a shell of its own was still running. Both commands
  now read the receipt: the task is listed and counted, `--until stop` stays open over it, and
  the row says which door it came through (`entered by ctrl+b`, `entered by timeout after 2m`,
  `entered to deliver a message`; JSON `entered_by`, `timed_out_after_ms`). On three real
  sessions the open-task count went 3 → 5, 5 → 6 and 51 → 54.
- **The launch record behind such a task is recovered, and an unknown launch time is said
  aloud.** The launching line carries none of the scanner's needles, so a second targeted pass
  fetches its instant, command and description — which is also what `--ignore-background` then
  matches on. When that line is gone the row keeps the receipt instant under an explicit
  `launched-at unknown; receipt at …` note (JSON `launch_note`) instead of passing it off as a
  launch. The receipt must be a whole sentence — the clause, a real `b`+8 task id, and that
  arm's closing clause — so a transcript that merely renders the template (a grep of the
  harness binary does exactly this) cannot fabricate a task. A fifth way in, a plugin's turn
  abort, writes the ordinary sentence and cannot be told apart from a model-requested launch;
  csift does not claim it.
- **A fabricated reply no longer reads as the model's.** Those `No response requested.`
  records classified `agent.message`, which claims the assistant wrote a sentence no model
  produced — they carry the `<synthetic>` model sentinel and are built with no model call at
  all. The new predicate is a port of Claude Code's own recogniser, and the sentinel is its
  load-bearing half, so a genuine reply that happens to say the same words is unaffected.
- **`status` and `wait` stop reporting the loader as your last exchange.** A session resumed
  onto a dangling prompt ends with the repair pair, so the `last` section showed `No response
  requested.` as the newest reply. Both halves are now skipped and the real last prompt and
  reply come back.

- **An inbound cross-session message was invisible.** When one Claude Code session sends a
  message to another, the receiver's transcript gets a `type:"user"` record whose content is
  framed as `<cross-session-message from="uds:…" from-name="…" from-mode="…">` — a third peer
  framing beside `<teammate-message>` and `<agent-message>`. csift knew the other two, so the
  record matched no marker, its `isMeta` flag sent it down the "harness pseudo-turn" path, and
  it came out with no label at all: no leaf, no census row, nothing under any `-t`, and
  `csift show --line` on it answered `no such record(s)` even though `--raw` printed the line.
  The framing now classifies `agent.communication.inbox` and opens a turn like its two
  siblings. Its direction reads the sender's NAME (`from-name`, or the structured `origin.name`
  the harness fills from the same attribute) rather than the socket path in `from`. Detection
  is boundary-anchored like the others, so prose that merely quotes the tag — this repository's
  own docs do — stays `user.message`. Recorded as claim TURN-031, traced from the socket
  handler through the element template to the record stamps.
- **The queue rider stopped being counted as your typing.** A `queue-operation` enqueue line
  carries the same framed message one line earlier. `user.queued` means "the queue line carries
  the human's text", and it asks the same peer predicate the records do — so widening that one
  predicate takes the rider out of the human count with no second rule to keep in sync.
- **`show --line` renders a record csift has no label for.** `classify` deliberately emits
  nothing for a few real message shapes (an `isMeta` pseudo-turn matching no harness marker, a
  block record with no text) rather than mislabel them as the user. An explicit address on such
  a record used to bail. It now renders one unlabeled unit — `? (no label)` in text, `"label":
  null` with an empty `"labels"` in JSON — because an address is a promise to show the record it
  names. A plain scan still emits nothing, so no count, census or `-t` result changes; a line
  that is no record at all (a session-state cache line, a torn line) is still a hard miss
  pointing at `--raw`.
- **A plain search stopped reporting live history as pre-cut.** Two of the byte needles that
  pick candidate lines run on every scan, and both can pick up an `attachment` line a plain
  search has no label for — one looks for the channel envelope, the other for the word
  `compact_boundary`, which any payload may simply use. Such a record was removed before
  matching so it could not surface under a gated leaf, and removing it punched a hole in the
  conversation chain, which threads through attachment records: the walk stopped at the hole
  and everything above it came back marked `pre-cut`, on transcripts that had never compacted.
  The record is now reduced to its structural fields instead of removed — the same spine row a
  line the prefilter never picked up already gets. It still emits nothing, so no hit, count,
  census key or turn moves; the chain simply sees it. On this corpus, 173 lines across 5 of 76
  transcripts were affected. (Denominators in this entry read 75 or 76 top-level transcripts
  depending on the bullet: the corpus is live and gained one file between measurements. Each
  figure is whole against the scope it was taken on; none of them are a single census.) The
  plain scan and `--attachments` now report the same chain on 76
  of 76 files, where 3 disagreed before, and on one 7.2 MB transcript 1,190 of 1,246 rows read
  `pre-cut` before and none do now. `show --line` was always right about those lines, because an
  address opens the gates — so a search and a fetch of the same record no longer contradict each
  other.

- **The `harness.meta.system` help list names every subtype csift models.** `csift search
  --help` listed one of the two model-refusal subtypes and stopped at the six it could name,
  which reads as a closed set. It now carries `model_refusal_no_fallback` beside
  `model_refusal_fallback` and says that a subtype a later build adds lands there too, which is
  what the leaf has always done and what SKILL.md has always said. Help and SKILL are meant to
  carry the same information, and this one had drifted.

- **Peer message bodies render the same way everywhere.** `verbatim` and `list` already stripped
  the wrapper tags, the relay preamble and the security footer from an inbound peer message;
  `search` and `show` printed the whole tagged section, so the same message read two different
  ways depending on the command. All four now render the peer's own words. Know the consequence
  before you rely on it: the wrapper attributes are no longer part of any text `search` can match,
  so `search 'teammate_id='` and `search 'from-name='` find nothing at all — `--raw` does not
  recover them, because it only prints the source line of a hit the matcher already made. The
  peer's own words are matchable as before, the direction line still names the sender, and the
  verbatim line of a record you can ADDRESS is still one `csift show @<id> --line N --raw` away.

## [0.11.1] - 2026-09-08

### Added

- **The unsent diff line.** A rendered `user.unsent` draft now says how far it sits from
  the message that replaced it: `differs from the sent message in N chars (P% of the
  final): may carry an addition or a correction`, with `superseding_line`,
  `superseding_uuid`, `diff_chars`, `diff_pct` and `diff_exact` in JSON (on the `search`
  exchange row and the `show` record row). N counts insertions plus deletions of a
  shortest character-level edit script, so it is the work the edit did and not a length
  difference; P is N over the final message's length and can exceed 100 when the draft
  was the longer text. The engine is a bounded char-level Myers walk with no new
  dependency, and it runs only when a draft is actually rendered, so `-c`, `-l` and
  `--count-by` pay nothing. Measured over the whole corpus after the replay guard below,
  on a population of 887 drafts (a live corpus gains drafts as sessions run, so a re-run
  sees a slightly larger one): 877 exact, the other 10 reporting a proven floor between
  3,813 and 9,360 characters, the largest draft (237,434 characters) diffed in 0.755 ms,
  and both a whole-corpus draft scan and a broad search within noise of 0.11.0.

### Changed

- **`user.unsent` is documented as retroactive.** A draft and its resend are the same
  record shape (same key set on 852 of 886 pairs; nothing but identity and time always
  differs), and no successor line shape separates the two populations — every shape that
  reads zero after a control record reads zero because that window is cut at the next
  assistant reply, not because of anything about drafts. So the only discriminator is the
  later sibling, and a hook running at prompt submit cannot apply the label, because that
  sibling is the prompt being submitted. The closest approximation, skipping a trailing
  turn-opener that no assistant record follows, hides 92.2% of drafts but wrongly skips
  2.9% of genuine messages, and is now stated as a heuristic in SKILL's wrong-assumptions
  table rather than implied to be the label. Censused over the full populations: all 886
  drafts, each joined to the survivor csift itself names, against all 2,595 `user.message`
  records of the 28 draft-carrying sessions. Recorded as ledger claim TURN-026, which
  carries the commands and the whole per-shape table.

### Fixed

- **A replayed record is no longer read as a resend, and turn numbers shift where it was.**
  A compaction re-anchor re-appends a block of already-written records with their uuids
  preserved, and a re-appended user message keeps its `parentUuid` too, differing from the
  original in one field (`promptId`). The draft rule groups openers by `parentUuid` and
  keeps the last, so it read the copy as a later sibling: the ORIGINAL message, sent and
  answered, was labelled `user.unsent`, dropped from turn numbering, and shown as differing
  from the sent message in 0 chars. Seven genuine messages were affected in one session.
  Same-uuid openers under one parent now collapse to their first occurrence: the first
  keeps its label and its turn, the copy opens nothing but stays a turn member, so it is
  still addressable and still renders like every other replayed record. **Turn numbering
  shifts on any transcript carrying a replayed block** — as it did for the v0.5 slash-wrapper
  fix and the 0.9.2 draft work. Re-read the `tN` from current output rather than reusing a
  noted one; `--uuid` is the address that survives. Recorded as ledger claim CMP-019.
- **An unedited resend says so.** A draft whose text is identical to the message that
  replaced it now reads `identical to the sent message` rather than `differs from the sent
  message in 0 chars`.

### Changed

- **BREAKING: a bare role selector now selects what the model RECEIVED, per record.** It
  used to decide that per leaf, from a table of which labels are conversation. Claude Code
  decides it per record, with one drop predicate inside the assembler that builds an API
  request, and that predicate disagrees with the leaf table in exactly three cases. csift
  mirrors those three and nothing else. The two census changes, measured on this corpus
  with the two builds run back to back over 72 top-level transcripts:
  - `-t harness` now surfaces `system`/`local_command` records, a slash command's own echo
    and its stdout, because the assembler re-mints them as a user message and sends them.
    Sixteen records appear where 0.11.0 showed none, although `harness.meta.system` is an
    invisible leaf and stays one by default.
  - `-t agent` now drops the API-error placeholders, the assistant records Claude Code
    fabricates when a call fails and never sends back. `agent.message` falls by exactly
    118, the whole population of records carrying `isApiErrorMessage` (118 of 118 also
    carry the `<synthetic>` model that the predicate tests for). An `isVirtual` record
    leaves a bare `-t user` the same way.
  Globs (`-t 'agent.*'`), intermediate prefixes and exact leaf paths are untouched and
  still reach every record, so nothing became unreachable. Under those forms an
  undelivered record renders `[not delivered]` in the label zone, and every JSON hit
  gained a `delivered` field carrying the verdict. Recorded as ledger claims CLS-026 and
  CLS-027.

- **A bare `-t harness` admits the `type:"system"` lines.** Those lines were gated behind
  an explicit selector, so the delivered record above could not have been reached at all.
  The other five gated leaves keep the explicit-selector rule exactly, and the per-record
  rule drops every other system record the admission parsed. `gated_leaves_unreached` and
  its stderr note now report what the scan actually parsed instead of which selector was
  typed, and the note's leaf list is derived from the gated set instead of typed out.

- **Slash commands the model never sees, documented.** Three families leave nothing on
  disk, so finding no wrapper record is not evidence a command was not run: `/btw`,
  `/tasks` (= `/bashes`) and `/release-notes` return skip on every normal exit, any panel
  closed with Escape writes nothing whatever its own option was, and `/rewind`
  (= `/checkpoint`/`/undo`) and `/stop` take the same path through the `local` result map.
  A record on disk is not a record the model saw either: `/release-notes` appends a
  harness notice and then returns skip, so it is rendered to the human and dropped by the
  assembler. SKILL's wrong-assumptions table carries both rows. Recorded as ledger claims
  TURN-027 through TURN-030 and MISC-031.

## [0.11.0] - 2026-09-06

The csift channel: a way to get a message to a Claude Code lane the official channel
cannot reach, and to find out afterwards whether it arrived.

### Changed

- **BREAKING: the read-only law becomes a writers law.** Through 0.10.5 csift wrote
  nothing at all. From 0.11.0 exactly three commands write, `send`, `deliver` and `ack`,
  and they write exactly one place: a `csift-channel` directory csift creates in the
  session's own sidecar folder. Never a transcript, never the team mailbox, never the
  cross-session messaging socket, never the session registry, never any settings file.
  csift does not install its hook either; `deliver --recipe` prints the block and you
  paste it. Every other subcommand still only reads. Nothing that worked before stops
  working, but the promise a reader had is narrower now, and a narrower promise is a
  breaking one.

### Added

- **`csift send @<lane> "message"`** queues one message for one lane: a top-level
  session, an unnamed subagent, a teammate, a workflow lane. It exists because the
  official channel is not weaker for some receivers, it is absent: a running workflow
  lane cannot be reached by the send tool at all, an unnamed subagent has no arm to
  address its own parent subagent, nothing wakes an idle session that published no
  socket, and a sender outside Claude Code holds no tool to call. Every send prints one
  verdict, `OK`, `FULL`, `MAY-FAIL`, `UNPREDICTABLE` or `REFUSED`, and exits 0 even for a
  refusal, because a refusal is a definitive answer about the receiver and not a usage
  error. Where an official transport does exist, csift prints the exact call for you to
  make and still queues its own copy.
- **`csift deliver --slot k`** is the hook entry that carries the channel into a lane.
  You never run it by hand: `csift deliver --recipe` prints the settings block (both the
  plain and the PowerShell command form, any number of slots) and a human installs it.
  Slot k emits chunk k of what is waiting, the slots of one event order themselves, and a
  queue message at a turn boundary may block the turn from ending while the harness's own
  block cap allows it.
- **`csift msg <id>`** answers the question a queue cannot: did it actually arrive. The
  per-lane ledger records what csift emitted, the receiver's transcript proves what
  landed, and `msg` joins the two into one verdict (`DELIVERED`, `INTENT-ONLY`, `QUEUED`,
  `HELD`, `EXPIRED`, `ACKED`, `REFUSED`). The proof half on its own is exactly
  `csift search '<id>' @<lane> --additional-context`, so the join can be audited without
  trusting it. **`csift ack <id>`** records the one thing only a receiver can say.
- **`whoami` grew a lane layer**: a `@<agent-id>` target, the `self`, `parent` and
  `topology` sections, `--to @<lane>` for a reach prediction that sends nothing, and
  `--peers`, which lists every live lane as id, kind and state and deliberately nothing
  else. A description or a role-shaped name is the material one lane would use to claim
  standing over another, so the census answers who is alive, not who should be obeyed.
  Outside Claude Code, `whoami` now prints the not-a-lane answer with the one channel out
  and what a receiver needs installed.
- **`agent.communication.channel`**, the 35th label. A delivery lands in the receiving
  lane as a hook-context attachment, and this is the one attachment leaf a default search
  reaches, because a message addressed at the lane is not machinery. It renders verbatim,
  envelope header and all, so a reader can see who sent it, under what relation, and that
  it came from neither the user nor the harness.
- **`@<Name>@<Team>`**, a teammate's routing form, as a target. A teammate carries two ids
  minted apart at spawn: the routing form the official send tool needs, which can collide
  when two teammates share a name, and the transcript form on disk, which never does.
  csift resolves either, keys its own work on the transcript form, and now prints both
  wherever a teammate appears.
- The **settings cascade** csift reads to answer "will a delivery hook actually run
  there": five scopes in Claude Code's own order, `env` merged per key, `hooks`
  concatenated per event, plugin hook manifests unioned, the policy tier composed
  first-wins, and the three policy switches that can empty the whole hook set. What is
  not observable from disk is listed as such, so a gate verdict says "unknown" with its
  evidence instead of implying the file scopes are the whole story. `plan` now reads its
  `plansDirectory` through the same model, with its precedence unchanged.

## [0.10.5] - 2026-09-05

A correction release for the introspection ledger and for four csift defects the ledger's
own drift verdicts had named and the previous release shipped around.

### Fixed

- `image` joins the `#N` handle by NUMBER. A record's `imagePasteIds` lists the ids in the
  order of its image blocks, while the `[Image #N]` markers keep the operator's text order;
  the two diverge on 18 of 662 corpus records, and the positional zip gave those a silently
  wrong handle. Without the array the positional zip stays, under its count guard.
- `recover` no longer replays a Read echo that carries no text. Claude Code blanks the text
  of a tool result older than its retention window before persisting it (320 such echoes in
  the reference corpus, `content` empty with the line counts intact), and four result arms
  never carry a text; a blanked echo of a complete read used to replay as a whole-file
  snapshot of nothing. Each is now a counted `blanked-read` with a soft annotation boundary.
- The `status`/`wait` pid probe pins `LC_ALL=C` and `TZ=UTC` on its own `ps` call, as the
  harness does for its `procStart`; under a German or French locale the rendering parsed
  under neither English pattern and the pid-reuse guard was silently skipped.
- Search hits carry `refetch_uuid` beside `refetch`: a line number is a durable address only
  while the transcript is append-only, and three harness paths rewrite a live transcript in
  place; the uuid survives them.
- Four statements csift printed or taught were wrong at 2.1.258 and are corrected with
  their dates: TaskStop resolves a teammate by its name or `name@team` from Claude Code
  2.1.198 (the agents footer, the JSON control hint, the SKILL rows and hook recipe said it
  rejects every form); the file-history store is also written by an approved in-place `sed`
  preview (`recover --list-backups` said bash edits never land there); the pending
  AskUserQuestion split is a timing outcome of the write frontier, not a question-count
  rule and not a 2.1.258 change (a comment, a note, the help and three documents said
  otherwise).

### Changed

- Ledger schema: the attribution `upstream` (the producer lies outside the shipped binary by
  construction: the model or API side, the operating system, a native runtime binding, with
  the client-side treatment traced), and three new gate rules: a claim's latest check is
  never `drifted`, no open leg is an unconsumed text correction or a recorded rejection,
  and every claim below end-to-end carries its leg fields. Ledger text now follows the
  prose law: no dates, no csift versions, no audit narrative; the commit and the release
  carry those.
- README: the verification section states what the project will never publish (the
  method, a mechanical ledger comparison, any corpus) and why the ledger is re-verified
  periodically rather than for every Claude Code release.

## [0.10.4] - 2026-09-04

### Fixed

- `wait` and the `status`/`wait` tail window no longer memory-map a live transcript. A
  mapped file that another process truncates faults with SIGBUS on the first page
  touched past the new end, and nothing catches it; Claude Code does rewrite a
  transcript in place (a rewind tombstone truncates and rewrites the tail, an armed
  local GC rewrites on compaction), so a long `wait` polling a live file every few
  hundred milliseconds carried that risk on every poll. Both readers now use plain
  positional reads, which return fewer bytes on a shrink and never fault. The one-shot
  full scans keep the memory map, whose sub-second life bounds the exposure.
- `wait` detects a transcript that shrank between two polls, moves its baseline to the
  new end and reports it (`transcript shrank N time(s)` in the activity line, JSON
  `shrinks`); before, a shrunk file was skipped silently.

## [0.10.3] - 2026-09-04

The attribution push: every claim in the introspection ledger now states how its
behavior was traced (the producing code in the shipped Claude Code binary, a specimen
on disk or in a live trial, both, or neither), the README carries the tally as a
gate-generated table, and the five defects the tracing found in csift are fixed.

### Fixed

- `search`/`verbatim`/`show`: an AskUserQuestion answered through the freeform
  `toolUseResult.response` field (the binary's `The user responded: ...` branch, which
  leaves `answers` empty) classifies as `user.answer` and opens a turn like every
  other answer. The AUQ unit renders the questions asked and a `response:` line. The
  marker joins the answer prefixes and the verifiable synth needles, so the whole-file
  gate stays sound. The idle-timeout branch (`No response after ...`) is not an answer
  and still opens no turn.
- `status`/`wait`: a completion pulse whose status is `blocked` (Claude Code's
  remote-agent notifier writes it) or any literal csift does not know is its own bucket
  (`N blocked`, `N with an unknown status`; JSON `blocked`, `other`) instead of being
  booked as a clean completion.
- `wait --until notification` fires in every watched lane. Claude Code normally delivers a
  pulse to the main transcript, but one addressed to the owning agent lands in that
  agent's lane (2 of 2906 delivered records in the reference corpus), and the main-only
  scope could never fire on it.
- `recover`: a Read cut at its token budget (`truncatedByTokenCap`) is never replayed as
  a whole-file snapshot. On a file whose lines are too long to paginate the harness
  recounts `numLines` from the cut slice, so it can equal `totalLines` and the old
  full-read test accepted truncated content as the whole file.
- `image` lists the images of a `queued_command` attachment: a prompt queued and then
  edited or recalled never becomes a user record, so its pasted images existed only
  there (45 of 52 such blocks in the reference corpus had no other copy).

### Changed

- `INTROSPECTION.json`: every claim carries `producer_trace` (`complete` | `partial` |
  `none`), `specimen` (`observed` | `none`) and the derived `attribution` (`end-to-end`
  | `producer-only` | `specimen-only` | `partial-producer` | `by-elimination`), plus
  `open_legs` naming the instrument that would close each missing leg. A complete
  producer trace is three hops in the binary, quoted verbatim at byte offsets: the
  trigger, the gate and the writer. A negative ("nothing else writes this") closes only
  by an `enumeration` of every site in the binary, each read; a claim whose text a hop
  refuted was rewritten, not annotated; an end-to-end claim carries no open leg (a
  non-gap note lives in `residue`). Version floors were checked against historical
  builds fetched by version and bisected on string literals. The gate refuses a
  by-elimination claim that `holds`, a leg pair that disagrees with its attribution, an
  end-to-end claim with an open leg, and a README tally that disagrees with the ledger.
- README: a "How much of this is verified" section with the tally table, regenerated
  from the ledger and checked by the gate.

## [0.10.2] - 2026-09-03

The first re-read of the introspection ledger after 0.10.1: the 17 claims that had no
code site were anchored, every site the release had moved was relocated, and the
reading turned up the defects below. Each fix carries its claim update with the
instrument that measured it.

### Fixed

- `wait --until notification` never fired on a completion absorbed mid-turn. Claude
  Code delivers a `<task-notification>` as a user record only when the session is idle;
  a pulse that lands mid-turn exists only on a `queue-operation` enqueue line and a
  `queued_command` attachment (this corpus: 3218 pulse-bearing user records against
  5893 enqueue lines). The condition and the wait activity census now read the same
  three carriers the background section joins; a queue remove or dequeue repeats the
  enqueue's pulse and counts nothing.
- `plan`: the slug-only binding resolved `plansDirectory` against the slug record's
  own `cwd`, which follows the tracked shell cwd and can already sit in a
  subdirectory, so the plan file was joined under that subdirectory and a
  project-scope `plansDirectory` was silently dropped. Claude Code memoizes the
  directory at its first access early in the session, so csift now binds against the
  transcript's first recorded cwd and reads the settings scopes from that root.
- `search`: a subagent transcript that is a `/fork` clone (line 1 is a
  `fork-context-ref` record) had its first turn-opener labeled as the spawn-prompt seed
  (`agent.communication.inbox`) although it is the parent's own human message; 10 of
  the 42 clones in this corpus carry such a record. A clone now has no seed.
- `agents`: the global spawn index folded subagent locals later-wins, so a clone's
  copy of a sibling's spawn record could re-parent that sibling onto the clone. The
  fold is first-wins (main first, then discovery order).
- `search`: a superseded draft whose text is sectioned (a pulse or relay shape) fanned
  out into per-section classes its own `labels[]` did not carry; it keeps the single
  `user.unsent` view now.
- Two shipped strings had lost their line-continuation backslashes and printed runs of
  spaces: the `wait` timeout-guard message and a `recover` bash-append boundary detail.

### Changed

- `show --turn` help says what the fetch omits: the gated non-record lines a turn carries
  (turn-duration, stop-hooks, away-summary, queue, snapshot, system) are addressed by
  `--line`/`--uuid` or `search -t harness.meta`, never by a turn range.
- `search --help` names the eight LLM-invisible leaves (the two of 0.9.4 plus the six
  gated leaves) instead of "exactly two".
- The ledger gate requires at least one code site per claim; AGENTS.md section 7.1
  documents the ledger schema, the verdict semantics, the six gate rules and the four
  procedures.

## [0.10.1] - 2026-09-03

Verified against a real Claude Code 2.1.258 session on Windows 11 ARM64 (a Sonnet 5
build with four subagents and a dev server left running as a background task), which
the 0.10.0 release matrix never exercised: its Windows suite was green while the pid
probe was compiled out there.

### Fixed

- `status` / `wait`: the registry's `shell` status was read as a running shape. The
  harness writes `shell` only as `idle` relabeled while a background shell task is
  open, so a session that had ended its turn with `npm run dev` running reported
  `running` and `wait --until stop` could not fire even under a lens that ignored the
  dev server. `shell` now reads as the idle-with-background-shell shape it is: the
  seventh verdict when the scan counts the task, `idle-eot` when the lens excludes it,
  with a note either way. `busy` is the only registry running signal.
- `status` / `wait` on Windows: the pid probe now exists there. `procStart` in a
  Windows registry row is a FILETIME tick count (100ns since 1601), not an asctime
  string, so the old parser silently skipped the reuse guard; the probe now reads the
  owner's start time through PowerShell `Get-Process` (falling back to `tasklist` for
  liveness alone), compares instants with the same 2s tolerance, and reports
  `stale-dead` for a dead owner. A row whose `pidDomain` names another domain
  (`darwin`, `linux`, `win32:<host>`) is never probed and the verdict says so.
- `status` / `wait`: a child lane whose completion notification already landed in the
  main transcript is `settled` regardless of its tail, so a subagent that finished
  seconds ago no longer counts as a live lane for up to 300 seconds.
- `plan`: the slug-only binding resolved a relative `plansDirectory` against the
  config home and took an absolute one verbatim. Claude Code resolves it against the
  project root (the session's cwd) through the merged settings scopes, and refuses a
  value that escapes that root, falling back to `~/.claude/plans`. csift now does the
  same, reading the user, project and project-local settings in that precedence. The
  introspection ledger's first audit caught this one.
- `agents`: a `/fork` child reported depth 65. Its transcript is a clone of the parent's
  and carries the spawning tool_use itself, so the spawn join named the child as its own
  parent and the depth walk ran to its cycle cap. csift now reads the `parentAgentId`
  the harness writes into the child's meta.json and never accepts a node as its own
  parent. Also from the audit: the parent's record of a subagent return carries an
  appended continuation footer, not a truncation (help and docs corrected), and
  recover's Bash read anchors already reach built-in and teammate lanes, whose results
  carry the `toolUseResult` echo; only workflow lanes lack it (comment corrected).
- `search`: an inbound peer message relayed mid-turn carried no label at all. Claude
  Code 2.1.258 relays under three preambles ("Another Claude session sent a message:",
  the same "while you were working:" form, and "A peer session sent a message while
  you were working:"); csift knew only the first, so 29 of 47 `<agent-message>` records
  in the reference corpus were invisible to every census. All three are section
  boundaries now, and the corpus-wide `agent.communication.inbox` census moves from
  8721 to 8750 records.
- `search`: the third AskUserQuestion answer phrasing Claude Code 2.1.258 writes
  ("The user answered: ...") is an answer marker and a turn opener; the unanswered
  branch ("The user did not answer the questions.") never is. The retired "User has
  answered your questions" form stays recognised for older transcripts.
- `status` / `wait`: the registry reader takes `procStartFt` when a row carries it
  beside `procStart`, matching the harness's own `procStartFt ?? procStart` readers.

### Added

- `waiting-hitl` gains two instruments beside the elicitation sidecar: the registry's
  `waiting` status (the harness sets it for any blocking dialog, including permission
  prompts, plan approvals and sandbox or worker requests) and an unreturned
  `AskUserQuestion` / `ExitPlanMode` at the main tail. Claude Code 2.1.258 writes a
  multi-question AskUserQuestion to the transcript at question time; a single-question
  one still stays buffered until answered, which the sidecar covers. The idle-verdict
  honesty note now names the registry status it saw instead of calling every
  permission prompt invisible.
- `INTROSPECTION.json`: a structured ledger of every Claude Code behavior csift depends
  on, one claim per entry with its code site, its verbatim snippet, the version it was
  pinned against and a per-release check record carrying the instrument, the
  observation and the counting rule. A pre-commit gate ties the README's
  "verified against Claude Code" badge to the ledger: the badge version is admitted
  only when every claim carries a check at that version, snippets must still exist in
  their files, and check evidence must be claim-specific.
- README badges for the verified Claude Code version and the crate-wide mutation score
  (cargo-mutants over the whole crate: killed mutants, counting a mutant that hangs the
  suite as killed, over every viable mutant; the release notes carry the caught-only
  floor and the timeout share).
- `harness.meta.system`, the 34th label: a gated catch-all for every other `type:"system"`
  subtype the harness writes for its own UI and never sends to the model, such as the
  `informational` warning that Remote Control disconnected after an account switch,
  `api_error`, the model-refusal fallbacks, `agents_killed`, `local_command` and
  `scheduled_task_fire`. It renders as `[<subtype> <level>] <content>`, is scanned only
  under an explicit `-t` like the other promoted leaves, and `show --line` renders it
  without `--raw`. The compaction boundary keeps its own leaf.

### Changed

- README highlight 8 is laid out as short lines.
- Documentation records the re-measured laws: the registry status vocabulary
  `busy | shell | idle | waiting` and what each means, the `claude -p` row with a null
  status, the multi-question AskUserQuestion flush, the Windows record shapes (both
  `Bash` and `PowerShell` tools in one session, the same background-task result
  grammar, task output files under the local temp dir).

## [0.10.0] - 2026-09-02

Sessions stop lying about being stopped: status and wait now see every
background shell, async agent and Monitor a session launched, wait
requires a timeout, and the lines csift used to skip become five new
searchable leaves.

- status + wait: BACKGROUND TASKS. A Bash launched with run_in_background
  gets its tool_result within milliseconds, so the tail state machine
  paired it at once and a session idle with a dev server still running
  read as a clean stop; the harness itself writes nothing about a running
  shell at end of turn (measured on 100 turn_duration records emitted
  with an open shell: no shell field at all; the REPL's "N shell still
  running" is process memory). status now scans the whole main transcript
  for backgrounded shells, async agents and Monitor arms and joins their
  task-notification completions by the launching tool_use id across all
  three carriers (a user record, a queue-operation line, a queued_command
  attachment), reading launches from every lane and completions from the
  main file only, where they always land. Every open task renders a bg
  row with kind, id, launch instant and age, description or command, and
  the output file's size and last write; closed ones fold to counts. Not
  returned is not proof of running: Claude Code's own orphan summary says
  a UI stop, a Monitor timeout or agent teardown leaves no transcript
  marker, and the section repeats that. Measured: 24 of 3133 corpus
  launches never returned, 22 of them launched more than a day before
  their session ended.
- status + wait: THE SEVENTH VERDICT and THE LENS. idle-background-open
  means the turn ended but background task(s) the lens counts have not
  returned, neither running nor stopped, and it never satisfies
  --until stop. --background-since WHEN (the shared time grammar, now
  with 2mo and 1y units, a tolerated leading minus, and the token now
  for the command's own start instant) counts only tasks launched at or
  after WHEN; --ignore-background RE (repeatable) excludes tasks whose
  command or description matches. Every task is still listed with the
  rule that excluded it.
- wait: BREAKING. --timeout is required, because a background task can
  be designed never to return, so an unbounded wait on stop was a bug
  in every 0.9.x. A call without it is rejected with that reason. On
  every exit the report carries the tail state in words, a census of
  what landed while waiting (tool calls by name, thinking, messages,
  prompts, notifications), the bg rows, and the last prompt and reply as
  excerpts. Both commands print those excerpts under a warning written
  for a model reader: an excerpt is a partial view of the final state,
  useful only for judging whether a background task is still meaningful,
  never a review of the work.
- search: two classification fixes. The harness's agents-stopped notice
  ("N background agents were stopped by the user: ..." and its singular
  form) was counted as a human turn; it is harness.notification.subagent,
  never genuine, never a turn opener, rendered "[subagent stopped] ...".
  A Background command pulse is always background-command: the quoted
  name heuristic that routed re-arm and monitor-named commands to the
  monitor leaf predated the real Monitor tool and produced 40 false
  monitor records on one project against zero genuine pulses. Historical
  counts for those two leaves move by design.
- search: FIVE PROMOTED LEAVES (taxonomy 28 -> 33). The non-record jsonl
  line types become searchable, classifiable leaves. user.queued is a
  queue-operation line carrying the human's text, with the queue event
  in the label zone (enqueue, popAll, or remove with its reason) and as
  JSON queue_operation / queue_reason. harness.meta.turn-duration renders
  the turn_duration record as "[turn duration: 1m 5s . durationMs=64911
  messageCount=908 pendingBackgroundAgentCount=2]", the structured body
  behind the REPL's "Done in Ns" line, which never lands on disk itself.
  harness.meta.away-summary is the model-generated recap shown on return
  after five minutes away. harness.meta.stop-hooks is the Stop-hook
  execution ledger. harness.meta.snapshot covers file-history snapshot
  and delta lines. Every one of the five is LLM-invisible by the same
  instrument as the compaction boundary: none carries a message field.
- search: THE GATED-LEAF LAW. A promoted leaf is parsed only when an
  explicit -t reaches it (the full path, a glob, or the harness.meta
  prefix) or a show address names the line; a bare scan never parses
  those lines (measured 1.03x, noise). The three fabricated renders
  register their type value as a synth marker so the whole-file gate
  stays sound; a zero-match run without such a selector says so.
- search: QUEUE FACTS, measured. Content rides enqueue, popAll and most
  removes, never dequeue; the remove reason is absorbed_mid_turn or
  delivered_to_agent; the queue line has no join key, so csift never
  asserts dispatched. Correction to the 0.9.2 entry: the "about 61% never
  become user records" figure counted every queue operation over three
  sessions; counting enqueue lines only over six sessions, the human's
  prose reaches a user record 72 to 81 percent of the time.
- show: an explicit --line or --uuid address renders every promoted line
  with no flag; the miss error names what stays --raw only.
- help and docs: 33 leaves, the GATED LEAVES rule, the seven verdicts,
  the background and lens paragraphs, the how-to-wait procedure, and the
  last-messages warning.

## [0.9.4] - 2026-09-02

Bash reads become reads and bash writes become writes in recover, plan
binding matches Claude Code's own rule, list names a forked clone's
origin, status shows what is actually moving, and metachar regex
searches stop paying full price.

- recover: BASH CONTENT ANCHORS. The deterministic shell subset now
  replays as first-class content instead of boundaries. Writes anchor
  per segment - a quoted-delimiter heredoc via cat/tee (the body is
  byte-verbatim in the transcript; an unquoted delimiter is admitted
  only with an expansion-free body), literal echo/printf, and
  truncate -s 0 - so the dominant real shape, write-the-file-then-run-it
  in one compound command, anchors; a compound command additionally
  demands a clean result echo (empty stderr, not interrupted: only the
  last segment owns the exit code, and a failing write always says so
  on stderr), and a second touch of the same resolved path anywhere in
  the command refuses the anchor. Reads anchor as single simple
  commands only (cat, head -n N, sed -n 'A,Bp') under the completeness
  gate; a window from line 1 that hits EOF is the whole file. A
  byte-known >> append is placed only onto a complete
  newline-terminated buffer, else it is disclosed as
  bash_append_unplaced. Deliberate non-anchors, measured or
  unplaceable: tail, sed -i, variable targets, interpreter and ssh
  heredocs. Coverage counts bash-read-anchor / bash-write-anchor;
  segment provenance names bash-heredoc / bash-cat / bash-write. A
  real heredoc-then-run python tool went from "no recoverable history"
  to 33/33 lines recovered verbatim.
- plan: a correctness fix. csift bound only via the plan_mode
  attachment, while Claude Code itself binds by the FIRST record
  carrying a valid slug - so on a forked clone (attachments stripped,
  slug records kept) csift answered "no plan" for a session whose plan
  CC will re-inject. Two binding laws now apply in precedence order;
  rows carry binding_source ("plan_mode" | "slug-only") and
  minted_at_compaction (the slug's first carrier is a compaction
  boundary - the fork mint site). plansDirectory from settings.json is
  honored.
- list: clone lineage. A transcript whose first timestamped record is
  a compact_boundary was minted by copying another session at that
  compaction (a background-job fork: uuids preserved, timestamps
  predating the file, slug stripped; zero false positives on a
  61-file real dir - file-birthtime rules were refuted). The row
  annotates the fork and names the ORIGIN session (a prose quote of
  the boundary uuid or a co-clone can never win the join); JSON gains
  is_clone / clone_of / clone_boundary_uuid. Documented corollary: a
  clone double-counts its inherited records on every spanning surface
  until scoped away.
- status: child lanes gain a `generating` state - a tail record
  younger than 300s whose last assistant stop_reason is not end_turn
  is mid-generation (measured: intra-lane record gaps reach p99.9 =
  295s while dead lanes sit 31h+ out; the old 15s mtime window
  misread one lane in 17, and stop_reason alone would mark 73% of
  dead lanes live). The mtime `active` state is retired; settled lanes
  fold to a count (JSON children[] carries live lanes only, beside
  settled_children); and a tasks section reads the harness task list
  (open tasks in_progress-first with blockers, completed folded to a
  count). The status help schema line also drops since_utc/since_local,
  which the verdict row never emitted.
- search: required-needle prefilter extraction. A pattern with
  metacharacters now derives a necessity-only literal set from its
  parsed structure (an alternation gates only when every branch
  demands a safe needle), so `TodoWrite.*legacy|legacy.*TodoWrite`
  runs 1.70x faster wall / 1.9x less CPU unscoped with byte-identical
  output; a space-carrying plain pattern now anchors its longest
  whitespace-free run.
- files + recover: the file-history snapshot instrument. Claude Code
  rewrites its settings files in-process (/model, /config, plugin
  toggles) with no tool record - measured, half of all settings.json
  mutations are invisible to the tool stream, and one such write
  silently deleted a freshly-edited key while recover replayed the
  file WITH it, calling a never-existed state 100% complete. CC's own
  per-prompt snapshot version sequence is now read as an instrument:
  recover compares the replayed buffer to mtime-verified snapshot
  content at every version change and REBASES on divergence (an
  authoritative external_write boundary; content-less jumps disclose
  the same boundary without rebasing), and files emits
  "external write" timeline rows - scope hard-limited to the settings
  family (.claude/settings*.json): the tracked set spans 1701 corpus
  paths against 11 settings-family ones. The version counter resets
  mid-session (148 real cases) and the @vN store name collides across
  a reset, so generations are segmented and unverified blobs refused.
- search: bare role selectors now speak LLM-visibility - a
  correctness fix. user.unsent under -t user broke a 0.7-era consumer
  (a superseded draft 12 seconds before the real submit poisoned a
  last-human-touch hook). A bare role (-t user) selects the role's
  LLM-visible leaves only; a new glob form (-t 'user.*') selects
  every leaf under the prefix; intermediate prefixes and full paths
  keep their full sets, so -t harness.compaction still reaches the
  boundary. Exactly two leaves are invisible, instrument-verified:
  user.unsent (CC's own preservedMessages accounting excludes every
  draft uuid - a draft is not in the surviving conversation) and
  harness.compaction.boundary (a metrics-only system record).
  -t user restores the 0.7 contract; 0.9.2 through 0.9.3 briefly
  included drafts under it.
- SKILL: the staleness guard is mechanical (run csift --version at
  first use after any compaction; a mismatch means the in-context copy
  is a stale echo), the description gains the corpus-first trigger
  (live sessions need /reload-skills), and two stale claims are fixed
  (the turns rename is v0.4; the retired @trap retry ritual is gone
  from the whoami section).

## [0.9.3] - 2026-09-01

Help corrections and a documentation catch-up; no behavior changes.

- The root help's hand-written SUBCOMMANDS block still listed eleven
  commands, contradicting the generated list one screen below: status
  and wait join it and the span-default enumeration.
- The status/wait help described owner-process liveness as a signal-0
  probe - a mechanism that never shipped. Corrected to the ps-based
  probe with the /proc fallback (the 0.9.1 mechanism).
- The --siblings policy text gains the narration cap of 1 shipped in
  0.9.2, in the help and in every reference document.
- README catches up two releases: thirteen subcommands with status and
  wait rows, a live-status highlight, Quickstart rows for status /
  wait / user.unsent / the label census, and the 0.8.1+ flags in their
  rows. SPEC gains per-command sections for status (6.13) and wait
  (6.14) and rewrites the stats section to the deduped token
  accounting; SKILL's superseded-draft bullet and JSON reference catch
  up to 0.9.2.

## [0.9.2] - 2026-08-31

Narration-aware classification plus a token-accounting correction. Saved
numbers move in two places, both on purpose: label censuses split, and
stats token totals drop.

- New label `agent.thinking.narration` (taxonomy 26 -> 27). Since at
  least Claude Code 2.1.170 the API can return a SECOND thinking block
  in an assistant message: a one-sentence, user-language summary of the
  reasoning beside it, distinguished only by a tag encoded inside the
  base64 `signature` (protobuf field path 2 -> 1 -> 8, LAST field at
  each level; clients 2.1.241+ render it dim under the hint word
  `summarized`). csift counted these as reasoning. Classification is by
  signature alone (about 4 percent of narration blocks have no
  reasoning sibling, so adjacency is never consulted); the whole
  signature is decoded (they reach 200K+ base64 chars); every failure
  path degrades to plain `agent.thinking`; the tag set is open. Hits
  carry a `[narration summary]` marker in the label zone; narration
  gets a sibling cap of 1; `verbatim` never replays narration (already
  true by construction, now pinned). `-t agent.thinking` still selects
  both leaves; pure reasoning is `-t agent.thinking -T
  agent.thinking.narration`. Historical records re-classify BY DESIGN:
  narration blocks exist on disk from 2026-06-10, so saved
  `--count-by label` figures change, conserving the sum.
- New label `user.unsent` (taxonomy -> 28). A message that was sent,
  esc-recalled into the input box, edited and re-sent leaves the
  ORIGINAL on disk, sharing the resend's parentUuid - and since 0.8.2
  csift collapsed it to a bare count. Drafts are now searchable and
  censusable under their own leaf: a matching draft renders as its own
  annotated unit (`<tok>·draft`, JSON `superseded_draft:true`, null
  `turn_index`, hits labeled `user.unsent`), turn numbering is
  untouched, `--turn` windows suppress draft units, and `user.message`
  counts are unchanged (drafts were never censused before). Documented
  limits, both measured: a recalled-then-abandoned message has no
  resend sibling and is structurally undetectable; a QUEUED text edited
  before dispatch never becomes a user record at all - the bytes
  survive only in `queue-operation` lines, which csift treats as
  non-records (raw-reachable; a future release may gate them).
- `stats` token sums are corrected: Claude Code repeats the identical
  `message.usage` object on every per-block record of one API message,
  and stats summed per record - an over-report measured at 2.2x to 3.5x
  per field and model. Sums now dedupe per transcript by `message.id`
  (per-field MAX, immune to the compaction-replay shape that rewrites
  an id with zeroed usage); id-less records count individually as
  before; the scope TOTAL still sums transcripts (the same id recurs
  across a session's transcripts with genuinely different per-file
  usage). Printed totals drop accordingly.
- `stats` gains a narration census: `narration_blocks` per model (block
  counts only - the token split is not derivable from the jsonl, so
  none is invented) and `unknown_thinking_tags` (a signature tag that
  is neither `thinking` nor `narration` surfaces without a csift
  release).
- The narration decode is byte-gated on the hot path (a signature
  containing none of the tag's three base64 alignments skips the
  decode), keeping large-corpus search at its previous speed.
- Help staleness swept: the `-t` reference said "25 leaves" and omitted
  `harness.meta.attachment` since v0.8.1.

## [0.9.1] - 2026-08-30

- Fixed: pid liveness on busybox-ps hosts (Alpine and friends). busybox
  `ps` rejects `-p` and the `lstart` field outright, so the probe's ps
  form failed for a LIVE pid exactly as for a dead one, and `status`
  read a live session as `stale-dead`. When the ps form fails the probe
  now consults `/proc/<pid>` on Linux: present means alive with the
  start time unknown (the reuse-guard skip stays disclosed); absent
  keeps the no-such-process verdict. Found by the release matrix's musl
  test lanes.

## [0.9.0] - 2026-08-30

A new command class: `status` and `wait`, the live-truth pair. Every other
command answers "what happened" reproducibly; these two answer "what is
happening NOW", are point-in-time, and are explicitly non-reproducible - a
deliberate, documented departure from the forensic contract.

- `csift status <target>`: one-shot liveness verdict for a session -
  `running` | `waiting-children` | `waiting-hitl` | `idle-eot` |
  `stale-dead` | `unknown` - from a three-way join, never a single-surface
  inference: the harness session registry (`<claude-home>/sessions/
  <pid>.json`, transition-writes only, never a heartbeat), the transcript
  tail state machine (an unreturned tool call at the tail = a tool in
  flight), and a `ps`-based owner-pid probe guarded against pid reuse by
  the process start time (the registry renders it UTC, `ps lstart` renders
  it local; both parse as instants). Child liveness joins each subagent
  transcript's own tail with the incremental workflow journal (`started`
  minus `result` = agents in flight). The elicitation sidecar covers
  human-in-the-loop blocks. Every verdict ships its evidence rows, and
  every degradation is stated in the output: a skipped reuse guard, a
  missing registry row, the invisible pending permission prompt.
- `csift wait <target> --until COND[,...]`: block until a condition fires,
  first hit wins. The closed condition set: `stop`, `hitl`, `auq`,
  `notification[:REGEX]`, `tool:NAME[:REGEX]`, `write:PATH_RE[:LINE_RE]`,
  `verdict:V`. STRICT post-start baseline semantics: only bytes appended
  after the watch starts count as events; history is `search`'s job. A
  readiness line on stderr makes scripted waits race-free against their
  own trigger. Polling is incremental (byte offsets, torn tails held) and
  adaptive (200ms floor to 2s ceiling; `--interval` overrides); child lanes
  and the elicitation sidecar born after the watch starts join it
  automatically with a zero baseline.
- Exit codes: `wait` exits 124 on `--timeout` expiry (the GNU `timeout`
  convention) - the ONE documented exception to the crate's 0-vs-non-zero
  exit law; it applies to no other command.
- `Message.stop_reason` joins the tolerant record model.

## [0.8.2] - 2026-08-30

Field-incident fixes plus one soundness correction. Every item traces to a
measured incident or a live re-measurement; two items correct csift's own
documentation and output where they stated a wrong mechanism or a
fabricated certainty.

- Targeting: a bare-basename `*.jsonl` token (no path separator) now
  resolves and classifies correctly on every command; it used to fail with
  a wrong error, and a bare `agent-<hex>.jsonl` was misread as a top-level
  session.
- The @trap timing mechanism was documented wrong and is corrected
  everywhere, including the runtime error text: a subagent transcript
  flushes per content block (on disk at dispatch, first try resolves); the
  main conversation's record is an async flush of the completed assistant
  message landing about 1-3.4 seconds after dispatch - a race, not a wait.
  The no-match error now routes `@main` first, and a @trap that resolves
  to the main transcript prints a stderr lane note instead of succeeding
  silently.
- Lane honesty: bare `whoami` reported `is_subagent:false`, `depth:0`, and
  an echoed parent id from inside a subagent - three confidently wrong
  fields on exactly the command an agent runs to check its identity. The
  env form now reports those fields as null (the env names the top-level
  session in every lane, so the answer is unknowable), prints a lane line
  in text, and notes the resolution path on stderr; every `@main`
  resolution prints the same unconditional stderr note.
- Image discoverability: the first image-bearing row per run (search and
  show) carries a paste-ready extraction hint in input id forms (a `#N`
  handle as the bare number); the search footer gains a capability note;
  `search --help` and `verbatim --help` gain SEE ALSO sections naming
  `csift image`. Driven by a fleet incident that published "images are
  unreadable" without ever testing extraction.
- Errored tool results are visible: hit JSON carries `is_error`, the text
  render decorates an errored result `[error]`, and a new `--count-by
  result` axis buckets `ok` | `error` (the closed `pairing` enum is
  untouched: pairing answers "did a result come back", result answers
  "was it good").
- Superseded-draft honesty: the esc-edit draft collapse in turn
  reconstruction is now disclosed (search footer + JSON
  `superseded_drafts`), an explicitly addressed draft fetches via
  `show --line`/`--uuid` as an annotated unit outside turn numbering, and
  the address-miss error states the real render domain. Previously a
  multi-megabyte genuine user record could vanish from every scan with no
  count and fail an addressed fetch with a wrong reason.
- SKILL: a new "Why not hand-roll this format" section (nine measured
  traps, each returning a plausible wrong answer with no error, each
  mapped to a csift move) placed before the routing table; the frontmatter
  description now carries the cost of skipping csift and names the image
  capability.

## [0.8.1] - 2026-08-27

A maintenance round: additive surfaces plus one correctness flip, each
verified against real corpora (and the live on-disk stores) before
implementation. Two proposed features failed that verification and shipped
as fact-reporting surfaces instead: a live/abandoned rewind-branch
classifier false-positived on parallel tool fan-out across most sessions
(shipped as `show --branch-points`, facts ranked by inter-child gap, no
verdicts), and a file-history reconstruction merge was cut down to a
listing after the store proved pruned with reused version counters
(shipped as `recover --list-backups`).

- `list`: `version` and `git_branch` now report the LAST-seen value (what
  the session is on now; the docs always promised the session's version,
  and a mid-flight upgrade or branch switch previously reported stale
  opening samples). The opening values ride new `version_first` /
  `git_branch_first` fields, JSON mirrors `*_last`, and text shows a drift
  arrow (`branch a->b, CC x->y`) when they moved. `cwd` stays first-seen
  on purpose: the record cwd follows the tracked shell cwd.
- `search --attachments` + the `harness.meta.attachment` label +
  `--count-by attachment`: attachment records (the bulk of many
  transcripts' bytes) become searchable behind an explicit gate, a
  superset of `--additional-context`; the matchable text is the verbatim
  payload JSON; the census axis implies the gate. A default scan still
  never parses attachment lines; an explicit `show` address renders any
  attachment record flag-free. The label taxonomy grows to 26 leaves.
- `search --count-by version`: a per-record census of the Claude Code
  version stamp (which versions a session ran under, where an upgrade
  landed); stampless records are excluded and disclosed.
- `stats`: a whole-file line-type census (`types` line, JSON `line_types`,
  merged scope totals) counting every physical line by its top-level
  `type`; a file fact like `lines`, never windowed. The probe fully
  validates non-candidate lines, so a framed line with an invalid interior
  is now counted malformed even off the candidate path.
- `recover --list-backups`: lists Claude Code's own file-history
  checkpoint store for an absolute `--file` (store key sha256 of the
  path), ordered by backup instant, with the provenance bounds stated in
  the output: tool-layer writes only, pruned, version counters reset per
  session dir. Listing only; checkpoint content is never merged into a
  reconstruction. Four doc sites claiming `backupFileName` is frequently
  null were corrected (measured 83-98% present).
- `show --branch-points`: every record with two or more conversation
  children (a rewind, retry, or parallel lane), children with lines and
  timestamps, ranked by the widest inter-child time gap; tool-result
  carriers, isMeta records, and compaction summaries never count as
  children. Facts only: csift ranks, never classifies which side is live.
  The compaction boundary's `logicalParentUuid` (the true predecessor the
  compaction re-links to) now rides the boundary's rendered excerpt.
- `plan`: binding output gains the plan `slug` (read off the bind record);
  `plan --audit` joins the scope's structured plan-file mutations against
  the corpus's plan bindings and warns when the mutating session does not
  bind the file (only the bound plan is re-injected after a compaction).
- `agents`: fork provenance (`fork_parent_last_uuid`, `fork_context_length`,
  a `forked-at` text line) from the `fork-context-ref` record a `/fork`
  transcript opens with, and a repeatable exact-match `--agent-type`
  filter (`--agent-type fork` lists fork children).
- Out of scope, recorded: the live team/task coordination files stay
  unread; the transcript is the durable record.
- New dependency: sha2 (the checkpoint-store key).

## [0.8.0] - 2026-08-22

Bash file mutations now resolve against the shell cwd Claude Code itself
records, and every `recover` output accounts for what the replay could not
include. Grounded in a full-corpus investigation: 22,410 real Bash commands
joined against csift's own output, and the cwd + freshness mechanisms
extracted from the Claude Code 2.1.237 binary and validated on 18,185
commands (SPEC 4.9).

How much improved, measured on that corpus: 19.4% of Bash calls mutate a
persistent file; csift's full-target attribution of those mutations rises
from 27.1% to a measured 77.0% ceiling; `recover`'s relative-operand vs
absolute `--file` join closes from 95.84% to 99.65%.

How it stays deterministic: nothing is guessed. Every resolved path carries
an explicit resolution class - `absolute` (typed absolute), `cwd-joined`
(joined to the record's own `cwd` field, data Claude Code wrote, zero
inference), `cd-tracked` (literal in-command cds, a lexical inference
validated at 99.65% against Claude Code's own modified-file hints), or
`unresolved` (kept verbatim and disclosed, never fabricated into a path).
Commands whose file sets are not in the command text are counted and
disclosed, never attributed.

- `files`: a bash row's `path` is the resolved spelling, so relative and
  absolute spellings of one file share every bucket; timeline JSON rows gain
  `resolution`, `path_verbatim`, `command_errored`. Mutations from a
  partially failed bash chain are kept and flagged instead of dropped;
  `git apply --check` / `git clean -n` dry runs and rsync remote
  destinations no longer emit rows.
- Parser increments: `perl -i` (the `sed -i` twin); interpreter write idioms
  (python/node/ruby heredoc and inline scripts) with literal and
  one-hop-constant targets as real rows and an `interp:<lang>` marker
  otherwise; leading-`~` operands kept verbatim as `unresolved` instead of
  dropped; mutating-class markers `fmt:<tool>`, `pkg:<manager>`,
  `extract:<tool>` in the `git:<sub>` style (dry runs emit nothing; a
  formatter with named operands emits real rows).
- `recover` joins bash events on resolved paths (verbatim kept as a belt)
  and discloses, per window and in every mode: integrity boundaries with a
  hard/soft split, opaque mutating-class and PowerShell command counts and
  rows, and a ready-to-run time-bounded `csift search` command. Restore's
  status states a clean window positively, or says "complete from the tool
  stream; NOT verified against disk" and lists what was not replayed.
- Claude Code's own freshness signals are adopted: `staleReadFileStateHint`
  (Claude Code names the files a shell command modified) becomes a hard
  `hint_modified` boundary; `staleRecovered` on a successful Edit becomes a
  `stale_recovered` annotation; the over-budget `edited_text_file` form is
  named; an external-edit boundary names a formatter-class command that ran
  in its window. `String to replace not found` and `File does not exist`
  are counted annotations; a soft bash boundary no longer disarms the
  originalFile cross-check.
- Breaking: the batch `recovery-report.tsv` gains `boundaries`, `bash_file`,
  `bash_opaque` columns; restore's status lines and failure diagnostics are
  reworded (an invalidated history no longer claims "never Read/Written/
  Edited"; an empty salvage names "at the latest state"); restore's JSON
  error paths emit their row and summary before the non-zero exit; boundary
  JSON rows gain `source_session_id`/`source_line`; coverage rows gain
  `hard_boundaries`/`soft_boundaries`/`opaque_commands`/
  `powershell_commands`/`suggested_search`.

## [0.7.8] - 2026-08-17

- `recover` finds windows-shaped paths (drive letters, backslashes) again: the
  file-level basename prefilter split on `/` only, so such targets silently
  reported no history; the basename-suffix match also accepts a backslash
  boundary now.
- First release validated on all three platforms: the full test suite passes
  on macOS (arm64/x64), Linux (glibc and musl, x64/arm64), and Windows (MSVC
  arm64/x64).

## [0.7.7] - 2026-08-17

- Help text reworked for plain punctuation across every `--help` page; flag
  semantics, examples, and JSON schemas unchanged.
- Source comments are ASCII-only (enforced by the pre-commit gate); output
  glyphs and test fixtures in string literals are unaffected.

## [0.7.6] - 2026-08-17

- `search --additional-context` — opt-in scan of hook-injected
  additionalContext (the attachment records a SessionStart /
  UserPromptSubmit / ... hook writes into the transcript). Off by default;
  hits surface under `harness.meta.hook`; an explicit `show --line`/`--uuid`
  address renders such a record without the flag, so the refetch a search
  hit prints always resolves. A default scan pays nothing for the widening.
- README restructured scenario-first (why -> highlights -> install, the
  pronunciation under the title, the agent-skill install beside the binary
  install); SKILL documents the file-mtime semantics of Claude Code's
  `cleanupPeriodDays` retention; Conventional Commits codified in AGENTS.md;
  .gitignore gains editor/OS/local-settings rules.

## [0.7.5] - 2026-08-16

- Published to crates.io — `cargo install csift` is now the primary install
  path. Cargo.toml gained the publication metadata (repository, readme,
  keywords, categories) and dropped the `publish = false` guard; the
  `CLAUDE.md` symlink is excluded from the crate tarball (it would
  dereference into a duplicate of `AGENTS.md`).
- README documents the name — pronounced "c-sift", in the `csplit`/`ctags`
  naming tradition: c for Claude Code, sift for what it does — and the
  crates.io install. No CLI surface change.

## [0.7.4] - 2026-08-12

- The Windows shell is a SEPARATE Claude Code tool named `PowerShell` (same
  `input.command` field; enabled by env override, forced on when
  Git-for-Windows bash is absent, else feature-gated — extracted from the CC
  2.1.228 binary; the Windows `Bash` tool runs the real Git-for-Windows
  bash). `@trap` self-identification now matches BOTH shell tools — it was
  blind exactly in the bashless Windows fallback mode. Error/retry guidance
  says "shell (Bash / PowerShell) invocation".
- Documented deliberate non-changes: the bash-lexical layers (dangerous-rm
  escalation classification, shell-side mutation attribution in
  files/recover) do not run on PowerShell records — a pending PowerShell
  lane classifies awaiting-execution; structured Read/Write/Edit attribution
  is unaffected. Also recorded: CC 2.1.228's dangerous-rm has evolved past
  the ported generation (fixpoint substitution stripping, a tree-sitter bail
  at 64+ command substitutions) — a port refresh is a tracked follow-up.

## [0.7.3] - 2026-08-12

- Path encoding is now EXACTLY Claude Code's (evidence extracted from the CC
  2.1.228 binary): the cwd is NFC-normalized, then replaced per UTF-16 code
  unit — an NFD-spelled accented path now encodes identically to its NFC
  spelling (closing a formerly-documented divergence that resolved the wrong
  dir on macOS NFD paths), and an astral char yields two dashes, matching the
  JS regex's view.
- Windows drive-encoded project dirs (`C:\Users\x` → `C--Users-x`,
  letter-led) are first-class targets: both the bare positional token and
  `@C--Users-…` resolve; a drive-shaped token matching no projects dir falls
  through to real-path resolution. A UNC-encoded dir (`--server-…`) is
  targeted via the `@` form (the mistyped-flag guard's error now says so).
- Verified from the same binary, no code change needed: CC's config home is
  `CLAUDE_CONFIG_DIR ?? os.homedir() + "/.claude"` (NFC-normalized) — Windows
  never consults `HOME`, confirming the 0.7.1 per-platform split.

## [0.7.2] - 2026-08-12

- Performance round (behavior-identical — a 28-command byte-exact A/B battery
  pins stdout, stderr, and exit codes unchanged): every per-line byte
  prefilter now uses construct-once memmem finders (the stateless form
  rebuilt its searcher on every call); the parallel line scanner no longer
  runs a serial whole-file newline count (skipped outright for single-chunk
  files, computed in parallel otherwise); `search`'s per-turn match+render
  phase fans out on rayon when the scope is small or the file is 64 MB+ (the
  straggler class), gated so broad scans keep the serial walk. Measured warm
  on the reference corpus: big-session census 1.21x, no-match unscoped
  1.13x, caseless literal 1.09x, verbatim 1.08x; user CPU down 2-4 percent.
- README gains a coverage badge (94.8 percent line coverage,
  cargo-llvm-cov over the full suite).

## [0.7.1] - 2026-08-12

- The default data root resolves per platform, the way Claude Code's own
  `os.homedir()` does (correctness): `$HOME/.claude` on Unix,
  `%USERPROFILE%\.claude` on Windows — `HOME` is never read on Windows, so a
  stray Git-Bash/MSYS `HOME` (often a POSIX-style path a native process cannot
  open) no longer points csift at a `.claude` dir Claude Code never writes.
  Precedence is unchanged: `--claude-home` > `$CLAUDE_CONFIG_DIR` > the OS
  home's `.claude`. The error message and `--claude-home` help name both
  variables.

## [0.7.0] - 2026-08-12

Breaking text-surface release; JSON output is unchanged.

- **Breaking:** `search` exchange headers are self-resolving. Each header opens
  with a STABLE id-prefix token — the first 8 chars of the owning transcript id
  (`<tok>·t<N>`) — instead of a per-invocation `sN` ordinal, and the
  `sN = <id>` session legend block is removed entirely. A token is a valid `@`
  target as-is and identical across invocations; within one output, distinct
  ids sharing their first 8 chars lengthen together (8 → 12 → full id); a
  teammate id (name-embedded, not hex-led) renders whole. A subagent exchange
  carries `(parent <first-8>)` on EVERY header.
- Resolver widening so every emitted token round-trips (all fail-loud on
  ambiguity): the `@`-prefix match domain is the UNION of top-level session
  uuids and subagent agent ids; a literal `8-4-4-4-12`-layout prefix longer
  than 11 chars is a valid uuid-prefix token; a 12+-hex token keeps
  exact-agent-id semantics first, then falls back to a unique literal-prefix
  match.
- Output geometry: a head `matches` banner (true totals · `oldest first` · the
  emitted window · `undated last` when present) follows the scope banner; the
  tail footer repeats the TRUE pre-cap totals beside its drop accounting; the
  stderr zero-match diagnosis discloses the malformed-line count (an absence
  claim is definitive for parseable lines only). The both-ends placement law
  joins SPEC section 0 as a crate-wide design law.
- `--max-count` is SIGNED: `N` keeps the EARLIEST N of the chronological
  stream, `-N` the LATEST N, `0` stays uncapped; the kept exchanges still emit
  oldest-first among themselves. Both ends disclose the window; the footer
  names the dropped side (`N later|earlier dropped by --max-count`).
- Docs: an OUTPUT GEOMETRY section in `search --help` and SKILL; recipes for
  "when did X first happen" (`--max-count 1`), "most recent occurrence"
  (`--max-count -1`), and the header-token follow-up into `show`.

## [0.6.10] - 2026-07-14

- `@trap` retry guidance states the granularity: the retry must be a NEW,
  SEPARATE Bash invocation — two attempts inside one shell script are still one
  in-flight tool_use, so both miss (error text + SKILL + assumption table).
- Documented: EVERY `tool_use`'s matchable text is its name + the re-serialized
  JSON input, so an embedded real newline is the two-character `\n` by match
  time — match the literal `\\n`; `--multiline` is correctly irrelevant there.
- SKILL completeness: verbatim header fields `automation_triggers`,
  `budget_is_per_session`, `sessions_rendered` named; the self-echo trap
  recorded (a nonce used as a search pattern writes itself into your own live
  transcript — scope absence checks away from your own session).

## [0.6.9] - 2026-07-14

- Stage-1 candidate detection is serialization-tolerant (correctness): a
  valid-JSON record whose serialization differs from the compact wire format
  (whitespace around the colon — python `json.dumps` defaults, a jq/editor
  round-trip) used to vanish one layer BEFORE any malformed counter: no match,
  no count, zero disclosure. The role needles now route through shared
  whitespace-tolerant matchers; every other prefilter needle is
  serialization-safe by construction, and the needle law is codified in
  AGENTS.md. Framing is unchanged: one record per line.

## [0.6.8] - 2026-07-14

- `list`/`agents` head+tail scans no longer double-book malformed lines: the
  tail scan floors at the head scan's consumed end, so the two windows are
  disjoint and every malformed line in them is counted exactly once (an
  all-garbage file used to report exactly 2×).
- `list`'s malformed count is a DISCLOSED window census, never a whole-file
  verdict: the note reads `… skipped (among the head/tail lines read — full
  census: csift stats)`, and `stats` is named the full-scan census authority.
- A sidecar marker line the current schema cannot read (a pre-release fossil
  under old field names) is counted as malformed — provably-ours yet
  uninterpretable never buys silence.

## [0.6.7] - 2026-07-14

Doc-only convergence round.

- SKILL names verbatim's two automation header fields: `automation_by_kind`
  (the SELECTED triggers per class) vs `automation_in_scope_by_kind` (every
  in-scope pulse regardless of budget).
- `agents` `returned_message` semantics stated: it is the ORCHESTRATOR's
  record of the child's return, not the agent's own conclusion; the child's
  final words are always `show @<agent-id> --turn -1..`.

## [0.6.6] - 2026-07-14

- Obviously-corrupt lines are COUNTED (correctness): every byte-prefilter
  rejection path runs an O(1) shape check (non-blank but not `{…}`-framed ⇒
  malformed), so free-text garbage and crash-truncation move `skipped_lines`
  on every command. Documented residue: a `{…}`-framed invalid INTERIOR is
  only counted on a parse candidate.
- `verbatim`'s header reads `spanned K of N compaction boundaries in scope`
  (K alone read as a transcript property); its JSON header carries the full
  budget accounting (`round_trip_fraction`, `chars_used`, `boundaries_*`,
  `selected_*`).
- Docs: under `--turn`/time windows every `stats` figure windows EXCEPT
  `lines`; an hours-old `awaiting-execution` lane is overwhelmingly an
  abandoned parent session — weigh `pending_since_utc`; `--siblings` caps
  apply to NON-matching context records only.

## [0.6.5] - 2026-07-13

- Bare ISO datetimes are LOCAL wall-clock time (correctness): `--since
  "2026-07-13T20:00:00"` used to collapse silently to local midnight (the
  civil-Date parser kept only the date part). A civil-DateTime arm now
  precedes the Date arm; a string carrying a malformed offset still bails.
  One fix covers every `--since`/`--until` consumer.
- The id trio (`session_id` / `is_subagent` / `parent_session_id`) rides EVERY
  search hit and sibling object, so bare `.hits[]` flattening keeps real ids.
- Advisory notes fire AFTER target resolution — never a warning about a run
  that was never going to happen.
- SKILL: the missing `plan` / `recover` / `image` JSON row schemas added;
  `@trap` marker uniqueness stated as conversation-wide.

## [0.6.4] - 2026-07-13

- The removed `turns` name gets a tombstone error: a hidden variant always
  bails naming the rename (`verbatim`, same flags) and routes plain turn
  reading to `show <target> --turn -3..`. A wall, never a shim — it never
  runs.
- `agents` text brands a non-completed lane's `returned_message` inline
  (`history — predates the still-open lane, NOT the outcome`); a completed
  lane stays unbranded.
- Docs: a workflow RUN row's `status` is journal-verbatim (an open set, not a
  csift enum); the richest-view dedup rule stated mechanically (`labels[]` is
  richest-first; the rendered view is the first label surviving `-t`/`-T`);
  exit codes de facto (usage errors 2, csift errors 1 — the contract stays
  0-vs-non-zero); the record-level jq pipeline idiom (select in jq, run the
  csift-generated `refetch`).

## [0.6.3] - 2026-07-13

- Elicitation-sidecar GHOST-PENDING guard (correctness): Claude Code fires no
  PostToolUse for a REJECTED AskUserQuestion/ExitPlanMode, so the hook can
  never write `resolved` there. A pending whose key appears on a native record
  as an actual `tool_use` block id / `tool_result` id (structural check) is
  dropped like a resolved pair — the native transcript outranks the sidecar.
- `list`'s scope banner / JSON `sessions_in_scope` report the PRE-cap resolved
  range; the flood guard caps only the rows.
- `--count-by label` census keys pass the active `-t`/`-T` predicate — a
  dual-labeled record no longer leaks its filtered-out twin into the keys.
- `show` rejects the span pair with the single-transcript rule; legacy flat
  `-t` values (`thinking`/`tool`/`tool-response`) name their successor path.

## [0.6.2] - 2026-07-12

- `image --id` miss error explains itself: it names the handles PRESENT,
  states that `#N` is inherited from paste-time numbering (holes and non-1
  starts are source gaps, not csift drops), and routes to the plain listing.
- The three count units are cross-referenced where the numbers collide:
  `-c` counts EXCHANGES · `--count-by` counts RECORDS (a tool call + its
  result carrier ⇒ ≈2× the call figure) · `stats` tools count CALLS.
- Docs: the jq merge idiom for flattening hits with their exchange-row ids;
  `select(.kind==…)` before projecting; `returned_message` is the NEWEST
  message the child ever returned (on a frozen lane it predates the pending
  call).
- Every subcommand's `long_about` was dead text — now rendered.

## [0.6.1] - 2026-07-12

- An unrecognized `@`-token is a HARD error naming the @-grammar — it never
  falls through to path resolution (a stripped `@a` used to become a
  cwd-relative path with a misleading project-dir error); a 1-3-char hex token
  gets the dedicated too-short-for-a-prefix message.
- `@trap` main-thread timing documented and routed by the error: a subagent's
  transcript records the launching tool_use eagerly, but the MAIN
  conversation's record flushes only after the current Bash call completes —
  a top-level first use always misses; `@main` for the main thread, re-run
  the SAME marker otherwise.
- Docs: `--count-by model` reports the raw `<synthetic>` key verbatim; text
  excerpts keep literal newlines (`| head -N` can cut mid-record — the
  line-safe machine form is `--format json`).

## [0.6.0] - 2026-07-12

- **Breaking (agents JSON):** `completed_utc/_local` (+ `duration`) are
  non-null ONLY when `status == "completed"` (a frozen lane is never "done");
  every timestamped lane gains the `last_activity_utc/_local` pair (the tail
  newest-record instant).
- `show`'s TARGET is a Vec so a mistyped or foreign `--flag` is rejected BY
  NAME instead of being consumed as the target; two real targets get a
  pointed one-transcript arity error.
- Censuses count RECORDS, not per-section hits — a leaf tally now equals
  exactly what `-t <leaf>` surfaces.
- `pairing` rides the tool BLOCK through the communication views: a frozen
  SendMessage is `pending` under ANY selector ("any pending tools?" needs no
  `-t`).

## [0.5.2] - 2026-07-11

- `search --help`'s COUNT section says the `-c` integer is the EXCHANGE total
  and routes session listing to `-l`.

## [0.5.1] - 2026-07-11

Help-parity release; behavior unchanged.

- The five-document contract: `SKILL.md` = the LLM manual · `--help` = the
  human (CLI-proficient) manual, information-parity with SKILL · `README.md` =
  promotion · `SPEC.md` = design intent · `AGENTS.md` = maintenance.
- Root `--help` gains the human-toned sections (the rules every command
  follows, JSON output, pitfalls, non-goals, retention); `search --help`
  gains the full 3-role / 25-leaf label taxonomy; `show`/`stats`/`plan`/
  `whoami`/`image` gain JSON SCHEMA sections; every `--sessions-from` help
  states the span rules; `whoami`'s composition example matches the flat
  envelopes.

## [0.5.0] - 2026-07-11

Breaking rework, zero backcompat.

- The per-command turn-window flag is `--turn` everywhere (was `--turn-range`);
  same range grammar, same AND-intersection with `--since`/`--until`.
- The label census generalizes to `--count-by <AXIS>` with six closed axes:
  `label` · `tool` · `turn` · `session` · `pairing` · `model`; JSON row kind
  is `census`; records outside an axis's domain are excluded and reported.
- `agents --format json` is FLAT (envelope v2, no exceptions): session → run →
  agent rows in tree pre-order; nesting is text-only, rebuilt from
  `parent_agent_id`/`depth`; an unreachable node is appended, never dropped.
- `show`: an EXPLICIT `--turn` miss is a hard error naming the domain
  (open/from-end forms clamp); a 200-record-unit flood guard with the exact
  continuation command; `--max-count 0` = uncapped uniformly on
  list/stats/search/show.
- Timestamps (text) take the ONE canonical local form
  `YYYY-MM-DD HH:MM:SS[.mmm] TZAB(UTC±offset)` — the second UTC copy is gone.
- Slash-command wrappers detected in BOTH tag orders (`<command-message>`
  first is current CC); a new-order wrapper no longer masquerades as human
  prose or opens a turn.
- `list` rows gain `sidecar_present` (tri-state elicitation evidence); `files`
  JSON summary gains `sessions`; `verbatim` prints a per-session
  no-compaction note routing to `show --turn`; `normalize_argv` locates the
  subcommand by scanning past root flags — flag order is free in combination
  with `--claude-home`.

## [0.4.1] - 2026-07-11

- Version + tag discipline codified: `Cargo.toml` ≡ SKILL surface header ≡
  `csift --version` move together in the same commit; every release gets an
  annotated `vX.Y.Z` tag; `--help` text is release surface. This release
  bumps for the v0.4 round's `--help` corrections.

## [0.4.0] - 2026-07-11

Breaking rework, zero backcompat.

- `turns` is renamed `verbatim` and reframed as the compaction-fidelity
  specialist (restore the verbatim turns a compaction summary clipped);
  tail-peek reading moves to `show --turn` — a third addressing mode that
  fetches EVERY record of the named turn(s) (`-3..` = the last 3).
- ONE range grammar everywhere: `N` · `A..B` · `N..` · `..N` · `-k` from the
  end — all inclusive, resolved per target; the dash form `A-B` hard-errors
  teaching the `..` spelling.
- `search --count-by-label`: a per-leaf label census terminal mode (empty
  pattern = whole-scope census; a leaf's count = what `-t <leaf>` would
  surface); JSON `label_count` rows.
- Empty-result self-diagnosis: a zero-match run prints a stderr diagnosis —
  "a DEFINITIVE absence (exit 0), NOT an error", the active filters, and an
  active probe naming the label(s) the pattern DOES occur under; JSON summary
  gains `definitive_absence` / `active_filters` / `excluded_by_label`.
- Flood guards: an unscoped all-projects `list` caps at the 50
  most-recently-active rows (drop reported; `--max-count` overrides); `stats`
  gains an opt-in `--max-count`.
- JSON rename (search summary): `session_ids` → `transcript_ids`
  (+ `transcript_ids_truncated`) — named apart from `-l`'s owning-session ids.

## [0.3.0] - 2026-07-11

- `-T`/`--label-not` (search): label EXCLUSION with the same selector grammar
  as `-t`; richest-SURVIVING-view dedup; statically-empty combos hard-error.
- `--sessions-from <FILE|->` on every multi-target command (union an id list
  into the scope; an explicit empty list = an empty scope); `search -l` emits
  the matching owning-session ids to pipe into it; `search --raw` emits
  matched records' verbatim jsonl lines (stdout pure, notes on stderr).
- Search JSON hits + verbatim collapsed-agent rows carry `refetch` — the
  ready-to-run `csift show` command addressed at the line-owning transcript.
- The turn window and `--since`/`--until` INTERSECT on every command;
  `verbatim` REQUIRES a target (budget × every-session flood guard); `list`
  gains `--since`/`--until`.
- Teammate ids with dashed NAMES round-trip as `@<agent-id>` targets.

## [0.2.0] - 2026-07-10

Breaking ergonomics rework, zero backcompat — one way per intent.

- New `show` (§ record FETCH by `--line`/`--uuid`, rendered full or `--raw`
  verbatim jsonl bytes) owns fetching; `search --line/--uuid` are REMOVED.
  New `stats`: one-scan per-session aggregates (tokens by model, tool counts,
  turns, span, compactions).
- Envelope v2: EVERY `--format json` stream is ONE header line + kind-tagged
  rows + ONE summary line, no exceptions; the jsonl-line key is `line`
  everywhere.
- Flag surface: `-t`'s long form is `--label`; `agents --kind` → `--shape`;
  `recover --line-range` → `--file-lines`; the uniform span pair
  `--subagents`/`--no-subagents`; verbatim's five tuning knobs collapse into
  `--profile heavy|light`; `--siblings` is a zero-arg fixed policy;
  `image --id` takes bare digits or `L<line>i<n>`.
- Guardrails: a bare id target errors "did you mean '@<id>'?"; a search
  PATTERN starting `@` errors; a uuid-shaped pattern notes on stderr.

## [0.1.0] - 2026-06-07

- Initial scaffold of csift — "ripgrep for Claude Code session transcripts":
  a fast Rust CLI to list and regex-search the Claude Code session `.jsonl`
  transcripts under `~/.claude/projects/`.
