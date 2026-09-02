use std::{
    collections::{HashMap, HashSet},
    path::Path,
    str::FromStr,
    time::Duration,
};

use chrono::{DateTime, Utc};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use url::Url;
use uuid::Uuid;

use crate::{
    domain::{
        ActiveOAuthBinding, ApiProviderData, AppLanguage, AppSettings, AppTheme, CliId,
        CliProtocol, ConfigurationTarget, ConnectionAuthType, ManualCliLocation, OAuthKind,
        OAuthProviderData, ProviderConnection, ProviderData, ProviderProfile, PublicProvider,
        SavedConfiguration, VerificationInfo, VerificationStatus, normalize_name,
    },
    error::{AppError, AppResult},
    filesystem::private_paths::set_private_file_permissions,
    services::redaction::Redactor,
};

#[derive(Debug, Clone)]
pub struct Repository {
    pool: SqlitePool,
    redactor: Redactor,
    data_root: std::path::PathBuf,
}

impl Repository {
    pub async fn open(path: &Path, redactor: Redactor) -> AppResult<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        set_private_file_permissions(path).await?;
        let data_root = path
            .parent()
            .ok_or_else(|| AppError::Validation("database path has no parent".into()))?
            .to_path_buf();
        let repository = Self {
            pool,
            redactor,
            data_root,
        };
        repository.register_saved_secrets().await?;
        Ok(repository)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn register_saved_secrets(&self) -> AppResult<()> {
        let rows = sqlx::query("SELECT api_key FROM provider_connections")
            .fetch_all(&self.pool)
            .await?;
        for row in rows {
            self.redactor.register(row.try_get::<String, _>("api_key")?);
        }
        Ok(())
    }

    pub async fn list_providers(&self) -> AppResult<Vec<PublicProvider>> {
        let ids = sqlx::query("SELECT id FROM provider_profiles ORDER BY created_at, id")
            .fetch_all(&self.pool)
            .await?;
        let mut providers = Vec::with_capacity(ids.len());
        for row in ids {
            let id = parse_uuid(row.try_get::<String, _>("id")?)?;
            let provider = self.get_provider(id).await?;
            let references = sqlx::query(
                "SELECT DISTINCT c.name, c.creation_order FROM saved_configurations c JOIN configuration_targets t ON t.configuration_id = c.id WHERE t.provider_id = ? ORDER BY c.creation_order",
            )
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| row.try_get::<String, _>("name"))
            .collect::<Result<Vec<_>, _>>()?;
            providers.push(provider.public(references));
        }
        Ok(providers)
    }

    pub async fn get_provider(&self, id: Uuid) -> AppResult<ProviderProfile> {
        let row = sqlx::query("SELECT * FROM provider_profiles WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("provider {id}")))?;
        self.hydrate_provider(row).await
    }

    async fn hydrate_provider(&self, row: SqliteRow) -> AppResult<ProviderProfile> {
        let id = parse_uuid(row.try_get::<String, _>("id")?)?;
        let kind: String = row.try_get("kind")?;
        let data = match kind.as_str() {
            "api" => {
                let rows = sqlx::query(
                    "SELECT * FROM provider_connections WHERE provider_id = ? ORDER BY rowid",
                )
                .bind(id.to_string())
                .fetch_all(&self.pool)
                .await?;
                let connections = rows
                    .into_iter()
                    .map(|connection| {
                        let api_key: String = connection.try_get("api_key")?;
                        self.redactor.register(&api_key);
                        Ok(ProviderConnection {
                            id: parse_uuid(connection.try_get::<String, _>("id")?)?,
                            template_endpoint_id: connection.try_get("template_endpoint_id")?,
                            credential_slot_id: connection.try_get("credential_slot_id")?,
                            protocol: CliProtocol::from_str(
                                &connection.try_get::<String, _>("protocol")?,
                            )?,
                            endpoint: Url::parse(&connection.try_get::<String, _>("endpoint")?)?,
                            auth_type: ConnectionAuthType::from_str(
                                &connection.try_get::<String, _>("auth_type")?,
                            )?,
                            api_key,
                            default_model: connection.try_get("default_model")?,
                            verification: VerificationInfo {
                                status: VerificationStatus::from_str(
                                    &connection.try_get::<String, _>("verification_status")?,
                                )?,
                                verified_at: optional_datetime(&connection, "verified_at")?,
                                error: connection.try_get("verification_error")?,
                            },
                        })
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                ProviderData::Api(ApiProviderData { connections })
            }
            "oauth" => {
                let oauth = sqlx::query("SELECT * FROM oauth_credentials WHERE provider_id = ?")
                    .bind(id.to_string())
                    .fetch_optional(&self.pool)
                    .await?
                    .ok_or_else(|| AppError::NotFound(format!("OAuth payload for {id}")))?;
                let oauth_path = self
                    .oauth_absolute_path(&oauth.try_get::<String, _>("relative_path")?)
                    .await?;
                let raw_content = tokio::fs::read_to_string(oauth_path).await?;
                self.redactor.register(&raw_content);
                ProviderData::Oauth(OAuthProviderData {
                    oauth_kind: OAuthKind::from_str(
                        &row.try_get::<Option<String>, _>("oauth_kind")?
                            .ok_or_else(|| AppError::Serialization("missing OAuth kind".into()))?,
                    )?,
                    account_id: oauth.try_get("account_id")?,
                    account_label: oauth.try_get("account_label")?,
                    raw_content,
                    digest: oauth.try_get("digest")?,
                    manually_modified: oauth.try_get::<i64, _>("manually_modified")? != 0,
                    verification: VerificationInfo {
                        status: VerificationStatus::from_str(
                            &oauth.try_get::<String, _>("verification_status")?,
                        )?,
                        verified_at: optional_datetime(&oauth, "verified_at")?,
                        error: None,
                    },
                })
            }
            _ => return Err(AppError::Serialization("invalid provider kind".into())),
        };
        Ok(ProviderProfile {
            id,
            name: row.try_get("name")?,
            template_id: row.try_get("template_id")?,
            revision: row.try_get("revision")?,
            created_at: required_datetime(&row, "created_at")?,
            updated_at: required_datetime(&row, "updated_at")?,
            data,
        })
    }

    async fn oauth_absolute_path(&self, relative: &str) -> AppResult<std::path::PathBuf> {
        let path = crate::filesystem::private_paths::PrivatePaths::safe_relative(
            &self.data_root,
            Path::new(relative),
        )?;
        let auth_root = self.data_root.join("auth");
        if !path.starts_with(&auth_root) {
            return Err(AppError::Blocked(
                "OAuth credential path escaped the auth directory".into(),
            ));
        }
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Blocked(
                "OAuth credential path is not a regular file".into(),
            ));
        }
        let canonical_root = tokio::fs::canonicalize(auth_root).await?;
        let canonical_path = tokio::fs::canonicalize(path).await?;
        if !canonical_path.starts_with(canonical_root) {
            return Err(AppError::Blocked(
                "OAuth credential path escaped the auth directory".into(),
            ));
        }
        Ok(canonical_path)
    }

    pub async fn insert_provider(
        &self,
        provider: &ProviderProfile,
        oauth_relative_path: Option<&Path>,
    ) -> AppResult<()> {
        self.insert_provider_transaction(provider, oauth_relative_path, None)
            .await
    }

    pub async fn insert_provider_with_active_oauth_binding(
        &self,
        provider: &ProviderProfile,
        oauth_relative_path: &Path,
        cli_id: CliId,
        native_digest: &str,
        account_identity: Option<&str>,
    ) -> AppResult<()> {
        validate_active_oauth_binding(provider, cli_id, native_digest)?;
        self.insert_provider_transaction(
            provider,
            Some(oauth_relative_path),
            Some((cli_id, native_digest, account_identity)),
        )
        .await
    }

    async fn insert_provider_transaction(
        &self,
        provider: &ProviderProfile,
        oauth_relative_path: Option<&Path>,
        active_oauth_binding: Option<(CliId, &str, Option<&str>)>,
    ) -> AppResult<()> {
        provider.validate()?;
        let mut transaction = self.pool.begin().await?;
        let (kind, oauth_kind) = match &provider.data {
            ProviderData::Api(_) => ("api", None),
            ProviderData::Oauth(oauth) => ("oauth", Some(oauth.oauth_kind.to_string())),
        };
        sqlx::query(
            "INSERT INTO provider_profiles(id, name, normalized_name, kind, coding_plan, coding_plan_name, oauth_kind, template_id, revision, created_at, updated_at) VALUES(?, ?, ?, ?, 0, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(provider.id.to_string())
        .bind(provider.name.trim())
        .bind(normalize_name(&provider.name)?)
        .bind(kind)
        .bind(oauth_kind)
        .bind(provider.template_id.as_deref())
        .bind(provider.revision)
        .bind(provider.created_at.to_rfc3339())
        .bind(provider.updated_at.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .map_err(map_unique_conflict)?;
        self.insert_provider_data(&mut transaction, provider, oauth_relative_path)
            .await?;
        if let Some((cli_id, native_digest, account_identity)) = active_oauth_binding {
            upsert_active_oauth_binding_in_transaction(
                &mut transaction,
                cli_id,
                provider.id,
                native_digest,
                account_identity,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn insert_provider_data(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        provider: &ProviderProfile,
        oauth_relative_path: Option<&Path>,
    ) -> AppResult<()> {
        match &provider.data {
            ProviderData::Api(api) => {
                for connection in &api.connections {
                    self.redactor.register(&connection.api_key);
                    sqlx::query("INSERT INTO provider_connections(id, provider_id, template_endpoint_id, credential_slot_id, protocol, endpoint, auth_type, api_key, default_model, verification_status, verified_at, verification_error) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                        .bind(connection.id.to_string())
                        .bind(provider.id.to_string())
                        .bind(connection.template_endpoint_id.as_deref())
                        .bind(&connection.credential_slot_id)
                        .bind(connection.protocol.to_string())
                        .bind(connection.endpoint.as_str())
                        .bind(connection.auth_type.to_string())
                        .bind(&connection.api_key)
                        .bind(connection.default_model.trim())
                        .bind(connection.verification.status.to_string())
                        .bind(connection.verification.verified_at.map(|value| value.to_rfc3339()))
                        .bind(connection.verification.error.as_deref())
                        .execute(&mut **transaction)
                        .await
                        .map_err(map_unique_conflict)?;
                }
            }
            ProviderData::Oauth(oauth) => {
                let relative = oauth_relative_path.ok_or_else(|| {
                    AppError::Validation("OAuth relative path is required".into())
                })?;
                sqlx::query("INSERT INTO oauth_credentials(provider_id, relative_path, digest, account_id, account_label, manually_modified, verification_status, verified_at) VALUES(?, ?, ?, ?, ?, ?, ?, ?)")
                    .bind(provider.id.to_string())
                    .bind(relative.to_string_lossy().to_string())
                    .bind(&oauth.digest)
                    .bind(oauth.account_id.as_deref())
                    .bind(oauth.account_label.as_deref())
                    .bind(oauth.manually_modified)
                    .bind(oauth.verification.status.to_string())
                    .bind(oauth.verification.verified_at.map(|value| value.to_rfc3339()))
                    .execute(&mut **transaction)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn update_provider(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        oauth_relative_path: Option<&Path>,
    ) -> AppResult<()> {
        self.update_provider_transaction(provider, expected_revision, oauth_relative_path, None)
            .await
    }

    pub async fn update_provider_with_active_oauth_binding(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        oauth_relative_path: &Path,
        cli_id: CliId,
        native_digest: &str,
        account_identity: Option<&str>,
    ) -> AppResult<()> {
        validate_active_oauth_binding(provider, cli_id, native_digest)?;
        self.update_provider_transaction(
            provider,
            expected_revision,
            Some(oauth_relative_path),
            Some((cli_id, native_digest, account_identity)),
        )
        .await
    }

    async fn update_provider_transaction(
        &self,
        provider: &ProviderProfile,
        expected_revision: i64,
        oauth_relative_path: Option<&Path>,
        active_oauth_binding: Option<(CliId, &str, Option<&str>)>,
    ) -> AppResult<()> {
        provider.validate()?;
        let mut transaction = self.pool.begin().await?;
        let (kind, oauth_kind) = match &provider.data {
            ProviderData::Api(_) => ("api", None),
            ProviderData::Oauth(oauth) => ("oauth", Some(oauth.oauth_kind.to_string())),
        };
        let result = sqlx::query("UPDATE provider_profiles SET name = ?, normalized_name = ?, kind = ?, oauth_kind = ?, template_id = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND revision = ?")
            .bind(provider.name.trim())
            .bind(normalize_name(&provider.name)?)
            .bind(kind)
            .bind(oauth_kind)
            .bind(provider.template_id.as_deref())
            .bind(Utc::now().to_rfc3339())
            .bind(provider.id.to_string())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await
            .map_err(map_unique_conflict)?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict("provider was changed externally".into()));
        }
        match &provider.data {
            ProviderData::Api(api) => {
                let existing_protocols = sqlx::query(
                    "SELECT id, protocol FROM provider_connections WHERE provider_id = ?",
                )
                .bind(provider.id.to_string())
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get::<String, _>("id")?,
                        row.try_get::<String, _>("protocol")?,
                    ))
                })
                .collect::<Result<HashMap<_, _>, sqlx::Error>>()?;
                let existing = existing_protocols.keys().cloned().collect::<HashSet<_>>();
                let desired = api
                    .connections
                    .iter()
                    .map(|connection| connection.id.to_string())
                    .collect::<HashSet<_>>();

                for removed in existing.difference(&desired) {
                    let references: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM configuration_targets WHERE connection_id = ?",
                    )
                    .bind(removed)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if references > 0 {
                        return Err(AppError::Blocked(format!(
                            "connection is referenced by {references} configuration(s)"
                        )));
                    }
                    sqlx::query(
                        "DELETE FROM provider_connections WHERE provider_id = ? AND id = ?",
                    )
                    .bind(provider.id.to_string())
                    .bind(removed)
                    .execute(&mut *transaction)
                    .await?;
                }

                for connection in &api.connections {
                    let id = connection.id.to_string();
                    if existing_protocols
                        .get(&id)
                        .is_some_and(|protocol| protocol != &connection.protocol.to_string())
                    {
                        let references: i64 = sqlx::query_scalar(
                            "SELECT COUNT(*) FROM configuration_targets WHERE connection_id = ?",
                        )
                        .bind(&id)
                        .fetch_one(&mut *transaction)
                        .await?;
                        if references > 0 {
                            return Err(AppError::Blocked(format!(
                                "connection protocol cannot change while referenced by {references} configuration(s)"
                            )));
                        }
                    }
                }

                // Move retained rows out of the final protocol namespace before applying the
                // requested values. This keeps a valid protocol swap from tripping the UNIQUE
                // constraint midway through the transaction.
                for retained in existing.intersection(&desired) {
                    sqlx::query(
                        "UPDATE provider_connections SET protocol = ? WHERE provider_id = ? AND id = ?",
                    )
                    .bind(format!("__cliswitch-update-{retained}"))
                    .bind(provider.id.to_string())
                    .bind(retained)
                    .execute(&mut *transaction)
                    .await?;
                }

                for connection in &api.connections {
                    self.redactor.register(&connection.api_key);
                    let updated = sqlx::query("UPDATE provider_connections SET template_endpoint_id = ?, credential_slot_id = ?, protocol = ?, endpoint = ?, auth_type = ?, api_key = ?, default_model = ?, verification_status = ?, verified_at = ?, verification_error = ? WHERE provider_id = ? AND id = ?")
                        .bind(connection.template_endpoint_id.as_deref())
                        .bind(&connection.credential_slot_id)
                        .bind(connection.protocol.to_string())
                        .bind(connection.endpoint.as_str())
                        .bind(connection.auth_type.to_string())
                        .bind(&connection.api_key)
                        .bind(connection.default_model.trim())
                        .bind(connection.verification.status.to_string())
                        .bind(connection.verification.verified_at.map(|value| value.to_rfc3339()))
                        .bind(connection.verification.error.as_deref())
                        .bind(provider.id.to_string())
                        .bind(connection.id.to_string())
                        .execute(&mut *transaction)
                        .await
                        .map_err(map_unique_conflict)?;
                    if updated.rows_affected() == 0 {
                        sqlx::query("INSERT INTO provider_connections(id, provider_id, template_endpoint_id, credential_slot_id, protocol, endpoint, auth_type, api_key, default_model, verification_status, verified_at, verification_error) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                            .bind(connection.id.to_string())
                            .bind(provider.id.to_string())
                            .bind(connection.template_endpoint_id.as_deref())
                            .bind(&connection.credential_slot_id)
                            .bind(connection.protocol.to_string())
                            .bind(connection.endpoint.as_str())
                            .bind(connection.auth_type.to_string())
                            .bind(&connection.api_key)
                            .bind(connection.default_model.trim())
                            .bind(connection.verification.status.to_string())
                            .bind(connection.verification.verified_at.map(|value| value.to_rfc3339()))
                            .bind(connection.verification.error.as_deref())
                            .execute(&mut *transaction)
                            .await
                            .map_err(map_unique_conflict)?;
                    }
                }
            }
            ProviderData::Oauth(oauth) => {
                let relative = oauth_relative_path.ok_or_else(|| {
                    AppError::Validation("OAuth relative path is required".into())
                })?;
                let updated = sqlx::query("UPDATE oauth_credentials SET relative_path = ?, digest = ?, account_id = ?, account_label = ?, manually_modified = ?, verification_status = ?, verified_at = ? WHERE provider_id = ?")
                    .bind(relative.to_string_lossy().to_string())
                    .bind(&oauth.digest)
                    .bind(oauth.account_id.as_deref())
                    .bind(oauth.account_label.as_deref())
                    .bind(oauth.manually_modified)
                    .bind(oauth.verification.status.to_string())
                    .bind(oauth.verification.verified_at.map(|value| value.to_rfc3339()))
                    .bind(provider.id.to_string())
                    .execute(&mut *transaction)
                    .await?;
                if updated.rows_affected() == 0 {
                    sqlx::query("INSERT INTO oauth_credentials(provider_id, relative_path, digest, account_id, account_label, manually_modified, verification_status, verified_at) VALUES(?, ?, ?, ?, ?, ?, ?, ?)")
                        .bind(provider.id.to_string())
                        .bind(relative.to_string_lossy().to_string())
                        .bind(&oauth.digest)
                        .bind(oauth.account_id.as_deref())
                        .bind(oauth.account_label.as_deref())
                        .bind(oauth.manually_modified)
                        .bind(oauth.verification.status.to_string())
                        .bind(oauth.verification.verified_at.map(|value| value.to_rfc3339()))
                        .execute(&mut *transaction)
                        .await?;
                }
            }
        }
        if let Some((cli_id, native_digest, account_identity)) = active_oauth_binding {
            upsert_active_oauth_binding_in_transaction(
                &mut transaction,
                cli_id,
                provider.id,
                native_digest,
                account_identity,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_provider(&self, id: Uuid, expected_revision: i64) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let references: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM configuration_targets WHERE provider_id = ?")
                .bind(id.to_string())
                .fetch_one(&mut *transaction)
                .await?;
        if references > 0 {
            return Err(AppError::Blocked(format!(
                "provider is referenced by {references} configuration(s)"
            )));
        }
        let result = sqlx::query("DELETE FROM provider_profiles WHERE id = ? AND revision = ?")
            .bind(id.to_string())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict("provider is missing or changed".into()));
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn set_connection_verification(
        &self,
        provider_id: Uuid,
        connection_id: Uuid,
        expected_revision: i64,
        verification: &VerificationInfo,
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let provider = sqlx::query("UPDATE provider_profiles SET revision = revision + 1, updated_at = ? WHERE id = ? AND revision = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(provider_id.to_string())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await?;
        if provider.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "provider changed while the connection was being verified".into(),
            ));
        }
        let result = sqlx::query("UPDATE provider_connections SET verification_status = ?, verified_at = ?, verification_error = ? WHERE provider_id = ? AND id = ?")
            .bind(verification.status.to_string())
            .bind(verification.verified_at.map(|value| value.to_rfc3339()))
            .bind(verification.error.as_deref())
            .bind(provider_id.to_string())
            .bind(connection_id.to_string())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::NotFound(format!(
                "connection {connection_id} on provider {provider_id}"
            )));
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn list_configurations(&self) -> AppResult<Vec<SavedConfiguration>> {
        let rows = sqlx::query("SELECT id FROM saved_configurations ORDER BY creation_order")
            .fetch_all(&self.pool)
            .await?;
        let mut configurations = Vec::with_capacity(rows.len());
        for row in rows {
            configurations.push(
                self.get_configuration(parse_uuid(row.try_get::<String, _>("id")?)?)
                    .await?,
            );
        }
        Ok(configurations)
    }

    pub async fn get_configuration(&self, id: Uuid) -> AppResult<SavedConfiguration> {
        let row = sqlx::query("SELECT * FROM saved_configurations WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("configuration {id}")))?;
        let targets = sqlx::query(
            "SELECT * FROM configuration_targets WHERE configuration_id = ? ORDER BY cli_id",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(target_from_row)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(SavedConfiguration {
            id,
            name: row.try_get("name")?,
            creation_order: row.try_get("creation_order")?,
            revision: row.try_get("revision")?,
            targets,
            last_applied_at: optional_datetime(&row, "last_applied_at")?,
            last_apply_summary: row.try_get("last_apply_summary")?,
            created_at: required_datetime(&row, "created_at")?,
            updated_at: required_datetime(&row, "updated_at")?,
        })
    }

    pub async fn insert_configuration(&self, configuration: &SavedConfiguration) -> AppResult<i64> {
        configuration.validate()?;
        let mut transaction = self.pool.begin().await?;
        validate_targets(&mut transaction, &configuration.targets).await?;
        let creation_order: i64 = sqlx::query_scalar("INSERT INTO saved_configurations(id, name, normalized_name, creation_order, revision, last_applied_at, last_apply_summary, created_at, updated_at) VALUES(?, ?, ?, (SELECT COALESCE(MAX(creation_order), 0) + 1 FROM saved_configurations), ?, ?, ?, ?, ?) RETURNING creation_order")
            .bind(configuration.id.to_string())
            .bind(configuration.name.trim())
            .bind(normalize_name(&configuration.name)?)
            .bind(configuration.revision)
            .bind(configuration.last_applied_at.map(|value| value.to_rfc3339()))
            .bind(configuration.last_apply_summary.as_deref())
            .bind(configuration.created_at.to_rfc3339())
            .bind(configuration.updated_at.to_rfc3339())
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_unique_conflict)?;
        insert_targets(&mut transaction, configuration.id, &configuration.targets).await?;
        transaction.commit().await?;
        Ok(creation_order)
    }

    pub async fn update_configuration(
        &self,
        configuration: &SavedConfiguration,
        expected_revision: i64,
    ) -> AppResult<()> {
        configuration.validate()?;
        let mut transaction = self.pool.begin().await?;
        validate_targets(&mut transaction, &configuration.targets).await?;
        let result = sqlx::query("UPDATE saved_configurations SET name = ?, normalized_name = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND revision = ?")
            .bind(configuration.name.trim())
            .bind(normalize_name(&configuration.name)?)
            .bind(Utc::now().to_rfc3339())
            .bind(configuration.id.to_string())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await
            .map_err(map_unique_conflict)?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "configuration was changed externally".into(),
            ));
        }
        sqlx::query("DELETE FROM configuration_targets WHERE configuration_id = ?")
            .bind(configuration.id.to_string())
            .execute(&mut *transaction)
            .await?;
        insert_targets(&mut transaction, configuration.id, &configuration.targets).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn rename_configuration(
        &self,
        id: Uuid,
        name: &str,
        expected_revision: i64,
    ) -> AppResult<()> {
        let result = sqlx::query("UPDATE saved_configurations SET name = ?, normalized_name = ?, revision = revision + 1, updated_at = ? WHERE id = ? AND revision = ?")
            .bind(name.trim())
            .bind(normalize_name(name)?)
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .bind(expected_revision)
            .execute(&self.pool)
            .await
            .map_err(map_unique_conflict)?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "configuration is missing or changed".into(),
            ));
        }
        Ok(())
    }

    pub async fn delete_configuration(&self, id: Uuid, expected_revision: i64) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM saved_configurations WHERE id = ? AND revision = ?")
            .bind(id.to_string())
            .bind(expected_revision)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "configuration is missing or changed".into(),
            ));
        }
        Ok(())
    }

    pub async fn record_apply_result(
        &self,
        configuration_id: Uuid,
        run_id: Uuid,
        status: &str,
        summary_json: &str,
        fully_successful: bool,
    ) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO latest_apply_runs(configuration_id, run_id, status, summary_json, updated_at) VALUES(?, ?, ?, ?, ?) ON CONFLICT(configuration_id) DO UPDATE SET run_id = excluded.run_id, status = excluded.status, summary_json = excluded.summary_json, updated_at = excluded.updated_at")
            .bind(configuration_id.to_string())
            .bind(run_id.to_string())
            .bind(status)
            .bind(summary_json)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE saved_configurations SET last_applied_at = CASE WHEN ? THEN ? ELSE last_applied_at END, last_apply_summary = ?, updated_at = ? WHERE id = ?")
            .bind(fully_successful)
            .bind(&now)
            .bind(summary_json)
            .bind(&now)
            .bind(configuration_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn upsert_active_oauth_binding(
        &self,
        cli_id: CliId,
        provider_id: Uuid,
        native_digest: &str,
        account_identity: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query("INSERT INTO active_oauth_bindings(cli_id, provider_id, native_digest, account_identity, updated_at) VALUES(?, ?, ?, ?, ?) ON CONFLICT(cli_id) DO UPDATE SET provider_id = excluded.provider_id, native_digest = excluded.native_digest, account_identity = excluded.account_identity, updated_at = excluded.updated_at")
            .bind(cli_id.to_string())
            .bind(provider_id.to_string())
            .bind(native_digest)
            .bind(account_identity)
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_oauth_binding(
        &self,
        cli_id: CliId,
    ) -> AppResult<Option<ActiveOAuthBinding>> {
        let row = sqlx::query("SELECT * FROM active_oauth_bindings WHERE cli_id = ?")
            .bind(cli_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(active_oauth_binding_from_row).transpose()
    }

    pub async fn list_active_oauth_bindings(&self) -> AppResult<Vec<ActiveOAuthBinding>> {
        sqlx::query("SELECT * FROM active_oauth_bindings ORDER BY cli_id")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(active_oauth_binding_from_row)
            .collect()
    }

    pub async fn get_settings(&self) -> AppResult<AppSettings> {
        let row = sqlx::query("SELECT * FROM app_settings WHERE singleton = 1")
            .fetch_one(&self.pool)
            .await?;
        let locations = sqlx::query("SELECT * FROM manual_cli_locations ORDER BY cli_id")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| {
                Ok(ManualCliLocation {
                    cli_id: CliId::from_str(&row.try_get::<String, _>("cli_id")?)?,
                    executable_path: row
                        .try_get::<Option<String>, _>("executable_path")?
                        .map(Into::into),
                    config_directory: row
                        .try_get::<Option<String>, _>("config_directory")?
                        .map(Into::into),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let mut settings = AppSettings {
            language: AppLanguage::from_str(&row.try_get::<String, _>("language")?)?,
            theme: AppTheme::from_str(&row.try_get::<String, _>("theme")?)?,
            scan_on_startup: row.try_get::<i64, _>("scan_on_startup")? != 0,
            plaintext_risk_accepted: row.try_get::<i64, _>("plaintext_risk_accepted")? != 0,
            revision: row.try_get("revision")?,
            manual_locations: locations,
        };
        for cli_id in CliId::ALL {
            if !settings
                .manual_locations
                .iter()
                .any(|item| item.cli_id == cli_id)
            {
                settings.manual_locations.push(ManualCliLocation {
                    cli_id,
                    executable_path: None,
                    config_directory: None,
                });
            }
        }
        Ok(settings)
    }

    pub async fn update_settings(
        &self,
        settings: &AppSettings,
        expected_revision: i64,
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query("UPDATE app_settings SET language = ?, theme = ?, scan_on_startup = ?, plaintext_risk_accepted = ?, revision = revision + 1, updated_at = ? WHERE singleton = 1 AND revision = ?")
            .bind(settings.language.to_string())
            .bind(settings.theme.to_string())
            .bind(settings.scan_on_startup)
            .bind(settings.plaintext_risk_accepted)
            .bind(Utc::now().to_rfc3339())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "settings were changed externally".into(),
            ));
        }
        for location in &settings.manual_locations {
            sqlx::query("INSERT INTO manual_cli_locations(cli_id, executable_path, config_directory, updated_at) VALUES(?, ?, ?, ?) ON CONFLICT(cli_id) DO UPDATE SET executable_path = excluded.executable_path, config_directory = excluded.config_directory, updated_at = excluded.updated_at")
                .bind(location.cli_id.to_string())
                .bind(location.executable_path.as_ref().map(|path| path.to_string_lossy().to_string()))
                .bind(location.config_directory.as_ref().map(|path| path.to_string_lossy().to_string()))
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

fn validate_active_oauth_binding(
    provider: &ProviderProfile,
    cli_id: CliId,
    native_digest: &str,
) -> AppResult<()> {
    let catalog = crate::catalog::runtime_catalog()?;
    match &provider.data {
        ProviderData::Oauth(oauth)
            if provider
                .template_id
                .as_deref()
                .is_some_and(|template_id| catalog.supports_auth_template(cli_id, template_id))
                && oauth.digest.as_str() == native_digest =>
        {
            Ok(())
        }
        ProviderData::Oauth(_) => Err(AppError::Validation(
            "active OAuth binding does not match the provider or native digest".into(),
        )),
        ProviderData::Api(_) => Err(AppError::Validation(
            "active OAuth binding requires an OAuth provider".into(),
        )),
    }
}

async fn upsert_active_oauth_binding_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    cli_id: CliId,
    provider_id: Uuid,
    native_digest: &str,
    account_identity: Option<&str>,
) -> AppResult<()> {
    sqlx::query("INSERT INTO active_oauth_bindings(cli_id, provider_id, native_digest, account_identity, updated_at) VALUES(?, ?, ?, ?, ?) ON CONFLICT(cli_id) DO UPDATE SET provider_id = excluded.provider_id, native_digest = excluded.native_digest, account_identity = excluded.account_identity, updated_at = excluded.updated_at")
        .bind(cli_id.to_string())
        .bind(provider_id.to_string())
        .bind(native_digest)
        .bind(account_identity)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn dynamic_target_supported(
    catalog: &crate::catalog::ProviderCatalog,
    cli_id: CliId,
    template_id: &str,
    endpoint_id: Option<&str>,
    protocol: CliProtocol,
) -> bool {
    match endpoint_id.filter(|id| !id.is_empty()) {
        Some(endpoint_id) => catalog.supports_api_endpoint(cli_id, template_id, endpoint_id),
        None => catalog.supports_protocol(cli_id, protocol),
    }
}

async fn validate_targets(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    targets: &[ConfigurationTarget],
) -> AppResult<()> {
    let catalog = crate::catalog::runtime_catalog()?;
    let legacy_catalog = crate::catalog::legacy_catalog()?;
    for target in targets {
        match target {
            ConfigurationTarget::Api {
                cli_id,
                provider_id,
                connection_id,
                model,
            } => {
                let row = sqlx::query(
                    "SELECT p.kind AS provider_kind, p.template_id AS template_id, c.protocol AS protocol, c.template_endpoint_id AS template_endpoint_id FROM provider_profiles p LEFT JOIN provider_connections c ON c.provider_id = p.id AND c.id = ? WHERE p.id = ?",
                )
                .bind(connection_id.to_string())
                .bind(provider_id.to_string())
                .fetch_optional(&mut **transaction)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("provider {provider_id}")))?;
                if row.try_get::<String, _>("provider_kind")? != "api" {
                    return Err(AppError::Validation(
                        "API target must reference an API provider".into(),
                    ));
                }
                let protocol = row
                    .try_get::<Option<String>, _>("protocol")?
                    .ok_or_else(|| AppError::Validation("connection does not exist".into()))?;
                let protocol = CliProtocol::from_str(&protocol)?;
                let template_id: Option<String> = row.try_get("template_id")?;
                let template_endpoint_id: Option<String> = row.try_get("template_endpoint_id")?;
                let supported = match (template_id.as_deref(), template_endpoint_id.as_deref()) {
                    (Some(template_id), Some(endpoint_id)) => {
                        if catalog.dynamic_provider_info(template_id).is_some() {
                            dynamic_target_supported(
                                &catalog,
                                *cli_id,
                                template_id,
                                Some(endpoint_id),
                                protocol,
                            )
                        } else if catalog.api_template(template_id).is_some() {
                            catalog.supports_api_endpoint(*cli_id, template_id, endpoint_id)
                        } else if legacy_catalog.api_template(template_id).is_some() {
                            legacy_catalog.supports_api_endpoint(*cli_id, template_id, endpoint_id)
                        } else {
                            false
                        }
                    }
                    (Some(template_id), None) => {
                        if catalog.dynamic_provider_info(template_id).is_some() {
                            dynamic_target_supported(&catalog, *cli_id, template_id, None, protocol)
                        } else {
                            // A custom provider may retain a source/template label which is not
                            // in the current catalog. Its resolved connection is still checked
                            // against the fixed CLI protocol contract.
                            catalog.api_template(template_id).is_none()
                                && legacy_catalog.api_template(template_id).is_none()
                                && catalog.supports_protocol(*cli_id, protocol)
                        }
                    }
                    (None, None) => catalog.supports_protocol(*cli_id, protocol),
                    _ => false,
                };
                if !supported {
                    return Err(AppError::Validation(format!(
                        "{cli_id} does not support this provider endpoint"
                    )));
                }
                if let (Some(template_id), Some(endpoint_id)) =
                    (template_id.as_deref(), template_endpoint_id.as_deref())
                    && (catalog
                        .api_template(template_id)
                        .or_else(|| legacy_catalog.api_template(template_id)))
                    .is_some_and(|template| template.model_routing)
                {
                    let model = model.trim();
                    let routed_endpoint = catalog
                        .model_routed_endpoint(template_id, model)
                        .or_else(|| legacy_catalog.model_routed_endpoint(template_id, model))
                        .ok_or_else(|| {
                            AppError::Validation(format!(
                                "model {model} has no route in provider template {template_id}"
                            ))
                        })?;
                    if routed_endpoint.id != endpoint_id {
                        return Err(AppError::Validation(format!(
                            "model {model} routes to endpoint {}, not {endpoint_id}",
                            routed_endpoint.id
                        )));
                    }
                }
            }
            ConfigurationTarget::Oauth {
                cli_id,
                provider_id,
                ..
            } => {
                let row = sqlx::query(
                    "SELECT kind, oauth_kind, template_id FROM provider_profiles WHERE id = ?",
                )
                .bind(provider_id.to_string())
                .fetch_optional(&mut **transaction)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("provider {provider_id}")))?;
                let kind: String = row.try_get("kind")?;
                let oauth_kind = row.try_get::<Option<String>, _>("oauth_kind")?;
                let template_id = row.try_get::<Option<String>, _>("template_id")?;
                let supports_auth_template = template_id.as_deref().is_some_and(|template_id| {
                    catalog.supports_auth_template(*cli_id, template_id)
                        || legacy_catalog.supports_auth_template(*cli_id, template_id)
                });
                let oauth_kind = oauth_kind.as_deref().map(OAuthKind::from_str).transpose()?;
                let template_matches_kind = template_id
                    .as_deref()
                    .and_then(|template_id| {
                        catalog
                            .auth_template(template_id)
                            .or_else(|| legacy_catalog.auth_template(template_id))
                    })
                    .is_some_and(|template| Some(template.auth_kind) == oauth_kind);
                if kind != "oauth" || !template_matches_kind || !supports_auth_template {
                    return Err(AppError::Validation(
                        "OAuth target must reference a matching OAuth provider".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

async fn insert_targets(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    configuration_id: Uuid,
    targets: &[ConfigurationTarget],
) -> AppResult<()> {
    for target in targets {
        let (kind, cli_id, provider_id, connection_id, model) = match target {
            ConfigurationTarget::Api {
                cli_id,
                provider_id,
                connection_id,
                model,
            } => ("api", *cli_id, *provider_id, Some(*connection_id), model),
            ConfigurationTarget::Oauth {
                cli_id,
                provider_id,
                model,
            } => ("oauth", *cli_id, *provider_id, None, model),
        };
        sqlx::query("INSERT INTO configuration_targets(configuration_id, cli_id, target_kind, provider_id, connection_id, model) VALUES(?, ?, ?, ?, ?, ?)")
            .bind(configuration_id.to_string())
            .bind(cli_id.to_string())
            .bind(kind)
            .bind(provider_id.to_string())
            .bind(connection_id.map(|value| value.to_string()))
            .bind(model.trim())
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

fn target_from_row(row: SqliteRow) -> AppResult<ConfigurationTarget> {
    let cli_id = CliId::from_str(&row.try_get::<String, _>("cli_id")?)?;
    let provider_id = parse_uuid(row.try_get::<String, _>("provider_id")?)?;
    let model = row.try_get("model")?;
    match row.try_get::<String, _>("target_kind")?.as_str() {
        "api" => Ok(ConfigurationTarget::Api {
            cli_id,
            provider_id,
            connection_id: parse_uuid(
                row.try_get::<Option<String>, _>("connection_id")?
                    .ok_or_else(|| AppError::Serialization("missing connection id".into()))?,
            )?,
            model,
        }),
        "oauth" => Ok(ConfigurationTarget::Oauth {
            cli_id,
            provider_id,
            model,
        }),
        _ => Err(AppError::Serialization("invalid target kind".into())),
    }
}

fn active_oauth_binding_from_row(row: SqliteRow) -> AppResult<ActiveOAuthBinding> {
    Ok(ActiveOAuthBinding {
        cli_id: CliId::from_str(&row.try_get::<String, _>("cli_id")?)?,
        provider_id: parse_uuid(row.try_get::<String, _>("provider_id")?)?,
        native_digest: row.try_get("native_digest")?,
        account_identity: row.try_get("account_identity")?,
        updated_at: required_datetime(&row, "updated_at")?,
    })
}

fn parse_uuid(value: String) -> AppResult<Uuid> {
    Uuid::parse_str(&value).map_err(|error| AppError::Serialization(error.to_string()))
}

fn required_datetime(row: &SqliteRow, column: &str) -> AppResult<DateTime<Utc>> {
    parse_datetime(row.try_get::<String, _>(column)?)
}

fn optional_datetime(row: &SqliteRow, column: &str) -> AppResult<Option<DateTime<Utc>>> {
    row.try_get::<Option<String>, _>(column)?
        .map(parse_datetime)
        .transpose()
}

fn parse_datetime(value: String) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| AppError::Serialization(error.to_string()))
}

fn map_unique_conflict(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database) = &error
        && database.is_unique_violation()
    {
        return AppError::Conflict("name or protocol must be unique".into());
    }
    AppError::Database(error)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::filesystem::private_paths::PrivatePaths;

    async fn repository() -> (TempDir, PrivatePaths, Repository) {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        (temp, paths, repository)
    }

    #[tokio::test]
    async fn template_migration_preserves_legacy_providers_and_targets_as_custom_data() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let provider_id = Uuid::new_v4().to_string();
        let connection_id = Uuid::new_v4().to_string();
        let configuration_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO provider_profiles(id, name, normalized_name, kind, coding_plan, coding_plan_name, revision, created_at, updated_at) VALUES(?, 'Legacy', 'legacy', 'api', 1, 'Legacy plan label', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .bind(&provider_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO provider_connections(id, provider_id, protocol, endpoint, auth_type, api_key, default_model) VALUES(?, ?, 'openai-chat', 'https://legacy.invalid/custom/path', 'bearer', 'legacy-secret', 'legacy-model')")
            .bind(&connection_id)
            .bind(&provider_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO saved_configurations(id, name, normalized_name, creation_order, revision, created_at, updated_at) VALUES(?, 'Legacy config', 'legacy config', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .bind(&configuration_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO configuration_targets(configuration_id, cli_id, target_kind, provider_id, connection_id, model) VALUES(?, 'opencode', 'api', ?, ?, 'legacy-model')")
            .bind(&configuration_id)
            .bind(&provider_id)
            .bind(&connection_id)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::raw_sql(include_str!("../../migrations/0002_provider_templates.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let row = sqlx::query("SELECT p.template_id, p.coding_plan, p.coding_plan_name, c.endpoint, c.api_key, c.template_endpoint_id, c.credential_slot_id FROM provider_profiles p JOIN provider_connections c ON c.provider_id = p.id WHERE p.id = ?")
            .bind(&provider_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<Option<String>, _>("template_id"), None);
        assert_eq!(row.get::<i64, _>("coding_plan"), 1);
        assert_eq!(
            row.get::<Option<String>, _>("coding_plan_name").as_deref(),
            Some("Legacy plan label")
        );
        assert_eq!(
            row.get::<String, _>("endpoint"),
            "https://legacy.invalid/custom/path"
        );
        assert_eq!(row.get::<String, _>("api_key"), "legacy-secret");
        assert_eq!(row.get::<Option<String>, _>("template_endpoint_id"), None);
        assert_eq!(
            row.get::<String, _>("credential_slot_id"),
            format!("legacy-{connection_id}")
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT connection_id FROM configuration_targets WHERE configuration_id = ?",
            )
            .bind(&configuration_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            connection_id
        );

        sqlx::query("INSERT INTO provider_connections(id, provider_id, protocol, endpoint, auth_type, api_key, default_model, credential_slot_id) VALUES(?, ?, 'openai-chat', 'https://second.invalid/v1', 'bearer', 'second-secret', 'second-model', 'second-key')")
            .bind(Uuid::new_v4().to_string())
            .bind(&provider_id)
            .execute(&pool)
            .await
            .expect("endpoint identity must allow two connections using one protocol");
    }

    #[tokio::test]
    async fn minimax_migration_reclassifies_saved_profiles_without_changing_connection_ids() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0002_provider_templates.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let token_provider_id = Uuid::new_v4().to_string();
        let token_connection_id = Uuid::new_v4().to_string();
        let api_provider_id = Uuid::new_v4().to_string();
        let api_connection_id = Uuid::new_v4().to_string();
        for (provider_id, connection_id, name, normalized_name, api_key) in [
            (
                &token_provider_id,
                &token_connection_id,
                "MiniMax token",
                "minimax token",
                "sk-cp-fixture",
            ),
            (
                &api_provider_id,
                &api_connection_id,
                "MiniMax API",
                "minimax api",
                "sk-api-fixture",
            ),
        ] {
            sqlx::query("INSERT INTO provider_profiles(id, name, normalized_name, kind, coding_plan, template_id, revision, created_at, updated_at) VALUES(?, ?, ?, 'api', 0, 'minimax-coding-plan', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
                .bind(provider_id)
                .bind(name)
                .bind(normalized_name)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO provider_connections(id, provider_id, protocol, endpoint, auth_type, api_key, default_model, verification_status, verified_at, template_endpoint_id, credential_slot_id) VALUES(?, ?, 'anthropic-messages', 'https://api.minimax.io/anthropic/v1', 'api-key', ?, 'MiniMax-M2.7', 'valid', '2026-01-01T00:00:00Z', 'anthropic', 'api-key')")
                .bind(connection_id)
                .bind(provider_id)
                .bind(api_key)
                .execute(&pool)
                .await
                .unwrap();
        }

        sqlx::raw_sql(include_str!(
            "../../migrations/0003_minimax_credential_kinds.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let token = sqlx::query("SELECT p.template_id, p.revision, c.id, c.auth_type, c.verification_status, c.verified_at FROM provider_profiles p JOIN provider_connections c ON c.provider_id = p.id WHERE p.id = ?")
            .bind(&token_provider_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(token.get::<String, _>("template_id"), "minimax-coding-plan");
        assert_eq!(token.get::<i64, _>("revision"), 2);
        assert_eq!(token.get::<String, _>("id"), token_connection_id);
        assert_eq!(token.get::<String, _>("auth_type"), "bearer");
        assert_eq!(
            token.get::<String, _>("verification_status"),
            "user-modified-unverified"
        );
        assert_eq!(token.get::<Option<String>, _>("verified_at"), None);

        let api = sqlx::query("SELECT p.template_id, p.revision, c.id, c.auth_type FROM provider_profiles p JOIN provider_connections c ON c.provider_id = p.id WHERE p.id = ?")
            .bind(&api_provider_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(api.get::<String, _>("template_id"), "minimax-api");
        assert_eq!(api.get::<i64, _>("revision"), 2);
        assert_eq!(api.get::<String, _>("id"), api_connection_id);
        assert_eq!(api.get::<String, _>("auth_type"), "api-key");
    }

    #[tokio::test]
    async fn destructive_provider_migration_removes_api_profiles_and_mixed_configurations() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0002_provider_templates.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let api_provider_id = Uuid::new_v4().to_string();
        let api_connection_id = Uuid::new_v4().to_string();
        let oauth_provider_id = Uuid::new_v4().to_string();
        let api_configuration_id = Uuid::new_v4().to_string();
        let oauth_configuration_id = Uuid::new_v4().to_string();
        for (id, name, normalized_name, kind, oauth_kind) in [
            (&api_provider_id, "Legacy API", "legacy api", "api", None),
            (
                &oauth_provider_id,
                "OAuth account",
                "oauth account",
                "oauth",
                Some("codex"),
            ),
        ] {
            sqlx::query("INSERT INTO provider_profiles(id, name, normalized_name, kind, coding_plan, oauth_kind, revision, created_at, updated_at) VALUES(?, ?, ?, ?, 0, ?, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
                .bind(id)
                .bind(name)
                .bind(normalized_name)
                .bind(kind)
                .bind(oauth_kind)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO provider_connections(id, provider_id, protocol, endpoint, auth_type, api_key, default_model, credential_slot_id) VALUES(?, ?, 'openai-chat', 'https://legacy.invalid/v1', 'bearer', 'fixture-api-secret', 'legacy-model', 'api-key')")
            .bind(&api_connection_id)
            .bind(&api_provider_id)
            .execute(&pool)
            .await
            .unwrap();
        for (id, name, normalized_name, order) in [
            (
                &api_configuration_id,
                "API configuration",
                "api configuration",
                1_i64,
            ),
            (
                &oauth_configuration_id,
                "OAuth configuration",
                "oauth configuration",
                2_i64,
            ),
        ] {
            sqlx::query("INSERT INTO saved_configurations(id, name, normalized_name, creation_order, revision, created_at, updated_at) VALUES(?, ?, ?, ?, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
                .bind(id)
                .bind(name)
                .bind(normalized_name)
                .bind(order)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO configuration_targets(configuration_id, cli_id, target_kind, provider_id, connection_id, model) VALUES(?, 'opencode', 'api', ?, ?, 'legacy-model')")
            .bind(&api_configuration_id)
            .bind(&api_provider_id)
            .bind(&api_connection_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO configuration_targets(configuration_id, cli_id, target_kind, provider_id, connection_id, model) VALUES(?, 'codex', 'oauth', ?, NULL, 'gpt-5.5')")
            .bind(&oauth_configuration_id)
            .bind(&oauth_provider_id)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::raw_sql(include_str!(
            "../../migrations/0004_reset_legacy_api_providers.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM provider_profiles WHERE kind = 'api'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM provider_profiles WHERE kind = 'oauth'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM saved_configurations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM configuration_targets")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    fn api_provider(name: &str) -> ProviderProfile {
        let now = Utc::now();
        ProviderProfile {
            id: Uuid::new_v4(),
            name: name.into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: Uuid::new_v4(),
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiResponses,
                    endpoint: Url::parse("https://example.test/v1").unwrap(),
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: "private-value".into(),
                    default_model: "model-a".into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        }
    }

    fn templated_provider(name: &str, template_id: &str) -> ProviderProfile {
        let now = Utc::now();
        let template = crate::catalog::legacy_catalog()
            .unwrap()
            .api_template(template_id)
            .unwrap();
        ProviderProfile {
            id: Uuid::new_v4(),
            name: name.into(),
            template_id: Some(template.id.clone()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: template
                    .endpoints
                    .iter()
                    .map(|endpoint| ProviderConnection {
                        id: Uuid::new_v4(),
                        template_endpoint_id: Some(endpoint.id.clone()),
                        credential_slot_id: endpoint.credential_slot_id.clone(),
                        protocol: endpoint.protocol,
                        endpoint: endpoint.base_url.clone(),
                        auth_type: endpoint.default_auth_type().unwrap(),
                        api_key: "shared-fixture-secret".into(),
                        default_model: endpoint.default_model().unwrap().into(),
                        verification: VerificationInfo::default(),
                    })
                    .collect(),
            }),
        }
    }

    fn configuration(name: &str, targets: Vec<ConfigurationTarget>) -> SavedConfiguration {
        let now = Utc::now();
        SavedConfiguration {
            id: Uuid::new_v4(),
            name: name.into(),
            creation_order: 0,
            revision: 1,
            targets,
            last_applied_at: None,
            last_apply_summary: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn active_oauth_binding_rejects_non_oauth_or_mismatched_provider_data() {
        let (_temp, _paths, repository) = repository().await;
        let api = api_provider("API provider");
        assert!(matches!(
            repository
                .insert_provider_with_active_oauth_binding(
                    &api,
                    Path::new("unused"),
                    CliId::Codex,
                    "sha256:fixture",
                    None,
                )
                .await,
            Err(AppError::Validation(_))
        ));

        let now = Utc::now();
        let oauth = ProviderProfile {
            id: Uuid::new_v4(),
            name: "Codex OAuth".into(),
            template_id: Some("codex-auth".into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Codex,
                account_id: Some("account-a".into()),
                account_label: None,
                raw_content: "fixture".into(),
                digest: "sha256:fixture".into(),
                manually_modified: false,
                verification: VerificationInfo::default(),
            }),
        };
        for (cli_id, digest) in [
            (CliId::ClaudeCode, "sha256:fixture"),
            (CliId::Codex, "sha256:different"),
        ] {
            assert!(matches!(
                repository
                    .insert_provider_with_active_oauth_binding(
                        &oauth,
                        Path::new("unused"),
                        cli_id,
                        digest,
                        Some("account-a"),
                    )
                    .await,
                Err(AppError::Validation(_))
            ));
        }
        assert!(repository.list_providers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn provider_crud_and_public_separation() {
        let (_temp, _paths, repository) = repository().await;
        let provider = api_provider("Example");
        repository.insert_provider(&provider, None).await.unwrap();
        let list = repository.list_providers().await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            !serde_json::to_string(&list)
                .unwrap()
                .contains("private-value")
        );
        assert_eq!(
            repository.get_provider(provider.id).await.unwrap().name,
            "Example"
        );
        repository.delete_provider(provider.id, 1).await.unwrap();
        assert!(repository.list_providers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn names_are_unique_without_case() {
        let (_temp, _paths, repository) = repository().await;
        repository
            .insert_provider(&api_provider("Example"), None)
            .await
            .unwrap();
        let error = repository
            .insert_provider(&api_provider(" example "), None)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)));
    }

    #[tokio::test]
    async fn api_targets_follow_catalog_endpoint_relations_and_reject_mixed_identity() {
        let (_temp, _paths, repository) = repository().await;
        let provider = templated_provider("GLM Coding Plan", "glm-coding-plan");
        let ProviderData::Api(api) = &provider.data else {
            unreachable!()
        };
        let anthropic_connection = api
            .connections
            .iter()
            .find(|connection| connection.template_endpoint_id.as_deref() == Some("anthropic"))
            .unwrap()
            .id;
        let chat_connection = api
            .connections
            .iter()
            .find(|connection| connection.template_endpoint_id.as_deref() == Some("openai-chat"))
            .unwrap()
            .id;
        repository.insert_provider(&provider, None).await.unwrap();

        repository
            .insert_configuration(&configuration(
                "Claude via GLM",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::ClaudeCode,
                    provider_id: provider.id,
                    connection_id: anthropic_connection,
                    model: "glm-4.7".into(),
                }],
            ))
            .await
            .unwrap();

        let wrong_relation = repository
            .insert_configuration(&configuration(
                "Claude via wrong GLM endpoint",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::ClaudeCode,
                    provider_id: provider.id,
                    connection_id: chat_connection,
                    model: "glm-4.7".into(),
                }],
            ))
            .await
            .unwrap_err();
        assert!(matches!(wrong_relation, AppError::Validation(_)));

        sqlx::query("UPDATE provider_connections SET template_endpoint_id = NULL WHERE id = ?")
            .bind(chat_connection.to_string())
            .execute(repository.pool())
            .await
            .unwrap();
        let mixed_identity = repository
            .insert_configuration(&configuration(
                "Mixed endpoint identity",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::Opencode,
                    provider_id: provider.id,
                    connection_id: chat_connection,
                    model: "glm-4.7".into(),
                }],
            ))
            .await
            .unwrap_err();
        assert!(matches!(mixed_identity, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn model_routed_api_targets_require_the_connection_for_the_selected_model() {
        let (_temp, _paths, repository) = repository().await;
        let provider = templated_provider("OpenCode Zen", "opencode-zen");
        let ProviderData::Api(api) = &provider.data else {
            unreachable!()
        };
        let responses_connection = api
            .connections
            .iter()
            .find(|connection| connection.template_endpoint_id.as_deref() == Some("responses"))
            .unwrap()
            .id;
        repository.insert_provider(&provider, None).await.unwrap();

        repository
            .insert_configuration(&configuration(
                "OpenCode Zen Responses",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::Opencode,
                    provider_id: provider.id,
                    connection_id: responses_connection,
                    model: "gpt-5.6-sol".into(),
                }],
            ))
            .await
            .unwrap();

        let wrong_route = repository
            .insert_configuration(&configuration(
                "OpenCode Zen wrong route",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::Opencode,
                    provider_id: provider.id,
                    connection_id: responses_connection,
                    model: "glm-5".into(),
                }],
            ))
            .await
            .unwrap_err();
        assert!(
            wrong_route
                .to_string()
                .contains("routes to endpoint chat, not responses")
        );

        let unknown_model = repository
            .insert_configuration(&configuration(
                "OpenCode Zen unknown route",
                vec![ConfigurationTarget::Api {
                    cli_id: CliId::Opencode,
                    provider_id: provider.id,
                    connection_id: responses_connection,
                    model: "outside-catalog-model".into(),
                }],
            ))
            .await
            .unwrap_err();
        assert!(
            unknown_model
                .to_string()
                .contains("has no route in provider template opencode-zen")
        );
    }

    #[tokio::test]
    async fn migration_failure_preserves_the_existing_database() {
        let (temp, paths, repository) = repository().await;
        sqlx::query("CREATE TABLE preserved_user_data(value TEXT NOT NULL)")
            .execute(repository.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO preserved_user_data(value) VALUES('keep-me')")
            .execute(repository.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES(999, 'future', 1, X'00', 0)")
            .execute(repository.pool())
            .await
            .unwrap();
        repository.pool().close().await;
        drop(repository);

        assert!(matches!(
            Repository::open(&paths.database, Redactor::default()).await,
            Err(AppError::Migration(_))
        ));

        let options = SqliteConnectOptions::new()
            .filename(&paths.database)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let value: String = sqlx::query_scalar("SELECT value FROM preserved_user_data")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, "keep-me");
        pool.close().await;
        drop(temp);
    }

    #[tokio::test]
    async fn revision_conflict_is_detected() {
        let (_temp, _paths, repository) = repository().await;
        let provider = api_provider("Example");
        repository.insert_provider(&provider, None).await.unwrap();
        let mut edited = provider.clone();
        edited.name = "Changed".into();
        repository.update_provider(&edited, 1, None).await.unwrap();
        assert!(matches!(
            repository.update_provider(&edited, 1, None).await,
            Err(AppError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn connection_verification_is_persisted_without_exposing_the_key() {
        let (_temp, _paths, repository) = repository().await;
        let provider = api_provider("Example");
        let connection_id = match &provider.data {
            ProviderData::Api(api) => api.connections[0].id,
            _ => unreachable!(),
        };
        repository.insert_provider(&provider, None).await.unwrap();
        let verified_at = Utc::now();
        repository
            .set_connection_verification(
                provider.id,
                connection_id,
                provider.revision,
                &VerificationInfo {
                    status: VerificationStatus::Invalid,
                    verified_at: Some(verified_at),
                    error: Some("HTTP 401".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            repository.get_provider(provider.id).await.unwrap().revision,
            2
        );
        let public = repository.list_providers().await.unwrap();
        assert_eq!(
            public[0].connections[0].verification.status,
            VerificationStatus::Invalid
        );
        assert_eq!(
            public[0].connections[0].verification.error.as_deref(),
            Some("HTTP 401")
        );
        assert!(
            !serde_json::to_string(&public)
                .unwrap()
                .contains("private-value")
        );
        assert!(matches!(
            repository
                .set_connection_verification(
                    provider.id,
                    connection_id,
                    provider.revision,
                    &VerificationInfo::default(),
                )
                .await,
            Err(AppError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn references_are_distinct_when_one_configuration_uses_a_provider_twice() {
        let (_temp, _paths, repository) = repository().await;
        let provider = api_provider("Example");
        let connection_id = match &provider.data {
            ProviderData::Api(api) => api.connections[0].id,
            _ => unreachable!(),
        };
        repository.insert_provider(&provider, None).await.unwrap();
        repository
            .insert_configuration(&configuration(
                "Dev",
                vec![CliId::Codex, CliId::Opencode]
                    .into_iter()
                    .map(|cli_id| ConfigurationTarget::Api {
                        cli_id,
                        provider_id: provider.id,
                        connection_id,
                        model: "model-a".into(),
                    })
                    .collect(),
            ))
            .await
            .unwrap();

        let public = repository.list_providers().await.unwrap();
        assert_eq!(public[0].referenced_by, vec!["Dev"]);
    }

    #[tokio::test]
    async fn oauth_targets_and_credential_paths_are_scope_checked() {
        let (_temp, paths, repository) = repository().await;
        let now = Utc::now();
        let id = Uuid::new_v4();
        let directory = paths.auth_profile_dir(id).await.unwrap();
        tokio::fs::write(directory.join("auth.txt"), b"fixture")
            .await
            .unwrap();
        let provider = ProviderProfile {
            id,
            name: "Codex OAuth".into(),
            template_id: Some("codex-auth".into()),
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Oauth(OAuthProviderData {
                oauth_kind: OAuthKind::Codex,
                account_id: Some("account-a".into()),
                account_label: None,
                raw_content: "fixture".into(),
                digest: "sha256:fixture".into(),
                manually_modified: false,
                verification: VerificationInfo::default(),
            }),
        };
        let relative = Path::new("auth").join(id.to_string()).join("auth.txt");
        repository
            .insert_provider(&provider, Some(&relative))
            .await
            .unwrap();

        repository
            .insert_configuration(&configuration(
                "Correct OAuth CLI",
                vec![ConfigurationTarget::Oauth {
                    cli_id: CliId::Codex,
                    provider_id: id,
                    model: "gpt-fixture".into(),
                }],
            ))
            .await
            .unwrap();

        let error = repository
            .insert_configuration(&configuration(
                "Wrong CLI",
                vec![ConfigurationTarget::Oauth {
                    cli_id: CliId::ClaudeCode,
                    provider_id: id,
                    model: "claude".into(),
                }],
            ))
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Validation(_)));

        sqlx::query(
            "UPDATE oauth_credentials SET relative_path = '../outside' WHERE provider_id = ?",
        )
        .bind(id.to_string())
        .execute(repository.pool())
        .await
        .unwrap();
        assert!(matches!(
            repository.get_provider(id).await,
            Err(AppError::Validation(_) | AppError::Blocked(_))
        ));
    }

    #[tokio::test]
    async fn concurrent_configuration_creation_assigns_unique_order() {
        let (_temp, _paths, repository) = repository().await;
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
        let spawn_insert = |name: &'static str| {
            let repository = repository.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                let configuration = configuration(name, Vec::new());
                barrier.wait().await;
                repository.insert_configuration(&configuration).await
            })
        };
        let first = spawn_insert("First");
        let second = spawn_insert("Second");
        barrier.wait().await;
        let mut orders = vec![
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap(),
        ];
        orders.sort_unstable();
        assert_eq!(orders, vec![1, 2]);
    }

    #[tokio::test]
    async fn referenced_provider_cannot_be_deleted() {
        let (_temp, _paths, repository) = repository().await;
        let provider = api_provider("Example");
        let connection_id = match &provider.data {
            ProviderData::Api(api) => api.connections[0].id,
            _ => unreachable!(),
        };
        repository.insert_provider(&provider, None).await.unwrap();
        let now = Utc::now();
        let config = SavedConfiguration {
            id: Uuid::new_v4(),
            name: "Dev".into(),
            creation_order: 1,
            revision: 1,
            targets: vec![ConfigurationTarget::Api {
                cli_id: CliId::Codex,
                provider_id: provider.id,
                connection_id,
                model: "model-a".into(),
            }],
            last_applied_at: None,
            last_apply_summary: None,
            created_at: now,
            updated_at: now,
        };
        repository.insert_configuration(&config).await.unwrap();
        assert!(matches!(
            repository.delete_provider(provider.id, 1).await,
            Err(AppError::Blocked(_))
        ));
    }

    #[tokio::test]
    async fn referenced_connection_is_updated_in_place_and_cannot_be_removed() {
        let (_temp, _paths, repository) = repository().await;
        let mut provider = api_provider("Example");
        if let ProviderData::Api(api) = &mut provider.data {
            let mut secondary = api.connections[0].clone();
            secondary.id = Uuid::new_v4();
            secondary.credential_slot_id = "secondary-key".into();
            secondary.protocol = CliProtocol::AnthropicMessages;
            api.connections.push(secondary);
        }
        let connection_id = match &provider.data {
            ProviderData::Api(api) => api.connections[0].id,
            _ => unreachable!(),
        };
        repository.insert_provider(&provider, None).await.unwrap();
        let now = Utc::now();
        repository
            .insert_configuration(&SavedConfiguration {
                id: Uuid::new_v4(),
                name: "Dev".into(),
                creation_order: 1,
                revision: 1,
                targets: vec![ConfigurationTarget::Api {
                    cli_id: CliId::Codex,
                    provider_id: provider.id,
                    connection_id,
                    model: "model-a".into(),
                }],
                last_applied_at: None,
                last_apply_summary: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();

        let mut edited = provider.clone();
        if let ProviderData::Api(api) = &mut edited.data {
            api.connections[0].api_key = "replacement-key".into();
        }
        repository.update_provider(&edited, 1, None).await.unwrap();
        let reloaded = repository.get_provider(provider.id).await.unwrap();
        let ProviderData::Api(api) = reloaded.data else {
            unreachable!();
        };
        assert_eq!(api.connections[0].id, connection_id);
        assert_eq!(api.connections[0].api_key, "replacement-key");

        let mut changed_protocol = edited.clone();
        changed_protocol.revision = 2;
        if let ProviderData::Api(api) = &mut changed_protocol.data {
            api.connections[0].protocol = CliProtocol::OpenaiChat;
        }
        assert!(matches!(
            repository.update_provider(&changed_protocol, 2, None).await,
            Err(AppError::Blocked(_))
        ));

        let mut without_connection = edited;
        without_connection.revision = 2;
        if let ProviderData::Api(api) = &mut without_connection.data {
            api.connections
                .retain(|connection| connection.id != connection_id);
        }
        assert!(matches!(
            repository
                .update_provider(&without_connection, 2, None)
                .await,
            Err(AppError::Blocked(_))
        ));
    }
}
