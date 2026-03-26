//! `lw-wasm` — Direct JavaScript/TypeScript API for lang-workbench.
//!
//! This crate is **WASM-only**. It exposes a high-level [`Compiler`] object
//! directly to JavaScript via `wasm-bindgen`, without going through the LSP
//! wire protocol. Use this for:
//!
//! * The visual Storybook test harness (inspecting tokens, AST, diagnostics)
//! * Embedding a REPL or playground in a web page
//! * Custom tooling that needs direct access to compiler internals
//!
//! For full LSP protocol support in the browser, see `lw-lsp-wasm` instead.

use wasm_bindgen::prelude::*;

use lw_lsp_core::LanguageService;

/// WASM-exported compiler facade.
///
/// Wraps all core crates (`lw-vfs`, `lw-cst`, `lw-ast`, `lw-analysis`,
/// `lw-lsp-core`) behind a simple JS-friendly API. All methods that return
/// structured data serialise to JSON strings for easy consumption from JS/TS.
#[wasm_bindgen]
pub struct Compiler {
    service: LanguageService,
}

#[wasm_bindgen]
impl Compiler {
    /// Creates a new `Compiler` instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Compiler {
        Compiler {
            service: LanguageService::new(),
        }
    }

    /// Opens (or replaces) a file in the VFS with the given source text.
    pub fn open_file(&mut self, path: &str, text: &str) {
        self.service.vfs.open(path, text);
    }

    /// Parses the given file and returns a stub JSON representation of the
    /// parse result.
    ///
    /// Stub implementation — always returns `"{}"`.
    pub fn parse(&self, _path: &str) -> String {
        // TODO: drive lw-cst / lw-ast parsing and serialise the result.
        String::from("{}")
    }

    /// Returns a JSON array of diagnostics for the given file.
    ///
    /// Stub implementation — always returns `"[]"`.
    pub fn get_diagnostics(&self, path: &str) -> String {
        let diags = self.service.get_diagnostics(path);
        if diags.is_empty() {
            String::from("[]")
        } else {
            // TODO: serialise diagnostics properly via serde_json.
            String::from("[]")
        }
    }
}
