-- V006 交易金额筛选索引（issue #40）
--
-- search_transactions 支持金额区间筛选（amount_min_cents / amount_max_cents，
-- 按原始币种分值过滤，MVP 阶段与 amount_native_cents 1:1）。
-- 「仅筛选」查询（无关键字）直接扫描 transactions 主表，金额 B-tree 索引可被
-- 查询规划器选用；日期筛选复用已有 idx_transactions_date（V001），无需重复建索引。

CREATE INDEX IF NOT EXISTS idx_transactions_amount
    ON transactions(amount_cents);
