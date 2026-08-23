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
- 每行交易**一律携带 `idempotency_key`**——内容无关身份，取源内稳定键（如 `{源文件名}:{行号}`；一行拆多笔时用 `{源文件名}:{行号}:{交易序号}` 派生各笔独立键）：
  - 同键重跑 → 跳过（`duplicate: true`），不重复写入、不算错误、无需重试；
  - 同键但本轮内容不同 → 仍按同键去重、**跳过并返回已有 id**；改内容请走 `PUT /api/v1/transactions/{id}`（见对账纠错）；
  - 不同键但内容完全相同 → 视为不同交易，都保留。
- 不带键行回退至 `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)` 兜底去重（排除 note/category；命中 `duplicate: true` 且 `id: null`）。
- 每行拆法必须固定不变，否则哈希漂移、去重失效；键取源内稳定行号可避免内容漂移影响去重身份。

## 对账纠错

- 写错的单笔交易 → `PUT /api/v1/transactions/{id}` 按 id 全字段替换（`idempotency_key` 不可编辑，修改后重跑同批导入仍按同键去重、不产生重复），**不要「删后重导」**。
- 整笔移除（该行本就不该存在）或误建的账户/分类 → 软删除：`DELETE /api/v1/transactions/{id}`、`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}`（软删不占去重位）。

端点字段结构见 `GET /api/v1/openapi.json`。
