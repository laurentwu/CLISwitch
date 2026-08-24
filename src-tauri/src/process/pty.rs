use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    error::{AppError, AppResult},
    process::fixed_command::prepare_fixed_invocation,
};

const MAX_OUTPUT: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct PtyControl {
    cancel: watch::Sender<bool>,
    input: mpsc::Sender<String>,
    process_id: Arc<AtomicU32>,
}

impl PtyControl {
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }

    pub async fn send_line(&self, line: String) -> AppResult<()> {
        if line.len() > 8 * 1024
            || line
                .chars()
                .any(|character| matches!(character, '\0' | '\r' | '\n'))
        {
            return Err(AppError::Validation(
                "OAuth input is invalid or too long".into(),
            ));
        }
        self.input
            .send(line)
            .await
            .map_err(|_| AppError::NotFound("OAuth session has finished".into()))
    }

    pub fn process_id(&self) -> Option<u32> {
        match self.process_id.load(Ordering::SeqCst) {
            0 => None,
            value => Some(value),
        }
    }
}

#[derive(Debug)]
pub struct SpawnedPty {
    pub control: PtyControl,
    pub output: mpsc::Receiver<String>,
    pub completion: oneshot::Receiver<AppResult<i32>>,
}

pub fn spawn_fixed_pty(
    executable: PathBuf,
    args: Vec<String>,
    environment: std::collections::BTreeMap<String, String>,
    working_directory: PathBuf,
    timeout: Duration,
) -> AppResult<SpawnedPty> {
    if !executable.is_file() {
        return Err(AppError::NotFound("OAuth CLI executable is missing".into()));
    }
    let (executable, args) = prepare_fixed_invocation(executable, args)?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (input_tx, input_rx) = mpsc::channel::<String>(8);
    let (output_tx, output_rx) = mpsc::channel::<String>(64);
    let (completion_tx, completion_rx) = oneshot::channel();
    let process_id = Arc::new(AtomicU32::new(0));
    let worker_pid = process_id.clone();
    tokio::task::spawn_blocking(move || {
        let result = run_blocking_pty(
            executable,
            args,
            environment,
            working_directory,
            timeout,
            cancel_rx,
            input_rx,
            output_tx,
            worker_pid,
        );
        let _ = completion_tx.send(result);
    });
    Ok(SpawnedPty {
        control: PtyControl {
            cancel: cancel_tx,
            input: input_tx,
            process_id,
        },
        output: output_rx,
        completion: completion_rx,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_blocking_pty(
    executable: PathBuf,
    args: Vec<String>,
    environment: std::collections::BTreeMap<String, String>,
    working_directory: PathBuf,
    timeout: Duration,
    cancel_rx: watch::Receiver<bool>,
    mut input_rx: mpsc::Receiver<String>,
    output_tx: mpsc::Sender<String>,
    process_id: Arc<AtomicU32>,
) -> AppResult<i32> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(other_io)?;
    let mut command = CommandBuilder::new(&executable);
    command.args(&args);
    command.cwd(&working_directory);
    command.env_clear();
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = pair.slave.spawn_command(command).map_err(other_io)?;
    drop(pair.slave);
    if let Some(pid) = child.process_id() {
        process_id.store(pid, Ordering::SeqCst);
    }
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            process_id.store(0, Ordering::SeqCst);
            return Err(other_io(error));
        }
    };
    let mut writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            process_id.store(0, Ordering::SeqCst);
            return Err(other_io(error));
        }
    };
    let (reader_output, reader_messages) = std::sync::mpsc::channel::<String>();
    let reader_thread = std::thread::spawn(move || {
        let mut total = 0usize;
        let mut buffer = [0u8; 4096];
        while total < MAX_OUTPUT {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let allowed = read.min(MAX_OUTPUT - total);
                    total += allowed;
                    let text = String::from_utf8_lossy(&buffer[..allowed]).into_owned();
                    let _ = reader_output.send(text);
                }
            }
        }
    });
    let started = Instant::now();
    let exit = 'wait: loop {
        forward_reader_output(&reader_messages, &output_tx);
        if *cancel_rx.borrow() || started.elapsed() >= timeout {
            let _ = child.kill();
            break child.wait().map_err(AppError::Io);
        }
        while let Ok(mut line) = input_rx.try_recv() {
            line.push('\n');
            if let Err(error) = writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.flush())
            {
                let _ = child.kill();
                let _ = child.wait();
                break 'wait Err(error.into());
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error.into());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(writer);
    drop(pair.master);
    finish_reader(
        reader_thread,
        &reader_messages,
        &output_tx,
        Duration::from_secs(2),
    );
    process_id.store(0, Ordering::SeqCst);
    let exit = exit?;
    if *cancel_rx.borrow() {
        return Err(AppError::Cancelled);
    }
    if started.elapsed() >= timeout && !exit.success() {
        return Err(AppError::Blocked("OAuth login timed out".into()));
    }
    Ok(exit.exit_code() as i32)
}

fn forward_reader_output(
    messages: &std::sync::mpsc::Receiver<String>,
    output: &mpsc::Sender<String>,
) {
    while let Ok(message) = messages.try_recv() {
        match output.try_send(message) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => break,
        }
    }
}

fn finish_reader(
    reader_thread: std::thread::JoinHandle<()>,
    messages: &std::sync::mpsc::Receiver<String>,
    output: &mpsc::Sender<String>,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while !reader_thread.is_finished() && Instant::now() < deadline {
        forward_reader_output(messages, output);
        std::thread::sleep(Duration::from_millis(10));
    }
    forward_reader_output(messages, output);
    if reader_thread.is_finished() {
        let _ = reader_thread.join();
        true
    } else {
        // Dropping the handle detaches a reader that is still held open by a descendant process.
        // The async output sender stays in this worker, so returning still closes the UI channel.
        false
    }
}

fn other_io(error: impl std::fmt::Display) -> AppError {
    AppError::Io(std::io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stuck_reader_does_not_block_completion() {
        let (message_tx, message_rx) = std::sync::mpsc::channel();
        let (output_tx, _output_rx) = mpsc::channel(1);
        let reader = std::thread::spawn(move || {
            let _sender = message_tx;
            std::thread::sleep(Duration::from_millis(200));
        });
        let started = Instant::now();
        assert!(!finish_reader(
            reader,
            &message_rx,
            &output_tx,
            Duration::from_millis(10)
        ));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn a_full_ui_output_channel_does_not_block_process_cleanup() {
        let (message_tx, message_rx) = std::sync::mpsc::channel();
        message_tx.send("first".into()).unwrap();
        message_tx.send("second".into()).unwrap();
        let (output_tx, mut output_rx) = mpsc::channel(1);

        let started = Instant::now();
        forward_reader_output(&message_rx, &output_tx);

        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(output_rx.try_recv().unwrap(), "first");
    }
}
