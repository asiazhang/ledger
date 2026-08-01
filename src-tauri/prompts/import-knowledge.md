# Ledger 导入知识

- 金额一律以「分」存储（字段带 `_cents`）：元 × 100 取整（45.50 元 → 4550）。
- 中文币种名 → `currency_code`：人民币 → CNY、港币 → HKD；完整清单 `GET /api/v1/currencies`。
- 日期严格 `YYYY-MM-DD`。

## 每行拆解（确定性，勿漂移）

- 流入金额 > 0 → `income`，金额取流入；流出金额 > 0 → `expense`，金额取流出。
- 两列同时 > 0（通常相等）→ `transfer`；两列同时为 0 → 无金额变动，跳过该行。
- `资金账户` 含 ` → ` → `transfer`：`account_id`=箭头左侧（转出），`to_account_id`=箭头右侧（转入），金额任取一列。
- `资金账户` = 无（或转账任一侧为无）→ 映射黑洞账户 `无(CNY)` / `无(HKD)`（`GET /api/v1/accounts` 返回，`is_hidden=true`）。黑洞信号只看资金账户列；`收支大类=无` 不是黑洞信号。
- `备注` → `note`；`标签` 可忽略或并入 `note`。

## 幂等与去重

- 账户/分类创建按自然键幂等：重复创建返回已有 id，可放心重跑。
- 批量交易默认去重：`dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`，排除 note/category。
- 命中 `duplicate: true` → 该交易已存在，跳过即可，不算错误、无需重试。
- 每行拆法必须固定不变，否则哈希漂移、去重失效。

端点字段结构见 `GET /api/v1/openapi.json`。
