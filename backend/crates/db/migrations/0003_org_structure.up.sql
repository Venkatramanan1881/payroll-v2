-- 0003 Org Structure: branches, reference lists, exit reasons, contractors, payroll groups, settings.
-- Domain: DOMAIN_MODEL §2.2-2.6. Rules: BR-OR-01..06, AC-OR-02.3, PG-R5, EM-R5.
-- Every table carries (org_id, id) as a unique key so child tables can use composite foreign keys:
-- a row can only point to a row of the same organisation, even though FK checks ignore RLS.

CREATE TABLE app.branches (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  short_code  text NOT NULL CHECK (short_code ~ '^[A-Z0-9]{2,6}$'),    -- EMQ-26
  name        text NOT NULL CHECK (btrim(name) <> ''),
  address     text,
  time_zone   text NOT NULL,                                          -- BR-OR-03; checked by trigger below
  state       app.item_state NOT NULL DEFAULT 'active',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE (org_id, short_code)                                         -- BR-OR-01
);

CREATE FUNCTION app.check_time_zone() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = NEW.time_zone) THEN
    RAISE EXCEPTION 'unknown time zone: %', NEW.time_zone USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER check_time_zone BEFORE INSERT OR UPDATE OF time_zone ON app.branches
  FOR EACH ROW EXECUTE FUNCTION app.check_time_zone();

-- Reference lists: four separate tables (DOMAIN_MODEL §2.3), same shape.
CREATE TABLE app.departments (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id uuid NOT NULL REFERENCES app.organisations (id),
  code text NOT NULL CHECK (btrim(code) <> ''),
  name text NOT NULL CHECK (btrim(name) <> ''),
  state app.item_state NOT NULL DEFAULT 'active',
  row_version integer NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  created_by uuid DEFAULT app.current_user_id(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id)
);
CREATE TABLE app.designations (LIKE app.departments INCLUDING DEFAULTS INCLUDING CONSTRAINTS);
CREATE TABLE app.categories (LIKE app.departments INCLUDING DEFAULTS INCLUDING CONSTRAINTS);
CREATE TABLE app.employee_types (LIKE app.departments INCLUDING DEFAULTS INCLUDING CONSTRAINTS);
-- Exit reasons: editable list, seeded per organisation (EM-R5).
CREATE TABLE app.exit_reasons (LIKE app.departments INCLUDING DEFAULTS INCLUDING CONSTRAINTS);

-- LIKE copies columns, defaults and CHECKs, but not keys or foreign keys.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['designations', 'categories', 'employee_types', 'exit_reasons'] LOOP
    EXECUTE format('ALTER TABLE app.%I ADD PRIMARY KEY (id)', t);
    EXECUTE format('ALTER TABLE app.%I ADD FOREIGN KEY (org_id) REFERENCES app.organisations (id)', t);
    EXECUTE format('ALTER TABLE app.%I ADD UNIQUE (org_id, id)', t);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['departments', 'designations', 'categories', 'employee_types', 'exit_reasons'] LOOP
    EXECUTE format('CREATE UNIQUE INDEX %I ON app.%I (org_id, lower(code))', t || '_code_uq', t);  -- BR-OR-01
  END LOOP;
END $$;

CREATE TABLE app.contractors (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id        uuid NOT NULL REFERENCES app.organisations (id),
  code          text NOT NULL CHECK (btrim(code) <> ''),
  name          text NOT NULL CHECK (btrim(name) <> ''),
  contact_name  text,
  contact_phone text,
  contact_email text,
  state         app.item_state NOT NULL DEFAULT 'active',
  row_version   integer NOT NULL DEFAULT 1,
  created_at    timestamptz NOT NULL DEFAULT now(),
  created_by    uuid DEFAULT app.current_user_id(),
  updated_at    timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id)
);
CREATE UNIQUE INDEX contractors_code_uq ON app.contractors (org_id, lower(code));

CREATE TYPE app.pay_frequency AS ENUM ('daily', 'weekly', 'bi_weekly', 'monthly');

-- Membership is not stored here: it is a dated field of each employee's job version (DOMAIN_MODEL §2.5).
CREATE TABLE app.payroll_groups (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id          uuid NOT NULL REFERENCES app.organisations (id),
  name            text NOT NULL CHECK (btrim(name) <> ''),
  pay_frequency   app.pay_frequency NOT NULL DEFAULT 'monthly',
  month_start_day smallint CHECK (month_start_day BETWEEN 1 AND 28),  -- monthly custom cycle; NULL = calendar month (DBQ-02)
  week_start_day  smallint CHECK (week_start_day BETWEEN 1 AND 7),    -- weekly; ISO 1 = Monday
  cycle_anchor    date,                                               -- bi-weekly: first day of any cycle
  is_default      boolean NOT NULL DEFAULT false,
  state           app.item_state NOT NULL DEFAULT 'active',
  row_version     integer NOT NULL DEFAULT 1,
  created_at      timestamptz NOT NULL DEFAULT now(),
  created_by      uuid DEFAULT app.current_user_id(),
  updated_at      timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  CHECK (CASE pay_frequency
           WHEN 'daily'     THEN month_start_day IS NULL AND week_start_day IS NULL AND cycle_anchor IS NULL
           WHEN 'weekly'    THEN month_start_day IS NULL AND week_start_day IS NOT NULL AND cycle_anchor IS NULL
           WHEN 'bi_weekly' THEN month_start_day IS NULL AND week_start_day IS NULL AND cycle_anchor IS NOT NULL
           WHEN 'monthly'   THEN week_start_day IS NULL AND cycle_anchor IS NULL
         END),
  CHECK (NOT is_default OR state = 'active')
);
CREATE UNIQUE INDEX payroll_groups_name_uq ON app.payroll_groups (org_id, lower(name));
CREATE UNIQUE INDEX payroll_groups_default_uq ON app.payroll_groups (org_id) WHERE is_default;  -- BR-OR-06

-- Organisation settings (OR-01..04, EM-01..11), one versioned record per settings group (TSD §5.4, G5).
-- Values are validated against the core library's schema. Insert-only: a correction is a newer row.
-- In force on date D: the row with the latest valid_from <= D, ties broken by the latest recorded_at.
-- No row for a group = all defaults.
CREATE TABLE app.org_settings_versions (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id          uuid NOT NULL REFERENCES app.organisations (id),
  settings_group  text NOT NULL CHECK (settings_group ~ '^[A-Z]{2}$'),   -- e.g. 'OR', 'EM'
  valid_from      date NOT NULL,
  setting_values  jsonb NOT NULL CHECK (jsonb_typeof(setting_values) = 'object'),
  reason          text,
  recorded_at     timestamptz NOT NULL DEFAULT now(),
  recorded_by     uuid DEFAULT app.current_user_id()
);
CREATE INDEX org_settings_versions_lookup_idx
  ON app.org_settings_versions (org_id, settings_group, valid_from DESC, recorded_at DESC);

-- Name history (AC-OR-02.3): past job versions show a list entry as it was named then.
-- Filled only by trigger; the API can read it but not write it.
CREATE TYPE app.named_item_kind AS ENUM
  ('branch', 'department', 'designation', 'category', 'employee_type', 'exit_reason', 'contractor', 'payroll_group');

CREATE TABLE app.reference_name_history (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id      uuid NOT NULL REFERENCES app.organisations (id),
  item_kind   app.named_item_kind NOT NULL,
  item_id     uuid NOT NULL,           -- no FK: points into one of eight tables, chosen by item_kind
  name        text NOT NULL,
  valid_from  timestamptz NOT NULL DEFAULT now(),
  recorded_by uuid DEFAULT app.current_user_id()
);
CREATE INDEX reference_name_history_item_idx
  ON app.reference_name_history (org_id, item_kind, item_id, valid_from DESC);

CREATE FUNCTION app.record_name_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = app, pg_temp AS
$$ BEGIN
  IF TG_OP = 'UPDATE' THEN
    IF NEW.name IS NOT DISTINCT FROM OLD.name THEN
      RETURN NULL;
    END IF;
  END IF;
  INSERT INTO app.reference_name_history (org_id, item_kind, item_id, name)
  VALUES (NEW.org_id, TG_ARGV[0]::app.named_item_kind, NEW.id, NEW.name);
  RETURN NULL;
END $$;

DO $$
DECLARE r record;
BEGIN
  FOR r IN SELECT * FROM (VALUES
      ('branches', 'branch'), ('departments', 'department'), ('designations', 'designation'),
      ('categories', 'category'), ('employee_types', 'employee_type'), ('exit_reasons', 'exit_reason'),
      ('contractors', 'contractor'), ('payroll_groups', 'payroll_group')) AS v (tbl, kind)
  LOOP
    EXECUTE format(
      'CREATE TRIGGER record_name_history AFTER INSERT OR UPDATE OF name ON app.%I '
      'FOR EACH ROW EXECUTE FUNCTION app.record_name_history(%L)', r.tbl, r.kind);
    PERFORM app.add_updated_at_trigger(format('app.%I', r.tbl)::regclass);
    PERFORM app.enable_tenant_rls(format('app.%I', r.tbl)::regclass);
    -- No DELETE: used entries are deactivated, never deleted (BR-OR-02). Payroll groups below.
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON app.%I TO app_api', r.tbl);
  END LOOP;
END $$;

-- A payroll group that never had members may be deleted (SM-8). The job-version FK
-- (ON DELETE RESTRICT, migration 0004) refuses the delete for any group ever used.
GRANT DELETE ON app.payroll_groups TO app_api;

SELECT app.enable_tenant_rls('app.org_settings_versions');
GRANT SELECT, INSERT ON app.org_settings_versions TO app_api;

SELECT app.enable_tenant_rls('app.reference_name_history');
GRANT SELECT ON app.reference_name_history TO app_api;
