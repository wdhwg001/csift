//! Session/agent id shape predicates (uuid, bare hex, teammate, prefixes).

/// True for a canonical `8-4-4-4-12` hex UUID (a top-level session jsonl basename). Re-exports
/// the ONE canonical uuid-shape validator so the `turns` TESTS discriminate a top-level uuid
/// from a bare-hex subagent id without rolling their own (the only remaining caller now that
/// `--session`/bare-uuid routing is gone, hence `#[cfg(test)]` — production code reaches this
/// shape only through `pins_single_session`).
#[cfg(test)]
#[must_use]
pub fn is_session_uuid(s: &str) -> bool {
    is_uuid(s)
}

/// True for a canonical `8-4-4-4-12` hex UUID (the top-level session jsonl basename).
pub(crate) fn is_uuid(s: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != groups.len() {
        return false;
    }
    parts
        .iter()
        .zip(groups)
        .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// True for a bare-hex SUBAGENT id: a dash-less run of hex digits long enough to be an
/// agent id (≥12), never a short word. (`agents` prints these; `bare_agent_id` produces
/// them.) Used to GUIDE the error (subagent transcripts are not top-level jsonl basenames)
/// and, via [`is_subagent_id`], to route `@<agent-id>` targets.
pub fn is_bare_subagent_hex(s: &str) -> bool {
    s.len() >= 12 && !s.contains('-') && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for the NEW "teammate" subagent id shape (`taskKind:"in_process_teammate"`): the
/// canonical agent id EMBEDS the teammate name, e.g. `aVSRepro-68a2a1661c9390c1` — a leading
/// `a`, the name, then `-<hex≥12>`. Unlike a built-in/workflow agent (a bare hex run) it
/// carries a dash + uppercase, so [`is_bare_subagent_hex`] rejects it. The NAME itself may
/// carry dashes (a real `aP1-engine-9cf2f06d6235ca64` was minted from the teammate name
/// `P1-engine`), so the head accepts `[A-Za-z0-9-]`; the explicit `!is_uuid` guard keeps an
/// `a`-led session uuid out — its final segment is exactly 12 hex, which the dash-tolerant
/// head would otherwise admit. An encoded project dir can never collide (those start with
/// `-`, the leading-slash sanitisation). Recognised here so the id `csift agents` prints
/// round-trips back as an `@<agent-id>` target.
pub(crate) fn is_teammate_agent_id(s: &str) -> bool {
    if is_uuid(s) {
        return false;
    }
    let Some((head, tail)) = s.rsplit_once('-') else {
        return false;
    };
    head.len() >= 2
        && head.starts_with('a')
        && head.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && tail.len() >= 12
        && tail.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for ANY canonical subagent id `csift agents` emits — a bare hex run
/// ([`is_bare_subagent_hex`], built-in/workflow) OR a name-embedded teammate id
/// ([`is_teammate_agent_id`]). This is the single gate the `@<agent-id>` grammar branch
/// (`resolve_session_files`), the pinned-target existence guard, and `pins_single_session`
/// key on, so EVERY emitted agent_id round-trips regardless of shape (downstream resolution
/// already matches a subagent by exact id).
pub fn is_subagent_id(s: &str) -> bool {
    is_bare_subagent_hex(s) || is_teammate_agent_id(s)
}

/// True for a session-uuid PREFIX in either emitted form:
/// - the short dash-less run (`@13d9645a`, 4..=11 hex) — long enough to be near-collision-free
///   (a uuid's first segment is 8 hex = 4 billion), short enough that it is unambiguously
///   NEITHER a full uuid (32 hex + dashes) NOR an agent hex (≥12);
/// - a longer LITERAL prefix of the canonical `8-4-4-4-12` layout (`@13d9645a-3a5b`, 12..=35
///   chars, a dash exactly at each template position) — the collision-lengthened header token
///   `search` emits when two in-scope transcript ids share their first 8 chars.
///
/// The caller prefix-matches either form against the enumerated ids and errors if it is not
/// unique. (A dash-less run of ≥12 hex is claimed as an agent id BEFORE this predicate runs.)
pub(crate) fn is_uuid_prefix(s: &str) -> bool {
    if (4..=11).contains(&s.len()) {
        return s.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if (12..36).contains(&s.len()) {
        return s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    }
    false
}
