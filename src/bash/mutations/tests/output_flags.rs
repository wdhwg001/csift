//! Tool output flags that name a written file: curl/wget/dd/zip/report flags.

use super::*;

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
fn output_flag_format_selector_edge_forms() {
    // The inline `--output=<format>` form is dropped just like the spaced one.
    assert!(paths("kubectl get pods --out=wide").is_empty());
    assert!(paths("tool --logfile=none").is_empty());
    // `--output` followed by ANOTHER flag does not consume the flag as a path (and the
    // flag is not skipped) - no phantom row, the next flag is still scannable.
    assert!(paths("tool --output --verbose").is_empty());
    // A format word that nonetheless carries a path shape (slash/extension) is a real file.
    assert_eq!(
        paths("tool --output ./json"),
        vec![("./json".to_string(), "flag-output")]
    );
}

// ── Fix B - curl / wget output flags ──

#[test]
fn curl_dash_o_output_caught() {
    // `curl -s URL -o /tmp/x.json` - the dominant Smain miss (7/7).
    assert_eq!(
        paths("curl -s https://api.example.com/d -o /tmp/x.json"),
        vec![("/tmp/x.json".to_string(), "curl")]
    );
}

#[test]
fn curl_long_output_flag_both_forms() {
    // `--output /tmp/x` (spaced) and `--output=/tmp/x` (inline). The LONG `--output`
    // forms are owned by the generic flag-output scan (verb `flag-output`, NOT
    // double-emitted under `curl`); only the path is load-bearing. Exactly ONE row.
    assert_eq!(
        paths("curl URL --output /tmp/a.json"),
        vec![("/tmp/a.json".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("curl URL --output=/tmp/b.json"),
        vec![("/tmp/b.json".to_string(), "flag-output")]
    );
}

#[test]
fn curl_capital_o_no_path_is_skipped() {
    // `curl -O URL` derives the local name from the URL → no deterministic path.
    assert!(paths("curl -O https://example.com/file.tar.gz").is_empty());
    // `curl -sO https://… /tmp/x` - the bundled `-sO` is not our `-O`-takes-next
    // form (curl's -O takes no path), so no fabricated path either.
    assert!(paths("curl -sO https://example.com/x").is_empty());
}

#[test]
fn wget_capital_o_output_caught() {
    // `wget -O /tmp/x.bin URL` - wget's capital-O DOES take a path.
    assert_eq!(
        paths("wget -O /tmp/x.bin https://example.com/x"),
        vec![("/tmp/x.bin".to_string(), "wget")]
    );
}

#[test]
fn wget_output_document_caught() {
    assert_eq!(
        paths("wget --output-document /tmp/y.bin https://example.com/y"),
        vec![("/tmp/y.bin".to_string(), "wget")]
    );
}

// ── Fix C - flag-specified outputs, dd, zip ──

#[test]
fn junit_xml_flag_both_dashes_caught() {
    // `--junit-xml=/tmp/x.xml` and `--junitxml=/tmp/x.xml` (the two pytest spellings).
    assert_eq!(
        paths("pytest --junit-xml=/tmp/r.xml"),
        vec![("/tmp/r.xml".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("pytest --junitxml=/tmp/r2.xml"),
        vec![("/tmp/r2.xml".to_string(), "flag-output")]
    );
}

#[test]
fn report_path_flag_spaced_caught() {
    // `gitleaks --report-path /tmp/x.json` (spaced value form).
    assert_eq!(
        paths("gitleaks detect --report-path /tmp/leaks.json"),
        vec![("/tmp/leaks.json".to_string(), "flag-output")]
    );
}

#[test]
fn generic_output_flags_caught() {
    // `--output=/tmp/o` under a non-curl/wget verb still resolves via the generic scan.
    assert_eq!(
        paths("sometool --output=/tmp/o.txt"),
        vec![("/tmp/o.txt".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("sometool --logfile /tmp/run.log"),
        vec![("/tmp/run.log".to_string(), "flag-output")]
    );
}

#[test]
fn dd_of_output_caught() {
    // `dd if=/dev/zero of=/tmp/x.bin` - `of=` parsed specially (KEY=VALUE otherwise
    // rejected); `if=` (input) is NOT emitted.
    assert_eq!(
        paths("dd if=/dev/zero of=/tmp/x.bin bs=1M count=4"),
        vec![("/tmp/x.bin".to_string(), "dd")]
    );
}

#[test]
fn dd_of_dev_null_dropped() {
    // `of=/dev/null` is a sink, not a created file.
    assert!(paths("dd if=/tmp/src of=/dev/null").is_empty());
}

#[test]
fn zip_dest_is_first_operand_only() {
    // `zip /tmp/x.zip a b` - only the archive dest, NOT the input members.
    assert_eq!(
        paths("zip /tmp/x.zip a b c"),
        vec![("/tmp/x.zip".to_string(), "zip")]
    );
    // With flags before the dest, the flag is skipped and the first non-flag wins.
    assert_eq!(
        paths("zip -r /tmp/y.zip dir/"),
        vec![("/tmp/y.zip".to_string(), "zip")]
    );
}
