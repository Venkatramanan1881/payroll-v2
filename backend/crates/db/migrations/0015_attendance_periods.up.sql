-- 0015 Attendance — Regularisation and periods: requests and decisions, day overrides, period settings,
-- attendance periods, lock units and their transitions, period results, adjustments, notifications.
-- Design: docs/database/DATA_MODEL-attendance.md. Domain: DOMAIN_MODEL-attendance §2.17–2.24. SM-AT-5, 9.

-- Regularisation (US-AT-32, SM-AT-5, RG-12).
CREATE TYPE app.regularisation_kind  AS ENUM ('missed_punch', 'late', 'early', 'wrong_punch', 'whole_day');
CREATE TYPE app.regularisation_state AS ENUM ('pending', 'escalated', 'approved', 'rejected', 'auto_rejected',
                                              'withdrawn', 'cancelled');

CREATE TABLE app.regularisation_requests (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id         uuid NOT NULL,
  employee_id    uuid NOT NULL,
  work_date      date NOT NULL,
  kind           app.regularisation_kind NOT NULL,
  requested      jsonb NOT NULL CHECK (jsonb_typeof(requested) = 'object'),   -- times or punch fix asked for
  reason         text NOT NULL CHECK (btrim(reason) <> ''),                   -- RG-09
  attachment_ref text,
  raised_by      uuid NOT NULL DEFAULT app.current_user_id(),
  on_behalf      boolean NOT NULL DEFAULT false,                              -- REL-09
  levels         smallint NOT NULL DEFAULT 1 CHECK (levels IN (1, 2)),        -- RG-05
  current_level  smallint NOT NULL DEFAULT 1 CHECK (current_level BETWEEN 1 AND levels),
  state          app.regularisation_state NOT NULL DEFAULT 'pending',
  over_limit     boolean NOT NULL DEFAULT false,                              -- RG-08: goes to HR only
  escalated_at   timestamptz,
  cancelled_by   uuid,
  cancelled_at   timestamptz,
  cancel_reason  text,
  row_version    integer NOT NULL DEFAULT 1,
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  CHECK (state <> 'cancelled' OR (cancelled_at IS NOT NULL AND btrim(coalesce(cancel_reason, '')) <> ''))
);
CREATE INDEX regularisation_requests_open_idx ON app.regularisation_requests (org_id, state, created_at)
  WHERE state IN ('pending', 'escalated');
CREATE INDEX regularisation_requests_employee_idx ON app.regularisation_requests (org_id, employee_id, work_date);

-- Every decision, one row each (level 1, level 2, escalation, automatic). Insert-only.
CREATE TYPE app.decision_action AS ENUM ('approve', 'reject', 'escalate', 'auto_approve', 'auto_reject');

CREATE TABLE app.regularisation_decisions (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id      uuid NOT NULL,
  request_id  uuid NOT NULL,
  level       smallint NOT NULL CHECK (level IN (1, 2)),
  action      app.decision_action NOT NULL,
  actor_id    uuid,                         -- NULL = system (RG-06, RG-07)
  reason      text,
  decided_at  timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, request_id) REFERENCES app.regularisation_requests (org_id, id),
  CHECK (action NOT IN ('approve', 'reject') OR actor_id IS NOT NULL),
  CHECK (action <> 'reject' OR btrim(coalesce(reason, '')) <> '')
);
CREATE INDEX regularisation_decisions_request_idx ON app.regularisation_decisions (request_id);
CREATE TRIGGER regularisation_decisions_immutable BEFORE UPDATE OR DELETE ON app.regularisation_decisions
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();

-- No one decides their own request (BR-AT-15). Checked here as well as in the API.
CREATE FUNCTION app.check_not_own_decision() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  IF NEW.actor_id IS NOT NULL AND NEW.actor_id = (SELECT raised_by FROM app.regularisation_requests WHERE id = NEW.request_id) THEN
    RAISE EXCEPTION 'a request can not be decided by the user who raised it' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER regularisation_decisions_not_own BEFORE INSERT ON app.regularisation_decisions
  FOR EACH ROW EXECUTE FUNCTION app.check_not_own_decision();

ALTER TABLE app.manual_punches
  ADD CONSTRAINT manual_punches_regularisation_fk
  FOREIGN KEY (org_id, regularisation_id) REFERENCES app.regularisation_requests (org_id, id);
ALTER TABLE app.punch_adjudications
  ADD CONSTRAINT punch_adjudications_regularisation_fk
  FOREIGN KEY (org_id, regularisation_id) REFERENCES app.regularisation_requests (org_id, id);

-- HR direct correction (US-AT-33, RG-10). Insert-only except voiding.
CREATE TABLE app.day_overrides (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL,
  employee_id uuid NOT NULL,
  work_date   date NOT NULL,
  status      app.day_status,
  net_minutes integer CHECK (net_minutes >= 0),
  fields      jsonb NOT NULL DEFAULT '{}',          -- other fields overridden (marks, minutes)
  reason      text NOT NULL CHECK (btrim(reason) <> ''),
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  voided_at   timestamptz,
  voided_by   uuid,
  void_reason text,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  CHECK (status IS NOT NULL OR net_minutes IS NOT NULL OR fields <> '{}'),
  CHECK (voided_at IS NULL OR btrim(coalesce(void_reason, '')) <> '')
);
CREATE INDEX day_overrides_employee_idx ON app.day_overrides (org_id, employee_id, work_date) WHERE voided_at IS NULL;

-- Period settings PL-02..07 per payroll group (PL-01 stays on payroll_groups). Same lifecycle as policy versions.
CREATE TABLE app.period_settings_versions (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL,
  payroll_group_id uuid NOT NULL,
  valid_from       date NOT NULL,
  state            app.settings_version_state NOT NULL DEFAULT 'approved',
  settings         jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(settings) = 'object'),
  author_id        uuid DEFAULT app.current_user_id(),
  decided_by       uuid,
  decided_at       timestamptz,
  decision_reason  text,
  superseded_at    timestamptz,
  recorded_at      timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, payroll_group_id) REFERENCES app.payroll_groups (org_id, id),
  CHECK (decided_by IS NULL OR decided_by IS DISTINCT FROM author_id),
  CHECK (state <> 'superseded' OR superseded_at IS NOT NULL)
);
CREATE UNIQUE INDEX period_settings_versions_approved_uq ON app.period_settings_versions (payroll_group_id, valid_from)
  WHERE state = 'approved';
CREATE TRIGGER period_settings_versions_freeze BEFORE UPDATE ON app.period_settings_versions
  FOR EACH ROW EXECUTE FUNCTION app.freeze_approved_settings();

-- Attendance periods (US-AT-36). No overlap within a payroll group (INV-AT-12); no gaps: API.
CREATE TABLE app.attendance_periods (
  id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id           uuid NOT NULL,
  payroll_group_id uuid NOT NULL,
  period           daterange NOT NULL CHECK (NOT isempty(period) AND NOT lower_inf(period) AND NOT upper_inf(period)),
  cut_off          date,                                   -- PL-02
  created_at       timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, payroll_group_id) REFERENCES app.payroll_groups (org_id, id),
  CHECK (cut_off IS NULL OR cut_off <@ period),
  CONSTRAINT attendance_periods_no_overlap
    EXCLUDE USING gist (payroll_group_id WITH =, period WITH &&)
);
CREATE INDEX attendance_periods_lookup_idx ON app.attendance_periods (org_id, payroll_group_id, lower(period));

-- Lock units (BR-AT-17, SM-AT-9). One per period and branch; no row = Open.
CREATE TYPE app.lock_state AS ENUM ('open', 'locked', 'finalised', 'paid');

CREATE TABLE app.lock_units (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL,
  period_id   uuid NOT NULL,
  branch_id   uuid NOT NULL,
  state       app.lock_state NOT NULL DEFAULT 'open',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE (period_id, branch_id),                                              -- INV-AT-13
  FOREIGN KEY (org_id, period_id) REFERENCES app.attendance_periods (org_id, id),
  FOREIGN KEY (org_id, branch_id) REFERENCES app.branches (org_id, id)
);
CREATE INDEX lock_units_closed_idx ON app.lock_units (org_id, branch_id) WHERE state <> 'open';

-- Every move, one step at a time (BR-AT-19). Insert-only; a step back needs a reason.
CREATE TABLE app.lock_unit_transitions (
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id       uuid NOT NULL,
  lock_unit_id uuid NOT NULL,
  from_state   app.lock_state NOT NULL,
  to_state     app.lock_state NOT NULL,
  actor_id     uuid,                                     -- NULL = automatic lock (PL-04)
  reason       text,
  occurred_at  timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, lock_unit_id) REFERENCES app.lock_units (org_id, id),
  CHECK (abs(array_position(enum_range(NULL::app.lock_state), to_state)
           - array_position(enum_range(NULL::app.lock_state), from_state)) = 1),
  CHECK (array_position(enum_range(NULL::app.lock_state), to_state)
           > array_position(enum_range(NULL::app.lock_state), from_state)
         OR (actor_id IS NOT NULL AND btrim(coalesce(reason, '')) <> ''))
);
CREATE INDEX lock_unit_transitions_unit_idx ON app.lock_unit_transitions (lock_unit_id, occurred_at);
CREATE TRIGGER lock_unit_transitions_immutable BEFORE UPDATE OR DELETE ON app.lock_unit_transitions
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();

-- Lock state may change only by one step, and only together with its transition row (checked by the API in
-- the same transaction; the step size is checked here).
CREATE FUNCTION app.check_lock_step() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  IF abs(array_position(enum_range(NULL::app.lock_state), NEW.state)
         - array_position(enum_range(NULL::app.lock_state), OLD.state)) > 1 THEN
    RAISE EXCEPTION 'lock unit % can move one step at a time (% -> %)', OLD.id, OLD.state, NEW.state
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER lock_units_one_step BEFORE UPDATE OF state ON app.lock_units
  FOR EACH ROW EXECUTE FUNCTION app.check_lock_step();

-- Period-end results (US-AT-31). Versioned like day results; exactly one current per employee and period.
CREATE TABLE app.period_results (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id        uuid NOT NULL,
  employee_id   uuid NOT NULL,
  period_id     uuid NOT NULL,
  version       integer NOT NULL CHECK (version > 0),
  is_current    boolean NOT NULL DEFAULT true,
  reason        text NOT NULL,
  calculated_at timestamptz NOT NULL DEFAULT now(),
  late_marks    integer NOT NULL DEFAULT 0,
  early_marks   integer NOT NULL DEFAULT 0,
  late_minutes  integer NOT NULL DEFAULT 0,
  early_minutes integer NOT NULL DEFAULT 0,
  penalty_days  numeric(5, 1) NOT NULL DEFAULT 0 CHECK (penalty_days >= 0),
  detail        jsonb NOT NULL DEFAULT '{}',        -- penalties, recovery order applied, excess absence, absconding flag
  UNIQUE (org_id, id),
  UNIQUE (employee_id, period_id, version),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, period_id)   REFERENCES app.attendance_periods (org_id, id)
);
CREATE UNIQUE INDEX period_results_current_uq ON app.period_results (employee_id, period_id) WHERE is_current;

-- Adjustments (US-AT-39, BR-AT-18). Insert-only.
CREATE TYPE app.adjustment_kind AS ENUM ('correction', 'after_cut_off', 'penalty_reversal', 'late_punch');

CREATE TABLE app.adjustments (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id             uuid NOT NULL,
  employee_id        uuid NOT NULL,
  target_period_id   uuid NOT NULL,                 -- the next open period
  original_period_id uuid NOT NULL,
  original_date      date,                          -- NULL for a period-level change (a penalty)
  kind               app.adjustment_kind NOT NULL,
  days_delta         numeric(5, 1) NOT NULL DEFAULT 0,
  minutes_delta      integer NOT NULL DEFAULT 0,
  detail             jsonb NOT NULL DEFAULT '{}',
  reason             text NOT NULL CHECK (btrim(reason) <> ''),
  created_at         timestamptz NOT NULL DEFAULT now(),
  created_by         uuid DEFAULT app.current_user_id(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id)        REFERENCES app.employees (org_id, id),
  FOREIGN KEY (org_id, target_period_id)   REFERENCES app.attendance_periods (org_id, id),
  FOREIGN KEY (org_id, original_period_id) REFERENCES app.attendance_periods (org_id, id),
  CHECK (target_period_id <> original_period_id),
  CHECK (days_delta <> 0 OR minutes_delta <> 0)
);
CREATE INDEX adjustments_target_idx ON app.adjustments (org_id, target_period_id, employee_id);
CREATE TRIGGER adjustments_immutable BEFORE UPDATE OR DELETE ON app.adjustments
  FOR EACH ROW EXECUTE FUNCTION app.reject_change();

-- Notifications (US-AT-42). Email still goes through outbox_events and the mailer.
CREATE TYPE app.notification_channel AS ENUM ('email', 'in_app');

CREATE TABLE app.notification_settings (
  org_id     uuid NOT NULL REFERENCES app.organisations (id),
  event      text NOT NULL CHECK (event ~ '^[A-Za-z]+$'),
  channel    app.notification_channel NOT NULL,
  recipient  text NOT NULL CHECK (recipient IN ('employee', 'manager', 'hr_manager', 'org_admin')),
  enabled    boolean NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now(),
  updated_by uuid DEFAULT app.current_user_id(),
  PRIMARY KEY (org_id, event, channel, recipient)         -- no row = default (on)
);

CREATE TABLE app.in_app_notifications (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id     uuid NOT NULL,
  user_id    uuid NOT NULL,
  event      text NOT NULL,
  payload    jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  read_at    timestamptz,
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, user_id) REFERENCES app.users (org_id, id)
);
CREATE INDEX in_app_notifications_unread_idx ON app.in_app_notifications (org_id, user_id, created_at DESC)
  WHERE read_at IS NULL;

-- Tenancy, timestamps, privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['regularisation_requests', 'regularisation_decisions', 'day_overrides',
                           'period_settings_versions', 'attendance_periods', 'lock_units', 'lock_unit_transitions',
                           'period_results', 'adjustments', 'notification_settings', 'in_app_notifications'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
    EXECUTE format('GRANT SELECT, INSERT ON app.%I TO app_api', t);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['regularisation_requests', 'lock_units', 'notification_settings'] LOOP
    PERFORM app.add_updated_at_trigger(format('app.%I', t)::regclass);
  END LOOP;
END $$;

GRANT UPDATE (current_level, state, escalated_at, cancelled_by, cancelled_at, cancel_reason, row_version, updated_at)
  ON app.regularisation_requests TO app_api;
GRANT UPDATE (voided_at, voided_by, void_reason) ON app.day_overrides TO app_api;
GRANT UPDATE (state, decided_by, decided_at, decision_reason, superseded_at) ON app.period_settings_versions TO app_api;
GRANT UPDATE (cut_off) ON app.attendance_periods TO app_api;
GRANT UPDATE (state, row_version, updated_at) ON app.lock_units TO app_api;
GRANT UPDATE (is_current) ON app.period_results TO app_api;
GRANT UPDATE (enabled, updated_at, updated_by) ON app.notification_settings TO app_api;
GRANT UPDATE (read_at) ON app.in_app_notifications TO app_api;
