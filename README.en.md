<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[中文](README.md)

**Map the terminal Herdr session onto the local window, another computer, and a phone.** The page *is* Herdr: click, drag, type. Windows-first, a Herdr satellite, not a second workspace product.

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## What it solves

A coding agent runs inside Herdr on Windows. Workbench is a thin outer terminal around a real Herdr client. The same page on the host, another computer, or a phone can click, drag, and type. Workspaces, tabs, splits, and right-click stay Herdr’s.

v0.4 ships that mapping. Embedded browser and file browsing are not in this release.

## Features

- 🖥️ **Mapped Herdr** — the page is the live Herdr TUI
- 🖥️ **Windows host** — one Tauri process embeds Axum and listens on `127.0.0.1:17321` by default
- 📱 **Same UI on LAN** — enable LAN on the host to listen on `0.0.0.0:17321`; another computer or phone enters the pairing code and is in Herdr
- 🔗 **One client per connection** — each page gets its own ConPTY; closing detaches that client only

## Install

Requires Windows 10/11 and [Herdr](https://herdr.dev) on the same machine.

1. Download `Herdr Workbench_0.4.0_x64-setup.exe` from [Releases](https://github.com/zxbdzh/herdr-workbench/releases)
2. Install and launch
3. Opening Workbench attaches Herdr. If no server is up, the first client starts it
4. For a phone or another computer: enable LAN on the host, note the pairing code, open `http://<lan-ip>:17321/`, enter the code

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
