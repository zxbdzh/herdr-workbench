<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[English](README.en.md)

**把本机 WebView2 预览、截图和诊断发给当前 Herdr Agent。** 这是 Windows-first 的工作台，不是浏览器插件，也不是远程桌面。

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## 它解决什么

Coding Agent 改完前端，你还得自己开浏览器、看 console、再把报错贴回去。远程电脑或手机更没法看见本机 localhost。——Herdr Workbench 在 Windows 上绑住 Herdr workspace，打开独立 WebView2 预览，采集真实页面状态，再把结构化反馈发给当前 Agent。

v0.1 覆盖本机这条闭环。LAN / 手机 / 文件编辑还没做。

## 功能

- 🖥️ **Windows 主机** — 单个 Tauri 进程内嵌 Axum，默认只听 `127.0.0.1:17321`
- 🔗 **Herdr 绑定** — 启动时从 Herdr CLI 同步 workspace；Named Pipe 事件立刻补绑，30s/2min 协调兜底
- 🌐 **WebView2 Preview** — 每个 workspace 一个独立预览窗口，可复用
- 📸 **截图与诊断** — PNG 截图 + 内存里的 console / network 诊断
- 📡 **实时状态** — workspace 房间 WebSocket；断线后 Query `/state` 并 1s 重连
- 🤖 **发给 Agent** — 结构化 prompt（url / title / 截图路径 / 诊断），走 `herdr agent prompt`

## 安装

需要：Windows 10/11、本机已安装 [Herdr](https://herdr.dev)。

1. 打开 [Releases](https://github.com/zxbdzh/herdr-workbench/releases) 下载 `Herdr Workbench_0.1.5_x64-setup.exe`
2. 安装并启动
3. 先打开 Herdr，再打开 Workbench；工作区列表会从本机 Herdr 同步

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
