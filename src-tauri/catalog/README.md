# Provider catalog

These bundled JSONC files are the source of truth for compatibility. They are compiled into the
application and exposed read-only to the frontend; user-edited provider instances stay in SQLite.

- `clis.jsonc`: CLI protocol capabilities, auth modes, and protocol adapter packages.
- `provider-templates.jsonc`: API templates and auth templates. An API template owns credential
  slots; endpoints reference a slot and contain suggested (not exclusive) models.
- `cli-provider-relations.jsonc`: explicit CLI-to-endpoint/auth-mode joins, including native
  provider aliases and any relation-specific package or Base URL override.

Keep persisted IDs stable. To add a template, add its relations in the same change. A
multi-endpoint relation may omit `default` on every endpoint when the user must choose explicitly;
at most one endpoint may be the default. The Rust catalog validator and tests reject broken
references, duplicate routes, invalid URLs, or incompatible protocol packages.
