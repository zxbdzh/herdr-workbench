# Herdr Workbench 架构总览

> 当前架构基线：Windows-first、模块化单体、单个 Tauri 进程内嵌 Axum 服务，远程客户端统一使用 HTTP/WebSocket。

## 1. 系统上下文

```mermaid
flowchart LR
    user[本机用户]
    remote[远程电脑 / 手机]
    agent[Coding Agent\nClaude / Codex / Pi / Hermes 等]
    herdr[Herdr\nWorkspace / Pane / Agent 状态]
    host[Windows Workbench\nTauri + Axum + WebView2]

    user -->|React UI| host
    remote -->|HTTPS / WSS 或 HTTP / WS| host
    agent -->|Herdr CLI / Socket API| herdr
    host -->|HERDR_BIN_PATH / Socket API| herdr
    host -->|workspace context| agent
```

## 2. 部署架构

```mermaid
flowchart TB
    subgraph windows[Windows 10/11 交互式用户会话]
        app[herdr-workbench.exe\n单个 Tauri 进程]

        subgraph runtime[Rust 应用运行时]
            axum[Axum\nREST + WebSocket]
            core[Application Core\nCommand / Query / Event Bus]
            preview[Preview Manager\nPreview Session]
            webview[Tauri WebView2 Preview Window]
            herdr_adapter[Herdr Adapter\nSocket / CLI]
            sqlite[SQLx SQLite Adapter]
            fs[Windows File Adapter\nWorkspace Sandbox]
            git[Git Adapter]
            auth[Auth & Device Manager]
        end

        web[React + TypeScript\n嵌入静态资源]
        db[(SQLite\n%LOCALAPPDATA%\\HerdrWorkbench)]
        files[(Workspace 文件系统\n源码唯一事实来源)]
        artifacts[(截图 / 日志\n%LOCALAPPDATA%\\HerdrWorkbench)]
        pipe[Windows Named Pipe\n本机 IPC]
    end

    app --> web
    app --> axum
    axum --> core
    web -->|REST / WebSocket| axum
    core --> preview
    core --> herdr_adapter
    core --> sqlite
    core --> fs
    core --> git
    core --> auth
    preview --> webview
    herdr_adapter -->|HERDR_BIN_PATH\nSocket API| herdr[Herdr Server]
    sqlite --> db
    fs --> files
    preview --> artifacts
    app --- pipe
```

## 3. 代码依赖方向

```mermaid
flowchart BT
    domain[domain\n领域模型 / 规则 / 强类型事件]
    app[app-core\nCommand / Query / 事务编排]
    contracts[contracts\nREST DTO / WS DTO / 错误码]
    transport[transport\nAxum / WS / Auth Middleware]
    adapters[adapters\nSQLite / Filesystem / Git / Herdr / Preview]
    windows[windows-platform\nWebView2 / Named Pipe / Windows API]
    tauri[src-tauri\n窗口 / 托盘 / 桌面生命周期]
    web[web\nReact UI]

    domain --> app
    app --> transport
    app --> adapters
    contracts --> transport
    contracts --> web
    windows --> adapters
    windows --> tauri
```

约束：

- `domain` 不依赖 Axum、SQLite、Tauri 或 Windows API。
- `app-core` 只依赖 Adapter Interface，不依赖具体基础设施实现。
- `transport` 不直接读写 SQLite。
- `web` 只依赖生成的 contracts，不知道 Rust 内部模块。
- Windows API 只存在于 `windows-platform` 和对应 Adapter。

## 4. 核心模块

| 模块 | 职责 | 主要接口 |
| --- | --- | --- |
| `domain` | 领域模型、Command、Query、Event、业务规则 | 强类型值对象和事件 |
| `app-core` | 权限校验、事务编排、revision、事件发布 | Application Service |
| `contracts` | REST / WebSocket 对外 DTO、错误结构 | OpenAPI / JSON Schema |
| `transport` | HTTP 路由、WebSocket 会话、认证中间件 | Axum Handler |
| `preview` | Preview Session 生命周期、导航、诊断、截图 | `PreviewManager` |
| `workspace` | Herdr workspace 映射和 reconcile | `WorkspaceManager` |
| `files` | workspace 沙箱、读取、保存、版本冲突 | `FileManager` |
| `git` | diff、分支、状态查询 | `GitAdapter` |
| `agent` | Herdr Agent 状态和上下文发送 | `HerdrAdapter` |
| `auth` | 用户、Session Cookie、设备和权限 | `AuthManager` |
| `adapters` | 外部系统具体实现 | Adapter implementations |
| `windows-platform` | WebView2、Named Pipe、防火墙、启动 | Windows Adapter |

## 5. 事实来源和状态

```mermaid
flowchart LR
    fs[Workspace 文件系统]
    sqlite[(SQLite 状态表)]
    journal[(Durable Event Journal)]
    memory[Rust 内存状态]
    bus[Tokio broadcast]
    ws[WebSocket 客户端]

    fs -->|源码 / 配置 / Git| memory
    memory -->|当前运行态| sqlite
    sqlite --> journal
    sqlite --> memory
    memory --> bus
    bus --> ws
```

- Workspace 文件系统：源码、配置和资源的唯一事实来源。
- SQLite 状态表：Workbench 当前可查询状态。
- Durable Event Journal：重要状态变化、审计和 revision 同步记录。
- Rust 内存：WebView2、连接和临时运行态。
- Tokio `broadcast`：进程内实时传播，不负责补发历史。
- WebSocket：对外实时通知，断线后通过 Query 恢复。

重要 Command 的顺序：

```text
权限校验
→ workspace / revision 校验
→ 执行外部副作用
→ SQLite transaction
   ├── 更新状态表
   └── 写入 durable event
→ COMMIT
→ 发布 AppEvent
→ WebSocket 广播
→ 返回响应
```

## 6. API 契约链

```mermaid
flowchart LR
    rust[Rust DTO / Event Types]
    utoipa[utoipa]
    openapi[OpenAPI]
    generator[openapi-typescript]
    client[Generated TS Client]
    react[React UI]

    rust --> utoipa --> openapi --> generator --> client --> react
    rust --> ws_schema[WebSocket Event JSON Schema]
    ws_schema --> client
```

Rust 类型是唯一契约源。REST 使用 `utoipa` 生成 OpenAPI；WebSocket 事件使用相同的强类型事件定义并生成 JSON Schema / OpenAPI 扩展。
