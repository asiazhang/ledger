-- V017 余额与净资产缓存（issue #491 / 父 #489 B2，ADR-0066）
--
-- 推翻 ADR-0019「个人账本规模下持久化收益趋零」前提（50 万笔实测两项读基准
-- p95 ~88s），以持久化缓存消灭读路径全量聚合：账户余额与净资产总览读缓存，
-- 写路径在既有事务内对受影响账户按唯一口径表达式整体重算（禁止增量加减），
-- 净资产由读探针指纹自愈（无定时任务）。
--
-- 两表均为派生缓存（单一事实来源恒为 accounts/transactions 及各贡献表）：
-- 污染/缺失经手动审计命令修复，不参与同步、不承载独立业务语义。

-- 账户余额缓存：每账户一行，balance_cents 恒等于
--   initial_balance_cents + Σ account_flow(转出侧) + Σ account_flow(转入侧)
-- 回填使用与 Rust `account_flow_expr` 同一矩阵的唯一口径表达式
-- （income/refund/sell/dividend 为 +，expense/buy 为 −，transfer 按侧取号，
-- split 恒 0；is_deleted=0）。迁移是一次性冻结产物，文本与代码侧表达式
-- 的一致性由迁移回填测试（回填值 == 实时重算）锁定。
CREATE TABLE account_balance_cache (
    account_id    TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    balance_cents INTEGER NOT NULL,
    updated_at    TEXT NOT NULL                           -- 最后重算时刻（毫秒精度，本地时钟）
);

INSERT INTO account_balance_cache (account_id, balance_cents, updated_at)
SELECT a.id,
       a.initial_balance_cents
       + COALESCE((SELECT SUM(CASE
             WHEN t.kind IN ('income','refund','sell','dividend') THEN t.amount_native_cents
             WHEN t.kind IN ('expense','transfer','buy') THEN -t.amount_native_cents
             ELSE 0 END)
         FROM transactions t WHERE t.is_deleted = 0 AND t.account_id = a.id), 0)
       + COALESCE((SELECT SUM(CASE
             WHEN t.kind IN ('income','transfer','refund','sell','dividend') THEN t.amount_native_cents
             WHEN t.kind IN ('expense','buy') THEN -t.amount_native_cents
             ELSE 0 END)
         FROM transactions t WHERE t.is_deleted = 0 AND t.to_account_id = a.id), 0),
       strftime('%Y-%m-%dT%H:%M:%S', 'now') || '.000Z'
FROM accounts a;

-- 净资产终值缓存：单例行（id=1）。fingerprint 为各贡献表 MAX(updated_at)
-- 组合（实物资产估值表为 append-only，取 MAX(created_at)），由读探针在
-- 读取时比对——不匹配即调既有实时聚合重算回填，故迁移不回填本表
-- （首次读取即自愈完成首次回填）。
CREATE TABLE net_worth_cache (
    id                          INTEGER PRIMARY KEY CHECK (id = 1),
    fingerprint                 TEXT NOT NULL,
    native_currency             TEXT NOT NULL,
    net_worth_cents             INTEGER NOT NULL,
    accounts_balance_cents      INTEGER NOT NULL,
    holdings_market_value_cents INTEGER NOT NULL,
    physical_assets_value_cents INTEGER NOT NULL,
    updated_at                  TEXT NOT NULL
);
