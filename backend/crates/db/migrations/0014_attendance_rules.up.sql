-- 0014 Attendance — Rules: policy sets and versions, assignments, the employee override column,
-- waivers, and versioned day results (monthly partitions, DEC-48).
-- Design: docs/database/DATA_MODEL-attendance.md. Domain: DOMAIN_MODEL-attendance §2.13–2.16. SM-AT-1, 8, 11.

CREATE TABLE app.policy_sets (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  name        text NOT NULL CHECK (btrim(name) <> ''),
  is_default  boolean NOT NULL DEFAULT false,
  state       app.item_state NOT NULL DEFAULT 'active',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  CHECK (NOT is_default OR state = 'active')
);
CREATE UNIQUE INDEX policy_sets_name_uq ON app.policy_sets (org_id, lower(name));
CREATE UNIQUE INDEX policy_sets_default_uq ON app.policy_sets (org_id) WHERE is_default;     -- AC-AT-21.1

-- All attendance settings of one version as one JSON document, validated by the core library
-- (same pattern as org_settings_versions, TSD §5.4). Keys missing = BRD defaults as changed by decisions.
CREATE TABLE app.policy_set_versions (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id          uuid NOT NULL,
  policy_set_id   uuid NOT NULL,
  valid_from      date NOT NULL,
  state           app.settings_version_state NOT NULL DEFAULT 'draft',
  settings        jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(settings) = 'object'),
  change_reason   text,
  author_id       uuid DEFAULT app.current_user_id(),
  submitted_at    timestamptz,
  decided_by      uuid,
  decided_at      timestamptz,
  decision_reason text,
  superseded_at   timestamptz,
  recorded_at     timestamptz NOT NULL DEFAULT now(),
  row_version     integer NOT NULL DEFAULT 1,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, policy_set_id) REFERENCES app.policy_sets (org_id, id),
  CHECK (decided_by IS NULL OR decided_by IS DISTINCT FROM author_id),       -- maker-checker (§3.23)
  CHECK (state <> 'superseded' OR superseded_at IS NOT NULL),
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> '')
);
-- In force on D: the approved version with the latest valid_from <= D (BR-AT-21).
CREATE UNIQUE INDEX policy_set_versions_approved_uq ON app.policy_set_versions (policy_set_id, valid_from)
  WHERE state = 'approved';
CREATE INDEX policy_set_versions_pending_idx ON app.policy_set_versions (org_id) WHERE state = 'pending_approval';

-- Assignments to groups (US-AT-21). The organisation default is policy_sets.is_default;
-- the employee override is a job-version column (DQ-AT-03).
CREATE TYPE app.assignment_level AS ENUM ('branch', 'department', 'category', 'employee_type');
CREATE TYPE app.assignment_state AS ENUM ('active', 'superseded');

CREATE TABLE app.policy_assignments (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id        uuid NOT NULL,
  level         app.assignment_level NOT NULL,
  target_id     uuid NOT NULL,                -- branch / department / category / employee type id (checked by the API)
  policy_set_id uuid NOT NULL,
  valid         daterange NOT NULL CHECK (NOT isempty(valid) AND NOT lower_inf(valid)),
  state         app.assignment_state NOT NULL DEFAULT 'active',
  recorded_at   timestamptz NOT NULL DEFAULT now(),
  recorded_by   uuid DEFAULT app.current_user_id(),
  superseded_at timestamptz,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, policy_set_id) REFERENCES app.policy_sets (org_id, id),
  CHECK (state <> 'superseded' OR superseded_at IS NOT NULL),
  CONSTRAINT policy_assignments_no_overlap                                     -- INV-AT-05
    EXCLUDE USING gist (org_id WITH =, level WITH =, target_id WITH =, valid WITH &&) WHERE (state = 'active')
);

ALTER TABLE app.employee_job_versions ADD COLUMN policy_set_id uuid;
ALTER TABLE app.employee_job_versions
  ADD CONSTRAINT job_versions_policy_set_fk FOREIGN KEY (org_id, policy_set_id) REFERENCES app.policy_sets (org_id, id);

-- Bulk waivers (US-AT-34, SM-AT-11).
CREATE TYPE app.waiver_kind  AS ENUM ('late', 'early');
CREATE TYPE app.waiver_state AS ENUM ('active', 'withdrawn');

CREATE TABLE app.waivers (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id         uuid NOT NULL,
  work_date      date NOT NULL,
  kind           app.waiver_kind NOT NULL,
  branch_id      uuid,
  shift_id       uuid,
  reason         text NOT NULL CHECK (btrim(reason) <> ''),
  state          app.waiver_state NOT NULL DEFAULT 'active',
  created_at     timestamptz NOT NULL DEFAULT now(),
  created_by     uuid DEFAULT app.current_user_id(),
  withdrawn_at   timestamptz,
  withdrawn_by   uuid,
  withdraw_reason text,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, branch_id) REFERENCES app.branches (org_id, id),
  FOREIGN KEY (org_id, shift_id)  REFERENCES app.shifts (org_id, id),
  CHECK (branch_id IS NOT NULL OR shift_id IS NOT NULL),
  CHECK (state <> 'withdrawn' OR (withdrawn_at IS NOT NULL AND btrim(coalesce(withdraw_reason, '')) <> ''))
);
CREATE INDEX waivers_date_idx ON app.waivers (org_id, work_date) WHERE state = 'active';

-- Day results (US-AT-22, SM-AT-8). One row per calculated version; exactly one current per employee-date
-- (INV-AT-09). Columns used by reports and dashboards are typed; the rest (sessions, explanation) is JSON.
CREATE TYPE app.day_status AS ENUM ('present', 'half_day', 'absent', 'absent_pending', 'weekly_off',
                                    'holiday', 'holiday_unpaid', 'long_leave');   -- leave/on-duty values come with Package 3

CREATE TABLE app.day_results (
  id                uuid NOT NULL DEFAULT gen_random_uuid(),
  org_id            uuid NOT NULL,
  employee_id       uuid NOT NULL,
  work_date         date NOT NULL,
  version           integer NOT NULL CHECK (version > 0),
  is_current        boolean NOT NULL DEFAULT true,
  reason            text NOT NULL,                     -- new punch, approval, settings change, …
  input_fingerprint text NOT NULL,                     -- versions of every input used (DM-04)
  calculated_at     timestamptz NOT NULL DEFAULT now(),
  finalised_at      timestamptz,                       -- SM-AT-8; cleared by a newer version
  status            app.day_status NOT NULL,
  branch_id         uuid NOT NULL,                     -- master branch on the date (lock unit, DQ-AT-06)
  shift_version_id  uuid,
  policy_version_id uuid NOT NULL,
  first_in          timestamptz,
  last_out          timestamptz,
  net_minutes       integer NOT NULL DEFAULT 0 CHECK (net_minutes >= 0),       -- unrounded (BR-AT-11)
  net_minutes_rounded integer NOT NULL DEFAULT 0 CHECK (net_minutes_rounded >= 0),
  late_minutes      integer NOT NULL DEFAULT 0 CHECK (late_minutes >= 0),
  early_minutes     integer NOT NULL DEFAULT 0 CHECK (early_minutes >= 0),
  late_mark         boolean NOT NULL DEFAULT false,
  early_mark        boolean NOT NULL DEFAULT false,
  deduction_days    numeric(3, 1) NOT NULL DEFAULT 0 CHECK (deduction_days IN (0, 0.5, 1)),  -- no quarter days (DEC-24)
  flags             text[] NOT NULL DEFAULT '{}',
  detail            jsonb NOT NULL DEFAULT '{}',       -- sessions, costed branches, rules applied, explanation (BR-08)
  PRIMARY KEY (id, work_date),
  UNIQUE (employee_id, work_date, version),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, branch_id)   REFERENCES app.branches (org_id, id),
  FOREIGN KEY (org_id, shift_version_id)  REFERENCES app.shift_versions (org_id, id),
  FOREIGN KEY (org_id, policy_version_id) REFERENCES app.policy_set_versions (org_id, id)
) PARTITION BY RANGE (work_date);
CREATE UNIQUE INDEX day_results_current_uq ON app.day_results (employee_id, work_date) WHERE is_current;   -- INV-AT-09
CREATE INDEX day_results_org_date_idx ON app.day_results (org_id, work_date, branch_id) WHERE is_current;
CREATE INDEX day_results_open_idx ON app.day_results (org_id, work_date) WHERE is_current AND finalised_at IS NULL;

SELECT app.add_month_partitions('app.day_results', DATE '2026-01-01', 36);
CREATE TABLE app.day_results_default PARTITION OF app.day_results DEFAULT;
SELECT app.enable_tenant_rls('app.day_results_default');

-- Tenancy, timestamps, privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['policy_sets', 'policy_set_versions', 'policy_assignments', 'waivers', 'day_results'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
    EXECUTE format('GRANT SELECT, INSERT ON app.%I TO app_api', t);
  END LOOP;
  PERFORM app.add_updated_at_trigger('app.policy_sets');
END $$;

GRANT UPDATE (name, state, row_version, updated_at) ON app.policy_sets TO app_api;
-- Draft settings can be edited; once approved only the supersede marker changes.
GRANT UPDATE (valid_from, settings, change_reason, state, submitted_at, decided_by, decided_at, decision_reason,
              superseded_at, row_version)
  ON app.policy_set_versions TO app_api;
GRANT UPDATE (state, superseded_at) ON app.policy_assignments TO app_api;
GRANT UPDATE (state, withdrawn_at, withdrawn_by, withdraw_reason) ON app.waivers TO app_api;
GRANT UPDATE (is_current, finalised_at) ON app.day_results TO app_api;

-- An approved policy version's content is frozen (SM-AT-1): only its state may move on (to superseded).
CREATE FUNCTION app.freeze_approved_settings() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  IF OLD.state IN ('approved', 'superseded', 'rejected', 'discarded')
     AND (NEW.settings IS DISTINCT FROM OLD.settings OR NEW.valid_from IS DISTINCT FROM OLD.valid_from) THEN
    RAISE EXCEPTION 'settings version % is %, its content can no longer change', OLD.id, OLD.state
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER policy_set_versions_freeze BEFORE UPDATE ON app.policy_set_versions
  FOR EACH ROW EXECUTE FUNCTION app.freeze_approved_settings();
