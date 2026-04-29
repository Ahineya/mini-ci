mod db;
mod embed;
mod git_ops;
mod models;
mod package_zip;
mod runner;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use axum::body::Body;
use axum::extract::{Path as AxPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use embed::Assets;
use models::{
    ArtifactRow, CreateProject, Project, RunRow, RunTaskBody, TaskInfo,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: Arc<db::Db>,
    data_root: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let data_root = std::env::var("MINICI_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mini-ci")
        });
    std::fs::create_dir_all(&data_root).context("create data root")?;

    let db_path = data_root.join("mini-ci.sqlite");
    let db = Arc::new(db::Db::open(&db_path).context("open database")?);
    let state = AppState { db, data_root };

    let api = Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project).delete(delete_project))
        .route("/projects/{id}/tasks", get(list_tasks))
        .route("/projects/{id}/runs", get(list_runs).post(run_task))
        .route("/projects/{id}/runs/{run_id}", get(get_run).delete(delete_run))
        .route("/projects/{id}/package", post(package_project))
        .route("/projects/{id}/artifacts", get(list_artifacts))
        .route(
            "/artifacts/{artifact_id}/download",
            get(download_artifact),
        );

    let app = Router::new()
        .nest("/api", api)
        .fallback(static_handler)
        .with_state(state);

    let addr: std::net::SocketAddr = "127.0.0.1:8787".parse().unwrap();
    tracing::info!("mini-ci listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn static_handler(req: axum::http::Request<Body>) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            if !path.contains('.') {
                return match Assets::get("index.html") {
                    Some(index) => Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html")
                        .body(Body::from(index.data))
                        .unwrap(),
                    None => not_found(),
                };
            }
            not_found()
        }
    }
}

fn not_found() -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("not found"))
        .unwrap()
}

fn repo_dir(data_root: &Path, project_id: &str) -> PathBuf {
    data_root.join("repos").join(project_id)
}

async fn list_projects(State(st): State<AppState>) -> impl IntoResponse {
    match st.db.list_projects() {
        Ok(p) => Json(p).into_response(),
        Err(e) => {
            tracing::error!("{e:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e:#}"),
            )
                .into_response()
        }
    }
}

async fn get_project(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    match st.db.get_project(&id) {
        Ok(Some(p)) => Json(p).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response(),
    }
}

async fn create_project(
    State(st): State<AppState>,
    axum::Json(body): axum::Json<CreateProject>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let p = Project {
        id: id.clone(),
        name: body.name,
        repo_url: body.repo_url,
        dist_path: body.dist_path,
        build_branch: body.build_branch,
        created_at,
    };
    if let Err(e) = st.db.insert_project(&p) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response();
    }
    Json(p).into_response()
}

async fn delete_project(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    if st.db.get_project(&id).ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let repo = repo_dir(&st.data_root, &id);
    let artifacts = st.data_root.join("artifacts").join(&id);
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&artifacts);
    if let Err(e) = st.db.delete_project(&id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn list_tasks(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    let p = match st.db.get_project(&id) {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e:#}"),
            )
                .into_response()
        }
    };
    let dest = repo_dir(&st.data_root, &id);
    if !dest.join(".git").exists() {
        if let Err(e) = git_ops::ensure_repo_latest(&p.repo_url, &dest, &p.build_branch, None).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("prepare repo: {e:#}"),
            )
                .into_response();
        }
    }
    match git_ops::microci_scripts(&dest) {
        Ok(paths) => {
            let tasks: Vec<TaskInfo> = paths
                .into_iter()
                .filter_map(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| TaskInfo {
                            name: n.to_string(),
                        })
                })
                .collect();
            Json(tasks).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response(),
    }
}

async fn list_runs(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    match st.db.list_runs(&id) {
        Ok(r) => Json(r).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct LogQuery {
    log_offset: Option<usize>,
}

async fn get_run(
    State(st): State<AppState>,
    AxPath((project_id, run_id)): AxPath<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> impl IntoResponse {
    match q.log_offset {
        Some(offset) => match st.db.get_run_log_since(&run_id, offset) {
            Ok(Some((r, total_len))) if r.project_id == project_id => {
                Json(json!({
                    "id": r.id,
                    "project_id": r.project_id,
                    "task_name": r.task_name,
                    "status": r.status,
                    "log": r.log,
                    "log_offset": total_len,
                    "started_at": r.started_at,
                    "finished_at": r.finished_at,
                }))
                .into_response()
            }
            Ok(Some(_)) => StatusCode::NOT_FOUND.into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        },
        None => match st.db.get_run(&run_id) {
            Ok(Some(r)) if r.project_id == project_id => Json(r).into_response(),
            Ok(Some(_)) => StatusCode::NOT_FOUND.into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        },
    }
}

async fn delete_run(
    State(st): State<AppState>,
    AxPath((project_id, run_id)): AxPath<(String, String)>,
) -> impl IntoResponse {
    match st.db.get_run(&run_id) {
        Ok(Some(r)) if r.project_id == project_id => {}
        Ok(Some(_)) | Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
    match st.db.delete_run(&run_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
}

async fn run_task(
    State(st): State<AppState>,
    AxPath(project_id): AxPath<String>,
    axum::Json(body): axum::Json<RunTaskBody>,
) -> impl IntoResponse {
    let project = match st.db.get_project(&project_id) {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e:#}"),
            )
                .into_response()
        }
    };

    let script_name = body.task_name.trim().to_string();
    if script_name.contains('/') || script_name.contains('\\') || script_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "task_name must be a bare filename like build.sh",
        )
            .into_response();
    }

    let run_id = Uuid::new_v4().to_string();
    let run_id_response = run_id.clone();
    let started = chrono::Utc::now();
    let row = RunRow {
        id: run_id.clone(),
        project_id: project_id.clone(),
        task_name: script_name.clone(),
        status: "running".to_string(),
        log: String::new(),
        started_at: Some(started),
        finished_at: None,
    };
    if let Err(e) = st.db.insert_run(&row) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response();
    }

    let db = st.db.clone();
    let repo = repo_dir(&st.data_root, &project_id);

    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let db_log = db.clone();
        let rid = run_id.clone();
        let log_writer = tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                let _ = db_log.append_run_log(&rid, &chunk);
            }
        });

        // 1) Clone or pull — git output goes into the run log
        if let Err(e) = git_ops::ensure_repo_latest(
            &project.repo_url,
            &repo,
            &project.build_branch,
            Some(&tx),
        )
        .await
        {
            let _ = tx.send(format!("ERROR: repository update failed: {e:#}\n"));
            drop(tx);
            let _ = log_writer.await;
            let _ = db.set_run_status(&run_id, "failed", Some(chrono::Utc::now()));
            return;
        }

        // 2) Verify script exists after clone
        let script_path = repo.join(".mini-ci").join(&script_name);
        if !script_path.is_file() {
            let _ = tx.send(format!("ERROR: script not found: .mini-ci/{script_name}\n"));
            drop(tx);
            let _ = log_writer.await;
            let _ = db.set_run_status(&run_id, "failed", Some(chrono::Utc::now()));
            return;
        }

        // 3) Run the build script
        let run_result =
            runner::run_shell_script(&script_path, &repo, tx).await;
        let _ = log_writer.await;

        let exit_msg = match &run_result {
            Ok(code) => format!("\n--- exit code: {code} ---\n"),
            Err(e) => format!("\n--- runner error: {e:#} ---\n"),
        };
        let _ = db.append_run_log(&run_id, &exit_msg);

        let status = match run_result {
            Ok(0) => "success",
            Ok(_) => "failed",
            Err(_) => "failed",
        };
        let finished = chrono::Utc::now();
        let _ = db.set_run_status(&run_id, status, Some(finished));
    });

    Json(json!({ "run_id": run_id_response })).into_response()
}

async fn package_project(
    State(st): State<AppState>,
    AxPath(project_id): AxPath<String>,
) -> impl IntoResponse {
    let p = match st.db.get_project(&project_id) {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e:#}"),
            )
                .into_response()
        }
    };

    let repo = repo_dir(&st.data_root, &project_id);
    let dist = repo.join(&p.dist_path);
    if !dist.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            format!("Dist directory does not exist: {}", dist.display()),
        )
            .into_response();
    }

    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("{}-{}.zip", p.name.replace(' ', "-"), ts);
    let rel_storage = format!("{project_id}/{filename}");
    let out_path =
        package_zip::artifact_paths(&st.data_root, &project_id, &filename);

    match package_zip::zip_directory(&dist, &out_path) {
        Ok(bytes) => {
            let aid = Uuid::new_v4().to_string();
            let row = ArtifactRow {
                id: aid.clone(),
                project_id: project_id.clone(),
                filename: filename.clone(),
                rel_path: rel_storage,
                bytes,
                created_at: chrono::Utc::now(),
            };
            if let Err(e) = st.db.insert_artifact(&row) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("{e:#}"),
                )
                    .into_response();
            }
            Json(json!({
                "artifact_id": aid,
                "filename": filename,
                "bytes": bytes,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("zip: {e:#}"),
        )
            .into_response(),
    }
}

async fn list_artifacts(
    State(st): State<AppState>,
    AxPath(project_id): AxPath<String>,
) -> impl IntoResponse {
    match st.db.list_artifacts(&project_id) {
        Ok(a) => Json(a).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response(),
    }
}

async fn download_artifact(
    State(st): State<AppState>,
    AxPath(artifact_id): AxPath<String>,
) -> impl IntoResponse {
    let row = match st.db.get_artifact(&artifact_id) {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e:#}"),
            )
                .into_response()
        }
    };

    let path = st
        .data_root
        .join("artifacts")
        .join(&row.project_id)
        .join(&row.filename);
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            "artifact file missing on disk",
        )
            .into_response();
    }

    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                "application/zip",
            )
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", row.filename),
            )
            .body(Body::from(bytes))
            .unwrap(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read: {e:#}"),
        )
            .into_response(),
    }
}
