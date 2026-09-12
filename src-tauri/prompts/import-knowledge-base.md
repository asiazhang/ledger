# Ledger 导入知识

- 金额一律以「分」存储（字段带 `_cents`）：元 × 100 取整（45.50 元 → 4550）。
- **价格刻度**：buy/sell 成交单价与标的价格以**万分之一元**存储（4 位小数保真，元 × 10000）；行金额 `amount_cents` 与 `fee_cents` 仍是分——字段级细节以契约为准。
- 中文币种名 → `currency_code`：人民币 → CNY、港币 → HKD；完整清单 `GET /api/v1/currencies`。
- 日期严格 `YYYY-MM-DD`。

## 知识索引（分域知识节按需获取）

导入知识按域分节：本页是基础知识（记账主路径一次拉取即覆盖），分域知识节按会话内容按需获取。

- **投资节**：投资交易（buy / sell）、基金申赎、基金转换、份额调整、现金分红五节——标的解析与投资落账，及投资的对账与纠错口径，全文 `GET /api/v1/import/knowledge/investment`（`text/plain`）。
- **何时需要读**：会话中出现投资类流水（`buy` / `sell` / `convert` / `split` / `dividend`，即下方「迁移拆行」所指）或投资标的，即须先拉取投资节再落账，且整场会话黏住——对账与纠错口径一并补读；投资类记录不得套用普通收支写法。

## 每行拆解（确定性，勿漂移）

- 流入金额 > 0 → `income`，金额取流入；流出金额 > 0 → `expense`，金额取流出。
- 两列同时 > 0（通常相等）→ `transfer`；两列同时为 0 → 无金额变动，跳过该行。
- `资金账户` 含 ` → ` → `transfer`：`account_id`=箭头左侧（转出），`to_account_id`=箭头右侧（转入），金额任取一列。
- `资金账户` = 无（或转账任一侧为无）→ 映射黑洞账户 `无(CNY)` / `无(HKD)`（`GET /api/v1/accounts` 返回，`is_hidden=true`）。黑洞信号只看资金账户列；`收支大类=无` 不是黑洞信号。
- `备注` → `note`；`标签` 可忽略或并入 `note`。
- 迁移拆行：通用行只产生 `income` / `expense` / `transfer` 三类 kind；投资流水走 `buy` / `sell`（见「投资交易」）、`convert`（基金转换，见「基金转换」）、`split`（份额调整，见「份额调整」）与 `dividend`（现金分红，见「现金分红」）。四者均可用。

## 商户（Merchant）

- 交易可带 `merchant_name`（与 `merchant_id` 互斥）：**同一商户始终用同一名字**，想复用已有名先 `GET /api/v1/merchants` 拉在用列表按名提交——归一化责任在后端（命中复用、未命中即建）。
- 仅 `income` / `expense` / `transfer` 可携带商户；`buy` / `sell` / `convert` / `split` / `dividend` 不能带（提交会被拒绝）；退款（refund）自动继承原支出商户，携带的商户会被忽略。

## 个人间借贷（借出 / 借入 / 还款）

- 借贷不是收支，本金往来一律落 `transfer`（不增减收支报表）：借出 = `transfer` 自资金账户转入 `receivable`（借出款）账户；借入 = `transfer` 自 `debt`（负债）账户转入资金账户；还款/收回 = 反向转账，部分还款即多笔。切勿把「张三借了我 5000」记成 `expense`——现金减少但支出报表失真。
- 一人一账户：对手方由账户承载，账户类型取 `receivable` / `debt`，命名约定「借出·张三」（应收）/「借入·李四」（欠款）；同名账户按自然键幂等复用（同一人所有借贷落同一账户）。`receivable` 余额为正 = 对方尚未归还，`debt` 余额为负 = 我方尚未偿还；「借出·张三」余额即张三未还金额。
- 借贷行不带商户（AI 落账不输出 `merchant_name` / `merchant_id`）：对手方是账户，不为借贷对手建商户、不写人员标注。transfer 携带商户是人工关联能力，AI 不用它。
- 利息才进收支：收息记 `income`、付息记 `expense`（与本金转账分笔各落一笔）；本金往来永远走转账，不混入收支。
- 建账时既有的借出/欠款经账户期初余额 `initial_balance_cents` 表达（外借填正值、欠款填负值），不伪造历史交易补账。
- 不代做坏账核销：对方确定不还时，由用户在应用内把该账户余额调整归零（不计收支），AI 只忠实记录实际发生的交易，不自行清零余额。
- 对账沿用普通转账口径：读回核对按涉及账户过滤（`involving_account_id`），余额核对（`GET /api/v1/accounts/balances`）天然覆盖借贷账户，借贷行与普通转账行同一条判定标准。

## 幂等与去重

- 账户/分类创建按自然键幂等：重复创建返回已有 id，可放心重跑。
- 每行交易**一律携带 `idempotency_key`**——内容无关身份，取源内稳定键（如 `{源文件名}:{行号}`；一行拆多笔时用 `{源文件名}:{行号}:{交易序号}` 派生各笔独立键）：
  - 同键重跑 → 跳过（`duplicate: true`），不重复写入、不算错误、无需重试；
  - 同键但本轮内容不同 → 仍按同键去重、**跳过并返回已有 id**；改内容请走 `PUT /api/v1/transactions/{id}`（见对账纠错）；
  - 不同键但内容完全相同 → 视为不同交易，都保留。
- 不带键行回退至 `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)` 兜底去重（排除 note/category；携带出资账户时追加入哈希：仅出资账户不同的两笔不互相去重；命中 `duplicate: true` 且 `id: null`）。
- 每行拆法必须固定不变，否则哈希漂移、去重失效。

## 对账完成判定

迁移完成的判定，以下两项全过才算完成：

- **读回核对**：`GET /api/v1/transactions` 按日期区间过滤（区间取源文件覆盖范围）核对：响应为 `{items, total}`，读回取 `.items`；不传分页参数（`page`/`page_size`）即返回满足条件的全部交易，逐行核对源文件各行是否全部落库、金额是否一致；按账户核对（含转账转入侧）时加 `involving_account_id`（涉及账户：`account_id` 或 `to_account_id` 命中即算），账户 id 取自 `GET /api/v1/accounts`（**含黑洞账户**）。
- 读回过滤参数全部可选：`kinds=expense,refund` 逗号分隔多类型（与其余维度 AND 组合）、`category_id` / `merchant_id` 按分类/商户精确过滤（含软删字典的历史行）、`uncategorized_only=true` 仅无分类行、`limit` 取前 N 条与分页互斥；默认按日期倒序稳定排序，翻页无重复无遗漏。
- **查询纪律**：子集检查——只确认某类行是否存在或求合计——直接用服务端过滤参数查询（如 `kinds=buy,sell`），返回行即全部待核对对象。
- **分页纪律**：分页读回时响应 `total` 是满足过滤条件的总条数，`len(items)` 只是本页条数；未核对 `total` 前不得下「不存在 / 已全部读回」的结论（按日期倒序的首页只覆盖最新一段，更早区间可能仍有行）。
- buy/sell/convert 行核对标的关联认 `source` 字段（`kind: "instrument"`，`entity_id` 为标的 id、`display_name` 为「代码 名称」；convert 行是转出标的）；交易行上没有 `instrument_id` / `quantity` 字段（二者只出现在写入入参，不经交易读回返回；convert 两腿读回投影见契约）。
- **余额核对**：`GET /api/v1/accounts/balances`（**含黑洞账户**）核对各账户期末余额与源数据吻合。

## 对账纠错

- 写错的单笔交易 → `PUT /api/v1/transactions/{id}` 按 id 全字段替换（幂等键保持不变，修改后重跑同批导入仍按同键去重、不产生重复），**不要「删后重导」**。
- 整笔移除（该行本就不该存在）或误建的账户/分类 → 软删除：`DELETE /api/v1/transactions/{id}`、`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}`（软删不占去重位）。buy/sell / 转换删除的持仓回补与级联清理见「投资交易」「基金转换」节纠错条，重导前无需手工清理持仓残留。
- 商户名写错 → `PUT /api/v1/merchants/{id}` 改名：改名即时生效、历史交易照常引用，无需重导。

端点与字段契约见 `GET /api/v1/contract`（紧凑 JSON 方言，首部图例解释形状）。
