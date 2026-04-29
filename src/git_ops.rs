use anyhow::{Context, Result};
use async_stream::stream;
use bytes::Bytes;
use futures_util::stream::Stream;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use std::process::Stdio;

fn combine_output(out: &std::process::Output) -> String {
    let mut s = String::new();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        s.push_str(&stderr);
        if !stderr.ends_with('\n') {
            s.push('\n');
        }
    }
    if !stdout.trim().is_empty() {
        s.push_str(&stdout);
        if !stdout.ends_with('\n') {
            s.push('\n');
        }
    }
    s
}

/// Live-streamed clone/fetch/checkout with stderr lines (clone progress, fetch, checkout).
pub fn sync_repository_stream(repo_url: String, dest: PathBuf, branch: String) -> impl Stream<Item = Result<Bytes, Infallible>> + Send {
    stream! {
        yield Ok(Bytes::from(format!(
            "=== mini-ci sync ===\ndestination: {}\n",
            dest.display()
        )));

        if let Err(e) = maybe_create_parent(&dest) {
            yield Ok(Bytes::from(format!("ERROR: {e:#}\n")));
            return;
        }

        let dest_str = match dest.to_str() {
            Some(s) => s.to_string(),
            None => {
                yield Ok(Bytes::from_static(b"ERROR: repository path must be UTF-8\n"));
                return;
            }
        };

        if !dest.join(".git").exists() {
            yield Ok(Bytes::from_static(b"=== git clone (--progress) ===\n"));
            let mut cmd = Command::new("git");
            cmd.args(["clone", "--progress", "--depth", "1", &repo_url, &dest_str]);
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    yield Ok(Bytes::from(format!("ERROR: spawn git clone: {e:#}\n")));
                    return;
                }
            };

            if let Some(stderr) = child.stderr.take() {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    yield Ok(Bytes::copy_from_slice(format!("{line}\n").as_bytes()));
                }
            }

            match child.wait().await {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    yield Ok(Bytes::from(format!(
                        "ERROR: git clone failed with status {status}\n"
                    )));
                    return;
                }
                Err(e) => {
                    yield Ok(Bytes::from(format!("ERROR: wait git clone: {e:#}\n")));
                    return;
                }
            }
        } else {
            yield Ok(Bytes::from_static(
                b"=== existing clone - skipping clone ===\n",
            ));
        }

        yield Ok(Bytes::from(format!(
            "=== git fetch origin {} ===\n",
            branch
        )));
        let fetch_out = match Command::new("git")
            .args(["fetch", "origin", &branch])
            .current_dir(&dest)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                yield Ok(Bytes::from(format!("ERROR: git fetch: {e:#}\n")));
                return;
            }
        };
        yield Ok(Bytes::copy_from_slice(combine_output(&fetch_out).as_bytes()));

        if !fetch_out.status.success() {
            yield Ok(Bytes::from_static(b"=== git fetch origin (fallback) ===\n"));
            let fb = match Command::new("git")
                .args(["fetch", "origin"])
                .current_dir(&dest)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    yield Ok(Bytes::from(format!(
                        "ERROR: git fetch fallback: {e:#}\n"
                    )));
                    return;
                }
            };
            yield Ok(Bytes::copy_from_slice(combine_output(&fb).as_bytes()));
        }

        yield Ok(Bytes::from_static(b"=== git checkout ===\n"));
        let co = match Command::new("git")
            .args(["checkout", "-B", &branch, &format!("origin/{branch}")])
            .current_dir(&dest)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                yield Ok(Bytes::from(format!("ERROR: git checkout: {e:#}\n")));
                return;
            }
        };
        yield Ok(Bytes::copy_from_slice(combine_output(&co).as_bytes()));

        let mut ok = co.status.success();
        if !ok {
            yield Ok(Bytes::from_static(b"=== git checkout (local branch) ===\n"));
            let co2 = match Command::new("git")
                .args(["checkout", &branch])
                .current_dir(&dest)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    yield Ok(Bytes::from(format!(
                        "ERROR: git checkout local: {e:#}\n"
                    )));
                    return;
                }
            };
            yield Ok(Bytes::copy_from_slice(combine_output(&co2).as_bytes()));
            ok = co2.status.success();
        }

        if !ok {
            yield Ok(Bytes::from_static(b"ERROR: git checkout failed\n"));
            return;
        }

        let rev = match Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dest)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                yield Ok(Bytes::from(format!("ERROR: git rev-parse: {e:#}\n")));
                return;
            }
        };
        let hash = String::from_utf8_lossy(&rev.stdout).trim().to_string();
        yield Ok(Bytes::from(format!(
            "=== done ===\nchecked out {} @ {}\n",
            branch, hash
        )));
    }
}

fn maybe_create_parent(dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).context("create parent dirs")?;
    }
    Ok(())
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
