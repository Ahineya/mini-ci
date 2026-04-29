import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { LogStore } from "../LogStore";
import { VirtualLog } from "../VirtualLog";
import {
  artifactDownloadUrl,
  deleteProject,
  deleteRun,
  getProject,
  getRun,
  listArtifacts,
  listRuns,
  listTasks,
  packageProject,
  runTask,
  type Artifact,
  type Project,
  type Run,
  type TaskInfo,
} from "../api";

/* ------------------------------------------------------------------ */
/*  Status badge                                                       */
/* ------------------------------------------------------------------ */
function StatusDot({ status }: { status: string }) {
  const colors: Record<string, string> = {
    success: "bg-accent",
    failed: "bg-danger",
    running: "bg-warning animate-pulse",
  };
  return <span className={`inline-block h-2 w-2 rounded-full ${colors[status] ?? "bg-text-tertiary"}`} />;
}

/* ------------------------------------------------------------------ */
/*  Tabs                                                               */
/* ------------------------------------------------------------------ */
type Tab = "runs" | "artifacts";

/* ------------------------------------------------------------------ */
/*  Main component                                                     */
/* ------------------------------------------------------------------ */
export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<TaskInfo[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);

  const [activeRun, setActiveRun] = useState<Run | null>(null);
  const [pollRunId, setPollRunId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("runs");

  const logOffsetRef = useRef<number>(0);
  const logStore = useMemo(() => new LogStore(), []);

  /* ---- data loading ---- */
  const load = useCallback(async () => {
    if (!id) return;
    setError(null);
    const [p, r, a] = await Promise.all([getProject(id), listRuns(id), listArtifacts(id)]);
    setProject(p);
    setRuns(r);
    setArtifacts(a);
    try {
      setTasks(await listTasks(id));
    } catch {
      setTasks([]);
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  /* ---- polling ---- */
  useEffect(() => {
    if (!pollRunId || !id) return;
    const t = setInterval(async () => {
      try {
        const r = await getRun(id, pollRunId, logOffsetRef.current);
        logStore.append(r.log);
        logOffsetRef.current = r.log_offset;
        setActiveRun((prev) =>
          prev && prev.id === r.id ? { ...prev, status: r.status, finished_at: r.finished_at } : prev,
        );
        if (r.status !== "running") {
          setPollRunId(null);
          void load();
        }
      } catch {
        setPollRunId(null);
      }
    }, 400);
    return () => clearInterval(t);
  }, [pollRunId, id, load, logStore]);

  if (!id) return <p className="p-8 text-text-tertiary">Missing project id.</p>;
  const projectId = id;

  /* ---- actions ---- */
  async function onRemoveProject() {
    if (!project) return;
    if (!window.confirm(`Delete "${project.name}"? This cannot be undone.`)) return;
    setError(null);
    try {
      await deleteProject(projectId);
      navigate("/");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onRun(taskName: string) {
    setError(null);
    setTab("runs");
    try {
      const { run_id } = await runTask(projectId, taskName);
      logOffsetRef.current = 0;
      logStore.clear();
      setPollRunId(run_id);
      const initial = await getRun(projectId, run_id);
      logStore.append(initial.log);
      logOffsetRef.current = initial.log_offset;
      setActiveRun(initial);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function selectRun(runId: string) {
    setPollRunId(null);
    try {
      const full = await getRun(projectId, runId);
      logStore.load(full.log);
      logOffsetRef.current = full.log_offset;
      setActiveRun(full);
      if (full.status === "running") {
        setPollRunId(runId);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onDeleteRun(e: React.MouseEvent, runId: string) {
    e.stopPropagation();
    try {
      await deleteRun(projectId, runId);
      if (activeRun?.id === runId) {
        setActiveRun(null);
        setPollRunId(null);
      }
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function onPackage() {
    setError(null);
    try {
      await packageProject(projectId);
      setTab("artifacts");
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  /* ---------------------------------------------------------------- */
  /*  Render                                                           */
  /* ---------------------------------------------------------------- */
  const tabClass = (t: Tab) =>
    `px-3 py-1.5 text-sm rounded-md transition cursor-pointer ${
      tab === t
        ? "bg-surface-3 text-text-primary font-medium"
        : "text-text-secondary hover:text-text-primary hover:bg-surface-2"
    }`;

  return (
    <div className="flex h-full flex-col">
      {/* ---- Top section: project info + actions ---- */}
      <div className="shrink-0 border-b border-border bg-surface-1/50 px-6 py-4">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="mb-1 flex items-center gap-2">
              <Link to="/" className="text-xs text-text-tertiary transition hover:text-accent">
                Projects
              </Link>
              <span className="text-text-tertiary">/</span>
              <h1 className="truncate text-base font-semibold text-text-primary">
                {project?.name ?? "..."}
              </h1>
            </div>
            {project && (
              <div className="flex flex-wrap items-center gap-4 text-xs text-text-tertiary">
                <span className="font-mono">{project.repo_url}</span>
                <span className="rounded bg-surface-3 px-1.5 py-0.5">{project.build_branch}</span>
                <span>dist: <span className="font-mono">{project.dist_path}</span></span>
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              onClick={() => void onPackage()}
              className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-surface-0 transition hover:brightness-110"
            >
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                <rect x="2" y="3" width="10" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
                <path d="M5 3V1.5h4V3" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
                <path d="M5.5 7h3" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
              </svg>
              Package
            </button>
            <button
              type="button"
              onClick={() => void onRemoveProject()}
              className="rounded-lg border border-border p-1.5 text-text-tertiary transition hover:border-danger/30 hover:bg-danger-muted hover:text-danger"
              title="Delete project"
            >
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                <path d="M2.5 3.5h9M5.5 3.5V2.5a1 1 0 011-1h1a1 1 0 011 1v1M4 5.5l.5 6h5l.5-6" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </div>

        {error && (
          <p className="mt-3 rounded-lg border border-danger/20 bg-danger-muted px-3 py-2 text-sm text-danger">
            {error}
          </p>
        )}
      </div>

      {/* ---- Main area: sidebar + log ---- */}
      <div className="flex min-h-0 flex-1">
        {/* Left sidebar */}
        <aside className="flex w-72 shrink-0 flex-col border-r border-border bg-surface-1/30">
          {/* Tasks */}
          <div className="border-b border-border p-4">
            <h3 className="mb-2 text-xs font-medium uppercase tracking-wider text-text-tertiary">
              Tasks
            </h3>
            {tasks.length === 0 ? (
              <p className="text-xs text-text-tertiary">No .mini-ci scripts found.</p>
            ) : (
              <div className="space-y-1">
                {tasks.map((t) => (
                  <button
                    key={t.name}
                    type="button"
                    onClick={() => void onRun(t.name)}
                    className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm transition hover:bg-surface-3"
                  >
                    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" className="shrink-0 text-accent">
                      <path d="M3 1.5L10 6L3 10.5V1.5Z" fill="currentColor" />
                    </svg>
                    <span className="truncate font-mono text-xs text-text-primary">{t.name}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Tabs */}
          <div className="flex gap-1 border-b border-border px-4 py-2">
            <button type="button" onClick={() => setTab("runs")} className={tabClass("runs")}>
              Runs
            </button>
            <button type="button" onClick={() => setTab("artifacts")} className={tabClass("artifacts")}>
              Artifacts
            </button>
          </div>

          {/* Tab content */}
          <div className="min-h-0 flex-1 overflow-y-auto">
            {tab === "runs" && (
              <div className="p-2">
                {runs.length === 0 ? (
                  <p className="px-2 py-4 text-xs text-text-tertiary">No runs yet.</p>
                ) : (
                  <div className="space-y-0.5">
                    {runs.map((r) => {
                      // Use live status from activeRun if this is the active run
                      const status = activeRun?.id === r.id ? activeRun.status : r.status;
                      return (
                      <div
                        key={r.id}
                        role="button"
                        tabIndex={0}
                        onClick={() => void selectRun(r.id)}
                        onKeyDown={(e) => { if (e.key === "Enter") void selectRun(r.id); }}
                        className={`group/run flex w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition ${
                          activeRun?.id === r.id
                            ? "bg-surface-3"
                            : "hover:bg-surface-2"
                        }`}
                      >
                        <StatusDot status={status} />
                        <div className="min-w-0 flex-1">
                          <div className="truncate font-mono text-xs text-text-primary">
                            {r.task_name}
                          </div>
                          <div className="text-[10px] text-text-tertiary">
                            {r.started_at
                              ? new Date(r.started_at).toLocaleString(undefined, {
                                  month: "short",
                                  day: "numeric",
                                  hour: "2-digit",
                                  minute: "2-digit",
                                })
                              : "pending"}
                          </div>
                        </div>
                        <span
                          className={`shrink-0 text-[10px] font-medium ${
                            status === "success"
                              ? "text-accent"
                              : status === "failed"
                                ? "text-danger"
                                : "text-warning"
                          }`}
                        >
                          {status}
                        </span>
                        <button
                          type="button"
                          onClick={(e) => void onDeleteRun(e, r.id)}
                          className="shrink-0 rounded p-0.5 text-text-tertiary opacity-0 transition hover:bg-danger-muted hover:text-danger group-hover/run:opacity-100"
                          title="Delete run"
                        >
                          <svg width="12" height="12" viewBox="0 0 14 14" fill="none">
                            <path d="M3.5 3.5L10.5 10.5M10.5 3.5L3.5 10.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                          </svg>
                        </button>
                      </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            {tab === "artifacts" && (
              <div className="p-2">
                {artifacts.length === 0 ? (
                  <p className="px-2 py-4 text-xs text-text-tertiary">
                    No artifacts. Use Package to create one.
                  </p>
                ) : (
                  <div className="space-y-0.5">
                    {artifacts.map((a) => (
                      <a
                        key={a.id}
                        href={artifactDownloadUrl(a.id)}
                        className="flex items-center justify-between rounded-md px-2.5 py-2 transition hover:bg-surface-2"
                      >
                        <div className="min-w-0">
                          <div className="truncate font-mono text-xs text-text-primary">{a.filename}</div>
                          <div className="text-[10px] text-text-tertiary">
                            {(a.bytes / 1024).toFixed(1)} KB
                          </div>
                        </div>
                        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="shrink-0 text-text-tertiary">
                          <path d="M7 2v7.5M3.5 7L7 10.5 10.5 7M2.5 12.5h9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                        </svg>
                      </a>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </aside>

        {/* Log panel — fills remaining space */}
        <div className="flex min-w-0 flex-1 flex-col bg-surface-0">
          {/* Log header */}
          <div className="flex shrink-0 items-center gap-3 border-b border-border px-5 py-2.5">
            {activeRun ? (
              <>
                <StatusDot status={activeRun.status} />
                <span className="font-mono text-sm text-text-primary">{activeRun.task_name}</span>
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
                    activeRun.status === "success"
                      ? "bg-accent-muted text-accent"
                      : activeRun.status === "failed"
                        ? "bg-danger-muted text-danger"
                        : "bg-warning/10 text-warning"
                  }`}
                >
                  {activeRun.status}
                </span>
              </>
            ) : (
              <span className="text-sm text-text-tertiary">Select a run or start a task to view logs</span>
            )}
          </div>

          {/* Log output */}
          <VirtualLog store={logStore} />
        </div>
      </div>
    </div>
  );
}
