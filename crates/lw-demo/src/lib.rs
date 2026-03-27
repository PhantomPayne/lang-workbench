//! `lw-demo` — End-to-end language pipeline for lang-workbench.
//!
//! This crate demonstrates a complete Data-Oriented compiler pipeline for a
//! tiny expression language supporting let-bindings, binary expressions, and
//! imports.
//!
//! ```text
//! // math.lw
//! let pi = 3
//!
//! // main.lw
//! import "math.lw"
//! let area = pi * 10 * 10
//! ```
//!
//! ## Pipeline
//!
//! ```text
//! source text → lex_file → (CstArena, LineIndex)
//!                              ↓
//!                         parser → CstTree (flat Vec<CstNode>)
//!                              ↓
//!                         lower → Ast (flat Vec<Expr> + Vec<Stmt>)
//!                              ↓
//!                         resolver (cross-file)
//!                              ↓
//!                         diagnostics (Salsa)
//! ```
//!
//! All data is stored in flat, contiguous `Vec`s.  No `Box`, `Rc`, `RefCell`,
//! or lifetimes are used to represent syntax trees.  Token text is derived
//! from position in the source string.  Line/column positions are computed
//! on demand via `LineIndex::byte_to_lsp_position`.

pub mod lexer;
pub mod lower;
pub mod parser;
pub mod resolver;
pub mod salsa_db;
