package com.example.keycloak;

import java.util.logging.Logger;

/**
 * Logging-based implementation of AuditSink.
 * Records events to stdout; no framework annotations.
 */
public class LoggingAuditSink implements AuditSink {

    private static final Logger logger = Logger.getLogger(LoggingAuditSink.class.getName());

    @Override
    public void record(String type, String realmId) {
        logger.info(String.format("Audit event recorded: type=%s, realm=%s", type, realmId));
        this.flush();
    }

    @Override
    public void flush() {
        logger.info("Flushing audit records");
    }
}
