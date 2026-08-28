-- V009: 物品（Item）表（issue #115 / spec #113 / ADR-0014）
-- 物品是独立领域实体（非参考数据、非交易流水）：用户拥有的耐用实物，
-- 记录总成本与生命周期（in_use 在用 / disposed 已处置），供「每天使用成本」计算。
-- 金额沿用仓库约定：整数分（_cents 后缀），raw（total_cost_cents）与
-- native（cost_native_cents，折算默认币种）分离。
CREATE TABLE IF NOT EXISTS items (
    id                  TEXT PRIMARY KEY,                            -- 全局唯一 ID（UUID v7）
    name                TEXT NOT NULL,                               -- 物品名称
    purchase_date       TEXT NOT NULL,                               -- 购买日期，ISO 8601 日期格式（YYYY-MM-DD）
    total_cost_cents    INTEGER NOT NULL,                            -- 总成本（原始币种，整数分）
    currency_code       TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 原始币种代码
    cost_native_cents   INTEGER NOT NULL,                            -- 总成本折算本位币（默认币种，整数分）
    status              TEXT NOT NULL DEFAULT 'in_use' CHECK(status IN ('in_use','disposed')),  -- 生命周期：在用/已处置
    disposal_date       TEXT,                                        -- 处置日期（仅 disposed；YYYY-MM-DD）
    residual_value_cents INTEGER,                                    -- 残值（仅 disposed 可填，整数分；可空）
    note                TEXT,                                        -- 备注（品牌/型号/购买渠道等）
    is_deleted          INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1)),  -- 软删除标志
    version             INTEGER NOT NULL DEFAULT 1,                  -- 版本计数
    device_id           TEXT NOT NULL,                               -- 创建设备/最后修改设备标识
    created_at          TEXT NOT NULL,                               -- 创建时间，UTC ISO 8601
    updated_at          TEXT NOT NULL                                -- 最后修改时间，UTC ISO 8601
);
