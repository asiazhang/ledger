-- V005 交易搜索索引：FTS5 全文索引 + 重建队列 + 触发器（ADR-0004 交易搜索）
--
-- 设计说明：
-- 1. search_transactions：FTS5 虚拟表（contentful，存储拼接后的可搜索文本副本）。
--    原 ADR 决策为 contentless（content=''），但实现中确认 contentless 表存在两个
--    与需求冲突的限制（见 ADR-0004 已确认决策 #12）：
--       a) 删除文档必须携带原文列值（'delete' 特殊命令），而 contentless 无法回读
--          已存内容，导致仅按 rowid 删除时索引词条残留、rowid 复用后旧词条复活；
--       b) contentless 表无法使用 rank/bm25 排序，与「按相关度排序」需求冲突。
--    故改为 contentful：支持普通 DELETE / UPDATE / INSERT OR REPLACE，词条随操作
--    干净增删；rank 排序可用；代价是重复存储 content（备注+账户名+分类名+拼音首字母，
--    单条数百字节，几十万条规模约百 MB 级，可接受）。
--    content 由应用层拼接：备注 + 账户名 + 分类名 + 三者拼音首字母（仅首字母、小写）。
-- 2. search_reindex_queue：搜索重建队列。交易增删改、账户/分类改名时由触发器
--    纯 SQL 入队受影响交易，应用层消费重建 FTS 文档（重复入队 INSERT OR REPLACE 覆盖）。
--    队列不参与设备间同步，仅协调本地搜索索引与主表一致性（ADR-0004）。
-- 3. 触发器只入队、不调用应用层代码（SQLite 触发器无法调用 Rust）。

CREATE VIRTUAL TABLE IF NOT EXISTS search_transactions USING fts5(
    content,             -- 拼接后的可搜索文本（contentful，存储副本以便维护与排序）
    transaction_id UNINDEXED  -- 回查主表用
);

CREATE TABLE IF NOT EXISTS search_reindex_queue (
    transaction_id TEXT PRIMARY KEY,
    enqueued_at TEXT NOT NULL,          -- ISO 8601，入队时间
    FOREIGN KEY (transaction_id) REFERENCES transactions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_search_reindex_queue_enqueued_at
    ON search_reindex_queue(enqueued_at);

-- 交易插入 → 入队
CREATE TRIGGER IF NOT EXISTS trg_search_enqueue_txn_insert
AFTER INSERT ON transactions
BEGIN
    INSERT OR REPLACE INTO search_reindex_queue(transaction_id, enqueued_at)
    VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;

-- 交易内容字段更新（备注/账户/分类/软删除标志）→ 入队。
-- OLD/NEW 双入队，覆盖 account_id / category_id 变更导致的可搜索内容变化。
CREATE TRIGGER IF NOT EXISTS trg_search_enqueue_txn_update
AFTER UPDATE OF note, account_id, to_account_id, category_id, is_deleted ON transactions
BEGIN
    INSERT OR REPLACE INTO search_reindex_queue(transaction_id, enqueued_at)
    VALUES (OLD.id, strftime('%Y-%m-%dT%H:%M:%SZ','now'));
    INSERT OR REPLACE INTO search_reindex_queue(transaction_id, enqueued_at)
    VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;

-- 账户改名 → 该账户全部未删除交易入队（可搜索内容含账户名）
CREATE TRIGGER IF NOT EXISTS trg_search_enqueue_account_rename
AFTER UPDATE OF name ON accounts
BEGIN
    INSERT OR REPLACE INTO search_reindex_queue(transaction_id, enqueued_at)
    SELECT id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
    FROM transactions WHERE account_id = NEW.id AND is_deleted = 0;
END;

-- 分类改名 → 该分类全部未删除交易入队（可搜索内容含分类名）
CREATE TRIGGER IF NOT EXISTS trg_search_enqueue_category_rename
AFTER UPDATE OF name ON categories
BEGIN
    INSERT OR REPLACE INTO search_reindex_queue(transaction_id, enqueued_at)
    SELECT id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
    FROM transactions WHERE category_id = NEW.id AND is_deleted = 0;
END;
