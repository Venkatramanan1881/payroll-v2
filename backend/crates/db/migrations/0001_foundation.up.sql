-- 0001 Foundation: schema, extension, roles, tenant helpers.
-- Design: docs/database/DATA_MODEL.md §2 (tenancy), §3 (roles).

CREATE SCHEMA app;
REVOKE ALL ON SCHEMA app FROM PUBLIC;

-- Exclusion constraints on (uuid/text =, daterange &&) need btree_gist.
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- Group roles are cluster-wide and shared by the dev/test/live databases.
-- Each environment's LOGIN role (e.g. api_live) is created by operations,
-- made a member of one group role, and granted CONNECT on its own database only (DATA_MODEL §3).
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_api') THEN
    CREATE ROLE app_api NOLOGIN NOBYPASSRLS;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_outbox') THEN
    CREATE ROLE app_outbox NOLOGIN NOBYPASSRLS;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_vendor') THEN
    CREATE ROLE app_vendor NOLOGIN NOBYPASSRLS;
  END IF;
END $$;

GRANT USAGE ON SCHEMA app TO app_api, app_outbox, app_vendor;

-- Functions are not executable by PUBLIC by default; every grant is explicit.
ALTER DEFAULT PRIVILEGES IN SCHEMA app REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

-- The organisation and user for the current transaction.
-- The API runs: SELECT set_config('app.org_id', <org from verified token>, true)
-- as the first statement of every transaction. Unset => NULL => every policy returns no rows.
CREATE FUNCTION app.current_org_id() RETURNS uuid
LANGUAGE sql STABLE AS
$$ SELECT NULLIF(current_setting('app.org_id', true), '')::uuid $$;

CREATE FUNCTION app.current_user_id() RETURNS uuid
LANGUAGE sql STABLE AS
$$ SELECT NULLIF(current_setting('app.user_id', true), '')::uuid $$;

GRANT EXECUTE ON FUNCTION app.current_org_id(), app.current_user_id() TO app_api, app_outbox, app_vendor;

CREATE FUNCTION app.touch_updated_at() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN NEW.updated_at := now(); RETURN NEW; END $$;

CREATE FUNCTION app.reject_change() RETURNS trigger
LANGUAGE plpgsql AS
$$ BEGIN
  RAISE EXCEPTION '% on %.% is not allowed', TG_OP, TG_TABLE_SCHEMA, TG_TABLE_NAME
    USING ERRCODE = 'insufficient_privilege';
END $$;

-- Standard tenant policy: a row is visible and writable only for the transaction's organisation.
CREATE FUNCTION app.enable_tenant_rls(t regclass) RETURNS void
LANGUAGE plpgsql AS
$$ BEGIN
  EXECUTE format('ALTER TABLE %s ENABLE ROW LEVEL SECURITY', t);
  EXECUTE format(
    'CREATE POLICY tenant_isolation ON %s TO app_api '
    'USING (org_id = app.current_org_id()) WITH CHECK (org_id = app.current_org_id())', t);
END $$;

CREATE FUNCTION app.add_updated_at_trigger(t regclass) RETURNS void
LANGUAGE plpgsql AS
$$ BEGIN
  EXECUTE format(
    'CREATE TRIGGER touch_updated_at BEFORE UPDATE ON %s '
    'FOR EACH ROW EXECUTE FUNCTION app.touch_updated_at()', t);
END $$;

-- Build check (TSD §4.5): must return no rows. Any app table with an org_id column but RLS off is listed.
CREATE FUNCTION app.client_tables_without_rls() RETURNS TABLE (table_name text)
LANGUAGE sql STABLE AS
$$ SELECT c.relname::text
   FROM pg_class c
   JOIN pg_namespace n ON n.oid = c.relnamespace
   JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'org_id' AND NOT a.attisdropped
   WHERE n.nspname = 'app' AND c.relkind IN ('r', 'p') AND NOT c.relrowsecurity $$;
