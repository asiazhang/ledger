# ADR-0060: 后端易 panic 构造门禁——clippy 六件套 deny 与存量豁免纪律

- 状态：已接受
- 日期：2026-09-03
- 作者：Ledger 项目
- 关联：spec #430（父）/ #432（门禁落地）；后续结构性消除 #433（定时计划域两处）、#434（投资域同步两处）；形状乙 async 化与宏豁免扩容 ADR-0069（spec #498，#504 豁免清单整体复核）；错误码化契约 ADR-0050；自动执行 Failure Policy ADR-0024

## 背景

后端生产路径散布着 unwrap/expect/panic 等易 panic 构造：新增代码随手 `.unwrap()` 编译期无感，只有运行到才炸；存量 panic 点里既有「启动期失败即无法运行」的合理 fail loud，也有业务路径上可防御化的违规。缺乏机器门禁时，「哪里允许 panic」只靠评审记忆。本 ADR 记录六件套 deny 门禁的动机、测试面豁免机制与存量豁免安置纪律，使「易 panic 构造」从评审约定升级为编译期约束。spec 见 #430。

## 决策

### 1. 六件套 deny 门禁

包清单 `[lints.clippy]` 将六个 restriction lints 全部 deny：`unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented` / `unreachable`。`check.sh` 的 clippy 环节（`--all-targets --all-features -D warnings`）自动拦截一切新增易 panic 代码，脚本零改动。构建脚本 `build.rs` 的 fail-loud 守门（ADR-0047 扫描器失灵即拒绝构建）刻意保留 panic!/expect，文件级豁免并注明「构建期代码不进入生产运行时」。

### 2. 测试面整体豁免（cfg(test)）

测试代码（单元、集成、BDD）是易 panic 构造的合法主场，整体豁免而非逐点安置：crate 根（`lib.rs`）与各测试 crate 头部（`tests/e2e.rs`、`tests/api_server/main.rs`）加 `#![cfg_attr(test, allow(六项))]`。`cfg(test)` 只在测试目标编译时为真，生产构建零放宽。集成测试以非 `cfg(test)` 构建链接 lib，故 lib 内的测试专用夹具模块（`src/test_utils.rs`）无法被 crate 根豁免覆盖，按 C 类文件级放行（见下）。

### 3. 存量逐点豁免纪律（B/C/A 三类）

存量生产 panic 点逐点 allow + 理由注释（`#[allow(...)]` 贴最近语句，注释引用本 ADR 与理由）；除 C 类夹具与宏生成代码两类文件级豁免（各自无法更窄，见下）外，禁止函数级/模块级宽放：

- **B 类（7 处，启动期「失败即无法运行」，长期保留）**：应用启动装配链（`lib.rs` Tauri Builder 构建失败）、HTTP API 壳的 Tokio 运行时创建 / 端口绑定 / 服务器异常退出（`api_server/router.rs`）、日志系统首次初始化（`logger.rs`）。这些点失败即进程不可用，fail loud 是正确行为。
- **C 类（1 个模块，文件级 allow + 「仅测试用」声明）**：集成测试专用夹具模块 `src/test_utils.rs`——被集成测试以非 test 构建链接而无法 `cfg(test)` 门控，文件级放行六件套并声明生产路径不得消费。
- **A 类（原 4 处，已经 #433 / #434 结构性消除并摘除豁免，清点为零）**：定时计划域 2 处（`scheduled_transactions/engine.rs`：Occurrence 扩展 Option unwrap、穷尽 match 防御臂 unreachable）、投资域同步 2 处（`sync/fund_nav.rs` 现价选取 expect、`sync/incremental.rs` 北京时间计算 expect），两域防御面改写为 let-else 防线 / 码化错误后逐点豁免随之摘除；新增强制走码化错误路径，不再产生 A 类存量。
- **宏生成代码（21 个文件，升 tauri 后移除）**：tauri 宏为 async 命令生成 `let _check: _ = unreachable!()`（tauri-macros wrapper.rs），宏不透传逐点 allow，只能在命令壳文件级放行——存量 `commands/investment.rs`、`commands/sync.rs`；#501 / #502 / #503 三批 async 化（形状乙，决策见 ADR-0069）新增 `commands/accounts.rs`、`commands/categories.rs`、`commands/currencies.rs`、`commands/merchants.rs`、`commands/search.rs`、`commands/dashboard.rs`、`commands/transactions.rs`、`commands/scheduled.rs`、`commands/reports.rs`、`commands/budget.rs`、`commands/item.rs`、`commands/policy.rs`、`commands/physical_asset.rs`、`commands/financial_freedom.rs`、`commands/backup.rs`、`commands/data_location.rs`；`commands/encryption.rs`（#570）与 `commands/boot.rs`（#601）后续 async 化同批豁免；`commands/insurer.rs`（#712，保司命令面）同款豁免；属上游缺陷，tauri 修复后移除。#504 收口复核：豁免注释各文件形态一致（均引本 ADR 与上游缺陷归属），无旧形态残留。

### 4. 后续收紧方向

门禁生效后按存量治理节奏收紧：A 类四处经 #433/#434 结构性消除并摘除豁免；宏生成豁免随 tauri 升级移除；B 类七处为长期豁免，不设消除期限（启动期 fail loud 是终态）。新增强制走错误化路径（错误码契约见 ADR-0050），clippy 门禁保证违规在编译期即红。

## 后果

- **新增易 panic 代码编译期即红**：clippy 环节零脚本改动自动拦截，评审不再依赖记忆。
- **测试写法零负担**：测试面整体豁免，断言辅助照常用 unwrap；生产构建零放宽由 `cfg(test)` 语义保证。
- **豁免可清点**：全库 `rg` 豁免点数量与位置即豁免清单（B 7 + C 1 + 宏 21 文件 + build.rs 1；A 类 4 处已消除，清点为零），漂移在 review 可见。
- **不改变运行时行为**：本门禁只加 lint 与豁免注释，零语义变化；A 类四处行为改写由后续 ticket 承载并有既有测试兜底。
