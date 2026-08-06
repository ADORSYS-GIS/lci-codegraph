//! JavaScript language support (`.js`/`.jsx`/`.mjs`/`.cjs`). The JavaScript grammar already parses
//! JSX, so `.jsx` shares it. Definitions + call references come from the grammar's bundled `tags.scm`
//! query, run by [`crate::tags`]. The JavaScript query also underpins TypeScript/TSX, which compose
//! it — see [`super::typescript`].

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

pub struct JavaScript;

impl LanguageSupport for JavaScript {
    fn id(&self) -> &'static str {
        "javascript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["js", "jsx", "mjs", "cjs"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_javascript::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), tree_sitter_javascript::TAGS_QUERY)
                .expect("bundled javascript tags.scm query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions_cover_js_jsx_mjs_cjs() {
        assert_eq!(JavaScript.id(), "javascript");
        assert_eq!(JavaScript.extensions(), &["js", "jsx", "mjs", "cjs"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            JavaScript.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_jsx_without_errors() {
        // The JS grammar already parses JSX, so `.jsx` shares it — confirm no error nodes.
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&JavaScript.ts_language()).unwrap();
        let tree = parser
            .parse("function App() { return <div/>; }\n", None)
            .unwrap();
        assert!(!tree.root_node().has_error());
    }
}
