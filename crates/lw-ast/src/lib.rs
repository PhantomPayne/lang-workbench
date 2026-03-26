//! `lw-ast` — Abstract Syntax Tree for lang-workbench.
//!
//! This crate provides a **columnar / Structure-of-Arrays (SoA)** AST layout.
//! Rather than a tree of individually-allocated nodes, all data for nodes is
//! stored in parallel `Vec` columns. This improves cache locality for batch
//! analysis passes (type checking, dataflow, etc.) and makes serialization to
//! columnar formats (Parquet, Arrow) straightforward.
//!
//! Node identity is a plain [`AstNodeId`] (`u32`) index into the columns.
//! No lifetime parameters, no allocator references — just `Vec`s.
//! The storage is WASM-safe.

pub use lw_cst::{BinOp, TextRange};

/// Unique node identifier — an index into the [`AstNodes`] columns.
pub type AstNodeId = u32;

/// The semantic kind of an AST node.
#[derive(Debug, Clone)]
pub enum AstNodeKind {
    /// The root of a source file.
    Root,
    /// A `let <name> = <value>` binding.
    LetBinding { name: String, value: AstNodeId },
    /// An `import "<path>"` statement.
    Import { path: String },
    /// A binary expression.
    BinaryExpr {
        op: BinOp,
        lhs: AstNodeId,
        rhs: AstNodeId,
    },
    /// An integer literal.
    IntLiteral(i64),
    /// A floating-point literal.  Equality uses bit-level comparison so that
    /// this type can be used as a Salsa query result.
    FloatLiteral(f64),
    /// An identifier reference.
    Identifier(String),
}

impl PartialEq for AstNodeKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Root, Self::Root) => true,
            (
                Self::LetBinding { name: n1, value: v1 },
                Self::LetBinding { name: n2, value: v2 },
            ) => n1 == n2 && v1 == v2,
            (Self::Import { path: p1 }, Self::Import { path: p2 }) => p1 == p2,
            (
                Self::BinaryExpr {
                    op: o1,
                    lhs: l1,
                    rhs: r1,
                },
                Self::BinaryExpr {
                    op: o2,
                    lhs: l2,
                    rhs: r2,
                },
            ) => o1 == o2 && l1 == l2 && r1 == r2,
            (Self::IntLiteral(a), Self::IntLiteral(b)) => a == b,
            (Self::FloatLiteral(a), Self::FloatLiteral(b)) => a.to_bits() == b.to_bits(),
            (Self::Identifier(a), Self::Identifier(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for AstNodeKind {}

/// Columnar AST storage. Each index into the `Vec`s is an [`AstNodeId`].
///
/// Structure-of-Arrays layout for cache efficiency: all node kinds live
/// together, all spans live together, etc.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AstNodes {
    /// The semantic kind of each node.
    pub kinds: Vec<AstNodeKind>,
    /// The source span of each node.
    pub spans: Vec<TextRange>,
    /// The parent node of each node, if any.
    pub parents: Vec<Option<AstNodeId>>,
}

impl AstNodes {
    /// Creates a new, empty columnar node store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a new node and returns its [`AstNodeId`].
    pub fn alloc(
        &mut self,
        kind: AstNodeKind,
        span: TextRange,
        parent: Option<AstNodeId>,
    ) -> AstNodeId {
        let id = self.kinds.len() as AstNodeId;
        self.kinds.push(kind);
        self.spans.push(span);
        self.parents.push(parent);
        id
    }
}
