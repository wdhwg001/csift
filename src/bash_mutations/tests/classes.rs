//! Mutating-class markers: formatter/pkg/extract dispatch, dry-run suppression,
//! explicit-operand promotion, and the marker resolution contract.

use super::*;

#[test]
fn formatter_markers_and_dry_runs() {
    assert_eq!(just_paths("cargo fmt"), ["fmt:cargo"]);
    assert_eq!(just_paths("cargo fmt --all"), ["fmt:cargo"]);
    assert!(just_paths("cargo fmt --check").is_empty());
    assert!(just_paths("cargo build --release").is_empty());
    assert_eq!(just_paths("dprint fmt"), ["fmt:dprint"]);
    // No `--write` / `--fix` / `-w` / `-i` means a check run: nothing is written.
    assert!(just_paths("prettier src/a.ts").is_empty());
    assert!(just_paths("eslint src/").is_empty());
    assert!(just_paths("gofmt main.go").is_empty());
    assert!(just_paths("black --check .").is_empty());
    assert!(just_paths("ruff format --check").is_empty());
}

#[test]
fn formatter_with_named_operands_emits_real_fmt_rows() {
    assert_eq!(
        paths("npx prettier --write src/a.ts src/b.ts"),
        vec![
            ("src/a.ts".to_string(), "fmt"),
            ("src/b.ts".to_string(), "fmt"),
        ]
    );
    assert_eq!(
        paths("rustfmt src/lib.rs"),
        vec![("src/lib.rs".to_string(), "fmt")]
    );
    assert_eq!(
        paths("gofmt -w main.go"),
        vec![("main.go".to_string(), "fmt")]
    );
    assert_eq!(
        paths("clang-format -i render.cc"),
        vec![("render.cc".to_string(), "fmt")]
    );
    assert_eq!(
        paths("ruff format srv/api.py"),
        vec![("srv/api.py".to_string(), "fmt")]
    );
    // A formatter row resolves through the record cwd like any relative operand.
    let m = parse_bash_mutations("eslint --fix src/app.js");
    assert_eq!(
        m[0].resolve(Some("/repo")),
        ("/repo/src/app.js".to_string(), Resolution::CwdJoined)
    );
}

#[test]
fn formatter_with_flags_only_falls_back_to_the_marker() {
    assert_eq!(just_paths("prettier --write"), ["fmt:prettier"]);
    // A `$VAR` operand is dropped by the precision rule, leaving the marker.
    assert_eq!(just_paths("prettier --write $FILES"), ["fmt:prettier"]);
}

#[test]
fn package_manager_markers() {
    assert_eq!(just_paths("npm install"), ["pkg:npm"]);
    assert_eq!(just_paths("npm ci"), ["pkg:npm"]);
    assert_eq!(just_paths("npm audit fix"), ["pkg:npm"]);
    assert_eq!(just_paths("pnpm add left-pad"), ["pkg:pnpm"]);
    assert_eq!(just_paths("yarn install"), ["pkg:yarn"]);
    assert_eq!(just_paths("bun add elysia"), ["pkg:bun"]);
    assert_eq!(just_paths("cargo add serde"), ["pkg:cargo"]);
    assert_eq!(just_paths("pip install requests"), ["pkg:pip"]);
    assert_eq!(just_paths("poetry lock"), ["pkg:poetry"]);
    assert_eq!(just_paths("uv sync"), ["pkg:uv"]);
    // Read-only invocations stay silent.
    assert!(just_paths("npm run build").is_empty());
    assert!(just_paths("npm test").is_empty());
    assert!(just_paths("pip list").is_empty());
    assert!(just_paths("cargo tree").is_empty());
}

#[test]
fn npm_run_formatter_alias_is_a_marker() {
    // The alias hides the real tool; the alias NAME is the only signal.
    assert_eq!(just_paths("npm run format"), ["fmt:npm-script"]);
    assert_eq!(just_paths("pnpm run lint:fix"), ["fmt:npm-script"]);
    assert!(just_paths("npm run dev").is_empty());
}

#[test]
fn extract_markers_and_their_dry_runs() {
    assert_eq!(just_paths("unzip bundle.zip"), ["extract:unzip"]);
    assert!(just_paths("unzip -l bundle.zip").is_empty());
    assert_eq!(just_paths("7z x bundle.7z"), ["extract:7z"]);
    assert!(just_paths("7z l bundle.7z").is_empty());
    assert_eq!(just_paths("patch -p1 < fix.patch"), ["extract:patch"]);
    assert!(just_paths("patch --dry-run -p1 < fix.patch").is_empty());
}

#[test]
fn runner_prefixes_forward_to_the_real_tool() {
    assert_eq!(just_paths("npx eslint --fix src/a.js"), ["src/a.js"]);
    assert_eq!(just_paths("bunx prettier --write"), ["fmt:prettier"]);
    // An unknown tool behind the runner emits nothing.
    assert!(just_paths("npx cowsay hello").is_empty());
}

#[test]
fn class_markers_survive_resolution_verbatim() {
    for head in [
        "fmt:cargo",
        "interp:python",
        "pkg:npm",
        "extract:tar",
        "git:add",
    ] {
        assert!(is_class_marker(head), "{head} is a marker");
    }
    let m = parse_bash_mutations("cargo fmt");
    assert_eq!(
        m[0].resolve(Some("/repo")),
        ("fmt:cargo".to_string(), Resolution::Unresolved)
    );
}

// ── Mutation-kill pins: marker verbs, positive formatter runs, and the audit gate ──

#[test]
fn marker_rows_carry_their_class_verbs() {
    // The verb is part of the wire row (files JSON `op` derives from it): pin the
    // class -> verb map per marker family, not just the pseudo-path.
    assert_eq!(paths("cargo fmt"), vec![("fmt:cargo".to_string(), "fmt")]);
    assert_eq!(paths("npm install"), vec![("pkg:npm".to_string(), "pkg")]);
    assert_eq!(
        paths("unzip bundle.zip"),
        vec![("extract:unzip".to_string(), "extract")]
    );
}

#[test]
fn black_and_ruff_positive_runs_emit_rows() {
    // The write-mode runs (no --check/--diff) emit real fmt rows.
    assert_eq!(paths("black app.py"), vec![("app.py".to_string(), "fmt")]);
    assert_eq!(
        paths("ruff --fix lint.py"),
        vec![("lint.py".to_string(), "fmt")]
    );
    // The `check` subcommand word is peeled, never emitted as a file.
    assert_eq!(
        paths("ruff check --fix srv/api.py"),
        vec![("srv/api.py".to_string(), "fmt")]
    );
    // `npm audit` WITHOUT `fix` is read-only.
    assert!(just_paths("npm audit").is_empty());
}
