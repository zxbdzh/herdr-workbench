# Issue tracker: GitHub

问题、规格和实现 tickets 使用 GitHub Issues。使用 `gh` CLI 进行所有操作。

## 当前状态

项目目录当前尚未绑定 GitHub remote。创建仓库并配置 remote 后，以下命令可在项目根目录直接使用；在此之前不要创建依赖远程仓库的 issue。

## 约定

- 创建 issue：`gh issue create --title "..." --body "..."`
- 查看 issue：`gh issue view <number> --comments`
- 列出 issue：`gh issue list --state open --json number,title,body,labels,comments`
- 评论：`gh issue comment <number> --body "..."`
- 添加或移除标签：`gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- 关闭 issue：`gh issue close <number> --comment "..."`

## Pull requests as a triage surface

**PRs as a request surface: no.** 外部 PR 默认不进入 triage 流程；如果项目以后需要把外部 PR 当作需求入口，再显式改为 yes 并补充对应流程。

## Wayfinding operations

如果使用 `wayfinder`：

- Map：创建一个带 `wayfinder:map` 标签的 GitHub issue，保存 Notes、Decisions-so-far 和 Fog。
- Child ticket：创建独立 issue，顶部标注其所属 map，并使用 `wayfinder:<type>` 标签，其中 type 为 `research`、`prototype`、`grilling` 或 `task`。
- Blocking：优先使用 GitHub 原生 issue dependencies；不支持时在 issue 顶部写 `Blocked by: #<n>, #<n>`。
- Claim：`gh issue edit <n> --add-assignee @me`。
- Resolve：评论答案、关闭 issue，再把上下文指针补回 map。

## 技能映射

- `triage` 负责把原始问题推进到可执行状态。
- `to-spec` 将当前讨论整理为 GitHub issue 规格。
- `implement` 从规格或 tickets 开始实现。
- `code-review` 对固定起点之后的变更执行 Standards + Spec 双轴审查。
