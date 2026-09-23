# 存量 `items.cost_native_cents` 回填评估（#1694，只评估不实施）

- 评估日期：2026-09-23（Asia/Shanghai）；父票 #1676（票 2 = #1693 已落地，本票为票 3）
- 评估对象：#1693 把物品写路径折算修正为交易日口径（继承溯源交易行留痕汇率）之后，存量行与新口径的不一致是否需要回填迁移
- 证据标注：
  - **【实测】** 对真实库 `/Users/zhangheng/Work/personal/ledger-migrate/ledger.db`（账本数据目录指针 `data_location.json` 所指，约 33MB，只读打开）与其同目录 49 份历史备份逐库扫描所得
  - **【代码】** 仓库内代码与迁移文件坐标（相对仓库根）
- 重要前提：本应用为单用户个人账本（`com.zhangheng.ledger`），真实库即全部存量；多设备场景按机制推演，不来自实测。

---

## 0. 结论摘要

1. **存量不一致规模 = 0 行。** 真实库 `items` 表 0 行（含软删），物品功能自 v0.4.0 交付以来在真实库与全部 49 份历史备份上**从未写入过任何一行**；全库 4,459 笔交易 0 笔跨币种，物品不一致的物质基础（跨币种物品）不存在。回填无可回填对象。
2. **建议：不做回填迁移（选项 B：保持现状仅增量走新口径）。** 选项 A（迁移回填）的全部成本换零收益；且 #1693 后任何物品编辑都会按新口径整体重折算，旧行随编辑自然收敛（自愈通道已存在）。
3. **发布边界**：`items` 表 / `cost_native_cents` 列自 v0.4.0 起已发布；但「新口径」本身（#1693 与 V029 留痕列）尚不在任何 tag 中，将随下一版本首发。若未来做回填，属于「新增迁移改已发布表的数据行」，schema 只增不改，但使用者可见数值会变，按发布纪律须在 CHANGELOG 记 BREAKING 条目。
4. **若未来出现存量行，回填自动化有一个硬约束**：无留痕旧行回落序列重查时，购买周若不在 `fx_rate_history`（当前实测仅 2026-06-22 → 2026-09-21 共 14 周），迁移时点无法自动定值（迁移不能联网）——本文审计 SQL 对这类行单列 `series_gap` 分级。ECB 数据源支持全量历史回补（`fetch_ecb_full_history` 全量通道），需先手动同步再回填。
5. **范围外发现（设计差距，报告待裁决，不擅动）**：物品编辑路径没有交易域 #1550 式「三元组未变沿用行内留痕」机制——无留痕的跨币种旧行若购买周序列缺点，用户编辑名称等无关字段也会报 `fx.rate-missing` 改不动。当前真实库 0 物品、现实发生率 0，是否为该差距立票请裁决（见第 6 节）。

---

## 1. 背景与新口径的复算规则

#1693（ADR-0014 2026-09-22 修订）之后，物品创建/修改的本位币成本折算收口在 `src-tauri/crates/item/src/domain.rs` 的 `validate_and_convert`，取数规则：

1. **本位币行**（物品币种 = 账本级默认币种）：入口自持 1:1，不留痕；
2. **跨币种 + 溯源交易行同币种且留痕在场**（`transactions.fx_rate_used` 非空、行未删）：行内留痕作为显式汇率传 `convert_to_native_on_trade_date`，物品行与交易行折出**同一个本位币数**；
3. **无留痕 / 无溯源 / 物品币种被单独编辑**：按购买日所属 ISO 周查汇率历史序列 `fx_rate_history`（正反向兜底，`src-tauri/crates/transaction/src/amount/convert.rs` `lookup_fx_history_rate`）；
4. 该周序列无点：码化错误 `fx.rate-missing`（区分「该周尚未发布」与「该周历史空缺」），不静默回落当期表。

存量行则按**创建/编辑时点的当期汇率**折算落库（v0.4.0 → #1693 间的行为，ADR-0014 决策 5 旧文）。本票票面判据「留痕汇率折算值 ≠ 当期折算值」中的「当期折算值」，对存量行而言**就是现存 `cost_native_cents`**——历史时点的当期汇率无从复原，存值本身是它唯一的权威载体。故判据落地为：

> **跨币种行：新口径复算值 ≠ 存值 → 不一致。**

注意这只影响物品详情抽屉展示的「本位币总成本」（`src/views/ItemsView.vue`，`formatAmount(detail.cost_native_cents, …)`）。列表的「总成本 / 每天成本」展示 raw 值（物品币种），dashboard 的「在用物品每天成本合计」（`item::domain::item_daily_total`）按 raw 分子走当期入口实时折算（ADR-0011 决策 3 读路径分工）——**三者都不读 `cost_native_cents`**。即：不回填的可见影响面 = 物品详情一处数字。

## 2. 审计方法

审计 SQL（附录 A）对每行活物品按第 1 节规则复算新口径值，与存值比对，并按取数来源分级：

| 分级 | 含义 | 回填可行性 |
|---|---|---|
| `same_base` | 本位币行（新口径 = raw 原样） | 存值 ≠ raw 即不一致（本位币变更后写入的痕迹） |
| `trace` | 继承交易行留痕折算 | 可自动定值 |
| `series` | 序列按购买周折算（正反兜底） | 可自动定值（前提该周序列在场） |
| `series_gap` | 购买周序列无点 | **不可自动定值**（迁移不能联网；新口径重查本身也报错） |

**SQL 正确性【实测】**：先在合成夹具上验证（8 行物品覆盖全部分级与不一致/一致两臂 + 软删排除 + 空缺周 + 溯源被软删），①–⑥ 各段查询的每一格预期值与实际输出一致（夹具与验证输出见附录 B，附录 A 为最终版）。`ROUND` 的远离零取整语义与 Rust `f64::round` 对同一 f64 乘积一致，折算算术无偏差源。

## 3. 存量不一致规模【实测】

对真实库（只读 `sqlite3 -readonly`）与全部历史备份扫描：

| 指标 | 值 |
|---|---|
| `user_version` | 29（V029 已应用；留痕列已在库中） |
| 默认币种（`app_settings.ledger.base_currency`） | CNY |
| 交易总数 / 跨币种交易 / 留痕行（`fx_rate_used` 非空） | 4,459 / **0** / **0** |
| 物品总数（含软删） | **0** |
| 其中跨币种（若存在） | 0 |
| `sync_device` 设备数 | 1 |
| `fx_rate_history` 覆盖 | 10 币种对 × 14 周（2026-06-22 → 2026-09-21，90 天增量窗口） |
| 历史备份（`backups/`，49 份）中物品行数 | 全部为 0 |

审计 SQL 第 ③–⑥ 段对真实库输出 0 行（无物品可分级）。**分布**（票面交付物 1 的第二问）：不存在的分布——物品功能在真实数据上从未启用过。

附带的两个相关事实：

- 跨币种交易为 0 → 「同一笔购买在交易行（交易日 native）与物品明细（当期 native）出现两个本位币数」这一 #1676 指出的现象，在本账本的现实发生率也为零；
- 序列覆盖仅 14 周 → 即使现在手工建一条 2026-06-22 之前购买的跨币种物品，#1693 的回落路径也会因「该周历史空缺」报错；全量历史可通过设置页「币种」页签的手动同步回补（`src-tauri/crates/market-sync/src/fx.rs` `fetch_full` 通道 → ECB 全量历史文件）。

## 4. 回填策略选项

### 选项 A：迁移回填（新迁移按新口径 UPDATE 全量）

- **利**：全库口径立即统一；物品行与交易行对同一笔购买折出同值；无「编辑前旧口径」窗口。
- **弊**：
  1. **series_gap 不可自动定值**：迁移运行时不能联网，序列缺点行要么迁移失败、要么部分回填 + 遗留行兜底策略，产品上还得先引导全量汇率同步；
  2. **使用者可见数值变化**：物品详情本位币成本数字翻转，属 CHANGELOG BREAKING（见第 5 节）；
  3. **多端纪律**：items 走 oplog LWW，Create/Update op 携带折算结果、重放不重折算（ADR-0091 决策 3；`src-tauri/crates/item/src/domain.rs` `replay_update` / `write_update_row` 整行覆盖编辑面字段）。静默 UPDATE 回填**不产 op** → 其他设备既收不到回填值，其已排队的旧 Update op 重放时还会把旧值写回。多端场景回填必须 op 化（回填即真写），工程复杂度显著上升；当前单设备无现实冲突，但设计必须按多端假设；
  4. 本库收益 = 0 行（第 3 节）。

### 选项 B：保持现状，仅增量走新口径（不做迁移）

- **利**：零风险、零工作量；新口径随 #1693 增量生效；**旧行随编辑自然收敛**——#1693 后任何 `update_item` 都会整体按新口径重折算（继承留痕 / 回落序列），编辑即自愈，无需专门通道。
- **弊**：
  1. 不被编辑的存量行长期保持创建时点口径，交易行（交易日）与物品详情（创建时点）数值不一致持续可见——本库为 0 行，不可见；
  2. 存量跨币种行若购买周序列缺点，编辑会报 `fx.rate-missing` 改不动（见第 6 节的范围外发现）。

### 选项 C（变体，不推荐）：惰性回填

V018 note_pinyin 先例（读路径按需分批回填冗余列）。对本案不成立：物品读路径的重算是**当期口径**（ADR-0011 决策 3 的读分工），把交易日写口径的落库挪到读路径，恰恰违反刚在 #1692 收口的「写路径不触碰当期表 / 读路径不做写折算」的读写分工；且物品读路径无批处理钩子可挂。

## 5. 发布边界判断与迁移纪律

【代码/实测】以最新 tag `v0.7.0`（2026-09-19，5f337e71）为界，`v0.7.0..HEAD` 共 99 提交：

| 对象 | 载体 | 发布状态 |
|---|---|---|
| `items` 表与 `cost_native_cents` 列 | V009（9f9a66ec） | **已发布**（tag 包含至 v0.4.0） |
| 交易行留痕列 `fx_rate_used` / `fx_rate_source` | V029（a798af8b） | **未发布**（无 tag 包含，将随下一版本首发） |
| 物品写路径新口径（#1693，PR #1698，2026-09-22 合入） | 代码行为 | **未发布**（同上） |

由此的纪律判断：

- 若未来做回填：迁移是**新文件**（V031+），不触已发布迁移文件，schema 只增不改——不触发「已发布迁移就地修改须注明 CHANGELOG 条目」的文件头纪律；
- 但回填**改已发布表的数据行**，使用者可见数值（物品详情本位币成本）会变——按「使用者可见的不兼容变更」标准，须在 CHANGELOG 对应版本或 `Unreleased` 下记 **BREAKING 条目**（数值口径翻转，超出「Changed」的解释义务）；
- 时序上值得注意：新口径本身也未发布。若回填迁移与 #1693 同版本首发，对从未见过「交易日口径物品行」的使用者，这次变化是「升级后：新物品按交易日折算 + 旧物品数字被对齐」一次到达，BREAKING 条目两条并写即可；相对 v0.7.0 的边界不受「同时首发」影响。

## 6. 建议、工作量与风险

**建议：选项 B，不做回填迁移。**

1. 收益面：存量 = 0 行，选项 A 的全部成本（迁移 + 兜底 + op 化 + BREAKING 公告）换零收益；
2. 收敛面：编辑即自愈的增量通道已存在（#1693 语义），无需回填也能单调收敛；
3. 复评估触发条件（满足其一再议）：
   - 真实库出现跨币种物品且 `trace`/`series` 分级不一致行 ≥ 数行（用附录 A SQL 复跑即可，只读）；
   - 启用多端同步且出现旧 Update op 回写冲突的现实证据；
   - 全量汇率历史回补后，`series_gap` 清零且存量行规模可观的场景。

**风险陈述**：选项 B 下唯一残留风险 = 第 4 节弊 2 的「旧行改不动」边界（见下），以及不被编辑的旧行口径不一致持续存在——两者在本库的发生率均为 0。

**范围外发现（设计差距，待裁决）**：物品编辑路径没有交易域 #1550 式的「币种/金额/日期三元组未变沿用行内留痕」机制（对照 `src-tauri/crates/transaction/src/amount/convert.rs` `convert_to_native_on_edit`）。后果：无留痕的跨币种旧行，若购买周序列缺点，用户编辑名称等无关字段也会 `fx.rate-missing` 整笔失败。#1693 票面「不做」明写无留痕回落序列重查、ADR-0014 修订记录同此口径，故这不是 #1693 的实现缺陷，而是两域编辑语义的知情/未知情差距——按 AGENTS.md 属设计或范围问题，**停下报告，不擅动**。是否立票（给物品编辑补沿用机制，或明示豁免补 ADR 注记）请用户裁决；当前 0 物品的现状下无紧迫性。

**工作量**：选项 B = 0（本票交付物即本报告）；选项 A 若未来启动 ≈ 中型票（迁移 SQL 三分支复刻 + series_gap 兜底策略 + op 化回填 + 域单测/迁移测试/同步重放测试 + BREAKING 公告，估 2–3 人日 + 评审）。

---

## 附录 A：审计 SQL（已夹具验证，只读执行）

执行方式：`sqlite3 -readonly "file:<库路径>?mode=ro" < 本文件`（对活库必须 `mode=ro`，且生产库直接复制副本后跑更稳）；以下即本次实测所跑文件的原文。

```sql
-- =====================================================================
-- items.cost_native_cents 存量口径审计（只读，#1694）
--
-- 新口径（ADR-0014 2026-09-22 修订 / #1693）复算规则：
--   ① 本位币行 → 1:1（total_cost_cents 原样）；
--   ② 跨币种 + 溯源交易行同币种且留痕在场（fx_rate_used 非空、交易行未删）
--      → ROUND(total_cost_cents × fx_rate_used)；
--   ③ 其余（无留痕/无溯源/物品币种被单独改过）→ 按购买日所属 ISO 周查
--      fx_rate_history（正反兜底）→ ROUND(total_cost_cents × 序列值)；
--   ④ 该周序列无点 → 新口径重查报 fx.rate-missing，无法自动定值（series_gap）。
-- ncv_source 分级：same_base / trace / series / series_gap。
-- ROUND 语义与 Rust f64::round 同为远离零取整，对同一 f64 乘积结果一致。
-- =====================================================================

-- ---------- ① 库级概览 ----------
SELECT 'user_version' AS k, user_version AS v FROM pragma_user_version;
SELECT 'base_currency' AS k,
       COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY') AS v;

-- ---------- ② 物品总量与跨币种分布 ----------
SELECT
  COUNT(*)                                AS items_all,
  COALESCE(SUM(i.is_deleted), 0)          AS items_deleted,
  COUNT(*) - COALESCE(SUM(i.is_deleted), 0) AS items_live,
  COALESCE(SUM(CASE WHEN i.is_deleted = 0
     AND i.currency_code <> COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY')
     THEN 1 ELSE 0 END), 0)               AS live_cross_ccy
FROM items i;

-- ---------- ③ 分类汇总：不一致行数与偏差 ----------
WITH base AS (
  SELECT COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY') AS code
),
calc AS (
  SELECT
    i.id                                   AS item_id,
    i.name                                 AS item_name,
    i.currency_code                        AS item_ccy,
    b.code                                 AS base_ccy,
    i.purchase_date,
    i.total_cost_cents,
    i.cost_native_cents                    AS stored_native,
    CASE WHEN t.is_deleted = 0
          AND t.currency_code = i.currency_code
          AND t.fx_rate_used IS NOT NULL
         THEN t.fx_rate_used END           AS trace_rate,
    CASE WHEN i.currency_code <> b.code THEN
      COALESCE(
        (SELECT h.rate FROM fx_rate_history h
          WHERE h.base_code  = i.currency_code
            AND h.quote_code = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1')),
        (SELECT 1.0 / h.rate FROM fx_rate_history h
          WHERE h.quote_code = i.currency_code
            AND h.base_code  = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1'))
      )
    END                                   AS series_rate
  FROM items i
  CROSS JOIN base b
  LEFT JOIN transactions t ON t.id = i.purchase_transaction_id
  WHERE i.is_deleted = 0
),
v AS (
  SELECT
    calc.*,
    CASE
      WHEN item_ccy = base_ccy     THEN 'same_base'
      WHEN trace_rate IS NOT NULL  THEN 'trace'
      WHEN series_rate IS NOT NULL THEN 'series'
      ELSE 'series_gap'
    END AS ncv_source,
    CASE
      WHEN item_ccy = base_ccy     THEN total_cost_cents
      WHEN trace_rate IS NOT NULL  THEN CAST(ROUND(total_cost_cents * trace_rate) AS INTEGER)
      WHEN series_rate IS NOT NULL THEN CAST(ROUND(total_cost_cents * series_rate) AS INTEGER)
      ELSE NULL
    END AS ncv_native
  FROM calc
)
SELECT
  ncv_source,
  COUNT(*)                                                          AS rows_total,
  COALESCE(SUM(CASE WHEN stored_native <> ncv_native THEN 1 ELSE 0 END), 0)
                                                                    AS rows_inconsistent,
  COALESCE(SUM(CASE WHEN stored_native <> ncv_native
                    THEN ABS(stored_native - ncv_native) END), 0)   AS abs_dev_cents,
  ROUND(COALESCE(MAX(CASE WHEN stored_native <> ncv_native
                    THEN ABS(stored_native - ncv_native) * 100.0 / ncv_native END), 0), 2)
                                                                    AS max_dev_pct
FROM v
GROUP BY ncv_source
ORDER BY ncv_source;

-- ---------- ④ 不一致明细（回填涉及的行） ----------
WITH base AS (
  SELECT COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY') AS code
),
calc AS (
  SELECT
    i.id                                   AS item_id,
    i.name                                 AS item_name,
    i.currency_code                        AS item_ccy,
    b.code                                 AS base_ccy,
    i.purchase_date,
    i.total_cost_cents,
    i.cost_native_cents                    AS stored_native,
    CASE WHEN t.is_deleted = 0
          AND t.currency_code = i.currency_code
          AND t.fx_rate_used IS NOT NULL
         THEN t.fx_rate_used END           AS trace_rate,
    CASE WHEN i.currency_code <> b.code THEN
      COALESCE(
        (SELECT h.rate FROM fx_rate_history h
          WHERE h.base_code  = i.currency_code
            AND h.quote_code = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1')),
        (SELECT 1.0 / h.rate FROM fx_rate_history h
          WHERE h.quote_code = i.currency_code
            AND h.base_code  = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1'))
      )
    END                                   AS series_rate
  FROM items i
  CROSS JOIN base b
  LEFT JOIN transactions t ON t.id = i.purchase_transaction_id
  WHERE i.is_deleted = 0
),
v AS (
  SELECT
    calc.*,
    CASE
      WHEN item_ccy = base_ccy     THEN 'same_base'
      WHEN trace_rate IS NOT NULL  THEN 'trace'
      WHEN series_rate IS NOT NULL THEN 'series'
      ELSE 'series_gap'
    END AS ncv_source,
    CASE
      WHEN item_ccy = base_ccy     THEN total_cost_cents
      WHEN trace_rate IS NOT NULL  THEN CAST(ROUND(total_cost_cents * trace_rate) AS INTEGER)
      WHEN series_rate IS NOT NULL THEN CAST(ROUND(total_cost_cents * series_rate) AS INTEGER)
      ELSE NULL
    END AS ncv_native
  FROM calc
)
SELECT
  item_id, item_name, item_ccy, purchase_date, total_cost_cents,
  ncv_source, stored_native, ncv_native,
  stored_native - ncv_native                                              AS dev_cents,
  ROUND((stored_native - ncv_native) * 100.0 / ncv_native, 2)             AS dev_pct
FROM v
WHERE stored_native <> ncv_native
ORDER BY ABS(stored_native - ncv_native) DESC;

-- ---------- ⑤ series_gap 明细：新口径重查必然报错的行（回填不可达） ----------
WITH base AS (
  SELECT COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY') AS code
),
calc AS (
  SELECT
    i.id                                   AS item_id,
    i.name                                 AS item_name,
    i.currency_code                        AS item_ccy,
    b.code                                 AS base_ccy,
    i.purchase_date,
    i.total_cost_cents,
    i.cost_native_cents                    AS stored_native,
    CASE WHEN t.is_deleted = 0
          AND t.currency_code = i.currency_code
          AND t.fx_rate_used IS NOT NULL
         THEN t.fx_rate_used END           AS trace_rate,
    CASE WHEN i.currency_code <> b.code THEN
      COALESCE(
        (SELECT h.rate FROM fx_rate_history h
          WHERE h.base_code  = i.currency_code
            AND h.quote_code = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1')),
        (SELECT 1.0 / h.rate FROM fx_rate_history h
          WHERE h.quote_code = i.currency_code
            AND h.base_code  = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1'))
      )
    END                                   AS series_rate
  FROM items i
  CROSS JOIN base b
  LEFT JOIN transactions t ON t.id = i.purchase_transaction_id
  WHERE i.is_deleted = 0
)
SELECT item_id, item_name, item_ccy, purchase_date, total_cost_cents, stored_native
FROM calc
WHERE item_ccy <> base_ccy
  AND trace_rate IS NULL
  AND series_rate IS NULL
ORDER BY purchase_date;

-- ---------- ⑥ 按购买年分布 ----------
WITH base AS (
  SELECT COALESCE((SELECT value FROM app_settings WHERE key='ledger.base_currency'),'CNY') AS code
),
calc AS (
  SELECT
    i.id                                   AS item_id,
    i.currency_code                        AS item_ccy,
    b.code                                 AS base_ccy,
    i.purchase_date,
    i.total_cost_cents,
    i.cost_native_cents                    AS stored_native,
    CASE WHEN t.is_deleted = 0
          AND t.currency_code = i.currency_code
          AND t.fx_rate_used IS NOT NULL
         THEN t.fx_rate_used END           AS trace_rate,
    CASE WHEN i.currency_code <> b.code THEN
      COALESCE(
        (SELECT h.rate FROM fx_rate_history h
          WHERE h.base_code  = i.currency_code
            AND h.quote_code = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1')),
        (SELECT 1.0 / h.rate FROM fx_rate_history h
          WHERE h.quote_code = i.currency_code
            AND h.base_code  = b.code
            AND h.week_start = date(i.purchase_date, '-6 days', 'weekday 1'))
      )
    END                                   AS series_rate
  FROM items i
  CROSS JOIN base b
  LEFT JOIN transactions t ON t.id = i.purchase_transaction_id
  WHERE i.is_deleted = 0
),
v AS (
  SELECT
    calc.*,
    CASE
      WHEN item_ccy = base_ccy     THEN total_cost_cents
      WHEN trace_rate IS NOT NULL  THEN CAST(ROUND(total_cost_cents * trace_rate) AS INTEGER)
      WHEN series_rate IS NOT NULL THEN CAST(ROUND(total_cost_cents * series_rate) AS INTEGER)
      ELSE NULL
    END AS ncv_native
  FROM calc
)
SELECT
  strftime('%Y', purchase_date)                                     AS purchase_year,
  COUNT(*)                                                          AS rows_total,
  COALESCE(SUM(CASE WHEN ncv_native IS NOT NULL
                     AND stored_native <> ncv_native THEN 1 ELSE 0 END), 0)
                                                                    AS rows_inconsistent,
  SUM(CASE WHEN ncv_native IS NULL THEN 1 ELSE 0 END)               AS rows_series_gap
FROM v
GROUP BY purchase_year
ORDER BY purchase_year;
```

---

## 附录 B：夹具验证结果（节选）

8 行夹具（`same_base` 一致/不一致、`trace` 一致/不一致、`series` 一致/不一致、`series_gap`、软删排除）跑附录 A，③④⑤⑥ 全部与手算预期逐格一致：

| 段 | 预期 | 实际 |
|---|---|---|
| ② 分布 | all=8, deleted=1, live=7, cross=5 | `8\|1\|7\|5` ✓ |
| ③ 汇总 | same_base 2/1/1000/10.0；series 2/1/200/2.56；series_gap 1；trace 2/1/2500/3.68 | 逐格一致 ✓ |
| ④ 明细 | i1(trace +2500)、i7(same_base −1000)、i3(series −200)，按绝对偏差降序 | 一致 ✓ |
| ⑤ series_gap | 仅空缺周行 | 一致 ✓ |
| ⑥ 按年 | 2025：1/0/1；2026：6/3/0 | 一致 ✓ |
