//! Dart language support (ADR-0086 language expansion). Definitions + call references come from the
//! grammar's bundled `tags.scm` query, run by [`crate::tags`].

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

pub struct Dart;

impl LanguageSupport for Dart {
    fn id(&self) -> &'static str {
        "dart"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["dart"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_dart::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), tree_sitter_dart::TAGS_QUERY)
                .expect("bundled dart tags.scm query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Dart.id(), "dart");
        assert_eq!(Dart.extensions(), &["dart"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            Dart.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_dart_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Dart.ts_language()).unwrap();
        let tree = parser.parse("void a() {}\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
