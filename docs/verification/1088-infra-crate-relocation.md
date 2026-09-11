# 基础设施 crate 全量归位（issue #1088 / 父 spec #1086）

> 本票把数据库、错误、设置、文件工具、日志、事件、信号、闭集与壳层统一读写入口
> 全部移入 `ledger-infra`（`src-tauri/crates/infra`），根包以再导出形态保留原引用
> 路径（expand 形态）——域与壳层的既有调用点零改动即可编译。本文件记录挂载点
> 清单与数量、结构守门变更、负向验收证据与编译耗时/体积观测，作为后续拆分票的
> 观测点（上一份基线见 `1087-workspace-gate-baseline.md`）。

## 交付物

- `src-tauri/crates/infra/src/`：`db/`（连接、schema 守卫、加密、口令缓存、数据位置、
  账本注册表、事务原语、perf trace）、`error.rs`、`settings.rs`、`fs_util.rs`、
  `logger.rs`、`events.rs`、`signals.rs` + `signals/`、`closed_set.rs`、
  `write_entry.rs`、`read_entry.rs`（整文件/整目录迁移，`git` 记录为重命名）。
- `src-tauri/src/lib.rs`：原 `pub mod <模块>` 改为
  `pub use ledger_infra::{closed_set, db, error, events, fs_util, logger, read_entry,
  settings, signals, write_entry};`——`crate::db::…` / `crate::error::…` 等引用路径
  不变；`closed_set!` 宏经 `pub use closed_set` 分层再导出保持
  `crate::closed_set::closed_set` 调用形态。
- `src-tauri/crates/infra/Cargo.toml`：依赖面与迁移前根包同名依赖一致（rusqlite /
  rusqlite_migration / chrono / uuid / thiserror / tauri / serde / serde_json / sha2 /
  zip / tracing* / axum + macOS 钥匙串与生物认证目标依赖）；`[dev-dependencies]`
  经 dev-dependency 环引根包（测试工厂与共享器具）。
- `scripts/check-structure.ts`：基础设施模块清单从根 src 白名单改为 crate 清单
  `INFRA_MODULES`（基准 `crates/infra/src`）；认许边、全树扫描与摘要随之改基准。
- 根包迁移的测试引用形态：基础设施单测的域侧引用改为 `tauri_app_lib::…`
  （测试工厂 / 共享断言 / 交易与账户域薄皮），断言与场景文本未改。

## 基础设施→业务域挂载点清单（AC2：数量有记录、不新增）

**生产挂载点：0 条**（迁移前 1 条，本票消除）。

| # | 迁移前挂载点 | 形态 | 本票处置 |
| --- | --- | --- | --- |
| 1 | `db/mod.rs::after_commit` → `backup::{mark_dirty, shared_prefs, run_due_backup}`（ADR-0032 提交点置脏单点，#246） | 生产直调 | 注册点反转：基础设施只保留 `db::AfterCommitHook` 注册点与调用时机，备份域提供实现（`backup::after_commit_hook`），壳层启动（`lib.rs::run`）、测试建库单点（`test_support::open`）、BDD world 与基础设施单测各自接线一次 |

**测试专用边：4 条**（迁移前 5 条；减少的 1 条正是上表生产挂载点，不新增）。

| # | 文件（相对 `crates/infra/src`） | 目标域 | 动机 |
| --- | --- | --- | --- |
| 1 | `settings.rs` | `test_support` | 内联 `#[cfg(test)]` 经测试工厂建库（#758 收口） |
| 2 | `logger.rs` | `test_support` | 内联 `#[cfg(test)]` 经测试工厂建库（#758 收口） |
| 3 | `write_entry.rs` | `test_support` | 内联 `#[cfg(test)]` 建库 + 引用 `FIXED_NOW`（#758 收口） |
| 4 | `read_entry.rs` | `test_support` | 内联 `#[cfg(test)]` 建库 + 账户种子（#758 收口） |

清单的机器面 = `scripts/check-structure.ts::INFRA_DOMAIN_ALLOWED_EDGES`（逐条精确到
文件 + 目标域 + 成因），守门摘要打印条目数（`认许边 4 条`）；清单外的基础设施→域
引用一律红——新增即变红（负向夹具见下）。

## 接线入口核对（「所有入口都必须遵循」约束）

本票引入的约束：**每个建库/启动入口都必须注册提交点后置动作**（否则写路径静默
丢置脏与到期检查）。用 `rg` 枚举全部建库入口并逐一核对：

| 入口 | 形态 | 是否接线 |
| --- | --- | --- |
| `lib.rs::run` → `.setup` | 生产启动 | ✅ `backup::install_after_commit_hook()`（先于建库/写库） |
| `test_support::open` | 域单测 / API 集成 / 命令集成建库唯一入口 | ✅ 同函数内注册 |
| `tests/e2e/world.rs::LedgerWorld::new` | BDD world 自建库 | ✅ 同函数内注册 |
| `crates/infra/src/db/tests/common.rs::write_test_state` | 基础设施 crate 单测（dev-dependency 环，crate 实例分离） | ✅ 直接向本实例注册点注册备份域实现 |
| `src/bin/ledger-perf/generate.rs`、`commands/boot`、`sync_engine::checkpoint`、`tests/commands/*` 的建库 | 性能生成器 / 生产引导 / 同步检查点 / 命令集成 | 不消费脏标记断言（`rg 'dirty\|backup' tests/commands tests/api_server` 零命中）；同进程内世界级注册已生效，不另接线 |

生产启动接线的「删除即变红」由源码扫描守门兜底（启动路径不被任何测试直接执行）：
`scripts/check-background-services.ts::BOOT_WIRING` 要求 `lib.rs` 内存在
`install_after_commit_hook` 调用，缺失即红；夹具用例「删除启动接线 → 报红」锁死。

## 负向验收（删除即变红）

1. **根包测试工厂接线删除**：临时移除 `test_support::open` 内的
   `crate::backup::install_after_commit_hook()` →
   `cargo test -p tauri-app --lib writer_rows_do_not_mark_dirty_entry_does` 红：

   ```
   panicked at src/transaction/writer/tests/rows.rs:311: 写入口提交点应置脏
   test result: FAILED. 0 passed; 1 failed
   ```

2. **基础设施 crate 单测接线删除**：临时移除
   `crates/infra/src/db/tests/common.rs` 内的
   `crate::db::register_after_commit_hook(tauri_app_lib::backup::after_commit_hook)` →
   `cargo test -p ledger-infra --lib dirty_marker` 红：

   ```
   panicked at crates/infra/src/db/tests/dirty_marker.rs:20: 闭包成功后应置脏
   test result: FAILED. 2 passed; 2 failed
   ```

3. **生产挂载点复活**：夹具把迁移前的 `db/mod.rs` 直调备份域形态写回基础设施
   crate → `bun scripts/check-structure.ts` 红（`引用域目录 backup`，认许边不再含
   该条）——`src/__tests__/check-structure.test.ts` 用例「生产挂载点已反转」钉死。
4. **基础设施 crate 内引用壳层**：夹具写入 `db/helper.rs: use crate::commands::…` →
   红（`反向依赖`，定位 `db/helper.rs:1`）——crate 内清单基准随归位改到
   `crates/infra/src`，删除清单条目即报「白名单路径不存在」。
5. **启动接线删除**：`bun scripts/check-background-services.ts` 夹具删掉 `lib.rs`
   内的注册调用 → 红（`启动接线缺失`，含「删掉这行不会让任何断言变红」的动机说明）。

## 归位带来的形态调整（crate 边界强制，均不改变对外行为）

- **`impl IntoResponse for AppError` 随错误迁入基础设施**：孤儿规则（E0117）要求
  trait 实现与类型同 crate，`IntoResponse` 属 axum、`AppError` 属基础设施，实现
  只能住民 `crates/infra/src/error.rs`；壳层保留 `ErrorResponse` DTO 与路由接线。
- **`test_utils` 随基础设施归位**：`GatedEmitter` 实现 `events::SignalEmitter`，
  dev-dependency 环下跨 crate 消费会拿到第二份基础设施类型实例（类型身份不相容，
  实测报 `the trait bound GatedEmitter: events::SignalEmitter is not satisfied`），
  故与基础设施同 crate，根包再导出 `crate::test_utils` 供集成测试消费。
- **`WriteOp::from_ident` 去 `#[cfg(test)]` 门禁**：消费方 `signals_cross_check` 住根包
  的 `#[cfg(test)]` 模块，跨 crate 时本 crate 的 `cfg(test)` 不生效（实测「无此关联
  函数」），改为恒生成 + `#[doc(hidden)]`。
- **跨 crate 消费项的可见性 `pub(crate)` → `pub`**：`db::tx_scope::{ensure_transaction,
  hold_transaction}`、`db::encryption::{verify_source_passphrase,
  passphrase_incorrect_error, is_not_a_database}`、`db::passphrase_cache::{supported,
  current_mode, CacheLoad, store, load, delete}`、`events::post_emit_with`、
  `signals::WriteEvidence::merchant_created`——根包（域/壳层及壳层守门测试）继续按
  原路径消费，签名与语义不变。

## 编译耗时 / 体积观测

### CI（Build workflow 后端测试 job）

- **本票前**（#1087 基线，commit `de097771` / run `34618048780`）：依赖缓存恢复
  `~2009 MB`，编译阶段 `Finished test profile in 1m 23s`（83s，编 `tauri-app` +
  `ledger-infra` 两个 crate）。
- **本票后**：见 PR run（待 CI 完成后回填：缓存体积、`Finished` 阶段耗时、job wall
  clock）。

### 本机（控制变量：touch 源文件后重编，依赖全热，kache 命中）

| 项 | #1087 基线（首位成员） | #1088（全量归位） |
| --- | --- | --- |
| `cargo test --no-run` 重编耗时 | 50.94s | 50.50s |
| 基础设施 crate 单独重编（`touch crates/infra/src/lib.rs` + `cargo check -p ledger-infra --lib`） | 2.57s | 10.47s |

结论：全量归位后本地全量重编耗时在噪声内（50.5s vs 50.9s）；基础设施 crate 自身
体量从「1 个模块」涨到「约 10k 行、12 个模块」，单 crate 重编 10.5s——这是后续
按域拆分后各 crate 可独立重编的观测起点，不构成回退理由（编译收益不是保留判据，
spec #1086）。

## 未验证项

- 桌面三平台与 Android 打包（DMG / deb / AppImage / nsis / APK）未在本机跑通——
  发布构建仅 tag / 手动 dispatch 触发；已用 `cargo check --workspace --all-targets`
  与三层测试证明归位不破坏编译。
- CI 镜像/缓存调优仍归 #1110；crate 拆分 ADR 与分层指导同步归 #1111。
