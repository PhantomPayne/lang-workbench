//! Integration tests for the full `lw-demo` pipeline.
//!
//! Tests exercise the complete pipeline: source → lex → parse → lower → resolve
//! → Salsa incremental diagnostics.

use lw_ast::{Expr, Stmt};
use lw_cst::CstNode;
use lw_demo::{
    lower::lower,
    parser::parse,
    salsa_db::{Database, file_diagnostics},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect diagnostic messages from the Salsa DB for a file.
fn diag_messages(db: &Database, path: &str) -> Vec<String> {
    let file = db.get_file(path).expect("file not found in DB");
    file_diagnostics(db, file)
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1: Parse single file, no imports
// ---------------------------------------------------------------------------

#[test]
fn single_file_parse_and_lower() {
    let source = "let x = 1 + 2";
    let result = parse(source);

    // Zero parse errors.
    assert!(
        result.errors.is_empty(),
        "unexpected parse errors: {:?}",
        result.errors
    );

    // CST: Root > LetBinding
    let root = result.arena.get(result.root);
    let CstNode::Root { children } = root else {
        panic!("expected Root, got {:?}", root);
    };
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

    // CST: LetBinding > BinaryExpr
    let binding = result.arena.get(*non_trivia[0]);
    let CstNode::LetBinding { value, .. } = binding else {
        panic!("expected LetBinding, got {:?}", binding);
    };
    assert!(
        matches!(result.arena.get(*value), CstNode::BinaryExpr { .. }),
        "expected BinaryExpr, got {:?}",
        result.arena.get(*value)
    );

    // AST: typed Expr children
    let lr = lower(&result.arena, result.root, source);
    assert!(lr.errors.is_empty());
    assert_eq!(lr.ast.top_level.len(), 1);
    let stmt = lr.ast.stmt(lr.ast.top_level[0]);
    let Stmt::Let { name, value } = stmt else {
        panic!("expected Let stmt");
    };
    assert_eq!(name, "x");
    let Expr::Binary { lhs, rhs, .. } = lr.ast.expr(*value) else {
        panic!("expected Binary expr");
    };
    assert_eq!(lr.ast.expr(*lhs), &Expr::IntLiteral(1));
    assert_eq!(lr.ast.expr(*rhs), &Expr::IntLiteral(2));

    // Salsa DB: zero diagnostics.
    let mut db = Database::new();
    db.set_file("main.lw", source);
    assert!(diag_messages(&db, "main.lw").is_empty());
}

// ---------------------------------------------------------------------------
// Test 2: Import not found
// ---------------------------------------------------------------------------

#[test]
fn import_not_found() {
    let mut db = Database::new();
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10");

    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.iter()
            .any(|m| m.contains("import not found") && m.contains("math.lw")),
        "expected 'import not found: math.lw', got: {msgs:?}"
    );
    assert!(
        msgs.iter()
            .any(|m| m.contains("unknown identifier") && m.contains("pi")),
        "expected 'unknown identifier: pi', got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Import found — cross-file resolution
// ---------------------------------------------------------------------------

#[test]
fn import_found_cross_file() {
    let mut db = Database::new();
    db.set_file("math.lw", "let pi = 3");
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10 * 10");

    let msgs = diag_messages(&db, "main.lw");
    assert!(msgs.is_empty(), "expected zero diagnostics, got: {msgs:?}");

    // Verify the AST for `main.lw` contains the `area` binding
    // (imported `pi` is defined in `math.lw`, not in main's AST).
    let source = "import \"math.lw\"\nlet area = pi * 10 * 10";
    let pr = parse(source);
    let lr = lower(&pr.arena, pr.root, source);
    let binding_names: Vec<&str> = lr
        .ast
        .top_level
        .iter()
        .filter_map(|&id| {
            if let Stmt::Let { name, .. } = lr.ast.stmt(id) {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        binding_names.contains(&"area"),
        "expected 'area' binding, got: {binding_names:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Salsa incremental invalidation
// ---------------------------------------------------------------------------

#[test]
fn salsa_incremental_invalidation() {
    let mut db = Database::new();
    db.set_file("math.lw", "let pi = 3");
    db.set_file("main.lw", "import \"math.lw\"\nlet area = pi * 10 * 10");

    // Step 1: initial state — zero diagnostics.
    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.is_empty(),
        "step 1: expected zero diagnostics, got: {msgs:?}"
    );

    // Step 2: remove pi from math.lw — unknown identifier should appear.
    db.set_file("math.lw", "");
    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.iter()
            .any(|m| m.contains("unknown identifier") && m.contains("pi")),
        "step 2: expected 'unknown identifier: pi', got: {msgs:?}"
    );

    // Step 3: restore math.lw — zero diagnostics again.
    db.set_file("math.lw", "let pi = 3");
    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.is_empty(),
        "step 3: expected zero diagnostics after restore, got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Duplicate binding
// ---------------------------------------------------------------------------

#[test]
fn duplicate_binding() {
    let mut db = Database::new();
    db.set_file("main.lw", "let x = 1\nlet x = 2");

    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.iter()
            .any(|m| m.contains("duplicate binding") && m.contains("x")),
        "expected 'duplicate binding: x', got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Typed AST structure — BinaryExpr has Expr children, not IDs
// ---------------------------------------------------------------------------

#[test]
fn ast_binary_expr_has_typed_expr_children() {
    let source = "let z = 10 + 20 * 30";
    let pr = parse(source);
    let lr = lower(&pr.arena, pr.root, source);
    assert!(lr.errors.is_empty());

    let Stmt::Let { value, .. } = lr.ast.stmt(lr.ast.top_level[0]) else {
        panic!("expected Let");
    };

    // The outer expression is a BinaryExpr.  Its children are ExprId values
    // that can ONLY point at Expr nodes — this is enforced at the type level.
    fn walk_expr(ast: &lw_ast::Ast, id: lw_ast::ExprId) -> usize {
        match ast.expr(id) {
            Expr::Binary { lhs, rhs, .. } => 1 + walk_expr(ast, *lhs) + walk_expr(ast, *rhs),
            Expr::IntLiteral(_) | Expr::Identifier(_) => 1,
            Expr::Missing => 1,
        }
    }

    // Should have at least 5 nodes: (10 + (20 * 30)) or ((10 + 20) * 30)
    let count = walk_expr(&lr.ast, *value);
    assert!(
        count >= 5,
        "expected at least 5 expression nodes, got {count}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Integer overflow produces diagnostic, not silent 0
// ---------------------------------------------------------------------------

#[test]
fn integer_overflow_produces_diagnostic() {
    let mut db = Database::new();
    db.set_file("main.lw", "let big = 999999999999999999999999999999");

    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.iter().any(|m| m.contains("invalid integer literal")),
        "expected integer overflow diagnostic, got: {msgs:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: Empty file produces no diagnostics
// ---------------------------------------------------------------------------

#[test]
fn empty_file_no_diagnostics() {
    let mut db = Database::new();
    db.set_file("empty.lw", "");
    assert!(diag_messages(&db, "empty.lw").is_empty());
}

// ---------------------------------------------------------------------------
// Test 9: Comments are preserved in CST but stripped from AST
// ---------------------------------------------------------------------------

#[test]
fn comments_preserved_in_cst_stripped_from_ast() {
    let source = "// a comment\nlet x = 1";
    let pr = parse(source);
    assert!(pr.errors.is_empty());

    // CST should have a Comment node.
    let CstNode::Root { children } = pr.arena.get(pr.root) else {
        panic!("expected Root");
    };
    let has_comment = children
        .iter()
        .any(|&id| matches!(pr.arena.get(id), CstNode::Comment { .. }));
    assert!(has_comment, "CST should contain a Comment node");

    // AST should NOT contain comments — they're stripped during lowering.
    let lr = lower(&pr.arena, pr.root, source);
    assert_eq!(lr.ast.top_level.len(), 1);
    assert!(matches!(lr.ast.stmt(lr.ast.top_level[0]), Stmt::Let { .. }));
}
