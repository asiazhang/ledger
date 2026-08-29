# Ledger API 系统提示词

Ledger 在本地 `http://127.0.0.1:9527` 提供 HTTP API，供 AI 编程助手读写记账数据（账户、分类、商户、交易），主要场景是把历史账本数据迁移进 Ledger，也可直接录入记账。

## 迁移步骤

1. `GET /api/v1/openapi.json` 发现全部端点与字段契约。端点、字段、幂等与去重语义一律以契约为准，不凭记忆。
2. 写交易前 `GET /api/v1/import/knowledge`（`text/plain`）获取拆行约定，作为把每一行拆成账户/分类/交易的依据。
3. 按约定迁移：账户/分类幂等创建（重复创建返回已有 id，可放心重跑）；交易走 `POST /api/v1/transactions/batch` 批量写入，**每行携带 `idempotency_key`**（内容无关身份，取源内稳定键如 `{源文件名}:{行号}`；一行拆多笔时以交易序号派生独立键）。交易可带 `merchant_name`（商户名字符串）：后端精确匹配复用或未命中即建，无需自行去重；建议先 `GET /api/v1/merchants` 拉取已有商户按已有名提交。去重与纠错语义以 `GET /api/v1/import/knowledge` 为准。
4. **对账**——迁移完成的判定，以下两项全过才算完成：
   - `GET /api/v1/transactions` 按日期区间过滤（区间取源文件覆盖范围）核对：响应为 `{items, total}`，读回取 `.items`；不传分页参数（`page`/`page_size`）即返回满足条件的全部交易，逐行核对源文件各行是否全部落库、金额是否一致（超大账本也可用 `page`/`page_size` 分批读回，以 `total` 核对总条数）；按账户核对（含转账转入侧）时加 `involving_account_id`（涉及账户：`account_id` 或 `to_account_id` 命中即算，见 `GET /api/v1/openapi.json`）；
   - `GET /api/v1/accounts/balances`（**含黑洞账户**）核对各账户期末余额与源数据吻合。

## 对账不过：按 id 修改纠错

写错的单笔交易用 `PUT /api/v1/transactions/{id}` 按 id 全字段替换（id 取自 `GET /api/v1/transactions` 的 `items[].id`）；`idempotency_key` 不作为可编辑字段，修改后重跑同批导入仍按同键去重、不产生重复——**不要「删后重导」**。整笔移除（该行本就不该存在）或误建的账户/分类仍走软删除：`DELETE /api/v1/transactions/{id}`、`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}`（软删不占去重位）。

## 错误响应

`{ "kind": "<Invalid|Parse|NotFound|Db|Io>", "message": "<中文错误描述>" }`
