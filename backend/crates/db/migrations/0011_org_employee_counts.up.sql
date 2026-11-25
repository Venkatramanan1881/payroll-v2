-- 0011 Vendor console V2: each organisation's headcount, without giving app_vendor any access to
-- client tables (TSD §4.3). Same count as the API's plan-limit check (BR-EM-12).
CREATE FUNCTION app.org_employee_counts() RETURNS TABLE (org_id uuid, employees bigint)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = app, pg_temp AS
$$ SELECT org_id, count(*) FROM app.employees WHERE is_current GROUP BY org_id $$;

REVOKE EXECUTE ON FUNCTION app.org_employee_counts() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.org_employee_counts() TO app_vendor;
