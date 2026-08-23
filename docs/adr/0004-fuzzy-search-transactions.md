# ADR-0004: 记账模糊搜索（交易）

## 状态

已接受（2026 年实现：独立「搜索」视图 + FTS5 + 拼音首字母）

## 背景

Ledger 当前采用 SQLite 离线优先架构，核心交易表 `transactions` 是资金流动的中心记录。随着交易数量增加，用户需要通过关键字快速定位历史交易，包括：

- 交易备注（`note`）
- 关联账户名称（`accounts.name`）
- 关联分类名称（`categories.name`）
- 金额区间与日期范围

现有模型仅在 ID、日期、账户、分类上建有 B-tree 索引，`transactions.note` 没有索引，无法高效支持模糊搜索。

## 决策

1. **搜索范围第一阶段聚焦交易**：全局搜索入口先支持交易搜索，账户/分类/投资标的的独立搜索在后续阶段扩展。
2. **使用 SQLite FTS5 虚拟表**：新建 `search_transactions` 作为全文搜索索引，与主表解耦。
3. **可搜索内容去规范化**：将 `note + account_name + category_name` 拼接为单一 `content` 列，便于一次性全文匹配。
4. **拼音首字母匹配**：在 `content` 中额外加入账户名、分类名、备注的拼音首字母串，支持输入如 `cf` 匹配「吃饭」。
5. **金额/日期筛选走主表**：FTS5 负责文本匹配，再通过 `transaction_id` JOIN 回 `transactions` 用 B-tree 索引过滤 `amount_cents` 与 `date`。
6. **软删除同步过滤**：搜索结果始终 JOIN `accounts`/`categories` 并限定 `is_deleted = 0`；当交易、账户或分类被软删除时，同步从 FTS 索引中移除对应文档。

## 模型设计

### 搜索文档表（FTS5 虚拟表）

```sql
CREATE VIRTUAL TABLE search_transactions USING fts5(
    content,           -- 拼接后的可搜索文本
    transaction_id,    -- UNINDEXED，用于回查主表
    content_row='',
    content=''
);
```

> 注：使用 `contentless` 表， searchable text 只保留在 FTS5 索引中，不重复存一份完整文档；`transaction_id` 作为 UNINDEXED 元数据，用于关联回主表。

### 可搜索内容生成规则

对每一笔非删除交易，生成：

```
content = note || ' ' || account_name || ' ' || category_name || ' '
          || account_name_pinyin_initials || ' '
          || category_name_pinyin_initials || ' '
          || note_pinyin_initials
```

- 拼音首字母由 Rust 层在写入索引时生成（例如「招商银行」→ `zsyh`）。
- 所有字段为空时，`content` 为空字符串，仍保留一行以便后续补充文本。

### 索引维护策略

由于 `content` 包含跨表字段（账户名、分类名），无法仅通过 `transactions` 上的触发器自动维护，采用**应用层重建 + 关键触发器兜底**的混合策略：

1. **事务级维护**：
   - 新增/修改/删除交易后，应用层立即调用 `reindex_transaction(transaction_id)`。
   - 软删除交易时同步删除 FTS 文档。
2. **级联维护**：
   - 修改 `accounts.name` 或 `categories.name` 后，应用层批量重建受影响交易的索引。
3. **触发器兜底**：在 `transactions`/`accounts`/`categories` 上建 AFTER 触发器，调用应用层注册的同步函数（Tauri 命令），确保通过其他入口写入的数据也能更新索引。

### 查询模式

```sql
SELECT t.*, a.name AS account_name, c.name AS category_name
FROM search_transactions s
JOIN transactions t ON s.transaction_id = t.id
JOIN accounts a ON t.account_id = a.id AND a.is_deleted = 0
LEFT JOIN categories c ON t.category_id = c.id AND c.is_deleted = 0
WHERE search_transactions MATCH ?
  AND t.is_deleted = 0
  AND t.amount_cents BETWEEN ? AND ?
  AND t.date BETWEEN ? AND ?
ORDER BY rank DESC, t.date DESC
LIMIT ?;
```

- `MATCH` 参数由用户输入分词后拼接（如 `cf 吃饭` → `cf OR 吃饭`）。
- 金额与日期筛选在 JOIN 后完成，复用现有 B-tree 索引。
- 排序先用 FTS5 内置 `rank`，再按交易日期倒序。

## 影响

- 需要引入一个拼音首字母生成库（Rust 侧）。
- 需要新增 FTS5 维护相关的 Tauri 命令和迁移脚本。
- 搜索入口 UI 需要独立视图；结果列表展示交易日期、账户、分类、金额、备注摘要。
- 后续扩展全局搜索（账户、分类、投资标的）时，可复用同一模式：为每个实体建独立 FTS5 表或统一 `search_documents` 表。

## 备选方案

| 方案 | 优点 | 缺点 | 结论 |
|------|------|------|------|
| `LIKE '%keyword%'` | 实现最简单 | 无法走索引，数据量大时性能差 | 否决 |
| 自维护倒排表 | 灵活，可完全控制拼音逻辑 | 实现复杂，需要维护分词、词频、排名 | 暂否决，未来数据规模超大时考虑 |
| FTS5 外部内容表 | 直接映射 `transactions` 列 | 无法自动包含账户名/分类名 | 否决 |
| FTS5 contentless 表 + 应用层维护 | 查询快，结构清晰，跨表字段可控 | 需要应用层维护同步 | 采纳 |

## 已确认决策

1. 搜索范围：全局搜索入口，第一阶段只支持交易，但包含全部交易类型（不区分普通交易与投资交易）。
2. 实现方式：SQLite FTS5 contentless 虚拟表 `search_transactions`。
3. 可搜索内容：交易备注 + 账户名 + 分类名 + 对应拼音首字母。
4. 拼音匹配：统一转小写存储与查询，仅支持首字母匹配（如 `cf` 匹配「吃饭」）。
5. 金额/日期筛选：通过 JOIN 回 `transactions` 主表用现有 B-tree 索引过滤。
6. 账户/分类名变更：使用独立的 `search_reindex_queue` 表做延迟批量重建，避免污染同步字段。
7. 脏数据消费：改名后延迟几秒异步消费，用户无感知。
8. 搜索结果：不展示高亮，只展示交易信息。
9. **tokenizer**：明确使用默认 `unicode61`（不引入 trigram）。中文按连续汉字整词 token 匹配；查询时对每个词条附加前缀通配（如「吃」→ `吃*`），覆盖「记得词首」场景；「记得词中任意片段」不支持，由拼音首字母前缀兜底。
10. **触发器修正**：原设计"调用应用层注册的同步函数（Tauri 命令）"不可行——SQLite 触发器无法调用 Rust 代码。改为触发器纯 SQL 向 `search_reindex_queue` 插入 `transaction_id`，由应用层异步消费重建，与第 6/7 条 queue 设计一致。
11. **服务端分页**：搜索命令带 `LIMIT/OFFSET` 并返回命中总数，供前端分页与"命中 N 条"展示；排序按 `rank` 优先、`date` 倒序次之。

## 模型补充：搜索重建队列

由于账户名/分类名变更会级联影响大量交易的搜索内容，且不能通过修改 `transactions.updated_at` 触发同步，因此引入独立的搜索索引队列：

```sql
CREATE TABLE search_reindex_queue (
    transaction_id TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL,          -- ISO 8601，入队时间
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX idx_search_reindex_queue_enqueued_at ON search_reindex_queue(enqueued_at);
```

### 入队条件

- 交易新增/修改/软删除：立即入队。
- 账户名修改：批量把该账户下所有 `is_deleted = 0` 的交易入队。
- 分类名修改：批量把该分类下所有 `is_deleted = 0` 的交易入队。
- 重复入队时使用 `INSERT OR REPLACE` 覆盖，保证同一交易只有一行。

### 消费流程

1. 账户/分类改名事务提交后，触发一个延迟任务（如 3 秒后执行）。
2. 消费端从队列中按 `enqueued_at` 升序取出一批 `transaction_id`。
3. 对每个交易重新生成 `content` 并写入/更新 `search_transactions`。
4. 软删除交易从 FTS 索引中物理删除。
5. 消费完成后删除队列中对应行。

### 与同步机制的边界

`search_reindex_queue` 不参与设备间同步。它只在本地数据库存在，用于协调本地搜索索引与主表的一致性。跨设备同步仍只由 `transactions.updated_at` / `version` 驱动。

## 参考

- [SQLite FTS5](https://www.sqlite.org/fts5.html)
- `docs/model/basic/transactions.md`
- `docs/model/basic/accounts.md`
- `docs/model/basic/categories.md`
