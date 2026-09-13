# 账户域 crate（issue #1093 / 父 spec #1086）

> 本票把账户域（账户 CRUD / 余额口径与余额缓存 / 同步命令）自根包域目录拆为
> workspace 成员 `ledger-accounts`（`src-tauri/crates/accounts`，spec #1086 P3
> 叶子业务域 crate），成为可被投资域与多端同步域依赖的独立编译单元。与 #1092
> 不同，本票无前置接缝反转提交：核心交易域对本域的两条引用（写路径余额重算
> #1090、出资账户视图 #1092）已在前票按挂载点反转收敛，本域实现侧
> （`install_balance_refresh_hook` / `install_funding_account_hook`）随目录
> 原样迁入。本文件记录迁移形态、结构守门变更、负向验收证据与测试矩阵
>（上一份观测点见 `1092-transaction-crate.md`）。

## 交付物

- `src-tauri/crates/accounts/`：新成员 crate（整目录 `git mv`，git 记录为重命名，
  历史可跟随）；`ledger-infra` + `ledger-sync-protocol` + `ledger-transaction`
  + rusqlite/serde/chrono/utoipa（生产依赖面恰好三条 workspace 依赖，AC1）；
  `[dev-dependencies]` 经 dev-dependency 环引根包（tauri-app）与 test-utils
  器具；`[lints] workspace = true` 继承六件套。
- crate 迁移：根包 `pub use ledger_accounts as accounts;` 再导出（expand 形态），
  壳层/域/e2e 约 100+ 处调用点零改动；`replay_command` 跨 crate 消费项
  `pub(crate)`→`pub`（sync_engine::registry 经再导出面调用，签名与语义不变，
  #1092 同款）；balance.rs 的 compile_fail 负向例（`affected_accounts` 私有性）
  语义不变保留——再导出面对 doctest 可见，但私有项在两份实例中同源不可达，
  再公开即红。
- 守门同步：accounts 自 WHITELIST 移除、`ledger-accounts` 登记 CRATES、新增
  ACCOUNTS_MODULES 模块级扫描（对壳层/同步域零容忍照扫，#1092 形态）、全树
  扫描基准纳入 crate（模型域化禁令 / 原生事务语句禁令）；负向夹具随迁
  （infra→域扫描靶域自 accounts 改以 investment——accounts 已拆出，且
  categories/currencies/merchants 为同波并发拆分票 #1094–#1096 的靶域）。

## 测试实例纪律（dev 依赖环双实例）

`cargo test -p ledger-accounts` 的依赖图内存在本 crate 两份实例（被测本实例 +
根包图内实例；ledger-backup/ledger-transaction 先例同款）。接缝注册静态由测试
工厂（`tauri_app_lib::test_support::open`）接在根包图实例上，带类型签名的钩子
无法跨实例注册，故：

- 走接缝的行为路径测试经 `tauri_app_lib::…` 驱动——账户域 tests.rs 的建库走
  `tauri_app_lib::test_support::open()`，`adjust_account_balance` 内嵌的
  `create_transaction_internal`（共享 ledger-transaction 实例）触发的余额刷新
  钩子在根包图实例上接线（同一份源码，断言与场景文本零改动）；
- 纯函数用例（`balance/tests.rs` 的 `affected_accounts` 并集口径直测）不读注册
  静态，直接驱动本实例；
- 依据留痕于 crate 根 lib.rs「测试实例纪律」段与 Cargo.toml dev-dependencies
  注释。

行为等价主证据不受此影响：根包图内只有一个账户域实例，三层测试（域单测经测试
工厂、API/命令集成、e2e）全部走该实例。

## 负向验收（删除即变红 / 编译期双证）

1. **生产依赖方向（AC3 结构守门侧）**：夹具把 `tauri-app = { path = "../.." }`
   写进 `crates/accounts/Cargo.toml` 的 `[dependencies]` →
   `bun scripts/check-structure.ts` 红（`crate 依赖方向`），负向夹具用例
   「账户域 crate 生产依赖壳层 crate → 红」锁死
   （`scripts/check-structure.test.ts`）。
2. **生产依赖方向（AC3 编译期侧）**：lib 目标的生产依赖面没有根包与任何同级
   业务域，本 crate 生产代码构造 `tauri_app_lib::…` 或同级域引用即编译失败；
   整环（根包 ⇄ ledger-accounts 生产互依）由 cargo 解析拒绝。本 crate 因
   dev-dependency 环不能用 `use tauri_app_lib::…` compile_fail 文档用例承载
   该负向例（先例说明见 ledger-backup crate 根文档），机器面负向核对住结构
   守门的 crate 依赖方向。
3. **成员漏继承门禁**：删 `crates/accounts/Cargo.toml` 的
   `[lints] workspace = true` → 结构守门红（既有夹具族覆盖，#1087）。
4. **守门基准随迁**：accounts 自 WHITELIST 移除后，根 src 下残留 `accounts/`
   目录不再被扫描（白名单即规格）；ACCOUNTS_MODULES 对 crate 内 4 个生产模块
   照扫对壳层/同步域零容忍，夹具「账户域 crate 模块引用壳层/同步域 → 红」
   锁死；原生事务语句与模型域化禁令的全树扫描基准纳入 crate。
5. **接缝接线删除即红**：任一建库/启动入口（壳层启动、测试工厂、BDD world、
   ledger-perf、命令集成 `fresh_app`/`reattach_app`）删
   `accounts::balance::install_balance_refresh_hook()` → 交易写入路径余额断言
   的既有测试立即红（码化错误 `transaction.*-unregistered` 族）；删
   `transaction_wiring::install_all()` 内的 `accounts::install_funding_account_hook()`
   → buy/sell 出资准入用例红。本次迁移零接线点增删（`rg` 核对：五类入口的
   账户域接线调用原样保留，仅经由再导出面解析）。

## 测试矩阵（本机，`cargo test --workspace` 全量）

| 目标 | 用例数 | 耗时 |
| --- | --- | --- |
| ledger-accounts（新 crate，含 44 迁移用例） | 44 | 1.71s |
| ledger-backup | 40 | 1.44s |
| ledger-infra | 237 | 4.32s |
| ledger-transaction | 234 | 10.40s |
| 根包 lib | 642 | 31.45s |
| ledger-perf | 36 | 7.54s |
| API 集成（api_server） | 231 | 14.35s |
| 命令集成（commands） | 15 | 6.75s |
| e2e BDD | 453 场景 / 3139 步 | 全绿 |
| 同步轮询（sync_trigger_poll） | 1 | 0.28s |
| doctest（含账户域 compile_fail 负向例） | ledger-accounts 1 / backup 1 / protocol 1 / transaction 1 | 全绿 |
| check-structure 守门测试（vitest） | 135 | 13.02s |

- 迁移用例数核对：`tests.rs` 32 + `balance/tests.rs` 12 = 44，与基线
  f0ff9269 根包内同文件 `#[test]` 计数逐一相等；`balance/tests.rs` 为纯重命名
  （0 行差异）。
- 断言零改动：`git diff -M` 逐行核对，测试文件差异全部为引用形态（包名/路径），
  无断言与场景文本修改（AC2；e2e 453/3139 与 #1092 基线完全一致，余额与余额
  缓存行为零变化）。
- `./scripts/check.sh` 全绿（前端类型检查 / clippy 六件套 / gate-off / fmt /
  rustdoc / 文档一致性 / 命令注册 / 结构守门 / i18n / 异步守门等全序列）。
- AC3 双证：cargo 依赖图编译期强制（生产依赖面无根包，生产环解析拒绝）+
  结构守门 CRATES 依赖方向核对（负向夹具「账户域 crate 生产依赖壳层 crate → 红」）。

## 未验证项

- 桌面三平台与 Android 发布构建未在本机跑通（仅 tag / 手动 dispatch 触发）；
  已用 `cargo check --workspace` 与三层测试证明迁移不破坏编译。
- CI 编译阶段耗时对照未回填：本机 4 份并发构建干扰计时，编译收益观测留待 CI
  数据（按 spec #1086 口径，编译收益不是保留判据）。
