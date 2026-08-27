-- CLISwitch is still pre-release and the provider model is intentionally replaced by the
-- models.dev-backed design. Remove legacy API records and any saved configuration that depends
-- on them. OAuth providers, OAuth-only configurations, settings, and backups are retained.
DELETE FROM saved_configurations
WHERE id IN (
  SELECT DISTINCT configuration_id
  FROM configuration_targets
  WHERE target_kind = 'api'
);

DELETE FROM provider_profiles
WHERE kind = 'api';
