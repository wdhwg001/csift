//! Canonical id derivation from on-disk paths: bare hex, parent session, sidecar dirs.

use super::*;

// ── Branch-completeness ──

#[test]
fn sidecar_dir_none_for_pathological_paths() {
    // A path with no file stem (root) → the `file_stem()?` None arm.
    assert!(sidecar_dir_for_session(Path::new("/")).is_none());
    // A bare relative filename has no parent dir component that is a real dir, and
    // the sidecar dir won't exist → None.
    assert!(sidecar_dir_for_session(Path::new("nonexistent.jsonl")).is_none());
}

#[test]
fn bare_agent_id_strips_prefix_only_when_present() {
    // The one rule, shared by recover/session/files: a subagent stem loses `agent-`;
    // a top-level uuid (no prefix) is unchanged.
    assert_eq!(bare_agent_id("agent-aaa111"), "aaa111");
    assert_eq!(
        bare_agent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"),
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    );
}

#[test]
fn session_id_from_path_is_canonical_bare_hex() {
    // The SINGLE per-file id derivation every surface (list/search/files/recover/
    // turns) now routes through, so the same transcript reports an IDENTICAL id
    // whichever subcommand prints it. A subagent stem loses its `agent-` prefix; a
    // top-level uuid passes through; a stem-less path yields an empty string.
    assert_eq!(
        session_id_from_path(Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl"
        )),
        "a585e25a580c59e7a"
    );
    assert_eq!(
        session_id_from_path(Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl"
        )),
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    );
    // A root path with no file stem → empty (never panics).
    assert_eq!(session_id_from_path(Path::new("/")), "");
}

#[test]
fn parent_session_id_and_is_subagent_from_path() {
    let sub = Path::new(
        "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl",
    );
    let wf = Path::new(
        "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/workflows/wf_abc/agent-aaa.jsonl",
    );
    let top = Path::new("/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl");
    // A subagent path → parent is the dir before `subagents`, and is_subagent is true.
    assert_eq!(
        parent_session_id_from_path(sub).as_deref(),
        Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
    );
    assert!(is_subagent_path(sub));
    // A workflow subagent path → same parent (the segment before `subagents`).
    assert_eq!(
        parent_session_id_from_path(wf).as_deref(),
        Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
    );
    assert!(is_subagent_path(wf));
    // A top-level path → no parent (it IS its own session), is_subagent false.
    assert_eq!(parent_session_id_from_path(top), None);
    assert!(!is_subagent_path(top));
}
