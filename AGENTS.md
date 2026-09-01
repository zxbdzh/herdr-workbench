# Herdr Workbench

Windows-first Herdr 远程工作台：让用户从本机电脑、远程电脑或手机查看本地网页、检查项目文件，并把结构化浏览器反馈发回 coding agent。

## Agent skills

本项目使用 Matt Pocock 工程技能流程：

- 先用 `grill-with-docs` 澄清复杂想法，并维护 `CONTEXT.md` 与 ADR。
- 用 `to-spec` 将已确认的讨论整理为 GitHub Issue 中的实现规格。
- 用 `implement` 按 ticket 实现，每个行为遵循 `tdd` 的 red-green-refactor 循环。
- 用 `code-review` 从 Standards 和 Spec 两个维度审查变更。
- 新问题先经过 `triage`，使用 `docs/agents/triage-labels.md` 中的标签映射。

### Issue tracker

问题、规格和实现 tickets 使用 GitHub Issues。具体命令约定见 `docs/agents/issue-tracker.md`。项目绑定 GitHub remote 后，在项目根目录直接运行 `gh issue ...`。

### Triage labels

使用五个标准 triage labels：

- `needs-triage`
- `needs-info`
- `ready-for-agent`
- `ready-for-human`
- `wontfix`
  映射和含义见 `docs/agents/triage-labels.md`。

### Domain docs

这是单上下文项目。开始探索前，按需读取：

- `CONTEXT.md`：当前领域术语、边界和已确认决策
- `docs/adr/`：影响架构的不可逆或高成本决策
- `docs/agents/domain.md`：文档消费者规则

## Windows-first 约束

- 第一目标平台是 Windows 10/11 原生环境。
- Herdr 插件命令必须使用 Windows 兼容的 argv；不能把 Unix `sh`、Bash 或 Unix 路径当默认实现。
- 浏览器预览优先使用 WebView2；远程客户端通过 HTTP/WebSocket 访问，不依赖 `herdr --remote` 将 Windows 作为目标主机。
- 本地 IPC 优先使用 Windows Named Pipe；调用 Herdr 优先使用 `HERDR_BIN_PATH`。
- 文件操作默认限制在当前 workspace 根目录内，拒绝路径穿越、绝对路径和 UNC 路径。
- 远程访问默认关闭，仅在用户明确开启后提供 LAN / Tailscale / SSH 隧道模式。
- 不依赖 Kitty graphics 将网页截图塞回 Herdr 终端 pane。

## 提交规范

使用 Conventional Commits，提交标题使用中文：

```text
feat(scope): 中文摘要
fix(scope): 中文摘要
docs(scope): 中文摘要
正文用短列表说明具体改动。
```

