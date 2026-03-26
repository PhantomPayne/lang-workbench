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

use lw_ast::AstNodes;

use crate::{
    lower::lower,
    parser::{ParseResult, parse},
    resolver::{Diagnostic, Severity, check_identifiers_against, collect_bindings_from},
};

// ---------------------------------------------------------------------------
// Salsa::Update for local types
// ---------------------------------------------------------------------------

// `file_diagnostics` returns `Vec<Diagnostic>`.  salsa requires the element
// type to implement `salsa::Update`.  `Diagnostic` is defined in this crate
// so the orphan rule permits the impl here.
unsafe impl salsa::Update for Diagnostic {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        // SAFETY: caller guarantees old_pointer is valid and aligned.
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
// Pipeline helpers (not Salsa tracked — called inside tracked queries)
// ---------------------------------------------------------------------------

/// Parse a source file into a CST.
///
/// Not a Salsa tracked query; call this from within a tracked function so
/// the read of `file.text(db)` is recorded as a dependency.
pub fn parse_cst(db: &dyn Db, file: SourceFile) -> ParseResult {
    let text = file.text(db);
    parse(&text)
}

/// Lower a source file's CST into an [`AstNodes`] store.
///
/// Not a Salsa tracked query; call this from within a tracked function.
pub fn lower_ast(db: &dyn Db, file: SourceFile) -> AstNodes {
    let text = file.text(db);
    let result = parse(&text);
    lower(&result.arena, result.root, &text)
}

// ---------------------------------------------------------------------------
// Salsa tracked query
// ---------------------------------------------------------------------------

/// Collect all diagnostics for a file (parse errors + resolution errors).
///
/// This is the only `#[salsa::tracked]` query in the pipeline.  Salsa
/// records every read of a `SourceFile`'s `text` field that occurs during
/// this query — including reads for imported files — so changing any
/// dependency automatically re-runs this query for files that depend on it.
#[salsa::tracked]
pub fn file_diagnostics(db: &dyn Db, file: SourceFile) -> Vec<Diagnostic> {
    let text = file.text(db);
    let parse_result = parse(&text);
    let ast = lower(&parse_result.arena, parse_result.root, &text);

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

    // Resolve imports — reading each imported file's text establishes a Salsa
    // dependency, so changes to imported files propagate here automatically.
    for (i, kind) in ast.kinds.iter().enumerate() {
        let lw_ast::AstNodeKind::Import { path: import_path } = kind else {
            continue;
        };
        match db.file_for_path(import_path) {
            Some(imported_file) => {
                let imported_text = imported_file.text(db);
                let imported_result = parse(&imported_text);
                let imported_ast =
                    lower(&imported_result.arena, imported_result.root, &imported_text);
                collect_bindings_from(&imported_ast, &mut bindings, &mut diagnostics);
            }
            None => {
                diagnostics.push(Diagnostic {
                    range: ast.spans[i].clone(),
                    message: format!("import not found: {}", import_path),
                    severity: Severity::Error,
                });
            }
        }
    }

    // Collect own bindings (detecting duplicates with imported bindings).
    collect_bindings_from(&ast, &mut bindings, &mut diagnostics);

    // Check for unknown identifiers.
    check_identifiers_against(&ast, &bindings, &mut diagnostics);

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
