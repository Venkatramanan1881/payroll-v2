-- 0006 Audit log and transactional outbox.
-- Audit: DOMAIN_MODEL §2.14, BR-EM-17, INV-14, SKILL §6. Outbox: DOMAIN_MODEL §7, DEC-03.

CREATE TYPE app.actor_kind AS ENUM ('user', 'system', 'vendor');

-- Append-only. Written in the same transaction as the change it records.
-- Never holds PINs, passwords, tokens or other secrets.
CREATE TABLE app.audit_log (
  id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id         uuid NOT NULL REFERENCES app.organisations (id),
  occurred_at    timestamptz NOT NULL DEFAULT now(),
  actor_kind     app.actor_kind NOT NULL,
  actor_id       uuid,                                  -- user or vendor user; NULL for system
  action         text NOT NULL CHECK (btrim(action) <> ''),     -- operation code, e.g. employee.status.approve
  target_type    text NOT NULL,
  target_id      uuid,
  old_values     jsonb,
  new_values     jsonb,
  reason         text,
  correlation_id uuid,                                  -- ties together the entries of one request
  CHECK (actor_kind = 'system' OR actor_id IS NOT NULL)
);
CREATE INDEX audit_log_target_idx ON app.audit_log (org_id, target_type, target_id, occurred_at);
CREATE INDEX audit_log_time_idx   ON app.audit_log (org_id, occurred_at);

CREATE TRIGGER audit_log_no_update_delete BEFORE UPDATE OR DELETE ON app.audit_log
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();
CREATE TRIGGER audit_log_no_truncate BEFORE TRUNCATE ON app.audit_log
  FOR EACH STATEMENT EXECUTE FUNCTION app.reject_change();

SELECT app.enable_tenant_rls('app.audit_log');
GRANT SELECT, INSERT ON app.audit_log TO app_api;

-- Outbox: events written in the business transaction, delivered after commit through Cloud Tasks.
-- Payloads carry ids and dates only, never personal details or secrets.
CREATE TABLE app.outbox_events (
  id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id         uuid NOT NULL REFERENCES app.organisations (id),
  event_type     text NOT NULL,                         -- EmployeeCreated, JobChanged, ...
  aggregate_type text NOT NULL,
  aggregate_id   uuid NOT NULL,
  payload        jsonb NOT NULL,
  occurred_at    timestamptz NOT NULL DEFAULT now(),
  dispatched_at  timestamptz,
  attempts       integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  last_error     text
);
CREATE INDEX outbox_events_pending_idx ON app.outbox_events (id) WHERE dispatched_at IS NULL;

SELECT app.enable_tenant_rls('app.outbox_events');
-- The dispatcher reads every organisation's pending events, and nothing else.
CREATE POLICY dispatcher_all ON app.outbox_events TO app_outbox USING (true) WITH CHECK (true);

GRANT INSERT ON app.outbox_events TO app_api;
GRANT SELECT, UPDATE (dispatched_at, attempts, last_error) ON app.outbox_events TO app_outbox;

-- Identity columns: allow the API to insert into tables with generated ids.
GRANT USAGE ON ALL SEQUENCES IN SCHEMA app TO app_api;
