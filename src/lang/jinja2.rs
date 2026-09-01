//! Jinja2 language support (ADR-0086 language expansion). A templating language, not a call graph:
//! the grammar ships no `tags.scm` upstream (only `highlights.scm`), so `graph_strategy` is `None`.
//! Registering it still buys a real parse — and thus structured error detection — over a template
//! file that today only gets the windowed-text fallback.

use tree_sitter::Language;

use super::{GraphStrategy, LanguageSupport};

pub struct Jinja2;

impl LanguageSupport for Jinja2 {
    fn id(&self) -> &'static str {
        "jinja2"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jinja", "jinja2", "j2"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_jinja2::LANGUAGE.into()
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
        assert_eq!(Jinja2.id(), "jinja2");
        assert_eq!(Jinja2.extensions(), &["jinja", "jinja2", "j2"]);
    }

    #[test]
    fn has_no_graph_strategy() {
        assert!(Jinja2.graph_strategy().is_none());
    }

    #[test]
    fn ts_language_parses_jinja2_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Jinja2.ts_language()).unwrap();
        let tree = parser.parse("{{ a }}\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
