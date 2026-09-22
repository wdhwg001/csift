//! Rules 1, 5 and 8: the README's two badges and its regenerated tally block.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

fn specimen_anchors() -> anchors::Anchors {
    anchors::Anchors {
        total: 3,
        absent: 1,
        prefix_only: 0,
        closed: 2,
        other_build: 1,
        measured: Some((1, 1)),
    }
}

/// Rule 8 is a GENERATOR, so its specimen is the generator's own output with one byte
/// changed. That is the whole point of regenerating rather than matching rows: a hand-typed
/// number anywhere in the block - a count, a share, the parenthetical, a word of the
/// meaning - is a difference, and the failure hands back the block that should be there.
#[test]
fn ledger_gate_rule_8_names_a_tally_block_that_disagrees_with_the_ledger() {
    let claims = vec![good_claim()];
    let a = specimen_anchors();
    let good = tally::render_block(&claims, &a, "2.1.258", (1, 1));
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

    // A SHARE - the column the old row regex never read at all.
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
