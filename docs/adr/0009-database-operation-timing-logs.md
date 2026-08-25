# ADR-0009: 数据库操作耗时日志采用连接级 trace_v2 hook

- 状态：已接受
- 日期：2026-08-23
- 作者：Ledger 项目

## 背景

应用缺少数据库层的性能观测：无法回答"哪条 SQL 慢"、"导入一批花多久"、"启动迁移耗时多少"。为后续性能改进建立基线，需要给所有数据库 IO 操作加耗时日志。现实约束：

- 全局单连接 `Arc<Mutex<Connection>>`（`db/mod.rs`），50+ 调用点直接 `conn.execute/query_row/prepare`，`db/query.rs` 封装只覆盖其中一小部分——封装层计时无法全量覆盖；
- 已有 `tracing` 基础设施（ADR-0006）：默认 INFO、按天滚动文件、INFO 级不落金额/备注等业务值；
- 导入批量路径（现为 `batch::TransactionBatch::run`）是单条 INSERT 循环，每笔交易约 4~5 条 SQL——逐条全量日志在默认级别下量级不可接受。

## 决策

1. **挂载点**：在 `db::open_connection` / `db::open_in_memory` 经共享 helper 注册 `Connection::trace_v2`（PROFILE 事件），全局覆盖所有执行上下文（IPC 命令、HTTP 导入、定时引擎、搜索索引刷新、启动迁移）。不选已废弃的 `profile()`（`&mut self` 注册时机受限）；不选封装层计时（无法覆盖 50+ 直调点）。
2. **记录内容**：耗时 + 带占位符的 SQL 原文 + 调用方归因（span）。**不记行数**：rusqlite 0.40 的 `StatementStatus` 无 RowCount/RowsWritten 变体且 `StmtRef` 指针私有，无法从 hook 取得；如后续需要，随 rusqlite 升级补上。**不记成功/失败**：PROFILE 事件不携带结果码，失败语句仍会被记录（仅耗时 + SQL），执行失败由现有错误传播（命令返回 `AppError`）观测。展开 SQL（内联参数值）仅 DEBUG 级，延续 ADR-0006 隐私约定（默认级别不落金额/备注）。
3. **级别策略**：全量 `tracing::debug!`；单条耗时 > 100ms 升 `tracing::warn!`。默认 `RUST_LOG=info` 时只落慢查询，`RUST_LOG=debug` 才有全量明细。阈值后续按观测数据调整。
4. **归因**：IPC 侧在 `logged_invoke_handler` 用 `tracing::info_span!`（command + id_hint）包裹命令执行，hook 事件自动继承 span（同步命令与 wrapper 同线程，实现时冒烟验证；若异步命令丢归因，对热点函数手包 span 兜底）；HTTP 侧由既有 `tower_http::trace` 请求 span 归因。
5. **批次汇总**：批量导入路径（现为 `batch::TransactionBatch::run`）在 COMMIT 后打一条 `info!` 汇总——总耗时（手动 `Instant`）+ 交易条数 + 失败条数；逐条明细见 DEBUG 行。不在 hook 内做线程级聚合（回调是无状态 `fn` 指针，聚合需共享状态，收益低）。

## 理由

- hook 方案以一处注册换全量覆盖，零业务代码改动，改造成本最低；
- 阈值 + 双级别让默认日志量保持在一批导入仅一条汇总 + 少量慢查询行，可长期常驻；性能分析时开 DEBUG 拿全量；
- 观测单位分两层（SQL 语句级 + 命令/批次级），能分别回答"哪条 SQL 慢"和"哪个功能慢"。

## 后果

- 启动迁移、后台索引刷新、定时引擎的 SQL 也会被记录（属预期，正是隐藏慢点）；
- 慢查询 warn 在启动/迁移、首次全量重建索引时可能成群出现，是预期信号而非故障；
- 未来若需行数、per-语句聚合或应用内查询，升级 rusqlite 或加 query_log 表即可，不改变挂载点。
