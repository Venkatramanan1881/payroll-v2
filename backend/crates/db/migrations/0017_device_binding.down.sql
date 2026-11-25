DROP INDEX app.user_devices_one_bound;
ALTER TABLE app.user_devices
  DROP CONSTRAINT user_devices_unbind_reason,
  DROP CONSTRAINT user_devices_unbound_by_fk,
  DROP COLUMN unbind_reason,
  DROP COLUMN unbound_by,
  DROP COLUMN unbound_at,
  DROP COLUMN bound_at;
