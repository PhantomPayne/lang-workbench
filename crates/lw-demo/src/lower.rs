//! CST → AST lowering for the `lw-demo` language.
//!
//! Strips trivia (whitespace, comments) and maps [`CstNode`]s into the flat
//! columnar [`AstNodes`] representation.

use lw_ast::{AstNodeId, AstNodeKind, AstNodes};
use lw_cst::{CstArena, CstNode, CstNodeId, TextRange};

/// Lower a parsed CST into a flat [`AstNodes`] store.
///
/// `source` is the original source text, needed to extract string slices for
/// identifier names and import paths.
pub fn lower(arena: &CstArena, root: CstNodeId, source: &str) -> AstNodes {
    let mut nodes = AstNodes::new();
    lower_node(arena, root, source, None, &mut nodes);
    nodes
}

fn lower_node(
    arena: &CstArena,
    id: CstNodeId,
    source: &str,
    parent: Option<AstNodeId>,
    out: &mut AstNodes,
) -> Option<AstNodeId> {
    match arena.get(id) {
        CstNode::Root { children } => {
            // Allocate the Root node first to get its ID, then process children.
            let root_id = out.alloc(AstNodeKind::Root, TextRange::default(), parent);
            for &child in children {
                lower_node(arena, child, source, Some(root_id), out);
            }
            Some(root_id)
        }
        CstNode::LetBinding { name, value } => {
            let name_str = slice(source, name).to_string();
            // We need to allocate the LetBinding node, but the value node
            // isn't allocated yet. Use a placeholder and patch it after.
            let binding_id = out.alloc(
                AstNodeKind::LetBinding { name: name_str, value: 0 },
                name.clone(),
                parent,
            );
            // Lower the value expression.
            let value_id =
                lower_node(arena, *value, source, Some(binding_id), out).unwrap_or(binding_id);
            // Patch the value field.
            out.kinds[binding_id as usize] = AstNodeKind::LetBinding {
                name: match &out.kinds[binding_id as usize] {
                    AstNodeKind::LetBinding { name, .. } => name.clone(),
                    _ => unreachable!(),
                },
                value: value_id,
            };
            Some(binding_id)
        }
        CstNode::Import { path } => {
            // Strip the surrounding quotes from the string literal range.
            let raw = slice(source, path);
            let path_str = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw.to_string()
            };
            Some(out.alloc(AstNodeKind::Import { path: path_str }, path.clone(), parent))
        }
        CstNode::BinaryExpr { op, lhs, rhs } => {
            // Allocate BinaryExpr with placeholder children, then patch.
            let expr_id = out.alloc(
                AstNodeKind::BinaryExpr { op: op.clone(), lhs: 0, rhs: 0 },
                TextRange::default(),
                parent,
            );
            let lhs_id =
                lower_node(arena, *lhs, source, Some(expr_id), out).unwrap_or(expr_id);
            let rhs_id =
                lower_node(arena, *rhs, source, Some(expr_id), out).unwrap_or(expr_id);
            out.kinds[expr_id as usize] =
                AstNodeKind::BinaryExpr { op: op.clone(), lhs: lhs_id, rhs: rhs_id };
            Some(expr_id)
        }
        CstNode::Literal { kind: lw_cst::LiteralKind::Integer, range } => {
            let text = slice(source, range);
            let value: i64 = text.parse().unwrap_or(0);
            Some(out.alloc(AstNodeKind::IntLiteral(value), range.clone(), parent))
        }
        CstNode::Literal { kind: lw_cst::LiteralKind::Float, range } => {
            let text = slice(source, range);
            let value: f64 = text.parse().unwrap_or(0.0);
            Some(out.alloc(AstNodeKind::FloatLiteral(value), range.clone(), parent))
        }
        CstNode::Identifier { range } => {
            let name = slice(source, range).to_string();
            Some(out.alloc(AstNodeKind::Identifier(name), range.clone(), parent))
        }
        // Trivia and errors are stripped during lowering.
        CstNode::Whitespace { .. }
        | CstNode::Comment { .. }
        | CstNode::Error { .. } => None,
    }
}

fn slice<'a>(source: &'a str, range: &TextRange) -> &'a str {
    let start = range.start as usize;
    let end = (range.end as usize).min(source.len());
    &source[start..end]
}
