# Ledger 导入知识

## 完整契约

端点契约（请求/响应结构）见 `GET /api/v1/openapi.json`。本知识只含导入必需的确定性约定。

## 金额单位（分）

所有金额字段以「分」为单位（字段名带 `_cents` 后缀），一律用整数。
元换算成分：× 100 后取整（如 45.50 元 → `amount_cents = 4550`）。

## 币种名映射

把源数据的中文币种名映射为 `currency_code`：人民币 → `CNY`、港币 → `HKD`。
完整映射用 `GET /api/v1/currencies` 获取，勿硬编码猜测。

## 日期格式

严格使用 `YYYY-MM-DD`（如 2026-01-15），不要带时间部分。

## 交易类型判定（按金额正负）

对每一行，看 `流入金额` / `流出金额` 两列：
- `流入金额` > 0 → `kind = income`，金额取 `流入金额`
- `流出金额` > 0 → `kind = expense`，金额取 `流出金额`
- 两列同时 > 0（通常相等）→ `kind = transfer`，金额任取其一
- 两列同时为 0 → 无金额变动，无法生成合法交易（`amount_cents` 必须 > 0），跳过该行

## 转账拆分（A → B）

`资金账户` 含 ` → `（空格 + 箭头 + 空格）时拆成两个账户：
- `account_id` = 箭头左侧账户（转出方）
- `to_account_id` = 箭头右侧账户（转入方）
- `kind = transfer`，`amount_cents` 取流入/流出金额（二者相等）

## 黑洞账户（资金账户=无）

黑洞账户信号**只看 `资金账户` 列**（含 `→` 拆出的任一侧）；`收支大类=无` 不映射黑洞账户，只影响分类选择。

`资金账户` 为 `无`，或转账任一侧为 `无` 时，映射到预置黑洞账户
（`GET /api/v1/accounts` 返回，`is_hidden = true`，按币种名为 `无(CNY)` / `无(HKD)`，type 为 other）：
- 普通交易 `无` → `account_id` 指向黑洞账户，kind 照常按金额正负判定
- `x → 无` → 转账，`to_account_id` 指向黑洞账户
- `无 → x` → 转账，`account_id` 指向黑洞账户

## 幂等与去重（dedup）

- 账户/分类创建按自然键幂等：重复创建返回已有 id，不报错、不重复插入，可放心重跑。
- `POST /api/v1/transactions/batch` 默认开启去重（`dedup` 缺省 `true`）：
  - `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`
  - 字段集排除 note/category；`to_account_id` 缺省拼空串
  - 命中已存在（未删除）交易返回 `{success: true, duplicate: true, id: null}` —— 非新建也非失败，无需重试、不应上报错误
- 只匹配未删除交易：软删除后重跑会重新写入；`dedup: false` 可强制重复写入。
- `dedup_hash` 导入后保持不变，编辑备注/分类不改变它。
- 因此每行拆解必须确定性一致：同一天、同 kind、同金额、同账户的拆法若变化，哈希即漂移、去重失效。

## 备注与标签

`备注` 列写入 `note`；`标签` 列不参与交易映射，可忽略或并入 `note`。
