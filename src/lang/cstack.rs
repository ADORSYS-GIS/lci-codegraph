//! CrateStack `.cstack` schema support. Definitions come from the grammar's bundled `tags.scm`
//! query (the same path every non-Rust language takes), run by [`crate::tags`].
//!
//! **`.cstack` contributes definitions and no call sites, permanently.** It is a declarative schema
//! language: a `procedure` is *declared* in the schema and implemented in Rust elsewhere, and
//! nothing inside a schema invokes anything else in it. The grammar's `tags.scm` therefore emits no
//! `@reference.call`, so these files produce graph nodes and zero `calls` edges.
//!
//! That is the language, not a gap in the query, and it is exactly the case AGENTS.md §4 is about:
//! a resolution rate over these files must render `n/a (no call sites)` rather than `0.0%`, because
//! nothing was ever recorded to resolve. The `type` references a schema *does* contain are emitted
//! as `@reference.type`, which [`crate::tags`] skips by design.
//!
//! The grammar's tag vocabulary is mapped onto the convention's Java/JS-shaped terms, which have no
//! word for "schema declaration": `model`/`type`/`view` → class, `mixin` → interface, `enum` → enum,
//! `procedure` → function.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::{GraphStrategy, LanguageSupport};

pub struct Cstack;

impl LanguageSupport for Cstack {
    fn id(&self) -> &'static str {
        "cstack"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cstack"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_cstack::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        static QUERY: OnceLock<Query> = OnceLock::new();
        let query = QUERY.get_or_init(|| {
            Query::new(&self.ts_language(), tree_sitter_cstack::TAGS_QUERY)
                .expect("bundled cstack tags.scm query compiles")
        });
        Some(GraphStrategy::Tags(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "mixin Timestamps {\n  createdAt DateTime\n}\n\n\
                          enum Role {\n  Admin\n}\n\n\
                          model User {\n  id Int @id\n  role Role\n  @use(Timestamps)\n}\n\n\
                          procedure ping(): Int\n";

    #[test]
    fn id_and_extensions() {
        assert_eq!(Cstack.id(), "cstack");
        assert_eq!(Cstack.extensions(), &["cstack"]);
    }

    #[test]
    fn graph_strategy_is_a_tags_query_not_rust_native() {
        assert!(matches!(
            Cstack.graph_strategy(),
            Some(GraphStrategy::Tags(_))
        ));
    }

    #[test]
    fn ts_language_parses_cstack_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Cstack.ts_language()).unwrap();
        let tree = parser.parse(SCHEMA, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    /// AGENTS.md §11: a tags query that matched nothing would fail open — every declaration
    /// silently absent, with no error anywhere. Assert the capture names actually fire.
    #[test]
    fn tags_query_classifies_every_declaration_kind() {
        let language = Cstack.ts_language();
        let query = Query::new(&language, tree_sitter_cstack::TAGS_QUERY).unwrap();

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(SCHEMA, None).unwrap();

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut matches = cursor.matches(&query, tree.root_node(), SCHEMA.as_bytes());
        while let Some(m) = tree_sitter::StreamingIterator::next(&mut matches) {
            for capture in m.captures {
                seen.insert(query.capture_names()[capture.index as usize]);
            }
        }

        for expected in [
            "definition.class",
            "definition.interface",
            "definition.enum",
            "definition.function",
        ] {
            assert!(seen.contains(expected), "missing {expected}: {seen:?}");
        }
    }

    /// The permanent property this language has, pinned so a future grammar change cannot quietly
    /// start emitting call edges that would then be measured as a resolution rate.
    #[test]
    fn the_grammar_emits_no_call_references_because_cstack_has_no_call_sites() {
        let language = Cstack.ts_language();
        let query = Query::new(&language, tree_sitter_cstack::TAGS_QUERY).unwrap();

        assert!(
            !query.capture_names().contains(&"reference.call"),
            "`.cstack` is declarative; a call reference here would be a grammar bug",
        );
    }
}
