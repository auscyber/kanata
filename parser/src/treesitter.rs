//! Optional Tree-sitter helpers for the `kanata` configuration language.

use anyhow::Result;

/// Parse `source` using the provided `tree_sitter::Language`.
pub fn parse_with_language(
    source: &str,
) -> Result<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_kanata::LANGUAGE.into())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed"))?;
    Ok(tree)
}
