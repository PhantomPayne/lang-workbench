//! CST → AST lowering for the `lw-demo` language.
//!
//! Strips trivia (whitespace, comments) and maps [`CstNode`]s into the typed
//! [`Ast`] representation.  Because the AST uses typed arenas ([`ExprId`],
//! [`StmtId`]), lowering is straightforward — no placeholder-then-patch
//! pattern is needed.
//!
//! # Error handling
//!
//! - Trivia and CST error nodes are silently dropped (they don't appear in
//!   the AST).
//! - If a literal cannot be parsed (e.g. integer overflow), the lowerer
//!   emits [`Expr::Missing`] and records a [`LowerError`].

use lw_ast::{Ast, Expr, ExprId, Stmt, StmtId, TextRange};
use lw_cst::{CstArena, CstNode, CstNodeId};

/// An error produced during CST → AST lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub range: TextRange,
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
/// `source` is the original source text, needed to extract string slices for
/// identifier names, import paths, and literal values.
pub fn lower(arena: &CstArena, root: CstNodeId, source: &str) -> LowerResult {
    let mut ctx = LowerCtx {
        arena,
        source,
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
    source: &'a str,
    ast: Ast,
    errors: Vec<LowerError>,
}

impl LowerCtx<'_> {
    fn lower_root(&mut self, id: CstNodeId) {
        let CstNode::Root { children } = self.arena.get(id) else {
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
        match self.arena.get(id) {
            CstNode::LetBinding { name, value } => {
                let name = name.clone();
                let value = *value;
                let name_str = self.slice(&name).to_string();
                let value_id = self.lower_expr(value);
                Some(self.ast.alloc_stmt(
                    Stmt::Let {
                        name: name_str,
                        value: value_id,
                    },
                    name,
                ))
            }
            CstNode::Import { path } => {
                let path = path.clone();
                let raw = self.slice(&path);
                let path_str = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                    raw[1..raw.len() - 1].to_string()
                } else {
                    raw.to_string()
                };
                Some(self.ast.alloc_stmt(Stmt::Import { path: path_str }, path))
            }
            // Trivia and errors are stripped.
            CstNode::Whitespace { .. } | CstNode::Comment { .. } | CstNode::Error { .. } => None,
            // Expressions, literals, identifiers at statement level are not
            // valid top-level forms in this language — skip them.
            _ => None,
        }
    }

    fn lower_expr(&mut self, id: CstNodeId) -> ExprId {
        match self.arena.get(id) {
            CstNode::BinaryExpr { op, lhs, rhs } => {
                let op = op.clone();
                let lhs = *lhs;
                let rhs = *rhs;
                // Lower children first — no placeholders needed because
                // ExprId is returned directly by lower_expr.
                let lhs_id = self.lower_expr(lhs);
                let rhs_id = self.lower_expr(rhs);
                self.ast.alloc_expr(
                    Expr::Binary {
                        op,
                        lhs: lhs_id,
                        rhs: rhs_id,
                    },
                    TextRange::default(),
                )
            }
            CstNode::Literal {
                kind: lw_cst::LiteralKind::Integer,
                range,
            } => {
                let range = range.clone();
                let text = self.slice(&range);
                match text.parse::<i64>() {
                    Ok(value) => self.ast.alloc_expr(Expr::IntLiteral(value), range),
                    Err(e) => {
                        self.errors.push(LowerError {
                            range: range.clone(),
                            message: format!("invalid integer literal: {e}"),
                        });
                        self.ast.alloc_expr(Expr::Missing, range)
                    }
                }
            }
            CstNode::Literal {
                kind: lw_cst::LiteralKind::Float,
                range,
            } => {
                let range = range.clone();
                // Float literals are not currently part of the language spec.
                // Emit Missing and report an error.
                self.errors.push(LowerError {
                    range: range.clone(),
                    message: "float literals are not supported".to_string(),
                });
                self.ast.alloc_expr(Expr::Missing, range)
            }
            CstNode::Identifier { range } => {
                let range = range.clone();
                let name = self.slice(&range).to_string();
                self.ast.alloc_expr(Expr::Identifier(name), range)
            }
            // Trivia nodes can appear when the CST has them as children of
            // compound nodes — just produce Missing.
            CstNode::Error { range, .. } => {
                let range = range.clone();
                self.ast.alloc_expr(Expr::Missing, range)
            }
            _ => self.ast.alloc_expr(Expr::Missing, TextRange::default()),
        }
    }

    fn slice(&self, range: &TextRange) -> &str {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.source.len());
        &self.source[start..end]
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
        let lr = lower(&result.arena, result.root, "let x = 42");
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
        let lr = lower(&result.arena, result.root, "let y = 1 + 2");
        assert!(lr.errors.is_empty());

        let stmt = lr.ast.stmt(lr.ast.top_level[0]);
        let Stmt::Let { value, .. } = stmt else {
            panic!("expected Let");
        };
        let Expr::Binary { lhs, rhs, .. } = lr.ast.expr(*value) else {
            panic!("expected Binary");
        };
        // lhs and rhs are ExprId — they can only point at Expr nodes.
        assert_eq!(lr.ast.expr(*lhs), &Expr::IntLiteral(1));
        assert_eq!(lr.ast.expr(*rhs), &Expr::IntLiteral(2));
    }

    #[test]
    fn lower_integer_overflow_produces_missing_and_error() {
        // A 30-digit number will overflow i64.
        let src = "let big = 999999999999999999999999999999";
        let result = parse(src);
        let lr = lower(&result.arena, result.root, src);
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
        let lr = lower(&result.arena, result.root, "import \"math.lw\"");
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
