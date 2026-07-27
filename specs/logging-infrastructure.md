# 日志基础设施

- [GitHub Issue #15](https://github.com/asiazhang/ledger/issues/15)

## Problem Statement

Ledger 应用目前没有任何日志基础设施。所有 Rust 后端的命令调用、数据库操作、定时交易执行、股票同步、导入解析、HTTP API 请求等行为在服务端完全不可见。当用户遇到问题时（命令失败、数据异常、同步中断），开发者只能依赖用户在前端看到的错误消息，无法回溯后端发生了什么。排查问题只能靠用户复现。

## Solution

引入基于 tracing 生态的日志基础设施，将后端的关键行为（IPC 调用、错误、定时交易、同步、HTTP 请求等）写入日志文件，并按天滚动保留 7 天。日志同时输出到终端（始终）和文件，默认级别为 INFO。IPC 调用通过 Tauri plugin 自动拦截记录，现有 32+ 个 `#[tauri::command]` 无需任何修改。

## User Stories

1. 作为用户，当我遇到程序异常时，我可以找到"打开日志目录"入口，快速定位日志文件发送给开发者，以便开发者快速定位问题根因。

2. 作为开发者，当用户反馈某条交易创建失败时，我可以在日志文件中搜索对应的 `create_transaction` 调用及其参数和错误信息，不需要用户复现操作。

3. 作为开发者，当定时交易引擎某期执行失败时，我可以在日志中看到是哪个计划、哪一期、什么原因失败，不需要手动检查数据库状态。

4. 作为开发者，当股票同步中断时，我可以在日志中看到是哪个 HTTP 请求失败、返回了什么状态码或错误，不需要用户重跑同步并观察。

5. 作为开发者，当数据导入解析异常时，我可以在日志中看到是哪一行 CSV/Excel 数据解析失败、失败原因是什么，不需要用户重新上传文件并逐个排查。

6. 作为开发者，当应用启动时数据库初始化失败，我可以从日志文件看到失败原因（迁移错误、磁盘空间不足等），而不只是用户看到的一个弹窗。

7. 作为开发者，我可以通过设置 `RUST_LOG=debug` 环境变量获得更详细的日志（包括完整参数），用于深度排查疑难问题。

8. 作为用户，我不需要关心日志文件的管理——日志按天自动切分，超过 7 天的自动清理，不会占满磁盘空间。

9. 作为用户，当前日志文件始终叫 `ledger.log`，我不需要猜测文件名就能找到最新日志。

10. 作为用户，我的敏感数据（交易金额、备注、账户名称）不会在默认日志级别（INFO）中暴露，保护个人隐私。

11. 作为 AI 编程助手的用户，HTTP API（127.0.0.1:9527）的请求也会被记录，我可以排查 AI 调用 Ledger API 时的问题。

## Implementation Decisions

### 日志框架与依赖

- 使用 `tracing` + `tracing-subscriber` + `tracing-appender`（而非 `log` + `env_logger`）。理由是项目已用 `tokio`，tracing 的 async span 能串联异步调用链，且 tracing-appender 内置按天滚动。
- axum HTTP API 通过 `tower-http` 的 `trace` 中间件记录请求，一行代码零侵入 handler。
- 新增 Rust crate 依赖：
  - `tracing`（门面）
  - `tracing-subscriber`（layer 组合、环境变量过滤）
  - `tracing-appender`（文件滚动写入）
  - `tower-http` feat `trace`（axum 中间件）

### 输出目标与级别

- 始终同时输出到终端（stderr）和文件。终端的 layer 用 `tracing_subscriber::fmt` 的 `with_ansi`（有色彩），文件 layer 用 `tracing_appender::non_blocking`（异步写入，避免阻塞主线程）。
- 默认级别为 `INFO`，通过 `RUST_LOG` 环境变量可覆盖（如 `RUST_LOG=debug`）。
- MVP 阶段不提供设置页 UI 或配置文件控制日志级别。

### 日志文件策略

- 存放于 `app_log_dir()`（Tauri 标准日志路径，macOS 为 `~/Library/Logs/<bundle-id>/`）。
- 当前日志固定文件名 `ledger.log`。
- 每天零点（或应用启动时检测日期变更）将当前日志重命名为 `ledger.YYYY-MM-DD.log`，新建 `ledger.log` 继续写入。
- 每次应用启动时扫描日志目录，删除超过 7 天的 `ledger.YYYY-MM-DD.log` 文件。

### Tauri IPC 拦截

- 以 Tauri plugin 方式（内联，不拆独立 crate）在 `on_invoke` 阶段自动记录所有 32+ 个 `#[tauri::command]` 调用。
- `INFO` 级别只记录命令名和关键资源 ID（如 `create_transaction(account_id=5, kind=expense)`），不记录金额、备注、账户名等用户数据。
- `DEBUG` 级别记录完整参数值供开发排查。
- 现有命令代码零修改，完全由 plugin 层拦截。

### 关键路径手动埋点

以下高风险区域在代码内手动补 `DEBUG`/`ERROR` 级别日志：

- 定时交易引擎：期次开始执行（INFO）、执行失败（ERROR）、CAS 冲突（WARN）
- 股票同步：HTTP 请求发出（DEBUG）、请求失败（ERROR）、解析失败（WARN）
- 导入解析：解析失败行（WARN，含行号和原因）
- 数据库初始化：开始迁移（INFO）、迁移失败（ERROR）
- api_server.rs：保留 `tower_http::trace` 记录每个 HTTP 请求

### 代码组织

- 新建 `logger.rs`：subscriber 初始化、日志目录创建、旧日志清理逻辑。在 `main.rs` 最早处调用 `logger::init(app_handle)`。
- 新建 `log_plugin.rs`：Tauri plugin 实现（`on_invoke` / `on_invoke_return` 回调）。
- 在 `lib.rs` 的 `run()` 中注册 plugin。
- 在 `api_server.rs` 中为 axum Router 添加 `tower_http::trace` 中间件。

### 前端入口

- 在设置页添加"打开日志目录"入口，使用已有的 `tauri-plugin-opener` 调用 `opener::open_path(app_log_dir())`。
- MVP 不提供"导出日志"功能（用户可直接压缩日志目录发送）。

### 隐私约定

| 日志级别 | 命令参数 | 账户名 | 金额 | 备注/交易对手 | 数据库 SQL |
|----------|----------|--------|------|---------------|------------|
| ERROR | 无参数 | 无 | 无 | 无 | 仅错误类型 |
| WARN | 无参数 | 无 | 无 | 无 | 仅错误类型 |
| INFO | 命令名 + 资源 ID | 无 | 无 | 无 | — |
| DEBUG | 完整参数 | 有 | 有 | 有 | 有 |

## Testing Decisions

- **好测试的标准**：只测试 logger 模块产出的外部行为（路径正确、文件存在、内容包含预期行），不测试 tracing-subscriber 内部行为、tracing-appender 滚动逻辑、tower-http 中间件内部——这些都是框架的职责。
- **测试模块**：
  - `logger.rs` 的目录创建和清理逻辑：单元测试，mock `app_log_dir()` 路径到临时目录，验证清理最近 7 天逻辑。
  - Tauri plugin 的拦截行为：不加独立测试，通过现有 6 个 BDD cucumber scenario（`cargo test --test e2e`）隐式验证——加 plugin 后所有 command 行为不变即为通过。
  - 日志文件产出：集成测试——启动 subscriber → 触发一个 command → 调用 `WorkerGuard::flush_and_drop` → 断言 `ledger.log` 存在且含预期行。
- **不新增前端测试**（设置页"打开日志目录"是 Naive UI 的简单点击行为，不涉及业务逻辑）。

## Out of Scope

- 前端日志（浏览器 console 级别）——开发时用浏览器 devtools 即可。
- 设置页日志级别 UI 或配置文件——MVP 仅 `RUST_LOG` 环境变量。
- 导出日志功能——用户自行压缩日志目录发送。
- 分布式追踪 / APM / 远程日志上报。
- 日志压缩或归档（7 天保留策略下日志量极小，不需要压缩）。
- 日志的 CI/CD 自动化（日志不影响构建流程）。

## Further Notes

- ADR 0006 记录了选择 `tracing` 生态的详细理由。
- 本次改动完全零侵入现有 32+ 个 `#[tauri::command]`，所有 IPC 日志由 plugin 层自动完成。
- 新增代码量预计约 200-300 行 Rust（logger.rs ~100 行，log_plugin.rs ~80 行，Cargo.toml 依赖 ~5 行，lib.rs/main.rs/api_server.rs 改动各 ~5 行）。
