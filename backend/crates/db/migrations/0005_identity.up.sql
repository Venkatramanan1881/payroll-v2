-- 0005 Identity & Access: users, credentials, devices, sessions, roles, permissions, scopes.
-- Domain: DOMAIN_MODEL §2.12-2.13, SM-6, PERMISSION_MATRIX. Rules: BR-EM-08, 10, 29, 32; TSD §8.
-- Secrets: PINs and passwords are stored only as argon2id hashes. A temporary PIN is never stored
-- in plain text anywhere: not in a table, the outbox, the audit log or application logs.

CREATE TYPE app.user_kind  AS ENUM ('admin', 'employee');
CREATE TYPE app.user_state AS ENUM ('invited', 'active', 'suspended', 'revoked');

CREATE TABLE app.users (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id       uuid NOT NULL REFERENCES app.organisations (id),
  kind         app.user_kind NOT NULL,
  employee_id  uuid,
  email        text,
  display_name text NOT NULL CHECK (btrim(display_name) <> ''),
  state        app.user_state NOT NULL DEFAULT 'active',
  row_version  integer NOT NULL DEFAULT 1,
  created_at   timestamptz NOT NULL DEFAULT now(),
  created_by   uuid DEFAULT app.current_user_id(),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  FOREIGN KEY (org_id, employee_id) REFERENCES app.employees (org_id, id),
  CHECK ((kind = 'employee') = (employee_id IS NOT NULL)),
  CHECK (kind = 'employee' OR email IS NOT NULL)
);
-- One user per employee. The row exists only once portal access has been granted (INV-16);
-- removing access sets it to 'revoked', granting again reactivates the same row (SM-6).
CREATE UNIQUE INDEX users_employee_uq ON app.users (employee_id) WHERE employee_id IS NOT NULL;
-- Admin sign-in looks the user up by email before the organisation is known, so an admin
-- email is unique across the platform (DBQ-01).
CREATE UNIQUE INDEX users_admin_email_uq ON app.users (lower(email)) WHERE kind = 'admin';

CREATE TYPE app.credential_kind  AS ENUM ('pin', 'password');
-- Stored states of SM-6. 'Locked' and 'Expired' are derived from locked_until and temporary_expires_at.
CREATE TYPE app.credential_state AS ENUM ('not_issued', 'temporary', 'active', 'revoked');

CREATE TABLE app.user_credentials (
  user_id               uuid PRIMARY KEY,
  org_id                uuid NOT NULL,
  kind                  app.credential_kind NOT NULL,
  secret_hash           text,                                   -- argon2id PHC string
  state                 app.credential_state NOT NULL DEFAULT 'not_issued',
  temporary_expires_at  timestamptz,                            -- issue time + 7 days
  failed_attempts       smallint NOT NULL DEFAULT 0 CHECK (failed_attempts >= 0),
  locked_until          timestamptz,                            -- 5 failures => +15 minutes
  totp_secret_encrypted bytea,                                  -- admins only (second step)
  updated_at            timestamptz NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, user_id) REFERENCES app.users (org_id, id),
  CHECK (state IN ('not_issued', 'revoked') OR secret_hash IS NOT NULL),
  CHECK (state <> 'temporary' OR temporary_expires_at IS NOT NULL),
  CHECK (kind = 'password' OR totp_secret_encrypted IS NULL)
);

-- Registered phones (DEC-20).
CREATE TABLE app.user_devices (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id            uuid NOT NULL,
  user_id           uuid NOT NULL,
  device_identifier text NOT NULL CHECK (btrim(device_identifier) <> ''),
  platform          text,
  first_seen_at     timestamptz NOT NULL DEFAULT now(),
  last_seen_at      timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE (user_id, device_identifier),
  FOREIGN KEY (org_id, user_id) REFERENCES app.users (org_id, id)
);

-- Refresh-token sessions. Token format and lookup: DATA_MODEL §6.3.
CREATE TABLE app.user_sessions (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id             uuid NOT NULL,
  user_id            uuid NOT NULL,
  device_id          uuid,
  refresh_token_hash text NOT NULL,                     -- SHA-256 of the token's secret part
  issued_at          timestamptz NOT NULL DEFAULT now(),
  expires_at         timestamptz NOT NULL,
  revoked_at         timestamptz,
  revoke_reason      text,
  FOREIGN KEY (org_id, user_id)   REFERENCES app.users (org_id, id),
  FOREIGN KEY (org_id, device_id) REFERENCES app.user_devices (org_id, id),
  CHECK (expires_at > issued_at),
  CHECK (revoked_at IS NULL OR revoke_reason IS NOT NULL)
);
CREATE INDEX user_sessions_live_idx ON app.user_sessions (user_id) WHERE revoked_at IS NULL;   -- revoke-all (BR-EM-08)

-- Permission catalogue: platform data, the operation codes of PERMISSION_MATRIX.md.
CREATE TABLE app.permissions (
  code        text PRIMARY KEY CHECK (code ~ '^[a-z0-9_]+(\.[a-z0-9_]+)+$'),
  description text NOT NULL,
  is_fixed    boolean NOT NULL DEFAULT false             -- 🔒 rows: cannot be granted or removed
);

-- Roles. org_id NULL = built-in template (read-only); each organisation gets its own editable copies.
CREATE TABLE app.roles (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id      uuid REFERENCES app.organisations (id),
  builtin_key text CHECK (builtin_key IN ('org_admin', 'hr_manager', 'manager', 'employee')),
  name        text NOT NULL CHECK (btrim(name) <> ''),
  state       app.item_state NOT NULL DEFAULT 'active',
  row_version integer NOT NULL DEFAULT 1,
  created_at  timestamptz NOT NULL DEFAULT now(),
  created_by  uuid DEFAULT app.current_user_id(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (org_id, id),
  UNIQUE NULLS NOT DISTINCT (org_id, name),
  CHECK (org_id IS NOT NULL OR builtin_key IS NOT NULL)
);
CREATE UNIQUE INDEX roles_builtin_uq ON app.roles (org_id, builtin_key) NULLS NOT DISTINCT
  WHERE builtin_key IS NOT NULL;

CREATE TABLE app.role_permissions (
  org_id          uuid,                                 -- NULL only for template roles
  role_id         uuid NOT NULL REFERENCES app.roles (id),
  permission_code text NOT NULL REFERENCES app.permissions (code),
  PRIMARY KEY (role_id, permission_code),
  FOREIGN KEY (org_id, role_id) REFERENCES app.roles (org_id, id)
);

CREATE TYPE app.scope_kind AS ENUM ('organisation', 'branches', 'departments');

CREATE TABLE app.user_roles (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id     uuid NOT NULL,
  user_id    uuid NOT NULL,
  role_id    uuid NOT NULL,
  scope_kind app.scope_kind NOT NULL DEFAULT 'organisation',   -- AC-07, EM-10
  created_at timestamptz NOT NULL DEFAULT now(),
  created_by uuid DEFAULT app.current_user_id(),
  UNIQUE (org_id, id),
  UNIQUE (user_id, role_id),
  FOREIGN KEY (org_id, user_id) REFERENCES app.users (org_id, id),
  FOREIGN KEY (org_id, role_id) REFERENCES app.roles (org_id, id)
);
CREATE INDEX user_roles_role_idx ON app.user_roles (org_id, role_id);   -- "last Org Admin" check (BR-EM-29)

CREATE TABLE app.user_role_scope_items (
  org_id        uuid NOT NULL,
  user_role_id  uuid NOT NULL,
  branch_id     uuid,
  department_id uuid,
  FOREIGN KEY (org_id, user_role_id)  REFERENCES app.user_roles (org_id, id) ON DELETE CASCADE,
  FOREIGN KEY (org_id, branch_id)     REFERENCES app.branches (org_id, id),
  FOREIGN KEY (org_id, department_id) REFERENCES app.departments (org_id, id),
  CHECK (num_nonnulls(branch_id, department_id) = 1),
  UNIQUE NULLS NOT DISTINCT (user_role_id, branch_id, department_id)
);

-- Users referenced from People tables (created before users existed).
ALTER TABLE app.status_change_requests
  ADD CONSTRAINT status_requests_raised_by_fk    FOREIGN KEY (org_id, raised_by)    REFERENCES app.users (org_id, id),
  ADD CONSTRAINT status_requests_decided_by_fk   FOREIGN KEY (org_id, decided_by)   REFERENCES app.users (org_id, id),
  ADD CONSTRAINT status_requests_cancelled_by_fk FOREIGN KEY (org_id, cancelled_by) REFERENCES app.users (org_id, id);
ALTER TABLE app.bulk_imports
  ADD CONSTRAINT bulk_imports_uploaded_by_fk FOREIGN KEY (org_id, uploaded_by) REFERENCES app.users (org_id, id);

-- Tenancy and privileges.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['users', 'user_credentials', 'user_devices', 'user_sessions',
                           'user_roles', 'user_role_scope_items'] LOOP
    PERFORM app.enable_tenant_rls(format('app.%I', t)::regclass);
  END LOOP;
  FOREACH t IN ARRAY ARRAY['users', 'user_credentials', 'roles'] LOOP
    PERFORM app.add_updated_at_trigger(format('app.%I', t)::regclass);
  END LOOP;
END $$;

-- Roles and their permissions: templates (org_id NULL) are readable by everyone, never writable.
ALTER TABLE app.roles ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_own_and_templates ON app.roles FOR SELECT TO app_api
  USING (org_id = app.current_org_id() OR org_id IS NULL);
CREATE POLICY write_own ON app.roles FOR INSERT TO app_api WITH CHECK (org_id = app.current_org_id());
CREATE POLICY update_own ON app.roles FOR UPDATE TO app_api
  USING (org_id = app.current_org_id()) WITH CHECK (org_id = app.current_org_id());

ALTER TABLE app.role_permissions ENABLE ROW LEVEL SECURITY;
CREATE POLICY read_own_and_templates ON app.role_permissions FOR SELECT TO app_api
  USING (org_id = app.current_org_id() OR org_id IS NULL);
CREATE POLICY insert_own ON app.role_permissions FOR INSERT TO app_api WITH CHECK (org_id = app.current_org_id());
CREATE POLICY delete_own ON app.role_permissions FOR DELETE TO app_api USING (org_id = app.current_org_id());

GRANT SELECT ON app.permissions TO app_api;
GRANT SELECT, INSERT, UPDATE ON app.users, app.user_credentials, app.user_devices, app.user_sessions TO app_api;
GRANT SELECT, INSERT, UPDATE ON app.roles TO app_api;
GRANT SELECT, INSERT, DELETE ON app.role_permissions, app.user_roles, app.user_role_scope_items TO app_api;
GRANT UPDATE (scope_kind) ON app.user_roles TO app_api;
