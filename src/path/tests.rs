use super::*;

use crate::testfs::Scratch;

mod agent_copies;
mod dedup;
mod encode;
mod ids;
mod roots;
mod settings;
mod settings_scopes;
mod settings_tolerance;
mod trap;

/// A throwaway directory tree (no external dev-dep), removed on drop. Every fixture path
/// is generated, so nothing about this machine reaches a test literal.
#[derive(Debug)]
struct Tree {
    root: Scratch,
}

impl Tree {
    fn new() -> Tree {
        Tree {
            root: Scratch::dir("settings"),
        }
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
