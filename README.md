<div align="center">

<img src="src-tauri/icons/icon.png" alt="Herdr Workbench" width="96" height="96" />

# Herdr Workbench

[English](README.en.md)

**把终端里的 Herdr 映射到本机窗口、另一台电脑和手机。** 中间就是真 Herdr：可点、可拖、可打字。Windows-first，Herdr 附属，不是独立工作区软件。

<sub>// Windows-first · Herdr · WebView2 · Browser-to-Agent</sub>

<br />

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.88-DEA584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![License](https://img.shields.io/badge/License-Apache%202.0-green)

</div>

---

## 它解决什么

Coding Agent 跑在 Windows 上的 Herdr 里。Workbench 是薄壳：网页当外层终端，里面跑真 Herdr 客户端。本机、另一台电脑、手机打开同一套页面就能点、拖、打字。工作区 / tab / 分栏 / 右键仍是 Herdr 自己的。

v0.4 先交付映射出去的 Herdr。内嵌浏览器和文件浏览还没做。

## 功能

- 🖥️ **映射 Herdr** — 网页中间就是终端里那个 Herdr，键鼠直达
- 🖥️ **Windows 主机** — 单个 Tauri 进程内嵌 Axum，默认只听 `127.0.0.1:17321`
- 📱 **局域网同一套 UI** — 本机点「开启局域网」后听 `0.0.0.0:17321`，手机/另一台电脑输入配对码就是 Herdr
- 🔗 **每路一个客户端** — 每个网页连接自己的 ConPTY；关窗口只 detach，不杀 server

## 安装

需要：Windows 10/11、本机已安装 [Herdr](https://herdr.dev)。

1. 打开 [Releases](https://github.com/zxbdzh/herdr-workbench/releases) 下载 `Herdr Workbench_0.4.5_x64-setup.exe`
2. 安装并启动
3. 打开就是 Herdr。没有 server 时，第一份客户端会把它拉起来
4. 要给手机或另一台电脑用：在本机点「开启局域网」，记下配对码，另一台设备打开 `http://<局域网IP>:17321/`，输入配对码

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
