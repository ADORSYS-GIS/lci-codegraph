//! PostgreSQL language support (ADR-0086 language expansion). No `tags.scm` ships upstream (only
//! `highlights.scm`/`injections.scm`/`outline.scm`), so `graph_strategy` is `None` — same tier as
//! [`super::json`] and [`super::jinja2`].
//!
//! `tree-sitter-postgres` also ships a second grammar, `LANGUAGE_PLPGSQL`, for PL/pgSQL function
//! bodies. It is deliberately **not** registered here: PL/pgSQL is not written to its own files in
//! practice — it lives dollar-quoted inside a `CREATE FUNCTION ... LANGUAGE plpgsql AS $$ ... $$`
//! statement in an ordinary `.sql` file, which the `postgres` grammar below already parses (as an
//! opaque string body). There is no standalone file extension to route a second [`LanguageSupport`]
//! by; add one if a real need for parsing extracted PL/pgSQL bodies shows up.

use tree_sitter::Language;

use super::{GraphStrategy, LanguageSupport};

pub struct Postgres;

impl LanguageSupport for Postgres {
    fn id(&self) -> &'static str {
        "postgres"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["sql", "pgsql"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_postgres::LANGUAGE.into()
    }

    fn graph_strategy(&self) -> Option<GraphStrategy> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_extensions() {
        assert_eq!(Postgres.id(), "postgres");
        assert_eq!(Postgres.extensions(), &["sql", "pgsql"]);
    }

    #[test]
    fn has_no_graph_strategy() {
        assert!(Postgres.graph_strategy().is_none());
    }

    #[test]
    fn ts_language_parses_postgres_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Postgres.ts_language()).unwrap();
        let tree = parser.parse("SELECT 1;\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
