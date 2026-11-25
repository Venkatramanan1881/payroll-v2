DROP FUNCTION app.provision_organisation(uuid, text, text, uuid, integer, app.limit_behaviour, text, text);
DROP FUNCTION app.active_org_ids();
DROP FUNCTION app.org_id_for_admin_email(text);
DROP FUNCTION app.org_id_for_short_code(text);
DELETE FROM app.role_permissions WHERE org_id IS NULL;
DELETE FROM app.roles WHERE org_id IS NULL;
DELETE FROM app.permissions;
