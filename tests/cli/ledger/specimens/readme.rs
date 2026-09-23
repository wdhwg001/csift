//! Rules 1, 5 and 8: the README's two badges and its regenerated tally block.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

fn specimen_anchors() -> anchors::Anchors {
    anchors::Anchors {
        // The five counts PARTITION the total, which the gate asserts, so the fixture
        // every rule-8 specimen deviates from is one that does: 1 + 1 + 1 + 0 + 1 = 4.
        total: 4,
        absent: 1,
        prefix_only: 0,
        closed: 2,
        other_build: 1,
        measured: Some((1, 1, 0)),
    }
}

/// The page says the five counts partition the total, so the gate checks that they do. A
/// PARTITION is what makes the arithmetic re-derivable: every anchor falls in exactly ONE
/// bucket, so an `absent`-marked hop also credited with its real bytes is counted twice
/// and the sum runs over the total. The arithmetic is the page's to own.
#[test]
fn ledger_gate_rule_8_names_anchor_counts_that_do_not_partition_the_total() {
    let claims = vec![good_claim()];
    let mut a = specimen_anchors();
    a.measured = Some((3, 1, 0)); // one more byte-exact than the total can hold
    let good = tally::render_block(&claims, &a, "2.1.258", (2, 1, 0));
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &good, &mut f);
    assert!(
        f.iter()
            .any(|m| m.contains("do not partition the total") && m.contains("= 6, against 4")),
        "{f:?}"
    );
}

/// Rule 8 is a GENERATOR, so its specimen is the generator's own output with one byte
/// changed. That is the whole point of regenerating rather than matching rows: a hand-typed
/// number anywhere in the block - a count, a share, the parenthetical, a word of the
/// meaning - is a difference, and the failure hands back the block that should be there.
#[test]
fn ledger_gate_rule_8_names_a_tally_block_that_disagrees_with_the_ledger() {
    let claims = vec![good_claim()];
    let a = specimen_anchors();
    let good = tally::render_block(&claims, &a, "2.1.258", (1, 1, 0));
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &good, &mut f);
    assert!(
        f.is_empty(),
        "the generated block is the one it expects: {f:?}"
    );

    // A count.
    let bad = good.replace("| end-to-end | 1 |", "| end-to-end | 9 |");
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &bad, &mut f);
    assert!(only(&f).contains("README ledger tally block differs from the ledger's"));

    // A SHARE - the column a counts-only row regex never reads at all.
    let bad = good.replace("| 100.0% |", "| 79.2% |");
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &bad, &mut f);
    assert!(only(&f).contains("the block is REGENERATED, never typed"));

    // The chain-traced row, whose key is not a bare word and was invisible to the regex.
    let bad = good.replace("of which chain traced | 1 |", "of which chain traced | 5 |");
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &bad, &mut f);
    assert_eq!(f.len(), 1, "the chain-traced row is read: {f:?}");

    // The parenthetical inside a meaning cell.
    let bad = good.replace(
        "(0 of them with a partly traced writer)",
        "(24 of them with a partly traced writer)",
    );
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &bad, &mut f);
    assert_eq!(f.len(), 1, "the parenthetical is read: {f:?}");

    // The ELLIPSIS SUBSET, one off. It is the one figure in the block that had been a
    // literal in the gate's own source rather than a count it took, and it was stale by
    // 154 in four documents at once; a number the page states is a number the page is held
    // to, so moving it by one is a difference like any other. On a host with no build the
    // count is read BACK off the page, which is why the mutation has to move the ledger's
    // reading of it too - otherwise the page would simply be agreeing with itself.
    let mut off_by_one = specimen_anchors();
    off_by_one.measured = Some((1, 1, 1));
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &off_by_one, "2.1.258", &good, &mut f);
    assert_eq!(f.len(), 1, "the ellipsis subset is read: {f:?}");
    assert!(
        f[0].contains("1 such excerpts ARE byte-exact"),
        "the failure hands back the block that should be there: {}",
        f[0]
    );

    // And the anchors line.
    let bad = good.replace("closed-state anchors 2", "closed-state anchors 7");
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &bad, &mut f);
    assert_eq!(f.len(), 1, "the anchors line is read: {f:?}");
}

#[test]
fn ledger_gate_rule_1_and_5_name_a_badge_that_disagrees() {
    let mut f = Vec::new();
    tally::badges(
        "2.1.258",
        "![](https://img.shields.io/badge/verified%20against%20Claude%20Code-2.1.200-x) \
         ![](https://img.shields.io/badge/mutation%20score-90.1%25-x)",
        &mut f,
    );
    assert!(only(&f)
        .contains("README badge says Claude Code 2.1.200 but the ledger is verified at 2.1.258"));

    let mut f = Vec::new();
    tally::badges(
        "2.1.258",
        "![](https://img.shields.io/badge/verified%20against%20Claude%20Code-2.1.258-x)",
        &mut f,
    );
    assert!(only(&f).contains("README has no mutation-score badge"));
}
