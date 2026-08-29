# Issue tracker: GitHub

仓库的 issue 和 PRD 使用 GitHub issue 管理。所有操作使用 `gh` CLI。

## 约定

- **创建 issue**: `gh issue create --title "..." --body "..."`。多行正文用 heredoc。
- **查看 issue**: `gh issue view <number> --comments`，可配合 `jq` 过滤评论并获取标签。
- **列出 issue**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`，按需添加 `--label` 和 `--state` 过滤。
- **评论 issue**: `gh issue comment <number> --body "..."`
- **添加 / 移除标签**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **关闭**: `gh issue close <number> --comment "..."`

仓库从 `git remote -v` 推断 —— 在 clone 中运行 `gh` 会自动识别。

## 将 PR 作为 triage 入口

**PR 作为请求入口：否。** （若仓库希望将外部 PR 当作 feature request 处理，可改为 `yes`，`/triage` 会读取该标志。）

若设为 `yes`，PR 与 issue 使用相同的标签和状态，对应 `gh pr` 命令：

- **查看 PR**: `gh pr view <number> --comments` 和 `gh pr diff <number>` 查看 diff。
- **列出待 triage 的外部 PR**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments`，仅保留 `authorAssociation` 为 `CONTRIBUTOR`、`FIRST_TIME_CONTRIBUTOR` 或 `NONE` 的项（排除 `OWNER`/`MEMBER`/`COLLABORATOR`）。
- **评论 / 标签 / 关闭**: `gh pr comment`、`gh pr edit --add-label`/`--remove-label`、`gh pr close`。

GitHub 的 issue 和 PR 共享编号空间，单独的 `#42` 可能是任一类型 —— 先用 `gh pr view 42`，失败则回退到 `gh issue view 42`。

## 当 skill 说“发布到 issue tracker”

创建一条 GitHub issue。

## Spec 与实现 ticket 的关联

- **Spec issue**：功能规格文档（PRD）类 issue 必须带 `spec` 标签（分类标签，见 `triage-labels.md`），与实现 ticket 区分。
- **子 ticket 必须通过 sub-issue 关联回父 spec**：从 spec 拆出的实现 ticket 一律用 GitHub 原生 sub-issue 挂到父 spec issue 下，并在 ticket 正文顶部写 `## Parent` 引用父 spec：
  ```bash
  # 1) 创建 ticket（依赖序：blocker 先建），拿到 issue 号
  gh issue create --label ready-for-agent --body-file t.md
  # 2) 获取子 issue 的数据库 ID（不是 #number）
  CHILD_ID=$(gh api repos/<owner>/<repo>/issues/<child-number> --jq .id)
  # 3) 挂到父 spec 下
  gh api --method POST repos/<owner>/<repo>/issues/<parent-number>/sub_issues \
    --input <(echo "{\"sub_issue_id\": $CHILD_ID}")
  ```
- 若 sub-issue 功能不可用（API 返回 404/501），回退为：子 ticket 正文顶部写 `Part of #<parent>`，并在父 spec 正文维护任务列表。
- 同一 spec 下的多个 ticket 也互为 sub-issue 关系。
- **阻塞关系必须用 GitHub 原生 blocked-by**（不要只写在正文里）：存在先后依赖时，用下文 Wayfinding 操作的「阻塞关系」命令添加原生依赖。原生依赖可被 `/wayfinder` 与查询自动识别（`issue_dependencies_summary.blocked_by`），正文文本 `Blocked by: #<n>` 只在原生依赖功能不可用（API 返回 404/501）时作为回退。
- **检查既有依赖**：`gh api graphql -f query='query{repository(owner:"<owner>",name:"<repo>"){issue(number:<n>){blockedBy(first:50){nodes{number title}} blocking(first:50){nodes{number title}}}}}' `。

## 当 skill 说“获取相关 ticket”

运行 `gh issue view <number> --comments`。

## Wayfinding 操作

供 `/wayfinder` 使用。**地图** 是一个 issue，**子 ticket** 也是 issue。

- **地图**: 一个带 `wayfinder:map` 标签的 issue，正文包含 Notes / Decisions-so-far / Fog。创建命令：`gh issue create --label wayfinder:map`。
- **子 ticket**: 通过 GitHub sub-issue 关联到地图的 issue。如果不支持 sub-issue，就在地图正文中添加任务列表，并在子 issue 正文顶部加上 `Part of #<map>`。标签：`wayfinder:<type>`（`research`/`prototype`/`grilling`/`task`）。被认领后，分配给负责人。
- **阻塞关系**: 必须使用 GitHub 原生 issue 依赖。使用 `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>` 添加依赖，其中 `<blocker-db-id>` 是阻塞者的**数据库 ID**（通过 `gh api repos/<owner>/<repo>/issues/<n> --jq .id` 获取，不是 `#number` 或 `node_id`）。GitHub 通过 `issue_dependencies_summary.blocked_by` 报告（仅统计 open 的阻塞者）。若依赖功能不可用，在子 issue 正文顶部使用 `Blocked by: #<n>, #<n>` 作为回退。当所有阻塞者都关闭时，ticket 才解除阻塞。
- **前沿查询**: 列出地图的 open 子 issue（`gh issue list --state open`，范围限定为地图的 sub-issues / 任务列表），剔除仍有 open 阻塞者（`issue_dependencies_summary.blocked_by > 0`，或 `Blocked by` 行中有 open issue）或已有负责人的项；按地图顺序取第一个。
- **认领**: `gh issue edit <n> --add-assignee @me` —— 这是会话中的第一次写入。
- **解决**: `gh issue comment <n> --body "<answer>"`，然后 `gh issue close <n>`，再将上下文指针（gist + 链接）追加到地图的 Decisions-so-far。
