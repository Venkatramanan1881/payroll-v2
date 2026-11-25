-- Development databases only: removes the default policy sets and attendance permissions.
DROP TRIGGER organisations_default_policy_set ON app.organisations;
DROP FUNCTION app.organisations_default_policy_set();
DROP FUNCTION app.add_default_policy_set(uuid);
DELETE FROM app.policy_set_versions;
DELETE FROM app.policy_sets;
DELETE FROM app.role_permissions
WHERE permission_code LIKE 'attendance.%'
   OR permission_code IN ('report.r1', 'report.r2', 'report.r3', 'report.r4', 'report.r5', 'report.r23', 'report.r26',
                          'dashboard.attendance', 'notification.settings.manage');
DELETE FROM app.permissions
WHERE code LIKE 'attendance.%'
   OR code IN ('report.r1', 'report.r2', 'report.r3', 'report.r4', 'report.r5', 'report.r23', 'report.r26',
               'dashboard.attendance', 'notification.settings.manage');
