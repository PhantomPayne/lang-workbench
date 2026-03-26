//! Cross-file name resolution for the `lw-demo` language.
//!
//! Given the [`AstNodes`] for a single file and access to the VFS for
//! resolving imports, produces a [`ResolvedProgram`] that maps binding names
//! to their declaration node IDs and collects any diagnostics.

use std::collections::HashMap;

use lw_ast::{AstNodeId, AstNodeKind, AstNodes};
use lw_cst::TextRange;
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

/// The result of resolving a single file and its transitive imports.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    /// All bindings visible in this file (own + imported), mapping name → node ID.
    pub bindings: HashMap<String, AstNodeId>,
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Resolve `ast` (the AST of the file at `path`) against the VFS.
///
/// - Reads imported files from `vfs`, parses and lowers them.
/// - Collects diagnostics for: import not found, duplicate binding, unknown
///   identifier.
pub fn resolve(path: &str, vfs: &Vfs, ast: &AstNodes) -> ResolvedProgram {
    let mut bindings: HashMap<String, AstNodeId> = HashMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // --- collect imported bindings first ---
    for (i, kind) in ast.kinds.iter().enumerate() {
        let AstNodeKind::Import { path: import_path } = kind else {
            continue;
        };
        let span = ast.spans[i].clone();

        match vfs.read(import_path) {
            Some(rope) => {
                let imported_source = rope.to_string();
                let result = parse(&imported_source);
                let imported_ast = lower(&result.arena, result.root, &imported_source);
                collect_bindings(&imported_ast, import_path, &mut bindings, &mut diagnostics);
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
    collect_bindings(ast, path, &mut bindings, &mut diagnostics);

    // --- check for unknown identifiers ---
    check_identifiers(ast, &bindings, &mut diagnostics);

    ResolvedProgram { bindings, diagnostics }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Add all top-level let-binding names from `ast` into `bindings`.
/// Emits a duplicate-binding diagnostic if a name is already present.
fn collect_bindings(
    ast: &AstNodes,
    _source_path: &str,
    bindings: &mut HashMap<String, AstNodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (i, kind) in ast.kinds.iter().enumerate() {
        let AstNodeKind::LetBinding { name, .. } = kind else {
            continue;
        };
        let span = ast.spans[i].clone();
        if bindings.contains_key(name) {
            diagnostics.push(Diagnostic {
                range: span,
                message: format!("duplicate binding: {}", name),
                severity: Severity::Error,
            });
        } else {
            bindings.insert(name.clone(), i as AstNodeId);
        }
    }
}

/// Walk all [`AstNodeKind::Identifier`] nodes and emit a diagnostic for any
/// name not present in `bindings`.
fn check_identifiers(
    ast: &AstNodes,
    bindings: &HashMap<String, AstNodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (i, kind) in ast.kinds.iter().enumerate() {
        let AstNodeKind::Identifier(name) = kind else {
            continue;
        };
        if !bindings.contains_key(name) {
            diagnostics.push(Diagnostic {
                range: ast.spans[i].clone(),
                message: format!("unknown identifier: {}", name),
                severity: Severity::Error,
            });
        }
    }
}
