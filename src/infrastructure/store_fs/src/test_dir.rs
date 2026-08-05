use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A throwaway directory under [`std::env::temp_dir`], removed when the test
/// that made it ends.
///
/// # Why not a `tempfile` dependency
///
/// The workspace has none, and this crate is the only place that would want
/// one. Everything needed here is a unique name and a `Drop`: the name is the
/// test's label plus the process id plus a counter, which is unique across
/// concurrently running test binaries as well as across threads in one, and the
/// `Drop` removes the tree even when the test panicked — a failing test that
/// leaves rubbish in `/tmp` is a second problem to chase.
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
            "distro-store-fs-{label}-{}-{unique}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a test needs its temp directory");

        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside this directory. The file need not exist.
    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // A test that made the directory unwritable to prove a write failure
        // has to hand it back writable, or the tree survives the run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700));
        }

        let _ = fs::remove_dir_all(&self.path);
    }
}
