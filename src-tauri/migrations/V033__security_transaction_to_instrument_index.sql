-- V033 证券转入腿索引（issue #1804）
--
-- 目标：`security_transactions` 的 OR 读路径此前只有转出臂单索引
--（V002 `idx_security_transactions_instrument`），`to_instrument_id` 臂无索引——
-- SQLite 无法走 MULTI-INDEX OR，「首笔持仓流水日」标量子查询
--（`FIRST_POSITION_DATE`，历史补全队列收集等消费）逐标的重执行时退回
-- 「`transactions` 全扫 + 按 PK 回表」：生产日志单条 1737ms（2026-09-24 现场），
-- 且该读跑在门面写槽裸作业上、越 1s 持锁探针（hold_ms=1738，ADR-0069 决策 4
-- 的运行时守门信号），同刻把自动备份轮次顶到 5s 等锁超时。
--
-- 实测（37MB 活动库只读复测，热缓存 5 次取最优，去掉 sqlite_stat 后复测同形）：
-- 实测（37MB 活动库只读复测，热缓存 5 次取最优，去 sqlite_stat 后复测同形）：
-- 整条收集查询 183.8ms → 14.7ms（标量子查询片段 131.2ms → ~6ms）。补本索引后
-- OR 单表达式在该数据量下可触发 MULTI-INDEX OR（13.6ms），但该选择只在数据量
-- 足够时成立——空库与小库成本平局会退回「`transactions` 全扫 + 按 PK 回表」。
-- 故常量侧两臂拆分并以 `INDEXED BY` 钉定本索引（V030 查询侧钉定先例）：计划由
-- SQL 文本确定、两臂各一次索引 seek，索引缺失时 prepare 直接报错。计划不再
-- 依赖 ANALYZE 统计，故本迁移不带 ANALYZE。
--
-- 附带收益：同形 OR 读路径同受其益——时点持仓推算（`holdings_as_of_in`）、
-- 投资明细列表（ledger_tab）、交易列表（read/list）。
--
-- 只增不改（迁移面）：查询口径不变，常量改两臂钉定形态、消费方与 SQL 别名契约
-- 不变；写路径多一棵 B 树，仅 convert 行携带转入腿（其余行 NULL 键）。删除本
-- 迁移即红：投资域 `tests::first_position_plan`（两臂计划 + 钉定缺失报错两断言）
-- 与首笔腿既有用例（tests::holdings_as_of）同时变红（ADR-0087）。

CREATE INDEX IF NOT EXISTS idx_security_transactions_to_instrument
    ON security_transactions(to_instrument_id);
