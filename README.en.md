<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[中文](README.md)

**Send a real WebView2 preview, screenshot, and diagnostics to the current Herdr agent.** This is a Windows-first workbench, not a browser extension and not remote desktop.

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## What it solves

After a coding agent changes the frontend, you still open a browser, read the console, and paste errors back. A remote computer or phone cannot see localhost at all. Herdr Workbench binds Herdr workspaces on Windows, opens an isolated WebView2 preview, captures real page state, and sends structured feedback to the current agent.

v0.1 covers this local loop. LAN / phone / file editing are not in this release.

## Features

- 🖥️ **Windows host** — one Tauri process embeds Axum and listens on `127.0.0.1:17321` by default
- 🔗 **Herdr binding** — syncs workspaces from the Herdr CLI on startup; Named Pipe events rebind immediately, with a 30s/2min reconcile fallback
- 🌐 **WebView2 preview** — one reusable preview window per workspace
- 📸 **Screenshot and diagnostics** — PNG screenshots plus in-memory console / network diagnostics
- 📡 **Live state** — workspace-room WebSocket; after disconnect, Query `/state` and reconnect in 1s
- 🤖 **Send to agent** — structured prompt (url / title / screenshot path / diagnostics) via `herdr agent prompt`

## Install

Requires Windows 10/11 and [Herdr](https://herdr.dev) on the same machine.

1. Download `Herdr Workbench_0.1.4_x64-setup.exe` from [Releases](https://github.com/zxbdzh/herdr-workbench/releases)
2. Install and launch
3. Start Herdr first, then Workbench; the workspace list syncs from local Herdr

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
