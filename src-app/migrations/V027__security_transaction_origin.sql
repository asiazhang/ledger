-- V027：期初存量标记列（issue #1343）。加列只增，不改任何既有对象、零 BREAKING。
--
-- origin：证券扩展行的来源口径闭集（'trade' | 'opening'）。'opening' = 期初存量
-- ——补记（持仓初始化）落进来的存量持仓，其真实建仓时点未知，不是当日真实入金；
-- 'trade' = 真实成交（默认，存量行全部回填此值）。列只承载投资域语义，不动核心域
-- kind 闭集。
--
-- 唯一消费方是资金加权收益率（ADR-0115 / issue #1195）：'opening' 行不进现金流
-- 集——它不是真实入金，把它当当日 inflow 会把「成本 → 现值」的累计涨幅摊到 1 天
-- 年化（实测 #1343：成本 1.1533 → 现值 1.1898 的 +3.16% 年化成 8,594,733.89%）。
-- 含期初存量的标的不给年化、改给未年化收益率（口径见 ADR-0115 修订）。
--
-- schema 层只守取值闭集：「仅 buy 可携带」是跨 kind 规则，归行为层准入守卫
-- （write::protocol::guard_reference_admission），与 V023 出资账户 / V026 信用卡
-- 档案同款纪律——字段准入保持单源，不在列 CHECK 里出现第二套口径。

ALTER TABLE security_transactions ADD COLUMN origin TEXT NOT NULL DEFAULT 'trade'
    CHECK(origin IN ('trade','opening'));
