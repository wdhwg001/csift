//! `@<Name>@<Team>`: teammate routing form -> the unique transcript-form agent id.

use super::*;

/// Resolve a teammate ROUTING form to the ONE transcript-form agent id it names, scanning the
/// teammate metas under `dirs`. The answer lives on disk (a meta's `name` + `teamName`), so
/// this runs after the project dirs are known; the id it returns then dispatches exactly like
/// an `@<agent-id>` target.
///
/// Fail-loud both ways, per the targeting law: zero matches names the grammar and how to list
/// the real ids; more than one lists every matching TRANSCRIPT id, because the ambiguity is
/// real - two same-named teammates in one team share the routing form (Claude Code mints the
/// id from the requested name and de-duplicates only its own registry key), while the
/// transcript id carries random hex and never collides.
pub(crate) fn resolve_teammate_routing(
    dirs: &[ProjectDir],
    name: &str,
    team: &str,
) -> Result<String> {
    let mut hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for pd in dirs {
        for top in top_level_jsonls(&pd.dir) {
            for id in crate::subagent::teammate_ids_by_routing(&top, name, team)? {
                hits.insert(id);
            }
        }
    }
    let ids: Vec<String> = hits.into_iter().collect();
    match ids.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!(
            "no teammate `{name}@{team}` under the resolved target(s) — `@<name>@<team>` is a \
             teammate's ROUTING form (what the official SendMessage addresses): a name, `@`, \
             its team, both matched EXACTLY (case included). List the teammates in scope with \
             `csift agents <session> --shape teammate` and read the `routing:` field, or \
             target the transcript id `a<Name>-<hex>` directly."
        ),
        many => bail!(
            "`@{name}@{team}` is AMBIGUOUS: {} teammates in scope share that routing id ({}). \
             The routing form CAN collide (two same-named teammates in one team share it); the \
             transcript id never does — target one of those directly.",
            many.len(),
            many.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    // The grammar predicate is exercised beside the resolver that consumes it: the two are
    // one decision (what shape routes here, and what it splits into).
    use super::*;

    #[test]
    fn routing_form_grammar_accepts_name_at_team_only() {
        assert!(is_teammate_routing_id("Relay@harbor"));
        assert!(is_teammate_routing_id("P1-engine@region_two"));
        assert!(is_teammate_routing_id("a@b"));
        assert_eq!(
            split_teammate_routing_id("Relay@harbor"),
            Some(("Relay", "harbor"))
        );
        assert_eq!(
            split_teammate_routing_id("P1-engine@region_two"),
            Some(("P1-engine", "region_two"))
        );
    }

    #[test]
    fn routing_form_grammar_rejects_every_other_at_target_shape() {
        // No other `@`-target carries an interior `@`, so none of these may route here.
        assert!(!is_teammate_routing_id("main"));
        assert!(!is_teammate_routing_id("trap:CrimsonWillowFen5180"));
        assert!(!is_teammate_routing_id(
            "00000000-0000-4000-8000-000000000001"
        ));
        assert!(!is_teammate_routing_id("13d9645a"));
        assert!(!is_teammate_routing_id("aRelay-0123456789abcdef"));
        assert!(!is_teammate_routing_id("-Users-dev-Projects-relay"));
        // Malformed: an empty half, two `@`s, or a character outside the name charset.
        assert!(!is_teammate_routing_id("@harbor"));
        assert!(!is_teammate_routing_id("Relay@"));
        assert!(!is_teammate_routing_id("Relay@har@bor"));
        assert!(!is_teammate_routing_id("Relay team@harbor"));
        assert!(!is_teammate_routing_id("Relay@har.bor"));
        assert!(split_teammate_routing_id("Relay").is_none());
    }
}
