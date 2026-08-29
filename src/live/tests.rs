//! Unit tests for the live engine: shared fixtures + feature modules.

use super::*;

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
