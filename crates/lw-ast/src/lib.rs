//! `lw-ast` — Abstract Syntax Tree for lang-workbench.
//!
//! # Design: Typed Arenas
//!
//! The AST uses **typed arena indices** rather than a flat untyped ID space.
//! Each node category (expressions, statements) lives in its own
//! [`la_arena::Arena`], and references between nodes carry their type in the
//! index type itself:
//!
//! - [`ExprId`] (`Idx<Expr>`) — can only point at an [`Expr`]
//! - [`StmtId`] (`Idx<Stmt>`) — can only point at a [`Stmt`]
//!
//! This means `BinaryExpr { lhs: ExprId, rhs: ExprId }` is **statically
//! guaranteed** to reference expressions, not arbitrary nodes.  The compiler
//! rejects code that tries to stick a `StmtId` where an `ExprId` is expected.
//!
//! ## Why not a flat columnar/SoA layout?
//!
//! A flat `Vec<AstNodeKind>` with `u32` indices is simpler, but:
//!
//! 1. **No type safety** — a `u32` can index into any column, so
//!    `BinaryExpr { lhs: u32 }` could silently point at a `Root` node.
//! 2. **Error-prone** — lowering must use placeholder-then-patch patterns,
//!    which can create self-referential cycles on failure.
//! 3. **Harder to extend** — adding new node categories (types, patterns)
//!    pollutes a single enum rather than composing separate arenas.
//!
//! The typed-arena approach (used by rust-analyzer) avoids all three issues
//! while remaining cache-friendly (each arena is a contiguous `Vec`).
//!
//! ## Error recovery
//!
//! [`Expr::Missing`] and [`Stmt::Error`] represent nodes that failed to parse
//! or lower.  They participate in the tree without creating cycles.
//!
//! The storage is WASM-safe: no OS threads, no allocator tricks, just `Vec`s
//! behind `Arena<T>`.

use la_arena::{Arena, ArenaMap, Idx};

pub use lw_cst::{BinOp, TextRange};

// ---------------------------------------------------------------------------
// Typed indices
// ---------------------------------------------------------------------------

/// A typed index into the expression arena.
pub type ExprId = Idx<Expr>;

/// A typed index into the statement arena.
pub type StmtId = Idx<Stmt>;

// ---------------------------------------------------------------------------
// Expression nodes
// ---------------------------------------------------------------------------

/// A source-level expression.
///
/// Every variant that references sub-expressions does so via [`ExprId`],
/// ensuring at the type level that only expressions can appear as children
/// of an expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A binary operation: `lhs op rhs`.
    Binary { op: BinOp, lhs: ExprId, rhs: ExprId },
    /// An integer literal (e.g. `42`).
    IntLiteral(i64),
    /// An identifier reference (e.g. `pi`).
    Identifier(String),
    /// A placeholder for an expression that could not be parsed or lowered.
    ///
    /// This is always safe to construct because it has no children — it cannot
    /// create reference cycles.
    Missing,
}

// ---------------------------------------------------------------------------
// Statement nodes
// ---------------------------------------------------------------------------

/// A source-level statement.
///
/// Statements that contain expressions reference them via [`ExprId`],
/// maintaining the typed boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// A `let <name> = <expr>` binding.
    Let { name: String, value: ExprId },
    /// An `import "<path>"` declaration.
    Import { path: String },
    /// A placeholder for a statement that could not be parsed or lowered.
    Error,
}

// ---------------------------------------------------------------------------
// Ast — the complete tree for one source file
// ---------------------------------------------------------------------------

/// The complete AST for a single source file.
///
/// Expressions and statements live in separate typed arenas.  Source spans are
/// stored in parallel [`ArenaMap`]s so that the core node types remain small
/// and cache-friendly, while span information is available when needed (e.g.
/// for diagnostics and hover).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ast {
    /// All expressions in this file.
    pub exprs: Arena<Expr>,
    /// All statements in this file.
    pub stmts: Arena<Stmt>,
    /// The ordered list of top-level statements (execution order).
    pub top_level: Vec<StmtId>,
    /// Source spans for expressions, keyed by [`ExprId`].
    pub expr_ranges: ArenaMap<ExprId, TextRange>,
    /// Source spans for statements, keyed by [`StmtId`].
    pub stmt_ranges: ArenaMap<StmtId, TextRange>,
}

impl Ast {
    /// Creates a new, empty AST.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate an expression and record its source span.
    pub fn alloc_expr(&mut self, expr: Expr, range: TextRange) -> ExprId {
        let id = self.exprs.alloc(expr);
        self.expr_ranges.insert(id, range);
        id
    }

    /// Allocate a statement and record its source span.
    pub fn alloc_stmt(&mut self, stmt: Stmt, range: TextRange) -> StmtId {
        let id = self.stmts.alloc(stmt);
        self.stmt_ranges.insert(id, range);
        id
    }

    /// Look up an expression by ID.
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id]
    }

    /// Look up a statement by ID.
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id]
    }

    /// Get the source span of an expression.
    pub fn expr_range(&self, id: ExprId) -> Option<&TextRange> {
        self.expr_ranges.get(id)
    }

    /// Get the source span of a statement.
    pub fn stmt_range(&self, id: StmtId) -> Option<&TextRange> {
        self.stmt_ranges.get(id)
    }
}
