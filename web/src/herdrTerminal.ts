import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

export const PAIR_STORAGE = "herdr-workbench-pair";

export const readPair = () => {
  const query = new URLSearchParams(window.location.search).get("pair");
  if (query) {
    localStorage.setItem(PAIR_STORAGE, query);
    return query;
  }
  return localStorage.getItem(PAIR_STORAGE);
};

export const withPair = (path: string) => {
  const pair = readPair();
  if (!pair) return path;
  return `${path}${path.includes("?") ? "&" : "?"}pair=${encodeURIComponent(pair)}`;
};

export const herdrSocketUrl = (cols?: number, rows?: number) => {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  let url = withPair(`${protocol}//${window.location.host}/ws/v1/herdr`);
  if (cols && rows) {
    url += `${url.includes("?") ? "&" : "?"}cols=${cols}&rows=${rows}`;
  }
  return url;
};

export const isTouch = () => window.matchMedia("(pointer: coarse)").matches;

export const createHerdrTerminal = (host: HTMLElement) => {
  const terminal = new Terminal({
    cursorBlink: false,
    convertEol: false,
    windowsMode: true,
    fontFamily: "Consolas, 'Cascadia Mono', monospace",
    fontSize: 14,
    letterSpacing: 0,
    lineHeight: 1,
    theme: { background: "#080b10", foreground: "#e6edf3", cursor: "#55d68a" },
    scrollback: 0,
    overviewRulerWidth: 0,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(host);
  fit.fit();
  return { terminal, fit };
};

const sendMouse = (target: EventTarget, type: string, button: number, clientX: number, clientY: number) => {
  target.dispatchEvent(
    new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      view: window,
      button,
      buttons: button === 0 ? 1 : button === 2 ? 2 : 0,
      clientX,
      clientY,
    }),
  );
};

export const attachHerdrSocket = (
  host: HTMLElement,
  terminal: Terminal,
  fit: FitAddon,
  onStatus: (status: "connecting" | "open" | "closed") => void,
) => {
  let socket: WebSocket | null = null;
  let closed = false;
  let composing = false;
  let lastCommitted: string | null = null;
  let longPress: number | undefined;
  let lastTouch: { x: number; y: number } | null = null;
  let twoFingerOrigin: { y: number } | null = null;

  let lastCols = 0;
  let lastRows = 0;
  let resizeTimer: number | undefined;

  const sendResize = () => {
    fit.fit();
    const cols = terminal.cols;
    const rows = terminal.rows;
    if (cols === lastCols && rows === lastRows) return;
    lastCols = cols;
    lastRows = rows;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: "resize", cols, rows }));
    }
  };

  const connect = () => {
    if (closed) return;
    onStatus("connecting");
    fit.fit();
    lastCols = terminal.cols;
    lastRows = terminal.rows;
    socket = new WebSocket(herdrSocketUrl(terminal.cols, terminal.rows));
    socket.binaryType = "arraybuffer";
    socket.onopen = () => {
      onStatus("open");
    };
    socket.onmessage = (event) => {
      if (typeof event.data === "string") return;
      terminal.write(new Uint8Array(event.data as ArrayBuffer));
    };
    socket.onclose = () => {
      onStatus("closed");
      socket = null;
    };
    socket.onerror = () => {
      onStatus("closed");
    };
  };

  terminal.onData((data) => {
    if (composing) return;
    if (lastCommitted && data === lastCommitted) {
      lastCommitted = null;
      return;
    }
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(new TextEncoder().encode(data));
    }
  });

  terminal.onSelectionChange(() => {
    const text = terminal.getSelection();
    if (text) void navigator.clipboard.writeText(text).catch(() => undefined);
  });

  const textarea = host.querySelector("textarea");
  const onCompositionStart = () => {
    composing = true;
  };
  const onCompositionEnd = (event: Event) => {
    composing = false;
    const text = (event as CompositionEvent).data;
    if (text && socket?.readyState === WebSocket.OPEN) {
      lastCommitted = text;
      socket.send(new TextEncoder().encode(text));
    }
  };
  textarea?.addEventListener("compositionstart", onCompositionStart);
  textarea?.addEventListener("compositionend", onCompositionEnd);

  const onContextMenu = (event: Event) => event.preventDefault();
  host.addEventListener("contextmenu", onContextMenu);

  const onTouchStart = (event: TouchEvent) => {
    if (event.touches.length === 1) {
      const touch = event.touches[0];
      lastTouch = { x: touch.clientX, y: touch.clientY };
      longPress = window.setTimeout(() => {
        sendMouse(event.target ?? host, "mousedown", 2, touch.clientX, touch.clientY);
        sendMouse(event.target ?? host, "mouseup", 2, touch.clientX, touch.clientY);
        longPress = undefined;
      }, 480);
    } else {
      if (longPress) window.clearTimeout(longPress);
      longPress = undefined;
      lastTouch = null;
      if (event.touches.length === 2) {
        twoFingerOrigin = { y: (event.touches[0].clientY + event.touches[1].clientY) / 2 };
      }
    }
  };
  const onTouchMove = (event: TouchEvent) => {
    event.preventDefault();
    if (event.touches.length === 1 && lastTouch) {
      const touch = event.touches[0];
      if (Math.hypot(touch.clientX - lastTouch.x, touch.clientY - lastTouch.y) > 8 && longPress) {
        window.clearTimeout(longPress);
        longPress = undefined;
      }
    }
    if (event.touches.length === 2 && twoFingerOrigin) {
      const y = (event.touches[0].clientY + event.touches[1].clientY) / 2;
      const delta = twoFingerOrigin.y - y;
      if (Math.abs(delta) > 12) {
        host.dispatchEvent(
          new WheelEvent("wheel", {
            bubbles: true,
            cancelable: true,
            deltaY: delta,
            clientX: event.touches[0].clientX,
            clientY: event.touches[0].clientY,
          }),
        );
        twoFingerOrigin = { y };
      }
    }
  };
  const onTouchEnd = () => {
    if (longPress) window.clearTimeout(longPress);
    longPress = undefined;
    lastTouch = null;
    twoFingerOrigin = null;
  };
  host.addEventListener("touchstart", onTouchStart, { passive: false });
  host.addEventListener("touchmove", onTouchMove, { passive: false });
  host.addEventListener("touchend", onTouchEnd);
  host.addEventListener("touchcancel", onTouchEnd);

  const observer = new ResizeObserver(() => {
    if (resizeTimer) window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(sendResize, 80);
  });
  observer.observe(host);
  connect();

  return {
    sendPrefix: () => {
      if (socket?.readyState === WebSocket.OPEN) {
        socket.send(new TextEncoder().encode("\u0002"));
      }
    },
    paste: async () => {
      const text = await navigator.clipboard.readText();
      if (text && socket?.readyState === WebSocket.OPEN) {
        socket.send(new TextEncoder().encode(text));
      }
    },
    dispose: () => {
      closed = true;
      observer.disconnect();
      if (resizeTimer) window.clearTimeout(resizeTimer);
      if (longPress) window.clearTimeout(longPress);
      textarea?.removeEventListener("compositionstart", onCompositionStart);
      textarea?.removeEventListener("compositionend", onCompositionEnd);
      host.removeEventListener("contextmenu", onContextMenu);
      host.removeEventListener("touchstart", onTouchStart);
      host.removeEventListener("touchmove", onTouchMove);
      host.removeEventListener("touchend", onTouchEnd);
      host.removeEventListener("touchcancel", onTouchEnd);
      socket?.close();
      terminal.dispose();
    },
  };
};
