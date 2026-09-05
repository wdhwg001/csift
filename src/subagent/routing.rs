//! Teammate routing form (`<Name>@<Team>`): the name+team index over subagent metas.

use super::*;

/// The teammate transcript ids under `session_jsonl` whose `meta.json` declares
/// `taskKind:"in_process_teammate"` with EXACTLY this `name` and `teamName`.
///
/// Both halves compare byte-exactly, case included: Claude Code interpolates the requested
/// name and the raw team name into the routing id and folds no case anywhere on that path, so
/// a case-differing token names a DIFFERENT teammate and must miss rather than match.
///
/// The result is a set, not a single id: the routing form can name more than one live
/// teammate (the spawn de-duplicates only the name-to-id registry key), so the caller decides
/// what an ambiguity means instead of this index silently picking one.
pub fn teammate_ids_by_routing(
    session_jsonl: &Path,
    name: &str,
    team: &str,
) -> Result<Vec<String>> {
    Ok(discover_subagents(session_jsonl)?
        .into_iter()
        .filter(|s| {
            s.kind == SubagentKind::Teammate
                && s.name.as_deref() == Some(name)
                && s.team_name.as_deref() == Some(team)
        })
        .map(|s| s.agent_id)
        .collect())
}

/// The routing id `<Name>@<Team>` for a teammate node, or `None` when the meta carried only
/// one half (a teammate without a `teamName` has no routing form to print - fabricating one
/// would name a teammate that cannot be addressed).
#[must_use]
pub fn routing_id(name: Option<&str>, team: Option<&str>) -> Option<String> {
    match (name, team) {
        (Some(n), Some(t)) if !n.is_empty() && !t.is_empty() => Some(format!("{n}@{t}")),
        _ => None,
    }
}
