# Domain Docs

工程技能在探索代码库时应如何消费本仓库的领域文档。

## 探索前先阅读

- 仓库根目录的 **`CONTEXT.md`**，或者
- 仓库根目录的 **`CONTEXT-MAP.md`**（如果存在）—— 它会指向每个上下文的 `CONTEXT.md`。阅读与主题相关的每一个。
- **`docs/adr/`** —— 阅读与你即将工作区域相关的 ADR。在多上下文仓库中，还要检查 `src/<context>/docs/adr/` 中的上下文级决策。

如果上述文件不存在，**静默继续**。不要提示缺失；也不要建议预先创建。`/domain-modeling` 技能（通过 `/grill-with-docs` 和 `/improve-codebase-architecture` 触发）会在术语或决策真正被确定时惰性创建它们。

## 文件结构

单上下文仓库（大多数仓库）：

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-event-sourced-orders.md
│   └── 0002-postgres-for-write-model.md
└── src/
```

多上下文仓库（根目录存在 `CONTEXT-MAP.md`）：

```
/
├── CONTEXT-MAP.md
├── docs/adr/                          ← 系统级决策
└── src/
    ├── ordering/
    │   ├── CONTEXT.md
    │   └── docs/adr/                  ← 上下文特定决策
    └── billing/
        ├── CONTEXT.md
        └── docs/adr/
```

## 使用术语表词汇

当输出中命名一个领域概念（issue 标题、重构提案、假设、测试名）时，使用 `CONTEXT.md` 中定义的术语。不要漂移为术语表明确避免的同义词。

如果你需要的概念不在术语表中，这是一个信号 —— 要么你在发明项目不使用的语言（请重新考虑），要么确实存在缺口（记录给 `/domain-modeling`）。

## 标记 ADR 冲突

如果输出与现有 ADR 冲突，请显式指出，而不是静默覆盖：

> _与 ADR-0007（event-sourced orders）冲突 —— 但鉴于……值得重新讨论。_
