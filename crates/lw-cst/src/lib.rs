//! `lw-cst` — Data-Oriented Concrete Syntax Tree for lang-workbench.
//!
//! # Design: Flat Arrays, No Pointers
//!
//! All syntax data is stored in flat, contiguous `Vec`s.  There are no `Box`,
//! `Rc`, `RefCell`, or lifetime-bound references in the syntax tree.
//!
//! **Rule: no line or column numbers are stored in tokens or nodes.**
//! Line/column mapping is performed on-demand via [`LineIndex`], which uses
//! binary search over a precomputed newline-offset table.
//!
//! ## Core types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`SyntaxToken`] | A token: `kind: u16` + `len: u32` (byte length). No text. |
//! | [`TokenSpan`] | Packed span: `start_idx: u32` + `len: u16` (token count). |
//! | [`CstArena`] | The flat ledger: owns `text: String` + `tokens: Vec<SyntaxToken>`. |
//! | [`LineIndex`] | Newline positions for `O(log n)` byte → LSP-position lookup. |
//!
//! The source text for any token is derived from its position in the token
//! array and its byte length.  This keeps tokens at 6 bytes each while
//! retaining full-fidelity access to the source.
//!
//! ## Tree layer
//!
//! The parser produces a flat `Vec<CstNode>` on top of the token ledger.
//! Tree nodes reference each other by `u32` index — no heap pointers.

// ---------------------------------------------------------------------------
// Token span
// ---------------------------------------------------------------------------

/// A packed span referencing a contiguous range of tokens.
///
/// Memory-optimized: 6 bytes total (vs 8 for a `(u32, u32)` start/end pair).
/// Covers up to 65 535 tokens per node, which is sufficient for any
/// single syntactic construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenSpan {
    /// Index of the first token in this span.
    pub start_idx: u32,
    /// Number of tokens this span covers.
    pub len: u16,
}

impl TokenSpan {
    /// Create a new token span.
    pub fn new(start_idx: u32, len: u16) -> Self {
        Self { start_idx, len }
    }

    /// Create a single-token span.
    pub fn single(idx: u32) -> Self {
        Self {
            start_idx: idx,
            len: 1,
        }
    }

    /// Create a span covering tokens from `start` to `end` (exclusive).
    pub fn from_range(start: u32, end: u32) -> Self {
        let len = end.saturating_sub(start).min(u16::MAX as u32) as u16;
        Self {
            start_idx: start,
            len,
        }
    }

    /// The exclusive end index (one past the last token).
    pub fn end_idx(&self) -> u32 {
        self.start_idx + self.len as u32
    }
}

// ---------------------------------------------------------------------------
// Syntax token
// ---------------------------------------------------------------------------

/// A single token in the flat CST ledger.
///
/// Stores only its **kind** and **byte length** — the actual text is derived
/// from the token's position in the source string.  No line or column numbers
/// are stored.
///
/// `kind` is a `u16` so that each language can define its own `TokenKind` enum
/// and convert to/from `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxToken {
    /// Token kind (language-specific; stored as u16 for compactness).
    pub kind: u16,
    /// Byte length of this token's text in the source.
    pub len: u32,
}

// ---------------------------------------------------------------------------
// CstArena — the flat ledger
// ---------------------------------------------------------------------------

/// The flat CST arena ("ledger").
///
/// Owns the raw source text and a flat, contiguous array of tokens.
/// Every byte of the source is covered by exactly one token — the token
/// stream is lossless.
///
/// Token text is derived on-demand from position + byte length; tokens
/// themselves store no text or line numbers.
#[derive(Debug, Clone)]
pub struct CstArena {
    /// The complete source text.
    text: String,
    /// Flat array of tokens in source order.
    tokens: Vec<SyntaxToken>,
    /// Precomputed cumulative byte offsets: `byte_starts[i]` is the byte
    /// offset where token `i` begins.  Length = `tokens.len() + 1` (the
    /// last entry equals the total source length).
    byte_starts: Vec<u32>,
}

impl Default for CstArena {
    fn default() -> Self {
        Self::new()
    }
}

impl CstArena {
    /// Create a new, empty arena.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            tokens: Vec::new(),
            byte_starts: vec![0],
        }
    }

    /// Build a `CstArena` from source text and a pre-built token list.
    ///
    /// Computes the cumulative byte-offset table automatically.
    pub fn from_tokens(text: String, tokens: Vec<SyntaxToken>) -> Self {
        let mut byte_starts = Vec::with_capacity(tokens.len() + 1);
        let mut offset: u32 = 0;
        for tok in &tokens {
            byte_starts.push(offset);
            offset += tok.len;
        }
        byte_starts.push(offset);
        Self {
            text,
            tokens,
            byte_starts,
        }
    }

    /// The full source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of tokens.
    pub fn token_count(&self) -> u32 {
        self.tokens.len() as u32
    }

    /// Access a token by index.
    pub fn token(&self, idx: u32) -> &SyntaxToken {
        &self.tokens[idx as usize]
    }

    /// All tokens as a slice.
    pub fn tokens(&self) -> &[SyntaxToken] {
        &self.tokens
    }

    /// Byte offset where token `idx` starts.
    pub fn byte_start(&self, idx: u32) -> u32 {
        self.byte_starts[idx as usize]
    }

    /// Byte offset where token `idx` ends (exclusive).
    pub fn byte_end(&self, idx: u32) -> u32 {
        self.byte_starts[idx as usize] + self.tokens[idx as usize].len
    }

    /// Byte range `(start, end)` covered by a [`TokenSpan`].
    pub fn span_byte_range(&self, span: &TokenSpan) -> (u32, u32) {
        let start = self.byte_starts[span.start_idx as usize];
        let end_idx = (span.start_idx + span.len as u32) as usize;
        let end = if end_idx <= self.byte_starts.len() {
            self.byte_starts[end_idx.min(self.byte_starts.len() - 1)]
        } else {
            self.text.len() as u32
        };
        (start, end)
    }

    /// Extract the source text of a single token.
    pub fn token_text(&self, idx: u32) -> &str {
        let start = self.byte_starts[idx as usize] as usize;
        let end = start + self.tokens[idx as usize].len as usize;
        &self.text[start..end.min(self.text.len())]
    }

    /// Extract the source text covered by a [`TokenSpan`].
    pub fn span_text(&self, span: &TokenSpan) -> &str {
        let (start, end) = self.span_byte_range(span);
        &self.text[start as usize..end as usize]
    }
}

// ---------------------------------------------------------------------------
// LineIndex — O(log n) byte → LSP-position mapping
// ---------------------------------------------------------------------------

/// Line index for O(log n) byte-offset → line/column mapping.
///
/// Stores the byte offset of every `\n` character in the source.
/// Line and column positions are computed on demand — never stored in
/// tokens or nodes.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each `\n` character in the source.
    newlines: Vec<u32>,
}

impl Default for LineIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl LineIndex {
    /// Create an empty line index.
    pub fn new() -> Self {
        Self {
            newlines: Vec::new(),
        }
    }

    /// Build a line index by scanning the source for `\n` characters.
    pub fn from_source(source: &str) -> Self {
        let newlines = source
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| i as u32)
            .collect();
        Self { newlines }
    }

    /// Convert a byte offset to an LSP `Position` (0-based line, 0-based
    /// UTF-16 column offset).
    ///
    /// Uses binary search (`partition_point`) for O(log n) lookup.
    pub fn byte_to_lsp_position(&self, byte_offset: u32) -> lsp_types::Position {
        // The line number is the count of newlines before this offset.
        let line = self.newlines.partition_point(|&nl| nl < byte_offset) as u32;

        // Column = distance from the start of the line.
        let line_start = if line == 0 {
            0
        } else {
            self.newlines[line as usize - 1] + 1
        };
        let col = byte_offset.saturating_sub(line_start);

        lsp_types::Position {
            line,
            character: col,
        }
    }

    /// The total number of lines (newlines.len() + 1).
    pub fn line_count(&self) -> usize {
        self.newlines.len() + 1
    }
}

// ---------------------------------------------------------------------------
// Semantic types shared with lw-ast
// ---------------------------------------------------------------------------

/// Binary operators supported by the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// The kind of a literal token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    Integer,
    Float,
}

// ---------------------------------------------------------------------------
// CST tree nodes — flat array of parser output
// ---------------------------------------------------------------------------

/// Index into the flat `Vec<CstNode>` tree-node array.
pub type CstNodeId = u32;

/// A sentinel value indicating "no node" (e.g. missing child).
pub const CST_NODE_NONE: CstNodeId = u32::MAX;

/// A node in the concrete syntax tree.
///
/// Stored in a flat `Vec<CstNode>`.  Children are referenced by `CstNodeId`
/// (a plain `u32` index into the same array).  No `Box`, `Rc`, or lifetimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CstNode {
    /// The top-level root of a source file.
    Root { children: Vec<CstNodeId> },
    /// A `let <name> = <expr>` binding.
    LetBinding { name: TokenSpan, value: CstNodeId },
    /// An `import "<path>"` statement.
    Import { path: TokenSpan },
    /// A binary expression `<lhs> <op> <rhs>`.
    BinaryExpr {
        op: BinOp,
        lhs: CstNodeId,
        rhs: CstNodeId,
    },
    /// An integer or float literal.
    Literal { kind: LiteralKind, span: TokenSpan },
    /// An identifier reference.
    Identifier { span: TokenSpan },
    /// A whitespace trivia node.
    Whitespace { span: TokenSpan },
    /// A line-comment trivia node (`// …`).
    Comment { span: TokenSpan },
    /// An error-recovery node for unexpected input.
    Error { span: TokenSpan, message: String },
}

/// A flat CST tree: a contiguous array of [`CstNode`]s plus a root index.
///
/// The parser builds this on top of the [`CstArena`] token ledger.
/// All inter-node references are `CstNodeId` (`u32`) indices into `nodes`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CstTree {
    /// Flat array of tree nodes.
    nodes: Vec<CstNode>,
}

impl CstTree {
    /// Create a new, empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a node and return its index.
    pub fn alloc(&mut self, node: CstNode) -> CstNodeId {
        let id = self.nodes.len() as CstNodeId;
        self.nodes.push(node);
        id
    }

    /// Look up a node by index.
    pub fn get(&self, id: CstNodeId) -> &CstNode {
        &self.nodes[id as usize]
    }

    /// Number of nodes in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_span_from_range() {
        let span = TokenSpan::from_range(3, 7);
        assert_eq!(span.start_idx, 3);
        assert_eq!(span.len, 4);
        assert_eq!(span.end_idx(), 7);
    }

    #[test]
    fn cst_arena_token_text() {
        let arena = CstArena::from_tokens(
            "let x = 42".to_string(),
            vec![
                SyntaxToken { kind: 0, len: 3 }, // "let"
                SyntaxToken { kind: 1, len: 1 }, // " "
                SyntaxToken { kind: 2, len: 1 }, // "x"
                SyntaxToken { kind: 1, len: 1 }, // " "
                SyntaxToken { kind: 3, len: 1 }, // "="
                SyntaxToken { kind: 1, len: 1 }, // " "
                SyntaxToken { kind: 4, len: 2 }, // "42"
            ],
        );
        assert_eq!(arena.token_text(0), "let");
        assert_eq!(arena.token_text(2), "x");
        assert_eq!(arena.token_text(6), "42");
        assert_eq!(arena.token_count(), 7);
    }

    #[test]
    fn cst_arena_span_text() {
        let arena = CstArena::from_tokens(
            "let x = 42".to_string(),
            vec![
                SyntaxToken { kind: 0, len: 3 },
                SyntaxToken { kind: 1, len: 1 },
                SyntaxToken { kind: 2, len: 1 },
                SyntaxToken { kind: 1, len: 1 },
                SyntaxToken { kind: 3, len: 1 },
                SyntaxToken { kind: 1, len: 1 },
                SyntaxToken { kind: 4, len: 2 },
            ],
        );
        let span = TokenSpan::from_range(0, 3); // "let x"
        assert_eq!(arena.span_text(&span), "let x");
    }

    #[test]
    fn line_index_single_line() {
        let idx = LineIndex::from_source("hello world");
        let pos = idx.byte_to_lsp_position(6);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 6);
    }

    #[test]
    fn line_index_multi_line() {
        let idx = LineIndex::from_source("line one\nline two\nline three");
        // "line two" starts at byte 9, so byte 14 = 'two' offset 5
        let pos = idx.byte_to_lsp_position(14);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn line_index_start_of_line() {
        let idx = LineIndex::from_source("aaa\nbbb\nccc");
        let pos = idx.byte_to_lsp_position(4); // start of "bbb"
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn cst_tree_alloc_and_get() {
        let mut tree = CstTree::new();
        let id = tree.alloc(CstNode::Identifier {
            span: TokenSpan::single(0),
        });
        assert_eq!(id, 0);
        assert!(matches!(tree.get(id), CstNode::Identifier { .. }));
    }
}
