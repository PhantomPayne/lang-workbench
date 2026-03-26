//! Integration tests for the full `lw-demo` pipeline.
//!
//! Five tests are provided:
//!
//! 1. Parse a single file with no imports — verify CST shape and zero diagnostics.
//! 2. Import not found — verify the right diagnostics are produced.
//! 3. Import found — cross-file resolution, zero diagnostics.
//! 4. Salsa incremental invalidation — changing an imported file propagates.
//! 5. Duplicate binding — verify duplicate-binding diagnostic.

use lw_cst::CstNode;
use lw_demo::{
    lower::lower,
    parser::parse,
    salsa_db::{Database, file_diagnostics},
};

// ---------------------------------------------------------------------------
// Test 1: single file, no imports
// ---------------------------------------------------------------------------

#[test]
fn test_parse_single_file_no_imports() {
    let source = "let x = 1 + 2";
    let result = parse(source);

    // Zero parse errors.
    assert!(
        result.errors.is_empty(),
        "unexpected parse errors: {:?}",
        result.errors
    );

    // Root node exists.
    let root = result.arena.get(result.root);
    let children = match root {
        CstNode::Root { children } => children,
        other => panic!("expected Root, got {:?}", other),
    };

    // Root > LetBinding
    let non_trivia: Vec<_> = children
        .iter()
        .filter(|&&id| {
            !matches!(
                result.arena.get(id),
                CstNode::Whitespace { .. } | CstNode::Comment { .. }
            )
        })
        .collect();
    assert_eq!(non_trivia.len(), 1, "expected one LetBinding child");

    let binding = result.arena.get(*non_trivia[0]);
    let value_id = match binding {
        CstNode::LetBinding { value, .. } => *value,
        other => panic!("expected LetBinding, got {:?}", other),
    };

    // value should be a BinaryExpr
    assert!(
        matches!(result.arena.get(value_id), CstNode::BinaryExpr { .. }),
        "expected BinaryExpr, got {:?}",
        result.arena.get(value_id)
    );

    // Also check through the Salsa DB — zero diagnostics.
    let mut db = Database::new();
    db.set_file("main.lw", source);
    let file = db.get_file("main.lw").unwrap();
    let diags = file_diagnostics(&db, file);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
}

// ---------------------------------------------------------------------------
// Test 2: import not found
// ---------------------------------------------------------------------------

#[test]
fn test_import_not_found() {
    let mut db = Database::new();
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10");

    let file = db.get_file("main.lw").unwrap();
    let diags = file_diagnostics(&db, file);

    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("import not found") && m.contains("math.lw")),
        "expected 'import not found: math.lw' diagnostic, got: {:?}",
        messages
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unknown identifier") && m.contains("pi")),
        "expected 'unknown identifier: pi' diagnostic, got: {:?}",
        messages
    );
}

// ---------------------------------------------------------------------------
// Test 3: import found — cross-file resolution
// ---------------------------------------------------------------------------

#[test]
fn test_import_found_cross_file() {
    let mut db = Database::new();
    db.set_file("math.lw", "let pi = 3");
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10 * 10");

    let file = db.get_file("main.lw").unwrap();
    let diags = file_diagnostics(&db, file);
    assert!(
        diags.is_empty(),
        "expected zero diagnostics, got: {:?}",
        diags
    );

    // Verify the AST has both bindings.
    let source = "import \"math.lw\"\nlet area = pi * 10 * 10";
    let pr = parse(source);
    let ast = lower(&pr.arena, pr.root, source);
    let binding_names: Vec<&str> = ast
        .kinds
        .iter()
        .filter_map(|k| {
            if let lw_ast::AstNodeKind::LetBinding { name, .. } = k {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        binding_names.contains(&"area"),
        "expected 'area' binding, got: {:?}",
        binding_names
    );
}

// ---------------------------------------------------------------------------
// Test 4: Salsa incremental invalidation
// ---------------------------------------------------------------------------

#[test]
fn test_salsa_incremental_invalidation() {
    let mut db = Database::new();
    db.set_file("math.lw", "let pi = 3");
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10 * 10");

    let main_file = db.get_file("main.lw").unwrap();

    // Step 1: initial state — zero diagnostics.
    let diags = file_diagnostics(&db, main_file);
    assert!(
        diags.is_empty(),
        "step 1: expected zero diagnostics, got: {:?}",
        diags
    );

    // Step 2: remove pi from math.lw — unknown identifier should appear.
    db.set_file("math.lw", "");
    let diags = file_diagnostics(&db, main_file);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unknown identifier") && d.message.contains("pi")),
        "step 2: expected 'unknown identifier: pi', got: {:?}",
        diags
    );

    // Step 3: restore math.lw — zero diagnostics again.
    db.set_file("math.lw", "let pi = 3");
    let diags = file_diagnostics(&db, main_file);
    assert!(
        diags.is_empty(),
        "step 3: expected zero diagnostics after restore, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Test 5: duplicate binding
// ---------------------------------------------------------------------------

#[test]
fn test_duplicate_binding() {
    let mut db = Database::new();
    db.set_file("main.lw", "let x = 1\nlet x = 2");

    let file = db.get_file("main.lw").unwrap();
    let diags = file_diagnostics(&db, file);

    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("duplicate binding") && d.message.contains("x")),
        "expected 'duplicate binding: x' diagnostic, got: {:?}",
        diags
    );
}
