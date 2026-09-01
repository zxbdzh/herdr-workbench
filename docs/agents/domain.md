# Domain Docs

本项目采用 single-context 文档布局。工程技能在探索代码或设计变更前，按主题读取领域文档。

## 读取顺序

- `CONTEXT.md`：项目领域术语、核心边界、当前共识和已确认决策。
- `docs/adr/`：影响架构、协议、安全边界或数据模型的高成本决策。
- 当前正在修改的代码及其测试：实现层面的事实来源。

如果某个文件或目录尚不存在，继续工作，不因为缺少文档而停止；只有在术语或架构决策真正确定时才创建它们。

## 术语规则

- issue、spec、ticket、测试名称和实现说明优先使用 `CONTEXT.md` 中的术语。
- 如果需要的概念尚未进入 glossary，先判断是误用新词，还是领域模型确实存在缺口。
- 与已有 ADR 冲突时，必须明确指出冲突，不要静默覆盖。

## 布局

```text
/
├── AGENTS.md
├── CONTEXT.md
├── docs/
│   ├── adr/
│   └── agents/
│       ├── domain.md
│       ├── issue-tracker.md
│       └── triage-labels.md
└── src/
```

## Windows-first 领域边界

本项目的默认宿主是 Windows 10/11 原生环境。涉及 Herdr 插件、WebView2、Windows Named Pipe、HTTP/WebSocket 远程访问、workspace 绑定、文件沙箱和浏览器诊断的设计，均应优先从 Windows 的实际运行约束出发；跨平台能力只能在明确验证后加入。
