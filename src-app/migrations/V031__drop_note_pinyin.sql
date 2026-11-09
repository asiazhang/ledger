-- V031 note_pinyin 派生列退役：交易搜索退回原文（issue #1728 / 父 #1725，
-- ADR-0027 修订记录「语义契约定稿」条目）
--
-- 背景：#1725 grilling 定案——拼音首字母子序列语义在几十万行交易候选集上区分度
-- 崩塌（`zs` 命中一切含 z…s 序的条目），TransactionSearch 退回原文搜索；拼音可搜
-- 语义收窄归下拉侧（PinyinSelect / pinyin-filter，候选集小、字典内检索，不受影
-- 响）。#1727 已拆除读路径拼音分支与 Writer 派生列维护，本迁移收敛 schema：
-- 已发布迁移 V018 按 ADR-0027 决策 4 修订注记走追加收敛迁移（迁移文件只增不改，
-- 存量库升级自动收敛、无感；CHANGELOG「Unreleased」BREAKING 条目随本迁移落）。
--
-- 收敛顺序（不可换：SQLite DROP COLUMN 拒绝被索引引用或出现在 partial 索引谓词
-- 中的列）：
-- 1. DROP 回填探针 partial 索引——#1727 拆除惰性回填后恒空、零消费者；
-- 2. DROP 搜索覆盖索引（V018 引入，原定义含 note_pinyin）；
-- 3. DROP COLUMN note_pinyin——Writer 自 #1727 停写、读路径停读，列为纯派生冗
--    余（单一事实来源恒为 note），不承载独立语义，数据随列一并移除；
-- 4. 重建搜索覆盖索引：列清单去掉 note_pinyin，形态保持「列表序键 + id/note/
--    三引用列」、partial 谓词 WHERE is_deleted = 0 不变——下推查询
--    INDEXED BY 钉定（search.rs `build_stage1_query`）仍 index-only 命中且无
--    ORDER BY 临时 B-tree（perf.rs 计划钉定测试随迁同步更新）。
--
-- 迁移尾部刷新统计（V016/V025/V030 先例）：索引集合与定义变化后重算
-- sqlite_stat1，planner 在重建索引上的选择基于与 schema 一致的统计；新装库空表
-- 统计近似空，应用侧经批量导入后的 PRAGMA optimize 逐步收敛（issue #490）。

DROP INDEX idx_transactions_note_pinyin_backlog;
DROP INDEX idx_transactions_note_search;

ALTER TABLE transactions DROP COLUMN note_pinyin;

CREATE INDEX IF NOT EXISTS idx_transactions_note_search
    ON transactions(date, created_at, id, note, account_id, merchant_id, category_id)
    WHERE is_deleted = 0;

ANALYZE;
