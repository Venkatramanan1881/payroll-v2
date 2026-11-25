-- 0010 Let the API change a custom field's type (O19). The API allows it only while the field
-- has no values (cannot_change_type_with_values); the options CHECK still matches options to the type.
GRANT UPDATE (field_type) ON app.custom_field_definitions TO app_api;
