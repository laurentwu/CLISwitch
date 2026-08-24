use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    filesystem::private_paths::{set_private_directory_permissions, set_private_file_permissions},
};

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub requested_path: PathBuf,
    pub write_path: PathBuf,
    pub existed: bool,
}

pub async fn resolve_target(path: &Path, allowed_root: &Path) -> AppResult<ResolvedTarget> {
    ensure_private_directory_tree(allowed_root).await?;
    let allowed = tokio::fs::canonicalize(allowed_root).await?;
    let candidate = canonicalize_allow_missing(path).await?;
    if !candidate.starts_with(&allowed) {
        return Err(AppError::Blocked(
            "resolved configuration path is outside the approved directory".into(),
        ));
    }
    let metadata = tokio::fs::symlink_metadata(path).await;
    let (write_path, existed) = match metadata {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let resolved = tokio::fs::canonicalize(path).await.map_err(|error| {
                AppError::Blocked(format!("cannot resolve configuration symlink: {error}"))
            })?;
            let target_metadata = tokio::fs::metadata(&resolved).await?;
            if !target_metadata.is_file() {
                return Err(AppError::Blocked(
                    "symlink target is not a regular file".into(),
                ));
            }
            (resolved, true)
        }
        Ok(metadata) if metadata.is_file() => (tokio::fs::canonicalize(path).await?, true),
        Ok(_) => return Err(AppError::Blocked("target is not a regular file".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| AppError::Validation("target has no parent directory".into()))?;
            ensure_private_directory_tree(parent).await?;
            let parent = tokio::fs::canonicalize(parent).await?;
            let file_name = path
                .file_name()
                .ok_or_else(|| AppError::Validation("target has no file name".into()))?;
            (parent.join(file_name), false)
        }
        Err(error) => return Err(error.into()),
    };
    if !write_path.starts_with(&allowed) {
        return Err(AppError::Blocked(
            "resolved configuration path is outside the approved directory".into(),
        ));
    }
    Ok(ResolvedTarget {
        requested_path: path.to_path_buf(),
        write_path,
        existed,
    })
}

/// Resolves every existing path component without creating the missing suffix. This is used for
/// containment checks that must happen before any filesystem mutation.
pub async fn canonicalize_allow_missing(path: &Path) -> AppResult<PathBuf> {
    let mut cursor = path.to_path_buf();
    let mut missing = Vec::<OsString>::new();
    loop {
        match tokio::fs::symlink_metadata(&cursor).await {
            Ok(_) => {
                if !missing.is_empty() && !tokio::fs::metadata(&cursor).await?.is_dir() {
                    return Err(AppError::Blocked(
                        "path has a non-directory existing ancestor".into(),
                    ));
                }
                let mut resolved = tokio::fs::canonicalize(&cursor).await?;
                for component in missing.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = cursor
                    .file_name()
                    .ok_or_else(|| AppError::Validation("path has no existing ancestor".into()))?;
                missing.push(component.to_os_string());
                cursor = cursor
                    .parent()
                    .ok_or_else(|| AppError::Validation("path has no parent directory".into()))?
                    .to_path_buf();
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn ensure_private_directory_tree(path: &Path) -> AppResult<()> {
    let mut missing = Vec::new();
    let mut cursor = path;
    loop {
        match tokio::fs::symlink_metadata(cursor).await {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if tokio::fs::metadata(cursor).await?.is_dir() {
                    break;
                }
                return Err(AppError::Blocked(
                    "approved directory symlink does not resolve to a directory".into(),
                ));
            }
            Ok(_) => {
                return Err(AppError::Blocked(
                    "approved configuration directory is not a directory".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_path_buf());
                cursor = cursor.parent().ok_or_else(|| {
                    AppError::Validation("configuration directory has no existing ancestor".into())
                })?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    for directory in missing.into_iter().rev() {
        match tokio::fs::create_dir(&directory).await {
            Ok(()) => set_private_directory_permissions(&directory).await?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !tokio::fs::metadata(&directory).await?.is_dir() {
                    return Err(AppError::Blocked(
                        "configuration path changed while creating directories".into(),
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub async fn atomic_replace(target: &ResolvedTarget, content: &[u8]) -> AppResult<()> {
    let parent = target
        .write_path
        .parent()
        .ok_or_else(|| AppError::Validation("target has no parent directory".into()))?;
    let temp_path = parent.join(format!(".cliswitch-{}.tmp", Uuid::new_v4()));
    let existing_permissions = if target.existed {
        Some(tokio::fs::metadata(&target.write_path).await?.permissions())
    } else {
        None
    };
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.create_new(true).write(true);
        let mut file = options
            .open(&temp_path)
            .await
            .map_err(|error| staged_io("temporary file creation", error))?;
        set_private_file_permissions(&temp_path)
            .await
            .map_err(|error| staged_error("temporary file permissions", error))?;
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)
                .await
                .map_err(|error| staged_io("temporary file permissions", error))?;
        }
        file.write_all(content)
            .await
            .map_err(|error| staged_io("temporary file write", error))?;
        file.flush()
            .await
            .map_err(|error| staged_io("temporary file flush", error))?;
        file.sync_all()
            .await
            .map_err(|error| staged_io("temporary file sync", error))?;
        drop(file);
        replace_path(&temp_path, &target.write_path)
            .await
            .map_err(|error| staged_error("atomic replace", error))?;
        sync_parent(parent)
            .await
            .map_err(|error| staged_error("parent directory sync", error))?;
        AppResult::Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    result
}

fn staged_io(stage: &str, error: std::io::Error) -> AppError {
    AppError::Io(std::io::Error::new(
        error.kind(),
        format!("{stage} failed: {error}"),
    ))
}

fn staged_error(stage: &str, error: AppError) -> AppError {
    AppError::Io(std::io::Error::other(format!("{stage} failed: {error}")))
}

#[cfg(not(windows))]
async fn replace_path(from: &Path, to: &Path) -> AppResult<()> {
    tokio::fs::rename(from, to).await?;
    Ok(())
}

#[cfg(windows)]
async fn replace_path(from: &Path, to: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
async fn sync_parent(path: &Path) -> AppResult<()> {
    let file = tokio::fs::File::open(path).await?;
    file.sync_all().await?;
    Ok(())
}

#[cfg(not(unix))]
async fn sync_parent(_path: &Path) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn atomic_replace_preserves_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("actual.json");
        tokio::fs::write(&actual, b"old").await.unwrap();
        let link = temp.path().join("link.json");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&actual, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&actual, &link).unwrap();
        let target = resolve_target(&link, temp.path()).await.unwrap();
        atomic_replace(&target, b"new").await.unwrap();
        assert_eq!(tokio::fs::read(&actual).await.unwrap(), b"new");
        assert!(
            tokio::fs::symlink_metadata(&link)
                .await
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn missing_approved_directories_are_created_privately() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("new").join("config");
        let path = root.join("nested").join("settings.json");
        let target = resolve_target(&path, &root).await.unwrap();
        atomic_replace(&target, b"{}\n").await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"{}\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                tokio::fs::metadata(&root)
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                tokio::fs::metadata(path.parent().unwrap())
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[tokio::test]
    async fn rejected_missing_target_does_not_create_outside_directories() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let outside_parent = temp.path().join("outside").join("nested");
        tokio::fs::create_dir_all(&allowed).await.unwrap();

        assert!(matches!(
            resolve_target(&outside_parent.join("settings.json"), &allowed).await,
            Err(AppError::Blocked(_))
        ));
        assert!(!outside_parent.exists());
    }
}
