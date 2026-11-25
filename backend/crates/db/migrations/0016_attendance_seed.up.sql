-- 0016 Attendance — seed: permission codes and built-in role defaults (PERMISSION_MATRIX Package 2),
-- and a default policy set for every organisation (AC-AT-21.1), for new and existing ones.

INSERT INTO app.permissions (code, description, is_fixed) VALUES
  ('attendance.shift.manage',               'Shifts and shift versions', false),
  ('attendance.roster.manage',              'Default shifts, rosters, upload, copy, rotation', false),
  ('attendance.roster.view',                'View rosters', false),
  ('attendance.shift_swap.request',         'Ask for a shift change or swap (SH-12)', false),
  ('attendance.shift_swap.approve',         'Decide a shift change or swap', false),
  ('attendance.calendar.manage',            'Holiday calendars, branch links, weekly-off patterns', false),
  ('attendance.optional_holiday.request',   'Pick, withdraw or cancel optional holidays', false),
  ('attendance.optional_holiday.approve',   'Decide optional holiday picks', false),
  ('attendance.device.manage',              'Register devices, rotate tokens, deactivate devices', false),
  ('attendance.enrolment.manage',           'Map enrolment IDs, set enrolment status, see unmapped IDs', false),
  ('attendance.fence.manage',               'Geo-fences: create, test, activate', false),
  ('attendance.device_binding.reset',       'Reset an employee''s bound phone', false),
  ('attendance.punch.self',                 'Check in and out from the app or web', true),
  ('attendance.punch.view',                 'View punches with location and selfie', false),
  ('attendance.punch.review',               'Approve or reject flagged punches', false),
  ('attendance.punch.import',               'Import punches from a file or the API', false),
  ('attendance.punch.manual',               'Manual attendance for a device failure', false),
  ('attendance.punch.adjudicate',           'Exclude, include again, move date, change direction of a punch', false),
  ('attendance.policy.manage',              'Policy sets, versions, assignments; period settings', false),
  ('attendance.policy.approve',             'Approve settings when maker-checker is on', false),
  ('attendance.day.view',                   'View day results and explanations', false),
  ('attendance.day.correct',                'Direct correction of a day (RG-10)', false),
  ('attendance.waiver.manage',              'Bulk waivers of late or early marks', false),
  ('attendance.outage.regularise',          'Bulk regularisation for a device outage (RG-11)', false),
  ('attendance.regularisation.request',     'Raise or withdraw a regularisation (own, or on behalf)', false),
  ('attendance.regularisation.approve',     'Decide a regularisation', false),
  ('attendance.regularisation.cancel',      'Cancel an approved regularisation, with reason', false),
  ('attendance.period.view',                'View periods, lock units and the pre-lock checklist', false),
  ('attendance.period.lock',                'Lock, finalise and mark periods as paid', false),
  ('attendance.period.step_back',           'Step a lock unit back one state, with reason', true),
  ('attendance.adjustment.view',            'View adjustments', false),
  ('attendance.payroll_input.export',       'Payroll input export (MD-06, R13)', false),
  ('report.r1',                             'Daily attendance', false),
  ('report.r2',                             'Monthly attendance register', false),
  ('report.r3',                             'Late arrival and early check-out', false),
  ('report.r4',                             'Missed punch and regularisation', false),
  ('report.r5',                             'Exceptions: flagged punches, geo-fence, late sync, device status', false),
  ('report.r23',                            'Adjustments and arrears', false),
  ('report.r26',                            'Settings history and assignments', false),
  ('dashboard.attendance',                  'Attendance dashboard widgets', false),
  ('notification.settings.manage',          'Notification events, channels and recipients', false);

-- Defaults per built-in role, for the templates (org NULL) and every organisation's built-in copies.
-- Custom roles are left alone.
WITH grants (builtin_key, code) AS (
  SELECT 'org_admin', code FROM app.permissions
  WHERE (code LIKE 'attendance.%' OR code IN ('report.r1', 'report.r2', 'report.r3', 'report.r4', 'report.r5',
                                              'report.r23', 'report.r26', 'dashboard.attendance', 'notification.settings.manage'))
    AND code NOT IN ('attendance.punch.self', 'attendance.shift_swap.request', 'attendance.optional_holiday.request')
  UNION ALL
  SELECT 'hr_manager', code FROM app.permissions
  WHERE (code LIKE 'attendance.%' OR code IN ('report.r1', 'report.r2', 'report.r3', 'report.r4', 'report.r5',
                                              'report.r23', 'report.r26', 'dashboard.attendance', 'notification.settings.manage'))
    AND code NOT IN ('attendance.punch.self', 'attendance.shift_swap.request', 'attendance.optional_holiday.request',
                     'attendance.policy.approve', 'attendance.period.step_back')
  UNION ALL
  SELECT 'manager', unnest(ARRAY['attendance.roster.view', 'attendance.shift_swap.approve', 'attendance.optional_holiday.approve',
                                 'attendance.punch.view', 'attendance.punch.review', 'attendance.day.view',
                                 'attendance.regularisation.approve', 'report.r1', 'report.r2', 'report.r3', 'report.r4',
                                 'report.r5', 'dashboard.attendance'])
  UNION ALL
  SELECT 'employee', unnest(ARRAY['attendance.roster.view', 'attendance.shift_swap.request',
                                  'attendance.optional_holiday.request', 'attendance.punch.self', 'attendance.punch.view',
                                  'attendance.day.view', 'attendance.regularisation.request', 'attendance.adjustment.view',
                                  'dashboard.attendance'])
)
INSERT INTO app.role_permissions (org_id, role_id, permission_code)
SELECT r.org_id, r.id, g.code
FROM app.roles r JOIN grants g ON g.builtin_key = r.builtin_key
ON CONFLICT DO NOTHING;

-- Every organisation has one default policy set holding all defaults ('{}' = BRD defaults as changed by decisions).
CREATE FUNCTION app.add_default_policy_set(p_org uuid) RETURNS void
LANGUAGE plpgsql SET search_path = app, pg_temp AS
$$
DECLARE v_set uuid;
BEGIN
  IF EXISTS (SELECT 1 FROM app.policy_sets WHERE org_id = p_org AND is_default) THEN
    RETURN;
  END IF;
  INSERT INTO app.policy_sets (org_id, name, is_default) VALUES (p_org, 'Default', true) RETURNING id INTO v_set;
  INSERT INTO app.policy_set_versions (org_id, policy_set_id, valid_from, state, settings, change_reason, author_id)
  VALUES (p_org, v_set, DATE '2000-01-01', 'approved', '{}', 'Created with the organisation', NULL);
END
$$;
REVOKE EXECUTE ON FUNCTION app.add_default_policy_set(uuid) FROM PUBLIC;

-- New organisations: provision_organisation() (SECURITY DEFINER, owner) inserts the organisation; this trigger
-- runs in that same transaction with the owner's rights, so no change to provision_organisation is needed.
CREATE FUNCTION app.organisations_default_policy_set() RETURNS trigger
LANGUAGE plpgsql SET search_path = app, pg_temp AS
$$ BEGIN PERFORM app.add_default_policy_set(NEW.id); RETURN NEW; END $$;
REVOKE EXECUTE ON FUNCTION app.organisations_default_policy_set() FROM PUBLIC;
CREATE TRIGGER organisations_default_policy_set AFTER INSERT ON app.organisations
  FOR EACH ROW EXECUTE FUNCTION app.organisations_default_policy_set();

-- Existing organisations.
SELECT app.add_default_policy_set(id) FROM app.organisations;
