//! Where each scope is READ from, and what the reader admits it cannot see.
//!
//! The fold rules live beside their own tests in [`super::settings`]; this file pins the
//! inventory: the platform directory the policy tier is composed from, the inputs listed as
//! unobservable, a plugin manifest that could not be parsed, and the local scope Claude Code
//! may canonicalise to the git root.

use super::*;

use crate::path::settings::{managed_settings_dir, merged_in, SCOPE_LOCAL_GIT, SCOPE_PLUGIN};

#[test]
fn the_managed_settings_directory_is_the_platform_literal() {
    // An absolute system path no test may create, so the string IS the checkable fact. Each
    // arm is pinned under its own cfg: a run on that platform checks its own literal, and a
    // rename on any of the three fails there rather than silently reading nothing.
    let dir = managed_settings_dir();
    #[cfg(target_os = "macos")]
    assert_eq!(
        dir,
        PathBuf::from("/Library/Application Support/ClaudeCode")
    );
    #[cfg(target_os = "windows")]
    assert_eq!(dir, PathBuf::from("C:\\Program Files\\ClaudeCode"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    assert_eq!(dir, PathBuf::from("/etc/claude-code"));
    assert!(
        dir.is_absolute(),
        "the managed tier is anchored, never relative to a cwd: {}",
        dir.display()
    );
}

#[test]
fn the_unobservable_list_names_every_input_that_leaves_no_trace() {
    // The list is the reader's own admission of what it cannot see, so it is pinned item by
    // item: dropping one turns a verdict that should say "unknown" into a false "no".
    let t = Tree::new();
    let m = merged_in(&t.dir("home"), None, &t.path("managed"));
    assert_eq!(m.unobservable.len(), 5, "{:?}", m.unobservable);
    for want in [
        // A settings file or inline JSON handed to Claude Code per invocation.
        "--settings",
        // A spawning parent's policy tier, held in memory.
        "--managed-settings",
        // The switches that can empty the user, project and local scopes outright.
        "--setting-sources / --restricted",
        // Before the trust dialog is accepted only some scopes apply in full.
        "the trust-dialog state",
        // A managed plist or a registry tree csift never reads.
        "MDM policy",
    ] {
        assert!(
            m.unobservable.iter().any(|u| u.contains(want)),
            "the unobservable list dropped `{want}`: {:?}",
            m.unobservable
        );
    }
}

#[test]
fn a_plugin_manifest_that_cannot_be_parsed_is_reported_with_its_reason() {
    // Plugin hooks bypass the settings merge and are unioned at read time, so a manifest that
    // fails to parse contributes nothing and would otherwise be indistinguishable from a
    // plugin that declares no hooks at all. The note is the only place it is visible.
    let t = Tree::new();
    let home = t.dir("home");
    t.write("home/plugins/relay/plugin.json", "{ not json at all");
    t.write(
        "home/plugins/relay/hooks/hooks.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"}]}]}}"#,
    );
    t.write(
        "home/settings.json",
        r#"{"env":{"CSIFT_REGION":"user-scope"}}"#,
    );

    let m = merged_in(&home, None, &t.path("managed"));
    let broken = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_PLUGIN && !s.read)
        .expect("the unreadable manifest is reported");
    assert_eq!(broken.path, t.path("home/plugins/relay/plugin.json"));
    assert!(
        broken
            .note
            .as_deref()
            .is_some_and(|n| n.contains("malformed JSON")),
        "the reason rides the report: {:?}",
        broken.note
    );
    // The plugin's OTHER manifest still folded, and so did the rest of the cascade: one
    // broken file never hides what the reader could read.
    assert_eq!(
        crate::path::settings::hooks_for_event(&m, "SessionStart").len(),
        1,
        "hooks.json is a separate source from plugin.json"
    );
    assert_eq!(
        crate::path::settings::env_value(&m, "CSIFT_REGION"),
        Some(("user-scope", crate::path::settings::SCOPE_USER))
    );
}

#[test]
fn the_git_root_local_file_is_read_even_when_the_project_root_has_none() {
    // Claude Code canonicalises the local scope to the git root only when a uid-ownership
    // probe passes, and that probe leaves nothing on disk. csift therefore reads BOTH paths;
    // with only the git-root file present it is the local scope, and the project-root path is
    // still reported so a reader can see which of the two answered.
    let t = Tree::new();
    let home = t.dir("home");
    t.dir("repo/.git");
    let proj = t.dir("repo/work");
    t.write(
        "repo/.claude/settings.local.json",
        r#"{"env":{"CSIFT_REGION":"git-root"}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    assert_eq!(
        crate::path::settings::env_value(&m, "CSIFT_REGION"),
        Some(("git-root", SCOPE_LOCAL_GIT)),
        "nothing deeper exists to override it"
    );
    let deeper = m
        .sources
        .iter()
        .find(|s| s.scope == crate::path::settings::SCOPE_LOCAL)
        .expect("the project-root local path is reported even when absent");
    assert!(!deeper.read);
    assert!(
        deeper.note.is_none(),
        "a missing file is normal and earns no note: {:?}",
        deeper.note
    );
    assert_eq!(deeper.path, t.path("repo/work/.claude/settings.local.json"));
}
