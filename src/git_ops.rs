use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

fn log_line(tx: Option<&mpsc::UnboundedSender<String>>, line: impl Into<String>) {
    if let Some(tx) = tx {
        let _ = tx.send(line.into());
    }
}

async fn pipe_stderr_lines(
    child: &mut tokio::process::Child,
    log_tx: Option<&mpsc::UnboundedSender<String>>,
) -> Result<()> {
    if let Some(stderr) = child.stderr.take() {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log_line(log_tx, format!("{line}\n"));
        }
    }
    Ok(())
}

/// Shallow clone into `dest`, replacing any existing directory.
async fn clone_shallow(
    repo_url: &str,
    dest: &Path,
    branch: &str,
    log_tx: Option<&mpsc::UnboundedSender<String>>,
) -> Result<()> {
    log_line(
        log_tx,
        format!("=== git clone --depth 1 --branch {branch} ===\n"),
    );

    maybe_create_parent(dest)?;

    if dest.exists() {
        log_line(log_tx, "=== removing previous clone ===\n");
        std::fs::remove_dir_all(dest).context("remove old clone")?;
    }

    let dest_str = dest.to_str().context("repository path must be UTF-8")?;

    let mut child = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--branch",
            branch,
            "--progress",
            repo_url,
            dest_str,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git clone")?;

    pipe_stderr_lines(&mut child, log_tx).await?;

    let status = child.wait().await.context("wait git clone")?;
    if !status.success() {
        anyhow::bail!("git clone failed with status {status}");
    }

    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dest)
        .output()
        .await
        .context("git rev-parse")?;
    let hash = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    log_line(
        log_tx,
        format!("=== cloned {branch} @ {hash} ===\n\n"),
    );

    Ok(())
}

async fn fetch_and_reset(
    repo_url: &str,
    dest: &Path,
    branch: &str,
    log_tx: Option<&mpsc::UnboundedSender<String>>,
) -> Result<()> {
    log_line(log_tx, "=== git remote set-url origin ===\n");
    let mut remote = Command::new("git")
        .args(["remote", "set-url", "origin", repo_url])
        .current_dir(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git remote set-url")?;
    pipe_stderr_lines(&mut remote, log_tx).await?;
    let status = remote.wait().await.context("wait git remote set-url")?;
    if !status.success() {
        anyhow::bail!("git remote set-url failed with status {status}");
    }

    log_line(
        log_tx,
        format!("=== git fetch --depth 1 origin {branch} ===\n"),
    );
    let mut fetch = Command::new("git")
        .args([
            "fetch",
            "--depth",
            "1",
            "--progress",
            "origin",
            branch,
        ])
        .current_dir(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git fetch")?;
    pipe_stderr_lines(&mut fetch, log_tx).await?;
    let status = fetch.wait().await.context("wait git fetch")?;
    if !status.success() {
        anyhow::bail!("git fetch failed with status {status}");
    }

    log_line(log_tx, "=== git reset --hard FETCH_HEAD ===\n");
    let mut reset = Command::new("git")
        .args(["reset", "--hard", "FETCH_HEAD"])
        .current_dir(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn git reset")?;
    pipe_stderr_lines(&mut reset, log_tx).await?;
    let status = reset.wait().await.context("wait git reset")?;
    if !status.success() {
        anyhow::bail!("git reset failed with status {status}");
    }

    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dest)
        .output()
        .await
        .context("git rev-parse")?;
    let hash = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    log_line(
        log_tx,
        format!("=== updated {branch} @ {hash} ===\n\n"),
    );

    Ok(())
}

/// Clone if missing, otherwise fetch + reset to the latest remote tip for `branch`.
/// Git output is appended to `log_tx` when present (build logs); omit for silent refresh.
pub async fn ensure_repo_latest(
    repo_url: &str,
    dest: &Path,
    branch: &str,
    log_tx: Option<&mpsc::UnboundedSender<String>>,
) -> Result<()> {
    if !dest.join(".git").exists() {
        clone_shallow(repo_url, dest, branch, log_tx).await
    } else {
        fetch_and_reset(repo_url, dest, branch, log_tx).await
    }
}

fn maybe_create_parent(dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).context("create parent dirs")?;
    }
    Ok(())
}

/// Compare local `HEAD` to the remote branch tip via `git ls-remote` (does not fetch — avoids
/// leaving FETCH_HEAD ahead of HEAD between polls and triggering duplicate auto-runs).
pub async fn remote_has_new_commits(repo_url: &str, dest: &Path, branch: &str) -> Result<bool> {
    if !dest.join(".git").exists() {
        return Ok(false);
    }

    let local = git_rev_parse(dest, "HEAD").await?;

    let spec = format!("refs/heads/{branch}");
    let out = Command::new("git")
        .args(["ls-remote", repo_url, &spec])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("git ls-remote")?;
    if !out.status.success() {
        anyhow::bail!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let remote_sha = stdout
        .lines()
        .find(|l| !l.is_empty())
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_string);

    let Some(remote_sha) = remote_sha else {
        return Ok(false);
    };

    Ok(remote_sha != local)
}

async fn git_rev_parse(dest: &Path, rev: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dest)
        .output()
        .await
        .with_context(|| format!("git rev-parse {rev}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse {} failed: {}",
            rev,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn microci_scripts(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let dir = repo_root.join(".mini-ci");
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".sh"))
                .unwrap_or(false)
                && p.is_file()
        })
        .collect();
    paths.sort();
    Ok(paths)
}
