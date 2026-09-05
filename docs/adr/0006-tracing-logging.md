# 使用 tracing 生态作为日志基础设施

项目当前零日志基础设施，需要加入日志文件记录以方便排查问题。选择 `tracing` + `tracing-subscriber` + `tracing-appender` 而非更轻量的 `log` + `env_logger`。

**选择 tracing 的理由**：
- 项目已使用 `tokio`（定时交易引擎、axum HTTP、reqwest 同步线程），`tracing` 的 async span 可以串联异步调用链，是 `log` 做不到的；
- 结构化字段（`account_id=5`、`command="create_transaction"`）比纯文本 grep 更利于定位问题；
- `tracing-appender` 内置按天滚动日志文件，不需要自己实现；
- Tauri 2 内部也在迁向 `tracing`，生态兼容性好。

**总体策略**：
- 默认档位 `INFO`（`RUST_LOG` 环境变量可覆盖），发布版始终同时写终端和文件，日志文件存放于 `app_log_dir()`；
- 按天滚动，保留 7 天，启动时清理过期文件，当前日志固定为 `ledger.log`，历史文件带日期后缀；
- IPC 调用经 `invoke_handler` 包装层（`logged_invoke_handler`）自动记录所有命令调用，零侵入各 `#[tauri::command]`；
- 关键路径（定时交易引擎、股票同步、导入解析、数据库初始化）手动补 `DEBUG`/`ERROR` 级别日志；
- axum HTTP API 通过 `tower_http::trace` 中间件记录请求；
- `INFO` 档位只记录命令名和资源 ID，不记录金额和备注等用户数据；`DEBUG` 档位记录完整参数供开发排查。

**日志等级可调的四段取舍（spec #608 确认，文字为后续实现的契约）**：

- **启动顺序约束**：`logger::init` 在 Tauri `setup` 内运行、早于数据库打开（现役代码顺序已如此）；持久化档位存于库内 `app_settings`，logger 初始化阶段读不到库。订阅器只能先以「`RUST_LOG` 环境变量或默认 info」建立，数据库就绪并读出持久化档位后再 reload 接管——启动初段（含数据库初始化自身的日志）存在按默认档记录的短暂窗口，属可接受取舍，不为此推迟 logger 初始化。
- **运行期 reload 机制**：现役订阅器把 `EnvFilter` 直接注册进 registry，注册后无法替换。改为包一层 `tracing_subscriber::reload::Layer` 并持有 `reload::Handle`，设置写入命令经 `Handle` 替换滤镜，改动立即生效、无需重启；文件与终端两条输出共用同一滤镜、一起变化。需启用 `tracing-subscriber` 的 `reload` feature。
- **随备份迁移**：日志等级由后端消费，按 AppSettings 判定规则（ADR-0017）落 `app_settings`，随 Backup/Restore 迁移——定位是排查工具性配置而非设备个性，换机恢复备份后排查配置仍在；恢复旧版本备份缺 key / 缺表时回默认 info，行为免费正确。与界面语言、主题这类不迁移的轻量设备偏好（ADR-0017）相反。
- **环境变量优先级**：显式 `RUST_LOG` > 持久化档位 > 默认 info。`RUST_LOG` 是开发者逃生舱，只在启动时消费一次、本次启动内压过持久化档位；不做主动探测（避免解析复杂 directives），由「关于」页静态提示说明该覆盖关系，界面展示的持久化档位与实际生效档位可能因此不一致。

**去插件化补记（issue #283）**：早期实现曾把「打开日志目录」承载于自研内联 Tauri 插件（`log_plugin`），试图在插件的 `on_invoke` 阶段拦截记录 IPC——该职责实际一直由上文的 `invoke_handler` 包装层承担，插件只剩单个命令，还得在构建脚本注册 `InlinedPlugin` 与 capability 授权两处维护 ACL（且从未配置，导致该按钮自 v0.3.0 起始终被 ACL 拒绝），插件命令更绕过统一包装形成 IPC 日志盲区。已删除该插件，「打开日志目录」改为普通命令（`commands::logs::open_log_dir`）注册进主命令表。
