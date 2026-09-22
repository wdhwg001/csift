//! The RENDER as a writer: the unit the ordered-parallel pipeline emits, the one cross-row
//! decision it cannot make on a worker, and what a write error does.
//!
//! A row loop that swallowed a write error would report success on a short stream, which is
//! exactly the silent truncation the output contract forbids - and these are the only
//! callers between a failing terminal and that contract, so the propagation is pinned here
//! rather than inferred from the `?`.

use super::*;

/// A sink that refuses every write - the terminal a closed pipe or a full disk gives.
#[derive(Debug)]
struct Refusing;

impl std::io::Write for Refusing {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("the sink refused"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("the sink refused"))
    }
}

fn hit(class: Class, line: usize, excerpt: &str, image_ids: Vec<String>) -> Hit {
    Hit {
        class: Some(class),
        labels: vec![class.path()],
        excerpt: excerpt.to_string(),
        body: None,
        timestamp_utc: None,
        tool_name: None,
        model: None,
        attachment_type: None,
        version: None,
        is_error: None,
        denial_kind: None,
        direction: None,
        tool_use_id: None,
        pair: None,
        line,
        uuid: None,
        raw: None,
        image_ids,
        from_sidecar: false,
        queue_operation: None,
        queue_reason: None,
        task_ids: Vec::new(),
        orphan_kind: None,
        delivery: None,
        resume_paired: None,
        scheduled_at: None,
        survival: crate::model::Survival::Live,
        rewound_branch: false,
        replay_copy_of: None,
        compaction: None,
        truncated: false,
        for_root: crate::model::RootLink::default(),
        markers: Vec::new(),
        facts: Vec::new(),
    }
}

fn exchange(turn: usize, hits: Vec<Hit>, siblings: Vec<Hit>) -> Exchange {
    Exchange {
        session_id: "11111111-2222-4333-8444-555555555555".to_string(),
        is_subagent: false,
        parent_session_id: "11111111-2222-4333-8444-555555555555".to_string(),
        turn_index: Some(turn),
        started_utc: Some("2026-01-02T03:04:05.000Z".to_string()),
        hits,
        siblings,
        siblings_hidden: 0,
        turn_lines: (0, 0),
        record_uuids: Vec::new(),
        superseded_draft: false,
        abandoned_kind: None,
        abandoned_root_line: None,
        draft_diff: None,
    }
}

fn tokens(exchanges: &[Exchange]) -> HashMap<&str, String> {
    header_tokens(exchanges)
}

#[test]
fn write_exchange_emits_the_header_then_one_line_per_hit_then_the_siblings() {
    // The pipeline's UNIT: a blank separator, the `<tok>·t<N>  <instant>` header, each hit
    // under its role glyph, then the `--siblings` context rows under the dim marker. The
    // glyphs and the turn tag are the documented shape (SPEC 6.2), not a copied run.
    let exchanges = vec![exchange(
        7,
        vec![hit(Class::UserMessage, 12, "hello", Vec::new())],
        vec![hit(Class::AgentMessagePlain, 13, "world", Vec::new())],
    )];
    let tok = tokens(&exchanges);
    let mut buf: Vec<u8> = Vec::new();
    write_exchange(&mut buf, &exchanges[0], &tok, None).expect("a Vec sink cannot fail");
    let text = String::from_utf8(buf).expect("utf8");
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines[0], "", "the block opens with a blank separator");
    assert!(lines[1].starts_with("11111111·t7  "), "got {:?}", lines[1]);
    assert!(
        lines[2].starts_with("  ◂ user.message  L12  hello"),
        "got {:?}",
        lines[2]
    );
    assert!(
        lines[3].starts_with("  · agent.message.plain  L13  world"),
        "got {:?}",
        lines[3]
    );
}

#[test]
fn first_image_row_reads_hits_before_siblings_and_stops_at_the_first() {
    // The ONE cross-row decision the text render makes, and the order it must reproduce:
    // within an exchange the hits are printed before the siblings, and the whole scan stops
    // at the first row carrying image ids - a later one never wins.
    let no_img = || hit(Class::UserMessage, 1, "a", Vec::new());
    let img = |line| hit(Class::UserMessage, line, "a", vec!["#3".to_string()]);

    // A sibling of the FIRST exchange beats a hit of the second.
    let a = vec![
        exchange(0, vec![no_img()], vec![img(9)]),
        exchange(1, vec![img(20)], Vec::new()),
    ];
    assert_eq!(
        first_image_row(&a),
        Some(ImageHint {
            exchange: 0,
            sibling: true,
            row: 0
        })
    );

    // A hit of an exchange beats that same exchange's sibling.
    let b = vec![exchange(0, vec![no_img(), img(9)], vec![img(10)])];
    assert_eq!(
        first_image_row(&b),
        Some(ImageHint {
            exchange: 0,
            sibling: false,
            row: 1
        })
    );

    // No image anywhere is the no-hint answer, which is what suppresses the note.
    let c = vec![exchange(0, vec![no_img()], vec![no_img()])];
    assert_eq!(first_image_row(&c), None);
}

#[test]
fn the_image_hint_prints_under_its_own_row_and_nowhere_else() {
    // The hint is handed to ONE exchange, and inside it to ONE row: the exchange that does
    // not own it prints no hint even though its rows carry image ids, which is what keeps
    // the note singular when the exchanges render on different threads.
    let img = |line| hit(Class::UserMessage, line, "a", vec!["#3".to_string()]);
    let exchanges = vec![
        exchange(0, vec![img(9)], Vec::new()),
        exchange(1, vec![img(20)], Vec::new()),
    ];
    let tok = tokens(&exchanges);
    let hint = first_image_row(&exchanges).expect("an image row");

    let mut owner: Vec<u8> = Vec::new();
    write_exchange(&mut owner, &exchanges[0], &tok, Some(hint)).expect("sink");
    let owner = String::from_utf8(owner).expect("utf8");
    assert!(
        owner.contains("read the image(s): csift image @"),
        "{owner}"
    );

    let mut other: Vec<u8> = Vec::new();
    write_exchange(&mut other, &exchanges[1], &tok, None).expect("sink");
    let other = String::from_utf8(other).expect("utf8");
    assert!(!other.contains("read the image(s)"), "{other}");
}

#[test]
fn every_row_writer_propagates_a_write_error() {
    // Each of the four: a refusing sink must come back as an Err, never as a short write
    // reported as success.
    let h = hit(Class::UserMessage, 1, "x", vec!["#1".to_string()]);
    let ex = exchange(0, vec![h.clone()], vec![h.clone()]);
    let tok = tokens(std::slice::from_ref(&ex));
    let hint = first_image_row(std::slice::from_ref(&ex));

    assert!(write_record_line(&mut Refusing, '◂', &h).is_err());
    assert!(write_exchange(&mut Refusing, &ex, &tok, hint).is_err());
    assert!(write_truncation_caution(&mut Refusing).is_err());

    // The chain disclosure only writes when it HAS something to disclose, so the counts
    // are set: an all-zero `ChainCounts` writes nothing and cannot fail.
    let mut counts = ChainCounts::default();
    assert!(write_chain_disclosure(&mut Refusing, &counts).is_ok());
    counts.drafts = 1;
    counts.rewound_turns = 1;
    counts.abandoned_records = 2;
    counts.replay_copies = 1;
    counts.boundary_cut_line = Some(42);
    counts.leaf_source = Some("tail");
    assert!(write_chain_disclosure(&mut Refusing, &counts).is_err());
}

#[test]
fn the_chain_disclosure_names_each_count_it_has() {
    // Every non-zero count earns one line and an all-zero set earns none, so an ordinary
    // transcript's footer is unchanged and a reader is never left to infer a suppression.
    let mut buf: Vec<u8> = Vec::new();
    write_chain_disclosure(&mut buf, &ChainCounts::default()).expect("sink");
    assert!(buf.is_empty(), "an all-zero disclosure writes nothing");

    let counts = ChainCounts {
        drafts: 3,
        rewound_turns: 1,
        abandoned_records: 7,
        replay_copies: 2,
        boundary_cut_line: Some(42),
        leaf_source: Some("tail"),
        ..ChainCounts::default()
    };
    let mut buf: Vec<u8> = Vec::new();
    write_chain_disclosure(&mut buf, &counts).expect("sink");
    let text = String::from_utf8(buf).expect("utf8");
    assert!(text.contains("3 superseded draft(s)"), "{text}");
    assert!(text.contains("1 rewound turn(s)"), "{text}");
    assert!(
        text.contains("7 record(s) off the surviving conversation"),
        "{text}"
    );
    assert!(text.contains("2 replay copy line(s)"), "{text}");
    assert!(text.contains("compaction boundary L42"), "{text}");
    assert!(text.contains("(chain leaf: tail)"), "{text}");
}
