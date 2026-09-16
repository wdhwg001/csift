//! The `api_error` leaf: what its head and body say, and the one field that reaches
//! no output path on either road - the modeled leaf's named fields and the unmodeled
//! subtype's key=value excerpt.

use super::*;

/// THE HEADERS LAW. An `api_error`'s response headers carry a session cookie, an
/// organization id and a CDN trace id. They reach no rendered or JSON output path on any
/// surface; `--raw` prints the line and is the one documented way to the bytes.
#[test]
fn api_error_response_headers_never_reach_an_output_path() {
    let h = system_home();
    const SECRETS: [&str; 3] = ["zzsession=deadbeef", "org-zzsecret", "zzray-syd"];
    let t = at(SESS);
    let runs: [&[&str]; 5] = [
        &["search", "", &t, "-t", "harness.system.api-error"],
        &[
            "search",
            "",
            &t,
            "-t",
            "harness.system.api-error",
            "--no-truncate",
        ],
        &[
            "search",
            "",
            &t,
            "-t",
            "harness.system.api-error",
            "--format",
            "json",
        ],
        &["show", &t, "--line", "4"],
        &["show", &t, "--line", "4", "--format", "json"],
    ];
    for argv in runs {
        let out = h.run(argv);
        assert!(out.success, "{argv:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("api-error") || out.stdout.contains("api_error"),
            "{argv:?} must still render the record:\n{}",
            out.stdout
        );
        for s in SECRETS {
            assert!(
                !out.stdout.contains(s),
                "{argv:?} leaked {s:?}:\n{}",
                out.stdout
            );
        }
        assert!(
            !out.stdout.contains("\"headers\""),
            "{argv:?} leaked the headers object:\n{}",
            out.stdout
        );
    }
    // The JSON `error` fact carries the two descriptive fields and nothing else - read
    // off the HEADER-BEARING record, which is the one this law is about.
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.api-error",
        "--format",
        "json",
    ]);
    let hit = hits_of(&js.stdout)
        .into_iter()
        .find(|hh| hh["line"] == serde_json::json!(4))
        .expect("the HTTP-shape hit");
    assert_eq!(hit["status"], serde_json::json!(529));
    assert_eq!(hit["retry_attempt"], serde_json::json!(3));
    assert_eq!(hit["max_retries"], serde_json::json!(20));
    assert_eq!(hit["retry_in_ms"], serde_json::json!(1522.5));
    assert_eq!(
        hit["error"],
        serde_json::json!({"type":"overloaded_error","message":"Overloaded zzoverload"})
    );
    // The DOCUMENTED exception: `--raw` prints the source line, headers and all.
    let raw = h.run(&["show", &at(SESS), "--line", "4", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    for s in SECRETS {
        assert!(
            raw.stdout.contains(s),
            "--raw is the escape hatch and prints the line whole; {s:?} missing:\n{}",
            raw.stdout
        );
    }
}

/// What an api_error SAYS failed is SEARCHABLE - on BOTH observed shapes. The record
/// carries no `content`, so the failure's own wording is the body and its kind is in the
/// head; a reader who searches for either reaches the record that holds it, and the two
/// shapes keep those values in different places.
#[test]
fn the_api_error_failure_text_is_searchable() {
    let h = system_home();
    for pattern in [
        "Overloaded zzoverload", // the HTTP shape's wording, two hops down its chain
        "overloaded_error",      // its kind, which rides the head
        "Connection error zzconn", // the connection shape's wording, at the top level
    ] {
        let out = h.run(&[
            "search",
            pattern,
            &at(SESS),
            "-t",
            "harness.system.api-error",
            "-c",
        ]);
        assert!(out.success, "{pattern} stderr: {}", out.stderr);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "{pattern} should reach the api_error record:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
}

/// THE TWO MEASURED SHAPES, each rendered from where IT puts its fields.
///
/// The HTTP shape names the kind in `error.type` and keeps the response body it received,
/// so its wording is two hops down the `error` chain; the connection shape keeps its
/// wording at the top of that object and names no kind anywhere. Reading one level renders
/// the kind and silently drops the sentence that says what failed, which is exactly what a
/// reader of a failed request is looking for.
#[test]
fn both_measured_api_error_shapes_render_their_kind_and_their_wording() {
    let h = system_home();
    let http = h.run(&["show", &at(SESS), "--line", "4"]);
    assert!(http.success, "stderr: {}", http.stderr);
    assert!(
        http.stdout.contains(
            "[api-error status=529 type=overloaded_error retry 3/20 in 1522.5ms] \
             Overloaded zzoverload"
        ),
        "the HTTP shape:\n{}",
        http.stdout
    );
    // The envelope's own `type` is one hop down and is the word `error`, so naming the
    // first `type` on the chain would report it as the failure kind.
    assert!(
        !http.stdout.contains("type=error"),
        "the envelope word is never the kind:\n{}",
        http.stdout
    );
    let conn = h.run(&["show", &at(SESS), "--line", "13"]);
    assert!(conn.success, "stderr: {}", conn.stderr);
    assert!(
        conn.stdout
            .contains("[api-error status=429 retry 1/10 in 600ms] Connection error zzconn"),
        "the connection shape, which names no kind to print:\n{}",
        conn.stdout
    );
    // Each hit's `error` key names exactly what its own text named.
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.api-error",
        "--format",
        "json",
    ]);
    let hits = hits_of(&js.stdout);
    let by_line = |n: u64| {
        hits.iter()
            .find(|hh| hh["line"] == serde_json::json!(n))
            .unwrap_or_else(|| panic!("a hit on line {n}"))
            .clone()
    };
    assert_eq!(
        by_line(4)["error"],
        serde_json::json!({"type":"overloaded_error","message":"Overloaded zzoverload"})
    );
    assert_eq!(
        by_line(13)["error"],
        serde_json::json!({"message":"Connection error zzconn"})
    );
    assert_eq!(by_line(13)["status"], serde_json::json!(429));
}

/// THE HEADERS LAW HOLDS FOR A SUBTYPE CSIFT MODELS NO LEAF FOR.
///
/// The five modeled event kinds render named fields, so their headers never reach a
/// render by construction. A subtype the taxonomy does not name takes the OTHER road -
/// the 1:1 law's unmodeled unit, whose key=value excerpt walks the line's own values
/// without knowing what they are - and 48 subtype names ship against 11 observed, so a
/// new one is the expected case rather than a hypothetical. Every `headers` member is
/// elided there at every depth, and `--raw` stays the one way to the bytes.
#[test]
fn an_unmodeled_system_subtype_elides_its_headers_too() {
    let h = Home::new();
    let sess = "7e7e7e7e-8f8f-4a4a-8b8b-9c9c9c9c9c9c";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzask"}}"#, "\n",
            r#"{"type":"system","subtype":"api_error_v2","uuid":"x1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","error":{"type":"overloaded_error","headers":{"set-cookie":"zzsession=deadbeef","anthropic-organization-id":"org-zzsecret"}}}"#, "\n",
        ),
    );
    const SECRETS: [&str; 2] = ["zzsession=deadbeef", "org-zzsecret"];
    let t = at(sess);
    let runs: [&[&str]; 2] = [
        &["show", &t, "--line", "2"],
        &["show", &t, "--line", "2", "--format", "json"],
    ];
    for argv in runs {
        let out = h.run(argv);
        assert!(out.success, "{argv:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("api_error_v2"),
            "{argv:?} must still name the shape it cannot model:\n{}",
            out.stdout
        );
        for s in SECRETS {
            assert!(
                !out.stdout.contains(s),
                "{argv:?} leaked {s:?}:\n{}",
                out.stdout
            );
        }
        assert!(
            out.stdout.contains("elided"),
            "{argv:?} states that the set is there rather than dropping it:\n{}",
            out.stdout
        );
    }
    // The DOCUMENTED exception, on this road as on the modeled one.
    let raw = h.run(&["show", &t, "--line", "2", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    for s in SECRETS {
        assert!(
            raw.stdout.contains(s),
            "{s:?} missing from --raw:\n{}",
            raw.stdout
        );
    }
}
