import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Link } from "react-router-dom";
import { createProject, listProjects, type Project } from "../api";

export function ProjectList() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  const [name, setName] = useState("");
  const [repoUrl, setRepoUrl] = useState("");
  const [distPath, setDistPath] = useState("dist");
  const [branch, setBranch] = useState("main");

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      setProjects(await listProjects());
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
      await createProject({ name, repo_url: repoUrl, dist_path: distPath, build_branch: branch });
      setName("");
      setRepoUrl("");
      setShowForm(false);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col px-6 py-10">
      {/* Header */}
      <div className="mb-8 flex items-center justify-between">
        <h1 className="text-xl font-semibold text-text-primary">Projects</h1>
        <button
          type="button"
          onClick={() => setShowForm((v) => !v)}
          className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-surface-0 shadow-sm transition hover:brightness-110"
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path d="M7 1.5V12.5M1.5 7H12.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          New project
        </button>
      </div>

      {/* Create form */}
      {showForm && (
        <form
          onSubmit={onSubmit}
          className="mb-8 rounded-xl border border-border bg-surface-1 p-5"
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <label className="flex flex-col gap-1.5 text-sm">
              <span className="text-text-secondary">Name</span>
              <input
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="rounded-lg border border-border bg-surface-0 px-3 py-2 text-text-primary outline-none transition placeholder:text-text-tertiary focus:border-accent/50 focus:ring-1 focus:ring-accent/30"
                placeholder="My app"
              />
            </label>
            <label className="flex flex-col gap-1.5 text-sm sm:col-span-2">
              <span className="text-text-secondary">Repository URL</span>
              <input
                required
                value={repoUrl}
                onChange={(e) => setRepoUrl(e.target.value)}
                className="rounded-lg border border-border bg-surface-0 px-3 py-2 font-mono text-sm text-text-primary outline-none transition placeholder:text-text-tertiary focus:border-accent/50 focus:ring-1 focus:ring-accent/30"
                placeholder="https://github.com/org/repo.git"
              />
            </label>
            <label className="flex flex-col gap-1.5 text-sm">
              <span className="text-text-secondary">Dist directory</span>
              <input
                required
                value={distPath}
                onChange={(e) => setDistPath(e.target.value)}
                className="rounded-lg border border-border bg-surface-0 px-3 py-2 font-mono text-sm text-text-primary outline-none transition placeholder:text-text-tertiary focus:border-accent/50 focus:ring-1 focus:ring-accent/30"
                placeholder="dist"
              />
            </label>
            <label className="flex flex-col gap-1.5 text-sm">
              <span className="text-text-secondary">Branch</span>
              <input
                required
                value={branch}
                onChange={(e) => setBranch(e.target.value)}
                className="rounded-lg border border-border bg-surface-0 px-3 py-2 font-mono text-sm text-text-primary outline-none transition placeholder:text-text-tertiary focus:border-accent/50 focus:ring-1 focus:ring-accent/30"
                placeholder="main"
              />
            </label>
          </div>
          <div className="mt-5 flex gap-2">
            <button
              type="submit"
              className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-surface-0 transition hover:brightness-110"
            >
              Create
            </button>
            <button
              type="button"
              onClick={() => setShowForm(false)}
              className="rounded-lg px-4 py-2 text-sm text-text-secondary transition hover:bg-surface-3 hover:text-text-primary"
            >
              Cancel
            </button>
          </div>
        </form>
      )}

      {error && (
        <p className="mb-4 rounded-lg border border-danger/20 bg-danger-muted px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}

      {/* Project list */}
      {loading ? (
        <p className="text-sm text-text-tertiary">Loading...</p>
      ) : projects.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center text-center">
          <div className="mb-3 text-4xl opacity-20">CI</div>
          <p className="text-sm text-text-tertiary">No projects yet. Create one to get started.</p>
        </div>
      ) : (
        <div className="space-y-1">
          {projects.map((p) => (
            <Link
              key={p.id}
              to={`/p/${p.id}`}
              className="group flex items-center justify-between rounded-lg px-4 py-3 transition hover:bg-surface-2"
            >
              <div className="flex items-center gap-3">
                <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent-muted text-xs font-semibold text-accent">
                  {p.name.charAt(0).toUpperCase()}
                </div>
                <div>
                  <div className="text-sm font-medium text-text-primary">{p.name}</div>
                  <div className="font-mono text-xs text-text-tertiary">{p.repo_url}</div>
                </div>
              </div>
              <svg
                width="16"
                height="16"
                viewBox="0 0 16 16"
                className="text-text-tertiary opacity-0 transition group-hover:opacity-100"
              >
                <path d="M6 4L10 8L6 12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" fill="none" />
              </svg>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
