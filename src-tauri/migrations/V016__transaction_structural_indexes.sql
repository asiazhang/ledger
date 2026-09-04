-- V016 结构索引与统计（issue #490 / 父 #489 B1）
--
-- 目标：不动任何 SQL 契约（列表/深分页 SQL 与 ADR-0008 契约零改动），
-- 以 6 条 partial 覆盖索引 + 统计刷新消灭 6 项慢基准
-- （列表首页 1357ms→~1ms、深分页 2389ms→25ms、账户日期筛选 1755ms→<1ms、
-- 月度 1147ms→130ms、分类 1276ms→86ms、时点持仓 980ms→12ms，本地 NVMe 参考值）。
--
-- 全部索引均为 partial（WHERE is_deleted=0）：软删行不进索引，查询侧统一
-- is_deleted=0 谓词与之精确匹配；覆盖列免去回表。只增不改，无就地修改。

-- 1) 列表序：首页分页与深分页共用的确定性排序键
--    （ORDER BY date DESC, created_at DESC, id DESC，ADR-0008）。
--    反向索引扫描天然满足排序，LIMIT 早停，消除 ORDER BY 临时 B-tree。
CREATE INDEX IF NOT EXISTS idx_transactions_list_order
    ON transactions(date, created_at, id) WHERE is_deleted = 0;

-- 2) 账户筛选序：账户 × 日期窗口过滤 + 同款列表排序
--    （account_id 等值前缀 + date 区间 + created_at/id 排序全在索引内）。
CREATE INDEX IF NOT EXISTS idx_transactions_account_date
    ON transactions(account_id, date, created_at, id) WHERE is_deleted = 0;

-- 3) 账户现金流：余额转出侧 account_flow 聚合
--    （SUM(CASE kind … amount_native_cents) WHERE account_id=? AND is_deleted=0，
--    kind 与金额全在索引内，覆盖聚合不回表）。
CREATE INDEX IF NOT EXISTS idx_transactions_account_flow
    ON transactions(account_id, kind, amount_native_cents) WHERE is_deleted = 0;

-- 4) 转入现金流：余额转入侧 account_flow 聚合（to_account_id 侧同口径）。
CREATE INDEX IF NOT EXISTS idx_transactions_to_account_flow
    ON transactions(to_account_id, kind, amount_native_cents) WHERE is_deleted = 0;

-- 5) 月度表达式索引：月度汇总 GROUP BY substr(date,1,7)
--    （表达式列供分组序、kind/金额/日期覆盖聚合与过滤；
--    查询侧以 INDEXED BY 钉定，防 planner 统计边际摇摆退回临时 B-tree）。
CREATE INDEX IF NOT EXISTS idx_transactions_month_expr
    ON transactions(substr(date, 1, 7), kind, amount_native_cents, date)
    WHERE is_deleted = 0;

-- 6) 分类覆盖：分类聚合（GROUP BY category_id，kind/日期过滤 + 金额求和全在索引内）。
CREATE INDEX IF NOT EXISTS idx_transactions_category_covering
    ON transactions(category_id, kind, date, amount_native_cents) WHERE is_deleted = 0;

-- 迁移尾部刷新统计：存量库升级时对全量数据重算 sqlite_stat1，
-- 时点持仓（security_transactions 侧驱动）等 join 顺序依赖统计假设即愈；
-- 新装库空表统计近似空，应用侧经批量导入后的 PRAGMA optimize 逐步收敛（issue #490）。
ANALYZE;
