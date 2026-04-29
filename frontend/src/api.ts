export type Project = {
  id: string;
  name: string;
  repo_url: string;
  dist_path: string;
  build_branch: string;
  created_at: string;
};

export type Run = {
  id: string;
  project_id: string;
  task_name: string;
  status: string;
  log: string;
  started_at: string | null;
  finished_at: string | null;
};

export type Artifact = {
  id: string;
  project_id: string;
  filename: string;
  rel_path: string;
  bytes: number;
  created_at: string;
};

export type TaskInfo = { name: string };

const base = "/api";

async function jsonFetch<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const headers: HeadersInit = { ...(init?.headers ?? {}) };
  if (init?.body != null && !(headers as Record<string, string>)["Content-Type"]) {
    (headers as Record<string, string>)["Content-Type"] = "application/json";
  }
  const res = await fetch(`${base}${path}`, {
    ...init,
    headers,
  });
  if (!res.ok) {
    const t = await res.text();
    throw new Error(t || res.statusText);
  }
  return res.json() as Promise<T>;
}

export function listProjects() {
  return jsonFetch<Project[]>("/projects");
}

export function createProject(body: {
  name: string;
  repo_url: string;
  dist_path: string;
  build_branch: string;
}) {
  return jsonFetch<Project>("/projects", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function getProject(id: string) {
  return jsonFetch<Project>(`/projects/${encodeURIComponent(id)}`);
}

export function syncProject(id: string) {
  return jsonFetch<{ ok: boolean; log: string }>(
    `/projects/${encodeURIComponent(id)}/sync`,
    { method: "POST" },
  );
}

export function listTasks(id: string) {
  return jsonFetch<TaskInfo[]>(
    `/projects/${encodeURIComponent(id)}/tasks`,
  );
}

export function listRuns(id: string) {
  return jsonFetch<Run[]>(`/projects/${encodeURIComponent(id)}/runs`);
}

export function runTask(id: string, task_name: string) {
  return jsonFetch<{ run_id: string }>(
    `/projects/${encodeURIComponent(id)}/runs`,
    {
      method: "POST",
      body: JSON.stringify({ task_name }),
    },
  );
}

export function getRun(projectId: string, runId: string) {
  return jsonFetch<Run>(
    `/projects/${encodeURIComponent(projectId)}/runs/${encodeURIComponent(runId)}`,
  );
}

export function packageProject(id: string) {
  return jsonFetch<{ artifact_id: string; filename: string; bytes: number }>(
    `/projects/${encodeURIComponent(id)}/package`,
    { method: "POST" },
  );
}

export function listArtifacts(id: string) {
  return jsonFetch<Artifact[]>(
    `/projects/${encodeURIComponent(id)}/artifacts`,
  );
}

export function artifactDownloadUrl(artifactId: string) {
  return `${base}/artifacts/${encodeURIComponent(artifactId)}/download`;
}
