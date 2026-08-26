# search_transactions（交易搜索索引）

交易模糊搜索的全文索引，基于 SQLite FTS5 虚拟表实现。它不是业务主表，而是 `transactions` 及其关联账户名的去规范化搜索视图。

## 设计原则

- **离线优先**：所有搜索在本地 SQLite 完成，不依赖远程服务。
- **与主表解耦**：FTS5 虚拟表只存可搜索文本和交易 ID 元数据，不重复存完整交易行。
- **软删除感知**：FTS5 本身不感知 `is_deleted`，查询时通过 JOIN 主表过滤。
- **不污染同步字段**：账户名变更导致的级联重建，使用独立的 `search_reindex_queue`，不动 `transactions.updated_at`。

## 虚拟表结构

```sql
CREATE VIRTUAL TABLE search_transactions USING fts5(
    content,           -- 拼接后的可搜索文本（contentful：存储副本以便维护与 rank 排序）
    transaction_id UNINDEXED  -- 回查主表用
);
```

> 使用 **contentful** 模式（非 contentless）：contentless 表删除文档必须携带原文列值、且无法使用 `rank`/`bm25` 排序，与「按相关度排序」需求冲突（ADR-0004 已确认决策 #12）。contentful 支持普通 `DELETE`/`UPDATE`/`INSERT OR REPLACE`，词条随操作干净增删；代价是重复存储 content（备注+账户名+拼音首字母，单条数百字节，几十万条规模约百 MB 级，可接受）。

## 可搜索内容生成规则

对每一笔非软删除交易， Rust 层生成如下 `content`：

```text
content = note || ' ' || account_name || ' ' || note_pinyin_initials || ' ' || account_name_pinyin_initials
```

其中 `account_name` 为转出账户名（`transactions.account_id` 关联的 `accounts.name`）。

示例：

| 字段 | 原始值 | 拼音首字母 |
|------|--------|------------|
| note | 吃饭 | cf |
| account_name | 招商银行 | zsyh |

生成的 `content`：

```text
吃饭 招商银行 cf zsyh
```

- 所有拼音首字母统一转小写（`pinyin_initials`：中文字符取拼音首字母，ASCII 字母/数字小写保留，其余字符跳过）。
- 空字段贡献为空字符串；所有字段为空时 content 为空串（仍保留文档行，便于后续补充）。
- **不在索引中**：分类名（`categories.name`）、转入账户名（转账 `to_account_id` 关联的账户名）。转账仅转出账户名可搜。分类改名、转入账户改名不影响搜索结果。

## 索引维护

### 搜索重建队列

由于 `content` 包含跨表的 `accounts.name`，账户名变更会级联影响大量交易。引入独立队列表：

```sql
CREATE TABLE search_reindex_queue (
    transaction_id TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL,          -- ISO 8601
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX idx_search_reindex_queue_enqueued_at ON search_reindex_queue(enqueued_at);
```

### 入队条件（由迁移 V005 中的触发器实现）

- 交易新增：`trg_search_enqueue_txn_insert` 立即入队。
- 交易更新（`note` / `account_id` / `is_deleted` 变化）：`trg_search_enqueue_txn_update` 入队（OLD/NEW 双入队，覆盖 account_id 变更）。
- 账户改名：`trg_search_enqueue_account_rename` 把该账户下所有 `is_deleted = 0` 的交易入队。
- 重复入队使用 `INSERT OR REPLACE` 覆盖，保证一行交易只有一条待重建记录。
- 分类改名、转账的 `to_account_id` 变更不入队（相关字段不在索引中）。

### 消费流程

1. 交易写入路径**不做同步索引工作**（ADR-0004 决策 #14：写路径零索引开销，界面操作不受影响）；触发器已入队 `search_reindex_queue`。
2. 后台刷新线程固定周期（默认 60s）检查队列，非空则按 `enqueued_at` 升序取出一批 `transaction_id` 批量消费重建；批量导入命令在事务提交后**立即消费一次**（导入是成批写入场景，一次性重建比等下一个周期更合理）。
3. 对每个交易重新生成 `content`：
   - 如果交易已软删除，从 `search_transactions` 中删除对应行。
   - 否则执行 `INSERT OR REPLACE` 写入索引。
4. 消费完成后从队列删除对应行。
5. 启动对账（`reconcile_search_index`）：FTS 文档数 ≠ 未删除交易数时全量重建（`rebuild_search_index`），否则消费队列。
6. 搜索结果附 `stale` 标志：队列非空（存在尚未消费的写入）时 `true`，前端提示索引可能滞后。写入后到下次刷新前，新建交易不可搜、软删除交易仍可搜（时效性要求低，可接受）。

### 与同步机制的边界

`search_reindex_queue` 和 `search_transactions` 只在本地存在，不参与设备间同步。跨设备同步仍由 `transactions`/`accounts`/`categories` 的 `updated_at` 与 `version` 驱动。其他设备在同步完成后，会各自维护自己的本地搜索索引。

## 查询模式

```sql
SELECT t.*
FROM search_transactions s
JOIN transactions t ON s.transaction_id = t.id
JOIN accounts a ON t.account_id = a.id
LEFT JOIN categories c ON t.category_id = c.id
WHERE search_transactions MATCH ?
  AND t.is_deleted = 0
  AND a.is_deleted = 0
  AND (c.is_deleted = 0 OR c.id IS NULL)
ORDER BY rank DESC, t.date DESC, t.created_at DESC, t.id DESC
LIMIT ? OFFSET ?;
```

- `MATCH` 参数由用户输入按空白分词后拼接（`build_match_query`）：每个词条生成 `"词条" OR "词条"*`（整词 + 前缀通配）并 OR，词条间 AND。如输入 `cf 午餐` → `("cf" OR "cf"*) AND ("午餐" OR "午餐"*)`。`"` 与 `*` 剥离防注入。
- 中文按连续汉字整词 token（unicode61 tokenizer），不支持词中片段（`商银` 搜不到「招商银行」），由拼音首字母前缀兜底。
- 排序先用 FTS5 内置 `rank`，再按交易日期倒序、`id` 兜底。
- 金额与日期筛选**已实现**（issue #40/#41）：`amount_min_cents` / `amount_max_cents`（整数分，含边界，单边可用）与 `date_from` / `date_to`（`YYYY-MM-DD` 字符串比较，含边界），与关键字 AND 组合；仅筛选（无关键字）查询不 JOIN FTS 虚拟表，直接走主表 B-tree 索引（`idx_transactions_amount` V006 / `idx_transactions_date` V001）。
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

- ADR-0004：`docs/adr/0004-fuzzy-search-transactions.md`
- `docs/adr/glossary-fuzzy-search.md`
- Migration：`src-tauri/migrations/V005__search_index.sql`
