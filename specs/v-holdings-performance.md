# 持仓视图 v_holdings 性能优化需求

> 记录 `src-tauri/migrations/V002__investment.sql` 中 `v_holdings` 视图的正确性缺陷与性能待优化点。当前视图在个人记账数据量下可运行，但存在跨币种盈亏计算错误，且随行情、汇率、持仓批次数据增长，多处写法可能退化为高复杂度查询。

## 问题

`v_holdings` 视图实时聚合 `security_lots` 并关联 `market_prices` 与 `exchange_rates` 计算市值和未实现盈亏。当前实现既存在跨币种盈亏等正确性缺陷，也存在随数据增长退化的性能写法：

✅ 1. **跨币种未实现盈亏计算错误（正确性）**：视图把 `market_value_cents` 折算到账户本位币 `a.currency_code`，但 `cost_basis_cents` 仍是 lot 成本币种 `h.currency_code`，从未折算。`unrealized_pnl_cents = market_value_cents - cost_basis_cents` 在 lot 币种 ≠ 账户币种时（如 CNY 账户持 USD 标的）等于“账户币市值 − 成本币成本”，两个币种直接相减，结果错误。现有测试 `db.rs` 的 `market_price_and_holding_view` 用同币种账户+标的，恰好掩盖了该场景。
✅ 2. **`exchange_rates` 保留历史行情导致取最新汇率需关联子查询**：`exchange_rates` 唯一键是 `UNIQUE(base_code, quote_code, priced_at)`，`create_exchange_rate` 是裸 `INSERT`，每个货币对按日期累积多行历史。视图因此必须用 `WHERE priced_at = (SELECT MAX(priced_at) ...)` 关联子查询挑最新行。但汇率历史对本应用无必要——`amount_native_cents` 在交易创建时已按当时汇率快照写入，事后无需按历史汇率重算。应像 `market_prices`（`UNIQUE(instrument_id)` + `create_market_price` upsert）一样收敛为单行最新，视图里即可变普通 `LEFT JOIN`，关联子查询自然消失。
✅ 3. **聚合缺少 partial / covering index**：`security_lots` 的 `WHERE remaining_quantity > 0` 聚合只命中 `idx_security_lots_account_instrument` 的前两列，需要回表读取 `remaining_quantity`、`cost_per_unit_cents`、`currency_code`、`updated_at`。
✔️ 4. **每次调用全量计算视图**：`list_holdings` 直接 `SELECT * FROM v_holdings`，没有分页或过滤，行情/汇率也全量参与计算。当前全量查询即为预期行为，无需修改。
✅ 5. **`id` 可能重复**：`id` 是 `account_id || '-' || instrument_id`，但 `GROUP BY` 还包含 `currency_code`，极端情况下会生成重复 key。
✅ 6. **未过滤软删除账户**：视图 `FROM (...) h LEFT JOIN accounts a`，仅在 JOIN 上加 `is_deleted = 0` 只会让已删账户的 `a.*` 变 NULL（进而因 `a.currency_code` 为 NULL 使汇率 join 失败、市值静默变 NULL），holding 行照常返回。要真正排除需在聚合子查询里过滤或改 INNER JOIN。
✅ 7. **汇率只查单向**：只查找 `base_code = 价格币种 AND quote_code = 账户币种`，若库中只存了反向汇率则得到 `NULL`。
✅ 8. **`exchange_rate_for_date` / `convert_to_native` 的日期取数逻辑与单行设计冲突**：当前 `exchange_rate_for_date` 用 `priced_at <= 目标日期 ORDER BY priced_at DESC LIMIT 1` 取时点汇率，服务于交易创建时的 `amount_native_cents` 折算。若按问题 2 收敛为单行最新，该日期参数与 `priced_at <=` 过滤变得多余——库里每个货币对只有一行。应简化为按 `(base_code, quote_code)` 直查一行；副作用是补录历史日期的交易会用当前汇率折算（对个人记账可接受，因 `amount_native_cents` 创建时快照、事后不重算）。

## 次要问题

- 视图未 JOIN `instruments`，`list_holdings` 只返回 `instrument_id`，前端拿不到 symbol/name。
- `quantity` 为 REAL，与 INTEGER cents 相乘再 `SUM` 后 `CAST`，大数量下有浮点精度风险。
- `updated_at` 取 `MAX(lot.updated_at)`，反映批次更新时间而非市值更新时间，语义易误解。

## 优化方向

> 标记说明：✅ 已实现 / ❌ 未实现 / ✔️ 无需修改

✅ 1. 修正跨币种盈亏：对 `cost_basis_cents` 再做一次 `exchange_rates` join（成本币 `h.currency_code` → 账户币 `a.currency_code`），使市值与成本同币种后再相减；或统一用 `instruments.currency_code` 作为成本币种并同样折算。
✅ 2. 将 `exchange_rates` 收敛为每货币对单行最新：唯一键改 `UNIQUE(base_code, quote_code)`，`create_exchange_rate` 改 `ON CONFLICT(base_code, quote_code) DO UPDATE` upsert（参照 `create_market_price`），`priced_at` 保留为“行情采集时间”但不再参与唯一键与取数。视图的 `exchange_rates` 子查询随之改为普通 `LEFT JOIN`，关联子查询消失，无需窗口函数改写。
✅ 3. 补充 `security_lots` 索引：仅保留针对 `remaining_quantity > 0` 的 partial index，作为聚合的覆盖索引。实现为 `idx_security_lots_active_covering(account_id, instrument_id, currency_code, remaining_quantity, cost_per_unit_cents, updated_at) WHERE remaining_quantity > 0`——前三列对齐 GROUP BY 提供有序扫描，后三列覆盖 `SUM(remaining_quantity)` / `SUM(remaining_quantity * cost_per_unit_cents)` / `MAX(updated_at)` 免回表（相比初版提议补入 `remaining_quantity` 列，否则 `SUM(remaining_quantity)` 仍需回表）。原 `idx_security_lots_account_instrument` 与 partial index 重复，且 account_id+instrument_id 查询已由 `UNIQUE(account_id, instrument_id, buy_transaction_id)` 自动索引覆盖，已删除。
✔️ 4. 经评估当前全量查询即为预期行为（持仓行数有限，账户/标的过滤与分页收益不抵 API 复杂度），不修改；次要问题中“视图未 JOIN `instruments` 致前端拿不到 symbol/name” 可在前端按 instrument_id 自行关联 instruments 表解决。
✅ 5. 修正 `id` 生成逻辑：将 `currency_code` 纳入 `id`（采用 `account_id || '-' || instrument_id || '-' || currency_code`），与 `GROUP BY account_id, instrument_id, currency_code` 一致，避免同账户同标的但 lot 币种不同时生成重复 key。保留 `GROUP BY currency_code`：因 `security_lots` 以 lot 成本币种存成本，不同币种 lot 是独立财务行，合并会丢失成本币信息。
✅ 6. 过滤软删除账户：在 `h` 聚合子查询里加 `AND account_id IN (SELECT id FROM accounts WHERE is_deleted = 0)`，在聚合前剔除已删账户的 lot，避免其持仓行进入视图。
✅ 7. 汇率查找支持反向汇率兌底：视图 `v_holdings` 对 `er`/`ec` 各增加反向 join `er_rev`/`ec_rev`，CASE 表达式在正向 `rate IS NULL` 时用 `除以反向 rate` 取倒数折算；`commands.rs::exchange_rate` 同样在正向查不到时查 `quote→base` 并返回 `1/rev`（同币种直返回 1.0，反向 rate 非正时报错）。
✅ 8. 同步简化 `exchange_rate_for_date` / `convert_to_native`：去掉 `priced_at <= 日期` 的时点取数，改为按 `(base_code, quote_code)` 单行直查；`convert_to_native` 的 `rate_date` 参数可移除。明确汇率只反映“最新”快照，不再支持 as-of 日期。

## 关联

- `src-tauri/migrations/V002__investment.sql` 的 `v_holdings` 视图与 `market_prices` 的 `UNIQUE(instrument_id)` 约束
- `src-tauri/src/commands.rs` 的 `list_holdings` 命令与 `create_market_price` 的 upsert 语义
- `src-tauri/src/commands.rs` 的 `exchange_rate_for_date` / `convert_to_native` / `create_exchange_rate`（单行化后需简化取数与写入）
- `src-tauri/migrations/V001__initial.sql` 的 `exchange_rates` 表（`UNIQUE(base_code, quote_code, priced_at)` 需改）、`idx_exchange_rates_lookup` 索引与 `accounts.is_deleted` 字段
- `src-tauri/src/db.rs` 的 `market_price_and_holding_view` 测试（当前未覆盖跨币种场景）
