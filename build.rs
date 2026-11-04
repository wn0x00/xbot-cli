//! Emit XBOT_PAYLOAD_HASH: a content hash over the in-process payload sources
//! (payload/injectee + payload/shim + payload/contracts). The embedded runtime
//! uses it to name the %LOCALAPPDATA%\guanzhu-xbot\payload-<hash> cache dir, so
//! different payload builds never overwrite each other's extracted files.
//! Derived binaries (payload/deploy) are intentionally excluded — they are
//! rebuilt from these sources by scripts/build-payload.ps1.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

fn main() {
    for dir in ["payload/injectee", "payload/shim", "payload/contracts"] {
        println!("cargo:rerun-if-changed={dir}");
    }
    let mut hasher = DefaultHasher::new();
    for dir in ["payload/injectee", "payload/shim", "payload/contracts"] {
        hash_tree(Path::new(dir), &mut hasher);
    }
    println!("cargo:rustc-env=XBOT_PAYLOAD_HASH={:016x}", hasher.finish());
}

fn hash_tree(dir: &Path, hasher: &mut DefaultHasher) {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().collect(),
        Err(_) => return,
    };
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "deploy" {
            continue;
        }
        if path.is_dir() {
            hash_tree(&path, hasher);
        } else {
            name.hash(hasher);
            if let Ok(bytes) = std::fs::read(&path) {
                bytes.hash(hasher);
            }
        }
    }
}
