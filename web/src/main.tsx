import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

interface HealthResponse { status: string; }
interface Workspace { workspace_id: string; herdr_workspace_id: string; label: string; cwd: string; revision: number; }
interface WorkspaceListResponse { workspaces: Workspace[]; }
interface PreviewState { preview_session_id: string | null; preview_url: string | null; preview_status: string | null; preview_title?: string | null; }
interface PreviewDiagnostic { kind: string; level: string; message: string; source: string | null; status: number | null; occurred_at: string; }
interface PreviewDiagnosticsResponse { diagnostics: PreviewDiagnostic[]; }
interface WorkspaceEventEnvelope {
  event_type: string;
  payload?: {
    preview_session_id?: string | null;
    preview_url?: string | null;
    preview_status?: string | null;
    preview_title?: string | null;
  };
}
interface ApiError { error?: { code?: string; message?: string }; }

const previewFromEnvelope = (event: WorkspaceEventEnvelope): PreviewState | null => {
  if (
    event.event_type === "workspace.snapshot" ||
    event.event_type === "preview.opened" ||
    event.event_type === "preview.state_updated"
  ) {
    return {
      preview_session_id: event.payload?.preview_session_id ?? null,
      preview_url: event.payload?.preview_url ?? null,
      preview_status: event.payload?.preview_status ?? null,
      preview_title: event.payload?.preview_title ?? null,
    };
  }
  return null;
};

const workspaceSocketUrl = (workspaceId: string) => {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws/v1/workspaces/${workspaceId}`;
};

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
  const [capturing, setCapturing] = useState<string | null>(null);
  const [shots, setShots] = useState<Record<string, string>>({});
  const [diagnostics, setDiagnostics] = useState<Record<string, PreviewDiagnostic[]>>({});

  useEffect(() => {
    Promise.all([api<HealthResponse>("/api/v1/health"), api<WorkspaceListResponse>("/api/v1/workspaces")])
      .then(([healthResponse, workspaceResponse]) => { setHealth(healthResponse); setWorkspaces(workspaceResponse.workspaces); })
      .catch((reason: Error) => setError(reason.message));
  }, []);

  useEffect(() => {
    if (!workspaces?.length) return;
    const sockets = workspaces.map((workspace) => {
      const socket = new WebSocket(workspaceSocketUrl(workspace.workspace_id));
      socket.onmessage = (message) => {
        const event = JSON.parse(message.data) as WorkspaceEventEnvelope;
        if (event.event_type === "resync") {
          api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/state`)
            .then((preview) => setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview })))
            .catch((reason: Error) => setError(reason.message));
          return;
        }
        const preview = previewFromEnvelope(event);
        if (preview) setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview }));
      };
      socket.onerror = () => setError("无法订阅 Preview 状态");
      return socket;
    });
    return () => sockets.forEach((socket) => socket.close());
  }, [workspaces]);

  const openPreview = async (workspace: Workspace) => {
    setOpening(workspace.workspace_id);
    setError(null);
    try {
      const preview = await api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/preview/open`, {
        method: "POST",
        body: JSON.stringify({}),
      });
      setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法读取 Preview 状态");
    } finally { setOpening(null); }
  };

  const captureScreenshot = async (workspace: Workspace) => {
    setCapturing(workspace.workspace_id);
    setError(null);
    try {
      const response = await fetch(`/api/v1/workspaces/${workspace.workspace_id}/preview/screenshot`, { method: "POST" });
      if (!response.ok) {
        const body = await response.json() as ApiError;
        throw new Error(body.error?.message ?? `请求失败：${response.status}`);
      }
      const image = await fetch(`/api/v1/workspaces/${workspace.workspace_id}/preview/screenshot`);
      if (!image.ok) throw new Error(`无法读取截图：${image.status}`);
      const blob = await image.blob();
      const url = URL.createObjectURL(blob);
      setShots((current) => {
        const previous = current[workspace.workspace_id];
        if (previous) URL.revokeObjectURL(previous);
        return { ...current, [workspace.workspace_id]: url };
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法捕获 Preview 截图");
    } finally { setCapturing(null); }
  };

  const loadDiagnostics = async (workspace: Workspace) => {
    setError(null);
    try {
      const response = await api<PreviewDiagnosticsResponse>(`/api/v1/workspaces/${workspace.workspace_id}/preview/diagnostics`);
      setDiagnostics((current) => ({ ...current, [workspace.workspace_id]: response.diagnostics }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法读取 Preview 诊断");
    }
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
        {workspaces?.length === 0 && <div className="empty"><span className="empty-mark">/</span><strong>还没有绑定工作区</strong><p>启动时会从本机 Herdr 同步 workspace。Herdr 没在跑时，这里会是空的。</p></div>}
        {workspaces?.map((workspace) => {
          const preview = previews[workspace.workspace_id];
          return <article className="workspace" key={workspace.workspace_id}>
            <div className="workspace-index">W</div><div className="workspace-main"><h3>{workspace.label}</h3><code>{workspace.cwd}</code><span className="workspace-id">Herdr · {workspace.herdr_workspace_id}</span>{preview && <span className="preview-state">Preview · {preview.preview_status ?? "未打开"}{preview.preview_title ? ` · ${preview.preview_title}` : ""}{preview.preview_url ? ` · ${preview.preview_url}` : ""}</span>}</div>
            <button className="preview-button" type="button" onClick={() => openPreview(workspace)} disabled={opening === workspace.workspace_id}>{opening === workspace.workspace_id ? "打开中..." : "查看 Preview"}</button>
            <button className="preview-button" type="button" onClick={() => captureScreenshot(workspace)} disabled={capturing === workspace.workspace_id}>{capturing === workspace.workspace_id ? "截图中..." : "截图"}</button>
            <button className="preview-button" type="button" onClick={() => loadDiagnostics(workspace)}>诊断</button>
            {shots[workspace.workspace_id] && <img className="preview-shot" alt={`${workspace.label} preview`} src={shots[workspace.workspace_id]} />}
            {diagnostics[workspace.workspace_id]?.length ? <ul className="preview-diagnostics">{diagnostics[workspace.workspace_id].map((item, index) => <li key={`${item.occurred_at}-${index}`}>{item.level} · {item.kind} · {item.message}</li>)}</ul> : null}<div className="revision">REV {workspace.revision}</div>
          </article>;
        })}
      </section>
      <footer><span>Herdr Workbench P0</span><span>localhost · Windows-first</span></footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
