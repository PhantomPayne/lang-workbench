//! Recursive-descent parser for the `lw-demo` language.
//!
//! Produces a lossless [`CstArena`]: whitespace, comments, and errors are all
//! represented as nodes so the tree is a complete, round-trippable copy of the
//! source.

use lw_cst::{BinOp, CstArena, CstNode, CstNodeId, LiteralKind, TextRange};

use crate::lexer::{Token, TokenKind, lex};

/// A parse error with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub range: TextRange,
    pub message: String,
}

/// The output of a single-file parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResult {
    pub arena: CstArena,
    pub root: CstNodeId,
    pub errors: Vec<ParseError>,
}

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

struct Parser<'src> {
    tokens: Vec<Token>,
    pos: usize,
    arena: CstArena,
    errors: Vec<ParseError>,
    #[allow(dead_code)]
    source: &'src str,
}

impl<'src> Parser<'src> {
    fn new(source: &'src str) -> Self {
        let tokens = lex(source);
        Self {
            source,
            tokens,
            pos: 0,
            arena: CstArena::new(),
            errors: Vec::new(),
        }
    }

    // --- token access helpers ---

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    /// Consume trivia (whitespace, newlines, comments) and emit them as CST
    /// nodes that are appended to `children`.
    fn eat_trivia(&mut self, children: &mut Vec<CstNodeId>) {
        loop {
            match self.current_kind() {
                TokenKind::Whitespace => {
                    let range = self.current().range.clone();
                    self.advance();
                    let id = self.arena.alloc(CstNode::Whitespace { range });
                    children.push(id);
                }
                TokenKind::Newline => {
                    let range = self.current().range.clone();
                    self.advance();
                    let id = self.arena.alloc(CstNode::Whitespace { range });
                    children.push(id);
                }
                TokenKind::Comment => {
                    let range = self.current().range.clone();
                    self.advance();
                    let id = self.arena.alloc(CstNode::Comment { range });
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
    //
    // Note: trivia (whitespace, comments) between operands and operators is
    // consumed but not emitted as CST nodes inside expressions.  The CST is
    // lossless at the *top level* (between statements), but trivia inside
    // compound nodes like LetBinding and BinaryExpr is currently not tracked.
    // This is a known limitation — full inner-trivia tracking requires either
    // a red-green tree or an explicit children list per node.

    fn parse_primary(&mut self) -> Option<CstNodeId> {
        match self.current_kind().clone() {
            TokenKind::IntLiteral => {
                let range = self.current().range.clone();
                self.advance();
                Some(self.arena.alloc(CstNode::Literal {
                    kind: LiteralKind::Integer,
                    range,
                }))
            }
            TokenKind::Ident => {
                let range = self.current().range.clone();
                self.advance();
                Some(self.arena.alloc(CstNode::Identifier { range }))
            }
            _ => {
                let range = self.current().range.clone();
                self.errors.push(ParseError {
                    range: range.clone(),
                    message: format!("expected expression, found {:?}", self.current_kind()),
                });
                let id = self.arena.alloc(CstNode::Error {
                    range,
                    message: "expected expression".to_string(),
                });
                None.or(Some(id))
            }
        }
    }

    /// Parse a binary expression with left-to-right associativity.
    fn parse_expr(&mut self) -> CstNodeId {
        let mut lhs = match self.parse_primary() {
            Some(id) => id,
            None => {
                return self.arena.alloc(CstNode::Error {
                    range: self.current().range.clone(),
                    message: "expected expression".to_string(),
                });
            }
        };

        loop {
            // Skip whitespace between tokens in an expression.
            let mut lookahead_pos = self.pos;
            while matches!(
                self.tokens[lookahead_pos].kind,
                TokenKind::Whitespace | TokenKind::Comment
            ) {
                lookahead_pos += 1;
            }

            let op = match &self.tokens[lookahead_pos].kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                _ => break,
            };

            // Consume any trivia before the operator (but don't attach them as
            // separate children here — they sit inside the BinaryExpr).
            while self.pos < lookahead_pos {
                self.advance();
            }
            // Consume the operator itself.
            self.advance();

            // Skip whitespace after the operator.
            while matches!(
                self.current_kind(),
                TokenKind::Whitespace | TokenKind::Comment
            ) {
                self.advance();
            }

            let rhs = self.parse_primary().unwrap_or_else(|| {
                let range = self.current().range.clone();
                self.errors.push(ParseError {
                    range: range.clone(),
                    message: "expected right-hand side of expression".to_string(),
                });
                self.arena.alloc(CstNode::Error {
                    range,
                    message: "expected rhs".to_string(),
                })
            });

            lhs = self.arena.alloc(CstNode::BinaryExpr { op, lhs, rhs });
        }

        lhs
    }

    // --- statement parsing ---

    /// Parse a `let <name> = <expr>` statement. The `let` token has already
    /// been consumed by the caller.
    fn parse_let(&mut self, _let_range: TextRange) -> CstNodeId {
        // Skip whitespace after `let`.
        while matches!(
            self.current_kind(),
            TokenKind::Whitespace | TokenKind::Comment
        ) {
            self.advance();
        }

        // name
        let name_range = if self.current_kind() == &TokenKind::Ident {
            let r = self.current().range.clone();
            self.advance();
            r
        } else {
            let range = self.current().range.clone();
            self.errors.push(ParseError {
                range: range.clone(),
                message: "expected identifier after `let`".to_string(),
            });
            range
        };

        // Skip whitespace before `=`.
        while matches!(
            self.current_kind(),
            TokenKind::Whitespace | TokenKind::Comment
        ) {
            self.advance();
        }

        // `=`
        if self.current_kind() == &TokenKind::Equals {
            self.advance();
        } else {
            let range = self.current().range.clone();
            self.errors.push(ParseError {
                range: range.clone(),
                message: "expected `=` after binding name".to_string(),
            });
        }

        // Skip whitespace before the value expression.
        while matches!(
            self.current_kind(),
            TokenKind::Whitespace | TokenKind::Comment
        ) {
            self.advance();
        }

        let value = self.parse_expr();

        self.arena.alloc(CstNode::LetBinding {
            name: name_range,
            value,
        })
    }

    /// Parse an `import "<path>"` statement. The `import` token has already
    /// been consumed by the caller.
    fn parse_import(&mut self, _import_range: TextRange) -> CstNodeId {
        // Skip whitespace after `import`.
        while matches!(
            self.current_kind(),
            TokenKind::Whitespace | TokenKind::Comment
        ) {
            self.advance();
        }

        if self.current_kind() == &TokenKind::StringLiteral {
            let path_range = self.current().range.clone();
            self.advance();
            self.arena.alloc(CstNode::Import { path: path_range })
        } else {
            let range = self.current().range.clone();
            self.errors.push(ParseError {
                range: range.clone(),
                message: "expected string literal after `import`".to_string(),
            });
            self.arena.alloc(CstNode::Error {
                range,
                message: "expected import path".to_string(),
            })
        }
    }

    // --- top level ---

    fn parse_root(&mut self) -> CstNodeId {
        let mut children: Vec<CstNodeId> = Vec::new();

        loop {
            self.eat_trivia(&mut children);

            match self.current_kind().clone() {
                TokenKind::Eof => break,
                TokenKind::Let => {
                    let let_range = self.current().range.clone();
                    self.advance();
                    let node = self.parse_let(let_range);
                    children.push(node);
                }
                TokenKind::Import => {
                    let import_range = self.current().range.clone();
                    self.advance();
                    let node = self.parse_import(import_range);
                    children.push(node);
                }
                _ => {
                    let range = self.current().range.clone();
                    self.errors.push(ParseError {
                        range: range.clone(),
                        message: format!(
                            "unexpected token at top level: {:?}",
                            self.current_kind()
                        ),
                    });
                    let id = self.arena.alloc(CstNode::Error {
                        range,
                        message: "unexpected token".to_string(),
                    });
                    children.push(id);
                    self.advance();
                }
            }
        }

        self.arena.alloc(CstNode::Root { children })
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

        let root = result.arena.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let non_trivia: Vec<_> = children
            .iter()
            .filter(|&&id| {
                !matches!(
                    result.arena.get(id),
                    CstNode::Whitespace { .. } | CstNode::Comment { .. }
                )
            })
            .collect();
        assert_eq!(non_trivia.len(), 1);
        assert!(matches!(
            result.arena.get(*non_trivia[0]),
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

        let root = result.arena.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let non_trivia: Vec<_> = children
            .iter()
            .filter(|&&id| {
                !matches!(
                    result.arena.get(id),
                    CstNode::Whitespace { .. } | CstNode::Comment { .. }
                )
            })
            .collect();
        assert_eq!(non_trivia.len(), 1);
        assert!(matches!(
            result.arena.get(*non_trivia[0]),
            CstNode::Import { .. }
        ));
    }

    #[test]
    fn parse_error_recovery_at_top_level() {
        let result = parse("@ let x = 1");
        // Should recover and still parse the let binding.
        assert!(!result.errors.is_empty());

        let root = result.arena.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        // Should have at least an Error node and a LetBinding.
        let has_error = children
            .iter()
            .any(|&id| matches!(result.arena.get(id), CstNode::Error { .. }));
        let has_let = children
            .iter()
            .any(|&id| matches!(result.arena.get(id), CstNode::LetBinding { .. }));
        assert!(has_error, "expected an error node");
        assert!(has_let, "expected a let binding after recovery");
    }

    #[test]
    fn parse_preserves_trivia_between_statements() {
        let result = parse("let x = 1\n\nlet y = 2");
        assert!(result.errors.is_empty());

        let root = result.arena.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let trivia_count = children
            .iter()
            .filter(|&&id| {
                matches!(
                    result.arena.get(id),
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

        let root = result.arena.get(result.root);
        let CstNode::Root { children } = root else {
            panic!("expected Root");
        };
        let stmts = children
            .iter()
            .filter(|&&id| {
                matches!(
                    result.arena.get(id),
                    CstNode::LetBinding { .. } | CstNode::Import { .. }
                )
            })
            .count();
        assert_eq!(stmts, 3);
    }
}
