//! Python language support. Definitions + call references come from the grammar's bundled `tags.scm`
//! query (ADR-0086 language expansion), run by [`crate::tags`].

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

pub struct Python;

impl LanguageSupport for Python {
    fn id(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), tree_sitter_python::TAGS_QUERY)
                .expect("bundled python tags.scm query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Python.id(), "python");
        assert_eq!(Python.extensions(), &["py"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            Python.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_python_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Python.ts_language()).unwrap();
        let tree = parser.parse("def a():\n    pass\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
