package com.example.keycloak;

/**
 * Contract for audit event storage.
 * Plain interface; no Spring annotations.
 */
public interface AuditSink {

    /**
     * Record an audit event.
     *
     * @param type the event type
     * @param realmId the realm identifier
     */
    void record(String type, String realmId);

    /**
     * Flush any buffered records to storage.
     */
    void flush();
}
