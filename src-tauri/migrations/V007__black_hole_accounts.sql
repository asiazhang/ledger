-- V007 黑洞账户：accounts 增加 is_hidden 列 + 预置种子账户
--
-- 黑洞账户用于承接「资金账户=无」的导入交易，作为数据修正的缓冲池：
-- 交易照常写入并出现在交易列表/报表，但账户本身对用户隐藏。
-- 使用确定性 UUID v5（基于 name+currency_code）保证多设备种子一致。

ALTER TABLE accounts ADD COLUMN is_hidden INTEGER NOT NULL DEFAULT 0 CHECK(is_hidden IN (0, 1));

INSERT OR IGNORE INTO accounts (id, name, type, currency_code, initial_balance_cents, created_at, updated_at, version, device_id, is_deleted, is_hidden) VALUES
  ('6f51d386-8c74-5bc6-9176-a4a7e09ae1d5', '无(CNY)', 'other', 'CNY', 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed', 0, 1),
  ('325c4ee3-77ea-5352-8521-15989b0a815b', '无(HKD)', 'other', 'HKD', 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed', 0, 1);
