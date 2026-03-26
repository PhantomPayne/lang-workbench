//! Hand-written lexer for the `lw-demo` language.

use lw_cst::TextRange;

/// Every kind of token the lexer can produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// `let` keyword.
    Let,
    /// `import` keyword.
    Import,
    /// An identifier (`[a-zA-Z_][a-zA-Z0-9_]*`).
    Ident,
    /// An integer literal (`[0-9]+`).
    IntLiteral,
    /// A double-quoted string literal.
    StringLiteral,
    /// `=`
    Equals,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// One or more newline characters.
    Newline,
    /// One or more non-newline whitespace characters.
    Whitespace,
    /// A `// …` line comment.
    Comment,
    /// The logical end of input.
    Eof,
    /// An unrecognised character.
    Error,
}

/// A single lexed token.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub range: TextRange,
}

/// Lex `source` into a flat list of tokens, ending with a single [`TokenKind::Eof`].
pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut pos: usize = 0;

    while pos < len {
        let start = pos as u32;
        let ch = bytes[pos] as char;

        // --- newlines ---
        if ch == '\n' || ch == '\r' {
            pos += 1;
            if ch == '\r' && pos < len && bytes[pos] == b'\n' {
                pos += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Newline,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- non-newline whitespace ---
        if ch.is_ascii_whitespace() {
            while pos < len && {
                let c = bytes[pos] as char;
                c.is_ascii_whitespace() && c != '\n' && c != '\r'
            } {
                pos += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Whitespace,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- line comments ---
        if ch == '/' && pos + 1 < len && bytes[pos + 1] == b'/' {
            while pos < len && bytes[pos] != b'\n' {
                pos += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Comment,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- single-char punctuation ---
        match ch {
            '=' => {
                pos += 1;
                tokens.push(Token {
                    kind: TokenKind::Equals,
                    range: TextRange::new(start, pos as u32),
                });
                continue;
            }
            '+' => {
                pos += 1;
                tokens.push(Token {
                    kind: TokenKind::Plus,
                    range: TextRange::new(start, pos as u32),
                });
                continue;
            }
            '-' => {
                pos += 1;
                tokens.push(Token {
                    kind: TokenKind::Minus,
                    range: TextRange::new(start, pos as u32),
                });
                continue;
            }
            '*' => {
                pos += 1;
                tokens.push(Token {
                    kind: TokenKind::Star,
                    range: TextRange::new(start, pos as u32),
                });
                continue;
            }
            '/' => {
                pos += 1;
                tokens.push(Token {
                    kind: TokenKind::Slash,
                    range: TextRange::new(start, pos as u32),
                });
                continue;
            }
            _ => {}
        }

        // --- integer literals ---
        if ch.is_ascii_digit() {
            while pos < len && (bytes[pos] as char).is_ascii_digit() {
                pos += 1;
            }
            tokens.push(Token {
                kind: TokenKind::IntLiteral,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- string literals ---
        if ch == '"' {
            pos += 1; // consume opening quote
            while pos < len && bytes[pos] != b'"' && bytes[pos] != b'\n' {
                pos += 1;
            }
            if pos < len && bytes[pos] == b'"' {
                pos += 1; // consume closing quote
            }
            tokens.push(Token {
                kind: TokenKind::StringLiteral,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- identifiers / keywords ---
        if ch.is_ascii_alphabetic() || ch == '_' {
            while pos < len && {
                let c = bytes[pos] as char;
                c.is_ascii_alphanumeric() || c == '_'
            } {
                pos += 1;
            }
            let word = &source[start as usize..pos];
            let kind = match word {
                "let" => TokenKind::Let,
                "import" => TokenKind::Import,
                _ => TokenKind::Ident,
            };
            tokens.push(Token {
                kind,
                range: TextRange::new(start, pos as u32),
            });
            continue;
        }

        // --- unrecognised ---
        pos += 1;
        tokens.push(Token {
            kind: TokenKind::Error,
            range: TextRange::new(start, pos as u32),
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        range: TextRange::new(pos as u32, pos as u32),
    });

    tokens
}
