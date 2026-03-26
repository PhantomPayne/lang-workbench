//! `lw-demo` — End-to-end demo pipeline for lang-workbench.
//!
//! This crate demonstrates a complete parse pipeline for a tiny expression
//! language supporting let-bindings, binary expressions, and imports:
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
//! The pipeline is:
//!
//! ```text
//! source text → lexer → parser → CstArena → lower → Ast (typed arenas)
//!                                                  ↓
//!                                            resolver (cross-file)
//!                                                  ↓
//!                                            diagnostics (Salsa)
//! ```

pub mod lexer;
pub mod lower;
pub mod parser;
pub mod resolver;
pub mod salsa_db;
