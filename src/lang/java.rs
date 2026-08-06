//! Java language support (ADR-0086 language expansion). Definitions + call references come from the
//! grammar's bundled `tags.scm` query, run by [`crate::tags`].

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

pub struct Java;

impl LanguageSupport for Java {
    fn id(&self) -> &'static str {
        "java"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), tree_sitter_java::TAGS_QUERY)
                .expect("bundled java tags.scm query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Java.id(), "java");
        assert_eq!(Java.extensions(), &["java"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            Java.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_java_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Java.ts_language()).unwrap();
        let tree = parser.parse("class C { void m() {} }\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
