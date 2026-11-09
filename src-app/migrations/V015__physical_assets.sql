-- V015: 实物资产（PhysicalAsset）与估值历史（PhysicalAssetValuation）两张新表
-- （issue #466 / spec #465 / ADR-0064）
-- 实物资产是单列小域（先例物品/保单）：大件实物的估值档案，回答
-- 「我有哪几件大件、各值多少钱、家底合计多少」。与物品域按「要不要跟踪市值」
-- 互斥分家——物品回答使用成本摊薄，实物资产回答当前市值；黄金等贵金属不在范围。
-- 估值全手动、只追加不改写：每次估值落一条估值历史行，当前估值 = 最新一条
-- （估值日期最新，同日按插入序——UUID v7 主键时间有序）；系统不做自动折旧/增值。
-- 购买价与币种成对可空（先例保单保额，应用层守卫成对）；状态在持/已处置，
-- 处置日期/处置价由处置路径（T3）写入，schema 一次建全避免日后修改已发布迁移；
-- 删除为软删除（is_deleted），软删后数据与估值历史保留。
-- 估值历史是只追加的扩展行：依附资产存续（外键 CASCADE），不改写、不软删。
-- 同步字段口径：实物资产是主业务表，携带 device_id / version 供多端 LWW；
-- 估值历史只追加不改写，无更新语义，仅留 created_at / device_id 审计。

-- 1. physical_assets（实物资产表）
CREATE TABLE IF NOT EXISTS physical_assets (
    id                      TEXT PRIMARY KEY,                            -- 全局唯一 ID（UUID v7）
    name                    TEXT NOT NULL,                               -- 资产名称（建档必填，应用层守卫非空）
    purchase_date           TEXT,                                        -- 购买日期（可空；YYYY-MM-DD）
    purchase_price_cents    INTEGER,                                     -- 购买价（可空，整数分；与购买币种成对）
    purchase_currency_code  TEXT REFERENCES currencies(code) ON DELETE RESTRICT,  -- 购买价币种（购买价存在时必填，成对）
    status                  TEXT NOT NULL DEFAULT 'holding' CHECK(status IN ('holding','disposed')),  -- 在持/已处置
    disposal_date           TEXT,                                        -- 处置日期（仅 disposed；YYYY-MM-DD；处置必填）
    disposal_price_cents    INTEGER,                                     -- 处置价（可空，整数分；纯记录，与处置币种成对）
    disposal_currency_code  TEXT REFERENCES currencies(code) ON DELETE RESTRICT,  -- 处置价币种（处置价存在时必填，成对）
    is_deleted              INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1)),  -- 软删除标志
    version                 INTEGER NOT NULL DEFAULT 1,                  -- 版本计数
    device_id               TEXT NOT NULL,                               -- 创建设备/最后修改设备标识
    created_at              TEXT NOT NULL,                               -- 创建时间，UTC ISO 8601
    updated_at              TEXT NOT NULL                                -- 最后修改时间，UTC ISO 8601
);

-- 2. physical_asset_valuations（估值历史表，只追加不改写）
--    - 当前估值 = 每资产最新一条（估值日期最新，同日按插入序 = id 降序首条）。
--    - amount_cents 为估值金额（整数分，应用层守卫 > 0）；币种必填。
--    - 外键 CASCADE：估值历史依附资产存续，资产硬删（未来 purge）时随父行消失；
--      软删不触发行删除，历史保留。
CREATE TABLE IF NOT EXISTS physical_asset_valuations (
    id             TEXT PRIMARY KEY,                                      -- 全局唯一 ID（UUID v7，时间有序 = 插入序）
    asset_id       TEXT NOT NULL REFERENCES physical_assets(id) ON DELETE CASCADE,  -- 所属资产
    valuation_date TEXT NOT NULL,                                         -- 估值日期（YYYY-MM-DD；可补过去，拒绝未来）
    amount_cents   INTEGER NOT NULL,                                      -- 估值金额（整数分）
    currency_code  TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 估值币种
    device_id      TEXT NOT NULL,                                         -- 录入设备标识
    created_at     TEXT NOT NULL                                          -- 录入时间，UTC ISO 8601
);

CREATE INDEX IF NOT EXISTS idx_physical_asset_valuations_asset
    ON physical_asset_valuations(asset_id, valuation_date);
