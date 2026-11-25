-- 0004 People: employees, employment periods, job versions, status, code sequences, custom fields, bulk import.
-- Domain: DOMAIN_MODEL §2.7-2.11, §4 (INV-01..12), §6. State machines: SM-1, SM-2, SM-3, SM-5, SM-7.

CREATE TYPE app.portal_access    AS ENUM ('none', 'employee_portal');                        -- EM-R15
CREATE TYPE app.enrolment_status AS ENUM ('not_enrolled', 'enrolled', 're_enrolment_needed'); -- PC-R8

CREATE TABLE app.employees (
  id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id                  uuid NOT NULL REFERENCES app.organisations (id),
  employee_code           text NOT NULL CHECK (btrim(employee_code) <> ''),
  full_name               text NOT NULL CHECK (btrim(full_name) <> ''),
  date_of_birth           date,
  gender                  text,
  photo_ref               text,                                   -- file store path, starts with org id
  mobile                  text NOT NULL CHECK (mobile ~ '^\+[1-9][0-9]{7,14}$'),  -- E.164, mandatory (DQ-03)
  email                   text,
  address                 text,
  emergency_contact_name  text,
  emergency_contact_phone text,
  portal_access           app.portal_access NOT NULL DEFAULT 'none',
  enrolment_status        app.enrolment_status NOT NULL DEFAULT 'not_enrolled',
  -- false once an exit takes effect, true again on rejoin. Frees the mobile number (BR-EM-09)
  -- and drives the plan-limit count (BR-EM-12). Status history itself is in employee_status_changes.
  is_current              boolean NOT NULL DEFAULT true,
  row_version             integer NOT NULL DEFAULT 1,
  created_at              timestamptz NOT NULL DEFAULT now(),
  created_by              uuid DEFAULT app.current_user_id(),
  updated_at              timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE (org_id, employee_code)                                          -- INV-01
);
CREATE UNIQUE INDEX employees_mobile_current_uq ON app.employees (org_id, mobile) WHERE is_current;  -- INV-07
CREATE INDEX employees_name_idx ON app.employees (org_id, lower(full_name));

CREATE TYPE app.employment_period_state AS ENUM ('open', 'closed');

CREATE TABLE app.employment_periods (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id             uuid NOT NULL,
  employee_id        uuid NOT NULL,
  joining_date       date NOT NULL,
  probation_end_date date CHECK (probation_end_date >= joining_date),
  last_working_day   date CHECK (last_working_day >= joining_date),
  exit_reason_id     uuid,
  exit_remarks       text,
  state              app.employment_period_state NOT NULL DEFAULT 'open',
  created_at         timestamptz NOT NULL DEFAULT now(),
  created_by         uuid DEFAULT app.current_user_id(),
  updated_at         timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id)    REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, exit_reason_id) REFERENCES app.exit_reasons (org_id, id),
  CHECK (state = 'open' OR (last_working_day IS NOT NULL AND exit_reason_id IS NOT NULL)),
  -- Periods of one employee never overlap; an open period runs to infinity.
  CONSTRAINT employment_periods_no_overlap
    EXCLUDE USING gist (employee_id WITH =, (daterange(joining_date, last_working_day, '[]')) WITH &&)
);
CREATE UNIQUE INDEX employment_periods_one_open_uq ON app.employment_periods (employee_id) WHERE state = 'open';  -- INV-04
CREATE INDEX employment_periods_probation_idx ON app.employment_periods (org_id, probation_end_date)
  WHERE state = 'open' AND probation_end_date IS NOT NULL;          -- reminder job (EM-04)

-- Dated job fields (SM-3, BR-EM-05). A saved row's content never changes:
-- a correction marks rows 'superseded' and inserts new ones; a scheduled row can be 'cancelled'.
-- Effective vs scheduled is not stored: it is lower(valid) <= or > today in the master branch's time zone.
-- When an exit takes effect, the open-ended row is superseded by a copy ending on the last working day.
CREATE TYPE app.job_version_state AS ENUM ('active', 'superseded', 'cancelled');

CREATE TABLE app.employee_job_versions (
  id                   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id               uuid NOT NULL,
  employee_id          uuid NOT NULL,
  employment_period_id uuid NOT NULL,
  valid                daterange NOT NULL CHECK (NOT isempty(valid) AND NOT lower_inf(valid)),
  master_branch_id     uuid NOT NULL,
  department_id        uuid NOT NULL,                     -- always mandatory (BR-EM-13)
  designation_id       uuid,
  category_id          uuid NOT NULL,
  employee_type_id     uuid NOT NULL,
  contractor_id        uuid,                              -- EM-11
  reporting_manager_id uuid CHECK (reporting_manager_id IS DISTINCT FROM employee_id),
  payroll_group_id     uuid NOT NULL,
  enrolment_id         text CHECK (btrim(enrolment_id) <> ''),
  -- Package 2 adds shift/roster and policy-set override columns.
  state                app.job_version_state NOT NULL DEFAULT 'active',
  change_reason        text,
  recorded_at          timestamptz NOT NULL DEFAULT now(),
  recorded_by          uuid DEFAULT app.current_user_id(),
  superseded_at        timestamptz,
  superseded_by_id     uuid,
  cancelled_reason     text,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id)          REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, employment_period_id) REFERENCES app.employment_periods (org_id, id),
  FOREIGN KEY (org_id, master_branch_id)     REFERENCES app.branches (org_id, id),
  FOREIGN KEY (org_id, department_id)        REFERENCES app.departments (org_id, id),
  FOREIGN KEY (org_id, designation_id)       REFERENCES app.designations (org_id, id),
  FOREIGN KEY (org_id, category_id)          REFERENCES app.categories (org_id, id),
  FOREIGN KEY (org_id, employee_type_id)     REFERENCES app.employee_types (org_id, id),
  FOREIGN KEY (org_id, contractor_id)        REFERENCES app.contractors (org_id, id),
  FOREIGN KEY (org_id, reporting_manager_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, payroll_group_id)     REFERENCES app.payroll_groups (org_id, id) ON DELETE RESTRICT,
  FOREIGN KEY (org_id, superseded_by_id)     REFERENCES app.employee_job_versions (org_id, id),
  CHECK (state <> 'superseded' OR superseded_at IS NOT NULL),
  CHECK (state <> 'cancelled' OR btrim(coalesce(cancelled_reason, '')) <> ''),
  -- INV-02: one active version per employee per date => exactly one master branch and payroll group.
  CONSTRAINT job_versions_no_overlap
    EXCLUDE USING gist (employee_id WITH =, valid WITH &&) WHERE (state = 'active'),
  -- INV-08: an enrolment ID belongs to one employee on any date; freed when that employee's range ends.
  CONSTRAINT job_versions_enrolment_unique
    EXCLUDE USING gist (org_id WITH =, enrolment_id WITH =, valid WITH &&, employee_id WITH <>)
    WHERE (state = 'active' AND enrolment_id IS NOT NULL)
);
CREATE INDEX job_versions_branch_idx  ON app.employee_job_versions (org_id, master_branch_id) WHERE state = 'active';
CREATE INDEX job_versions_manager_idx ON app.employee_job_versions (org_id, reporting_manager_id)
  WHERE state = 'active' AND reporting_manager_id IS NOT NULL;
CREATE INDEX job_versions_group_idx   ON app.employee_job_versions (org_id, payroll_group_id);
CREATE INDEX job_versions_scheduled_idx ON app.employee_job_versions (org_id, lower(valid)) WHERE state = 'active';

-- Allowed branches of a job version (EM-06). Written with the version, never changed.
CREATE TABLE app.job_version_allowed_branches (
  org_id         uuid NOT NULL,
  job_version_id uuid NOT NULL,
  branch_id      uuid NOT NULL,
  PRIMARY KEY (job_version_id, branch_id),
  FOREIGN KEY (org_id, job_version_id) REFERENCES app.employee_job_versions (org_id, id),
  FOREIGN KEY (org_id, branch_id)      REFERENCES app.branches (org_id, id)
);
CREATE INDEX job_version_allowed_branches_branch_idx ON app.job_version_allowed_branches (org_id, branch_id);

-- Status (SM-1, SM-2).
CREATE TYPE app.employee_status AS ENUM ('probation', 'active', 'notice', 'long_leave', 'absconding', 'exited');
CREATE TYPE app.status_request_state AS ENUM ('pending', 'scheduled', 'effective', 'rejected', 'withdrawn', 'cancelled');
CREATE TYPE app.approver_role AS ENUM ('org_admin', 'hr_manager');

CREATE TABLE app.status_change_requests (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL,
  employee_id      uuid NOT NULL,
  from_status      app.employee_status NOT NULL,
  to_status        app.employee_status NOT NULL,
  effective_date   date NOT NULL,
  reason           text NOT NULL CHECK (btrim(reason) <> ''),
  last_working_day date,                                 -- exit details (to_status = 'exited')
  exit_reason_id   uuid,
  exit_remarks     text,
  raised_by        uuid NOT NULL,                        -- FK to users added in 0005
  approver_role    app.approver_role NOT NULL,           -- BR-EM-27
  state            app.status_request_state NOT NULL DEFAULT 'pending',
  decided_by       uuid,
  decided_at       timestamptz,
  decision_reason  text,
  cancelled_by     uuid,
  cancelled_at     timestamptz,
  cancel_reason    text,
  row_version      integer NOT NULL DEFAULT 1,
  created_at       timestamptz NOT NULL DEFAULT now(),
  updated_at       timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id)    REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, exit_reason_id) REFERENCES app.exit_reasons (org_id, id),
  -- SM-1 allowed moves: nothing moves to Probation; Exited is final; Notice and Absconding never go to Long leave.
  CHECK (to_status <> from_status),
  CHECK (to_status <> 'probation' AND from_status <> 'exited'),
  CHECK (NOT (from_status IN ('notice', 'absconding') AND to_status = 'long_leave')),
  CHECK ((to_status = 'exited') = (last_working_day IS NOT NULL AND exit_reason_id IS NOT NULL)),
  CHECK (to_status = 'exited' OR exit_remarks IS NULL),
  -- INV-06: no one decides their own request (also checked in the API).
  CHECK (decided_by IS NULL OR decided_by <> raised_by),
  CHECK (state IN ('pending', 'withdrawn') OR (decided_by IS NOT NULL AND decided_at IS NOT NULL)),
  CHECK (state <> 'rejected' OR btrim(coalesce(decision_reason, '')) <> ''),
  CHECK (state <> 'cancelled' OR (cancelled_by IS NOT NULL AND btrim(coalesce(cancel_reason, '')) <> ''))
);
CREATE UNIQUE INDEX status_requests_one_pending_uq ON app.status_change_requests (employee_id) WHERE state = 'pending';  -- INV-05
CREATE INDEX status_requests_open_idx ON app.status_change_requests (org_id, state, effective_date)
  WHERE state IN ('pending', 'scheduled');               -- approver inbox; 00:15 job

-- Status timeline: one row per change, by start date. The status on date D is the latest
-- effective_from <= D, so periods cannot overlap or leave gaps by construction (INV-05).
-- The first row of each employment period has no request (status chosen at joining or rejoin).
CREATE TABLE app.employee_status_changes (
  id                   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id               uuid NOT NULL,
  employee_id          uuid NOT NULL,
  employment_period_id uuid NOT NULL,
  status               app.employee_status NOT NULL,
  effective_from       date NOT NULL,
  request_id           uuid,
  recorded_at          timestamptz NOT NULL DEFAULT now(),
  recorded_by          uuid DEFAULT app.current_user_id(),
  superseded_at        timestamptz,                      -- Org Admin correction (SM-5); never deleted
  FOREIGN KEY (org_id, employee_id)          REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, employment_period_id) REFERENCES app.employment_periods (org_id, id),
  FOREIGN KEY (org_id, request_id)           REFERENCES app.status_change_requests (org_id, id)
);
CREATE UNIQUE INDEX employee_status_changes_date_uq
  ON app.employee_status_changes (employee_id, effective_from) WHERE superseded_at IS NULL;

-- Employee code running numbers (BR-EM-01). One row per pattern key, e.g. 'PBY|CHN'.
CREATE TABLE app.employee_code_sequences (
  org_id       uuid NOT NULL REFERENCES app.organisations (id),
  sequence_key text NOT NULL CHECK (btrim(sequence_key) <> ''),
  last_value   integer NOT NULL DEFAULT 0 CHECK (last_value >= 0),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (org_id, sequence_key)
);

CREATE FUNCTION app.sequence_only_increases() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  IF NEW.last_value < OLD.last_value THEN
    RAISE EXCEPTION 'employee code sequence % cannot go back (% to %)', OLD.sequence_key, OLD.last_value, NEW.last_value
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER sequence_only_increases BEFORE UPDATE ON app.employee_code_sequences
  FOR EACH ROW EXECUTE FUNCTION app.sequence_only_increases();   -- INV-12

-- Custom fields (EM-R8). Key/value by design: the fields themselves are defined by each organisation.
CREATE TYPE app.custom_field_type AS ENUM ('text', 'number', 'date', 'yes_no', 'dropdown', 'multiple_choice');

CREATE TABLE app.custom_field_definitions (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id       uuid NOT NULL REFERENCES app.organisations (id),
  label        text NOT NULL CHECK (btrim(label) <> ''),
  field_type   app.custom_field_type NOT NULL,
  options      text[],
  is_mandatory boolean NOT NULL DEFAULT false,
  state        app.item_state NOT NULL DEFAULT 'active',
  row_version  integer NOT NULL DEFAULT 1,
  created_at   timestamptz NOT NULL DEFAULT now(),
  created_by   uuid DEFAULT app.current_user_id(),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  CHECK ((field_type IN ('dropdown', 'multiple_choice')) = (options IS NOT NULL AND cardinality(options) > 0))
);
CREATE UNIQUE INDEX custom_field_definitions_label_uq ON app.custom_field_definitions (org_id, lower(label));

-- Current value only; history is the audit log (DQ-06). Value type is matched to field_type in the API.
CREATE TABLE app.custom_field_values (
  org_id        uuid NOT NULL,
  employee_id   uuid NOT NULL,
  field_id      uuid NOT NULL,
  value_text    text,
  value_number  numeric,
  value_date    date,
  value_bool    boolean,
  value_options text[],
  updated_at    timestamptz NOT NULL DEFAULT now(),
  updated_by    uuid DEFAULT app.current_user_id(),
  PRIMARY KEY (employee_id, field_id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, field_id)    REFERENCES app.custom_field_definitions (org_id, id),
  CHECK (num_nonnulls(value_text, value_number, value_date, value_bool, value_options) <= 1)
);

-- Bulk import (SM-7, EM-R7).
CREATE TYPE app.bulk_import_state AS ENUM ('uploaded', 'validating', 'applying', 'completed', 'failed');
CREATE TYPE app.bulk_row_outcome  AS ENUM ('applied', 'error', 'skipped');

CREATE TABLE app.bulk_imports (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id         uuid NOT NULL REFERENCES app.organisations (id),
  file_ref       text NOT NULL,
  file_checksum  text NOT NULL CHECK (file_checksum ~ '^[0-9a-f]{64}$'),   -- SHA-256
  uploaded_by    uuid NOT NULL,                                          -- FK to users added in 0005
  state          app.bulk_import_state NOT NULL DEFAULT 'uploaded',
  total_rows     integer CHECK (total_rows >= 0),
  applied_count  integer NOT NULL DEFAULT 0 CHECK (applied_count >= 0),
  error_count    integer NOT NULL DEFAULT 0 CHECK (error_count >= 0),
  failure_reason text,
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now(),
  completed_at   timestamptz,
  UNIQUE (org_id, id),
  CHECK (state <> 'failed' OR failure_reason IS NOT NULL)
);
CREATE INDEX bulk_imports_recent_idx ON app.bulk_imports (org_id, created_at DESC);

CREATE TABLE app.bulk_import_rows (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id        uuid NOT NULL,
  import_id     uuid NOT NULL,
  file_checksum text NOT NULL,
  row_number    integer NOT NULL CHECK (row_number > 0),
  employee_code text,
  employee_id   uuid,
  outcome       app.bulk_row_outcome NOT NULL,
  error_message text,
  created_at    timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, import_id)   REFERENCES app.bulk_imports (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  UNIQUE (import_id, row_number),
  CHECK (outcome <> 'error' OR error_message IS NOT NULL)
);
-- The same file uploaded again never applies a row twice (AC-EM-15.5).
CREATE UNIQUE INDEX bulk_import_rows_applied_once_uq
  ON app.bulk_import_rows (org_id, file_checksum, row_number) WHERE outcome = 'applied';

-- Tenancy, timestamps, privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['employees', 'employment_periods', 'employee_job_versions', 'job_version_allowed_branches',
                           'status_change_requests', 'employee_status_changes', 'employee_code_sequences',
                           'custom_field_definitions', 'custom_field_values', 'bulk_imports', 'bulk_import_rows'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
    EXECUTE format('GRANT SELECT, INSERT ON app.%I TO app_api', t);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['employees', 'employment_periods', 'status_change_requests', 'employee_code_sequences',
                           'custom_field_definitions', 'custom_field_values', 'bulk_imports'] LOOP
    PERFORM app.add_updated_at_trigger(format('app.%I', t)::regclass);
  END LOOP;
END $$;

-- UPDATE is granted only on the columns that may change. Everything else is fixed once written:
-- id, org_id and employee_code never change (BR-EM-01); job version content never changes (BR-EM-05).
GRANT UPDATE (full_name, date_of_birth, gender, photo_ref, mobile, email, address,
              emergency_contact_name, emergency_contact_phone, portal_access, enrolment_status,
              is_current, row_version, updated_at)
  ON app.employees TO app_api;
GRANT UPDATE (probation_end_date, last_working_day, exit_reason_id, exit_remarks, state, updated_at)
  ON app.employment_periods TO app_api;
GRANT UPDATE (state, superseded_at, superseded_by_id, cancelled_reason)
  ON app.employee_job_versions TO app_api;
GRANT UPDATE (state, decided_by, decided_at, decision_reason, cancelled_by, cancelled_at, cancel_reason,
              row_version, updated_at)
  ON app.status_change_requests TO app_api;
GRANT UPDATE (superseded_at) ON app.employee_status_changes TO app_api;
GRANT UPDATE (last_value, updated_at) ON app.employee_code_sequences TO app_api;
GRANT UPDATE (label, options, is_mandatory, state, row_version, updated_at)
  ON app.custom_field_definitions TO app_api;
GRANT UPDATE (value_text, value_number, value_date, value_bool, value_options, updated_at, updated_by),
      DELETE
  ON app.custom_field_values TO app_api;
GRANT UPDATE (state, total_rows, applied_count, error_count, failure_reason, updated_at, completed_at)
  ON app.bulk_imports TO app_api;
