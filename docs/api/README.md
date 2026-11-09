# API 门牌

本地 HTTP API 是 AI 会话（AI 记账 / 数据迁移）的统一入口：`http://127.0.0.1:9527`，仅本机回环，术语与边界见 `docs/contexts/CONTEXT-ai-import.md`。端点契约唯一权威 = 运行期 `GET /api/v1/openapi.json`，本页零手写端点细节。

自描述入口（新需求先读）：`GET /api/v1/openapi.json`（机器可读契约）· `GET /api/v1/import/knowledge`（导入基础知识，含知识索引；投资节见 `/api/v1/import/knowledge/investment`）· `src-app/prompts/ledger-api.md`（AI 入口提示词模板）。

| 想知道 | 从代码查 |
|---|---|
|| HTTP 端点契约 / 实现 | 运行期 `GET /api/v1/openapi.json`；`src-app/src/api_server/`（契约装配 `openapi.rs`、路由表 `router.rs`、端点 `handlers/`）|
| 前端调用封装与类型 | `packages/api/src/index.ts` + `packages/types/src/index.ts`（issue #1150 起类型抽为 `@ledger/types` 包，issue #1155 起调用面抽为 `@ledger/api` 包） |

领域边界一句话：HTTP API 服务 AI 编程助手（AI 导入域唯一壳通道；#2083 切轨后产品壳为 NSH 直调域入口、IPC 面已退役），HTTP 层是薄壳编排，写入口径统一收口 Writer / Amount 接缝（见 `AGENTS.md`）。
