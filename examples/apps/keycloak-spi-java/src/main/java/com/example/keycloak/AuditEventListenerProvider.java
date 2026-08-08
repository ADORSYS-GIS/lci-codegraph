package com.example.keycloak;

import org.keycloak.events.Event;
import org.keycloak.events.EventListenerProvider;
import org.keycloak.events.admin.AdminEvent;
import org.keycloak.models.KeycloakSession;

/**
 * Keycloak event listener for audit logging.
 * Implements EventListenerProvider; wired by factory pattern.
 */
public class AuditEventListenerProvider implements EventListenerProvider {

    private final KeycloakSession session;
    private final AuditSink sink;

    public AuditEventListenerProvider(KeycloakSession session, AuditSink sink) {
        this.session = session;
        this.sink = sink;
    }

    @Override
    public void onEvent(Event event) {
        sink.record("user_event", event.getRealmId());
    }

    @Override
    public void onEvent(AdminEvent adminEvent, boolean includeRepresentation) {
        sink.record("admin_event", adminEvent.getRealmId());
    }

    @Override
    public void close() {
        sink.flush();
    }
}
