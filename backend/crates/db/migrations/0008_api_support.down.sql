DROP TABLE app.export_jobs;
DROP TYPE app.export_state;
DROP TABLE app.idempotency_keys;
ALTER TABLE app.user_credentials
  DROP CONSTRAINT otp_hash_has_expiry,
  DROP COLUMN otp_expires_at,
  DROP COLUMN otp_hash;
ALTER TABLE app.employment_periods DROP COLUMN probation_reminder_sent_for;
ALTER TABLE app.custom_field_definitions DROP COLUMN inactive_options;
