# Keycloak Event Listener SPI

A Keycloak SPI (Service Provider Interface) plugin built with Maven, demonstrating extraction without Spring annotations and the invisibility of runtime ServiceLoader discovery.

## What this sample demonstrates

- **Java without Spring**: this is plain Java, wired by Keycloak's own factory pattern. The framework pass is gated on actual Spring imports in the source, so extraction stays completely silent here. Routes and external services: zero.
- **Cross-file method calls still resolve**: `AuditEventListenerProvider` calls `AuditSink.record()` and `AuditSink.flush()`, and these calls are visible despite the absence of any framework annotation.
- **ServiceLoader discovery is invisible**: the `src/main/resources/META-INF/services/org.keycloak.events.EventListenerProviderFactory` file wires `AuditEventListenerProviderFactory` to Keycloak's runtime. That binding is a string match in plain text, not source, so the graph cannot see it. A reviewer must know the convention.

## The graph

**20 nodes, 20 edges; 4 calls.** No route nodes, no external services. Cross-file calls resolve: factory methods call provider methods; providers call the sink. The meta-inf file is a plain text resource and does not appear as a node.

## What it does NOT show

The mapping between factory and SPI name in `META-INF/services` is runtime wiring, not statically analyzable from source. The graph shows that `AuditEventListenerProviderFactory` implements the factory interface, but not that it is *the* factory Keycloak loads for this plugin.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
