# Herdr Workbench Context

## 项目定位

Herdr Workbench 是 Herdr 的附属壳：把终端里那个 Herdr 映射到本机窗口、另一台电脑和手机。网页中间就是真 Herdr 客户端（ConPTY + 终端仿真），键鼠直达。不另做工作区，也不拿对话快照当主界面。浏览器、文件是后续扩展。

## 核心概念

- **Windows host**：运行 Herdr Workbench 的 Windows 10/11 主机。
- **Herdr session**：Herdr server 拥有的工作区 / tab / pane / Agent。Workbench 只 attach 客户端。
- **Mapped client**：每一路网页连接自己的 ConPTY + `herdr` 客户端。
- **Preview session**：某个 workspace 对应的独立 WebView2 预览会话（后续扩展）。
- **Remote access**：localhost 或显式 LAN + 6 位配对码；不等同于官方 `herdr --remote`。

## 已确认的边界

- Windows 是第一目标平台。
- Windows 不能作为官方 `herdr --remote` 的目标主机；`herdr terminal attach` 在 Windows 上 unsupported。
- 映射走 ConPTY + 页面终端仿真，不把点击翻译成 `pane send-keys`，也不投 Windows Terminal 窗口。
- 关窗口 / 断线只 detach 这一路客户端，不 `herdr server stop`。
- 远程访问默认关闭；公网不是默认部署。

## 目标闭环

```text
Windows Herdr server
→ Workbench 每路开一个 ConPTY 客户端
→ 本机 / 电脑 / 手机点进同一个 Herdr
→ 浏览器 / 文件作为后续扩展
```

## 文档维护

- 新增领域术语前，先检查本文件是否已有可复用定义。
- 高成本架构、安全或协议决策写入 `docs/adr/`。
- 实现规格和执行 tickets 放在 GitHub Issues。
