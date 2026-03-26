//! `lw-cst` — Concrete Syntax Tree for lang-workbench.
//!
//! This crate provides a **lossless, full-fidelity** CST: every byte of the
//! source, including whitespace, comments, and error nodes, is represented in
//! the tree. This fidelity is essential for LSP features such as formatting,
//! range highlighting, and accurate diagnostics.
//!
//! Nodes are stored in a typed [`la_arena::Arena`], giving `O(1)` allocation
//! and stable [`Idx`]-based handles that are `Copy`, serializable, and free
//! from lifetime parameters. The arena is WASM-safe.

use la_arena::{Arena, Idx};

/// A byte-offset range in the source text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// Binary operators supported by the language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// The kind of a literal token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralKind {
    Integer,
    Float,
}

/// A node in the Concrete Syntax Tree.
///
/// All nodes—including trivia (whitespace, comments) and error recovery
/// nodes—are represented here so the tree is always lossless and complete,
/// even for invalid input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CstNode {
    /// The top-level root node of a source file.
    Root { children: Vec<CstNodeId> },
    /// A `let <name> = <expr>` binding.
    LetBinding { name: TextRange, value: CstNodeId },
    /// An `import "<path>"` statement.
    Import { path: TextRange },
    /// A binary expression `<lhs> <op> <rhs>`.
    BinaryExpr {
        op: BinOp,
        lhs: CstNodeId,
        rhs: CstNodeId,
    },
    /// An integer or float literal.
    Literal { kind: LiteralKind, range: TextRange },
    /// An identifier reference.
    Identifier { range: TextRange },
    /// A whitespace trivia node.
    Whitespace { range: TextRange },
    /// A line-comment trivia node (`// …`).
    Comment { range: TextRange },
    /// An error-recovery node for unexpected input.
    Error { range: TextRange, message: String },
}

/// Typed index into a [`CstArena`].
pub type CstNodeId = Idx<CstNode>;

/// Arena that owns all CST nodes for a single parse.
///
/// Allocate nodes with [`CstArena::alloc`] and reference them through the
/// returned [`CstNodeId`] handles. Drop the arena to free all nodes at once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CstArena {
    arena: Arena<CstNode>,
}

impl CstArena {
    /// Creates a new, empty CST arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a node and returns its stable [`CstNodeId`].
    pub fn alloc(&mut self, node: CstNode) -> CstNodeId {
        self.arena.alloc(node)
    }

    /// Returns a reference to the node identified by `id`.
    pub fn get(&self, id: CstNodeId) -> &CstNode {
        &self.arena[id]
    }
}
