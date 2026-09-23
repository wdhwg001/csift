//! role.class.sub selector machinery: label_selectors / label_selected / LabelFilter.

use super::*;

/// True iff `selector` (a dotted `role.class.sub` path) is a dot-SEGMENT prefix of `path` -
/// the `-t` match rule (SPEC section 5). `agent` matches `agent.tool.use`; `agent.tool` matches
/// `agent.tool.use`/`agent.tool.result` but NOT a hypothetical `agent.toolbar` (segment-wise,
/// so a partial trailing segment never leaks). Shared by the `-t` gate and the `--siblings` caps.
#[must_use]
pub fn selector_is_segment_prefix(selector: &str, path: &str) -> bool {
    match path.strip_prefix(selector) {
        Some(rest) => rest.is_empty() || rest.starts_with('.'),
        None => false,
    }
}

/// Every VALID `-t` selector: each dot-segment prefix of every [`Class::path`] in [`Class::ALL`]
/// (role / role.class / role.class.sub), in taxonomy order, de-duplicated. The single source of
/// truth for the `-t` value space - the clap value_parser validates against it and the `--help`
/// / error text lists it (so a new [`Class`] leaf automatically widens the selector space).
#[must_use]
pub fn label_selectors() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for class in Class::ALL {
        let segs: Vec<&str> = class.path().split('.').collect();
        for i in 1..=segs.len() {
            let prefix = segs[..i].join(".");
            if !out.contains(&prefix) {
                out.push(prefix);
            }
        }
    }
    out
}

/// True iff `selector` is a valid `-t` value: some [`Class`] path has it as a
/// segment-prefix, or it is the `.*` glob form of such a prefix.
#[must_use]
pub fn selector_is_valid(selector: &str) -> bool {
    let bare = selector.strip_suffix(".*").unwrap_or(selector);
    !bare.is_empty()
        && Class::ALL
            .iter()
            .any(|c| selector_is_segment_prefix(bare, c.path()))
}

/// A BARE FACE selector: a ROLE (`user` / `agent` / `harness`) or a CATEGORY
/// (`agent.message`, `harness.bookkeeping`, …). These are the two coarse forms that
/// claim to show THE CONVERSATION, so they select only the leaves the model receives -
/// and only they consult the per-record delivery and survival verdicts. Everything
/// else a selector can be (a full leaf path, or any form ending `.*`) names what it
/// wants and gets it.
///
/// The category list is [`Class::CATEGORIES`], so a new category is a bare-face
/// selector the moment the taxonomy declares it; `user` is both a role and a category
/// and takes the same treatment either way.
#[must_use]
pub fn is_bare_face(selector: &str) -> bool {
    !selector.contains('.') || Class::CATEGORIES.contains(&selector)
}

/// The three selector forms (the visibility law):
/// - a BARE ROLE or CATEGORY ([`is_bare_face`]) matches only the leaves the model
///   RECEIVED - the coarse "show me the conversation" ask (a superseded draft under
///   `-t user` poisoned a real downstream consumer, which is how this law was born);
/// - a GLOB (`user.*`, or any valid prefix + `.*`) matches EVERY leaf under the
///   prefix, visibility ignored - the explicit "truly everything" form;
/// - a full leaf path (`user.unsent`, `harness.turn.duration`) matches regardless of
///   visibility - a deliberate drill-down names what it wants.
///
/// `delivered` is the RECORD-level override ([`crate::model::Record::delivery_override`]):
/// `Some(_)` when Claude Code's own request-assembler drop predicate disagrees with the
/// leaf default for the record this label came from, `None` otherwise. Only the
/// bare-face form consults it, because only the bare-face form claims to be the
/// conversation. `None` = no record in hand (or none that disagrees), so the leaf
/// default stands. ONE leaf is exempt from that consult
/// ([`keeps_face_against_delivery`]).
#[must_use]
pub fn selector_matches(selector: &str, path: &str, delivered: Option<bool>) -> bool {
    if let Some(prefix) = selector.strip_suffix(".*") {
        return selector_is_segment_prefix(prefix, path);
    }
    if !selector_is_segment_prefix(selector, path) {
        return false;
    }
    if !is_bare_face(selector) {
        return true; // an exact leaf: any visibility.
    }
    // A bare role or category: what the model received - the leaf default unless this
    // record's own delivery verdict overrides it.
    if let Some(d) = delivered {
        if !keeps_face_against_delivery(path) {
            return d;
        }
    }
    Class::ALL
        .iter()
        .find(|c| c.path() == path)
        .is_none_or(|c| c.llm_visible())
}

/// The ONE leaf a bare face keeps despite its record's delivery verdict, and the one
/// stated exception to the visible-face law.
///
/// `agent.message.structured` is a schema-bound subagent's final answer, carried by a
/// `structured_output` attachment. That payload type is in
/// [`crate::model::inventory::NEVER_CONVERTS`], so the record-level override reads NOT
/// delivered and the delivery axis would drop the leaf out of `-t agent` and
/// `-t agent.message` - leaving the bare `agent.message` face holding only half of a
/// two-leaf category, with no way to learn the other half exists short of a glob.
///
/// The drop is right for every other row it applies to and wrong for this one, because
/// the axis asks "did the model RECEIVE this" and this leaf is the case where the model
/// WROTE it. A bare face asks for the conversation, and an answer the model produced is
/// part of the conversation whatever the assembler later did with the record. The API
/// itself offers text or a schema-bound answer and never both, so a reader asking for
/// `agent.message` is asking for whichever one this turn used.
///
/// What changes is FACE MEMBERSHIP alone. The hit's own `delivered` column still reads
/// false, because the record genuinely was not delivered; nothing here edits a fact.
fn keeps_face_against_delivery(path: &str) -> bool {
    path == Class::AgentMessageStructured.path()
}

/// The bare-face selectors among `selectors` whose visible face is EMPTY - every leaf
/// under them is one the model never receives, so the selector is accepted, matches
/// nothing, and says so rather than reading as a miss. `harness.bookkeeping` is the one
/// such category in the taxonomy today; the answer is derived, so a visibility change
/// moves it without a second edit.
#[must_use]
pub fn empty_visible_faces(selectors: &[String]) -> Vec<String> {
    selectors
        .iter()
        .filter(|s| is_bare_face(s))
        .filter(|s| {
            let reaches_any = Class::ALL
                .iter()
                .any(|c| selector_is_segment_prefix(s, c.path()));
            let reaches_visible = Class::ALL
                .iter()
                .any(|c| selector_is_segment_prefix(s, c.path()) && c.llm_visible());
            reaches_any && !reaches_visible
        })
        .cloned()
        .collect()
}

/// Does a record-label `path` satisfy the active `-t` selectors? Empty selectors ⇒ every label is
/// eligible (no `-t` filter). Otherwise the label matches iff ANY selector matches it under the
/// three-form rule ([`selector_matches`]) - `-t agent` surfaces the agent role's delivered leaves,
/// `-t agent.tool` use+result, `-t 'user.*'` every user leaf incl. `user.unsent`. `delivered`
/// is the record-level override the bare-role form consults (see [`selector_matches`]).
#[must_use]
pub fn label_selected(selectors: &[String], path: &str, delivered: Option<bool>) -> bool {
    selectors.is_empty()
        || selectors
            .iter()
            .any(|s| selector_matches(s, path, delivered))
}

/// The active `-t`/`-T` label filter - the include selectors (empty ⇒ every label) MINUS the
/// exclude selectors, both matched by the same segment-prefix rule. The ONE membership
/// predicate every hit-selection surface keys on (the rg `-t`/`-T` duality). Exclusion only
/// ever SHRINKS the include set, so the §7 stage-1 role prefilter (a conservative superset)
/// stays valid unchanged.
#[derive(Debug, Clone, Copy)]
pub struct LabelFilter<'a> {
    include: &'a [String],
    exclude: &'a [String],
    /// The RECORD-level delivery verdict this filter is being applied WITH (see
    /// [`LabelFilter::with_delivery`]). `None` on the base filter, which is what every
    /// per-ARGS use wants (a candidate gate, `reaches_gated`, the statically-empty
    /// check): those decide before any record is in hand.
    delivered: Option<bool>,
    /// The RECORD-level SURVIVAL verdict: is this record still in the conversation Claude
    /// Code's own chain rule reconstructs ([`LabelFilter::with_survival`])? `true` on the
    /// base filter, so a decision taken before any record is in hand is unchanged.
    live: bool,
}

impl<'a> LabelFilter<'a> {
    #[must_use]
    pub fn new(include: &'a [String], exclude: &'a [String]) -> Self {
        Self {
            include,
            exclude,
            delivered: None,
            live: true,
        }
    }

    /// The same filter, applied to ONE record whose delivery verdict is `delivered`
    /// ([`crate::model::Record::delivery_override`]). Bare-role selection then follows
    /// what the model received rather than the leaf default; every other selector form
    /// is unchanged. Applied ONCE per record, at the hit-emission and census seams.
    #[must_use]
    pub fn with_delivery(self, delivered: Option<bool>) -> Self {
        Self { delivered, ..self }
    }

    /// The same filter, applied to ONE record whose SURVIVAL verdict is `live` (false only
    /// for a record Claude Code's conversation chain does not reach). A bare ROLE or
    /// CATEGORY selector then skips it: `-t user` means "what the human said in this conversation",
    /// and a prompt that was recalled or rewound past is not in it. Every other selector
    /// form - an intermediate prefix, a full leaf, a glob - reaches it unchanged,
    /// which is what makes `-t user.rewound` and `-t 'user.*'` the deliberate drill-downs.
    /// Applied ONCE per record, at the hit-emission and census seams.
    #[must_use]
    pub fn with_survival(self, live: bool) -> Self {
        Self { live, ..self }
    }

    /// Every label is eligible - the no-filter view (`--siblings` rendering ignores `-t`/`-T`;
    /// selectors filter HITS, never a turn's other records).
    #[must_use]
    pub fn all() -> LabelFilter<'static> {
        LabelFilter {
            include: &[],
            exclude: &[],
            delivered: None,
            live: true,
        }
    }

    /// Does a record-label `path` survive include-minus-exclude? Both sides speak
    /// the same three-form rule ([`selector_matches`]) and both see this filter's
    /// record-level delivery verdict, so `-T user` excludes the delivered user leaves
    /// while `-T 'user.*'` excludes them all.
    #[must_use]
    pub fn selected(&self, path: &str) -> bool {
        let reaches = |s: &String| {
            (self.live || !is_bare_face(s)) && selector_matches(s, path, self.delivered)
        };
        let included = if self.live {
            label_selected(self.include, path, self.delivered)
        } else {
            self.include.is_empty() || self.include.iter().any(&reaches)
        };
        included && !self.exclude.iter().any(&reaches)
    }

    /// True when NO leaf of [`Class::ALL`] survives because `-T` cancels `-t` - a
    /// statically-CONTRADICTORY combination that could never match anything (hard error
    /// at the caller, fail-loud).
    ///
    /// The VISIBILITY narrowing is deliberately excluded from the include side. A bare
    /// category whose whole face is invisible (`harness.bookkeeping`) also matches no
    /// leaf, and that is a different fact with a different remedy: the query is well
    /// formed, its answer is empty by the law, and the caller says so through
    /// [`empty_visible_faces`] rather than refusing to run. Only the exclude side can
    /// make a filter contradictory.
    #[must_use]
    pub fn is_statically_empty(&self) -> bool {
        let survives = |exclude: &[String]| {
            Class::ALL.iter().any(|c| {
                let path = c.path();
                label_selected(self.include, path, self.delivered)
                    && !exclude
                        .iter()
                        .any(|s| selector_matches(s, path, self.delivered))
            })
        };
        // Empty WITH the exclude side and non-empty without it: `-T` is what cancelled
        // `-t`. Empty either way means the INCLUDE side selected nothing on its own,
        // which is the honest-empty case above and not an error.
        !survives(self.exclude) && survives(&[])
    }
}

/// clap value_parser for one `-t`/`--label` selector: accept a dotted `role.class.sub` path
/// that is a segment-prefix of some [`Class`] path; reject anything else with a HARD error that
/// LISTS the valid selectors (no back-compat - the flat `thinking`/`tool`/`tool-response`
/// spellings therefore error; a bare `user`/`agent`/`harness` is a valid ROLE selector -
/// SPEC section 5).
pub(crate) fn parse_label_selector(s: &str) -> Result<String, String> {
    let s = s.trim();
    if selector_is_valid(s) {
        return Ok(s.to_string());
    }
    // A RETIRED spelling gets a direct successor pointer (faster convergence than scanning
    // the full selector list) - a guidance hint, not a compat shim: still a hard error.
    // The `.*` form of a retired prefix is the same retirement, so it is looked up bare.
    let bare = s.strip_suffix(".*").unwrap_or(s);
    let hint = retired_successors(bare).map_or(String::new(), |(era, t)| {
        format!(" ('{bare}' is the {era} - today that is: {t}.)")
    });
    Err(format!(
        "unknown label selector '{s}'.{hint} A selector is a dotted role.category.leaf path, \
         its role or category prefix (a bare role or category selects its LLM-visible leaves \
         only), or a prefix + `.*` (every leaf under it, visibility ignored). Valid: {}",
        label_selectors().join(", ")
    ))
}

/// The RETIRED selector spellings and the leaves that replaced each. Every entry is a
/// HARD ERROR carrying this pointer - csift ships no alias, no forwarding arm and no
/// deprecated path, so a stale invocation fails loudly and names where its records went.
///
/// `harness.meta` split along the PRODUCERS its leaves were pooled from: a hook's
/// injected text, a hook's own run record, a slash command's caveat, the scheduler's
/// ticks, the end-of-turn telemetry, the file-history instrument, and the harness's own
/// UI events. Two more entries are LEAF spellings that became CATEGORIES rather than
/// disappearing: `agent.message` and `agent.thinking` are still valid selectors, so they
/// never reach this table - only the paths below do.
/// The table itself: `(retired selector, the era it belonged to, its successors)`.
///
/// A TABLE rather than a match, so the set is WALKABLE. `SKILL.md` section 10 is the one
/// place a stale caller is sent to, and the only way to keep it from drifting away from
/// these strings is a test that iterates them - which a match arm cannot offer.
pub(crate) const RETIRED_SELECTORS: &[(&str, &str, &str)] = &[
    ("thinking", "pre-v0.5 flat spelling", "agent.thinking"),
    ("tool", "pre-v0.5 flat spelling", "agent.tool"),
    (
        "tool-response",
        "pre-v0.5 flat spelling",
        "agent.tool.result",
    ),
    // The resume repair prompt belongs outside the schedule family: the record comes from
    // the resume loader, never from the scheduler. Same predicate, same records, a
    // different path.
    (
        "harness.schedule.continuation",
        "pre-v0.12.0 name",
        "harness.resume.prompt",
    ),
    (
        "harness.meta",
        "pre-v0.13.0 catch-all category",
        "harness.hook.* · harness.command.caveat · harness.enforce.retry · \
         harness.schedule.tick · harness.schedule.event · harness.context.* · \
         harness.turn.* · harness.system.* · harness.bookkeeping.* · \
         agent.message.structured",
    ),
    (
        "harness.meta.hook",
        "pre-v0.13.0 name",
        "harness.hook.context (hook-injected text) · harness.hook.feedback (a \
         blocking hook's feedback, content opening <Name> hook feedback: and a \
         newline) · harness.command.caveat (<local-command-caveat>) · \
         harness.enforce.retry (the edit-failed-retry nudge)",
    ),
    (
        "harness.meta.loop",
        "pre-v0.13.0 name",
        "harness.schedule.tick",
    ),
    (
        "harness.meta.attachment",
        "pre-v0.13.0 catch-all",
        "harness.context.* (the injected payloads) · harness.hook.outcome (a hook \
         run's own record) · agent.message.structured (a subagent's structured answer)",
    ),
    (
        "harness.meta.turn-duration",
        "pre-v0.13.0 name",
        "harness.turn.duration",
    ),
    (
        "harness.meta.away-summary",
        "pre-v0.13.0 name",
        "harness.turn.recap",
    ),
    (
        "harness.meta.stop-hooks",
        "pre-v0.13.0 name",
        "harness.turn.stop-hooks",
    ),
    (
        "harness.meta.snapshot",
        "pre-v0.13.0 name",
        "harness.bookkeeping.file-history",
    ),
    (
        "harness.meta.system",
        "pre-v0.13.0 catch-all",
        "harness.system.{refusal,api-error,notice,kill,command} · \
         harness.schedule.event (the scheduler's fire record)",
    ),
];

/// `(the era it belonged to, its successors)` for a retired selector, else `None`.
///
/// A lookup over [`RETIRED_SELECTORS`] rather than a match, so the set stays WALKABLE:
/// the table is what `SKILL.md` section 10 is held to, and a match arm could not be
/// iterated. Linear over a table this size, on a path that only runs when a caller
/// already typed a spelling csift refuses.
fn retired_successors(selector: &str) -> Option<(&'static str, &'static str)> {
    RETIRED_SELECTORS
        .iter()
        .find(|(s, _, _)| *s == selector)
        .map(|(_, era, to)| (*era, *to))
}
