//! Integration tests for the full `lw-demo` pipeline.
//!
//! Tests exercise the complete pipeline: source → lex_file → parse → lower
//! → resolve → Salsa incremental diagnostics.

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

    assert!(
        result.errors.is_empty(),
        "unexpected parse errors: {:?}",
        result.errors
    );

    // CST: Root > LetBinding
    let root = result.tree.get(result.root);
    let CstNode::Root { children } = root else {
        panic!("expected Root, got {:?}", root);
    };
    let non_trivia: Vec<_> = children
        .iter()
        .filter(|&&id| {
            !matches!(
                result.tree.get(id),
                CstNode::Whitespace { .. } | CstNode::Comment { .. }
            )
        })
        .collect();
    assert_eq!(non_trivia.len(), 1, "expected one LetBinding child");

    let binding = result.tree.get(*non_trivia[0]);
    let CstNode::LetBinding { value, .. } = binding else {
        panic!("expected LetBinding, got {:?}", binding);
    };
    assert!(
        matches!(result.tree.get(*value), CstNode::BinaryExpr { .. }),
        "expected BinaryExpr, got {:?}",
        result.tree.get(*value)
    );

    // AST: typed Expr children
    let lr = lower(&result.arena, &result.tree, result.root);
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
    let lr = lower(&pr.arena, &pr.tree, pr.root);
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

    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.is_empty(),
        "step 1: expected zero diagnostics, got: {msgs:?}"
    );

    db.set_file("math.lw", "");
    let msgs = diag_messages(&db, "main.lw");
    assert!(
        msgs.iter()
            .any(|m| m.contains("unknown identifier") && m.contains("pi")),
        "step 2: expected 'unknown identifier: pi', got: {msgs:?}"
    );

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
// Test 6: Typed AST structure — BinaryExpr has Expr children
// ---------------------------------------------------------------------------

#[test]
fn ast_binary_expr_has_typed_expr_children() {
    let source = "let z = 10 + 20 * 30";
    let pr = parse(source);
    let lr = lower(&pr.arena, &pr.tree, pr.root);
    assert!(lr.errors.is_empty());

    let Stmt::Let { value, .. } = lr.ast.stmt(lr.ast.top_level[0]) else {
        panic!("expected Let");
    };

    fn walk_expr(ast: &lw_ast::Ast, id: lw_ast::ExprId) -> usize {
        match ast.expr(id) {
            Expr::Binary { lhs, rhs, .. } => 1 + walk_expr(ast, *lhs) + walk_expr(ast, *rhs),
            Expr::IntLiteral(_) | Expr::Identifier(_) => 1,
            Expr::Missing => 1,
        }
    }

    let count = walk_expr(&lr.ast, *value);
    assert!(
        count >= 5,
        "expected at least 5 expression nodes, got {count}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Integer overflow produces diagnostic
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
// Test 9: Comments preserved in CST, stripped from AST
// ---------------------------------------------------------------------------

#[test]
fn comments_preserved_in_cst_stripped_from_ast() {
    let source = "// a comment\nlet x = 1";
    let pr = parse(source);
    assert!(pr.errors.is_empty());

    let CstNode::Root { children } = pr.tree.get(pr.root) else {
        panic!("expected Root");
    };
    let has_comment = children
        .iter()
        .any(|&id| matches!(pr.tree.get(id), CstNode::Comment { .. }));
    assert!(has_comment, "CST should contain a Comment node");

    let lr = lower(&pr.arena, &pr.tree, pr.root);
    assert_eq!(lr.ast.top_level.len(), 1);
    assert!(matches!(lr.ast.stmt(lr.ast.top_level[0]), Stmt::Let { .. }));
}

// ---------------------------------------------------------------------------
// Test 10: Line index integration — positions are computed on demand
// ---------------------------------------------------------------------------

#[test]
fn line_index_computes_positions_on_demand() {
    let source = "let x = 1\nlet y = 2";
    let pr = parse(source);
    assert!(pr.errors.is_empty());

    // The line index should report 2 lines.
    assert_eq!(pr.line_index.line_count(), 2);

    // byte offset 10 = start of "let y = 2" → line 1, col 0
    let pos = pr.line_index.byte_to_lsp_position(10);
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
}

// ---------------------------------------------------------------------------
// Test 11: CstArena token text extraction
// ---------------------------------------------------------------------------

#[test]
fn cst_arena_extracts_token_text() {
    let source = "let pi = 3";
    let pr = parse(source);

    // Tokens: let(0) ws(1) pi(2) ws(3) =(4) ws(5) 3(6)
    assert_eq!(pr.arena.token_text(2), "pi");
    assert_eq!(pr.arena.token_text(6), "3");
}
