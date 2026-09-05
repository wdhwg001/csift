use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

mod encode;
mod ids;
mod roots;
mod settings;
mod settings_scopes;
mod settings_tolerance;
mod trap;

static TREE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throwaway directory tree (no external dev-dep), removed on drop. Every fixture path
/// is generated, so nothing about this machine reaches a test literal.
#[derive(Debug)]
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Tree {
        let n = TREE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("csift-settings-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temp tree");
        Tree { root }
    }

    fn write(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, body).expect("write fixture");
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(&path).expect("create dir");
        path
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
