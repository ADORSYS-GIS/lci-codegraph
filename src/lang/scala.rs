//! Scala language support (ADR-0086 language expansion). Definitions + call references come from
//! the grammar's `tags.scm` query — **vendored** into `queries/scala_tags.scm` rather than referenced
//! from the crate, because `tree-sitter-scala` does not re-export a `TAGS_QUERY` constant even though
//! its own repository ships one. See that file's header for the exact source commit and the re-sync
//! obligation on every `tree-sitter-scala` version bump.
//!
//! Scala's `object` (a singleton, not present in any other language this crate supports) has no
//! ready-made bucket in [`crate::tags::map_def_kind`]'s vocabulary — it is mapped onto `"class"`
//! there, the closest existing graph-node kind, rather than dropped.
//!
//! The vendored query is **composed with a local supplement**
//! (`queries/scala_tags_supplement.scm`) the same way `typescript.rs` composes the JS and TS
//! queries — the upstream query alone has no pattern for a qualified/member call (`Foo.helper()`,
//! `x.foo()`), only a bare call (`helper()`); see the supplement file for the verified gap.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

const TAGS_QUERY: &str = include_str!("queries/scala_tags.scm");
const TAGS_QUERY_SUPPLEMENT: &str = include_str!("queries/scala_tags_supplement.scm");

/// The vendored upstream query plus the local qualified-call supplement, composed into one source.
fn composed_tags_source() -> String {
    format!("{TAGS_QUERY}\n{TAGS_QUERY_SUPPLEMENT}")
}

pub struct Scala;

impl LanguageSupport for Scala {
    fn id(&self) -> &'static str {
        "scala"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["scala", "sc"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_scala::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), &composed_tags_source())
                .expect("composed scala tags query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Scala.id(), "scala");
        assert_eq!(Scala.extensions(), &["scala", "sc"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            Scala.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_scala_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Scala.ts_language()).unwrap();
        let tree = parser
            .parse("object Hello { def a(): Unit = {} }\n", None)
            .unwrap();
        assert!(!tree.root_node().has_error());
    }
}
