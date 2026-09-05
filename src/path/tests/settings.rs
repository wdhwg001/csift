//! The settings cascade: scope order, the two per-block merge rules, both local paths,
//! the composed policy tier, the plugin manifests, the three policy switches, and the
//! accessors the channel commands read.

use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::path::settings::{
    deliver_slots, env_value, hooks_for_event, merged_in, string_value_in, SCOPE_LOCAL,
    SCOPE_LOCAL_GIT, SCOPE_POLICY_DROPIN, SCOPE_POLICY_MANAGED, SCOPE_PROJECT, SCOPE_USER,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throwaway directory tree (no external dev-dep), removed on drop. Every fixture path
/// is generated, so nothing about this machine reaches a test literal.
#[derive(Debug)]
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Tree {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("csift-settings-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp tree");
        Tree { root }
    }

    fn write(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, body).expect("write fixture");
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(&path).expect("create dir");
        path
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One `SessionStart` command hook, as a settings `hooks` block.
fn session_start(command: &str) -> String {
    format!(
        r#"{{"hooks":{{"SessionStart":[{{"hooks":[{{"type":"command","command":"{command}"}}]}}]}}}}"#
    )
}

#[test]
fn settings_scopes_fold_in_claude_code_order_with_the_later_scope_winning() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write(
        "home/settings.json",
        r#"{"env":{"CSIFT_RELAY":"user"},"model":"beacon-user"}"#,
    );
    t.write(
        "proj/.claude/settings.json",
        r#"{"env":{"CSIFT_RELAY":"project"},"model":"beacon-project"}"#,
    );
    t.write(
        "proj/.claude/settings.local.json",
        r#"{"env":{"CSIFT_RELAY":"local"},"model":"beacon-local"}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    assert_eq!(env_value(&m, "CSIFT_RELAY"), Some(("local", SCOPE_LOCAL)));
    assert_eq!(
        string_value_in(&m, "model", &[SCOPE_USER, SCOPE_PROJECT, SCOPE_LOCAL]),
        Some(("beacon-local", SCOPE_LOCAL))
    );
    // A narrower cascade still answers with ITS last scope, not the widest one.
    assert_eq!(
        string_value_in(&m, "model", &[SCOPE_USER, SCOPE_PROJECT]),
        Some(("beacon-project", SCOPE_PROJECT))
    );
    let scopes: Vec<&str> = m.sources.iter().map(|s| s.scope).collect();
    assert!(
        scopes.starts_with(&[SCOPE_USER, SCOPE_PROJECT]),
        "read order is the merge order: {scopes:?}"
    );
    assert!(
        !m.unobservable.is_empty(),
        "the unobservable list is stated"
    );
}

#[test]
fn settings_env_merges_per_key_so_a_later_scope_keeps_the_keys_it_does_not_name() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write(
        "home/settings.json",
        r#"{"env":{"CSIFT_HARBOR":"user","CSIFT_REGION":"user"}}"#,
    );
    t.write(
        "proj/.claude/settings.json",
        r#"{"env":{"CSIFT_REGION":"project"}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    assert_eq!(env_value(&m, "CSIFT_HARBOR"), Some(("user", SCOPE_USER)));
    assert_eq!(
        env_value(&m, "CSIFT_REGION"),
        Some(("project", SCOPE_PROJECT))
    );
    assert_eq!(env_value(&m, "CSIFT_ABSENT"), None);
}

#[test]
fn settings_hooks_concatenate_across_scopes_and_dedupe_by_content() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write(
        "home/settings.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write(
        "proj/.claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"csift deliver --slot 1"},
            {"type":"command","command":"csift deliver --slot 2"}]}]}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    let entries = hooks_for_event(&m, "SessionStart");
    assert_eq!(entries.len(), 2, "concat, then one content dedupe");
    assert_eq!(entries[0].command, "csift deliver --slot 1");
    assert_eq!(entries[0].source, SCOPE_USER, "the first writer keeps it");
    assert_eq!(entries[1].command, "csift deliver --slot 2");
    assert_eq!(entries[1].source, SCOPE_PROJECT);
    assert!(m.policy_switch.is_none());
}

#[test]
fn settings_project_scope_adds_an_event_the_user_scope_never_names() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write(
        "home/settings.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write(
        "proj/.claude/settings.json",
        r#"{"hooks":{"Stop":[{"matcher":"*","hooks":[
            {"type":"command","command":"csift deliver --slot 1","timeout":30,"asyncRewake":true}]}]}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    assert_eq!(hooks_for_event(&m, "SessionStart").len(), 1);
    let stop = hooks_for_event(&m, "Stop");
    assert_eq!(stop.len(), 1, "the project scope adds, it does not replace");
    assert_eq!(stop[0].matcher.as_deref(), Some("*"));
    assert_eq!(stop[0].timeout, Some(30));
    assert!(stop[0].async_rewake && !stop[0].async_);
    assert_eq!(stop[0].source, SCOPE_PROJECT);
}

#[test]
fn settings_reads_both_local_paths_with_the_deeper_one_winning() {
    let t = Tree::new();
    let home = t.dir("home");
    t.dir("repo/.git");
    let proj = t.dir("repo/work");
    t.write(
        "repo/.claude/settings.local.json",
        r#"{"env":{"CSIFT_REGION":"git-root"},"plansDirectory":"root-plans"}"#,
    );
    t.write(
        "repo/work/.claude/settings.local.json",
        r#"{"env":{"CSIFT_REGION":"deeper"}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    assert_eq!(env_value(&m, "CSIFT_REGION"), Some(("deeper", SCOPE_LOCAL)));
    // Both paths are reported, and the git-root one really was read.
    let git_root = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_LOCAL_GIT)
        .expect("the git-root local path is reported");
    assert!(git_root.read, "the git-root local file was read");
    assert_eq!(git_root.path, t.path("repo/.claude/settings.local.json"));
    assert_eq!(
        string_value_in(&m, "plansDirectory", &[SCOPE_LOCAL_GIT]),
        Some(("root-plans", SCOPE_LOCAL_GIT))
    );
}

#[test]
fn settings_policy_dropins_fold_in_sorted_name_order_and_skip_dotfiles() {
    let t = Tree::new();
    let home = t.dir("home");
    let managed = t.dir("managed");
    t.write(
        "managed/managed-settings.json",
        r#"{"env":{"CSIFT_TIER":"base"}}"#,
    );
    t.write(
        "managed/managed-settings.d/20-beta.json",
        r#"{"env":{"CSIFT_TIER":"twenty"}}"#,
    );
    t.write(
        "managed/managed-settings.d/10-alpha.json",
        r#"{"env":{"CSIFT_TIER":"ten"}}"#,
    );
    t.write(
        "managed/managed-settings.d/.hidden.json",
        r#"{"env":{"CSIFT_TIER":"hidden"}}"#,
    );
    t.write("managed/managed-settings.d/notes.txt", "ignored");

    let m = merged_in(&home, None, &managed);
    assert_eq!(
        env_value(&m, "CSIFT_TIER"),
        Some(("twenty", SCOPE_POLICY_DROPIN)),
        "the last drop-in in name order wins"
    );
    let dropins: Vec<String> = m
        .sources
        .iter()
        .filter(|s| s.scope == SCOPE_POLICY_DROPIN)
        .filter_map(|s| s.path.file_name()?.to_str().map(str::to_string))
        .collect();
    assert_eq!(dropins, vec!["10-alpha.json", "20-beta.json"]);
}

#[test]
fn settings_server_managed_cache_is_the_whole_policy_tier() {
    let t = Tree::new();
    let home = t.dir("home");
    let managed = t.dir("managed");
    t.write(
        "home/remote-settings.json",
        r#"{"env":{"CSIFT_TIER":"remote"}}"#,
    );
    t.write(
        "managed/managed-settings.json",
        r#"{"env":{"CSIFT_TIER":"file"}}"#,
    );

    let m = merged_in(&home, None, &managed);
    assert_eq!(
        env_value(&m, "CSIFT_TIER"),
        Some(("remote", "policy-remote"))
    );
    let managed_row = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_POLICY_MANAGED)
        .expect("the out-composed managed file is still reported");
    assert!(!managed_row.read);
    assert!(
        managed_row
            .note
            .as_deref()
            .is_some_and(|n| n.contains("highest present policy source")),
        "the first-wins rule is stated, not left looking like a missing file"
    );
}

#[test]
fn settings_plugin_manifests_union_into_the_hook_set_below_the_user_scope() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write(
        "home/plugins/repos/relay-pack/hooks/hooks.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write(
        "home/plugins/repos/relay-pack/plugin.json",
        r#"{"name":"relay","hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo harbor"}]}]}}"#,
    );
    t.write("home/settings.json", &session_start("echo region"));

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    let entries = hooks_for_event(&m, "SessionStart");
    let sources: Vec<&str> = entries.iter().map(|h| h.source.as_str()).collect();
    assert_eq!(
        sources,
        vec!["plugin:relay", "plugin:relay", SCOPE_USER],
        "both plugin manifests union in, below the user scope"
    );
    assert_eq!(deliver_slots(&m, "SessionStart"), vec![1]);
}

#[test]
fn settings_policy_disable_all_hooks_empties_the_hook_set() {
    let t = Tree::new();
    let home = t.dir("home");
    let managed = t.dir("managed");
    t.write(
        "home/settings.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write(
        "managed/managed-settings.json",
        r#"{"disableAllHooks":true,"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"csift deliver --slot 9"}]}]}}"#,
    );

    let m = merged_in(&home, None, &managed);
    assert!(m.hooks.is_empty(), "not even the policy hooks survive");
    assert_eq!(
        m.policy_switch,
        Some("policy disableAllHooks: no hooks run")
    );
    assert!(deliver_slots(&m, "SessionStart").is_empty());
}

#[test]
fn settings_allow_managed_hooks_only_keeps_the_policy_hooks_alone() {
    let t = Tree::new();
    let home = t.dir("home");
    let managed = t.dir("managed");
    t.write(
        "home/settings.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write(
        "home/plugins/relay-pack/hooks/hooks.json",
        &session_start("csift deliver --slot 2"),
    );
    t.write(
        "managed/managed-settings.json",
        r#"{"allowManagedHooksOnly":true,"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"csift deliver --slot 9"}]}]}}"#,
    );

    let m = merged_in(&home, None, &managed);
    let entries = hooks_for_event(&m, "SessionStart");
    assert_eq!(
        entries.len(),
        1,
        "the plugin and user hooks are dropped too"
    );
    assert_eq!(entries[0].source, SCOPE_POLICY_MANAGED);
    assert_eq!(deliver_slots(&m, "SessionStart"), vec![9]);
    assert_eq!(
        m.policy_switch,
        Some("policy allowManagedHooksOnly: policy hooks only")
    );
}

#[test]
fn settings_disable_all_hooks_outside_policy_keeps_the_policy_hooks() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    let managed = t.dir("managed");
    t.write(
        "home/settings.json",
        &session_start("csift deliver --slot 1"),
    );
    t.write("proj/.claude/settings.json", r#"{"disableAllHooks":true}"#);
    t.write(
        "managed/managed-settings.json",
        r#"{"hooks":{"Stop":[{"hooks":[
            {"type":"command","command":"csift deliver --slot 4"}]}]}}"#,
    );

    let m = merged_in(&home, Some(&proj), &managed);
    assert!(
        hooks_for_event(&m, "SessionStart").is_empty(),
        "the user hook is dropped"
    );
    assert_eq!(deliver_slots(&m, "Stop"), vec![4], "the policy hook stays");
    assert_eq!(
        m.policy_switch,
        Some("settings disableAllHooks: policy hooks only")
    );
}

#[test]
fn settings_malformed_file_becomes_a_note_and_never_stops_the_fold() {
    let t = Tree::new();
    let home = t.dir("home");
    let proj = t.dir("proj");
    t.write("home/settings.json", "{not json at all");
    t.write(
        "proj/.claude/settings.json",
        r#"["an array, not an object"]"#,
    );
    t.write(
        "proj/.claude/settings.local.json",
        r#"{"env":{"CSIFT_RELAY":"local"}}"#,
    );

    let m = merged_in(&home, Some(&proj), &t.path("managed"));
    let user = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_USER)
        .expect("the user file is reported");
    assert!(!user.read);
    assert!(
        user.note
            .as_deref()
            .is_some_and(|n| n.contains("malformed")),
        "the reason is on the report: {:?}",
        user.note
    );
    let project = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_PROJECT)
        .expect("the project file is reported");
    assert_eq!(project.note.as_deref(), Some("not a JSON object"));
    assert_eq!(
        env_value(&m, "CSIFT_RELAY"),
        Some(("local", SCOPE_LOCAL)),
        "one broken file never hides the rest of the cascade"
    );
}

#[test]
fn settings_deliver_slots_reads_the_slot_number_off_the_command() {
    let t = Tree::new();
    let home = t.dir("home");
    t.write(
        "home/settings.json",
        r#"{"hooks":{"SessionStart":[
            {"hooks":[
                {"type":"command","command":"csift deliver --slot 3"},
                {"type":"command","command":"/usr/local/bin/csift deliver --slot 2"},
                {"type":"command","command":"csift  deliver   --slot  1"},
                {"type":"command","command":"csift deliver --slot 12x"},
                {"type":"command","command":"csift status"}]},
            {"matcher":"resume","hooks":[
                {"type":"command","command":"csift deliver --slot 1"}]}],
          "Stop":[{"hooks":[{"type":"command","command":"echo region"}]}]}}"#,
    );

    let m = merged_in(&home, None, &t.path("managed"));
    assert_eq!(
        deliver_slots(&m, "SessionStart"),
        vec![1, 2, 3],
        "sorted, deduplicated across matchers, and a malformed slot is not a slot"
    );
    assert!(deliver_slots(&m, "Stop").is_empty());
    assert!(deliver_slots(&m, "PreToolUse").is_empty());
}

#[test]
fn settings_env_value_names_the_scope_that_set_the_key() {
    let t = Tree::new();
    let home = t.dir("home");
    let managed = t.dir("managed");
    t.write(
        "home/settings.json",
        r#"{"env":{"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1"}}"#,
    );
    t.write(
        "managed/managed-settings.json",
        r#"{"env":{"CLAUDE_CODE_HARBOR_KITE":"0"}}"#,
    );

    let m = merged_in(&home, None, &managed);
    assert_eq!(
        env_value(&m, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some(("1", SCOPE_USER))
    );
    assert_eq!(
        env_value(&m, "CLAUDE_CODE_HARBOR_KITE"),
        Some(("0", SCOPE_POLICY_MANAGED))
    );
}
