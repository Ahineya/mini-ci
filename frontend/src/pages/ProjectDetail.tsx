import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  artifactDownloadUrl,
  getProject,
  getRun,
  listArtifacts,
  listRuns,
  listTasks,
  packageProject,
  runTask,
  syncProject,
  type Artifact,
  type Project,
  type Run,
  type TaskInfo,
} from "../api";

export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<TaskInfo[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [syncLog, setSyncLog] = useState<string | null>(null);
  const [activeRun, setActiveRun] = useState<Run | null>(null);
  const [pollRunId, setPollRunId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const logRef = useRef<HTMLPreElement>(null);

  const load = useCallback(async () => {
    if (!id) return;
    setError(null);
    const [p, r, a] = await Promise.all([
      getProject(id),
      listRuns(id),
      listArtifacts(id),
    ]);
    setProject(p);
    setRuns(r);
    setArtifacts(a);
    try {
      const t = await listTasks(id);
      setTasks(t);
    } catch {
      setTasks([]);
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!pollRunId || !id) return;
    const t = setInterval(async () => {
      try {
        const r = await getRun(id, pollRunId);
        setActiveRun(r);
        if (r.status !== "running") {
          setPollRunId(null);
          void load();
        }
      } catch {
        setPollRunId(null);
      }
    }, 400);
    return () => clearInterval(t);
  }, [pollRunId, id, load]);

  useEffect(() => {
    if (logRef.current && activeRun?.log) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [activeRun?.log]);

  if (!id) {
    return <p className="text-zinc-500">Missing project id.</p>;
  }

  const projectId = id;

  async function onSync() {
    setError(null);
    setSyncLog(null);
    try {
      const res = await syncProject(projectId);
      setSyncLog(res.log);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onRun(taskName: string) {
    setError(null);
    try {
      const { run_id } = await runTask(projectId, taskName);
      setPollRunId(run_id);
      const initial = await getRun(projectId, run_id);
      setActiveRun(initial);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onPackage() {
    setError(null);
    try {
      await packageProject(projectId);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="space-y-8">
      <div className="flex flex-wrap items-center gap-4">
        <Link to="/" className="text-sm text-emerald-400 hover:text-emerald-300">
          ← Projects
        </Link>
        {project && (
          <h1 className="text-2xl font-semibold text-white">{project.name}</h1>
        )}
      </div>

      {project && (
        <dl className="grid gap-3 rounded-xl border border-zinc-800 bg-zinc-900/40 p-4 text-sm md:grid-cols-2">
          <div>
            <dt className="text-zinc-500">Repo</dt>
            <dd className="font-mono text-zinc-200">{project.repo_url}</dd>
          </div>
          <div>
            <dt className="text-zinc-500">Branch</dt>
            <dd className="font-mono text-zinc-200">{project.build_branch}</dd>
          </div>
          <div>
            <dt className="text-zinc-500">Dist path</dt>
            <dd className="font-mono text-zinc-200">{project.dist_path}</dd>
          </div>
        </dl>
      )}

      {error && (
        <p className="rounded-lg border border-red-900/80 bg-red-950/40 px-3 py-2 text-sm text-red-200">
          {error}
        </p>
      )}

      <section className="flex flex-wrap gap-3">
        <button
          type="button"
          onClick={() => void onSync()}
          className="rounded-lg bg-zinc-800 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-700"
        >
          Clone / sync branch
        </button>
        <button
          type="button"
          onClick={() => void onPackage()}
          className="rounded-lg bg-emerald-700 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-600"
        >
          Zip dist → artifact
        </button>
      </section>

      {syncLog && (
        <pre className="max-h-40 overflow-auto rounded-lg border border-zinc-800 bg-black/40 p-3 font-mono text-xs text-zinc-300">
          {syncLog}
        </pre>
      )}

      <section>
        <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-400">
          Tasks (.microci/*.sh)
        </h2>
        {tasks.length === 0 ? (
          <p className="text-sm text-zinc-500">
            No scripts yet. Sync the repo, then add shell scripts under{" "}
            <code className="rounded bg-zinc-800 px-1">.microci/</code>.
          </p>
        ) : (
          <ul className="flex flex-wrap gap-2">
            {tasks.map((t) => (
              <li key={t.name}>
                <button
                  type="button"
                  onClick={() => void onRun(t.name)}
                  className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-1.5 font-mono text-sm text-zinc-100 hover:border-emerald-600"
                >
                  Run {t.name}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="grid gap-6 lg:grid-cols-2">
        <div>
          <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-400">
            Live log
          </h2>
          <pre
            ref={logRef}
            className="h-72 overflow-auto rounded-xl border border-zinc-800 bg-black/50 p-4 font-mono text-xs leading-relaxed text-emerald-100/90"
          >
            {activeRun
              ? `${activeRun.task_name} — ${activeRun.status}\n\n${activeRun.log || ""}`
              : "Run a task to stream logs here."}
          </pre>
        </div>
        <div>
          <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-400">
            Recent runs
          </h2>
          <ul className="max-h-72 space-y-2 overflow-auto rounded-xl border border-zinc-800 bg-zinc-900/30 p-2 text-sm">
            {runs.map((r) => (
              <li key={r.id}>
                <button
                  type="button"
                  className="w-full rounded-lg px-2 py-2 text-left hover:bg-zinc-800"
                  onClick={() => {
                    setActiveRun(r);
                    setPollRunId(null);
                  }}
                >
                  <div className="flex justify-between gap-2">
                    <span className="font-mono text-zinc-200">{r.task_name}</span>
                    <span
                      className={
                        r.status === "success"
                          ? "text-emerald-400"
                          : r.status === "failed"
                            ? "text-red-400"
                            : "text-amber-300"
                      }
                    >
                      {r.status}
                    </span>
                  </div>
                </button>
              </li>
            ))}
            {runs.length === 0 && (
              <li className="px-2 py-4 text-zinc-500">No runs yet.</li>
            )}
          </ul>
        </div>
      </section>

      <section>
        <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-zinc-400">
          Artifacts
        </h2>
        {artifacts.length === 0 ? (
          <p className="text-sm text-zinc-500">
            Package creates a zip of the dist directory under the data directory.
          </p>
        ) : (
          <ul className="divide-y divide-zinc-800 rounded-xl border border-zinc-800">
            {artifacts.map((a) => (
              <li
                key={a.id}
                className="flex flex-wrap items-center justify-between gap-2 px-4 py-3"
              >
                <span className="font-mono text-sm text-zinc-200">{a.filename}</span>
                <span className="text-xs text-zinc-500">
                  {(a.bytes / 1024).toFixed(1)} KB
                </span>
                <a
                  href={artifactDownloadUrl(a.id)}
                  className="text-sm text-emerald-400 hover:text-emerald-300"
                >
                  Download
                </a>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
