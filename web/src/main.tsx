import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

interface HealthResponse { status: string; }
interface Workspace { workspace_id: string; herdr_workspace_id: string; label: string; cwd: string; revision: number; }
interface WorkspaceListResponse { workspaces: Workspace[]; }
interface PreviewState { preview_session_id: string | null; preview_url: string | null; preview_status: string | null; }
interface ApiError { error?: { code?: string; message?: string }; }

const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(path, { headers: { "content-type": "application/json", ...init?.headers }, ...init });
  const body = await response.json() as T & ApiError;
  if (!response.ok) throw new Error(body.error?.message ?? `请求失败：${response.status}`);
  return body as T;
};

function App() {
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[] | null>(null);
  const [previews, setPreviews] = useState<Record<string, PreviewState>>({});
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api<HealthResponse>("/api/v1/health"), api<WorkspaceListResponse>("/api/v1/workspaces")])
      .then(([healthResponse, workspaceResponse]) => { setHealth(healthResponse); setWorkspaces(workspaceResponse.workspaces); })
      .catch((reason: Error) => setError(reason.message));
  }, []);

  const openPreview = async (workspace: Workspace) => {
    setOpening(workspace.workspace_id);
    setError(null);
    try {
      const preview = await api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/state`);
      setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法读取 Preview 状态");
    } finally { setOpening(null); }
  };

  return (
    <main className="shell">
      <header className="topbar">
        <div><p className="eyebrow">HERDR WORKBENCH / WINDOWS HOST</p><h1>工作区控制台</h1></div>
        <div className={`status ${health?.status === "ready" ? "ready" : ""}`}><span className="status-dot" />{health?.status === "ready" ? "Host ready" : "Connecting"}</div>
      </header>
      {error && <div className="notice error">{error}</div>}
      <section className="summary-grid" aria-label="Host summary">
        <article className="metric"><span className="metric-label">HOST STATUS</span><strong>{health?.status ?? "--"}</strong></article>
        <article className="metric"><span className="metric-label">WORKSPACES</span><strong>{workspaces?.length ?? "--"}</strong></article>
        <article className="metric"><span className="metric-label">PREVIEW</span><strong className="muted">{Object.keys(previews).length ? "Connected" : "Ready to inspect"}</strong></article>
      </section>
      <section className="section-heading"><div><p className="eyebrow">WORKSPACE REGISTRY</p><h2>已绑定工作区</h2></div><span className="count">{workspaces?.length ?? 0} 个</span></section>
      <section className="workspace-list" aria-live="polite">
        {workspaces === null && <div className="empty">正在读取 workspace 状态...</div>}
        {workspaces?.length === 0 && <div className="empty"><span className="empty-mark">/</span><strong>还没有绑定工作区</strong><p>从 Herdr plugin action 打开 Workbench 后，workspace 会显示在这里。</p></div>}
        {workspaces?.map((workspace) => {
          const preview = previews[workspace.workspace_id];
          return <article className="workspace" key={workspace.workspace_id}>
            <div className="workspace-index">W</div><div className="workspace-main"><h3>{workspace.label}</h3><code>{workspace.cwd}</code><span className="workspace-id">Herdr · {workspace.herdr_workspace_id}</span>{preview && <span className="preview-state">Preview · {preview.preview_status ?? "未打开"}{preview.preview_url ? ` · ${preview.preview_url}` : ""}</span>}</div>
            <button className="preview-button" type="button" onClick={() => openPreview(workspace)} disabled={opening === workspace.workspace_id}>{opening === workspace.workspace_id ? "读取中..." : "查看 Preview"}</button><div className="revision">REV {workspace.revision}</div>
          </article>;
        })}
      </section>
      <footer><span>Herdr Workbench P0</span><span>localhost · Windows-first</span></footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
