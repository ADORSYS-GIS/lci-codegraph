package com.example.keycloak;

import org.keycloak.Config;
import org.keycloak.events.EventListenerProvider;
import org.keycloak.events.EventListenerProviderFactory;
import org.keycloak.models.KeycloakSession;
import org.keycloak.models.KeycloakSessionFactory;

/**
 * Factory for AuditEventListenerProvider.
 * Keycloak discovers this via ServiceLoader.
 * No Spring annotations; implements Keycloak's factory SPI.
 */
public class AuditEventListenerProviderFactory implements EventListenerProviderFactory {

    private AuditSink sink;

    @Override
    public EventListenerProvider create(KeycloakSession session) {
        return new AuditEventListenerProvider(session, sink);
    }

    @Override
    public void init(Config.Scope config) {
        this.sink = new LoggingAuditSink();
    }

    @Override
    public void postInit(KeycloakSessionFactory factory) {
        // Initialization after factory setup
    }

    @Override
    public void close() {
        // Cleanup resources
    }

    @Override
    public String getId() {
        return "audit-listener";
    }
}
