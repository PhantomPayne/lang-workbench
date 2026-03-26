//! `lw-cst` — Concrete Syntax Tree for lang-workbench.
//!
//! This crate provides a **lossless, full-fidelity** CST: every byte of the
//! source, including whitespace, comments, and error nodes, is represented in
//! the tree. This fidelity is essential for LSP features such as formatting,
//! range highlighting, and accurate diagnostics.
//!
//! Nodes are arena-allocated using [`bumpalo::Bump`], giving O(1) allocation
//! and zero-cost bulk deallocation when the arena is dropped. The arena is
//! WASM-safe.

use bumpalo::Bump;

/// Arena that owns all CST nodes for a single parse.
///
/// Allocate nodes with the bump allocator and store handles (indices or
/// references) into this arena. Drop the arena to free all nodes at once.
pub struct CstArena {
    bump: Bump,
}

impl Default for CstArena {
    fn default() -> Self {
        Self::new()
    }
}

impl CstArena {
    /// Creates a new, empty CST arena.
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    /// Returns a reference to the underlying bump allocator.
    pub fn bump(&self) -> &Bump {
        &self.bump
    }
}

/// A node in the Concrete Syntax Tree.
///
/// This is a placeholder enum. Future variants will cover all syntactic
/// constructs of the language, including trivia (whitespace/comments) and
/// error recovery nodes so that the tree is always complete even for invalid
/// input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CstNode {
    /// The top-level root node of a source file.
    Root,
}
