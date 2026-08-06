//! Rust language support. Rust is special: it keeps its **own** tree-sitter node-kind extractor
//! (the chunker's `interesting_node` plus the Rust `call_expression` navigation in [`crate::graph`])
//! rather than a `tags.scm` query, so chunk + graph symbols stay in lock-step and the committed
//! golden is byte-stable (ADR-0086 "Rust language first").

use tree_sitter::Language;

use super::{GraphStrategy, LanguageSupport};

pub struct Rust;

impl LanguageSupport for Rust {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        Some(GraphStrategy::RustNative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Rust.id(), "rust");
        assert_eq!(Rust.extensions(), &["rs"]);
    }

    #[test]
    fn graph_strategy_is_rust_native_not_tags() {
        assert!(matches!(
            Rust.graph_strategy(),
            Some(GraphStrategy::RustNative)
        ));
    }

    #[test]
    fn ts_language_parses_rust_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Rust.ts_language()).unwrap();
        let tree = parser.parse("fn a() {}\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
