//! The canonical, pinned WIT editing grammar shared by query tests and formatting.
//! Semantic validation belongs to `wit-parser`, not this crate.

pub use tree_sitter_wit::LANGUAGE;

/// Load the grammar registered by the Zed extension.
pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}
