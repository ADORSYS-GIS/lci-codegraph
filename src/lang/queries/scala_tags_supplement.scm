; Local supplement to the vendored upstream query (scala_tags.scm) — NOT part of that file, so it
; survives a re-vendor untouched and stays easy to diff against upstream on a version bump.
;
; The upstream query's only `@reference.call` pattern is `(call_expression (identifier) @name)`,
; which matches a bare call (`helper()`) but not a qualified/member call: `Foo.helper()`, `x.foo()`,
; and `this.foo()` all parse as `call_expression function: (field_expression field: (identifier))`,
; a different node shape the upstream pattern never matches. Verified empirically against
; tree-sitter-scala 0.26.2: without this pattern, a qualified call produces zero `calls` edges at
; all — the graph only ever sees unqualified calls, which is a small slice of real Scala. Composed
; with the vendored query the same way `typescript.rs` composes JS+TS — see `scala.rs`.

(call_expression
  function: (field_expression
    field: (identifier) @name)) @reference.call
