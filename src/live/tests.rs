//! Unit tests for the live engine: shared fixtures + feature modules.

use super::*;

mod background;
mod background_scan;
mod conditions;
mod surfaces;
mod verdict;

// ── The assess() join matrix (pure - every arm reachable without fs) ──

fn shape(unreturned: Option<(&str, &str)>, stop: Option<&str>, seen: usize) -> TailShape {
    TailShape {
        unreturned_use: unreturned.map(|(t, ts)| (t.to_string(), Some(ts.to_string()))),
        last_stop_reason: stop.map(str::to_string),
        last_ts_utc: Some("2026-06-07T05:00:05Z".to_string()),
        records_seen: seen,
    }
}

fn row(status: &str, pid: Option<u32>) -> RegistryRow {
    RegistryRow {
        pid,
        status: Some(status.to_string()),
        status_updated_at_ms: Some(1_767_000_000_000),
        proc_start: None,
    }
}

fn note_text(a: &Assessment) -> String {
    a.notes.join(" | ")
}

// ── tail_shape against real files ──

struct TempJsonl(std::path::PathBuf);

impl TempJsonl {
    fn new(content: &str) -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("csift-live-{}-{n}.jsonl", std::process::id()));
        std::fs::write(&p, content).unwrap();
        TempJsonl(p)
    }
}

impl Drop for TempJsonl {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ── Background-task fixtures (shared by background + background_scan) ──

/// A temp session tree: `<root>/<uuid>.jsonl` + `<root>/<uuid>/subagents/agent-x.jsonl`.
pub(super) struct TempSession {
    pub(super) root: std::path::PathBuf,
    pub(super) main: std::path::PathBuf,
}

pub(super) const UUID: &str = "5a4b3c2d-1e0f-4a9b-8c7d-6e5f4a3b2c1d";

impl TempSession {
    pub(super) fn new(main: &str, sub: Option<&str>) -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("csift-bg-{}-{n}", std::process::id()));
        std::fs::create_dir_all(root.join(UUID).join("subagents")).unwrap();
        let main_p = root.join(format!("{UUID}.jsonl"));
        std::fs::write(&main_p, main).unwrap();
        if let Some(s) = sub {
            std::fs::write(
                root.join(UUID)
                    .join("subagents")
                    .join("agent-abcdef0123456789.jsonl"),
                s,
            )
            .unwrap();
        }
        TempSession { root, main: main_p }
    }

    pub(super) fn sub_path(&self) -> std::path::PathBuf {
        self.root
            .join(UUID)
            .join("subagents")
            .join("agent-abcdef0123456789.jsonl")
    }
}

impl Drop for TempSession {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) const LAUNCH: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm run dev","description":"Serve the harbor app","run_in_background":true}}]}}"#;
pub(super) const LAUNCH_RESULT: &str = r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command running in background with ID: b1a2b3c4d. Output is being written to: /nonexistent/b1a2b3c4d.output. You will be notified when it completes."}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"backgroundTaskId":"b1a2b3c4d"}}"#;
pub(super) const EOT: &str = r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"serving"}]}}"#;

pub(super) fn lines(parts: &[&str]) -> String {
    let mut s = parts.join("\n");
    s.push('\n');
    s
}

pub(super) fn report(t: &TempSession, lens: &BackgroundLens) -> BackgroundReport {
    background_report(&t.main, true, lens).unwrap()
}
