//! Character-level shortest-edit-script distance (Myers O(ND)), bounded.
//!
//! The one consumer is the C-27 unsent diff line: how far a superseded draft
//! (`user.unsent`) sits from the message that replaced it. The number reported is
//! INSERTIONS + DELETIONS of a shortest character-level edit script - the count of
//! characters covered by the differing regions - never a length difference, so a
//! pure reorder or a mid-text replacement reads as the work it actually was.
//!
//! Characters, not bytes: a multi-byte body must never be split inside a UTF-8
//! sequence, and a percentage of a byte length would misreport every non-ASCII draft.
//!
//! THE COST, and the four things that bound it. Myers is O(N*D) in the residual
//! length N and the distance D, and the greedy loop ALONE walks sum(d+1 for d<=D) =
//! about D^2/2 diagonals, so a cap on D is nowhere near a cap on work: at D = 200k
//! that is 2e10 steps. In order:
//!   1. the common head and tail are stripped, so only the residual is ever walked
//!      (this is what keeps a multi-megabyte draft with a small edit exact and cheap);
//!   2. one O(N) subsequence test decides the shape the corpus is full of - a short
//!      draft replaced by a much longer message. If the shorter side IS a subsequence
//!      of the longer, the distance IS the length difference, exactly and for free;
//!      if it is NOT, the distance is strictly GREATER than that difference;
//!   3. that difference is therefore a strict floor, and it is the floor every giving-up
//!      arm reports - never a weaker one, so a pair past [`MAX_DIFF_CHARS`] states its
//!      own length difference rather than the cap. It also lets the walk be skipped
//!      whenever its first affordable depth already exceeds the budget (measured: 32 of
//!      892 corpus drafts, 97.5% of the walk time);
//!   4. what survives all that walks under [`MAX_DIFF_STEPS`].
//!
//! Whenever a bound stops the walk the result is a PROVEN STRICT floor - the true
//! distance is greater than the number reported - and the caller renders it as
//! "more than N", never as an estimate.

/// The distance cap: past this many differing characters the exact number stops
/// being informative (the draft and the resend are simply different texts) and the
/// O(ND) walk stops being worth its cost. It bounds the WALK, not the number: a pair
/// this far apart reports its own length difference, which is both larger and known.
pub(crate) const MAX_DIFF_CHARS: usize = 200_000;

/// The work budget for the greedy walk, in diagonal steps plus characters slid along
/// the common runs. It is the bound the distance cap cannot give: two texts of equal
/// length that share nothing reach D = N + M, and the loop's D^2/2 diagonals would
/// run for minutes on a megabyte pair. Sized so one draft can never cost more than a
/// few milliseconds, which is what keeps a whole-corpus draft scan unchanged.
const MAX_DIFF_STEPS: usize = 2_000_000;

/// The distance between two texts: `chars` insertions plus deletions when `exact`,
/// otherwise a proven lower bound (the true distance is strictly greater).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CharDiff {
    pub(crate) chars: usize,
    pub(crate) exact: bool,
}

/// Characters covered by the differing regions of `a` and `b` (see the module doc).
pub(crate) fn char_diff(a: &str, b: &str) -> CharDiff {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    // Strip the common head and tail before measuring: a shortest edit script never
    // touches either, so the residual pair is exactly what the walk has to cost.
    let mut head = 0usize;
    while head < av.len() && head < bv.len() && av[head] == bv[head] {
        head += 1;
    }
    let (a_rest, b_rest) = (av.len() - head, bv.len() - head);
    let mut tail = 0usize;
    while tail < a_rest && tail < b_rest && av[av.len() - 1 - tail] == bv[bv.len() - 1 - tail] {
        tail += 1;
    }
    myers(&av[head..av.len() - tail], &bv[head..bv.len() - tail])
}

/// The greedy Myers walk over the already-stripped residuals.
fn myers(a: &[char], b: &[char]) -> CharDiff {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        // A pure insertion or deletion: the distance IS the surviving side's length,
        // free to state exactly however large it is (no walk happens here).
        return CharDiff {
            chars: n + m,
            exact: true,
        };
    }
    // A shortest script must at least reconcile the length difference, so that
    // difference is already a floor - and it is the floor every later arm reports,
    // because a weaker one (the cap, a shallow walk) would understate what is known.
    let lb = n.abs_diff(m);
    // The dominant corpus shape: a short draft replaced by a much longer message. If
    // the shorter side is a SUBSEQUENCE of the longer, the script is a pure insertion
    // and the distance IS `lb`, decided in one linear pass instead of an lb^2/2 walk.
    // Tested BEFORE the cap: it settles the exact case at any size, and it is what
    // makes `lb` a STRICT floor for everything below (the distance exceeds it).
    if is_subsequence(a, b) || is_subsequence(b, a) {
        return CharDiff {
            chars: lb,
            exact: true,
        };
    }
    // Not a subsequence, so the distance is STRICTLY greater than `lb` (and, by parity,
    // at least lb + 2). Past the cap the walk is out of the question and `lb` is the
    // answer; below it, the walk could only improve on `lb` after depth lb, which costs
    // about lb^2/2 diagonals, so when that alone exceeds the budget `lb` is again the
    // floor to report rather than a number bought with seconds of work.
    if lb > MAX_DIFF_CHARS || lb.saturating_mul(lb) / 2 > MAX_DIFF_STEPS {
        return CharDiff {
            chars: lb,
            exact: false,
        };
    }
    let max_d = (n + m).min(MAX_DIFF_CHARS);
    let off = max_d + 1;
    // The furthest-reaching endpoint per diagonal k, indexed at `off + k`.
    let mut v = vec![0isize; 2 * max_d + 3];
    let mut steps = 0usize;
    for d in 0..=max_d {
        let di = d as isize;
        for k in (-di..=di).step_by(2) {
            let (x, y, slid) = step(a, b, &v, off, di, k);
            steps += slid;
            let idx = (off as isize + k) as usize;
            v[idx] = x;
            if x >= n as isize && y >= m as isize {
                return CharDiff {
                    chars: d,
                    exact: true,
                };
            }
        }
        steps += d + 1;
        if steps > MAX_DIFF_STEPS {
            // Every distance up to d was explored and failed, so the true distance is
            // strictly greater than d; it is strictly greater than `lb` too (the
            // subsequence test above ruled that out). Report the stronger floor.
            return CharDiff {
                chars: d.max(lb),
                exact: false,
            };
        }
    }
    CharDiff {
        chars: max_d,
        exact: false,
    }
}

/// Is `small` a subsequence of `large` (its characters in order, gaps allowed)? The
/// greedy earliest-match walk is exact for this question and runs in one pass, which
/// is what makes "the draft was replaced by a longer message that still contains it"
/// a free answer instead of a quadratic walk. False when `small` is the longer side.
fn is_subsequence(small: &[char], large: &[char]) -> bool {
    if small.len() > large.len() {
        return false;
    }
    let mut it = large.iter();
    small.iter().all(|c| it.any(|l| l == c))
}

/// One diagonal of the frontier: take the cheaper of the two predecessors (down =
/// an insertion, right = a deletion), then slide along the common run. Returns the
/// endpoint and how many characters the slide consumed (for the step budget).
fn step(
    a: &[char],
    b: &[char],
    v: &[isize],
    off: usize,
    d: isize,
    k: isize,
) -> (isize, isize, usize) {
    let idx = (off as isize + k) as usize;
    let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
        v[idx + 1]
    } else {
        v[idx - 1] + 1
    };
    let mut y = x - k;
    let mut slid = 0usize;
    while x >= 0
        && y >= 0
        && x < a.len() as isize
        && y < b.len() as isize
        && a[x as usize] == b[y as usize]
    {
        x += 1;
        y += 1;
        slid += 1;
    }
    (x, y, slid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_have_no_distance() {
        assert_eq!(
            char_diff("chart the reef", "chart the reef"),
            CharDiff {
                chars: 0,
                exact: true
            }
        );
        assert_eq!(
            char_diff("", ""),
            CharDiff {
                chars: 0,
                exact: true
            }
        );
    }

    #[test]
    fn a_pure_insertion_counts_the_inserted_characters() {
        // "chart the reef" -> "chart the whole reef": six inserted characters.
        let d = char_diff("chart the reef", "chart the whole reef");
        assert_eq!(
            d,
            CharDiff {
                chars: 6,
                exact: true
            }
        );
        // The prepend case the parent-uuid rule exists for.
        assert_eq!(char_diff("look", "take a closer look").chars, 14);
    }

    #[test]
    fn a_pure_deletion_counts_the_deleted_characters() {
        let d = char_diff("chart the whole reef", "chart the reef");
        assert_eq!(
            d,
            CharDiff {
                chars: 6,
                exact: true
            }
        );
        // One side empty: the distance is the other side's length, stated exactly.
        assert_eq!(char_diff("abcd", "").chars, 4);
        assert_eq!(char_diff("", "abcd").chars, 4);
    }

    #[test]
    fn a_replacement_costs_a_deletion_plus_an_insertion() {
        // "reef" -> "reeX": one delete + one insert, NOT a length difference of 0.
        assert_eq!(char_diff("reef", "reeX").chars, 2);
        // Mid-text replacement of three characters by two.
        assert_eq!(char_diff("ab-XYZ-cd", "ab-QP-cd").chars, 5);
    }

    #[test]
    fn the_metric_is_not_the_length_difference() {
        // Same length, completely different: every character is replaced.
        assert_eq!(char_diff("abcd", "wxyz").chars, 8);
    }

    #[test]
    fn multibyte_characters_count_as_one_each() {
        // Accented Latin: one character replaced, so one delete + one insert.
        assert_eq!(char_diff("caf\u{e9} au lait", "caf\u{e8} au lait").chars, 2);
        // An emoji is ONE character, not four bytes.
        assert_eq!(
            char_diff("ok \u{1f600}", "ok "),
            CharDiff {
                chars: 1,
                exact: true
            }
        );
        // A CJK edit: two characters inserted.
        assert_eq!(
            char_diff("\u{6d77}\u{56fe}", "\u{6d77}\u{6d0b}\u{5730}\u{56fe}").chars,
            2
        );
    }

    #[test]
    fn the_common_head_and_tail_are_stripped_before_the_walk() {
        // A large shared body with one edit in the middle stays exact and cheap:
        // the walk only ever sees the residual, so this must not hit any bound.
        let head = "x".repeat(400_000);
        let tail = "y".repeat(400_000);
        let a = format!("{head}ALPHA{tail}");
        let b = format!("{head}BETA{tail}");
        let d = char_diff(&a, &b);
        assert!(
            d.exact,
            "a small edit inside a huge shared body stays exact"
        );
        // The shared trailing "A" is stripped too, leaving ALPH against BET: nothing
        // in common there, so four deletes and three inserts.
        assert_eq!(d.chars, 7, "the residual pair is what gets measured");
    }

    #[test]
    fn a_distance_past_the_cap_reports_the_length_difference_not_the_cap() {
        // Nothing shared and a length difference beyond the cap: the walk never runs,
        // and the floor reported is the LENGTH DIFFERENCE (200,010 - 5 = 200,005),
        // which is both known and stronger than the cap. Reporting the cap here would
        // understate a distance the lengths alone already prove.
        let a = "a".repeat(MAX_DIFF_CHARS + 10);
        let b = "b".repeat(5);
        let d = char_diff(&a, &b);
        assert!(!d.exact, "past the cap the result is a floor: {d:?}");
        assert_eq!(d.chars, MAX_DIFF_CHARS + 5, "the length difference: {d:?}");
        assert!(
            d.chars > MAX_DIFF_CHARS,
            "never a floor weaker than the length difference: {d:?}"
        );
    }

    #[test]
    fn a_draft_the_resend_still_contains_is_exact_for_free() {
        // The dominant expensive shape: a short draft, a much longer resend that
        // keeps every character of it. A pure insertion, so the distance IS the
        // length difference and the walk never runs.
        let a = "chart the reef";
        let b = format!("please {} and the harbor before the tide turns", a);
        let d = char_diff(a, &b);
        assert!(d.exact, "a contained draft is exact: {d:?}");
        assert_eq!(d.chars, b.chars().count() - a.chars().count());
    }

    #[test]
    fn a_long_replacement_reports_the_length_difference_as_a_floor() {
        // Not a subsequence (the draft carries a character the resend lacks), and the
        // first depth that could beat the length difference costs more than the whole
        // budget: the difference itself is the floor, and it is a STRICT one.
        let a = format!("{}Q", "a".repeat(10));
        let b = "a".repeat(5_000);
        let d = char_diff(&a, &b);
        assert!(!d.exact, "{d:?}");
        assert_eq!(d.chars, 4_989, "the residual length difference: {d:?}");
    }

    #[test]
    fn a_wide_walk_stops_on_the_step_budget_with_a_floor() {
        // Equal lengths, no shared character, so the length bound says nothing and
        // the greedy walk would run for D^2/2 diagonals: the step budget stops it
        // and reports the largest distance it fully explored.
        let a = "a".repeat(60_000);
        let b = "b".repeat(60_000);
        let d = char_diff(&a, &b);
        assert!(!d.exact, "the budget stops the walk: {d:?}");
        // The exact depth matters: the floor is only a floor because every distance up
        // to it was explored IN FULL, which is one diagonal for d=0, two for d=1 and so
        // on. Nothing slides here, so depth 1999 is where that sum first passes the
        // budget - report 2000 and the number stops being a proven floor.
        assert_eq!(
            d.chars, 1999,
            "the floor is the last fully explored depth: {d:?}"
        );
    }

    /// The exact insert-plus-delete distance from the LCS table - the definition the
    /// greedy walk is an optimisation of. Quadratic, so it only ever runs on tiny pairs.
    fn reference_distance(a: &str, b: &str) -> usize {
        let av: Vec<char> = a.chars().collect();
        let bv: Vec<char> = b.chars().collect();
        let mut lcs = vec![vec![0usize; bv.len() + 1]; av.len() + 1];
        for i in 1..=av.len() {
            for j in 1..=bv.len() {
                lcs[i][j] = if av[i - 1] == bv[j - 1] {
                    lcs[i - 1][j - 1] + 1
                } else {
                    lcs[i - 1][j].max(lcs[i][j - 1])
                };
            }
        }
        av.len() + bv.len() - 2 * lcs[av.len()][bv.len()]
    }

    #[test]
    fn the_walk_is_exact_against_the_reference_on_every_small_pair() {
        // The walk's own arithmetic - which predecessor a diagonal extends, the slide
        // guard, how far the slide runs - has no output surface of its own: a wrong step
        // shows up only as a wrong distance on SOME pair. So sweep a deterministic set of
        // small pairs over a 3-letter alphabet (shared runs, ties between the two
        // predecessors, one side empty, equal lengths) and check every answer against the
        // definition. Nothing here is near a bound, so every answer must also be exact.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = ['a', 'b', 'c'];
        for _ in 0..600 {
            let mut pair = [String::new(), String::new()];
            for side in &mut pair {
                let len = next() % 11;
                for _ in 0..len {
                    side.push(alphabet[(next() % 3) as usize]);
                }
            }
            let (a, b) = (&pair[0], &pair[1]);
            let got = char_diff(a, b);
            assert!(got.exact, "a tiny pair never hits a bound: {a:?} {b:?}");
            assert_eq!(
                got.chars,
                reference_distance(a, b),
                "walked {a:?} against {b:?}"
            );
        }
    }

    #[test]
    fn a_huge_common_head_is_stripped_before_the_budget_can_see_it() {
        // The head strip is not a speed-up alone. Sliding along a common run longer than
        // the whole step budget would spend it at depth 0 and turn an exact answer into a
        // floor of the length difference. Stripped, the walk only ever sees the residual.
        let head = "x".repeat(MAX_DIFF_STEPS + 100_000);
        let a = format!("{head}ALPHA");
        let b = format!("{head}BETA");
        let d = char_diff(&a, &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 7,
                exact: true
            },
            "the residual pair (ALPH against BET) is what gets measured: {d:?}"
        );
    }

    #[test]
    fn a_huge_common_tail_is_stripped_before_the_budget_can_see_it() {
        // The mirror of the head case, with a length difference so the slide happens on a
        // diagonal that does NOT reach the endpoint: unstripped, that wrong diagonal runs
        // the whole common run at depth 2, spends the budget, and reports a floor of 2
        // where the residual pair ("y" against "CD") is worth an exact 3.
        let tail = "y".repeat(MAX_DIFF_STEPS + 100_000);
        let a = format!("y{tail}");
        let b = format!("CD{tail}");
        let d = char_diff(&a, &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 3,
                exact: true
            },
            "{d:?}"
        );
    }

    #[test]
    fn a_contained_draft_stays_exact_however_far_past_the_cap_it_sits() {
        // The subsequence test runs BEFORE the cap precisely so the dominant corpus shape
        // - a short draft the resend still contains - is settled exactly at any size. The
        // insertion is split in two so the head and tail strips cannot reduce it to the
        // one-side-empty case. Lose the shortcut and the number is the same but becomes a
        // floor, which reads to the caller as "more than 700000".
        let b = format!("A{}B{}", "c".repeat(300_000), "d".repeat(400_000));
        let d = char_diff("AB", &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 700_000,
                exact: true
            },
            "{d:?}"
        );
        assert!(d.chars > MAX_DIFF_CHARS, "well past the cap: {d:?}");
    }

    #[test]
    fn the_walk_runs_when_its_first_affordable_depth_fits_the_budget() {
        // The give-up guard is "the walk could only improve on lb after depth lb, which
        // costs about lb^2/2". At lb = 1500 that is 1.1M steps, inside the budget, so the
        // walk runs and returns the exact distance. Widen the guard and this pair reports
        // 1500 as a floor instead of 1502 as a fact.
        let a = format!("Q{}", "a".repeat(10));
        let b = format!("R{}", "a".repeat(1510));
        let d = char_diff(&a, &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 1502,
                exact: true
            },
            "{d:?}"
        );
    }

    #[test]
    fn the_step_budget_counts_the_characters_slid_not_only_the_diagonals() {
        // One common run longer than the budget, in the MIDDLE: the diagonals alone are a
        // handful, so only the slide can spend the budget. It does, at depth 2, and the
        // answer is the floor that depth proves - not the exact 4 an uncounted slide
        // would go on to find.
        let mid = "m".repeat(MAX_DIFF_STEPS + 100_000);
        let a = format!("P{mid}Q");
        let b = format!("R{mid}S");
        let d = char_diff(&a, &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 2,
                exact: false
            },
            "{d:?}"
        );
    }

    #[test]
    fn the_budget_gives_up_past_the_limit_not_on_it() {
        // Sized so the walk stands EXACTLY on the budget at the end of depth 2: one
        // diagonal at d=0, two at d=1, the slide, then three at d=2. Standing on the
        // limit is not exceeding it, so the walk goes one depth further and reports the
        // stronger floor.
        let mid = "m".repeat(MAX_DIFF_STEPS - 6);
        let a = format!("P{mid}Q");
        let b = format!("R{mid}S");
        let d = char_diff(&a, &b);
        assert_eq!(
            d,
            CharDiff {
                chars: 3,
                exact: false
            },
            "{d:?}"
        );
    }
}
