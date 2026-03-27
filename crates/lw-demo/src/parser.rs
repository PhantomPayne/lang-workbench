//! Recursive-descent parser for the `lw-demo` language.
//!
//! Produces a lossless [`CstTree`] on top of the flat [`CstArena`] token
//! ledger: whitespace, comments, and errors are all represented as nodes so
//! the tree is a complete, round-trippable copy of the source.
//!
//! # Trivia handling
//!
//! Trivia (whitespace, comments) between top-level statements is preserved
//! as `CstNode::Whitespace` / `CstNode::Comment` nodes in the root's
//! children list.  Trivia inside compound nodes (e.g. between `let` and the
//! binding name) is consumed but not currently tracked — this is a known
//! limitation.  Full inner-trivia tracking requires a red-green tree.

use lw_cst::{BinOp, CstArena, CstNode, CstNodeId, CstTree, LiteralKind, TokenSpan};

use crate::lexer::{TokenKind, lex_file};

/// A parse error with its source location (as a token span).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub span: TokenSpan,
    pub message: String,
}

/// The output of a single-file parse.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub arena: CstArena,
    pub line_index: lw_cst::LineIndex,
    pub tree: CstTree,
    pub root: CstNodeId,
    pub errors: Vec<ParseError>,
}

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

struct Parser {
    arena: CstArena,
    line_index: lw_cst::LineIndex,
    tree: CstTree,
    errors: Vec<ParseError>,
    pos: u32,
}

impl Parser {
    fn new(source: &str) -> Self {
        let (arena, line_index) = lex_file(source);
        Self {
            arena,
            line_index,
            tree: CstTree::new(),
            errors: Vec::new(),
            pos: 0,
        }
    }

    // --- token access helpers ---

    fn current_kind(&self) -> TokenKind {
        if self.pos < self.arena.token_count() {
            TokenKind::from_u16(self.arena.token(self.pos).kind)
        } else {
            TokenKind::Eof
        }
    }

    fn advance(&mut self) -> u32 {
        let idx = self.pos;
        if self.pos + 1 < self.arena.token_count() {
            self.pos += 1;
        }
        idx
    }

    /// Consume trivia (whitespace, newlines, comments) and emit them as CST
    /// nodes appended to `children`.
    fn eat_trivia(&mut self, children: &mut Vec<CstNodeId>) {
        loop {
            match self.current_kind() {
                TokenKind::Whitespace | TokenKind::Newline => {
                    let idx = self.pos;
                    self.advance();
                    let id = self.tree.alloc(CstNode::Whitespace {
                        span: TokenSpan::single(idx),
                    });
                    children.push(id);
                }
                TokenKind::Comment => {
                    let idx = self.pos;
                    self.advance();
                    let id = self.tree.alloc(CstNode::Comment {
                        span: TokenSpan::single(idx),
                    });
                    children.push(id);
                }
                _ => break,
            }
        }
    }

    // --- expression parsing ---
    //
    // All binary operators currently have the same precedence and associate
    // left-to-right.  Precedence levels (e.g. `*`/`/` > `+`/`-`) can be
    // added later via precedence climbing or a Pratt parser.

    fn parse_primary(&mut self) -> Option<CstNodeId> {
        match self.current_kind() {
            TokenKind::IntLiteral => {
                let idx = self.pos;
                self.advance();
                Some(self.tree.alloc(CstNode::Literal {
                    kind: LiteralKind::Integer,
                    span: TokenSpan::single(idx),
                }))
            }
            TokenKind::Ident => {
                let idx = self.pos;
                self.advance();
                Some(self.tree.alloc(CstNode::Identifier {
                    span: TokenSpan::single(idx),
                }))
            }
            _ => {
                let span = TokenSpan::single(self.pos);
                self.errors.push(ParseError {
                    span,
                    message: format!("expected expression, found {:?}", self.current_kind()),
                });
                let id = self.tree.alloc(CstNode::Error {
                    span,
                    message: "expected expression".to_string(),
                });
                None.or(Some(id))
            }
        }
    }

    fn parse_expr(&mut self) -> CstNodeId {
        let start_pos = self.pos;
        let mut lhs = match self.parse_primary() {
            Some(id) => id,
            None => {
                return self.tree.alloc(CstNode::Error {
                    span: TokenSpan::single(self.pos),
                    message: "expected expression".to_string(),
                });
            }
        };

        loop {
            // Skip whitespace between tokens in an expression.
            let mut lookahead = self.pos;
            while matches!(
                TokenKind::from_u16(self.arena.token(lookahead).kind),
                TokenKind::Whitespace | TokenKind::Comment
            ) {
                lookahead += 1;
            }

            let op = match TokenKind::from_u16(self.arena.token(lookahead).kind) {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                _ => break,
            };

            // Consume trivia + operator.
            while self.pos <= lookahead {
                self.advance();
            }

            // Skip whitespace after the operator.
            while self.current_kind().is_trivia() {
                self.advance();
            }

            let rhs = self.parse_primary().unwrap_or_else(|| {
                let span = TokenSpan::single(self.pos);
                self.errors.push(ParseError {
                    span,
                    message: "expected right-hand side of expression".to_string(),
                });
                self.tree.alloc(CstNode::Error {
                    span,
                    message: "expected rhs".to_string(),
                })
            });

            let _ = start_pos; // suppress unused warning
            lhs = self.tree.alloc(CstNode::BinaryExpr { op, lhs, rhs });
        }

        lhs
    }

    // --- statement parsing ---

    fn parse_let(&mut self) -> CstNodeId {
        // Skip whitespace after `let`.
        while self.current_kind().is_trivia() {
            self.advance();
        }

        // Name
        let name_span = if self.current_kind() == TokenKind::Ident {
            let span = TokenSpan::single(self.pos);
            self.advance();
            span
        } else {
            let span = TokenSpan::single(self.pos);
            self.errors.push(ParseError {
                span,
                message: "expected identifier after `let`".to_string(),
            });
            span
        };

        // Skip whitespace before `=`.
        while self.current_kind().is_trivia() {
            self.advance();
        }

        // `=`
        if self.current_kind() == TokenKind::Equals {
            self.advance();
        } else {
            let span = TokenSpan::single(self.pos);
            self.errors.push(ParseError {
                span,
                message: "expected `=` after binding name".to_string(),
            });
        }

        // Skip whitespace before value.
        while self.current_kind().is_trivia() {
            self.advance();
        }

        let value = self.parse_expr();

        self.tree.alloc(CstNode::LetBinding {
            name: name_span,
            value,
        })
    }

    fn parse_import(&mut self) -> CstNodeId {
        // Skip whitespace after `import`.
        while self.current_kind().is_trivia() {
            self.advance();
        }

        if self.current_kind() == TokenKind::StringLiteral {
            let span = TokenSpan::single(self.pos);
            self.advance();
            self.tree.alloc(CstNode::Import { path: span })
        } else {
            let span = TokenSpan::single(self.pos);
            self.errors.push(ParseError {
                span,
                message: "expected string literal after `import`".to_string(),
            });
            self.tree.alloc(CstNode::Error {
                span,
                message: "expected import path".to_string(),
            })
        }
    }

    // --- top level ---

    fn parse_root(&mut self) -> CstNodeId {
        let mut children: Vec<CstNodeId> = Vec::new();

        loop {
            self.eat_trivia(&mut children);

            match self.current_kind() {
                TokenKind::Eof => break,
                TokenKind::Let => {
                    self.advance();
                    let node = self.parse_let();
                    children.push(node);
                }
                TokenKind::Import => {
                    self.advance();
                    let node = self.parse_import();
                    children.push(node);
                }
                _ => {
                    let span = TokenSpan::single(self.pos);
                    self.errors.push(ParseError {
                        span,
                        message: format!(
                            "unexpected token at top level: {:?}",
                            self.current_kind()
                        ),
                    });
                    let id = self.tree.alloc(CstNode::Error {
                        span,
                        message: "unexpected token".to_string(),
                    });
                    children.push(id);
                    self.advance();
                }
            }
        }

        self.tree.alloc(CstNode::Root { children })
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse `source` text into a lossless CST.
pub fn parse(source: &str) -> ParseResult {
    let mut p = Parser::new(source);
    let root = p.parse_root();
    ParseResult {
        arena: p.arena,
        line_index: p.line_index,
        tree: p.tree,
        root,
        errors: p.errors,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_let_binding() {
        let result = parse("let x = 1");
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        let root = result.tree.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let non_trivia: Vec<_> = children
            .iter()
            .filter(|&&id| {
                !matches!(
                    result.tree.get(id),
                    CstNode::Whitespace { .. } | CstNode::Comment { .. }
                )
            })
            .collect();
        assert_eq!(non_trivia.len(), 1);
        assert!(matches!(
            result.tree.get(*non_trivia[0]),
            CstNode::LetBinding { .. }
        ));
    }

    #[test]
    fn parse_binary_expr() {
        let result = parse("let y = 1 + 2 * 3");
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[test]
    fn parse_import() {
        let result = parse("import \"std.lw\"");
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        let root = result.tree.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let non_trivia: Vec<_> = children
            .iter()
            .filter(|&&id| {
                !matches!(
                    result.tree.get(id),
                    CstNode::Whitespace { .. } | CstNode::Comment { .. }
                )
            })
            .collect();
        assert_eq!(non_trivia.len(), 1);
        assert!(matches!(
            result.tree.get(*non_trivia[0]),
            CstNode::Import { .. }
        ));
    }

    #[test]
    fn parse_error_recovery_at_top_level() {
        let result = parse("@ let x = 1");
        assert!(!result.errors.is_empty());

        let root = result.tree.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let has_error = children
            .iter()
            .any(|&id| matches!(result.tree.get(id), CstNode::Error { .. }));
        let has_let = children
            .iter()
            .any(|&id| matches!(result.tree.get(id), CstNode::LetBinding { .. }));
        assert!(has_error, "expected an error node");
        assert!(has_let, "expected a let binding after recovery");
    }

    #[test]
    fn parse_preserves_trivia_between_statements() {
        let result = parse("let x = 1\n\nlet y = 2");
        assert!(result.errors.is_empty());

        let root = result.tree.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let trivia_count = children
            .iter()
            .filter(|&&id| {
                matches!(
                    result.tree.get(id),
                    CstNode::Whitespace { .. } | CstNode::Comment { .. }
                )
            })
            .count();
        assert!(trivia_count > 0, "expected trivia nodes between statements");
    }

    #[test]
    fn parse_multiple_statements() {
        let result = parse("let a = 1\nlet b = 2\nlet c = 3");
        assert!(result.errors.is_empty());

        let root = result.tree.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let stmts = children
            .iter()
            .filter(|&&id| {
                matches!(
                    result.tree.get(id),
                    CstNode::LetBinding { .. } | CstNode::Import { .. }
                )
            })
            .count();
        assert_eq!(stmts, 3);
    }
}
