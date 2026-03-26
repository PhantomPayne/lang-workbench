//! `lw-lsp-core` — WASM-safe LSP logic layer for lang-workbench.
//!
//! This crate contains all language-server *logic*: diagnostics, hover,
//! completions, go-to-definition, etc. It depends only on [`lsp_types`] for
//! protocol data structures, [`lw_vfs`] for file contents, and
//! [`lw_analysis`] for incremental queries.
//!
//! **No transport, no async runtime, no `tokio`, no `tower`.** This makes the
//! crate safe to compile to `wasm32-unknown-unknown` and reuse verbatim in:
//!
//! * `lw-lsp-native` — wraps this with a `tower-lsp` stdio transport.
//! * `lw-lsp-wasm` — wraps this with a `postMessage` Web Worker transport.

use lsp_types::{Diagnostic, Hover, Position};
use lw_analysis::Database;
use lw_vfs::Vfs;

/// The core language service: stateful, WASM-safe, transport-agnostic.
///
/// Owns a [`Vfs`] and a Salsa [`Database`]. Both transport adapters
/// (`lw-lsp-native`, `lw-lsp-wasm`) hold a `LanguageService` and delegate
/// all semantic work to it.
pub struct LanguageService {
    vfs: Vfs,
    db: Database,
}

impl Default for LanguageService {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageService {
    /// Creates a new `LanguageService` with an empty VFS and database.
    pub fn new() -> Self {
        Self {
            vfs: Vfs::new(),
            db: Database::default(),
        }
    }

    // -----------------------------------------------------------------------
    // VFS delegation
    // -----------------------------------------------------------------------

    /// Opens (or replaces) a file in the VFS with the given source text.
    pub fn open_file(&self, path: &str, text: &str) {
        self.vfs.open(path, text);
    }

    /// Closes a file, removing it from the VFS.
    pub fn close_file(&self, path: &str) {
        self.vfs.close(path);
    }

    /// Applies an incremental text change to an open file.
    ///
    /// See [`lw_vfs::Vfs::apply_change`] for range semantics and return value.
    pub fn apply_change(&self, path: &str, range: (usize, usize), new_text: &str) -> bool {
        self.vfs.apply_change(path, range, new_text)
    }

    // -----------------------------------------------------------------------
    // LSP queries
    // -----------------------------------------------------------------------

    /// Returns a list of diagnostics for the given file path.
    ///
    /// Stub implementation — always returns an empty list.
    pub fn get_diagnostics(&self, _path: &str) -> Vec<Diagnostic> {
        Vec::new()
    }

    /// Returns hover information at `position` in the given file, or `None`
    /// if there is nothing to show.
    ///
    /// Stub implementation — always returns `None`.
    pub fn get_hover(&self, _path: &str, _position: Position) -> Option<Hover> {
        None
    }

    // -----------------------------------------------------------------------
    // Internal access (crate-only)
    // -----------------------------------------------------------------------

    /// Returns a reference to the underlying [`Database`] for use by analysis
    /// code within this crate.
    #[allow(dead_code)]
    fn db(&self) -> &Database {
        &self.db
    }
}
