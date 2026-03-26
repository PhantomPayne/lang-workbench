//! Salsa incremental computation layer for `lw-demo`.
//!
//! Wires the parse → lower → resolve pipeline into Salsa so that only the
//! queries that depend on a changed file are re-executed.

use std::collections::HashMap;

use lw_ast::AstNodes;

use crate::{
    lower::lower,
    parser::{ParseResult, parse},
    resolver::{Diagnostic, ResolvedProgram, Severity, collect_bindings_from, check_identifiers_against},
};

// ---------------------------------------------------------------------------
// Salsa input
// ---------------------------------------------------------------------------

/// A source file tracked by Salsa.
///
/// Each open file in the VFS is represented as one `SourceFile` input.
/// When `text` changes, all downstream queries that depended on it are
/// automatically invalidated.
#[salsa::input]
pub struct SourceFile {
    /// The filesystem path of this file (used as its identity key).
    pub path: String,
    /// The raw source text.
    pub text: String,
}

// ---------------------------------------------------------------------------
// Salsa tracked queries
// ---------------------------------------------------------------------------

/// Parse a source file into a CST.
#[salsa::tracked]
pub fn parse_cst(db: &dyn Db, file: SourceFile) -> ParseResult {
    let text = file.text(db);
    parse(text)
}

/// Lower a source file's CST into an [`AstNodes`] store.
#[salsa::tracked]
pub fn lower_ast(db: &dyn Db, file: SourceFile) -> AstNodes {
    let text = file.text(db);
    let result = parse_cst(db, file);
    lower(&result.arena, result.root, text)
}

/// Collect all diagnostics for a file (parse errors + resolution errors).
///
/// This query establishes Salsa dependencies on every file it reads, so
/// changing an imported file automatically invalidates this query for the
/// importing file.
#[salsa::tracked]
pub fn file_diagnostics(db: &dyn Db, file: SourceFile) -> Vec<Diagnostic> {
    let text = file.text(db);
    let parse_result = parse_cst(db, file);
    let ast = lower_ast(db, file);

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Convert parse errors to diagnostics.
    for e in &parse_result.errors {
        diagnostics.push(Diagnostic {
            range: e.range.clone(),
            message: e.message.clone(),
            severity: Severity::Error,
        });
    }

    let mut bindings = HashMap::new();

    // Resolve imports — calling lower_ast on each imported file registers a
    // Salsa dependency, so changes to imported files propagate here.
    for kind in &ast.kinds {
        let lw_ast::AstNodeKind::Import { path: import_path } = kind else {
            continue;
        };
        match db.file_for_path(import_path) {
            Some(imported_file) => {
                let imported_ast = lower_ast(db, imported_file);
                collect_bindings_from(&imported_ast, &mut bindings, &mut diagnostics);
            }
            None => {
                // Find the span from the ast.
                let span = ast.kinds.iter().zip(ast.spans.iter()).find_map(|(k, s)| {
                    if let lw_ast::AstNodeKind::Import { path } = k {
                        if path == import_path {
                            return Some(s.clone());
                        }
                    }
                    None
                });
                diagnostics.push(Diagnostic {
                    range: span.unwrap_or_default(),
                    message: format!("import not found: {}", import_path),
                    severity: Severity::Error,
                });
            }
        }
    }

    // Collect own bindings.
    collect_bindings_from(&ast, &mut bindings, &mut diagnostics);

    // Check for unknown identifiers.
    check_identifiers_against(&ast, &bindings, &mut diagnostics);

    let _ = text; // keep the read recorded for Salsa
    diagnostics
}

// ---------------------------------------------------------------------------
// Database trait and implementation
// ---------------------------------------------------------------------------

/// Salsa database trait for the `lw-demo` pipeline.
#[salsa::db]
pub trait Db: salsa::Database {
    /// Look up a [`SourceFile`] by its path, if one has been registered.
    fn file_for_path(&self, path: &str) -> Option<SourceFile>;
}

/// Concrete Salsa database for the `lw-demo` pipeline.
#[salsa::db]
pub struct Database {
    storage: salsa::Storage<Self>,
    /// Path → SourceFile handle, maintained alongside Salsa storage.
    files: HashMap<String, SourceFile>,
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    /// Creates a new, empty database.
    pub fn new() -> Self {
        Self {
            storage: salsa::Storage::default(),
            files: HashMap::new(),
        }
    }

    /// Open or update a file.
    ///
    /// If the file already exists in the database its `text` field is updated
    /// (which Salsa tracks as an input change). Otherwise a new [`SourceFile`]
    /// input is created.
    pub fn set_file(&mut self, path: impl Into<String>, text: impl Into<String>) {
        let path = path.into();
        let text = text.into();
        if let Some(&existing) = self.files.get(&path) {
            existing.set_text(self).to(text);
        } else {
            let file = SourceFile::new(self, path.clone(), text);
            self.files.insert(path, file);
        }
    }

    /// Retrieve a [`SourceFile`] handle by path.
    pub fn get_file(&self, path: &str) -> Option<SourceFile> {
        self.files.get(path).copied()
    }
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl Db for Database {
    fn file_for_path(&self, path: &str) -> Option<SourceFile> {
        self.files.get(path).copied()
    }
}
