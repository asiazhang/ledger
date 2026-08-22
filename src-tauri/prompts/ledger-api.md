# Ledger API 系统提示词

Ledger 在本地 `http://127.0.0.1:9527` 提供 HTTP API，供 AI 编程助手读写记账数据（账户、分类、交易），主要场景是把历史账本数据迁移进 Ledger，也可直接录入记账。

## 迁移步骤

1. `GET /api/v1/openapi.json` 发现全部端点与字段契约。端点、字段、幂等与去重语义一律以契约为准，不凭记忆。
2. 写交易前 `GET /api/v1/import/knowledge`（`text/plain`）获取拆行约定，作为把每一行拆成账户/分类/交易的依据。
3. 按约定迁移：账户/分类幂等创建（重复创建返回已有 id，可放心重跑），交易走 `POST /api/v1/transactions/batch` 批量写入。
4. **对账**——迁移完成的判定，以下两项全过才算完成：
   - `GET /api/v1/transactions` 按日期区间过滤（区间取源文件覆盖范围）核对：源文件各行是否全部落库、金额是否一致；
   - `GET /api/v1/accounts/balances`（**含黑洞账户**）核对各账户期末余额与源数据吻合。

## 对账不过：删除纠错后重跑

`DELETE /api/v1/transactions/{id}`、`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}` 均为软删除；删掉写错的行或误建的账户/分类后，重跑同一批导入即重新写回（去重只匹配未删除交易，软删不占去重位）。

## 错误响应

`{ "kind": "<Invalid|Parse|NotFound|Db|Io>", "message": "<中文错误描述>" }`
