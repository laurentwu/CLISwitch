use std::path::{Path, PathBuf};

use chrono::Utc;
use sqlx::Row;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    domain::{BackupMetadata, CliId},
    error::{AppError, AppResult},
    filesystem::{
        atomic_replace::{ResolvedTarget, atomic_replace, resolve_target},
        digest::{bytes_digest, file_digest},
        private_paths::{
            PrivatePaths, set_private_directory_permissions, set_private_file_permissions,
        },
    },
    persistence::repository::Repository,
};

const KEEP_PER_SOURCE: usize = 5;

#[derive(Debug, Clone)]
pub struct BackupRecord {
    pub metadata: BackupMetadata,
    pub content: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct BackupService {
    repository: Repository,
    paths: PrivatePaths,
}

impl BackupService {
    pub fn new(repository: Repository, paths: PrivatePaths) -> Self {
        Self { repository, paths }
    }

    pub async fn create(
        &self,
        cli_id: CliId,
        target: &ResolvedTarget,
        configuration_id: Option<Uuid>,
        contains_credentials: bool,
        expected_source_digest: Option<&str>,
    ) -> AppResult<BackupRecord> {
        let content = match tokio::fs::read(&target.write_path).await {
            Ok(content) => Some(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let actual_digest = content.as_deref().map(bytes_digest);
        if actual_digest.as_deref() != expected_source_digest {
            return Err(AppError::Conflict(
                "source changed while the backup was being created".into(),
            ));
        }
        let source_file_id = source_file_id(cli_id, &target.write_path);
        let id = Uuid::new_v4();
        let directory = self
            .paths
            .backups
            .join(cli_id.to_string())
            .join(&source_file_id);
        tokio::fs::create_dir_all(&directory).await?;
        set_private_directory_permissions(&directory).await?;
        let relative_backup_path = if let Some(content) = &content {
            let absolute = directory.join(format!("{id}.bin"));
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&absolute)
                .await?;
            set_private_file_permissions(&absolute).await?;
            file.write_all(content).await?;
            file.flush().await?;
            file.sync_all().await?;
            Some(
                absolute
                    .strip_prefix(&self.paths.root)
                    .map_err(|_| AppError::Blocked("backup path escaped data directory".into()))?
                    .to_path_buf(),
            )
        } else {
            None
        };
        #[cfg(unix)]
        let permissions = tokio::fs::metadata(&target.write_path)
            .await
            .ok()
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode()
            });
        #[cfg(not(unix))]
        let permissions = None;
        let metadata = BackupMetadata {
            id,
            cli_id,
            source_file_id: source_file_id.clone(),
            original_path: target.write_path.clone(),
            created_at: Utc::now(),
            configuration_id,
            original_digest: actual_digest,
            permissions,
            originally_existed: content.is_some(),
            contains_credentials,
            relative_backup_path,
        };
        self.insert_metadata(&metadata).await?;
        self.prune(cli_id, &source_file_id).await?;
        Ok(BackupRecord { metadata, content })
    }

    pub async fn rollback(&self, record: &BackupRecord, allowed_root: &Path) -> AppResult<()> {
        let target = resolve_target(&record.metadata.original_path, allowed_root).await?;
        match &record.content {
            Some(content) => {
                atomic_replace(&target, content).await?;
                #[cfg(unix)]
                if let Some(mode) = record.metadata.permissions {
                    use std::os::unix::fs::PermissionsExt;
                    tokio::fs::set_permissions(
                        &target.write_path,
                        std::fs::Permissions::from_mode(mode),
                    )
                    .await?;
                }
                Ok(())
            }
            None => match tokio::fs::remove_file(&target.write_path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
        }
    }

    pub async fn list(&self, cli_id: Option<CliId>) -> AppResult<Vec<BackupMetadata>> {
        let rows = if let Some(cli_id) = cli_id {
            sqlx::query("SELECT * FROM backup_metadata WHERE cli_id = ? ORDER BY created_at DESC")
                .bind(cli_id.to_string())
                .fetch_all(self.repository.pool())
                .await?
        } else {
            sqlx::query("SELECT * FROM backup_metadata ORDER BY created_at DESC")
                .fetch_all(self.repository.pool())
                .await?
        };
        rows.into_iter().map(metadata_from_row).collect()
    }

    pub async fn get(&self, id: Uuid) -> AppResult<BackupRecord> {
        let row = sqlx::query("SELECT * FROM backup_metadata WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.repository.pool())
            .await?
            .ok_or_else(|| AppError::NotFound(format!("backup {id}")))?;
        let metadata = metadata_from_row(row)?;
        let content = if let Some(relative) = &metadata.relative_backup_path {
            let path = self.resolve_backup_file(relative).await?;
            Some(tokio::fs::read(path).await?)
        } else {
            None
        };
        if content.as_deref().map(bytes_digest) != metadata.original_digest {
            return Err(AppError::Conflict(
                "backup content digest does not match metadata".into(),
            ));
        }
        Ok(BackupRecord { metadata, content })
    }

    async fn resolve_backup_file(&self, relative: &Path) -> AppResult<PathBuf> {
        let path = PrivatePaths::safe_relative(&self.paths.root, relative)?;
        if !path.starts_with(&self.paths.backups) {
            return Err(AppError::Blocked(
                "backup content path escaped the backup directory".into(),
            ));
        }
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Blocked(
                "backup content path is not a regular file".into(),
            ));
        }
        let canonical_root = tokio::fs::canonicalize(&self.paths.backups).await?;
        let canonical_path = tokio::fs::canonicalize(path).await?;
        if !canonical_path.starts_with(canonical_root) {
            return Err(AppError::Blocked(
                "backup content path escaped the backup directory".into(),
            ));
        }
        Ok(canonical_path)
    }

    pub async fn restore(
        &self,
        id: Uuid,
        allowed_root: &Path,
        expected_current_digest: Option<String>,
    ) -> AppResult<()> {
        let record = self.get(id).await?;
        if file_digest(&record.metadata.original_path).await? != expected_current_digest {
            return Err(AppError::Conflict(
                "target changed after restore preview".into(),
            ));
        }
        let target = resolve_target(&record.metadata.original_path, allowed_root).await?;
        let _undo = self
            .create(
                record.metadata.cli_id,
                &target,
                record.metadata.configuration_id,
                record.metadata.contains_credentials,
                expected_current_digest.as_deref(),
            )
            .await?;
        self.rollback(&record, allowed_root).await
    }

    async fn insert_metadata(&self, metadata: &BackupMetadata) -> AppResult<()> {
        sqlx::query("INSERT INTO backup_metadata(id, cli_id, source_file_id, original_path, created_at, configuration_id, original_digest, permissions, originally_existed, contains_credentials, relative_backup_path) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(metadata.id.to_string())
            .bind(metadata.cli_id.to_string())
            .bind(&metadata.source_file_id)
            .bind(metadata.original_path.to_string_lossy().to_string())
            .bind(metadata.created_at.to_rfc3339())
            .bind(metadata.configuration_id.map(|id| id.to_string()))
            .bind(metadata.original_digest.as_deref())
            .bind(metadata.permissions.map(i64::from))
            .bind(metadata.originally_existed)
            .bind(metadata.contains_credentials)
            .bind(metadata.relative_backup_path.as_ref().map(|path| path.to_string_lossy().to_string()))
            .execute(self.repository.pool())
            .await?;
        Ok(())
    }

    async fn prune(&self, cli_id: CliId, source_file_id: &str) -> AppResult<()> {
        let rows = sqlx::query("SELECT id, relative_backup_path FROM backup_metadata WHERE cli_id = ? AND source_file_id = ? ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?")
            .bind(cli_id.to_string())
            .bind(source_file_id)
            .bind(KEEP_PER_SOURCE as i64)
            .fetch_all(self.repository.pool())
            .await?;
        for row in rows {
            let id: String = row.try_get("id")?;
            let relative: Option<String> = row.try_get("relative_backup_path")?;
            if let Some(relative) = relative {
                let path = PrivatePaths::safe_relative(&self.paths.root, Path::new(&relative))?;
                if !path.starts_with(&self.paths.backups) {
                    return Err(AppError::Blocked(
                        "backup rotation escaped backup root".into(),
                    ));
                }
                match tokio::fs::remove_file(path).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            sqlx::query("DELETE FROM backup_metadata WHERE id = ?")
                .bind(id)
                .execute(self.repository.pool())
                .await?;
        }
        Ok(())
    }
}

fn source_file_id(cli_id: CliId, path: &Path) -> String {
    bytes_digest(format!("{cli_id}\0{}", path.to_string_lossy()).as_bytes())
        .trim_start_matches("sha256:")
        .to_string()
}

fn metadata_from_row(row: sqlx::sqlite::SqliteRow) -> AppResult<BackupMetadata> {
    use std::str::FromStr;
    Ok(BackupMetadata {
        id: Uuid::parse_str(&row.try_get::<String, _>("id")?)
            .map_err(|error| AppError::Serialization(error.to_string()))?,
        cli_id: CliId::from_str(&row.try_get::<String, _>("cli_id")?)?,
        source_file_id: row.try_get("source_file_id")?,
        original_path: PathBuf::from(row.try_get::<String, _>("original_path")?),
        created_at: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)
            .map_err(|error| AppError::Serialization(error.to_string()))?
            .with_timezone(&Utc),
        configuration_id: row
            .try_get::<Option<String>, _>("configuration_id")?
            .map(|value| {
                Uuid::parse_str(&value).map_err(|error| AppError::Serialization(error.to_string()))
            })
            .transpose()?,
        original_digest: row.try_get("original_digest")?,
        permissions: row
            .try_get::<Option<i64>, _>("permissions")?
            .map(|value| value as u32),
        originally_existed: row.try_get::<i64, _>("originally_existed")? != 0,
        contains_credentials: row.try_get::<i64, _>("contains_credentials")? != 0,
        relative_backup_path: row
            .try_get::<Option<String>, _>("relative_backup_path")?
            .map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{persistence::repository::Repository, services::redaction::Redactor};

    #[tokio::test]
    async fn keeps_only_five_backups_per_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths.clone());
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("settings.json");
        tokio::fs::write(&path, b"0").await.unwrap();
        for index in 0..7 {
            tokio::fs::write(&path, index.to_string()).await.unwrap();
            let target = resolve_target(&path, &root).await.unwrap();
            let expected = file_digest(&path).await.unwrap();
            service
                .create(CliId::ClaudeCode, &target, None, true, expected.as_deref())
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert_eq!(
            service.list(Some(CliId::ClaudeCode)).await.unwrap().len(),
            5
        );
    }

    #[tokio::test]
    async fn tombstone_rollback_removes_a_file_created_later() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths);
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("auth.json");
        let target = resolve_target(&path, &root).await.unwrap();
        let tombstone = service
            .create(CliId::Codex, &target, None, true, None)
            .await
            .unwrap();
        assert!(!tombstone.metadata.originally_existed);
        tokio::fs::write(&path, b"created later").await.unwrap();
        service.rollback(&tombstone, &root).await.unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rollback_restores_recorded_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths);
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("settings.json");
        tokio::fs::write(&path, b"original").await.unwrap();
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))
            .await
            .unwrap();
        let target = resolve_target(&path, &root).await.unwrap();
        let expected = file_digest(&path).await.unwrap();
        let record = service
            .create(CliId::ClaudeCode, &target, None, true, expected.as_deref())
            .await
            .unwrap();
        tokio::fs::write(&path, b"changed").await.unwrap();
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .unwrap();
        service.rollback(&record, &root).await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"original");
        assert_eq!(
            tokio::fs::metadata(&path)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn backup_reads_reject_symlink_substitution() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths.clone());
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("auth.json");
        tokio::fs::write(&source, b"saved secret").await.unwrap();
        let target = resolve_target(&source, &root).await.unwrap();
        let expected = file_digest(&source).await.unwrap();
        let record = service
            .create(CliId::Codex, &target, None, true, expected.as_deref())
            .await
            .unwrap();
        let backup_path = PrivatePaths::safe_relative(
            &paths.root,
            record.metadata.relative_backup_path.as_ref().unwrap(),
        )
        .unwrap();
        tokio::fs::remove_file(&backup_path).await.unwrap();
        let outside = temp.path().join("outside-secret");
        tokio::fs::write(&outside, b"must not be read")
            .await
            .unwrap();
        std::os::unix::fs::symlink(&outside, &backup_path).unwrap();

        assert!(matches!(
            service.get(record.metadata.id).await,
            Err(AppError::Blocked(_))
        ));
    }

    #[tokio::test]
    async fn backup_creation_rejects_a_source_changed_after_preview() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths);
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("settings.json");
        tokio::fs::write(&path, b"after-preview").await.unwrap();
        let target = resolve_target(&path, &root).await.unwrap();

        assert!(matches!(
            service
                .create(
                    CliId::ClaudeCode,
                    &target,
                    None,
                    false,
                    Some("sha256:stale-preview-digest")
                )
                .await,
            Err(AppError::Conflict(_))
        ));
        assert!(service.list(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn restore_creates_an_undo_backup_before_replacing_the_current_file() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths);
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("settings.json");
        tokio::fs::write(&path, b"version one").await.unwrap();
        let target = resolve_target(&path, &root).await.unwrap();
        let first_digest = file_digest(&path).await.unwrap();
        let first = service
            .create(
                CliId::ClaudeCode,
                &target,
                None,
                false,
                first_digest.as_deref(),
            )
            .await
            .unwrap();
        tokio::fs::write(&path, b"version two").await.unwrap();
        let second_digest = file_digest(&path).await.unwrap();

        service
            .restore(first.metadata.id, &root, second_digest.clone())
            .await
            .unwrap();

        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"version one");
        let backups = service.list(Some(CliId::ClaudeCode)).await.unwrap();
        assert_eq!(backups.len(), 2);
        assert!(backups.iter().any(|backup| {
            backup.id != first.metadata.id && backup.original_digest == second_digest
        }));
    }

    #[tokio::test]
    async fn restoring_a_tombstone_removes_the_later_file_and_keeps_an_undo() {
        let temp = tempfile::tempdir().unwrap();
        let paths = PrivatePaths::from_root(temp.path().join("data"));
        paths.ensure().await.unwrap();
        let repository = Repository::open(&paths.database, Redactor::default())
            .await
            .unwrap();
        let service = BackupService::new(repository, paths);
        let root = temp.path().join("config");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("new-auth.json");
        let target = resolve_target(&path, &root).await.unwrap();
        let tombstone = service
            .create(CliId::Codex, &target, None, true, None)
            .await
            .unwrap();
        tokio::fs::write(&path, b"created later").await.unwrap();
        let current_digest = file_digest(&path).await.unwrap();

        service
            .restore(tombstone.metadata.id, &root, current_digest.clone())
            .await
            .unwrap();

        assert!(!path.exists());
        let backups = service.list(Some(CliId::Codex)).await.unwrap();
        assert_eq!(backups.len(), 2);
        assert!(backups.iter().any(|backup| {
            backup.id != tombstone.metadata.id && backup.original_digest == current_digest
        }));
    }
}
