# Triage Labels

技能使用五个标准 triage 角色。本文件将这些角色映射到本仓库 issue tracker 中实际使用的标签字符串。

| mattpocock/skills 中的标签 | 我们 tracker 中的标签 | 含义 |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | 维护者需要评估此 issue |
| `needs-info`               | `needs-info`         | 等待报告者补充信息 |
| `ready-for-agent`          | `ready-for-agent`    | 已完全明确，可交给 AFK agent |
| `ready-for-human`          | `ready-for-human`    | 需要人工实现 |
| `wontfix`                  | `wontfix`            | 不会处理 |

当技能提到某个角色（例如“应用 AFK-ready triage label”）时，请使用本表中对应的标签字符串。

如需匹配实际使用的标签词汇，编辑右侧列。

## 分类标签（非 triage 角色）

| 标签 | 含义 |
| ---- | ---- |
| `spec` | 功能规格文档（PRD）：标注 spec 类 issue，与实现 ticket 区分 |

`spec` 标签用于父 spec issue；由其拆出的实现 ticket 不重复加 `spec`（只加 triage 标签，并按下文 `issue-tracker.md` 约定以 sub-issue 关联回父 spec）。
