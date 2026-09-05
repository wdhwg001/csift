//! Claude Code's settings cascade, read from disk as far as it is observable.
//!
//! Claude Code folds five named scopes in one fixed order, later winning:
//! `userSettings < projectSettings < localSettings < flagSettings < policySettings`, with
//! a plugin `settings` base below the user scope. csift reads the FILE scopes (`user`,
//! `project`, both `local` paths and the composed policy tier) plus the plugin hook
//! manifests; the flag scope and MDM policy are per-invocation or out-of-tree and are
//! reported as unobservable rather than guessed at.
//!
//! Two block rules differ from the scalar "later scope wins": `env` merges PER KEY (a
//! later scope overrides only the keys it names) and `hooks` CONCATENATES per event (a
//! project scope ADDS to the user's hooks). Three policy switches run after the fold and
//! can empty or replace the whole hook set. Every read is tolerant: an unreadable or
//! malformed file becomes a note on its [`SourceReport`], never an error, so one broken
//! file never hides the rest of the cascade.

use super::*;

use std::collections::BTreeMap;

use serde_json::{Map, Value};

#[allow(unused_imports)] // the channel commands are the consumers
pub(crate) use super::settings_slots::deliver_slots;

/// The user scope: `<claude-home>/settings.json`.
pub(crate) const SCOPE_USER: &str = "user";
/// The project scope: `<project-root>/.claude/settings.json`.
pub(crate) const SCOPE_PROJECT: &str = "project";
/// The local scope at the project root: `<project-root>/.claude/settings.local.json`.
pub(crate) const SCOPE_LOCAL: &str = "local";
/// The local scope at the git root above the project root. Claude Code canonicalises the
/// local scope to the git root only when a uid-ownership probe passes, and that probe is
/// not observable from disk, so csift reads BOTH paths and lets the deeper one win.
pub(crate) const SCOPE_LOCAL_GIT: &str = "local-git-root";
/// The server-managed policy cache: `<claude-home>/remote-settings.json`.
pub(crate) const SCOPE_POLICY_REMOTE: &str = "policy-remote";
/// The managed-settings file in the platform's managed directory.
pub(crate) const SCOPE_POLICY_MANAGED: &str = "policy-managed";
/// A drop-in under `<managed-dir>/managed-settings.d/`.
pub(crate) const SCOPE_POLICY_DROPIN: &str = "policy-managed.d";
/// A plugin hook manifest. Plugin SETTINGS VALUES are not modeled; the hooks are.
pub(crate) const SCOPE_PLUGIN: &str = "plugin";

/// The three policy switches, in the order they are evaluated: policy `disableAllHooks`
/// leaves nothing, and either policy `allowManagedHooksOnly` or a `disableAllHooks` set
/// outside the policy tier leaves the policy hooks alone.
const SWITCH_DISABLE_ALL: &str = "policy disableAllHooks: no hooks run";
const SWITCH_MANAGED_ONLY: &str = "policy allowManagedHooksOnly: policy hooks only";
const SWITCH_SETTINGS_DISABLE: &str = "settings disableAllHooks: policy hooks only";

/// Why a managed file did not contribute while the server-managed cache is present: the
/// composed policy tier is first-wins, so the highest PRESENT source is the whole tier.
const POLICY_FIRST_WINS: &str =
    "not composed: the server-managed cache is the highest present policy source";

/// Inputs that decide the effective settings and leave NO trace a reader can find, listed
/// so a verdict says "unknown" instead of implying the file scopes are all there is.
const UNOBSERVABLE: [&str; 5] = [
    "--settings (a file path or inline JSON, materialised per invocation)",
    "--managed-settings (a spawning parent's policy tier, in memory only)",
    "--setting-sources / --restricted (can empty the user, project and local scopes)",
    "the trust-dialog state (before acceptance only user, flag and policy env applies in full)",
    "MDM policy (a managed plist or a registry tree), which csift never reads",
];

/// How deep under `<claude-home>/plugins` the manifest walk descends, and how many
/// directories it may visit before it stops and says so.
const PLUGIN_WALK_DEPTH: usize = 6;
const PLUGIN_WALK_DIRS: usize = 512;

/// One settings file the reader tried to open, and what came of it.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields are the reported surface; the channel commands read them
pub(crate) struct SourceReport {
    /// The scope label (one of the `SCOPE_*` constants) and the path it was read from.
    pub scope: &'static str,
    pub path: PathBuf,
    /// True when the file parsed into a JSON object and contributed to the fold.
    pub read: bool,
    /// Why it did not contribute: a missing file carries `None`, a broken one the reason.
    pub note: Option<String>,
}

/// One command hook, flattened out of its matcher group. Only `type:"command"` entries
/// are modeled: a plugin's JS module hook carries no command line and nothing in csift
/// can predict what it does.
#[derive(Debug, Clone)]
#[allow(dead_code)] // the fields are read by the channel commands and the unit tests
pub(crate) struct HookEntry {
    /// The event name the entry is registered under (`SessionStart`, `Stop`, ...).
    pub event: String,
    /// The matcher of the group this entry came from, when the group named one.
    pub matcher: Option<String>,
    /// The command line, verbatim, then the entry's `timeout` (seconds), `async` and
    /// `asyncRewake` as it set them.
    pub command: String,
    pub timeout: Option<u64>,
    pub async_: bool,
    pub async_rewake: bool,
    /// The scope label it came from, or `plugin:<name>` for a plugin manifest.
    pub source: String,
}

/// The folded cascade.
#[derive(Debug, Clone)]
#[allow(dead_code)] // `sources`/`unobservable`/`policy_switch` are read by the reporters
pub(crate) struct Merged {
    /// Every file the reader tried, in read order.
    pub sources: Vec<SourceReport>,
    /// Inputs that change the outcome and leave no trace ([`UNOBSERVABLE`]).
    pub unobservable: Vec<&'static str>,
    /// The merged `env` block (per key, later scope winning) and, per key, the scope that
    /// set it, so a verdict can name a file instead of asserting.
    pub env: BTreeMap<String, String>,
    pub env_scope: BTreeMap<String, &'static str>,
    /// The concatenated hooks keyed by event, in fold order, and the policy switch that
    /// rewrote that set when one fired.
    pub hooks: BTreeMap<String, Vec<HookEntry>>,
    pub policy_switch: Option<&'static str>,
    /// Every OTHER top-level key that carried a string, with each scope that set it in
    /// fold order: a caller may need a NARROWER cascade than the full one.
    pub strings: BTreeMap<String, Vec<(&'static str, String)>>,
}

/// The whole cascade for one project root (its first recorded cwd); with `None` only the
/// home-anchored scopes are read.
pub(crate) fn merged(claude_home: &Path, project_root: Option<&Path>) -> Merged {
    merged_in(claude_home, project_root, &managed_settings_dir())
}

/// [`merged`] with the managed-settings directory supplied explicitly: the platform
/// directory is an absolute system path a test cannot write to.
pub(crate) fn merged_in(
    claude_home: &Path,
    project_root: Option<&Path>,
    managed_dir: &Path,
) -> Merged {
    let mut fold = Fold::new();
    // The plugin base sits BELOW the user scope, so it folds first.
    read_plugin_hooks(claude_home, &mut fold);
    fold.scope(SCOPE_USER, claude_home.join("settings.json"));
    if let Some(root) = project_root {
        fold.scope(SCOPE_PROJECT, root.join(".claude").join("settings.json"));
        // The git-root local file folds BEFORE the project-root one so the deeper path
        // wins the keys both set.
        if let Some(git) = git_root_above(root) {
            fold.scope(
                SCOPE_LOCAL_GIT,
                git.join(".claude").join("settings.local.json"),
            );
        }
        fold.scope(
            SCOPE_LOCAL,
            root.join(".claude").join("settings.local.json"),
        );
    }
    read_policy_tier(claude_home, managed_dir, &mut fold);
    fold.finish()
}

/// The hooks registered for one event, in fold order.
#[allow(dead_code)] // the channel commands are the consumers
pub(crate) fn hooks_for_event<'a>(m: &'a Merged, event: &str) -> Vec<&'a HookEntry> {
    m.hooks
        .get(event)
        .map(|v| v.iter().collect())
        .unwrap_or_default()
}

/// The winning value of one `env` key and the scope that set it.
#[allow(dead_code)] // the channel commands are the consumers
pub(crate) fn env_value<'a>(m: &'a Merged, key: &str) -> Option<(&'a str, &'a str)> {
    let value = m.env.get(key)?;
    let scope = m.env_scope.get(key).copied().unwrap_or("unknown");
    Some((value.as_str(), scope))
}

/// The winning string value of one top-level key, restricted to `scopes` and taking the
/// LAST of them in fold order: a caller may mirror a narrower cascade than the full one.
pub(crate) fn string_value_in<'a>(
    m: &'a Merged,
    key: &str,
    scopes: &[&str],
) -> Option<(&'a str, &'a str)> {
    m.strings
        .get(key)?
        .iter()
        .rev()
        .find(|(scope, _)| scopes.contains(scope))
        .map(|(scope, value)| (value.as_str(), *scope))
}

/// The platform's managed-settings directory.
fn managed_settings_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    let dir = "/Library/Application Support/ClaudeCode";
    #[cfg(target_os = "windows")]
    let dir = "C:\\Program Files\\ClaudeCode";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let dir = "/etc/claude-code";
    PathBuf::from(dir)
}

/// The nearest git root STRICTLY above `root`, by a `.git` entry (a directory in a normal
/// checkout, a file in a linked worktree).
fn git_root_above(root: &Path) -> Option<PathBuf> {
    let mut cur = root.parent()?;
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// What one settings file turned out to be. `Missing` is normal and earns no note.
#[derive(Debug)]
enum FileRead {
    Missing,
    Bad(String),
    Ok(Map<String, Value>),
}

fn read_settings_file(path: &Path) -> FileRead {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return FileRead::Missing,
        Err(e) => return FileRead::Bad(format!("unreadable: {e}")),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(obj)) => FileRead::Ok(obj),
        Ok(_) => FileRead::Bad("not a JSON object".to_string()),
        Err(e) => FileRead::Bad(format!("malformed JSON: {e}")),
    }
}

/// The composed policy tier. It is first-wins across its sources, so when the
/// server-managed cache is present it IS the tier; otherwise the managed file and its
/// sorted drop-ins fold in that order. MDM sits between the two and is unobservable.
fn read_policy_tier(claude_home: &Path, managed_dir: &Path, fold: &mut Fold) {
    let remote = claude_home.join("remote-settings.json");
    match read_settings_file(&remote) {
        FileRead::Ok(obj) => {
            fold.report(SCOPE_POLICY_REMOTE, remote, true, None);
            fold.fold(SCOPE_POLICY_REMOTE, &obj);
            report_uncomposed_managed(managed_dir, fold);
            return;
        }
        FileRead::Missing => fold.report(SCOPE_POLICY_REMOTE, remote, false, None),
        FileRead::Bad(note) => fold.report(SCOPE_POLICY_REMOTE, remote, false, Some(note)),
    }
    fold.scope(
        SCOPE_POLICY_MANAGED,
        managed_dir.join("managed-settings.json"),
    );
    for path in dropins(managed_dir) {
        fold.scope(SCOPE_POLICY_DROPIN, path);
    }
}

/// Report the managed files that exist but did not compose, so the first-wins rule is
/// visible instead of looking like a missing file.
fn report_uncomposed_managed(managed_dir: &Path, fold: &mut Fold) {
    let mut paths = vec![managed_dir.join("managed-settings.json")];
    paths.extend(dropins(managed_dir));
    for path in paths {
        if path.exists() {
            fold.report(
                SCOPE_POLICY_MANAGED,
                path,
                false,
                Some(POLICY_FIRST_WINS.to_string()),
            );
        }
    }
}

/// Every `managed-settings.d/*.json` whose name does not start with a dot, sorted by
/// name: the order Claude Code reads them in, and therefore the order they win in.
fn dropins(managed_dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(managed_dir.join("managed-settings.d")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|n| n.ends_with(".json") && !n.starts_with('.'))
        })
        .collect();
    out.sort();
    out
}

/// Plugin hook manifests: `<claude-home>/plugins/**/hooks/hooks.json` plus each plugin's
/// own `plugin.json` `hooks` field. They bypass the settings merge in Claude Code and are
/// unioned at read time, so they fold as one source labelled `plugin:<name>`.
fn read_plugin_hooks(claude_home: &Path, fold: &mut Fold) {
    let root = claude_home.join("plugins");
    if !root.is_dir() {
        return;
    }
    let mut budget = PLUGIN_WALK_DIRS;
    walk_plugins(&root, PLUGIN_WALK_DEPTH, &mut budget, fold);
    if budget == 0 {
        fold.report(
            SCOPE_PLUGIN,
            root,
            false,
            Some(format!(
                "stopped after {PLUGIN_WALK_DIRS} directories; deeper manifests were not read"
            )),
        );
    }
}

fn walk_plugins(dir: &Path, depth: usize, budget: &mut usize, fold: &mut Fold) {
    if depth == 0 || *budget == 0 {
        return;
    }
    *budget -= 1;
    if dir.join("plugin.json").is_file() || dir.join("hooks").join("hooks.json").is_file() {
        read_one_plugin(dir, fold);
        return; // a plugin does not contain another plugin
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && !p
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|n| n.starts_with('.'))
        })
        .collect();
    subs.sort();
    for sub in subs {
        walk_plugins(&sub, depth - 1, budget, fold);
    }
}

fn read_one_plugin(dir: &Path, fold: &mut Fold) {
    let manifest = dir.join("plugin.json");
    let name = plugin_name(dir, &manifest);
    let source = format!("plugin:{name}");
    // plugin.json's hooks are declared additional to hooks/hooks.json, which folds first.
    fold.hooks_only(dir.join("hooks").join("hooks.json"), &source);
    fold.hooks_only(manifest, &source);
}

fn plugin_name(dir: &Path, manifest: &Path) -> String {
    if let FileRead::Ok(obj) = read_settings_file(manifest) {
        if let Some(name) = obj.get("name").and_then(Value::as_str) {
            if !name.trim().is_empty() {
                return name.to_string();
            }
        }
    }
    dir.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("plugin")
        .to_string()
}

fn is_policy_scope(scope: &str) -> bool {
    scope.starts_with("policy-")
}

/// A settings value csift can carry as text. Claude Code's schema types `env` as a string
/// map, but a hand-edited file can carry a number or a bool, and dropping one silently
/// would report a key as unset that IS set.
fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

/// The fold accumulator. The three switch inputs live here, not on [`Merged`], because
/// they are consumed at the end and are not part of the reported surface.
#[derive(Debug)]
struct Fold {
    m: Merged,
    disable_all_hooks: bool,
    policy_disable_all_hooks: bool,
    policy_allow_managed_only: bool,
}

impl Fold {
    fn new() -> Self {
        Fold {
            m: Merged {
                sources: Vec::new(),
                unobservable: UNOBSERVABLE.to_vec(),
                env: BTreeMap::new(),
                env_scope: BTreeMap::new(),
                hooks: BTreeMap::new(),
                policy_switch: None,
                strings: BTreeMap::new(),
            },
            disable_all_hooks: false,
            policy_disable_all_hooks: false,
            policy_allow_managed_only: false,
        }
    }

    fn report(&mut self, scope: &'static str, path: PathBuf, read: bool, note: Option<String>) {
        self.m.sources.push(SourceReport {
            scope,
            path,
            read,
            note,
        });
    }

    /// Read one settings file and fold it as `scope`.
    fn scope(&mut self, scope: &'static str, path: PathBuf) {
        match read_settings_file(&path) {
            FileRead::Missing => self.report(scope, path, false, None),
            FileRead::Bad(note) => self.report(scope, path, false, Some(note)),
            FileRead::Ok(obj) => {
                self.report(scope, path, true, None);
                self.fold(scope, &obj);
            }
        }
    }

    /// Read one file for its `hooks` block only (a plugin manifest: values are not modeled).
    fn hooks_only(&mut self, path: PathBuf, source: &str) {
        match read_settings_file(&path) {
            FileRead::Missing => {}
            FileRead::Bad(note) => self.report(SCOPE_PLUGIN, path, false, Some(note)),
            FileRead::Ok(obj) => {
                self.report(SCOPE_PLUGIN, path, true, None);
                if let Some(Value::Object(hooks)) = obj.get("hooks") {
                    self.fold_hooks(source, hooks);
                }
            }
        }
    }

    fn fold(&mut self, scope: &'static str, obj: &Map<String, Value>) {
        if let Some(Value::Object(env)) = obj.get("env") {
            self.fold_env(scope, env);
        }
        if let Some(Value::Object(hooks)) = obj.get("hooks") {
            self.fold_hooks(scope, hooks);
        }
        self.fold_strings(scope, obj);
        self.fold_switches(scope, obj);
    }

    /// `env` merges PER KEY: a later scope overrides only the keys it names.
    fn fold_env(&mut self, scope: &'static str, env: &Map<String, Value>) {
        for (key, value) in env {
            if let Some(text) = scalar_string(value) {
                self.m.env.insert(key.clone(), text);
                self.m.env_scope.insert(key.clone(), scope);
            }
        }
    }

    /// `hooks` CONCATENATES per event. The event's value is an array of matcher groups,
    /// each with its own `hooks` array; a bare entry in the event array is tolerated.
    fn fold_hooks(&mut self, source: &str, hooks: &Map<String, Value>) {
        for (event, groups) in hooks {
            let Some(list) = groups.as_array() else {
                continue;
            };
            for group in list {
                let matcher = group
                    .get("matcher")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                match group.get("hooks").and_then(Value::as_array) {
                    Some(entries) => {
                        for entry in entries {
                            self.push_hook(event, matcher.clone(), entry, source);
                        }
                    }
                    None => self.push_hook(event, matcher, group, source),
                }
            }
        }
    }

    fn push_hook(&mut self, event: &str, matcher: Option<String>, entry: &Value, source: &str) {
        let Some(command) = entry.get("command").and_then(Value::as_str) else {
            return;
        };
        let entry = HookEntry {
            event: event.to_string(),
            matcher,
            command: command.to_string(),
            timeout: entry.get("timeout").and_then(Value::as_u64),
            async_: entry.get("async").and_then(Value::as_bool).unwrap_or(false),
            async_rewake: entry
                .get("asyncRewake")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            source: source.to_string(),
        };
        let bucket = self.m.hooks.entry(event.to_string()).or_default();
        // Claude Code dedupes the concatenated arrays by object IDENTITY, which two parsed
        // objects never share, so it would run one command twice. csift dedupes by CONTENT:
        // one command text is one delivery point, and counting it twice over-reports slots.
        if bucket.iter().any(|h| same_hook(h, &entry)) {
            return;
        }
        bucket.push(entry);
    }

    /// Every other top-level string key, kept per scope in fold order.
    fn fold_strings(&mut self, scope: &'static str, obj: &Map<String, Value>) {
        for (key, value) in obj {
            if key == "env" || key == "hooks" {
                continue;
            }
            if let Value::String(text) = value {
                self.m
                    .strings
                    .entry(key.clone())
                    .or_default()
                    .push((scope, text.clone()));
            }
        }
    }

    fn fold_switches(&mut self, scope: &'static str, obj: &Map<String, Value>) {
        let disable = obj.get("disableAllHooks").and_then(Value::as_bool);
        if let Some(flag) = disable {
            self.disable_all_hooks = flag;
        }
        if !is_policy_scope(scope) {
            return;
        }
        if disable == Some(true) {
            self.policy_disable_all_hooks = true;
        }
        if obj.get("allowManagedHooksOnly").and_then(Value::as_bool) == Some(true) {
            self.policy_allow_managed_only = true;
        }
    }

    /// The three policy switches run after the fold, so they reach every source.
    fn finish(mut self) -> Merged {
        if self.policy_disable_all_hooks {
            self.m.hooks.clear();
            self.m.policy_switch = Some(SWITCH_DISABLE_ALL);
        } else if self.policy_allow_managed_only || self.disable_all_hooks {
            for entries in self.m.hooks.values_mut() {
                entries.retain(|h| is_policy_scope(&h.source));
            }
            self.m.hooks.retain(|_, entries| !entries.is_empty());
            self.m.policy_switch = Some(if self.policy_allow_managed_only {
                SWITCH_MANAGED_ONLY
            } else {
                SWITCH_SETTINGS_DISABLE
            });
        }
        self.m
    }
}

fn same_hook(a: &HookEntry, b: &HookEntry) -> bool {
    a.matcher == b.matcher
        && a.command == b.command
        && a.timeout == b.timeout
        && a.async_ == b.async_
        && a.async_rewake == b.async_rewake
}
