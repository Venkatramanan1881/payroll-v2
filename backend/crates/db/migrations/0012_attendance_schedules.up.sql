-- 0012 Attendance — Schedules: shifts, rosters, holiday calendars, optional holidays, shift swaps,
-- the shared attendance import tables, and the job-version columns Package 1 reserved.
-- Design: docs/database/DATA_MODEL-attendance.md. Domain: DOMAIN_MODEL-attendance §2.1–2.5. SM-AT-1, 6, 7, 10.

-- Settings versions (shift versions, policy-set versions, period settings) share one lifecycle (SM-AT-1).
-- Scheduled vs Active is not stored: an approved version is in force from valid_from (latest valid_from <= D wins).
CREATE TYPE app.settings_version_state AS ENUM
  ('draft', 'pending_approval', 'approved', 'rejected', 'discarded', 'superseded');

CREATE TYPE app.shift_kind      AS ENUM ('fixed', 'flexible', 'flexible_core', 'split');        -- SH-01 (DEC-22)
CREATE TYPE app.break_kind      AS ENUM ('fixed', 'window');                                     -- split shift break (ATT-05)
CREATE TYPE app.half_day_point  AS ENUM ('middle', 'fixed_time', 'break_start');                 -- SH-11, ATT-05
CREATE TYPE app.night_date_rule AS ENUM ('start_date', 'end_date');                              -- SH-06

CREATE TABLE app.shifts (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  code        text NOT NULL CHECK (btrim(code) <> ''),
  name        text NOT NULL CHECK (btrim(name) <> ''),
  branch_id   uuid,                                       -- NULL = usable at every branch
  state       app.item_state NOT NULL DEFAULT 'active',   -- deactivated, never deleted (AC-AT-01.7)
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, branch_id) REFERENCES app.branches (org_id, id)
);
CREATE UNIQUE INDEX shifts_code_uq ON app.shifts (org_id, lower(code));

CREATE TABLE app.shift_versions (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id             uuid NOT NULL,
  shift_id           uuid NOT NULL,
  valid_from         date NOT NULL,
  state              app.settings_version_state NOT NULL DEFAULT 'approved',
  kind               app.shift_kind NOT NULL,
  start_time         time NOT NULL,              -- fixed/split: shift start; flexible: nominal start
  end_time           time NOT NULL,
  is_night           boolean NOT NULL DEFAULT false,          -- DEC-23
  night_date_rule    app.night_date_rule NOT NULL DEFAULT 'start_date',
  required_minutes   integer NOT NULL DEFAULT 480 CHECK (required_minutes BETWEEN 1 AND 1440),  -- SH-03, per day only
  window_before_min  integer NOT NULL DEFAULT 180 CHECK (window_before_min BETWEEN 0 AND 720),  -- SH-07
  window_after_min   integer NOT NULL DEFAULT 360 CHECK (window_after_min BETWEEN 0 AND 720),
  flex_earliest      time,                       -- SH-05
  flex_latest        time,
  core_start         time,                       -- SH-04
  core_end           time,
  break_kind         app.break_kind,             -- split shifts only
  break_start        time,
  break_end          time,
  half_day_point     app.half_day_point NOT NULL DEFAULT 'middle',
  half_day_time      time,
  author_id          uuid DEFAULT app.current_user_id(),
  decided_by         uuid,
  decided_at         timestamptz,
  decision_reason    text,
  superseded_at      timestamptz,
  recorded_at        timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, shift_id) REFERENCES app.shifts (org_id, id),
  -- A shift ending at or before its start crosses midnight and must carry the night flag (DEC-23).
  CHECK (kind IN ('flexible', 'flexible_core') OR is_night = (end_time <= start_time)),
  CHECK (kind NOT IN ('flexible', 'flexible_core') OR (flex_earliest IS NOT NULL AND flex_latest IS NOT NULL)),
  CHECK (kind <> 'flexible_core' OR (core_start IS NOT NULL AND core_end IS NOT NULL)),
  CHECK ((kind = 'split') = (break_kind IS NOT NULL AND break_start IS NOT NULL AND break_end IS NOT NULL)),
  CHECK ((half_day_point = 'fixed_time') = (half_day_time IS NOT NULL)),
  CHECK (half_day_point <> 'break_start' OR kind = 'split'),
  CHECK (state <> 'superseded' OR superseded_at IS NOT NULL),
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> '')
);
-- One approved version per shift and start date; the one in force on D is the latest valid_from <= D.
CREATE UNIQUE INDEX shift_versions_approved_uq ON app.shift_versions (shift_id, valid_from) WHERE state = 'approved';

-- Default shift, allowed shifts (auto-detect, ATT-04) live on the employee's job version (DQ-AT-03).
-- The policy-set override column is added in 0014, once policy_sets exists.
ALTER TABLE app.employee_job_versions ADD COLUMN default_shift_id uuid;
ALTER TABLE app.employee_job_versions
  ADD CONSTRAINT job_versions_default_shift_fk FOREIGN KEY (org_id, default_shift_id) REFERENCES app.shifts (org_id, id);

CREATE TABLE app.job_version_allowed_shifts (
  org_id         uuid NOT NULL,
  job_version_id uuid NOT NULL,
  shift_id       uuid NOT NULL,
  PRIMARY KEY (job_version_id, shift_id),
  FOREIGN KEY (org_id, job_version_id) REFERENCES app.employee_job_versions (org_id, id),
  FOREIGN KEY (org_id, shift_id)       REFERENCES app.shifts (org_id, id)
);

-- Imports shared by roster uploads and punch imports (SM-AT-10 = Package 1 SM-7).
CREATE TYPE app.attendance_import_kind AS ENUM ('roster', 'punch');

CREATE TABLE app.attendance_imports (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id         uuid NOT NULL REFERENCES app.organisations (id),
  kind           app.attendance_import_kind NOT NULL,
  file_ref       text,                                   -- NULL for an API (JSON) import
  file_checksum  text NOT NULL CHECK (file_checksum ~ '^[0-9a-f]{64}$'),
  uploaded_by    uuid NOT NULL,
  state          app.bulk_import_state NOT NULL DEFAULT 'uploaded',
  total_rows     integer CHECK (total_rows >= 0),
  applied_count  integer NOT NULL DEFAULT 0 CHECK (applied_count >= 0),
  error_count    integer NOT NULL DEFAULT 0 CHECK (error_count >= 0),
  failure_reason text,
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now(),
  completed_at   timestamptz,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, uploaded_by) REFERENCES app.users (org_id, id),
  CHECK (state <> 'failed' OR failure_reason IS NOT NULL)
);
CREATE INDEX attendance_imports_recent_idx ON app.attendance_imports (org_id, kind, created_at DESC);

CREATE TABLE app.attendance_import_rows (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id        uuid NOT NULL,
  import_id     uuid NOT NULL,
  file_checksum text NOT NULL,
  row_number    integer NOT NULL CHECK (row_number > 0),
  employee_id   uuid,
  outcome       app.bulk_row_outcome NOT NULL,
  error_message text,
  created_at    timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, import_id)   REFERENCES app.attendance_imports (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  UNIQUE (import_id, row_number),
  CHECK (outcome <> 'error' OR error_message IS NOT NULL)
);
-- The same file never applies a row twice (AC-AT-02.3, AC-AT-17.2 for files).
CREATE UNIQUE INDEX attendance_import_rows_applied_once_uq
  ON app.attendance_import_rows (org_id, file_checksum, row_number) WHERE outcome = 'applied';

-- Rosters (US-AT-02). One current entry per employee and date (INV-AT-06); replaced entries are kept.
CREATE TYPE app.roster_source AS ENUM ('screen', 'upload', 'copy', 'rotation', 'swap', 'shift_change');

CREATE TABLE app.roster_entries (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL,
  employee_id      uuid NOT NULL,
  work_date        date NOT NULL,
  shift_id         uuid,                       -- NULL = weekly off
  is_weekly_off    boolean NOT NULL DEFAULT false,
  source           app.roster_source NOT NULL,
  import_id        uuid,
  run_key          text,                       -- copy/rotation run, for idempotent re-runs
  recorded_at      timestamptz NOT NULL DEFAULT now(),
  recorded_by      uuid DEFAULT app.current_user_id(),
  superseded_at    timestamptz,
  superseded_by_id uuid,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id)      REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, shift_id)         REFERENCES app.shifts (org_id, id),
  FOREIGN KEY (org_id, import_id)        REFERENCES app.attendance_imports (org_id, id),
  FOREIGN KEY (org_id, superseded_by_id) REFERENCES app.roster_entries (org_id, id),
  CHECK ((shift_id IS NULL) = is_weekly_off)
);
CREATE UNIQUE INDEX roster_entries_current_uq ON app.roster_entries (employee_id, work_date) WHERE superseded_at IS NULL;
CREATE INDEX roster_entries_date_idx ON app.roster_entries (org_id, work_date) WHERE superseded_at IS NULL;

-- Holiday calendars (US-AT-06). A branch has at most one calendar per year (INV-AT-07).
CREATE TYPE app.holiday_kind AS ENUM ('national', 'festival', 'optional');

CREATE TABLE app.holiday_calendars (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  year        smallint NOT NULL CHECK (year BETWEEN 2000 AND 2100),
  name        text NOT NULL CHECK (btrim(name) <> ''),
  state       app.item_state NOT NULL DEFAULT 'active',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id)
);
CREATE UNIQUE INDEX holiday_calendars_name_uq ON app.holiday_calendars (org_id, year, lower(name));

CREATE TABLE app.holidays (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id       uuid NOT NULL,
  calendar_id  uuid NOT NULL,
  holiday_date date NOT NULL,
  name         text NOT NULL CHECK (btrim(name) <> ''),
  kind         app.holiday_kind NOT NULL,
  removed_at   timestamptz,                    -- removed holidays are kept for recalculation history
  removed_by   uuid,
  created_at   timestamptz NOT NULL DEFAULT now(),
  created_by   uuid DEFAULT app.current_user_id(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, calendar_id) REFERENCES app.holiday_calendars (org_id, id)
);
CREATE UNIQUE INDEX holidays_date_uq ON app.holidays (calendar_id, holiday_date) WHERE removed_at IS NULL;

CREATE TABLE app.branch_calendars (
  org_id      uuid NOT NULL,
  branch_id   uuid NOT NULL,
  year        smallint NOT NULL,
  calendar_id uuid NOT NULL,
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (branch_id, year),                               -- INV-AT-07
  FOREIGN KEY (org_id, branch_id)   REFERENCES app.branches (org_id, id),
  FOREIGN KEY (org_id, calendar_id) REFERENCES app.holiday_calendars (org_id, id)
);

-- Optional holiday picks (US-AT-08, SM-AT-6).
CREATE TYPE app.pick_state AS ENUM ('pending', 'approved', 'rejected', 'withdrawn', 'cancelled');

CREATE TABLE app.optional_holiday_picks (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id          uuid NOT NULL,
  employee_id     uuid NOT NULL,
  holiday_id      uuid NOT NULL,
  state           app.pick_state NOT NULL DEFAULT 'pending',
  raised_by       uuid DEFAULT app.current_user_id(),
  decided_by      uuid,
  decided_at      timestamptz,
  decision_reason text,
  row_version     integer NOT NULL DEFAULT 1,
  created_at      timestamptz NOT NULL DEFAULT now(),
  updated_at      timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, holiday_id)  REFERENCES app.holidays (org_id, id),
  CHECK (decided_by IS NULL OR decided_by IS DISTINCT FROM raised_by),          -- BR-AT-15
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> '')
);
CREATE UNIQUE INDEX optional_holiday_picks_live_uq ON app.optional_holiday_picks (employee_id, holiday_id)
  WHERE state IN ('pending', 'approved');

-- Shift change and swap requests (US-AT-05, SM-AT-7).
CREATE TYPE app.swap_kind AS ENUM ('change', 'swap');
CREATE TYPE app.request_state AS ENUM ('pending', 'approved', 'rejected', 'withdrawn');

CREATE TABLE app.shift_swap_requests (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id             uuid NOT NULL,
  kind               app.swap_kind NOT NULL,
  requester_id       uuid NOT NULL,
  work_date          date NOT NULL,
  wanted_shift_id    uuid NOT NULL,
  colleague_id       uuid,
  colleague_shift_id uuid,
  reason             text,
  state              app.request_state NOT NULL DEFAULT 'pending',
  raised_by          uuid DEFAULT app.current_user_id(),
  decided_by         uuid,
  decided_at         timestamptz,
  decision_reason    text,
  row_version        integer NOT NULL DEFAULT 1,
  created_at         timestamptz NOT NULL DEFAULT now(),
  updated_at         timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, requester_id)       REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, colleague_id)       REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, wanted_shift_id)    REFERENCES app.shifts (org_id, id),
  FOREIGN KEY (org_id, colleague_shift_id) REFERENCES app.shifts (org_id, id),
  CHECK ((kind = 'swap') = (colleague_id IS NOT NULL AND colleague_shift_id IS NOT NULL)),
  CHECK (colleague_id IS DISTINCT FROM requester_id),
  CHECK (decided_by IS NULL OR decided_by IS DISTINCT FROM raised_by),
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> '')
);
CREATE INDEX shift_swap_requests_pending_idx ON app.shift_swap_requests (org_id, work_date) WHERE state = 'pending';

-- Tenancy, timestamps, privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['shifts', 'shift_versions', 'job_version_allowed_shifts', 'attendance_imports',
                           'attendance_import_rows', 'roster_entries', 'holiday_calendars', 'holidays',
                           'branch_calendars', 'optional_holiday_picks', 'shift_swap_requests'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
    EXECUTE format('GRANT SELECT, INSERT ON app.%I TO app_api', t);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['shifts', 'attendance_imports', 'holiday_calendars', 'branch_calendars',
                           'optional_holiday_picks', 'shift_swap_requests'] LOOP
    PERFORM app.add_updated_at_trigger(format('app.%I', t)::regclass);
  END LOOP;
END $$;

GRANT UPDATE (name, branch_id, state, row_version, updated_at) ON app.shifts TO app_api;
GRANT UPDATE (state, decided_by, decided_at, decision_reason, superseded_at) ON app.shift_versions TO app_api;
GRANT UPDATE (state, total_rows, applied_count, error_count, failure_reason, updated_at, completed_at)
  ON app.attendance_imports TO app_api;
GRANT UPDATE (superseded_at, superseded_by_id) ON app.roster_entries TO app_api;
GRANT UPDATE (name, state, row_version, updated_at) ON app.holiday_calendars TO app_api;
GRANT UPDATE (removed_at, removed_by) ON app.holidays TO app_api;
GRANT UPDATE (calendar_id, updated_at) ON app.branch_calendars TO app_api;
GRANT UPDATE (state, decided_by, decided_at, decision_reason, row_version, updated_at)
  ON app.optional_holiday_picks, app.shift_swap_requests TO app_api;
