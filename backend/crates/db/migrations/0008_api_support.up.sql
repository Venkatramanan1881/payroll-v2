-- 0008 Additions found while designing the Package 1 API (Phase 4). Additive only; for approval at Gate 4.
-- See docs/api/API_CONTRACTS/package1-employee-master.md §1.9.

-- AC-EM-04.2: a dropdown / multiple-choice option in use is deactivated, not deleted.
ALTER TABLE app.custom_field_definitions
  ADD COLUMN inactive_options text[] NOT NULL DEFAULT '{}';
GRANT UPDATE (inactive_options) ON app.custom_field_definitions TO app_api;

-- AC-EM-12.1/12.3/12.4: send each probation reminder once. Holds the probation end date the reminder
-- was sent for; extending probation makes it differ, so a new reminder goes out for the new date.
ALTER TABLE app.employment_periods
  ADD COLUMN probation_reminder_sent_for date;
GRANT UPDATE (probation_reminder_sent_for) ON app.employment_periods TO app_api;

-- AC-EM-19.5: email one-time code. Hash only; wrong codes share failed_attempts / locked_until with the PIN (AC-EM-19.6).
ALTER TABLE app.user_credentials
  ADD COLUMN otp_hash       text,
  ADD COLUMN otp_expires_at timestamptz,
  ADD CONSTRAINT otp_hash_has_expiry CHECK (otp_hash IS NULL OR otp_expires_at IS NOT NULL);

-- Idempotency-Key for create-type commands (API §1.6): a retried request returns the first response.
CREATE TABLE app.idempotency_keys (
  org_id          uuid NOT NULL REFERENCES app.organisations (id),
  user_id         uuid NOT NULL,
  idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 8 AND 100),
  request_hash    text NOT NULL,                       -- SHA-256 of method + route + body
  response_status smallint,                            -- NULL while the first request is still running
  response_body   jsonb,                               -- never holds a temporary PIN (those routes are not replayed)
  created_at      timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, user_id, idempotency_key),
  FOREIGN KEY (org_id, user_id) REFERENCES app.users (org_id, id)
);
CREATE INDEX idempotency_keys_age_idx ON app.idempotency_keys (created_at);   -- purge after 24 hours

-- Background report exports (AC-EM-22.3, TSD §7).
CREATE TYPE app.export_state AS ENUM ('queued', 'running', 'ready', 'failed', 'expired');

CREATE TABLE app.export_jobs (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id       uuid NOT NULL REFERENCES app.organisations (id),
  requested_by uuid NOT NULL,
  report_code  text NOT NULL CHECK (report_code ~ '^[a-z0-9_]+$'),     -- r10, r11, employees
  parameters   jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(parameters) = 'object'),
  state        app.export_state NOT NULL DEFAULT 'queued',
  file_ref     text,
  error        text,
  created_at   timestamptz NOT NULL DEFAULT now(),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  expires_at   timestamptz,                                           -- file kept 30 days
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, requested_by) REFERENCES app.users (org_id, id),
  CHECK (state <> 'ready' OR file_ref IS NOT NULL),
  CHECK (state <> 'failed' OR error IS NOT NULL)
);
CREATE INDEX export_jobs_user_idx ON app.export_jobs (org_id, requested_by, created_at DESC);

SELECT app.enable_tenant_rls('app.idempotency_keys');
SELECT app.enable_tenant_rls('app.export_jobs');
SELECT app.add_updated_at_trigger('app.export_jobs');
GRANT SELECT, INSERT, DELETE ON app.idempotency_keys TO app_api;
GRANT UPDATE (response_status, response_body) ON app.idempotency_keys TO app_api;
GRANT SELECT, INSERT ON app.export_jobs TO app_api;
GRANT UPDATE (state, file_ref, error, updated_at, completed_at, expires_at) ON app.export_jobs TO app_api;
