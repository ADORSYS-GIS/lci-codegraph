; Vendored from https://github.com/tree-sitter/tree-sitter-scala, tag `v0.26.2`
; (queries/tags.scm, commit b931fcc338390925eb893d70ad070033f5856ccf) — the exact version pinned as
; `tree-sitter-scala` in this crate's Cargo.toml. The published crate does not re-export this as a
; `TAGS_QUERY` constant the way `tree-sitter-python`/`-java`/`-javascript` do (see `src/lang/scala.rs`),
; so it is copied here verbatim rather than referenced. Re-vendor on every `tree-sitter-scala` bump —
; `Query::new` in `scala.rs` fails loudly (a panic on first use, caught by
; `graph_strategy_is_a_tags_query_not_rust_native`-style tests) if a grammar update renames a node kind
; this file references, so drift cannot pass silently.

; Definitions

(package_clause
  name: (package_identifier) @name) @definition.module

(trait_definition
  name: (identifier) @name) @definition.interface

(enum_definition
  name: (identifier) @name) @definition.enum

(simple_enum_case
  name: (identifier) @name) @definition.class

(full_enum_case
  name: (identifier) @name) @definition.class

(class_definition
  name: (identifier) @name) @definition.class

(object_definition
  name: (identifier) @name) @definition.object

(function_definition
  name: (identifier) @name) @definition.function

(val_definition
  pattern: (identifier) @name) @definition.variable

(given_definition
  name: (identifier) @name) @definition.variable

(var_definition
  pattern: (identifier) @name) @definition.variable

(val_declaration
  name: (identifier) @name) @definition.variable

(var_declaration
  name: (identifier) @name) @definition.variable

(type_definition
  name: (type_identifier) @name) @definition.type

(class_parameter
  name: (identifier) @name) @definition.property

; References

(call_expression
  (identifier) @name) @reference.call

(instance_expression
  (type_identifier) @name) @reference.interface

(instance_expression
  (generic_type
    (type_identifier) @name)) @reference.interface

(extends_clause
  (type_identifier) @name) @reference.class

(extends_clause
  (generic_type
    (type_identifier) @name)) @reference.class
