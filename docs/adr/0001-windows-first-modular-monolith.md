# ADR-0001：采用 Windows-first 模块化单体架构

- 状态：Accepted
- 日期：2026-09-01
- 范围：整体系统架构、进程边界、远程访问

## 背景

Herdr Workbench 的主要宿主是纯血 Windows Herdr，但用户需要从本机、远程电脑和手机查看同一个工作区中的本地网页、文件和 Agent 反馈。

Herdr 当前 Windows 支持仍是 beta，插件能力是 preview / best-effort；Windows 不能作为官方 `herdr --remote` 的目标主机；plugin v1 的 pane 主要是终端 pane，原生非终端插件 UI 不属于当前稳定能力。因此不能把产品建立在 Herdr 原生嵌入 WebView2 或官方 Windows remote 上。

## 决策

采用以下架构：

```text
Windows 10/11
└── herdr-workbench.exe
    ├── Tauri 2 + React 主窗口
    ├── 独立 Tauri Preview 窗口 + WebView2
    ├── Axum REST API
    ├── WebSocket 事件流
    ├── Rust Application Core
    ├── SQLite / SQLx
    └── Windows Named Pipe / Herdr Adapter
```

核心是**模块化单体**，不是一开始拆分为多个长期运行服务。

- Rust + Tokio：核心语言和异步运行时
- Axum：HTTP / WebSocket 服务
- Tauri 2：Windows 交互式应用和多窗口容器
- WebView2：独立 Preview Session 的实际页面渲染器
- React + TypeScript：本机和远程共用的 Web UI
- SQLx + SQLite：状态、审计和 durable event journal
- Workspace 文件系统：源码唯一事实来源
- Tokio `broadcast`：进程内实时事件总线
- Herdr Socket API：实时 workspace / tab / pane / agent 事件来源
- reconcile：启动、重连和定期同步的最终一致性兜底

## 进程和模块边界

Tauri 进程同时持有：

- 本机 React Workbench UI
- 独立 Preview WebView2 窗口
- Axum HTTP/WebSocket server
- Rust Application Core

不提前引入 Windows Service。WebView2 需要交互式用户会话；如果未来需要无人登录运行，再拆出后台 Service 与交互式 Host。

代码依赖方向：

```text
domain ← app-core ← transport
                  ← adapters ← windows-platform
                  ← src-tauri
contracts ← transport
contracts ← web
```

`domain` 不依赖 Axum、SQLite、Tauri 或 Windows API；`app-core` 只依赖 Adapter Interface；`transport` 不直接操作数据库；Windows API 只进入 Windows adapter。

## 远程访问决策

所有客户端统一使用：

```text
REST API + WebSocket
```

- localhost 默认开启：`127.0.0.1:17321`
- LAN 默认关闭，用户明确开启后才监听
- 跨网络优先使用 Tailscale / ZeroTier 或 SSH 隧道
- 公网直连和自建中继不作为 MVP 能力
- 远程客户端不依赖 `herdr --remote windows-host`
- 账号密码使用 Argon2id hash
- 登录后使用 HttpOnly Session Cookie
- 手机默认只有 viewer + reviewer 权限

## Preview 决策

Workbench Host 持有 Preview Session。每个 Herdr workspace 对应一个独立 Preview session 和 WebView2 实例。

```text
workspace-a → preview-a → WebView2-a
workspace-b → preview-b → WebView2-b
```

Tauri Preview 窗口是独立窗口，不嵌入主窗口。远程客户端不连接该窗口，而是通过 Workbench API 获取：

- 截图
- URL / title / load state
- console errors
- failed network requests
- 有限输入事件

生产 Adapter 使用受控 Tauri Rust command 和页面事件桥接；Playwright 只作为测试 Adapter，不作为生产主控制层。

## 数据和事件决策

```text
文件系统 = 源码事实
SQLite = 当前状态 + durable event journal
内存 = WebView2 / 连接 / 临时状态
broadcast = 进程内实时通知
WebSocket = 对外实时通知
```

Durable Command 的顺序：

```text
权限校验
→ revision 校验
→ 外部副作用
→ SQLite transaction
   ├── 状态表
   └── durable event
→ COMMIT
→ AppEvent
→ WebSocket
```

事件使用强类型事件信封，包含 event id、event type、workspace id、发生时间、revision 和 payload。每个 workspace 独立维护 revision；客户端检测到跳号时重新 Query 快照，不尝试补造事件。

## 替代方案

### 微服务架构

拒绝。当前是单机 Windows-first 工具，没有服务发现、远程数据库或跨节点部署需求。拆分会提前引入进程生命周期、IPC、版本和部署复杂度。

### 纯 Tauri invoke

拒绝。远程浏览器无法复用 Tauri invoke，会形成两套业务接口。

### 远程客户端各自加载页面

拒绝。每个客户端会拥有不同的页面状态、Cookie、console 和 network 结果，无法形成一个共享 Preview Session。

### 只使用 Herdr plugin event hooks

拒绝。事件 hook 适合一次性触发命令，不适合长期订阅 workspace、pane 和 agent 状态流。Workbench 直接订阅 Herdr Socket API，并用 reconcile 兜底。

### 完整 Event Sourcing

拒绝。当前查询以状态为主，完整重放会增加 SQLite 查询和迁移复杂度。采用 State Tables + Durable Event Journal。

## 后果

### 正面

- 本机、远程电脑和手机共享同一套 API 和 UI
- Windows 能力集中，跨平台约束不污染领域层
- WebView2 页面、诊断和截图只有一个事实来源
- 远程访问不依赖 Herdr Windows remote 的未来进度
- 模块 Interface 清晰，测试可替换 Adapter
- 后续可以把某个模块拆成独立进程而不推翻业务层

### 代价

- Tauri 与 Axum 同进程，Preview 崩溃隔离能力有限
- WebView2 页面需要通过截图和事件远程同步
- Windows 插件能力仍需针对当前 Herdr 版本做实际验证
- 文件系统与 SQLite 无法组成真正的跨系统事务
- 远程输入、认证和文件沙箱需要严格安全测试

## 相关文档

- [架构总览](../architecture/overview.md)
- [核心流程](../architecture/flows.md)
- [远程访问](../architecture/remote-access.md)
- [Herdr Windows 支持](https://herdr.dev/docs/windows-beta/)
- [Herdr 插件开发](https://herdr.dev/docs/plugins/)
- [Herdr Socket API](https://herdr.dev/docs/socket-api/)
