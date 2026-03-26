//! `lw-lsp-native` — native stdio/TCP transport for the lang-workbench LSP.
//!
//! This crate is **native-only** — it should never be compiled to WASM or
//! added to WASM CI. It wires [`lw_lsp_core::LanguageService`] up to a
//! `tower-lsp` + `tokio` server that communicates over stdio (the standard
//! LSP editor integration channel).
//!
//! For browser/WASM LSP, see `lw-lsp-wasm` instead.

use std::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use lw_lsp_core::LanguageService;

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

/// The `tower-lsp` backend.  Wraps [`LanguageService`] and implements the
/// LSP protocol methods required by `tower_lsp::LanguageServer`.
struct Backend {
    _client: Client,
    _service: Mutex<LanguageService>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            _client: client,
            _service: Mutex::new(LanguageService::new()),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {}

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
