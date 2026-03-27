# lw-demo

End-to-end Data-Oriented language pipeline for a tiny expression language,
demonstrating how the lang-workbench crates compose into a real compiler.

## The Language

```
// math.lw
let pi = 3

// main.lw
import "math.lw"
let area = pi * 10 * 10
```

**Supported constructs:**

| Construct | Example | Description |
|---|---|---|
| Let binding | `let x = 42` | Bind a name to an expression |
| Import | `import "math.lw"` | Import bindings from another file |
| Binary expr | `1 + 2 * 3` | `+`, `-`, `*`, `/` (left-to-right, flat precedence) |
| Int literal | `42` | 64-bit signed integer |
| Identifier | `pi` | Reference to a binding |
| Comment | `// ...` | Line comment (preserved in CST, stripped in AST) |

---

## Pipeline Architecture

```
source text
    │
    ▼
┌──────────┐   logos 0.16 DFA          (CstArena, LineIndex)
│ lex_file  │──────────────────────►  flat token array + newline table
└──────────┘
    │
    ▼
┌──────────┐   recursive descent       CstTree (flat Vec<CstNode>)
│  parser   │──────────────────────►  nodes reference tokens via TokenSpan
└──────────┘
    │
    ▼
┌──────────┐   typed lowering           Ast { exprs: Vec<Expr>, stmts: Vec<Stmt> }
│  lower    │──────────────────────►  typed ExprId/StmtId indices
└──────────┘
    │
    ▼
┌──────────┐   cross-file resolution    Vec<Diagnostic>
│ resolver  │──────────────────────►  span = TokenSpan (no line/col)
└──────────┘
    │
    ▼
┌──────────┐   incremental              Cached diagnostics
│ salsa_db  │──────────────────────►  file_diagnostics(file) → Vec<Diagnostic>
└──────────┘
```

---

## Design Decisions

### Rule 1: No Pointers

No `Box`, `Rc`, `RefCell`, or lifetimes are used to represent syntax trees.
All inter-node references are integer indices (`u32`) into flat arrays.

**Why?** Pointer-based trees have poor cache locality (nodes scattered across
the heap), can't be trivially serialized/deserialized, and lifetime parameters
infect all code that touches the tree. Integer indices into contiguous arrays
are cache-friendly, `Copy`, and trivially `Send`/`Sync`.

### Rule 2: Flat Arrays Only

All syntax data is stored in flat, contiguous `Vec`s:

- `CstArena.tokens: Vec<SyntaxToken>` — the token ledger
- `CstTree.nodes: Vec<CstNode>` — the tree node array
- `Ast.exprs: Vec<Expr>` — expression arena
- `Ast.stmts: Vec<Stmt>` — statement arena
- `Ast.expr_spans: Vec<TokenSpan>` — parallel span array

Each `Vec` is a single contiguous allocation. Indices into these arrays are
plain `u32` values (wrapped in newtypes for type safety).

### Rule 3: No Line Numbers in Nodes

No token or AST node stores line or column numbers. Instead:

- `SyntaxToken` stores only `kind: u16` and `len: u32` (byte length)
- `TokenSpan` stores `start_idx: u32` and `len: u16` (token index + count)
- `LineIndex` stores `newlines: Vec<u32>` (byte offsets of `\n` characters)

Line/column positions are computed **on demand** via
`LineIndex::byte_to_lsp_position(byte_offset)`, which uses binary search
(`partition_point`) for O(log n) lookup.

**Why?** Storing line numbers per-token wastes space and creates maintenance
burden (they must be recomputed on every edit). The line index is built once
during lexing and shared by all downstream passes.

### The Packed Token Span (`TokenSpan`)

```rust
pub struct TokenSpan {
    pub start_idx: u32,  // index into the token array
    pub len: u16,        // number of tokens this span covers
}
```

**6 bytes** total. Covers up to 65,535 tokens per syntactic construct, which
is more than sufficient. Token spans can be resolved to byte ranges via
`CstArena::span_byte_range(&self, span: &TokenSpan) -> (u32, u32)`.

### The CST Arena (The Flat Ledger)

```rust
pub struct SyntaxToken {
    pub kind: u16,  // language-specific token kind
    pub len: u32,   // byte length of this token's text
}

pub struct CstArena {
    text: String,               // complete source text
    tokens: Vec<SyntaxToken>,   // flat token array
    byte_starts: Vec<u32>,      // precomputed cumulative byte offsets
}
```

Token text is derived on demand: `arena.token_text(idx)` extracts the
substring from `text` using the cumulative byte offset table. No token
stores its own text.

### The Line Index (For LSP Communication)

```rust
pub struct LineIndex {
    newlines: Vec<u32>,  // byte offset of each '\n'
}
```

`byte_to_lsp_position` uses `partition_point` (binary search) to find the
line, then computes the column as `byte_offset - line_start`.

### Typed AST Indices

```rust
pub struct ExprId(pub u32);  // index into Ast.exprs
pub struct StmtId(pub u32);  // index into Ast.stmts

pub enum Expr {
    Binary { op: BinOp, lhs: ExprId, rhs: ExprId },
    IntLiteral(i64),
    Identifier(String),
    Missing,
}
```

**Why newtypes instead of plain `u32`?** The compiler enforces that
`Binary { lhs: ExprId }` can only reference expressions, not statements.
A plain `u32` would allow silently mixing indices from different arrays.

**Why `Expr::Missing` instead of `Option<ExprId>`?** `Missing` is a concrete
node — it has an `ExprId` and can appear anywhere an expression is expected.
This means `Binary { lhs, rhs }` always has valid children (no `Option`
unwrapping), and analyses can pattern-match on `Missing` to report errors.

### Span storage: parallel arrays

Source spans are stored in separate `Vec<TokenSpan>` arrays parallel to the
node arrays (`expr_spans[i]` corresponds to `exprs[i]`). This keeps the
core `Expr`/`Stmt` enums small and cache-friendly for passes that don't need
location info (e.g. type checking, evaluation).

### CST trivia handling — known limitation

The CST is lossless at the **top level**: whitespace and comments between
statements are preserved as CST nodes. However, trivia inside compound nodes
(e.g. whitespace between `let` and the binding name) is consumed without
creating CST nodes. Full inner-trivia tracking requires a red-green tree.

---

## Testing

Tests are organized in three tiers:

### Unit tests (in-module)

- **`lw-cst`** — 7 tests: TokenSpan, CstArena text extraction, LineIndex
- **`lexer.rs`** — 8 tests: token sequences, CRLF, lossless coverage, errors, LineIndex
- **`parser.rs`** — 6 tests: CST shape, error recovery, trivia, multi-statement
- **`lower.rs`** — 4 tests: typed children, integer overflow, imports

```sh
cargo test -p lw-cst --lib
cargo test -p lw-demo --lib
```

### Integration tests (`tests/pipeline.rs`)

11 end-to-end tests:

1. Single file parse + lower + zero diagnostics
2. Import not found
3. Cross-file import resolution
4. Salsa incremental invalidation (edit → error → restore → clean)
5. Duplicate binding
6. Typed `ExprId` children verification
7. Integer overflow diagnostic
8. Empty file
9. Comment preservation in CST / stripping in AST
10. Line index on-demand position computation
11. CstArena token text extraction

```sh
cargo test -p lw-demo --test pipeline
```

### Full suite
```sh
cargo test -p lw-cst -p lw-ast -p lw-demo
```
