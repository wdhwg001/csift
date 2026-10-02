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
     A `harness.bookkeeping` unit renders its key=value HEAD and then the line's own\n  \
     VERBATIM JSON as the body. Every key and every full uuid on the line therefore stays\n  \
     matchable, beyond the few a head names.\n\n  \
     SIXTEEN of the eighteen non-transcript line types build that head from their own\n  \
     bytes, and it opens with the line's own `type` value, as in\n  \
     `[cost-state totalCostUSD=1.5 totalDuration=42 startTime=1767225600000]`. The types\n  \
     that share one leaf (mode/permission-mode, ai-title/agent-name, the three artifact\n  \
     ledgers) therefore stay apart in the output, although their label does not tell them\n  \
     apart. The other two are `file-history-snapshot` and `file-history-delta`, which share\n  \
     one leaf. They keep the head built from the fields csift models,\n  \
     `[file-history snapshot at <ts>: <path>@vN]`, which names the tracked path where a raw\n  \
     key walk could only print the snapshot object as JSON.\n\n  \
     Two rules decide a head's values, and there is no third.\n    \
     a key naming an ACCOUNT, an   prints its first 8 characters (leafUuid, parentLastUuid,\n    \
     ORGANISATION or a RECORD      accountUuid, ownerAccountUuid, ownerOrganizationUuid,\n                                  \
     vetoedAgainstAccountUuid)\n    \
     every other value             is cut at 80 characters with the shared\n                                  \
     `... (+N chars)` marker. An OBJECT value prints as its\n                                  \
     sorted key list, which is where the artifact ids are.\n  \
     A session-shaped value (bridgeSessionId, parentSessionId) prints whole under the\n  \
     second rule, because it is a join key carried to another file and half a uuid joins\n  \
     nothing.\n\n  \
     Every unit of the family carries the marker [not delivered]. None of these lines\n  \
     reaches the model, which is what the JSON `delivered` field says about all thirteen\n  \
     leaves.\n\n  \
     FIVE line types also name something outside themselves. Three name a RECORD and one\n  \
     names a FILE, and those four ride the shared provenance edge, `[for: leaf L42]` in the\n  \
     label zone and a `for` array in JSON.\n    \
     `last-prompt`          -> leaf, its leafUuid\n    \
     a file-history line    -> versions, its messageId\n    \
     `fork-context-ref`     -> forked_from, the record the forked agent's prefix was cut\n                              \
     at, with the parent session beside it\n    \
     `continued-in`         -> continued_in, the CHILD transcript a background handover\n                              \
     moved to\n  \
     The `continued-in` target is a whole file and no record in it, so its edge renders\n  \
     `[for: continued_in @<session>]`, an `@` target you can feed straight back. A uuid\n  \
     resolves to a LINE of this transcript when one carries it. Otherwise the id prints\n  \
     with `(not in this file)`, and a line naming no such key renders\n  \
     `[for: <kind> unknown (<reason>)]` in place of a dropped edge.\n\n  \
     ONE relation is no edge, because its target is an INSTANT, neither a record nor a\n  \
     file. `cost-state` renders `[checkpoint closes <instant>]`, the startTime plus\n  \
     totalDuration, which is the instant `list` joins a cleared session on as\n  \
     `cleared_from`. JSON carries it as the timestamp pair `checkpoint_closes_at_utc` and\n  \
     `checkpoint_closes_at_local`, plus `checkpoint_join`.";
