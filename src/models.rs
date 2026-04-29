use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo_url: String,
    pub dist_path: String,
    pub build_branch: String,
    /// When true, periodically fetch the remote and run `auto_run_task` when new commits appear.
    pub auto_run_on_change: bool,
    /// Basename of a `.mini-ci/*.sh` script (e.g. `build.sh`).
    pub auto_run_task: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub repo_url: String,
    pub dist_path: String,
    pub build_branch: String,
    #[serde(default)]
    pub auto_run_on_change: bool,
    #[serde(default)]
    pub auto_run_task: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchProject {
    pub auto_run_on_change: bool,
    pub auto_run_task: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRow {
    pub id: String,
    pub project_id: String,
    pub task_name: String,
    pub status: String,
    pub log: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Run fields without the log body (`length(log)` matches incremental `log_offset` units).
#[derive(Debug, Clone, Serialize)]
pub struct RunMeta {
    pub id: String,
    pub project_id: String,
    pub task_name: String,
    pub status: String,
    pub log_char_len: usize,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RunTaskBody {
    pub task_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRow {
    pub id: String,
    pub project_id: String,
    pub filename: String,
    pub rel_path: String,
    pub bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
