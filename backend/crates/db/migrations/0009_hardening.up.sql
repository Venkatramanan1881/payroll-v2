-- 0009 Hardening from the Supabase security advisor (22 Nov 2026).
-- 1. Pin search_path on every app function that lacked one, so a caller's search_path
--    can never redirect a name inside it (lint 0011_function_search_path_mutable).
-- 2. Move btree_gist out of public (lint 0014_extension_in_public). Existing exclusion
--    constraints refer to its operator classes by OID, so they keep working.

ALTER FUNCTION app.current_org_id()                 SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.current_user_id()                SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.touch_updated_at()               SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.reject_change()                  SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.check_time_zone()                SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.sequence_only_increases()        SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.client_tables_without_rls()      SET search_path = pg_catalog, pg_temp;
ALTER FUNCTION app.enable_tenant_rls(regclass)      SET search_path = app, pg_catalog, pg_temp;
ALTER FUNCTION app.add_updated_at_trigger(regclass) SET search_path = app, pg_catalog, pg_temp;

CREATE SCHEMA IF NOT EXISTS extensions;
ALTER EXTENSION btree_gist SET SCHEMA extensions;
