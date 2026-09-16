//! The `search --help` section for the BOOKKEEPING family: the thirteen
//! `harness.bookkeeping` leaves - their key=value heads, the provenance edges a
//! bookkeeping line names, and how a key the head elides stays matchable.
//!
//! It opens with the RENDER LAW because that is the breaking change the family shipped;
//! the family lane EXTENDS this section with its heads, markers and `for` edges, in the
//! same commit series as the surface they document, without any lane touching a string
//! another lane is editing. Information parity with SKILL is the contract (AGENTS
//! section 1): every marker word, JSON key and render form the family ships is named
//! here too.
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
     it is a join key carried to another file and half a uuid joins nothing.";
