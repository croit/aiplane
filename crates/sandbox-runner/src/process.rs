// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::process::Output;
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const CONTROL_OUTPUT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("sandbox control command exceeded its deadline")]
    Timeout,
    #[error("sandbox control command output exceeded 64 KiB")]
    TooLarge,
    #[error("sandbox control command failed: {0}")]
    Io(#[from] std::io::Error),
}

async fn read_control(reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, ProcessError> {
    let mut bytes = Vec::new();
    reader
        .take(CONTROL_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > CONTROL_OUTPUT_BYTES {
        return Err(ProcessError::TooLarge);
    }
    Ok(bytes)
}

pub async fn run_control(mut command: Command, deadline: Duration) -> Result<Output, ProcessError> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("control stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("control stderr unavailable"))?;
    let result = tokio::time::timeout(deadline, async {
        let (stdout, stderr, status) =
            tokio::try_join!(read_control(stdout), read_control(stderr), async {
                child.wait().await.map_err(ProcessError::Io)
            })?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
    .await
    .unwrap_or(Err(ProcessError::Timeout));
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    }
    result
}

pub async fn run_control_with_input(
    mut command: Command,
    input: &[u8],
    deadline: Duration,
    output_limit: u64,
) -> Result<Output, ProcessError> {
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("control stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("control stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("control stderr unavailable"))?;
    let result = tokio::time::timeout(deadline, async {
        let ((), stdout, stderr, status) = tokio::try_join!(
            async move {
                use tokio::io::AsyncWriteExt;
                let mut stdin = stdin;
                stdin.write_all(input).await.map_err(ProcessError::Io)?;
                stdin.shutdown().await.map_err(ProcessError::Io)
            },
            read_control_capped(stdout, output_limit),
            read_control_capped(stderr, CONTROL_OUTPUT_BYTES),
            async { child.wait().await.map_err(ProcessError::Io) },
        )?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
    .await
    .unwrap_or(Err(ProcessError::Timeout));
    if result.is_err() {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
    }
    result
}

async fn read_control_capped(
    reader: impl tokio::io::AsyncRead + Unpin,
    limit: u64,
) -> Result<Vec<u8>, ProcessError> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > limit {
        return Err(ProcessError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[tokio::test]
    async fn bounded_control_command_preserves_exit_status_and_both_capped_streams() {
        let output = run_control(
            shell("printf output; printf diagnostic >&2; exit 7"),
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"output");
        assert_eq!(output.stderr, b"diagnostic");
    }

    #[tokio::test]
    async fn bounded_control_command_rejects_cap_plus_one_on_either_stream() {
        for redirect in ["", " >&2"] {
            let script = format!(
                "i=0; while [ $i -lt 4097 ]; do printf 0123456789abcdef{redirect}; i=$((i+1)); done"
            );
            assert!(matches!(
                run_control(shell(&script), std::time::Duration::from_secs(2)).await,
                Err(ProcessError::TooLarge)
            ));
        }
    }

    #[tokio::test]
    async fn bounded_control_command_times_out_and_reports_spawn_failure() {
        assert!(matches!(
            run_control(shell("exec sleep 1"), std::time::Duration::from_millis(10)).await,
            Err(ProcessError::Timeout)
        ));
        assert!(matches!(
            run_control(
                tokio::process::Command::new("/nonexistent-control-command"),
                std::time::Duration::from_secs(1)
            )
            .await,
            Err(ProcessError::Io(_))
        ));
    }

    #[tokio::test]
    async fn typed_control_input_reaches_stdin_without_becoming_an_argument() {
        let output = run_control_with_input(
            shell("read value; printf 'received:%s' \"$value\""),
            b"private-marker\n",
            Duration::from_secs(2),
            1024,
        )
        .await
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"received:private-marker");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn typed_control_input_output_obeys_the_requested_cap() {
        assert!(matches!(
            run_control_with_input(
                shell("cat >/dev/null; printf 12345"),
                b"input\n",
                Duration::from_secs(2),
                4,
            )
            .await,
            Err(ProcessError::TooLarge)
        ));
    }
}
