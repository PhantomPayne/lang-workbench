# lw-demo

End-to-end language pipeline for a tiny expression language, demonstrating how
the lang-workbench crates compose into a real compiler.

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
┌─────────┐   logos 0.16 DFA          Token stream
│  lexer   │──────────────────────►  Vec<Token>
└─────────┘
    │
    ▼
┌─────────┐   recursive descent       Lossless concrete syntax tree
│  parser  │──────────────────────►  CstArena + CstNodeId
└─────────┘                           (la-arena, preserves trivia)
    │
    ▼
┌─────────┐   typed lowering           Typed abstract syntax tree
│  lower   │──────────────────────►  Ast { exprs: Arena<Expr>, stmts: Arena<Stmt> }
└─────────┘                           (la-arena, no trivia)
    │
    ▼
┌──────────┐  cross-file resolution    Diagnostics
│ resolver  │──────────────────────►  Vec<Diagnostic>
└──────────┘  (VFS-backed)
    │
    ▼
┌──────────┐  incremental              Cached diagnostics
│ salsa_db  │──────────────────────►  file_diagnostics(file) → Vec<Diagnostic>
└──────────┘  (tracks cross-file deps)
```

---

## Design Decisions

### Why typed arena indices instead of flat `u32` IDs?

The previous design used a flat columnar layout:

```rust
// OLD: untyped u32 indices — any ID can point at anything
pub type AstNodeId = u32;
pub enum AstNodeKind {
    BinaryExpr { op: BinOp, lhs: AstNodeId, rhs: AstNodeId },
    LetBinding { name: String, value: AstNodeId },
    Import { path: String },
    // ...
}
```

This has three problems:

1. **No type safety.** `BinaryExpr { lhs: AstNodeId }` could point at a `Root`
   or an `Import` node — nothing prevents it at the type level.

2. **Error-prone lowering.** Because parent nodes need to reference children
   that haven't been allocated yet, the lowerer must use a
   "placeholder-then-patch" pattern: allocate with `lhs: 0`, then mutate.
   If lowering the child fails, the fallback `unwrap_or(parent_id)` creates
   **self-referential cycles** that break later traversals.

3. **Harder to extend.** Adding a new node category (types, patterns, etc.)
   pollutes a single `AstNodeKind` enum.

The new design uses **typed arenas** (the approach used by rust-analyzer):

```rust
// NEW: typed indices — only Expr can appear where ExprId is expected
pub type ExprId = Idx<Expr>;
pub type StmtId = Idx<Stmt>;

pub enum Expr {
    Binary { op: BinOp, lhs: ExprId, rhs: ExprId },
    IntLiteral(i64),
    Identifier(String),
    Missing,  // error recovery — no cycles possible
}

pub enum Stmt {
    Let { name: String, value: ExprId },
    Import { path: String },
    Error,
}
```

Benefits:
- **Type-level correctness:** `BinaryExpr { lhs: ExprId }` can only reference
  expressions. The compiler rejects `StmtId` there.
- **No placeholder pattern:** Children are lowered first, then the parent is
  allocated with the real IDs. `Expr::Missing` handles failures without cycles.
- **Composable:** New node categories get their own arena + ID type.
- **Cache-friendly:** Each `Arena<T>` is a contiguous `Vec<T>` internally.

### Why la-arena?

[`la-arena`](https://crates.io/crates/la-arena) (from rust-analyzer) provides:
- `Arena<T>`: a typed arena backed by `Vec<T>` — O(1) alloc, O(1) index
- `Idx<T>`: a typed index that is `Copy`, `Eq`, `Hash`, `Ord` — perfect for
  inter-node references
- `ArenaMap<Idx<T>, V>`: a sparse map from arena indices to values — used
  here for source spans
- WASM-safe: no OS threads, no custom allocators

Compared to `bumpalo` (the previous choice):
- `bumpalo` returns `&'bump T` references → lifetime hell when nodes
  cross-reference each other, and a poor fit for Salsa's query model
- `la-arena` returns `Idx<T>` handles → no lifetimes, `Copy`, serializable

### Why separate expression and statement types?

In a real compiler, expressions and statements have fundamentally different
semantics:
- Expressions produce values and compose recursively
- Statements have side effects (binding, importing) and execute in order

Mixing them in a single enum means every match arm must handle irrelevant
variants. Separate types make each handler focused and exhaustive.

### Why `Expr::Missing` instead of `Option<ExprId>`?

`Expr::Missing` is a concrete node in the arena — it has an `ExprId` and can
appear anywhere an expression is expected. This means:
- `BinaryExpr { lhs, rhs }` always has valid child IDs (no `Option` unwrapping)
- Analyses can pattern-match on `Missing` to report errors
- No risk of `None`-induced panics in later passes

This is the approach used by rust-analyzer's `hir_def::body::Body`.

### Why are spans stored in `ArenaMap` rather than inline?

Storing spans separately from node data keeps the `Expr` and `Stmt` enums
small and cache-friendly for passes that don't need location info (e.g. type
checking, evaluation). Passes that do need spans (diagnostics, hover) pay for
the lookup only when needed.

### CST trivia handling — known limitation

The CST is lossless at the **top level**: whitespace and comments between
statements are preserved as `CstNode::Whitespace` / `CstNode::Comment` nodes
in the root's `children` list.

However, trivia **inside** compound nodes (e.g. whitespace between `let` and
the binding name, or between operands in a binary expression) is currently
consumed by the parser without creating CST nodes. This means the CST is not
fully round-trippable for formatting purposes.

Fixing this requires either:
- A **red-green tree** (like Roslyn/rust-analyzer) where every node has a flat
  `children: Vec<Element>` containing both tokens and sub-nodes
- Explicit **leading/trailing trivia** attached to each token

This is a planned improvement.

---

## Testing

Tests are organized in three tiers:

### Unit tests (in-module)

Each module has `#[cfg(test)] mod tests` with focused tests:

- **`lexer.rs`** — token sequences, CRLF handling, lossless coverage, error tokens
- **`parser.rs`** — CST shape, error recovery, trivia preservation, multi-statement
- **`lower.rs`** — typed children, integer overflow, import lowering

Run with:
```sh
cargo test -p lw-demo --lib
```

### Integration tests (`tests/pipeline.rs`)

End-to-end tests exercising the full pipeline:

1. Single file parse + lower + zero diagnostics
2. Import not found
3. Cross-file import resolution
4. Salsa incremental invalidation (edit → error → restore → clean)
5. Duplicate binding
6. Typed `ExprId` children verification
7. Integer overflow diagnostic
8. Empty file
9. Comment preservation in CST / stripping in AST

Run with:
```sh
cargo test -p lw-demo --test pipeline
```

### Full suite
```sh
cargo test -p lw-demo
```
