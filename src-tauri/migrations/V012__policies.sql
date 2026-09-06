-- V012: 保单（Policy）表（issue #360 / spec #358 / ADR-0051）
-- 保单是消费型保险合同的静态档案（单列小域，先例物品/预算）：回答
-- 「我有哪几张保单、保什么、保到什么时候」。一切缴费动态参数（每期保费金额、
-- 缴费频率、扣款账户）不进保单，由缴费协议（复用定时计划域订阅形态，后续票）
-- 单点持有——保单重复存即第二口径。
-- 保司引用保险域自有保司字典（insurers，表由 V019 建；ADR-0082 决策 1）：
-- 保险公司与商户分家，保单以 insurer_id 引用保司，不再混用商户字典。
-- 【就地修改注记】本文件已被就地修改（两级 BREAKING 标记之一，另一级见
-- CHANGELOG「Unreleased」BREAKING 条目，issue #713 / ADR-0082 决策 5）：
-- 保司字段由商户引用换为保司引用——merchant_id（REFERENCES merchants）就地
-- 替换为 insurer_id（REFERENCES insurers）。insurers 表在序列更后的 V019 建，
-- DDL 允许前向引用、V012 与 V019 之间无本表 DML，全新安装直接落新形状；
-- 已执行过旧版迁移的存量库不自动升级（无收敛迁移，升级后保单功能不可用，
-- 需重建库），见 CHANGELOG 对应 BREAKING 条目。
-- 保额纯展示：不进任何金额口径（不走 Amount 接缝折算、不参与聚合）。
-- 生命周期：无手动状态字段（到期由保障期间推导，可推导的状态不持久化，
-- 先例：预算永久滚动 ADR-0029）；删除为软删除，已删保单不进列表，
-- 其上历史引用保留不置空（保单是档案非字典，置空会毁掉已缴/已赔历史）。
CREATE TABLE IF NOT EXISTS policies (
    id                      TEXT PRIMARY KEY,                            -- 全局唯一 ID（UUID v7）
    insurer_id              TEXT NOT NULL REFERENCES insurers(id) ON DELETE RESTRICT,  -- 保险公司（保司字典引用，ADR-0082）
    policy_number           TEXT NOT NULL,                               -- 保单号
    product_name            TEXT NOT NULL,                               -- 险种名称
    start_date              TEXT NOT NULL,                               -- 保障期间起（YYYY-MM-DD）
    end_date                TEXT,                                        -- 保障期间止（YYYY-MM-DD；可空 = 长期/终身）
    coverage_amount_cents   INTEGER,                                     -- 保额（整数分，可选；纯展示，不进任何金额口径）
    coverage_currency_code  TEXT REFERENCES currencies(code) ON DELETE RESTRICT,  -- 保额币种（保额存在时必填，两者成对）
    note                    TEXT,                                        -- 备注
    is_deleted              INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1)),  -- 软删除标志
    version                 INTEGER NOT NULL DEFAULT 1,                  -- 版本计数
    device_id               TEXT NOT NULL,                               -- 创建设备/最后修改设备标识
    created_at              TEXT NOT NULL,                               -- 创建时间，UTC ISO 8601
    updated_at              TEXT NOT NULL                                -- 最后修改时间，UTC ISO 8601
);
