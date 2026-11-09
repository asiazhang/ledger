-- V024：份额调整（split）批次重述审计表（issue #1049 / 父 spec #1045 / ADR-0106 决策 3）。
-- additive 迁移：零 CHECK 改动、零 BREAKING、存量库免重建。
--
-- security_lot_adjustments：一笔 split 落账对**在用批次**逐批次按比例重述
-- （remaining_quantity × f、initial_quantity × f、cost_per_unit_cents ÷ f，
-- f = (当前持仓 + Δ) / 当前持仓；批次总成本精确不变，舍入尾差归末批次闭合）的
-- 逐批次 before / after 记录。重述不可逆（含舍入），本表是后续修改/删除精确回补
-- 与审计的唯一依据（角色对齐 security_lot_conversions 之于 convert 转出腿）。
-- 列名带 _before / _after 后缀逐列成对，读侧不做位置约定。
--
-- 外键动作比照 security_lot_conversions（扩展行语义，V002 头部纪律）：
-- - transaction_id 指向 security_transactions.transaction_id（split 行以
--   transaction_id 为主键，一笔调整至多一行扩展），交易扩展行删除级联消失；
-- - lot_id 指向 security_lots(id)，批次硬删（随其买入交易级联）时本行级联消失
--   ——批次已不在场时 before/after 快照失去关联实体，无独立存续意义；
--   「批次被在用 split 重述后其买入不可改删」由行为层在用占用守卫收口（ADR-0106
--   决策 5 判据家族），不依赖库层 RESTRICT。
-- 币种不冗余落列：成本随被重述批次币种，审计经 lot_id 关联 security_lots 取用
-- （先例：security_lot_conversions）。
-- quantity 列不设（重述是「同批次数量与成本的重写」而非消耗，Δ 记在
-- security_transactions.quantity 上）；price_cents 留 NULL 的 split 行不改本表。
-- 两条索引均服务外键级联与按批次归因（批次在用占用守卫、审计回查）：
-- SQLite 不为外键自动建索引。

CREATE TABLE IF NOT EXISTS security_lot_adjustments (
    id                         TEXT PRIMARY KEY,  -- 重述记录全局唯一 ID（UUID v7）
    transaction_id             TEXT NOT NULL REFERENCES security_transactions(transaction_id) ON DELETE CASCADE,  -- 关联 split 交易
    lot_id                     TEXT NOT NULL REFERENCES security_lots(id) ON DELETE CASCADE,  -- 被重述的批次
    initial_quantity_before    REAL NOT NULL,     -- 重述前批次初始数量（份）
    initial_quantity_after     REAL NOT NULL,     -- 重述后批次初始数量（份）
    remaining_quantity_before  REAL NOT NULL,     -- 重述前批次剩余数量（份）
    remaining_quantity_after   REAL NOT NULL,     -- 重述后批次剩余数量（份）
    cost_per_unit_cents_before INTEGER NOT NULL,  -- 重述前每份成本（万分之一元，刻度见 V002 头部注记）
    cost_per_unit_cents_after  INTEGER NOT NULL,  -- 重述后每份成本（万分之一元），末批次吸收舍入尾差
    created_at                 TEXT NOT NULL      -- 创建时间
);

CREATE INDEX IF NOT EXISTS idx_security_lot_adjustments_transaction
    ON security_lot_adjustments(transaction_id);
CREATE INDEX IF NOT EXISTS idx_security_lot_adjustments_lot
    ON security_lot_adjustments(lot_id);
