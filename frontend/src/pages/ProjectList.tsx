import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Link } from "react-router-dom";
import {
  createProject,
  listProjects,
  type Project,
} from "../api";

export function ProjectList() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [repoUrl, setRepoUrl] = useState("");
  const [distPath, setDistPath] = useState("dist");
  const [branch, setBranch] = useState("main");

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const data = await listProjects();
      setProjects(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      await createProject({
        name,
        repo_url: repoUrl,
        dist_path: distPath,
        build_branch: branch,
      });
      setName("");
      setRepoUrl("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="space-y-10">
      <section className="rounded-xl border border-zinc-800 bg-zinc-900/40 p-6 shadow-xl shadow-black/30">
        <h2 className="mb-4 text-sm font-medium uppercase tracking-wide text-zinc-400">
          New project
        </h2>
        <form onSubmit={onSubmit} className="grid gap-4 md:grid-cols-2">
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-zinc-400">Name</span>
            <input
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 outline-none ring-emerald-500/40 focus:border-emerald-600 focus:ring-2"
              placeholder="My app"
            />
          </label>
          <label className="flex flex-col gap-1 text-sm md:col-span-2">
            <span className="text-zinc-400">Repository URL</span>
            <input
              required
              value={repoUrl}
              onChange={(e) => setRepoUrl(e.target.value)}
              className="rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-sm outline-none ring-emerald-500/40 focus:border-emerald-600 focus:ring-2"
              placeholder="https://github.com/org/repo.git"
            />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-zinc-400">Dist directory (relative)</span>
            <input
              required
              value={distPath}
              onChange={(e) => setDistPath(e.target.value)}
              className="rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-sm outline-none ring-emerald-500/40 focus:border-emerald-600 focus:ring-2"
              placeholder="dist"
            />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-zinc-400">Build branch</span>
            <input
              required
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              className="rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 font-mono text-sm outline-none ring-emerald-500/40 focus:border-emerald-600 focus:ring-2"
              placeholder="main"
            />
          </label>
          <div className="md:col-span-2">
            <button
              type="submit"
              className="rounded-lg bg-emerald-600 px-4 py-2 text-sm font-medium text-white shadow hover:bg-emerald-500"
            >
              Create project
            </button>
          </div>
        </form>
        {error && (
          <p className="mt-4 rounded-lg border border-red-900/80 bg-red-950/40 px-3 py-2 text-sm text-red-200">
            {error}
          </p>
        )}
      </section>

      <section>
        <h2 className="mb-4 text-sm font-medium uppercase tracking-wide text-zinc-400">
          Projects
        </h2>
        {loading ? (
          <p className="text-zinc-500">Loading…</p>
        ) : projects.length === 0 ? (
          <p className="text-zinc-500">No projects yet.</p>
        ) : (
          <ul className="divide-y divide-zinc-800 rounded-xl border border-zinc-800 bg-zinc-900/30">
            {projects.map((p) => (
              <li key={p.id}>
                <Link
                  to={`/p/${p.id}`}
                  className="flex flex-col gap-1 px-4 py-3 transition hover:bg-zinc-800/50 sm:flex-row sm:items-center sm:justify-between"
                >
                  <span className="font-medium text-white">{p.name}</span>
                  <span className="font-mono text-xs text-zinc-500">{p.repo_url}</span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
