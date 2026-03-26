//! `lw-vfs` — Virtual File System layer for lang-workbench.
//!
//! Provides an in-memory, concurrent VFS backed by [`dashmap::DashMap`] and
//! [`ropey::Rope`]. The VFS is the single source of truth for all open file
//! contents. It is WASM-safe: no threading primitives that require an OS, and
//! no `std`-only I/O.
//!
//! All compiler phases (parsing, analysis, LSP) read from the VFS rather than
//! from the real filesystem, enabling clean incremental updates via
//! `apply_change`.

use dashmap::DashMap;
use ropey::Rope;

/// An in-memory, concurrent virtual file system.
///
/// Files are keyed by path strings and stored as [`Rope`]s, which support
/// efficient incremental edits at any granularity (character, line, byte).
pub struct Vfs {
    files: DashMap<String, Rope>,
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs {
    /// Creates a new, empty VFS.
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
        }
    }

    /// Opens (or replaces) a file with the given full text.
    pub fn open(&self, path: impl Into<String>, text: &str) {
        self.files.insert(path.into(), Rope::from_str(text));
    }

    /// Closes a file, removing it from the VFS.
    pub fn close(&self, path: &str) {
        self.files.remove(path);
    }

    /// Applies an incremental text change to an open file.
    ///
    /// `range` is a pair of `(start_char, end_char)` character offsets.
    /// The text between those offsets is replaced with `new_text`.
    ///
    /// Returns `false` if the file is not open.
    pub fn apply_change(&self, path: &str, range: (usize, usize), new_text: &str) -> bool {
        let Some(mut rope) = self.files.get_mut(path) else {
            return false;
        };
        let (start, end) = range;
        rope.remove(start..end);
        rope.insert(start, new_text);
        true
    }

    /// Returns a clone of the [`Rope`] for an open file, or `None` if the
    /// file is not open.
    pub fn read(&self, path: &str) -> Option<Rope> {
        self.files.get(path).map(|r| r.clone())
    }
}
