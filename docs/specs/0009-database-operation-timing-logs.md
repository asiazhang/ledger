# 数据库操作耗时日志（DB Timing Logs）

## Problem Statement

应用缺少数据库层的性能观测：无法回答"哪条 SQL 慢"、"导入一批花多久"、"启动迁移耗时多少"。数据库 IO 全部经过唯一全局连接，但任何一层都没有耗时记录——后续做性能改进时没有基线数据可参照，只能靠猜。

## Solution

给所有数据库 IO 操作加耗时日志：在连接层注册一个全局 hook（`trace_v2` PROFILE 事件），覆盖所有 SQL 语句执行（IPC 命令、HTTP 导入、定时引擎、后台索引刷新、启动迁移）。默认级别只落**慢查询**（单条 > 100ms，warn 级），全量明细走 DEBUG（`RUST_LOG=debug` 开启）。每条记录含耗时、SQL 原文与调用方归因（命令名/请求）。导入批次结束时打一条汇总（总耗时 + 条数 + 失败数）。性能分析时开 DEBUG 拿全量，日常日志只出现慢查询与批次汇总，可长期常驻。

## User Stories

1. As a 开发者, I want 每次 SQL 语句执行都被记录耗时, so that 我能确切知道每条语句实际花了多久
2. As a 开发者, I want 耗时超过阈值的语句以 warn 级别出现, so that 默认日志里就能发现异常慢查询，无需开调试
3. As a 开发者, I want 阈值可注入（默认 100ms）, so that 我能按数据规模调整什么算"慢"
4. As a 开发者, I want 全量逐条明细在 `RUST_LOG=debug` 下可见, so that 性能分析时能拿到完整语句数据
5. As a 开发者, I want 每条日志带 SQL 原文, so that 我能一眼定位是哪个查询慢
6. As a 开发者, I want SQL 日志关联到发起它的命令/请求, so that 我能区分"哪条 SQL 慢"和"哪个功能慢"
7. As a 开发者, I want 导入批次结束时有一条汇总（总耗时+条数+失败数）, so that 批量写入的整体耗时一目了然
8. As a 开发者, I want 启动迁移、种子数据、索引重建的耗时也被记录, so that 启动性能问题可观测
9. As a 开发者, I want 后台索引刷新线程、定时交易引擎的 SQL 也被覆盖, so that 隐藏慢点不遗漏
10. As a 用户, I want 日志不包含金额等隐私数据, so that 我的财务数据不会落到日志文件里
11. As a 开发者, I want 默认级别下日志量可控, so that 日志可长期常驻而不灌爆文件（一批导入默认只产生一条汇总）
12. As a 开发者, I want 观测对业务零侵入, so that 现有 50+ 数据库调用点无需改动
13. As a 开发者, I want 失败语句也被记录耗时, so that 慢+失败的组合（如锁等待超时）也可观测
14. As a 开发者, I want hook 挂载集中在连接工厂一处, so that 以后加字段、调阈值、换实现的成本都低

## Implementation Decisions

- **挂载点**：`db` 模块新增耗时日志 helper，由 `open_connection` / `open_in_memory` 两个连接工厂共享调用；注册 `Connection::trace_v2` 的 PROFILE 事件，全局覆盖所有执行上下文。不选已废弃的 `profile()`（`&mut self` 注册时机受限）；不选封装层计时（50+ 调用点直接 `conn.execute/query_row/prepare`，封装无法全量覆盖）。
- **接口形状**：`install_perf_trace(conn, threshold: Duration)` 注册 hook 并发射事件；默认 100ms 由薄包装提供。阈值可注入使测试无需造慢语句。
- **分类规则**：纯函数 `timing_level(threshold, duration)`——耗时 > 阈值 → `warn`，否则 `debug`。这是唯一决策核心，独立成函数以便边界测试。
- **记录字段**：耗时 + 带占位符的 SQL 原文（默认级别安全）；展开 SQL（内联参数值）仅 DEBUG 级，延续 ADR-0006 隐私约定（默认级别不落金额/备注）。
- **归因**：IPC 侧在 `logged_invoke_handler` 用 `info_span!(command, id_hint)` 包裹命令执行，hook 事件自动继承当前 span（同步命令与 wrapper 同线程，实现时冒烟验证；若异步命令丢归因，对热点函数手包 span 兜底）；HTTP 侧由既有 `tower_http::trace` 请求 span 归因。
- **批次汇总**：`create_transactions_internal` 在 COMMIT 后打一条 `info!`——总耗时（手动 `Instant`）+ 交易条数 + 失败条数，错误路径同样记录。
- **明确不记录**：行数（rusqlite 0.40 的 `StatementStatus` 无 RowCount/RowsWritten 变体且 `StmtRef` 指针私有）、成功/失败结果码（PROFILE 事件不携带 rc；失败观测走现有 `AppError` 错误传播路径）。
- **无 schema 变更、无前端改动、无 API 契约变更。**

## Testing Decisions

- 好的测试只验证外部行为——"语句执行后以正确级别发射事件"，不断言日志行格式、不测 span 内部实现。
- **测试 seam（唯一）**：`db` 模块的耗时日志 helper，单元测试扩展在 `db/tests.rs` 既有风格上：
  - 纯函数 `timing_level` 边界测试：0、恰好阈值、略低于/略高于阈值；
  - 接线回归：`open_in_memory()` + 捕获 subscriber（`tracing::subscriber::set_default`，线程本地，测试线程执行 SQL 时 hook 同线程发射可捕获），执行 `SELECT 1` 断言捕获到事件且含 SQL 文本；传 `threshold=0` 断言命中 warn 分支。
- **不写 BDD**：cucumber 测试领域行为，日志是基础设施，不属于特征文件语义。
- **测试先例**：`src-tauri/src/db/tests.rs`、`commands/*/tests` 均使用 `open_in_memory()` + `init_db()` 建内存库——沿用同一模式。
- **冒烟验证（不自动化）**：批次汇总与 span 归因——`RUST_LOG=debug` 跑 `tauri dev`，人工检查 `ledger.log` 中 SQL 行带 `command=` 归因、导入后出现批次汇总行。

## Out of Scope

- **行数记录**：rusqlite 0.40 hook 层不可得，随 rusqlite 升级后另行补充。
- **成功/失败标记**：PROFILE 事件无结果码，失败观测已有错误传播路径兜底。
- **平均/最大单条耗时聚合**：需在无状态 hook 回调内引入共享聚合状态，收益低，砍掉；逐条明细在 DEBUG 行中。
- **`query_log` 表 / 应用内查询**：本次决策只进日志文件（ledger.log 按天滚动），不建表。
- **导入路径的已知性能嫌疑修复**：单条 INSERT 循环 + 事务内每笔 3 次 SELECT——本次只观测不修，留给观测数据驱动的后续优化。
- **前端改动**：无。

## Further Notes

- 配套文档已就绪：ADR-0009（已接受）、CONTEXT.md 新增"耗时日志 / 慢查询"词条。
- 慢查询 warn 在启动迁移、首次全量重建索引时可能成群出现——是预期信号而非故障。
- 阈值 100ms 是初始值，随观测数据调整，不构成对外契约。
- 归因的"同步命令与 wrapper 同线程"假设需实现时冒烟验证，失败则按 ADR 决策 4 回退到热点函数手包 span。
