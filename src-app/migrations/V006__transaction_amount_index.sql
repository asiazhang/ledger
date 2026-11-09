-- V006 交易金额筛选索引（issue #40）
--
-- 【就地修改注记】本文件头部注释已被就地修改（issue #395）：仅注释，DDL 零变化——
-- 新旧库执行本迁移所得 schema 完全一致，无存量库/新装库分叉，不入两级 BREAKING
-- 标记（该标记仅适用于产生 schema 分叉的就地修改）。
-- 修改原因：search_transactions 的金额区间筛选改按本位币分（amount_native_cents）
-- 过滤，与全仓聚合口径同源；实测（EXPLAIN QUERY PLAN）改后金额筛选与日期筛选同
-- 计划（idx_transactions_deleted 定位 + 排序临时 B-tree），本索引不再被任何查询选用。
-- 建索引当时的背景（已失效，留作历史）：金额区间筛选（amount_min_cents /
-- amount_max_cents）彼时按原始币种分值过滤（与 amount_native_cents 1:1），
-- 「仅筛选」查询可选用本 B-tree 索引。列仍为 amount_cents 不回收，保留存量形状。

CREATE INDEX IF NOT EXISTS idx_transactions_amount
    ON transactions(amount_cents);
