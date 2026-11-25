-- 0007 Reference seed (permissions, built-in role templates) and the few SECURITY DEFINER functions
-- that must work before an organisation is set: sign-in lookups, job fan-out, organisation provisioning.
-- Source: PERMISSION_MATRIX.md (Gate 2). Each function returns only what its caller needs.

INSERT INTO app.permissions (code, description, is_fixed) VALUES
  ('org.settings.edit',             'Edit organisation settings OR-01..04, EM-01..11', false),
  ('org.branch.manage',             'Create, edit, deactivate branches', false),
  ('org.lists.manage',              'Departments, designations, categories, employee types, exit reasons', false),
  ('org.contractor.manage',         'Contractors', false),
  ('org.payroll_group.manage',      'Create and edit payroll groups, bulk assign by rule', false),
  ('employee.view',                 'View employee record', false),
  ('employee.create',               'Create employee', false),
  ('employee.edit',                 'Edit personal and job details, schedule and cancel job changes', false),
  ('employee.history.view',         'View dated history and "as on date"', false),
  ('employee.status.raise',         'Raise or withdraw a status change', false),
  ('employee.status.approve',       'Approve or reject a status change', false),
  ('employee.status.cancel',        'Cancel an approved future status change, with reason', false),
  ('employee.probation.extend',     'Change probation end date, with reason', false),
  ('employee.rejoin',               'Rejoin an employee (keeps old code)', false),
  ('employee.bulk_import',          'Upload a bulk import and view its result', false),
  ('employee.pin.issue',            'Grant or remove portal access; issue or reset temporary PIN', false),
  ('employee.profile.edit_own',     'Edit own contact details (EM-03)', false),
  ('employee.custom_fields.manage', 'Define custom fields', false),
  ('access.role.edit',              'Edit role permissions, create custom roles', false),
  ('access.role.assign',            'Assign users to roles and scopes', false),
  ('report.r10',                    'Employee list and headcount', false),
  ('report.r11',                    'Joiners and leavers', false),
  ('report.export',                 'Export a report to spreadsheet', false),
  ('audit.view',                    'View the audit trail', false),
  ('support.access',                'Approve time-limited vendor support access (VC-R6)', true);

INSERT INTO app.roles (org_id, builtin_key, name) VALUES
  (NULL, 'org_admin',  'Organisation Admin'),
  (NULL, 'hr_manager', 'HR Manager'),
  (NULL, 'manager',    'Manager'),
  (NULL, 'employee',   'Employee');

-- Org Admin: everything in the catalogue. HR Manager: everything except approving support access.
-- Scope (S / Chain / Own) is not a permission: it comes from user_roles.scope_kind and the API's scope resolver.
INSERT INTO app.role_permissions (org_id, role_id, permission_code)
SELECT NULL, r.id, p.code
FROM app.roles r CROSS JOIN app.permissions p
WHERE r.org_id IS NULL
  AND (   (r.builtin_key = 'org_admin'  AND p.code <> 'employee.profile.edit_own')
       OR (r.builtin_key = 'hr_manager' AND p.code NOT IN ('employee.profile.edit_own', 'support.access'))
       OR (r.builtin_key = 'manager'    AND p.code IN ('employee.view', 'report.r10', 'report.r11'))
       OR (r.builtin_key = 'employee'   AND p.code IN ('employee.view', 'employee.profile.edit_own')));

-- Employee sign-in: organisation short code -> organisation id. Suspended organisations can still sign in (VC-R4).
CREATE FUNCTION app.org_id_for_short_code(p_short_code text) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = app, pg_temp AS
$$ SELECT id FROM app.organisations WHERE short_code = upper(p_short_code) $$;

-- Admin sign-in: email -> organisation id (admin emails are platform-unique, DBQ-01).
CREATE FUNCTION app.org_id_for_admin_email(p_email text) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = app, pg_temp AS
$$ SELECT org_id FROM app.users WHERE kind = 'admin' AND lower(email) = lower(p_email) $$;

-- Timed jobs fan out one task per organisation (TSD §4.2).
CREATE FUNCTION app.active_org_ids() RETURNS SETOF uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = app, pg_temp AS
$$ SELECT id FROM app.organisations WHERE state = 'active' $$;

GRANT EXECUTE ON FUNCTION app.org_id_for_short_code(text), app.org_id_for_admin_email(text), app.active_org_ids()
  TO app_api;

-- Vendor console: create an organisation with everything it needs on day one (VC-R1, PG-R5, EM-R5),
-- without giving the vendor role any access to client tables.
CREATE FUNCTION app.provision_organisation(
  p_vendor_user_id  uuid,
  p_short_code      text,
  p_name            text,
  p_plan_id         uuid,
  p_employee_limit  integer,
  p_limit_behaviour app.limit_behaviour,
  p_admin_email     text,
  p_admin_name      text
) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path = app, pg_temp AS
$$
DECLARE
  v_org   uuid;
  v_admin uuid;
  v_role  uuid;
  r       record;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM app.vendor_users WHERE id = p_vendor_user_id AND state = 'active') THEN
    RAISE EXCEPTION 'unknown or disabled vendor user' USING ERRCODE = 'insufficient_privilege';
  END IF;

  INSERT INTO app.organisations (short_code, name, plan_id, employee_limit, limit_behaviour, created_by)
  VALUES (p_short_code, p_name, p_plan_id, p_employee_limit, p_limit_behaviour, p_vendor_user_id)
  RETURNING id INTO v_org;

  INSERT INTO app.payroll_groups (org_id, name, pay_frequency, is_default)
  VALUES (v_org, 'All employees', 'monthly', true);

  FOR r IN SELECT id, builtin_key, name FROM app.roles WHERE org_id IS NULL LOOP
    INSERT INTO app.roles (org_id, builtin_key, name) VALUES (v_org, r.builtin_key, r.name)
    RETURNING id INTO v_role;
    INSERT INTO app.role_permissions (org_id, role_id, permission_code)
    SELECT v_org, v_role, permission_code FROM app.role_permissions WHERE role_id = r.id;
  END LOOP;

  INSERT INTO app.exit_reasons (org_id, code, name) VALUES
    (v_org, 'RESIGNED', 'Resigned'), (v_org, 'TERMINATED', 'Terminated'), (v_org, 'ABSCONDED', 'Absconded'),
    (v_org, 'RETIRED', 'Retired'), (v_org, 'CONTRACT_ENDED', 'Contract ended'), (v_org, 'OTHER', 'Other');

  INSERT INTO app.users (org_id, kind, email, display_name, state)
  VALUES (v_org, 'admin', p_admin_email, p_admin_name, 'invited')
  RETURNING id INTO v_admin;
  INSERT INTO app.user_credentials (user_id, org_id, kind) VALUES (v_admin, v_org, 'password');
  INSERT INTO app.user_roles (org_id, user_id, role_id)
  SELECT v_org, v_admin, id FROM app.roles WHERE org_id = v_org AND builtin_key = 'org_admin';

  INSERT INTO app.audit_log (org_id, actor_kind, actor_id, action, target_type, target_id, new_values)
  VALUES (v_org, 'vendor', p_vendor_user_id, 'org.create', 'organisation', v_org,
          jsonb_build_object('short_code', p_short_code, 'name', p_name, 'plan_id', p_plan_id,
                             'employee_limit', p_employee_limit, 'limit_behaviour', p_limit_behaviour,
                             'first_admin_user_id', v_admin));
  RETURN v_org;
END
$$;

GRANT EXECUTE ON FUNCTION app.provision_organisation(uuid, text, text, uuid, integer, app.limit_behaviour, text, text)
  TO app_vendor;
