//! The CROSSING BUDGET: how many compaction boundaries the survival walk may step over,
//! and what it says about the ones it did not.
//!
//! Claude Code's own loader stops dead at a `compact_boundary` - it reads `parentUuid`
//! only, and every boundary carries a null one. csift walks the same links, so it stopped
//! there too and flagged the whole region above as `PreCut`. But the conversation does
//! naturally continue across a compaction (a subagent dispatched before one returns after
//! it), and the boundary itself names the record it re-linked from, in
//! `logicalParentUuid`. Following that link resolves the region above by the ordinary
//! rules instead of declaring it unresolved.
//!
//! It is BOUNDED because a long-running lane can carry dozens of compactions, and tracing
//! all of them by default would make every chain-walking command pay for history nobody
//! asked about. So the walk carries a budget of N boundaries; when it runs out - or when a
//! boundary's `logicalParentUuid` names no record the map still holds - the walk ends there
//! and that record is the FLOOR, exactly as before. [`CrossingReport`] is what the surfaces
//! print about it: what was crossed, what was not, where the floor is, WHY it is there, and
//! whether raising the budget would move it - which for a boundary naming a deleted record
//! it would not.
//!
//! Budget 0 is Claude Code's own rule, and under it csift discloses NOTHING about crossing,
//! because it crossed nothing: a `--cross-compactions 0` run is byte-for-byte the reading
//! csift gave before the budget existed.

/// The default crossing budget, and the value the global `--cross-compactions` flag
/// carries when it is not given.
///
/// PROVISIONAL: the value is chosen by measurement (the acid set's chain-walking rows on a
/// compacted transcript and on the performance landmark, medians of 21 reps against the
/// same-binary control band), and the measurement needs an idle host. Until that run lands
/// this is the candidate the ruling names first, not a measured answer.
pub const CROSS_COMPACTIONS_DEFAULT: CrossingBudget = CrossingBudget::Limited(3);

/// How many compaction boundaries one walk may step over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossingBudget {
    /// At most this many. `Limited(0)` never crosses - the loader's own reading.
    Limited(u32),
    /// Every boundary, however many a transcript carries.
    Unbounded,
}

impl std::fmt::Display for CrossingBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrossingBudget::Limited(n) => write!(f, "{n}"),
            CrossingBudget::Unbounded => f.write_str("all"),
        }
    }
}

impl Default for CrossingBudget {
    /// An unbuilt chain crossed nothing, so it claims nothing: the quiet value, never the
    /// flag's default (which is [`CROSS_COMPACTIONS_DEFAULT`] and is passed explicitly).
    fn default() -> CrossingBudget {
        CrossingBudget::NONE
    }
}

impl CrossingBudget {
    /// Never cross: Claude Code's own loader rule.
    pub const NONE: CrossingBudget = CrossingBudget::Limited(0);

    /// May the walk step over one more boundary, having stepped over `done` already?
    #[must_use]
    pub fn allows(self, done: usize) -> bool {
        match self {
            CrossingBudget::Limited(n) => (done as u64) < u64::from(n),
            CrossingBudget::Unbounded => true,
        }
    }

    /// Is crossing off entirely? Then no surface says anything about it (see the module
    /// doc): csift did not cross, so there is nothing to disclose.
    #[must_use]
    pub fn is_off(self) -> bool {
        self == CrossingBudget::NONE
    }

    /// The ONE spelling the flag accepts and every disclosure prints (the [`Display`] form,
    /// which is also what clap renders as the flag's default).
    #[must_use]
    pub fn as_text(self) -> String {
        self.to_string()
    }

    /// The JSON value: the number, or the string `all`.
    #[must_use]
    pub fn json(self) -> serde_json::Value {
        match self {
            CrossingBudget::Limited(n) => serde_json::json!(n),
            CrossingBudget::Unbounded => serde_json::json!("all"),
        }
    }

    /// The flag grammar, kept here so the CLI parser and the printed form cannot drift:
    /// a decimal count, or the literal `all`.
    ///
    /// # Errors
    /// Any other token, naming both accepted forms.
    pub fn parse(s: &str) -> Result<CrossingBudget, String> {
        if s.eq_ignore_ascii_case("all") {
            return Ok(CrossingBudget::Unbounded);
        }
        s.parse::<u32>()
            .map(CrossingBudget::Limited)
            .map_err(|_| format!("expected a count (0, 1, 3, ...) or `all`, got `{s}`"))
    }
}

/// Why the walk ended where it did - the FLOOR's cause, which is what a reader needs to
/// know whether raising the budget would resolve anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FloorCause {
    /// A record with no parent that is not a boundary: the conversation's own head.
    #[default]
    Head,
    /// No leaf resolved at all, so nothing is on the chain.
    NoLeaf,
    /// A compaction boundary the crossing budget would not step over. Raising the budget
    /// moves this floor.
    BudgetExhausted,
    /// A compaction boundary whose `logicalParentUuid` names no record on any line of
    /// this file - the pre-compaction records were deleted from disk, or the boundary
    /// names nothing at all. Raising the budget changes nothing here.
    UnresolvableBoundary,
    /// A compaction boundary whose `logicalParentUuid` names a record this file DOES
    /// carry, on a line a reader can open, which the pre-boundary RELINK CUT took out of
    /// the uuid map before any leaf was picked. On disk and off the map is a different
    /// fact from gone, and a reader chasing the line number needs to be told which.
    RelinkRemoved,
    /// A compaction boundary whose `logicalParentUuid` names a record the walk has ALREADY
    /// reached, so its door leads back into the conversation rather than above the cut.
    /// Half of the boundary floors on real data: the relink re-parents a preserved set
    /// into the chain, the walk climbs it, and the boundary's own logical parent is that
    /// segment's tail - 9 of the 18 boundary floors over one projects root read this way
    /// at `--cross-compactions all`.
    BoundaryLoop,
    /// A `parentUuid` naming no record in this file, with no repair within the 5 s window.
    DanglingParent,
    /// The parent links loop back on themselves.
    Cycle,
}

impl FloorCause {
    /// The stable wire spelling (JSON `floor_cause`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FloorCause::Head => "head",
            FloorCause::NoLeaf => "no-leaf",
            FloorCause::BudgetExhausted => "budget-exhausted",
            FloorCause::UnresolvableBoundary => "unresolvable-boundary",
            FloorCause::RelinkRemoved => "relink-removed",
            FloorCause::BoundaryLoop => "boundary-loop",
            FloorCause::DanglingParent => "dangling-parent",
            FloorCause::Cycle => "cycle",
        }
    }

    /// The phrase the text disclosure prints for it.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            FloorCause::Head => "the conversation head",
            FloorCause::NoLeaf => "no leaf resolved, so nothing is on the chain",
            FloorCause::BudgetExhausted => "a compaction boundary the budget would not cross",
            FloorCause::UnresolvableBoundary => {
                "a compaction boundary whose logicalParentUuid names no record this file carries"
            }
            FloorCause::RelinkRemoved => {
                "a compaction boundary whose logicalParentUuid names a record the relink cut \
                 removed from the uuid map"
            }
            FloorCause::BoundaryLoop => {
                "a compaction boundary whose logicalParentUuid names a record the walk had \
                 already reached"
            }
            FloorCause::DanglingParent => "a parentUuid naming no record in this file",
            FloorCause::Cycle => "a cycle in the parent links",
        }
    }
}

/// What one scan discloses about the crossing: never silent when csift stepped over a cut,
/// and silent by construction when it was told not to.
#[derive(Debug, Clone, Default)]
pub struct CrossingReport {
    /// The budget the walk ran under.
    pub budget: CrossingBudget,
    /// Compaction boundaries the walk stepped over.
    pub boundaries_crossed: usize,
    /// The walk met a boundary with no budget left.
    pub budget_exhausted: bool,
    /// Compaction boundaries the walk did NOT cross: every admitted survivor boundary at
    /// or above the floor index, the floor's own included. `boundaries_crossed` plus this
    /// is the budget at which the budget stops being the obstacle.
    pub boundaries_above_floor: usize,
    /// Why the walk stopped where it did - a SINGLE-transcript fact, so it is dropped
    /// (with the floor's address) the moment a second transcript folds into the total: one
    /// scope has as many floors as it has files and naming one of them would name the wrong
    /// one. This is the rule `boundary_cut_line` already follows.
    pub cause: Option<FloorCause>,
    /// Raising the budget would MOVE this floor: it is a boundary the budget stopped at
    /// and the record its `logicalParentUuid` names is still on a line of this file. A
    /// single-transcript fact, dropped with the cause the moment a second one folds in.
    pub floor_crossable: bool,
    /// The floor record's 1-based jsonl line, when the surface can name one.
    pub floor_line: Option<usize>,
    /// The floor record's own timestamp, raw, for the local render.
    pub floor_ts: Option<String>,
}

impl CrossingReport {
    /// A report for a walk that crossed nothing, under the budget the RUN was given.
    ///
    /// The budget is a property of the invocation, never of a file, so every report a
    /// surface builds for itself - an empty transcript, a file the whole-file gate pruned,
    /// a scan that found no target, the accumulator a fold starts from - is built here
    /// rather than by `default()`. A `default()` report carries `Limited(0)`, and folding
    /// one in used to make the SCOPE read as "crossing off": the keys and both footer
    /// lines vanished for every transcript because one file in the set had nothing to say.
    #[must_use]
    pub fn new(budget: CrossingBudget) -> CrossingReport {
        CrossingReport {
            budget,
            ..CrossingReport::default()
        }
    }

    /// Is there anything to say? Nothing at all under budget 0 (csift crossed nothing on
    /// purpose), and nothing on a transcript whose walk never met a compaction boundary.
    #[must_use]
    pub fn any(&self) -> bool {
        !self.budget.is_off()
            && (self.boundaries_crossed > 0
                || self.budget_exhausted
                || matches!(
                    self.cause,
                    Some(
                        FloorCause::UnresolvableBoundary
                            | FloorCause::RelinkRemoved
                            | FloorCause::BoundaryLoop
                    )
                ))
    }

    /// Do the JSON keys ride this stream? Whenever crossing was ENABLED, so a machine
    /// reader can tell "asked to cross, found nothing" from "asked not to cross".
    #[must_use]
    pub fn projects(&self) -> bool {
        !self.budget.is_off()
    }

    /// The budget at which the BUDGET is no longer the obstacle - every boundary in the
    /// file gets tried. Whether the walk then reaches them is a different question, and it
    /// is the one [`CrossingReport::remedy`] answers off the floor's cause.
    #[must_use]
    pub fn resolving_budget(&self) -> usize {
        self.boundaries_crossed + self.boundaries_above_floor
    }

    /// What a reader can DO about the boundaries above the floor. A flag value is named
    /// only where it would MOVE the floor, which takes two facts and not one: the budget
    /// has to be what stopped the walk, AND the boundary it stopped at has to have a door -
    /// a `logicalParentUuid` naming a record still on a line here. A boundary whose door is
    /// shut is a floor at every budget, and a walk that ended at the head, at a dangling
    /// parent or in a cycle never reached the boundaries above it at all; naming a value in
    /// any of those cases sends a reader to re-run a scan that lands on the same record.
    ///
    /// What the named value promises is TRIES, not arrivals: it is the budget at which
    /// every boundary here gets its door opened, and one of them further up may still be
    /// shut. That is the honest promise, because whether a door opens is a property of what
    /// is on disk rather than of the flag.
    fn remedy(&self) -> String {
        match self.cause {
            Some(FloorCause::BudgetExhausted) if self.floor_crossable => format!(
                "to try them: --cross-compactions {} (or all)",
                self.resolving_budget()
            ),
            // The budget stopped the walk at a boundary a budget could not have got past
            // anyway: its logicalParentUuid names no record this file carries, so every
            // larger budget walks to this same record.
            Some(FloorCause::BudgetExhausted) => {
                "raising the budget lands on this same record - the boundary it stopped at \
                 names no record this file carries"
                    .to_string()
            }
            // Every other cause prints its own words immediately before this clause, so the
            // remedy says only what a reader can DO and never repeats the reason.
            Some(_) => "raising the budget reaches none of them".to_string(),
            // A scope of several transcripts has several floors and several causes; the
            // budget is the one remedy that applies to whichever of them it made, and the
            // per-transcript probe is exactly the fact a fold cannot carry.
            None => format!(
                "to try them where a budget can: --cross-compactions {} (or all)",
                self.resolving_budget()
            ),
        }
    }

    /// The footer lines every survival-printing surface emits, in order. Empty unless
    /// [`CrossingReport::any`].
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        if !self.any() {
            return Vec::new();
        }
        let exhausted = if self.budget_exhausted {
            ", exhausted"
        } else {
            ""
        };
        let mut out = vec![format!(
            "(compaction crossing: {} boundary(s) stepped over under --cross-compactions \
             {}{exhausted}; {} above the chain floor were not)",
            self.boundaries_crossed,
            self.budget.as_text(),
            self.boundaries_above_floor
        )];
        if self.boundaries_above_floor > 0 {
            // The floor's ADDRESS and its cause are single-transcript facts; over a scope
            // of several they are dropped and the line names only the remedy.
            let at = match (self.floor_line, self.floor_ts.as_deref()) {
                (Some(l), Some(ts)) => {
                    format!(" is L{l} = {}", crate::timez::format_timestamp(Some(ts)))
                }
                (Some(l), None) => format!(" is L{l}"),
                _ => String::new(),
            };
            let why = self
                .cause
                .map(|c| format!(" ({})", c.reason()))
                .unwrap_or_default();
            out.push(format!(
                "(the chain floor{at}{why}; records above it read pre-cut - {})",
                self.remedy()
            ));
        }
        out
    }

    /// The one-segment form for a surface whose row is a single dense line (`stats`'
    /// `chain` row). Same fields as [`CrossingReport::lines`], no second line: the JSON
    /// row carries the floor's address there.
    #[must_use]
    pub fn compact(&self) -> Option<String> {
        self.any().then(|| {
            format!(
                "compaction boundaries crossed {}/{} (budget {}{})",
                self.boundaries_crossed,
                self.resolving_budget(),
                self.budget.as_text(),
                if self.budget_exhausted {
                    ", exhausted"
                } else {
                    ""
                }
            )
        })
    }

    /// The JSON keys, in the order a summary carries them. Empty unless
    /// [`CrossingReport::projects`].
    #[must_use]
    pub fn json_fields(&self) -> Vec<(&'static str, serde_json::Value)> {
        if !self.projects() {
            return Vec::new();
        }
        vec![
            ("crossing_budget", self.budget.json()),
            (
                "boundaries_crossed",
                serde_json::json!(self.boundaries_crossed),
            ),
            ("budget_exhausted", serde_json::json!(self.budget_exhausted)),
            (
                "boundaries_above_floor",
                serde_json::json!(self.boundaries_above_floor),
            ),
            ("floor_line", serde_json::json!(self.floor_line)),
            (
                "floor_cause",
                serde_json::json!(self.cause.map(FloorCause::as_str)),
            ),
        ]
    }

    /// Fold a whole scope's per-transcript reports into one total under the RUN's budget.
    /// The one place the fold rule lives, so the six surfaces that need a scope total cannot
    /// disagree about it; an empty scope yields a report that crossed nothing.
    #[must_use]
    pub fn fold<'a>(
        budget: CrossingBudget,
        members: impl IntoIterator<Item = &'a CrossingReport>,
    ) -> CrossingReport {
        let mut out = CrossingReport::new(budget);
        for (i, m) in members.into_iter().enumerate() {
            out.add(m, i == 0);
        }
        out
    }

    /// Fold another transcript's report into a scope total. The COUNTS add; the two facts
    /// that name ONE record (the floor's line and its instant) are dropped the moment a
    /// second transcript contributes, the same rule `boundary_cut_line` follows - a line
    /// number from an unnamed file is worse than silence.
    pub fn add(&mut self, other: &CrossingReport, first: bool) {
        // The budget is NOT folded: it is the run's, the accumulator already carries it,
        // and reading it off a member would let one default-built report turn the whole
        // scope's disclosure off.
        self.boundaries_crossed += other.boundaries_crossed;
        self.boundaries_above_floor += other.boundaries_above_floor;
        self.budget_exhausted |= other.budget_exhausted;
        if first {
            self.cause = other.cause;
            self.floor_crossable = other.floor_crossable;
            self.floor_line = other.floor_line;
            self.floor_ts.clone_from(&other.floor_ts);
        } else {
            self.cause = None;
            self.floor_crossable = false;
            self.floor_line = None;
            self.floor_ts = None;
        }
    }
}
