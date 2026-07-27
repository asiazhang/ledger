# 使用 tracing 生态作为日志基础设施

项目当前零日志基础设施，需要加入日志文件记录以方便排查问题。选择 `tracing` + `tracing-subscriber` + `tracing-appender` 而非更轻量的 `log` + `env_logger`。

**选择 tracing 的理由**：
- 项目已使用 `tokio`（定时交易引擎、axum HTTP、reqwest 同步线程），`tracing` 的 async span 可以串联异步调用链，是 `log` 做不到的；
- 结构化字段（`account_id=5`、`command="create_transaction"`）比纯文本 grep 更利于定位问题；
- `tracing-appender` 内置按天滚动日志文件，不需要自己实现；
- Tauri 2 内部也在迁向 `tracing`，生态兼容性好。

**总体策略**：
- 默认级别 `INFO`（`RUST_LOG` 环境变量可覆盖），发布版始终同时写终端和文件，日志文件存放于 `app_log_dir()`；
- 按天滚动，保留 7 天，启动时清理过期文件，当前日志固定为 `ledger.log`，历史文件带日期后缀；
- IPC 调用通过 Tauri plugin（内联）在 `on_invoke` 阶段自动记录所有命令调用，零侵入现有 32+ 个 `#[tauri::command]`；
- 关键路径（定时交易引擎、股票同步、导入解析、数据库初始化）手动补 `DEBUG`/`ERROR` 级别日志；
- axum HTTP API 通过 `tower_http::trace` 中间件记录请求；
- `INFO` 级别只记录命令名和资源 ID，不记录金额和备注等用户数据；`DEBUG` 级别记录完整参数供开发排查。
