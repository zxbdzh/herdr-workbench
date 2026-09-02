# 贡献指南

感谢参与 Herdr Workbench。项目当前以 Windows 10/11 原生环境为第一目标，所有贡献都需要尊重 Windows-first 的运行约束。

## 开始之前

1. 阅读 `AGENTS.md`、`CONTEXT.md` 和涉及区域的 `docs/adr/`。
2. 搜索现有 GitHub Issues，确认需求不是重复问题。
3. 新功能或架构变化先创建/更新 Issue；复杂设计先在 Discussions 中确认。
4. 不要把密钥、Cookie、token、私有地址或用户数据提交到仓库。

## 实现流程

```text
gril-with-docs（复杂想法）
→ to-spec（规格）
→ GitHub Issue
→ implement
→ tdd
→ code-review
→ PR
```

小型修复可以直接从 Issue 开始，但仍然需要测试和 review。

## TDD 约束

- 先写一个描述用户可观察行为的失败测试。
- 只实现让当前测试通过的最小行为。
- 每次只推进一个垂直切片。
- 测试公共 Interface，不测试私有实现细节。
- 优先使用最高层 seam；Application Core 测试使用 fake Adapter。
- 真实 Windows、Tauri、WebView2、Herdr Socket 测试使用单独的 integration test。

## 架构约束

- `domain` 不依赖 Axum、SQLx、Tauri、WebView2 或 Windows API。
- `app-core` 依赖 Interface，不依赖具体基础设施。
- `transport` 不直接访问数据库。
- `contracts` 是对外 DTO 和事件契约的稳定 seam。
- Windows API 只放在 `windows-platform` 和对应 Adapter。
- 源码文件系统是源码事实来源；SQLite 只保存 Workbench 状态、索引和 durable event journal。
- durable state 与 durable event 必须在同一 SQLite 事务中提交，提交后才发布事件。
- 远程客户端统一使用 REST + WebSocket，不为 Tauri 单独维护第二套业务协议。
- 不依赖 Kitty graphics 将网页截图塞入 Herdr 终端 pane。

## Windows-first 约束

- 插件命令必须是 Windows 兼容的 argv。
- 不把 `sh`、Bash、Unix socket 或 Unix 路径作为默认实现。
- 本机 IPC 优先使用 Windows Named Pipe。
- 调用 Herdr 优先使用 `HERDR_BIN_PATH`；原始 socket 行为必须封装在 Adapter 内。
- WebView2 运行在交互式用户会话，不放入 Windows Service。
- 文件操作拒绝绝对路径、UNC 路径和路径穿越。
- 远程访问默认关闭；公网暴露、中继和 NAT 穿透不属于 MVP 默认能力。

## 验证要求

提交前运行与改动相关的检查。常用命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
pnpm install --frozen-lockfile
pnpm web:typecheck
pnpm web:build
```

只报告真实执行结果：

- 没有运行的命令写“未运行”。
- 失败的测试写明失败命令和错误。
- 不得伪造 CI、截图、日志、性能数据、部署结果或外部 API 响应。
- 外部系统的写操作必须读回目标确认效果。

## 提交与 PR

使用中文 Conventional Commits：

```text
feat(scope): 中文摘要
fix(scope): 中文摘要
test(scope): 中文摘要
docs(scope): 中文摘要
ci(scope): 中文摘要
```

PR 必须：

- 关联 GitHub Issue。
- 说明用户可见行为。
- 列出实际运行过的验证命令和结果。
- 说明风险、兼容性和回滚方式。
- 明确列出未完成内容和已知限制。
- 等 Windows CI 通过后再请求合并。

## AI 贡献者特别规则

AI 修改代码前必须读取相关 `AGENTS.md`、`CONTEXT.md` 和 ADR。AI 不得：

- 凭记忆猜测项目事实、API 行为或依赖版本。
- 把“声称生成”当成实际生成结果。
- 把 exit code 200 当成服务内容有效，必须检查响应内容。
- 把本地测试通过当成 Windows CI 或外部系统已成功。
- 未经确认扩大规格范围、创建新类别、安装额外依赖或修改外部系统。
- 为了通过测试而削弱安全边界或删除有意义的失败测试。

AI 应在 PR 中留下真实的验证证据，并在不确定时明确写出假设和阻塞项。
