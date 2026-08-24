use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

use crate::error::{AppError, AppResult};

const MAX_PROBE_OUTPUT: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_fixed(
    executable: &Path,
    args: &[&str],
    environment: Option<&BTreeMap<String, String>>,
    timeout: Duration,
) -> AppResult<CommandOutput> {
    if !executable.is_file() {
        return Err(AppError::NotFound(
            "CLI executable is not a regular file".into(),
        ));
    }
    let (executable, args) = prepare_fixed_invocation(
        executable.to_path_buf(),
        args.iter().map(|arg| (*arg).to_string()).collect(),
    )?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(environment) = environment {
        command.env_clear();
        for (key, value) in environment {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Io(std::io::Error::other("CLI stdout pipe is unavailable")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Io(std::io::Error::other("CLI stderr pipe is unavailable")))?;
    let mut stdout_task = tokio::spawn(read_bounded(stdout));
    let mut stderr_task = tokio::spawn(read_bounded(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(AppError::Blocked("CLI probe timed out".into()));
        }
    };
    let stdout = finish_reader(&mut stdout_task).await?;
    let stderr = finish_reader(&mut stderr_task).await?;
    Ok(CommandOutput {
        status: status.code().unwrap_or(-1),
        stdout: bounded_utf8(&stdout),
        stderr: bounded_utf8(&stderr),
    })
}

async fn read_bounded(mut reader: impl AsyncRead + Unpin) -> AppResult<Vec<u8>> {
    let mut kept = Vec::with_capacity(MAX_PROBE_OUTPUT);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_PROBE_OUTPUT.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(kept)
}

async fn finish_reader(
    task: &mut tokio::task::JoinHandle<AppResult<Vec<u8>>>,
) -> AppResult<Vec<u8>> {
    match tokio::time::timeout(Duration::from_secs(2), &mut *task).await {
        Ok(result) => {
            result.map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))?
        }
        Err(_) => {
            task.abort();
            Err(AppError::Blocked(
                "CLI probe output stream did not close".into(),
            ))
        }
    }
}

/// Converts Windows PowerShell npm shims into an explicit, fixed-argv
/// invocation. Batch-only shims are rejected because launching them would
/// require passing a command string through `cmd.exe`.
pub fn prepare_fixed_invocation(
    executable: PathBuf,
    args: Vec<String>,
) -> AppResult<(PathBuf, Vec<String>)> {
    #[cfg(not(windows))]
    {
        Ok((executable, args))
    }
    #[cfg(windows)]
    {
        let extension = executable
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let script = match extension.as_str() {
            "ps1" => executable,
            "cmd" | "bat" => {
                let sibling = executable.with_extension("ps1");
                if !sibling.is_file() {
                    return Err(AppError::Unsupported(
                        "batch-only CLI shims cannot be launched safely; select an .exe or .ps1 executable"
                            .into(),
                    ));
                }
                sibling
            }
            _ => return Ok((executable, args)),
        };
        let windows_root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("WINDIR"))
            .ok_or_else(|| AppError::NotFound("Windows system directory is unavailable".into()))?;
        let powershell = PathBuf::from(windows_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if !powershell.is_file() {
            return Err(AppError::NotFound(
                "Windows PowerShell executable is unavailable".into(),
            ));
        }
        let mut fixed_args = vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
        ];
        fixed_args.extend(args);
        Ok((powershell, fixed_args))
    }
}

fn bounded_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PROBE_OUTPUT)]).into_owned()
}

pub fn isolated_environment(
    home: &Path,
    additions: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    const PASSTHROUGH: &[&str] = &[
        "PATH",
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
    ];
    let mut environment: BTreeMap<String, String> = PASSTHROUGH
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), value)))
        .collect();
    let home = home.to_string_lossy().to_string();
    environment.insert("HOME".into(), home.clone());
    environment.insert("USERPROFILE".into(), home.clone());
    environment.insert("XDG_CONFIG_HOME".into(), format!("{home}/.config"));
    environment.insert("XDG_DATA_HOME".into(), format!("{home}/.local/share"));
    for (key, value) in additions {
        environment.insert(key.clone(), value.clone());
    }
    environment
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn probe_output_is_drained_but_kept_within_the_limit() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        let write = tokio::spawn(async move {
            writer
                .write_all(&vec![b'x'; MAX_PROBE_OUTPUT * 3])
                .await
                .unwrap();
            writer.shutdown().await.unwrap();
        });
        let output = read_bounded(reader).await.unwrap();
        write.await.unwrap();
        assert_eq!(output.len(), MAX_PROBE_OUTPUT);
    }
}
