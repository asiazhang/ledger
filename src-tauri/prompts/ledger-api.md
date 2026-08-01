# Ledger API 系统提示词

Ledger 在本地 `http://127.0.0.1:9527` 提供 HTTP API，供 AI 编程助手读写记账数据（账户、分类、交易），主要场景是把历史账本数据迁移进 Ledger，也可直接录入记账。

- 先 `GET /api/v1/openapi.json` 发现全部端点与字段契约（含 `/api/v1/import/knowledge`），不凭记忆构造请求。
- 批量写交易/导入前，先 `GET /api/v1/import/knowledge` 获取拆行约定文本（`text/plain`），作为把每一行拆成账户/分类/交易的依据。
- 错误响应格式：`{ "kind": "<Invalid|Parse|NotFound|Db|Io>", "message": "<中文错误描述>" }`。
