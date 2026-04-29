import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { LogStore } from "../LogStore";
import { VirtualLog } from "../VirtualLog";
import {
  artifactDownloadUrl,
  deleteArtifact,
  deleteProject,
  deleteRun,
  getProject,
  getRepoStatus,
  patchProject,
  getRun,
  listArtifacts,
  listRuns,
  listTasks,
  repoInitStreamUrl,
  runLogStreamUrl,
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
  /** null = initial; true = need clone UX; false = repo ready */
  const [needsRepoClone, setNeedsRepoClone] = useState<boolean | null>(null);
  const [repoSetupActive, setRepoSetupActive] = useState(false);
  const [draftAutoRun, setDraftAutoRun] = useState(false);
  const [draftAutoTask, setDraftAutoTask] = useState("");
  const [settingsSaving, setSettingsSaving] = useState(false);

  const logOffsetRef = useRef<number>(0);
  /** Prevents duplicate SSE `done` events from forcing Artifacts tab again after user switches back to Runs. */
  const artifactTabHandledForRunId = useRef<string | null>(null);
  const logStore = useMemo(() => new LogStore(), []);

  /** Newest first (matches API); tie-break for stable order. */
  const artifactsSorted = useMemo(() => {
    return [...artifacts].sort((a, b) => {
      const tb = new Date(b.created_at).getTime();
      const ta = new Date(a.created_at).getTime();
      if (tb !== ta) return tb - ta;
      return b.id.localeCompare(a.id);
    });
  }, [artifacts]);

  /* ---- data loading ---- */
  const load = useCallback(async () => {
    if (!id) return;
    setError(null);
    const [rs, p, r, a] = await Promise.all([
      getRepoStatus(id),
      getProject(id),
      listRuns(id),
      listArtifacts(id),
    ]);
    setNeedsRepoClone(!rs.ready);
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

  useEffect(() => {
    if (!project) return;
    setDraftAutoRun(project.auto_run_on_change ?? false);
    setDraftAutoTask(project.auto_run_task ?? "");
  }, [project]);

  /** Reset artifact-tab guard when a new live log session starts (manual or auto-run). */
  useEffect(() => {
    if (pollRunId) {
      artifactTabHandledForRunId.current = null;
    }
  }, [pollRunId]);

  /** Poll runs/artifacts so server-started jobs (e.g. auto-run) show up without reload. */
  useEffect(() => {
    if (!id) return;
    const tick = async () => {
      try {
        const [r, a] = await Promise.all([listRuns(id), listArtifacts(id)]);
        setRuns(r);
        setArtifacts(a);
      } catch {
        /* ignore */
      }
    };
    const iv = setInterval(() => {
      void tick();
    }, 4000);
    void tick();
    return () => clearInterval(iv);
  }, [id]);

  /* ---- first clone: stream git output into the main log ---- */
  useEffect(() => {
    if (!id || needsRepoClone !== true) return;
    logStore.clear();
    setRepoSetupActive(true);
    const es = new EventSource(repoInitStreamUrl(id));
    let streamDone = false;

    const finish = () => {
      if (streamDone) return;
      streamDone = true;
      es.close();
      setRepoSetupActive(false);
      setNeedsRepoClone(false);
      void load();
    };

    es.onmessage = (ev: MessageEvent) => {
      logStore.append(ev.data as string);
    };

    es.addEventListener("end", finish);

    // When the stream closes after a successful clone, many browsers fire `error` and the
    // native EventSource would reconnect to the same URL — each hit sees `.git` and logs
    // "repository already present" in a tight loop. Closing here disables auto-reconnect.
    es.onerror = () => {
      es.close();
      if (!streamDone) {
        finish();
      }
    };

    return () => {
      streamDone = true;
      es.close();
      setRepoSetupActive(false);
    };
  }, [id, needsRepoClone, logStore, load]);

  /* ---- live log: SSE (server wakes on each append; no 400ms polling) ---- */
  useEffect(() => {
    if (!pollRunId || !id) return;
    const es = new EventSource(runLogStreamUrl(id, pollRunId, 0));

    es.onmessage = (ev: MessageEvent) => {
      logStore.append(ev.data as string);
    };

    es.addEventListener("done", (ev: Event) => {
      const me = ev as MessageEvent<string>;
      try {
        const d = JSON.parse(me.data) as Run;
        setActiveRun({
          id: d.id,
          project_id: d.project_id,
          task_name: d.task_name,
          status: d.status,
          log: "",
          started_at: d.started_at,
          finished_at: d.finished_at,
        });
        if (d.status === "success") {
          if (artifactTabHandledForRunId.current !== d.id) {
            artifactTabHandledForRunId.current = d.id;
            setTab("artifacts");
          }
        }
        void load();
      } catch {
        /* ignore malformed */
      }
    });

    es.addEventListener("end", () => {
      es.close();
      setPollRunId(null);
      void load();
    });

    return () => {
      es.close();
    };
  }, [pollRunId, id, logStore, load]);

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
      const started = new Date().toISOString();
      setActiveRun({
        id: run_id,
        project_id: projectId,
        task_name: taskName,
        status: "running",
        log: "",
        started_at: started,
        finished_at: null,
      });
      setPollRunId(run_id);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function selectRun(runId: string) {
    setPollRunId(null);
    try {
      const meta = await getRun(projectId, runId, undefined, { omitLog: true });
      if (meta.status === "running") {
        logStore.clear();
        logOffsetRef.current = meta.log_offset ?? 0;
        setActiveRun({ ...meta, log: "" });
        setPollRunId(runId);
        return;
      }
      const full = await getRun(projectId, runId);
      logStore.load(full.log);
      logOffsetRef.current = full.log_offset ?? [...full.log].length;
      setActiveRun(full);
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

  async function saveAutoRunSettings() {
    if (!project || !id) return;
    if (draftAutoRun && !draftAutoTask.trim()) {
      setError("Select a task script when auto-run is enabled.");
      return;
    }
    setError(null);
    setSettingsSaving(true);
    try {
      const updated = await patchProject(project.id, {
        auto_run_on_change: draftAutoRun,
        auto_run_task: draftAutoTask.trim(),
      });
      setProject(updated);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSettingsSaving(false);
    }
  }

  async function onDeleteArtifact(e: React.MouseEvent, artifactId: string) {
    e.preventDefault();
    e.stopPropagation();
    if (!window.confirm("Delete this artifact zip? This cannot be undone.")) return;
    try {
      await deleteArtifact(artifactId);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
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

        {project && (
          <details className="group mt-3 rounded-md border border-border bg-surface-1/60">
            <summary className="cursor-pointer list-none px-3 py-2 text-xs text-text-secondary transition hover:bg-surface-2/80 [&::-webkit-details-marker]:hidden">
              <span className="flex items-center justify-between gap-2">
                <span className="font-medium text-text-tertiary">
                  Auto-run{" "}
                  <span className="font-normal text-text-secondary">
                    {draftAutoRun && draftAutoTask.trim()
                      ? `· ${draftAutoTask.trim()}`
                      : draftAutoRun
                        ? "· (pick script)"
                        : "· off"}
                  </span>
                </span>
                <svg
                  className="h-4 w-4 shrink-0 text-text-tertiary transition group-open:rotate-180"
                  viewBox="0 0 14 14"
                  fill="none"
                  aria-hidden
                >
                  <path d="M3.5 5.25L7 8.75L10.5 5.25" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </span>
            </summary>
            <div className="space-y-2 border-t border-border px-3 pb-3 pt-2">
              <p className="text-[11px] leading-snug text-text-tertiary">
                Polls remote ~15s; on new commits runs the chosen script like a manual build.
              </p>
              <label className="flex cursor-pointer items-center gap-2 text-xs text-text-primary">
                <input
                  type="checkbox"
                  checked={draftAutoRun}
                  onChange={(e) => setDraftAutoRun(e.target.checked)}
                  className="rounded border-border"
                />
                Enable
              </label>
              <div className="flex flex-wrap items-end gap-2">
                <label className="flex min-w-[140px] flex-1 flex-col gap-1 text-[11px] text-text-tertiary">
                  Task
                  <select
                    value={draftAutoTask}
                    onChange={(e) => setDraftAutoTask(e.target.value)}
                    disabled={!draftAutoRun}
                    className="rounded border border-border bg-surface-0 px-2 py-1.5 font-mono text-xs text-text-primary outline-none transition disabled:opacity-50"
                  >
                    <option value="">— script —</option>
                    {tasks.map((t) => (
                      <option key={t.name} value={t.name}>
                        {t.name}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  type="button"
                  disabled={
                    settingsSaving || (draftAutoRun && !draftAutoTask.trim())
                  }
                  onClick={() => void saveAutoRunSettings()}
                  className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-surface-0 transition hover:brightness-110 disabled:opacity-40"
                >
                  {settingsSaving ? "…" : "Save"}
                </button>
              </div>
              {tasks.length === 0 && draftAutoRun && (
                <p className="text-[11px] text-warning">Clone repo first so tasks appear.</p>
              )}
            </div>
          </details>
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
                    No artifacts yet. A successful task run creates a zip of the dist folder (see run log).
                  </p>
                ) : (
                  <div className="space-y-0.5">
                    {artifactsSorted.map((a, i) => (
                      <div
                        key={a.id}
                        className="group/art flex items-stretch gap-0.5 rounded-md transition hover:bg-surface-2"
                      >
                        <a
                          href={artifactDownloadUrl(a.id)}
                          className="flex min-w-0 flex-1 items-center justify-between px-2.5 py-2"
                        >
                          <div className="min-w-0">
                            <div className="flex min-w-0 items-center gap-2">
                              {i === 0 && (
                                <span className="shrink-0 rounded bg-accent-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-accent">
                                  Latest
                                </span>
                              )}
                              <span className="truncate font-mono text-xs text-text-primary">{a.filename}</span>
                            </div>
                            <div className="text-[10px] text-text-tertiary">
                              {(a.bytes / 1024).toFixed(1)} KB
                            </div>
                          </div>
                          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="ml-2 shrink-0 text-text-tertiary">
                            <path d="M7 2v7.5M3.5 7L7 10.5 10.5 7M2.5 12.5h9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                          </svg>
                        </a>
                        <button
                          type="button"
                          onClick={(e) => void onDeleteArtifact(e, a.id)}
                          className="shrink-0 rounded-md px-2 text-text-tertiary opacity-0 transition hover:bg-danger-muted hover:text-danger group-hover/art:opacity-100"
                          title="Delete artifact"
                        >
                          <svg width="12" height="12" viewBox="0 0 14 14" fill="none">
                            <path d="M3.5 3.5L10.5 10.5M10.5 3.5L3.5 10.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                          </svg>
                        </button>
                      </div>
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
            ) : repoSetupActive ? (
              <span className="text-sm text-warning">Setting up repository (git clone)…</span>
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
