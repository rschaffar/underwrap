use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    // Only fresh, single-test child processes select a private journal sink.
    // No environment-selected destination exists in production binaries.
    pub(crate) static JOURNAL_SOCKET: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}
