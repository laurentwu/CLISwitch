use std::path::{Component, Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct PrivatePaths {
    pub root: PathBuf,
    pub database: PathBuf,
    pub auth: PathBuf,
    pub backups: PathBuf,
    pub oauth_tmp: PathBuf,
    pub logs: PathBuf,
}

impl PrivatePaths {
    pub fn platform_default() -> AppResult<Self> {
        let dirs = ProjectDirs::from("io.github", "laurentwu", "CLISwitch")
            .ok_or_else(|| AppError::Io(std::io::Error::other("no application data directory")))?;
        Ok(Self::from_root(dirs.data_local_dir().to_path_buf()))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            database: root.join("cliswitch.db"),
            auth: root.join("auth"),
            backups: root.join("backups"),
            oauth_tmp: root.join("oauth-tmp"),
            logs: root.join("logs"),
            root,
        }
    }

    pub async fn ensure(&self) -> AppResult<()> {
        for path in [
            &self.root,
            &self.auth,
            &self.backups,
            &self.oauth_tmp,
            &self.logs,
        ] {
            tokio::fs::create_dir_all(path).await?;
            set_private_directory_permissions(path).await?;
        }
        Ok(())
    }

    pub fn safe_relative(root: &Path, relative: &Path) -> AppResult<PathBuf> {
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(AppError::Validation("unsafe relative path".into()));
        }
        Ok(root.join(relative))
    }

    pub async fn auth_profile_dir(&self, provider_id: uuid::Uuid) -> AppResult<PathBuf> {
        let directory = self.auth.join(provider_id.to_string());
        tokio::fs::create_dir_all(&directory).await?;
        set_private_directory_permissions(&directory).await?;
        Ok(directory)
    }
}

#[cfg(unix)]
pub async fn set_private_directory_permissions(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn set_private_directory_permissions(_path: &Path) -> AppResult<()> {
    Ok(())
}

#[cfg(unix)]
pub async fn set_private_file_permissions(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn set_private_file_permissions(_path: &Path) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_cannot_escape_root() {
        let root = Path::new("/safe");
        assert!(PrivatePaths::safe_relative(root, Path::new("auth/value.json")).is_ok());
        assert!(PrivatePaths::safe_relative(root, Path::new("../outside")).is_err());
        assert!(PrivatePaths::safe_relative(root, Path::new("/absolute")).is_err());
    }
}
