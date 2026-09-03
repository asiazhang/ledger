# API 门牌

本地 HTTP API 是 AI 驱动导入的唯一入口：`http://127.0.0.1:9527`，仅本机回环，术语与边界见 `docs/contexts/CONTEXT-ai-import.md`。端点契约唯一权威 = 运行期 `GET /api/v1/openapi.json`，本页零手写端点细节。

自描述入口（新需求先读）：`GET /api/v1/openapi.json`（机器可读契约）· `GET /api/v1/import/knowledge`（导入约定纯文本）· `src-tauri/prompts/ledger-api.md`（AI 入口提示词模板）。

| 想知道 | 从代码查 |
|---|---|
|| HTTP 端点契约 / 实现 | 运行期 `GET /api/v1/openapi.json`；`src-tauri/src/api_server/`（契约装配 `openapi.rs`、路由表 `router.rs`、端点 `handlers/`）|
| 已注册 IPC 命令 | `src-tauri/src/lib.rs` 的 `generate_handler!` |
| 前端调用封装与类型 | `src/api/index.ts` + `src/types/index.ts` |
| 命令实现 / serde 契约类型 | `src-tauri/src/commands/` / `src-tauri/src/models/` |

领域边界一句话：HTTP API 服务 AI 编程助手、IPC 服务前端，两层皆为薄壳编排，写入口径统一收口 Writer / Amount 接缝（见 `AGENTS.md`）。
