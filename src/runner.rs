use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\].*?\x07|\r").unwrap()
});

fn strip_ansi(s: &str) -> String {
    ANSI_RE.replace_all(s, "").into_owned()
}

pub async fn run_shell_script(
    script_path: &Path,
    cwd: &Path,
    log_tx: mpsc::UnboundedSender<String>,
) -> Result<i32> {
    #[cfg(unix)]
    let mut child = Command::new("/bin/sh")
        .arg(script_path)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawn script")?;

    #[cfg(not(unix))]
    anyhow::bail!("mini-ci only supports Unix shells for .mini-ci tasks");

    let stdout = child.stdout.take().context("stdout")?;
    let stderr = child.stderr.take().context("stderr")?;

    let tx_out = log_tx.clone();
    let tx_err = log_tx;
    let out_fut = pump_lines(stdout, tx_out, "stdout");
    let err_fut = pump_lines(stderr, tx_err, "stderr");
    let wait_fut = async move { child.wait().await };

    let (_, _, status) = tokio::join!(out_fut, err_fut, wait_fut);

    Ok(status.context("wait script")?.code().unwrap_or(-1))
}

async fn pump_lines<R: AsyncRead + Unpin>(
    pipe: R,
    tx: mpsc::UnboundedSender<String>,
    label: &'static str,
) {
    let mut reader = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        let clean = strip_ansi(&line);
        let msg = format!("[{label}] {clean}\n");
        let _ = tx.send(msg);
    }
}
