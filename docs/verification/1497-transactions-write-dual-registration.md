# 交易写入主干步骤双注册（issue #1497 / 父 spec #1494）

> 本票是增量迁移链的关键路径：把 `tests/e2e/transactions_write_steps.rs`（被
> 30/40 个 feature 共享的交易写入主干）整文件改为**双注册**——同一函数同时挂
> cucumber 与 rstest-bdd 属性宏。改写只涉及属性语法与占位符形态，函数体、断言、
> 步骤动词与快照分组纪律零改动；旧 cucumber 目标行为零变化。本文件记录运行时
> 注册表断言、静态覆盖守门口径、负向证据与接线核对。

## 交付物

- `tests/e2e/transactions_write_steps.rs`：22 条步骤全部双注册（Given 1 +
  When 12 + Then 9），占位符按语义改写（`{string}` → `{<参数名>:string}`、
  `{int}` → `{<参数名>:i64}`），参数名与函数形参对齐。账户域消费的 5 条由
  #1495 先行，本票补齐其余 17 条。
- `tests/e2e_rstest.rs`：新增运行时注册表断言
  `transactions_write_steps_are_registered_in_rstest_bdd`——本文件在新目标注册面
  的条数（22）与占位符改写语义（`string` 剥引号、`i64` 解析整数、可选后缀、无
  占位符）经 rstest-bdd 公开注册表 API 在运行时核验。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

## 验收证据

1. **AC1 该文件全部步骤在新目标可被匹配（运行时注册表断言）**：
   `cargo test --test e2e_rstest transactions_write_steps_are_registered_in_rstest_bdd`
   → 1 passed；`./scripts/e2e.sh` 的新目标段 → `13 tests run: 13 passed, 0 skipped`
   （12 个账户域场景 + 1 条注册表断言，nextest 进程级 per-test，0.153s）。
   断言口径：按 `Step.file` 归属本文件的注册条数 = 22，且 4 条代表文本
   （`创建交易 … 备注`、`该转账 to_account_id …`、`注入软删失败触发器` 等）经
   `find_step_with_metadata` 命中本文件注册——删除任一新注册即红（见负向证据）。
2. **AC2 步骤库静态覆盖守门（AC 口径按 #1494 裁决更新）**：
   `bun scripts/check-e2e-step-coverage.ts` → 目标 2 个 · 绑定 feature 40 个 ·
   步骤行 3193 条 · 注册 761 条（rstest-bdd 37 / cucumber 724）· 未覆盖 0 ·
   歧义 0 · 无绑定 0；其中 `tests/e2e_rstest.rs` 注册面 20 → 37 条（账户域 15 +
   本文件 22），未覆盖保持 0。原始 AC「严格编译期步骤校验通过」在 rstest-bdd 0.6.0
   的 `scenarios!` 形态不可得（#1495 实测：`compile-time-validation` 只挂
   `#[scenario]` 路径），经 #1494 裁决改为本静态守门（#1510 落地，口径更新见
   #1497 评论）。
3. **AC3 负向证据（删除即变红）**：临时删去
   `#[rstest_bdd_macros::when("关联上一笔交易创建退款 金额 {amount:i64} 日期 {date:string}")]`
   （只留 cucumber 注册）→ 新目标 2 红：
   - 注册表断言 `left: 21, right: 22`；
   - 已绑场景 `accounts___2` 运行时红：`Step not found at index 2: When 关联上一笔
     交易创建退款 金额 300 日期 "2026-05-02" (feature:
     tests/e2e/features/accounts.feature, scenario: 退款计入账户余额)`。
   恢复该注册后 13/13 复绿。
4. **AC4 未迁移步骤的既有覆盖不丢（旧目标场景数不减）**：
   `cargo test --test e2e` → 40 features / 453 scenarios (453 passed) /
   3139 steps (3139 passed)，与迁移前同口径（场景数、步数均不减）。
5. **门禁与全量测试**：`./scripts/check.sh` 全绿（含
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构/文档/
   i18n/测试支撑守门与 e2e 步骤库覆盖守门）；`./scripts/test.sh` 全绿：并发入口
   32/32 二进制（1787 passed / 0 failed / 4 ignored）+ e2e 入口（新目标 nextest
   13/13、旧目标 453 scenarios）+ doc-test 各包通过。本机
   `cargo-nextest 0.9.145`（与 CI 固定版本一致）为跑通 e2e 入口的前置工具。

## 双注册接线核对清单（`rg` 枚举全部调用点）

「本文件所有步骤都必须双注册」是成对调用型约束，故逐一核对而非抽样：

| 关键字 | 条数 | 覆盖的 feature 步骤（去引号形态） |
| --- | --- | --- |
| Given | 1 | 存在账户…币种 |
| When | 12 | 创建交易（含备注变体）、尝试创建转账、尝试创建交易、尝试买入标的、尝试买入/卖出不存在标的、注入买入建仓中途失败触发器、注入软删失败触发器、尝试删除最近交易、创建转账、关联上一笔交易创建退款 |
| Then | 9 | 交易列表应包含 N 条记录、第 N 条交易类型/金额（含备注变体）、应返回错误、无买入持仓与买卖明细残留、该转账类型、该转账 account_id/to_account_id 匹配账户、退款交易的 refund_of 指向原支出交易 |

核对口径：`rg -c '^#\[(given|when|then)\('` = 22 ↔
`rg -c '^#\[rstest_bdd_macros::(given|when|then)\('` = 22，逐函数同体（同一函数
上下相邻两条属性）；rstest-bdd 侧占位符 100% 带类型提示（`{<名>:string|i64}`），
零无类型形态。

## 证据边界（不夸大）

#1510 静态守门按「目标的绑定 feature」判定注册面：`tests/e2e_rstest.rs` 当前只绑
`accounts.feature`，故守门对 rstest 侧全等判定的范围是该 feature 的 54 条步骤行
（覆盖本文件 5 条）。本文件其余 17 条在新目标尚无绑定 feature，其 rstest 侧由本票
的运行时注册表断言覆盖（22 条全量注册 + 代表文本按占位符语义命中）；与消费 feature
步骤行的逐条语义全等，随各 feature 被绑到新目标（后续逐域票）交由 #1510 守门接管。
cucumber 侧因 40 个 feature 全绑定，本文件 22 条模式与消费步骤行的全等判定已在
#1510 门下。

## 与相邻票的边界

- 本票只保证 `transactions_write_steps.rs` 整文件在新目标注册面齐全，不扩绑
  `transactions_write.feature`——该 feature 还消费投资域等未迁移步骤文件，
  逐域并入与新目标绑定面扩展归后续票。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
- 旧 cucumber 目标与 `cucumber` dev-dependency 由 spec #1494 收口票删除，不做
  双轨长跑。
- 本票给 `tests/e2e_rstest` 目标加了第 13 个测试（运行时注册表断言），该目标当前
  测试数 = 12 场景 + 1 断言；`docs/verification/1496-e2e-nextest.md` 的「12 个
  Scenario 各占一个进程」是该票当时的计数，nextest 调度面本身不变。
