-- Dropping the audit log destroys history. Only ever run against a development database.
DROP TABLE app.outbox_events;
DROP TABLE app.audit_log;
DROP TYPE app.actor_kind;
