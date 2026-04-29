use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::models::{ArtifactRow, Project, RunMeta, RunRow};

pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create db parent dir")?;
        }
        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(
            r"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                repo_url TEXT NOT NULL,
                dist_path TEXT NOT NULL,
                build_branch TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                task_name TEXT NOT NULL,
                status TEXT NOT NULL,
                log TEXT NOT NULL DEFAULT '',
                started_at INTEGER,
                finished_at INTEGER,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                rel_path TEXT NOT NULL,
                bytes INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_runs_project ON runs(project_id);
            CREATE INDEX IF NOT EXISTS idx_artifacts_project ON artifacts(project_id);
            ",
        )
        .context("migrate")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn insert_project(&self, p: &Project) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO projects (id, name, repo_url, dist_path, build_branch, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                p.id,
                p.name,
                p.repo_url,
                p.dist_path,
                p.build_branch,
                p.created_at.timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, name, repo_url, dist_path, build_branch, created_at FROM projects ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                repo_url: row.get(2)?,
                dist_path: row.get(3)?,
                build_branch: row.get(4)?,
                created_at: chrono::DateTime::from_timestamp(row.get::<_, i64>(5)?, 0)
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, name, repo_url, dist_path, build_branch, created_at FROM projects WHERE id = ?1",
        )?;
        stmt
            .query_row(params![id], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    repo_url: row.get(2)?,
                    dist_path: row.get(3)?,
                    build_branch: row.get(4)?,
                    created_at: chrono::DateTime::from_timestamp(row.get::<_, i64>(5)?, 0)
                        .unwrap_or_else(chrono::Utc::now),
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_run(&self, r: &RunRow) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO runs (id, project_id, task_name, status, log, started_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                r.id,
                r.project_id,
                r.task_name,
                r.status,
                r.log,
                r.started_at.map(|t| t.timestamp()),
                r.finished_at.map(|t| t.timestamp()),
            ],
        )?;
        Ok(())
    }

    pub fn set_run_status(
        &self,
        id: &str,
        status: &str,
        finished_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE runs SET status = ?2, finished_at = ?3 WHERE id = ?1",
            params![id, status, finished_at.map(|t| t.timestamp())],
        )?;
        Ok(())
    }

    pub fn append_run_log(&self, id: &str, chunk: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE runs SET log = log || ?2 WHERE id = ?1",
            params![id, chunk],
        )?;
        Ok(())
    }

    /// Full run row plus `LENGTH(log)` (SQLite character count — must match `get_run_log_since` offsets).
    pub fn get_run_meta(&self, id: &str) -> Result<Option<RunMeta>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, project_id, task_name, status, length(log), started_at, finished_at FROM runs WHERE id = ?1",
        )?;
        stmt
            .query_row(params![id], |row| {
                Ok(RunMeta {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    task_name: row.get(2)?,
                    status: row.get(3)?,
                    log_char_len: row.get::<_, i64>(4)? as usize,
                    started_at: row
                        .get::<_, Option<i64>>(5)?
                        .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                    finished_at: row
                        .get::<_, Option<i64>>(6)?
                        .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<(RunRow, usize)>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, project_id, task_name, status, log, length(log), started_at, finished_at FROM runs WHERE id = ?1",
        )?;
        stmt
            .query_row(params![id], |row| {
                Ok((
                    RunRow {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        task_name: row.get(2)?,
                        status: row.get(3)?,
                        log: row.get(4)?,
                        started_at: row
                            .get::<_, Option<i64>>(6)?
                            .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                        finished_at: row
                            .get::<_, Option<i64>>(7)?
                            .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                    },
                    row.get::<_, i64>(5)? as usize,
                ))
            })
            .optional()
            .map_err(Into::into)
    }

    /// Return run metadata with only the log content starting at byte `offset`.
    /// Also returns `log_offset` = total log length so the caller can request the next chunk.
    pub fn get_run_log_since(&self, id: &str, offset: usize) -> Result<Option<(RunRow, usize)>> {
        let c = self.conn.lock().unwrap();
        // SQLite substr is 1-indexed; length gives total byte count
        let mut stmt = c.prepare(
            "SELECT id, project_id, task_name, status, substr(log, ?2), length(log), started_at, finished_at FROM runs WHERE id = ?1",
        )?;
        stmt
            .query_row(params![id, (offset + 1) as i64], |row| {
                let log_tail: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
                let total_len: usize = row.get::<_, i64>(5)? as usize;
                Ok((
                    RunRow {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        task_name: row.get(2)?,
                        status: row.get(3)?,
                        log: log_tail,
                        started_at: row
                            .get::<_, Option<i64>>(6)?
                            .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                        finished_at: row
                            .get::<_, Option<i64>>(7)?
                            .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                    },
                    total_len,
                ))
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn list_runs(&self, project_id: &str) -> Result<Vec<RunRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, project_id, task_name, status, log, started_at, finished_at FROM runs WHERE project_id = ?1 ORDER BY COALESCE(started_at, 0) DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(RunRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_name: row.get(2)?,
                status: row.get(3)?,
                log: row.get(4)?,
                started_at: row
                    .get::<_, Option<i64>>(5)?
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
                finished_at: row
                    .get::<_, Option<i64>>(6)?
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0)),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_run(&self, id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM runs WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_artifact(&self, a: &ArtifactRow) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO artifacts (id, project_id, filename, rel_path, bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                a.id,
                a.project_id,
                a.filename,
                a.rel_path,
                a.bytes as i64,
                a.created_at.timestamp(),
            ],
        )?;
        Ok(())
    }

    pub fn list_artifacts(&self, project_id: &str) -> Result<Vec<ArtifactRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, project_id, filename, rel_path, bytes, created_at FROM artifacts WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(ArtifactRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                filename: row.get(2)?,
                rel_path: row.get(3)?,
                bytes: row.get::<_, i64>(4)? as u64,
                created_at: chrono::DateTime::from_timestamp(row.get::<_, i64>(5)?, 0)
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_artifact(&self, id: &str) -> Result<Option<ArtifactRow>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, project_id, filename, rel_path, bytes, created_at FROM artifacts WHERE id = ?1",
        )?;
        stmt
            .query_row(params![id], |row| {
                Ok(ArtifactRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    filename: row.get(2)?,
                    rel_path: row.get(3)?,
                    bytes: row.get::<_, i64>(4)? as u64,
                    created_at: chrono::DateTime::from_timestamp(row.get::<_, i64>(5)?, 0)
                        .unwrap_or_else(chrono::Utc::now),
                })
            })
            .optional()
            .map_err(Into::into)
    }
}
