# search_transactions（交易搜索索引）

交易模糊搜索的全文索引，基于 SQLite FTS5 虚拟表实现。它不是业务主表，而是 `transactions` 及其关联账户、分类名的去规范化搜索视图。

## 设计原则

- **离线优先**：所有搜索在本地 SQLite 完成，不依赖远程服务。
- **与主表解耦**：FTS5 虚拟表只存可搜索文本和交易 ID 元数据，不重复存完整交易行。
- **软删除感知**：FTS5 本身不感知 `is_deleted`，查询时通过 JOIN 主表过滤。
- **不污染同步字段**：账户/分类名变更导致的级联重建，使用独立的 `search_reindex_queue`，不动 `transactions.updated_at`。

## 虚拟表结构

```sql
CREATE VIRTUAL TABLE search_transactions USING fts5(
    content,           -- 可搜索文本
    transaction_id,    -- UNINDEXED，用于回查主表
    content_row='',
    content=''
);
```

> 使用 `contentless` 模式，只保留 FTS5 索引，不重复保存一份完整文档。`transaction_id` 标记为 `UNINDEXED`，避免被误用于全文匹配。

## 可搜索内容生成规则

对每一笔非软删除交易， Rust 层生成如下 `content`：

```text
content = note || ' ' || account_name || ' ' || category_name || ' '
          || account_name_pinyin_initials || ' '
          || category_name_pinyin_initials || ' '
          || note_pinyin_initials
```

示例：

| 字段 | 原始值 | 拼音首字母 |
|------|--------|------------|
| note | 吃饭 | cf |
| account_name | 招商银行 | zsyh |
| category_name | 餐饮 | cy |

生成的 `content`：

```text
吃饭 招商银行 餐饮 zsyh cy cf
```

- 所有拼音首字母统一转小写。
- 空字段贡献为空字符串，多个空格会被 FTS5 分词器忽略。

## 索引维护

### 搜索重建队列

由于 `content` 包含跨表的 `accounts.name` 和 `categories.name`，账户/分类名变更会级联影响大量交易。引入独立队列表：

```sql
CREATE TABLE search_reindex_queue (
    transaction_id TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL,          -- ISO 8601
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX idx_search_reindex_queue_enqueued_at ON search_reindex_queue(enqueued_at);
```

### 入队条件

- 交易新增、修改、软删除：立即入队。
- 账户名修改：批量把该账户下所有 `is_deleted = 0` 的交易入队。
- 分类名修改：批量把该分类下所有 `is_deleted = 0` 的交易入队。
- 重复入队使用 `INSERT OR REPLACE` 覆盖，保证一行交易只有一条待重建记录。

### 消费流程

1. 触发条件：交易写入完成后，或账户/分类名变更后延迟几秒（如 3 秒）。
2. 消费端按 `enqueued_at` 升序取出一批 `transaction_id`。
3. 对每个交易重新生成 `content`：
   - 如果交易已软删除，从 `search_transactions` 中删除对应行。
   - 否则执行 `INSERT OR REPLACE` 写入索引。
4. 消费完成后从队列删除对应行。

### 与同步机制的边界

`search_reindex_queue` 和 `search_transactions` 只在本地存在，不参与设备间同步。跨设备同步仍由 `transactions`/`accounts`/`categories` 的 `updated_at` 与 `version` 驱动。其他设备在同步完成后，会各自维护自己的本地搜索索引。

## 查询模式

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

- `MATCH` 参数由用户输入分词后拼接。例如输入 `cf 吃饭` 会转换为 `cf OR 吃饭`，同时命中拼音首字母和原始中文。
- 金额与日期筛选在 JOIN 回主表后完成，复用 `idx_transactions_date` 与 `idx_transactions_amount`（如金额索引不存在则需新增）。
- 排序先用 FTS5 内置 `rank`，再按交易日期倒序。
- 不返回高亮片段，结果列表只展示交易信息。

## 索引

- `search_transactions` 本身即 FTS5 全文索引。
- `idx_search_reindex_queue_enqueued_at`：按入队时间消费。

## 被引用关系

- `search_transactions.transaction_id` → `transactions.id`（逻辑关联，无 FK）
- `search_reindex_queue.transaction_id` → `transactions.id`（ON DELETE CASCADE）

## 扩展性

- 后续支持账户/分类/投资标的独立搜索时，可按相同模式新增 `search_accounts`、`search_categories`、`search_instruments` 虚拟表与对应队列。
- 若需要更复杂的拼音逻辑（如全拼、同音字），可将 `content` 进一步扩展，或引入 `pinyin` 表做单独匹配。

## 参考

- ADR-005：`docs/adr/005-fuzzy-search-transactions.md`
- `docs/adr/glossary-fuzzy-search.md`
- Migration：待新增（`V00X__search_transactions.sql`）
