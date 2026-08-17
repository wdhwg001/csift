//! Per-verb path extraction: rm, cp, mv, sed, tar, git and friends; destination resolution.

use super::*;

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
fn git_mutating_vs_readonly_subcommand() {
    assert_eq!(paths("git add ."), vec![("git:add".to_string(), "git")]);
    assert_eq!(
        paths("git commit -m wip"),
        vec![("git:commit".to_string(), "git")]
    );
    // Read-only subcommands record nothing.
    assert!(paths("git status").is_empty());
    assert!(paths("git log --oneline").is_empty());
    assert!(paths("git diff HEAD").is_empty());
}

#[test]
fn git_skips_leading_dash_c_option() {
    // `git -c key=val commit` → the subcommand is the first non-flag token.
    assert_eq!(
        paths("git -c user.name=x commit"),
        vec![("git:commit".to_string(), "git")]
    );
    // A bare global option (`--no-pager`) is skipped to find the subcommand.
    assert_eq!(
        paths("git --no-pager add ."),
        vec![("git:add".to_string(), "git")]
    );
    // `-C <path>` also consumes its value before the subcommand.
    assert_eq!(
        paths("git -C /repo commit"),
        vec![("git:commit".to_string(), "git")]
    );
    // git with ONLY options and no subcommand → git_subcommand returns None.
    assert!(paths("git --version").is_empty());
    assert!(paths("git -c a=b").is_empty());
}

#[test]
fn mv_with_only_flags_has_no_paths() {
    // `mv -f` (only a flag, no operands) → emit_mv's split_last None arm.
    assert!(paths("mv -f").is_empty());
}

#[test]
fn cp_with_only_flags_emits_nothing() {
    // `cp -r` (no operands) → emit_last_operand finds no paths.
    assert!(paths("cp -r").is_empty());
}

#[test]
fn sudo_and_env_prefixes_are_stripped() {
    // sudo rm → still detected.
    assert_eq!(paths("sudo rm /etc/x"), vec![("/etc/x".to_string(), "rm")]);
    // env VAR=1 touch → the real verb is `touch` after stripping the env assignment.
    assert_eq!(
        paths("env FOO=1 BAR=2 touch out.txt"),
        vec![("out.txt".to_string(), "touch")]
    );
}

// ── R1: cp/mv/install `-t DIR` correctness (source-vs-dest inversion) ──

#[test]
fn cp_target_directory_flag_dest_is_the_t_value_not_a_source() {
    // `cp -t DIR src…`: the DEST is DIR; the sources are reads, never reported as cp.
    assert_eq!(
        paths("cp -t /tmp/destdir /home/a.txt /home/b.txt"),
        vec![("/tmp/destdir".to_string(), "cp")]
    );
    // `--target-directory=DIR` inline form.
    assert_eq!(
        paths("cp --target-directory=/tmp/d /home/a.txt"),
        vec![("/tmp/d".to_string(), "cp")]
    );
    // install behaves the same.
    assert_eq!(
        paths("install -m755 -t /usr/local/bin a b"),
        vec![("/usr/local/bin".to_string(), "install")]
    );
}

#[test]
fn mv_target_directory_flag_dest_and_sources() {
    // `mv -t DIR a b`: DIR is the destination (mv); a + b are mv-from sources; DIR is
    // NOT also listed as a source.
    let got = paths("mv -t /tmp/destdir /home/a.txt /home/b.txt");
    assert!(
        got.contains(&("/tmp/destdir".to_string(), "mv")),
        "got: {got:?}"
    );
    assert!(
        got.contains(&("/home/a.txt".to_string(), "mv-from")),
        "got: {got:?}"
    );
    assert!(
        got.contains(&("/home/b.txt".to_string(), "mv-from")),
        "got: {got:?}"
    );
    assert!(
        !got.iter()
            .any(|(p, v)| p == "/tmp/destdir" && *v == "mv-from"),
        "the -t DIR must not also be a source: {got:?}"
    );
}

// ── R1: recall — ln / install / rsync / >| ──

#[test]
fn ln_install_rsync_destinations_caught() {
    assert_eq!(
        paths("ln -s /src /tmp/link"),
        vec![("/tmp/link".to_string(), "ln")]
    );
    assert_eq!(
        paths("install -m755 bin /usr/local/bin/tool"),
        vec![("/usr/local/bin/tool".to_string(), "install")]
    );
    assert_eq!(
        paths("rsync -a src/ /tmp/dest/"),
        vec![("/tmp/dest/".to_string(), "rsync")]
    );
}

// ── R7: a trailing OUTPUT redirect must not poison a positional-dest verb ──
// collect_redirections now REMOVES the redirect tokens (operator + its spaced path) from
// the operand stream before verb dispatch — symmetric to strip_input_redirects for `<`.
// Without this, the surviving `2>&1` / `2>/dev/null` / spaced `> file` token displaced the
// real cp/mv/ln/install/rsync destination (RECALL MISS), got mislabeled as a source
// (SEMANTIC LEAK), or double-emitted the redirect path (DOUBLE-EMIT).

#[test]
fn cp_dest_survives_trailing_fd_dup_redirect() {
    // `2>&1` is an fd-dup (no path); it must NOT become cp's positional.last() dest. The
    // REAL dest `/tmp/CP_DEST.txt` is emitted exactly once, the source is a read.
    assert_eq!(
        paths("cp src.txt /tmp/CP_DEST.txt 2>&1"),
        vec![("/tmp/CP_DEST.txt".to_string(), "cp")]
    );
}

#[test]
fn install_ln_rsync_dest_survive_trailing_redirect() {
    // install with `2>&1`: the real dest /etc/DEST.conf survives.
    assert_eq!(
        paths("install -m 644 s /etc/DEST.conf 2>&1"),
        vec![("/etc/DEST.conf".to_string(), "install")]
    );
    // ln -s with `2>/dev/null`: the link name survives (the dev-sink emits no row).
    assert_eq!(
        paths("ln -s t /tmp/LINKNAME 2>/dev/null"),
        vec![("/tmp/LINKNAME".to_string(), "ln")]
    );
    // rsync with `2>&1`: the dest dir survives.
    assert_eq!(
        paths("rsync -a src/ /tmp/RSYNC_DEST/ 2>&1"),
        vec![("/tmp/RSYNC_DEST/".to_string(), "rsync")]
    );
}

#[test]
fn mv_into_dir_not_leaked_as_source_by_trailing_redirect() {
    // `mv /a /b /tmp/DESTDIR/ 2>/dev/null`: the spaced/attached `2>/dev/null` is a dev-sink
    // (emits no row) AND is removed from operands, so DESTDIR stays the dest (verb `mv`) and
    // /a,/b stay sources (mv-from). Before the fix, `2>/dev/null` became the dest_tok
    // (rejected as a sink → NO mv dest) and ALL of /a,/b,DESTDIR leaked as mv-from reads.
    assert_eq!(
        paths("mv /a /b /tmp/DESTDIR/ 2>/dev/null"),
        vec![
            ("/tmp/DESTDIR/".to_string(), "mv"),
            ("/a".to_string(), "mv-from"),
            ("/b".to_string(), "mv-from")
        ]
    );
}

#[test]
fn cp_dest_survives_trailing_append_redirect_no_double_emit() {
    // `cp a b /tmp/CDEST/ >> /tmp/log.txt`: the `>>` and its spaced path are both consumed,
    // so /tmp/log.txt is emitted ONCE (by the redirect collector, verb `>>`) and the real
    // dest /tmp/CDEST/ stays cp's last positional — no double-emit, no dropped dest.
    assert_eq!(
        paths("cp a b /tmp/CDEST/ >> /tmp/log.txt"),
        vec![
            ("/tmp/log.txt".to_string(), ">>"),
            ("/tmp/CDEST/".to_string(), "cp")
        ]
    );
}

#[test]
fn cp_dest_survives_spaced_truncate_redirect() {
    // The spaced `> /tmp/log` form (operator + path are two tokens): both consumed.
    assert_eq!(
        paths("cp src /tmp/CP2/ > /tmp/log"),
        vec![
            ("/tmp/log".to_string(), ">"),
            ("/tmp/CP2/".to_string(), "cp")
        ]
    );
}

#[test]
fn target_directory_value_all_forms() {
    // `-t DIR`, `--target-directory DIR`, `--target-directory=DIR`, and absent.
    assert_eq!(target_directory_value(&["-t", "/d", "a"]), Some("/d"));
    assert_eq!(
        target_directory_value(&["--target-directory", "/d", "a"]),
        Some("/d")
    );
    assert_eq!(
        target_directory_value(&["--target-directory=/d", "a"]),
        Some("/d")
    );
    assert_eq!(target_directory_value(&["a", "b"]), None);
    // `-t` at the very end with no value → None (the `get(i+1)` None arm).
    assert_eq!(target_directory_value(&["a", "-t"]), None);
}

#[test]
fn cp_install_without_t_flag_uses_last_operand() {
    // The non-`-t` path of emit_copy_like (last positional is the dest).
    assert_eq!(paths("cp a b /dest"), vec![("/dest".to_string(), "cp")]);
    assert_eq!(
        paths("install -m644 src /etc/conf"),
        vec![("/etc/conf".to_string(), "install")]
    );
    // `-T`/`--no-target-directory` (forces 2-operand) → default last-operand path.
    assert_eq!(
        paths("cp -T src /dest/file"),
        vec![("/dest/file".to_string(), "cp")]
    );
}

#[test]
fn mv_target_dir_when_dir_token_repeats_in_sources() {
    // The `src == dir` skip arm of emit_mv's `-t` path: the `-t` value is excluded
    // from the mv-from sources even though it is also a non-flag positional.
    let got = paths("mv -t /tmp/d /tmp/a /tmp/b");
    // /tmp/d is the destination (mv); a + b are sources; /tmp/d is NOT a source.
    assert!(got.contains(&("/tmp/d".to_string(), "mv")));
    assert!(got.contains(&("/tmp/a".to_string(), "mv-from")));
    assert!(got.contains(&("/tmp/b".to_string(), "mv-from")));
    assert_eq!(
        got.iter().filter(|(p, _)| p == "/tmp/d").count(),
        1,
        "the -t DIR appears exactly once (as mv), never as a source: {got:?}"
    );
}

// ── tar archive creation recall ──

#[test]
fn tar_create_emits_archive_dest() {
    // `-czf <archive>` (dashed bundle, spaced archive).
    assert_eq!(
        paths("tar -czf /tmp/arch.tar.gz src/"),
        vec![("/tmp/arch.tar.gz".to_string(), "tar")]
    );
    // `czf <archive>` (bundle without a leading dash).
    assert_eq!(
        paths("tar czf backup.tar.gz ."),
        vec![("backup.tar.gz".to_string(), "tar")]
    );
    // `-cf <archive>` (no compression flag).
    assert_eq!(
        paths("tar -cf out.tar a b c"),
        vec![("out.tar".to_string(), "tar")]
    );
    // Long-flag inline + spaced forms.
    assert_eq!(
        paths("tar --create --file=/tmp/x.tar dir/"),
        vec![("/tmp/x.tar".to_string(), "tar")]
    );
    assert_eq!(
        paths("tar --create --file /tmp/y.tar dir/"),
        vec![("/tmp/y.tar".to_string(), "tar")]
    );
    // A glued archive (`-czfARCHIVE`).
    assert_eq!(
        paths("tar -czf/tmp/glued.tgz src/"),
        vec![("/tmp/glued.tgz".to_string(), "tar")]
    );
}

#[test]
fn tar_extract_or_list_writes_nothing() {
    // No create flag → no archive is written, so nothing is emitted.
    assert!(just_paths("tar -xzf /tmp/arch.tar.gz").is_empty());
    assert!(just_paths("tar -tzf /tmp/arch.tar.gz").is_empty());
    assert!(just_paths("tar tf archive.tar").is_empty());
}

#[test]
fn tar_create_to_stdout_emits_nothing() {
    // A create bundle with NO `f` (archive → stdout, e.g. piped) writes no named file.
    assert!(just_paths("tar cz src/").is_empty());
    // `--create` long flag with no `--file` likewise has no destination to emit.
    assert!(just_paths("tar --create --gzip dir/").is_empty());
}

#[test]
fn tar_file_without_create_writes_nothing() {
    // `-f <archive>` but NO create flag (`-rf` append is not a create) → no emit, since
    // `has_create` is false.
    assert!(just_paths("tar -rf /tmp/x.tar extra").is_empty());
    // A spaced `--file <archive>` with no `--create` likewise emits nothing.
    assert!(just_paths("tar --list --file /tmp/x.tar").is_empty());
}
