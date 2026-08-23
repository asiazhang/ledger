-- V007 交易客户端幂等键（issue #49）
--
-- 批量导入以客户端提供的 `idempotency_key` 为主做去重（内容无关），内容哈希
-- (`dedup_hash`) 降为无键行的兼容兜底。幂等键唯一性由部分唯一索引兜底：
-- "一键至多一活交易"（未删除范围内同键唯一），批量去重查询亦由此索引命中，
-- 不再对每行做全表扫描（ADR-0010）。

-- 幂等键：客户端提供的、内容无关的稳定标识（指向"该交易来自源文件哪一行"）。
-- 可空（无键行走内容哈希兜底），不建普通唯一索引——唯一性由下述部分唯一索引保证。
ALTER TABLE transactions ADD COLUMN idempotency_key TEXT;

-- 部分唯一索引：未删除范围内一键一活。软删除（is_deleted=1）不占去重位，
-- 重跑导入可重新写入；客户端造键重复时立即得到数据库约束错误，暴露造键 bug。
CREATE UNIQUE INDEX IF NOT EXISTS idx_transactions_idempotency_key
    ON transactions(idempotency_key)
    WHERE idempotency_key IS NOT NULL AND is_deleted = 0;
