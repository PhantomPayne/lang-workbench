//! `lw-ast` — Data-Oriented Abstract Syntax Tree for lang-workbench.
//!
//! # Design: Flat Typed Arrays
//!
//! The AST uses **typed newtype indices** over flat `Vec`s — no `Box`, `Rc`,
//! `RefCell`, or lifetimes.  Each node category (expressions, statements)
//! lives in its own `Vec`, and references between nodes carry their type in
//! the index wrapper:
//!
//! - [`ExprId`] — can only index into `Ast::exprs`
//! - [`StmtId`] — can only index into `Ast::stmts`
//!
//! This means `Binary { lhs: ExprId, rhs: ExprId }` is statically guaranteed
//! to reference expressions, not arbitrary nodes.
//!
//! ## Span storage
//!
//! Source spans are stored in parallel `Vec`s (`expr_spans`, `stmt_spans`)
//! using [`TokenSpan`] — a packed (token-index, count) pair.  No line or
//! column numbers are stored in nodes; LSP positions are computed on demand
//! via [`lw_cst::LineIndex`].
//!
//! ## Error recovery
//!
//! [`Expr::Missing`] and [`Stmt::Error`] represent nodes that failed to parse
//! or lower.  They are leaf nodes with no children, so they cannot create
//! reference cycles.

pub use lw_cst::{BinOp, TokenSpan};

// ---------------------------------------------------------------------------
// Typed indices
// ---------------------------------------------------------------------------

/// A typed index into `Ast::exprs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// A typed index into `Ast::stmts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StmtId(pub u32);

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
/// All data is stored in flat `Vec`s.  Source spans are stored in parallel
/// arrays keyed by the same index, so the core node types remain small and
/// cache-friendly while span information is available when needed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ast {
    /// All expressions in this file (indexed by [`ExprId`]).
    pub exprs: Vec<Expr>,
    /// All statements in this file (indexed by [`StmtId`]).
    pub stmts: Vec<Stmt>,
    /// The ordered list of top-level statements (execution order).
    pub top_level: Vec<StmtId>,
    /// Source spans for expressions, parallel to `exprs`.
    pub expr_spans: Vec<TokenSpan>,
    /// Source spans for statements, parallel to `stmts`.
    pub stmt_spans: Vec<TokenSpan>,
}

impl Ast {
    /// Creates a new, empty AST.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate an expression and record its source span.
    pub fn alloc_expr(&mut self, expr: Expr, span: TokenSpan) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(expr);
        self.expr_spans.push(span);
        id
    }

    /// Allocate a statement and record its source span.
    pub fn alloc_stmt(&mut self, stmt: Stmt, span: TokenSpan) -> StmtId {
        let id = StmtId(self.stmts.len() as u32);
        self.stmts.push(stmt);
        self.stmt_spans.push(span);
        id
    }

    /// Look up an expression by ID.
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    /// Look up a statement by ID.
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id.0 as usize]
    }

    /// Get the source span of an expression.
    pub fn expr_span(&self, id: ExprId) -> &TokenSpan {
        &self.expr_spans[id.0 as usize]
    }

    /// Get the source span of a statement.
    pub fn stmt_span(&self, id: StmtId) -> &TokenSpan {
        &self.stmt_spans[id.0 as usize]
    }

    /// Iterate over all expressions with their IDs.
    pub fn iter_exprs(&self) -> impl Iterator<Item = (ExprId, &Expr)> {
        self.exprs
            .iter()
            .enumerate()
            .map(|(i, e)| (ExprId(i as u32), e))
    }
}
