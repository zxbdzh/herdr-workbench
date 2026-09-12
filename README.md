<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[English](README.en.md)

**把本机 Herdr 工作区通过同一套 Web UI 提供给远程使用。** 进工作区、看 Agent 输出、回复；blocked 时批准或拒绝。Windows-first，不依赖终端，也不是远程桌面。

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## 它解决什么

Coding Agent 跑在 Windows 上的 Herdr 里，远程电脑或手机却只能盯着终端，或者只能看一个 Preview 仪表盘。——Herdr Workbench 在 Windows 上绑住 Herdr workspace，把同一套使用面开给本机、另一台电脑和手机：进工作区，看 Agent 近期输出，回复；Agent 卡住时批准或拒绝。

v0.3 先交付工作区 + Agent 对话/批准。Preview / 截图仍可用，但不是这一刀的产品。文件浏览还没做。

## 功能

- 🤖 **远程用 Herdr** — 进工作区，看 Agent 输出，回复；`blocked` 时批准/拒绝
- 🖥️ **Windows 主机** — 单个 Tauri 进程内嵌 Axum，默认只听 `127.0.0.1:17321`
- 📱 **局域网同一套 UI** — 本机点「开启局域网」后听 `0.0.0.0:17321`，手机/另一台电脑输入配对码进同一套工作区
- 🔗 **Herdr 绑定** — 启动时从 Herdr CLI 同步 workspace；Named Pipe 事件立刻补绑，30s/2min 协调兜底
- 🌐 **WebView2 Preview** — 每个 workspace 一个独立预览窗口，可复用
- 📸 **截图与诊断** — PNG 截图 + 内存里的 console / network 诊断
- 📡 **实时状态** — workspace 房间 WebSocket；断线后 Query `/state` 并 1s 重连

## 安装

需要：Windows 10/11、本机已安装 [Herdr](https://herdr.dev)。

1. 打开 [Releases](https://github.com/zxbdzh/herdr-workbench/releases) 下载 `Herdr Workbench_0.3.1_x64-setup.exe`
2. 安装并启动
3. 先打开 Herdr，再打开 Workbench；工作区列表会从本机 Herdr 同步
4. 要给手机或另一台电脑用：在本机点「开启局域网」，记下配对码，另一台设备打开显示的 `http://<局域网IP>:17321/`，输入配对码，再点进工作区

开发者也可以直接跑 debug 可执行文件：

```powershell
pnpm install
pnpm exec tauri build --debug --no-bundle
.\target\debug\herdr-workbench-desktop.exe
```

## 开发

```powershell
pnpm install
pnpm web:typecheck
pnpm web:build
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm exec tauri build --debug --no-bundle
```

质量门禁以 Cargo / pnpm / Windows CI 为准。文件工具报的 Rust 2015 `async fn` 是假阳性。

## 许可证

[Apache-2.0](./LICENSE)
