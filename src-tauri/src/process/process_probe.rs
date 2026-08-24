use std::path::Path;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

pub fn is_cli_running(executable: &Path, command_name: &str) -> Option<bool> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    let expected = std::fs::canonicalize(executable).ok();
    let mut could_inspect = false;
    for process in system.processes().values() {
        let process_exe = process.exe();
        if let Some(process_exe) = process_exe {
            could_inspect = true;
            if expected.as_ref().is_some_and(|expected| {
                std::fs::canonicalize(process_exe).ok().as_ref() == Some(expected)
            }) {
                return Some(true);
            }
            if process_exe
                .file_stem()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(command_name))
            {
                return Some(true);
            }
        }
    }
    could_inspect.then_some(false)
}
