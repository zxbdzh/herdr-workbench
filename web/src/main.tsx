import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { nextWorkspaceSocketAction } from "./workspaceSocket";
import { nextWorkspaceListRefreshMs } from "./workspaceList";

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
interface LanStatus { enabled: boolean; listen: string; urls: string[]; pairing_code?: string | null; can_manage?: boolean; }

const PAIR_STORAGE = "herdr-workbench-pair";
const isPairingError = (message: string) => message.toLowerCase().includes("pairing code");

const readPair = () => {
  const query = new URLSearchParams(window.location.search).get("pair");
  if (query) {
    sessionStorage.setItem(PAIR_STORAGE, query);
    return query;
  }
  return sessionStorage.getItem(PAIR_STORAGE);
};

const pairHeaders = (): Record<string, string> => {
  const pair = readPair();
  return pair ? { "x-workbench-pair": pair } : {};
};

const withPair = (path: string) => {
  const pair = readPair();
  if (!pair) return path;
  return `${path}${path.includes("?") ? "&" : "?"}pair=${encodeURIComponent(pair)}`;
};

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
  return withPair(`${protocol}//${window.location.host}/ws/v1/workspaces/${workspaceId}`);
};

const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(withPair(path), {
    headers: { "content-type": "application/json", ...pairHeaders(), ...init?.headers },
    ...init,
  });
  const raw = await response.text();
  let body: T & ApiError;
  try {
    body = JSON.parse(raw) as T & ApiError;
  } catch {
    throw new Error("Workbench API 没有返回 JSON");
  }
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
  const [sending, setSending] = useState<string | null>(null);
  const [shots, setShots] = useState<Record<string, string>>({});
  const [diagnostics, setDiagnostics] = useState<Record<string, PreviewDiagnostic[]>>({});
  const [sent, setSent] = useState<Record<string, string>>({});
  const [urls, setUrls] = useState<Record<string, string>>({});
  const [lan, setLan] = useState<LanStatus | null>(null);
  const [pairDraft, setPairDraft] = useState(readPair() ?? "");
  const [needsPair, setNeedsPair] = useState(false);
  const [lanBusy, setLanBusy] = useState(false);

  useEffect(() => {
    let stopped = false;
    let timer: number | undefined;
    const load = () => {
      Promise.all([api<HealthResponse>("/api/v1/health"), api<WorkspaceListResponse>("/api/v1/workspaces")])
        .then(([healthResponse, workspaceResponse]) => {
          if (stopped) return;
          setHealth(healthResponse);
          setWorkspaces(workspaceResponse.workspaces);
          setNeedsPair(false);
          setError(null);
          timer = window.setTimeout(load, nextWorkspaceListRefreshMs(workspaceResponse.workspaces.length));
        })
        .catch((reason: Error) => {
          if (stopped) return;
          if (isPairingError(reason.message)) setNeedsPair(true);
          setError(reason.message);
          timer = window.setTimeout(load, nextWorkspaceListRefreshMs(0));
        });
    };
    load();
    return () => {
      stopped = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (!workspaces?.length) return;
    const subscriptions = workspaces.map((workspace) => {
      let socket: WebSocket | null = null;
      let timer: number | undefined;
      let stopped = false;

      const refresh = () => {
        api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/state`)
          .then((preview) => setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview })))
          .catch((reason: Error) => setError(reason.message));
      };

      const connect = () => {
        if (stopped) return;
        if (timer !== undefined) {
          window.clearTimeout(timer);
          timer = undefined;
        }
        const previous = socket;
        const next = new WebSocket(workspaceSocketUrl(workspace.workspace_id));
        socket = next;
        if (previous && previous !== next) {
          previous.onclose = null;
          previous.close();
        }
        next.onmessage = (message) => {
          const event = JSON.parse(message.data) as WorkspaceEventEnvelope;
          if (event.event_type === "resync") {
            if (nextWorkspaceSocketAction("resync", stopped)?.type === "query-state") {
              refresh();
            }
            return;
          }
          const preview = previewFromEnvelope(event);
          if (preview) setPreviews((current) => ({ ...current, [workspace.workspace_id]: preview }));
        };
        next.onerror = () => {
          if (!stopped && socket === next) setError("无法订阅 Preview 状态");
        };
        next.onclose = () => {
          if (stopped || socket !== next) return;
          const action = nextWorkspaceSocketAction("close", false);
          if (action?.type !== "reconnect") return;
          refresh();
          if (timer !== undefined) window.clearTimeout(timer);
          timer = window.setTimeout(connect, action.delayMs);
        };
      };

      connect();
      return () => {
        stopped = true;
        if (timer !== undefined) window.clearTimeout(timer);
        socket?.close();
      };
    });
    return () => subscriptions.forEach((stop) => stop());
  }, [workspaces]);

  useEffect(() => {
    api<LanStatus>("/api/v1/lan")
      .then(setLan)
      .catch((reason: Error) => {
        if (isPairingError(reason.message)) setNeedsPair(true);
      });
  }, []);

  const savePair = () => {
    const pair = pairDraft.trim();
    if (!pair) return;
    sessionStorage.setItem(PAIR_STORAGE, pair);
    setNeedsPair(false);
    window.location.reload();
  };

  const toggleLan = async (enable: boolean) => {
    setLanBusy(true);
    setError(null);
    try {
      const status = await api<LanStatus>(enable ? "/api/v1/lan/enable" : "/api/v1/lan/disable", { method: "POST" });
      setLan(status);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法切换局域网");
    } finally {
      setLanBusy(false);
    }
  };

  const previewUrl = (workspace: Workspace) => {
    const value = urls[workspace.workspace_id]?.trim();
    return value ? value : undefined;
  };

  const openPreview = async (workspace: Workspace) => {
    setOpening(workspace.workspace_id);
    setError(null);
    try {
      const preview = await api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/preview/open`, {
        method: "POST",
        body: JSON.stringify({ url: previewUrl(workspace) }),
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
      await api<PreviewState>(`/api/v1/workspaces/${workspace.workspace_id}/preview/open`, {
        method: "POST",
        body: JSON.stringify({ url: previewUrl(workspace) }),
      });
      const response = await fetch(withPair(`/api/v1/workspaces/${workspace.workspace_id}/preview/screenshot`), {
        method: "POST",
        headers: pairHeaders(),
      });
      if (!response.ok) {
        const body = await response.json() as ApiError;
        throw new Error(body.error?.message ?? `请求失败：${response.status}`);
      }
      const image = await fetch(withPair(`/api/v1/workspaces/${workspace.workspace_id}/preview/screenshot`), {
        headers: pairHeaders(),
      });
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

  const sendContext = async (workspace: Workspace) => {
    setSending(workspace.workspace_id);
    setError(null);
    try {
      const receipt = await api<{ pane_id: string; agent: string; accepted: boolean }>(
        `/api/v1/workspaces/${workspace.workspace_id}/context/send`,
        { method: "POST", body: JSON.stringify({}) },
      );
      setSent((current) => ({
        ...current,
        [workspace.workspace_id]: `已发给 ${receipt.agent} · ${receipt.pane_id}`,
      }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法发给当前 Agent");
    } finally {
      setSending(null);
    }
  };

  return (
    <main className="shell">
      <header className="topbar">
        <div><p className="eyebrow">HERDR WORKBENCH / WINDOWS HOST</p><h1>工作区控制台</h1></div>
        <div className={`status ${health?.status === "ready" ? "ready" : ""}`}><span className="status-dot" />{health?.status === "ready" ? "Host ready" : "Connecting"}</div>
      </header>
      {error && <div className="notice error">{error}</div>}
      {needsPair && (
        <section className="lan-panel">
          <p className="eyebrow">LAN PAIRING</p>
          <h2>输入本机控制台显示的配对码</h2>
          <p>手机和另一台电脑打开同一套页面后，先填 6 位配对码，才能看工作区和截图。</p>
          <div className="lan-row">
            <input className="preview-url" value={pairDraft} placeholder="123456" inputMode="numeric" onChange={(event) => setPairDraft(event.target.value)} />
            <button className="preview-button" type="button" onClick={savePair}>连接</button>
          </div>
        </section>
      )}
      {lan && (
        <section className="lan-panel">
          <p className="eyebrow">LAN ACCESS</p>
          <h2>{lan.enabled ? "局域网已开启" : "默认只听本机"}</h2>
          <p>{lan.enabled ? "手机或另一台电脑打开下面的地址，输入配对码后看同一套 Preview 和截图。Windows 防火墙如果拦了，放行 17321。" : "点开启后会再听 0.0.0.0:17321，并显示局域网地址和一次性配对码。文件浏览下一刀再做。"}</p>
          {lan.can_manage && (
            <div className="lan-row">
              <button className="preview-button" type="button" disabled={lanBusy} onClick={() => toggleLan(!lan.enabled)}>{lanBusy ? "切换中..." : lan.enabled ? "关闭局域网" : "开启局域网"}</button>
              {lan.enabled && lan.pairing_code ? <strong className="pair-code">{lan.pairing_code}</strong> : null}
            </div>
          )}
          {lan.enabled && lan.urls.length > 0 && (
            <ul className="lan-urls">{lan.urls.map((url) => <li key={url}><code>{url}</code></li>)}</ul>
          )}
        </section>
      )}
      <section className="summary-grid" aria-label="Host summary">
        <article className="metric"><span className="metric-label">HOST STATUS</span><strong>{health?.status ?? "--"}</strong></article>
        <article className="metric"><span className="metric-label">WORKSPACES</span><strong>{workspaces?.length ?? "--"}</strong></article>
        <article className="metric"><span className="metric-label">PREVIEW</span><strong className="muted">{Object.keys(previews).length ? "Connected" : "Ready to inspect"}</strong></article>
      </section>
      <section className="section-heading"><div><p className="eyebrow">WORKSPACE REGISTRY</p><h2>已绑定工作区</h2></div><span className="count">{workspaces?.length ?? 0} 个</span></section>
      <section className="workspace-list" aria-live="polite">
        {workspaces === null && <div className="empty">正在读取 workspace 状态...</div>}
        {workspaces?.length === 0 && <div className="empty"><span className="empty-mark">/</span><strong>还没有绑定工作区</strong><p>正在从本机 Herdr 同步。如果 Herdr 刚打开，列表会在几秒内出现。</p></div>}
        {workspaces?.map((workspace) => {
          const preview = previews[workspace.workspace_id];
          return <article className="workspace" key={workspace.workspace_id}>
            <div className="workspace-index">W</div><div className="workspace-main"><h3>{workspace.label}</h3><code>{workspace.cwd}</code><span className="workspace-id">Herdr · {workspace.herdr_workspace_id}</span>{preview && <span className="preview-state">Preview · {preview.preview_status ?? "未打开"}{preview.preview_title ? ` · ${preview.preview_title}` : ""}{preview.preview_url ? ` · ${preview.preview_url}` : ""}</span>}</div>
            <input className="preview-url" value={urls[workspace.workspace_id] ?? preview?.preview_url ?? ""} placeholder="http://127.0.0.1:3000" onChange={(event) => setUrls((current) => ({ ...current, [workspace.workspace_id]: event.target.value }))} />
            <button className="preview-button" type="button" onClick={() => openPreview(workspace)} disabled={opening === workspace.workspace_id}>{opening === workspace.workspace_id ? "打开中..." : "查看 Preview"}</button>
            <button className="preview-button" type="button" onClick={() => captureScreenshot(workspace)} disabled={capturing === workspace.workspace_id}>{capturing === workspace.workspace_id ? "截图中..." : "截图"}</button>
            <button className="preview-button" type="button" onClick={() => loadDiagnostics(workspace)}>诊断</button>
            <button className="preview-button" type="button" onClick={() => sendContext(workspace)} disabled={sending === workspace.workspace_id}>{sending === workspace.workspace_id ? "发送中..." : "发给 Agent"}</button>
            {shots[workspace.workspace_id] && <img className="preview-shot" alt={`${workspace.label} preview`} src={shots[workspace.workspace_id]} />}
            {sent[workspace.workspace_id] && <span className="preview-state">{sent[workspace.workspace_id]}</span>}
            {diagnostics[workspace.workspace_id]?.length ? <ul className="preview-diagnostics">{diagnostics[workspace.workspace_id].map((item, index) => <li key={`${item.occurred_at}-${index}`}>{item.level} · {item.kind} · {item.message}</li>)}</ul> : null}<div className="revision">REV {workspace.revision}</div>
          </article>;
        })}
      </section>
      <footer><span>Herdr Workbench 0.2</span><span>{lan?.enabled ? "LAN · same UI" : "localhost · Windows-first"}</span></footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
