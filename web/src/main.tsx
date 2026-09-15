import { StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import {
  PAIR_STORAGE,
  attachHerdrSocket,
  createHerdrTerminal,
  isTouch,
  readPair,
} from "./herdrTerminal";

interface ApiError { error?: { code?: string; message?: string }; }
interface LanStatus { enabled: boolean; listen: string; urls: string[]; pairing_code?: string | null; can_manage?: boolean; }

const isPairingError = (message: string) => message.toLowerCase().includes("pairing code");

const pairHeaders = (): Record<string, string> => {
  const pair = readPair();
  return pair ? { "x-workbench-pair": pair } : {};
};

const withPair = (path: string) => {
  const pair = readPair();
  if (!pair) return path;
  return `${path}${path.includes("?") ? "&" : "?"}pair=${encodeURIComponent(pair)}`;
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
  const hostRef = useRef<HTMLDivElement | null>(null);
  const sessionRef = useRef<ReturnType<typeof attachHerdrSocket> | null>(null);
  const [lan, setLan] = useState<LanStatus | null>(null);
  const [pairDraft, setPairDraft] = useState(readPair() ?? "");
  const [ready, setReady] = useState(false);
  const [needsPair, setNeedsPair] = useState(false);
  const [lanBusy, setLanBusy] = useState(false);
  const [showLan, setShowLan] = useState(true);
  const [showTouch, setShowTouch] = useState(true);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">("connecting");
  const [error, setError] = useState<string | null>(null);
  const touch = isTouch();

  const loadLan = () => {
    api<LanStatus>("/api/v1/lan")
      .then((next) => {
        setLan(next);
        setNeedsPair(false);
        setReady(true);
        setError(null);
      })
      .catch((reason: Error) => {
        if (isPairingError(reason.message)) setNeedsPair(true);
        setReady(true);
        setError(reason.message);
      });
  };

  useEffect(() => {
    loadLan();
  }, []);

  useEffect(() => {
    if (!ready || needsPair || !hostRef.current) return;
    const { terminal, fit } = createHerdrTerminal(hostRef.current);
    const session = attachHerdrSocket(hostRef.current, terminal, fit, setStatus);
    sessionRef.current = session;
    const onVisual = () => {
      if (!touch) return;
      const viewport = window.visualViewport;
      if (!viewport || !hostRef.current) return;
      hostRef.current.style.height = `${Math.max(120, viewport.height - 88)}px`;
      fit.fit();
    };
    window.visualViewport?.addEventListener("resize", onVisual);
    return () => {
      window.visualViewport?.removeEventListener("resize", onVisual);
      session.dispose();
      sessionRef.current = null;
    };
  }, [ready, needsPair]);

  const savePair = () => {
    const value = pairDraft.trim();
    if (!value) return;
    localStorage.setItem(PAIR_STORAGE, value);
    setNeedsPair(false);
    setError(null);
    loadLan();
  };

  const toggleLan = async (enabled: boolean) => {
    setLanBusy(true);
    try {
      const next = await api<LanStatus>(enabled ? "/api/v1/lan/enable" : "/api/v1/lan/disable", { method: "POST" });
      setLan(next);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "无法切换局域网");
    } finally {
      setLanBusy(false);
    }
  };

  return (
    <main className="map-shell">
      {lan?.can_manage && (
        <header className={`map-lan ${showLan ? "" : "collapsed"}`}>
          <button className="preview-button" type="button" onClick={() => setShowLan((current) => !current)}>
            {showLan ? "收起" : "LAN"}
          </button>
          {showLan && (
            <>
              <span className={`status ${status === "open" ? "ready" : ""}`}>
                <span className="status-dot" />
                {status === "open" ? "Herdr attached" : status === "connecting" ? "Connecting" : "Detached"}
              </span>
              {lan.can_manage && (
                <button className="preview-button" type="button" disabled={lanBusy} onClick={() => toggleLan(!lan.enabled)}>
                  {lanBusy ? "切换中..." : lan.enabled ? "关闭局域网" : "开启局域网"}
                </button>
              )}
              {lan.enabled && lan.pairing_code ? <strong className="pair-code">{lan.pairing_code}</strong> : null}
              {lan.enabled && lan.urls.map((url) => <code key={url}>{url}</code>)}
            </>
          )}
        </header>
      )}
      {error && <div className="notice error">{error}</div>}
      {needsPair && (
        <section className="lan-panel">
          <p className="eyebrow">LAN PAIRING</p>
          <h2>输入本机显示的配对码</h2>
          <p>配对之后中间就是终端里那个 Herdr。</p>
          <div className="lan-row">
            <input className="preview-url" value={pairDraft} placeholder="123456" inputMode="numeric" onChange={(event) => setPairDraft(event.target.value)} />
            <button className="preview-button" type="button" onClick={savePair}>连接</button>
          </div>
        </section>
      )}
      {!needsPair && <div className="herdr-term" ref={hostRef} />}
      {touch && !needsPair && (
        <footer className={`map-touch ${showTouch ? "" : "collapsed"}`}>
          <button className="preview-button" type="button" onClick={() => setShowTouch((current) => !current)}>
            {showTouch ? "收起" : "快捷"}
          </button>
          {showTouch && (
            <>
              <button className="preview-button" type="button" onClick={() => hostRef.current?.querySelector("textarea")?.focus()}>键盘</button>
              <button className="preview-button" type="button" onClick={() => void sessionRef.current?.paste()}>粘贴</button>
              <button className="preview-button" type="button" onClick={() => sessionRef.current?.sendPrefix()}>prefix</button>
            </>
          )}
        </footer>
      )}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
