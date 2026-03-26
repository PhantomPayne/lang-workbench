//! `lw-lsp-wasm` — `postMessage` transport shim for running the
//! lang-workbench LSP server inside a browser Web Worker.
//!
//! This crate is **WASM-only**. It wraps [`lw_lsp_core::LanguageService`]
//! with a thin `wasm-bindgen` layer that accepts raw LSP JSON messages
//! (as JavaScript strings) and returns JSON response strings. The JavaScript
//! side is responsible for routing messages between the editor (Monaco /
//! CodeMirror) and this Web Worker via `postMessage`.
//!
//! Architecture:
//! ```text
//! Monaco Editor  ←→  Web Worker JS shim  ←→  LspWorker (this crate, WASM)
//!                      (postMessage)             (lw-lsp-core logic)
//! ```

use wasm_bindgen::prelude::*;

use lw_lsp_core::LanguageService;

/// WASM-exported LSP worker.
///
/// Instantiate once in the Web Worker and call [`handle_message`] for every
/// incoming LSP JSON-RPC message from the editor client.
#[wasm_bindgen]
pub struct LspWorker {
    service: LanguageService,
}

#[wasm_bindgen]
impl LspWorker {
    /// Creates a new `LspWorker`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> LspWorker {
        LspWorker {
            service: LanguageService::new(),
        }
    }

    /// Accepts a raw LSP JSON-RPC message string and returns a response string.
    ///
    /// Stub implementation — always returns `"{}"`.
    pub fn handle_message(&mut self, _msg: &str) -> String {
        // TODO: deserialize _msg as a JSON-RPC request, dispatch to
        // self.service, and serialize the response.
        let _ = &self.service;
        String::from("{}")
    }
}
