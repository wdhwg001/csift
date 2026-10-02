# Changelog

This file records the notable changes to csift, newest version first. Each version
gets one entry, written in the commit that releases it. csift is pre-1.0 and follows
SemVer in its pre-1.0 form. A breaking surface change bumps the MINOR version, and a
non-breaking surface change bumps the PATCH.

## [0.13.0] - 2026-09-26

### Added

- A PDF page now says which page it is. A `Read` with a `pages` parameter returns one
  image element per requested page, and `csift image` has always listed and extracted
  those pages. Until now each one looked exactly like a screenshot. No key on the element
  carries its page number. The harness's own mapper writes a successful page bare and
  labels only a page it failed to render, and the offset lives once on the carrying record
  as `toolUseResult.firstPage`. csift now derives the page as that offset plus the
  element's position among the content elements after the leading text. It counts
  positions and not images, so a failed page in the middle does not shift the pages after
  it. Text rows gain ` p<N>` beside the media type, with the document's basename once per
  record group (one transcript, one line, one document). JSON rows gain `page` and
  `pdf_path`, the path printed verbatim like every other path field. The derivation runs
  only when the record's own result reads `parts` with a `firstPage` and the record
  carries exactly one `tool_result` block. A screenshot, a result written before that key
  existed, and the REPL tool's re-emitted inner-`Read` pages all read a null `page` in
  place of an assumed page 1. Those REPL pages carry no offset and flatten several
  documents into one list. Addressing does not move. `--id`, `--turn` and the extracted
  filename stay as they were, because the page is provenance and not an id. A whole-PDF
  `Read` returns a `document` block with no images and stays unlisted. The same fact
  reaches the `agent.tool.result` unit as the exposed-keys family's `pdf_pages`, with the
  label zone `[pdf pages A-B of <basename>]` and the JSON object `{first, last, count,
  path}`. Both surfaces read it through the one function `image` uses, so `show --line`
  and `image` cannot disagree about which pages a record carries. The existing
  `[N images: ...]` annotation and its extraction hint are unchanged.

- One session id found under two project directories is now merged, and the merge says
  what it dropped. A renamed project whose directory was copied leaves one session id
  under two encoded directories. Claude Code reaches the same shape two ways of its own.
  The `CLAUDE_CODE_PROJECT_DIR_NAME` override keeps the natural directory beside the
  overridden one and searches both, and a relocation whose fallback mover strands its
  source leaves the transcript in the old place too. Claude Code's own resume lookup walks
  every project directory and returns nothing when it would have to choose. csift used to
  resolve both copies, count the session twice on every spanning surface, and walk its
  whole sidecar twice. The copies now collapse at the shared resolver, and the bytes
  decide. Identical copies merge onto the longest, and so does a copy that is a strict
  byte prefix of the other. Copies that differ before the shorter one ends merge into
  nothing and both stay in scope, because a turn number minted from one does not address
  the other. A copy csift could not read reads `unknown` and merges nothing either, and
  the note names that copy and the error the filesystem returned. An mtime is never
  consulted. The resolver names every merge on stderr. `list`, `search`, `stats`,
  `files`, `recover`, `verbatim`, `image` and `agents` carry `duplicate_transcripts` and
  `duplicate_records_dropped` in their JSON header. They also carry
  `divergent_transcripts`, the copies csift refused to merge, and `unreadable_transcripts`,
  the copies it could not read, because the first two keys read 0 both when nothing was
  duplicated and when a duplicate merged nothing. Text output gains a `copies` line, and a
  `list` row carries `also_at` plus `duplicate_relation`. `show` keeps refusing an address
  that resolves to two files and now names the paths. An `@<agent-id>` target takes the
  same merge, because a copied project directory carries the whole sidecar and so puts one
  `agent-<hex>.jsonl` under two directories too.
- The merge runs once over the whole resolved set, so one transcript is one file however
  many targets reached it. `list @<uuid> @<agent-hex>` reaches one `agent-<hex>.jsonl`
  down two paths. The session's sidecar yields it once, and the agent target finds it
  under both project directories. Merging each half separately let the two halves pick
  different winners, so both copies survived and each row named the other as dropped. A
  first pass still runs before the sidecars are gathered, so a dropped copy's sidecar is
  never walked. Everything that pass produced then goes through one final merge, which
  reports only what it newly decided. A group that merged nothing is still there for the
  final pass to find, and one group gets one note. The agent lookup now collects every
  copy, where it used to stop at the first one the directory listing yielded. It finds
  them by probing each project directory for the owning session's basename, with no
  second walk over every sidecar.
- One workflow run is one `kind:"run"` row, however many copies of its session found it.
  A run's identity is the pair of session and run id, and the id alone no longer stands
  for it. A copied project directory carries the workflow manifests with it, so the same
  run used to be read and printed once per copy. A run id is unique inside the session
  that minted it, and nothing makes it unique across sessions. The fold and the session a
  run renders under now both key on the owning session. Two sessions naming one workflow
  id keep a row each, where keying on the id alone dropped the second as though it were a
  copy.
- `csift agents` merges duplicated agent transcripts like every other surface. It
  resolves top-level sessions only and walks the sidecars itself, so the resolver's merge
  never saw those transcripts. A divergent session pair whose two sidecars held one
  byte-identical agent transcript printed two lanes under a header saying nothing was
  duplicated. In the same scope `list` showed one row and `search -c` counted the records
  once. All three agree now.
- Every `csift agents` row names its own transcript in `path`, and the text tree prints a
  `transcript` line for the ids that occur twice. Two copies of one session carry two
  copies of each agent transcript. Those rows are identical in every other field,
  including the agent id, so without the path a consumer sees two lanes it cannot tell
  apart.
- `csift agents` counts what it prints. The session count row's `agents` is now exactly
  the number of `kind:"agent"` rows under it, and the text tree prints that same set in
  the same order. Both projections take one walk, and its visited set keys on node
  identity. The agent id alone is not enough. Two copies of one session whose contents
  differ keep both sidecars in scope, and their agent transcripts share an id. The old
  walk deduped those while the count row did not.
- `csift list` emits one row per session instead of one per copy, and `sessions_in_scope`
  counts distinct sessions. This is BREAKING on a scope holding a duplicated session. The
  row count changes, every spanning count changes, and the header numbers change with
  them.

- Every emitted unit now carries `for` provenance. A `for` edge used to exist only where
  a record carried a key naming a target. An assistant reply, a thinking block, a tool
  call and its result, which make up most of a transcript, had no provenance at all.
  `for_root` now names the record that opened a unit's turn plus the number of parent
  hops up to it. It renders as `[root L12 +3]`, and nothing renders on the opener itself.
  Its JSON is always `{line, uuid, hops, note}`, because four different answers need four
  different shapes. The unit can be the opener, or the chain can have walked there in N
  hops. The grouping can have placed it where the links did not carry it, or there can be
  no opener above it at all. A missing key could not separate those. Two engine relations
  join the vocabulary and ride JSON alone. `parent` is the `parentUuid` link, and it
  stands down when any other edge on the same unit already names the same target. The test
  is on the target and never on who named it. `result_of` is a tool result's
  `tool_use_id`.
- The relation vocabulary is closed as `EdgeKind`, with nineteen words. Every family used
  to write its word as a bare string at the call site, so the set was whatever six files
  happened to contain. Nobody could enumerate it or census it. No relation was renamed, and
  every word is the one its family already rendered. The cross-family pin over the whole
  edge set landed first. Rendered bytes did move elsewhere in this version. An edge naming
  a call now resolves through the tool index and prints the call's line, where it used to
  print the wire id with `not in this file`.
- The two user-facing duals now carry `tool_use_id`. An answered AskUserQuestion and a
  refused tool call are the person's message and a tool result at once. Their hit now
  carries the API's id for the call it answers, where that key read null. The `result_of`
  edge resolves through it. `pairing`, `tool_name` and `is_error` stay null on both,
  because those keys belong to the tool view and the person's view is the richer one.
- `--count-by for-kind` is the census over that vocabulary. It answers which provenance
  relations a scope carries. It is multi-key like `label`, counts each relation once per
  record, and counts a relation whether or not csift resolved its target.
- `--for <ADDR>` is the reverse query. It returns everything that is for one record,
  transitively. The address is `L<line>` or a bare number, both read per file, or a record
  uuid, which finds its record in whichever scanned transcript carries it. The closure
  itself is computed per transcript. One query returns the reply a message drew, the
  results of the calls that reply made, and the `<task-notification>` that reports on one
  of those calls two turns later. The parent arm stops at a turn boundary, so the person's
  next question does not come back with it. Three structural relations admit a row. They
  are the parent link, a result answering a member's call, and a record naming a member's
  call by id. The rest of the vocabulary is not walked in reverse. The summary gains `for_of`,
  carrying `address`, `records` and `transcripts`.
- Every non-transcript line type now says what it holds. When the head-and-body law
  landed, twelve of the thirteen `harness.bookkeeping` leaves rendered the line alone, so
  a reader who named one still read raw JSON. Sixteen of the eighteen line types now build
  a `[<line type> key=value ...]` head from their own bytes, in the key order Claude
  Code's own writer uses. The head opens with the line's own `type`. That is how
  `mode`/`permission-mode`, `ai-title`/`agent-name` and the three artifact ledgers stay
  apart in the output when their label does not separate them. The two `file-history`
  forms keep the head built from the fields csift models. That head names the tracked path
  and its version, where a raw key walk could only print the snapshot object. Two rules
  decide a head's values, and there is no third. An account, organisation or record uuid
  prints its first 8 characters. Every other value is cut at 80 characters with the shared
  `... (+N chars)` marker, and an object prints as its sorted key list. The body is still
  the whole line, and that keeps a key the head leaves out matchable. A fabricated head is not part of
  the line, so each line type's own quoted value is registered as a synthesized-text
  marker under the same predicate that admits the line. Without that registration, four
  of five head-only patterns on a 21-line fixture read 0 instead of 1, and a default scan
  arms none of them.
- Five bookkeeping line types now say what they are for. Four ride the shared provenance
  edge. `last-prompt` points to `leaf` (its `leafUuid`), and a file-history line points to
  `versions` (its `messageId`). `fork-context-ref` points to `forked_from`, the record the
  forked agent's prefix was cut at, with the parent session beside it because that record
  lives in another file. `continued-in` points to `continued_in`, which names a whole
  transcript and no record in it. That is the edge builder's session-only answer, and it
  renders as `[for: continued_in @<session>]`, an `@` target you can feed straight back to
  csift. The fifth line type names an instant. No target shape can carry an instant
  without putting a timestamp in a field documented as the target record's uuid.
  `cost-state` therefore stays the family's own fact. It renders as `[checkpoint closes <instant>]`
  with the JSON keys `checkpoint_closes_at_utc`, `checkpoint_closes_at_local` and
  `checkpoint_join`. The instant is the `startTime + totalDuration` sum that `list` joins a
  cleared session on as `cleared_from`. Each of the five renders its honest empty when the
  key it reads is absent, and the field never drops out. A `fork-context-ref` naming a
  parent session but no cut record renders that session for the same reason.
- Every bookkeeping unit now carries `[not delivered]`. JSON `delivered` has always read
  false for all thirteen leaves, while the text surface said nothing. The engine prints
  that marker only for a record carrying a per-record delivery override, and a bookkeeping
  line is invisible because of its leaf. The marker is the engine's own, so both surfaces
  spell it one way. `show --format json` gains the record-row keys `for`,
  `checkpoint_closes_at_utc`, `checkpoint_closes_at_local` and `checkpoint_join` with it.
  The handover relation adds no key of its own. It rides the shared `for` array as the
  edge `continued_in`, whose target names the child session and no record in it. A
  `continued-in` line's row therefore carries that edge and no `continued_in` key.

- A thinking block now says what it is and which side wrote it, through four label-zone
  markers. `[no summary text]` marks `agent.thinking.omitted` and `[redacted thinking]`
  marks `agent.thinking.redacted`. Those two are the API's own textless answers.
  `[cancelled stream salvage]` marks the one thinking shape the client mints, which keeps
  `agent.thinking.summary` and stays delivered. `[thinking removed by the client]` marks
  an `agent.message.plain` block whose whole trimmed text is `[Thinking removed]`, so those
  three words never read as the assistant speaking.
- Every thinking block unit carries two new per-hit JSON keys. `signature_len` is the
  character length of the block's own `signature`. It reads `0` when the field is present
  and empty, and null when the block carries no such field. Absent and empty are different
  shapes, and the salvage discriminator rests on exactly that difference.
  `thinking_text_absent` rides `agent.thinking.omitted` alone. Both are named in
  `search --help` and `show --help`.

- Each `harness.system.*` event kind now renders the fields that make it useful. The old
  catch-all head named the subtype and threw the payload away. A refusal rendered its
  banner sentence and dropped both model ids, the category and the rewind target. An
  `api_error` rendered a head with no status, no retry counter and no word about what
  failed. A `refusal` now reads `[refusal <subtype>] originalModel=...
  fallbackModel=...|none apiRefusalCategory=...`. The `none` is printed and never elided,
  because the absence of a second model is what separates the two subtypes. With no source
  line to read, the head stops at the subtype and reports no key it never looked at. An
  `api-error` reads `[api-error status=... type=<kind> retry N/M in Xms]` plus the
  failure's own wording, each read where its own shape puts it. A connection failure keeps
  the wording in `error.message` and names no kind. An HTTP failure names the kind in
  `error.type` and buries the wording two hops down, inside the response body it kept. A
  `notice` reads `[notice <level>]`. A `kill` reads `[kill]`, because the record carries
  no payload on disk. A `command` reads `[command]`. The scheduler's
  `harness.schedule.event` reads `[scheduled <when>] task=... cron=... prompt=...`. Every
  one of those fields also rides JSON as its own key (`retried`, `original_model`,
  `fallback_model`, `category`, `status`, `retry_attempt`, `max_retries`, `retry_in_ms`,
  `error`, `task_id`, `cron`, and `prompt` cut at 200 characters). The head carries the
  same value, because a key that reaches JSON alone is unsearchable.
- A leaf that names another record now says which one. A refusal's
  `refusedUserMessageUuid` and a delivered command's `parentUuid` ride the hit as a `for`
  fact. It renders `[for: refused L42]` when this transcript holds the named record,
  `[for: wraps <id8> (not in this file)]` when it does not, and `[for: <kind> unknown
  (<reason>)]` when the record names none. An edge is therefore never silently absent. A
  fire record's `taskId` and a fallback turn's banner take that last form on purpose. The
  arming call is found by matching an id inside a tool result, and the banner descends from
  its record. A uuid map can make neither lookup. JSON `for` is an array with one object
  per edge, each carrying `kind`, `basis`, `target` and `reason`.
- A turn the API served from a fallback model now names both models. Its sole content
  block is the API's `fallback` block, written in place of the text, and nothing in the
  harness renders it. A reader had no way to learn that a different model served the turn.
  The unit now carries `[served by <to.model>, fell back from <from.model>]` in the label
  zone, plus JSON `served_by` and `fell_back_from`. That is the one place a transcript
  keeps the pre-fallback model. The ids are read off the parsed block, so a scanned unit
  and an addressed one carry the same fact. A record whose only block is that one still
  emits nothing on a scan, because it has no text to match or excerpt.
- `search --help` and `show --help` carry the system family's surface. A SYSTEM LEAVES
  region names every render form, marker word, `for` kind and per-hit key, and the `show`
  record-row table gains fifteen key rows.

- The exposed keys report what a record says about the call behind it. Claude Code stamps
  a record with the agent definition, skill, plugin and MCP tool the request ran under. It
  also stamps which API request and which content block of the response the record is, why
  the call failed, and what a person typed when refusing a tool. It stamps which scheduled
  task submitted the prompt and which hook run closed it. csift parsed none of it, so a
  reader who wanted any of it fell back to `--raw`. Every one now rides every `search` hit
  and every `show` record row as JSON: `attribution`, `request_id`, `api_block_index`,
  `tool_ends_turn`, `user_feedback`, `api_failure`, `interrupted_message_id`,
  `hook_run_id`, `scheduled_task_id` and `scheduled_fire_id`. Four of them also render a
  label-zone tag (`[via skill X]`, `[ends turn]`, `[feedback: ...]` and the api-failure
  headline). An address carries exactly what a scan does. A missing key means this record
  does not say, and it never means no. A key of an unexpected JSON type means the same
  thing and costs only its own key, never the record. All eighteen keys decode leniently,
  so a record carrying `"apiBlockIndex":"3"` still emits with its text searchable and is
  not booked malformed. The keys ride the record as one struct behind one pointer, read by
  the same serde pass that builds the record. The row every parsed line of every scan
  carries grows by 8 bytes, where eighteen inline fields would have cost 216.
  `size_of::<Record>()` moves from 1,368 to 1,376, and only a record that names a key pays
  for the allocation. Measured against the tip before it on 56 acid (row, size) pairs,
  every ratio sat inside a same-binary control band of 0.2467, and the compacted class
  read a mean of 1.0153.
- `csift list --lineage` reports the session a record was minted in when that differs
  from the session holding it. A record carries two session ids under two spellings, and
  they disagree exactly when the record was hydrated here from another session's stream.
  The row reports those ids as `minted_elsewhere` with a count beside them, under the text
  row `minted`. It is the fourth lineage link a row can carry and the only one a record
  states about itself. It therefore carries no inference wording.
- An attachment unit now says which record it is for. Every attachment unit carries two
  `for` edges and a producer fact. `attached_to` comes from the record's own `parentUuid`,
  resolved to a line of this transcript through the chain's uuid map. `for_tool_use` comes
  from the payload's inner `toolUseID`, a wire id no line carries, so the edge keeps the id
  and says it is not in this file. `[produced_by <command>]` comes from a hook run's own
  command, with the JSON object `produced_by:{command,hook_name,payload_type}`. The two
  edges fold into the shared JSON `for` array. An edge whose key the line does not carry
  renders its reason and stays visible. All three facts also ride an addressed record that
  csift models no leaf for.
- `search --help` and `show --help` name the attachment family's surface, so the two
  manuals stay at information parity with SKILL. An `ATTACHMENT LEAVES` region covers the
  two bodies, the markers, the provenance facts and the matching rule. The record-row
  table names `body_source`, `human_turn_render_differs`, `produced_by` and `for`.
- Eight new provenance relations arrive and one moves, all of them values csift already
  computed. The closed `for[]` vocabulary grows from nineteen words to twenty-seven. Eight
  words are new, and `reports` was already in the set and changed which records answer it
  (see the next bullet). The survival axis contributes four. `supersedes` names the
  message that replaced a recalled draft, and `continues` names the one the conversation
  continued from after a rewind. `branches` names the head of an abandoned branch, and
  `replays` names the surviving line a replay copy's uuid also names. A compaction event
  contributes three. `pairs` joins its boundary and its summary in both directions.
  `precedes` is the `logicalParentUuid` the boundary carries in place of the null parent
  every boundary has. `preserves` names the head and the tail of the segment the
  compaction kept, as two edges. A paired resume placeholder gains `repairs`, naming the
  dangling prompt it was spliced in to answer.

  No field moves, but one edge kind does. `parent` stands down when any other edge on the
  same unit already names the same target. The test is on the target and never on who
  named it, so the engine's own `pairs` and `repairs` stand it down exactly as a family's
  word does, since both name the record above. One relation gets one edge. On the landmark
  transcript that moves 83 units, `parent` from 169,180 to 169,097, against 80 compaction
  summaries and 3 paired resume placeholders. The `parentUuid` field is untouched on every
  one of them, and `for_root` still names the turn each belongs to. The counting rule is
  the `--count-by for-kind` census over the whole transcript with an empty pattern, one
  row per record per distinct relation word its edges name.

  Each edge is the machine-followable form of an answer the unit already rendered for a
  person, so a consumer can walk `for[]` and skip parsing a marker word. The four survival
  relations render no label-zone tag at all. Each restates `[abandoned]`, `[rewound]`,
  `[replay copy of L<n>]` or the draft-distance line, so they ride JSON alone under the
  same gate `parent` and `result_of` already take. A default text scan builds none of them
  and renders every byte unchanged.

- A notification pulse now names the call it reports on, whichever carrier it rides.
  `reports` was read in the isMeta injection family, whose gate is the authorship flag.
  It answered only for a pulse wrapped in a notification frame, which was 95 of the 4,017
  pulse-carrying records over one projects root. It is now read at the engine, off the
  record's own text, and the 3,922 ordinary carriers answer too. It resolves through the
  tool index and no longer prints a bare handle, because a `tool_use` id is a wire id and
  sits in no `uuid` field. Of the pulses, 2,470 carry the tag and 2,459 of those name a
  call of the same file, which gives 61% of pulses an address. A pulse that names no tag
  says so and keeps the edge. Joining those through their `<task-id>` would reach 1,228
  more, about 92%, and is a declared follow-on. The reader takes the same text the
  classifier takes, so a pulse whose content is a block array answers exactly as a string
  one does.

  The counting rule has two halves, the file set and the reading. One row is one record
  whose `message.content` carries `<task-notification>`, with the content read as the bare
  string or as the newline-join of the content array's `text` blocks. The file set is
  every `*.jsonl` under one projects root. It includes the top-level transcripts and the
  `<uuid>/subagents/**` sidecar ones, and it excludes `journal.jsonl`,
  `elicitations.jsonl` and anything under a `csift-channel/` directory. Both halves move
  the answer. The top-level transcripts alone miss the sidecars, and a string-only reading
  gives 3,805 rows instead of 4,017 and moves the frame count from 95 to 56. The corpus is
  live, and a re-run reads a few more rows. The rule is what reproduces.

- The relation inventory is now in SPEC 4.12 and in SKILL. It is one table over every
  relation csift can answer about a record. It lists the twenty-seven `for[]` words, the
  relations that ride a field on another surface instead, and the ones nothing on disk
  names. Two rules decide which is which, and both are now stated. A cross-file join is a
  field on the surface that owns it (`clone_of` / `cleared_from` / `continued_in` /
  `minted_elsewhere` on `list`, `spawn_tool_use_id` / `parent_agent_id` on `agents`, the
  verdict on `msg`), because an edge's target names a session without carrying the
  re-feedable id trio. A target that is not a record, such as a command, an instant, a
  lane or an external file, is a per-family fact with its own key. A drift test holds the
  table's `for[]` block to `EdgeKind` in both directions.

- All nine of the harness's turn continuations now reach `harness.enforce.retry`. When
  the model does not finish a turn, the harness continues it by writing a whole message
  into it as an `isMeta` user record. The producer interns nine such messages in one
  constant block, collected into one array. That array's single consumer is the
  producer's own is-this-a-record-I-wrote predicate, which tests by membership. csift knew
  four of them, the redo nudges. It carried no leaf for the other five, so they showed on
  no scan and had no census key, and an address rendered the residue unit. The five are
  the two classifier-stop forms, the output-token-limit resume and the two mid-stream
  cut-off forms. The classifier-stop forms are written when a safety classifier stops a
  response mid-turn and the turn continues once on the same model. The second is the first
  plus a sentence about a call that was already running. The two cut-off forms are chosen
  by lane. All nine are matched by whole-message equality, because they are complete
  bodies and the producer tests them that way. A longer body that opens with one is a
  record quoting it. Five carry an em dash, so a text retyped with an ASCII hyphen matches
  no record. The leaf's description widens with its domain, to the harness continuing a
  turn the model did not finish on its own instruction, and the shape inventory gains a
  row per event. The classifier stop writes three records, and only the continuation
  needed a rule. The notice beside it is decided by its subtype, and the refused assistant
  turn by its own blocks.
- An API refusal is now a fact on the assistant unit it stopped. `message.stop_details`
  is the API's own structured account of why it stopped a response. It holds a type, the
  classifier band, the API's own sentence about it, and whether a fallback claimed a
  prefill. The client builds none of it. It copies the object off the response frame onto
  every assistant record of the turn and removes exactly one member it does not persist.
  It constructs an object of its own only on refusal paths that null every remaining
  member, so a true prefill claim on disk came from the API. csift read none of it. A
  refusal whose only content block is a thinking block classified `agent.thinking.summary`,
  and nothing on any surface said the API had refused. The exposed-keys family now stamps
  the object on every emitted unit of a record that names at least one member. The label
  zone carries `[api <type>: <category>]`, and the wire carries `stop_details` beside the
  coarse `stop_reason`. The gate is the object and not the leaf, because a key is true of a
  record whatever csift labelled it. An object whose every member is null is refused,
  because the absent key already says what four nulls would. The tag is label-zone text and
  therefore display-only, like the thinking family's. A pattern scan matches a unit's
  rendered section and never its zone. Two members stay out of the model and are reachable
  with `--raw` alone. One never reaches disk, and the other reaches it only as a null.
  `size_of::<Record>()` moves from 1,376 to 1,384, with the object behind its own pointer
  inside `message`.
- Two attachment payload types get their inventory rows. Neither carried a leaf, and
  neither reached a scan even under `--attachments`. Neither could be a key on
  `--count-by attachment`, since that axis reads emitted units, and each rendered as the
  unmodeled unit under an address. `mcp_instructions_delta` is what the harness writes
  when the set of MCP servers whose instructions belong in the request changes. Its
  builder returns nothing when the diff is empty, so a record exists only where the set
  moved. The converter turns it into content, a `# MCP Server Instructions` section
  carrying the blocks added plus a second section naming servers that disconnected. It
  persists that conversion back as a top-level `rendered`. The type is therefore
  `harness.context.listing` with the model column true, and the body rule takes the
  persisted conversion. That is what makes injected server instructions findable by a
  pattern scan. `thinking_drop` is the harness's accounting of the thinking blocks it
  dropped from a request, with the model, the query source, the counts and the reason. Its
  converter arm is an explicit empty list, so it joins `NEVER_CONVERTS` and reads
  `not delivered` whatever its leaf default says. It takes `harness.context.task` beside
  `thinking_stripped` and falls to the verbatim payload, which is the only place its
  numbers are. The human column on all three rows is read off the transcript render's own
  hide set, never inferred from the leaf, and that list differs per build. The verified
  build's hide set names `mcp_instructions_delta` and neither of the two thinking
  payloads. It cannot name them, since neither name occurs anywhere in that build. The
  2.1.278 hide set names all three. The column on the two thinking rows is therefore a
  reading of the build that knows the types. That reading also corrects
  `thinking_stripped`, whose column said unstated while a hide set answered for it.
- A bundled skill body that ships no files now reaches `harness.context.skill`. The leaf
  keys on the `Base directory for this skill:` line. One wrapper in the registrar writes
  that line, and only for a registration that names files. The wrapper materialises the
  directory and prepends that line and a blank one to the first text block of the body the
  registration returns. A registration with no files keeps its body getter untouched, so
  the same kind of body reaches the same kind of record bare. It carried no leaf and
  rendered `? unmodeled: user/text` under an address, because its content is a text block
  and not a string. The leaf now answers to a second content-start head, the
  workflow-authoring reference's own, which is what the leaf already means. The prefix is
  an artefact of shipping files and says nothing about the body. csift names the one such
  body whose producer has been read, and says so where the leaf is documented. The bare
  bodies share no head, and the records carry no field saying which command expanded them.
  The parent link names the command's own wrapper on three of the four measured records
  and an attachment on the fourth.
- `whoami --to` predicts a send from the lane its positional names. The prediction
  depends partly on the relation between sender and receiver. The environment names the
  top-level session in every lane, so a subagent used to get the prediction for a send
  from its parent. `whoami @trap:<marker> --to @<lane>` and `whoami @<agent-id> --to
  @<lane>` now print the positional's sections and then the prediction from that lane.
  The pair used to be refused. In JSON the positional's rows, the `reach` row and one
  summary carrying both key sets make up one stream. With no positional the sender is
  still assumed to be the top-level session, and the stderr note now says to name your own
  lane as the positional.
- A quota's reset instant rides as the timestamp pair. A rate-limited assistant record's
  `api_failure.quota` carried `resets_at` the way the harness writes it, as bare epoch
  seconds. It was the one instant on any csift surface with no `_utc` / `_local`
  pair and no unit. The quota object now carries `resets_at_utc` and `resets_at_local` in
  its place. The label zone reads `quota <window> resets <local instant>` in the one
  canonical local form, where it used to read `quota <window>` and leave the instant to
  JSON. `search --help`'s exposed-keys section and `show --help`'s key table name the
  pair.

### Changed

- The root `--help` now names every `floor_cause` value. Seven help pages named
  `floor_cause` as a key, and none named its value domain. That left `--help` thinner than
  `SKILL.md` on a closed enum a consumer branches on. The global `--cross-compactions` doc
  now names all eight values, sorted by what a reader can do about each. Only
  `budget-exhausted` is one a larger budget can move, and only when that boundary's door
  is open. Help text is surface.
- The root `--help`'s newline pitfall said the opposite of what the binary does. It read
  "Text-mode excerpts keep a record's LITERAL newlines (a multiline Bash command renders
  as-is)". The excerpt builder in fact collapses every whitespace run to a single space,
  so a text excerpt is one line per record. The pitfall it warned about was real, and its
  mechanism was inverted. A `head -N` cuts whole records off the end and takes the footer
  with them. It does not cut into a record. The help now states the collapse, names the
  JSON `body` and `--raw` as the two ways back to the newlines, and puts the pipe advice on
  its true footing. The same measurement corrects a second sentence, in `SKILL.md` alone.
  `body` keeps the newlines of a render csift did not already flatten. An opener csift
  reconstructs (`user.message`, `user.answer`, `user.rejection`) is flattened a layer
  earlier, in the model. Its `body` is flat too, and `--raw` is the only way to its bytes.
  Help text is surface.
- `whoami --help` named one summary terminator where the stream has three. An ordinary
  run closes `{identities, live_child_lanes, other_live_lanes}`, `--to` closes
  `{queued, refused}`, and `--peers` closes `{peers}`. The three shapes share no key, so a
  consumer reading the terminator has to know which mode produced it. The help described
  it as `{kind:"summary", identities}`, one key of the five, and `SKILL.md` named two of
  the three. Both now name all three. The same edit adds the clause the newline
  correction left out. The JSON `body` is null unless the excerpt cap was lifted
  (`--no-truncate`, or a `show --line`/`--uuid` address). Help text is surface.
- `SKILL.md` is rewritten as the always-current mastery manual. It had become a ledger.
  37 lines carried a csift version rider, so a reader had to work out which sentences
  described the binary in front of them. It carries none now, and the Surface line,
  checked against `csift --version`, is its only staleness detector. The order is
  orientation first, then the cross-cutting laws every command obeys (targeting, the five
  laws, labels, provenance, survival), then the commands, the machine surface and the
  failure modes. Each fact is stated once and cross-referenced by section. That retires six
  duplicate clusters running to four, five and six copies apiece. Every retired spelling
  now lands in one section, so a caller who errored on one has a single address. Two
  statements that had gone stale are corrected against the binary. A text excerpt is one
  line per record, and sixteen of the eighteen bookkeeping line types build their
  key=value head from their own bytes. It ships at 59,591 tokens, 2,863 over the 56,728 it
  replaced (tiktoken `cl100k_base` throughout). The rewrite itself landed at 56,687, so its
  dedup and rider removal paid for every section it added. Two correction rounds bought
  the rest. They grew the retirement table to every spelling the binary refuses, and added
  the crossing default's value and the label-zone tags the file named nowhere, on
  `search`/`show` and on thirteen other commands. They also added the three empty-result
  stderr lines it had never quoted and the third summary `whoami` can close with. The two
  73-row transparency tables and the relation inventory keep their exact rows, so every
  drift test that pins them still holds in both directions. One header line moved. The
  shape table's third column was the same escape-hatch command on all 73 rows, so it is
  stated once above the table, and the drift test's literal moves with it.
- The skill is now a directory, `skills/csift/`, and the root `SKILL.md` is gone. The
  body, `skills/csift/SKILL.md`, routes by question and states the laws, targets and core
  read paths. Fifteen `references/*.md` hold the topics and their tables, opened when the
  body routes there. Three hook scripts ship in `scripts/` with their executable bit.
  `compact-slice.sh` re-injects clipped turns after a compaction. `taskstop-teammate.sh`
  redirects a failed TaskStop aimed at a teammate. `elicitation-marker.sh` records pending
  questions to the sidecar. The body is 5,876 tokens by the create-skill validator's o200k
  count, under its 6,000-token refactor gate, and the references total 67,046 tokens, read
  on demand. The Surface line reads `Surface: csift 0.13.0`, as `csift --version` prints
  it. A clone installs it with `ln -s "$PWD/skills/csift" ~/.claude/skills/csift`. The
  drift tests that pinned the old file now pin the reference that owns each table.
- The survival walk now crosses a compaction boundary under a budget, and every surface
  says what it crossed. Claude Code's loader reads `parentUuid` only, so it stops at a
  `compact_boundary`, and the whole history above the cut leaves the conversation. csift
  stopped there too and flagged that region `pre-cut`. The boundary does name the record
  the compaction re-anchored onto, in `logicalParentUuid`, and the conversation genuinely
  continues across a compaction. The records above a cut belong to the same conversation.
  Until the walk gets up there, a turn the operator rewound past cannot be told apart from
  history csift merely failed to resolve. The walk now follows that link up to N
  boundaries. N is set by the new global flag `--cross-compactions <N|all>`, which every
  chain-walking command honours: `search`, `show`, `stats`, `files`, `recover`, `verbatim`
  and `image`. Above a boundary the walk crossed, the ordinary rules apply. A turn the
  operator rewound past above a cut is finally named for what it is, where it used to be
  filed as unresolved history. Above the floor, everything off-chain stays `pre-cut`. The
  default is `--cross-compactions 3`, the measured value. On an idle host, budget 3 ran
  inside the same-binary noise band against budget 0 on every one of 95 chain-walking rows
  over a compacted transcript and the 397 MiB landmark. SPEC 7j carries the rule and the
  numbers. This is a surface change on a compacted transcript, and it moves records in
  both directions. A record whose survival was `pre-cut` becomes `live` where the chain
  reaches it and `abandoned` where it does not, and a turn that becomes abandoned leaves
  the numbering. The measurement covered eight transcripts, the seven top-level ones of
  one projects root that carry a boundary and sit between 9 and 21 MiB, plus the 397 MiB
  landmark. Three of them move at the default. 490 emitted hits go from `pre-cut` to
  `live` on the landmark, and 1,150 and 952 on the other two, while their whole-file
  `abandoned_records` rise by 258, 1,231 and 1,135. `--cross-compactions 0` restores the
  previous reading exactly. All 32 of 32 streams are byte-identical to a build that
  predates the flag. Under it csift says nothing about crossing at all, neither a footer
  line nor a JSON key. That equality is a measurement with one known exception, and it is
  not a law. The guard the pre-flag walk kept for a boundary whose own `parentUuid` is
  non-null and names no record crossed such a boundary, and budget 0 does not. No
  transcript of the measured corpus carries one. Otherwise the footer is never silent. It
  names how many boundaries were stepped over, how many were not, and where the chain
  floor sits. It also says why the floor sits there. Either a budget ran out, or a
  boundary's door is shut by one of three facts. The record can be on no line of the
  file, on a line the pre-boundary cut took out of the map, or on a line the walk had
  already reached. The footer offers a flag value only after probing whether one more unit
  of budget would actually move that floor. A budget can run out at a boundary whose own
  logical parent is gone, and there every larger budget lands on the same record. JSON
  carries the same six facts as `crossing_budget`, `boundaries_crossed`,
  `budget_exhausted`, `boundaries_above_floor`, `floor_line` and `floor_cause`.

- An unmodeled unit's excerpt now elides an opaque `data` blob. The key=value excerpt cuts
  each value at 80 characters. A base64 image source or a `redacted_thinking` ciphertext
  swallowed the whole budget, and the sibling keys that say what the shape is, such as
  `media_type` and the block's own `type`, never printed. `data` is now elided by name at
  every depth, as `headers` already was, and those keys print in its place.
- A pattern carrying a backslash now matches the text csift renders. The needle safety
  predicate counted the double quote, the control characters and DEL. It did not count
  the backslash, which a raw jsonl line stores doubled. `search 'C:\\Users\\dev'`
  therefore reported a definitive absence over text that `show --line` put on screen, on
  an attachment's converted body and on a plain user message alike. The predicate counts
  it now, which splits such a run into needles that do occur in the raw bytes.
- The help now names three JSON keys it never named. `show --help`'s record-row table did
  not carry `superseded_draft`, and `search --help`'s hit schema carried neither
  `denial_kind` nor `image_ids`. A consumer met those keys on the wire and could not find
  them on the page. All three are named now. A contract test asserts the direction, so
  every key the two commands emit is one their own `--help` names.
- The `for` array is named once on each help surface. Four families each listed the
  shared provenance key in `show --help`'s record-row table under their own block, spelled
  four different ways, so a consumer reading that enumeration read four keys. It is one
  row under the shared block now. `search --help`'s JSON schema listed every hit key
  except `for`, so the array shipped on the wire and appeared in no list. It is there now.
- `scheduled_at` now answers on the scheduler's own fire record too. The field carried the
  instant for the fired prompt and read null on the `harness.schedule.event` record, which
  is the line the instant is written on. It now reads that line's own parenthesised tail,
  so the key means one thing on both halves of the pair. The head still prints it.
- `show --format json` carries a record's queue facts. `queue_operation` and
  `queue_reason` reached a `search` hit and no `show` record row. Running a hit's own
  `refetch` command answered null for a record that the query it came from had just
  reported in full. Both keys are on the row now and in `show --help`'s key table. The
  pair also has one writer now. The scan used to read the record while the address site
  hard-coded both keys to null.
- A leafless unit renders its facts on the text surface too. An addressed record that
  csift models no leaf for carried its family facts into JSON and dropped every one of
  them from the label zone. `show --line N` and `show --line N --format json` therefore
  disagreed about the same record. The label zone now renders the shape in place of the
  leaf path and the facts after it, exactly as a labelled unit does.
- A `for` edge cuts a uuid to its handle and prints every other id whole. The label zone
  cut an unresolved target to eight characters whatever it was. That is right for a uuid,
  whose first eight hex digits identify it, and the compaction excerpt has printed that
  handle for versions. It is wrong for a wire id, whose head is the part all of them
  share. `hook-bb000000` became `hook-bb0` and `toolu_01zzreef` became `toolu_01`, which
  kept the constant prefix and threw away the distinguishing tail. The cut now applies
  only to a uuid-shaped target. JSON is unchanged, because `target.uuid` always carried the
  id whole.
- An attachment unit renders the text the model received. This is BREAKING. The body of
  a `harness.hook.context` unit, and of every attachment unit whose record carries a
  persisted conversion, is now that converted text. It used to be the joined content or
  the payload JSON. The unit says which of the record's two copies it shows, `[rendered]`
  or `[payload]`, with JSON `body_source`. It names itself beside that marker with
  `[hook <event> <name>]`, `[hook <event> <name> exit <code> <ms>ms]`,
  `[context <payload type>]`, `[structured]` or `[mid-turn]`. A record carrying a
  conversion is searchable by the converted text and no longer by the payload text it
  replaced. `--raw` does not recover the difference, because the backfill runs after the
  match phase. The way to the bytes is `csift show @<id> --line N --raw`. A
  `queued_command` carrying a second conversion for a human turn keeps the ordinary one as
  its body and reports the difference (`[human-turn render differs]`, JSON
  `human_turn_render_differs`). Which copy the model received depends on the turn the
  record was replayed into, and the record does not say. The body decodes a JSON string
  where csift used to emit the payload bytes unparsed, one parse per record. The cost was
  measured on two synthetic 200,000-line transcripts, seven interleaved rounds with a
  same-binary control in each. A file whose every line carries a conversion costs 1.131x
  (control 1.033x), and one where none does costs 1.068x (control 0.942x). An ordinary
  scan of a transcript with no attachment line reads 0.959x against a 0.967x control.

- Every taxonomy family now has one stamping site and one place it edits. The extension
  points on a search hit existed with nothing filling them, so each family would have cut
  its own dispatch through the same shared files. `family_facts` is that one seam, called
  at both hit-construction sites. A record fetched by `show --line` therefore carries
  every fact the same record carries under `search`. `search --help` becomes one section
  file per family, and `show --help` renders its record-row key list from a table with a
  family column. Splitting them moved no rendered byte. What the families then add is
  visible on the page. `search --help` carries the first of it, a `BOOKKEEPING LEAVES`
  region stating the render law in the next bullet. `show --help` renders exactly what it
  did, since no family had added a key yet.
- A bookkeeping unit renders its key=value head over its own bytes. A summary is lossy,
  and `search` matches rendered text, so a key that only the head elided had stopped being
  findable at all. The head stays, the line's verbatim JSON follows it, and every key the
  head does not name is matchable again. The `harness.system.*` leaves are deliberately
  outside the rule. An `api_error` line carries the whole HTTP response header set, and
  csift prints that set on no path but `--raw`.
- A uuid that a record names now resolves to that record's line.
  `ClassifyCtx::line_of_uuid` is backed by the chain's own uuid map and needs no second
  index, so an edge can render an address. A caller with no row space gets an honest
  absence and never a guessed line.
- A taxonomy family now decides what its own lines say. The facts seam had no render twin,
  so a body was still decided in the shared chain. `family_render` dispatches by the leaf's
  category to the family that owns it, ahead of the engine's generic arms. Four renders
  moved into their families. They are the two attachment bodies, the bookkeeping
  head-and-body composition, the six `type:"system"` event heads, and a thinking block's
  body. Not one rendered byte moved. 122 of 122 corpus-derived commands compare
  byte-identical against a build of the base commit, with the golden baseline and the
  whole suite beside them.
- There is one `for` edge now, built one way. Four families point at other records, and
  three had each invented a different fact for it. The one edge renders `for: <kind>
  L<n>` when this transcript carries the uuid, and the id with `(not in this file)` when it
  does not. It renders `unknown (<reason>)` when the record names nothing, so an edge is
  never silently absent. Several edges on one hit accumulate under the one `for` key, where
  they used to overwrite one another.
- A `headers` member reaches no rendered surface. An `api_error` record carries the
  failed request's whole HTTP response header set, which holds a session cookie and an
  organization id among 18 names. The key=value excerpt csift falls back to for a line it
  models nothing about used to print it. Every `headers` member is now elided at every
  depth. `<elided>` stands in its place so a reader learns the set is there, and `--raw`
  stays the one way to the bytes.
- Two more payload types are marked as never reaching the model, and a result set moves
  with them. The harness's conversion table drops `goal_status` and
  `async_hook_response_batch` outright. csift's `NEVER_CONVERTS` list had neither, so it
  reported both as delivered. They are in it now, and the consequence shows on three
  surfaces, measured on a `goal_status` attachment. `search -t harness.context` used to
  return it and now returns nothing, and the empty-result probe names
  `harness.context.task` as where it went. The bare category face shows only what the
  model received, and the leaf's own `-t` still returns it. The hit's JSON `delivered`
  reads `false` where it read `true`, and the rendered label zone gains `[not delivered]`.
  A query written against the bare face has to name the leaf instead.
- A coordinator relay now says where it came from. Every other inbound message renders a
  `from => to` arrow, and this one rendered none. It now reads `coordinator => self`. The
  from side is the literal word because the record carries no sender field. The harness
  builds that origin object in two places, and both build the one-key form.

- The `-t` label taxonomy is now 17 categories over 73 leaves, split by producer. The old
  38-leaf space had one bag in it. `harness.meta` pooled hook-injected text, a hook run's
  own record, a slash command's caveat, the loop tick, the end-of-turn telemetry and the
  file-history instrument. Its `.attachment` and `.system` leaves were catch-alls for
  everything else. A selector could not name what it wanted, and a census could not say
  what a scope held. The new space asks one question of every record, what wrote it, and
  the answer is its category. The categories are `user`,
  `agent.{message,thinking,tool,communication}`, and under `harness` the twelve producers
  `notification`, `compaction`, `command`, `interrupt`, `schedule`, `resume`, `hook`,
  `context`, `enforce`, `turn`, `system` and `bookkeeping`. Renames land one for one.
  `agent.message` becomes `agent.message.plain`, `agent.thinking` becomes
  `agent.thinking.summary`, and `harness.meta.loop` becomes `harness.schedule.tick`. The
  three telemetry leaves become `harness.turn.*`, and `harness.meta.snapshot` becomes
  `harness.bookkeeping.file-history`. `harness.meta.hook` splits three ways by producer,
  into `harness.hook.context`, `harness.hook.feedback` and `harness.command.caveat`. The two
  catch-alls become per-value tables. `agent.message` and `agent.thinking` still parse.
  They were leaf paths and are now categories, so each selects its whole face.
- A bare category narrows exactly as a bare role does. Both are the coarse "show me the
  conversation" ask, so both select only the leaves whose content the model receives. A
  glob (`-t 'user.*'`) and an exact leaf path still reach every record.
  `harness.bookkeeping` is the one category whose whole face is invisible. The bare form is
  accepted and matches nothing by that law. `search` prints one stderr note naming the
  remedy beside the ordinary empty-result diagnosis, with `empty_visible_face` in the JSON
  summary. A `-T` that cancels your `-t` is still a hard error. An honest empty is not.
- `classify` became a dispatch over five mechanical family tables. They read a top-level
  line `type`, a `type:"system"` subtype, an attachment payload `type`, a content block's
  shape and an isMeta user template. Each takes a value in and gives a leaf out, with no
  look at the payload. The v0.10.0 `classify/promoted.rs` is now `classify/bookkeeping.rs`,
  one of the five. Three behaviour changes ride the dispatch, and each gives a leaf to
  records that carried none. Every non-transcript line type now carries a
  `harness.bookkeeping.*` leaf. An attachment carries the leaf of its own payload type in
  place of one catch-all. A thinking block with no summary text
  (`agent.thinking.omitted`) and a `redacted_thinking` block (`agent.thinking.redacted`)
  each take their own leaf, where both used to read as plain reasoning.
- Every leaf the shape inventory records is now one a record can carry. The first pass at
  the taxonomy advertised `harness.context.image`, `harness.context.skill` and the four
  `harness.enforce.*` leaves in its tables while nothing in the classifier could produce
  them. Its drift test walked three of the five axes, so the gap did not show. The
  isMeta-template table now carries an arm per row, each keyed on the literal its producer
  writes. Two more arms are structural. A relayed message whose `origin.kind` says `human`
  is the person's own, and the coordinator relay takes the same leaf as a peer's. The
  `[SYSTEM NOTIFICATION - NOT USER INPUT]` envelope is treated as a carrier and not as a
  shape, so the background-task pulse inside it classifies by its own kind and no longer
  vanishes. A `<system-reminder>` matching no more specific template lands under the
  reminder leaf. An image-only record carries the leaf its text form carries, and the
  `dynamic_skill` payload joins the listing family. The drift test covers all five axes,
  with a hand-written specimen per row.
- `show --line N` never hard-misses a parseable line. A shape csift models no leaf for
  renders one unit reading `? unmodeled: <type[/subtype|/attachment.type]>`, with a
  key=value excerpt of the line's own top-level keys. JSON carries `unmodeled` beside a
  null `label` and an empty `labels`. Before this, every bookkeeping line type read as
  absent. A scan never emits such a unit, so no census, count or `-t` result moves. A line
  that is not a JSON object at all is still a hard miss pointing at `--raw`.
- A message record with no text now renders under an address. The 1:1 law reached the
  non-record line types and missed the message lines. A `fallback` block, a `document`
  block or an image with no caption produced no unit, and the address bailed on a line
  that is plainly there. Such a record renders now, and the unit names the axis its leaf
  would have come from (`user/document`, `user/isMeta`). There is one unmodeled form.
  `? (no label)` is gone, because it told a reader nothing a shape name does not. A record
  that has a leaf and merely lacks text keeps its leaf. An assistant turn the API served
  from a fallback model takes `agent.message.plain` with an empty body.
- `-t user` and `-t agent` no longer parse every attachment line. Two leaves ride an
  attachment as a second view and are not its kind, the human's mid-turn steer and the
  model's structured answer. They sat in the set that turns on the keep admitting every
  attachment record. The most ordinary selectors in the tool therefore paid for lines they
  did not want, 236 ms against a flagless scan's 70 ms on an 89 MB attachment-heavy
  transcript. Each leaf now has its own payload needle, and the three selectors run at
  flagless speed.
- The person's mid-turn steer reaches `-t user` again. When someone types while a turn is
  running, the harness folds the text in as a `queued_command` attachment and stamps its
  provenance inside the payload, beside `commandMode`. csift read a record-level `origin`
  that no line of that shape carries. Over two corpora 0 of 3,692 and 0 of 3,574 lines
  carry it, against 219 and 217 that carry `attachment.origin`. The arm never fired, and
  every steer classified as harness context. It reads the payload now. The second half of
  the defect was quieter. The leaf rendered through the opener path, and an attachment has
  no `message{}` for that path. Even when the arm did fire the record emitted nothing at
  all, with no hit, no census key and no `-t user` result, and only an address reached it.
  It now falls through to the render seam and takes the attachment family's ordinary body
  rule, like every other view of that line. The body is the conversion the harness
  persisted when the record carries one, marked `[rendered]`, and otherwise the verbatim
  payload JSON. Measured over one projects root, one parse per non-blank line, 20 of the
  164 human-origin `queued_command` lines carry a conversion.
- A flagless scan reaches the person's mid-turn steer too. The needle that admits a
  `queued_command` line was gated on an explicit selector, so `-t user` found the steer and
  a bare `csift search` did not. A flagless scan is a superset of `-t user`, so that broke
  the selector law and saved nothing worth saving. The needle now takes the gate the
  csift-channel delivery keep has had since 0.11.0. It is on unless your `-t` excludes
  `user.message`, which keeps `-t harness` paying nothing. The parse still decides. Only a
  payload whose own `origin.kind` reads `human` becomes `user.message`, and every other
  `queued_command` stays `harness.context.task` behind its own gate. The model's own
  `structured_output` answer takes the same gate for the same reason. `-t agent` reaches
  `agent.message.structured`, so a flagless scan cannot miss it. Those two leaves are the
  only attachment-carried exceptions, since both are words someone owned, a person or the
  model. The coarse keep is untouched, so a flagless scan still parses no other attachment
  payload. That cuts both ways, and the cost is worth knowing. A selector reaching an
  attachment leaf with no needle of its own still parses every attachment line, and a bare
  `-t harness` reaches those leaves and pays for it. Measured on an 85 MB transcript of
  150,000 `file` attachment lines, 21 interleaved reps with a pattern that matches,
  `-t harness` reads 3.4x a flagless scan (251.0 ms against 73.3 ms) and
  `-t harness.context.file` reads 359.5 ms. Forty payload types cannot each carry a
  needle. The two that do are the two an ordinary conversation query names.
- A relayed message reaches an ordinary scan, and every other leaf is now swept for the
  same defect. A coordinator's relay carries `agent.communication.inbox`, and the record it
  rides is one the harness wrote into the conversation. It is isMeta, so it opens no turn.
  The arm rendered through the opener path, which answers nothing for a record that opens
  none, and the relay is no attachment either. The record carried a delivered message and
  emitted nothing at all, with no hit, no census key and nothing under
  `-t agent.communication.inbox`. 141 such records across 89 files of one projects root
  (57 across 49 of a second) were reachable only by `csift show --line`. The arm now falls
  back to the record's own text, which is what the peer relay beside it has always done.
  That makes twice in this release that a leaf was reachable only by an address, so the
  pin is a sweep and not a third one-off. For every specimen in the shape inventory, a
  flagless scan must emit exactly one hit for each record carrying a leaf the model
  receives. Two specimens have no text to emit and are named in the test with their
  reasons. An image-only record is a picture, and an assistant turn the API served from a
  fallback model wrote no text. The exemption is keyed on the specimen and never on the
  leaf. `agent.message.plain` is the ordinary assistant message, and exempting the leaf
  would exempt the whole population the sweep exists to guard.
- `csift msg` prints a command that still runs. Its lane listing recited the fact half of
  its own join as `csift search '<id>' @<lane> --additional-context`. This release retires
  that flag, so the one recipe a reader needs to check the channel without trusting `msg`
  exited 1. It now prints `-t harness.hook.context`, which is what the help has said all
  along.
- `wait`'s activity census counts all four thinking leaves. It named two of them one by
  one, so the leaf split silently dropped the other two from the count. It keys on the
  category now.
- `harness.schedule.fire` changes its gate and keeps its meaning. It keyed on
  `promptSource:"system"`, a stamp the submit path puts on every isMeta submission. It now
  keys on the parent link to the `system`/`scheduled_task_fire` record the scheduler wrote
  just before it, which is the only mark the producer leaves on the prompt itself. That
  fire record gets a leaf of its own, `harness.schedule.event`.
- A shape inventory (`model::inventory`) backs the taxonomy. It holds one row per on-disk
  shape on those five axes, covering 22 top-level types, 11 system subtypes, 50 attachment
  payload types, 8 block types and 17 isMeta template families. Each row names its leaf,
  its gate, whether the harness converts it for the model and whether a person sees it. It
  records the mapping and is never a second copy of it. A unit test walks every row back
  through the family table that decides the leaf. Another asserts that the shaped leaves
  and a hand-kept table of the rest partition the leaf set exactly. The candidate gates and
  the generated SPEC and SKILL tables all derive from it, so the honest-empties note can
  never name a stale leaf.
- The source tree is re-managed. Command modules now live under `src/commands/`, and the
  bash lexical layers under `src/bash/`. `src/` sat at both structure caps (20 counted
  direct `.rs` files, 16 subfolders), and a cap that binds is a refactor gate, never a
  number to raise. `bash_danger*.rs` and `bash_mutations/` become `src/bash/danger.rs` +
  `src/bash/danger/` and `src/bash/mutations.rs` + `src/bash/mutations/`. The thirteen
  subcommand families move under `src/commands/`, which leaves the crate root to the base
  layer they build on (`cli`, `model`, `parse`, `path`, `subagent`, `bash`, `chardiff`,
  `text`, `time_window`, `timez`). The four `model/classify_*.rs` siblings become
  `model/classify/{support,promoted,channel,resume}.rs` under the engine that uses them.
  `Record::direction` moved verbatim into a fifth child so the engine stays under the
  600-line cap, and the engine's cases are nested to match. Every path is respelled at
  every call site, with no alias and no forwarding shim. No behaviour changes.
- The isMeta injection family now says what each record was written for. The harness
  writes text into a turn shaped exactly like the operator's own message. The label said
  who wrote it, and nothing said what it answers. Most units of the family now carry one
  `for` edge (JSON `for`). `redoes` names the assistant turn an enforce nudge sends the
  model back to, and `blocked` names the turn a hook's feedback stopped. `fires` names the
  scheduler event a fired prompt came from, and `reports` names the tool_use a notification
  frame's pulse names. `attached_to` names the record everything else was injected under.
  The line is resolved through the chain's own uuid map, so an edge is an address you can
  follow and not a uuid to go searching for. A key that resolves to no line keeps the id
  and says so, and a record naming no such key renders its reason and keeps the edge. The
  two inbound relays are the case that needs the reason. A coordinator relay and a peer
  relay carry one leaf, so the arm reads `origin.kind`, and each answers for its own shape.
  The coordinator's origin carries only its kind. The peer's names the lane that sent it
  (`[peer lane <id>]`, JSON `peer_sender_task_id`).
- A person's message that the harness relayed is now marked. A message delivered into a
  running lane is the person's own prose and takes `user.message` like any other. On every
  surface it was byte-identical to one they typed, although it opens no turn, being
  isMeta. It renders `[relayed]` and carries the JSON twin `relayed`. A marker is
  display-only, and without the key a machine consumer could not tell the two apart at
  all.
- `harness.hook.feedback` reaches every blocking-hook name. One formatter writes the
  template over a closed four-name table, and csift matched only the name the corpus
  happens to hold. The first time a TeammateIdle, TaskCreated or TaskCompleted hook
  blocked, its feedback would have landed in the isMeta residue, with no leaf, no census
  key, and only an address to reach it.
- A bare `-t agent` now returns the model's structured answer too, and the help pages name
  every key they emit. `agent.message.structured` rides a `structured_output` attachment,
  which the harness converts to nothing. The record-level delivery verdict therefore read
  not delivered, and the bare-face law took the leaf out of `-t agent` and
  `-t agent.message`. A reader asking for the agent's messages got the plain half of a
  two-leaf category and nothing saying the other half existed. The leaf is now the one
  stated exception to that law. The axis asks what the model received, and this is the
  leaf the model wrote. The API gives a turn free text or a schema-bound answer and never
  both, so the coarse ask answers with whichever the turn used. The fact does not move.
  The record was not delivered, so its `delivered` still reads false and the unit still
  renders `[not delivered]`. Only the face it belongs to changes. A glob and an exact leaf
  reached it all along. Alongside it, both manuals now write every JSON key they name in
  backticks or in their own key block. The `search` hit schema gained `tool_use_id` and
  `source`, which a hit carries and the schema had never listed.

- The provenance relation `hook run` is respelled `hook_run`. `--count-by for-kind`
  prints `<count>  <key>` per line like every other census axis. A key with a space in it
  cannot be read back by the whitespace split every reader of that surface writes, and
  this was the only key in the closed vocabulary that carried one. The word means what it
  meant. The id a `stop_hook_summary` names is a hook run id, and it is not the API
  tool_use id its field name suggests. Three surfaces move together, the census key, the
  `[for: hook_run ...]` label-zone tag and the JSON `for[].kind`. Neither spelling has
  shipped in a released binary.

- A retired `-t` spelling now says why it is not a selector, beside where its records
  went. All thirteen are hard errors, and each message carries `('<spelling>' <reason>,
  so this is: <successors>.)` ahead of the valid-selector list. The middle column is a
  reason and not a release, because the only reader of that message holds a binary on
  which the spelling is already gone. Which release retired it changes nothing for them,
  while the reason carries over to the next spelling they guess. Three reasons cover the
  thirteen rows. `thinking`, `tool` and `tool-response` are flat spellings, and the
  taxonomy is `role.category.leaf`. Seven share one clause, that a category is decided by
  the producer that wrote the record, and each says what it did against that rule.
  `harness.meta` pooled several producers under one category. The five
  `harness.meta.{loop,turn-duration,away-summary,stop-hooks,snapshot}` paths each named a
  record by the category it was pooled in. `harness.schedule.continuation` put the resume
  loader's own prompt under the scheduler's. The last three,
  `harness.meta.{hook,attachment,system}`, pooled three or four producers under one leaf,
  and a leaf names one shape. A retired prefix plus `.*` is the same retirement, is looked
  up bare, and prints the bare row's message. The set is a walkable table and not a match
  arm. That is what lets a test hold `SKILL.md` section 10 to these exact successor strings
  row by row. SPEC's own copy of the table was two successors short on `harness.meta` and
  one short on `harness.meta.hook`, and it now prints what the errors print.

- Every string a help page quotes is one the binary prints, and no page carries csift's
  own chronology. The measurement covered all eighteen pages (`csift --help` plus one per
  subcommand) against the last released binary, both invoked under the basename the usage
  line echoes. 602 rendered non-blank lines differ when counted once per page, 215 only in
  the older arm and 387 only in this one. Counted once per distinct line text, 479 differ.
  The two figures differ by the lines that render on more than one page, and by the two
  lines of the `--claude-home` entry that `search --help` moved within its page, which a
  diff reports as removed and added. The new root `--cross-compactions` block is seven of
  those lines and renders on every page. It accounts for 126 of the first figure and 7 of
  the second, and on `ack`, `plan` and `send` it is the whole difference. `search` carries
  348 of the 602 by itself, its taxonomy block rewritten leaf by leaf. Every other moved
  line falls into one of three classes. Most restate a change this record already names
  one by one. One is a quoted string corrected to the bytes. `list --help` quoted the
  subagent brand with one space where `list` prints two after the word and two around the
  separator, so a reader matching the manual against the output matched nothing. The rest
  carried no fact about the binary and are gone. They were csift's own chronology (eleven
  version riders naming seven versions over five pages, one date on which something was
  measured, and the sentences describing a past csift), a word for who is asking in place
  of what a flag does, and an example token, now in the hand-patterned shape every fixture
  id in these documents uses. Each was rewritten as the fact it was there to give, so a
  page reads correctly to someone who has seen no other csift. Every date that remains is
  an example the grammar needs and belongs to no release. There are twenty-three ISO8601
  instants in the `--since`/`--until` blocks of five pages, two more in the time-grammar
  list, the root page's timestamp-format sample, and one synthetic record instant inside a
  rendered bookkeeping head.

- A manual now quotes the brand its own command prints, because the subagent brand is
  four strings. One fact is spelled four ways across the seven surfaces that print it. The
  fact is that this row is a subagent transcript and its bare id is not a re-feedable
  target. `list` writes `SUBAGENT  <hex>  ·  parent SESSION <uuid>`, with two spaces after
  the word and two around the separator. `files`, `recover`, `stats` and `verbatim` write
  one space after the word and two around the separator. `show` writes one of each.
  `image` suffixes its row with `  SUBAGENT <hex> · parent <uuid>`, with no `SESSION` at
  all. `SKILL.md`'s `list` row and SPEC's `image` row therefore quote different strings,
  and both are right. A manual that copied its sibling's would send a reader looking for
  bytes that surface never writes. `SKILL.md` states the `body` field's full condition
  beside it. `body` is non-null exactly where the excerpt cap was lifted, which is
  `search --no-truncate` and a `show --line`/`--uuid` address. `show --turn` is a window
  over the turn numbering and not an address, so its rows render through the ordinary
  excerpt budget and carry a null `body`.
- A `.rs` comment carries a non-ASCII character only as a quotation of csift's own
  rendering. A glyph in a comment used to be either a quotation of what csift prints or
  the comment's own arrow, heading rule or ellipsis, and nothing in the character said
  which. Now only the first use is allowed, with the glyph exactly as printed inside a
  backtick span. The set a comment may quote is derived from what the binary prints. It
  holds every non-ASCII character in a string or char literal of a non-test source file
  under `src/`, decoded through its escapes, plus every one in a clap `///` help doc, 26
  glyphs in all. The pre-commit gate enforces the character and the balance of the span.
  The em dash is one of the printed glyphs, so the older outright ban on it is this rule
  read narrowly. The rewrite the rule required moved no byte of the release binary's
  strings or help pages. 2,027 comment lines carried such a character, 1,653 of them
  outside any quotation, and the tip carries 0 violations. `AGENTS.md` section 8 states
  the rule with its derivation.
- `recover --help` states the content-anchor rule for reads and writes separately. It
  said every anchor demands a single simple command and that pipes and chains stay in the
  boundary lanes. That holds for a read anchor only, because a compound command's stdout
  names no one file. A write anchor is per segment of a pipe or of a `;`, `&&`, `||` or
  newline chain. A heredoc write followed by a command that runs the file therefore
  anchors, and a clean two-segment chain like that restores the file whole and counts one
  `bash-write-anchor`. The whole command's result must be clean, and no other segment may
  write the same path. A `cp` over the file refuses the anchor, while a `wc -l` or a
  `chmod +x` of it leaves the anchor standing. The page now says both.
- `send`'s receipt counts the body in the unit the budget is counted in. The chunk budget
  is UTF-16 code units, and the `message` line printed the body in characters beside it. A
  body of 6,000 emoji read `6000 char(s), 2 chunk(s) of 9200`, a size that fits one chunk
  beside a count of two. The line now prints the body in both units and names the
  budget's unit, as `6000 char(s) / 12000 UTF-16 unit(s), 2 chunk(s) of 9200 UTF-16
  units`. An ASCII body reads the same number twice. The JSON summary gains
  `message_utf16_units` and `chunk_budget_utf16_units` beside `message_chars`.
- The `--turn` help now says what turn 0 holds. The `search`, `files`, `recover` and
  `verbatim` pages called turn 0 the pre-first-user lead and said `1..N` skips it. The
  grouper actually folds that lead into the first opener's turn. Turn 0 holds the first
  human message, so `1..N` drops the first human turn as well. The four pages now say so.
  They name five turn openers where they named three, which are a typed message, an
  answered AskUserQuestion, a tool rejection carrying typed text, an inbound peer message
  and a `<task-notification>` pulse. They add that a slash command with arguments opens no
  turn. `stats --turn` says its axis is the turn order with turn 0 holding the first
  opener's turn, where it said genuine-turn order.

### Fixed

- The two textless thinking leaves rendered a body they do not have. An
  `agent.thinking.redacted` unit printed the fabricated words `[redacted thinking]` as its
  text. That was matchable text with no substring behind it in the raw line. The literal
  prefilter and the whole-file gate both reason about matchable text, and the law admits
  fabricated text only through a registered marker. An `agent.thinking.omitted` unit whose
  `thinking` field was whitespace printed a run of spaces while promising none. Both bodies
  are empty now, and the consequence is stated. With nothing rendered, a pattern scan
  reaches neither leaf. An empty pattern plus an exact `-t`, `--count-by label`, or a
  `show` address reaches them, and `--raw` shows a redacted block's ciphertext.

- Three isMeta template arms could be claimed by a record that quoted them. The marker
  law says every such arm tests the text after one left trim. The hook-feedback and
  `<local-command-caveat>` arms tested the untrimmed string. The edit-failed-retry notice
  tested `contains`, which on that path can only reach a record quoting the notice. All
  three anchor now. Measured over two projects roots, the emitted leaf moves for no record,
  so this closes a hole and changes no count.
- A textless image record now says what it is. An isMeta record whose content is image
  blocks and no text renders no text. `show --line` on one answered with the key=value
  excerpt of its own top-level keys and nothing else, with the head of a base64 payload
  among the values. It now carries its block counts in the label zone beside that excerpt,
  as `[context image-only: N image block(s), M document block(s)]` with JSON `image_only`.
  The excerpt stays, because it is the line's own keys, rendered by the path every
  textless address takes.

- An `api_error`'s response headers never reach a rendered or JSON output path. The HTTP
  shape of that record carries the response headers verbatim, cookie and organization id
  included. The render names the status and the retry counters. The JSON `error` fact
  carries the error's `type` and `message` and nothing else. `--raw` prints the source line
  and is the one documented way to the bytes.
- What an `api_error` says failed is searchable on both observed shapes. The record
  carries no `content` key at all, so the failure's kind rides the head and its wording is
  the body. Without them, the statement of the failure was in the output under no pattern
  that could reach it. Neither shape carries both at one level. The header-bearing HTTP
  shape keeps the wording two hops down, inside the response body it received, so reading
  one level printed the kind and dropped the sentence.
- The whole-file gate re-renders from the record's source line. Stage 2 of the gate is the
  one place csift proves a negative, and it re-rendered without the line. For a record
  whose text lives in fields the record model does not carry, it could prove a file
  unmatchable on text the search would have produced.
- Four shape-inventory rows said a person sees a record they never saw. The image
  reference, the image block, the coordinator relay and the notification frame are all
  isMeta user records. The transcript render refuses those unless their origin is one of
  five kinds, and none of them writes one. SPEC's inventory table and SKILL's identity
  cells follow, and a drift test now compares the two row for row.
- Three more of those rows were wrong, and two payload types the harness drops were
  missing. `atis-latch` and `fork-context-ref` read `indirect`, and `queue-operation` read
  `yes`. Reading every occurrence of the three type literals in the shipped binary finds no
  reader that renders any of them, so all three read `no`. Separately, `goal_status` and
  `async_hook_response_batch` were missing from the never-convert set. `goal_status`
  carries a leaf that is otherwise visible, so csift reported such a record as delivered
  where the converter drops it.
- The image-only arm tests authorship like every other injection arm. A leaf saying the
  harness wrote a record decided that from block shape alone. It now also requires
  `isMeta`, the conjunction the rest of the family already takes. All 94 such records carry
  the flag, so what the change decides is the shape that has yet to appear. A record
  without the flag renders `? unmodeled: user/image` under an address.
- Doc surfaces stated two facts the code contradicts, each in more than one place.
  `harness.hook.feedback` was described as the Stop hook's, where one builder writes four
  prefixes (`Stop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted`). That is corrected in
  the classify arm's own doc, in `search --help`'s leaf list and in the SPEC taxonomy
  clause. A redacted thinking block was described as a placeholder, in
  `model/automation.rs` and in the block loop's module doc. It renders an empty body, and
  what it is rides the label zone as a marker, which keeps the fabricated word out of
  matchable text.
- A stdout write that cannot land is now reported and no longer aborts the process.
  Piping a command into a reader that goes away, such as `| head` or a closed pipe, made
  `println!` abort. The process exited 101 with a std panic message naming
  `library/std/src/io/stdio.rs` where csift's own message belongs, and with a backtrace
  only under `RUST_BACKTRACE=1`. Through the shared buffered stdout, the failed write now
  takes the one error arm every other failure takes, exit 1 and `csift: error: Broken pipe
  (os error 32)`. The rule follows the writer that makes the failing write, never the
  subcommand. A `println!` still aborts, so a surface whose rows are buffered but whose
  first stdout byte is a `println!` aborts at that byte. Inside a buffered surface there is
  no exception. Its `scope` and `copies` banners go through the same writer as its rows, so
  one query has one answer for a closed pipe however wide its scope. These surfaces report
  the error. They are `search` in all four of its forms, `show` rendered, `--raw`,
  `--turn` and `--uuid`, `agents` in both projections, `files` in its three, `image` in
  both, and every one of `verbatim`'s streams. These surfaces still abort. They are `list`,
  `stats`, `recover` in all six forms measured, `status`, `wait`, `whoami`, `send`, `msg`,
  `deliver`, `show --branch-points`, and `search`'s `-c` / `-l` / `--count-by`. `plan`
  with no binding and `ack` write no stdout at all, and `csift --version` uses clap's own
  writer and exits 0. What a reader that stays receives does not move. Only the failure
  path does.

- A turn-companion record was counted as the person's own turn. `turnCompanion` is the
  harness's second authorship stamp. It sits on a `type:"user"` record the harness wrote
  to accompany a turn, which the person never sent. Claude Code's own
  is-this-the-person's-message predicate excludes it beside `isMeta`, before it looks at
  the content at all. The two stamps are not one set. Of the fourteen stamping sites in the
  verified build, twelve build an object that carries `isMeta`, and two do not. One is a
  mapper over a slash command's expansion messages. The other is the tool-dispatch path for
  the extra `type:"user"` messages a tool returns beside its result. Both spread an
  existing record and add the one key, so either can construct a record carrying the
  companion stamp and no meta flag. Reading `isMeta` alone made such a record the
  operator's own turn. It opened a turn, shifted every later turn number on that
  transcript, and moved the live-turn count every surface derives from the same predicate.
  That count feeds `search --count-by turn`, `stats`, the `<tok>·tN` headers and every
  `--turn` window. `is_genuine_user` reads both flags now, so `opens_turn` opens no turn
  on a companion. `classify` reads them at the same arm, so the label agrees with the
  boundary, and a record csift refuses to call the human is not `user.message` under a
  selector either. A companion matching a template csift models keeps that template's
  leaf. One matching none carries no leaf and renders `? unmodeled: user/turnCompanion`
  under an address. It names the narrower stamp, because the companion word describes
  exactly the records that carry both. The key rides the flattened, boxed exposed-key
  struct and adds no field to `Record`. A record naming no exposed key pays nothing for it,
  and the row every parsed line of every scan carries does not grow. It is the one exposed
  key read by a predicate and not projected as a fact, because what a reader acts on is
  the leaf and the turn boundary it changes.

- A uuid prefix longer than the first segment resolved nothing. The grammar promised a
  dash-less prefix of 4 to 11 hex digits, and the resolver compared it literally against
  the dashed basename. `@<first 9 hex>` held its ninth digit against the dash at position
  8 and failed with "no session or agent id starts with", while `@<first 8 hex>` resolved.
  The dashed spelling had the mirror gap. A literal prefix of the uuid layout resolved from
  12 characters up and was "not a recognized @-target" at 9 to 11. A prefix is now compared
  by its hex digits with the dashes dropped on both sides, so `@1a2b3c4d5` and
  `@1a2b3c4d-5` name the same sessions. The dashed spelling is accepted from the first dash
  on (claim PATH-016). Uniqueness is unchanged, and two ids sharing the digits are still an
  error naming both.
- The four edge shapes of the `@` hex grammar are decided, and each miss names the rule.
  A dashed prefix past 11 characters and a dash followed by one or two hex digits both
  resolve by their digits. A dash-less run of 12 or more hex digits is an agent id, either
  that agent or the one agent id it starts, and never a session. A session prefix that
  long keeps its dashes, and the agent miss now says so. An agent-id prefix resolves, 4 to
  11 digits through the shared prefix match and 12 to 16 through the agent fallback. The
  unrecognized-target error lists `@<id-prefix>` and `@<agent-id>` with what each
  resolves.
- A cost checkpoint said it could not find the two numbers it carries. The
  `harness.bookkeeping.cost` fact read `startTime` as a string holding an ISO instant.
  The harness writes it as a number, the session start in epoch milliseconds, and its own
  schema for the line declares `number().nonnegative().finite()`. Every `cost-state` unit
  therefore rendered `[checkpoint closes unknown (needs startTime and
  totalDuration)]` with a null JSON instant, 96 of 96 lines on one projects root, while
  `--raw` showed both keys on every one. The fact now reads the pair through the same
  function `list`'s `/clear` join uses, so the instant a unit shows is the one the join
  compares. The label prints it as local text,
  `[checkpoint closes 2026-08-31 18:58:32.913 AEST(UTC+10)]`, and JSON carries the instant
  as the pair `checkpoint_closes_at_utc` and `checkpoint_closes_at_local`. A value that is
  not a non-negative number reads as absent, which is the line the harness's own readers
  refuse. The empty label now says so with `(needs
  startTime and totalDuration as numbers)`. The `search --help` sample head showed
  `startTime` as an ISO string and now shows epoch milliseconds (claims QT-042, QT-043).
- `search --help` tagged the structured answer as a gated leaf. The leaf tree gave
  `agent.message.structured` the gate word `[-t/att]`. The page's own legend reserves that
  word for an attachment leaf that only a selector or `--attachments` admits. A flagless
  scan reaches this leaf through its own value needle. A session carrying two structured
  answers counts 2 under the leaf in a plain `--count-by label`. The tree now says
  `[scan/needle]`, and a test holds every leaf whose inventory rows all carry the
  default-needle gate to that word.
- The root help listed six `status` verdicts. Its `status` entry named running,
  waiting-children, waiting-hitl, idle-eot, stale-dead and unknown. It left out
  `idle-background-open`, the verdict for a turn that ended with background tasks still
  open, which `status --help` and the verdict enum both carry. The entry names all seven
  now, and a test holds it and the `status --help` verdict paragraph to the enum's own
  set.
- `recover --help` said five modes, and the parser holds six. The head listed the default
  restore plus four flags. The MODE section of the same page names five mutually exclusive
  flags, `--list-backups` among them. The head now counts six, the default restore and the
  five flags, and lists `--list-backups` with the others. A test reads the count and the
  listed flags off the clap `mode` group.
- A file-history head printed its instant in UTC. The `harness.bookkeeping.file-history`
  head read `[file-history snapshot at 2026-09-01T05:49:11.520Z: ...]`. It was the one
  text instant csift left in UTC, on every one of the 7,694 such units one projects root
  carries. Both forms now print local text in the millisecond form the search and show
  headers use. A delta reads its top-level `timestamp`, and a snapshot reads its nested
  `snapshot.timestamp`, the only instant that line carries. The UTC string stays matchable
  in the unit's verbatim JSON body (claim QT-025).
- A queue rider that quoted the channel envelope reached a flagless scan. The
  csift-channel keep admits any line carrying `csift-channel v1`. A `queue-operation`
  rider carries whatever its `<task-notification>` summary says. A pulse that mentioned the
  envelope therefore surfaced as `harness.bookkeeping.queue` with no `-t` at all, 13 units
  on one projects root. A `user.queued` or `harness.bookkeeping.*` record now survives only
  a scan that reaches its own leaf. `-t harness.bookkeeping.queue` (or the family glob)
  still emits the line, and an address still renders it (claim QT-005).
- `search --since -1y` was a usage error. The WHEN grammar tolerates a leading `-`, and
  every other WHEN flag took the space form. `search`'s `--since` and `--until` lacked the
  hyphen-value allowance, so clap read `-1y` as an unknown short flag and exited 2
  (`unexpected argument '-1' found`), while `--since=-1y` worked. Both flags take it now.
  `--turn` already did, and `--max-count` reads a negative number by design.
- `--for` on an address that names no record read as an empty closure.
  `search --for L99999999` exited 0 with `for_of {records:0}` and the ordinary zero-match
  note. "Nothing is for this line" and "this line does not exist" read alike. An address
  that names no record in any transcript of the scope now fails as `show` does, in the same
  words (`no such record(s): L99999999 - ...`). The test reads the file's own bytes, so it
  does not depend on the pattern or the prefilter. A real address whose closure matches
  nothing stays a definitive absence, exit 0.
- Two `wait --help` examples could not run as written. `csift wait @main --until auq` and
  `csift wait @<uuid> --until tool:Read:handover` omitted `--timeout`, which `wait`
  requires, so both failed when pasted. They carry `--timeout 600` now, and a test holds
  every `csift wait` example on the page to the required flag.
- A `show` record row lacked seven keys that the `search` hit for the same record carries.
  A notification pulse fetched by its hit's own `refetch` came back without `task_ids` and
  `orphan_kind`. Every row lacked `delivered`, `is_error`, `denial_kind`, `resume_paired`
  and `scheduled_at`. Both surfaces now build their shared keys through one projector, so a
  refetch answers every key the hit answered. `show --help`'s key list names the seven,
  plus the five draft-distance keys (`superseding_line`, `superseding_uuid`, `diff_chars`,
  `diff_pct`, `diff_exact`) that the row already carried and the list never named.
- `status`'s stopped-agents note printed a raw UTC instant. `N background agent(s)
  were stopped by the user at 2026-06-07T05:10:00` was the one text instant on the surface
  not in the canonical local form. It now renders through the shared timestamp helper, as
  `2026-06-07 15:10:00 AEST(UTC+10)` in that zone, at the second precision the note's
  de-duplication already keyed on.
- The stopped-agents notice on the input queue read as the person's text. Claude Code
  queues the `N background agents were stopped by the user: ...` notice the way it queues
  a typed prompt. csift labelled that queue line `user.queued`, 13 records on one live root
  and 22 on a snapshot, while its delivered twin was already
  `harness.notification.subagent`. The line keeps no key a person's line lacks, so the
  template itself refuses it, through the predicate that refuses the delivered record. It
  takes `harness.bookkeeping.queue` like every other rider (claim QT-005).
- `status` and `wait` answered a multi-session target for one session they picked.
  `csift status <project path>` resolved every session in the directory and reported the
  first by sort order, with nothing saying so. On one live project directory that meant 23
  sessions and a verdict for an idle one nobody named. Both commands now refuse a target
  that resolves to more than one session id, naming the count and the target grammar.
  Copies of one session id still count as one.
- One agent id at both sidecar locations of one session was read in halves. A workflow
  agent whose conversation continued past its run has a second transcript at the built-in
  `subagents/agent-<id>.jsonl`, beside its `subagents/workflows/<run>/` one. The second is
  a continuation and not a copy. `@<id>` read one half and said nothing. A `*.jsonl` path
  naming the built-in file read the workflow file instead. The note `list` printed for the
  pair called it two project directories. Every route now takes one rule. `@<id>` reads
  both halves. A path reads the file it names and names the other on stderr, and two paths
  naming byte-identical copies of one transcript still read as one. The note says the pair
  sits under one session's sidecar, at the built-in and the workflow locations (claim
  PATH-017, PATH-014 refined). `show @<id>` still refuses the pair and names both paths.
- `whoami --help`'s fallback recipe built `@null`. Its second line read
  `parent_session_id` off a `whoami` identity row, and the env form leaves that key null.
  The environment names the top-level session in every lane, so it cannot say which lane
  the caller is. The recipe now reads the parent off the `agents` node, and it says that
  `no subagent matched` means the env id is already the root. A test holds every recipe on
  the page off the identity row's parent.
- `show`'s address miss spelled a range with a dash. `csift show @<id> --line 99999..` on
  a 5,003-line file failed with `no such record(s): L99999-5003`, and `--line` refuses the
  `A-B` form it printed. Both fetch modes now name a missed range in a spelling the parser
  accepts when pasted back. A range that resolved in order uses the grammar's own `A..B`.
  A range that resolved backwards is printed as written. `--line 99999..` misses as
  `L99999.. (file has 5003 lines)`, because the resolved pair `L99999..5003` is a range
  `--line` refuses as reversed.
- `show` booked lines past the end of the file as non-record lines. `--line 4995..5045` on
  a 5,003-line transcript reported `records: 9, non_record_lines: 42`. The 42 lines that do
  not exist were counted with the metadata lines that do. A range is now clamped to the
  file, and the overhang gets its own count, JSON `lines_past_end` beside
  `non_record_lines`. A footer line (a stderr note under `--raw`) names the file's last
  line. The same address now reads `non_record_lines: 0, lines_past_end: 42`. A line or
  range that starts past the end stays a miss, and the miss names the file's line count in
  both fetch modes. `show --help` names the new key.
- `verbatim` let the last span flag win. `csift verbatim @<id> --subagents
  --no-subagents` exited 0 and quietly reconstructed the top-level thread, while every
  other span command refuses the pair as a usage error. `verbatim` now declares the same
  conflict and exits 2 on the pair in either order. `verbatim --help` no longer says
  `--no-subagents` cancels an earlier `--subagents`.
- `stats` counted its scope after the row cap. `csift stats . --max-count 1 --format json`
  headed its stream `sessions_in_scope: 1`, while `csift list . --max-count 1` read 1,713
  for the same target, because `stats` counted the rows it kept. The scope banner and the
  JSON header now count the whole resolved scope before the cap trims the rows, the rule
  `list` already kept. The two commands report one scope for one target, and after the
  change both read 1,722 for `.`.
- Smart-case read the letter in an escape as uppercase. `\Sbash` searched case-sensitively,
  38 records on one transcript (the `(?-i)\Sbash` count), because the `S` of `\S` was taken
  for an uppercase letter. `(?i)\Sbash` found 46. Smart-case now reads the parsed pattern,
  ripgrep's rule. A letter the pattern spells counts, bare or in a bracket class (`[A-Z]`
  stays case-sensitive), and `\S`, `\W`, `\D` and `\B` do not. `\Sbash` now reads 46 and
  `\Wbash` 68. `search --help` states the rule.
- Two help recipes read the envelope's header first. `csift status @<uuid> --format json
  | jq .verdict` on `status --help` printed `null` (the header) before the verdict. `csift
  msg <ID> --format json | jq .fact` on `msg --help` printed `null` around the fact. Both
  recipes now select the row that carries the key (`select(.kind=="verdict").verdict`,
  `select(.kind=="msg").fact`). A test holds every `jq` recipe on every help page to a row
  select, `tail -1`, or `wait`'s single object.
- A malformed session-id variable was read as a session. `CLAUDE_CODE_SESSION_ID=not-a-uuid
  csift whoami` printed `session  not-a-uuid` and exited 0. Claude Code writes the calling
  session's own id there, a uuid on every build, so any other shape was set by something
  else. `whoami`, every `@main` resolution, `plan` with no target, `msg` and `ack` now read
  the variables through one reader that refuses such a value. The refusal names the
  variable, the value and the `@<uuid>` remedy. An unset or blank variable still reads as
  not set. `@main`'s note that it resolved the top-level session via the environment now
  follows the read, so a refused or unset variable prints its refusal alone.
- An empty target token scoped to the current directory. `csift search zzqq ""` searched
  every session of the cwd's project (1,722 of them) and exited 0, because the empty token
  was read as the empty relative path. Sixteen target spots now refuse an empty or
  whitespace-only target at parse time (exit 2), with the target grammar in the message.
  They are the twelve commands that take a path target, and the four channel spots that
  take a lane as a string. There `send "" <text>` had queued the message to the cwd's
  session, and `whoami --to ""` predicted a delivery to it. `msg --lane ""` and
  `ack --lane ""` were already refused, by the @-grammar at run time. An empty `$(...)`
  substitution is the usual source, and the message says so. `search ""` as the PATTERN is
  unchanged.
- The three terminal modes said nothing about a zero-match search. `csift search
  zzqqnothing @<id> -c` printed `0` and no diagnosis, and `-l`, `--count-by` and `--raw`
  were as silent. The ordinary render framed the same absence on stderr as definitive and,
  under a label filter, named the labels the pattern does occur under. Every mode now
  prints that diagnosis on stderr, and stdout is unchanged.
- `agents --agent` refused the `@` sigil. `csift agents @<uuid> --agent @<agent-id>`
  failed with `no subagent matched id`, while every other id position takes the sigil and
  the error's own remedy prints ids beside `@<uuid>`. The flag now takes the id bare or
  `@`-prefixed, and both grab the same node. `agents --help` says so. One sigil is all it
  takes. A token still carrying an `@` after it (`@@<id>`, a `<Name>@<Team>` routing form)
  is refused as no agent id, where it used to be searched for and reported unmatched.
- `search --for L<n>` walked every transcript in scope. A line number names a record in
  one file. `csift search "" --for L42` with no target still closed over line 42 of every
  transcript under the projects root, 1,308,495 output lines on one machine. Over a
  session with subagents it mixed a different record's closure from each file. A line
  address now needs a scope that resolves to exactly one transcript, and it is refused
  with the count otherwise. A uuid address names its record in any scope and is
  unchanged.
- `agents` called a lane completed in the middle of its work. A subagent whose newest
  record was a returned tool_result, with the model still writing its next block, read
  `completed`, and `completed_utc` was set to that record's instant. The end-of-turn check
  had walked back to an earlier turn's text. A running lane was measured that way while
  `status` counted it live in the same minute. The verdict now comes from the newest
  meaningful record, the one the frozen-lane check already reads. An assistant text ending
  its turn is `completed`. A returned tool_result, or a text stamped `stop_reason:
  "tool_use"` whose call is still to come, is `running` with no completion instant. On one
  root 150 of 10,947 lanes move from `completed` to `running`, every one stopped mid-turn
  hours ago, 127 of them by an interrupt. `running` is documented as "still running, or
  interrupted" (claim SUB-034). `agents --help` states the rule.
- `status` counted a finished workflow's agents as in flight forever. On a session whose
  31 workflow runs had all ended, `status` read `1399 lane(s), 145 live (144 workflow
  agent(s) in flight)`. It summed `started` minus `result` over every journal. An agent
  that failed writes `failed` instead, and one killed with its run writes neither. The
  in-flight count now covers only runs with no manifest yet, since the harness writes a
  run's manifest once the run ends, and it counts `failed` as a close. A workflow lane is
  settled by its ended run or its journal's closing line before its tail is read. The same
  session reads `1399 lane(s), 1 live` (claim TAIL-008). An open agent whose own lane is
  already live by its tail is counted once, and not again through its journal's `started`
  line. `agents` reads the same rule before a lane's tail. On that session it had called 42
  workflow lanes `running` that `status` settled, and it now calls none. `agents --help`
  states the rule.
- `agents` called a lane `completed` when its run killed it or its journal failed it. A
  workflow lane settled by its run read `completed`, with a completion instant, whatever
  the run said. A lane of a run whose manifest reads `killed`, and a lane its journal
  closes with `failed`, looked exactly like a lane that returned. The run's word now
  decides. The agent's journal line comes first (`result` means completed, `failed` means
  failed), and otherwise the ended run's manifest status applies verbatim. Such a lane
  reads `killed` or `failed`, carries no `completed_utc` or `duration`, and names why in
  JSON `settled_by`. The text row carries the same words in brackets, as `status` words a
  settled lane. A lane that returned before its run was killed still reads `completed`. On
  the live root 224 of 10,992 lanes move. 120 move to `failed` by their journal line, and
  101 to `killed` and 3 to `failed` by their run's manifest (claim SUB-034).
- `files` joined an unexpanded shell variable onto the cwd. A script writing
  `$FIX/.claude/.../$parent.jsonl` from an unquoted heredoc was reported as `cwd-joined`
  with the path `<cwd>/$FIX/.claude/.../$parent.jsonl`, a file that never existed. An
  operand still carrying `$` or a backtick is now `unresolved` and kept verbatim, whatever
  its shape. Over one root 98 rows move (35 `cd-tracked`, 63 `cwd-joined`), and no other
  row changes class. A `~` operand stays `unresolved`, since the recording shell's HOME is
  not in the transcript (claim BASH-023).
- A fallback-served turn's `served_by` reached no scan. Every `fallback` block on disk is
  the sole block of its record (48 of 48 on one root), so the record had no text and a scan
  emitted nothing for it. `show --line` rendered `served_by`, while `search` over the 12
  sessions that carry a refusal returned 0 hits with it. The record now emits one
  `agent.message.plain` unit on a scan, as under an address, with the marker's own words
  as its body (`served by <to.model>, fell back from <from.model>`). `-t
  agent.message.plain`, a model id or `fell back` now reach it, and the same query returns
  46 hits, one per distinct record. A record that fell back and spoke keeps the marker on
  its text unit and emits no second one. `search --help` says so (claim CLS-060).
- A loop tick fired by the scheduler lost its instant and its edge. A fired prompt whose
  armed text opens with a loop marker takes the wakeup or tick leaf. Those leaves read
  `scheduled_at: null` with only a `parent` edge, although a `scheduled_task_fire` record
  is their parent exactly as it is a plain fired prompt's (3 such prompts on one root). The
  instant and the `fires` edge are now read off the parent link for every leaf a fired
  prompt can carry, which are fire, wakeup and tick. The fire index is built whenever a
  selection reaches one of those leaves, and the label zone prints `[scheduled fire
  <when>]` on them. A tick with no fire record above it carries neither. Neither does any
  other record whose parent is a fire record. Claude Code parents each append to the
  running tail, so a background agent's `<task-notification>` arriving next is such a
  record, and it did not fire (claim CLS-032).
- The `history-suppression` head printed a raw UTC instant. `csift search ''
  <session> -t harness.bookkeeping.suppression` showed `ts=2026-09-03T00:23:01.606Z`, the
  last text instant in the bookkeeping family that was not in csift's local form. A head
  value that is an RFC 3339 instant now renders as the local text every other text instant
  takes, `ts=2026-09-03 10:23:01.606 AEST(UTC+10)` in that zone. The UTC string stays
  matchable in the unit's verbatim JSON body. `cost-state`'s `startTime` is an
  epoch-millisecond number and no instant string, and it prints as written (claim
  QT-049).
- `files --help` named row keys the rows do not carry. Its `op` list stopped at
  `notebook_edit`, although the timeline reports `external_write` (49 rows on one root).
  The timeline schema left out `line` and `detail`. The `--by file` schema called the
  group key `file` where the row carries `path`. The dir and summary views were said to
  group "under a `dir`/`bucket` key", where those are the row kinds and the group sits in
  `path`. The page now lists what the rows carry, and a test holds each view's row keys to
  its schema entry.
- The root `--help` page said every command emits one envelope and three commands write.
  `deliver` prints a hook-output object or nothing, and `wait` prints one bare
  `{kind:"wait"}` object. `recover --out`, `recover --files-from ... --out-dir`,
  `verbatim --out` and `image --out` write wherever the user points them. The page now
  names the two envelope exceptions and lists each user-named output, with what it writes
  and whether it replaces an existing file. A test derives the output flags from the clap
  tree and requires the page to name each.
- `search --help` described a scan wider than the one it runs, and it named the wrong
  gate on three leaves. "With no `-t`, every label is searched" skipped the gated leaves,
  which are the whole `harness.bookkeeping` category and the gated attachment, system and
  turn leaves. The page now says a flagless scan reaches the leaves the tree tags `[scan]`
  or `[scan/needle]`, and that a gated leaf needs `-t` (or `--attachments`). The tree
  tagged `harness.hook.context` as `[-t]`, while `--attachments` admits it. It tagged the
  csift-channel delivery `[scan]` beside a sentence calling it a needle leaf. The `-t` flag
  named three of the 28 leaves a bare face skips. It gave `system`/`local_command` as its
  delivery example, a leaf the model receives anyway. It now names all 28 by group and
  uses a rendered `hook_success`. The EXPOSED KEYS section said five keys render a tag,
  where six do, and it never named `stop_reason` / `stop_details`. The attachment section
  kept a sentence saying the mid-turn steer reads `[payload]`. `harness.context.skill` is
  now described as any injected skill body, beyond a subagent's preloaded one. Tests derive
  every leaf's gate word from the candidate gate's own leaf sets, every skipped leaf from
  the visibility table and every key from the keys family's projector.
- `msg --help` called a csift-channel delivery the one attachment payload a default scan
  parses. A flagless scan reaches three attachment leaves, the channel delivery, the
  person's mid-turn steer and the model's structured answer. The page now says so. It also
  says that `msg` reads the delivery off the envelope header at the start of a
  hook-context attachment and runs no search.
- `whoami --help` said a built-in subagent's environment holds its own id, and that
  whoami takes `@trap:<marker>` or `@main` only. Its long description contradicted its own
  after-help. Current Claude Code hands every lane the top-level session's id and
  withholds the subagent's own. A lane id (`@<agent-id>`) and a teammate's routing form
  (`@<Name>@<Team>`) both resolve as the self target and print that lane's sections. The
  page now states the one reading and names all four forms and the refused session uuid. A
  test holds both.
- `verbatim --help` said csift's own walk stops at a compaction cut, and it named the
  exchange header `s·t<n>`. The survival walk crosses up to `--cross-compactions`
  boundaries (default 3) through each boundary's `logicalParentUuid`. Only budget 0 stops
  where Claude Code's loader does. The turns the walk reaches read `live`, and file order
  still replays the ones above the floor as `pre-cut`. The header token `search` prints is
  `<tok>·t<N>`. A test reads the default off the flag's own constant.
- `agents --help` put the notification pulses under `search -t user` and read a run's
  status off the journal. A `<task-notification>` pulse is `harness.notification.<kind>`
  and never `user`. Its kind set includes `workflow`, the one token the page says the two
  axes share. A run row's `status` is the run manifest's own field
  (`<session>/workflows/<run>.json`, written once the run ends), and the journal carries
  no run status. The page now says both and lists `failed` beside `completed` and
  `killed`.
- `plan --help` said `--reverse` matches `plan_mode` attachments only, and two messages
  said a plan is bound only by entering Plan Mode. The reverse lookup resolves each
  transcript's binding by the same two laws as the forward direction. A session bound by
  its first slug alone, such as a forked or background session, is found too, with
  `binding_source: "slug-only"`. The flag's help now names both laws, and the
  `recover --file @plan` refusal and the `plan` no-binding note name both as absent.
- `verbatim`'s markers never agreed with a count of one. A turn with one tool call printed
  `[1 tool calls]`, and a cut removing one line printed `1 lines elided`, while the fold
  placeholder already wrote `1 agent message`. The kept-turn marker, the elision marker
  and the placeholder's character count now follow the placeholder's rule, each noun
  agreeing with its own count (`[1 tool call]`, `[+1 char, 1 line elided]`). The cost model
  reads the same line the renderer prints. Three lines of the golden baseline moved, two
  `[1 tool calls]` markers and the header's chars-used total, which counts them.
- `verbatim --help` said Claude Code's hook-context cap counts Unicode scalars. It counts
  UTF-16 code units. That is the same number for BMP text, CJK included. An emoji or a CJK
  ext-B character is one scalar and two code units, so a `--window` sized at the cap can
  cross it. The `--slice` and `--window` help now name the two units apart and ask for more
  headroom on a document carrying astral characters.
- A duplicate note said "under 1 project directories". Two copies of one agent id in two
  sessions of one project read `at 2 paths under 1 project directories`. The count now
  takes its noun in the singular at one, like every other count the disclosure prints.
- `plan` bound every subagent to its session's plan file. With no `plan_mode` attachment
  the first slug binds, and a subagent's records carry the session's slug. Every subagent
  transcript therefore resolved to the session's `<slug>.md`. One session with 1,401
  subagents printed 1,402 bindings of that one file, and `plan --reverse` on it listed all
  of them. Claude Code names a subagent's plan `<slug>-agent-<agent id>.md`, with the
  lane's id verbatim (a teammate's name included). The slug law now names that file for a
  subagent transcript, and `binding_source` stays `slug-only`. The reverse lookup of the
  session's plan now finds the session alone. Each of the two subagent plan files on disk
  whose transcript survives now resolves to that subagent.
- A channel message of emoji or rare CJK could arrive as a file pointer. The chunker kept
  each chunk within 9,200 characters. The harness spills an `additionalContext` over 10,000
  UTF-16 code units to disk and shows the model a pointer and a preview without the
  envelope header. An emoji or a CJK extension-B ideograph is one character and two units,
  so a body of 6,000 emoji rendered one chunk of 12,486 units. The budget is now counted in
  code units, and every cut falls between whole characters. That body renders two parts of
  9,199 and 3,335 units, and `send` counts the parts the same way. Bodies of ASCII and BMP
  text chunk exactly as before.
- A zero-match search called `harness.hook.context` reachable only by `-t`. Its inventory
  row still carried the selector-only gate from before `--attachments` covered it. The note
  listing the leaves a run never scanned named it even on an `--attachments` run that had
  parsed its line. A run that named the leaf printed no note, although the 23 other
  selector-only leaves went unscanned. The row now carries the attachment gate the scan
  applies. The note lists 23 leaves and never the hook context, and
  `-t harness.hook.context` alone prints it with `gated_leaves_unreached: true`. The SPEC
  inventory table's row reads `-t or --attachments`.
- An excerpt clipped by one character said `… (+1 chars)`. The explicit elision marker
  that ends every excerpt now names one character in the singular, `… (+1 char)`. That
  covers `search`'s head and match-centered forms, `list`'s previews, `show --turn`,
  `agents --returned-message` and the bookkeeping heads. It uses the same count-and-noun
  rule the `verbatim` markers take. Both marker builders go through one function, so the
  forms cannot drift apart again.
- A zero-match search without `--attachments` said nothing about the attachment lines it
  skipped. A flagless scan reads an attachment line only through the three default
  needles. A pattern whose only match sits in, say, a hook's injected context returned 0
  with no word on stderr about those lines. The zero-match note now also names the
  attachment leaves the scan did not read, and says that `--attachments` or a `-t` naming
  the leaf reaches them, whenever neither was given. The JSON summary carries the list as
  `attachment_leaves_unread`. The list is the shape inventory's attachment axis, derived
  and never typed.
- An empty `--from` read as a sender nobody named. `send <lane> <text> --from ""` queued a
  message from an `external unknown` sender, while an empty lane target is refused.
  `--from` takes `@<lane>` inside Claude Code or a non-empty free label outside it.
  `send` now refuses the empty token at parse time (exit 2) and names that grammar.
- `show --line -1..-3` answered differently in the two fetch modes. A from-end range whose
  ends reverse on the file (the last line, then the third from the end) names no line. The
  rendered fetch refused it with the unparseable-line explanation, while `--raw` printed
  nothing and exited 0. Both modes now resolve every `--line` range the same way and refuse
  an empty one before either reads a line. One message names the range as written and the
  file's length, `no such record(s): L-1..-3 (file has 3 lines)`.
- A draft one character from its resend said `differs ... in 1 chars`. The draft-distance
  line on a `user.unsent` unit (`search` and `show`) formatted its count with a fixed
  plural, outside the rule the excerpt marker takes. It now goes through that rule and
  reads `differs from the sent message in 1 char`. JSON `diff_chars` stays a number.
- `verbatim --help` listed a key the rows do not carry. The JSON schema named `line_no`
  in the per-unit block, while every unit row carries `line`, the jsonl-line key every
  command's rows use. The same blocks left out keys the rows do carry. They missed a
  unit's `source` (`elicitation-sidecar` on a merged pending unit, whose `line` is null),
  the four keys an inbound peer-message unit adds, the id trio, `turn_index` and
  `compactions_before` on a `collapsed_agents` row, and `command` plus the four
  duplicate-copy keys on the header. The blocks now list what the rows carry, and
  `recover --help` names `line` as the key `--at @line:<N>` reads. A test holds every key
  each verbatim row kind emits to the page, and every entry of the per-unit block to the
  rows.
- `image --format json`'s header left out the scope counts. Every other spanning command's
  header carries `sessions_in_scope`, `top_level_sessions` and `subagent_sessions`, built
  by one function. `image` built its header with the scope-less form in both the listing
  and the `--out` extract stream. Both now go through the span builder and count the
  resolved transcript set before any filter narrows the images, so an image header reads
  the same three numbers `list` gives the same scope.
- The teammate hint told a reader that SendMessage takes the routing id. Its third line
  said the official SendMessage needs `Name@Team`, and the JSON `control_hint` said
  SendMessage takes `routing_id`. SendMessage's input validation refuses any `to` carrying
  `@` (`to must be a bare teammate name`). The hint now says what each tool takes.
  SendMessage takes the bare name, or the `a<Name>-<hex>` transcript id, which also resumes
  a teammate that is not running. TaskStop takes the name or `Name@Team`. `agents --help`,
  the root help's `@<Name>@<Team>` sentence and the no-match error of that target said the
  same wrong thing, and they now say the same true one (claim SUB-035).
- `send` printed a SendMessage call that SendMessage refuses. For a teammate receiver the
  delegated call read `SendMessage(to: "Name@Team", ...)`, the routing form, and
  SendMessage's input validation refuses any `to` carrying `@`. `send` and `whoami --to`
  now print the receiver's transcript id, `SendMessage(to: "a<Name>-<hex>", ...)`, the
  form they already printed for every other agent lane. SendMessage's agent-id arm
  delivers it to the running teammate whose resumable id it is, or resumes that teammate,
  which is what the resume row delegates. The receiver line still names the routing form
  beside the lane. The outbox's `official.to_form` records the id the call was printed with
  (claims SUB-035, MSG-037).
- `recover --file <basename>` stitched several files into one. The basename-suffix
  fallback matched every record path the argument ended. A basename that three files share
  therefore replayed all three files' writes and edits into one buffer. On a fixture of
  three `lib.rs` files, the default restore printed one of them and called it complete,
  and `--coverage` counted all six events. A target that matches more than one distinct
  file in scope is now refused before anything is replayed, in every mode. The refusal
  names each candidate and the count. A relative record path counts as the file its
  record's shell cwd resolves it to, and a tool result's `filePath` resolves through the
  cwd of the call it answers. `other/lib.rs` beside `/p/m/lib.rs` is therefore refused too.
  A file-history snapshot key is relative and its line carries no cwd. It resolves against
  the transcript's first cwd, the directory it is relative to, and so names the same file
  as the absolute Write/Edit path it is the tail of, never a second one. That join is the
  candidate a refusal prints for it. In a session that changed directory before its first
  write, that candidate can be a path no record wrote, and `recover --help` says so. A
  candidate, or a longer suffix that names exactly one file, works as before. An absolute
  target now also finds a relative record path that resolves to it. Every candidate the
  refusal prints replays when passed back. One that the other candidates end with (a
  relative path from a transcript with no cwd) replays that file alone, with a note. A
  `--files-from` line takes the same rule. Its report row reads `ambiguous`, nothing is
  written for it, and the summary line counts it.
- `verbatim`'s compaction banner said the turns below it predate the summary. The banner
  sits where the walk, going down the file, passes from turns that end before the
  summary's line to turns that end after it. The turns below it therefore mostly follow
  the summary. On a fixture whose summary is L5, the banner sat between L2 and L3, and
  every line below it except the L3 opener came after L5. It now reads `══ compaction
  boundary · summary at L5 · (turns above end before it, turns below end after it) ══`. A
  turn below can still open before the summary when the compaction lands mid-turn. The
  banner names where turns end and makes no claim about when they began. The golden
  reconstruction moves on its three banners and on the header's `chars used`, which counts
  them.
- `verbatim --budget 1` said `budget 1 chars`. The scope line printed `budget 1 chars is
  PER session → up to 1 chars total`, and the per-session header printed `budget N chars`
  with a fixed plural. Both budget figures, and the header's reserved worst case, now go
  through the count-and-noun rule every other csift count takes. The line reads `budget 1
  char is PER session → up to 1 char total`, and `budget 2 chars` stays as before.
- A shell result quoting the AskUserQuestion answer text no longer opens a turn. The
  answer's marker fallback, for a carrier with no `toolUseResult`, read the text alone. A
  `tool_result` whose content opened with `Your questions have been answered:` was
  labelled `user.answer` and opened a turn, and a `strings` run over the harness binary
  prints exactly that. The fallback now answers only when that result's `tool_use_id` names
  an AskUserQuestion call of the same transcript. Such a shell result keeps
  `agent.tool.result`, and every surface that numbers turns (`search`, `show`, `stats`,
  `verbatim`, `files`, `recover`, `image`) counts one turn fewer on its transcript.
  `files` and `recover`, whose prefilters keep file traffic, now also parse the line
  carrying the AskUserQuestion call. A genuine marker-only answer therefore opens its turn
  there as it does on every other surface. A surface that reads no whole transcript (the
  `list` preview, the live tail) renders an answer from its structured fields only.
- `verbatim --format json` names itself in its header. The header line read
  `"command":"turns"`, the command's name before its rename, while every other command's
  header names that command. It reads `"command":"verbatim"` now.
- `[not delivered]` prints wherever the JSON `delivered` reads false. The text tag followed
  only the per-record delivery override. A record the model never receives by its leaf
  read `delivered: false` in JSON and carried no tag on `search` or `show`. That covers the
  turn closers, a compaction boundary, a scheduler event, a kill, and a queued or recalled
  message. The bookkeeping family printed its own copy of the tag to cover its leaves. One
  verdict now drives both surfaces. Every undelivered unit is tagged once, and a
  bookkeeping line's label is unchanged. `search --help` states the rule.
- `search -c`, `-l` and `--count-by` now name a skipped malformed line. A torn line was
  never matched, so a count, an id list or a census over a scope holding one covers the
  parseable lines only, and those three modes said nothing about it. Each now prints
  `csift: note: N malformed line(s) skipped` on stderr, as `--raw` does, and the
  `-c --format json` summary carries `skipped_lines`. On a zero match the note stands down
  in every terminal mode. The zero-match caveat already names the line and what it means
  for the absence, and one mention is enough.
- `deliver --recipe` says where its block goes. The block's top level is
  `{"hooks": {...}}`, and the note said to paste it into the `hooks` object of a
  settings.json, which nests `hooks.hooks`. The note now says to merge it at the file's top
  level. The block's `hooks` key merges with the file's own, and each event's array is
  appended to the one already there, never replacing it. `deliver --help` names the
  fragment the same way.
- The documents now say which `show --raw` forms disclose a compaction crossing. SPEC and
  the maintenance manual listed `show --raw` among the modes that print the crossing report
  on stderr. Only a `--turn` window does, as `show --help` already said. A `--line` or
  `--uuid` raw fetch builds no chain, since its bytes are the same under every budget. It
  prints nothing about crossing on either stream. No output moved.
- `verbatim`'s skip note names the budget that renders. A skipped session's `needs ≥ N
  chars` added the header rendered at the budget the run was given. That header prints the
  budget, so N moved with the budget's digits and its plural. On one fixture `--budget 1`
  said 392 and `--budget 100` said 399, the budget that renders. N is now the least budget
  that fits the header rendered at N itself, the same at every skipped budget.
- `recover`'s several-files refusal now says where it came from and what it prevents. A
  `--files-from` line's note named it `--file`. The batch summary counted an unreplayed
  ambiguous line among the recovered files. The refusal named only the stitching outcome,
  though candidates in different transcripts would replay one file's history and drop the
  others'. It also listed every candidate, one line each, however many there were. The
  note now names the manifest line, and the summary counts it beside the recovered files.
  The refusal names both outcomes and lists the first ten candidates with a count of the
  rest.
- `recover --file` with one spelling of a file read only that spelling's events. A
  relative record path joined to its cwd was normalised, and every other spelling was
  compared as a string. After a write to `./a/lib.rs` and an edit through `a/lib.rs`,
  `--file ./a/lib.rs` printed the first write's content and called it complete. `/p//q/lib.rs`
  beside `/p/q/lib.rs` was refused as two files. Paths now compare lexically, the target
  and every record path alike. `.` segments are dropped, and a run of `/` collapses to one
  (a leading `//` stays). Any spelling therefore replays the file's whole history. A `..`
  segment and a symlink's other spelling are not resolved. Those stay two files, and
  `recover --help` says what each spelling then replays.
- The `agents` teammate hint no longer offers SendMessage's refusal as the reason TaskStop
  takes the routing id. Its third line now states the two tools' facts separately. The
  routing id `Name@Team` is a form TaskStop takes. SendMessage refuses any `to` carrying
  `@` and takes the bare name or the transcript id. `agents --help`'s paragraph on a
  teammate's two ids says the same, a form TaskStop takes, beside the bare name.
- `whoami`'s environment form no longer says a lane has no parent. With no target the lane
  comes from `$CLAUDE_CODE_SESSION_ID`, which names the top-level session in every lane.
  The `parent` line read `none (a top-level session has no lane above it)`, while the JSON
  parent was null and the depth line said unknown. Read from a subagent, the line was
  false. It now reads `unknown from the environment alone (a top-level session has none; a
  subagent's parent is this session)`. A target that resolves the lane keeps its parent.
- A zero match under `--turn` blamed the label filter for what the window left out. An
  abandoned record, such as a recalled draft, a rewound turn or a record on an abandoned
  branch, belongs to no numbered turn, so a `--turn` window leaves it out. The diagnosis
  probed only `-t`. `-t user.unsent --turn ..` over a session with one draft said the
  pattern occurs under other labels and the label filter excluded them. The diagnosis now
  also counts the records the pattern and label filter match outside turn numbering. When
  there are any, it says so on a line printed first and names both ways back (drop
  `--turn`, or window by time). The JSON summary carries the count as
  `outside_turn_numbering`. `search --help` says so.
- `show`'s cap continuation dropped the records after an abandoned one, and it fetched
  records nobody addressed. The rendered fetch kept its first `cap` units in render order,
  which puts an abandoned unit after every live one, and it read the continuation off that
  order. A window with an abandoned record printed a continuation that stopped at that
  record or ran backwards. On one transcript `--line 1..400` continued with
  `--line 201..393` and lost L394-L400. The cap now keeps the records on the lowest lines,
  whole records. The continuation names the units it dropped by their lines, with touching
  ranges merged. `--line 1..4,9..14` cut at L3 continues as `--line 3..4,9..14`, where one
  range from L3 would also have fetched L5-L8. A dropped record merged from the
  elicitation sidecar has no line and is named by `--uuid`. Every address form continues
  this way, `--turn` included, so a turn wider than the cap pages forward and the footer
  always says which records the page left out. A wide window pays for that in length.
  `--turn ..` on a 397 MiB transcript continues with 17,534 line pieces. Following the
  continuations at one cap until none is printed yields every unit of the uncapped fetch
  exactly once. The footer now says `--max-count 0` returns the whole fetch. `show --raw`'s
  stderr continuation follows the same rule, and `show --help` says so under
  `--max-count`.
- `show --raw` by uuid fetches the records the rendered fetch does. A uuid that a replay
  copy shares with the line that survives it fetched both lines rendered and only the copy
  raw. It now prints both lines, lowest first. The uuid of a record merged from the
  elicitation sidecar exited 1 with `no such record(s)` under `--raw`, while the rendered
  fetch showed it. It now prints nothing for that record plus a stderr note saying the
  record has no transcript line, and exits 0, as `search --raw` does. A uuid that names no
  record stays a miss. `show --help` says so under RAW MODE.
- A quoted relay framing no longer emits an inbox unit. Two records each emitted an
  `agent.communication.inbox` unit whose `label` was not among the record's own `labels`.
  One was an assistant text opening with `Another Claude session sent a message:` and a
  quoted `<agent-message>`. The other was a compaction summary carrying a peer section.
  The per-section units now come only from the records the classification reads sections
  on, a user record that is neither an elicitation marker nor a compaction summary.
- A refusal typed over several lines now prints its hit on one line. The
  `[feedback: ...]` tag kept the newlines inside the first 60 characters of the typed
  text, so the rest of the hit continued on new lines. The tag now flattens the text before
  it cuts it, and JSON `user_feedback` still carries the words whole.
- A resumed workflow run's live lanes no longer read as killed. A run resumed under its
  own id keeps running under the manifest its killed predecessor wrote, until it ends.
  That manifest settled every lane with no closing journal line, so `agents` read those
  lanes `killed` and `status` counted none live. A manifest now settles a lane only when
  the lane's newest record is no more than a minute newer than the manifest's own end
  instant. The kill's own writes land within 66 ms of it, and a resumed run's land hours
  later. A newer lane is read from its tail, and its row names the stale manifest. The row
  says the lane wrote after its run's end and names a resume as the likely cause.
- `show --help` said a compaction boundary fetched without its summary reports no mode.
  The pairing is read off the whole file before any address narrows the output, so a
  boundary fetched alone reports its mode. The help now says so, and says that a boundary
  reads null only when the file itself leaves it unpaired.
- `status` printed no `last ◂` when the newest prompt was a slash command with arguments.
  The row took the newest turn-opening record, and a slash command typed with arguments is
  the person's words without opening a turn. It now takes the newest record labelled
  `user.message`, `user.answer` or `user.rejection`, turn-opening or not, in the text
  `search` renders. That is `/name args` for the slash command, and a relayed or queued
  message's own body. A `<task-notification>` pulse is not the person's words and no longer
  takes the row.
- A fork child's first carrier emitted an inbox unit that named no call. A subagent return
  carrier holding text beside its result emits two `agent.communication.inbox` units. The
  text one read `pairing` null, with no tool name and no `result_of` edge, while the result
  one read the call. The text unit now carries the carrier's call, so both report one
  pairing, one tool name and one edge. They read `orphan` and `not in this file` when the
  call is on no line of the file.
- A spawn prompt that names a fork record is still the lane's seed. `search` took a lane
  for a `/fork` clone when the literal `fork-context-ref` occurred anywhere in the first
  4096 bytes of its line 1. A subagent's line 1 is its spawn prompt, so a prompt about
  forks read as a clone. Its seed classified as `user.message` and not as
  `agent.communication.inbox`, lost its `parent ⇨ self` direction, and joined every
  spanning count of the person's words. Line 1's type decides now. It is read as the quoted
  value `"fork-context-ref"` that the candidate gate already uses, which a quote-escaping
  JSON string cannot carry.
- A closed pipe shared by stdout and stderr no longer aborts a reporting surface. Under
  `csift ... 2>&1 | head -c 1`, the failed stdout write reached the error arm, whose
  `eprintln!` failed on the same pipe and panicked. Every surface that reports a closed
  stdout exited 1 with stderr open and 101 under `2>&1`. The error arm and every stderr
  line those surfaces print now go through one helper that drops a write it cannot make,
  so they exit 1 either way. A `println!` surface still exits 101 on its own stdout. The
  same helper keeps a note that cannot land, after a stream that did, from turning the run
  into an abort. Such a note is a `--raw` cap drop, a zero-match diagnosis or `verbatim`'s
  no-compaction note, and the run exits 0 with its whole stream. The root `--help`'s
  exit-code paragraph says so.
- `search --raw` says what the survival walk crossed. A raw scan builds the chain, because
  the walk decides which records are hits, so what it prints moves with
  `--cross-compactions`. It was still the one scan mode that said nothing about the
  crossing on either stream. It now prints the same one or two `(compaction crossing ...)`
  lines on stderr after its stream, beside its sidecar, cap and malformed notes, the
  stream `--count-by` already uses. Stdout stays the matched lines alone, and at budget 0
  nothing is printed. A raw stream has no envelope, so no JSON key is added.
  `search --help` says so under `--raw`.
- An env note names the variable that answered. With `CLAUDE_CODE_SESSION_ID` unset and
  the Codex companion's `CODEX_COMPANION_SESSION_ID` set, `whoami`, `@main` and the `msg` /
  `ack` lane resolve from the alias as documented. Their stderr notes still said the id
  came from `$CLAUDE_CODE_SESSION_ID`, a variable that was empty. Each note now names the
  one that carried the id, and `@main`'s note now names a variable at all. `whoami --help`
  says so.
- `verbatim` renders a pending sidecar question as the assistant's. A pending
  AskUserQuestion or ExitPlanMode merged from the elicitation sidecar was emitted as
  `▽ (elicitation sidecar)  USER` with JSON `role: "user"`. The assistant's question read
  as words the person said. A hook that re-injects `verbatim`'s reconstruction after a
  compaction carried that mistake into the model's context. The pending line stands for
  the assistant record Claude Code writes once the question resolves, so the unit now sits
  on the turn's assistant side, as `△ (elicitation sidecar)  ASSISTANT` with
  `role: "assistant"`. The body still names the tool and the question. The header's
  user/assistant unit split counts it as an assistant unit. `verbatim --help` says so.

### Performance

- The render was the other serial half of a search. Everything the scan does in parallel
  was spent before the first output byte. The test session was 1.18 GB, counting the
  top-level transcript plus every `*.jsonl` under its subagent tree except
  `journal.jsonl`, which is what csift parses. On it a whole-scope `--format json` dump
  cost 1.94 s of CPU on one thread while nine rayon workers waited. A wide-match text
  search (`search 'the'`) paid 0.45 s of that phase's 0.59 s inside `println!`, which makes
  one `write(2)` and one backward newline scan per row, 297,976 of them. Two changes
  address it. Every row-dumping surface now writes through one buffered stdout, one
  syscall per megabyte. Those surfaces are `search` text, JSON and `--raw`, both of
  `show`'s, `files`, `agents`, `image`, and both of `verbatim`'s, whose documented
  `--subagents` span is 125,269 text rows on that session. `search` also renders its
  per-exchange rows in bounded parallel chunks and writes the chunk buffers in index order,
  so the byte stream is the one a serial loop produced. The medians below are of 7
  interleaved reps on that session, at a host load of 7 to 27 that the wall numbers carry
  and the CPU numbers do not.

  | query | before (s) | after (s) |
  |---|---|---|
  | whole-scope JSON dump | 2.958 | 1.532 |
  | `search 'x'` | 1.247 | 1.040 |
  | `search 'the'` (now at its own `-c` floor) | 1.140 | 0.931 |
  | a `--for` closure | 1.080 | 0.901 |
  | `show --branch-points` | 0.397 | 0.355 |
  | the `agents` tree | 0.664 | 0.619 |
  | `files --by timeline` | 0.890 | 0.858 |

  CPU for the two wide text rows falls about 10%, with their sys time halving from 1.10 to
  0.70 and from 1.07 to 0.68. The JSON dump's CPU rises 3.9%, which is what the fan-out
  costs to spread over ten cores. The look-ahead can hold up to twice 128 rendered rows in
  memory. 128 rows are rendered before any is written, and each chunk buffer grows by
  doubling, so it can sit at twice the bytes it holds. On that session, with rows averaging
  78 KB, that is 10 to 20 MB against a peak of about 1,570 MiB, and it does not clear the
  noise. Medians of 13 interleaved reps differ by 0.1 MiB, against per-arm spreads of 12
  and 17 MiB. On a synthetic transcript whose rendered rows are 4,195,818 B, three runs of
  five interleaved pairs read medians of 715.8, 844.2 and 723.7 MiB. The widest single
  observation was 929.3 and the narrowest 522.3, against a one-copy figure of 512.2 MiB.
  The peak is not a floor on the bound, and one run is not a property of the pipeline. A
  reader sizing a run should work from twice 128 rows. The smallest inputs stay inside the
  same-binary control band, at a median ratio of 1.009 against the control's 0.9997.

  Output bytes do not move. The acid set has 169 (row, size class) pairs over three size
  classes. 165 of them are compared on all four streams, which are stdout, stderr, the exit
  code, and an interleaved `2>&1` capture, the only one that can see the order between the
  first two. 163 are byte-identical on every stream, and stderr and the exit code are
  identical on all 165. The two that differ are one binding in its two size classes,
  `csift --help`. Its page grew by the 512 bytes of the clause the exit law gained this
  release, a documented surface change and the only stdout difference anywhere in the set.
  Four more pairs are excluded because they print live state, and both bindings are named.
  The `whoami --format json` rows carry lane instants that move when the session they name
  gains a record. The `status` rows carry an elapsed-seconds field on their fourth stdout
  line that advances by the gap between the two captures, so a repeat of the same binary
  differs from itself by exactly however long it waited. All four are run anyway, under the
  condition that makes their comparison mean anything, with the two arms of a pair landing
  inside one second. Whether a given pair then matches depends on when it ran, which is
  exactly why it is not counted among the 165. One limit is stated plainly. A render change
  can never reach a heavy regex row, which reads 1.189 s against a 1.140 s floor with the
  render removed entirely. Its output is 2.5 MB, and its clock is the regex over that
  1.18 GB.
- A record's width is now a number, and it is written down. The amendment gave `Record` a
  second conversion key and a memo holding both parsed conversions. The memo held its
  payload inline, 56 bytes on a field almost no record carries, paid by every parsed line of
  every scan. It now sits behind a pointer, 16 bytes whatever the pair grows to and no
  allocation at all for a record carrying neither key. `size_of::<Record>()` reads 1,368,
  where the inline form measured 1,408 and the pre-amendment record measured 1,336. The
  amendment costs 32 bytes instead of 72. A unit test pins that exact number beside the
  spine row's 72, because widening the record is worth doing deliberately, and nothing else
  in the crate notices when it happens.
- An attachment payload is parsed once per record. Four layers ask one attachment record
  about its payload, and each ask used to re-parse bytes that can hold a whole file
  snapshot.
- The nine provenance relations cost nothing a same-binary band can see. The measurement
  ran on an idle host, the tip that carries them against the tip they were built from, over
  31 queries in three transcript sizes at 21 repetitions per cell. 90 of 93 rows sit inside
  the control band. The three outside are slower by 0.4, 27 and 51 milliseconds. The
  whole-scope JSON dump of the 397 MiB transcript reads 1.0009, and the `for-kind` census
  reads 1.0034 while printing 85 percent more. SPEC section 7i carries the rows.
- `whoami` reads its own session's lanes once. The topology section read the calling
  session's child lanes for its own line. The count of other live lanes then read every one
  of them again for the same session. The census now takes the rows already read for the
  registry row whose transcript is that file. On a session holding 1,401 subagent
  transcripts, `whoami` went from 1.339 s to 1.069 s (hyperfine, 5 runs after a warm-up).
  Its text and JSON are byte-identical to before, on a two-session fixture and on the live
  root.

### Removed

- `harness.meta` and every `harness.meta.*` selector are removed. They are hard errors
  naming their successors, one error per retired spelling, and there is no alias, no
  forwarding arm and no twin in `labels[]`. The same goes for
  `harness.schedule.continuation`, whose producer is the resume loader
  (`harness.resume.prompt`). The 0.6-era flat `thinking`, `tool` and `tool-response` were
  already errors and now point at the same three successors.
- `--additional-context` is removed. The flag gated one leaf, and that leaf now has a name,
  `-t harness.hook.context`. Naming any attachment-carried leaf is a gate of its own, so the
  narrow ask needs no flag, and `--attachments` keeps its wide meaning. The flag is a hidden
  tombstone that bails with the successor. It deliberately does not become a quiet no-op. A
  run that silently scanned fewer lines than the caller asked for would read as the
  definitive absence an empty result promises.

- The introspection ledger moves to schema 2, with version-ranged states, one receipt each,
  and the prose law as a gate. `INTROSPECTION.json` had grown a `checks[]` history, one
  appended record per release audit, and the history became the file. It held 1,856 checks
  behind 739 claims, 1,117 of them superseded by a later check on the same claim. Every one
  carried the calendar dates, csift versions and audit-process words the ledger's own prose
  law bars. A fact record does not need its own minutes. A claim now carries its `code[]`
  sites at claim level, because csift's tree is one tree at any time, and where a fact is
  relied on is never a property of a Claude Code version range. It also carries one or two
  `states[]`, exactly one of them open. A state is a version range (`from`, and `to` = null
  while it is open). It holds the claim text for that range, the two attribution legs, the
  provenance and one `verified` receipt. `located` and `first_seen_claude_code` retire into
  the first state's `from`, and the per-check dates and csift versions retire outright.
  Each state also gains a `from_basis`, one word saying whether its floor was measured,
  bounded or defaulted. It gains a `probe` too, the machine-readable twin of `instrument`,
  which names the half a stranger re-runs first as one of bytes, corpus, csift, live or
  repo.

  A receipt is the last check at the badge's build whose instrument names an artefact
  outside csift's own tree. That artefact can be a build, a byte offset in one, a corpus, a
  registry directory or a live trial. The artefact is the test and never the command.
  `grep` over `src` is the same text pass as reading the file, while `grep -ab` over a
  shipped build is a measurement, and a text pass can never back the badge.

  The gate is rewritten to read all of it and grows from twelve rules to twenty-two, each
  failing by name on a hand-built specimen. Rule 13 is the state shape, including that the
  open state comes last. Rule 14 is the prose law over every field a pass wrote. It is now
  word-bounded and first-person-aware, and its backtick exemption is bounded. Rule 10
  reaches `residue` and a receipt's `note`, which is where every unconsumed text
  instruction was hiding. Rules 15 to 20 cover the state's own text and probe, a closed
  state's receipt, and a retired selector handed to csift as an argument. The bare `tool`
  is among those selectors, and the valid `--count-by tool` census axis is spared by
  position and not by word. The same rules cover the file's own shape, every heredoc handed
  back to its own language to parse, and the receipt's artefact itself. Rule 21 checks the
  `bytes` probe's own literal. It reads the literal in the builds its state names, and in a
  closed state's builds too wherever this host holds them, because four of those literals
  counted zero in every build and a probe that counts nothing cannot be run.

  Rule 22 holds a probe's kind and its scope to the same artefact. The kind follows what the
  command reads, and the scope is derived from the instrument, and nothing compared the two.
  Thirty-eight states shipped claiming `corpus` beside a scope naming the shipped binary,
  and one claimed `bytes` beside a scope naming a projects root. Only the two kinds whose
  artefact is fixed are tested. `csift`, `live` and `repo` name a run, a gesture and this
  tree, and any of the three may mention either noun without contradiction. The rule's
  corpus word list learns the harness session registry under its English names, since a row
  set on disk had been reachable only as a path. The rule stops at the scope and never
  reads the command. A scope is a noun phrase that a word list decides. A program classified
  by the words in it has the same false-negative shape the pair had.

  Rule 12 learns that a hop can name the build it was read in. A build nobody holds is not a
  pass, so such a hop is named and not counted wherever the verified build itself is
  present. The rule refuses a run of four or more dots the way it refuses an empty excerpt.
  It holds an elided excerpt to the run before its first ellipsis, at its own offset. That
  bucket used to be compared against nothing, so replacing an already-elided excerpt with
  fabricated text moved no figure. It refuses an excerpt that reads as prose whether or not
  the hop wears a mark, since English occurs in no build and satisfies `absent` by
  construction. It decides a marked hop by its mark before the eliding, so the hop lands in
  one of the five counts and never in two. It compares bytes and no longer a UTF-8 round
  trip. How many `...`-bearing excerpts are byte-exact anyway is derived by the walk and
  carried on the README, and typed nowhere. Rule 8 no longer matches rows with a regex. It
  regenerates the README block and compares it byte for byte.

  A folded check is not simply dropped. Every fact token with no survivor in the state is
  carried into `residue`, and each carried entry names what superseded it. Some carried
  entries had already been cut short in the source. Such an entry is repaired where the same
  state spells the whole word, and trimmed back to the last complete item where it does not.
  It is completed from the claim's own schema-1 text where that text says what came next. A
  cut landing on a word boundary, where every token is spelled correctly and the sentence
  stops anyway, is found by its length, using the four truncation constants the source
  itself writes at. The prose rewrite masks every quoted run, literal, offset, version and
  whitespace run before it fires and restores each byte for byte. It therefore cannot cut a
  comma out of a jq argument list, normalise the double space out of an asctime literal, or
  flatten the indent a python heredoc's block structure lives in. A date that is the object
  of an epoch stays.

## [0.12.3] - 2026-09-13

### Added

- Every `search` hit and `show` record gains `body`, a newline-preserving field under
  `--format json`. A hit's `excerpt` is a match-centered fragment and is one line by
  contract, so it collapses every newline to a space. A markdown table arrived as one run of
  `|` characters. A reader who took `excerpt` for the message lost every paragraph
  boundary, even under `--no-truncate` where nothing was clipped. `body` carries the same
  rendered section text before that collapse. It is non-null exactly where the excerpt cap
  is lifted, which is `search --no-truncate` and `show`'s `--line`/`--uuid` address. Under
  the default cap it is null. The excerpt there is a fragment anyway, and a second full copy
  of every matched record would multiply a wide scan's output for a consumer that did not
  ask for it. `show --turn` is a window and not an address, so it keeps the cap and reports
  null too. `excerpt` and `show`'s `text` keep their exact bytes in every mode.

- `verbatim` folds a same-prefix re-send. Sometimes one turn carries two assistant
  messages, and the later one's body contains the earlier one's whole body, a re-send with
  an addendum. The text render now prints the earlier one as `△ L<n>  [superseded by the
  same-prefix re-send at L<m>, N chars]` in place of a second copy of the prose. The JSON row
  keeps the full text with `superseded_by_line` set, so a machine reader loses nothing.
  Containment is the predicate, and that choice is measured. The corpus was every top-level
  transcript of one projects root, 80 files, 6,468 turns and 35,052 assistant messages. In
  it, 53 same-turn pairs shared their first 80 normalized chars. Only 1 was a true
  duplicate, 25 diverged before char 120, and on 22 the later member was shorter. A fold
  keyed on the 80-char prefix alone would have dropped prose the survivor never said. Under
  containment the same corpus yields 2 folds. The name follows the same discipline (claim
  MSG-095). The shape that motivated the rule is a Stop hook exiting 2. None of that
  corpus's 36 `Stop hook feedback:` records produced a same-prefix pair. Their neighbouring
  assistant messages share 0 to 4 normalized chars, and 0 of the 30 same-turn pairs reach
  20. No same-prefix pair has one between its members. The marker therefore names what it
  matched and no producer it cannot attribute.
- `verbatim`'s fold marker says what it folded. A collapsed run of agent messages rendered
  `[X agent message(s), Y tool call(s)]`, counts that could not tell a folded finding from a
  folded "let me look into this" without fetching the range. The marker is now
  `[X agent message(s) collapsed, N chars, Y tool call(s)]`. N is the summed `full_chars` of
  the collapsed bodies, so the line says how much prose it stands for. Every collapsed
  message of at least 210 chars is followed by an indented `L<n>  <first 60 chars>… (+K
  chars)` preview line. 210 sits above the pure-declaration band, so a preview means "this
  was not a declaration". JSON `collapsed_agents` rows gain `collapsed_chars` and
  `collapsed_previews[]`. The budget charges exactly the lines emitted, because the renderer
  and the cost model both walk one `agent_placeholder_lines` list. The cost was measured on
  the installed hook's path (`--slices 4 --window 9000`) on the two largest top-level
  transcripts of one corpus. The whole four-slice document carried 2 and 3 fold markers and
  0 preview lines, since every folded body there is under the threshold. The `N chars`
  clause added +41 / +61 chars to a ~24,700-char document. Over a whole session at an
  unbounded budget, 42 of 1,224 and 212 of 1,759 folded messages reach the threshold.

- `list` names the child a background handoff continued the session in. A session handed
  to a background child, by the left-arrow gesture or the background-fork command, leaves
  one line in the parent transcript, `{type:"continued-in", timestamp, sessionId,
  continuedInSessionId}`. It has four keys and no `uuid`, so it is not a chain node, and its
  only address is its jsonl line. This is the third lineage link on a `list` row and the
  only one that is not an inference. `clone_of` reads a boundary shape and `cleared_from` a
  checkpoint sum, while here Claude Code writes the child id itself. Text gains a `handoff`
  row naming the child's first-8 token, and JSON gains `continued_in`. The docs carry two
  limits. An in-place `/fork` reaches the same spawner with `keepParent` and writes no such
  line, so absence never means "not forked". The append also runs on the failed-spawn path
  once a rescue row is queued, so presence does not prove the child ran. `list` reads
  head/tail windows only. A line that a later `--resume` of the parent pushed above the tail
  window reads null, and `stats`' line-type census (which already counts `continued-in`) is
  the whole-file answer. (claim REC-106)

- `list` reports the background-lane stamp, and `--lineage` gives its span. Every record a
  background lane writes carries a top-level `sessionKind:"bg"`. A foreground lane's records
  carry no such key at all. One stamp builds the value from one environment variable, whose
  value is undefined in a foreground lane, and the serializer drops an undefined property.
  That makes it a per-process fact. `session_kind` is therefore an array, and it is empty
  for an ordinary session in place of carrying a sentinel. One transcript can hold a
  background lane's records above the records that a foreground resume of the same session
  appended without the key. The values come from the head/tail windows, which is also where
  Claude Code's own session summary reads them (off the first record of the file). The span
  cannot come from the windows, because the stamp stops mid-file and no window can see
  where. The new `--lineage` flag runs one whole-file pass for `session_kind_first_line` /
  `session_kind_last_line` and an exact `continued_in`. `lineage_scanned` separates "not
  asked for" from "no carrier". That pass is a depth-1 key walk and never a record parse, so
  a nested key is not a field, and the pass adds nothing to `skipped_lines`. Measured on a
  720 MB transcript it costs 2.1x the same-binary control band, which is why it is opt-in.
  The default path stays inside that band. (claim REC-107)

- `plan --audit` qualifies each binding with four facts its own records carry. The audit
  said which plan file a session binds. These facts say how much that binding is worth, each
  as a text block and a JSON field, and each reads a fact on disk and derives none.
  (a) The slug's change points. `none -> <value>` is the mint, and `<a> -> <b>` reads as the
  binding having moved (`slug_changes`, `first_slug_line`, `first_slug_utc`,
  `first_slug_local`). The binding csift reports is the harness's, and this fact says when
  it became so, which is the question a fork raises. Over 9,601 transcripts, the 7,810 that
  carry a slug each have exactly one distinct value and exactly one absent-to-value
  transition, so the corpus has never shown two change points. (b) Whether the bound file
  is on disk, as `[exists]`/`[missing]` and `plan_exists`, the same field the forward view
  prints. (c) A `plan_file_reference` attachment present while no record carries a slug.
  That is plan text with nothing bound to it, which nothing will re-inject. It gets a
  `plan-unbound-text` row, a warning, and a count in `unbound_plan_text`. The corpus holds
  zero specimens. The check stays because the fork path that strips a slug can produce the
  shape. (d) The first slug-carrying record against the plan file's birth instant, read as
  `before`, `after`, `same`, or `unknown` with the reason named, and never a guessed
  direction. (claim PLAN-022)

### Changed

- `verbatim`'s docs say which path the 600/900 per-role caps apply to. This is
  documentation only, and no cap moved. The caps belong to the direct path, which is a plain
  run, `--out`, and the legacy budget-driven `--slice i` alone. `--slices N`, the
  fixed-fleet hook-injection path, drops them for a window cap. It holds its slice count by
  discarding the oldest turns that do not fit. The same session read two ways therefore
  differs on purpose. The direct path keeps more turns, each cut to 600/900, and the fleet
  path keeps fewer turns whole. Only the `--slices` flag doc had said so. The direct cap's
  cost was measured over the last 40 turns of the two largest top-level transcripts of one
  corpus, counting assistant units. The 900-char cap keeps 32,404 of 59,965 chars (27 of 42
  units cut) and 67,846 of 117,878 (48 of 101 cut). At 1,800 those would be 50,657 (12 cut)
  and 95,262 (18 cut), that is 18,253 and 27,416 more chars kept. That is recorded as a
  number and not acted on. The `--agent-msgs` help text also drops a stale claim that the
  default is `eot-only`. It has been `longest` since that mode shipped.

### Fixed

- `verbatim`'s `L lines elided` note counts the cut and no longer the message. The figure
  was the whole body's newline count. Deleting 9,807 chars and deleting 1,907 chars of one
  message printed the same number, and a cut of 17 chars could claim 95 elided lines. On
  two large sessions, 18 of the 2,480 truncated units reported more lines than they removed
  characters, which is arithmetically impossible. A unit now carries its original body's
  newlines as offsets into the one-line text it renders. The note counts the newlines
  strictly inside the removed span. An uncut body reports 0, a cut that falls between two
  newlines reports 0, and a wider cut of the same message reports more. Text
  `, N lines elided` and JSON `elided_lines` follow. A fabricated body, such as an
  AskUserQuestion scaffold, an automation attribution label or a peer-message preview,
  reports no lines at all. No original newline can be located in a string the record does
  not carry. The old figure was measured against a different string.

- The generation-3 blanking mask decides a backslash the way the harness does. The
  rewritten dangerous-removal classifier masks its input twice before its clause walk
  starts, once blanking every quoted run and comment and once keeping them. Those two
  answers decide whether a positional parameter (`$1`, `$@`, `$*`, `$!`) counts as a removal
  target. csift's mask read every backslash outside a single quote as an escape. The harness
  reads an in-quote backslash with the same helper its quote shield uses and blanks the
  escape as a pair, so a `\"` does not end the run that hides it. The old reading flipped
  the quote state for the whole rest of the command. A function definition written inside a
  quoted string counted as a real one, and a definition that genuinely followed the string
  was hidden. No corpus lane changes verdict. Over 165,314 distinct (Bash command, version)
  pairs the two readings differ on 4,097 masked strings and 165 function-scan answers. The
  generations before the rewrite never call a mask. The three differing commands recorded
  at or past the rewrite floor carry no `$`, which the classifier's entry guard wants first.

## [0.12.2] - 2026-09-12

### Added

- `harness.schedule.fire` labels the prompt a scheduled task fires. When a cron entry or a
  `ScheduleWakeup` timer comes due, Claude Code submits the armed text as its own `isMeta`
  record stamped `promptSource:"system"`. The text reaches that record verbatim. The fire
  path applies exactly one rewrite, the autonomous-loop sentinel resolution, and every
  other prompt falls through it unchanged. There was no marker to match, so the record fell
  into the isMeta exclusion, unsearchable on a flagless scan and absent from every census.
  The leaf keys on the record's own fields instead, and `-t harness.schedule.fire` now
  enumerates a session's scheduled work. The label zone names the instant the fire
  happened, read off the `system`/`scheduled_task_fire` record the prompt is parented to
  (`[scheduled fire <when>]`, JSON `scheduled_at`). A transcript that holds no such record
  reports null and no guessed time. A tick that does carry a loop marker keeps
  `harness.schedule.wakeup` or `harness.meta.loop` by arm order. An inbound peer message,
  which carries the identical stamp, is refused by its relay framing and its `origin`
  object. `Class::ALL` grows to 38 leaves, and every search hit's JSON gains a
  `scheduled_at` field.
- `list` joins a cleared session to the transcript it was cleared from. A `/clear` mints a
  new session id inside the same process and writes no lineage anywhere on disk, so the row
  infers the predecessor and says so. A transcript is `minted_by: "clear"` when its first
  non-isMeta user record is the `/clear` slash wrapper, which lands in the file the command
  created and not the one it ended. The predecessor is the sibling in the same project
  directory whose cost-ledger checkpoint (`startTime + totalDuration` on a `cost-state`
  line) closes within 2000 ms of that wrapper. The nearest line wins. A tie between two
  different files is reported and joined to neither. New JSON fields are `minted_by`,
  `cleared_from`, `cleared_from_distance_ms` and `cleared_from_candidates`, plus one
  `cleared` text row. The join never uses a file mtime or the time adjacency of ordinary
  records.
- `status` and `wait` report a cost-ledger checkpoint at the tail as evidence. When a
  transcript's last line is a `cost-state` line, a `checkpoint` evidence row names it and
  JSON carries `last_checkpoint: {line, kind}`. No verdict was added. The harness writes a
  checkpoint at a clear, a background handover, an in-app resume and at exit, most of them
  mid-file, so a tail checkpoint says only that nothing was appended after it. The registry
  row keeps deciding liveness. With no registry row and a tail checkpoint, that note gains a
  clause saying the session closed or was handed over.
- `status` finds the task store a clear left behind. The store is named after the id the
  process started with. The session's own id is now the first candidate and no longer the
  only one. The root of the `cleared_from` chain comes next, then the team file written
  within five seconds of the registry row's `startedAt`. A directory matching no candidate
  is never read. Every store that answered prints as `tasks store: <dir> (via
  <candidate>)`, with the same pairs in JSON `tasks_stores`, and an empty store prints too,
  marked `- empty`. This is the one place this release changes a pre-existing JSON key and
  does more than add one. A session whose store is found through the `cleared_from` root or
  the team file now reports its tasks where it reported none. `tasks` moves from null to an
  array, and `tasks_completed` from null to a count.
- `agents` gains `pending_reason` and `pending_checker` on a frozen lane, in JSON and as
  two lines under the text tree's `PENDING` line. The first is the harness's own reason tail
  for a predicted ask. The second is one disclosure line naming the path, the checker and
  the generation (`path: structured · checker: structured · gen2`), with `assumed` when the
  record carries no `version`. Both are `null` for a pending call these lexical layers do
  not model, a Windows `PowerShell` call included.
- `toolDenialKind` is modeled on the record. It is the harness's own reason a tool call did
  not run. It renders on a tool-result hit as `[denied: user-rejected]` in the label zone and
  rides hit JSON as `denial_kind`. Four values are written (`user-rejected`, `interrupted`,
  `cancelled`, `permission-rule`). The classifier's reason sentence is rendered into the
  prompt only and never reaches disk, so this field is the whole on-disk answer to why a
  call was refused. `--count-by result` is unchanged, and a denial is an `error`.

### Changed

- A frozen lane's `pending_classification` is now decided by the checker the harness
  would actually have run, for the Claude Code generation that recorded it. Claude Code
  decides a dangerous removal with one of two checkers, and the tree-sitter decomposition
  picks which. A too-complex parse reaches the lexical classifier, and a cleanly parsed
  removal reaches the structured one. Both produce the same bypass-immune ask. csift ran the
  lexical classifier on everything, so it attributed structured verdicts to the wrong
  grammar. It also stripped leading shell keywords, which the real clause head does not do,
  and that was a pure false positive on `do rm` / `then rm` / `else rm`. The port now
  branches on the parse shape, runs the generation's chain (`< 2.1.208`,
  `2.1.208..=2.1.260`, `>= 2.1.261`), and reports every arm's reason tail verbatim. Over
  162,118 distinct Bash commands in a local corpus, 12 lanes flip. 8 go up to
  `escalation-blocked` and 4 go down to `awaiting-execution`, all four of those the removed
  keyword false positive. No other command's output changes. The newest generation's walk
  also recurses into a nested `sh -c` script, and it reads that script rebuilt and never
  sliced out of the clause. The quote shield has already replaced the script's own spaces,
  so a slice arrives at the walk as one word, and the removal verb is never reached. The
  script is reassembled from its quoted and unquoted runs and unmasked first. An escaped
  `$` inside a double-quoted run is kept out of the target test unless it names a
  variable. Whether the positional form applies is derived from what follows the script, or
  from an `xargs` ahead of it. A too-complex head carrying a nested removal, such as
  `if true; then sh -c 'rm -rf $D/*'; fi`, now reads as the ask the harness makes.
- An arm csift cannot decide now says so and does not guess. The structured checker
  resolves the operand against the shell cwd, realpaths both, and compares against the
  working-directory set, and a transcript carries none of that. Those two arms answer
  `awaiting-execution` with the reason `removal target needs the filesystem state at the
  time`.

### Fixed

- The autonomous-loop markers now anchor at the start of a record's content.
  `harness.schedule.wakeup` and `harness.meta.loop` used to match their markers anywhere in
  a body, so a record that merely quoted a fired tick was read as one. That caught a skill's
  instruction text that embeds the loop preamble to explain it, a peer message relaying a
  tick, and a compaction summary recapping one. Each lost the leaf it had earned. The
  resolver builds the whole delivered prompt, and the fire writes it as its own record, so a
  real tick carries its marker at offset 0. The arms now test exactly that, after the same
  leading-whitespace trim the other harness marker arms use. A genuine tick classifies as
  before.

## [0.12.1] - 2026-09-11

### Changed

- The compaction-boundary prefilter keep now requires the `"subtype"` key beside the
  literal. A `compact_boundary` record always carries that key, and a payload that merely
  writes the word in prose never does. A flagless scan therefore stops parsing attachment
  and `queue-operation` lines it had no leaf to render them under. Every real boundary is
  still admitted, a refused line is still a node of the survival chain, and an explicit
  address still fetches it whole. The visible fix concerns a queued prompt naming the
  literal. It used to be emitted and censused as `user.queued` on a flagless scan, while the
  same run reported that leaf as unscanned.
- The survival axis got most of its cost back, and no output byte moved. 0.12.0 put each
  line's chain-structural spine row in the same vector as the records the command actually
  wanted, so every element was sized to a full record. The two kinds now ride separate
  output streams, and the spine row is its own narrow type, 72 bytes against a record's
  1184. The figures below are medians of 21 reps on a 397 MiB transcript, this release
  against 0.12.0.

  | command | faster by |
  |---|---|
  | `image` | 29.2% |
  | `verbatim` | 14.6% |
  | `show --branch-points` | 14.2% |
  | `files` | 8.1% |
  | a heavy `-t user.unsent` search | 6.8% |
  | `stats` | 6.7% |
  | `recover` | 6.1% |

  The same-binary control arm drifted at most 1.1% over those seven shapes, so the
  smallest win is five times the drift. A second, independent run of the same driver read
  the same direction with larger margins (`image` 31.8%, `verbatim` 16.0%). What remains
  against 0.11.1 is the axis itself. `image` reads 1.233x where 0.12.0 read 1.74x, and
  `verbatim` 1.153x where it read 1.35x. `files` reads 0.776x and is still faster than
  0.11.1, because one chain build replaced four. Nothing a consumer reads changed, and that
  is the checked part. Stdout, stderr and exit code are byte-identical to the 0.12.0 binary
  on 851 (command, file) pairs over the 76 top-level transcripts of one corpus, and on a
  further 1368 from a supplementary sweep of 18 more command shapes over the same files.

### Fixed

- An orphan reconciliation pulse now renders every task id it closes and names the kind.
  At the next session start, Claude Code reconciles the tasks a previous session left open
  with one `<task-notification>` carrying several `<task-id>` tags plus its own scan
  marker. csift joined every id already, but the render read only the first. A pulse
  closing five tasks showed one and hid four, and the marker could take the id slot
  outright. The real ids now render comma-joined in the label's id slot. The marker's kind
  renders as a trailing `(orphan reconciliation: <kind>)`. A `search` hit carries both, as
  JSON `task_ids` (an array) and `orphan_kind`. One helper sits behind `search`, `show`,
  `list` and `verbatim`, so every record surface follows. The shape is rare and worth
  knowing about. Of 3076 notification sections on the top-level transcripts of one corpus,
  2 carry more than one `<task-id>`.

### Documentation

- The compaction step-over is a guard and not the path the walk takes. The maintenance
  manual, the spec, the chain module comments, the `verbatim --help` text and four ledger
  claims said csift steps over a compaction cut through the boundary's own
  `logicalParentUuid`. A `compact_boundary` carries a null `parentUuid` on every corpus
  specimen (260 of 260). The walk therefore ends on the boundary and never consults that
  field, and everything physically above the terminating record reads `pre-cut` from the
  floor rule alone. Renaming the field on five real compacted transcripts of 9 to 21 MB
  leaves the per-hit `survival` census, the `stats` chain totals and the search-summary
  disclosures byte-identical. That is how the corrected reading was checked, and why no
  output moved. The coverage figure that justified the step (a median 9.4% against 46.5%)
  measured a walk that consults the field at a null-parent boundary, which is not the
  shipped walk. It is out of the maintenance docs and recorded as a residue on the claim.

## [0.12.0] - 2026-09-09

### Changed (breaking)

- BREAKING: the survival axis replaces the three opener heuristics. csift now asks Claude
  Code's own question, which records the conversation chain still reaches, in place of
  three special cases that approximated it. A record the chain no longer reaches is
  abandoned. It sits outside turn numbering and is skipped by a bare `-t user`/`-t
  agent`/`-t harness`. A glob or an exact leaf reaches it, with an `[abandoned]` /
  `[rewound]` marker. Turn numbering shifts on any transcript carrying a rewind or a
  replayed block.
- `-t user`/`-t agent`/`-t harness` gained a third exclusion axis. Delivery and leaf
  visibility already narrowed a bare role, and survival is the third. Every other selector
  form is unchanged.
- `user.unsent` narrows to drafts, and `user.rewound` is one of the two leaves this
  release adds, bringing the taxonomy to 37. `harness.resume.placeholder` is the other. A
  draft is a prompt that was recalled and never drew a reply. A turn the operator rewound
  past was sent and did draw one, and that reply is the whole discriminator. Both sit
  outside turn numbering and outside a bare `-t user`. `-t user.unsent` / `-t
  user.rewound` or a glob reaches them. A rewound opener's diff line leads with `rewound:
  the conversation continued from L<n> instead`.
- A replay copy is now the earlier line. The loader's uuid map keeps the last line carrying
  a uuid, so that line is the survivor for opener treatment and for addressing by uuid.
  The earlier line renders `[replay copy of L<n>]`. Both stay searchable and counted.
- A turn number names the same turn on every command. `verbatim` turn 7, a
  `search --count-by turn` row `t7`, `show --turn 7`, `files --turn 7` and `recover --at
  @turn:7` all mean the live turn 7 that Claude Code's own conversation chain still reaches.
  `image --turn` joins them, after staying on the older, more conservative grouper. That
  agreement had to be re-established, and it was not assumed. Splitting one numbering rule
  into two, which is what wiring the axis into only some surfaces does, put 2,085 turn
  stamps out of step on a 19-transcript `files`-to-`search` join, 23.1% of the paired
  rows. 0.12.0 reads 0 there, as 0.11.1 did. `stats` `turns` equals the
  `search --count-by turn` row count on 75 of 75 top-level transcripts.
- The census effect of the axis, so readers can reconcile their own numbers. Over this
  corpus the opener count is conserved (3650 turn openers before, 3650 + 7 after). Seven
  records that used to be counted as turn-opening `agent.communication.signal` are now
  `user.rewound`, so that leaf reads 397 where it read 404. Nothing was dropped, and the
  seven moved leaf. The whole of `user.rewound` is a re-labelling, and the arithmetic
  closes. Run the two binaries back to back over one corpus with
  `search "" --count-by label --max-count 0`. `user.message` drops 6 (2,781 to 2,775),
  `user.unsent` drops 30 (909 to 879), and `agent.communication.signal` drops 7 (404 to
  397), while `user.rewound` reads 43. 6 + 30 + 7 = 43, so every record is accounted for.
- Records above a compaction cut are `pre-cut`, and that is where the fail-open guard
  applies. A `compact_boundary` carries a null `parentUuid` (260 of 260 boundary records
  over every transcript under one projects root). csift's walk therefore ends on the
  boundary exactly as Claude Code's does, and everything physically above it is a blind
  region csift did not resolve. Nothing there is ever called abandoned. It is `pre-cut`,
  every selector still reaches it, and the same-parent draft rule still marks a recalled
  draft inside it. What csift adds above a cut is a refusal to discard, where Claude Code
  drops that history. An archive answers what the model saw at the time, so csift keeps
  every record the walk did not reach, flagged. The `logicalParentUuid` arm in the walk is
  a guard for one shape only, a boundary whose own `parentUuid` is non-null and resolves to
  no record, and the corpus holds zero specimens of it. Claim MISC-043 records what the
  guard would meet if it ever fired. 52 of 255 `compact_boundary` records, across 14 of the
  21 compacted transcripts among 77 top-level ones, name a `logicalParentUuid` matching no
  `uuid` on any line of their own file.
- `verbatim` replays live turns only. A turn the operator rewound past, or a prompt
  recalled and re-typed, is counted in a per-session note with the `csift show --line` that
  fetches it, and never re-emitted. The compaction summariser reads the in-memory message
  array, so it never saw that turn either. Replaying it would put text that no model ever
  read into a "what the compaction clipped" reconstruction. Reconstruction still crosses a
  compaction cut, where Claude Code's own loader stops. Those units are flagged `pre-cut`
  and kept.
- `files` and `recover` keep every abandoned mutation and mark it. Disk truth is file
  order, and an Edit on a rewound branch landed. The row stays, its turn slot reads
  `turn abandoned (root L<n>)`, and it carries an `[abandoned]` marker. A `--turn` window
  admits it by the live turn it physically follows, so a window over a span never hides a
  write that happened inside it.
- `image` lists an abandoned image, marked. The bytes are on disk, and `--out` writes the
  same file. Only `--turn`, which windows on numbered turns, leaves it out.
- `recover`'s external-write inference widened to the `--file` target. The file-history
  instrument now reports a version jump with no tool write on any path a recover run names,
  as `external write (inferred, snapshot vN->vM, no tool record since L<line>)`. `files`
  still reports it for the settings family only. The tracked set spans every written path,
  and a global timeline row would flood the output (58,115 such jumps over 1,121
  non-settings paths in one measured corpus). Both `file-history-snapshot` and the
  per-write `file-history-delta` now feed the version sequence.

- `harness.schedule.continuation` is now `harness.resume.prompt`. The predicate and the
  records are the same, and only the path is new. That leaf never came from the scheduler.
  The resume loader writes the record when the transcript it is loading ends on a dangling
  user prompt, and the leaf needed a family its new sibling could share. `harness.schedule`
  keeps only `wakeup`. The old spelling is a hard parse error that names its successor, the
  same way the pre-v0.5 flat label values do. A script or saved query that keys on the old
  path needs the new one.

- The axis costs time, and the number is stated here. Every full-scan command now reads
  the lines its own prefilter drops, keeping the five structural fields the chain needs and
  none of the payload. On a 397 MiB transcript that is 100,387 structural rows against as
  few as 183 records the command itself wanted. The measurement ran against 0.11.1 on that
  transcript, with medians of 21 runs per arm on an interleaved CPU-time driver with rotated
  cells and a same-binary control drifting under 2%. The ratios quoted are wall clock.
  `image` reads 1.74 to 1.77x, `verbatim` 1.35x, `show --branch-points` 1.54x, `recover`
  1.19x and `stats` 1.17x, and a heavy `-t user.unsent` search on a 721 MB transcript reads
  1.17x. `files` got faster, 0.84x, because four chain builds became one. The byte walk
  itself is nearly free in wall time, because it runs inside the parallel scan. What lands
  on the clock is the serial carry of a wider row vector plus the chain build. Four
  candidate savings were measured and rejected. The one that remains needs the structural
  rows to stop sharing a collection with full records. That is a later patch and not a
  last-minute change to the seam this release just settled.

### Added

- The resume repair pair has its own two leaves. Suppose a session is resumed whose last
  record is a prompt nobody answered, such as an esc-recalled draft or an interrupted turn.
  Claude Code repairs the tail at load time. It appends `Continue from where you left off.`
  as an `isMeta` user record (`harness.resume.prompt`). It then splices a stand-in reply
  reading `No response requested.` in after it (`harness.resume.placeholder`, new). The
  model receives both, so they shape the resumed turn while belonging to neither side of
  the conversation. `-t user` and `-t agent` hide them, `-t harness` and `-t
  harness.resume` show them, and neither opens a turn. The prompt leaf requires `isMeta`,
  which is the flag the injecting call site stamps and the one Claude Code's own recogniser
  tests. A message a person begins with that sentence is still theirs, labelled
  `user.message` and opening its turn.
- A placeholder hit carries `resume_paired`, with `[paired]` / `[unpaired]` in the label
  zone. One writer produces every placeholder, and it fires whenever the loaded tail is a
  user record. The prompt beside it needs an interrupted-turn verdict. The same record
  therefore also lands alone after an interrupt marker or a slash-command wrapper. In this
  corpus 8 of 18 are paired and 10 are not. Both forms carry the leaf. The pairing is a fact
  on the hit, read from the record's `parentUuid`, and never a second label.

- JSON gains `survival`, `abandoned_root_line` and `replay_copy_of` per hit and per `show`
  record. The `search` summary gains `abandoned_records`, `rewound_turns`,
  `replay_copies`, `boundary_cut_line` and `leaf_source`. The text footer states every one
  of them.
- `show --branch-points` names the live child. Each fork prints `live child: L<n>` from
  the chain and labels every other child `rewound`, `draft`, `abandoned` or `pre-cut`. The
  fork parent is now located over every line the loader admits. A fork parented to a hook's
  attachment record prints that line and its type. `parent uuid not in this file` replaces
  the old `(parent line not located)` and means what it says. JSON branch-point rows gain
  `parent_line`, `parent_type`, `live_child_line` and, per child, `survival` and
  `verdict`.
- `stats` counts live turns, the same numbering `search` prints. It adds three whole-file
  chain totals per session and in the scope TOTAL, `abandoned_turns`, `rewound_turns` and
  `replay_copies`. They never window, because an abandoned opener has no turn index to
  window on. The line-type census stays the exact whole-file corruption authority.
- The file-oriented commands gain JSON keys. A `files` mutation and boundary row carries
  `survival` + `abandoned_root_line`, with a null `turn_index` when abandoned, and so does a
  `recover` boundary. Every grouped `files` row carries `abandoned`, a share of `total` and
  never a subtraction. A `recover` coverage/snapshot row carries `abandoned_events`. The
  `verbatim` summary carries `abandoned_turns`, and each of its units carries `survival`.
  An `image` listing row carries `survival`.

- The compaction boundary shows what the compaction kept, and the two `/rewind` summarize
  modes are finally distinguishable. A boundary record always carried more than the four
  numbers csift printed. Its excerpt now names every field the record actually has, in a
  fixed order and only when present. Those are `messagesSummarized`,
  `cumulativeDroppedTokens` (the running total of context tokens every compaction of this
  session has dropped), `preserved=<N uuids, M allUuids, anchor abc12345>` and
  `segment=<head>..<tail>`. The `preserved` pair is the set the compaction kept. `uuids` is
  what reached the disk, and `allUuids` is the in-memory superset, which is why an
  `allUuids` entry sometimes resolves to no line. The excerpt counts those lists and prints
  eight-character handles, so a boundary line stays scannable. JSON `compact_metadata`
  carries the whole object verbatim, uuid lists included. A boundary with only the four
  original scalars renders exactly as it did before, byte for byte.
- Both compaction records carry `mode`, one of `compact`, `summarize-from-here` and
  `summarize-up-to-here`. Claude Code compacts three ways and writes the same two records
  for all three, so a `/rewind` "Summarize up to here" used to look like an ordinary
  auto-compact. One key separates them, and it sits on the summary.
  `summarizeMetadata:{direction, messagesSummarized}` is written in place of
  `isVisibleInTranscriptOnly`, and `direction` says which half was summarised. `up_to`
  summarised everything before the message you picked, and `from` everything after it.
  csift prints that as `mode` in JSON on the search hit, the show record and the verbatim
  boundary row. It also prints a `[summarize up_to]` tag in the summary's label zone and a
  `· summarize up_to ·` segment in the verbatim banner. The boundary carries no direction of
  its own, so it takes its mode from the summary that follows it. A boundary your query
  never paired with one reads null and never a guessed `compact`. A summarize is a
  compaction like any other, and `verbatim` still restores the turns it clipped.

### Fixed

- A turn you rewound past used to read as a recalled draft. The old rule grouped turn
  openers by their shared `parentUuid` and called every earlier one a superseded draft. A
  rewind that landed back on the same parent came out as `user.unsent`, with a character
  diff against the "resend". A rewind that landed anywhere else came out as an ordinary live
  turn. Over 910 shared-parent opener pairs in one corpus, 858 have no assistant record
  below them and are genuine drafts. 52 have one and were rewinds wearing a draft's label.
  0 of the 910 earlier openers are on the surviving conversation. That 52 and the leaf's own
  count are different censuses. The pair census needs a later opener under the same parent.
  The leaf needs the chain to have resolved that region, and then covers every abandoned
  opener that drew a reply, whatever sits beside it. On this corpus at this release's
  census, `user.rewound` reads 43 over 10 sessions. The likeliest source of the difference
  is the blind region above a dangling compaction boundary, where csift declines to call
  anything abandoned at all. That explanation is plausible and UNTESTED.
- An abandoned record used to count as part of the live conversation. That went beyond the
  opener. Every reply, tool call and tool result under a rewound prompt was numbered into a
  turn, selected by a bare `-t agent`, replayed by `verbatim` and counted by `stats`. All of
  that was the model's context on a branch the conversation left. Such a record is now
  outside turn numbering and outside a bare role. A glob or an exact leaf reaches it, with an
  `[abandoned]` / `[rewound]` marker, and every count that changed is disclosed in the
  footer and the JSON summary. `files`, `recover` and `image` deliberately keep their rows
  and mark them, because those three answer about the disk.
- A shell backgrounded by ctrl+b, a timeout or a delivered message is a background task.
  `run_in_background` is the only door the model asks for. Claude Code opens three more on
  a command the model ran in the foreground. The person presses ctrl+b on the in-flight
  call, the command hits its timeout, or it is moved aside so a queued message can reach the
  model. The launching tool call is never rewritten, so the receipt sentence is the only
  trace. `status` was calling such a session `idle-eot` while a shell of its own was still
  running. Both commands now read the receipt. The task is listed and counted, `--until
  stop` stays open over it, and the row says which door it came through (`entered by
  ctrl+b`, `entered by timeout after 2m`, `entered to deliver a message`, with JSON
  `entered_by` and `timed_out_after_ms`). On three real sessions the open-task count went
  3 → 5, 5 → 6 and 51 → 54.
- The launch record behind such a task is recovered, and an unknown launch time is stated
  as unknown. The launching line carries none of the scanner's needles, so a second
  targeted pass fetches its instant, command and description. Those are also what
  `--ignore-background` then matches on. When that line is gone, the row keeps the receipt
  instant under an explicit `launched-at unknown; receipt at …` note (JSON `launch_note`)
  and never passes it off as a launch. The receipt must be a whole sentence, with the
  clause, a real `b`+8 task id and that arm's closing clause. A transcript that merely
  renders the template therefore cannot fabricate a task, and a grep of the harness binary
  does exactly that rendering. A fifth way in, a plugin's turn abort, writes the ordinary
  sentence and cannot be told apart from a model-requested launch. csift does not claim it.
- A fabricated reply no longer reads as the model's. Those `No response requested.`
  records classified as `agent.message`, which claims the assistant wrote a sentence no
  model produced. They carry the `<synthetic>` model sentinel and are built with no model
  call at all. The new predicate is a port of Claude Code's own recogniser, and the
  sentinel is its load-bearing half, so a genuine reply that happens to say the same words
  is unaffected.
- `status` and `wait` stop reporting the loader as your last exchange. A session resumed
  onto a dangling prompt ends with the repair pair, so the `last` section showed `No
  response requested.` as the newest reply. Both halves are now skipped, and the real last
  prompt and reply come back.

- An inbound cross-session message was invisible. When one Claude Code session sends a
  message to another, the receiver's transcript gets a `type:"user"` record whose content
  is framed as `<cross-session-message from="uds:…" from-name="…" from-mode="…">`. That is a
  third peer framing beside `<teammate-message>` and `<agent-message>`. csift knew the other
  two, so the record matched no marker, and its `isMeta` flag sent it down the "harness
  pseudo-turn" path. It came out with no label at all, with no leaf, no census row and
  nothing under any `-t`. `csift show --line` on it answered `no such record(s)`, even though
  `--raw` printed the line. The framing now classifies as `agent.communication.inbox` and
  opens a turn like its two siblings. Its direction reads the sender's name (`from-name`, or
  the structured `origin.name` the harness fills from the same attribute) and not the
  socket path in `from`. Detection is boundary-anchored like the others, so prose that
  merely quotes the tag stays `user.message`, as this repository's own docs do. This is
  recorded as claim TURN-031, traced from the socket handler through the element template to
  the record stamps.
- The queue rider stopped being counted as your typing. A `queue-operation` enqueue line
  carries the same framed message one line earlier. `user.queued` means "the queue line
  carries the human's text", and it asks the same peer predicate the records do. Widening
  that one predicate takes the rider out of the human count with no second rule to keep in
  sync.
- `show --line` renders a record csift has no label for. `classify` deliberately emits
  nothing for a few real message shapes, such as an `isMeta` pseudo-turn matching no
  harness marker or a block record with no text, and never mislabels them as the user. An
  explicit address on such a record used to bail. It now renders one unlabeled unit, `? (no
  label)` in text and `"label": null` with an empty `"labels"` in JSON, because an address
  is a promise to show the record it names. A plain scan still emits nothing, so no count,
  census or `-t` result changes. A line that is no record at all, such as a session-state
  cache line or a torn line, is still a hard miss pointing at `--raw`.
- A plain search stopped reporting live history as pre-cut. Two of the byte needles that
  pick candidate lines run on every scan, and both can pick up an `attachment` line a plain
  search has no label for. One looks for the channel envelope. The other looks for the word
  `compact_boundary`, which any payload may simply use. Such a record was removed before
  matching, so it could not surface under a gated leaf. Removing it punched a hole in the
  conversation chain, which threads through attachment records. The walk stopped at the hole,
  and everything above it came back marked `pre-cut`, on transcripts that had never
  compacted. The record is now reduced to its structural fields and kept, the same spine
  row a line the prefilter never picked up already gets. It still emits nothing, so no hit,
  count, census key or turn moves. The chain simply sees it. On this corpus, 173 lines across
  5 of 76 transcripts were affected. Denominators in this entry read 75 or 76 top-level
  transcripts depending on the bullet, because the corpus is live and gained one file
  between measurements. Each figure is whole against the scope it was taken on, and none of
  them is a single census. The plain scan and `--attachments` now report the same chain on
  76 of 76 files, where 3 disagreed before. On one 7.2 MB transcript, 1,190 of 1,246 rows read
  `pre-cut` before, and none do now. `show --line` was always right about those lines,
  because an address opens the gates. A search and a fetch of the same record no longer
  contradict each other.

- The `harness.meta.system` help list names every subtype csift models. `csift search
  --help` listed one of the two model-refusal subtypes and stopped at the six it could name,
  which reads as a closed set. It now carries `model_refusal_no_fallback` beside
  `model_refusal_fallback`. It says that a subtype a later build adds lands there too, which
  is what the leaf has always done and what SKILL.md has always said. Help and SKILL are
  meant to carry the same information, and this one had drifted.

- Peer message bodies render the same way everywhere. `verbatim` and `list` already
  stripped the wrapper tags, the relay preamble and the security footer from an inbound peer
  message. `search` and `show` printed the whole tagged section, so the same message read
  two different ways depending on the command. All four now render the peer's own words.
  The consequence matters before relying on it. The wrapper attributes are no longer part
  of any text `search` can match, so `search 'teammate_id='` and `search 'from-name='` find
  nothing at all. `--raw` does not recover them, because it only prints the source line of a
  hit the matcher already made. The peer's own words are matchable as before, and the
  direction line still names the sender. The verbatim line of a record you can address is
  still one `csift show @<id> --line N --raw` away.

## [0.11.1] - 2026-09-08

### Added

- A rendered `user.unsent` draft now carries an unsent diff line. It says how far the
  draft sits from the message that replaced it, as `differs from the sent message in N
  chars (P% of the final): may carry an addition or a correction`. JSON carries
  `superseding_line`, `superseding_uuid`, `diff_chars`, `diff_pct` and `diff_exact`, on
  the `search` exchange row and the `show` record row. N counts insertions plus deletions of
  a shortest character-level edit script. It is the work the edit did and not a length
  difference. P is N over the final message's length, and it can exceed 100 when the draft
  was the longer text. The engine is a bounded char-level Myers walk with no new dependency.
  It runs only when a draft is actually rendered, so `-c`, `-l` and `--count-by` pay
  nothing. It was measured over the whole corpus after the replay guard below, on a
  population of 887 drafts. A live corpus gains drafts as sessions run, so a re-run sees a
  slightly larger one. 877 read exact, and the other 10 report a proven floor between 3,813
  and 9,360 characters. The largest draft (237,434 characters) diffed in 0.755 ms. A
  whole-corpus draft scan and a broad search both ran within noise of 0.11.0.

### Changed

- `user.unsent` is documented as retroactive. A draft and its resend are the same record
  shape, with the same key set on 852 of 886 pairs, and nothing but identity and time
  always differs. No successor line shape separates the two populations. Every shape that
  reads zero after a control record reads zero because that window is cut at the next
  assistant reply, and for no reason that has to do with drafts. The only discriminator is
  therefore the later sibling. A hook running at prompt submit cannot apply the label,
  because that sibling is the prompt being submitted. The closest approximation skips a
  trailing turn-opener that no assistant record follows. It hides 92.2% of drafts but
  wrongly skips 2.9% of genuine messages. SKILL's wrong-assumptions table now states it as a
  heuristic and no longer implies it is the label. The census covered the full populations,
  all 886 drafts, each joined to the survivor csift itself names, against all 2,595
  `user.message` records of the 28 draft-carrying sessions. It is recorded as ledger claim
  TURN-026, which carries the commands and the whole per-shape table.

### Fixed

- A replayed record is no longer read as a resend, and turn numbers shift where it was. A
  compaction re-anchor re-appends a block of already-written records with their uuids
  preserved. A re-appended user message keeps its `parentUuid` too and differs from the
  original in one field (`promptId`). The draft rule groups openers by `parentUuid` and
  keeps the last, so it read the copy as a later sibling. The original message, sent and
  answered, was labelled `user.unsent`, dropped from turn numbering, and shown as differing
  from the sent message in 0 chars. Seven genuine messages were affected in one session.
  Same-uuid openers under one parent now collapse to their first occurrence. The first
  keeps its label and its turn. The copy opens nothing but stays a turn member, so it is
  still addressable and still renders like every other replayed record. Turn numbering
  shifts on any transcript carrying a replayed block, as it did for the v0.5 slash-wrapper
  fix and the 0.9.2 draft work. A `tN` noted earlier has to be read again from current
  output, and `--uuid` is the address that survives. This is recorded as ledger claim
  CMP-019.
- An unedited resend says so. A draft whose text is identical to the message that replaced
  it now reads `identical to the sent message`, where it read `differs from the sent
  message in 0 chars`.

### Changed

- A bare role selector now selects what the model received, per record, and this is
  BREAKING. It used to decide that per leaf, from a table of which labels are conversation. Claude Code
  decides it per record, with one drop predicate inside the assembler that builds an API
  request. That predicate disagrees with the leaf table in exactly three cases, and csift
  mirrors those three and nothing else. The two census changes were measured on this corpus,
  with the two builds run back to back over 72 top-level transcripts.
  - `-t harness` now surfaces `system`/`local_command` records, a slash command's own echo
    and its stdout, because the assembler re-mints them as a user message and sends them.
    Sixteen records appear where 0.11.0 showed none, although `harness.meta.system` is an
    invisible leaf and stays one by default.
  - `-t agent` now drops the API-error placeholders. Claude Code fabricates those assistant
    records when a call fails and never sends them back. `agent.message` falls by exactly
    118, the whole population of records carrying `isApiErrorMessage`, and 118 of 118 also
    carry the `<synthetic>` model that the predicate tests for. An `isVirtual` record leaves
    a bare `-t user` the same way.

  Globs (`-t 'agent.*'`), intermediate prefixes and exact leaf paths are untouched and still
  reach every record, so nothing became unreachable. Under those forms an undelivered record
  renders `[not delivered]` in the label zone, and every JSON hit gained a `delivered` field
  carrying the verdict. This is recorded as ledger claims CLS-026 and CLS-027.

- A bare `-t harness` admits the `type:"system"` lines. Those lines were gated behind an
  explicit selector, so the delivered record above could not have been reached at all. The
  other five gated leaves keep the explicit-selector rule exactly. The per-record rule drops
  every other system record the admission parsed. `gated_leaves_unreached` and its stderr
  note now report what the scan actually parsed, where they used to report which selector
  was typed. The note's leaf list is derived from the gated set and no longer typed out.

- Slash commands the model never sees are now documented. Three families leave nothing on
  disk, so finding no wrapper record is not evidence a command was not run. `/btw`,
  `/tasks` (= `/bashes`) and `/release-notes` return skip on every normal exit. Any panel
  closed with Escape writes nothing, whatever its own option was. `/rewind`
  (= `/checkpoint`/`/undo`) and `/stop` take the same path through the `local` result
  map. A record on disk is not always a record the model saw either. `/release-notes`
  appends a harness notice and then returns skip, so the notice is rendered to the human
  and dropped by the assembler. SKILL's wrong-assumptions table carries both rows. This is
  recorded as ledger claims TURN-027 through TURN-030 and MISC-031.

## [0.11.0] - 2026-09-06

This release adds the csift channel. It carries a message to a Claude Code lane the
official channel cannot reach, and it can tell afterwards whether the message arrived.

### Changed

- The read-only law becomes a writers law, and this is BREAKING. Through 0.10.5 csift wrote nothing
  at all. From 0.11.0 exactly three commands write, `send`, `deliver` and `ack`. They write
  exactly one place, a `csift-channel` directory csift creates in the session's own sidecar
  folder. They never write a transcript, the team mailbox, the cross-session messaging
  socket, the session registry or any settings file. csift does not install its hook
  either. `deliver --recipe` prints the block, and you paste it. Every other subcommand
  still only reads. Nothing that worked before stops working. The promise a reader had is
  narrower now, though, and a narrower promise is a breaking one.

### Added

- `csift send @<lane> "message"` queues one message for one lane, which can be a
  top-level session, an unnamed subagent, a teammate or a workflow lane. It exists because
  for some receivers the official channel is absent outright. The send tool cannot reach a
  running workflow lane at all. An unnamed subagent has no arm to address its own parent
  subagent. Nothing wakes an idle session that published no socket. A sender outside Claude
  Code holds no tool to call. Every send prints one verdict, `OK`, `FULL`, `MAY-FAIL`,
  `UNPREDICTABLE` or `REFUSED`, and exits 0 even for a refusal. A refusal is a definitive
  answer about the receiver and not a usage error. Where an official transport does exist,
  csift prints the exact call for you to make and still queues its own copy.
- `csift deliver --slot k` is the hook entry that carries the channel into a lane. You
  never run it by hand. `csift deliver --recipe` prints the settings block, in both the
  plain and the PowerShell command form and for any number of slots, and a human installs
  it. Slot k emits chunk k of what is waiting, and the slots of one event order themselves.
  A queue message at a turn boundary may block the turn from ending, as far as the
  harness's own block cap allows.
- `csift msg <id>` answers the question a queue cannot, whether the message actually
  arrived. The per-lane ledger records what csift emitted, and the receiver's transcript
  proves what landed. `msg` joins the two into one verdict (`DELIVERED`, `INTENT-ONLY`,
  `QUEUED`, `HELD`, `EXPIRED`, `ACKED`, `REFUSED`). The proof half on its own is exactly
  `csift search '<id>' @<lane> --additional-context`, so the join can be audited without
  trusting it. `csift ack <id>` records the one thing only a receiver can say.
- `whoami` grew a lane layer. It takes a `@<agent-id>` target and prints `self`, `parent`
  and `topology` sections. `--to @<lane>` gives a reach prediction that sends nothing.
  `--peers` lists every live lane as id, kind and state, and deliberately nothing else. A
  description or a role-shaped name is the material one lane would use to claim standing
  over another, so the census answers who is alive and never who should be obeyed. Outside
  Claude Code, `whoami` now prints the not-a-lane answer, with the one channel out and what a
  receiver needs installed.
- `agent.communication.channel` is the 35th label. A delivery lands in the receiving lane
  as a hook-context attachment. This is the one attachment leaf a default search reaches,
  because a message addressed at the lane is not machinery. It renders verbatim, envelope
  header and all, so a reader can see who sent it, under what relation, and that it came
  from neither the user nor the harness.
- `@<Name>@<Team>`, a teammate's routing form, works as a target. A teammate carries two
  ids minted apart at spawn. The routing form is what the official send tool needs, and it
  can collide when two teammates share a name. The transcript form on disk never collides.
  csift resolves either, keys its own work on the transcript form, and now prints both
  wherever a teammate appears.
- csift reads the settings cascade to answer whether a delivery hook will actually run in a
  lane. It reads five scopes in Claude Code's own order, merges `env` per key, concatenates
  `hooks` per event, unions plugin hook manifests, composes the policy tier first-wins, and
  applies the three policy switches that can empty the whole hook set. What is not
  observable from disk is listed as such. A gate verdict therefore says "unknown" with its
  evidence and never implies the file scopes are the whole story. `plan` now reads its
  `plansDirectory` through the same model, with its precedence unchanged.

## [0.10.5] - 2026-09-05

This release corrects the introspection ledger, and four csift defects that the ledger's
own drift verdicts had named and the previous release shipped around.

### Fixed

- `image` joins the `#N` handle by number. A record's `imagePasteIds` lists the ids in the
  order of its image blocks, while the `[Image #N]` markers keep the operator's text order.
  The two diverge on 18 of 662 corpus records, and the positional zip gave those a silently
  wrong handle. Without the array, the positional zip stays, under its count guard.
- `recover` no longer replays a Read echo that carries no text. Claude Code blanks the text
  of a tool result older than its retention window before persisting it. The reference
  corpus holds 320 such echoes, with `content` empty and the line counts intact. Four result
  arms never carry a text at all. A blanked echo of a complete read used to replay as a
  whole-file snapshot of nothing. Each is now a counted `blanked-read` with a soft
  annotation boundary.
- The `status`/`wait` pid probe pins `LC_ALL=C` and `TZ=UTC` on its own `ps` call, as the
  harness does for its `procStart`. Under a German or French locale the rendering parsed
  under neither English pattern, and the pid-reuse guard was silently skipped.
- Search hits carry `refetch_uuid` beside `refetch`. A line number is a durable address
  only while the transcript is append-only, and three harness paths rewrite a live
  transcript in place. The uuid survives them.
- Four statements csift printed or taught were wrong at 2.1.258, and they are corrected
  with their dates. TaskStop resolves a teammate by its name or `name@team` from Claude
  Code 2.1.198. The agents footer, the JSON control hint, the SKILL rows and the hook recipe
  said it rejects every form. The file-history store is also written by an approved
  in-place `sed` preview, where `recover --list-backups` said bash edits never land there.
  The pending AskUserQuestion split is a timing outcome of the write frontier. It is
  neither a question-count rule nor a 2.1.258 change, though a comment, a note, the help and
  three documents said otherwise.

### Changed

- The ledger schema gains the attribution `upstream`. It marks a producer that lies outside
  the shipped binary by construction, on the model or API side, in the operating system or
  in a native runtime binding, with the client-side treatment traced. Three new gate rules
  arrive with it. A claim's latest check is never `drifted`. No open leg is an unconsumed
  text correction or a recorded rejection. Every claim below end-to-end carries its leg
  fields. Ledger text now follows the prose law, with no dates, no csift versions and no
  audit narrative, and the commit and the release carry those.
- The README's verification section states what the project will never publish (the
  method, a mechanical ledger comparison, any corpus). It also states why the ledger is
  re-verified periodically and not for every Claude Code release.

## [0.10.4] - 2026-09-04

### Fixed

- `wait` and the `status`/`wait` tail window no longer memory-map a live transcript. A
  mapped file that another process truncates faults with SIGBUS on the first page touched
  past the new end, and nothing catches it. Claude Code does rewrite a transcript in place.
  A rewind tombstone truncates and rewrites the tail, and an armed local GC rewrites on
  compaction. A long `wait` polling a live file every few hundred milliseconds carried that
  risk on every poll. Both readers now use plain positional reads, which return fewer bytes
  on a shrink and never fault. The one-shot full scans keep the memory map, whose
  sub-second life bounds the exposure.
- `wait` detects a transcript that shrank between two polls. It moves its baseline to the
  new end and reports the event (`transcript shrank N time(s)` in the activity line, JSON
  `shrinks`). Before this, a shrunk file was skipped silently.

## [0.10.3] - 2026-09-04

This release completes the attribution work. Every claim in the introspection ledger now
states how its behavior was traced. The trace can rest on the producing code in the
shipped Claude Code binary, on a specimen on disk or in a live trial, on both, or on
neither. The README carries the tally as a gate-generated table, and the five defects the
tracing found in csift are fixed.

### Fixed

- In `search`, `verbatim` and `show`, an AskUserQuestion answered through the freeform
  `toolUseResult.response` field classifies as `user.answer` and opens a turn like every
  other answer. That field is the binary's `The user responded: ...` branch, which leaves
  `answers` empty. The AUQ unit renders the questions asked and a `response:` line. The
  marker joins the answer prefixes and the verifiable synth needles, so the whole-file gate
  stays sound. The idle-timeout branch (`No response after ...`) is not an answer and still
  opens no turn.
- In `status` and `wait`, a completion pulse whose status is `blocked`, or any literal
  csift does not know, gets its own bucket (`N blocked`, `N with an unknown status`, JSON
  `blocked`, `other`). Claude Code's remote-agent notifier writes `blocked`. Such pulses
  used to be booked as a clean completion.
- `wait --until notification` fires in every watched lane. Claude Code normally delivers a
  pulse to the main transcript. One addressed to the owning agent lands in that agent's
  lane instead (2 of 2906 delivered records in the reference corpus), and the main-only
  scope could never fire on it.
- `recover` never replays a Read cut at its token budget (`truncatedByTokenCap`) as a
  whole-file snapshot. On a file whose lines are too long to paginate, the harness recounts
  `numLines` from the cut slice, so it can equal `totalLines`. The old full-read test then
  accepted truncated content as the whole file.
- `image` lists the images of a `queued_command` attachment. A prompt queued and then
  edited or recalled never becomes a user record, so its pasted images existed only there.
  45 of 52 such blocks in the reference corpus had no other copy.

### Changed

- In `INTROSPECTION.json`, every claim carries `producer_trace` (`complete` | `partial` |
  `none`), `specimen` (`observed` | `none`) and the derived `attribution` (`end-to-end`
  | `producer-only` | `specimen-only` | `partial-producer` | `by-elimination`). It also
  carries `open_legs`, naming the instrument that would close each missing leg. A complete
  producer trace is three hops in the binary, quoted verbatim at byte offsets, which are the
  trigger, the gate and the writer. A negative ("nothing else writes this") closes only by
  an `enumeration` of every site in the binary, each read. A claim whose text a hop refuted
  was rewritten and not annotated. An end-to-end claim carries no open leg, and a non-gap
  note lives in `residue`. Version floors were checked against historical builds fetched by
  version and bisected on string literals. The gate refuses a by-elimination claim that
  `holds`, a leg pair that disagrees with its attribution, an end-to-end claim with an open
  leg, and a README tally that disagrees with the ledger.
- The README gains a "How much of this is verified" section with the tally table,
  regenerated from the ledger and checked by the gate.

## [0.10.2] - 2026-09-03

This release is the first re-read of the introspection ledger after 0.10.1. The 17 claims
that had no code site were anchored, and every site the release had moved was relocated.
The reading turned up the defects below. Each fix carries its claim update, with the
instrument that measured it.

### Fixed

- `wait --until notification` never fired on a completion absorbed mid-turn. Claude Code
  delivers a `<task-notification>` as a user record only when the session is idle. A pulse
  that lands mid-turn exists only on a `queue-operation` enqueue line and a
  `queued_command` attachment. This corpus holds 3218 pulse-bearing user records against
  5893 enqueue lines. The condition and the wait activity census now read the same three
  carriers the background section joins. A queue remove or dequeue repeats the enqueue's
  pulse and counts nothing.
- `plan`'s slug-only binding resolved `plansDirectory` against the slug record's own
  `cwd`. That cwd follows the tracked shell cwd and can already sit in a subdirectory, so
  the plan file was joined under that subdirectory, and a project-scope `plansDirectory`
  was silently dropped. Claude Code memoizes the directory at its first access early in
  the session. csift now binds against the transcript's first recorded cwd and reads the
  settings scopes from that root.
- In `search`, a subagent transcript that is a `/fork` clone (line 1 is a
  `fork-context-ref` record) had its first turn-opener labeled as the spawn-prompt seed
  (`agent.communication.inbox`). That opener is in fact the parent's own human message,
  and 10 of the 42 clones in this corpus carry such a record. A clone now has no seed.
- In `agents`, the global spawn index folded subagent locals later-wins. A clone's copy of
  a sibling's spawn record could therefore re-parent that sibling onto the clone. The fold
  is first-wins now, the main transcript first and then discovery order.
- In `search`, a superseded draft whose text is sectioned (a pulse or relay shape) fanned
  out into per-section classes that its own `labels[]` did not carry. It now keeps the
  single `user.unsent` view.
- Two shipped strings had lost their line-continuation backslashes and printed runs of
  spaces. They were the `wait` timeout-guard message and a `recover` bash-append boundary
  detail.

### Changed

- `show --turn` help says what the fetch omits. A turn carries gated non-record lines
  (turn-duration, stop-hooks, away-summary, queue, snapshot, system). They are addressed by
  `--line`/`--uuid` or `search -t harness.meta`, and never by a turn range.
- `search --help` names the eight LLM-invisible leaves, the two of 0.9.4 plus the six gated
  leaves, where it said "exactly two".
- The ledger gate requires at least one code site per claim. AGENTS.md section 7.1
  documents the ledger schema, the verdict semantics, the six gate rules and the four
  procedures.

## [0.10.1] - 2026-09-03

This release was verified against a real Claude Code 2.1.258 session on Windows 11 ARM64,
a Sonnet 5 build with four subagents and a dev server left running as a background task.
The 0.10.0 release matrix never exercised that shape. Its Windows suite was green while the
pid probe was compiled out there.

### Fixed

- `status` and `wait` read the registry's `shell` status as a running shape. The harness
  writes `shell` only as `idle` relabeled while a background shell task is open. A session
  that had ended its turn with `npm run dev` running reported `running`, and
  `wait --until stop` could not fire even under a lens that ignored the dev server. `shell`
  now reads as the idle-with-background-shell shape it is. It gives the seventh verdict when
  the scan counts the task, and `idle-eot` when the lens excludes it, with a note either
  way. `busy` is the only registry running signal.
- `status` and `wait` now have a pid probe on Windows. `procStart` in a Windows registry
  row is a FILETIME tick count (100ns since 1601) and not an asctime string, so the old
  parser silently skipped the reuse guard. The probe now reads the owner's start time
  through PowerShell `Get-Process`, falling back to `tasklist` for liveness alone. It
  compares instants with the same 2s tolerance and reports `stale-dead` for a dead owner. A
  row whose `pidDomain` names another domain (`darwin`, `linux`, `win32:<host>`) is never
  probed, and the verdict says so.
- In `status` and `wait`, a child lane whose completion notification already landed in the
  main transcript is `settled` whatever its tail says. A subagent that finished seconds ago
  no longer counts as a live lane for up to 300 seconds.
- `plan`'s slug-only binding resolved a relative `plansDirectory` against the config home
  and took an absolute one verbatim. Claude Code resolves it against the project root (the
  session's cwd) through the merged settings scopes. It refuses a value that escapes that
  root and falls back to `~/.claude/plans`. csift now does the same, reading the user,
  project and project-local settings in that precedence. The introspection ledger's first
  audit caught this one.
- In `agents`, a `/fork` child reported depth 65. Its transcript is a clone of the
  parent's and carries the spawning tool_use itself. The spawn join named the child as its
  own parent, and the depth walk ran to its cycle cap. csift now reads the `parentAgentId`
  the harness writes into the child's meta.json, and it never accepts a node as its own
  parent. Two more findings came from the audit. The parent's record of a subagent return
  carries an appended continuation footer and no truncation, and the help and docs are
  corrected. recover's Bash read anchors already reach built-in and teammate lanes, whose
  results carry the `toolUseResult` echo. Only workflow lanes lack it, and the comment is
  corrected.
- In `search`, an inbound peer message relayed mid-turn carried no label at all. Claude
  Code 2.1.258 relays under three preambles. They are "Another Claude session sent a
  message:", the same "while you were working:" form, and "A peer session sent a message
  while you were working:". csift knew only the first, so 29 of 47 `<agent-message>`
  records in the reference corpus were invisible to every census. All three are section
  boundaries now. The corpus-wide `agent.communication.inbox` census moves from 8721 to
  8750 records.
- In `search`, the third AskUserQuestion answer phrasing that Claude Code 2.1.258 writes
  ("The user answered: ...") is an answer marker and a turn opener. The unanswered branch
  ("The user did not answer the questions.") never is. The retired "User has answered your
  questions" form stays recognised for older transcripts.
- In `status` and `wait`, the registry reader takes `procStartFt` when a row carries it
  beside `procStart`. That matches the harness's own `procStartFt ?? procStart` readers.

### Added

- `waiting-hitl` gains two instruments beside the elicitation sidecar. One is the
  registry's `waiting` status. The harness sets it for any blocking dialog, including
  permission prompts, plan approvals and sandbox or worker requests. The other is an
  unreturned `AskUserQuestion` / `ExitPlanMode` at the main tail. Claude Code 2.1.258
  writes a multi-question AskUserQuestion to the transcript at question time. A
  single-question one still stays buffered until answered, and the sidecar covers it. The
  idle-verdict honesty note now names the registry status it saw, where it used to call
  every permission prompt invisible.
- `INTROSPECTION.json` is a structured ledger of every Claude Code behavior csift depends
  on. Each entry is one claim, with its code site, its verbatim snippet, the version it was
  pinned against, and a per-release check record carrying the instrument, the observation
  and the counting rule. A pre-commit gate ties the README's "verified against Claude Code"
  badge to the ledger. The badge version is admitted only when every claim carries a check
  at that version. Snippets must still exist in their files, and check evidence must be
  claim-specific.
- The README gains badges for the verified Claude Code version and the crate-wide mutation
  score. The score is cargo-mutants over the whole crate, killed mutants over every viable
  mutant, counting a mutant that hangs the suite as killed. The release notes carry the
  caught-only floor and the timeout share.
- `harness.meta.system` is the 34th label. It is a gated catch-all for every other
  `type:"system"` subtype the harness writes for its own UI and never sends to the model.
  Examples are the `informational` warning that Remote Control disconnected after an account
  switch, `api_error`, the model-refusal fallbacks, `agents_killed`, `local_command` and
  `scheduled_task_fire`. It renders as `[<subtype> <level>] <content>`. Like the other
  promoted leaves, it is scanned only under an explicit `-t`, and `show --line` renders it
  without `--raw`. The compaction boundary keeps its own leaf.

### Changed

- README highlight 8 is laid out as short lines.
- The documentation records the re-measured laws. They cover the registry status
  vocabulary `busy | shell | idle | waiting` and what each value means, the `claude -p` row
  with a null status, and the multi-question AskUserQuestion flush. They also cover the
  Windows record shapes, with both `Bash` and `PowerShell` tools in one session, the same
  background-task result grammar, and task output files under the local temp dir.

## [0.10.0] - 2026-09-02

Sessions stop lying about being stopped. `status` and `wait` now see every background
shell, async agent and Monitor a session launched, `wait` requires a timeout, and the lines
csift used to skip become five new searchable leaves.

- `status` and `wait` now report background tasks. A Bash launched with
  `run_in_background` gets its tool_result within milliseconds. The tail state machine
  paired it at once, and a session idle with a dev server still running read as a clean
  stop. The harness itself writes nothing about a running shell at end of turn. Measured on
  100 turn_duration records emitted with an open shell, there was no shell field at all, and
  the REPL's "N shell still running" lives in process memory. `status` now scans the whole
  main transcript for backgrounded shells, async agents and Monitor arms. It joins their
  task-notification completions by the launching tool_use id across all three carriers (a
  user record, a queue-operation line, a queued_command attachment). It reads launches from
  every lane and completions from the main file only, where they always land. Every open
  task renders a bg row with kind, id, launch instant and age, description or command, and
  the output file's size and last write. Closed ones fold to counts. Not returned is not
  proof of running. Claude Code's own orphan summary says a UI stop, a Monitor timeout or
  agent teardown leaves no transcript marker, and the section repeats that. Measured over
  the corpus, 24 of 3133 launches never returned, 22 of them launched more than a day
  before their session ended.
- `status` and `wait` gain the seventh verdict and the lens. `idle-background-open` means
  the turn ended but background task(s) the lens counts have not returned. It reads as
  neither running nor stopped, and it never satisfies `--until stop`.
  `--background-since WHEN` counts only tasks launched at or after WHEN. It uses the shared
  time grammar, which now has `2mo` and `1y` units, a tolerated leading minus, and the token
  `now` for the command's own start instant. `--ignore-background RE` (repeatable) excludes
  tasks whose command or description matches. Every task is still listed with the rule that
  excluded it.
- `wait` is BREAKING. `--timeout` is required, because a background task can be designed
  never to return, so an unbounded wait on stop was a bug in every 0.9.x. A call without it
  is rejected with that reason. On every exit the report carries the tail state in words,
  and a census of what landed while waiting (tool calls by name, thinking, messages,
  prompts, notifications). It also carries the bg rows and the last prompt and reply as
  excerpts. Both commands print those excerpts under a warning written for a model reader.
  An excerpt is a partial view of the final state, useful only for judging whether a
  background task is still meaningful, and never a review of the work.
- `search` gets two classification fixes. The harness's agents-stopped notice ("N
  background agents were stopped by the user: ..." and its singular form) was counted as a
  human turn. It is `harness.notification.subagent`, never genuine and never a turn opener,
  rendered "[subagent stopped] ...". A Background command pulse is always
  `background-command`. The quoted-name heuristic that routed re-arm and monitor-named
  commands to the monitor leaf predated the real Monitor tool. It produced 40 false monitor
  records on one project against zero genuine pulses. Historical counts for those two
  leaves move by design.
- `search` gains five promoted leaves, taking the taxonomy from 28 to 33. The non-record
  jsonl line types become searchable, classifiable leaves. `user.queued` is a
  queue-operation line carrying the human's text. The queue event (enqueue, popAll, or
  remove with its reason) rides the label zone and JSON `queue_operation` /
  `queue_reason`. `harness.meta.turn-duration` renders the turn_duration record as
  "[turn duration: 1m 5s . durationMs=64911 messageCount=908
  pendingBackgroundAgentCount=2]". That is the structured body behind the REPL's "Done in
  Ns" line, which never lands on disk itself. `harness.meta.away-summary` is the
  model-generated recap shown on return after five minutes away.
  `harness.meta.stop-hooks` is the Stop-hook execution ledger. `harness.meta.snapshot`
  covers file-history snapshot and delta lines. All five are LLM-invisible by the same
  instrument as the compaction boundary, because none carries a message field.
- `search` follows the gated-leaf law. A promoted leaf is parsed only when an explicit
  `-t` reaches it (the full path, a glob, or the `harness.meta` prefix) or a show address
  names the line. A bare scan never parses those lines, measured at 1.03x, which is noise.
  The three fabricated renders register their type value as a synth marker, so the
  whole-file gate stays sound. A zero-match run without such a selector says so.
- `search` measures the queue facts. Content rides enqueue, popAll and most removes, and
  never dequeue. The remove reason is `absorbed_mid_turn` or `delivered_to_agent`. The
  queue line has no join key, so csift never asserts dispatched. This corrects the 0.9.2
  entry. Its "about 61% never become user records" figure counted every queue operation
  over three sessions. Counting enqueue lines only, over six sessions, the human's prose
  reaches a user record 72 to 81 percent of the time.
- `show` renders every promoted line under an explicit `--line` or `--uuid` address, with
  no flag. The miss error names what stays `--raw` only.
- The help and docs cover 33 leaves, the GATED LEAVES rule, the seven verdicts, the
  background and lens paragraphs, the how-to-wait procedure, and the last-messages
  warning.

## [0.9.4] - 2026-09-02

In this release Bash reads become reads and Bash writes become writes in `recover`. Plan
binding matches Claude Code's own rule, `list` names a forked clone's origin, `status`
shows what is actually moving, and metachar regex searches stop paying full price.

- `recover` gains Bash content anchors. The deterministic shell subset now replays as
  first-class content where it used to replay as boundaries. Writes anchor per segment. The
  anchoring writes are a quoted-delimiter heredoc via cat/tee, literal echo/printf, and
  `truncate -s 0`. A quoted-delimiter heredoc body is byte-verbatim in the transcript, and
  an unquoted delimiter is admitted only with an expansion-free body. The dominant real
  shape, write-the-file-then-run-it in one compound command, therefore anchors. A compound
  command additionally demands a clean result echo, with empty stderr and no interrupt. Only
  the last segment owns the exit code, and a failing write always says so on stderr. A
  second touch of the same resolved path anywhere in the command refuses the anchor. Reads
  anchor as single simple commands only (cat, `head -n N`, `sed -n 'A,Bp'`) under the
  completeness gate. A window from line 1 that hits EOF is the whole file. A byte-known
  `>>` append is placed only onto a complete newline-terminated buffer, and otherwise it is
  disclosed as `bash_append_unplaced`. Some shapes are deliberately not anchors, because
  they were measured or cannot be placed. They are tail, `sed -i`, variable targets, and
  interpreter and ssh heredocs. Coverage counts `bash-read-anchor` / `bash-write-anchor`,
  and segment provenance names `bash-heredoc` / `bash-cat` / `bash-write`. A real
  heredoc-then-run python tool went from "no recoverable history" to 33/33 lines recovered
  verbatim.
- `plan` gets a correctness fix. csift bound only via the `plan_mode` attachment, while
  Claude Code itself binds by the first record carrying a valid slug. On a forked clone,
  with attachments stripped and slug records kept, csift answered "no plan" for a session
  whose plan Claude Code will re-inject. Two binding laws now apply in precedence order.
  Rows carry `binding_source` ("plan_mode" | "slug-only") and `minted_at_compaction`, which
  marks a slug whose first carrier is a compaction boundary, the fork mint site.
  `plansDirectory` from settings.json is honored.
- `list` reports clone lineage. A transcript whose first timestamped record is a
  `compact_boundary` was minted by copying another session at that compaction. That is a
  background-job fork, with uuids preserved, timestamps predating the file and the slug
  stripped. The rule showed zero false positives on a 61-file real dir, and file-birthtime
  rules were refuted. The row annotates the fork and names the origin session. A prose quote
  of the boundary uuid or a co-clone can never win the join. JSON gains `is_clone` /
  `clone_of` / `clone_boundary_uuid`. A documented corollary follows. A clone double-counts
  its inherited records on every spanning surface until it is scoped away.
- `status` child lanes gain a `generating` state. A tail record younger than 300s whose
  last assistant `stop_reason` is not `end_turn` is mid-generation. Measured, intra-lane
  record gaps reach p99.9 = 295s, while dead lanes sit 31h+ out. The old 15s mtime window
  misread one lane in 17, and `stop_reason` alone would mark 73% of dead lanes live. The
  mtime `active` state is retired. Settled lanes fold to a count, so JSON `children[]`
  carries live lanes only, beside `settled_children`. A tasks section reads the harness task
  list, with open tasks in_progress-first with their blockers and completed tasks folded to
  a count. The status help schema line also drops `since_utc`/`since_local`, which the
  verdict row never emitted.
- `search` extracts required needles for its prefilter. A pattern with metacharacters now
  derives a necessity-only literal set from its parsed structure, and an alternation gates
  only when every branch demands a safe needle. `TodoWrite.*legacy|legacy.*TodoWrite` runs
  1.70x faster wall and uses 1.9x less CPU unscoped, with byte-identical output. A
  space-carrying plain pattern now anchors its longest whitespace-free run.
- `files` and `recover` gain the file-history snapshot instrument. Claude Code rewrites
  its settings files in-process (`/model`, `/config`, plugin toggles) with no tool record.
  Measured, half of all settings.json mutations are invisible to the tool stream. One such
  write silently deleted a freshly-edited key while `recover` replayed the file with it, and
  called a state that never existed 100% complete. Claude Code's own per-prompt snapshot
  version sequence is now read as an instrument. `recover` compares the replayed buffer to
  mtime-verified snapshot content at every version change and rebases on divergence, which
  is an authoritative `external_write` boundary. Content-less jumps disclose the same
  boundary without rebasing. `files` emits "external write" timeline rows, with the scope
  hard-limited to the settings family (`.claude/settings*.json`). The tracked set spans 1701
  corpus paths against 11 settings-family ones. The version counter resets mid-session (148
  real cases), and the `@vN` store name collides across a reset, so generations are
  segmented and unverified blobs refused.
- `search`'s bare role selectors now speak LLM-visibility, which is a correctness fix.
  `user.unsent` under `-t user` broke a 0.7-era consumer, when a superseded draft 12 seconds
  before the real submit poisoned a last-human-touch hook. A bare role (`-t user`) selects
  the role's LLM-visible leaves only. A new glob form (`-t 'user.*'`) selects every leaf
  under the prefix. Intermediate prefixes and full paths keep their full sets, so
  `-t harness.compaction` still reaches the boundary. Exactly two leaves are invisible,
  verified by instrument. `user.unsent` is invisible because Claude Code's own
  `preservedMessages` accounting excludes every draft uuid, and a draft is not in the
  surviving conversation. `harness.compaction.boundary` is a metrics-only system record.
  `-t user` restores the 0.7 contract, after 0.9.2 through 0.9.3 briefly included drafts
  under it.
- SKILL's staleness guard is mechanical. The skill runs `csift --version` at first use
  after any compaction, and a mismatch means the in-context copy is a stale echo. The
  description gains the corpus-first trigger (live sessions need `/reload-skills`). Two
  stale claims are fixed. The turns rename is v0.4, and the retired `@trap` retry ritual is
  gone from the whoami section.

## [0.9.3] - 2026-09-01

This release corrects help text and catches the documentation up. No behavior changes.

- The root help's hand-written SUBCOMMANDS block still listed eleven commands, which
  contradicted the generated list one screen below. `status` and `wait` join it and the
  span-default enumeration.
- The `status`/`wait` help described owner-process liveness as a signal-0 probe, a
  mechanism that never shipped. It now describes the ps-based probe with the `/proc`
  fallback, the 0.9.1 mechanism.
- The `--siblings` policy text gains the narration cap of 1 shipped in 0.9.2, in the help
  and in every reference document.
- The README catches up two releases. It lists thirteen subcommands with `status` and
  `wait` rows, adds a live-status highlight, adds Quickstart rows for status / wait /
  `user.unsent` / the label census, and puts the 0.8.1+ flags in their rows. SPEC gains
  per-command sections for `status` (6.13) and `wait` (6.14) and rewrites the `stats`
  section to the deduped token accounting. SKILL's superseded-draft bullet and JSON
  reference catch up to 0.9.2.

## [0.9.2] - 2026-08-31

This release makes classification narration-aware and corrects the token accounting.
Saved numbers move in two places, both on purpose. Label censuses split, and `stats` token
totals drop.

- A new label, `agent.thinking.narration`, takes the taxonomy from 26 to 27. Since at
  least Claude Code 2.1.170 the API can return a second thinking block in an assistant
  message. It is a one-sentence, user-language summary of the reasoning beside it,
  distinguished only by a tag encoded inside the base64 `signature` (protobuf field path
  2 -> 1 -> 8, the last field at each level). Clients 2.1.241+ render it dim under the hint
  word `summarized`. csift counted these as reasoning. Classification is by signature
  alone. About 4 percent of narration blocks have no reasoning sibling, so adjacency is
  never consulted. The whole signature is decoded, and signatures reach 200K+ base64 chars.
  Every failure path degrades to plain `agent.thinking`, and the tag set is open. Hits
  carry a `[narration summary]` marker in the label zone. Narration gets a sibling cap of
  1. `verbatim` never replays narration, which was already true by construction and is now
  pinned. `-t agent.thinking` still selects both leaves, and pure reasoning is
  `-t agent.thinking -T agent.thinking.narration`. Historical records re-classify by
  design. Narration blocks exist on disk from 2026-06-10, so saved `--count-by label`
  figures change while the sum is conserved.
- A new label, `user.unsent`, takes the taxonomy to 28. A message that was sent,
  esc-recalled into the input box, edited and re-sent leaves the original on disk, sharing
  the resend's `parentUuid`. Since 0.8.2 csift collapsed it to a bare count. Drafts are now
  searchable and censusable under their own leaf. A matching draft renders as its own
  annotated unit (`<tok>·draft`, JSON `superseded_draft:true`, null `turn_index`, hits
  labeled `user.unsent`). Turn numbering is untouched, `--turn` windows suppress draft
  units, and `user.message` counts are unchanged, since drafts were never censused before.
  Two limits are documented, and both were measured. A recalled-then-abandoned message has
  no resend sibling and is structurally undetectable. A queued text edited before dispatch
  never becomes a user record at all. Its bytes survive only in `queue-operation` lines,
  which csift treats as non-records, reachable with `--raw`, and a future release may gate
  them.
- `stats` token sums are corrected. Claude Code repeats the identical `message.usage`
  object on every per-block record of one API message, and `stats` summed per record. That
  over-reported by 2.2x to 3.5x per field and model. Sums now dedupe per transcript by
  `message.id`, taking the per-field MAX, which is immune to the compaction-replay shape that
  rewrites an id with zeroed usage. Id-less records count individually as before. The scope
  TOTAL still sums transcripts, because the same id recurs across a session's transcripts
  with genuinely different per-file usage. Printed totals drop accordingly.
- `stats` gains a narration census. `narration_blocks` per model gives block counts only,
  because the token split is not derivable from the jsonl and none is invented.
  `unknown_thinking_tags` surfaces a signature tag that is neither `thinking` nor
  `narration` without a csift release.
- The narration decode is byte-gated on the hot path. A signature containing none of the
  tag's three base64 alignments skips the decode, which keeps large-corpus search at its
  previous speed.
- Stale help is swept. The `-t` reference said "25 leaves" and had omitted
  `harness.meta.attachment` since v0.8.1.

## [0.9.1] - 2026-08-30

- Fixed pid liveness on busybox-ps hosts (Alpine and friends). busybox `ps` rejects `-p`
  and the `lstart` field outright. The probe's ps form therefore failed for a live pid
  exactly as for a dead one, and `status` read a live session as `stale-dead`. When the ps
  form fails, the probe now consults `/proc/<pid>` on Linux. Present means alive, with the
  start time unknown, and the reuse-guard skip stays disclosed. Absent keeps the
  no-such-process verdict. The release matrix's musl test lanes found it.

## [0.9.0] - 2026-08-30

This release adds a new command class, `status` and `wait`, the live-truth pair. Every
other command answers "what happened" reproducibly. These two answer "what is happening
NOW". They are point-in-time and explicitly non-reproducible, a deliberate, documented
departure from the forensic contract.

- `csift status <target>` gives a one-shot liveness verdict for a session, one of
  `running` | `waiting-children` | `waiting-hitl` | `idle-eot` | `stale-dead` | `unknown`.
  It comes from a three-way join and never from a single-surface inference. The first
  source is the harness session registry (`<claude-home>/sessions/<pid>.json`), written on
  transitions only and never as a heartbeat. The second is the transcript tail state
  machine, where an unreturned tool call at the tail means a tool in flight. The third is a
  `ps`-based owner-pid probe, guarded against pid reuse by the process start time. The
  registry renders that time in UTC and `ps lstart` renders it local, and both parse as
  instants. Child liveness joins each subagent transcript's own tail with the incremental
  workflow journal, where `started` minus `result` gives the agents in flight. The
  elicitation sidecar covers human-in-the-loop blocks. Every verdict ships its evidence
  rows, and every degradation is stated in the output. That covers a skipped reuse guard, a
  missing registry row, and the invisible pending permission prompt.
- `csift wait <target> --until COND[,...]` blocks until a condition fires, and the first
  hit wins. The closed condition set is `stop`, `hitl`, `auq`, `notification[:REGEX]`,
  `tool:NAME[:REGEX]`, `write:PATH_RE[:LINE_RE]` and `verdict:V`. It has strict post-start
  baseline semantics. Only bytes appended after the watch starts count as events, and
  history is `search`'s job. A readiness line on stderr makes scripted waits race-free
  against their own trigger. Polling is incremental, with byte offsets and torn tails held.
  It is also adaptive, from a 200ms floor to a 2s ceiling, and `--interval` overrides it.
  Child lanes and the elicitation sidecar born after the watch starts join it automatically
  with a zero baseline.
- `wait` exits 124 on `--timeout` expiry, the GNU `timeout` convention. That is the one
  documented exception to the crate's 0-vs-non-zero exit law, and it applies to no other
  command.
- `Message.stop_reason` joins the tolerant record model.

## [0.8.2] - 2026-08-30

This release fixes field incidents and makes one soundness correction. Every item traces to
a measured incident or a live re-measurement. Two items correct csift's own documentation
and output where they stated a wrong mechanism or a fabricated certainty.

- Targeting resolves a bare-basename `*.jsonl` token (no path separator) and classifies it
  correctly on every command. It used to fail with a wrong error, and a bare
  `agent-<hex>.jsonl` was misread as a top-level session.
- The `@trap` timing mechanism was documented wrong, and it is corrected everywhere,
  including the runtime error text. A subagent transcript flushes per content block, so the
  record is on disk at dispatch and a first try resolves. The main conversation's record is
  an async flush of the completed assistant message, landing about 1-3.4 seconds after
  dispatch. That is a race and not a wait. The no-match error now routes `@main` first. A
  `@trap` that resolves to the main transcript prints a stderr lane note and no longer
  succeeds silently.
- Lane honesty is restored. Bare `whoami` reported `is_subagent:false`, `depth:0` and an
  echoed parent id from inside a subagent. Those were three confidently wrong fields on
  exactly the command an agent runs to check its identity. The env form now reports those
  fields as null, because the env names the top-level session in every lane and the answer
  is unknowable. It prints a lane line in text and notes the resolution path on stderr.
  Every `@main` resolution prints the same unconditional stderr note.
- Images are easier to discover. The first image-bearing row per run (search and show)
  carries a paste-ready extraction hint in input id forms, with a `#N` handle as the bare
  number. The search footer gains a capability note. `search --help` and `verbatim --help`
  gain SEE ALSO sections naming `csift image`. A fleet incident drove this one. It
  published "images are unreadable" without ever testing extraction.
- Errored tool results are visible. Hit JSON carries `is_error`, the text render decorates
  an errored result with `[error]`, and a new `--count-by result` axis buckets `ok` |
  `error`. The closed `pairing` enum is untouched. Pairing answers "did a result come
  back", and result answers "was it good".
- Superseded drafts are disclosed honestly. The esc-edit draft collapse in turn
  reconstruction is now disclosed, in the search footer and in JSON `superseded_drafts`. An
  explicitly addressed draft fetches via `show --line`/`--uuid` as an annotated unit outside
  turn numbering. The address-miss error states the real render domain. Before this, a
  multi-megabyte genuine user record could vanish from every scan with no count and fail an
  addressed fetch with a wrong reason.
- SKILL gains a "Why not hand-roll this format" section, placed before the routing table.
  It lists nine measured traps, each returning a plausible wrong answer with no error and
  each mapped to a csift move. The frontmatter description now carries the cost of skipping
  csift and names the image capability.

## [0.8.1] - 2026-08-27

This is a maintenance release of additive surfaces plus one correctness flip. Each was
verified against real corpora, and against the live on-disk stores, before implementation.
Two proposed features failed that verification and shipped as fact-reporting surfaces
instead. A live/abandoned rewind-branch classifier false-positived on parallel tool fan-out
across most sessions. It shipped as `show --branch-points`, with facts ranked by
inter-child gap and no verdicts. A file-history reconstruction merge was cut down to a
listing after the store proved pruned, with reused version counters. It shipped as
`recover --list-backups`.

- `list` reports `version` and `git_branch` as the last-seen value, which is what the
  session is on now. The docs always promised the session's version, and a mid-flight
  upgrade or branch switch used to report stale opening samples. The opening values ride new
  `version_first` / `git_branch_first` fields, and JSON mirrors `*_last`. Text shows a
  drift arrow (`branch a->b, CC x->y`) when they moved. `cwd` stays first-seen on purpose,
  because the record cwd follows the tracked shell cwd.
- `search --attachments`, the `harness.meta.attachment` label and `--count-by attachment`
  make attachment records searchable behind an explicit gate. Those records are the bulk of
  many transcripts' bytes. The gate is a superset of `--additional-context`. The matchable
  text is the verbatim payload JSON, and the census axis implies the gate. A default scan
  still never parses attachment lines. An explicit `show` address renders any attachment
  record flag-free. The label taxonomy grows to 26 leaves.
- `search --count-by version` is a per-record census of the Claude Code version stamp. It
  shows which versions a session ran under and where an upgrade landed. Stampless records
  are excluded and disclosed.
- `stats` gains a whole-file line-type census (`types` line, JSON `line_types`, merged
  scope totals). It counts every physical line by its top-level `type`. It is a file fact
  like `lines` and is never windowed. The probe fully validates non-candidate lines, so a
  framed line with an invalid interior is now counted malformed even off the candidate
  path.
- `recover --list-backups` lists Claude Code's own file-history checkpoint store for an
  absolute `--file`, where the store key is the sha256 of the path. It orders entries by
  backup instant and states the provenance bounds in the output. Only tool-layer writes
  land there, the store is pruned, and version counters reset per session dir. It is a
  listing only, and checkpoint content is never merged into a reconstruction. Four doc sites
  claimed `backupFileName` is frequently null, and they were corrected (measured 83-98%
  present).
- `show --branch-points` lists every record with two or more conversation children, such
  as a rewind, a retry or a parallel lane. It shows the children with lines and timestamps,
  ranked by the widest inter-child time gap. Tool-result carriers, isMeta records and
  compaction summaries never count as children. It reports facts only. csift ranks and never
  classifies which side is live. The compaction boundary's `logicalParentUuid`, the true
  predecessor the compaction re-links to, now rides the boundary's rendered excerpt.
- `plan` binding output gains the plan `slug`, read off the bind record. `plan --audit`
  joins the scope's structured plan-file mutations against the corpus's plan bindings. It
  warns when the mutating session does not bind the file, because only the bound plan is
  re-injected after a compaction.
- `agents` reports fork provenance (`fork_parent_last_uuid`, `fork_context_length`, a
  `forked-at` text line) from the `fork-context-ref` record a `/fork` transcript opens
  with. It gains a repeatable exact-match `--agent-type` filter, and `--agent-type fork`
  lists fork children.
- One item is recorded as out of scope. The live team/task coordination files stay unread,
  because the transcript is the durable record.
- sha2 is a new dependency, for the checkpoint-store key.

## [0.8.0] - 2026-08-22

Bash file mutations now resolve against the shell cwd Claude Code itself records, and every
`recover` output accounts for what the replay could not include. A full-corpus
investigation grounds the work. It joined 22,410 real Bash commands against csift's own
output, and it extracted the cwd and freshness mechanisms from the Claude Code 2.1.237
binary and validated them on 18,185 commands (SPEC 4.9).

The improvement was measured on that corpus. 19.4% of Bash calls mutate a persistent file.
csift's full-target attribution of those mutations rises from 27.1% to a measured 77.0%
ceiling. `recover`'s join of a relative operand against an absolute `--file` closes from
95.84% to 99.65%.

The resolution stays deterministic, and nothing is guessed. Every resolved path carries an
explicit resolution class. `absolute` means typed absolute. `cwd-joined` means joined to the
record's own `cwd` field, data Claude Code wrote, with zero inference. `cd-tracked` means
literal in-command cds, a lexical inference validated at 99.65% against Claude Code's own
modified-file hints. `unresolved` means kept verbatim and disclosed, and never fabricated
into a path. Commands whose file sets are not in the command text are counted and
disclosed, and never attributed.

- `files` uses the resolved spelling as a bash row's `path`, so relative and absolute
  spellings of one file share every bucket. Timeline JSON rows gain `resolution`,
  `path_verbatim` and `command_errored`. Mutations from a partially failed bash chain are
  kept and flagged, where they used to be dropped. `git apply --check` / `git clean -n` dry
  runs and rsync remote destinations no longer emit rows.
- The parser gains several increments. It reads `perl -i`, the `sed -i` twin. It reads
  interpreter write idioms (python/node/ruby heredoc and inline scripts), with literal and
  one-hop-constant targets as real rows and an `interp:<lang>` marker otherwise. It keeps
  leading-`~` operands verbatim as `unresolved`, where it used to drop them. It adds the
  mutating-class markers `fmt:<tool>`, `pkg:<manager>` and `extract:<tool>` in the
  `git:<sub>` style. Dry runs emit nothing, and a formatter with named operands emits real
  rows.
- `recover` joins bash events on resolved paths, keeping the verbatim path as a fallback.
  Per window and in every mode it discloses integrity boundaries with a hard/soft split,
  opaque mutating-class and PowerShell command counts and rows, and a ready-to-run
  time-bounded `csift search` command. Restore's status states a clean window positively.
  Otherwise it says "complete from the tool stream; NOT verified against disk" and lists
  what was not replayed.
- csift adopts Claude Code's own freshness signals. `staleReadFileStateHint`, where Claude
  Code names the files a shell command modified, becomes a hard `hint_modified` boundary.
  `staleRecovered` on a successful Edit becomes a `stale_recovered` annotation. The
  over-budget `edited_text_file` form is named. An external-edit boundary names a
  formatter-class command that ran in its window. `String to replace not found` and
  `File does not exist` are counted annotations. A soft bash boundary no longer disarms the
  originalFile cross-check.
- This is breaking. The batch `recovery-report.tsv` gains `boundaries`, `bash_file` and
  `bash_opaque` columns. Restore's status lines and failure diagnostics are reworded. An
  invalidated history no longer claims "never Read/Written/Edited", and an empty salvage
  names "at the latest state". Restore's JSON error paths emit their row and summary before
  the non-zero exit. Boundary JSON rows gain `source_session_id`/`source_line`. Coverage
  rows gain `hard_boundaries`/`soft_boundaries`/`opaque_commands`/`powershell_commands`/
  `suggested_search`.

## [0.7.8] - 2026-08-17

- `recover` finds windows-shaped paths (drive letters, backslashes) again. The file-level
  basename prefilter split on `/` only, so such targets silently reported no history. The
  basename-suffix match also accepts a backslash boundary now.
- This is the first release validated on all three platforms. The full test suite passes
  on macOS (arm64/x64), Linux (glibc and musl, x64/arm64), and Windows (MSVC arm64/x64).

## [0.7.7] - 2026-08-17

- Help text is reworked for plain punctuation across every `--help` page. Flag semantics,
  examples and JSON schemas are unchanged.
- Source comments are ASCII-only, enforced by the pre-commit gate. Output glyphs and test
  fixtures in string literals are unaffected.

## [0.7.6] - 2026-08-17

- `search --additional-context` is an opt-in scan of hook-injected additionalContext, the
  attachment records a SessionStart / UserPromptSubmit / ... hook writes into the
  transcript. It is off by default, and hits surface under `harness.meta.hook`. An explicit
  `show --line`/`--uuid` address renders such a record without the flag, so the refetch a
  search hit prints always resolves. A default scan pays nothing for the widening.
- The README is restructured scenario-first, in the order why, highlights, install, with
  the pronunciation under the title and the agent-skill install beside the binary install.
  SKILL documents the file-mtime semantics of Claude Code's `cleanupPeriodDays` retention.
  Conventional Commits are codified in AGENTS.md. .gitignore gains editor, OS and
  local-settings rules.

## [0.7.5] - 2026-08-16

- csift is published to crates.io, and `cargo install csift` is now the primary install
  path. Cargo.toml gained the publication metadata (repository, readme, keywords,
  categories) and dropped the `publish = false` guard. The `CLAUDE.md` symlink is excluded
  from the crate tarball, because it would dereference into a duplicate of `AGENTS.md`.
- The README documents the name and the crates.io install. csift is pronounced "c-sift",
  in the `csplit`/`ctags` naming tradition, with c for Claude Code and sift for what it
  does. No CLI surface changes.

## [0.7.4] - 2026-08-12

- The Windows shell is a separate Claude Code tool named `PowerShell`, with the same
  `input.command` field. It is enabled by env override, forced on when Git-for-Windows bash
  is absent, and otherwise feature-gated, as extracted from the CC 2.1.228 binary. The
  Windows `Bash` tool runs the real Git-for-Windows bash. `@trap` self-identification now
  matches both shell tools. It was blind exactly in the bashless Windows fallback mode.
  Error and retry guidance says "shell (Bash / PowerShell) invocation".
- Some non-changes are deliberate and documented. The bash-lexical layers, which are
  dangerous-rm escalation classification and shell-side mutation attribution in
  files/recover, do not run on PowerShell records. A pending PowerShell lane classifies as
  awaiting-execution, and structured Read/Write/Edit attribution is unaffected. One more
  finding is recorded. CC 2.1.228's dangerous-rm has evolved past the ported generation,
  with fixpoint substitution stripping and a tree-sitter bail at 64+ command substitutions.
  A port refresh is a tracked follow-up.

## [0.7.3] - 2026-08-12

- Path encoding is now exactly Claude Code's, with evidence extracted from the CC 2.1.228
  binary. The cwd is NFC-normalized and then replaced per UTF-16 code unit. An NFD-spelled
  accented path now encodes identically to its NFC spelling. That closes a formerly
  documented divergence that resolved the wrong dir on macOS NFD paths. An astral char
  yields two dashes, matching the JS regex's view.
- Windows drive-encoded project dirs (`C:\Users\x` → `C--Users-x`, letter-led) are
  first-class targets. Both the bare positional token and `@C--Users-…` resolve. A
  drive-shaped token matching no projects dir falls through to real-path resolution. A
  UNC-encoded dir (`--server-…`) is targeted via the `@` form, and the mistyped-flag guard's
  error now says so.
- The same binary verified one more fact with no code change needed. CC's config home is
  `CLAUDE_CONFIG_DIR ?? os.homedir() + "/.claude"` (NFC-normalized). Windows never consults
  `HOME`, which confirms the 0.7.1 per-platform split.

## [0.7.2] - 2026-08-12

- This is a performance release, and behavior is identical. A 28-command byte-exact A/B
  battery pins stdout, stderr and exit codes unchanged. Every per-line byte prefilter now
  uses construct-once memmem finders, where the stateless form rebuilt its searcher on every
  call. The parallel line scanner no longer runs a serial whole-file newline count. It skips
  the count outright for single-chunk files and computes it in parallel otherwise.
  `search`'s per-turn match+render phase fans out on rayon when the scope is small or the
  file is 64 MB+, the straggler class, gated so broad scans keep the serial walk. Measured
  warm on the reference corpus, the speedups read 1.21x for the big-session census, 1.13x
  for no-match unscoped, 1.09x for a caseless literal and 1.08x for verbatim. User CPU fell 2-4 percent.
- The README gains a coverage badge, 94.8 percent line coverage by cargo-llvm-cov over the
  full suite.

## [0.7.1] - 2026-08-12

- The default data root now resolves per platform, the way Claude Code's own
  `os.homedir()` does, which is a correctness fix. It is `$HOME/.claude` on Unix and
  `%USERPROFILE%\.claude` on Windows. `HOME` is never read on Windows. A stray
  Git-Bash/MSYS `HOME`, often a POSIX-style path a native process cannot open, no longer
  points csift at a `.claude` dir Claude Code never writes. Precedence is unchanged, as
  `--claude-home` > `$CLAUDE_CONFIG_DIR` > the OS home's `.claude`. The error message and
  the `--claude-home` help name both variables.

## [0.7.0] - 2026-08-12

This release breaks the text surface. JSON output is unchanged.

- This is breaking. `search` exchange headers are self-resolving. Each header opens with a stable
  id-prefix token, the first 8 chars of the owning transcript id (`<tok>·t<N>`), in place
  of a per-invocation `sN` ordinal. The `sN = <id>` session legend block is removed
  entirely. A token is a valid `@` target as-is and identical across invocations. Within one
  output, distinct ids sharing their first 8 chars lengthen together (8 → 12 → full id). A
  teammate id is name-embedded and not hex-led, and it renders whole. A subagent exchange
  carries `(parent <first-8>)` on every header.
- The resolver widens so every emitted token round-trips, and every case fails loud on
  ambiguity. The `@`-prefix match domain is the union of top-level session uuids and
  subagent agent ids. A literal `8-4-4-4-12`-layout prefix longer than 11 chars is a valid
  uuid-prefix token. A 12+-hex token keeps exact-agent-id semantics first, then falls back to
  a unique literal-prefix match.
- The output geometry changes. A head `matches` banner follows the scope banner. It shows
  the true totals, `oldest first`, the emitted window, and `undated last` when present. The
  tail footer repeats the true pre-cap totals beside its drop accounting. The stderr
  zero-match diagnosis discloses the malformed-line count, because an absence claim is
  definitive for parseable lines only. The both-ends placement law joins SPEC section 0 as a
  crate-wide design law.
- `--max-count` is signed. `N` keeps the earliest N of the chronological stream, `-N` keeps
  the latest N, and `0` stays uncapped. The kept exchanges still emit oldest-first among
  themselves. Both ends disclose the window, and the footer names the dropped side
  (`N later|earlier dropped by --max-count`).
- The docs gain an OUTPUT GEOMETRY section in `search --help` and SKILL. They add recipes
  for "when did X first happen" (`--max-count 1`), "most recent occurrence"
  (`--max-count -1`), and the header-token follow-up into `show`.

## [0.6.10] - 2026-07-14

- `@trap` retry guidance states the granularity. The retry must be a new, separate Bash
  invocation. Two attempts inside one shell script are still one in-flight tool_use, so both
  miss. The error text, SKILL and the assumption table say so.
- The docs state that every `tool_use`'s matchable text is its name plus the re-serialized
  JSON input. An embedded real newline is therefore the two-character `\n` by match time,
  so a pattern has to match the literal `\\n`. `--multiline` is correctly irrelevant
  there.
- SKILL is completed. It names the verbatim header fields `automation_triggers`,
  `budget_is_per_session` and `sessions_rendered`. It records the self-echo trap. A nonce
  used as a search pattern writes itself into your own live transcript, so an absence check
  has to be scoped away from your own session.

## [0.6.9] - 2026-07-14

- Stage-1 candidate detection is serialization-tolerant, which is a correctness fix. Some
  valid-JSON records are serialized differently from the compact wire format, with
  whitespace around the colon, as python `json.dumps` defaults and a jq or editor
  round-trip produce. Such a record used to vanish one layer before any malformed counter,
  with no match, no count and zero disclosure. The role needles now route through shared
  whitespace-tolerant matchers. Every other prefilter needle is serialization-safe by
  construction, and the needle law is codified in AGENTS.md. Framing is unchanged, with one
  record per line.

## [0.6.8] - 2026-07-14

- `list`/`agents` head and tail scans no longer double-book malformed lines. The tail scan
  floors at the head scan's consumed end, so the two windows are disjoint, and every
  malformed line in them is counted exactly once. An all-garbage file used to report
  exactly 2×.
- `list`'s malformed count is a disclosed window census and never a whole-file verdict.
  The note reads `… skipped (among the head/tail lines read — full census: csift stats)`,
  and `stats` is named the full-scan census authority.
- A sidecar marker line the current schema cannot read, a pre-release fossil under old field
  names, is counted as malformed. A line that is provably csift's own and still cannot be
  interpreted never buys silence.

## [0.6.7] - 2026-07-14

This release converges the docs only.

- SKILL names verbatim's two automation header fields. `automation_by_kind` holds the
  selected triggers per class. `automation_in_scope_by_kind` holds every in-scope pulse
  regardless of budget.
- The semantics of `agents` `returned_message` are stated. It is the orchestrator's record
  of the child's return and not the agent's own conclusion. The child's final words are
  always `show @<agent-id> --turn -1..`.

## [0.6.6] - 2026-07-14

- Obviously corrupt lines are counted, which is a correctness fix. Every byte-prefilter
  rejection path runs an O(1) shape check, and a line that is non-blank but not
  `{…}`-framed counts as malformed. Free-text garbage and crash-truncation therefore move
  `skipped_lines` on every command. The documented residue is that a `{…}`-framed invalid
  interior is only counted on a parse candidate.
- `verbatim`'s header reads `spanned K of N compaction boundaries in scope`, because K
  alone read as a transcript property. Its JSON header carries the full budget accounting
  (`round_trip_fraction`, `chars_used`, `boundaries_*`, `selected_*`).
- The docs state three rules. Under `--turn` and time windows every `stats` figure
  windows except `lines`. An hours-old `awaiting-execution` lane is overwhelmingly an
  abandoned parent session, so `pending_since_utc` deserves weight. `--siblings` caps apply
  to non-matching context records only.

## [0.6.5] - 2026-07-13

- Bare ISO datetimes are local wall-clock time, which is a correctness fix.
  `--since "2026-07-13T20:00:00"` used to collapse silently to local midnight, because the
  civil-Date parser kept only the date part. A civil-DateTime arm now precedes the Date arm.
  A string carrying a malformed offset still bails. One fix covers every
  `--since`/`--until` consumer.
- The id trio (`session_id` / `is_subagent` / `parent_session_id`) rides every search hit
  and sibling object, so bare `.hits[]` flattening keeps real ids.
- Advisory notes fire after target resolution, so csift never warns about a run that was
  never going to happen.
- SKILL adds the missing `plan` / `recover` / `image` JSON row schemas and states `@trap`
  marker uniqueness as conversation-wide.

## [0.6.4] - 2026-07-13

- The removed `turns` name gets a tombstone error. A hidden variant always bails, naming
  the rename (`verbatim`, same flags), and routes plain turn reading to
  `show <target> --turn -3..`. It is a wall and never a shim, and it never runs.
- `agents` text brands a non-completed lane's `returned_message` inline
  (`history — predates the still-open lane, NOT the outcome`). A completed lane stays
  unbranded.
- The docs state four more facts. A workflow run row's `status` is journal-verbatim, an
  open set and not a csift enum. The richest-view dedup rule is stated mechanically.
  `labels[]` is richest-first, and the rendered view is the first label surviving
  `-t`/`-T`. The de facto exit codes are 2 for usage errors and 1 for csift errors, and the
  contract stays 0-vs-non-zero. The record-level jq pipeline idiom selects in jq and then
  runs the csift-generated `refetch`.

## [0.6.3] - 2026-07-13

- The elicitation sidecar gains a ghost-pending guard, which is a correctness fix. Claude
  Code fires no PostToolUse for a rejected AskUserQuestion/ExitPlanMode, so the hook can
  never write `resolved` there. A pending whose key appears on a native record as an actual
  `tool_use` block id / `tool_result` id is dropped like a resolved pair. The check is
  structural, and the native transcript outranks the sidecar.
- `list`'s scope banner and JSON `sessions_in_scope` report the pre-cap resolved range. The
  flood guard caps only the rows.
- `--count-by label` census keys pass the active `-t`/`-T` predicate. A dual-labeled
  record no longer leaks its filtered-out twin into the keys.
- `show` rejects the span pair with the single-transcript rule. The legacy flat `-t` values
  (`thinking`/`tool`/`tool-response`) name their successor path.

## [0.6.2] - 2026-07-12

- The `image --id` miss error explains itself. It names the handles present and states
  that `#N` is inherited from paste-time numbering. Holes and non-1 starts are source gaps
  and not csift drops. The error routes to the plain listing.
- The three count units are cross-referenced where the numbers collide. `-c` counts
  exchanges, and `--count-by` counts records, so a tool call plus its result carrier gives
  ≈2× the call figure. `stats` tools count calls.
- The docs add the jq merge idiom for flattening hits with their exchange-row ids, and the
  rule to `select(.kind==…)` before projecting. They state that `returned_message` is the
  newest message the child ever returned. On a frozen lane it predates the pending call.
- Every subcommand's `long_about` was dead text, and it is now rendered.

## [0.6.1] - 2026-07-12

- An unrecognized `@`-token is a hard error naming the @-grammar, and it never falls
  through to path resolution. A stripped `@a` used to become a cwd-relative path with a
  misleading project-dir error. A 1-3-char hex token gets the dedicated
  too-short-for-a-prefix message.
- `@trap` main-thread timing is documented, and the error routes by it. A subagent's
  transcript records the launching tool_use eagerly. The main conversation's record flushes
  only after the current Bash call completes, so a top-level first use always misses. The
  main thread uses `@main`, and any other caller re-runs the same marker.
- The docs state that `--count-by model` reports the raw `<synthetic>` key verbatim. They
  also state that text excerpts keep literal newlines, so `| head -N` can cut mid-record,
  and that the line-safe machine form is `--format json`.

## [0.6.0] - 2026-07-12

- This is breaking for agents JSON. `completed_utc/_local` (+ `duration`) are non-null only when
  `status == "completed"`, because a frozen lane is never "done". Every timestamped lane
  gains the `last_activity_utc/_local` pair, the tail's newest-record instant.
- `show`'s TARGET is a Vec, so a mistyped or foreign `--flag` is rejected by name and is
  no longer consumed as the target. Two real targets get a pointed one-transcript arity
  error.
- Censuses count records and not per-section hits. A leaf tally now equals exactly what
  `-t <leaf>` surfaces.
- `pairing` rides the tool block through the communication views. A frozen SendMessage is
  `pending` under any selector, so "any pending tools?" needs no `-t`.

## [0.5.2] - 2026-07-11

- `search --help`'s COUNT section says the `-c` integer is the exchange total, and it
  routes session listing to `-l`.

## [0.5.1] - 2026-07-11

This release brings the help to parity. Behavior is unchanged.

- The five-document contract is stated. `SKILL.md` is the LLM manual. `--help` is the human
  (CLI-proficient) manual, at information parity with SKILL. `README.md` is promotion,
  `SPEC.md` is design intent, and `AGENTS.md` is maintenance.
- The root `--help` gains the human-toned sections, covering the rules every command
  follows, JSON output, pitfalls, non-goals and retention. `search --help` gains the full
  3-role / 25-leaf label taxonomy. `show`/`stats`/`plan`/`whoami`/`image` gain JSON SCHEMA
  sections. Every `--sessions-from` help states the span rules. `whoami`'s composition
  example matches the flat envelopes.

## [0.5.0] - 2026-07-11

This is a breaking rework with zero backward compatibility.

- The per-command turn-window flag is `--turn` everywhere, replacing `--turn-range`. The
  range grammar is the same, and so is the AND-intersection with `--since`/`--until`.
- The label census generalizes to `--count-by <AXIS>` with six closed axes, `label`,
  `tool`, `turn`, `session`, `pairing` and `model`. The JSON row kind is `census`. Records
  outside an axis's domain are excluded and reported.
- `agents --format json` is flat, under envelope v2 with no exceptions. It emits session,
  run and agent rows in tree pre-order. Nesting is text-only, rebuilt from
  `parent_agent_id`/`depth`, and an unreachable node is appended and never dropped.
- `show` makes an explicit `--turn` miss a hard error naming the domain, while open and
  from-end forms clamp. It gains a 200-record-unit flood guard with the exact continuation
  command. `--max-count 0` means uncapped uniformly on list/stats/search/show.
- Text timestamps take the one canonical local form
  `YYYY-MM-DD HH:MM:SS[.mmm] TZAB(UTC±offset)`, and the second UTC copy is gone.
- Slash-command wrappers are detected in both tag orders, and `<command-message>` first is
  current CC. A new-order wrapper no longer masquerades as human prose or opens a turn.
- `list` rows gain `sidecar_present`, a tri-state elicitation evidence field. The `files`
  JSON summary gains `sessions`. `verbatim` prints a per-session no-compaction note routing
  to `show --turn`. `normalize_argv` locates the subcommand by scanning past root flags, so
  flag order is free in combination with `--claude-home`.

## [0.4.1] - 2026-07-11

- Version and tag discipline is codified. `Cargo.toml` ≡ SKILL surface header ≡
  `csift --version` move together in the same commit. Every release gets an annotated
  `vX.Y.Z` tag, and `--help` text is release surface. This release bumps for the v0.4
  round's `--help` corrections.

## [0.4.0] - 2026-07-11

This is a breaking rework with zero backward compatibility.

- `turns` is renamed `verbatim` and reframed as the compaction-fidelity specialist, which
  restores the verbatim turns a compaction summary clipped. Tail-peek reading moves to
  `show --turn`, a third addressing mode that fetches every record of the named turn(s)
  (`-3..` = the last 3).
- One range grammar applies everywhere, `N` · `A..B` · `N..` · `..N` · `-k` from the end.
  All forms are inclusive and resolved per target. The dash form `A-B` hard-errors and
  teaches the `..` spelling.
- `search --count-by-label` is a per-leaf label census terminal mode. An empty pattern
  gives a whole-scope census, and a leaf's count is what `-t <leaf>` would surface. JSON
  emits `label_count` rows.
- Empty results diagnose themselves. A zero-match run prints a stderr diagnosis with "a
  DEFINITIVE absence (exit 0), NOT an error", the active filters, and an active probe naming
  the label(s) the pattern does occur under. The JSON summary gains `definitive_absence` /
  `active_filters` / `excluded_by_label`.
- Flood guards arrive. An unscoped all-projects `list` caps at the 50 most-recently-active
  rows, reports the drop, and `--max-count` overrides the cap. `stats` gains an opt-in
  `--max-count`.
- A search-summary JSON key is renamed, `session_ids` → `transcript_ids`
  (+ `transcript_ids_truncated`), to keep it apart from `-l`'s owning-session ids.

## [0.3.0] - 2026-07-11

- `-T`/`--label-not` (search) excludes labels with the same selector grammar as `-t`. Dedup
  takes the richest surviving view, and statically empty combinations hard-error.
- `--sessions-from <FILE|->` arrives on every multi-target command. It unions an id list
  into the scope, and an explicit empty list means an empty scope. `search -l` emits the
  matching owning-session ids to pipe into it. `search --raw` emits matched records'
  verbatim jsonl lines, with stdout pure and notes on stderr.
- Search JSON hits and verbatim collapsed-agent rows carry `refetch`, the ready-to-run
  `csift show` command addressed at the line-owning transcript.
- The turn window and `--since`/`--until` intersect on every command. `verbatim` requires
  a target, as a guard against its budget flooding every session. `list` gains
  `--since`/`--until`.
- Teammate ids with dashed names round-trip as `@<agent-id>` targets.

## [0.2.0] - 2026-07-10

This is a breaking ergonomics rework with zero backward compatibility, and one way per
intent.

- New `show` owns fetching. It is a record fetch by `--line`/`--uuid` (§), rendered full or
  as `--raw` verbatim jsonl bytes, and `search --line/--uuid` are removed. New `stats` gives
  one-scan per-session aggregates (tokens by model, tool counts, turns, span,
  compactions).
- Envelope v2 arrives. Every `--format json` stream is one header line, kind-tagged rows
  and one summary line, with no exceptions. The jsonl-line key is `line` everywhere.
- The flag surface changes. `-t`'s long form is `--label`. `agents --kind` becomes
  `--shape`, and `recover --line-range` becomes `--file-lines`. The span pair is uniformly
  `--subagents`/`--no-subagents`. verbatim's five tuning knobs collapse into
  `--profile heavy|light`. `--siblings` is a zero-arg fixed policy. `image --id` takes bare
  digits or `L<line>i<n>`.
- Guardrails arrive. A bare id target errors with "did you mean '@<id>'?". A search
  PATTERN starting with `@` errors. A uuid-shaped pattern prints a note on stderr.

## [0.1.0] - 2026-06-07

- Initial scaffold of csift, "ripgrep for Claude Code session transcripts". It is a fast
  Rust CLI to list and regex-search the Claude Code session `.jsonl` transcripts under
  `~/.claude/projects/`.
