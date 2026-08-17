use super::*;

/// A minimal [`ImageRef`] carrying only what `ext()` consults.
fn media_ref(mt: &str) -> ImageRef {
    ImageRef {
        session_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
        is_subagent: false,
        parent_session_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
        line_no: 1,
        img_index: 1,
        seq: None,
        fingerprint: String::new(),
        source_kind: "base64".into(),
        media_type: mt.into(),
        b64_len: 0,
        est_bytes: 0,
        url: None,
        ts_utc: None,
        record_uuid: None,
        data: None,
    }
}

mod part01;
