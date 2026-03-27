# lang-workbench

A modular compiler engineering workbench built in Rust.

`lang-workbench` is a set of composable crates for building, analyzing, and serving a programming language — from the VFS and syntax tree all the way to a fully incremental LSP server that runs both natively and inside a browser Web Worker.

---

## Crate Structure

| Crate | Description |
|---|---|
| [`lw-vfs`](crates/lw-vfs) | Virtual file system — in-memory file store backed by `dashmap` + `ropey`. WASM-safe. |
| [`lw-cst`](crates/lw-cst) | Concrete Syntax Tree — Data-Oriented flat token ledger (`CstArena`), `LineIndex` for LSP positions, tree nodes in flat `Vec`. WASM-safe. |
| [`lw-ast`](crates/lw-ast) | Abstract Syntax Tree — typed flat arrays (`Expr`/`Stmt` with `ExprId`/`StmtId` newtypes), parallel span storage. WASM-safe. |
| [`lw-analysis`](crates/lw-analysis) | Incremental analyses — Salsa queries + Ascent datalog rules. WASM-safe. |
| [`lw-lsp-core`](crates/lw-lsp-core) | LSP logic layer — diagnostics, hover, completions; no transport, no async runtime. WASM-safe. |
| [`lw-lsp-native`](crates/lw-lsp-native) | Native LSP server — wraps `lw-lsp-core` with `tower-lsp` + `tokio` stdio transport. **Native only.** |
| [`lw-lsp-wasm`](crates/lw-lsp-wasm) | WASM LSP server — wraps `lw-lsp-core` with a `postMessage` Web Worker transport. **WASM only.** |
| [`lw-wasm`](crates/lw-wasm) | Direct JS/TS API — `wasm-bindgen` facade over all core crates; no LSP protocol. **WASM only.** |
| [`lw-demo`](crates/lw-demo) | End-to-end demo pipeline — lexer (logos), parser, typed AST, resolver, Salsa incremental. |

---

## Native Development

### Run all tests

```sh
cargo test
```

### Check formatting and lints

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude lw-lsp-wasm --exclude lw-wasm --all-targets -- -D warnings
```

---

## WASM Build Checks

Verify that each core crate still compiles to `wasm32-unknown-unknown`:

```sh
cargo build --target wasm32-unknown-unknown -p lw-vfs
cargo build --target wasm32-unknown-unknown -p lw-cst
cargo build --target wasm32-unknown-unknown -p lw-ast
cargo build --target wasm32-unknown-unknown -p lw-analysis
cargo build --target wasm32-unknown-unknown -p lw-lsp-core
```

> **Note:** `lw-lsp-native` is intentionally excluded — it depends on `tokio`
> and `tower-lsp` which are native-only.

### Build WASM packages (requires `wasm-pack`)

```sh
wasm-pack build crates/lw-lsp-wasm --target web
wasm-pack build crates/lw-wasm --target web
```

---

## CI

WASM compatibility is enforced in CI. Every pull request runs two jobs:

| Job | What it checks |
|---|---|
| `native` | `cargo fmt`, `cargo clippy`, `cargo test` for all non-WASM crates |
| `wasm` | `cargo build --target wasm32-unknown-unknown` for every core crate + `wasm-pack build` for the WASM packages |

If any core crate breaks `wasm32-unknown-unknown` compilation, CI fails loudly.
