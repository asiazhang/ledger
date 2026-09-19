# 交易×保单场景迁入新目标（issue #1499 / 父 spec #1494）

> 本票把 `transactions_policy.feature` 的 7 个 Scenario 绑进 rstest-bdd 新目标
> （`tests/e2e_rstest.rs`），并按需把该 feature 消费的步骤文件双注册：交易×保单
> 步骤文件整文件 13 条，加保单/保司/标的/买卖与持仓断言各按需条数。旧 cucumber
> 目标行为零变化。本文件记录验收证据、双注册接线核对清单、两处形态差异与负向证据。

## 交付物

- `tests/e2e_rstest.rs`：新增 `scenarios!("tests/e2e/features/transactions_policy.feature")`
  绑定（7 个 Scenario → 7 个独立测试）；并入所需步骤模块；新增运行时注册表断言
  `transactions_policy_steps_are_registered_in_rstest_bdd`（本文件 13 条注册 +
  string / i64 / usize / 数据表步骤命中），并把 #1497 的同类断言提取为共用形状
  `assert_steps_registered_in_rstest_bdd`（断言口径不变，仅去重）。
- `tests/e2e/transactions_policy_steps.rs`：整文件 13 条步骤在新目标注册（When 10 +
  Then 3），占位符按语义改写（`{string}` → `{<参数名>:string}`、`{int}` →
  `{<参数名>:i64|usize}`）。
- 按需双注册：`insurers_steps.rs` 1 条（存在保司）、`policies_steps.rs` 2 条
  （创建保单 / 软删第 N 张保单）、`instruments_steps.rs` 1 条（存在标的 名称 币种）、
  `transactions_edit_steps.rs` 2 条（买入标的 / 标的持仓数量）。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

## 验收证据

1. **AC1 步骤文件全量双注册**：`transactions_policy_steps.rs` 在新目标注册条数
   = 13（运行时注册表断言），且 5 条代表文本（挂单创建、挂单转账、挂单断言、
   软删引用保留、`批量导入挂单交易`）经真实占位符语义命中本文件注册。被消费的
   保单/保司/标的/买卖步骤各按其消费条数双注册（见核对清单）。
2. **AC2 场景数 = feature 场景数且全绿**：
   `grep -c '^  Scenario:' tests/e2e/features/transactions_policy.feature` = 7；
   `./scripts/e2e.sh` 新目标段 21 tests = 12 账户域场景 + 7 交易×保单场景 + 2 条
   注册表断言，`21 passed, 0 skipped`（cargo-nextest 进程级 per-test）。
3. **AC3 旧 e2e 目标全绿**：`cargo test --workspace --test e2e` →
   40 features / 453 scenarios (453 passed) / 3139 steps (3139 passed)，场景数与
   步数均与迁移前同口径。
4. **静态覆盖守门（#1510 口径）**：`bun scripts/check-e2e-step-coverage.ts` →
   目标 2 个 · 绑定 feature 40 个 · 步骤行 3256 条 · 注册 780 条 · 未覆盖 0 ·
   歧义 0 · 无绑定 0；新目标 `tests/e2e_rstest.rs`：绑定 feature 2 · 注册 56 ·
   步骤 117（其中交易×保单 63 条步骤行全部命中，未覆盖 0 / 无歧义）。
5. **门禁与全量测试**：`./scripts/check.sh` 全绿（含
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构/文档/
   i18n/测试支撑各守门与 e2e 步骤库覆盖守门）；`./scripts/test.sh` 退出码 0：
   并发入口 32/32 二进制（1788 passed / 0 failed / 4 ignored）+ e2e 入口（新目标
   21/21、旧目标 453 scenarios）+ doc-test 各包通过。本机 cargo-nextest 0.9.145
   （与 CI 固定版本一致）为跑通 e2e 入口的前置工具。
6. **负向证据（删除即变红）**：临时删去
   `#[rstest_bdd_macros::when("批量导入挂单交易")]`（只留 cucumber 适配）→ 新目标
   2 红：注册表断言 `left: 12, right: 13`；已绑场景
   `scenarios::transactions_policy_feature_scenarios::transactions_policy___4` 运行时红：
   `Step not found at index 3: When 批量导入挂单交易 (feature:
   tests/e2e/features/transactions_policy.feature, scenario: 批量导入挂单保费与挂单
   理赔款，归属与手动录入一致)`。恢复该注册后复绿。

## 双注册接线核对清单（`rg` 枚举全部调用点）

「被该 feature 消费的步骤都必须双注册」是成对调用型约束，故逐一核对而非抽样：

| 注册出处 | cucumber ↔ rstest | 覆盖的 feature 步骤（去引号形态） |
| --- | --- | --- |
| `transactions_policy_steps.rs` | 13 ↔ 13 | 创建交易（挂单）、尝试创建交易（挂单）、尝试创建转账（挂单）、尝试买入/卖出标的（挂单）、尝试创建退款（挂单）、批量导入挂单交易、修改最近交易挂保单/清除挂单、修改第 N 条交易备注保持原挂单、第 N 条交易挂单应为/应无挂单/挂单引用保留 |
| `insurers_steps.rs` | 15 ↔ 1（按需） | 存在保司 |
| `policies_steps.rs` | 20 ↔ 2（按需） | 创建保单、软删第 N 张保单 |
| `instruments_steps.rs` | 32 ↔ 1（按需） | 存在标的 名称 币种 |
| `transactions_edit_steps.rs` | 15 ↔ 2（按需） | 买入标的、标的 N 持仓数量应为 |

核对口径：`rg -c '^#\[(given|when|then)\('` ↔
`rg -c '^#\[rstest_bdd_macros::(given|when|then)\('`；同一函数上下相邻两条属性，
函数体与断言唯一。rstest-bdd 侧占位符 100% 带类型提示
（`{<名>:string|i64|usize}`），零无类型形态。

## 两处形态差异（显式报告）

1. **数据表步骤无法共用同一函数签名**：cucumber 以 `&cucumber::gherkin::Step`
   注入数据表，rstest-bdd 只识别名为 `datatable` 的 `Vec<Vec<String>>` 参数或
   `#[datatable]` 标记（`rstest-bdd-macros` 的 `type_shape` 分类），后者与
   `&Step` 不同型。故 `批量导入挂单交易` 保留**一个共用实现**
   （`batch_import_with_policy_rows`）+ **两个薄适配**（cucumber 形态与 rstest
   形态各一），写入与断言语义仍唯一，其余 12 条步骤维持「同一函数双属性」。
2. **`Result` 影子名与 rstest-bdd 生成代码冲突**：`instruments_steps.rs` 原以
   `use ledger_infra::error::Result;` 引入单参别名，rstest-bdd 生成的 wrapper 使用
   未限定的 `Result<StepExecution, StepError>`，双注册后编译失败（E0107）。改为
   `use ledger_infra::error::Result as AppResult;`（三处引用同步），函数体语义不变。

## 与相邻票的边界

- 本票只扩展新目标的绑定面与步骤注册面，不删旧 cucumber 目标与 `cucumber`
  dev-dependency（收口票 #1508）。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
- 场景级计数/执行覆盖守门（「删绑定 → 场景消失」置红）归 #1496/#1510；本票提供
  运行时注册表断言与静态覆盖守门两层证据，不另建第二套门禁。
