# Herdr Workbench Context

## 项目定位

Herdr Workbench 是一个 Windows-first 的 Herdr 远程工作台。Windows 主机承载工作区、Agent 会话、浏览器预览、文件访问和远程 API；Herdr 负责工作区与 Agent 上下文；远程电脑和手机通过同一套 Web 客户端**使用** Herdr，而不是盯着一个控制台。

## 核心概念

- **Windows host**：运行 Herdr Workbench 能力服务的 Windows 10/11 主机。
- **Herdr workspace**：Herdr 管理的工作区，是预览、文件和 Agent 上下文的绑定边界。
- **Preview session**：某个 workspace 对应的独立 WebView2 预览会话。
- **Browser-to-Agent Feedback**：从真实网页采集截图、console error、network failure 和 URL 等信息，再以结构化上下文发送给 Agent。
- **Workbench client**：本机、远程电脑或手机上的 Web 客户端。
- **Remote access**：通过 localhost、LAN、Tailscale/ZeroTier 或 SSH 隧道访问 Windows host；不等同于 Herdr 官方的 `herdr --remote` 协议。

## 已确认的边界

- Windows 是第一目标平台，跨平台不是 MVP 目标。
- Herdr plugin v1 的 pane 主要是终端 pane，不能假定存在原生 WebView 嵌入能力。
- Windows 不能作为 Herdr 官方 `herdr --remote` 的目标主机，因此 Workbench 需要自己的 HTTP/WebSocket 协议。
- 浏览器预览优先使用独立 WebView2 profile，不默认接管用户日常 Chrome/Edge 或读取其 Cookie。
- 远程画面优先采用截图 + 事件更新，不依赖 Kitty graphics。
- 文件访问默认限制在 workspace 根目录，拒绝绝对路径、UNC 路径和路径穿越。
- 远程访问默认关闭；公网暴露不是 MVP 的默认部署方式。
- 手机默认只拥有 viewer + reviewer 能力，不默认允许写文件或控制 Agent。

## 目标闭环

```text
Windows Herdr
→ Workbench 绑定工作区
→ 本机/远程电脑/手机进入工作区
→ 查看 Agent 输出并回复 / 批准
→ Preview 与文件作为配套能力
```

## 文档维护

- 新增领域术语前，先检查本文件是否已有可复用定义。
- 高成本架构、安全或协议决策写入 `docs/adr/`。
- 实现规格和执行 tickets 放在 GitHub Issues，不把短期任务堆进本文件。
- 发生重要边界变化时，先更新本文件或 ADR，再修改实现。
