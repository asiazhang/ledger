-- V006 导入去重：transactions 增加 dedup_hash 列
-- 去重是应用层行为（导入入口），因此不建唯一索引、可空。
-- 哈希：sha256("date|kind|amount_cents|currency_code|account_id|to_account_id")
ALTER TABLE transactions ADD COLUMN dedup_hash TEXT;
