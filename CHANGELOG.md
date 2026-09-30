# Changelog

All notable changes to csift are documented in this file, newest first — one
entry per released version, written in that version's release commit. Pre-1.0
SemVer: a BREAKING surface change bumps the MINOR version; a non-breaking
surface change bumps the PATCH.

## [0.13.0] - 2026-09-26

### Added

- **A PDF page says which page it is.** A `Read` with a `pages` parameter returns one image
  element per requested page, so `csift image` has listed and extracted those pages all
  along - as rows indistinguishable from a screenshot. The page number is on no key of the
  element: the harness's own mapper writes a successful page bare and labels only a page it
  failed to render, and the offset lives once on the carrying record as
  `toolUseResult.firstPage`. csift derives the page as that offset plus the element's
  position among the content elements after the leading text, counted over POSITIONS rather
  than over images, so a failed page in the middle does not shift the pages after it. Text
  rows gain ` p<N>` beside the media type and the document's basename once per record group
  (one transcript, one line, one document);
  JSON rows gain `page` and `pdf_path`, the path verbatim as every other path field is.
  The derivation is gated on the record's own result reading `parts` with a `firstPage` and
  on the record carrying exactly one `tool_result` block, so a screenshot, a result written
  before that key existed, and the REPL tool's re-emitted inner-`Read` pages (which carry no
  offset and flatten several documents into one list) all read `page: null` rather than an
  assumed page 1. Addressing does not move: `--id`, `--turn` and the extracted filename are
  what they were, because the page is provenance and not an id. A whole-PDF `Read` returns a
  `document` block rather than images and stays unlisted. The same fact reaches the
  `agent.tool.result` unit as the exposed-keys family's `pdf_pages` - the label zone
  `[pdf pages A-B of <basename>]`, JSON `{first, last, count, path}` - read through the one
  function `image` reads, so `show --line` and `image` cannot disagree about which pages a
  record carries; the existing `[N images: ...]` annotation and its extraction hint are
  unchanged.


- **One session id under two project directories is merged, and the merge says what it
  dropped.** A renamed project whose directory was copied leaves one session id under two
  encoded directories, and Claude Code reaches the same shape two ways of its own (the
  `CLAUDE_CODE_PROJECT_DIR_NAME` override keeps the natural directory beside the overridden
  one and searches both; a relocation whose fallback mover strands its source leaves the
  transcript in the old place too) - its own resume lookup walks every project directory and
  returns nothing rather than choose. csift used to resolve both copies and count the session
  twice on every spanning surface, walking its whole sidecar twice with it. The copies now
  collapse at the shared resolver, decided by the BYTES: identical, or one a strict byte
  prefix of the other, merge onto the longest; copies that differ before the shorter one ends
  merge into nothing and both stay in scope, because a turn number minted from one does not
  address the other; a copy csift could not READ reads `unknown` and merges nothing either,
  naming that copy and the error the filesystem returned. An mtime is never consulted.
  The resolver names every merge on stderr,
  `list`/`search`/`stats`/`files`/`recover`/`verbatim`/`image`/`agents` carry
  `duplicate_transcripts` and `duplicate_records_dropped` in their JSON header - plus
  `divergent_transcripts`, the copies csift refused to merge, and `unreadable_transcripts`,
  the copies it could not read, because the first two read 0 both when nothing was duplicated
  and when a duplicate merged nothing - and a `copies` line
  in text, and a `list` row carries `also_at` plus `duplicate_relation`. `show` keeps
  refusing an address that resolves to two files and now names the paths. An `@<agent-id>`
  target takes the same merge: a copied project directory carries the whole sidecar, so one
  `agent-<hex>.jsonl` is on disk under two directories too.
- **The merge runs once over the whole resolved set, so one transcript is one file however
  many targets reached it.** `list @<uuid> @<agent-hex>` reaches one `agent-<hex>.jsonl`
  down two paths - gathered from the session's sidecar, and found by the agent target under
  both project directories - and merging each half separately let the two halves pick
  different winners, so both copies survived and each row named the other as dropped. A
  first pass still runs before the sidecars are gathered, so a dropped copy's whole sidecar
  is never walked; everything that pass produced then goes through one final merge, which
  reports only what it newly decided (a group that merged nothing is still there for it to
  find, and one group is one note). The agent lookup collects every copy instead of the
  first one the directory listing yields, and it finds them by probing each project
  directory for the owning session's basename rather than walking every sidecar twice.
- **One workflow run is one `kind:"run"` row, however many copies of the session discovered
  it** - and a run's identity is the pair (session, run id), not the id alone. A copied
  project directory carries the workflow manifests with it, so the same run was read once
  per copy and printed once per copy. A run id is unique inside the session that minted it
  and nothing makes it unique across sessions, so both the fold and the session a run
  renders under now key on the owning session: two sessions naming one workflow id keep a
  row each, where keying on the id alone dropped the second as though it were a copy.
- **`csift agents` merges duplicated agent transcripts, like every other surface.** It
  resolves top-level-only and walks the sidecars itself, so the resolver's merge never saw
  them: a divergent session pair whose two sidecars held one byte-identical agent transcript
  printed two lanes under a header saying nothing was duplicated, while `list` showed one row
  and `search -c` counted the records once. All three agree now.
- **Every `csift agents` row names its own transcript** (`path`, and a `transcript` line in
  the text tree for the ids that occur twice). Two copies of one session carry two copies of
  each agent transcript, and those rows are identical in every other field - the agent id
  included - so without the path a consumer sees two lanes it cannot tell apart.
- **`csift agents` counts what it prints.** The session count row's `agents` is now exactly the
  number of `kind:"agent"` rows under it, and the text tree prints that same set in that same
  order: both projections take one walk, whose visited set keys on node identity rather than on
  the agent id. Two copies of one session whose contents differ keep both sidecars in scope and
  their agent transcripts share an id, which the old walk deduped while the count row did not.
- **`csift list` emits one row per session, not one per copy**, and `sessions_in_scope` counts
  distinct sessions. BREAKING on a scope holding a duplicated session: the row count and every
  spanning count change, and the header numbers with them.

- **`for` provenance on every emitted unit.** A `for` edge existed only where a record
  carried a key naming a target, so an assistant reply, a thinking block, a tool call and
  its result - most of a transcript - had no provenance at all. `for_root` now names the
  record that opened a unit's turn plus the parent hops up to it, rendering `[root L12 +3]`
  and nothing on the opener itself; its JSON is always `{line, uuid, hops, note}`, because
  "this unit IS the opener", "the chain walked there in N hops", "the grouping placed it and
  the links did not carry it" and "there is no opener above this unit" are four facts a
  missing key could not separate. Two
  engine relations join the vocabulary and ride JSON alone: `parent` (the `parentUuid`
  link, which stands down when ANY OTHER edge on the same unit already names that same
  TARGET - the test is on the target, never on who named it) and `result_of` (a tool
  result's `tool_use_id`).
- **The relation vocabulary is closed** (`EdgeKind`, nineteen words). Every family wrote
  its word as a bare string at the call site, so the set was whatever six files happened to
  contain - unenumerable, and impossible to census. No relation was renamed - every word is
  the one its family already rendered - and the cross-family pin over the whole edge SET
  landed first. Rendered bytes did move elsewhere in this version: an edge naming a CALL
  now resolves through the tool index and prints the call's line where it used to print
  the wire id with `not in this file`.
- **`tool_use_id` on the two user-facing duals.** An answered AskUserQuestion and a refused
  tool call are the person's message and a tool result at once, so their hit now carries the
  API's id for the call it ANSWERS, where it read null. That is what the `result_of` edge
  resolves; `pairing`, `tool_name` and `is_error` stay null on both, being the tool view of
  a record whose richer view is the person's.
- **`--count-by for-kind`**, the census over that vocabulary: which provenance relations a
  scope carries. Multi-key like `label`, distinct per record, counting a relation whether
  or not csift resolved its target.
- **`--for <ADDR>`**, the reverse query: everything that is for one record, transitively.
  The address is `L<line>` or a bare number (per file) or a record uuid, which finds its
  record in whichever scanned transcript carries it; the closure itself is computed per
  transcript. One query returns the reply a message drew, the results of the calls that
  reply made, and the `<task-notification>` that reports on one of those two turns later;
  the parent arm stops at a turn boundary, so the person's next question does not come back.
  Three structural relations admit a row - the parent link, a result answering a member's
  call, a record naming a member's call by id - and the rest of the vocabulary is not
  reverse-walked. The summary gains `for_of` (`address`, `records`, `transcripts`).
- **Every non-transcript line type says what it holds.** The head-and-body law landed with
  twelve of the thirteen `harness.bookkeeping` leaves rendering the line alone, so a reader
  who named one still read raw JSON. Sixteen of the eighteen line types now build a
  `[<line type> key=value ...]` head from their own bytes, in the key order Claude Code's
  own writer uses, and the head opens with the line's own `type` - which is how
  `mode`/`permission-mode`, `ai-title`/`agent-name` and the three artifact ledgers stay
  apart in the output when their label does not tell them apart. The two `file-history`
  forms keep the head built from the fields csift models: that one names the tracked path
  and its version where a raw key walk could only print the snapshot object. Two rules
  decide a head's values and there is no third: an account, organisation or record uuid
  prints its first 8 characters, every other value is cut at 80 with the shared
  `... (+N chars)` marker, an object printing as its sorted key list. The body is still
  the line, so a key the head elides stays matchable; a fabricated head is not, so each
  line type's own quoted value is registered as a synthesized-text marker under the same
  predicate that admits the line - without it, four of five head-only patterns on a
  21-line fixture read 0 instead of 1, and a default scan arms none of them.
- **Five bookkeeping line types now say what they are FOR.** Four ride the shared
  provenance edge: `last-prompt` -> `leaf` (its `leafUuid`), a file-history line ->
  `versions` (its `messageId`), `fork-context-ref` -> `forked_from` (the record the forked
  agent's prefix was cut at, with the parent session beside it, because that record lives
  in another file), and `continued-in` -> `continued_in`, which names a whole TRANSCRIPT
  and no record in it - the edge builder's session-only answer, rendering
  `[for: continued_in @<session>]`, an `@` target you can feed straight back to csift. The
  fifth names an INSTANT, which no target shape can carry without putting a timestamp in a
  field documented as the target record's uuid, so `cost-state` stays the family's own
  fact: `[checkpoint closes <instant>]` with JSON `checkpoint_closes_at` and
  `checkpoint_join`, the `startTime + totalDuration` sum `list` joins a cleared session on
  as `cleared_from`. Each renders its honest empty when the key it reads is absent, rather
  than dropping the field. A `fork-context-ref` naming a parent session but no cut record
  renders that session for the same reason.
- **`[not delivered]` on every bookkeeping unit.** JSON `delivered` has always read false
  for all thirteen leaves while the text surface said nothing, because the engine prints
  that marker only for a record carrying a per-record delivery override and a bookkeeping
  line's invisibility comes from its leaf. It is the engine's own marker, so both surfaces
  spell it one way. `show --format json` gains three record-row keys with it: `for`,
  `checkpoint_closes_at` and `checkpoint_join`. The handover relation is not a fourth key:
  it rides the shared `for` array as the edge `continued_in`, whose target names the child
  SESSION and no record in it, so a `continued-in` line's row carries that edge and no
  `continued_in` key of its own.


- **A thinking block says what it is, and which of the two wrote it.** Four label-zone
  markers: `[no summary text]` on `agent.thinking.omitted` and `[redacted thinking]` on
  `agent.thinking.redacted`, the API's own textless answers; `[cancelled stream salvage]`
  on the one client-minted thinking shape, which keeps `agent.thinking.summary` and stays
  delivered; and `[thinking removed by the client]` on an `agent.message.plain` block whose
  whole trimmed text is `[Thinking removed]`, so those three words never read as the
  assistant speaking.
- **Two per-hit JSON keys on every thinking block unit.** `signature_len` is the character
  length of the block's own `signature` - `0` when the field is present and empty, `null`
  when the block carries no such field, because absent and empty are different shapes and
  the salvage discriminator rests on exactly that one. `thinking_text_absent` rides
  `agent.thinking.omitted` alone. Both are named in `search --help` and `show --help`.


- **Each `harness.system.*` event kind renders the fields that make it useful.** The old
  catch-all head named the subtype and threw the payload away: a refusal rendered its banner
  sentence and dropped both model ids, the category and the rewind target, and an `api_error`
  rendered a head with no status, no retry counter and no word about what failed. Now `refusal`
  reads `[refusal <subtype>] originalModel=... fallbackModel=...|none apiRefusalCategory=...`
  (the `none` is printed rather than elided, because the absence of a second model is what
  separates the two subtypes; with no source line to read the head stops at the subtype rather
  than reporting a key it never looked at), `api-error` reads `[api-error status=... type=<kind>
  retry N/M in Xms]` plus the failure's own wording, each read where its own shape puts
  it (a connection failure keeps the wording in `error.message` and names no kind; an HTTP
  failure names the kind in `error.type` and buries the wording two hops down, inside the
  response body it kept), `notice` reads `[notice <level>]`, `kill`
  reads `[kill]` (the record carries no payload on disk), `command` reads `[command]`, and the
  scheduler's `harness.schedule.event` reads `[scheduled <when>] task=... cron=... prompt=...`.
  Every one of those fields also rides JSON as its own key - `retried`, `original_model`,
  `fallback_model`, `category`, `status`, `retry_attempt`, `max_retries`, `retry_in_ms`,
  `error`, `task_id`, `cron`, `prompt` (cut at 200 characters) - and the head carries the same
  value, because a key that reaches JSON alone is unsearchable.
- **A leaf that NAMES another record says which one.** A refusal's `refusedUserMessageUuid` and
  a delivered command's `parentUuid` ride the hit as a `for` fact: `[for: refused L42]` when
  this transcript holds the named record, `[for: wraps <id8> (not in this file)]` when it does
  not, and `[for: <kind> unknown (<reason>)]` when the record names none - so an edge is never
  silently absent. A fire record's `taskId` and a fallback turn's banner take that last form on
  purpose: the arming call is found by MATCHING an id inside a tool result and the banner
  DESCENDS from its record, and neither is a lookup a uuid map can make. JSON `for` is an array,
  one object per edge, each carrying `kind`, `basis`, `target` and `reason`.
- **A turn the API served from a FALLBACK model names both models.** Its sole content block is
  the API's `fallback` block, written in place of the text, and nothing in the harness renders
  it - so a reader had no way to learn the turn was served by a different model. It now carries
  `[served by <to.model>, fell back from <from.model>]` in the label zone plus JSON `served_by`
  and `fell_back_from`, the one place a transcript keeps the pre-fallback model. The ids are
  read off the parsed block, so a scanned unit and an addressed one carry the same fact; a
  record whose only block is that one still emits nothing on a scan, because there is no text
  to match or excerpt.
- **`search --help` and `show --help` carry the family's surface**: a SYSTEM LEAVES region
  naming every render form, marker word, `for` kind and per-hit key, and fifteen key rows on the
  `show` record-row table.


- **The exposed keys: what a record says about the CALL behind it.** Claude Code stamps a
  record with which agent definition, skill, plugin and MCP tool the request ran under,
  which API request and which content block of its response it is, why the call failed,
  what a person typed refusing a tool, which scheduled task submitted the prompt and which
  hook run closed it. csift parsed none of it, so a reader who wanted any of it fell back
  to `--raw`. Every one of them now rides every `search` hit and every `show` record row as
  JSON - `attribution`, `request_id`, `api_block_index`, `tool_ends_turn`, `user_feedback`,
  `api_failure`, `interrupted_message_id`, `hook_run_id`, `scheduled_task_id`,
  `scheduled_fire_id` - and four of them render a label-zone tag: `[via skill X]`,
  `[ends turn]`, `[feedback: ...]` and the api-failure headline. An address carries exactly
  what a scan does. A missing key means this record does not say, never no - and so does a
  key of an unexpected JSON type, which costs its own key and never the record: every one of
  the eighteen decodes leniently, so a record carrying `"apiBlockIndex":"3"` still emits with
  its text searchable instead of being booked malformed. They ride the record as ONE struct
  behind ONE pointer, read by the same serde pass that builds it, so the row every parsed
  line of every scan carries grows by 8 bytes rather than by the 216 eighteen fields cost:
  `size_of::<Record>()` moves from 1,368 to 1,376 and the allocation is paid only by a record
  that names a key. Measured against the tip before it on 56 acid (row, size) pairs, every
  ratio inside a same-binary control band of 0.2467 and the compacted class reading a mean
  of 1.0153.
- **The session a record was minted in, where that is not the session holding it.**
  A record carries two session ids under two spellings, and they disagree exactly when a
  record was hydrated here from another session's stream. `csift list --lineage` reports
  those ids as `minted_elsewhere` with a count beside them, text row `minted`. It is the
  fourth lineage link a row can carry and the only one a record states about itself, so it
  carries no inference wording.
- **An attachment unit says which record it is for.** Every attachment unit carries two
  `for` edges and a producer fact: `attached_to` from the record's own `parentUuid`,
  resolved to a LINE of this transcript through the chain's uuid map; `for_tool_use` from
  the payload's inner `toolUseID`, which is a wire id no line carries, so the edge keeps
  the id and says where it is not; and `[produced_by <command>]` from a hook run's own
  command, JSON `produced_by:{command,hook_name,payload_type}`. The two edges fold into
  the shared JSON `for` array. An edge whose key the line does not carry renders its
  reason rather than disappearing, and all three ride an addressed record csift models no
  leaf for as well.
- **`search --help` and `show --help` name the attachment family's surface**, so the two
  manuals stay at information parity with SKILL: an `ATTACHMENT LEAVES` region covering
  the two bodies, the markers, the provenance facts and the matching rule, and the record
  row's `body_source`, `human_turn_render_differs`, `produced_by` and `for` keys.
- **Eight new provenance relations and one moved, all of them values csift already
  computed.** The closed `for[]` vocabulary goes from nineteen words to twenty-seven: eight
  are new words, and `reports` was already in the set and changed which records answer it
  (the bullet below). The SURVIVAL axis contributes
  four: `supersedes` names the message that replaced a recalled draft, `continues` the one
  the conversation continued from after a rewind, `branches` the head of an abandoned
  branch, and `replays` the surviving line a replay copy's uuid also names. A COMPACTION
  event contributes three: `pairs` joins its boundary and its summary in both directions,
  `precedes` is the `logicalParentUuid` the boundary carries in place of the null parent
  every boundary has, and `preserves` names the head and the tail of the segment the
  compaction kept, as two edges. A paired resume placeholder gains `repairs`, naming the
  dangling prompt it was spliced in to answer.

  No field moves, but one edge KIND does. `parent` stands down when ANY OTHER edge on the
  same unit already names that same TARGET - the test is on the target, never on who named
  it, so the engine's own `pairs` and `repairs` stand it down exactly as a family's word
  does, both of them naming the record above. One relation, one edge. On the landmark
  transcript that moves 83 units, `parent` 169,180 to 169,097, against 80 compaction
  summaries and 3 paired resume placeholders; the `parentUuid` FIELD is untouched on every
  one of them and `for_root` still names the turn each belongs to. COUNTING RULE: the
  `--count-by for-kind` census over the whole transcript with an empty pattern, one row per
  record per distinct relation word its edges name.

  Each edge is the machine-followable form of an answer the unit already
  rendered for a person, so a consumer can walk `for[]` instead of parsing a marker word.
  The four survival relations render no label-zone tag at all - each restates
  `[abandoned]` / `[rewound]` / `[replay copy of L<n>]` or the draft-distance line - so they
  ride JSON alone under the same gate `parent` and `result_of` already take, and a default
  text scan builds none of them and renders every byte unchanged.

- **A notification pulse names the call it reports on, whichever carrier it rides.**
  `reports` was read in the isMeta injection family, whose gate is the authorship flag, so
  it answered only for the pulse wrapped in a notification FRAME: 95 of the 4,017
  pulse-carrying records over one projects root. It is read at the engine now, off the
  record's own text, so the 3,922 ordinary carriers answer too - and it resolves through the
  tool index rather than printing a bare handle, because a `tool_use` id is a wire id and is
  in no `uuid` field (2,470 pulses carry the tag; 2,459 of those name a call of the same
  file, so 61% of pulses get an address). A pulse that names no tag says so rather than
  dropping the edge; joining those through their `<task-id>` would reach 1,228 more, about
  92%, and is a declared follow-on. The reader takes the same text the CLASSIFIER takes, so
  a pulse whose content is a block array answers exactly as a string one does.

  COUNTING RULE, both halves - the FILE SET and the READING:
  one row per RECORD whose `message.content`, read as the bare string or as the
  newline-join of the content array's `text` blocks, carries `<task-notification>`; over
  every `*.jsonl` under one projects root - the top-level transcripts AND the
  `<uuid>/subagents/**` sidecar ones, excluding `journal.jsonl`, `elicitations.jsonl` and
  anything under a `csift-channel/` directory. Both halves move the answer: the top-level
  transcripts alone miss the sidecars, and a string-only reading gives 3,805 rows instead
  of 4,017 and moves the FRAME count from 95 to 56. The corpus is live, so a re-run reads
  a few more rows; the rule is what reproduces.

- **The relation INVENTORY, in SPEC 4.12 and SKILL.** One table over every relation csift
  can answer about a record: the twenty-seven `for[]` words, the relations that ride a FIELD
  on another surface instead, and the ones nothing on disk names. Two rules decide which is
  which, and both are now stated: a CROSS-FILE join is a field on the surface that owns it
  (`clone_of` / `cleared_from` / `continued_in` / `minted_elsewhere` on `list`,
  `spawn_tool_use_id` / `parent_agent_id` on `agents`, the verdict on `msg`), because an
  edge's target names a session without carrying the re-feedable id trio; and a target that
  is not a record - a command, an instant, a lane, an external file - is a per-family fact
  with its own key. A drift test holds the table's `for[]` block to `EdgeKind` in both
  directions.

- **The harness's nine TURN CONTINUATIONS all reach `harness.enforce.retry`.** A turn the
  model does not finish is continued by the harness writing a whole message into it as an
  `isMeta` user record, and the producer interns nine of them in one constant block
  collected into one array whose single consumer is its own is-this-a-record-I-wrote
  predicate, tested by membership. csift knew four - the redo nudges - and carried no leaf
  at all for the other five: nothing on any scan, no census key, and the residue unit under
  an address. The five are the two CLASSIFIER-STOP forms, written when a safety classifier
  stops a response mid-turn and the turn continues once on the same model (the second is
  the first plus a sentence about a call that was already running); the OUTPUT-TOKEN-LIMIT
  resume; and the two MID-STREAM CUT-OFF forms, chosen by lane. All nine are matched by
  WHOLE-MESSAGE equality rather than by prefix, because they are complete bodies and the
  producer tests them that way - a longer body opening with one is a record quoting it -
  and five carry an em dash, so a text retyped with an ASCII hyphen matches no record. The
  leaf's description widens with its domain, to the harness continuing a turn the model did
  not finish on its own instruction, and the shape inventory gains a row per event. The
  classifier stop writes THREE records and only the continuation needed a rule: the notice
  beside it is decided by its subtype and the refused assistant turn by its own blocks.
- **The API refusal is a fact on the assistant unit it stopped.** `message.stop_details` is
  the API's own structured account of why it stopped a response - a type, the classifier
  band, the API's own sentence about it, and whether a fallback claimed a prefill. The
  client builds none of it: it copies the object off the response frame onto every
  assistant record of the turn, removes exactly one member it does not persist, and
  constructs one of its own only on refusal paths that null every remaining member, so a
  true prefill claim on disk came from the API. csift read none of it, and a refusal whose
  only content block is a thinking block classified `agent.thinking.summary` with nothing
  on any surface saying the API had refused. The exposed-keys family stamps it on every
  emitted unit of a record that names at least one member: a label-zone tag
  `[api <type>: <category>]`, and `stop_details` beside the coarse `stop_reason` on the
  wire. The gate is the OBJECT and not the leaf, because a key is true of a record whatever
  csift labelled it; an object whose every member is null is refused, because the absent key
  already says what four nulls would. The tag is label-zone text and so display-only, like
  the thinking family's: a pattern scan matches a unit's rendered section and never its
  zone. Two members stay out of the model and reachable with `--raw` alone - one never
  reaches disk, the other reaches it only as a null. `size_of::<Record>()` moves from 1,376
  to 1,384, the object sitting behind its own pointer inside `message`.
- **Two attachment payload types get their inventory rows.** Both carried no leaf, so
  neither reached a scan even under `--attachments`, neither could be a key on
  `--count-by attachment` (that axis reads emitted units), and each rendered as the
  unmodeled unit under an address. `mcp_instructions_delta` is what the harness writes when
  the set of MCP servers whose instructions belong in the request changes; its builder
  returns nothing when the diff is empty, so a record exists only where the set moved. The
  converter turns it into CONTENT - a `# MCP Server Instructions` section carrying the
  blocks added, and a second section naming servers that disconnected - and persists that
  conversion back as a top-level `rendered`, so it is `harness.context.listing` with the
  model column true and the body rule takes the persisted conversion, which is what makes
  injected server instructions findable by a pattern scan. `thinking_drop` is the harness's
  accounting of the thinking blocks it dropped from a request - the model, the query
  source, the counts, and why - and its converter arm is an explicit empty list, so it
  joins `NEVER_CONVERTS`, reads `not delivered` whatever its leaf default says, takes
  `harness.context.task` beside `thinking_stripped`, and falls to the verbatim payload,
  which is the only place its numbers are. The HUMAN column on all three rows is read off
  the transcript render's own hide set rather than inferred from the leaf, and that list
  is per build: the verified build's hide set names `mcp_instructions_delta` and neither
  of the two thinking payloads - it cannot, since neither name occurs anywhere in that
  build - while the 2.1.278 hide set names all three. So the column on the two thinking
  rows is a reading of the build that knows the types, which is also the correction to
  `thinking_stripped`, whose column said unstated while a hide set answered for it.
- **A bundled skill body that ships no files reaches `harness.context.skill`.** The
  `Base directory for this skill:` line the leaf keys on is written by ONE wrapper in the
  registrar, and only for a registration that names FILES: it materialises the directory
  and prepends that line and a blank one to the first text block of the body the
  registration returns. A registration with no files keeps its body getter untouched, so
  the same kind of body reaches the same kind of record bare - and carried no leaf,
  rendering `? unmodeled: user/text` under an address because its content is a text block
  rather than a string. The leaf answers to a second content-start head, the
  workflow-authoring reference's own, which is what the leaf already means; the prefix is
  an artefact of shipping files rather than a property of the body. csift names the one
  such body whose producer has been read and says so where the leaf is documented: the bare
  bodies share no head, and the records carry no field saying which command expanded them -
  the parent link names the command's own wrapper on three of the four measured records and
  an attachment on the fourth.
- **`whoami --to` takes `--from`, the sender its prediction is made for.** The prediction
  is decided partly from the relation between sender and receiver, and the stderr note
  already told a lane caller to `pass --from @<your lane id> for an exact sender` while
  `whoami` rejected the flag. `--from` takes `csift send`'s grammar and only goes with
  `--to`, so a subagent that names itself gets the answer a send from that lane gets.

### Changed

- **The root `--help` names every `floor_cause` value.** `floor_cause` was named as a key on
  seven help pages and its value domain on none, which left `--help` thinner than `SKILL.md`
  on a closed enum a consumer branches on. The global `--cross-compactions` doc now names all
  eight, sorted by what a reader can do about each: only `budget-exhausted` is one a larger
  budget can move, and only when that boundary's door is open. Help text is surface.
- **The root `--help`'s newline pitfall said the opposite of what the binary does.** It read
  "Text-mode excerpts keep a record's LITERAL newlines (a multiline Bash command renders
  as-is)", and the excerpt builder collapses every whitespace run to a single space, so a
  text excerpt is ONE LINE per record. The pitfall it warned about was real and its mechanism
  was inverted: a `head -N` cuts whole records off the end and takes the footer with them, it
  does not cut into a record. The help now states the collapse, names the JSON `body` and
  `--raw` as the two ways back to the newlines, and keeps the pipe advice on its true
  footing. The same measurement corrects a second sentence, in `SKILL.md` alone: `body` keeps
  the newlines of a render csift did not already flatten, and an opener it RECONSTRUCTS
  (`user.message`, `user.answer`, `user.rejection`) is flattened a layer earlier, in the
  model, so its `body` is flat too and `--raw` is the only way to its bytes. Help text is
  surface.
- **`whoami --help` named one summary terminator where the stream has three.** An ordinary
  run closes `{identities, live_child_lanes, other_live_lanes}`, `--to` closes
  `{queued, refused}`, and `--peers` closes `{peers}` - three shapes sharing no key, so a
  consumer reading the terminator has to know which mode produced it. The help described it
  as `{kind:"summary", identities}`, one key of the five; `SKILL.md` named two of the three.
  Both now name all three. The same edit adds the clause the newline correction left out:
  the JSON `body` is null unless the excerpt cap was lifted (`--no-truncate`, or a
  `show --line`/`--uuid` address). Help text is surface.
- **`SKILL.md` is rewritten as the always-current mastery manual.** It had become a ledger:
  37 lines carried a csift version rider, so a reader had to work out which sentences
  described the binary in front of them. It carries none now, and the Surface line against
  `csift --version` is its only staleness detector. The order is orientation, then the
  cross-cutting laws every command obeys (targeting, the five laws, labels, provenance,
  survival), then the commands, then the machine surface, then the failure modes. Facts are
  stated once and cross-referenced by section, which retires six duplicate clusters running
  to four, five and six copies apiece; every retired spelling now lands in one section, so a
  caller who errored on one has a single address. Two statements that had gone stale are
  corrected against the binary: a text excerpt is one line per record, and sixteen of the
  eighteen bookkeeping line types build their key=value head from their own bytes. It ships
  at 59,591 tokens, 2,863 over the 56,728 it replaced (tiktoken `cl100k_base` throughout).
  The rewrite itself landed at 56,687, so its dedup and rider removal paid for every section
  it added; two correction rounds bought the rest - the retirement table grown to every
  spelling the binary refuses, the crossing default's value, the label-zone tags the file
  named nowhere on `search`/`show` and on thirteen other commands, the three empty-result
  stderr lines it had never quoted, and the third summary `whoami` can close with. The two
  73-row transparency tables and the relation inventory keep their exact rows, so every drift
  test that pins them still holds in both directions. One header line moved: the shape
  table's third column was the same escape-hatch command on all 73 rows, so it is stated once
  above the table and the drift test's literal moves with it.
- **The survival walk now CROSSES a compaction boundary, under a budget, and every surface
  says what it crossed.** Claude Code's loader reads `parentUuid` only, so it stops at a
  `compact_boundary` and the whole history above the cut leaves the conversation; csift
  stopped there too and flagged that region `pre-cut`. But the boundary names the record the
  compaction re-anchored onto, in `logicalParentUuid`, and the conversation genuinely
  continues across a compaction - the records above a cut are the same conversation, and
  until the walk gets up there a turn the operator rewound past cannot be told apart from
  history csift merely failed to resolve. The
  walk now follows that link, up to N boundaries, set by the new GLOBAL flag
  `--cross-compactions <N|all>` and honoured by every chain-walking command: `search`,
  `show`, `stats`, `files`, `recover`, `verbatim`, `image`. Above a boundary it CROSSED the
  ordinary rules apply, so a turn the operator rewound past above a cut is finally named for
  what it is rather than filed as unresolved history; above the FLOOR everything off-chain
  stays `pre-cut`. The DEFAULT is `--cross-compactions 3`, the measured value: on an idle
  host budget 3 ran inside the same-binary noise band against budget 0 on every one of 95
  chain-walking rows over a compacted transcript and the 397 MiB landmark (SPEC 7j carries
  the rule and the numbers). This is a surface change on a compacted
  transcript, and it moves records in BOTH directions: one whose survival was `pre-cut`
  becomes `live` where the chain reaches it and `abandoned` where it does not, and a turn
  that becomes abandoned leaves the numbering. Measured over eight transcripts (the seven
  top-level ones of one projects root carrying a boundary between 9 and 21 MiB, plus the
  397 MiB landmark), three of them move at the default: 490 emitted hits go `pre-cut` to
  `live` on the landmark and 1,150 and 952 on the other two, while their whole-file
  `abandoned_records` rise by 258, 1,231 and 1,135. `--cross-compactions 0` restores the
  previous reading exactly - 32 of 32 streams byte-identical to a build that predates the
  flag - and under it csift says nothing about crossing at all, neither a footer line nor a
  JSON key. That equality is a measurement with one known exception rather than a law: a
  boundary whose own `parentUuid` is non-null and names no record was crossed by the guard
  the pre-flag walk kept for exactly that shape, and budget 0 does not cross it. No
  transcript of the measured corpus carries one.
  The footer is never silent otherwise: it names how many boundaries were stepped over, how
  many were not, where the chain floor sits, WHY it sits there - a budget that ran out, or a
  boundary whose door is shut, named by which of three facts shut it: the record is on no
  line of the file, it is on a line the pre-boundary cut took out of the map, or it is on a
  line the walk had already reached - and a flag value, but only after
  probing whether one more unit of budget would actually move that floor: a budget can run
  out AT a boundary whose own logical parent is gone, and there every larger budget lands on
  the same record. JSON carries the same six facts as
  `crossing_budget`, `boundaries_crossed`, `budget_exhausted`, `boundaries_above_floor`,
  `floor_line` and `floor_cause`.

- **An unmodeled unit's excerpt elides an opaque `data` blob.** The key=value excerpt cuts
  each value at 80 characters, so a base64 image source or a `redacted_thinking`
  ciphertext swallowed the whole budget and the sibling keys that say what the shape is -
  `media_type`, the block's own `type` - never printed. `data` is now elided by name at
  every depth, as `headers` already was, and those keys print in its place.
- **A pattern carrying a backslash matches the text csift renders.** The needle safety
  predicate counted the double quote, the control characters and DEL but not the
  backslash, which a raw jsonl line stores doubled - so `search 'C:\\Users\\dev'` reported a
  definitive absence over text `show --line` put on screen, on an attachment's converted
  body and on a plain user message alike. The predicate counts it now, which splits such a
  run into needles that do occur in the raw bytes.
- **Three JSON keys the help never named.** `show --help`'s record-row table did not carry
  `superseded_draft`, and `search --help`'s hit schema carried neither `denial_kind` nor
  `image_ids` - keys a consumer meets on the wire and cannot find on the page. All three
  are named now, and a contract test asserts the direction: every key the two commands
  emit is one their own `--help` names.
- **The `for` array is named once on each help surface.** Four families each listed the
  shared provenance key in `show --help`'s record-row table under their own block, spelled
  four different ways, so a consumer reading that enumeration read four keys; it is one
  row under the shared block now. `search --help`'s JSON schema listed every hit key
  except `for`, so the array shipped on the wire and appeared in no list; it is there now.
- **`scheduled_at` answers on the scheduler's own fire record too.** The field carried the
  instant for the fired prompt and read null on the `harness.schedule.event` record - the
  line the instant is written on. It now reads that line's own parenthesised tail, so the
  key means one thing on both halves of the pair. The head still prints it.
- **`show --format json` carries a record's queue facts.** `queue_operation` and
  `queue_reason` reached a `search` hit and no `show` record row, so running a hit's own
  `refetch` command answered null for a record the query it came from had just reported in
  full. Both are on the row now and in `show --help`'s key table. The pair also has one
  writer: the scan read the record and the address site hard-coded both to null.
- **A leafless unit renders its facts on the text surface too.** An addressed record
  csift models no leaf for carried its family facts into JSON and dropped every one of
  them from the label zone, so `show --line N` and `show --line N --format json`
  disagreed about the same record. The label zone now renders the shape in place of the
  leaf path and the facts after it, exactly as a labelled unit does.
- **A `for` edge cuts a uuid to its handle and prints every other id whole.** The label
  zone cut an unresolved target to eight characters whatever it was. That is right for a
  uuid - the first eight hex digits identify one and the compaction excerpt has printed
  that handle for versions - and wrong for a wire id, whose head is the part they all
  share: `hook-bb000000` became `hook-bb0` and `toolu_01zzreef` became `toolu_01`, keeping
  the constant prefix and throwing away the whole distinguishing tail. The cut now applies
  only to a uuid-shaped target. JSON is unchanged: `target.uuid` always carried the id
  whole.
- **An attachment unit renders the text the model received.** BREAKING: the body of a
  `harness.hook.context` unit, and of every attachment unit whose record carries a
  persisted conversion, is now that converted text rather than the joined content or the
  payload JSON. The unit says which of the record's two copies it is showing -
  `[rendered]` or `[payload]`, JSON `body_source` - and names itself beside it:
  `[hook <event> <name>]`, `[hook <event> <name> exit <code> <ms>ms]`,
  `[context <payload type>]`, `[structured]`, `[mid-turn]`. A record carrying a
  conversion is searchable by the converted text and no longer by the payload text it
  replaced; `--raw` does not recover the difference (the backfill runs after the match
  phase), so the way to the bytes is `csift show @<id> --line N --raw`. A
  `queued_command` carrying a SECOND conversion for a human turn keeps the ordinary one
  as its body and reports the difference (`[human-turn render differs]`, JSON
  `human_turn_render_differs`), because which copy the model received depends on the turn
  it was replayed into and the record does not say. The body DECODES a JSON string where
  csift emitted the payload bytes unparsed, one parse per record: on two synthetic
  200,000-line transcripts, seven interleaved rounds with a same-binary control in each,
  a file whose every line carries a conversion costs 1.131x (control 1.033x) and one
  where none does costs 1.068x (control 0.942x), while an ordinary scan of a transcript
  with no attachment line reads 0.959x against a 0.967x control.

- **One stamping site for every taxonomy family, and one place each edits.** The extension
  points on a search hit existed with nothing filling them, so each family would have cut
  its own dispatch through the same shared files. `family_facts` is that one seam, called
  at both hit-construction sites, so a record fetched by `show --line` carries every fact
  the same record carries under `search`. `search --help` becomes one section file per
  family and `show --help`'s record-row key list is rendered from a table with a family
  column. Splitting them moved no rendered byte; what the families then add is visible on
  the page, and `search --help` carries the first of it - a `BOOKKEEPING LEAVES` region
  stating the render law below. `show --help` renders exactly what it did, no family
  having added a key yet.
- **A bookkeeping unit renders its key=value head over its own bytes.** A summary is lossy,
  and `search` matches rendered text, so a key only the head elided had stopped being
  findable at all. The head stays, the line's verbatim JSON follows it, and every key the
  head does not name is matchable again. The `harness.system.*` leaves are deliberately
  outside the rule: an `api_error` line carries the whole HTTP response header set, which
  csift prints on no path but `--raw`.
- **A uuid a record NAMES resolves to that record's line.** `ClassifyCtx::line_of_uuid` is
  backed by the chain's own uuid map rather than a second index, so an edge can render an
  address; a caller with no row space gets an honest absence instead of a guessed line.
- **A taxonomy family decides what its own lines SAY.** The facts seam had no render twin,
  so a body was still decided in the shared chain. `family_render` dispatches by the leaf's
  category to the family that owns it, ahead of the engine's generic arms, and four renders
  moved into their families: the two attachment bodies, the bookkeeping head-and-body
  composition, the six `type:"system"` event heads, and a thinking block's body. Not one
  rendered byte moved - 122 of 122 corpus-derived commands compare byte-identical against a
  build of the base commit, with the golden baseline and the whole suite beside them.
- **One `for` edge, built one way.** Four families point at other records, and three had
  each invented a different fact for it. There is now one: `for: <kind> L<n>` when this
  transcript carries the uuid, the id with `(not in this file)` when it does not, and
  `unknown (<reason>)` when the record names nothing - never a silently absent edge. Several
  edges on one hit accumulate under the one `for` key instead of overwriting one another.
- **A `headers` member reaches no rendered surface.** An `api_error` record carries the
  failed request's whole HTTP response header set - a session cookie and an organization id
  among 18 names - and the key=value excerpt csift falls back to for a line it models
  nothing about printed it. Every `headers` member is now elided at every depth, `<elided>`
  in its place so a reader learns the set is there, and `--raw` stays the one way to the
  bytes.
- **Two more payload types are marked as never reaching the model, and a result set moves
  with them.** The harness's conversion table drops `goal_status` and
  `async_hook_response_batch` outright; csift's `NEVER_CONVERTS` list had neither, so it
  reported both as delivered. They are in it now, and the consequence is visible on three
  surfaces. Measured on a `goal_status` attachment: `search -t harness.context` used to
  return it and now returns nothing, with the empty-result probe naming
  `harness.context.task` as where it went (the bare category face shows only what the model
  received; the leaf's own `-t` still returns it); the hit's JSON `delivered` reads `false`
  where it read `true`; and the rendered label zone gains `[not delivered]`. A query written
  against the bare face has to name the leaf instead.
- **A coordinator relay says where it came from.** Every other inbound message renders a
  `from => to` arrow; this one rendered none. It now reads `coordinator => self`, and the
  from is the literal word because the record carries no sender field: the harness builds
  that origin object in two places and both build the one-key form.

- **The `-t` label taxonomy is 17 categories over 73 leaves, split by producer.** The old
  38-leaf space had one bag in it: `harness.meta` pooled hook-injected text, a hook run's own
  record, a slash command's caveat, the loop tick, the end-of-turn telemetry and the
  file-history instrument, and its `.attachment` and `.system` leaves were catch-alls for
  everything else. A selector could not name what it wanted and a census could not say what a
  scope held. The new space asks one question of every record, what wrote it, and the answer
  is its category: `user`, `agent.{message,thinking,tool,communication}`, and under `harness`
  the twelve producers `notification`, `compaction`, `command`, `interrupt`, `schedule`,
  `resume`, `hook`, `context`, `enforce`, `turn`, `system` and `bookkeeping`. Renames land
  one for one (`agent.message` becomes `agent.message.plain`, `agent.thinking` becomes
  `agent.thinking.summary`, `harness.meta.loop` becomes `harness.schedule.tick`, the three
  telemetry leaves become `harness.turn.*`, `harness.meta.snapshot` becomes
  `harness.bookkeeping.file-history`); `harness.meta.hook` splits three ways by producer into
  `harness.hook.context`, `harness.hook.feedback` and `harness.command.caveat`; and the two
  catch-alls become per-value tables. `agent.message` and `agent.thinking` still parse - they
  were leaf paths and are now categories, so they select their whole face.
- **A bare CATEGORY narrows exactly as a bare ROLE does.** Both are the coarse "show me the
  conversation" ask, so both select only the leaves whose content the model receives, while a
  glob (`-t 'user.*'`) and an exact leaf path still reach every record. `harness.bookkeeping`
  is the one category whose whole face is invisible: the bare form is accepted, matches
  nothing by that law, and `search` prints one stderr note naming the remedy beside the
  ordinary empty-result diagnosis (`empty_visible_face` in the JSON summary). A `-T` that
  cancels your `-t` is still a hard error; an honest empty is not.
- **`classify` became a dispatch over five mechanical family tables** - a top-level line
  `type`, a `type:"system"` subtype, an attachment payload `type`, a content block's shape and
  an isMeta user template - each a value in and a leaf out, with no look at the payload (the
  v0.10.0 `classify/promoted.rs` is now `classify/bookkeeping.rs`, one of the five). Three
  behaviour changes ride it, each a leaf reaching records that carried none: every
  non-transcript line type now carries a `harness.bookkeeping.*` leaf, an attachment carries
  the leaf of its own payload type rather than one catch-all, and a thinking block with no
  summary text (`agent.thinking.omitted`) or a `redacted_thinking` block
  (`agent.thinking.redacted`) takes its own leaf instead of both reading as plain reasoning.
- **Every leaf the shape inventory records is one a record can carry.** The first pass at
  the taxonomy advertised `harness.context.image`, `harness.context.skill` and the four
  `harness.enforce.*` leaves in its tables while nothing in the classifier could produce
  them, and its drift test walked three of the five axes, so the gap did not show. The
  isMeta-template table carries an arm per row now, each keyed on the literal its producer
  writes, plus two structural ones: a relayed message whose `origin.kind` says `human` is
  the person's own, and the coordinator relay takes the same leaf as a peer's. The
  `[SYSTEM NOTIFICATION - NOT USER INPUT]` envelope is treated as a carrier rather than a
  shape, so the background-task pulse inside it classifies by its own kind instead of
  vanishing. A `<system-reminder>` matching no more specific template lands under the
  reminder leaf. An image-only record carries the leaf its text form carries, and the
  `dynamic_skill` payload joins the listing family. The drift test covers all five axes
  with a hand-written specimen per row.
- **`show --line N` never hard-misses a parseable line.** A shape csift models no leaf for
  renders one unit reading `? unmodeled: <type[/subtype|/attachment.type]>` with a key=value
  excerpt of the line's own top-level keys, and JSON carries `unmodeled` beside a null `label`
  and an empty `labels`. Before this, every bookkeeping line type read as absent. A SCAN never
  emits such a unit, so no census, count or `-t` result moves, and a line that is not a JSON
  object at all is still a hard miss pointing at `--raw`.
- **A message record with no text renders under an address instead of missing.** The 1:1
  law reached the non-record line types but not the message lines: a `fallback` block, a
  `document` block or an image with no caption produced no unit and the address bailed on
  a line that is plainly there. Such a record renders now, and the unit names the axis its
  leaf would have come from (`user/document`, `user/isMeta`). There is ONE unmodeled form:
  `? (no label)` is gone, because it told a reader nothing a shape name does not. A record
  that HAS a leaf and merely lacks text keeps its leaf, and an assistant turn the API
  served from a fallback model takes `agent.message.plain` with an empty body.
- **`-t user` and `-t agent` no longer parse every attachment line.** Two leaves an
  attachment carries as a second view rather than as its kind - the human's mid-turn steer
  and the model's structured answer - sat in the set that turns on the keep admitting every
  attachment record, so the most ordinary selectors in the tool paid for lines they did not
  want: 236 ms against a flagless scan's 70 ms on an 89 MB attachment-heavy transcript.
  Each now has its own payload needle, and the three selectors run at flagless speed.
- **The person's mid-turn steer reaches `-t user` again.** When someone types while a turn
  is running, the harness folds the text in as a `queued_command` attachment and stamps its
  provenance inside the payload, beside `commandMode`. csift read a record-level `origin`
  that no line of that shape carries - 0 of 3,692 and 0 of 3,574 over two corpora, against
  219 and 217 carrying `attachment.origin` - so the arm never fired and every steer
  classified as harness context. It reads the payload now. The second half was quieter: the
  leaf renders through the opener path, which an attachment has no `message{}` for, so even
  when the arm did fire the record emitted nothing at all - no hit, no census key, no
  `-t user` result, reachable only by an address. It falls through to the render seam now,
  so it takes the attachment family's ordinary body rule like every other view of that
  line: the conversion the harness persisted when the record carries one, marked
  `[rendered]`, and the verbatim payload JSON otherwise. Measured over one projects root,
  one parse per non-blank line: 20 of the 164 human-origin `queued_command` lines carry a
  conversion.
- **A flagless scan reaches the person's mid-turn steer too.** The needle that admits a
  `queued_command` line was gated on an explicit selector, so `-t user` found the steer
  and a bare `csift search` did not - and a flagless scan is a superset of `-t user`, so
  that was the selector law broken rather than a cost saved. It takes the same gate the
  csift-channel delivery keep has had since 0.11.0: on unless your `-t` excludes
  `user.message`, which keeps `-t harness` paying nothing. The parse still decides - only
  a payload whose own `origin.kind` reads `human` becomes `user.message`, and every other
  `queued_command` stays `harness.context.task` behind its own gate. The model's own
  `structured_output` answer takes the same gate for the same reason: `-t agent` reaches
  `agent.message.structured`, so a flagless scan cannot miss it. Those two leaves are the
  only attachment-carried exceptions - both are words someone owned, a person's or the
  model's - and the coarse keep is untouched, so a flagless scan still parses no other
  attachment payload. The coarse keep being untouched cuts both ways, and the cost is
  worth knowing: a selector reaching an attachment leaf that has NO needle of its own
  still parses every attachment line, and a bare `-t harness` reaches those leaves, so
  it pays that. Measured on an 85 MB transcript of 150,000 `file` attachment lines,
  21 interleaved reps with a pattern that matches: `-t harness` reads 3.4x a flagless
  scan (251.0 ms against 73.3 ms) and `-t harness.context.file` reads 359.5 ms. Forty
  payload types cannot each carry a needle; the two that do are the two an ordinary
  conversation query names.
- **A relayed message reaches an ordinary scan, and every other leaf is now swept for the
  same defect.** A coordinator's relay carries `agent.communication.inbox`, and the record
  it rides is one the harness wrote into the conversation: isMeta, so it opens no turn.
  The arm rendered through the opener path, which answers nothing for a record that opens
  none, and the relay is no attachment either - so the record carried a delivered message
  and emitted nothing at all: no hit, no census key, nothing under
  `-t agent.communication.inbox`. 141 such records across 89 files of one projects root
  (57 across 49 of a second) were reachable only by `csift show --line`. The arm falls
  back to the record's own text, which is what the peer relay beside it has always done.
  That makes twice in this release that a leaf was reachable only by an address, so the
  pin is a SWEEP rather than a third one-off: for every specimen in the shape inventory, a
  flagless scan must emit exactly one hit for each record carrying a leaf the model
  receives. Two specimens have no text to emit and are named in the test with their
  reasons - an image-only record is a picture, and an assistant turn the API served from a
  fallback model wrote no text - keyed on the specimen and never on the leaf, since
  `agent.message.plain` is the ordinary assistant message and exempting the leaf would
  exempt the whole population the sweep exists to guard.
- **`csift msg` prints a command that still runs.** Its lane listing recited the fact half
  of its own join as `csift search '<id>' @<lane> --additional-context`, a flag this release
  retires, so the one recipe a reader needs in order to check the channel without trusting
  `msg` exited 1. It prints `-t harness.hook.context`, which is what the help has said all
  along.
- **`wait`'s activity census counts all four thinking leaves.** It named two of them one by
  one, so the leaf split silently dropped the other two from the count; it keys on the
  category now.
- **`harness.schedule.fire` changes its gate, not its meaning.** It keyed on
  `promptSource:"system"`, a stamp the submit path puts on every isMeta submission; it now
  keys on the parent link to the `system`/`scheduled_task_fire` record the scheduler wrote
  just before it, which is the only mark the producer leaves on the prompt itself. That fire
  record gets a leaf of its own, `harness.schedule.event`.
- **A shape inventory backs the taxonomy** (`model::inventory`): one row per on-disk shape on
  those five axes - 22 top-level types, 11 system subtypes, 50 attachment payload types, 8
  block types, 17 isMeta template families - each naming its leaf, its gate, whether the
  harness converts it for the model and whether a person sees it. It is a record of the
  mapping rather than a second one: a unit test walks every row back through the family table
  that decides the leaf, and another asserts the shaped leaves and a hand-kept table of the
  rest partition the leaf set exactly. The candidate gates and the generated SPEC and SKILL
  tables all derive from it, so the honest-empties note can never name a stale leaf.
- **The source tree is re-managed: command modules under `src/commands/`, the bash
  lexical layers under `src/bash/`.** `src/` sat at both structure caps (20 counted
  direct `.rs` files, 16 subfolders), which is a refactor gate rather than a number to
  raise. `bash_danger*.rs` and `bash_mutations/` become `src/bash/danger.rs` +
  `src/bash/danger/` and `src/bash/mutations.rs` + `src/bash/mutations/`; the thirteen
  subcommand families move under `src/commands/`, leaving the crate root to the base
  layer they build on (`cli`, `model`, `parse`, `path`, `subagent`, `bash`, `chardiff`,
  `text`, `time_window`, `timez`); and the four `model/classify_*.rs` siblings become
  `model/classify/{support,promoted,channel,resume}.rs` under the engine that uses them,
  with `Record::direction` moved verbatim into a fifth child so the engine stays under
  the 600-line cap, and the engine's cases nested to match. Every path is respelled at
  every call site, with no alias and no forwarding shim. No behaviour change.
- **The isMeta INJECTION family says what each record was written FOR.** The harness writes
  text into a turn shaped exactly like the operator's own message, and until now the label
  said who wrote it and nothing said what it answers. Most units of the family now carry
  one `for` edge (JSON `for`): `redoes` the assistant turn an enforce nudge sends the model
  back to, `blocked` the turn a hook's feedback stopped, `fires` the scheduler event a fired
  prompt came from, `reports` the tool_use a notification frame's pulse names, and
  `attached_to` the record everything else was injected under. The line is resolved through
  the chain's own uuid map, so an edge is an address rather than a uuid to go searching for;
  a key that resolves to no line keeps the id and says so, and a record naming no such key
  renders its REASON rather than dropping the edge. The two inbound relays are the case that
  needs the reason: a coordinator relay and a peer relay carry one leaf, so the arm reads
  `origin.kind` and each answers for its own shape - the coordinator's origin carries only
  its kind, the peer's names the lane that sent it (`[peer lane <id>]`, JSON
  `peer_sender_task_id`).
- **A person's message the harness relayed is marked.** A message delivered into a running
  lane is the person's own prose and takes `user.message` like any other, so on every
  surface it was byte-identical to one they typed - while, being isMeta, it opens no turn.
  It renders `[relayed]` and carries the JSON twin `relayed`, because a marker is
  display-only and a machine consumer could otherwise not tell the two apart at all.
- **`harness.hook.feedback` reaches every blocking-hook name.** One formatter writes the
  template over a closed four-name table and csift matched only the name the corpus happens
  to hold, so the first time a TeammateIdle, TaskCreated or TaskCompleted hook blocks, its
  feedback would have landed in the isMeta residue - no leaf, no census key, reachable only
  by an address.
- **A bare `-t agent` gives you the model's structured answer too, and the help pages name
  every key they emit.** `agent.message.structured` rides a `structured_output`
  attachment, which the harness converts to nothing, so the record-level delivery verdict
  read NOT delivered and the bare-face law took the leaf out of `-t agent` and
  `-t agent.message` - a reader asking for the agent's messages got the plain half of a
  two-leaf category and nothing saying the other half existed. It is now the ONE stated
  exception to that law: the axis asks what the model RECEIVED and this is the leaf the
  model WROTE, and the API gives a turn free text or a schema-bound answer and never both,
  so the coarse ask answers with whichever the turn used. The FACT does not move - the
  record was not delivered, so its `delivered` still reads false and the unit still renders
  `[not delivered]`; only which face it belongs to. A glob and an exact leaf reached it all
  along. Alongside it, both manuals now write every JSON key they name in backticks or in
  their own key block, and the `search` hit schema gained `tool_use_id` and `source`, which
  a hit carries and the schema had never listed.

- **The provenance relation `hook run` is respelled `hook_run`.** `--count-by for-kind`
  prints `<count>  <key>` per line like every other census axis, so a key with a space in
  it cannot be read back by the whitespace split every reader of that surface writes, and
  this was the only key in the closed vocabulary that carried one. The word says what it
  said - the id a `stop_hook_summary` names is a hook RUN id, not the API tool_use id its
  field name suggests - and the three surfaces move together: the census key, the
  `[for: hook_run ...]` label-zone tag, and the JSON `for[].kind`. Neither spelling has
  shipped in a released binary.

- **A retired `-t` spelling says WHY it is not a selector, beside where its records went.**
  All thirteen are hard errors, and each message carries
  `('<spelling>' <reason>, so this is: <successors>.)` ahead of the valid-selector list. The
  middle column is a REASON rather than a release, because the only reader of that message
  holds a binary on which the spelling is already gone: which release retired it changes
  nothing, while the reason carries to the next spelling they guess. Three reasons cover the
  thirteen rows. `thinking`, `tool` and `tool-response` are flat spellings and the taxonomy
  is `role.category.leaf`. Seven share one clause - a category is decided by the producer
  that wrote the record - and each says what it did against that rule: `harness.meta` pooled
  several producers under one category; the five
  `harness.meta.{loop,turn-duration,away-summary,stop-hooks,snapshot}` paths each named a
  record by the category it was pooled in; `harness.schedule.continuation` put the resume
  loader's own prompt under the scheduler's. The last three,
  `harness.meta.{hook,attachment,system}`, pooled three or four producers under one leaf,
  and a leaf names one shape. A retired prefix plus `.*` is the same retirement and is
  looked up bare, so it prints the bare row's message. The set is a walkable table rather
  than a match arm, which is what lets a test hold `SKILL.md` section 10 to these exact
  successor strings row by row; SPEC's own copy of the table was two successors short on
  `harness.meta` and one short on `harness.meta.hook`, and now prints what the errors print.

- **Every string a help page quotes is one the binary prints, and no page carries csift's
  own chronology.** Measured over all eighteen pages (`csift --help` plus one per
  subcommand) against the last released binary, both invoked under the basename the usage
  line echoes: 602 rendered non-blank lines differ counted once per PAGE - 215 only in the
  older arm, 387 only in this one - or 479 counted once per distinct line TEXT, the two
  figures differing by the lines that render on more than one page and by the two lines
  of the `--claude-home` entry that `search --help` moved within its page, which a diff
  reports as removed and added. The new root
  `--cross-compactions` block is seven of those lines and renders on every page, so it is
  126 of the first figure and 7 of the second, and on `ack`, `plan` and `send` it is the
  whole difference; `search` carries 348 of the 602 by itself, its taxonomy block rewritten
  leaf by leaf. Every other moved line falls in one of three classes. Most restate a change
  this record already names one by one. ONE is a quoted string corrected to the bytes:
  `list --help` quoted the subagent brand with one space where `list` prints two after the
  word and two around the separator, so a reader matching the manual against the output
  matched nothing. The REST carry no fact about the binary and are gone: csift's own
  chronology (eleven version riders naming seven versions over five pages, one date on
  which something was measured, and the sentences describing a past csift), a word for who
  is asking rather than for what a flag does, and an example token, now in the
  hand-patterned shape every fixture id in these documents uses. Each of those was
  rewritten as the fact it was there to give, so a page reads correctly to someone who has
  seen no other csift. Every date that remains is an EXAMPLE the grammar needs and belongs
  to no release: twenty-three ISO8601 instants in the `--since`/`--until` blocks of five
  pages, two more in the time-grammar list, the root page's timestamp-format sample, and
  one synthetic record instant inside a rendered bookkeeping head.

- **A manual quotes the brand its OWN command prints, because the subagent brand is four
  strings.** One fact - this row is a subagent transcript, and its bare id is not a
  re-feedable target - is spelled four ways across the seven surfaces that print it: `list`
  writes `SUBAGENT  <hex>  ·  parent SESSION <uuid>`, two spaces after the word and two
  around the separator; `files`, `recover`, `stats` and `verbatim` write one space after
  the word and two around the separator; `show` writes one of each; and `image` suffixes
  its row `  SUBAGENT <hex> · parent <uuid>`, with no `SESSION` at all. So `SKILL.md`'s
  `list` row and SPEC's `image` row quote different strings and both are right, and a
  manual that copied its sibling's would send a reader looking for bytes that surface never
  writes. `SKILL.md` states the `body` field's full condition beside it: `body` is non-null
  exactly where the excerpt cap was lifted, which is `search --no-truncate` and a
  `show --line`/`--uuid` address, while `show --turn` is a WINDOW over the turn numbering
  rather than an address, so its rows render through the ordinary excerpt budget and carry
  a null `body`.
- **A `.rs` comment carries a non-ASCII character only as a quotation of csift's own
  rendering.** A glyph in a comment used to be either a quotation of what csift prints or
  the comment's own arrow, heading rule or ellipsis, and nothing in the character said
  which. Now it is the first only: the glyph exactly as printed, inside a backtick span, and
  the set a comment may quote is derived from what the binary prints (every non-ASCII
  character in a string or char literal of a non-test source file under `src/`, decoded
  through its escapes, plus every one in a clap `///` help doc), 26 glyphs. The pre-commit
  gate enforces it (the character and the balance of the span; the em dash is one of the
  printed glyphs, so the older outright ban on it is this rule read narrowly), and the
  rewrite it required moved no byte of the release binary's strings or help pages: 2,027
  comment lines carried such a character, 1,653 outside any quotation, and the tip carries 0
  violations. `AGENTS.md` section 8 states the rule with its derivation.
- **`recover --help` states the content-anchor rule for reads and writes apart.** It said
  every anchor demands a single simple command and that pipes and chains stay in the
  boundary lanes, which holds for a READ anchor only: a compound command's stdout names no
  one file. A WRITE anchor is per segment of a pipe or a `;`, `&&`, `||` or newline chain,
  so a heredoc write followed by a command that runs the file anchors - a clean
  two-segment chain like that restores the file whole and counts one `bash-write-anchor` -
  provided the whole command's result is clean and no other segment WRITES the same path:
  a `cp` over the file refuses the anchor, while a `wc -l` or a `chmod +x` of it leaves it
  standing. The page now says both.
- **`send`'s receipt counts the body in the unit the budget is counted in.** The chunk
  budget is UTF-16 code units, and the `message` line printed the body in characters beside
  it, so a body of 6,000 emoji read `6000 char(s), 2 chunk(s) of 9200` - a size that fits
  one chunk beside a count of two. The line now prints the body in both units and names the
  budget's: `6000 char(s) / 12000 UTF-16 unit(s), 2 chunk(s) of 9200 UTF-16 units`, where an
  ASCII body reads the same number twice. The JSON summary gains `message_utf16_units` and
  `chunk_budget_utf16_units` beside `message_chars`.

### Fixed

- **The two textless thinking leaves rendered a body they do not have.** An
  `agent.thinking.redacted` unit printed the fabricated words `[redacted thinking]` as its
  text - matchable text with no substring behind it in the raw line, which the literal
  prefilter and the whole-file gate both reason about and which the law admits only through
  a registered marker. An `agent.thinking.omitted` unit whose `thinking` field was
  whitespace printed a run of spaces while promising none. Both bodies are EMPTY now, and
  the consequence is stated rather than met: with nothing rendered, a pattern scan reaches
  neither leaf - use an empty pattern plus an exact `-t`, `--count-by label`, or a `show`
  address, and `--raw` for a redacted block's ciphertext.

- **Three isMeta template arms could be claimed by a record that QUOTED them.** The
  marker law says every such arm tests the text after one left trim, and the hook-feedback
  and `<local-command-caveat>` arms tested the untrimmed string while the edit-failed-retry
  notice tested `contains` - which on that path can only reach a record quoting the notice.
  All three anchor now. Measured over two projects roots the emitted leaf moves for no
  record, so this closes a hole rather than changing a count.
- **A textless image record said nothing about what it was.** An isMeta record whose
  content is image blocks and no text renders no text, so `show --line` on one answered
  with the key=value excerpt of its own top-level keys and nothing else - the head of a
  base64 payload among the values. It now carries its block counts in the label zone
  beside that excerpt: `[context image-only: N image block(s), M document block(s)]`,
  JSON `image_only`. The excerpt stays: it is the line's own keys, rendered by the path
  every textless address takes.

- **An `api_error`'s response headers never reach a rendered or JSON output path.** The HTTP
  shape of that record carries the response headers verbatim, cookie and organization id
  included. The render names the status and the retry counters, the JSON `error` fact carries
  the error's `type` and `message` and nothing else, and `--raw` prints the source line and is
  the one documented way to the bytes.
- **What an `api_error` says failed is searchable, on BOTH observed shapes.** The record
  carries no `content` key at all, so the failure's kind rides the head and its wording is the
  body; without them the statement of the failure was in the output under no pattern that
  could reach it. Neither shape carries both at one level: the header-bearing HTTP shape keeps
  the wording two hops down inside the response body it received, so reading one level printed
  the kind and dropped the sentence.
- **The whole-file gate re-renders from the record's source line.** Stage 2 of the gate is
  the one place csift proves a negative, and it was re-rendering without the line - so for
  a record whose text lives in fields the record model does not carry, it could prove a
  file unmatchable on text the search would have produced.
- **Four shape-inventory rows said a person sees a record they never saw.** The image
  reference, the image block, the coordinator relay and the notification frame are all
  isMeta user records, which the transcript render refuses unless their origin is one of
  five kinds none of them writes. SPEC's inventory table and SKILL's identity cells follow,
  and a drift test now compares the two row for row.
- **Three more of those rows, and two payload types the harness drops.** `atis-latch` and
  `fork-context-ref` read `indirect` and `queue-operation` read `yes`; reading every
  occurrence of the three type literals in the shipped binary finds no reader that renders
  any of them, so all three read `no`. Separately, `goal_status` and
  `async_hook_response_batch` were missing from the never-convert set, and `goal_status`
  carries a leaf that is otherwise visible - so csift reported such a record as delivered
  where the converter drops it.
- **The image-only arm tests authorship like every other injection arm.** A leaf saying the
  harness wrote a record decided that from block shape alone. It now requires `isMeta`, the
  conjunction the rest of the family already takes; all 94 such records carry the flag, so
  what it decides is the shape that has yet to appear. A record without it renders
  `? unmodeled: user/image` under an address.
- **Doc surfaces that said things the code does not.** Two facts, each stated in more than
  one place. `harness.hook.feedback` was described as the Stop hook's, where one builder
  writes four prefixes (`Stop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted`) - corrected
  in the classify arm's own doc, in `search --help`'s leaf list and in the SPEC taxonomy
  clause. And a redacted thinking block was described as a PLACEHOLDER, in
  `model/automation.rs` and in the block loop's module doc: it renders an empty body, and
  what it is rides the label zone as a marker, which is what keeps the fabricated word out
  of matchable text.
- **A stdout write that cannot land is reported now, instead of aborting the process.**
  Piping a command into a reader that goes away - `| head`, a closed pipe - made `println!`
  abort: exit 101 and a std panic message naming `library/std/src/io/stdio.rs` where csift's
  own belongs, with a backtrace only under `RUST_BACKTRACE=1`. Through the shared buffered
  stdout the failed write now takes the one error arm every other failure takes: exit 1 and
  `csift: error: Broken pipe (os error 32)`. The rule is the WRITER that makes the failing
  write and not the subcommand - a `println!` still aborts, so a surface whose rows are
  buffered but whose FIRST stdout byte is a `println!` aborts at that byte. Inside a
  buffered surface there is no exception: its `scope` and `copies` banners go through the
  same writer as its rows, so one query has one answer for a closed pipe however wide its
  scope. Reporting: `search` in all four of its forms, `show` rendered, `--raw`, `--turn`
  and `--uuid`, `agents` in both projections, `files` in its three, `image` in both, and
  every one of `verbatim`'s streams. Aborting: `list`, `stats`, `recover` in all six forms
  measured, `status`, `wait`, `whoami`, `send`, `msg`, `deliver`, `show --branch-points`,
  and `search`'s `-c` / `-l` / `--count-by`. `plan` with no binding and `ack` write no
  stdout at all, and `csift --version` is clap's own writer and exits 0. What a reader that
  stays receives does not move; only the failure path does.

- **A turn-companion record was counted as the person's own turn.** `turnCompanion` is the
  harness's SECOND authorship stamp, on a `type:"user"` record it wrote to ACCOMPANY a turn
  rather than receive from the person, and Claude Code's own is-this-the-person's-message
  predicate excludes it beside `isMeta`, before it looks at the content at all. The two are
  not one set: of the fourteen stamping sites in the verified build, twelve build an object
  that carries `isMeta` and TWO do not - a mapper over a slash command's expansion messages,
  and the tool-dispatch path for the extra `type:"user"` messages a tool returns beside its
  result - and both spread an EXISTING record and add the one key, so a record carrying the
  companion stamp and no meta flag is constructible from either. Reading `isMeta` alone made
  such a record the operator's own turn: it opened a turn, shifted every later turn number
  on that transcript, and moved the live-turn count every surface derives from the same
  predicate - `search --count-by turn`, `stats`, the `<tok>·tN` headers and every `--turn`
  window with them. `is_genuine_user` reads both flags now, so `opens_turn` opens no turn on
  a companion, and `classify` reads them at the same arm, so the LABEL agrees with the
  BOUNDARY: a record csift refuses to call the human is not `user.message` under a selector
  either. A companion matching a template csift models keeps that template's leaf; one
  matching none carries no leaf and renders `? unmodeled: user/turnCompanion` under an
  address, naming the NARROWER stamp: the companion word describes exactly the records
  that carry both, so it is the one a residue unit prints. The key rides the flattened,
  boxed exposed-key struct rather than a field of `Record`, so it costs a record naming no
  exposed key nothing and the row every parsed line of every scan carries does not grow; it
  is the one exposed key read by a PREDICATE rather than projected as a fact, because what a
  reader acts on is the leaf and the turn boundary it changes.

- **A uuid prefix longer than the first segment resolved nothing.** The grammar promised a
  dash-less prefix of 4 to 11 hex, and the resolver compared it literally against the dashed
  basename, so `@<first 9 hex>` held its ninth digit against the dash at position 8 and
  failed with "no session or agent id starts with" while `@<first 8 hex>` resolved. The
  dashed spelling had the mirror gap: a literal prefix of the uuid layout resolved from 12
  characters up and was "not a recognized @-target" at 9 to 11. A prefix is now compared by
  its hex digits, the dashes dropped on both sides, so `@1a2b3c4d5` and `@1a2b3c4d-5` name the
  same sessions, and the dashed spelling is accepted from the first dash on (claim
  PATH-016). Uniqueness is unchanged: two ids sharing the digits are still an error naming
  both.
- **The four edge shapes of the `@` hex grammar are decided, and each miss names the
  rule.** A dashed prefix past 11 characters and a dash followed by one or two hex both
  resolve by their digits. A dash-less run of 12 or more hex is an AGENT id - that agent,
  or the one agent id it starts - and never a session, so a session prefix that long keeps
  its dashes; the agent miss now says so. An agent-id prefix resolves (4 to 11 digits
  through the shared prefix match, 12 to 16 through the agent fallback), and the
  unrecognized-target error lists `@<id-prefix>` and `@<agent-id>` with what each
  resolves.
- **A cost checkpoint said it could not find the two numbers it carries.** The
  `harness.bookkeeping.cost` fact read `startTime` as a string holding an ISO instant, and
  the harness writes it as a number: the session start in epoch milliseconds, which its
  own schema for the line declares `number().nonnegative().finite()`. So every
  `cost-state` unit rendered `[checkpoint closes unknown (needs startTime and
  totalDuration)]` with JSON `checkpoint_closes_at: null`, 96 of 96 lines on one projects
  root, while `--raw` showed both keys on every one. The fact now reads the pair through
  the same function `list`'s `/clear` join uses, so the instant a unit shows is the one
  the join compares. The label prints it as local text,
  `[checkpoint closes 2026-08-31 18:58:32.913 AEST(UTC+10)]`, and `checkpoint_closes_at`
  carries the UTC instant. A value that is not a non-negative number reads as absent, the
  line the harness's own readers refuse, and the empty label now says so: `(needs
  startTime and totalDuration as numbers)`. The `search --help` sample head showed
  `startTime` as an ISO string and shows epoch milliseconds now (claims QT-042, QT-043).
- **`search --help` tagged the structured answer as a gated leaf.** The leaf tree gave
  `agent.message.structured` the gate word `[-t/att]`, which the page's own legend reserves
  for an attachment leaf only a selector or `--attachments` admits. A flagless scan reaches
  it through its own value needle (a session carrying two structured answers counts 2 under
  the leaf in a plain `--count-by label`), so the tree now says `[scan/needle]`. A test
  holds every leaf whose inventory rows all carry the default-needle gate to that word.
- **The root help listed six `status` verdicts.** Its `status` entry named running,
  waiting-children, waiting-hitl, idle-eot, stale-dead and unknown, and left out
  `idle-background-open`, the verdict for a turn that ended with background tasks still
  open, which `status --help` and the verdict enum both carry. The entry names all seven,
  and a test holds it and the `status --help` verdict paragraph to the enum's own set.
- **`recover --help` said FIVE modes and the parser holds six.** The head listed the default
  restore plus four flags while the MODE section of the same page names five mutually
  exclusive flags, `--list-backups` among them. The head now counts SIX - the default
  restore and the five flags - and lists `--list-backups` with the others; a test reads
  the count and the listed flags off the clap `mode` group.
- **A file-history head printed its instant in UTC.** The `harness.bookkeeping.file-history`
  head read `[file-history snapshot at 2026-09-01T05:49:11.520Z: ...]`, the one text
  instant csift left in UTC, on every one of the 7,694 such units one projects root
  carries. Both forms now print local text in the millisecond form the search and show
  headers use - a delta's top-level `timestamp` and a snapshot's nested
  `snapshot.timestamp`, the only instant that line carries - and the UTC string stays
  matchable in the unit's verbatim JSON body (claim QT-025).
- **A queue rider that quoted the channel envelope reached a flagless scan.** The
  csift-channel keep admits any line carrying `csift-channel v1`, and a `queue-operation`
  rider carries whatever its `<task-notification>` summary says, so a pulse that mentioned
  the envelope surfaced as `harness.bookkeeping.queue` with no `-t` at all - 13 units on
  one projects root. A `user.queued` or `harness.bookkeeping.*` record now survives only a
  scan that reaches its own leaf; `-t harness.bookkeeping.queue` (or the family glob)
  still emits the line, and an address still renders it (claim QT-005).
- **`search --since -1y` was a usage error.** The WHEN grammar tolerates a leading `-`
  and every other WHEN flag took the space form, but `search`'s `--since` and `--until`
  lacked the hyphen-value allowance, so clap read `-1y` as an unknown short flag and
  exited 2 (`unexpected argument '-1' found`) while `--since=-1y` worked. Both flags take
  it now. `--turn` already did and `--max-count` reads a negative number by design.
- **`--for` on an address that names no record read as an empty closure.**
  `search --for L99999999` exited 0 with `for_of {records:0}` and the ordinary
  zero-match note, so "nothing is for this line" and "this line does not exist" read
  alike. An address that names no record in any transcript of the scope now fails as
  `show` does, in the same words (`no such record(s): L99999999 - ...`); the test reads
  the file's own bytes, so it does not depend on the pattern or the prefilter. A real
  address whose closure matches nothing stays a definitive absence, exit 0.
- **Two `wait --help` examples could not run as written.** `csift wait @main --until
  auq` and `csift wait @<uuid> --until tool:Read:handover` omitted `--timeout`, which
  `wait` requires, so both were errors when pasted. They carry `--timeout 600`, and a
  test holds every `csift wait` example on the page to the required flag.
- **A `show` record row lacked seven keys the `search` hit for the same record
  carries.** A notification pulse fetched by its hit's own `refetch` came back without
  `task_ids` and `orphan_kind`, and every row lacked `delivered`, `is_error`,
  `denial_kind`, `resume_paired` and `scheduled_at`. Both surfaces now build their
  shared keys through one projector, so a refetch answers every key the hit answered,
  and `show --help`'s key list names the seven, plus the five draft-distance keys
  (`superseding_line`, `superseding_uuid`, `diff_chars`, `diff_pct`, `diff_exact`) the
  row already carried and the list never named.
- **`status`'s stopped-agents note printed a raw UTC instant.** `N background agent(s)
  were stopped by the user at 2026-06-07T05:10:00` was the one text instant on the
  surface not in the canonical local form. It renders through the shared timestamp
  helper now (`2026-06-07 15:10:00 AEST(UTC+10)` in that zone), at the second
  precision the note's de-duplication already keyed on.
- **The stopped-agents notice on the input queue read as the person's text.** Claude
  Code queues the `N background agents were stopped by the user: ...` notice the way it
  queues a typed prompt, and csift labelled that queue line `user.queued` (13 records on
  one live root, 22 on a snapshot) while its delivered twin was already
  `harness.notification.subagent`. The line keeps no key a person's lacks, so the
  template itself refuses it, the predicate the delivered record is refused by; it
  takes `harness.bookkeeping.queue` like every other rider (claim QT-005).
- **`status` and `wait` answered a multi-session target for one session it picked.**
  `csift status <project path>` resolved every session in the directory and reported
  the first by sort order, with nothing saying so (on one live project directory, 23
  sessions and a verdict for an idle one nobody named). Both commands now refuse a
  target that resolves to more than one session id, naming the count and the target
  grammar; copies of one session id still count as one.
- **One agent id at both sidecar locations of one session was read in halves.** A workflow
  agent whose conversation continued past its run has a second transcript at the built-in
  `subagents/agent-<id>.jsonl` beside its `subagents/workflows/<run>/` one (a continuation,
  not a copy). `@<id>` read one half with nothing said, a `*.jsonl` path naming the built-in
  file read the workflow file instead, and the note `list` printed for the pair called it
  two project directories. Every route now takes one rule: `@<id>` reads both halves, a
  path reads the file it names and names the other on stderr (two paths naming
  byte-identical copies of one transcript still read as one), and the note says the pair
  sits under one session's sidecar at the built-in and the workflow locations (claim
  PATH-017; PATH-014 refined). `show @<id>` still refuses the pair and names both paths.
- **`whoami --help`'s fallback recipe built `@null`.** Its second line read
  `parent_session_id` off a `whoami` identity row, which the env form leaves null (the
  environment names the top-level session in every lane, so it cannot say which lane
  the caller is). The recipe reads the parent off the `agents` node instead and says
  that `no subagent matched` means the env id is already the root; a test holds every
  recipe on the page off the identity row's parent.
- **`show`'s address miss spelled a range with a dash.** `csift show @<id> --line 99999..`
  on a 5,003-line file failed with `no such record(s): L99999-5003`, while `--line`
  refuses the `A-B` form it printed. Both fetch modes now name a missed range in a spelling
  the parser accepts when pasted back: the grammar's own `A..B` for a range that resolved in
  order, and the range as written for one that resolved backwards - `--line 99999..` misses
  as `L99999.. (file has 5003 lines)`, where the resolved pair `L99999..5003` is a range
  `--line` refuses as reversed.
- **`show` booked lines past the end of the file as non-record lines.** `--line 4995..5045`
  on a 5,003-line transcript reported `records: 9, non_record_lines: 42`: the 42 lines
  that do not exist were counted with the metadata lines that do. A range is now clamped
  to the file and the overhang is its own count, JSON `lines_past_end` beside
  `non_record_lines`, with a footer line (a stderr note under `--raw`) naming the file's
  last line; the same address now reads `non_record_lines: 0, lines_past_end: 42`. A line
  or range that starts past the end stays a miss, and the miss names the file's line
  count in both fetch modes. `show --help` names the new key.
- **`verbatim` let the last span flag win.** `csift verbatim @<id> --subagents
  --no-subagents` exited 0 and quietly reconstructed the top-level thread, while every
  other span command refuses the pair as a usage error. `verbatim` now declares the same
  conflict and exits 2 on the pair in either order; `verbatim --help` no longer says
  `--no-subagents` cancels an earlier `--subagents`.
- **`stats` counted its scope after the row cap.** `csift stats . --max-count 1 --format json`
  headed its stream `sessions_in_scope: 1` while `csift list . --max-count 1` read 1,713 for
  the same target: `stats` counted the rows it kept. The scope banner and the JSON header now
  count the whole resolved scope before the cap trims the rows, the rule `list` already kept,
  so the two commands report one scope for one target (after the change both read 1,722
  for `.`).
- **Smart-case read the letter in an escape as uppercase.** `\Sbash` searched
  case-sensitively (38 records on one transcript, the `(?-i)\Sbash` count) because the `S`
  of `\S` was taken for an uppercase letter, while `(?i)\Sbash` found 46. Smart-case now
  reads the parsed pattern, ripgrep's rule: a letter the pattern spells counts, bare or in
  a bracket class (`[A-Z]` stays case-sensitive), and `\S`, `\W`, `\D` and `\B` do not, so
  `\Sbash` reads 46 and `\Wbash` 68. `search --help` states the rule.
- **Two help recipes read the envelope's header first.** `csift status @<uuid> --format json
  | jq .verdict` on `status --help` printed `null` (the header) before the verdict, and `csift
  msg <ID> --format json | jq .fact` on `msg --help` printed `null` around the fact. Both
  recipes now select the row that carries the key (`select(.kind=="verdict").verdict`,
  `select(.kind=="msg").fact`), and a test holds every `jq` recipe on every help page to a row
  select, `tail -1`, or `wait`'s single object.
- **A malformed session-id variable was read as a session.** `CLAUDE_CODE_SESSION_ID=not-a-uuid
  csift whoami` printed `session  not-a-uuid` and exited 0. Claude Code writes the calling
  session's own id there, a uuid on every build, so any other shape was set by something
  else. `whoami`, every `@main` resolution, `plan` with no target, `msg` and `ack` now read
  the variables through one reader that refuses such a value, naming the variable, the value
  and the `@<uuid>` remedy; an unset or blank variable still reads as not set. `@main`'s
  note that it resolved the top-level session via the environment now follows the read, so
  a refused or unset variable prints its refusal alone.
- **An empty target token scoped to the current directory.** `csift search zzqq ""` searched
  every session of the cwd's project (1,722 of them) and exited 0: the empty token was read
  as the empty relative path. Sixteen target spots now refuse an empty or whitespace-only
  target at parse time (exit 2) with the target grammar in the message: the twelve commands
  that take a path target, and the four channel spots that take a lane as a string, where
  `send "" <text>` had queued the message to the cwd's session and `whoami --to ""`
  predicted a delivery to it (`msg --lane ""` and `ack --lane ""` were refused, by the
  @-grammar at run time). An empty `$(...)` substitution is the usual source, and the
  message says so. `search ""` as the PATTERN is unchanged.
- **The three terminal modes said nothing about a zero-match search.** `csift search
  zzqqnothing @<id> -c` printed `0` and no diagnosis, and `-l`, `--count-by` and `--raw`
  were as silent, while the ordinary render framed the same absence on stderr as definitive
  and, under a label filter, named the labels the pattern does occur under. Every mode now
  prints that diagnosis on stderr; stdout is unchanged.
- **`agents --agent` refused the `@` sigil.** `csift agents @<uuid> --agent @<agent-id>`
  failed with `no subagent matched id` while every other id position takes the sigil and
  the error's own remedy prints ids beside `@<uuid>`. The flag now takes the id bare or
  `@`-prefixed and both grab the same node; `agents --help` says so. One sigil is all it
  takes: a token still carrying an `@` after it (`@@<id>`, a `<Name>@<Team>` routing form)
  is refused as no agent id, where it was searched for and reported unmatched.
- **`search --for L<n>` walked every transcript in scope.** A line number names a record in
  one file, yet `csift search "" --for L42` with no target closed over line 42 of every
  transcript under the projects root (1,308,495 output lines on one machine), and over a
  session with subagents it mixed a different record's closure from each file. A line
  address now needs a scope that resolves to exactly one transcript and is refused with the
  count otherwise; a uuid address, which names its record in any scope, is unchanged.
- **`agents` called a lane completed in the middle of its work.** A subagent whose newest
  record was a RETURNED tool_result (the model still writing its next block) read
  `completed`, with `completed_utc` set to that record's instant, because the end-of-turn
  check walked back to an earlier turn's text; a running lane measured that way while
  `status` counted it live in the same minute. The verdict now comes from the newest
  meaningful record, the one the frozen-lane check already reads: an assistant text ending
  its turn is `completed`, a returned tool_result (or a text stamped `stop_reason:
  "tool_use"`, whose call is still to come) is `running` with no completion instant. On one
  root 150 of 10,947 lanes move from `completed` to `running`, every one stopped mid-turn
  hours ago (127 by an interrupt); `running` is documented as "still running, or
  interrupted" (claim SUB-034). `agents --help` states the rule.
- **`status` counted a finished workflow's agents as in flight forever.** On a session
  whose 31 workflow runs had all ended, `status` read `1399 lane(s), 145 live (144 workflow
  agent(s) in flight)`: it summed `started` minus `result` over every journal, and an agent
  that failed writes `failed` instead, while one killed with its run writes neither. The
  in-flight count now covers only runs with no manifest yet (the harness writes a run's
  manifest once it ends) and counts `failed` as a close, and a workflow lane is settled by
  its ended run or its journal's closing line before its tail is read; the same session
  reads `1399 lane(s), 1 live` (claim TAIL-008). An open agent whose own lane is already
  live by its tail is counted once, not again through its journal's `started` line.
  `agents` reads the same rule before a lane's tail: on that session it had called 42
  workflow lanes `running` that `status` settled, and now calls none; `agents --help`
  states the rule.
- **`agents` called a lane its run killed, or its journal failed, `completed`.** A workflow
  lane settled by its run read `completed` with a completion instant whatever the run said:
  a lane of a run whose manifest reads `killed`, and a lane its journal closes with
  `failed`, looked exactly like a lane that returned. The run's word now decides: the
  agent's journal line first (`result` completed, `failed` failed), else the ended run's
  manifest status verbatim, so such a lane reads `killed` or `failed`, carries no
  `completed_utc` or `duration`, and names why - JSON `settled_by`, and the same words in
  brackets on the text row, as `status` words a settled lane. A lane that returned before
  its run was killed still reads `completed`. On the live root 224 of 10,992 lanes move:
  120 to `failed` by their journal line, 101 to `killed` and 3 to `failed` by their run's
  manifest (claim SUB-034).
- **`files` joined an unexpanded shell variable onto the cwd.** A script writing
  `$FIX/.claude/.../$parent.jsonl` from an unquoted heredoc was reported as `cwd-joined`
  with the path `<cwd>/$FIX/.claude/.../$parent.jsonl`, a file that never existed. An
  operand still carrying `$` or a backtick is now `unresolved` and kept verbatim, whatever
  its shape; over one root 98 rows move (35 `cd-tracked`, 63 `cwd-joined`) and no other row
  changes class. A `~` operand stays `unresolved`, since the recording shell's HOME is not
  in the transcript (claim BASH-023).
- **A fallback-served turn's `served_by` reached no scan.** Every `fallback` block on disk
  is the sole block of its record (48 of 48 on one root), so the record had no text and a
  scan emitted nothing for it: `show --line` rendered `served_by`, while `search` over the
  12 sessions that carry a refusal returned 0 hits with it. The record now emits one
  `agent.message.plain` unit on a scan as under an address, its body the marker's own
  words (`served by <to.model>, fell back from <from.model>`), so `-t agent.message.plain`,
  a model id or `fell back` reach it; the same query now returns 46 hits, one per distinct
  record. A record that fell back and spoke keeps the marker on its text unit and emits no
  second one. `search --help` says so (claim CLS-060).
- **A loop tick fired by the scheduler lost its instant and its edge.** A fired prompt whose
  armed text opens with a loop marker takes the wakeup or tick leaf, and those read
  `scheduled_at: null` with only a `parent` edge, although a `scheduled_task_fire` record is
  their parent exactly as it is a plain fired prompt's (3 such prompts on one root). The
  instant and the `fires` edge are now read off the parent link for every leaf a fired
  prompt can carry (fire, wakeup, tick), the fire index is built whenever a selection
  reaches one of those leaves, and the label zone prints `[scheduled fire <when>]` on them;
  a tick with no fire record above it carries neither, and so does any OTHER record whose
  parent is a fire record - Claude Code parents each append to the running tail, so a
  background agent's `<task-notification>` arriving next is one, and it did not fire
  (claim CLS-032).
- **The `history-suppression` head printed a raw UTC instant.** `csift search ''
  <session> -t harness.bookkeeping.suppression` showed `ts=2026-09-03T00:23:01.606Z`, the
  last text instant in the bookkeeping family not in csift's local form. A head value that
  is an RFC 3339 instant now renders as the local text every other text instant takes
  (`ts=2026-09-03 10:23:01.606 AEST(UTC+10)` in that zone); the UTC string stays matchable
  in the unit's verbatim JSON body. `cost-state`'s `startTime` is an epoch-millisecond
  number, not an instant string, and prints as written (claim QT-049).
- **`files --help` named row keys the rows do not carry.** Its `op` list stopped at
  `notebook_edit` although the timeline reports `external_write` (49 rows on one root), the
  timeline schema left out `line` and `detail`, the `--by file` schema called the group key
  `file` where the row carries `path`, and the dir and summary views were said to group
  "under a `dir`/`bucket` key" where those are the row KINDS and the group sits in `path`.
  The page now lists what the rows carry, and a test holds each view's row keys to its
  schema entry.
- **The root `--help` page said every command emits one envelope and three commands
  write.** `deliver` prints a hook-output object or nothing and `wait` one bare
  `{kind:"wait"}` object, and `recover --out`, `recover --files-from ... --out-dir`,
  `verbatim --out` and `image --out` write wherever the user points them. The page now
  names the two envelope exceptions and lists each user-named output with what it writes
  and whether it replaces an existing file; a test derives the output flags from the clap
  tree and requires the page to name each.
- **`search --help` described a scan wider than the one it runs, and named the wrong gate
  on three leaves.** "With no `-t`, every label is searched" skipped the gated leaves (the
  whole `harness.bookkeeping` category, the gated attachment, system and turn leaves); the
  page now says a flagless scan reaches the leaves the tree tags `[scan]` or
  `[scan/needle]` and that a gated leaf needs `-t` (or `--attachments`). The tree tagged
  `harness.hook.context` `[-t]` while `--attachments` admits it, and the csift-channel
  delivery `[scan]` beside a sentence calling it a needle leaf. The `-t` flag named three
  of the 28 leaves a bare face skips and gave `system`/`local_command` as its delivery
  example, whose leaf the model receives anyway; it now names all 28 by group and uses a
  rendered `hook_success`. The EXPOSED KEYS section said five keys render a tag (six do)
  and never named `stop_reason` / `stop_details`; the attachment section kept a sentence
  saying the mid-turn steer reads `[payload]`; and `harness.context.skill` is now described
  as any injected skill body, not only a subagent's preloaded one. Tests derive every
  leaf's gate word from the candidate gate's own leaf sets, every skipped leaf from the
  visibility table and every key from the keys family's projector.
- **`msg --help` called a csift-channel delivery the one attachment payload a default
  scan parses.** Three attachment leaves are reached by a flagless scan: the channel
  delivery, the person's mid-turn steer and the model's structured answer. The page now
  says so, and says that `msg` reads the delivery off the envelope header at the start of
  a hook-context attachment rather than running a search.
- **`whoami --help` said a built-in subagent's environment holds its own id, and that
  whoami takes `@trap:<marker>` or `@main` only.** Its long description contradicted its
  own after-help: current Claude Code hands every lane the top-level session's id, the
  subagent's own being withheld. And a lane id (`@<agent-id>`) and a teammate's routing
  form (`@<Name>@<Team>`) both resolve as the self target, printing that lane's sections.
  The page now states the one reading and names all four forms and the refused session
  uuid; a test holds both.
- **`verbatim --help` said csift's own walk stops at a compaction cut, and named the
  exchange header `s·t<n>`.** The survival walk crosses up to `--cross-compactions`
  boundaries (default 3) through each boundary's `logicalParentUuid`, and only budget 0
  stops where Claude Code's loader does; the turns it reaches read `live`, and file order
  still replays the ones above the floor as `pre-cut`. The header token `search` prints is
  `<tok>·t<N>`. A test reads the default off the flag's own constant.
- **`agents --help` put the notification pulses under `search -t user` and read a run's
  status off the journal.** A `<task-notification>` pulse is `harness.notification.<kind>`,
  never `user`, and its kind set includes `workflow`, the one token the page says the two
  axes share. A RUN row's `status` is the run manifest's own field
  (`<session>/workflows/<run>.json`, written once the run ends); the journal carries no
  run status. The page now says both, and lists `failed` beside `completed` and `killed`.
- **`plan --help` said `--reverse` matches `plan_mode` attachments only, and two messages
  said a plan is bound only by entering Plan Mode.** The reverse lookup resolves each
  transcript's binding by the same two laws as the forward direction, so a session bound
  by its first slug alone (a forked or background session) is found too, with
  `binding_source: "slug-only"`. The flag's help now names both laws, and the
  `recover --file @plan` refusal and the `plan` no-binding note name both as absent.
- **`verbatim`'s markers never agreed with a count of one.** A turn with one tool call
  printed `[1 tool calls]` and a cut removing one line printed `1 lines elided`, while the
  fold placeholder already wrote `1 agent message`. The kept-turn marker, the elision
  marker and the placeholder's character count now take the placeholder's rule, each
  noun agreeing with its own count (`[1 tool call]`, `[+1 char, 1 line elided]`); the
  cost model reads the same line the renderer prints. Three lines of the golden baseline
  moved: two `[1 tool calls]` markers and the header's chars-used total, which counts
  them.
- **`verbatim --help` said Claude Code's hook-context cap counts Unicode scalars.** It
  counts UTF-16 code units: the same number for BMP text, CJK included, but an emoji or a
  CJK ext-B character is one scalar and two code units, so a `--window` sized at the cap
  can cross it. The `--slice` and `--window` help now name the two units apart and ask for
  more headroom on a document carrying astral characters.
- **A duplicate note said "under 1 project directories".** Two copies of one agent id in
  two sessions of one project read `at 2 paths under 1 project directories`; the count now
  takes its noun in the singular at one, like every other count the disclosure prints.
- **`plan` bound every subagent to its session's plan file.** With no `plan_mode`
  attachment the first slug binds, and a subagent's records carry the session's slug, so
  every subagent transcript resolved to the session's `<slug>.md`: one session with 1,401
  subagents printed 1,402 bindings of that one file, and `plan --reverse` on it listed all
  of them. Claude Code names a subagent's plan `<slug>-agent-<agent id>.md`, the lane's id
  verbatim (a teammate's name included), and the slug law now names that file for a
  subagent transcript; `binding_source` stays `slug-only`. The reverse lookup of the
  session's plan now finds the session alone, and each of the two subagent plan files on
  disk whose transcript survives now resolves to that subagent.
- **A channel message of emoji or rare CJK could arrive as a file pointer.** The chunker
  kept each chunk within 9,200 characters, while the harness spills an `additionalContext`
  over 10,000 UTF-16 code units to disk and shows the model a pointer and a preview without
  the envelope header. An emoji or a CJK extension-B ideograph is one character and two
  units, so a body of 6,000 emoji rendered one chunk of 12,486 units. The budget is now
  counted in code units and every cut falls between whole characters: that body renders
  two parts of 9,199 and 3,335 units, and `send` counts the parts the same way. Bodies of
  ASCII and BMP text chunk exactly as before.
- **A zero-match search called `harness.hook.context` reachable only by `-t`.** Its
  inventory row still carried the selector-only gate from before `--attachments` covered
  it, so the note listing the leaves a run never scanned named it even on an
  `--attachments` run that had parsed its line, and a run that named the leaf printed no
  note although the 23 other selector-only leaves went unscanned. The row now carries the
  attachment gate the scan applies: the note lists 23 leaves, never the hook context, and
  `-t harness.hook.context` alone prints it with `gated_leaves_unreached: true`. The
  SPEC inventory table's row reads `-t or --attachments`.
- **An excerpt clipped by one character said `… (+1 chars)`.** The explicit elision
  marker every excerpt ends with - `search`'s head and match-centered forms, `list`'s
  previews, `show --turn`, `agents --returned-message`, the bookkeeping heads - now names
  one character in the singular, `… (+1 char)`, through the same count-and-noun rule the
  `verbatim` markers take. Both marker builders go through one function, so the forms
  cannot drift apart again.
- **A zero-match search without `--attachments` said nothing about the attachment lines it
  skipped.** A flagless scan reads an attachment line only through the three default
  needles, so a pattern whose only match sits in, say, a hook's injected context returned
  0 with no word on stderr about those lines. The zero-match note now also names the
  attachment leaves the scan did not read, and that `--attachments` or a `-t` naming the
  leaf reaches them, whenever neither was given; the JSON summary carries the list as
  `attachment_leaves_unread`. The list is the shape inventory's attachment axis, derived
  rather than typed.
- **An empty `--from` read as a sender nobody named.** `whoami --to <lane> --from ""`
  predicted a send from an `external unknown` sender, and `send <lane> <text> --from ""`
  queued one, where `--to ""` and an empty lane target are refused. `--from` takes
  `@<lane>` inside Claude Code or a non-empty free label outside it, so both commands now
  refuse the empty token at parse time (exit 2) naming that grammar.
- **`show --line -1..-3` answered differently in the two fetch modes.** A from-end range
  whose ends reverse on the file (the last line, then the third from the end) names no
  line; the rendered fetch refused it with the unparseable-line explanation, while `--raw`
  printed nothing and exited 0. Both modes now resolve every `--line` range the same way
  and refuse an empty one before either reads a line, with one message naming the range as
  written and the file's length: `no such record(s): L-1..-3 (file has 3 lines)`.
- **A draft one character from its resend said `differs ... in 1 chars`.** The
  draft-distance line on a `user.unsent` unit (`search` and `show`) formatted its count
  with a fixed plural, outside the rule the excerpt marker takes; it now goes through that
  rule and reads `differs from the sent message in 1 char`. JSON `diff_chars` stays a
  number.
- **`verbatim --help` listed a key the rows do not carry.** The JSON schema named
  `line_no` in the per-unit block while every unit row carries `line`, the jsonl-line key
  every command's rows use. The same blocks left out keys the rows do carry: a unit's
  `source` (`elicitation-sidecar` on a merged pending unit, whose `line` is null), the four
  keys an inbound peer-message unit adds, the id trio, `turn_index` and
  `compactions_before` on a `collapsed_agents` row, and `command` plus the four
  duplicate-copy keys on the header. The blocks now list what the rows carry, and
  `recover --help` names `line` as the key `--at @line:<N>` reads. A test holds every key
  each verbatim row kind emits to the page, and every entry of the per-unit block to the
  rows.
- **`image --format json`'s header left out the scope counts.** Every other spanning command's
  header carries `sessions_in_scope`, `top_level_sessions` and `subagent_sessions`, built by
  one function, and `image` built its header with the scope-less form in both the listing
  and the `--out` extract stream. Both now go through the span builder, counting the resolved
  transcript set before any filter narrows the images, so an image header reads the same
  three numbers `list` gives the same scope.
- **The teammate hint told a reader SendMessage takes the routing id.** Its third line said
  the official SendMessage needs `Name@Team`, and the JSON `control_hint` said SendMessage
  takes `routing_id`, while SendMessage's input validation refuses any `to` carrying `@`
  (`to must be a bare teammate name`). The hint now says what each tool takes: SendMessage
  the bare name, or the `a<Name>-<hex>` transcript id, which also resumes a teammate that is
  not running; TaskStop the name or `Name@Team`. `agents --help`, the root help's
  `@<Name>@<Team>` sentence and the no-match error of that target said the same wrong
  thing and now say the same true one (claim SUB-035).
- **`send` printed a SendMessage call that SendMessage refuses.** For a teammate receiver the
  delegated call read `SendMessage(to: "Name@Team", ...)`, the routing form, and SendMessage's
  input validation refuses any `to` carrying `@`. `send` and `whoami --to` now print the
  receiver's transcript id, `SendMessage(to: "a<Name>-<hex>", ...)`, the form they already
  printed for every other agent lane: SendMessage's agent-id arm delivers it to the running
  teammate whose resumable id it is, or resumes that teammate, which is what the resume row
  delegates. The receiver line still names the routing form beside the lane, and the
  outbox's `official.to_form` records the id the call was printed with (claims SUB-035,
  MSG-037).
- **`recover --file <basename>` stitched several files into one.** The basename-suffix
  fallback matched every record path the argument ended, so a basename three files share
  replayed all three files' writes and edits into one buffer - on a fixture of three
  `lib.rs` files, the default restore printed one of them and called it complete, and
  `--coverage` counted all six events. A target that matches more than one distinct file
  in scope is now refused before anything is replayed, in every mode, naming each
  candidate and the count - a relative record path counts, as the file its record's shell
  cwd resolves it to (a tool result's `filePath` through the cwd of the call it answers), so
  `other/lib.rs` beside `/p/m/lib.rs` is refused too. A file-history
  snapshot key is relative and its line carries no cwd, so it resolves against the
  transcript's first cwd, the directory it is relative to, and names the same file as the
  absolute Write/Edit path it is the tail of rather than a second one; that join is the
  candidate a refusal prints for it, which in a session that changed directory before its
  first write can be a path no record wrote (`recover --help` says so). A candidate, or a
  longer suffix that names exactly one file, works as before, and an absolute target now
  also finds a relative record path that resolves to it. Every candidate the refusal prints
  replays when passed back: one the other candidates end with (a relative path from a
  transcript with no cwd) replays that file alone, with a note. A `--files-from` line takes
  the same rule: its report row reads `ambiguous`, nothing is written for it, and the
  summary line counts it.
- **`verbatim`'s compaction banner said the turns below it predate the summary.** The
  banner sits where the walk, going down the file, passes from turns that end before the
  summary's line to turns that end after it, so the turns below mostly FOLLOW the summary:
  on a fixture whose summary is L5, the banner sat between L2 and L3 and every line below it
  but the L3 opener came after L5. It now reads `══ compaction boundary · summary at L5 ·
  (turns above end before it, turns below end after it) ══`. A turn below can still open
  before the summary when the compaction lands mid-turn, so the banner names where turns
  end and makes no claim about when they began. The golden reconstruction moves on its
  three banners and the header's `chars used`, which counts them.
- **`verbatim --budget 1` said `budget 1 chars`.** The scope line printed `budget 1 chars is
  PER session → up to 1 chars total` and the per-session header `budget N chars` with a
  fixed plural; both budget figures, and the header's reserved worst case, now go through
  the count-and-noun rule every other csift count takes: `budget 1 char is PER session →
  up to 1 char total`, and `budget 2 chars` as before.
- **A shell result quoting the AskUserQuestion answer text no longer opens a turn.** The
  answer's marker fallback, for a carrier with no `toolUseResult`, read the text alone, so
  a `tool_result` whose content opened with `Your questions have been answered:` - a
  `strings` run over the harness binary prints exactly that - labelled `user.answer` and
  opened a turn. The fallback now answers only when that result's `tool_use_id` names an
  AskUserQuestion call of the same transcript; such a shell result keeps
  `agent.tool.result`, and every surface that numbers turns (`search`, `show`, `stats`,
  `verbatim`, `files`, `recover`, `image`) counts one turn fewer on its transcript.
  `files` and `recover`, whose prefilters keep file traffic, also parse the line carrying
  the AskUserQuestion call, so a genuine marker-only answer opens its turn there as it does
  on every other surface. A surface that reads no whole transcript (the `list` preview,
  the live tail) renders an answer from its structured fields only.
- **`verbatim --format json` names itself in its header.** The header line read
  `"command":"turns"`, the command's name before its rename, while every other command's
  header names that command; it reads `"command":"verbatim"`.
- **`[not delivered]` prints wherever the JSON `delivered` reads false.** The text tag
  followed only the per-record delivery override, so a leaf the model never receives by
  its leaf - the turn closers, a compaction boundary, a scheduler event, a kill, a queued
  or recalled message - read `delivered: false` in JSON and carried no tag on `search`
  or `show`; the bookkeeping family printed its own copy to cover its leaves. One verdict
  now drives both: every undelivered unit is tagged once, and a bookkeeping line's label
  is unchanged. `search --help` states the rule.
- **`search -c`, `-l` and `--count-by` name a skipped malformed line.** A torn line was
  never matched, so a count, an id list or a census over a scope holding one covers the
  parseable lines only - and those three modes said nothing about it. Each now prints
  `csift: note: N malformed line(s) skipped` on stderr, as `--raw` does, and
  `-c --format json`'s summary carries `skipped_lines`. On a zero match the note stands
  down in every terminal mode, because the zero-match caveat already names the line and
  what it means for the absence: one mention, never two.
- **`deliver --recipe` says where its block goes.** The block's top level is
  `{"hooks": {...}}`, and the note said to paste it into the `hooks` object of a
  settings.json, which nests `hooks.hooks`. The note now says to merge it at the file's
  top level - the block's `hooks` key with the file's own, each event's array appended to,
  never replacing, the one already there - and `deliver --help` names the fragment the
  same way.
- **The documents say which `show --raw` forms disclose a compaction crossing.** SPEC and
  the maintenance manual listed `show --raw` among the modes that print the crossing report
  on stderr; only a `--turn` window does, as `show --help` already said. A `--line` or
  `--uuid` raw fetch builds no chain - its bytes are the same under every budget - and
  prints nothing about crossing on either stream; no output moved.
- **`verbatim`'s skip note names the budget that renders.** A skipped session's `needs ≥ N
  chars` added the header rendered at the budget the run was given, and the header prints
  that budget, so N moved with its digits and its plural - on one fixture `--budget 1` said
  392 and `--budget 100` said 399, the budget that renders. N is now the least budget that
  fits the header rendered at N itself, the same at every skipped budget.
- **`recover`'s several-files refusal says where it came from and what it prevents.** A
  `--files-from` line's note named it `--file`; the batch summary counted an unreplayed
  ambiguous line among the recovered files; the refusal named only the stitching outcome,
  though candidates in different transcripts would replay one file's history and drop the
  others'; and it listed every candidate, one line each however many there were. The note now
  names the manifest line, the summary counts it beside the recovered files, the refusal
  names both outcomes and lists the first ten candidates with a count of the rest.
- **`recover --file` in one spelling of a file read only that spelling's events.** A
  relative record path joined to its cwd was normalised and every other spelling compared
  as a string, so after a write to `./a/lib.rs` and an edit through `a/lib.rs`,
  `--file ./a/lib.rs` printed the first write's content and called it complete, and
  `/p//q/lib.rs` beside `/p/q/lib.rs` was refused as two files. Paths now compare
  lexically, the target and every record path alike: `.` segments are dropped and a run of
  `/` collapses to one (a leading `//` stays), so any spelling replays the file's whole
  history. A `..` segment and a symlink's other spelling are not resolved: those stay two
  files, and `recover --help` says what each spelling then replays.
- **The `agents` teammate hint no longer offers SendMessage's refusal as the reason TaskStop
  takes the routing id.** Its third line now states the two tools' facts apart: the routing id
  `Name@Team` is a form TaskStop takes, and SendMessage, which refuses any `to` carrying `@`,
  takes the bare name or the transcript id. `agents --help`'s paragraph on a teammate's two ids
  says the same: a form TaskStop takes, beside the bare name.
- **`whoami`'s environment form no longer says a lane has no parent.** With no target the
  lane comes from `$CLAUDE_CODE_SESSION_ID`, which names the top-level session in every lane,
  and the `parent` line read `none (a top-level session has no lane above it)` while the
  JSON parent was null and the depth line said unknown; read from a subagent the line was
  false. It now reads `unknown from the environment alone (a top-level session has none; a
  subagent's parent is this session)`; a target that resolves the lane keeps its parent.
- **A zero match under `--turn` blamed the label filter for what the window left out.** An
  abandoned record - a recalled draft, a rewound turn, a record on an abandoned branch -
  belongs to no numbered turn, so a `--turn` window leaves it out, and the diagnosis probed
  only `-t`: `-t user.unsent --turn ..` over a session with one draft said the pattern
  occurs under other labels and the label filter excluded them. The diagnosis now also
  counts the records the pattern and label filter match outside turn numbering and, when
  there are any, says so on a line printed first, naming both ways back (drop `--turn`, or
  window by time); the JSON summary carries the count as `outside_turn_numbering`. `search
  --help` says so.
- **`show`'s cap continuation dropped the records after an abandoned one, and fetched
  records nobody addressed.** The rendered fetch kept its first `cap` units in render order,
  which puts an abandoned unit after every live one, and read the continuation off that
  order, so a window with an abandoned record printed a continuation that stopped at it or
  ran backwards: on one transcript `--line 1..400` continued with `--line 201..393` and lost
  L394-L400. The cap now keeps the records on the lowest lines, whole records, and the
  continuation is written in the addressing the reader used. A line address continues as
  its own ranges clipped at the first dropped line (`--line 1..4,9..14` cut at L3 continues
  `--line 3..4,9..14`, where one range from L3 also fetched L5-L8), a uuid address as the
  dropped uuids, and a mixed one as both; each fetches exactly the dropped records. A
  `--turn` window continues as a window, `--turn K..<its end>`, and when the cap cut inside
  turn K it passes over that turn's units already shown with the new `--skip M`, which
  drops the first M record units of what an address selects, in the order the cap keeps
  them, before the cap applies (lines under `--raw`; the JSON summary counts them as
  `dropped_by_skip`). A window starts at a turn's first record, so without it a turn wider
  than the cap continued as the command just run and its later units were never reached.
  Every form now moves forward: following the continuations at one cap until none is
  printed yields every unit of the uncapped fetch exactly once. A `--skip` past everything
  addressed prints an empty page and a stderr note, exit 0. `show --raw`'s stderr
  continuation follows the same rule. `show --help` says so under `--max-count` and
  `--skip`.
- **`show --raw` by uuid fetches the records the rendered fetch does.** A uuid a replay
  copy shares with the line that survives it fetched both rendered and only the copy raw;
  it now prints both lines, lowest first. The uuid of a record merged from the elicitation
  sidecar exited 1 with `no such record(s)` under `--raw` while the rendered fetch showed
  it; it now prints nothing for that record and a stderr note saying it has no transcript
  line, exit 0, as `search --raw` does. A uuid that names no record stays a miss.
  `show --help` says so under RAW MODE.
- **A quoted relay framing no longer emits an inbox unit.** An assistant text opening with
  `Another Claude session sent a message:` and a quoted `<agent-message>`, and a compaction
  summary carrying a peer section, each emitted an `agent.communication.inbox` unit whose
  `label` was not among the record's own `labels`. The per-section units now come only from
  the records the classification reads sections on: a user record that is neither an
  elicitation marker nor a compaction summary.
- **A refusal typed over several lines prints its hit on one line.** The `[feedback: ...]`
  tag kept the newlines inside the first 60 characters of the typed text, so the rest of
  the hit continued on new lines. The tag now flattens the text before it cuts it, and JSON
  `user_feedback` still carries the words whole.
- **A resumed workflow run's live lanes no longer read as killed.** A run resumed under its
  own id keeps running under the manifest its killed predecessor wrote until it ends, and
  that manifest settled every lane with no closing journal line: `agents` read them
  `killed`, `status` counted none live. A manifest now settles a lane only when the lane's
  newest record is no more than a minute newer than the manifest's own end instant (the
  kill's own writes land within 66 ms of it; a resumed run's, hours later); a newer lane is
  read from its tail and its row names the stale manifest, saying the lane wrote after its
  run's end and naming a resume as the likely cause.
- **`show --help` said a compaction boundary fetched without its summary reports no mode.**
  The pairing is read off the whole file before any address narrows the output, so a
  boundary fetched alone reports its mode; the help now says so, and that a boundary reads
  null only when the file itself leaves it unpaired.
- **`status` printed no `last ◂` when the newest prompt was a slash command with arguments.**
  The row took the newest turn-opening record, and a slash command typed with arguments is
  the person's words without opening a turn. It now takes the newest record labelled
  `user.message`, `user.answer` or `user.rejection`, turn-opening or not, in the text
  `search` renders (`/name args` for the slash command, a relayed or queued message's own
  body); a `<task-notification>` pulse is not the person's words and no longer takes it.
- **A fork child's first carrier emitted an inbox unit that named no call.** A subagent
  return carrier holding text beside its result emits two `agent.communication.inbox` units,
  and the text one read `pairing` null, no tool name and no `result_of` edge while the
  result one read the call. The text unit now carries the carrier's call, so both report
  one pairing, one tool name and one edge (`orphan` and `not in this file` when the call is
  on no line of the file).
- **A spawn prompt that names a fork record is still the lane's seed.** `search` took a
  lane for a `/fork` clone when the literal `fork-context-ref` occurred anywhere in the first
  4096 bytes of its line 1, and a subagent's line 1 is its spawn prompt, so a prompt about
  forks read as a clone: its seed classified `user.message` instead of
  `agent.communication.inbox`, lost its `parent ⇨ self` direction and joined every spanning
  count of the person's words. Line 1's type decides now, read as the quoted value
  `"fork-context-ref"` the candidate gate already uses, which a quote-escaping JSON string
  cannot carry.

### Performance

- **The render was the other serial half of a search.** Everything the scan does in parallel
  was spent before the first output byte: on a 1.18 GB session (the top-level transcript
  plus every `*.jsonl` under its subagent tree except `journal.jsonl`, which is what csift
  parses) a whole-scope `--format json` dump cost 1.94 s of CPU on ONE thread while nine
  rayon workers waited, and a wide-match text search (`search 'the'`) paid 0.45 s of that
  phase's 0.59 s inside `println!`, which is one `write(2)` and one backward newline scan
  per row - 297,976 of them. Two changes. Every row-dumping surface (`search` text, JSON and
  `--raw`, both of `show`'s, `files`, `agents`, `image`, and both of `verbatim`'s - its
  documented `--subagents` span is 125,269 text rows on that session) now writes through one
  buffered stdout, one syscall per megabyte. And `search` renders its per-exchange rows in
  bounded parallel chunks, writing the chunk buffers in index order, so the byte stream is
  the one a serial loop produced. Medians of 7 interleaved reps on that session, at a host
  load of 7 to 27 that the wall numbers carry and the CPU numbers do not: the JSON dump
  2.958 s -> 1.532 s, `search 'x'` 1.247 -> 1.040, `search 'the'` 1.140 -> 0.931 (at its own
  `-c` floor now), a `--for` closure 1.080 -> 0.901, `show --branch-points` 0.397 -> 0.355,
  the `agents` tree 0.664 -> 0.619, `files --by timeline` 0.890 -> 0.858. CPU for the two
  wide text rows falls about 10% (their sys halves 1.10 -> 0.70 and 1.07 -> 0.68); the JSON
  dump's rises 3.9%, which is what the fan-out costs to spread over ten cores. What the
  look-ahead costs in memory is UP TO TWICE 128 RENDERED ROWS - 128 rows are rendered before
  any is written, and each chunk buffer grows by doubling, so it can sit at twice the bytes
  it holds. On that session, rows averaging 78 KB, that is 10 to 20 MB against a peak of
  about 1,570 MiB and it does not clear the noise: medians of 13 interleaved reps differ by
  0.1 MiB, against per-arm spreads of 12 and 17 MiB. On a synthetic transcript whose
  rendered rows are 4,195,818 B, three runs of five interleaved pairs read medians of 715.8,
  844.2 and 723.7 MiB, the widest single observation 929.3 and the narrowest 522.3, against
  a one-copy figure of 512.2 MiB - so the peak is not a floor on the bound, one run is not a
  property of the pipeline, and a reader sizing a run works from twice 128 rows. The
  smallest inputs stay inside the same-binary control band, at a median ratio of 1.009
  against the control's 0.9997. Output bytes do not move: of the acid set's 169 (row, size
  class) pairs over three size classes, 165 are compared on all four streams - stdout,
  stderr, the exit code, and an interleaved `2>&1` capture, which is the only one that can
  see the ORDER between the first two - and 163 are byte-identical on every one of them,
  with stderr and the exit code identical on all 165. The two that are not are one binding
  in its two size classes, `csift --help`, whose page grew by the 512 bytes of the clause
  the exit law gained this release - a documented surface change, and the only stdout
  difference anywhere in the set. FOUR MORE are excluded because they print LIVE state, and
  both bindings are named: the `whoami --format json` rows, whose lane instants move when
  the session they name gains a record, and the `status` rows, whose fourth stdout line
  carries an elapsed-seconds field that advances by the gap between the two captures, so a
  repeat of the SAME binary differs from itself by exactly however long it waited. All four
  are run anyway, under the condition that makes their comparison mean anything: the two
  arms of a pair land inside one second. Whether a given pair then matches is a property of
  when it ran, which is exactly why it is not counted among the 165. What a render change
  can never reach is stated rather than claimed away: a heavy regex row is 1.189 s against a
  1.140 s floor with the render removed entirely, because its output is 2.5 MB and its clock
  is the regex over that 1.18 GB.
- **A record's width is a number now, and it is written down.** The amendment gave `Record`
  a second conversion key and a memo holding both parsed conversions, and the memo held its
  payload INLINE - 56 bytes on a field almost no record carries, paid by every parsed line
  of every scan. It is behind a pointer now (16 bytes whatever the pair grows to, and no
  allocation at all for a record carrying neither key), so `size_of::<Record>()` reads 1,368
  where the inline form measured 1,408 and the pre-amendment record measured 1,336: the
  amendment costs 32 bytes instead of 72. A unit test pins that exact number beside the
  spine row's 72, because widening the record is worth doing deliberately and nothing else
  in the crate notices when it happens.
- **An attachment payload is parsed once per record.** Four layers ask one attachment
  record about its payload, and each ask re-parsed bytes that can hold a whole file
  snapshot.
- **The nine provenance relations cost nothing a same-binary band can see.** Measured on an
  idle host, the tip that carries them against the tip they were built from, over 31 queries
  in three transcript sizes at 21 repetitions per cell: 90 of 93 rows sit inside the control
  band, the three outside are slower by 0.4, 27 and 51 milliseconds, the whole-scope JSON
  dump of the 397 MiB transcript reads 1.0009, and the `for-kind` census reads 1.0034 while
  printing 85 percent more. SPEC section 7i carries the rows.
- **`whoami` reads its own session's lanes once.** The topology section read the calling
  session's child lanes for its own line, and the count of other live lanes then read every
  one of them again for the same session. The census now takes the rows already read for
  the registry row whose transcript is that file. On a session holding 1,401 subagent
  transcripts, `whoami` went from 1.339 s to 1.069 s (hyperfine, 5 runs after a warm-up),
  and its text and JSON are byte-identical to before, on a two-session fixture and on the
  live root.

### Removed

- **`harness.meta` and every `harness.meta.*` selector.** They are hard errors naming their
  successors, one error per retired spelling, and there is no alias, no forwarding arm and no
  twin in `labels[]`. The same goes for `harness.schedule.continuation`, whose producer is the
  resume loader (`harness.resume.prompt`), and for the 0.6-era flat `thinking`, `tool` and
  `tool-response`, which were already errors and now point at the same three successors.
- **`--additional-context`.** The flag gated one leaf and that leaf now has a name:
  `-t harness.hook.context`. Naming any attachment-carried leaf is a gate of its own, so the
  narrow ask needs no flag; `--attachments` keeps its wide meaning. The flag is a hidden
  tombstone that bails with the successor, deliberately rather than becoming a quiet no-op: a
  run that silently scanned fewer lines than the caller asked for would read as the definitive
  absence an empty result promises.

- **The introspection ledger moves to schema 2: version-ranged states, one receipt each,
  and the prose law as a gate.** `INTROSPECTION.json` had grown a `checks[]` history - one
  appended record per release audit - and the history became the file: 1,856 checks behind
  739 claims, 1,117 of them superseded by a later check on the same claim, every one
  carrying the calendar dates, csift versions and audit-process words the ledger's own prose
  law bars. A fact record does not need its own minutes. A claim now carries its `code[]`
  sites at claim level, because csift's tree is one tree at any time and where a fact is
  relied on is never a property of a Claude Code version range, plus one or two `states[]`
  with exactly one of them open. A state is a version range (`from`, and `to` = null while
  it is open) holding the claim text for that range, the two attribution legs, the
  provenance and ONE `verified` receipt; `located` and `first_seen_claude_code` retire into
  the first state's `from`, and the per-check dates and csift versions retire outright. Each
  state also gains a `from_basis` - one word saying whether its floor was measured,
  bounded or defaulted - and a `probe`, the machine-readable twin of `instrument` naming
  the half a stranger re-runs first as one of bytes, corpus, csift, live or repo. A
  RECEIPT is the last check at the badge's build whose instrument names an artefact
  OUTSIDE csift's own tree: a build, a byte offset in one, a corpus, a registry directory,
  a live trial. The artefact is the test and never the command, so `grep` over `src` is the
  same text pass as reading the file while `grep -ab` over a shipped build is a
  measurement, and a text pass can never back the badge. The gate is rewritten to read all
  of it and grows from twelve rules to twenty-two, each failing by name on a hand-built
  specimen: rule 13 is the state shape, including that the open state is LAST; rule 14 is
  the prose law over every field a pass wrote, now word-bounded, first-person-aware and
  with the backtick exemption bounded; rule 10 reaches `residue` and a receipt's `note`,
  which is where every unconsumed text instruction was hiding; rules 15 to 20 are the
  state's own text and probe, a closed state's receipt, a retired selector handed to csift
  as an argument - the bare `tool` among them, with the valid `--count-by tool` census axis
  spared by position rather than by word - the file's own shape, every heredoc handed back
  to its own language to parse, and the receipt's artefact itself; rule 21 is the `bytes`
  probe's own literal, read in the builds its state names and in a CLOSED state's too
  wherever this host holds them, because four of them counted zero in every one and a
  probe that counts nothing cannot be run. Rule 22 holds a probe's KIND and its SCOPE to the
  same artefact: the kind follows what the command reads and the scope is derived from the
  instrument, and nothing compared the two, so thirty-eight states shipped claiming `corpus`
  beside a scope naming the shipped binary and one claimed `bytes` beside a scope naming a
  projects root. Only the two kinds whose artefact is fixed are tested, since `csift`, `live`
  and `repo` name a run, a gesture and this tree and any of the three may mention either noun
  without contradiction; its corpus word list learns the harness session REGISTRY under its
  English names, a row set on disk having been reachable only as a path before. It stops at the
  scope and never reads the command, a scope being a noun phrase a word list decides while a
  program classified by the words in it has the same false-negative shape the pair had. Rule 12
  learns that a hop can name the BUILD it was read in - and that a build nobody holds is not a
  pass, so such a hop is named rather than counted wherever the verified build itself is here -
  refuses a run of four or more dots the way it refuses an empty excerpt, holds an ELIDED
  excerpt to the run before its first ellipsis at its own offset - that bucket used to be
  compared against nothing, so replacing an already-elided excerpt with fabricated text moved no
  figure - refuses an excerpt that reads as prose whether or not the hop wears a MARK, since
  English occurs in no build and satisfies `absent` by construction, decides a marked hop by its
  mark BEFORE the eliding so it lands in one of the five counts and never in two, and compares
  bytes rather than a UTF-8 round trip. How many `...`-bearing excerpts are byte-exact anyway is
  derived by the walk and carried on the README rather than typed anywhere. Rule 8 stops
  matching rows with a regex and REGENERATES the README block, comparing it byte for byte. A
  folded check is not simply dropped: every fact token with no survivor in the state is carried
  into `residue`, and each carried entry names what superseded it. A carried entry the SOURCE
  had already cut short is repaired where the same state spells the whole word, trimmed back to
  the last complete item where it does not, and completed from the claim's own schema-1 text
  where that text says what came next; a cut landing on a word BOUNDARY, where every token is
  spelled correctly and the sentence stops anyway, is found by its LENGTH - the four truncation
  constants the source itself writes at. The prose rewrite masks every quoted run, literal,
  offset, version and whitespace run before it fires and restores each byte for byte, so it can
  neither cut a comma out of a jq argument list, nor normalise the double space out of an
  asctime literal, nor flatten the indent a python heredoc's block structure lives in; and a
  date that is the object of an epoch stays.

## [0.12.3] - 2026-09-13

### Added

- **`body`, a newline-preserving field on every `search` hit and `show` record
  (`--format json`).** A hit's `excerpt` is a match-centered fragment and it is one line by
  contract, so it collapses every newline to a space - a markdown table arrived as one run
  of `|` characters and a reader who took `excerpt` for the message lost every paragraph
  boundary, even under `--no-truncate` where nothing was clipped. `body` carries the same
  rendered section text before that collapse. It is non-null exactly where the excerpt cap
  is lifted: `search --no-truncate`, and `show`'s `--line`/`--uuid` address. Under the
  default cap it is null - the excerpt there is a fragment anyway, and a second full copy
  of every matched record would multiply a wide scan's output for a consumer that did not
  ask for it; `show --turn` is a window rather than an address, so it keeps the cap and
  reports null too. `excerpt` and `show`'s `text` keep their exact bytes in every mode.


- **`verbatim` folds a same-prefix re-send.** When one turn carries two assistant messages and
  the LATER one's body contains the earlier one's whole body - a re-send with an addendum - the
  text render prints the earlier as `△ L<n>  [superseded by the same-prefix re-send at L<m>, N
  chars]` instead of a second copy of the prose, and the JSON row keeps the full text with
  `superseded_by_line` set, so a machine reader loses nothing. CONTAINMENT is the predicate, and
  that is measured, not stylistic: over every top-level transcript of one corpus (80 files,
  6,468 turns, 35,052 assistant messages) 53 same-turn pairs shared their first 80 normalized
  chars, but only 1 was a true duplicate, 25 diverged before char 120, and on 22 the later
  member was SHORTER - so a fold keyed on the 80-char prefix alone would have dropped prose the
  survivor never said. Under containment the same corpus yields 2 folds. The name follows the
  same discipline (claim MSG-095): the shape that motivated the rule is a Stop hook exiting 2,
  but none of that corpus's 36 `Stop hook feedback:` records produced a same-prefix pair (their
  neighbouring assistant messages share 0 to 4 normalized chars; 0 of the 30 same-turn pairs
  reach 20), and no same-prefix pair has one between its members - so the marker names what it
  matched, not a producer it cannot attribute.
- **`verbatim`'s fold marker says what it folded.** A collapsed run of agent messages
  rendered `[X agent message(s), Y tool call(s)]` - counts, with no way to tell a folded
  finding from a folded "let me look into this" without fetching the range. The marker is now
  `[X agent message(s) collapsed, N chars, Y tool call(s)]` (N = the summed `full_chars` of the
  collapsed bodies, so the line says how much prose it stands for), and every collapsed message
  of at least 210 chars is followed by an indented `L<n>  <first 60 chars>… (+K chars)` preview
  line - above the pure-declaration band, so a preview means "this was not a declaration". JSON
  `collapsed_agents` rows gain `collapsed_chars` and `collapsed_previews[]`. The budget charges
  exactly the lines emitted: the renderer and the cost model both walk one
  `agent_placeholder_lines` list. Cost on the installed hook's path (`--slices 4 --window
  9000`), measured on the two largest top-level transcripts of one corpus: 2 and 3 fold markers
  in the whole four-slice document, 0 preview lines (every folded body there is under the
  threshold), and +41 / +61 chars from the `N chars` clause on a ~24,700-char document. Over a
  WHOLE session at an unbounded budget, 42 of 1,224 and 212 of 1,759 folded messages reach the
  threshold.

- **`list` names the child a background handoff continued the session in.** A session handed
  to a background child - the left-arrow gesture, or the background-fork command - leaves one
  line in the PARENT transcript: `{type:"continued-in", timestamp, sessionId,
  continuedInSessionId}`, four keys and no `uuid`, so it is not a chain node and its only
  address is its jsonl line. This is the third lineage link on a `list` row and the only one
  that is not an inference: `clone_of` reads a boundary shape and `cleared_from` a checkpoint
  sum, while here Claude Code writes the child id itself. Text gains a `handoff` row naming the
  child's first-8 token; JSON gains `continued_in`. Two limits ride the docs. An in-place
  `/fork` reaches the same spawner with `keepParent` and writes no such line, so absence never
  means "not forked"; and the append also runs on the failed-spawn path once a rescue row is
  queued, so presence does not prove the child ran. Because `list` reads head/tail windows
  only, a line a later `--resume` of the parent pushed above the tail window reads null, and
  `stats`' line-type census (which already counts `continued-in`) is the whole-file answer.
  (claim REC-106)

- **`list` reports the background-lane stamp, and `--lineage` gives its span.** Every record
  a background lane writes carries a top-level `sessionKind:"bg"`; a foreground lane's records
  carry no such key at all, because one stamp builds the value from one environment variable
  whose value is undefined there and the serializer drops an undefined property. That makes it
  a per-PROCESS fact, so `session_kind` is an ARRAY and is EMPTY for an ordinary session rather
  than carrying a sentinel - and one transcript can hold a background lane's records above the
  records a foreground resume of that same session appended without the key. The values come
  from the head/tail windows, which is where Claude Code's own session summary reads them too
  (off the first record of the file). The SPAN cannot: the stamp stops mid-file and no window
  can see where. So the new `--lineage` flag runs one whole-file pass for
  `session_kind_first_line` / `session_kind_last_line` and an exact `continued_in`, with
  `lineage_scanned` separating "not asked for" from "no carrier". That pass is a depth-1 key
  walk, never a record parse, so a nested key is not a field and it adds nothing to
  `skipped_lines`. Measured on a 720 MB transcript it costs 2.1x the same-binary control band,
  which is why it is opt-in; the default path stays inside that band. (claim REC-107)

- **`plan --audit` qualifies each binding with four facts its own records carry.** The audit
  said which plan file a session binds; these say how much that binding is worth, each as a
  text block and a JSON field, and each reading a fact on disk rather than deriving one.
  (a) The slug's change points: `none -> <value>` is the mint, `<a> -> <b>` reads as the
  binding having MOVED (`slug_changes`, `first_slug_line`, `first_slug_utc`,
  `first_slug_local`). The binding csift reports is the harness's; this says when it became
  so, which is the question a fork raises. Measured over 9,601 transcripts, the 7,810 that
  carry a slug each have exactly one distinct value and exactly one absent-to-value
  transition, so two change points is a shape the corpus has never shown. (b) Whether the
  bound file is on disk: `[exists]`/`[missing]` and `plan_exists`, the same field the forward
  view prints. (c) A `plan_file_reference` attachment present while no record carries a slug -
  plan text with nothing bound to it, which nothing will re-inject: a `plan-unbound-text` row,
  a warning, and a count in `unbound_plan_text`. Zero corpus specimens; kept because the fork
  path that strips a slug can produce it. (d) The first slug-carrying record against the plan
  file's birth instant: `before`, `after`, `same`, or `unknown` with the reason named rather
  than a guessed direction. (claim PLAN-022)

### Changed

- **`verbatim`'s docs say which path the 600/900 per-role caps apply to** (documentation only;
  no cap moved). The caps are the DIRECT path's - a plain run, `--out`, and the legacy
  budget-driven `--slice i` alone. `--slices N`, the fixed-fleet hook-injection path, drops them
  for a window cap and holds its slice count by discarding the oldest turns that do not fit, so
  the same session read two ways differs on purpose: the direct path keeps more turns each cut
  to 600/900, the fleet path keeps fewer turns whole. Only the `--slices` flag doc had said so.
  The measured cost of the direct cap, over the last 40 turns of the two largest top-level
  transcripts of one corpus (assistant units): the 900-char cap keeps 32,404 of 59,965 chars (27
  of 42 units cut) and 67,846 of 117,878 (48 of 101 cut); at 1,800 those would be 50,657 (12
  cut) and 95,262 (18 cut), i.e. 18,253 and 27,416 more chars kept. Recorded as a number, not
  acted on. The `--agent-msgs` help text also drops a stale claim that the default is
  `eot-only`; it has been `longest` since that mode shipped.

### Fixed

- **`verbatim`'s `L lines elided` note counts the cut, not the message.** The figure was the
  whole body's newline count, so deleting 9,807 chars and deleting 1,907 chars of one message
  printed the same number, and a cut of 17 chars could claim 95 elided lines - 18 of the 2,480
  truncated units on two large sessions reported more lines than they removed characters, which
  is arithmetically impossible. A unit now carries its original body's newlines as offsets into
  the one-line text it renders, and the note counts the ones STRICTLY INSIDE the removed span:
  an uncut body reports 0, a cut that falls between two newlines reports 0, and a wider cut of
  the same message reports more. Text `, N lines elided` and JSON `elided_lines` follow. A
  FABRICATED body - an AskUserQuestion scaffold, an automation attribution label, a peer-message
  preview - reports no lines at all, because no original newline can be located in a string the
  record does not carry; the old figure was measured against a different string.


- **The generation-3 blanking mask decides a backslash the way the harness does.** The
  rewritten dangerous-removal classifier masks its input twice before its clause walk
  starts, once blanking every quoted run and comment and once keeping them, and those two
  answers decide whether a positional parameter (`$1`, `$@`, `$*`, `$!`) counts as a removal
  target. csift's mask read every backslash outside a single quote as an escape; the harness
  reads an in-quote one with the same helper its quote shield uses and blanks the escape as a
  pair, so a `\"` does not end the run that hides it. The old reading flipped the quote state
  for the whole rest of the command, which made a function definition written inside a quoted
  string count as a real one and hid a definition that genuinely followed the string. No
  corpus lane changes verdict: over 165,314 distinct (Bash command, version) pairs the two
  readings differ on 4,097 masked strings and 165 function-scan answers, but the generations
  before the rewrite never call a mask and the three differing commands recorded at or past
  the rewrite floor carry no `$`, which the classifier's entry guard wants first.

## [0.12.2] - 2026-09-12

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
  keyword false positive). No other command's output changes. The newest generation's walk
  also recurses into a nested `sh -c` script, and it reads that script REBUILT rather than
  sliced out of the clause: the quote shield has already replaced the script's own spaces, so
  a slice arrives at the walk as one word and the removal verb is never reached. The script is
  reassembled from its quoted and unquoted runs and unmasked first, an escaped `$` inside a
  double-quoted run is kept out of the target test unless it names a variable, and whether the
  positional form applies is derived from what follows the script or from an `xargs` ahead of
  it. A too-complex head carrying a nested removal - `if true; then sh -c 'rm -rf $D/*'; fi` -
  now reads as the ask the harness makes.
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
