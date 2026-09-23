-- V032: 储蓄目标（SavingsGoal）表（spec #1750 / issue #1751 / ADR-0133）
--
-- 储蓄目标是单列小域（先例物品 / 保单 / 实物资产）：为特定用途设定的资金蓄水
-- 目标——目标金额 + 可选截止日 + 状态。蓄水动作是真实流水、进度是真实余额
-- （ADR-0133 决策 1）：创建目标在同一事务内经账户域公开写入口建其专属账户
-- （other 类型），目标经 account_id 绑定持有账户身份（1 目标 : 1 账户，UNIQUE
-- 钉定）；进度 = 专属账户余额（读余额缓存，ADR-0067），本表零流水语义。
--
-- schema 一次到位（后续票不再改表）：截止日（deadline）与手填「计划月存」
-- （planned_monthly_cents，可空）列首版即就位。目标金额正数守卫在应用层（码化
-- 错误，先例预算 amount / 实物资产估值）；币种不落表——即专属账户币种，目标域
-- 不折算（ADR-0133 决策 3）。
--
-- 状态列闭集 active / archived（进行中 / 归档）：达成是「余额 ≥ 目标额」的纯展示
-- 状态、读时实时派生不落表（词汇表「达成与归档」——落表就需要交易写路径反向挂
-- 目标域钩子，违反「目标域零新写入路径」）。删除为软删除（is_deleted）；级联软删
-- 专属账户是应用层编排（生命周期票），硬删语义下目标对账户是存续依赖，外键取
-- 强依赖 RESTRICT（docs/model 外键约定 2）。
--
-- 同步字段为主业务表形态（device_id / version / updated_at / is_deleted，LWW）；
-- Goal 实体 DomainCommand 的 op 产出随多端同步票接入（载荷只增不改）。
CREATE TABLE IF NOT EXISTS goals (
    id                    TEXT PRIMARY KEY,                            -- 全局唯一 ID（UUID v7）
    name                  TEXT NOT NULL,                               -- 目标名称（目标名权威，专属账户名随动只读）
    target_amount_cents   INTEGER NOT NULL,                            -- 目标金额（整数分；正数守卫在应用层）
    deadline              TEXT,                                        -- 截止日期（可空 = 无截止日；YYYY-MM-DD）
    status                TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived')),  -- 进行中 / 归档（达成为读时派生）
    planned_monthly_cents INTEGER,                                     -- 手填「计划月存」（可空，整数分；节奏来源闭集二值之一）
    account_id            TEXT NOT NULL UNIQUE REFERENCES accounts(id) ON DELETE RESTRICT,  -- 专属账户（1 目标 : 1 账户）
    is_deleted            INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1)),  -- 软删除标志
    version               INTEGER NOT NULL DEFAULT 1,                  -- 版本计数
    device_id             TEXT NOT NULL,                               -- 创建设备/最后修改设备标识
    created_at            TEXT NOT NULL,                               -- 创建时间，UTC ISO 8601
    updated_at            TEXT NOT NULL                                -- 最后修改时间，UTC ISO 8601
);
