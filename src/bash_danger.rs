//! The dangerous-removal decision for a Bash command, ported from Claude Code and
//! keyed on the Claude Code GENERATION that would have run it.
//!
//! Why csift carries it: `agents` and `status` see a lane whose transcript ends
//! with an unreturned Bash `tool_use`. The disk cannot tell "waiting for a human
//! to approve" from "slow" from "dead" - a pending approval lives only in the
//! harness's process memory. A removal the harness would hold for approval EVEN
//! under bypass-permissions is the one state the jsonl can positively confirm, so
//! such a lane is `escalation-blocked` and every other pending lane is
//! `awaiting-execution`.
//!
//! TWO CHECKERS, ONE GATE (offsets into the 2.1.258 build). The Bash tool's
//! permission path parses the command with tree-sitter and DECOMPOSES it. Only a
//! `too-complex` parse reaches the LEXICAL classifier (`Hno` @162867544 calls
//! `$no` @162861871, which calls `hnt` @162773520); a cleanly parsed removal is
//! decided by the STRUCTURED checker `_9` @162770807, reached through `EPe`
//! @162794175 and `Jon` @162790629. Both write the SAME decision object through
//! the one factory `sF` @162770607 - `behavior:"ask"`, `circuitBreaker:
//! "dangerousRemoval"`, `classifierApprovable:false` - and that breaker is one of
//! the three bypass-immune entries in `_lr` @157113966, so the ask survives
//! bypassPermissions. The generic too-complex ask (`bashMissKind:"too-complex"`)
//! and the no-rule-match ask are NOT immune, which is why a passthrough here is
//! not the same thing as "the harness will run it".
//!
//! THREE GENERATIONS, selected by the `version` stamped on the record:
//! - Gen1, before 2.1.208: the decomposition branch plus `hnt`.
//! - Gen2, 2.1.208 through 2.1.260: adds `Bno` @162863005 - the substitution
//!   census with its bail above 64, the per-substitution re-run of `hnt`, and a
//!   bounded sixteen-iteration `__CMDSUB__` fixpoint feeding `_9`.
//! - Gen3, 2.1.261 and later: `hnt` is replaced by the rewritten `out`, which
//!   walks tokens instead of matching a clause head, scans `find -exec`, widens
//!   the target forms and recurses into a nested `sh -c`.
//!
//! `hnt` itself is byte-identical from 2.1.142 through 2.1.260, fixpoint included.
//!
//! THE PORT PREDICTS THE HARNESS AND NEVER IMPROVES ON IT. Where a decision needs
//! the filesystem state at the time - `_9` resolves the operand against the shell
//! cwd, realpaths both, and compares against the working-directory set - the
//! verdict is `Unresolved`, never a guess in either direction.

use crate::bash_danger_census::CensusVerdict;
use crate::bash_danger_removal::OperandVerdict;

pub use crate::bash_danger_shape::Branch;

/// The lexical classifier's reason tail @162862700, verbatim.
pub const TAIL_POSSIBLY_EMPTY: &str = "on possibly-empty variable path";
/// csift's own note for a verdict the transcript cannot decide. It is NOT a
/// harness tail and is never presented as one.
pub const NOTE_NEEDS_FS: &str = "removal target needs the filesystem state at the time";

/// Which Claude Code generation's checker chain applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    /// Before 2.1.208: decomposition branch plus the lexical classifier.
    Gen1,
    /// 2.1.208 through 2.1.260: adds the substitution census.
    Gen2,
    /// 2.1.261 and later: the rewritten lexical classifier.
    Gen3,
}

impl Generation {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Generation::Gen1 => "gen1",
            Generation::Gen2 => "gen2",
            Generation::Gen3 => "gen3",
        }
    }
}

/// What the harness would do with the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// A dangerous-removal ask. Always bypass-immune.
    Ask,
    /// No dangerous removal was found by the checker that ran.
    Passthrough,
    /// The deciding arm reads state the transcript does not carry.
    Unresolved,
}

/// Which checker produced the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checker {
    /// The lexical classifier on the too-complex arm.
    Lexical,
    /// The structured removal checker on the cleanly parsed arm.
    Structured,
    /// The substitution census and the work that hangs off it.
    Census,
}

impl Checker {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Checker::Lexical => "lexical",
            Checker::Structured => "structured",
            Checker::Census => "census",
        }
    }
}

/// One command's predicted decision, with every fact the prediction rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub decision: Decision,
    /// True exactly for an ask: `dangerousRemoval` is bypass-immune.
    pub immune: bool,
    pub checker: Checker,
    /// For an ask, the harness's own reason tail with its value interpolated. For
    /// an unresolved verdict, csift's note. Empty for a passthrough.
    pub reason: String,
    pub generation: Generation,
    /// True when the record carried no usable `version` and the generation of the
    /// build the ledger is verified against was assumed.
    pub generation_assumed: bool,
    pub branch: Branch,
}

impl Verdict {
    /// Would the harness hold this command for a human even under bypass?
    #[must_use]
    pub fn blocks(&self) -> bool {
        self.decision == Decision::Ask && self.immune
    }

    /// The one spelling of the disclosure line: which checker, which generation,
    /// and which branch the command took.
    #[must_use]
    pub fn disclosure(&self) -> String {
        let assumed = if self.generation_assumed {
            " assumed"
        } else {
            ""
        };
        format!(
            "path: {} · checker: {} · {}{}",
            self.branch.label(),
            self.checker.label(),
            self.generation.label(),
            assumed
        )
    }
}

/// Decide what the harness would do with `command`, for the Claude Code `version`
/// stamped on the record that carries it.
#[must_use]
pub fn classify(command: &str, version: Option<&str>) -> Verdict {
    let (generation, generation_assumed) = generation_of(version);
    let branch = crate::bash_danger_shape::branch_of(command);
    let build = |decision, checker, reason: String| Verdict {
        decision,
        immune: decision == Decision::Ask,
        checker,
        reason,
        generation,
        generation_assumed,
        branch,
    };
    match branch {
        Branch::Structured => match crate::bash_danger_removal::structured(command) {
            OperandVerdict::Ask { tail, target } => build(
                Decision::Ask,
                Checker::Structured,
                format!("{tail}: {target}"),
            ),
            OperandVerdict::NeedsFs => build(
                Decision::Unresolved,
                Checker::Structured,
                NOTE_NEEDS_FS.to_string(),
            ),
            OperandVerdict::Clear => {
                build(Decision::Passthrough, Checker::Structured, String::new())
            }
        },
        Branch::Lexical(_) => {
            if let Some((verb, target)) = lexical_hit(command, generation) {
                let _ = verb;
                return build(
                    Decision::Ask,
                    Checker::Lexical,
                    format!("{TAIL_POSSIBLY_EMPTY}: {target}"),
                );
            }
            if generation == Generation::Gen1 {
                return build(Decision::Passthrough, Checker::Lexical, String::new());
            }
            match crate::bash_danger_census::census(command, generation == Generation::Gen3) {
                CensusVerdict::Bail { count } => build(
                    Decision::Ask,
                    Checker::Census,
                    format!("{} ({count})", crate::bash_danger_census::TAIL_TOO_MANY),
                ),
                CensusVerdict::Lexical { verb, target } => {
                    let _ = verb;
                    build(
                        Decision::Ask,
                        Checker::Census,
                        format!(
                            "{}: {target}",
                            crate::bash_danger_census::TAIL_IN_SUBSTITUTION
                        ),
                    )
                }
                CensusVerdict::Structured { tail, target } => {
                    build(Decision::Ask, Checker::Census, format!("{tail}: {target}"))
                }
                CensusVerdict::NeedsFs => build(
                    Decision::Unresolved,
                    Checker::Census,
                    NOTE_NEEDS_FS.to_string(),
                ),
                CensusVerdict::Clear => {
                    build(Decision::Passthrough, Checker::Census, String::new())
                }
            }
        }
    }
}

/// The lexical classifier of one generation.
fn lexical_hit(command: &str, generation: Generation) -> Option<(&'static str, String)> {
    if generation == Generation::Gen3 {
        crate::bash_danger_out::out(command).map(|h| (h.command, h.target))
    } else {
        crate::bash_danger_lexical::hnt(command).map(|h| (h.command, h.target))
    }
}

/// The first Claude Code version carrying the substitution census: the literal
/// `too many to analyze for catastrophic removals` is present at 2.1.208 and
/// absent at 2.1.207.
const CENSUS_FLOOR: (u32, u32, u32) = (2, 1, 208);
/// The first Claude Code version carrying the rewritten lexical classifier: the
/// new target regex's literal is present at 2.1.261 and absent at 2.1.260.
const REWRITE_FLOOR: (u32, u32, u32) = (2, 1, 261);

/// The generation a version belongs to, and whether it had to be assumed. A
/// missing or unparseable version falls to the generation of the build the
/// introspection ledger is verified against (2.1.258, which is Gen2).
#[must_use]
pub fn generation_of(version: Option<&str>) -> (Generation, bool) {
    let Some(triple) = version.and_then(parse_triple) else {
        return (Generation::Gen2, true);
    };
    if triple >= REWRITE_FLOOR {
        (Generation::Gen3, false)
    } else if triple >= CENSUS_FLOOR {
        (Generation::Gen2, false)
    } else {
        (Generation::Gen1, false)
    }
}

fn parse_triple(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.split('.');
    let major = leading_number(parts.next()?)?;
    let minor = leading_number(parts.next()?)?;
    let patch = leading_number(parts.next()?)?;
    Some((major, minor, patch))
}

fn leading_number(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
#[path = "bash_danger_tests.rs"]
mod tests;
