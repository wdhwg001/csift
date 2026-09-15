//! The BINDING facts one transcript's own records carry, beyond the binding itself.
//!
//! `plan --audit` reports which plan file a session binds. These four facts say how much
//! that binding is worth, and each of them is a fact ON DISK rather than a derivation:
//!
//! - **the slug and its change points** - the binding key is one stable value per session,
//!   and the point where it appears is the point the binding came into being. A fork strips
//!   the slug from the records it copies and a first own compaction can mint a fresh one, so
//!   the change points say WHEN this transcript became bound to what `plan` reports.
//! - **whether the bound plan file exists** - the plan name is minted at Plan-Mode entry and
//!   the `.md` file lands only when content is first written (claim PLAN-014), so a binding
//!   to a file that is not there is an ordinary state, not a fault.
//! - **plan text held without a binding** - a `plan_file_reference` attachment carries the
//!   bound plan's whole content back after a compaction (claim PLAN-016). A transcript
//!   holding one while NO record carries a slug is holding plan text nothing will re-inject.
//! - **the slug against the plan file's birth instant** - which came first, the binding or
//!   the file. Not every platform records a birth time, and where it does not the answer is
//!   `unknown` rather than a guess.

use super::*;

/// Where a transcript's `slug` field changed, in file order. `from`/`to` are `None` for
/// absent, so the mint of a slug reads `from: None`.
#[derive(Debug, Clone)]
pub(crate) struct SlugChange {
    pub(crate) line: usize,
    pub(crate) from: Option<String>,
    pub(crate) to: Option<String>,
}

/// One transcript's binding facts.
#[derive(Debug, Default)]
pub(crate) struct BindingFacts {
    /// Every change point, in file order. A transcript whose slug is minted once and never
    /// moves carries exactly one, with `from: None`.
    pub(crate) changes: Vec<SlugChange>,
    /// The first slug-carrying record's line and instant.
    pub(crate) first_slug_line: Option<usize>,
    pub(crate) first_slug_utc: Option<String>,
    /// The lines carrying a `plan_file_reference` attachment (the re-injected plan text).
    pub(crate) plan_ref_lines: Vec<usize>,
}

impl BindingFacts {
    /// True when NO record of this transcript carries a slug - the state in which a
    /// `plan_file_reference` is plan text with nothing bound to it.
    pub(crate) fn no_slug(&self) -> bool {
        self.first_slug_line.is_none()
    }
}

/// The candidate needle for the `plan_file_reference` half of the scan. Rare enough that the
/// few lines carrying it can be parsed properly, which is what makes the check exact: the
/// attachment's own `type` is compared, so a payload that merely spells the word is refused
/// the same way [`super::line_is_plan_candidate`] refuses a prose mention of `plan_mode`.
fn line_mentions_plan_reference(line: &[u8]) -> bool {
    static REF: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"plan_file_reference"));
    REF.find(line).is_some()
}

/// Read one transcript's binding facts in ONE walk.
///
/// The slug half goes through the depth-1 key walk (`parse::lineage_fields`), never a record
/// parse: a slugged transcript carries the key on most of its records, and decoding each of
/// those payloads to read one short string would cost the whole file. A key nested in a
/// payload is therefore not a slug, which is the same rule the record model applies. The
/// `plan_file_reference` half parses only the rare lines carrying that literal, because the
/// attachment's own `type` is what decides it.
pub(crate) fn binding_facts(path: &Path) -> Result<BindingFacts> {
    let mut out = BindingFacts::default();
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(out);
    };
    let bytes: &[u8] = &mmap;
    // The slug state the walk is in. Starts absent, so the first carrier is a change.
    let mut current: Option<String> = None;
    for (idx, line) in bytes.split(|&b| b == b'\n').enumerate() {
        let line_no = idx + 1;
        if line_mentions_plan_reference(line) && is_plan_reference(line) {
            out.plan_ref_lines.push(line_no);
        }
        if !crate::parse::line_has_slug_key(line) {
            continue;
        }
        let Some(f) = crate::parse::lineage_fields(line) else {
            // A line the walk cannot finish says nothing about the slug: it neither mints one
            // nor ends a run, exactly as a line with no key at all.
            continue;
        };
        // Only a line that CARRIES the key at top level can end a run - a record without the
        // key is one the harness stamped before the mint or a bookkeeping line, and neither
        // is the slug going away.
        let Some(slug) = f.slug else {
            continue;
        };
        if out.first_slug_line.is_none() {
            out.first_slug_line = Some(line_no);
            out.first_slug_utc = f.timestamp;
        }
        if current.as_deref() != Some(slug.as_str()) {
            out.changes.push(SlugChange {
                line: line_no,
                from: current.clone(),
                to: Some(slug.clone()),
            });
            current = Some(slug);
        }
    }
    Ok(out)
}

/// Whether one line is a `plan_file_reference` attachment record - the attachment's own
/// `type`, never the byte match that admitted the line.
fn is_plan_reference(line: &[u8]) -> bool {
    let Ok(Some(rec)) = crate::parse::parse_line(line) else {
        return false;
    };
    rec.attachment_value().is_some_and(|att| {
        att.get("type").and_then(serde_json::Value::as_str) == Some("plan_file_reference")
    })
}

/// How the first slug-carrying record sits against the bound plan file's BIRTH instant.
/// `Unknown` is a real answer: a platform that records no birth time cannot be asked, and a
/// plan file that is not on disk has no instant to compare with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlugVsFile {
    /// The slug was carried before the plan file existed - the ordinary order, because the
    /// name is minted at Plan-Mode entry and the file lands when content is first written.
    Before,
    /// The plan file was born first: this transcript bound a plan file that already existed,
    /// which is what a fork or a re-entry into an inherited plan looks like.
    After,
    /// The two instants are equal to the millisecond.
    Same,
    Unknown(&'static str),
}

impl SlugVsFile {
    /// The machine token, and for `Unknown` the reason rides beside it.
    pub(crate) fn token(self) -> &'static str {
        match self {
            SlugVsFile::Before => "before",
            SlugVsFile::After => "after",
            SlugVsFile::Same => "same",
            SlugVsFile::Unknown(_) => "unknown",
        }
    }

    /// Why the comparison could not be made, or `None` when it could.
    pub(crate) fn reason(self) -> Option<&'static str> {
        match self {
            SlugVsFile::Unknown(r) => Some(r),
            _ => None,
        }
    }
}

/// The plan file's birth instant in epoch ms, or the reason there is none. `created()` is the
/// birth time where the platform keeps one and an error where it does not, so the error is
/// reported rather than folded into "no file".
pub(crate) fn plan_file_created_ms(plan_file: &str) -> std::result::Result<i64, &'static str> {
    let md = std::fs::metadata(plan_file).map_err(|_| "the plan file is not on disk")?;
    let created = md
        .created()
        .map_err(|_| "this platform records no file birth time")?;
    let since = created
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "the file's birth time precedes the epoch")?;
    i64::try_from(since.as_millis()).map_err(|_| "the file's birth time is out of range")
}

/// Compare the first slug-carrying record's instant with the plan file's birth instant.
pub(crate) fn slug_vs_plan_file(first_slug_utc: Option<&str>, plan_file: &str) -> SlugVsFile {
    let Some(raw) = first_slug_utc else {
        return SlugVsFile::Unknown("no slug-carrying record has a timestamp");
    };
    let Some(slug_ms) = crate::timez::epoch_ms(raw) else {
        return SlugVsFile::Unknown("the slug record's timestamp is unparseable");
    };
    match plan_file_created_ms(plan_file) {
        Err(why) => SlugVsFile::Unknown(why),
        Ok(file_ms) => match slug_ms.cmp(&file_ms) {
            std::cmp::Ordering::Less => SlugVsFile::Before,
            std::cmp::Ordering::Greater => SlugVsFile::After,
            std::cmp::Ordering::Equal => SlugVsFile::Same,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "csift-planfacts-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn an_empty_transcript_yields_the_default_facts() {
        // A file the mapper cannot map (it is empty) is not an error: nothing is known, and
        // every derived fact says so rather than fabricating an absence.
        let p = tmp("empty.jsonl", "");
        let f = binding_facts(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert!(f.changes.is_empty());
        assert_eq!(f.first_slug_line, None);
        assert!(f.plan_ref_lines.is_empty());
        assert!(f.no_slug());
    }

    #[test]
    fn a_torn_slug_line_neither_mints_nor_ends_a_run() {
        // The walk cannot finish the torn line, so it says nothing about the slug: the run the
        // first line opened is still open, and the third line is no second change point.
        let p = tmp(
            "torn.jsonl",
            concat!(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","slug":"quiet-harbor-relay"}"#,
                "\n",
                r#"{"type":"user","slug":"quiet-harbor-relay","message":{"role":"us"#,
                "\n",
                r#"{"type":"user","timestamp":"2026-06-07T05:02:00.000Z","slug":"quiet-harbor-relay"}"#,
                "\n",
            ),
        );
        let f = binding_facts(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(f.changes.len(), 1, "one mint, no spurious second change");
        assert_eq!(f.changes[0].line, 1);
        assert_eq!(f.first_slug_line, Some(1));
    }

    #[test]
    fn the_plan_reference_needle_is_selective_on_its_own() {
        // The needle is pinned DIRECTLY rather than through the `&&` it guards. With the exact
        // check behind it, a needle that admitted every line would still give the right answer
        // and only cost a parse per line, so no consumer assertion can tell the two apart -
        // only a direct one can. The job it does is real: it is what keeps the audit from
        // running a record parse over every line of a 700 MB transcript.
        assert!(line_mentions_plan_reference(
            br#"{"attachment":{"type":"plan_file_reference"}}"#
        ));
        assert!(
            !line_mentions_plan_reference(
                br#"{"type":"user","message":{"role":"user","content":"hi"}}"#
            ),
            "an ordinary record must be skipped before the parse"
        );
        assert!(
            !line_mentions_plan_reference(br#"{"attachment":{"type":"plan_mode"}}"#),
            "the OTHER plan attachment is not this one"
        );
    }

    #[test]
    fn a_torn_line_carrying_the_attachment_literal_is_not_a_plan_reference() {
        // The literal admitted the line; the attachment's own `type` is what decides, and a
        // line that does not parse has none.
        let p = tmp(
            "tornref.jsonl",
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"plan_file_reference\",\"conte\n",
        );
        let f = binding_facts(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert!(f.plan_ref_lines.is_empty());
    }

    #[test]
    fn an_unparseable_slug_timestamp_reads_unknown_with_its_reason() {
        let v = slug_vs_plan_file(Some("not-a-time"), "/nonexistent/plan.md");
        assert_eq!(v.token(), "unknown");
        assert_eq!(
            v.reason(),
            Some("the slug record's timestamp is unparseable")
        );
    }

    #[test]
    fn an_equal_instant_reads_same() {
        // Derived, not hardcoded: read the file's OWN birth instant back and feed it in, so
        // the equality arm is exercised without depending on a clock. A platform that records
        // no birth time (a musl target: `created()` errors there) cannot reach that arm, and
        // its answer is Unknown with that reason - the claim the fact makes about such a host.
        let p = tmp("same.md", "# the plan\n");
        let path = p.to_str().unwrap().to_string();
        match plan_file_created_ms(&path) {
            Ok(created) => {
                let iso = jiff::Timestamp::from_millisecond(created)
                    .expect("an in-range instant")
                    .to_string();
                let v = slug_vs_plan_file(Some(&iso), &path);
                std::fs::remove_file(&p).ok();
                assert_eq!(v, SlugVsFile::Same, "iso {iso} vs created {created}");
                assert_eq!(v.token(), "same");
                assert_eq!(v.reason(), None);
            }
            Err(why) => {
                let v = slug_vs_plan_file(Some("2020-06-07T05:00:00.000Z"), &path);
                std::fs::remove_file(&p).ok();
                assert_eq!(why, "this platform records no file birth time");
                assert_eq!(v, SlugVsFile::Unknown(why));
                assert_eq!(v.token(), "unknown");
                assert_eq!(v.reason(), Some(why));
            }
        }
    }
}
