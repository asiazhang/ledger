-- V030 商户维度覆盖索引（issue #1655 / #1640 CI 首跑三项超标处置）
--
-- 目标：商户消费排行聚合（reports `merchant_shares_report`）需要的列组合
--（merchant_id、kind、date、amount_native_cents）此前无单一覆盖索引可满足，
-- 两种可达计划在 50 万笔 seed 42 基准库均 800ms 量级：merchant 索引
--（idx_transactions_merchant，仅 merchant_id）SCAN 全回表，或 date 索引范围扫
-- + GROUP BY 临时 B-tree——perf-bench CI 首跑实测「商户占比」p95 777.28ms，
-- 超 ADR-0068 默认线 200ms 约 3.9 倍，常态超标；钉定既有索引救不回来（#1640
-- 范围外诊断产出）。
--
-- 与分类覆盖索引（V016 idx_transactions_category_covering）同形的商户维度
-- 镜像：分组列打头、kind/日期过滤与金额求和全在索引内——GROUP BY 自带分组序
--（无临时 B-tree）、聚合零回表。partial 谓词 WHERE is_deleted=0 与查询谓词
-- 精确匹配（同 V016 六条纪律）。查询侧以 INDEXED BY 钉定（月度汇总 issue
-- #490 / 分类聚合 #1640 先例）：防 planner 统计边际摇摆，钉定自带防删守卫
--（索引缺失时 prepare 直接报错）。软删商户的历史引用照常入排行（JOIN 不滤
-- merchants.is_deleted，口径不变）；无商户关联的交易不进排行（INNER JOIN
-- 语义，merchant_id IS NULL 行在索引内但被 JOIN 排除，与迁移前一致）。
--
-- 只增不改，无就地修改；写路径多一棵 B 树，商户列为可选、无商户的交易行
-- 照常入索引（键前缀 NULL），与分类覆盖索引同代价形态。
--
-- 迁移尾部刷新统计（V016 先例）：存量库升级时对全量数据重算 sqlite_stat1，
-- 新装库空表统计近似空，应用侧经批量导入后的 PRAGMA optimize 逐步收敛
--（issue #490）。

CREATE INDEX IF NOT EXISTS idx_transactions_merchant_covering
    ON transactions(merchant_id, kind, date, amount_native_cents) WHERE is_deleted = 0;

ANALYZE;
