use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A throwaway profile directory under [`std::env::temp_dir`], removed when the
/// test that made it ends.
///
/// # Why not a `tempfile` dependency
///
/// The workspace has none, and `app` is a production binary crate: a
/// dev-dependency here would be a crate the release build's lockfile carries in
/// order to run one test. `infra-store-fs` makes the same trade for the same
/// reason, and this is deliberately the same forty lines — a unique name and a
/// `Drop`. The name is the test's label plus the process id plus a counter,
/// which is unique across concurrently running test binaries as well as across
/// threads in one, and the `Drop` removes the tree even when the test panicked.
///
/// Cleanup is best effort. It cannot fail a passing test, and it must not mask
/// the assertion that failed a failing one.
pub(crate) struct TestDir {
    path: PathBuf,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

impl TestDir {
    /// A fresh empty directory named after `label`.
    pub(crate) fn new(label: &str) -> Self {
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "distro-app-{label}-{}-{unique}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a test needs its temp directory");

        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
