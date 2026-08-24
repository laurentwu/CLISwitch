PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS app_settings (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  language TEXT NOT NULL DEFAULT 'zh-CN',
  theme TEXT NOT NULL DEFAULT 'system',
  scan_on_startup INTEGER NOT NULL DEFAULT 1,
  plaintext_risk_accepted INTEGER NOT NULL DEFAULT 0,
  revision INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('api', 'oauth')),
  coding_plan INTEGER NOT NULL DEFAULT 0,
  coding_plan_name TEXT,
  oauth_kind TEXT CHECK (oauth_kind IN ('anthropic', 'codex')),
  revision INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_connections (
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
  UNIQUE(provider_id, protocol)
);

CREATE TABLE IF NOT EXISTS oauth_credentials (
  provider_id TEXT PRIMARY KEY REFERENCES provider_profiles(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  digest TEXT NOT NULL,
  account_id TEXT,
  account_label TEXT,
  manually_modified INTEGER NOT NULL DEFAULT 0,
  verification_status TEXT NOT NULL DEFAULT 'not-online-verified',
  verified_at TEXT
);

CREATE TABLE IF NOT EXISTS saved_configurations (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL UNIQUE,
  creation_order INTEGER NOT NULL UNIQUE,
  revision INTEGER NOT NULL DEFAULT 1,
  last_applied_at TEXT,
  last_apply_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS configuration_targets (
  configuration_id TEXT NOT NULL REFERENCES saved_configurations(id) ON DELETE CASCADE,
  cli_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('api', 'oauth')),
  provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE RESTRICT,
  connection_id TEXT REFERENCES provider_connections(id) ON DELETE RESTRICT,
  model TEXT NOT NULL,
  PRIMARY KEY(configuration_id, cli_id),
  CHECK ((target_kind = 'api' AND connection_id IS NOT NULL) OR (target_kind = 'oauth' AND connection_id IS NULL))
);

CREATE TABLE IF NOT EXISTS manual_cli_locations (
  cli_id TEXT PRIMARY KEY,
  executable_path TEXT,
  config_directory TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS active_oauth_bindings (
  cli_id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES provider_profiles(id) ON DELETE CASCADE,
  native_digest TEXT NOT NULL,
  account_identity TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS latest_apply_runs (
  configuration_id TEXT PRIMARY KEY REFERENCES saved_configurations(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL,
  status TEXT NOT NULL,
  summary_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS backup_metadata (
  id TEXT PRIMARY KEY,
  cli_id TEXT NOT NULL,
  source_file_id TEXT NOT NULL,
  original_path TEXT NOT NULL,
  created_at TEXT NOT NULL,
  configuration_id TEXT,
  original_digest TEXT,
  permissions INTEGER,
  originally_existed INTEGER NOT NULL,
  contains_credentials INTEGER NOT NULL,
  relative_backup_path TEXT,
  UNIQUE(source_file_id, created_at)
);

CREATE INDEX IF NOT EXISTS idx_connections_provider ON provider_connections(provider_id);
CREATE INDEX IF NOT EXISTS idx_targets_provider ON configuration_targets(provider_id);
CREATE INDEX IF NOT EXISTS idx_backups_source ON backup_metadata(source_file_id, created_at DESC);

INSERT OR IGNORE INTO app_settings(singleton, updated_at) VALUES (1, CURRENT_TIMESTAMP);
