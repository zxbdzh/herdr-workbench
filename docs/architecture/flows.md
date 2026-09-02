# Herdr Workbench 核心流程

## 1. 应用启动与 workspace 同步

```mermaid
sequenceDiagram
    participant U as Windows 用户
    participant T as Tauri App
    participant A as Axum Server
    participant H as Herdr Adapter
    participant S as SQLite
    participant B as Event Bus
    participant C as Web 客户端

    U->>T: 启动 herdr-workbench.exe
    T->>A: 初始化 REST / WebSocket
    T->>H: 通过 HERDR_BIN_PATH / Socket 连接 Herdr
    H->>H: events.subscribe
    H->>A: Herdr workspace / tab / pane 事件
    A->>S: 完整 reconcile
    S-->>A: 恢复 workspace 映射
    A->>B: 发布 WorkspaceSynced
    B-->>C: 广播最新 workspace 快照
    A-->>T: 打开 React 主窗口
```

兜底规则：启动和 Herdr 重连执行完整 reconcile；正常运行依赖事件；默认每 30 秒轻量协调一次；Herdr 不可用时退避到 2 分钟。

## 2. 打开 Preview Session

```mermaid
sequenceDiagram
    participant U as 用户 / 远程客户端
    participant API as Axum API
    participant APP as Application Core
    participant DB as SQLite
    participant P as Preview Manager
    participant W as Tauri WebView2
    participant E as Event Bus

    U->>API: POST /workspaces/:id/preview/open
    API->>APP: OpenPreview Command
    APP->>APP: 校验 Session / workspace / 权限
    APP->>P: 创建或恢复 Preview Session
    P->>W: 创建独立 Preview Window
    W-->>P: 页面加载状态
    P->>DB: transaction: 保存 session 状态 + event
    DB-->>P: COMMIT
    P->>E: 发布 PreviewOpened
    E-->>API: WebSocket 广播
    API-->>U: 返回 preview state
```

## 3. 浏览器诊断与远程同步

```mermaid
flowchart TD
    w[WebView2 页面]
    nav[导航 / 加载事件]
    console[console.error / window error]
    network[失败请求]
    shot[截图更新]
    pm[Preview Manager]
    state[内存 Preview State]
    bus[Tokio broadcast]
    ws[Workspace WebSocket 房间]
    remote[本机 / 远程电脑 / 手机]
    store[截图文件 + 诊断索引]

    w --> nav --> pm
    w --> console --> pm
    w --> network --> pm
    w --> shot --> pm
    pm --> state
    pm --> store
    state --> bus --> ws --> remote
```

高频事件优先进入内存和 WebSocket；截图文件保存到 `%LOCALAPPDATA%\\HerdrWorkbench\\screenshots`，SQLite 只保存路径、hash、大小和 revision。重要诊断可按策略进入 journal，不能无限写入数据库。

## 4. Browser-to-Agent Feedback

```mermaid
flowchart LR
    agent[Agent 写代码]
    dev[启动 localhost dev server]
    open[Workbench 打开 Preview]
    user[用户查看真实页面]
    capture[采集 URL / title / screenshot\nconsole / network]
    review[用户确认或补充反馈]
    context[结构化 Context]
    send[Herdr Adapter\n发送给当前 Agent]
    fix[Agent 修复]
    reload[Preview reload]

    agent --> dev --> open --> user --> capture --> review --> context --> send --> fix --> reload --> user
```

Context 发送不允许直接拼接任意终端输入。Application Core 生成结构化反馈，Herdr Adapter 使用已确认的 Agent / pane 身份发送。

## 5. 文件保存流程

```mermaid
sequenceDiagram
    participant C as Web 客户端
    participant API as Axum API
    participant F as File Manager
    participant FS as Workspace 文件系统
    participant DB as SQLite
    participant E as Event Bus

    C->>API: POST files/save(path, content, base_revision)
    API->>F: SaveFile Command
    F->>F: 校验相对路径和 workspace 沙箱
    F->>FS: 读取当前 hash
    alt base_revision 不匹配
        FS-->>F: 文件已被修改
        F-->>API: 409 file_changed_since_read
        API-->>C: 冲突详情
    else revision 匹配
        F->>FS: 写入临时文件
        F->>FS: flush + rename 替换目标
        F->>FS: 重新计算 hash
        F->>DB: transaction: metadata + FileSaved journal
        DB-->>F: COMMIT
        F->>E: 发布 FileSaved
        E-->>API: WebSocket file.changed
        API-->>C: 返回新 revision
    end
```

文件系统是源码事实来源。若文件替换成功但 SQLite 提交失败，保留新文件并标记 `pending_reconcile`，由协调器重新扫描 metadata；不引入跨文件系统和数据库的伪事务。

## 6. 事件丢失恢复

```mermaid
flowchart TD
    event[收到 Herdr / Preview 事件]
    check{revision 是否连续?}
    apply[处理事件\n更新内存 + 广播]
    gap[暂停相关外发]
    snapshot[Query 当前快照]
    reconcile[reconcile Herdr + SQLite]
    resume[广播最新快照\n恢复事件处理]

    event --> check
    check -->|是| apply
    check -->|否| gap --> snapshot --> reconcile --> resume
```

Tokio `broadcast` 出现 `Lagged` 时，订阅者不能尝试补造事件；重新 Query 当前状态即可。
