//! `lw-ast` — Abstract Syntax Tree for lang-workbench.
//!
//! This crate provides a **columnar / Structure-of-Arrays (SoA)** AST layout.
//! Rather than a tree of individually-allocated nodes, all data for nodes of a
//! given kind is stored in parallel `Vec` columns. This improves cache
//! locality for batch analysis passes (type checking, dataflow, etc.) and
//! makes serialization to columnar formats (Parquet, Arrow) straightforward.
//!
//! The arena is allocated via [`bumpalo::Bump`] and is WASM-safe.

use bumpalo::Bump;

/// Arena that owns all AST storage for a single compilation unit.
///
/// In the columnar model the arena itself does not allocate individual nodes;
/// instead it backs the [`AstNodes`] column vectors.
pub struct AstArena {
    bump: Bump,
}

impl Default for AstArena {
    fn default() -> Self {
        Self::new()
    }
}

impl AstArena {
    /// Creates a new, empty AST arena.
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    /// Returns a reference to the underlying bump allocator.
    pub fn bump(&self) -> &Bump {
        &self.bump
    }
}

/// Columnar storage for AST nodes (Structure-of-Arrays layout).
///
/// Each field is a parallel column: `id[i]` and `span_start[i]` / `span_end[i]`
/// all describe the same node `i`. New columns can be added for additional
/// node metadata without changing the node identity encoding.
///
/// This is a placeholder. Future columns will cover node kind, parent index,
/// type annotation, etc.
#[derive(Debug, Default)]
pub struct AstNodes {
    /// Unique numeric identifiers for each node.
    pub id: Vec<u32>,
    /// Start byte offsets of each node's source span.
    pub span_start: Vec<u32>,
    /// End byte offsets of each node's source span.
    pub span_end: Vec<u32>,
}

impl AstNodes {
    /// Creates a new, empty columnar node store.
    pub fn new() -> Self {
        Self::default()
    }
}
