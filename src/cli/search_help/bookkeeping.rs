//! The `search --help` section for the BOOKKEEPING family: the thirteen
//! `harness.bookkeeping` leaves - their key=value heads, the provenance edges a
//! bookkeeping line names, and how a key the head elides stays matchable.
//!
//! It opens with the RENDER LAW, because no head below it reads correctly until a reader
//! knows the unit renders a key=value head PLUS the line's own verbatim JSON; the heads,
//! markers and `for` edges follow it. Information parity with SKILL is the contract
//! (AGENTS section 1): every marker word, JSON key and render form the family ships is
//! named here too, and this file is the family's ONLY string on the page, so extending it
//! disturbs no other family's sentences.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and closes
//! without one: `"\n\n"`, the head line at column zero, `"\n  "`, the body. That is the
//! shape every head on this page has, and the region sits after the JSON schema block and
//! its closing notes rather than inside them - a render law is not a subsection of the
//! JSON schema.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str = "\n\nBOOKKEEPING LEAVES (harness.bookkeeping)\n  \
     A `harness.bookkeeping` unit renders its key=value HEAD and then the line's own \
     VERBATIM JSON as the body, so every key and every full uuid on the line stays \
     matchable and not only the few a head names.\n\n  \
     SIXTEEN of the eighteen non-transcript line types build that head from their own \
     bytes, and it opens with the line's own `type` value - `[cost-state \
     totalCostUSD=1.5 totalDuration=42 startTime=2026-01-01T00:00:00Z]` - so the types \
     that share one leaf (mode/permission-mode, ai-title/agent-name, the three artifact \
     ledgers) stay apart in the output even though their label does not tell them \
     apart. The other two are `file-history-snapshot` and `file-history-delta`, one \
     leaf: they keep the head built from the fields csift models, `[file-history \
     snapshot at <ts>: <path>@vN]`, which names the tracked path where a raw key walk \
     could only print the snapshot object as JSON.\n\n  \
     Two rules decide a head's values, and there is no third. A key naming an ACCOUNT, \
     an ORGANISATION or a RECORD prints its first 8 characters (leafUuid, \
     parentLastUuid, accountUuid, ownerAccountUuid, ownerOrganizationUuid, \
     vetoedAgainstAccountUuid). Every other value is cut at 80 characters with the \
     shared `... (+N chars)` marker, an OBJECT value printing as its sorted key list, \
     which is where the artifact ids are. A session-shaped value (bridgeSessionId, \
     parentSessionId) prints whole under the second rule rather than the first, because \
     it is a join key carried to another file and half a uuid joins nothing.\n\n  \
     Every unit of the family carries the marker [not delivered]: none of these lines \
     reaches the model, which is what the JSON `delivered` field says about all \
     thirteen leaves.\n\n  \
     FIVE line types also name something outside themselves. Three name a RECORD and \
     ride the shared provenance edge - `[for: leaf L42]` in the label zone, a `for` \
     array in JSON: `last-prompt` -> leaf (its leafUuid), a file-history line -> \
     versions (its messageId), `fork-context-ref` -> forked_from (the record the \
     forked agent's prefix was cut at, with the parent session beside it), and \
     `continued-in` -> continued_in (the CHILD transcript a background handover moved \
     to - a whole file and no record in it, so the edge renders \
     `[for: continued_in @<session>]`, an `@` target you can feed straight back). A \
     uuid resolves to a LINE of this transcript when one carries it, else the id \
     prints with `(not in this file)`, and a line naming no such key renders \
     `[for: <kind> unknown (<reason>)]` rather than dropping the edge. ONE relation \
     is not an edge, because its target is an INSTANT rather than a record or a file: \
     `cost-state` renders `[checkpoint closes <instant>]` (JSON \
     `checkpoint_closes_at` plus `checkpoint_join`) - startTime plus totalDuration, \
     the instant `list` joins a cleared session on as `cleared_from`.";
