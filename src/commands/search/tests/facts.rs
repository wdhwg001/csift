//! The two EXTENSION POINTS as a consumer reads them: a family's own `Fact` reaching
//! both projections through [`HitFact`], and the one rule that folds facts onto a JSON
//! object.

use super::*;

use crate::commands::search::record_text::bookkeeping;

/// A family fact reaches BOTH projections through its `HitFact` arm.
///
/// This is the pin the six family arms rest on. Each arm forwards `label_zone` and
/// `json` to the family's own enum, and with every family uninhabited nothing takes
/// either forward - so both could be replaced by `None` and an empty list and the whole
/// suite would still pass. The bookkeeping family carries one `cfg(test)` variant for
/// exactly this, and the assertion is built from that variant's own constants rather
/// than from whatever the code returned.
#[test]
fn a_family_fact_reaches_both_projections_through_its_arm() {
    let fact = HitFact::Bookkeeping(bookkeeping::Fact::Probe("probe head"));
    assert_eq!(fact.label_zone().as_deref(), Some("probe head"));
    assert_eq!(
        fact.json(),
        vec![("bookkeeping_probe", serde_json::json!("probe head"))]
    );
}

/// And a family that has no variants yet projects NOTHING - which is what every other
/// arm does today, and why the pin above has to exist at all.
#[test]
fn an_uninhabited_family_contributes_no_projection() {
    // There is no value of an uninhabited `Fact` to build, so the statement is about
    // the hit: a unit of a record no family owns carries an empty fact list, and the
    // two projectors therefore add nothing to it.
    let mut obj = serde_json::json!({ "label": "user.message" });
    attach_facts(&mut obj, &[]);
    assert_eq!(obj, serde_json::json!({ "label": "user.message" }));
}

/// The MERGE rule, scalar side: a fact's key is written onto the object, and a second
/// fact writing the same key REPLACES it.
///
/// Replacement is the right answer for a scalar - two facts claiming one scalar key is a
/// family bug, and keeping the first would hide it behind stale data rather than showing
/// the newer answer. The ARRAY side of the same rule accumulates instead, which is what
/// lets one unit carry several facts of one kind.
#[test]
fn a_scalar_fact_key_is_written_and_a_second_replaces_it() {
    let mut obj = serde_json::json!({ "label": "harness.bookkeeping.mode" });
    attach_facts(
        &mut obj,
        &[
            HitFact::Bookkeeping(bookkeeping::Fact::Probe("first")),
            HitFact::Bookkeeping(bookkeeping::Fact::Probe("second")),
        ],
    );
    assert_eq!(obj["bookkeeping_probe"], serde_json::json!("second"));
    // The fixed keys the projector wrote before the facts are untouched: a fact
    // appends, it never rebuilds the row.
    assert_eq!(obj["label"], serde_json::json!("harness.bookkeeping.mode"));
}

/// The unmodeled SHAPE still projects what it always did - one key, one string - through
/// the widened `Vec` return. The 1:1 law's whole wire surface is that key.
#[test]
fn the_unmodeled_shape_projects_one_key_through_the_widened_return() {
    let fact = HitFact::Unmodeled("system/some_future_subtype".to_string());
    assert_eq!(fact.label_zone(), None, "the shape IS the label, not a tag");
    assert_eq!(
        fact.json(),
        vec![("unmodeled", serde_json::json!("system/some_future_subtype"))]
    );
}
