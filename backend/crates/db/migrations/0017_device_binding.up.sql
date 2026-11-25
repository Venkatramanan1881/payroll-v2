-- 0017 Phone binding for app punches (DEC-20, API_CONTRACTS Package 2 §13).
-- A phone is bound at the first app sign-in after any reset; HR can unbind it (P3).
ALTER TABLE app.user_devices
  ADD COLUMN bound_at      timestamptz,
  ADD COLUMN unbound_at    timestamptz,
  ADD COLUMN unbound_by    uuid,
  ADD COLUMN unbind_reason text,
  ADD CONSTRAINT user_devices_unbound_by_fk FOREIGN KEY (org_id, unbound_by) REFERENCES app.users (org_id, id),
  ADD CONSTRAINT user_devices_unbind_reason
    CHECK (unbound_at IS NULL OR (bound_at IS NOT NULL AND btrim(coalesce(unbind_reason, '')) <> ''));

-- One bound phone per user at a time.
CREATE UNIQUE INDEX user_devices_one_bound ON app.user_devices (user_id)
  WHERE bound_at IS NOT NULL AND unbound_at IS NULL;
