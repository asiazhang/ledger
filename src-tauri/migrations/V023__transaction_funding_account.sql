-- V023：buy/sell 出资账户列（issue #935 / ADR-0096）。加列只增，不改任何既有对象。
--
-- funding_account_id：buy/sell 交易可选的第二账户——结算现金实际流出（买入）/
-- 流入（卖出）的账户，解决「钱直接从银行卡变成份额」类直扣场景的资金流可见与
-- 余额正确。归因规则单条（ADR-0096 决策 2）：结算账户 = 出资账户 ?? 投资账户
-- （account_id）；buy 对结算账户记 −、sell 记 +，kind 矩阵符号不变、归因端点可变；
-- 出资账户命中时投资账户现金腿为 0。
--
-- 准入闭集（行为层收口，issue #935）：现金类账户（cash/bank/credit/ewallet/other）；
-- 排除投资账户与 receivable/debt；币种必须与交易币种一致；仅 buy/sell 可携带。
-- schema 层只设可空外键，不设 kind/类型约束（与 merchant_id/policy_id 同款纪律）。
--
-- 外键动作比照 account_id（RESTRICT，强依赖）：交易行的账户引用不容账户硬删悬空；
-- 软删前提不受影响（主业务表删除一律软删，RESTRICT 仅在显式硬删时生效）。
-- 出资账户为可空溯源引用，不参与导入去重唯一索引（去重哈希纳入出资账户是应用层
-- 行为，ADR-0096 决策 8，历史行不受影响）。

ALTER TABLE transactions ADD COLUMN funding_account_id TEXT REFERENCES accounts(id) ON DELETE RESTRICT;

-- 出资端余额聚合与按出资账户过滤的覆盖索引：部分索引只收带出资账户的行
-- （存量行/未出资行不进索引，与 idx_transactions_dedup_hash 同款纪律）。
CREATE INDEX IF NOT EXISTS idx_transactions_funding
    ON transactions(funding_account_id)
    WHERE funding_account_id IS NOT NULL;
