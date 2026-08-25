ALTER TABLE provider_profiles ADD COLUMN template_id TEXT;

ALTER TABLE provider_connections ADD COLUMN template_endpoint_id TEXT;
ALTER TABLE provider_connections ADD COLUMN credential_slot_id TEXT NOT NULL DEFAULT 'api-key';

-- Existing API profiles intentionally remain custom. Giving each legacy connection its own slot
-- preserves the previous ability to use a different key per endpoint without guessing that two
-- plaintext values represent the same account.
UPDATE provider_connections
SET credential_slot_id = 'legacy-' || id;

-- Existing native auth profiles have an exact, lossless template mapping.
UPDATE provider_profiles
SET template_id = CASE oauth_kind
  WHEN 'anthropic' THEN 'anthropic-auth'
  WHEN 'codex' THEN 'codex-auth'
END
WHERE kind = 'oauth';

-- Endpoint identity, not protocol, is the unique unit. A future template may expose two
-- independent endpoints using the same wire protocol, so remove the v1 provider/protocol
-- uniqueness constraint without discarding any target references.
CREATE TABLE provider_connections_v2 (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE CASCADE,
  protocol TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  auth_type TEXT NOT NULL,
  api_key TEXT NOT NULL,
  default_model TEXT NOT NULL,
  verification_status TEXT NOT NULL DEFAULT 'never-tested',
  verified_at TEXT,
  verification_error TEXT,
  template_endpoint_id TEXT,
  credential_slot_id TEXT NOT NULL
);

INSERT INTO provider_connections_v2(
  id, provider_id, protocol, endpoint, auth_type, api_key, default_model,
  verification_status, verified_at, verification_error, template_endpoint_id, credential_slot_id
)
SELECT
  id, provider_id, protocol, endpoint, auth_type, api_key, default_model,
  verification_status, verified_at, verification_error, template_endpoint_id, credential_slot_id
FROM provider_connections;

CREATE TABLE configuration_targets_v2 (
  configuration_id TEXT NOT NULL REFERENCES saved_configurations(id) ON DELETE CASCADE,
  cli_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('api', 'oauth')),
  provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE RESTRICT,
  connection_id TEXT REFERENCES provider_connections_v2(id) ON DELETE RESTRICT,
  model TEXT NOT NULL,
  PRIMARY KEY(configuration_id, cli_id),
  CHECK ((target_kind = 'api' AND connection_id IS NOT NULL) OR (target_kind = 'oauth' AND connection_id IS NULL))
);

INSERT INTO configuration_targets_v2(
  configuration_id, cli_id, target_kind, provider_id, connection_id, model
)
SELECT configuration_id, cli_id, target_kind, provider_id, connection_id, model
FROM configuration_targets;

DROP TABLE configuration_targets;
DROP TABLE provider_connections;
ALTER TABLE provider_connections_v2 RENAME TO provider_connections;
ALTER TABLE configuration_targets_v2 RENAME TO configuration_targets;

CREATE INDEX IF NOT EXISTS idx_provider_profiles_template ON provider_profiles(template_id);
CREATE INDEX IF NOT EXISTS idx_provider_connections_template_endpoint
  ON provider_connections(template_endpoint_id);
CREATE INDEX IF NOT EXISTS idx_connections_provider ON provider_connections(provider_id);
CREATE INDEX IF NOT EXISTS idx_targets_provider ON configuration_targets(provider_id);
