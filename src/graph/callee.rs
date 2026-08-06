//! Callee-reference extraction: given an AST node identified as a call site, recover its bare callee
//! name plus, when the call is qualified by a receiver/path (`A::new`, `Foo.bar`), the qualifier used
//! solely as an ambiguity tiebreaker by [`super::resolve::pick`]. Two independent sources feed the same
//! [`CalleeRef`] shape: the Rust `call_expression` navigation ([`callee_ref_of`]), and the tags-captured
//! callee name node for every other language ([`qualifier_from_callee_node`] — tags identify *that* a
//! node is a callee but drop the qualifier, so it is recovered here from the node's tree position).

use tree_sitter::Node;

/// The callee of a Rust `call_expression`: its bare name plus, for a qualified path, the qualifier
/// segment. `A::new()` → `{name: "new", qualifier: Some("A")}`; `a::b::foo()` → `{"foo", Some("b")}`;
/// `foo()` / `x.foo()` → no qualifier (a method receiver's type is not known without inference).
pub(super) struct CalleeRef {
    pub(super) name: String,
    pub(super) qualifier: Option<String>,
}

pub(super) fn callee_ref_of(call: &Node<'_>, bytes: &[u8]) -> Option<CalleeRef> {
    let func = call.child_by_field_name("function")?;
    callee_ref(&func, bytes)
}

fn callee_ref(node: &Node<'_>, bytes: &[u8]) -> Option<CalleeRef> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" => Some(CalleeRef {
            name: text(node, bytes)?,
            qualifier: None,
        }),
        // `a::b::foo` — callable is the final `name`; qualifier is the segment before it (`b`).
        "scoped_identifier" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| text(&n, bytes))?;
            let qualifier = node
                .child_by_field_name("path")
                .and_then(|p| path_tail(&p, bytes));
            Some(CalleeRef { name, qualifier })
        }
        // `x.foo` — the method name is the `field`; the receiver type is unknown, so no qualifier.
        "field_expression" => node
            .child_by_field_name("field")
            .and_then(|n| callee_ref(&n, bytes)),
        // `foo::<T>` — the callable is under the `function` field.
        "generic_function" => node
            .child_by_field_name("function")
            .and_then(|n| callee_ref(&n, bytes)),
        _ => None,
    }
}

/// The last segment of a path node — the receiver/namespace qualifier (`A` in `A`, `b` in `a::b`).
fn path_tail(node: &Node<'_>, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "scoped_identifier" => node
            .child_by_field_name("name")
            .and_then(|n| text(&n, bytes)),
        _ => text(node, bytes),
    }
}

/// The implementing type of an `impl_item` (`impl S` / `impl T for S` / `impl Vec<T>` → `S` / `Vec`),
/// used to scope the methods it contains for same-name disambiguation.
pub(super) fn impl_type_name(node: &Node<'_>, bytes: &[u8]) -> Option<String> {
    let ty = node.child_by_field_name("type")?;
    type_head_name(&ty, bytes)
}

fn type_head_name(node: &Node<'_>, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "type_identifier" => text(node, bytes),
        "generic_type" => node
            .child_by_field_name("type")
            .and_then(|n| type_head_name(&n, bytes)),
        "scoped_type_identifier" => node
            .child_by_field_name("name")
            .and_then(|n| text(&n, bytes)),
        _ => None,
    }
}

/// The qualifier for a tags-captured callee name node: its receiver, when the name is the property of
/// a member access (`Foo.bar()` → `Foo`; Python `obj.m()` / Java `Obj.m()`). A bare call
/// (`function`-field identifier) or a `self`/`this`/`cls`/`super` receiver yields no qualifier — the
/// call resolves on the bare name (single hit) or is dropped as ambiguous, never mis-attributed.
pub(super) fn qualifier_from_callee_node(name_node: &Node<'_>, bytes: &[u8]) -> Option<String> {
    let parent = name_node.parent()?;
    match parent.kind() {
        // JS/TS `obj.foo()`, Python `obj.foo()`, Java `obj.foo()` — the receiver is the `object` field.
        "member_expression" | "attribute" | "method_invocation" => parent
            .child_by_field_name("object")
            .and_then(|object| receiver_qualifier(&object, bytes)),
        _ => None,
    }
}

/// A receiver object as a qualifier, iff it is a plain identifier that could name a *type* (`Foo` in
/// `Foo.bar()`). Implicit-`self` receivers (`self`/`cls`/`this`/`super`) carry no type information, so
/// they yield no qualifier — the call resolves on the bare method name (single hit) or is dropped as
/// ambiguous, never mis-attributed by a bogus `self` qualifier.
fn receiver_qualifier(object: &Node<'_>, bytes: &[u8]) -> Option<String> {
    if object.kind() != "identifier" {
        return None;
    }
    let name = text(object, bytes)?;
    match name.as_str() {
        "self" | "cls" | "this" | "super" => None,
        _ => Some(name),
    }
}

fn text(node: &Node<'_>, bytes: &[u8]) -> Option<String> {
    std::str::from_utf8(&bytes[node.byte_range()])
        .ok()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    //! Unit tests for callee-reference parsing, isolated from the shared DFS walk in [`super::emit`].
    //! Real tree-sitter parses (never hand-mocked nodes) for both feed paths: the Rust
    //! `call_expression` navigation ([`callee_ref_of`]/[`impl_type_name`]), and the tags-captured
    //! callee name node other languages use ([`qualifier_from_callee_node`]).
    use super::*;
    use crate::lang;

    /// Parse Rust `src`, find the first node of `kind` in the tree (pre-order DFS), and pass it to
    /// `f`. Panics if no such node exists — test ergonomics.
    fn with_rust_node<R>(src: &str, kind: &str, f: impl FnOnce(&Node<'_>, &[u8]) -> R) -> R {
        let tree = lang::parse(src, "rust").expect("rust source parses");
        let bytes = src.as_bytes();
        let node = find_first(&tree.root_node(), kind)
            .unwrap_or_else(|| panic!("no {kind:?} node in {src:?}"));
        f(&node, bytes)
    }

    fn find_first<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(*node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_first(&child, kind) {
                return Some(found);
            }
        }
        None
    }

    // ── Rust `call_expression` navigation ──────────────────────────────────────────────────────

    #[test]
    fn bare_call_has_name_and_no_qualifier() {
        with_rust_node("fn go() { foo(); }", "call_expression", |call, bytes| {
            let r = callee_ref_of(call, bytes).expect("bare call has a callee ref");
            assert_eq!(r.name, "foo");
            assert_eq!(r.qualifier, None);
        });
    }

    #[test]
    fn scoped_call_captures_name_and_its_qualifier() {
        with_rust_node("fn go() { A::new(); }", "call_expression", |call, bytes| {
            let r = callee_ref_of(call, bytes).expect("scoped call has a callee ref");
            assert_eq!(r.name, "new");
            assert_eq!(r.qualifier.as_deref(), Some("A"));
        });
    }

    #[test]
    fn nested_scoped_call_qualifier_is_the_segment_before_the_name() {
        // `a::b::foo()` — callable is `foo`; the qualifier is the segment directly before it (`b`),
        // not the whole path.
        with_rust_node(
            "fn go() { a::b::foo(); }",
            "call_expression",
            |call, bytes| {
                let r = callee_ref_of(call, bytes).expect("nested scoped call has a callee ref");
                assert_eq!(r.name, "foo");
                assert_eq!(r.qualifier.as_deref(), Some("b"));
            },
        );
    }

    #[test]
    fn method_call_field_expression_has_no_qualifier() {
        // `x.foo()` — the receiver's type is unknown without inference, so no qualifier is recovered
        // (a bogus qualifier would risk mis-attribution).
        with_rust_node(
            "fn go(x: X) { x.foo(); }",
            "call_expression",
            |call, bytes| {
                let r = callee_ref_of(call, bytes).expect("method call has a callee ref");
                assert_eq!(r.name, "foo");
                assert_eq!(r.qualifier, None);
            },
        );
    }

    #[test]
    fn chained_method_call_resolves_to_the_final_segment() {
        // `a.b().c()` — the outer call's callee is `c`; the qualifier is still unknown (chained off
        // another call's result, not an identifier).
        with_rust_node(
            "fn go(a: A) { a.b().c(); }",
            "call_expression",
            |call, bytes| {
                let r = callee_ref_of(call, bytes).expect("outer call has a callee ref");
                assert_eq!(r.name, "c");
                assert_eq!(r.qualifier, None);
            },
        );
    }

    #[test]
    fn generic_function_call_unwraps_to_the_inner_callee() {
        with_rust_node(
            "fn go() { foo::<T>(); }",
            "call_expression",
            |call, bytes| {
                let r = callee_ref_of(call, bytes).expect("turbofish call has a callee ref");
                assert_eq!(r.name, "foo");
                assert_eq!(r.qualifier, None);
            },
        );
    }

    #[test]
    fn non_call_node_yields_no_callee_ref() {
        // An `index_expression` has no `function` field — `callee_ref_of` must yield `None`, not
        // panic or misparse.
        with_rust_node(
            "fn go(arr: [i32; 3]) { let _ = arr[0]; }",
            "index_expression",
            |node, bytes| {
                assert!(callee_ref_of(node, bytes).is_none());
            },
        );
    }

    // ── `impl_type_name` ────────────────────────────────────────────────────────────────────────

    #[test]
    fn impl_type_name_for_a_plain_inherent_impl() {
        with_rust_node("impl S { fn f() {} }", "impl_item", |node, bytes| {
            assert_eq!(impl_type_name(node, bytes).as_deref(), Some("S"));
        });
    }

    #[test]
    fn impl_type_name_for_a_trait_impl_is_the_implementing_type_not_the_trait() {
        with_rust_node("impl T for S { fn f() {} }", "impl_item", |node, bytes| {
            assert_eq!(impl_type_name(node, bytes).as_deref(), Some("S"));
        });
    }

    #[test]
    fn impl_type_name_for_a_generic_impl_uses_the_head_type() {
        with_rust_node("impl Vec<i32> { fn f() {} }", "impl_item", |node, bytes| {
            assert_eq!(impl_type_name(node, bytes).as_deref(), Some("Vec"));
        });
    }

    // ── `qualifier_from_callee_node` (tags-captured callee name node) ─────────────────────────────

    /// Parse `src` with `language`, find the first node of `kind`, and pass it to `f`.
    fn with_tagged_node<R>(
        src: &str,
        language: &str,
        kind: &str,
        f: impl FnOnce(&Node<'_>, &[u8]) -> R,
    ) -> R {
        let tree = lang::parse(src, language).expect("source parses");
        let bytes = src.as_bytes();
        let node = find_first(&tree.root_node(), kind)
            .unwrap_or_else(|| panic!("no {kind:?} node in {src:?}"));
        f(&node, bytes)
    }

    #[test]
    fn js_member_expression_receiver_is_the_qualifier() {
        // `Foo.bar()` — the callee name node is the `property` field; its qualifier is the receiver.
        with_tagged_node(
            "Foo.bar();",
            "javascript",
            "member_expression",
            |member, bytes| {
                let name_node = member.child_by_field_name("property").unwrap();
                assert_eq!(
                    qualifier_from_callee_node(&name_node, bytes).as_deref(),
                    Some("Foo")
                );
            },
        );
    }

    #[test]
    fn python_attribute_receiver_is_the_qualifier() {
        with_tagged_node("obj.m()", "python", "attribute", |attr, bytes| {
            let name_node = attr.child_by_field_name("attribute").unwrap();
            assert_eq!(
                qualifier_from_callee_node(&name_node, bytes).as_deref(),
                Some("obj")
            );
        });
    }

    #[test]
    fn java_method_invocation_receiver_is_the_qualifier() {
        with_tagged_node(
            "class C { void go() { obj.m(); } }",
            "java",
            "method_invocation",
            |call, bytes| {
                let name_node = call.child_by_field_name("name").unwrap();
                assert_eq!(
                    qualifier_from_callee_node(&name_node, bytes).as_deref(),
                    Some("obj")
                );
            },
        );
    }

    #[test]
    fn implicit_self_family_receivers_yield_no_qualifier() {
        // `self`/`cls`/`this`/`super` carry no type information — a call through any of them must
        // resolve on the bare method name, never a bogus qualifier.
        for receiver in ["self", "cls", "this", "super"] {
            let src = format!("{receiver}.m();");
            with_tagged_node(&src, "javascript", "member_expression", |member, bytes| {
                let name_node = member.child_by_field_name("property").unwrap();
                assert_eq!(
                    qualifier_from_callee_node(&name_node, bytes),
                    None,
                    "{receiver} must not yield a qualifier"
                );
            });
        }
    }

    #[test]
    fn non_identifier_receiver_yields_no_qualifier() {
        // `getObj().bar()` — the receiver is a call expression, not a plain identifier that could
        // name a type, so no qualifier is recovered.
        with_tagged_node(
            "getObj().bar();",
            "javascript",
            "member_expression",
            |member, bytes| {
                let name_node = member.child_by_field_name("property").unwrap();
                assert_eq!(qualifier_from_callee_node(&name_node, bytes), None);
            },
        );
    }

    #[test]
    fn bare_call_name_node_yields_no_qualifier() {
        // `foo()` — the name node's parent is the call itself, not a member/attribute access.
        with_tagged_node("foo();", "javascript", "call_expression", |call, bytes| {
            let name_node = call.child_by_field_name("function").unwrap();
            assert_eq!(qualifier_from_callee_node(&name_node, bytes), None);
        });
    }
}
