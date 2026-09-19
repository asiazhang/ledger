# 报表与仪表盘域场景迁入新目标（issue #1504 / 父 spec #1494）

> 本票把报表四份 feature（`reports` / `reports_category` / `reports_date_range` /
> `reports_period`）与 `dashboard` / `financial_freedom` 共 6 份 feature 的 36 个
> Scenario 绑进 rstest-bdd 新目标（`tests/e2e_rstest.rs`），并把消费的步骤文件按
> 「本域整文件 + 跨域按需」双注册：`reports_steps` 20 条、`dashboard_steps` 9 条、
> `financial_freedom_steps` 7 条整文件，加 `budget_steps` 按需 4 条。旧 cucumber
> 目标行为零变化。本文件记录验收证据、双注册接线核对清单、单字占位符提示的守门
> 扩展与负向证据。

## 交付物

- `tests/e2e_rstest.rs`：新增 6 条 `scenarios!` 绑定（reports 4 · reports_category 3 ·
  reports_date_range 4 · reports_period 6 · dashboard 10 · financial_freedom 9，共
  36 个 Scenario → 独立测试）；按 `#[path]` 并入 `reports_steps.rs` /
  `financial_freedom_steps.rs` / `budget_steps.rs`（`dashboard_steps.rs` 已在
  #1500 / #1502 并入）；新增运行时注册表断言
  `reports_and_dashboard_steps_are_registered_in_rstest_bdd`。
- `tests/e2e/reports_steps.rs`：整文件 20 条（Given 1 + When 10 + Then 9）双注册。
- `tests/e2e/dashboard_steps.rs`：本票补齐 5 条（标的现价 Given、已买入 Given/When、
  非投资账户余额合计应为、实物资产估值合计应为），整文件达 9 条 ⇔ 9 条。
- `tests/e2e/financial_freedom_steps.rs`：整文件 7 条双注册。
- `tests/e2e/budget_steps.rs`：按需 4 条（存在支出分类 / 为分类创建月预算 / 为分类
  创建年预算 / 分类本月有一笔支出）；其余 26 条预算域步骤归后续预算票。
- `scripts/check-e2e-step-coverage.ts` + `.test.ts`：覆盖守门新增 `word` 单字提示的
  匹配口径（见下「形态差异 1」）与一条守门用例。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

双注册只改属性形态与占位符写法（`{string}` → `{<参数名>:string}`、`{int}` →
有符号 / 无符号整数 hint、`{float}` → `{<参数名>:f64}`），参数名与函数形参对齐，
引号剥离与数值解析语义与 cucumber 一致（CONTEXT-testing「行为等价判据」）。

## 验收证据

1. **AC1 场景数 = feature 内 Scenario 数且全绿**：`cargo nextest list --test
   e2e_rstest` 的逐 feature 场景数（reports 4 / reports_category 3 /
   reports_date_range 4 / reports_period 6 / dashboard 10 / financial_freedom 9）与
   `grep -c '^  Scenario:' …/<feature>` 逐一相等；本票迁入的 36 个场景 + 1 条注册表
   断言在新目标全绿（`cargo nextest run --test e2e_rstest -E
   'test(reports)|test(dashboard)|test(financial_freedom)'` → `37 tests run: 37
   passed`；这 37 个测试在本票 10 次整跑中次次通过——整跑偶发的红见下节「既有间歇
   失败」，与票面 6 个 feature 无关）。新目标总数 239 = 202（基线 26773e17）+
   36 个场景 + 1 条注册表断言。
2. **AC2 旧 e2e 目标全绿**：`./scripts/e2e.sh` 旧目标段 →
   40 features / **453 scenarios（453 passed）** / 3139 steps（3139 passed），
   场景数与步数与迁移前同口径（双注册不改旧注册面与函数体）。
3. **静态覆盖守门（#1510 口径）**：`bun scripts/check-e2e-step-coverage.ts` →
   目标 2 个 · 绑定 feature 40 个 · 步骤行 4568 条 · 注册 1059 条 · 未覆盖 0 ·
   歧义 0 · 无绑定 0；新目标 `tests/e2e_rstest.rs`：绑定 feature 24 · 注册 335 ·
   步骤 1429（本票新增 6 个 feature 的 277 条步骤行全部命中，未覆盖 0 / 无歧义）。
   被绑 6 个 feature 的每条步骤行在新目标注册面**恰好一次**命中，删任一注册即红。
4. **负向证据（删除即变红，实测）**：临时删去 `reports_steps.rs` 里
   `{year_token:word}有一笔支出 …` 的新注册（只留 cucumber）→
   - 静态覆盖守门红：6 处未覆盖（`reports_date_range.feature` 的 6 条相对年份支出步骤）；
   - 新目标运行时红 4 个：注册表断言（`left: 19, right: 20`）+ `reports_date_range`
     的 3 个场景，红在
     `Step not found at index 1: Given 去年有一笔支出 100 到账户 "现金"`（feature:
     `reports_date_range.feature`, scenario: 未来日期流水的日期范围终点被最新流水日期撑大）。
   恢复该注册后门禁与场景复绿。
5. **守门脚本自测**：`pnpm vitest run scripts/check-e2e-step-coverage.test.ts` →
   `11 passed`（含本票新增的「rstest 单字提示 `{名:word}` 命中无引号单字」用例）。
6. **门禁与全量测试**：`./scripts/check.sh` 全绿（含 `cargo fmt --all -- --check`、
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构 / 文档 /
   i18n / 测试支撑 / 测试执行覆盖 / e2e 步骤库覆盖各守门）；`./scripts/test.sh` 退出码 0
   （并发入口 + e2e 入口新目标 nextest `239 tests run: 239 passed` 与旧目标
   453 scenarios + doc-test 各包通过）。

### 既有间歇失败（非本票引入，显式报告）

新目标经 nextest 进程级调度整跑时，实物资产域个别场景会间歇失败。本票整跑新目标
共 10 次（含 `scripts/test.sh` 与 `scripts/e2e.sh` 入口）：4 次全绿
（`239 tests run: 239 passed`），6 次出现 1–4 个红点，且红点每次都只落在下列
4 个实物资产场景——报表 / 仪表盘 / 自由度 37 个测试次次通过：

- `physical_asset_updates.feature`「更新估值后当前估值变为最新一条（缺省日期 =
  今天）」「同日更新估值按插入序取最新一条（追加不改写的同日口径）」「详情读回更新
  后的当前估值（与列表同口径）」（对应 #1489 的 `physical_asset_updates.feature:20`
  一族）；
- `physical_assets.feature`「外币估值经当期汇率折算进在持合计」，断言
  `当前估值折本位币不符 left: Some(30000) right: Some(72000)`
  （panic 于 `physical_assets_steps.rs:280`；独立复跑该 feature 13/13 通过）。

这三处正是 #1500「根因发现」登记的 #1489 同根因实例（更新估值读旧值 / 多资产列表
取错行）：同毫秒 UUID v7 排序不确定（`crates/physical-asset/src/crud.rs` 的
`ORDER BY created_at, id` 与 `valuation_date DESC, id DESC`，而 `now_iso` 只到秒、
UUID v7 同毫秒低位随机）。按 #1500 约定**不修**，见
`docs/verification/1500-items-physical-assets-migration.md`。本票迁入的 36 个场景与
实物资产步骤零耦合（覆盖守门「歧义 0」佐证无步骤抢占），失败与本次改动无关。

### 双注册接线核对清单（`rg` 枚举全部消费点）

「被 feature 消费的步骤都必须双注册」是成对调用型约束，故逐文件核对而非抽样
（`rg -c '^#\[(given|when|then)\('` ⇔ `rg -c 'rstest_bdd_macros::(given|when|then)'`，
同一函数上下相邻两条属性、函数体与断言唯一）：

| 注册出处 | cucumber ⇔ rstest | 覆盖面 |
| --- | --- | --- |
| `reports_steps.rs` | 20 ⇔ 20 | 相对年份支出夹具（单字提示）、带币种商户交易、年份 / 期间商户排行、分类份额年份 / 全时段 / 期间、日期范围查询与断言、月度汇总期间 / 年份查询与行断言、排行 / 份额行数与字段契约 |
| `dashboard_steps.rs` | 9 ⇔ 9 | 存在标的、标的现价、已买入（Given/When 双注册）、查询净资产总览、净资产 / 非投资账户余额 / 持仓市值 / 实物资产估值断言 |
| `financial_freedom_steps.rs` | 7 ⇔ 7 | 存在隐藏账户、查询财务自由度、分子 / 分母 / 自由度 / 覆盖年数 / 本位币断言 |
| `budget_steps.rs` | 30 ⇔ 4（按需） | 存在支出分类、为分类创建月 / 年预算、分类本月有一笔支出 |
| `accounts_steps.rs` | 15 ⇔ 15（既有） | 存在账户 / 创建账户（含初始余额）等本票消费的账户夹具 |
| `scheduled_steps/occurrence.rs` | 14 ⇔ 3（既有按需） | 存在汇率 X 兑 Y 为 R（跨币种折算夹具） |
| `merchants_steps.rs` | 18 ⇔ 6（既有按需） | 存在商户、无币种带商户交易（reports 场景） |
| `transactions_write_steps.rs` | 22 ⇔ 22（既有） | 创建交易 / 创建转账 / 关联上一笔创建退款 / 应返回错误 |
| `instruments_steps.rs` | 32 ⇔ 32（既有） | 存在市场…的标的（dashboard 场景） |
| `investment_trend_steps.rs` | 9 ⇔ 9（既有） | 卖出标的…从账户…日期（自由度场景） |
| `manual_quote_steps.rs` | 6 ⇔ 6（既有） | 给标的录价（自由度场景） |
| `physical_assets_steps.rs` | 18 ⇔ 18（既有） | 创建实物资产（dashboard 场景） |
| `physical_asset_disposal_steps.rs` | 7 ⇔ 7（既有） | 处置实物资产 / 软删除实物资产（dashboard 场景） |

## 两处形态差异（显式报告）

1. **单字占位符 `{word}` 无 rstest-bdd 内置对应**：`reports_steps.rs` 的
   `{word}有一笔支出 …`（相对年份记号「前年 / 去年 / 今年 / 明年」，无引号）是仓内
   唯一一处 cucumber `{word}`。rstest-bdd 占位符提示只内置 `string` / 整数族 / 浮点族，
   未知提示（含本票引入的 `word`）回退为惰性任意（`.+?`），故新目标侧注册
   `{year_token:word}`——运行期匹配该无引号单字并原位绑定 `String` 形参。覆盖守门原
   口径只认 `string` / 整数族 / 浮点族，本票为其 `typedMatcher` 增加 `word` → `\S+`
   （与 cucumber `{word}` 同义，严于 rstest-bdd 运行期的 `.+?`，不会把带空格文本误判
   为已覆盖），并补一条守门用例。守门口径的这一次扩展是本票为「整文件双注册」必需的
   适配，不改 cucumber 侧或既有已迁域的任何注册。
2. **`budget_steps.rs` 跨域按需注册**：报表 / 仪表盘 / 自由度场景消费 4 条预算分类
   夹具，其余 26 条预算域步骤不属本票（归后续预算票），故只按需双注册这 4 条——与
   #1500 处理 `scheduled_steps/occurrence.rs`、#1502 处理 `dashboard_steps.rs` /
   `migration_steps.rs` 同款。

## 与相邻票的边界

- 本票只扩展新目标的绑定面与步骤注册面，不删旧 cucumber 目标与 `cucumber`
  dev-dependency（收口票 #1508）。
- `budget.feature` / `books` / `migration` / `sync` 等引导类场景归 #1505 / #1507 等
  后续票；本票对 `budget_steps.rs` 的按需注册不改变其归属。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
- #1489 的实物资产域间歇失败（同毫秒 UUID v7 排序）不在本票范围，仅复现并显式报告。
