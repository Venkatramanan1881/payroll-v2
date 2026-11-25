-- 0002 Platform: plans, organisations, vendor users.
-- Domain: DOMAIN_MODEL §2.1. Rules: BR-OR-05, VC-01, VC-R1, MD-02.

CREATE TYPE app.org_state AS ENUM ('active', 'suspended');
CREATE TYPE app.limit_behaviour AS ENUM ('block', 'allow_and_alert');
CREATE TYPE app.item_state AS ENUM ('active', 'inactive');

-- Platform data (no organisation). Read by the API, written by the vendor console.
CREATE TABLE app.plans (
  id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  code                text NOT NULL UNIQUE CHECK (code ~ '^[a-z0-9_]{2,30}$'),
  name                text NOT NULL CHECK (btrim(name) <> ''),
  includes_mobile_app boolean NOT NULL,                    -- MD-02
  state               app.item_state NOT NULL DEFAULT 'active',
  created_at          timestamptz NOT NULL DEFAULT now(),
  updated_at          timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE app.organisations (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  short_code      text NOT NULL UNIQUE CHECK (short_code ~ '^[A-Z0-9]{2,6}$'),   -- BR-OR-05, EMQ-26
  name            text NOT NULL CHECK (btrim(name) <> ''),
  plan_id         uuid NOT NULL REFERENCES app.plans (id),
  employee_limit  integer NOT NULL CHECK (employee_limit > 0),
  limit_behaviour app.limit_behaviour NOT NULL DEFAULT 'allow_and_alert',         -- VC-01
  state           app.org_state NOT NULL DEFAULT 'active',
  row_version     integer NOT NULL DEFAULT 1,
  created_at      timestamptz NOT NULL DEFAULT now(),
  created_by      uuid,
  updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TYPE app.vendor_user_state AS ENUM ('active', 'disabled');

-- Vendor staff. Separate from client users (TSD §4.3). Only the vendor console role can read it.
CREATE TABLE app.vendor_users (
  id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  email                 text NOT NULL,
  display_name          text NOT NULL CHECK (btrim(display_name) <> ''),
  password_hash         text,                               -- argon2id PHC string
  totp_secret_encrypted bytea,                              -- second step (TSD §8)
  state                 app.vendor_user_state NOT NULL DEFAULT 'active',
  created_at            timestamptz NOT NULL DEFAULT now(),
  updated_at            timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX vendor_users_email_uq ON app.vendor_users (lower(email));

SELECT app.add_updated_at_trigger('app.plans');
SELECT app.add_updated_at_trigger('app.organisations');
SELECT app.add_updated_at_trigger('app.vendor_users');

-- Organisations: the API sees only its own row; the vendor console sees all.
ALTER TABLE app.organisations ENABLE ROW LEVEL SECURITY;
CREATE POLICY org_self ON app.organisations FOR SELECT TO app_api
  USING (id = app.current_org_id());
CREATE POLICY vendor_all ON app.organisations TO app_vendor
  USING (true) WITH CHECK (true);

GRANT SELECT ON app.plans, app.organisations TO app_api;
GRANT SELECT, INSERT, UPDATE ON app.plans, app.organisations, app.vendor_users TO app_vendor;
