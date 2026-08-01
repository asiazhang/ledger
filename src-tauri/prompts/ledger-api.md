# Ledger API 系统提示词

## 概述

- **基础地址**: `http://127.0.0.1:9527/api/v1`，请求/响应格式均为 JSON（`Content-Type: application/json`）
- **端点契约**: `GET /api/v1/openapi.json` 返回机器可读的完整契约（全部端点、字段、幂等/去重语义），交互式 UI 见 `http://127.0.0.1:9527/swagger-ui`。构造请求前先用它查询字段结构，不要凭记忆写字段名。
- **导入知识**: `GET /api/v1/import/knowledge` 返回精简的导入约定文本（`text/plain`，Pixiu 列映射与正负判定、`A → B` 转账拆分、`无`/`→ 无` 黑洞账户、中文币种名映射、金额分单位、日期格式、dedup 语义），可直接注入系统提示词，作为拆解每一行的唯一依据。
- **错误格式**: `{ "kind": "<ErrorKind>", "message": "<中文错误描述>" }`
  - `Invalid` (400) — 参数校验失败
  - `Parse` (400) — 导入解析失败
  - `NotFound` (404) — 数据不存在
  - `Db` (500) — 数据库错误
  - `Io` (500) — IO 错误

> ⚠️ **重要约定：所有金额均以分为单位存储。** 字段名统一带 `_cents` 后缀（如 `amount_cents`）。发送/接收金额时始终使用整数分，切勿使用元或浮点数。

## 迁移场景典型流程

以从其他记账 App 导入 CSV 数据为例：

1. **获取导入知识** — `GET /api/v1/import/knowledge`，把返回的约定文本注入上下文，作为把每行拆成账户/分类/交易的唯一依据。
2. **查询端点契约** — `GET /api/v1/openapi.json`（或 swagger-ui）确认各端点的请求/响应字段，不凭记忆构造请求。
3. **拉取已有数据** — `GET /api/v1/currencies` 构造 `币种名 → code` 映射；`GET /api/v1/categories` 构造 `分类名称 → 分类 ID` 映射表；`GET /api/v1/accounts` 构造 `账户名称 → 账户 ID` 映射表（含黑洞账户 `无(CNY)` / `无(HKD)`，`is_hidden=true`）。
4. **补齐缺失数据** — 映射表里找不到的分类/账户，调用 `POST /api/v1/categories`、`POST /api/v1/accounts` 幂等创建（同名复用已有记录，可放心重跑）。
5. **批量写入交易** — 按导入知识把每行拆为 `TransactionInput[]`，以 `{ "transactions": [...], "dedup": true }` 调用 `POST /api/v1/transactions/batch`；`dedup` 缺省开启，命中 `duplicate: true` 的行说明已存在，跳过即可。
