//! Content-anchor classification: the admissible shapes and every refusal gate.

use super::*;

/// The command's SINGLE anchor (read or sole write), for terse asserts.
fn one(cmd: &str) -> Option<AnchorCmd> {
    let a = bash_anchors(cmd);
    if a.read.is_some() {
        assert!(a.writes.is_empty(), "read and writes together: {a:?}");
        return a.read;
    }
    assert!(a.writes.len() <= 1, "expected at most one write: {a:?}");
    a.writes.into_iter().next()
}

#[test]
fn read_shapes_classify() {
    assert_eq!(
        one("cat /tmp/notes.md"),
        Some(AnchorCmd::ReadFull {
            operand: "/tmp/notes.md".into()
        })
    );
    assert_eq!(
        one("head -n 40 src/lib.rs"),
        Some(AnchorCmd::ReadWindow {
            operand: "src/lib.rs".into(),
            start: 1,
            end: Some(40)
        })
    );
    assert_eq!(
        one("head -n40 src/lib.rs"),
        Some(AnchorCmd::ReadWindow {
            operand: "src/lib.rs".into(),
            start: 1,
            end: Some(40)
        })
    );
    assert_eq!(
        one("sed -n '10,20p' src/lib.rs"),
        Some(AnchorCmd::ReadWindow {
            operand: "src/lib.rs".into(),
            start: 10,
            end: Some(20)
        })
    );
    assert_eq!(
        one("sed -n '7p' notes.txt"),
        Some(AnchorCmd::ReadWindow {
            operand: "notes.txt".into(),
            start: 7,
            end: Some(7)
        })
    );
    assert_eq!(
        one("sed -n '5,$p' notes.txt"),
        Some(AnchorCmd::ReadWindow {
            operand: "notes.txt".into(),
            start: 5,
            end: None
        })
    );
}

#[test]
fn read_refusals() {
    // Compound commands, pipes, substitutions, redirects, variables, globs: no anchor.
    for cmd in [
        "cat a.txt b.txt",
        "cat a.txt | head -5",
        "cd /x && cat a.txt",
        "cat $FILE",
        "cat *.txt",
        "cat a.txt > copy.txt",
        "cat \"$(latest)\"",
        "cat `latest`",
        "sed -n '20,10p' f.txt",
        "sed -i 's/a/b/' f.txt",
        "sed -n 10,20p f.txt extra",
        "head -c 100 f.txt",
        "tail -n 5 f.txt",
        "cat a.txt &",
    ] {
        assert_eq!(one(cmd), None, "{cmd}");
    }
    // A quoted operand with spaces IS literal.
    assert_eq!(
        one("cat '/tmp/my notes.md'"),
        Some(AnchorCmd::ReadFull {
            operand: "/tmp/my notes.md".into()
        })
    );
}

#[test]
fn heredoc_write_shapes() {
    let cmd = "cat > /tmp/out.txt <<'EOF'\nalpha line\nbeta line\nEOF";
    assert_eq!(
        one(cmd),
        Some(AnchorCmd::WriteFull {
            operand: "/tmp/out.txt".into(),
            content: "alpha line\nbeta line\n".into(),
            heredoc: true
        })
    );
    // Opener-after form + append + tee forms.
    let cmd2 = "cat <<'EOF' >> /tmp/out.txt\nmore\nEOF";
    assert_eq!(
        one(cmd2),
        Some(AnchorCmd::Append {
            operand: "/tmp/out.txt".into(),
            content: "more\n".into(),
            heredoc: true
        })
    );
    let tee = "tee /tmp/out.txt <<'EOF'\nbody\nEOF";
    assert_eq!(
        one(tee),
        Some(AnchorCmd::WriteFull {
            operand: "/tmp/out.txt".into(),
            content: "body\n".into(),
            heredoc: true
        })
    );
    let teea = "tee -a /tmp/out.txt <<'EOF'\nbody\nEOF";
    assert_eq!(
        one(teea),
        Some(AnchorCmd::Append {
            operand: "/tmp/out.txt".into(),
            content: "body\n".into(),
            heredoc: true
        })
    );
    // Unquoted delimiter: admissible only with an expansion-free body.
    assert_eq!(
        one("cat > f.txt <<EOF\nplain text\nEOF"),
        Some(AnchorCmd::WriteFull {
            operand: "f.txt".into(),
            content: "plain text\n".into(),
            heredoc: true
        })
    );
    assert_eq!(
        one("cat > f.txt <<EOF\nhome is $HOME\nEOF"),
        None,
        "unquoted delimiter + $ in body: bash would expand"
    );
    // The QUOTED delimiter keeps a $-bearing body verbatim.
    assert!(matches!(
        one("cat > f.txt <<'EOF'\nhome is $HOME\nEOF"),
        Some(AnchorCmd::WriteFull { content, .. }) if content == "home is $HOME\n"
    ));
}

#[test]
fn heredoc_consumer_gate() {
    // Interpreter heredocs are SCRIPTS; ssh heredocs write remotely. Neither anchors.
    for cmd in [
        "python3 <<'PY'\nopen('x','w').write('data')\nPY",
        "bash <<'SH'\necho hi > /tmp/x\nSH",
        "ssh host 'bash -s' <<'EOF'\ncat > /remote/f\nEOF",
        "ssh host tee /remote/f <<'EOF'\ndata\nEOF",
    ] {
        assert_eq!(one(cmd), None, "{cmd}");
    }
}

#[test]
fn literal_write_shapes() {
    assert_eq!(
        one("echo 'hello anchor' > /tmp/one.txt"),
        Some(AnchorCmd::WriteFull {
            operand: "/tmp/one.txt".into(),
            content: "hello anchor\n".into(),
            heredoc: false
        })
    );
    assert_eq!(
        one("echo -n done >> status.log"),
        Some(AnchorCmd::Append {
            operand: "status.log".into(),
            content: "done".into(),
            heredoc: false
        })
    );
    // Multiple literal args join with single spaces (echo semantics).
    assert!(matches!(
        one("echo alpha beta > f.txt"),
        Some(AnchorCmd::WriteFull { content, .. }) if content == "alpha beta\n"
    ));
    // -e escape processing / interpolation / vars: refused.
    assert_eq!(one("echo -e 'a\\nb' > f.txt"), None);
    assert_eq!(one("echo \"at $HOME\" > f.txt"), None);
    // printf: literal %-free escape-free format only, no added newline.
    assert!(matches!(
        one("printf 'raw bytes' > f.txt"),
        Some(AnchorCmd::WriteFull { content, .. }) if content == "raw bytes"
    ));
    assert_eq!(one("printf '%s\\n' x > f.txt"), None);
    assert_eq!(
        one("truncate -s 0 build.log"),
        Some(AnchorCmd::WriteFull {
            operand: "build.log".into(),
            content: String::new(),
            heredoc: false
        })
    );
    assert_eq!(one("truncate -s 100 build.log"), None);
}

#[test]
fn write_anchors_survive_compound_commands() {
    // The dominant real steer shape: write the file, then run it. The write
    // segment anchors; multi_segment tells the caller to demand a clean echo.
    let cmd = "cat > tool.py <<'EOF'\nprint('ready')\nEOF\npython3 tool.py && git diff --stat";
    let a = bash_anchors(cmd);
    assert!(a.multi_segment);
    assert_eq!(a.read, None, "no read anchor in a compound command");
    assert_eq!(
        a.writes,
        vec![AnchorCmd::WriteFull {
            operand: "tool.py".into(),
            content: "print('ready')\n".into(),
            heredoc: true
        }],
        "{a:?}"
    );
    // A read in a compound command never anchors (stdout is a concatenation).
    let r = bash_anchors("cd /x && cat a.txt");
    assert_eq!(r.read, None);
    assert!(r.writes.is_empty());
    // Two independent write segments each anchor their own file.
    let two = bash_anchors("echo alpha > a.txt && echo beta > b.txt");
    assert_eq!(two.writes.len(), 2, "{two:?}");
}

#[test]
fn redirect_token_forms_and_refusals() {
    // Attached redirect forms.
    assert_eq!(
        one("echo hi >f.txt"),
        Some(AnchorCmd::WriteFull {
            operand: "f.txt".into(),
            content: "hi\n".into(),
            heredoc: false
        })
    );
    assert_eq!(
        one("echo hi >>f.txt"),
        Some(AnchorCmd::Append {
            operand: "f.txt".into(),
            content: "hi\n".into(),
            heredoc: false
        })
    );
    // A SPACED heredoc opener (`<< 'EOF'`) still pairs its delimiter token.
    assert_eq!(
        one("cat > f.txt << 'EOF'\nbody\nEOF"),
        Some(AnchorCmd::WriteFull {
            operand: "f.txt".into(),
            content: "body\n".into(),
            heredoc: true
        })
    );
    // Two redirects, fd-form redirects, here-strings: refuse.
    assert_eq!(one("echo a > f.txt > g.txt"), None);
    assert_eq!(one("echo a 2>/dev/null > f.txt"), None);
    assert_eq!(one("cat <<< words"), None);
    // Two heredocs in one segment: refuse.
    assert_eq!(one("cat <<'A' <<'B' > f.txt\nx\nA\ny\nB"), None);
    // A variable redirect target refuses the whole segment.
    assert_eq!(one("echo hi > $OUT"), None);
    // Adjacent quoted spans are not one literal.
    assert_eq!(one("cat 'a'x'b'"), None);
    // tee with a flag-shaped operand refuses.
    assert_eq!(one("tee -x /tmp/f <<'EOF'\nbody\nEOF"), None);
}
