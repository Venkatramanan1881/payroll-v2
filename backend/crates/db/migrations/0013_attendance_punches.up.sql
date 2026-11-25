-- 0013 Attendance — Punch collection: devices, geo-fences, raw punches (monthly partitions, DEC-48),
-- dedupe keys, flagged-punch reviews, adjudications, manual punches.
-- Design: docs/database/DATA_MODEL-attendance.md. Domain: DOMAIN_MODEL-attendance §2.6–2.12. SM-AT-2, 3, 4.

-- Devices (US-AT-10). Only the token's SHA-256 is stored (DEC-46).
CREATE TABLE app.devices (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL REFERENCES app.organisations (id),
  branch_id        uuid NOT NULL,
  serial           text NOT NULL CHECK (btrim(serial) <> ''),
  make             text NOT NULL DEFAULT 'eSSL',
  model            text,
  token_hash       text NOT NULL CHECK (token_hash ~ '^[0-9a-f]{64}$'),
  token_rotated_at timestamptz NOT NULL DEFAULT now(),
  last_contact_at  timestamptz,
  is_online        boolean NOT NULL DEFAULT false,     -- last health state, so each offline spell alerts once
  state            app.item_state NOT NULL DEFAULT 'active',
  row_version      integer NOT NULL DEFAULT 1,
  created_at       timestamptz NOT NULL DEFAULT now(),
  created_by       uuid DEFAULT app.current_user_id(),
  updated_at       timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, branch_id) REFERENCES app.branches (org_id, id)
);
CREATE UNIQUE INDEX devices_serial_uq ON app.devices (org_id, lower(serial));        -- INV-AT-04
CREATE INDEX devices_health_idx ON app.devices (org_id, last_contact_at) WHERE state = 'active';

-- Geo-fences (US-AT-13, SM-AT-3). A punch is checked against the version in force at its punch time (BR-AT-09).
CREATE TYPE app.fence_state AS ENUM ('draft', 'active', 'inactive');

CREATE TABLE app.geo_fences (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  branch_id   uuid NOT NULL,
  name        text NOT NULL CHECK (btrim(name) <> ''),
  state       app.fence_state NOT NULL DEFAULT 'draft',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, branch_id) REFERENCES app.branches (org_id, id)
);
CREATE INDEX geo_fences_branch_idx ON app.geo_fences (org_id, branch_id);

-- Insert-only. "In force at T" = latest valid_from <= T; a version with in_use = false switches the fence off.
CREATE TABLE app.geo_fence_versions (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id     uuid NOT NULL,
  fence_id   uuid NOT NULL,
  valid_from timestamptz NOT NULL DEFAULT now(),
  latitude   numeric(9, 6) NOT NULL CHECK (latitude BETWEEN -90 AND 90),
  longitude  numeric(9, 6) NOT NULL CHECK (longitude BETWEEN -180 AND 180),
  radius_m   integer NOT NULL CHECK (radius_m BETWEEN 20 AND 5000),     -- ATQ-09
  in_use     boolean NOT NULL,
  created_by uuid DEFAULT app.current_user_id(),
  UNIQUE (org_id, id),
  UNIQUE (fence_id, valid_from),
  FOREIGN KEY (org_id, fence_id) REFERENCES app.geo_fences (org_id, id)
);

-- Raw punches (BR-AT-01, BR-AT-02). Append-only, partitioned by month of the effective punch time (DEC-48).
-- Device punches keep the enrolment ID; the employee is the one holding that ID on the punch date
-- (employee_job_versions.enrolment_id is dated), so an unmapped punch needs no later update (DEC-44, DQ-AT-07).
CREATE TYPE app.punch_source    AS ENUM ('device', 'app', 'web', 'import');
CREATE TYPE app.punch_direction AS ENUM ('in', 'out', 'untagged');
CREATE TYPE app.punch_flag      AS ENUM ('outside_fence', 'poor_accuracy', 'no_location', 'clock_mismatch', 'late_sync',
                                         'overlap', 'off_day', 'branch_not_allowed', 'liveness_failed', 'unmapped');

CREATE TABLE app.raw_punches (
  id               uuid NOT NULL DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL,
  punched_at       timestamptz NOT NULL,         -- effective time (PunchTimeResolver: DEC-21, §3.4)
  source           app.punch_source NOT NULL,
  employee_id      uuid,                         -- NULL only for device punches whose ID wasn't mapped at receipt
  enrolment_id     text,
  device_id        uuid,
  import_id        uuid,
  direction        app.punch_direction NOT NULL DEFAULT 'untagged',
  source_time      timestamptz NOT NULL,         -- as sent: device clock, phone clock, file value, server time (web)
  uptime_time      timestamptz,                  -- offline app: time derived from the phone's uptime clock
  received_at      timestamptz NOT NULL DEFAULT now(),
  branch_id        uuid,                         -- where it was made (device's branch, nearest fence, file)
  latitude         numeric(9, 6),
  longitude        numeric(9, 6),
  accuracy_m       numeric(8, 1),
  fence_distance_m numeric(9, 1),
  fence_version_id uuid,
  selfie_ref       text,                         -- file deleted after 13 months (DEC-51); the punch stays
  client_ip        inet,
  idempotency_key  text NOT NULL CHECK (btrim(idempotency_key) <> ''),
  flags            app.punch_flag[] NOT NULL DEFAULT '{}',
  note             text CHECK (length(note) <= 200),
  created_by       uuid DEFAULT app.current_user_id(),
  PRIMARY KEY (id, punched_at),
  UNIQUE (org_id, id, punched_at),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, device_id)   REFERENCES app.devices (org_id, id),
  FOREIGN KEY (org_id, import_id)   REFERENCES app.attendance_imports (org_id, id),
  FOREIGN KEY (org_id, branch_id)   REFERENCES app.branches (org_id, id),
  CHECK (employee_id IS NOT NULL OR (source = 'device' AND enrolment_id IS NOT NULL)),
  CHECK ((latitude IS NULL) = (longitude IS NULL))
) PARTITION BY RANGE (punched_at);
CREATE INDEX raw_punches_employee_idx ON app.raw_punches (org_id, employee_id, punched_at);
CREATE INDEX raw_punches_unmapped_idx ON app.raw_punches (org_id, enrolment_id, punched_at) WHERE employee_id IS NULL;
CREATE INDEX raw_punches_flagged_idx  ON app.raw_punches (org_id, punched_at) WHERE flags <> '{}';
-- Rows never change, by any role (INV-AT-01). The selfie is deleted from the file store, not here.
CREATE TRIGGER raw_punches_immutable BEFORE UPDATE OR DELETE ON app.raw_punches
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();

-- Dedupe (INV-AT-02). Separate from raw_punches because a unique index on a partitioned table must
-- include punched_at, and a retried live push can get a different effective time.
-- Hash-partitioned on the key; keys older than 90 days are purged by the cleanup job.
CREATE TABLE app.punch_keys (
  org_id          uuid NOT NULL,
  source          app.punch_source NOT NULL,
  idempotency_key text NOT NULL,
  punch_id        uuid NOT NULL,
  punched_at      timestamptz NOT NULL,
  created_at      timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, source, idempotency_key)
) PARTITION BY HASH (idempotency_key);
CREATE INDEX punch_keys_age_idx ON app.punch_keys (created_at);

-- Flagged-punch review (US-AT-16, SM-AT-4). Decisions are final (DQ-AT-04).
CREATE TYPE app.review_state AS ENUM ('pending', 'approved', 'rejected');

CREATE TABLE app.punch_reviews (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id          uuid NOT NULL,
  punch_id        uuid NOT NULL,
  punched_at      timestamptz NOT NULL,
  employee_id     uuid NOT NULL,
  flags           app.punch_flag[] NOT NULL CHECK (flags <> '{}'),
  state           app.review_state NOT NULL DEFAULT 'pending',
  punched_by      uuid,                           -- user who made the punch, never the decider (BR-AT-08)
  decided_by      uuid,
  decided_at      timestamptz,
  decision_reason text,
  row_version     integer NOT NULL DEFAULT 1,
  created_at      timestamptz NOT NULL DEFAULT now(),
  updated_at      timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE (punch_id),
  FOREIGN KEY (org_id, punch_id, punched_at) REFERENCES app.raw_punches (org_id, id, punched_at),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  CHECK (decided_by IS NULL OR decided_by IS DISTINCT FROM punched_by),
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> '')
);
CREATE INDEX punch_reviews_pending_idx ON app.punch_reviews (org_id, punched_at) WHERE state = 'pending';

-- Manual punches (US-AT-18, RG-10/11, approved regularisations). Insert-only except voiding (PUN-08).
CREATE TYPE app.manual_punch_origin AS ENUM ('device_failure', 'outage_fill', 'regularisation', 'hr_correction');

CREATE TABLE app.manual_punches (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id            uuid NOT NULL,
  employee_id       uuid NOT NULL,
  punched_at        timestamptz NOT NULL,
  direction         app.punch_direction NOT NULL,
  branch_id         uuid,
  origin            app.manual_punch_origin NOT NULL,
  reason            text NOT NULL CHECK (btrim(reason) <> ''),
  batch_id          uuid,                         -- one manual-attendance or outage entry
  regularisation_id uuid,                         -- FK added in 0015
  created_at        timestamptz NOT NULL DEFAULT now(),
  created_by        uuid DEFAULT app.current_user_id(),
  voided_at         timestamptz,
  voided_by         uuid,
  void_reason       text,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, branch_id)   REFERENCES app.branches (org_id, id),
  CHECK ((origin = 'regularisation') = (regularisation_id IS NOT NULL)),
  CHECK (voided_at IS NULL OR btrim(coalesce(void_reason, '')) <> '')
);
CREATE INDEX manual_punches_employee_idx ON app.manual_punches (org_id, employee_id, punched_at) WHERE voided_at IS NULL;

-- Adjudications (US-AT-19, CON-06). Insert-only; the latest one for a punch wins.
CREATE TYPE app.adjudication_kind   AS ENUM ('exclude', 'include', 'move_date', 'change_direction');
CREATE TYPE app.adjudication_origin AS ENUM ('hr', 'review', 'regularisation');

CREATE TABLE app.punch_adjudications (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id            uuid NOT NULL,
  punch_id          uuid NOT NULL,
  punched_at        timestamptz NOT NULL,
  employee_id       uuid NOT NULL,
  kind              app.adjudication_kind NOT NULL,
  new_work_date     date,
  new_direction     app.punch_direction,
  origin            app.adjudication_origin NOT NULL,
  regularisation_id uuid,                         -- FK added in 0015
  reason            text NOT NULL CHECK (btrim(reason) <> ''),
  created_at        timestamptz NOT NULL DEFAULT now(),
  created_by        uuid DEFAULT app.current_user_id(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, punch_id, punched_at) REFERENCES app.raw_punches (org_id, id, punched_at),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  CHECK ((kind = 'move_date') = (new_work_date IS NOT NULL)),
  CHECK ((kind = 'change_direction') = (new_direction IS NOT NULL AND new_direction <> 'untagged'))
);
CREATE INDEX punch_adjudications_punch_idx ON app.punch_adjudications (punch_id, created_at DESC);
CREATE INDEX punch_adjudications_employee_idx ON app.punch_adjudications (org_id, employee_id, punched_at);
CREATE TRIGGER punch_adjudications_immutable BEFORE UPDATE OR DELETE ON app.punch_adjudications
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();

-- Monthly partitions (DEC-48). Created ahead by the migration role, never by the API (it owns nothing).
-- Run app.add_month_partitions again before the last month runs out; the DEFAULT partition only catches
-- rows if that is missed (or for far-off dates), so nothing is ever rejected.
-- Each partition gets RLS too, so the client_tables_without_rls() build check stays empty;
-- the API only has grants on the parent, so it can't read a partition directly anyway.
CREATE FUNCTION app.add_month_partitions(p_parent regclass, p_from date, p_months integer) RETURNS void
LANGUAGE plpgsql SET search_path = app, pg_temp AS
$$
DECLARE
  m    date := date_trunc('month', p_from)::date;
  name text;
BEGIN
  FOR i IN 1..p_months LOOP
    name := format('%s_%s', (SELECT relname FROM pg_class WHERE oid = p_parent), to_char(m, 'YYYYMM'));
    IF to_regclass(format('app.%I', name)) IS NULL THEN
      EXECUTE format('CREATE TABLE app.%I PARTITION OF %s FOR VALUES FROM (%L) TO (%L)',
                     name, p_parent, m, (m + interval '1 month')::date);
      PERFORM app.enable_tenant_rls(format('app.%I', name)::regclass);
    END IF;
    m := (m + interval '1 month')::date;
  END LOOP;
END
$$;
REVOKE EXECUTE ON FUNCTION app.add_month_partitions(regclass, date, integer) FROM PUBLIC;

SELECT app.add_month_partitions('app.raw_punches', DATE '2026-01-01', 36);   -- 2026-01 to 2028-12
CREATE TABLE app.raw_punches_default PARTITION OF app.raw_punches DEFAULT;
SELECT app.enable_tenant_rls('app.raw_punches_default');

DO $$
BEGIN
  FOR i IN 0..15 LOOP
    EXECUTE format('CREATE TABLE app.punch_keys_%s PARTITION OF app.punch_keys FOR VALUES WITH (MODULUS 16, REMAINDER %s)', i, i);
    PERFORM app.enable_tenant_rls(format('app.punch_keys_%s', i)::regclass);
  END LOOP;
END $$;

-- Tenancy, timestamps, privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['devices', 'geo_fences', 'geo_fence_versions', 'raw_punches', 'punch_keys',
                           'punch_reviews', 'manual_punches', 'punch_adjudications'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
    EXECUTE format('GRANT SELECT, INSERT ON app.%I TO app_api', t);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['devices', 'geo_fences', 'punch_reviews'] LOOP
    PERFORM app.add_updated_at_trigger(format('app.%I', t)::regclass);
  END LOOP;
END $$;

GRANT UPDATE (branch_id, model, token_hash, token_rotated_at, last_contact_at, is_online, state, row_version, updated_at)
  ON app.devices TO app_api;
GRANT UPDATE (name, state, row_version, updated_at) ON app.geo_fences TO app_api;
GRANT UPDATE (state, decided_by, decided_at, decision_reason, row_version, updated_at) ON app.punch_reviews TO app_api;
GRANT UPDATE (voided_at, voided_by, void_reason) ON app.manual_punches TO app_api;
GRANT DELETE ON app.punch_keys TO app_api;      -- 90-day purge only

-- The gateway knows only the device serial and token until the organisation is found (TSD §9.1, DEC-46).
-- Returns the organisation and device for an active device whose token hash matches; nothing otherwise.
CREATE FUNCTION app.device_for_push(p_serial text, p_token_hash text)
RETURNS TABLE (org_id uuid, device_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = app, pg_temp AS
$$ SELECT d.org_id, d.id FROM app.devices d
   JOIN app.organisations o ON o.id = d.org_id AND o.state = 'active'
   WHERE lower(d.serial) = lower(p_serial) AND d.token_hash = p_token_hash AND d.state = 'active' $$;
REVOKE EXECUTE ON FUNCTION app.device_for_push(text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.device_for_push(text, text) TO app_api;
