//! role.class.sub selector machinery: label_selectors / label_selected / LabelFilter.

use super::*;

/// True iff `selector` (a dotted `role.class.sub` path) is a dot-SEGMENT prefix of `path` -
/// the `-t` match rule (GOLD §6). `agent` matches `agent.tool.use`; `agent.tool` matches
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

/// The three selector forms (v0.9.4 - the C-25 visibility law):
/// - a BARE ROLE (`user` / `agent` / `harness`, i.e. any single-segment selector)
///   matches only the role's LLM-VISIBLE leaves - the coarse "show me the
///   conversation" ask (a superseded draft under `-t user` poisoned a real
///   downstream consumer, which is how this law was born);
/// - a GLOB (`user.*`, or any valid prefix + `.*`) matches EVERY leaf under the
///   prefix, visibility ignored - the explicit "truly everything" form;
/// - an intermediate prefix or full leaf path (`harness.compaction`,
///   `user.unsent`) matches by segment-prefix regardless of visibility - a
///   deliberate drill-down names what it wants.
#[must_use]
pub fn selector_matches(selector: &str, path: &str) -> bool {
    if let Some(prefix) = selector.strip_suffix(".*") {
        return selector_is_segment_prefix(prefix, path);
    }
    if !selector_is_segment_prefix(selector, path) {
        return false;
    }
    if selector.contains('.') {
        return true; // intermediate prefix or exact leaf: any visibility.
    }
    // A bare role: visible leaves only.
    Class::ALL
        .iter()
        .find(|c| c.path() == path)
        .is_none_or(|c| c.llm_visible())
}

/// Does a record-label `path` satisfy the active `-t` selectors? Empty selectors ⇒ every label is
/// eligible (no `-t` filter). Otherwise the label matches iff ANY selector matches it under the
/// three-form rule ([`selector_matches`]) - `-t agent` surfaces the agent role's visible leaves,
/// `-t agent.tool` use+result, `-t 'user.*'` every user leaf incl. `user.unsent`.
#[must_use]
pub fn label_selected(selectors: &[String], path: &str) -> bool {
    selectors.is_empty() || selectors.iter().any(|s| selector_matches(s, path))
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
}

impl<'a> LabelFilter<'a> {
    #[must_use]
    pub fn new(include: &'a [String], exclude: &'a [String]) -> Self {
        Self { include, exclude }
    }

    /// Every label is eligible - the no-filter view (`--siblings` rendering ignores `-t`/`-T`;
    /// selectors filter HITS, never a turn's other records).
    #[must_use]
    pub fn all() -> LabelFilter<'static> {
        LabelFilter {
            include: &[],
            exclude: &[],
        }
    }

    /// Does a record-label `path` survive include-minus-exclude? Both sides speak
    /// the same three-form rule ([`selector_matches`]), so `-T user` excludes the
    /// visible user leaves while `-T 'user.*'` excludes them all.
    #[must_use]
    pub fn selected(&self, path: &str) -> bool {
        label_selected(self.include, path)
            && !self.exclude.iter().any(|s| selector_matches(s, path))
    }

    /// True when NO leaf of [`Class::ALL`] survives - a statically-contradictory `-t`/`-T`
    /// combination that could never match anything (hard error at the caller, fail-loud).
    #[must_use]
    pub fn is_statically_empty(&self) -> bool {
        Class::ALL.iter().all(|c| !self.selected(c.path()))
    }
}

/// clap value_parser for one `-t`/`--label` selector: accept a dotted `role.class.sub` path
/// that is a segment-prefix of some [`Class`] path; reject anything else with a HARD error that
/// LISTS the valid selectors (0 back-compat - the old flat `thinking`/`tool`/`tool-response`
/// therefore error; bare `user`/`agent`/`harness` are now valid ROLE selectors - GOLD §6).
pub(crate) fn parse_label_selector(s: &str) -> Result<String, String> {
    let s = s.trim();
    if selector_is_valid(s) {
        return Ok(s.to_string());
    }
    // The pre-v0.5 FLAT spellings get a direct successor pointer (faster convergence than
    // scanning the full selector list) - a guidance hint, not a compat shim: still a hard error.
    let legacy = match s {
        "thinking" => Some("agent.thinking"),
        "tool" => Some("agent.tool"),
        "tool-response" => Some("agent.tool.result"),
        _ => None,
    };
    let hint = legacy.map_or(String::new(), |t| {
        format!(" ('{s}' is the pre-v0.5 flat spelling — today that is `{t}`.)")
    });
    Err(format!(
        "unknown label selector '{s}'.{hint} A selector is a dotted role.class.sub path, any \
         prefix of one (a bare role selects its LLM-visible leaves only), or a prefix + `.*` \
         (every leaf under it, visibility ignored). Valid: {}",
        label_selectors().join(", ")
    ))
}
