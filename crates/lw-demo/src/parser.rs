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
    source: &'src str,
    tokens: Vec<Token>,
    pos: usize,
    arena: CstArena,
    errors: Vec<ParseError>,
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

    fn peek_non_trivia(&self) -> &TokenKind {
        let mut i = self.pos;
        loop {
            match &self.tokens[i].kind {
                TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment => i += 1,
                k => return k,
            }
        }
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

    // --- expression parsing (precedence climbing) ---

    fn parse_primary(&mut self) -> Option<CstNodeId> {
        match self.current_kind().clone() {
            TokenKind::IntLiteral => {
                let range = self.current().range.clone();
                self.advance();
                Some(
                    self.arena
                        .alloc(CstNode::Literal { kind: LiteralKind::Integer, range }),
                )
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
                    message: format!(
                        "expected expression, found {:?}",
                        self.current_kind()
                    ),
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
            None => return self.arena.alloc(CstNode::Error {
                range: self.current().range.clone(),
                message: "expected expression".to_string(),
            }),
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

        self.arena.alloc(CstNode::LetBinding { name: name_range, value })
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
    ParseResult { arena: p.arena, root, errors: p.errors }
}
