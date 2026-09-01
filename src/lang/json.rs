//! JSON language support (ADR-0086 language expansion). JSON has no functions, classes, or calls —
//! there is nothing for a structural graph to extract, and the grammar ships no `tags.scm` upstream
//! (only `highlights.scm`) — so `graph_strategy` is `None`. Registering it still upgrades `.json`
//! from a bare language *tag* (see `from_path`'s fallback arms) to a real grammar: parsed, not just
//! windowed-chunked.

use tree_sitter::Language;

use super::{GraphStrategy, LanguageSupport};

pub struct Json;

impl LanguageSupport for Json {
    fn id(&self) -> &'static str {
        "json"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["json"]
    }

    fn ts_language(&self) -> Language {
        tree_sitter_json::LANGUAGE.into()
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
        assert_eq!(Json.id(), "json");
        assert_eq!(Json.extensions(), &["json"]);
    }

    #[test]
    fn has_no_graph_strategy() {
        assert!(Json.graph_strategy().is_none());
    }

    #[test]
    fn ts_language_parses_json_source_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&Json.ts_language()).unwrap();
        let tree = parser.parse("{\"a\": 1}\n", None).unwrap();
        assert!(!tree.root_node().has_error());
    }
}
