use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn ensure_clone(repo_url: &str, dest: &Path) -> Result<()> {
    if dest.join(".git").exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).context("create repo parent")?;
    }
    let dest_str = dest.to_str().context("repo path utf-8")?;
    let status = Command::new("git")
        .args(["clone", "--depth", "1", repo_url, dest_str])
        .status()
        .await
        .context("spawn git clone")?;
    anyhow::ensure!(status.success(), "git clone failed with {:?}", status);
    Ok(())
}

pub async fn fetch_checkout(repo_root: &Path, branch: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["fetch", "origin", branch])
        .current_dir(repo_root)
        .output()
        .await
        .context("git fetch")?;
    let fetch_log = String::from_utf8_lossy(&out.stderr).to_string()
        + &String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        // shallow clone might need full branch first fetch
        let _ = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(repo_root)
            .output()
            .await;
    }

    let status = Command::new("git")
        .args(["checkout", "-B", branch, &format!("origin/{branch}")])
        .current_dir(repo_root)
        .status()
        .await
        .context("git checkout")?;

    if !status.success() {
        let status2 = Command::new("git")
            .args(["checkout", branch])
            .current_dir(repo_root)
            .status()
            .await
            .context("git checkout local")?;
        anyhow::ensure!(status2.success(), "git checkout failed");
    }

    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .await
        .context("git rev-parse")?;
    let hash = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    Ok(format!("{fetch_log}\nchecked out {branch} @ {hash}\n"))
}

pub fn microci_scripts(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let dir = repo_root.join(".microci");
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
