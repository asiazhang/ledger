-- V034：出资项子表 transaction_fundings + transactions.account_id 放宽 NOT NULL
-- （issue #1860 / ADR-0138 决策 1/6）；就地扩补购买项子表 transaction_purchases
-- （issue #1882 / ADR-0138 决策 9/10，未发布窗口内不新开版本号，文件名仍为出资
-- 项语义——本地已跑过旧版 V034 的开发库需重建库才能获得新子表）。
--
-- ── 一、表重建（SQLite 官方 ALTERNATIVE 规程，12 步）────────────────────────
--
-- 分解行（携出资子行的 expense/income、显式覆盖的 refund、继承分解原支出的缺省
-- refund）的账户口径由子行承载，主表 `account_id` 列落 NULL——V001 的 NOT NULL
-- 约束必须放宽。SQLite 不支持 ALTER COLUMN，唯一形态是整表重建：建新表（唯一
-- 差异 = account_id 去掉 NOT NULL）→ 显式列清单复制 → DROP 旧表 → RENAME 还名
-- → 逐一重建全部 14 条索引（索引随表删除，清单对准重建前 sqlite_master）。
--
-- 前提：`init_db` 在迁移期保持外键关闭（rusqlite_migration 的事务内 PRAGMA 是
-- no-op，关闭动作在进入迁移事务前完成）——外键开着时 DROP 的隐式 DELETE 会把
-- CASCADE 子表（security_transactions / transaction_fundings）清空。关闭下：
-- 子行原样保留，其 `REFERENCES transactions` 子句指向的是**表名**，RENAME 还名
-- 后自动解析到新表（`transactions_new` 无任何引用方，RENAME 不改写引用）。
-- 存量数据零迁移：列复制 1:1，无任何值改写。

CREATE TABLE transactions_new (
    id                        TEXT PRIMARY KEY,                        -- 交易全局唯一 ID（UUID v7）
    kind                      TEXT NOT NULL CHECK(kind IN ('income','expense','transfer','refund','buy','sell','dividend','split','convert')),
    amount_cents              INTEGER NOT NULL,                                        -- 原始币种金额，以「分」为单位的整数
    currency_code             TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 原始币种代码，关联 currencies.code
    amount_native_cents       INTEGER NOT NULL,                                        -- 本位币金额（当前 1:1，预留多币种换算），以「分」为单位
    -- account_id：唯一差异——去掉 NOT NULL（ADR-0138 决策 1：分解行落 NULL，
    -- 账户口径由 transaction_fundings 子行承载；单出资行恒有值，存量行零变化）。
    account_id                TEXT REFERENCES accounts(id) ON DELETE RESTRICT,  -- 关联账户 ID；支出/收入/转出账户
    to_account_id             TEXT REFERENCES accounts(id) ON DELETE SET NULL,    -- 转入账户 ID，仅转账（transfer）时必填；账户硬删时置空
    category_id               TEXT REFERENCES categories(id) ON DELETE SET NULL,   -- 关联分类 ID，转账通常为空；分类硬删时置空
    merchant_id               TEXT REFERENCES merchants(id) ON DELETE SET NULL,   -- 关联商户 ID（expense/refund/income 可携带）；商户硬删时置空
    refund_of_transaction_id  TEXT REFERENCES transactions(id) ON DELETE SET NULL,  -- 退款关联的原始支出交易 ID；原交易硬删时置空
    note                      TEXT,                                                    -- 交易备注（可选）
    dedup_hash                TEXT,                                                    -- 导入去重哈希（V007，可空，应用层行为不建唯一索引）
    date                      TEXT NOT NULL,                                           -- 交易日期，ISO 8601 日期格式（YYYY-MM-DD）
    created_at                TEXT NOT NULL,                                            -- 创建时间，UTC ISO 8601 格式
    updated_at                TEXT NOT NULL,                                            -- 最后修改时间，UTC ISO 8601 格式
    version                   INTEGER NOT NULL DEFAULT 1,                               -- 版本计数
    device_id                 TEXT NOT NULL,                                            -- 创建设备/最后修改设备标识
    is_deleted                INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1)),   -- 软删除标志
    idempotency_key           TEXT,                                                     -- 导入幂等键（V007）
    policy_id                 TEXT REFERENCES policies(id) ON DELETE RESTRICT,          -- 保单引用（V013）
    funding_account_id        TEXT REFERENCES accounts(id) ON DELETE RESTRICT,          -- buy/sell 出资账户列（V023 / ADR-0096）
    fx_rate_used              REAL,                                                     -- 折算来源留痕（V029 / ADR-0011 修订）
    fx_rate_source            TEXT                                                      -- 折算来源闭集字面量（V029）
);

-- 显式列清单复制（新旧列序一致；1:1 无值改写，存量数据零迁移）。显式
-- `ORDER BY rowid`：整表重建会给行分配新 rowid，而投资域 FIFO 落账序依赖
-- transactions 行的 rowid 相对序（active_lots / split 重述以 rowid 为界）——
-- 按旧表 rowid 序扫描插入，新表 rowid 依插入序分配，相对序逐行保持。
INSERT INTO transactions_new (
    id, kind, amount_cents, currency_code, amount_native_cents, account_id,
    to_account_id, category_id, merchant_id, refund_of_transaction_id, note,
    dedup_hash, date, created_at, updated_at, version, device_id, is_deleted,
    idempotency_key, policy_id, funding_account_id, fx_rate_used, fx_rate_source
)
SELECT
    id, kind, amount_cents, currency_code, amount_native_cents, account_id,
    to_account_id, category_id, merchant_id, refund_of_transaction_id, note,
    dedup_hash, date, created_at, updated_at, version, device_id, is_deleted,
    idempotency_key, policy_id, funding_account_id, fx_rate_used, fx_rate_source
FROM transactions ORDER BY rowid;

DROP TABLE transactions;

-- RENAME 还名：子表（security_transactions / refund 自引用 / 本迁移的
-- transaction_fundings）的 `REFERENCES transactions` 指向表名，自动解析到新表。
ALTER TABLE transactions_new RENAME TO transactions;

-- 索引重建（清单 = 重建前 sqlite_master 中 transactions 上的全部 14 条，
-- 逐字对准；列清单去 note_pinyin 的形态保持 V031 定稿）。
CREATE INDEX idx_transactions_date ON transactions(date);
CREATE INDEX idx_transactions_account_date
    ON transactions(account_id, date, created_at, id) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_account_flow
    ON transactions(account_id, kind, amount_native_cents) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_to_account_flow
    ON transactions(to_account_id, kind, amount_native_cents) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_list_order
    ON transactions(date, created_at, id) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_merchant ON transactions(merchant_id);
CREATE INDEX idx_transactions_merchant_covering
    ON transactions(merchant_id, kind, date, amount_native_cents) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_category_covering
    ON transactions(category_id, kind, date, amount_native_cents) WHERE is_deleted = 0;
CREATE INDEX idx_transactions_month_expr
    ON transactions(substr(date, 1, 7), kind, amount_native_cents, date)
    WHERE is_deleted = 0;
CREATE INDEX idx_transactions_note_search
    ON transactions(date, created_at, id, note, account_id, merchant_id, category_id)
    WHERE is_deleted = 0;
CREATE UNIQUE INDEX idx_transactions_idempotency_key
    ON transactions(idempotency_key)
    WHERE idempotency_key IS NOT NULL AND is_deleted = 0;
CREATE INDEX idx_transactions_dedup_hash
    ON transactions(dedup_hash, created_at)
    WHERE is_deleted = 0 AND dedup_hash IS NOT NULL;
CREATE INDEX idx_transactions_funding
    ON transactions(funding_account_id)
    WHERE funding_account_id IS NOT NULL;
CREATE INDEX idx_transactions_policy
    ON transactions(policy_id, kind, amount_native_cents)
    WHERE policy_id IS NOT NULL AND is_deleted = 0;

-- ── 二、出资项子表（ADR-0138 决策 1）─────────────────────────────────────
--
-- 出资项：一笔 expense / income 交易的出资分解子行——一条出资 = {账户, 金额,
-- 扣款标签}，全部出资项之和恒等于交易金额（行为层收口，schema 层只设正性
-- CHECK）。解决组合支付（一笔订单由多个账户共同付款）下「一行交易无法同时挂
-- 多个付款账户」的资金归因问题。refund 显式覆盖同样落本表（ADR-0138 决策 5），
-- 缺省按比例派生是读时推导、不落库。
--
-- 子行生命周期绑定主行（ADR-0138 决策 1）：主行软删即失效——读侧一律经
-- transactions.is_deleted=0 过滤，子行无独立软删位、不随主行软删改写；主行硬删
-- 时 CASCADE 清理。修改为全量替换（单 ⇄ 多就地互转走既有修改协议）。
--
-- 显式顺序位 sort 保读回稳定（读回顺序是用户可见语义——对账单顺序，不依赖
-- 数据库返回顺序）；(transaction_id, sort) 主键兼防重。账户引用比照 V023
-- 出资账户列（RESTRICT，强依赖）：账户硬删不容子行悬空。
-- account_id 索引服务两条反向查询：余额口径的按账户聚合（ADR-0067 整体重算的
-- 口径表达式含出资项端）与涉及账户过滤的子查询下推。

CREATE TABLE IF NOT EXISTS transaction_fundings (
    transaction_id TEXT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    sort           INTEGER NOT NULL CHECK(sort >= 0),
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    amount_cents   INTEGER NOT NULL CHECK(amount_cents > 0),
    label          TEXT,
    PRIMARY KEY (transaction_id, sort)
);

CREATE INDEX IF NOT EXISTS idx_transaction_fundings_account
    ON transaction_fundings(account_id);

-- ── 三、购买项子表（issue #1882 / ADR-0138 决策 9/10）────────────────────────
--
-- 购买项：一笔 expense 交易（订单）的逐件商品明细子行——一条购买项 = {名称,
-- 件数, 分类引用, 可选单价}，数组顺序即显式顺序位（对账单顺序，用户可见语义，
-- 不依赖数据库返回顺序）。解决多商品订单「这一单买了什么」不可读的问题；主线
-- 来源是 AI 导入电商回单（ADR-0138 决策 15：仅 AI 导入写入、界面只读）。
--
-- 子行生命周期绑定主行（与出资项同构，ADR-0138 决策 9）：主行软删即失效——读侧
-- 一律经 transactions.is_deleted=0 过滤，子行无独立软删位、不随主行软删改写；
-- 主行硬删时 CASCADE 清理。修改为全量替换。存量行零迁移（无购买项 = 现状，
-- 行为零变化）。
--
-- 单价为可空整数分（决策 10：源单只给订单总额时为空——价格是可缺的原始凭据
-- 而非必需字段），schema 层只设非负 CHECK；价格存而不显示、不进任何金额与
-- 折算口径、不与实付勾稽，标价小计由「单价 × 件数」复算、不另存。分类引用是
-- 明细指针非存续依赖（报表口径按交易行分类整单计入，不按购买项分摊，决策 11），
-- ON DELETE 语义比照 transactions.category_id（SET NULL，溯源指针）。无账户与
-- 金额列：购买项不承载资金语义。除主键外不建二级索引——读回按 (transaction_id,
-- sort) 主键前缀走；名称搜索不建索引——命中走子表预查询 + 命中 id 集合下推（前置通配 LIKE 无 B-tree 可定位，子表行集远小于交易行集，全扫即枚举级成本；50 万笔库定量证据见 ADR-0027 修订记录 issue #1885）。

CREATE TABLE IF NOT EXISTS transaction_purchases (
    transaction_id   TEXT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    sort             INTEGER NOT NULL CHECK(sort >= 0),
    name             TEXT NOT NULL,
    quantity         INTEGER NOT NULL CHECK(quantity > 0),
    category_id      TEXT REFERENCES categories(id) ON DELETE SET NULL,
    unit_price_cents INTEGER CHECK(unit_price_cents IS NULL OR unit_price_cents >= 0),
    PRIMARY KEY (transaction_id, sort)
);

