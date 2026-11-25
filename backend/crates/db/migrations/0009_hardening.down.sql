-- The extensions schema is left in place: on Supabase it is platform-owned and holds other extensions.
ALTER EXTENSION btree_gist SET SCHEMA public;

ALTER FUNCTION app.add_updated_at_trigger(regclass) RESET search_path;
ALTER FUNCTION app.enable_tenant_rls(regclass)      RESET search_path;
ALTER FUNCTION app.client_tables_without_rls()      RESET search_path;
ALTER FUNCTION app.sequence_only_increases()        RESET search_path;
ALTER FUNCTION app.check_time_zone()                RESET search_path;
ALTER FUNCTION app.reject_change()                  RESET search_path;
ALTER FUNCTION app.touch_updated_at()               RESET search_path;
ALTER FUNCTION app.current_user_id()                RESET search_path;
ALTER FUNCTION app.current_org_id()                 RESET search_path;
