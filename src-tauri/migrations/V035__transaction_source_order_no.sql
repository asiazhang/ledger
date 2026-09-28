-- V035：交易来源订单号列（issue #1862 / ADR-0138 决策 9）。
--
-- 来源订单号（Source Order No）：导入来源（如电商回单）携带的外部订单标识，
-- 落交易行可空列——订单可读性（行尾订单徽章、详情订单区）的行级归属锚点。
-- 纯增量：存量行零迁移（手动记账自然为 NULL，无徽章、无订单区，是可选增强
-- 的自然边界）；无默认值、无 NOT NULL、无 CHECK——来源元数据不承载资金语义，
-- schema 层不设 kind 准入（比照商户列先例，词汇表「来源订单号」）。
--
-- 部分索引服务订单汇总只读命令的同单行集下推（`WHERE source_order_no = ? AND
-- is_deleted = 0`）：NULL 键行（绝大多数手动行）不进索引，形态比照
-- idx_transactions_funding（V023）与 idx_transactions_policy（V013）。

ALTER TABLE transactions ADD COLUMN source_order_no TEXT;

CREATE INDEX IF NOT EXISTS idx_transactions_source_order
    ON transactions(source_order_no)
    WHERE source_order_no IS NOT NULL AND is_deleted = 0;
