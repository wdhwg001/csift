use super::*;

// ── Mutation-kill pins (cargo-mutants survivors): heredoc scanning boundaries the
//    ordinary fixtures never reached — wrong consumed-offset arithmetic corrupts the
//    SECOND delimiter on a line, and a broken closer comparison leaks body lines. ──

#[test]
fn heredoc_delims_forms_and_offsets() {
    assert_eq!(heredoc_delims("cat <<EOF"), ["EOF"]);
    assert_eq!(heredoc_delims("cat <<-END x"), ["END"]);
    assert_eq!(heredoc_delims("cat <<'A B'"), ["A B"]);
    assert_eq!(heredoc_delims(r#"cat <<"QD" y"#), ["QD"]);
    // Two on one line, in order — the consumed-offset arithmetic must be exact.
    assert_eq!(heredoc_delims("cat <<ONE <<'TWO'"), ["ONE", "TWO"]);
    // A here-STRING is not a heredoc (no body line follows).
    assert!(heredoc_delims("cat <<<word").is_empty());
    // A bare delimiter stops at a shell metacharacter.
    assert_eq!(heredoc_delims("cat <<EOF;echo x"), ["EOF"]);
}

#[test]
fn heredoc_bodies_dropped_until_exact_closer() {
    let cmd = "cat <<EOF > $OUT\nline one\nline two\nEOF\necho after";
    let stripped = strip_heredoc_bodies(cmd);
    assert!(
        !stripped.contains("line one") && !stripped.contains("line two"),
        "body lines must drop: {stripped}"
    );
    assert!(
        stripped.contains("echo after"),
        "post-closer line kept: {stripped}"
    );
    // A body line that merely CONTAINS the delimiter is not the closer.
    let s2 = strip_heredoc_bodies("cat <<EOF\nnot EOF here\nEOF\necho tail");
    assert!(
        !s2.contains("not EOF here") && s2.contains("echo tail"),
        "closer is exact-trimmed-match only: {s2}"
    );
}

#[test]
fn rm_each_non_flag_operand() {
    assert_eq!(
        paths("rm -rf /tmp/a /tmp/b"),
        vec![("/tmp/a".to_string(), "rm"), ("/tmp/b".to_string(), "rm")]
    );
}

#[test]
fn mkdir_p_creates_each_dir() {
    assert_eq!(
        paths("mkdir -p /tmp/x /tmp/y"),
        vec![
            ("/tmp/x".to_string(), "mkdir"),
            ("/tmp/y".to_string(), "mkdir")
        ]
    );
}

#[test]
fn touch_and_tee() {
    assert_eq!(paths("touch a.txt"), vec![("a.txt".to_string(), "touch")]);
    assert_eq!(paths("tee out.log"), vec![("out.log".to_string(), "tee")]);
}

#[test]
fn touch_value_flags_skip_their_arguments() {
    // `-r REFFILE`: the reference file is READ-ONLY — only the real target is created.
    assert_eq!(
        paths("touch -r /ref/file /tmp/out.txt"),
        vec![("/tmp/out.txt".to_string(), "touch")],
        "the -r reference file must not be fabricated as a created path"
    );
    // `-d DATE`: the date string must not be emitted as a path.
    assert_eq!(
        paths("touch -d '2020-01-01' /tmp/out.txt"),
        vec![("/tmp/out.txt".to_string(), "touch")]
    );
    // `--reference=REF` inline form.
    assert_eq!(
        paths("touch --reference=/ref/f /tmp/out.txt"),
        vec![("/tmp/out.txt".to_string(), "touch")]
    );
    // `-t STAMP`: the timestamp is dropped (also caught by the digit-noise filter, but
    // the explicit skip is the precise reason).
    assert_eq!(
        paths("touch -t 202001010000 /tmp/out.txt"),
        vec![("/tmp/out.txt".to_string(), "touch")]
    );
}

#[test]
fn tee_append_is_not_a_create() {
    // `tee -a` / `tee --append` do not truncate (mirrors `>>` vs `>`): verb `tee-a`.
    assert_eq!(
        paths("echo hi | tee -a /tmp/a.log"),
        vec![("/tmp/a.log".to_string(), "tee-a")]
    );
    assert_eq!(
        paths("echo hi | tee --append /tmp/b.log"),
        vec![("/tmp/b.log".to_string(), "tee-a")]
    );
    // Plain `tee` (truncate) stays a `tee` create.
    assert_eq!(
        paths("echo hi | tee /tmp/c.log"),
        vec![("/tmp/c.log".to_string(), "tee")]
    );
}

#[test]
fn ln_target_directory_emits_dest_not_source() {
    // `ln -s -t DIR target`: the real destination is DIR; `target` is the read source.
    assert_eq!(
        paths("ln -s -t /tmp/linkdir /target"),
        vec![("/tmp/linkdir".to_string(), "ln")],
        "the -t DIR is the link destination; the source target must not be reported"
    );
    // `--target-directory=DIR` inline form.
    assert_eq!(
        paths("ln --target-directory=/tmp/d /src"),
        vec![("/tmp/d".to_string(), "ln")]
    );
    // Plain `ln -s target linkname`: last positional (the link) is the destination.
    assert_eq!(
        paths("ln -s /target /tmp/link"),
        vec![("/tmp/link".to_string(), "ln")]
    );
}

#[test]
fn tar_create_emits_archive() {
    // `tar czf` creates the archive (verb `tar`); the source dir is not a written path.
    assert_eq!(
        paths("tar czf /tmp/a.tar.gz src/"),
        vec![("/tmp/a.tar.gz".to_string(), "tar")]
    );
}

#[test]
fn trailing_comment_words_are_not_fabricated_paths() {
    // A `# words` comment after a real mutation must NOT leak its words (nor the bare
    // `#`) as touched paths — only the genuine operand survives.
    assert_eq!(
        paths("touch /tmp/real.txt  # create the marker file"),
        vec![("/tmp/real.txt".to_string(), "touch")]
    );
    assert_eq!(
        paths("rm -rf /tmp/build && mkdir /tmp/build  # rebuild from scratch"),
        vec![
            ("/tmp/build".to_string(), "rm"),
            ("/tmp/build".to_string(), "mkdir")
        ]
    );
}

#[test]
fn in_comment_redirect_does_not_fabricate_a_path() {
    // A `>`/`>>` that appears INSIDE a trailing comment is masked with the comment, so
    // it is not read as a real redirect — only the genuine `mkdir` dir survives.
    assert_eq!(
        paths("mkdir -p /tmp/d # comment > /tmp/fabricated-by-comment-redirect.txt"),
        vec![("/tmp/d".to_string(), "mkdir")]
    );
}

#[test]
fn comment_does_not_displace_cp_destination() {
    // The comment must not steal the cp/mv/ln destination: the LAST positional after the
    // comment is masked away, so the true dest is the surviving last operand.
    assert_eq!(
        paths("cp /tmp/src.txt /tmp/real-dest.bak  # café note"),
        vec![("/tmp/real-dest.bak".to_string(), "cp")]
    );
    // `mv a b` reports the dest (`mv`) AND the source (`mv-from`); the `# note` tail must
    // be masked away so it neither displaces the dest nor adds a phantom source.
    assert_eq!(
        paths("mv /tmp/a /tmp/b  # note"),
        vec![
            ("/tmp/b".to_string(), "mv"),
            ("/tmp/a".to_string(), "mv-from")
        ]
    );
}

#[test]
fn in_path_hash_is_not_a_comment() {
    // A `#` that is part of a path (prev byte is a path char, not whitespace/separator)
    // is NOT a comment and the whole path is preserved.
    assert_eq!(
        paths("rm -f /tmp/a#b.txt"),
        vec![("/tmp/a#b.txt".to_string(), "rm")]
    );
    assert_eq!(
        paths("touch file#1.log"),
        vec![("file#1.log".to_string(), "touch")]
    );
}

#[test]
fn comment_after_separator_without_space_is_masked() {
    // `cmd;# c` / `cmd|# c`: a `#` directly after a command separator (no space) still
    // opens a comment (the separator is a word boundary).
    assert_eq!(
        paths("touch /tmp/x.txt;# trailing"),
        vec![("/tmp/x.txt".to_string(), "touch")]
    );
}

#[test]
fn full_line_leading_comment_emits_nothing() {
    // A line that is ENTIRELY a comment yields no rows (the verb itself is masked away).
    assert_eq!(paths("# just a note, nothing here"), vec![]);
}

#[test]
fn cp_records_only_destination() {
    assert_eq!(
        paths("cp src.txt dst.txt"),
        vec![("dst.txt".to_string(), "cp")]
    );
    // With flags, the LAST non-flag operand is the destination.
    assert_eq!(
        paths("cp -r a/ b/ /dest/"),
        vec![("/dest/".to_string(), "cp")]
    );
}

#[test]
fn mv_destination_and_sources() {
    // `mv a b c`: c is the destination (mv), a + b are moved-from sources (mv-from).
    assert_eq!(
        paths("mv a b c"),
        vec![
            ("c".to_string(), "mv"),
            ("a".to_string(), "mv-from"),
            ("b".to_string(), "mv-from")
        ]
    );
    // Two-operand mv: one destination, one source.
    assert_eq!(
        paths("mv old new"),
        vec![("new".to_string(), "mv"), ("old".to_string(), "mv-from")]
    );
    // Single operand (degenerate) → just a destination, no source.
    assert_eq!(paths("mv onlyone"), vec![("onlyone".to_string(), "mv")]);
}

#[test]
fn sed_in_place_vs_streaming() {
    // sed WITHOUT -i mutates nothing.
    assert!(paths("sed s/a/b/ file.txt").is_empty());
    // sed -i mutates the file.
    assert_eq!(
        paths("sed -i s/a/b/ file.txt"),
        vec![("file.txt".to_string(), "sed-i")]
    );
    // sed -i.bak (a backup-suffix in-place flag).
    assert_eq!(
        paths("sed -i.bak s/a/b/ file.txt"),
        vec![("file.txt".to_string(), "sed-i")]
    );
    // sed --in-place long form.
    assert_eq!(
        paths("sed --in-place s/a/b/ file.txt"),
        vec![("file.txt".to_string(), "sed-i")]
    );
}

#[test]
fn sed_bsd_empty_suffix_in_place() {
    // BSD/macOS in-place spelling `sed -i '' '<script>' file`: the `''` is the (empty)
    // backup suffix, NOT the script. The real script must not be fabricated as a file,
    // and the real file must still be recorded.
    assert_eq!(
        paths("sed -i '' 's/a/b/' /real/x.txt"),
        vec![("/real/x.txt".to_string(), "sed-i")]
    );
    assert_eq!(
        paths("sed -i \"\" '/pattern/d' /real/del.txt"),
        vec![("/real/del.txt".to_string(), "sed-i")]
    );
}

#[test]
fn sed_multiple_expression_flags() {
    // Multi `-e` scripts: every `-e`'s VALUE is a script, so the 2nd-and-later must NOT
    // be fabricated as files; only the real file is recorded. Both GNU `-i` and BSD
    // `-i ''` forms.
    assert_eq!(
        paths("sed -i -e 's/a/b/' -e 's/c/d/' /real/m.txt"),
        vec![("/real/m.txt".to_string(), "sed-i")]
    );
    assert_eq!(
        paths("sed -i '' -e 's/a/b/' -e 's/c/d/' /real/bm.txt"),
        vec![("/real/bm.txt".to_string(), "sed-i")]
    );
    // The long `--expression=` inline form, and `-f` script-file form.
    assert_eq!(
        paths("sed -i --expression='s/a/b/' /real/e.txt"),
        vec![("/real/e.txt".to_string(), "sed-i")]
    );
    assert_eq!(
        paths("sed -i -f script.sed /real/f.txt"),
        vec![("/real/f.txt".to_string(), "sed-i")]
    );
}

#[test]
fn sed_in_place_equals_suffix() {
    // GNU `--in-place=SUFFIX` long backup form must be recognized as in-place (was a
    // recall miss — the helper only matched `--in-place` / `-i…`).
    assert_eq!(
        paths("sed --in-place=.bak 's/a/b/' /real/s.txt"),
        vec![("/real/s.txt".to_string(), "sed-i")]
    );
}

#[test]
fn output_flag_rejects_format_selector() {
    // `--output <format>` / `--output=<format>`: a bare format-selector value (kubectl,
    // gh, docker, aws, jq idioms) is a render mode, never a created file.
    assert!(paths("gh pr list --output json").is_empty());
    assert!(paths("kubectl get pods --output=yaml").is_empty());
    assert!(paths("mytool --output summary").is_empty());
    // But a path-shaped value (extension or slash) still records.
    assert_eq!(
        paths("mytool --output report.json"),
        vec![("report.json".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("mytool --output=/tmp/out"),
        vec![("/tmp/out".to_string(), "flag-output")]
    );
}

#[test]
fn redirect_combined_stream_amp_after_gt() {
    // `>&file` / `>& file` is the combined stdout+stderr file redirect (equivalent to
    // `&>file`) — a real write. Both spacings, asymmetric-no-longer with `&>`.
    assert_eq!(
        paths("make >& /real/a.log"),
        vec![("/real/a.log".to_string(), ">")]
    );
    assert_eq!(
        paths("make >&/real/b.log"),
        vec![("/real/b.log".to_string(), ">")]
    );
    // `>&N` (N a bare fd number) is an fd-dup, NOT a file → no row. Attached + spaced.
    assert!(paths("cmd >&1").is_empty());
    assert!(paths("cmd >&2").is_empty());
    assert!(paths("cmd >& 2").is_empty());
    // `>&-` closes an fd — also not a file.
    assert!(paths("cmd >&-").is_empty());
    assert!(paths("cmd >& -").is_empty());
    // A bare `>&` with NO following token emits nothing (degenerate, no panic).
    assert!(paths("cmd >&").is_empty());
    // `&>file` (the already-working sibling) is unchanged.
    assert_eq!(
        paths("make &> /real/c.log"),
        vec![("/real/c.log".to_string(), ">")]
    );
}

#[test]
fn output_flag_format_selector_edge_forms() {
    // The inline `--output=<format>` form is dropped just like the spaced one.
    assert!(paths("kubectl get pods --out=wide").is_empty());
    assert!(paths("tool --logfile=none").is_empty());
    // `--output` followed by ANOTHER flag does not consume the flag as a path (and the
    // flag is not skipped) — no phantom row, the next flag is still scannable.
    assert!(paths("tool --output --verbose").is_empty());
    // A format word that nonetheless carries a path shape (slash/extension) is a real file.
    assert_eq!(
        paths("tool --output ./json"),
        vec![("./json".to_string(), "flag-output")]
    );
}

#[test]
fn is_fd_number_classifies_dup_vs_file() {
    // Direct helper coverage: digits / `-` are fd ops; a word is a file target.
    assert!(is_fd_number("1"));
    assert!(is_fd_number("22"));
    assert!(is_fd_number("-"));
    assert!(is_fd_number("'2'")); // quote-stripped digit
    assert!(!is_fd_number("build.log"));
    assert!(!is_fd_number("/tmp/x"));
    assert!(!is_fd_number(""));
}

#[test]
fn redirection_both_spacings() {
    // Spaced form.
    assert_eq!(
        paths("echo hi > out.txt"),
        vec![("out.txt".to_string(), ">")]
    );
    // Attached form.
    assert_eq!(
        paths("echo hi >out.txt"),
        vec![("out.txt".to_string(), ">")]
    );
    // Append, both spacings.
    assert_eq!(
        paths("echo hi >> log.txt"),
        vec![("log.txt".to_string(), ">>")]
    );
    assert_eq!(
        paths("echo hi >>log.txt"),
        vec![("log.txt".to_string(), ">>")]
    );
}
