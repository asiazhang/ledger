# Tauri IPC API 文档

本目录**不重复罗列命令清单**：RPC 命令名、参数、返回类型等可枚举信息以当前代码为唯一事实来源，直接读代码（见下方「从代码生成」）。本文件只回答代码里读不出来的问题：职责边界、新增命令的接缝与契约。

## 从代码生成（事实来源）

| 信息 | 读哪里 |
|------|--------|
| 全部已注册命令清单 | `src-tauri/src/lib.rs` 的 `generate_handler!` 宏（注册处即权威，含 `open_log_dir` 插件命令） |
| 前端调用封装（参数/返回类型） | `src/api/index.ts` 的 `api` 对象 + `src/types/index.ts`（`formatAmount` 定义在 `src/utils/money.ts`，经 `@/types` 转出） |
| 各命令实现与行号 | `src-tauri/src/commands/<领域>/`（目录化组织：`mod.rs` 命令外壳 + `core.rs` 核心逻辑；命令经 `commands/mod.rs` 的 `pub use` 重导出） |
| IPC ↔ 前端契约类型 | `src-tauri/src/models/`（按领域拆分，`mod.rs` 统一重导出；serde 结构即序列化契约） |
| 错误序列化格式 | `src-tauri/src/error.rs`（`AppError`，`{kind, message}`） |
| 本地 HTTP API（AI 导入） | `src-tauri/src/api_server.rs` + 运行期 `GET /api/v1/openapi.json`（utoipa 生成式返回，机器可读契约） |

**新增命令流程**（与 `AGENTS.md`「Tauri IPC 数据流」一致）：

1. `src-tauri/src/commands/<领域>/mod.rs` 加 `#[tauri::command]` 函数；
2. `src-tauri/src/lib.rs` 的 `generate_handler!` 中注册；
3. `src/api/index.ts` 的 `api` 对象加对应方法；
4. 必要时在 `src/types/` 加 TS 类型（与 `src-tauri/src/models/` 的 serde 结构对应，注意 `#[serde(rename = "type")]` 这类字段名映射）。

**不需要**为命令写文档页：命令本身、参数、返回类型均可在代码与上述生成点读到，本目录不再维护逐命令条目，避免与代码漂移。

## 职责边界（代码读不出来的部分）

### 命令层 vs 领域模块

- 命令层是**薄壳**：锁 `DbState` 后调核心函数（`*_rows` / `*_internal`），核心逻辑吃 `&Connection` 可独立单测。
- 写入口径不在命令层：交易写入统一经 `transaction::writer` 接缝（`normalize` / `insert_row` / `update_row`）落库；金额折算统一经 `transaction::amount` 接缝。命令层只做编排（事务边界、去重、事件发射），**不重复列映射与口径表达式**。
- buy/sell 的持仓副作用（`security_lots` / `security_lot_sales` / `security_transactions`）留在投资层（`commands/investment/trade.rs`）编排，交易行字段仍经 Writer 落库。

### 同步信号（跨端契约）

- 参考写入（账户/分类的 create/delete/update/reorder）成功后，后端发通用、粗粒度、无 payload 的 `ledger:changed` 事件；前端 `useReferenceStore` 订阅并自动重拉三张参考表（stale-while-revalidate）。
- **交易类写入不 emit**（不改参考表）；判定收口在 `src-tauri/src/events.rs` 的 `REFERENCE_WRITE_COMMANDS` 清单，新增参考写入命令须同步扩充（由单测锁定）。
- HTTP（AI 导入）写入与 IPC 写入走同一套失效信号：HTTP 端点结构上即参考写入，直接 emit（`AppHandle` 为 `Option`，测试传 `None` 跳过发射）。
- 行情同步进度经 `sync-instruments:progress` 事件推送（`done=true` 携带新增/更新计数或错误），前端 `useInstrumentSync` 监听。

### 错误与审计约定

- `AppError` 序列化为 `{kind, message}`；`NotFound` 映射 HTTP 404，`Invalid` 映射 400。
- 日志：IPC 命令经 `logged_invoke_handler` 自动记录（零侵入 45+ 命令）；数据库耗时经 `db/perf_trace.rs` 的 `trace_v2` hook 全量覆盖（慢查询 >100ms 升 warn）；导入批次结束打一条汇总（总耗时+条数+失败数）。默认级别不落金额/备注等业务值（展开 SQL 仅 DEBUG）。
