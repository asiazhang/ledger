//! BDD e2e 目标（rstest-bdd 通道，spec #1494 / ticket #1495）：把 `.feature` 场景
//! 绑成标准测试运行器里的**独立测试**——`scenarios!` 自动发现场景，测试世界以
//! rstest fixture 单点提供，场景失败由测试运行器置红（不再依赖自建 runner）。
//!
//! 本目标是增量迁移的**新通道**，与旧 cucumber 目标（`tests/e2e.rs`，
//! `harness = false`）并存：
//! - 已迁入的 feature（accounts.feature，#1495；transactions_write.feature，#1497；
//!   transactions_edit.feature / transactions_query.feature，#1498；
//!   transactions_policy.feature，#1499；items_* / physical_asset* 共 8 个 feature，
//!   #1500；policies.feature / policy_agreement.feature / policy_stats.feature，
//!   #1501；instruments.feature / manual_quote.feature，#1502；
//!   transactions_convert.feature / transactions_funding.feature /
//!   transactions_source.feature，#1503；reports.feature /
//!   reports_date_range.feature / reports_period.feature / dashboard.feature /
//!   financial_freedom.feature，#1504；merchants.feature / insurers.feature /
//!   search.feature，#1505；scheduled.feature / budget.feature，#1506；
//!   backup.feature / data_location.feature / encryption.feature /
//!   startup_failure.feature / books.feature / migration.feature /
//!   log_level.feature / sync.feature，#1507；
//!   reports_category.feature 由 #1504 / #1505 两票共同覆盖）
//!   在本目标运行，旧目标行为零变化。账户 / 交易 / 保单、物品、投资、报表 /
//!   仪表盘、参考数据与检索、定时计划与预算域以及引导 / 备份 / 同步 / 迁移类场景
//!   全绿；实物资产域若干场景（估值更新 / 在持合计）受 #1489 既有缺陷（同毫秒
//!   UUID v7 排序不确定）影响，间歇性红，按 #1500 约定不修；
//! - 已迁入域消费的步骤函数改为**双注册**（同一函数同时挂 cucumber 与 rstest-bdd
//!   属性宏），函数体与断言唯一，不复制；数据表步骤因两种 macro 的入参形态不同，
//!   抽共享实现 + 两侧注册适配器（`migration_steps::批量导入交易`、
//!   `transactions_policy_steps::批量导入挂单交易`、
//!   `investment_migration_steps::批量导入投资交易`）；
//!   需要 `await` 的行情抓取桩步骤同理保留共享 async 实现 + 两侧适配器，新目标侧
//!   经唯一接缝 `test_support::block_on` 驱动；
//! - 其余域按 spec #1494 的后续票逐域加注册，收口时删旧目标与 cucumber 依赖。
//!
//! 并行口径（spec #1494 决策）：进程内 libtest 线程并行对世界构造（内存库 + 迁移）
//! 是负收益，真实并行交进程级 per-test 调度（cargo-nextest，ticket #1496）——
//! 本目标只保证「场景 = 普通测试」这一接缝，不自行开线程池。
//!
//! 负向证据（ticket #1495 AC）：删除场景绑定（`scenarios!`）→ 本目标 0 个测试；
//! 删除 `world` fixture → 编译失败（场景绑定以 fixture 名解析测试世界）。
//!
//! 测试整体豁免（ADR-0060）：BDD 测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use rstest_bdd::StepKeyword;

// 顺序有意义：`world` 以 `#[macro_use]` 先入，`world_conn!` / `world_write!`
// 才对其后的步骤模块可见（与旧目标同形）。
//
// `allow(dead_code)` 是**迁移期形态**（spec #1494 / ticket #1495）：共享支撑模块
// （world / common / step_inputs / step_verbs）与整模块并入的共享步骤库
// （scheduled_steps 自 ticket #1500 起按整文件并入、各消费票按需补注册，ticket
// #1506 补齐后其步骤已全部有消费者）按整文件并入，消费者却是已迁移的步骤域子集——
// 尚未迁入的域在本目标里暂时无人调用。逐域迁移完成后本目标即全量目标，该 allow 随
// 最后一个域并入一并删除（届时 `-D warnings` 重新覆盖这些模块）。
#[allow(dead_code)]
#[macro_use]
#[path = "e2e/world.rs"]
mod world;

#[allow(dead_code)]
#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;
#[allow(dead_code)]
#[path = "e2e/backup_steps.rs"]
mod backup_steps;
#[allow(dead_code)]
#[path = "e2e/books_steps.rs"]
mod books_steps;
#[allow(dead_code)]
#[path = "e2e/common.rs"]
mod common;
#[allow(dead_code)]
#[path = "e2e/data_location_steps.rs"]
mod data_location_steps;
#[allow(dead_code)]
#[path = "e2e/encryption_steps.rs"]
mod encryption_steps;
#[allow(dead_code)]
#[path = "e2e/instruments_steps.rs"]
mod instruments_steps;
#[allow(dead_code)]
#[path = "e2e/insurers_steps.rs"]
mod insurers_steps;
#[allow(dead_code)]
#[path = "e2e/investment_migration_steps.rs"]
mod investment_migration_steps;
#[allow(dead_code)]
#[path = "e2e/investment_trend_steps.rs"]
mod investment_trend_steps;
#[path = "e2e/items_common.rs"]
mod items_common;
#[allow(dead_code)]
#[path = "e2e/items_cost_steps.rs"]
mod items_cost_steps;
#[allow(dead_code)]
#[path = "e2e/items_create_steps.rs"]
mod items_create_steps;
#[allow(dead_code)]
#[path = "e2e/items_dispose_steps.rs"]
mod items_dispose_steps;
#[allow(dead_code)]
#[path = "e2e/items_provenance_steps.rs"]
mod items_provenance_steps;
#[allow(dead_code)]
#[path = "e2e/items_update_steps.rs"]
mod items_update_steps;
#[allow(dead_code)]
#[path = "e2e/log_level_steps.rs"]
mod log_level_steps;
#[allow(dead_code)]
#[path = "e2e/manual_quote_steps.rs"]
mod manual_quote_steps;
#[allow(dead_code)]
#[path = "e2e/physical_asset_disposal_steps.rs"]
mod physical_asset_disposal_steps;
#[allow(dead_code)]
#[path = "e2e/physical_asset_updates_steps.rs"]
mod physical_asset_updates_steps;
#[allow(dead_code)]
#[path = "e2e/physical_assets_steps.rs"]
mod physical_assets_steps;
#[allow(dead_code)]
#[path = "e2e/policies_steps.rs"]
mod policies_steps;
#[allow(dead_code)]
#[path = "e2e/policy_agreement_steps.rs"]
mod policy_agreement_steps;
#[allow(dead_code)]
#[path = "e2e/policy_stats_steps.rs"]
mod policy_stats_steps;
#[allow(dead_code)]
#[path = "e2e/step_inputs.rs"]
mod step_inputs;
#[allow(dead_code)]
#[path = "e2e/step_verbs.rs"]
mod step_verbs;
#[allow(dead_code)]
#[path = "e2e/transactions_convert_steps.rs"]
mod transactions_convert_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_edit_steps.rs"]
mod transactions_edit_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_policy_steps.rs"]
mod transactions_policy_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_source_steps.rs"]
mod transactions_source_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;
// 定时计划域（#1506）步骤库按整模块并入其父模块：物品与实物资产域（#1500）与保单域
// （#1501）早先为各自消费的步骤（汇率夹具、保单协议期次）补过注册，本票补齐其余
// 定时计划步骤，`scheduled.feature` 32 场景与 `budget.feature` 17 场景随之进入本目标。
#[allow(dead_code)]
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;
#[allow(dead_code)]
#[path = "e2e/search_steps.rs"]
mod search_steps;
#[allow(dead_code)]
#[path = "e2e/startup_failure_steps.rs"]
mod startup_failure_steps;
#[allow(dead_code)]
#[path = "e2e/sync_steps.rs"]
mod sync_steps;

#[allow(dead_code)]
#[path = "e2e/budget_steps.rs"]
mod budget_steps;
#[allow(dead_code)]
#[path = "e2e/categories_steps.rs"]
mod categories_steps;
#[allow(dead_code)]
#[path = "e2e/dashboard_steps.rs"]
mod dashboard_steps;
#[allow(dead_code)]
#[path = "e2e/financial_freedom_steps.rs"]
mod financial_freedom_steps;
#[allow(dead_code)]
#[path = "e2e/fund_trade_steps.rs"]
mod fund_trade_steps;
#[allow(dead_code)]
#[path = "e2e/merchants_steps.rs"]
mod merchants_steps;
#[allow(dead_code)]
#[path = "e2e/migration_steps.rs"]
mod migration_steps;
#[allow(dead_code)]
#[path = "e2e/reports_steps.rs"]
mod reports_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_query_steps.rs"]
mod transactions_query_steps;

/// 场景绑定与测试世界 fixture：住子模块，避开 rstest fixture 生成模块与顶层
/// `mod world`（测试支撑模块）的同名冲突。
mod scenarios {
    use rstest::fixture;
    use rstest_bdd_macros::scenarios;

    /// 场景级测试世界（旧目标 `#[world(init = Self::new)]` 的 fixture 形态）：
    /// 每场景一次 [`LedgerWorld::new`]，内含接缝接线与黑洞账户种子注册，
    /// 场景间零状态交叉（各自独立内存库）。
    #[fixture]
    fn world() -> crate::world::LedgerWorld {
        crate::world::LedgerWorld::new()
    }

    scenarios!(
        "tests/e2e/features/accounts.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/items_cost.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/items_create.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/items_dispose.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/items_provenance.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/items_update.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/physical_assets.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/physical_asset_updates.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
    scenarios!(
        "tests/e2e/features/physical_asset_disposal.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/transactions_write.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/transactions_edit.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/transactions_query.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 交易×保单场景（ticket #1499）：流水直挂保单的 7 个场景进入新目标，
    // 与账户域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/transactions_policy.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/policies.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 保单域场景（ticket #1501）：静态档案 / 缴费协议 / 统计三份 feature 的
    // 29 个场景进入新目标，与账户域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/policy_agreement.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/policy_stats.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 投资与行情域场景（ticket #1502）：标的 34 个场景（含组合走势、投资迁移
    // 链路、按代码即拉建基金、股票通道行情桩）+ 手动报价 5 个场景，与既有域
    // 同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/instruments.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/manual_quote.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 交易转换 / 出资 / 来源溯源场景（ticket #1503）：4 + 1 + 21 个场景进入
    // 新目标，与既有域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/transactions_convert.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/transactions_funding.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/transactions_source.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 参考数据与检索域场景（ticket #1505）：分类份额 3 个、商户 11 个、保司 5 个、
    // 搜索 25 个场景，与既有域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/reports_category.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/merchants.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/insurers.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/search.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 报表与仪表盘域场景（ticket #1504）：报表四份 feature（商户排行 / 分类份额 /
    // 日期范围 / 期间过滤）17 个场景 + 首页净资产 10 个场景 + 财务自由度 9 个场景，
    // 与既有域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/reports.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/reports_date_range.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/reports_period.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/dashboard.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/financial_freedom.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 定时计划与预算域场景（ticket #1506）：定时交易引擎 32 个场景（native 折算、
    // 事务自持回滚、多周期花费口径、订阅编辑、计划挂商户、期次详情与自动追补）+
    // 预算滚动窗口 17 个场景，与既有域同用一份 `world` fixture（各自独立内存库）。
    scenarios!(
        "tests/e2e/features/scheduled.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/budget.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    // 引导、同步与迁移类场景（ticket #1507）：备份 30 / 数据位置 22 /
    // 加密 16 / 启动失败 12 / 多账本 3 / 迁移验证 9 / 日志等级 4 / 同步 7 个
    // 场景进入新目标，与既有域同用一份 `world` fixture（各自独立内存库/目录）。
    // encryption 的 2 个 @non-root-only 场景保留标签并加 @allow_skipped：旧目标
    // 由启动器过滤，新目标由场景内守卫步骤 `目录只读触发可用` 调 `skip!` 跳过。
    scenarios!(
        "tests/e2e/features/backup.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/data_location.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/encryption.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/startup_failure.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/books.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/migration.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/log_level.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );

    scenarios!(
        "tests/e2e/features/sync.feature",
        fixtures = [world: crate::world::LedgerWorld]
    );
}

/// 运行时注册表断言（ticket #1497 / #1499 AC1/AC3）的共用形状：某步骤文件整文件
/// 的 N 条步骤都已进入新目标的 rstest-bdd 注册表，且改写后的占位符能按真实语义
/// 匹配真实场景文本（`{<名>:string}` 剥引号、整数族解析整数、无占位符直命中）。
/// 「只留 cucumber 注册」（删掉任一新注册）即红。
///
/// 逐条模式的全等覆盖（feature 步骤行 ↔ 注册模式、无漏改 / 无歧义）由静态覆盖
/// 守门兜底（`bun scripts/check-e2e-step-coverage.ts`，ticket #1510 / AC2）；
/// 本断言只担运行时那半：宏确实把注册发进了目标注册表、且查找能命中。
fn assert_steps_registered_in_rstest_bdd(
    file: &str,
    expected: usize,
    samples: &[(StepKeyword, &str)],
) {
    use rstest_bdd::{Step, StepText, find_step_with_metadata, iter};

    /// 文件匹配：注册模式里的 `file`（`file!()` 展开）可能是绝对路径、测试根相对
    /// 路径或裸文件名；按「整段相等或路径分量相等」判定，避免
    /// `migration_steps.rs` 误配 `investment_migration_steps.rs` 这类后缀同名文件。
    fn matches_file(actual: &str, expected: &str) -> bool {
        actual == expected
            || actual.ends_with(&format!("/{expected}"))
            || actual.ends_with(&format!("\\{expected}"))
    }

    let registered: Vec<_> = iter::<Step>
        .into_iter()
        .filter(|step| matches_file(step.file, file))
        .collect();
    assert_eq!(
        registered.len(),
        expected,
        "本文件在新目标的注册条数不符（缺注册或多注册）：{:#?}",
        registered
            .iter()
            .map(|step| step.pattern.as_str())
            .collect::<Vec<_>>()
    );

    for (keyword, text) in samples {
        let found = find_step_with_metadata(*keyword, StepText::from(*text))
            .unwrap_or_else(|| panic!("新目标未匹配到步骤文本：{text}"));
        assert!(
            matches_file(found.file, file),
            "步骤文本 {text} 未命中本文件的注册：{}",
            found.file
        );
    }
}

/// `transactions_write_steps.rs` 整文件 22 条步骤（Given 1 + When 12 + Then 9）
/// 的运行时注册（ticket #1497）。占位符语义抽样：string 剥引号、i64 解析整数、
/// 可选后缀（备注）与无占位符。
#[test]
fn transactions_write_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "transactions_write_steps.rs",
        22,
        &[
            (
                StepKeyword::When,
                "创建交易 类型 \"income\" 金额 5000 到账户 \"现金\" 日期 \"2026-02-02\"",
            ),
            (
                StepKeyword::When,
                "创建交易 类型 \"expense\" 金额 1500 到账户 \"现金\" 日期 \"2026-02-01\" 备注 \"午餐\"",
            ),
            (
                StepKeyword::Then,
                "该转账 to_account_id 应匹配账户 \"B账户\"",
            ),
            (StepKeyword::When, "注入软删失败触发器"),
        ],
    );
}

/// `transactions_policy_steps.rs` 整文件 13 条步骤（When 10 + Then 3）的运行时
/// 注册（ticket #1499）。占位符语义抽样覆盖 string / i64 / usize 与数据表步骤
///（`批量导入挂单交易` 的 rstest 适配注册必须能命中文本）。
#[test]
fn transactions_policy_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "transactions_policy_steps.rs",
        13,
        &[
            (
                StepKeyword::When,
                "创建交易 类型 \"expense\" 金额 300000 到账户 \"现金\" 日期 \"2026-02-01\" 挂保单 \"P2026-101\"",
            ),
            (
                StepKeyword::When,
                "尝试创建转账 金额 3000 从账户 \"A账户\" 到账户 \"B账户\" 日期 \"2026-03-01\" 挂保单 \"P2026-104\"",
            ),
            (StepKeyword::Then, "第 1 条交易挂单应为保单号 \"P2026-101\""),
            (
                StepKeyword::Then,
                "第 1 条交易挂单引用应保留（软删保单不置空）",
            ),
            (StepKeyword::When, "批量导入挂单交易"),
        ],
    );
}

/// 交易转换 / 来源溯源与相关共享步骤的运行时注册（ticket #1503）：
/// `transactions_convert_steps.rs` 12 条、`transactions_source_steps.rs` 16 条
/// 整文件注册（本票消费的 `scheduled_steps/create.rs` 4 条计划创建步骤与
/// `search_steps.rs` 的 `搜索` 步骤的整文件计数断言，已分别收敛到 ticket #1506
/// 的 8 条与 ticket #1505 的 16 条）。占位符语义覆盖 string / i64 / usize、
/// f64 小数与无占位符直命中；删掉任一新注册即红。
#[test]
fn transaction_convert_and_source_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "transactions_convert_steps.rs",
        12,
        &[
            (
                StepKeyword::When,
                "按确认单于 \"2020-05-11\" 转换 \"006793\" 份额 684.77 为 \"519700\" 份额 588.67 转出金额 71229 转入金额 71229 手续费 0 到投资账户 \"基金户\"",
            ),
            (
                StepKeyword::When,
                "修改转换（转入 \"519700\"）为 转出份额 350 转入份额 344 转出金额 40000 转入金额 40000 手续费 200",
            ),
            (
                StepKeyword::Then,
                "转换转入 \"519772\" 的明细应为 转出 \"006793\" 份额 2791.11 转入份额 2805.14 转出金额 290332 转入金额 290332 手续费 500",
            ),
            (
                StepKeyword::Then,
                "全库 Σ 已实现盈亏应等于 Σ 卖出金额 − Σ 买入金额",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "transactions_source_steps.rs",
        16,
        &[
            (
                StepKeyword::Then,
                "交易列表第 1 条来源应为保单 \"P2026-201\" 险种 \"重疾险\"",
            ),
            (
                StepKeyword::Then,
                "交易列表第 1 条来源应为已删除保单 \"P2026-202\" 险种 \"医疗险\"",
            ),
            (
                StepKeyword::Then,
                "交易列表第 1 条来源应为订阅计划 备注 \"视频会员\"",
            ),
            (
                StepKeyword::Then,
                "交易列表第 1 条来源应为标的 \"600519\" 名称 \"招商银行\"",
            ),
            (StepKeyword::Then, "交易列表第 1 条应无来源"),
        ],
    );
}

/// 投资与行情域五个步骤文件整文件双注册的运行时注册（ticket #1502）：整文件注册
/// 条数与占位符语义（string 剥引号、整数族解析、`f64` 小数、数据表步骤、无占位符
/// 直命中）。「只留 cucumber 注册」（删掉任一新注册）即红。
///
/// 异步（行情抓取桩）三态：rstest 形态的注册是场景进入新目标的唯一入口，删掉即
/// `Step not found` 红；其运行方式统一经 `instruments_steps::block_on`（既有接缝
/// `test_support::block_on`）——删掉该调用点则桩不执行、场景断言行红。
///
/// 逐条模式的全等覆盖（feature 步骤行 ↔ 注册模式、无漏改 / 无歧义）由静态覆盖
/// 守门兜底（`bun scripts/check-e2e-step-coverage.ts`）；本断言只担运行时那半。
#[test]
fn investment_domain_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "instruments_steps.rs",
        32,
        &[
            (
                StepKeyword::When,
                "手动创建标的 \"HW-VR\" 类型 \"other\" 名称 \"华为虚拟股\" 币种 \"CNY\"",
            ),
            (StepKeyword::When, "搜索类型 \"fund\" 的标的 \"000001\""),
            (StepKeyword::Then, "标的搜索命中 2 条 总数 2"),
            (
                StepKeyword::Then,
                "标的字典存在类型 \"stock\" 代码 \"600519\" 名称 \"贵州茅台\" 来源 \"manual\" 市场 \"sh\"",
            ),
            (
                StepKeyword::When,
                "按代码添加基金 \"000001\" 东财返回名称 \"华夏成长混合\" 分类 \"混合型-灵活\" 净值 1.318 净值日期 \"2026-08-28\"",
            ),
            // 异步三态（共享 async 实现 + 新目标侧 `block_on` 接线）。
            (
                StepKeyword::When,
                "按代码添加投资标的 市场 \"sh\" 代码 \"600519\" 行情命中名称 \"贵州茅台\" 市场 \"sh\" 现价 1234.56 类型提示 \"stock\"",
            ),
            (
                StepKeyword::When,
                "按代码添加投资标的 市场 \"sh\" 代码 \"600999\" 行情查无此码",
            ),
            (
                StepKeyword::When,
                "按代码添加投资标的 市场 \"sh\" 代码 \"600519\" 行情临时不可达",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "manual_quote_steps.rs",
        6,
        &[
            (
                StepKeyword::When,
                "给标的 \"稳稳地幸福\" 录价 日期 \"2026-08-28\" 价格 13180 万分之一元",
            ),
            (
                StepKeyword::Then,
                "标的 \"稳稳地幸福\" 现价为 13180 万分之一元 priced_at \"2026-08-28\" 来源 \"manual\"",
            ),
            (
                StepKeyword::Then,
                "标的 \"稳稳地幸福\" 价格历史 \"2026-08-28\" 周点价格为 13180 万分之一元 来源 \"manual\"",
            ),
            (
                StepKeyword::Then,
                "标的 \"稳稳地幸福\" 持仓视图市值应为 131800",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "investment_trend_steps.rs",
        9,
        &[
            (
                StepKeyword::Given,
                "存在标的 \"110022\" 的价格历史 交易日 \"2026-02-02\" 价格 32930 万分之一元 币种 \"CNY\"",
            ),
            (
                StepKeyword::Given,
                "存在汇率历史 \"USD\" 兑 \"CNY\" 交易日 \"2026-01-05\" 汇率 7.2",
            ),
            (
                StepKeyword::When,
                "买入标的 \"600519\" 数量 2 单价 100000 到账户 \"投资户\" 日期 \"2026-01-30\"",
            ),
            (
                StepKeyword::When,
                "卖出标的 \"600036\" 数量 2 单价 110000 从账户 \"证券户\" 日期 \"2026-02-10\"",
            ),
            (StepKeyword::When, "查询组合走势"),
            (StepKeyword::Then, "组合走势应有 2 个周点"),
            (StepKeyword::When, "查询标的 \"110022\" 的走势"),
            (StepKeyword::Then, "标的走势应有 2 个周点"),
            (
                StepKeyword::Then,
                "组合走势 \"2026-02-02\" 周市值应为 331300",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "investment_migration_steps.rs",
        4,
        &[
            (
                StepKeyword::When,
                "幂等创建标的 \"600519\" 类型 \"stock\" 名称 \"贵州茅台\" 市场 \"sh\" 币种 \"CNY\"",
            ),
            // 数据表步骤：rstest 适配注册必须能命中文本（数据表由 macro 注入）。
            (StepKeyword::When, "批量导入投资交易"),
            (StepKeyword::Then, "导入的投资交易应有 2 行全部成功"),
            (StepKeyword::Then, "标的 \"600519\" 持仓应为 50"),
        ],
    );

    // fund_trade_steps 整文件 12 条（#1498 已注册 7 条，本票补齐其余 5 条：
    // 带日期申购/赎回、出资账户申购、买入双账户匹配）。
    assert_steps_registered_in_rstest_bdd(
        "fund_trade_steps.rs",
        12,
        &[
            (
                StepKeyword::Given,
                "存在基金标的 \"006793\" 名称 \"某可转换基金\"",
            ),
            (
                StepKeyword::When,
                "按确认单于 \"2020-03-12\" 申购基金 \"006793\" 份额 3475.88 金额 359062 手续费 0 到投资账户 \"基金户\"",
            ),
            (
                StepKeyword::When,
                "按确认单出资账户申购基金 \"000123\" 份额 5000 金额 5000 手续费 0 到投资账户 \"基金户\" 出资账户 \"银行卡\"",
            ),
            (
                StepKeyword::When,
                "按确认单于 \"2020-06-18\" 赎回基金 \"519700\" 份额 588.67 金额 67697 手续费 0 从投资账户 \"基金户\"",
            ),
            (StepKeyword::Then, "该买入 account_id 应匹配账户 \"基金户\""),
            (
                StepKeyword::Then,
                "该买入 funding_account_id 应匹配账户 \"银行卡\"",
            ),
            (StepKeyword::Then, "基金 \"000456\" 已实现盈亏合计应为 2000"),
        ],
    );
}

/// 保单域三个步骤文件的整文件运行时注册（ticket #1501）：`policies_steps.rs` 20 条、
/// `policy_agreement_steps.rs` 11 条、`policy_stats_steps.rs` 7 条。占位符语义抽样
/// 覆盖 string / i64 / usize 与无占位符断言。删掉任一新注册即红。
#[test]
fn policy_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "policies_steps.rs",
        20,
        &[
            (
                StepKeyword::When,
                "创建保单 保司 \"平安保险\" 保单号 \"P2026-001\" 险种 \"重疾险\" 起日 \"2026-01-01\" 止日 \"2036-01-01\" 保额 \"30000000\" 币种 \"CNY\"",
            ),
            (
                StepKeyword::Then,
                "第 1 张保单保额应为 30000000 币种应为 \"CNY\"",
            ),
            (StepKeyword::Then, "保单未发出失效信号"),
        ],
    );
    assert_steps_registered_in_rstest_bdd(
        "policy_agreement_steps.rs",
        11,
        &[
            (
                StepKeyword::When,
                "为最近保单创建缴费协议 金额 300000 币种 \"CNY\" 账户 \"现金\" 周期 \"yearly\" 起始日期 \"2026-01-01\"",
            ),
            (
                StepKeyword::Then,
                "最近保单第 1 段协议状态应为 \"cancelled\" 每期金额应为 300000",
            ),
        ],
    );
    assert_steps_registered_in_rstest_bdd(
        "policy_stats_steps.rs",
        7,
        &[
            (
                StepKeyword::Then,
                "保单 \"P2026-301\" 累计已缴应为 600000 现金流入应为 50000",
            ),
            (
                StepKeyword::Then,
                "保单 \"P2026-309\" 到期态应为 \"已到期\"",
            ),
        ],
    );
}

/// 报表与仪表盘域四个步骤文件的整文件 / 按需双注册运行时注册（ticket #1504）：
/// `reports_steps.rs` 20 条、`dashboard_steps.rs` 9 条（本票补齐 5 条）、
/// `financial_freedom_steps.rs` 7 条整文件（本票消费的 `budget_steps.rs` 4 条预算
/// 夹具步骤的整文件计数断言，已收敛到 ticket #1506 的 30 条）。
///
/// 占位符语义抽样覆盖 string 剥引号、整数族解析、`f64` 小数，以及本票新引入的
/// 单字提示 `{名:word}`（报表相对年份记号，rstest-bdd 无内置该提示，其未知提示
/// 回退为惰性任意）。删掉任一新注册即红。
#[test]
fn reports_and_dashboard_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "reports_steps.rs",
        20,
        &[
            // 单字提示：无引号的相对年份记号 + 整数 + 剥引号字符串。
            (StepKeyword::Given, "前年有一笔支出 100 到账户 \"现金\""),
            (
                StepKeyword::When,
                "创建交易 类型 \"expense\" 金额 1000 币种 \"USD\" 到账户 \"美元卡\" 日期 \"2026-03-06\" 商户 \"亚马逊\"",
            ),
            (StepKeyword::When, "查询 2026 年商户排行"),
            (
                StepKeyword::When,
                "查询分类份额 期间 \"2026-01-01\" 到 \"2026-12-31\"",
            ),
            (StepKeyword::When, "查询报表日期范围"),
            (StepKeyword::Then, "商户排行第 1 名应为 \"京东\" 金额 1700"),
            (
                StepKeyword::Then,
                "月度汇总第 1 行应为月份 \"2026-01\" 收入 1000 支出 0 退款 0",
            ),
            (StepKeyword::Then, "报表日期范围应为空"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "dashboard_steps.rs",
        9,
        &[
            (StepKeyword::Given, "存在标的 \"NVDA\" 币种 \"USD\""),
            (
                StepKeyword::Given,
                "标的 \"NVDA\" 现价 1500000 币种 \"USD\"",
            ),
            (
                StepKeyword::Given,
                "已买入 标的 \"NVDA\" 数量 2 单价 1000000 到账户 \"美股券商\"",
            ),
            (
                StepKeyword::When,
                "已买入 标的 \"AAPL\" 数量 1 单价 500000 到账户 \"美股券商\"",
            ),
            (StepKeyword::When, "查询净资产总览"),
            (StepKeyword::Then, "非投资账户余额合计应为 244000"),
            (StepKeyword::Then, "持仓市值合计应为 216000"),
            (StepKeyword::Then, "净资产应为 460000"),
            (StepKeyword::Then, "实物资产估值合计应为 100000"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "financial_freedom_steps.rs",
        7,
        &[
            (
                StepKeyword::Given,
                "存在隐藏账户 \"秘密券商\" 类型 \"investment\" 币种 \"CNY\" 初始余额 888000",
            ),
            (StepKeyword::When, "查询财务自由度"),
            (StepKeyword::Then, "自由度分子应为 20000"),
            (StepKeyword::Then, "自由度分母应为 660000"),
            (StepKeyword::Then, "自由度应为 4.3"),
            (StepKeyword::Then, "覆盖年数应为 1.4"),
            (StepKeyword::Then, "本位币应为 \"CNY\""),
        ],
    );
}

/// 参考数据与检索域三个步骤文件的整文件运行时注册（ticket #1505）：
/// `merchants_steps.rs` 18 条、`insurers_steps.rs` 15 条、`search_steps.rs` 16 条。
/// 占位符语义抽样覆盖 string / i64 / usize / f64 与无占位符断言。删掉任一新注册即红。
///
/// 检索消费的迁移与存量数据步骤（`migration_steps.rs` 的重跑批量导入、删除备注交易、
/// 批次结果断言）按需双注册，逐条模式的全等覆盖由静态覆盖守门兜底（ticket #1510）；
/// 本断言只担本票整文件转换的三个步骤文件的运行时那半。
#[test]
fn reference_data_and_search_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "merchants_steps.rs",
        18,
        &[
            (StepKeyword::Given, "存在商户 \"京东\""),
            (StepKeyword::When, "创建商户 \"京东\""),
            (StepKeyword::When, "修改商户 \"京东\" 名称为 \"京东商城\""),
            (
                StepKeyword::When,
                "尝试创建交易 类型 \"buy\" 金额 10000 到账户 \"证券账户\" 日期 \"2026-01-04\" 商户 \"京东\"",
            ),
            (
                StepKeyword::When,
                "创建转账 金额 3000 从账户 \"现金\" 到账户 \"银行\" 日期 \"2026-01-03\" 商户 \"京东\"",
            ),
            (StepKeyword::Then, "商户列表应包含 2 条记录"),
            (StepKeyword::Then, "商户 \"京东\" 关联交易条数应为 3"),
            (StepKeyword::Then, "第 1 条交易商户应为 \"京东\""),
            (StepKeyword::Then, "商户列表响应 JSON 不含字段 \"icon\""),
            (StepKeyword::Then, "商户表应存在且交易表含 merchant_id 列"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "insurers_steps.rs",
        15,
        &[
            (StepKeyword::Given, "存在保司 \"同方全球人寿\""),
            (StepKeyword::When, "创建保司 \"海峡金桥财产保险\""),
            (StepKeyword::When, "按名创建保司 \"  平安人寿  \""),
            (
                StepKeyword::When,
                "尝试修改保司 \"海峡金桥\" 名称为 \"中国人寿\"",
            ),
            (StepKeyword::When, "软删保司 \"同方全球人寿\""),
            (StepKeyword::Then, "保司表应存在"),
            (StepKeyword::Then, "在用保司总数应为 30"),
            (StepKeyword::Then, "保司列表应包含 \"中国人寿\""),
            (StepKeyword::Then, "保司含已删列表应包含 32 条记录"),
            (StepKeyword::Then, "按名创建保司 \"平安人寿\" 应复用已有行"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "search_steps.rs",
        16,
        &[
            (
                StepKeyword::Given,
                "存量交易 备注 \"午餐\" 金额 1000 账户 \"现金\" 日期 \"2026-02-05\"",
            ),
            (
                StepKeyword::Given,
                "存量外币交易 备注 \"美元订阅\" 金额 10000 币种 \"USD\" 本位币 72000 账户 \"美元账户\" 日期 \"2026-02-01\"",
            ),
            (StepKeyword::When, "搜索 \"午餐\""),
            (StepKeyword::When, "搜索 \"午餐\" 第 1 页 每页 20 条"),
            (StepKeyword::When, "搜索 \"午餐\" 金额区间 100 至 2000 分"),
            (
                StepKeyword::When,
                "搜索 \"午餐\" 日期区间 \"2026-02-01\" 至 \"2026-02-28\"",
            ),
            (StepKeyword::When, "搜索金额区间 15.5 至 20.5 元"),
            (
                StepKeyword::When,
                "搜索日期区间 \"2026-02-01\" 至 \"2026-02-28\"",
            ),
            (StepKeyword::Then, "搜索命中 1 条"),
            (StepKeyword::Then, "搜索命中 1 条 总数 5"),
            (StepKeyword::Then, "搜索结果第 1 条备注应为 \"午餐\""),
            (StepKeyword::Then, "搜索结果第 1 条金额应为 1500"),
            (StepKeyword::Then, "搜索结果第 1 条商户应为 \"京东\""),
        ],
    );
}

/// 引导、同步与迁移类步骤文件的整文件运行时注册（ticket #1507）：
/// `backup_steps.rs` 58 条、`data_location_steps.rs` 47 条、
/// `encryption_steps.rs` 48 条（含 `@non-root-only` 守卫步骤）、
/// `startup_failure_steps.rs` 26 条、`books_steps.rs` 15 条、
/// `migration_steps.rs` 18 条、`log_level_steps.rs` 5 条、`sync_steps.rs` 20 条。
/// 占位符语义抽样覆盖 string / 整数族 / f64、字面量花括号与无占位符直命中；
/// 删掉任一新注册即红。
///
/// 本票消费的共享步骤（`budget_steps.rs`、`reports_steps.rs`）已由 #1506 / #1504
/// 纳入整文件运行时断言；本断言只担本票整文件转换的八个步骤文件的运行时那半。
/// 逐条模式的全等覆盖由静态覆盖守门兜底（ticket #1510）。
#[test]
fn bootstrap_sync_migration_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "backup_steps.rs",
        58,
        &[
            (StepKeyword::When, "备份数据库到临时文件"),
            (
                StepKeyword::Then,
                "备份包应包含 \"ledger.db\" 与 \"backup.json\"",
            ),
            (StepKeyword::Then, "自动备份脏标记应为真"),
            (StepKeyword::When, "写入汇率 \"USD\" 兑 \"CNY\" 为 7.2"),
            (StepKeyword::When, "尝试从更高 schema 版本恢复"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "data_location_steps.rs",
        47,
        &[
            (StepKeyword::Given, "默认数据目录中已有一个含 2 条交易的库"),
            (StepKeyword::When, "执行 DataLocation 引导并打开数据库"),
            (StepKeyword::Then, "生效目录应为默认数据目录"),
            (
                StepKeyword::Given,
                "指针文件内容为损坏文本 \"{not valid json\"",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "encryption_steps.rs",
        48,
        &[
            (StepKeyword::Given, "目录只读触发可用"),
            (StepKeyword::When, "用主口令 \"correct horse\" 开启加密"),
            (StepKeyword::Then, "库文件应探测为密文库"),
            (
                StepKeyword::Then,
                "凭主口令 \"correct horse\" 打开当前库应包含 3 条交易且内容完整",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "startup_failure_steps.rs",
        26,
        &[
            (
                StepKeyword::Given,
                "默认数据目录中存在一个头部完好但内容损坏的明文库",
            ),
            (StepKeyword::Given, "目标目录中存在一个损坏的库文件"),
            (StepKeyword::When, "按启动处置流程尝试接管库文件"),
            (
                StepKeyword::When,
                "从备份恢复到启动失败的库位置（无已打开库连接参与）",
            ),
            (
                StepKeyword::Then,
                "启动失败错误码应为 \"boot.schema-drift\"",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "books_steps.rs",
        15,
        &[
            (StepKeyword::When, "新建账本 \"副业账\""),
            (StepKeyword::When, "切换到账本 \"副业账\" 并原位重引导"),
            (StepKeyword::When, "执行引导并打开当前账本"),
            (StepKeyword::When, "查询账本清单"),
            (StepKeyword::Then, "清单应包含 2 个账本"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "migration_steps.rs",
        18,
        &[
            // 数据表步骤：rstest 适配注册必须能命中文本（数据表由 macro 注入）。
            (StepKeyword::When, "批量导入交易"),
            (StepKeyword::When, "重跑刚才的批量导入"),
            (StepKeyword::Then, "读回交易 应包含 4 条记录"),
            (
                StepKeyword::Then,
                "最近一次导入应有 1 条去重跳过 1 条新写入",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "log_level_steps.rs",
        5,
        &[
            (StepKeyword::Then, "持久化日志档位应为 \"info\""),
            (StepKeyword::When, "写入持久化日志档位 \"debug\""),
            (
                StepKeyword::Then,
                "应返回错误码 \"settings.log-level-invalid\"",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "sync_steps.rs",
        20,
        &[
            (
                StepKeyword::Given,
                "以当前账本配置同步通道 空间 \"default\"",
            ),
            (StepKeyword::When, "打开应用即同步一轮"),
            (StepKeyword::Then, "通道上应有本机账本目录"),
            (StepKeyword::Then, "本端应已应用对端操作"),
        ],
    );
}

/// 定时计划与预算域的整文件运行时注册（ticket #1506）：`scheduled_steps/` 七个
/// 步骤文件（auto_run 5 / create 8 / merchant 9 / occurrence 14 / plan_detail 8 /
/// plan_edit 10 / spend 13）与 `budget_steps.rs` 30 条。占位符语义抽样覆盖 string
/// 剥引号、整数族、`usize`、`f64` 小数与无占位符直命中；「只留 cucumber 注册」
/// （删掉任一新注册）即红。
#[test]
fn scheduled_and_budget_steps_are_registered_in_rstest_bdd() {
    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/auto_run.rs",
        5,
        &[
            (StepKeyword::When, "以 \"2026-02-20\" 为今日执行自动追补"),
            (
                StepKeyword::When,
                "自动执行关闭时以 \"2026-03-20\" 为今日执行追补",
            ),
            (StepKeyword::Then, "追补汇总应为 到期 2 成功 1 失败 1"),
            (
                StepKeyword::Then,
                "最近计划生成的交易日期应依次为 \"2026-01-15,2026-02-15\"",
            ),
            (
                StepKeyword::Then,
                "备注为 \"缺汇率订阅\" 的计划状态为 \"failed\" 的期次应有 1 条",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/create.rs",
        8,
        &[
            (
                StepKeyword::When,
                "创建订阅计划 金额 10000 币种 \"USD\" 账户 \"美股订阅\" 起始日期 \"2026-01-15\" 备注 \"国际订阅\"",
            ),
            (
                StepKeyword::When,
                "创建分期计划 总额 3100 期数 3 账户 \"分期账户\" 起始日期 \"2026-01-15\"",
            ),
            (
                StepKeyword::When,
                "创建定时转账计划 金额 50000 从 \"工资卡\" 到 \"活期储蓄\" 起始日期 \"2026-01-15\"",
            ),
            (
                StepKeyword::When,
                "尝试创建定时转账计划 金额 5000 从 \"人民币账户\" 到 \"美元账户\" 期数 1 起始日期 \"2026-01-15\"",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/merchant.rs",
        9,
        &[
            (
                StepKeyword::When,
                "创建订阅计划 金额 3000 币种 \"CNY\" 账户 \"订阅账户\" 起始日期 \"2026-01-15\" 备注 \"会员\" 商户 \"视频平台\"",
            ),
            (
                StepKeyword::When,
                "尝试创建定时转账计划 金额 5000 从 \"账户A\" 到 \"账户B\" 期数 3 起始日期 \"2026-01-15\" 商户 \"京东\"",
            ),
            (StepKeyword::Then, "该期次交易商户应为 \"视频平台\""),
            (
                StepKeyword::Then,
                "计划扩展表应含 merchant_id 列且无 counterparty 列",
            ),
            (StepKeyword::Then, "第 1 笔计划交易商户应为 \"商户A\""),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/occurrence.rs",
        14,
        &[
            (StepKeyword::Given, "存在汇率 \"USD\" 兑 \"CNY\" 为 7.2"),
            (StepKeyword::When, "依次执行全部期次"),
            (StepKeyword::When, "注入交易落库失败触发器"),
            (StepKeyword::Then, "执行应失败并提示 \"汇率\""),
            (StepKeyword::Then, "期次未回填交易"),
            (
                StepKeyword::Then,
                "该期次交易类型应为 \"expense\" 金额应为 10000",
            ),
            (StepKeyword::Then, "该期次交易本位币金额应为 72000"),
            (
                StepKeyword::Then,
                "应生成 3 笔类型 \"expense\" 的交易 金额依次为 \"1033,1033,1034\"",
            ),
            (StepKeyword::Then, "计划状态应为 \"completed\""),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/plan_detail.rs",
        8,
        &[
            (StepKeyword::When, "将最近计划最早的一条待执行期次置为失败"),
            (StepKeyword::When, "查询该计划详情"),
            (StepKeyword::Then, "详情应含 10 条待执行期次"),
            (StepKeyword::Then, "详情期次总数应为 12"),
            (StepKeyword::Then, "详情状态为 \"failed\" 的期次应有 1 条"),
            (
                StepKeyword::Then,
                "详情状态为 \"failed\" 的期次日期应为 \"2026-02-15\"",
            ),
            (StepKeyword::When, "重试该失败期次"),
            (StepKeyword::When, "展开该计划期次"),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/plan_edit.rs",
        10,
        &[
            (
                StepKeyword::When,
                "编辑该订阅计划 备注 \"音乐会员\" 分类 \"软件服务\"",
            ),
            (StepKeyword::When, "编辑该订阅计划 商户 \"商户B\""),
            (
                StepKeyword::When,
                "编辑该订阅计划 备注 \"换户订阅\" 分类 \"软件服务\" 账户 \"新账户\"",
            ),
            (StepKeyword::When, "携带金额 5000 编辑该订阅计划"),
            (
                StepKeyword::Then,
                "编辑应失败并提示 \"改价 = 取消旧计划 + 新建\"",
            ),
            (StepKeyword::Then, "第 2 笔计划交易账户应为 \"新账户\""),
            (StepKeyword::Then, "该计划扣款账户应为 \"新账户\""),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/spend.rs",
        13,
        &[
            (
                StepKeyword::When,
                "创建订阅计划 金额 3000 币种 \"CNY\" 账户 \"订阅账户\" 周期 \"monthly\" 起始日期 \"2026-01-15\" 备注 \"视频会员\"",
            ),
            (StepKeyword::When, "执行该计划前 2 期"),
            (StepKeyword::When, "以 \"2026-03-20\" 为今日查询订阅花费"),
            (
                StepKeyword::Then,
                "近 12 个月中 \"2026-01\" 实际花费应为 3000",
            ),
            (StepKeyword::Then, "折算月成本应为 7000"),
            (StepKeyword::Then, "订阅花费行数应为 1"),
            (
                StepKeyword::Then,
                "订阅行 \"已退订服务\" 状态应为 \"cancelled\"",
            ),
            (
                StepKeyword::Then,
                "订阅行 \"视频会员\" 本年实际花费应为 6000",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "budget_steps.rs",
        30,
        &[
            (StepKeyword::Given, "存在支出分类 \"午餐\""),
            (StepKeyword::Given, "存在支出分类 \"奶茶\" 属于 \"餐饮总\""),
            (StepKeyword::Given, "存在收入分类 \"工资\""),
            (StepKeyword::Given, "为分类 \"午餐\" 创建月预算 金额 50000"),
            (
                StepKeyword::Given,
                "为分类 \"年度订阅\" 创建年预算 金额 60000",
            ),
            (
                StepKeyword::Given,
                "存量预算 分类 \"旧账分类\" 周期 \"monthly\" 金额 50000 开始日期 \"2020-01-15\"",
            ),
            (
                StepKeyword::Given,
                "分类 \"午餐\" 本月有一笔支出 2000 到账户 \"现金\"",
            ),
            (
                StepKeyword::When,
                "通过预算命令为分类 \"文具\" 创建 \"monthly\" 预算 金额 0",
            ),
            (StepKeyword::When, "查询预算进度"),
            (StepKeyword::When, "上一笔支出本月收到退款 300"),
            (StepKeyword::Then, "分类 \"午餐\" 的预算进度应为 2000"),
            (StepKeyword::Then, "分类 \"餐饮总\" 的预算应超支"),
            (StepKeyword::Then, "分类 \"早餐\" 的预算不应超支"),
            (StepKeyword::Then, "创建应失败并提示 \"预算金额必须为正数\""),
            (StepKeyword::Then, "编辑预算应成功"),
            (StepKeyword::Then, "分类 \"文具\" 的预算行数应为 0"),
            (StepKeyword::Then, "分类 \"日用\" 的预算金额仍应为 30000"),
        ],
    );
}
