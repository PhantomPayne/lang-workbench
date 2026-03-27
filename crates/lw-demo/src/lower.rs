//! CST → AST lowering for the `lw-demo` language.
//!
//! Strips trivia (whitespace, comments) and maps [`CstNode`]s into the typed
//! [`Ast`] representation.  Because the AST uses typed indices ([`ExprId`],
//! [`StmtId`]), lowering is straightforward — children are lowered first,
//! then the parent is allocated with real IDs.
//!
//! # Error handling
//!
//! - Trivia and CST error nodes are silently dropped.
//! - If a literal cannot be parsed (e.g. integer overflow), the lowerer
//!   emits [`Expr::Missing`] and records a [`LowerError`].

use lw_ast::{Ast, Expr, ExprId, Stmt, StmtId, TokenSpan};
use lw_cst::{CstArena, CstNode, CstNodeId, CstTree};

/// An error produced during CST → AST lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub span: TokenSpan,
    pub message: String,
}

/// The result of lowering a CST into an AST.
#[derive(Debug, Clone)]
pub struct LowerResult {
    pub ast: Ast,
    pub errors: Vec<LowerError>,
}

/// Lower a parsed CST into a typed [`Ast`].
///
/// `arena` is the token ledger (needed to extract text for identifiers,
/// literals, and import paths).
pub fn lower(arena: &CstArena, tree: &CstTree, root: CstNodeId) -> LowerResult {
    let mut ctx = LowerCtx {
        arena,
        tree,
        ast: Ast::new(),
        errors: Vec::new(),
    };
    ctx.lower_root(root);
    LowerResult {
        ast: ctx.ast,
        errors: ctx.errors,
    }
}

// ---------------------------------------------------------------------------
// Internal lowering context
// ---------------------------------------------------------------------------

struct LowerCtx<'a> {
    arena: &'a CstArena,
    tree: &'a CstTree,
    ast: Ast,
    errors: Vec<LowerError>,
}

impl LowerCtx<'_> {
    fn lower_root(&mut self, id: CstNodeId) {
        let CstNode::Root { children } = self.tree.get(id) else {
            return;
        };
        let children = children.clone();
        for child in &children {
            if let Some(stmt_id) = self.lower_stmt(*child) {
                self.ast.top_level.push(stmt_id);
            }
        }
    }

    fn lower_stmt(&mut self, id: CstNodeId) -> Option<StmtId> {
        match self.tree.get(id) {
            CstNode::LetBinding { name, value } => {
                let name_span = *name;
                let value_id = *value;
                let name_str = self.arena.span_text(&name_span).to_string();
                let value_expr = self.lower_expr(value_id);
                Some(self.ast.alloc_stmt(
                    Stmt::Let {
                        name: name_str,
                        value: value_expr,
                    },
                    name_span,
                ))
            }
            CstNode::Import { path } => {
                let path_span = *path;
                let raw = self.arena.span_text(&path_span);
                let path_str = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                    raw[1..raw.len() - 1].to_string()
                } else {
                    raw.to_string()
                };
                Some(
                    self.ast
                        .alloc_stmt(Stmt::Import { path: path_str }, path_span),
                )
            }
            // Trivia and errors are stripped.
            CstNode::Whitespace { .. } | CstNode::Comment { .. } | CstNode::Error { .. } => None,
            _ => None,
        }
    }

    fn lower_expr(&mut self, id: CstNodeId) -> ExprId {
        match self.tree.get(id) {
            CstNode::BinaryExpr { op, lhs, rhs } => {
                let op = *op;
                let lhs = *lhs;
                let rhs = *rhs;
                let lhs_id = self.lower_expr(lhs);
                let rhs_id = self.lower_expr(rhs);
                self.ast.alloc_expr(
                    Expr::Binary {
                        op,
                        lhs: lhs_id,
                        rhs: rhs_id,
                    },
                    TokenSpan::default(),
                )
            }
            CstNode::Literal {
                kind: lw_cst::LiteralKind::Integer,
                span,
            } => {
                let span = *span;
                let text = self.arena.span_text(&span);
                match text.parse::<i64>() {
                    Ok(value) => self.ast.alloc_expr(Expr::IntLiteral(value), span),
                    Err(e) => {
                        self.errors.push(LowerError {
                            span,
                            message: format!("invalid integer literal: {e}"),
                        });
                        self.ast.alloc_expr(Expr::Missing, span)
                    }
                }
            }
            CstNode::Literal {
                kind: lw_cst::LiteralKind::Float,
                span,
            } => {
                let span = *span;
                self.errors.push(LowerError {
                    span,
                    message: "float literals are not supported".to_string(),
                });
                self.ast.alloc_expr(Expr::Missing, span)
            }
            CstNode::Identifier { span } => {
                let span = *span;
                let name = self.arena.span_text(&span).to_string();
                self.ast.alloc_expr(Expr::Identifier(name), span)
            }
            CstNode::Error { span, .. } => {
                let span = *span;
                self.ast.alloc_expr(Expr::Missing, span)
            }
            _ => self.ast.alloc_expr(Expr::Missing, TokenSpan::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn lower_integer_literal() {
        let result = parse("let x = 42");
        let lr = lower(&result.arena, &result.tree, result.root);
        assert!(lr.errors.is_empty());
        assert_eq!(lr.ast.top_level.len(), 1);

        let stmt = lr.ast.stmt(lr.ast.top_level[0]);
        match stmt {
            Stmt::Let { name, value } => {
                assert_eq!(name, "x");
                assert_eq!(lr.ast.expr(*value), &Expr::IntLiteral(42));
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn lower_binary_expr_has_typed_children() {
        let result = parse("let y = 1 + 2");
        let lr = lower(&result.arena, &result.tree, result.root);
        assert!(lr.errors.is_empty());

        let stmt = lr.ast.stmt(lr.ast.top_level[0]);
        let Stmt::Let { value, .. } = stmt else {
            panic!("expected Let");
        };
        let Expr::Binary { lhs, rhs, .. } = lr.ast.expr(*value) else {
            panic!("expected Binary");
        };
        assert_eq!(lr.ast.expr(*lhs), &Expr::IntLiteral(1));
        assert_eq!(lr.ast.expr(*rhs), &Expr::IntLiteral(2));
    }

    #[test]
    fn lower_integer_overflow_produces_missing_and_error() {
        let src = "let big = 999999999999999999999999999999";
        let result = parse(src);
        let lr = lower(&result.arena, &result.tree, result.root);
        assert!(!lr.errors.is_empty(), "expected a lowering error");
        assert!(lr.errors[0].message.contains("invalid integer literal"));

        let stmt = lr.ast.stmt(lr.ast.top_level[0]);
        let Stmt::Let { value, .. } = stmt else {
            panic!("expected Let");
        };
        assert_eq!(lr.ast.expr(*value), &Expr::Missing);
    }

    #[test]
    fn lower_import() {
        let result = parse("import \"math.lw\"");
        let lr = lower(&result.arena, &result.tree, result.root);
        assert!(lr.errors.is_empty());
        let stmt = lr.ast.stmt(lr.ast.top_level[0]);
        assert_eq!(
            stmt,
            &Stmt::Import {
                path: "math.lw".to_string()
            }
        );
    }
}
