//! Salsa incremental computation layer for `lw-demo`.
//!
//! Wires the parse → lower → resolve pipeline into Salsa so that only the
//! queries that depend on a changed file are re-executed.
//!
//! Only [`file_diagnostics`] is a `#[salsa::tracked]` query — that is
//! sufficient for end-to-end incrementality because Salsa records every read
//! of a [`SourceFile`]'s `text` field that happens during the query, including
//! reads for imported files.  All other pipeline steps are plain functions
//! called within the tracked query.

use std::collections::HashMap;

use salsa::Setter;

use lw_ast::{Ast, Stmt, StmtId};

use crate::{
    lower::lower,
    parser::parse,
    resolver::{Diagnostic, Severity, check_identifiers_against, collect_bindings_from},
};

// ---------------------------------------------------------------------------
// Salsa::Update for local types
// ---------------------------------------------------------------------------

// `file_diagnostics` returns `Vec<Diagnostic>`.  Salsa requires the element
// type to implement `salsa::Update`.  `Diagnostic` is defined in this crate
// so the orphan rule permits the impl here.
//
// We cannot use `#[derive(salsa::Update)]` because `Diagnostic` contains
// `TokenSpan` (from `lw-cst`), and adding a `salsa` dependency to `lw-cst`
// just for this derive would be inappropriate.  The manual impl is trivially
// correct for these plain-data types.
unsafe impl salsa::Update for Diagnostic {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        unsafe {
            if *old_pointer == new_value {
                false
            } else {
                *old_pointer = new_value;
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Salsa input
// ---------------------------------------------------------------------------

/// A source file tracked by Salsa.
#[salsa::input]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

// ---------------------------------------------------------------------------
// Pipeline helpers (not Salsa tracked — called inside tracked queries)
// ---------------------------------------------------------------------------

/// Parse a source file into a CST.
pub fn parse_cst(db: &dyn Db, file: SourceFile) -> crate::parser::ParseResult {
    let text = file.text(db);
    parse(&text)
}

/// Lower a source file's CST into a typed [`Ast`].
pub fn lower_ast(db: &dyn Db, file: SourceFile) -> Ast {
    let text = file.text(db);
    let result = parse(&text);
    lower(&result.arena, &result.tree, result.root).ast
}

// ---------------------------------------------------------------------------
// Salsa tracked query
// ---------------------------------------------------------------------------

/// Collect all diagnostics for a file (parse + lower + resolution errors).
#[salsa::tracked]
pub fn file_diagnostics(db: &dyn Db, file: SourceFile) -> Vec<Diagnostic> {
    let text = file.text(db);
    let parse_result = parse(&text);
    let lr = lower(&parse_result.arena, &parse_result.tree, parse_result.root);
    let ast = &lr.ast;

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Convert parse errors to diagnostics.
    for e in &parse_result.errors {
        diagnostics.push(Diagnostic {
            span: e.span,
            message: e.message.clone(),
            severity: Severity::Error,
        });
    }

    // Convert lowering errors to diagnostics.
    for e in &lr.errors {
        diagnostics.push(Diagnostic {
            span: e.span,
            message: e.message.clone(),
            severity: Severity::Error,
        });
    }

    let mut bindings: HashMap<String, StmtId> = HashMap::new();

    // Resolve imports.
    for &stmt_id in &ast.top_level {
        let Stmt::Import { path: import_path } = ast.stmt(stmt_id) else {
            continue;
        };
        let import_path = import_path.clone();
        match db.file_for_path(&import_path) {
            Some(imported_file) => {
                let imported_text = imported_file.text(db);
                let imported_result = parse(&imported_text);
                let imported_lr = lower(
                    &imported_result.arena,
                    &imported_result.tree,
                    imported_result.root,
                );
                collect_bindings_from(&imported_lr.ast, &mut bindings, &mut diagnostics);
            }
            None => {
                let span = *ast.stmt_span(stmt_id);
                diagnostics.push(Diagnostic {
                    span,
                    message: format!("import not found: {}", import_path),
                    severity: Severity::Error,
                });
            }
        }
    }

    collect_bindings_from(ast, &mut bindings, &mut diagnostics);
    check_identifiers_against(ast, &bindings, &mut diagnostics);

    diagnostics
}

// ---------------------------------------------------------------------------
// Database trait and implementation
// ---------------------------------------------------------------------------

#[salsa::db]
pub trait Db: salsa::Database {
    fn file_for_path(&self, path: &str) -> Option<SourceFile>;
}

#[salsa::db]
pub struct Database {
    storage: salsa::Storage<Self>,
    files: HashMap<String, SourceFile>,
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    pub fn new() -> Self {
        Self {
            storage: salsa::Storage::default(),
            files: HashMap::new(),
        }
    }

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
