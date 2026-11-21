-- V038 交易 kind 维度计数覆盖索引（issue #2197）
--
-- 现象：`list_transactions` 的 COUNT（`transaction/src/read/list.rs`，过滤条件
-- 与 total 共用同一 WHERE）在 dev 构建冷启动首查报慢查询 WARN（288ms，阈值
-- 100ms），SQL 带 `hide_investment_related` 四个 NOT EXISTS（#1810 / ADR-0136，
-- 形态来自 #1924）。
--
-- 根因（90MB 活动库只读副本实测，24367 行未删除交易）：`transactions` 现存
-- 索引无一含 `kind` 列（`idx_transactions_deleted` 已随 V025 清理退役），带
-- `kind IN (...)` 的 COUNT 被 planner 判成 `SCAN transactions` 全表扫描肥表
-- （表本体约 38MB）：无 hide_investment 过滤的 COUNT 仅 1–3ms（covering index
-- 扫描），完整查询 43ms——差值主体在肥行读取，子查询探测本身是廉价的。
--
-- 方案：kind 打头的 partial covering index，`kind, date` 双键让无日期与带日期
-- 两种 COUNT 都纯索引内完成（实测带日期 COUNT 此前退到
-- `idx_transactions_list_order` 回表，本索引形态下 1.5ms）；三端引用列与 id
-- 齐备，`hide_investment_related` 的三端探测与 EXISTS 语义列无需回表。附带
-- 收益：外层脱离肥表后，fundings 子查询在真实库收敛为
-- `SEARCH f USING sqlite_autoindex_transaction_fundings_1 (transaction_id=?)`
-- 主键探测——#1924 注释宣称的形态由此成立（小 fundings 下 planner 曾按
-- stat1 自适应为 `SCAN f`，注释已随 #2197 修正为按可验证行为描述）。
--
-- 实测（副本）：无日期 COUNT 43ms → 12.5ms；索引体积约 2.4MB。
--
-- 只增不改（迁移面）：查询口径不变，无既有索引删除；写路径多一棵 B 树。
-- 删除本迁移即红：`infra/src/db/tests/perf.rs`
-- `v038_hide_investment_count_uses_kind_count_covering_index`（计划钉定，
-- ADR-0087）。
--
-- 迁移尾部刷新统计（V016/V025/V030/V031 先例）：新索引进入 sqlite_stat1 后
-- planner 的选择基于与 schema 一致的统计；新装库空表统计近似空，应用侧经
-- 批量导入后的 PRAGMA optimize 逐步收敛（issue #490）。

CREATE INDEX IF NOT EXISTS idx_transactions_kind_count
    ON transactions(kind, date, account_id, to_account_id, funding_account_id, id)
    WHERE is_deleted = 0;

ANALYZE;
