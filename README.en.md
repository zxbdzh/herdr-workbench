<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[中文](README.md)

**Use the same Web UI to work a local Herdr workspace remotely.** Enter a workspace, read the agent, reply, and approve or reject when it is blocked. Windows-first, no terminal, not remote desktop.

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## What it solves

A coding agent runs inside Herdr on Windows, but a remote computer or phone is stuck watching a terminal — or a Preview dashboard. Herdr Workbench binds Herdr workspaces on Windows and exposes the same usage surface locally, on another computer, and on a phone: enter a workspace, read recent agent output, reply, and approve or reject when the agent is blocked.

v0.3 ships the workspace agent session. Preview and screenshots still exist, but they are not the product of this slice. File browsing is not in this release.

## Features

- 🤖 **Use Herdr remotely** — enter a workspace, read the agent, reply; approve/reject when `blocked`
- 🖥️ **Windows host** — one Tauri process embeds Axum and listens on `127.0.0.1:17321` by default
- 📱 **Same UI on LAN** — enable LAN on the host to listen on `0.0.0.0:17321`; another computer or phone enters the pairing code and uses the same workspace
- 🔗 **Herdr binding** — syncs workspaces from the Herdr CLI on startup; Named Pipe events rebind immediately, with a 30s/2min reconcile fallback
- 🌐 **WebView2 preview** — one reusable preview window per workspace
- 📸 **Screenshot and diagnostics** — PNG screenshots plus in-memory console / network diagnostics
- 📡 **Live state** — workspace-room WebSocket; after disconnect, Query `/state` and reconnect in 1s

## Install

Requires Windows 10/11 and [Herdr](https://herdr.dev) on the same machine.

1. Download `Herdr Workbench_0.3.0_x64-setup.exe` from [Releases](https://github.com/zxbdzh/herdr-workbench/releases)
2. Install and launch
3. Start Herdr first, then Workbench; the workspace list syncs from local Herdr
4. For a phone or another computer: enable LAN on the host, note the pairing code, open `http://<lan-ip>:17321/`, enter the code, then open a workspace

Developers can also run the debug executable:

```powershell
pnpm install
pnpm exec tauri build --debug --no-bundle
.\target\debug\herdr-workbench-desktop.exe
```

## Development

```powershell
pnpm install
pnpm web:typecheck
pnpm web:build
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm exec tauri build --debug --no-bundle
```

Cargo / pnpm / Windows CI are the source of truth. File-tool Rust 2015 `async fn` errors are false positives.

## License

[Apache-2.0](./LICENSE)
