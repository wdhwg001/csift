//! Interpreter write-idiom analysis: literal targets, one-hop constants, opaque
//! markers, and the never-fabricate guards.

use super::*;

#[test]
fn python_heredoc_literal_write_text_is_a_row() {
    let cmd = "python3 - <<'PY'\nfrom pathlib import Path\nPath('notes.md').write_text('x')\nPY";
    assert_eq!(paths(cmd), vec![("notes.md".to_string(), "interp-write")]);
    // The relative target resolves through the record cwd like any operand.
    let m = parse_bash_mutations(cmd);
    assert_eq!(
        m[0].resolve(Some("/work/proj")),
        ("/work/proj/notes.md".to_string(), Resolution::CwdJoined)
    );
}

#[test]
fn python_one_hop_constant_binding_resolves() {
    // The dominant corpus shape: one top-level constant, then the write call.
    let cmd = "python3 - <<'PY'\np = 'out/report.json'\ns = build()\nopen(p, 'w').write(s)\nPY";
    assert_eq!(
        paths(cmd),
        vec![("out/report.json".to_string(), "interp-write")]
    );
    // The Path('lit') constructor form binds too.
    let cmd2 = "python3 - <<'PY'\ntarget = Path('data.csv')\ntarget.write_bytes(blob)\nPY";
    assert_eq!(paths(cmd2), vec![("data.csv".to_string(), "interp-write")]);
}

#[test]
fn one_hop_guards_reject_and_fall_back_to_the_opaque_marker() {
    // A reassigned name is not a constant.
    let reassigned = "python3 - <<'PY'\np = 'a.txt'\np = 'b.txt'\nopen(p, 'w').write(s)\nPY";
    assert_eq!(
        paths(reassigned),
        vec![("interp:python".to_string(), "interp")]
    );
    // A loop binder is not a constant.
    let loop_bound = "python3 - <<'PY'\nfor p in files:\n    open(p, 'w').write(s)\nPY";
    assert_eq!(
        paths(loop_bound),
        vec![("interp:python".to_string(), "interp")]
    );
    // argv / any non-literal RHS disqualifies.
    let argv = "python3 - <<'PY'\np = sys.argv[1]\nopen(p, 'w').write(s)\nPY";
    assert_eq!(paths(argv), vec![("interp:python".to_string(), "interp")]);
    // An f-string receiver is opaque.
    let fstring = "python3 - <<'PY'\nPath(f'{base}/x.md').write_text(s)\nPY";
    assert_eq!(
        paths(fstring),
        vec![("interp:python".to_string(), "interp")]
    );
}

#[test]
fn open_without_a_write_mode_is_not_a_write() {
    // A bare `open(p)` and an explicit read mode are reads: no row, no marker.
    assert!(paths("python3 - <<'PY'\ndata = open('cfg.json').read()\nPY").is_empty());
    assert!(paths("python3 - <<'PY'\ndata = open('cfg.json', 'r').read()\nPY").is_empty());
    // A non-literal mode is not provably a write either.
    assert!(paths("python3 - <<'PY'\nf = open('cfg.json', mode)\nPY").is_empty());
}

#[test]
fn node_and_ruby_idioms_are_covered() {
    assert_eq!(
        paths(r#"node -e "require('fs').writeFileSync('cfg.json', body)""#),
        vec![("cfg.json".to_string(), "interp-write")]
    );
    let one_hop = "node - <<'JS'\nconst p = 'dist/app.js';\nfs.writeFileSync(p, code);\nJS";
    assert_eq!(
        paths(one_hop),
        vec![("dist/app.js".to_string(), "interp-write")]
    );
    assert_eq!(
        paths(r#"ruby -e "File.write('log.txt', line)""#),
        vec![("log.txt".to_string(), "interp-write")]
    );
}

#[test]
fn literal_and_opaque_writes_in_one_payload_emit_both() {
    let cmd = "python3 - <<'PY'\nopen('known.md', 'w').write(a)\nopen(dynamic, 'w').write(b)\nPY";
    assert_eq!(
        paths(cmd),
        vec![
            ("known.md".to_string(), "interp-write"),
            ("interp:python".to_string(), "interp"),
        ]
    );
}

#[test]
fn payload_without_a_write_idiom_emits_nothing() {
    assert!(paths(r#"python3 -c "print(1 + 1)""#).is_empty());
    assert!(paths("python3 - <<'PY'\nprint(compute())\nPY").is_empty());
    // A script FILE argument has no visible payload: nothing is guessed off it.
    assert!(paths("python3 build_report.py --verbose input.csv").is_empty());
}

#[test]
fn open_needle_requires_a_word_boundary() {
    // `reopen(` must not match the `open(` needle and fabricate a write.
    assert!(paths("python3 - <<'PY'\nconn.reopen('x.db', 'w')\nPY").is_empty());
    // `io.open(` (a real alias) does match.
    assert_eq!(
        paths("python3 - <<'PY'\nio.open('x.log', 'w').write(s)\nPY"),
        vec![("x.log".to_string(), "interp-write")]
    );
}

#[test]
fn interp_marker_is_a_class_marker_never_resolved() {
    assert!(is_class_marker("interp:python"));
    let m = parse_bash_mutations("python3 - <<'PY'\nopen(p, 'w').write(s)\nPY");
    assert_eq!(m.len(), 1);
    assert_eq!(
        m[0].resolve(Some("/base")),
        ("interp:python".to_string(), Resolution::Unresolved)
    );
}
