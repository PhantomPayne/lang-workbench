//! Cross-file name resolution for the `lw-demo` language.
//!
//! Given the [`Ast`] for a single file and access to the VFS for resolving
//! direct imports, produces a [`ResolvedProgram`] that maps binding names to
//! their declaration IDs and collects any diagnostics.
//!
//! # Scope
//!
//! Resolution currently handles **direct imports only** — it does not recurse
//! into imports of imported files.  Cycle detection and transitive import
//! support are planned for a future pass.

use std::collections::HashMap;

use lw_ast::{Ast, Expr, Stmt, StmtId, TextRange};
use lw_vfs::Vfs;

use crate::{lower::lower, parser::parse};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Severity of a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A user-visible diagnostic message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub message: String,
    pub severity: Severity,
}

/// The result of resolving a single file and its direct imports.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    /// All bindings visible in this file (own + imported), mapping name → stmt ID.
    pub bindings: HashMap<String, StmtId>,
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Resolve `ast` (the AST of the file at `_path`) against the VFS.
///
/// - Reads directly imported files from `vfs`, parses and lowers them.
/// - Collects diagnostics for: import not found, duplicate binding, unknown
///   identifier.
pub fn resolve(_path: &str, vfs: &Vfs, ast: &Ast) -> ResolvedProgram {
    let mut bindings: HashMap<String, StmtId> = HashMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // --- collect imported bindings first ---
    for &stmt_id in &ast.top_level {
        let Stmt::Import { path: import_path } = ast.stmt(stmt_id) else {
            continue;
        };
        let span = ast.stmt_range(stmt_id).cloned().unwrap_or_default();

        match vfs.read(import_path) {
            Some(rope) => {
                let imported_source = rope.to_string();
                let result = parse(&imported_source);
                let lr = lower(&result.arena, result.root, &imported_source);
                collect_bindings_from(&lr.ast, &mut bindings, &mut diagnostics);
            }
            None => {
                diagnostics.push(Diagnostic {
                    range: span,
                    message: format!("import not found: {}", import_path),
                    severity: Severity::Error,
                });
            }
        }
    }

    // --- collect own bindings (detecting duplicates) ---
    collect_bindings_from(ast, &mut bindings, &mut diagnostics);

    // --- check for unknown identifiers ---
    check_identifiers_against(ast, &bindings, &mut diagnostics);

    ResolvedProgram {
        bindings,
        diagnostics,
    }
}

// ---------------------------------------------------------------------------
// Helpers (pub for reuse in salsa_db)
// ---------------------------------------------------------------------------

/// Add all top-level let-binding names from `ast` into `bindings`.
/// Emits a duplicate-binding diagnostic if a name is already present.
pub fn collect_bindings_from(
    ast: &Ast,
    bindings: &mut HashMap<String, StmtId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for &stmt_id in &ast.top_level {
        let Stmt::Let { name, .. } = ast.stmt(stmt_id) else {
            continue;
        };
        let span = ast.stmt_range(stmt_id).cloned().unwrap_or_default();
        if bindings.contains_key(name) {
            diagnostics.push(Diagnostic {
                range: span,
                message: format!("duplicate binding: {}", name),
                severity: Severity::Error,
            });
        } else {
            bindings.insert(name.clone(), stmt_id);
        }
    }
}

/// Walk all [`Expr::Identifier`] nodes in `ast` and emit a diagnostic for
/// any name not present in `bindings`.
pub fn check_identifiers_against(
    ast: &Ast,
    bindings: &HashMap<String, StmtId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (id, expr) in ast.exprs.iter() {
        let Expr::Identifier(name) = expr else {
            continue;
        };
        if !bindings.contains_key(name) {
            let span = ast.expr_range(id).cloned().unwrap_or_default();
            diagnostics.push(Diagnostic {
                range: span,
                message: format!("unknown identifier: {}", name),
                severity: Severity::Error,
            });
        }
    }
}
