use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    adapters::HostEnvironment,
    domain::CliId,
    error::{AppError, AppResult},
    process::fixed_command::run_fixed,
};

// Windows PowerShell can take several seconds to cold-start under load. Keep
// the shorter budget for native Unix executables while allowing Windows shims
// enough time to return their version reliably.
const VERSION_PROBE_TIMEOUT: Duration = if cfg!(windows) {
    Duration::from_secs(8)
} else {
    Duration::from_secs(3)
};

#[derive(Debug, Clone)]
pub struct DiscoveredExecutable {
    pub path: PathBuf,
    pub source: String,
    pub version: Option<String>,
}

pub async fn discover_executable(
    cli_id: CliId,
    environment: &HostEnvironment,
    manual: Option<&Path>,
) -> AppResult<Option<DiscoveredExecutable>> {
    if let Some(path) = manual {
        let path = validate_executable(path).await?;
        return Ok(Some(DiscoveredExecutable {
            version: probe_version(&path).await,
            path,
            source: "manual override".into(),
        }));
    }
    let command = cli_id.command();
    let mut candidates = Vec::<(PathBuf, String)>::new();
    if let Some(path) = environment.value("PATH") {
        for directory in std::env::split_paths(&OsString::from(path)) {
            append_command_candidates(&mut candidates, &directory, command, "process PATH");
        }
    }
    #[cfg(unix)]
    if let Some(shell_path) = login_shell_path().await {
        for directory in std::env::split_paths(&OsString::from(shell_path)) {
            append_command_candidates(&mut candidates, &directory, command, "login shell PATH");
        }
    }
    for path in common_locations(cli_id, environment) {
        candidates.push((path, "official common location".into()));
    }
    let mut seen = HashSet::new();
    for (candidate, source) in candidates {
        if !seen.insert(candidate.clone()) || !candidate.is_file() {
            continue;
        }
        if let Ok(path) = validate_executable(&candidate).await {
            return Ok(Some(DiscoveredExecutable {
                version: probe_version(&path).await,
                path,
                source,
            }));
        }
    }
    Ok(None)
}

fn append_command_candidates(
    candidates: &mut Vec<(PathBuf, String)>,
    directory: &Path,
    command: &str,
    source: &str,
) {
    candidates.push((directory.join(command), source.into()));
    #[cfg(windows)]
    for extension in ["exe", "ps1", "cmd", "bat"] {
        candidates.push((
            directory.join(format!("{command}.{extension}")),
            source.into(),
        ));
    }
}

fn common_locations(cli_id: CliId, environment: &HostEnvironment) -> Vec<PathBuf> {
    let command = cli_id.command();
    let mut values = vec![
        environment.home.join(".local/bin").join(command),
        environment.home.join(".npm-global/bin").join(command),
        environment.home.join(".bun/bin").join(command),
        environment.home.join("bin").join(command),
    ];
    #[cfg(unix)]
    {
        values.push(PathBuf::from("/usr/local/bin").join(command));
        values.push(PathBuf::from("/opt/homebrew/bin").join(command));
    }
    #[cfg(windows)]
    if let Some(app_data) = environment.value("APPDATA") {
        values.push(PathBuf::from(app_data).join(format!("{command}.cmd")));
    }
    values
}

async fn validate_executable(path: &Path) -> AppResult<PathBuf> {
    let metadata = tokio::fs::metadata(path).await?;
    if !metadata.is_file() {
        return Err(AppError::Validation(
            "CLI executable must be a regular file".into(),
        ));
    }
    let canonical = tokio::fs::canonicalize(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(AppError::Validation("CLI file is not executable".into()));
        }
    }
    Ok(canonical)
}

async fn probe_version(path: &Path) -> Option<String> {
    let output = run_fixed(path, &["--version"], None, VERSION_PROBE_TIMEOUT)
        .await
        .ok()?;
    let text = if output.stdout.trim().is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    let version = text.lines().next()?.trim();
    (!version.is_empty()).then(|| version.chars().take(160).collect())
}

#[cfg(unix)]
async fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL").map(PathBuf::from)?;
    let allowed = ["bash", "zsh", "fish", "sh"];
    if !shell
        .file_name()
        .is_some_and(|name| allowed.iter().any(|item| name == *item))
    {
        return None;
    }
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new(shell)
            .args(["-lc", "printf %s \"$PATH\""])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    (output.status.success()).then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use super::*;

    #[tokio::test]
    async fn manual_executable_is_canonicalized_and_probed() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let executable = {
            let path = std::env::var_os("PATH").expect("test PATH should be available");
            std::env::split_paths(&path)
                .map(|directory| directory.join("rustc.exe"))
                .find(|candidate| candidate.is_file())
                .expect("rustc.exe should be available while running Cargo tests")
        };
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;

            let executable = temp.path().join("codex");
            tokio::fs::write(&executable, "#!/bin/sh\necho codex 1.2.3\n")
                .await
                .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            executable
        };
        let environment = HostEnvironment {
            home: temp.path().to_path_buf(),
            variables: BTreeMap::new(),
            present_variables: HashSet::new(),
            os: std::env::consts::OS.into(),
        };
        let found = discover_executable(CliId::Codex, &environment, Some(&executable))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            found.path,
            tokio::fs::canonicalize(&executable).await.unwrap()
        );
        #[cfg(windows)]
        assert!(
            found
                .version
                .as_deref()
                .is_some_and(|version| version.starts_with("rustc "))
        );
        #[cfg(unix)]
        assert_eq!(found.version.as_deref(), Some("codex 1.2.3"));
    }
}
