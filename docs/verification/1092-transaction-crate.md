# 核心交易域 crate（issue #1092 / 父 spec #1086）

> 本票把核心交易域（交易写入协议/金额口径/读取/搜索）自根包域目录拆为 workspace
> 成员 `ledger-transaction`（`src-tauri/crates/transaction`，spec #1086 P2），是全部
> 业务域可依赖的最底层域。拆分前置：对投资/商户/币种/物品/保单/账户六向的残留
> 边按挂载点反转收敛（前置提交，ADR-0112 决策 5 挂载点⑤）。本文件记录挂载点
> 清单与接线入口核对、结构守门变更、负向验收证据与编译耗时观测（上一份观测点
> 见 `1088-infra-crate-relocation.md` 与 #1091/#1169 的 PR 描述）。

## 交付物

- `src-tauri/crates/transaction/`：新成员 crate（整目录 `git mv`，git 记录为重命名，
  历史可跟随）；`ledger-infra` + `ledger-sync-protocol` + rusqlite/serde/sha2/
  tracing/utoipa/pinyin；`[dev-dependencies]` 经 dev-dependency 环引根包
  （tauri-app）与 test-utils 器具；`[lints] workspace = true` 继承六件套。
- 前置提交（41dcd5f5，接缝反转，行为零变化）：
  - `transaction::investment_seam`：投资 kind 计划契约（`InvestmentPlan`，本域
    自有类型）与四写路径挂载点（Local/Replay 装配、修改回退、删除释放）+ 两读
    投影（来源列④标的反查、转换两腿）注册点；实现面 `investment::transaction_seam`
    （命令字段摘取与重放防御臂随接缝迁入，错误码与文案逐字保留）。
  - `transaction::merchant_seam`：商户名先查/后建钩子组（一槽原子注册）。
  - `transaction::base_currency_seam`：本位币基准读取注册点。
  - `transaction::funding`：出资账户视图注册点（`FundingAccountClass` 类别投影 +
    `FundingAccountView`）——原 `transaction/funding.rs → accounts` 的 AccountType
    类型只读认许边消亡（DOMAIN_PAIR_ALLOWED_EDGES 清零）。
  - `transaction::read`：来源列①保单直挂/③物品反查注册点（与 #1090 计划反查同族）。
- 后置提交（crate 迁移）：根包 `pub use ledger_transaction as transaction;` 再导出
  （expand 形态，约 100+ 处调用点零改动）；`replay_command` /
  `register_plan_source_resolver` 等跨 crate 消费项 `pub(crate)`→`pub`；search
  白名单内部项（`Stage1Filter` / `TermLowered` / `Stage1Query` / `SearchDicts` /
  `build_stage1_query` / `load_search_dicts`）以 `#[doc(hidden)] pub` 出 crate
  供测试实例纪律下的白盒测试（#1088 `WriteOp::from_ident` 先例）。

## 接缝挂载点与接线入口核对（「所有入口都必须遵循」约束）

本票新增注册点 11 个（投资 6、商户 1 组、本位币 1、出资账户视图 1、保单/物品
反查 2），未注册即码化错误（失败可见），不静默丢副作用——故无需 BOOT_WIRING
扫描兜底（#1088 的扫描针对「静默丢失」形态；#1091 的追补触发按同一理由入扫描，
其缺席仅记 error 日志属用户不可见失败，与本票的硬失败不同）。

用 `rg` 枚举全部可触发交易写/读路径的建库入口并逐一核对接线：

| 入口 | 形态 | 是否接线 |
| --- | --- | --- |
| `lib.rs::start_background_services`（壳层启动） | 生产启动 | ✅ 六向 `install_*` 各一次，先于任何建库/写库 |
| `test_support::open` | 域单测 / API 集成 / 命令集成建库唯一入口 | ✅ 同函数内六向接线 |
| `tests/e2e/world.rs::LedgerWorld::new` | BDD world 自建库 | ✅ 同函数内六向接线 |
| `src/bin/ledger-perf/main.rs` | 性能基准（bench-import 走写入协议） | ✅ 同函数内六向接线 |
| `crates/infra/src/boot/tests/encryption.rs::seed_transactions` | infra crate 单测（dev 依赖环，实例分离，显式产品建缝） | ✅ 落库前六向接线 |
| `tests/commands/sync_channel.rs::fresh_app` / `sync_checkpoint.rs::reattach_app` | 命令集成（产品建缝，不经测试工厂） | ✅ 同形六向接线 |

## 测试实例纪律（dev 依赖环双实例）

`cargo test -p ledger-transaction` 的依赖图内存在本 crate 两份实例（被测本实例 +
根包图内实例，静态与类型身份分离；ledger-backup/ledger-infra 先例同款，cargo
tree 单包、编译期两份）。接缝注册静态由测试工厂接在根包图实例上，且带类型签名
的钩子无法跨实例注册（名义类型不等价），故：

- 走接缝的行为路径测试一律经 `tauri_app_lib::transaction::…` 驱动（同一份源码，
  断言与场景文本零改动，仅引用形态变化）；
- 纯函数与守门用例（不读注册静态）直接驱动本实例（amount 矩阵、search_text、
  各接缝的 missing-error 构造与 dispatch 透传）；
- 依据留痕于 crate 根 lib.rs「测试实例纪律」段与 Cargo.toml dev-dependencies 注释。

行为等价主证据不受此影响：根包图内只有一个交易域实例，三层测试（域单测经测试
工厂、API/命令集成、e2e）全部走该实例。

## 负向验收（删除即变红 / 编译期双证）

1. **生产依赖方向（AC3 结构守门侧）**：夹具把 `tauri-app = { path = "../.." }`
   写进 `crates/transaction/Cargo.toml` 的 `[dependencies]` →
   `bun scripts/check-structure.ts` 红（`crate 依赖方向`），负向夹具用例
   「核心交易域 crate 生产依赖壳层 crate → 红」锁死
   （`scripts/check-structure.test.ts`）。
2. **生产依赖方向（AC3 编译期侧）**：lib 目标的生产依赖面没有根包，本 crate
   生产代码构造 `tauri_app_lib::…` 或业务域引用即编译失败；整环（根包 ⇄
   ledger-transaction 生产互依）由 cargo 解析拒绝。本 crate 因 dev-dependency
   环不能用 `use tauri_app_lib::…` compile_fail 文档用例承载该负向例
   （dev-dependency 对 doctest 可见），机器面负向核对住结构守门（先例说明见
   ledger-backup crate 根文档）。
3. **成员漏继承门禁**：删 `crates/transaction/Cargo.toml` 的
   `[lints] workspace = true` → 结构守门红（既有夹具族覆盖，#1087）。
4. **守门基准随迁**：transaction 自 WHITELIST 移除后，根 src 下残留
   `transaction/` 目录/文件不再被扫描（白名单即规格，清单漂移 fail loud 由
   WHITELIST 反向核对兜住）；TRANSACTION_MODULES 对 crate 内 14 个生产模块照扫
   对壳层/同步域零容忍，夹具「核心交易域 crate 模块引用壳层/同步域 → 红」锁死。
5. **接缝接线删除即红**：任一建库入口删六向 `install_*` 调用 → 走对应路径的
   既有测试立即红（码化错误 `transaction.*-unregistered` 族，本票实测：
   infra 加密单测删接线后 `enable_encryption_failure_keeps_original_db_intact`
   等 2 用例红）。

## 编译耗时 / 体积观测

### CI（Build workflow 后端测试 job，AC4）

- **本票前**（main @ `2532cba1`，run `34685171347`，Linux x64 runner）：
  - 依赖缓存恢复 `Cache Size: ~2009 MB (2107043937 B)`；
  - 编译阶段 `Finished \`test\` profile [unoptimized + debuginfo] target(s) in
    1m 22s`（82s）；
  - 后端测试 job wall clock ≈ 295s（09:11:01 → 09:15:56）；
  - 测试执行：ledger-backup 40 用例 1.51s / ledger-infra 237 用例 4.78s /
    根包 lib 911 用例 38.22s / API 集成 225 用例 9.97s / 命令集成 15 用例
    9.82s / e2e 453 场景 ≈ 52s / 同步轮询 0.28s。
- **本票后**（本分支 PR 的 Build run，push 后回填）：见文末「CI 回填」。

### 本机（控制变量：touch 源文件后重编，依赖全热，kache 命中；macOS / arm64）

| 项 | 耗时 |
| --- | --- |
| `touch crates/transaction/src/lib.rs` + `cargo check -p ledger-transaction --lib` | 3.07s |
| `touch` 交易 crate + 根包 lib 后 `cargo check --workspace --lib` | 8.73s |

交易 crate 可独立增量重编（3.1s），是后续逐域拆分后「改一域只编一域」的观测
起点；按 spec #1086 口径，编译收益不是保留判据。

## 未验证项

- 桌面三平台与 Android 发布构建未在本机跑通（仅 tag / 手动 dispatch 触发）；
  已用 `cargo check --workspace --all-targets` 与三层测试证明迁移不破坏编译。
- `cargo doc -p ledger-transaction` 未纳入 rustdoc 门禁（门禁现仅 `-p ledger-infra`，
  #1139 口径）；本票 crate 文档已按无警告标准书写。
