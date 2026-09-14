-- V025 transactions 索引清理：DROP 6 个零消费者/语义重叠索引 + 补保单聚合
-- 部分覆盖索引（issue #1300）
--
-- 对 transactions 表全部索引做消费方核对（608MB / 50 万笔基准库 EXPLAIN QUERY
-- PLAN 实测 + 全库查询面枚举，查询形状取自各 crate 真实 SQL，全量证据见
-- issue #1300 评论区）：每行交易写入要对每棵 B 树各做一次插入，是写路径真实
-- 成本（bench-import 实测口径，#1295 修复后量测）的构成之一。本迁移删除 4 个
-- 零消费者索引与 2 个被 V016 覆盖索引接管的语义重叠索引，并补 1 个只收保单
-- 流水的部分覆盖索引——净减 5 棵写路径 B 树（transactions 命名索引 19 → 14），
-- 全部写入口（手工/导入/计划/投资）共同受益。保留索引的既有计划由
-- crates/infra/src/db/tests/perf.rs 的 V025 前置断言先行钉住（断言在含全部旧
-- 索引的 schema 上写就、本迁移落地后保持绿；误删任一保留索引即变红）。
--
-- DROP 清单（全部为全库查询面零命中的 EXPLAIN 实测）：
-- - idx_transactions_amount（V006）：搜索金额筛选改按 amount_native_cents 后
--   不再被任何查询选用（V006 头注释已自认，#395 就地修改注记）；实测该筛选
--   由 idx_transactions_to_account_flow（ANY 前缀 + 金额区间）接管。
-- - idx_transactions_refund（V001）：全库不存在 refund_of_transaction_id 的
--   WHERE 反查（该列仅出现在 SELECT 输出列与 UPDATE SET）；退款创建按原支出
--   id 走主键。
-- - idx_transactions_sync（V001）：多端同步实际走 OpLog 写时追加/重放
--   （ADR-0091），同步引擎对 transactions 无任何 updated_at 差量查询；其历史
--   假设形状（is_deleted=0 AND updated_at>?）由 partial 结构索引的谓词内联
--   吸收，无计划退化。
-- - idx_transactions_account（V001）：账户筛选走 account_date、余额聚合走
--   account_flow/to_account_flow、账户引用守卫走双现金流索引（见下），EXPLAIN
--   全部绕开；无「按账户反查含删行」的代码路径。
-- - idx_transactions_category（V001）：与 idx_transactions_category_covering
--   语义重叠；删除后分类筛选列表与 category_id IS NULL 筛选由覆盖索引接管
--   （covering 无回表、计划形状不变），分类聚合本来就走覆盖索引。
-- - idx_transactions_deleted（V001）：唯一消费者是账户币种锁守卫的
--   is_deleted 前缀段扫（accounts/src/core.rs，本次同批改写为双 EXISTS 走
--   account_flow/to_account_flow 双索引：608MB 基准库实测原形状在无引用账户
--   的最坏情况 ~0.8s——段扫全索引且逐行回表，改写后两次 B 树等值定位），
--   updated_at 后缀零消费。
--
-- CREATE 清单（issue #1300 评估项 3）：
-- - idx_transactions_policy：保单统计（policy/src/stats.rs `sum_by_policy`，按
--   policy_id 聚合 JOIN）原计划经 month_expr ANY 前缀跳扫，50 万笔库实测
--   ~0.64s/次且与保单流水量无关；本部分覆盖索引按 policy_id 打头，GROUP BY
--   由索引序满足（无临时 B-tree），成本正比于保单流水数（占比极小，写放大
--   可忽略），partial 谓词与统计查询口径精确匹配。
--
-- 迁移尾部刷新统计（V016 先例）：索引集合变化后重算 sqlite_stat1，planner
-- 在接管索引上的选择基于与 schema 一致的统计；新装库空表统计近似空，应用侧
-- 经批量导入后的 PRAGMA optimize 逐步收敛（issue #490）。

DROP INDEX idx_transactions_amount;
DROP INDEX idx_transactions_refund;
DROP INDEX idx_transactions_sync;
DROP INDEX idx_transactions_account;
DROP INDEX idx_transactions_category;
DROP INDEX idx_transactions_deleted;

CREATE INDEX IF NOT EXISTS idx_transactions_policy
    ON transactions(policy_id, kind, amount_native_cents)
    WHERE policy_id IS NOT NULL AND is_deleted = 0;

ANALYZE;
