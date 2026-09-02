//! Shared infrastructure for inline unit tests — the one thing DAMP says to
//! DRY (mechanics, not scenarios). Compiled only for `cargo test`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a unique, empty temp directory. `prefix` names the test area (e.g.
/// `gflow-state-test`); pid + a process-wide counter make the path unique
/// across parallel tests without any extra dependency.
pub(crate) fn tmp_dir(prefix: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}
