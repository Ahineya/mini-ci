mod db;
mod embed;
mod git_ops;
mod models;
mod package_zip;
mod runner;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use async_stream::stream;
use axum::body::Body;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::Json;
use axum::Router;
use embed::Assets;
use models::{
    ArtifactRow, CreateProject, PatchProject, Project, RunRow, RunTaskBody, TaskInfo,
};
use serde_json::json;
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time::{interval, MissedTickBehavior};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
    name = "mini-ci",
    version = env!("CARGO_PKG_VERSION"),
    about = "Single-binary mini CI server with web UI"
)]
struct Cli {
    /// Listen address (localhost only by default; use 0.0.0.0 for all interfaces / LAN access)
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,

    #[arg(long, default_value_t = 8787)]
    port: u16,

    /// Data directory for SQLite, cloned repos, and artifacts (default: ~/.mini-ci, or MINICI_DATA)
    #[arg(long, value_name = "PATH")]
    dir: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    db: Arc<db::Db>,
    data_root: PathBuf,
    /// Wake SSE log streams when a run appends to its log (removed when the run finishes).
    run_notifies: Arc<RwLock<HashMap<String, Arc<Notify>>>>,
}

fn version_only_invocation() -> bool {
    let mut args = std::env::args_os();
    args.next(); // argv[0]
    match (args.next(), args.next()) {
        (Some(a), None) => {
            let s = a.to_string_lossy();
            s == "--version" || s == "-V"
        }
        _ => false,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if version_only_invocation() {
        println!("mini-ci {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let Cli { host, port, dir } = Cli::parse();

    let data_root = resolve_data_root(dir);
    std::fs::create_dir_all(&data_root).context("create data root")?;

    let db_path = data_root.join("mini-ci.sqlite");
    let db = Arc::new(db::Db::open(&db_path).context("open database")?);
    tracing::info!(
        data_root = %data_root.display(),
        "mini-ci data directory"
    );

    let state = AppState {
        db,
        data_root,
        run_notifies: Arc::new(RwLock::new(HashMap::new())),
    };

    tokio::spawn(repo_poll_loop(state.clone()));

    let api = Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project)
                .delete(delete_project)
                .patch(patch_project),
        )
        .route("/projects/{id}/repo/status", get(repo_status))
        .route("/projects/{id}/repo/init/stream", get(repo_init_stream))
        .route("/projects/{id}/tasks", get(list_tasks))
        .route("/projects/{id}/runs", get(list_runs).post(run_task))
        .route(
            "/projects/{id}/runs/{run_id}/log/stream",
            get(run_log_sse),
        )
        .route("/projects/{id}/runs/{run_id}", get(get_run).delete(delete_run))
        .route("/projects/{id}/artifacts", get(list_artifacts))
        .route(
            "/artifacts/{artifact_id}/download",
            get(download_artifact),
        )
        .route("/artifacts/{artifact_id}", delete(delete_artifact_entry));

    let app = Router::new()
        .nest("/api", api)
        .fallback(static_handler)
        .with_state(state);

    let addr = SocketAddr::new(host, port);
    tracing::info!("mini-ci listening on http://{}", addr);
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

fn resolve_data_root(cli_dir: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_dir {
        return p;
    }
    std::env::var("MINICI_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mini-ci")
        })
}

fn repo_dir(data_root: &Path, project_id: &str) -> PathBuf {
    data_root.join("repos").join(project_id)
}

/// Zip `project.dist_path` under the repo clone and insert an artifact row after a successful run.
/// Returns `Ok(None)` if that path is not a directory (build did not produce output there).
/// Zipping runs in `spawn_blocking` so large trees don't stall the async runtime (SSE still wakes from other notifies).
async fn try_package_dist(
    db: &db::Db,
    data_root: &Path,
    project: &Project,
    project_id: &str,
) -> anyhow::Result<Option<(String, u64, String)>> {
    let repo = repo_dir(data_root, project_id);
    let dist = repo.join(&project.dist_path);
    if !dist.is_dir() {
        return Ok(None);
    }

    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("{}-{}.zip", project.name.replace(' ', "-"), ts);
    let rel_storage = format!("{project_id}/{filename}");
    let out_path = package_zip::artifact_paths(data_root, project_id, &filename);

    let dist_blocking = dist.clone();
    let out_blocking = out_path.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        package_zip::zip_directory(&dist_blocking, &out_blocking)
    })
    .await
    .context("zip task join")??;

    let aid = Uuid::new_v4().to_string();
    let row = ArtifactRow {
        id: aid.clone(),
        project_id: project_id.to_string(),
        filename: filename.clone(),
        rel_path: rel_storage,
        bytes,
        created_at: chrono::Utc::now(),
    };
    db.insert_artifact(&row).context("insert artifact")?;
    Ok(Some((filename, bytes, aid)))
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

async fn patch_project(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    axum::Json(body): axum::Json<PatchProject>,
) -> impl IntoResponse {
    if st.db.get_project(&id).ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if body.auto_run_on_change && body.auto_run_task.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "auto_run_task is required when auto_run_on_change is enabled",
        )
            .into_response();
    }
    if let Err(e) = st.db.patch_project(&id, &body) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response();
    }
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
    if body.auto_run_on_change && body.auto_run_task.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "auto_run_task is required when auto_run_on_change is enabled",
        )
            .into_response();
    }
    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let p = Project {
        id: id.clone(),
        name: body.name,
        repo_url: body.repo_url,
        dist_path: body.dist_path,
        build_branch: body.build_branch,
        auto_run_on_change: body.auto_run_on_change,
        auto_run_task: body.auto_run_task.trim().to_string(),
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

async fn repo_status(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    if st.db.get_project(&id).ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dest = repo_dir(&st.data_root, &id);
    let ready = dest.join(".git").exists();
    Json(json!({ "ready": ready })).into_response()
}

/// First-time clone / fetch logs for the main UI (tasks poll uses [`list_tasks`] after clone).
async fn repo_init_stream(
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

    let stream = stream! {
        if dest.join(".git").exists() {
            yield Ok::<Event, Infallible>(Event::default().data(
                "=== repository already present ===\n",
            ));
            yield Ok::<Event, Infallible>(Event::default().event("end").data(""));
            return;
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let repo_url = p.repo_url.clone();
        let branch = p.build_branch.clone();
        let dest_clone = dest.clone();
        let task = tokio::spawn(async move {
            match git_ops::ensure_repo_latest(&repo_url, &dest_clone, &branch, Some(&tx)).await {
                Ok(()) => {}
                Err(e) => {
                    let _ = tx.send(format!("ERROR: {e:#}\n"));
                }
            }
            drop(tx);
        });

        while let Some(chunk) = rx.recv().await {
            yield Ok::<Event, Infallible>(Event::default().data(chunk));
        }
        let _ = task.await;
        yield Ok::<Event, Infallible>(Event::default().event("end").data(""));
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
        .into_response()
}

async fn list_tasks(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
) -> impl IntoResponse {
    if st.db.get_project(&id).ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dest = repo_dir(&st.data_root, &id);
    if !dest.join(".git").exists() {
        return Json(Vec::<TaskInfo>::new()).into_response();
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
    omit_log: Option<bool>,
}

#[derive(serde::Deserialize)]
struct SseFromQuery {
    from: Option<usize>,
}

async fn finalize_run_notifier(
    notify: &Notify,
    run_notifies: &RwLock<HashMap<String, Arc<Notify>>>,
    run_id: &str,
) {
    notify.notify_waiters();
    let mut m = run_notifies.write().await;
    m.remove(run_id);
}

async fn get_run(
    State(st): State<AppState>,
    AxPath((project_id, run_id)): AxPath<(String, String)>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> impl IntoResponse {
    if q.omit_log == Some(true) {
        return match st.db.get_run_meta(&run_id) {
            Ok(Some(m)) if m.project_id == project_id => Json(json!({
                "id": m.id,
                "project_id": m.project_id,
                "task_name": m.task_name,
                "status": m.status,
                "log": "",
                "log_offset": m.log_char_len,
                "started_at": m.started_at,
                "finished_at": m.finished_at,
            }))
            .into_response(),
            Ok(Some(_)) | Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        };
    }
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
            Ok(Some((r, log_offset))) if r.project_id == project_id => Json(json!({
                "id": r.id,
                "project_id": r.project_id,
                "task_name": r.task_name,
                "status": r.status,
                "log": r.log,
                "log_offset": log_offset,
                "started_at": r.started_at,
                "finished_at": r.finished_at,
            }))
            .into_response(),
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
        Ok(Some((r, _))) if r.project_id == project_id => {}
        Ok(Some(_)) | Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
    match st.db.delete_run(&run_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
}

/// Live run log via SSE: one `data` event per DB delta (each wake / poll), not per 256KiB slice —
/// same LAN / localhost CI shouldn’t simulate slow streaming over tiny frames.
async fn run_log_sse(
    State(st): State<AppState>,
    AxPath((project_id, run_id)): AxPath<(String, String)>,
    Query(q): Query<SseFromQuery>,
) -> impl IntoResponse {
    let from = q.from.unwrap_or(0);
    let db = st.db.clone();
    let run_notifies = st.run_notifies.clone();

    let stream = stream! {
        let mut offset = from;
        // `done` once when row becomes non-running (script exit); packaging log comes on later wakes.
        let mut sent_done = false;
        loop {
            let parsed = match db.get_run_log_since(&run_id, offset) {
                Ok(Some((r, total_len))) => {
                    if r.project_id != project_id {
                        yield Ok::<Event, Infallible>(Event::default().event("error").data("run not found"));
                        break;
                    }
                    (r, total_len)
                }
                Ok(None) => {
                    yield Ok::<Event, Infallible>(Event::default().event("error").data("run not found"));
                    break;
                }
                Err(e) => {
                    yield Ok::<Event, Infallible>(Event::default().event("error").data(format!("{e:#}")));
                    break;
                }
            };
            let (r, total_len) = parsed;
            let terminal = r.status != "running";

            if terminal && !sent_done {
                let payload = json!({
                    "id": r.id,
                    "project_id": r.project_id,
                    "task_name": r.task_name,
                    "status": r.status,
                    "log_offset": total_len,
                    "started_at": r.started_at,
                    "finished_at": r.finished_at,
                });
                yield Ok::<Event, Infallible>(Event::default().event("done").data(payload.to_string()));
                sent_done = true;
            }

            let log_empty = r.log.is_empty();
            if !log_empty {
                yield Ok::<Event, Infallible>(Event::default().data(r.log));
            }
            offset = total_len;

            // Close only when terminal + done was sent + no bytes left in this delta (log drained for now).
            // More log may arrive from packaging — keep the connection open until then.
            if terminal && sent_done && log_empty {
                yield Ok::<Event, Infallible>(Event::default().event("end").data(""));
                break;
            }

            let n = run_notifies.read().await.get(&run_id).cloned();
            match n {
                Some(ref notify) => {
                    notify.notified().await;
                }
                None => {
                    if terminal && sent_done {
                        yield Ok::<Event, Infallible>(Event::default().event("end").data(""));
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(16)).await;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

async fn repo_poll_loop(st: AppState) {
    let mut ticker = interval(Duration::from_secs(15));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if let Err(e) = repo_poll_once(&st).await {
            tracing::warn!(error = %e, "repo poll tick");
        }
    }
}

async fn repo_poll_once(st: &AppState) -> anyhow::Result<()> {
    let projects = st.db.list_projects()?;
    for p in projects {
        if !p.auto_run_on_change || p.auto_run_task.trim().is_empty() {
            continue;
        }
        let repo = repo_dir(&st.data_root, &p.id);
        if !repo.join(".git").is_dir() {
            continue;
        }
        if st.db.project_has_running_run(&p.id)? {
            continue;
        }
        let task = p.auto_run_task.trim().to_string();
        match git_ops::remote_has_new_commits(&p.repo_url, &repo, &p.build_branch).await {
            Ok(true) => {
                tracing::info!(
                    project_id = %p.id,
                    task = %task,
                    "poll: new commits on remote, starting auto-run"
                );
                match spawn_project_run(
                    st,
                    p.clone(),
                    task,
                    Some("=== Auto-run: new commits on remote ===\n".to_string()),
                )
                .await
                {
                    Ok(run_id) => {
                        tracing::debug!(run_id = %run_id, "auto-run started");
                    }
                    Err(e) => tracing::warn!(error = %e, "auto-run failed to start"),
                }
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(project_id = %p.id, error = %e, "poll: git check failed"),
        }
    }
    Ok(())
}

async fn spawn_project_run(
    st: &AppState,
    project: Project,
    script_name: String,
    preamble: Option<String>,
) -> Result<String, String> {
    let script_name = script_name.trim().to_string();
    if script_name.contains('/') || script_name.contains('\\') || script_name.is_empty() {
        return Err("task_name must be a bare filename like build.sh".into());
    }
    let project_id = project.id.clone();
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
        return Err(format!("{e:#}"));
    }

    let notify = Arc::new(Notify::new());
    {
        let mut map = st.run_notifies.write().await;
        map.insert(run_id.clone(), notify.clone());
    }

    let db = st.db.clone();
    let data_root = st.data_root.clone();
    let repo = repo_dir(&st.data_root, &project_id);
    let run_notifies = st.run_notifies.clone();
    let notify_bg = notify.clone();

    tokio::spawn(async move {
        run_task_background(
            db,
            data_root,
            run_notifies,
            project,
            project_id,
            run_id,
            script_name,
            notify_bg,
            preamble,
            repo,
        )
        .await;
    });

    Ok(run_id_response)
}

async fn run_task_background(
    db: Arc<db::Db>,
    data_root: PathBuf,
    run_notifies: Arc<RwLock<HashMap<String, Arc<Notify>>>>,
    project: Project,
    project_id: String,
    run_id: String,
    script_name: String,
    notify: Arc<Notify>,
    preamble: Option<String>,
    repo: PathBuf,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(pre) = preamble {
        let _ = tx.send(pre);
    }
    let db_log = db.clone();
    let rid = run_id.clone();
    let n = notify.clone();
    let log_writer = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let _ = db_log.append_run_log(&rid, &chunk);
            n.notify_waiters();
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
        let fin = chrono::Utc::now();
        let _ = db.set_run_status(&run_id, "failed", Some(fin));
        finalize_run_notifier(&notify, &run_notifies, &run_id).await;
        return;
    }

    // 2) Verify script exists after clone
    let script_path = repo.join(".mini-ci").join(&script_name);
    if !script_path.is_file() {
        let _ = tx.send(format!("ERROR: script not found: .mini-ci/{script_name}\n"));
        drop(tx);
        let _ = log_writer.await;
        let fin = chrono::Utc::now();
        let _ = db.set_run_status(&run_id, "failed", Some(fin));
        finalize_run_notifier(&notify, &run_notifies, &run_id).await;
        return;
    }

    // 3) Run the build script
    let run_result = runner::run_shell_script(&script_path, &repo, tx).await;
    let _ = log_writer.await;

    let exit_msg = match &run_result {
        Ok(code) => format!("\n--- exit code: {code} ---\n"),
        Err(e) => format!("\n--- runner error: {e:#} ---\n"),
    };
    let _ = db.append_run_log(&run_id, &exit_msg);
    notify.notify_waiters();

    // Terminal status as soon as the .sh exits — NOT after packaging. SSE `done` keys off this row.
    let status = match &run_result {
        Ok(0) => "success",
        Ok(_) => "failed",
        Err(_) => "failed",
    };
    let finished = chrono::Utc::now();
    let _ = db.set_run_status(&run_id, status, Some(finished));
    notify.notify_waiters();

    if matches!(&run_result, Ok(0)) {
        let _ = db.append_run_log(&run_id, "\n=== packaging dist… ===\n");
        notify.notify_waiters();

        match try_package_dist(&db, &data_root, &project, &project_id).await {
            Ok(Some((name, bytes, _))) => {
                let _ = db.append_run_log(
                    &run_id,
                    &format!("\n=== artifact: {name} ({bytes} bytes) ===\n"),
                );
                notify.notify_waiters();
            }
            Ok(None) => {
                let _ = db.append_run_log(
                    &run_id,
                    &format!(
                        "\n=== artifact skipped: `{}` is not a directory (build must create it under the project dist path) ===\n",
                        project.dist_path
                    ),
                );
                notify.notify_waiters();
            }
            Err(e) => {
                let _ = db.append_run_log(
                    &run_id,
                    &format!("\n=== artifact zip failed: {e:#} ===\n"),
                );
                notify.notify_waiters();
            }
        }
    }

    finalize_run_notifier(&notify, &run_notifies, &run_id).await;
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

    match spawn_project_run(&st, project, body.task_name, None).await {
        Ok(run_id) => Json(json!({ "run_id": run_id })).into_response(),
        Err(msg) => {
            if msg.contains("task_name must be") {
                (StatusCode::BAD_REQUEST, msg).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
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

async fn delete_artifact_entry(
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

    if let Err(e) = st.db.delete_artifact(&artifact_id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e:#}"),
        )
            .into_response();
    }

    let path = st
        .data_root
        .join("artifacts")
        .join(&row.project_id)
        .join(&row.filename);
    if path.is_file() {
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to remove artifact file after DB delete"
            );
        }
    }

    StatusCode::NO_CONTENT.into_response()
}
