//! `lw-analysis` — Incremental analyses for lang-workbench.
//!
//! This crate combines two complementary computation models:
//!
//! * **[Salsa]** handles *when* to recompute: it tracks which query results
//!   depend on which inputs and automatically re-executes only the queries
//!   that are invalidated when an input changes. This gives incremental,
//!   demand-driven compilation for free.
//!
//! * **[Ascent]** handles *what* to compute: Datalog rules written as Rust
//!   macros produce bottom-up fixpoint analyses (reachability, type
//!   constraints, dataflow, etc.) that compose cleanly with Salsa queries.
//!
//! All code in this crate is WASM-safe: no OS threads, no blocking I/O.
//!
//! [Salsa]: https://github.com/salsa-rs/salsa
//! [Ascent]: https://github.com/s-arash/ascent

// ---------------------------------------------------------------------------
// Salsa inputs and queries
// ---------------------------------------------------------------------------

/// A source file tracked by Salsa.
///
/// Create one per open file; set `text` whenever the file contents change.
/// Salsa will automatically re-run any tracked queries that depend on it.
#[salsa::input]
pub struct InputFile {
    /// The raw source text for this file.
    pub text: String,
}

/// Stub parse query: returns a placeholder string representing the parsed
/// output. In a real implementation this would return an interned CST/AST
/// handle.
#[salsa::tracked]
pub fn parse(db: &dyn Db, file: InputFile) -> String {
    let _text = file.text(db);
    // Stub: return a placeholder parse result.
    String::from("{}")
}

// ---------------------------------------------------------------------------
// Salsa database trait and concrete implementation
// ---------------------------------------------------------------------------

/// The Salsa database trait for analysis queries.
///
/// Additional tracked functions and inputs can be added as analysis grows.
#[salsa::db]
pub trait Db: salsa::Database {}

/// The concrete Salsa database that implements all analysis queries.
///
/// Annotated with `#[salsa::db]` so that Salsa can implement the storage and
/// revision-tracking machinery.
#[salsa::db]
#[derive(Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl Db for Database {}
