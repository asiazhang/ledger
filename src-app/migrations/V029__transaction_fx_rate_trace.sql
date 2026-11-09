-- V029：折算来源留痕列（issue #1548 / ADR-0011 2026-09-19 修订）。加列只增：
-- 不改任何已发布对象、零 BREAKING；新列可空，对既有行直接生效 NULL（SQLite
-- ADD COLUMN 语义），存量库打开即用、无需重建。
--
-- 每笔非本位币交易随行留痕「这个本位币金额是怎么来的」，事后可读回解释、
-- 可与重算对齐：
--   fx_rate_used：本笔折算使用的汇率值——amount_native_cents ≈ amount_cents ×
--     fx_rate_used（四舍五入到分）。序列只有反向量（quote→base）时存**使用值**
--     （倒数），保证仅凭本列即可复算行内本位币金额。未折算（与本位币同币种、
--     无现金腿的 split）为 NULL。
--   fx_rate_source：折算来源闭集 'series'（命中汇率历史序列 fx_rate_history 的
--     交易周）| 'explicit'（调用方逐笔显式给定，#1549 接入）。未折算为 NULL。
--
-- 不设 CHECK / NOT NULL DEFAULT（与 instruments.source、V023 纪律同款）：
-- 闭集由写入通道收口（Amount 接缝按交易日入口单点产出，ADR-0113 共享语义区）；
-- 存量行保持 NULL——折算来源是写入时点的事实，历史行无从推断、不做事后回填，
-- NULL 即「写入时点早于留痕功能或未折算」的诚实语义。两列不参与导入幂等身份
-- （去重哈希不含折算结果，既有行为不变）也不建索引（读回解释用，非检索键）。

ALTER TABLE transactions ADD COLUMN fx_rate_used REAL;
ALTER TABLE transactions ADD COLUMN fx_rate_source TEXT;
