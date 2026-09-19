# 交易转换·出资·来源溯源迁入新目标（issue #1503 / 父 spec #1494）

> 本票把 `transactions_convert.feature`（4）、`transactions_funding.feature`（1）与
> `transactions_source.feature`（21）共 26 个 Scenario 绑进 rstest-bdd 新目标
> （`tests/e2e_rstest.rs`），并把三个 feature 消费的步骤函数改为**双注册**：
> `transactions_convert_steps.rs` 与 `transactions_source_steps.rs` 整文件双注册；
> `scheduled_steps/create.rs` 与 `search_steps.rs` 按需补注册。改写只涉及属性语法
> 与占位符形态，函数体与断言语义唯一；旧 cucumber 目标行为零变化。本文件记录验收
> 证据、双注册接线核对清单与负向证据。

## 交付物

- `tests/e2e_rstest.rs`：新增 3 条 `scenarios!` 绑定（transactions_convert 4 /
  transactions_funding 1 / transactions_source 21），并按 `#[path]` 并入
  `transactions_convert_steps.rs`、`transactions_source_steps.rs` 与 `search_steps.rs`
  （`scheduled_steps.rs` 已由 #1500 / #1501 并入）；新增运行时注册表断言
  `transaction_convert_and_source_steps_are_registered_in_rstest_bdd`，核对
  `transactions_convert_steps.rs` 12 条、`transactions_source_steps.rs` 16 条整文件
  注册，以及 `scheduled_steps/create.rs` 4 条、`search_steps.rs` 1 条按需注册。
- `tests/e2e/transactions_convert_steps.rs`：整文件 12 条双注册（When 4 + Then 8）。
- `tests/e2e/transactions_source_steps.rs`：整文件 16 条双注册（Then 16）。
- `tests/e2e/scheduled_steps/create.rs`：来源溯源场景消费的 4 条按需双注册——
  无备注/带备注订阅、带备注分期、带备注定时转账；其余计划创建步骤归 #1506。
- `tests/e2e/search_steps.rs`：来源溯源场景消费的 `搜索 {string}` 1 条按需双注册；
  其余检索步骤归 #1505。
- 出资 feature 消费的账户、基金申赎、余额与涉及账户分页步骤已由
  #1495 / #1497 / #1498 / #1502 注册，本票只增加 feature 绑定，无新增共享注册。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

双注册只改属性形态与占位符写法（`{string}` → `{<参数名>:string}`、`{int}` →
有符号/无符号整数 hint、`{float}` → `{<参数名>:f64}`），参数名与函数形参对齐，
引号剥离与数值解析语义与 cucumber 一致（CONTEXT-testing「行为等价判据」）。

## 验收证据

1. **AC1 场景数 = feature 内 Scenario 数（新目标逐 feature 全等）**：
   `grep -c '^  Scenario:'` → transactions_convert 4 · transactions_funding 1 ·
   transactions_source 21；`cargo test --test e2e_rstest` →
   **229 passed; 0 failed**（迁移前新目标 202 个测试 + 本票 26 个场景 + 1 条运行时
   注册表断言）。新目标中三个 feature 的测试名分别生成 4 / 1 / 21 个场景测试，
   全部通过。
2. **AC2 旧 e2e 目标全绿、场景数不减**：`cargo test --workspace --test e2e` →
   **40 features / 453 scenarios（453 passed） / 3139 steps（3139 passed）**，
   与迁移前同口径，双注册未改变旧 cucumber 注册面。
3. **静态覆盖守门**：`bun scripts/check-e2e-step-coverage.ts` →
   目标 2 个 · 绑定 feature 40 个 · 步骤行 4508 条 · 注册 1056 条 ·
   未覆盖 0 · 歧义 0 · 无绑定 0；新目标 `tests/e2e_rstest.rs`：绑定 feature 21 ·
   注册 332 · 步骤 1369。即三个 feature 的每条步骤行在新目标注册面恰好一次命中。
4. **运行时注册表断言**：`transaction_convert_and_source_steps_are_registered_in_rstest_bdd`
   锁定 12 / 16 / 4 / 1 条注册与 string / i64 / usize / f64 占位符命中；删掉任一新
   注册即红（实测见下）。
5. **nextest 目标子集**：
   `cargo nextest run --test e2e_rstest -E '… transactions_convert / funding / source …'`
   → **27 tests run: 27 passed, 202 skipped**（26 个场景 + 1 条注册表断言）。
6. **门禁与全量测试**：`./scripts/check.sh` 退出码 0；`cargo test --workspace --doc`
   各包 doc-test 通过。`./scripts/test.sh` 的并发入口 32/32 通过
   （1788 passed / 0 failed / 4 ignored）；随后 nextest 段两次复跑分别在
   `physical_asset_updates___` 与 `physical_assets___11` 命中 #1500 已登记的既有
   #1489 缺陷（同毫秒 UUID v7 排序不确定，与本票步骤迁移无关），故完整 `test.sh`
   两轮均在该范围外 flaky 处中止；本票新增 26 个场景与注册表断言在两次 nextest
   全量运行及 27/27 目标子集中均全绿。旧目标已单跑通过（见 AC2），doc-test 已单跑
   通过。
7. **负向证据（删除即变红）**：
   - 本票实现的红灯阶段：加入 3 条 `scenarios!` 绑定、尚未补注册时，静态覆盖守门
     报 **67 处未覆盖**（转换/来源整文件步骤、来源 feature 的搜索与 4 条计划创建
     步骤），补注册后复绿。
   - 临时删去 `search_steps.rs` 的
     `#[rstest_bdd_macros::when("搜索 {query:string}")]`（只留 cucumber 注册）→
     静态覆盖守门报 5 处未覆盖（`transactions_source.feature:51/53/103/149/180`）；
     新目标 **5 failed**：4 个已绑来源场景红在
     `Step not found ... When 搜索 "…"`（用户可观察的场景步骤缺失），
     注册表断言红在 `left: 0 / right: 1`。恢复该注册后复绿。

### 双注册接线核对清单（`rg` 枚举全部调用点）

「本票纳入的步骤文件所有步骤都必须双注册」是成对调用型约束，故逐文件核对而非抽样
（口径：`rg -c '^#\[(given|when|then)\('` ⇔
`rg -c '^#\[rstest_bdd_macros::(given|when|then)\('`，同一函数上下相邻两条属性、
函数体唯一）：

| 步骤文件 | cucumber ⇔ rstest | 口径 |
| --- | --- | --- |
| `transactions_convert_steps.rs` | 12 ⇔ 12 | 整文件 |
| `transactions_source_steps.rs` | 16 ⇔ 16 | 整文件 |
| `scheduled_steps/create.rs` | 8 ⇔ 4 | 按需（来源 feature 消费的 4 条创建步骤） |
| `search_steps.rs` | 16 ⇔ 1 | 按需（来源 feature 消费的 `搜索 {string}`） |

其余跨文件共享步骤由既有票已双注册（`存在账户…`、基金申赎、余额与交易查询、
保单/计划/物品/标的来源前置等）；无其它跨文件依赖，由静态覆盖守门「未覆盖 0」背书。

## 证据边界（不夸大）

- 双注册的同一函数承载两份步骤文本（cucumber `{string}` 与 rstest-bdd
  `{<名>:type}`）：静态守门对两族各自与 feature 步骤行比对，故跨族文本漂移
  （rstest 侧 hint 写成合法但语义不同的整数族）不会被守门直接拦住——已由逐 feature
  场景运行（消费侧）覆盖；改写时以函数形参类型为准。
- `scheduled_steps.rs` 与 `search_steps.rs` 按整模块并入新目标（模块内其它步骤分别
  归 #1506 / #1505），本票只为被消费的 5 条步骤补注册，不提前迁移其余域。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围；收口删 cucumber 归 #1508。
