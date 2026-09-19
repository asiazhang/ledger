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
//!   transactions_source.feature，#1503）在本目标运行，旧目标行为零变化。
//!   账户 / 交易 / 保单、物品与投资域场景全绿；
//!   实物资产域 3 个场景受 #1489 既有缺陷（同毫秒 UUID v7 排序不确定）影响，
//!   间歇性红，按 #1500 约定不修；
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
// （scheduled_steps，为跨域汇率夹具而并入，ticket #1500）按整文件并入，消费者却是
// 已迁移的步骤域子集——未迁移的步骤在本目标里暂时无人调用。逐域迁移完成后本目标即
// 全量目标，该 allow 随最后一个域并入一并删除（届时 `-D warnings` 重新覆盖这些模块）。
#[allow(dead_code)]
#[macro_use]
#[path = "e2e/world.rs"]
mod world;

#[allow(dead_code)]
#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;
#[allow(dead_code)]
#[path = "e2e/common.rs"]
mod common;
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
// 物品与实物资产域（#1500）与保单域（#1501）消费的共享步骤文件：汇率夹具
// `存在汇率 X 兑 Y 为 R` 与保单协议期次步骤（执行该计划第一期等）均住
// `scheduled_steps/`，故按整模块并入其父模块——两票只为各自被消费的步骤补
// rstest-bdd 注册，其余定时计划步骤归 ticket #1506。
#[allow(dead_code)]
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;
#[allow(dead_code)]
#[path = "e2e/search_steps.rs"]
mod search_steps;

#[allow(dead_code)]
#[path = "e2e/categories_steps.rs"]
mod categories_steps;
#[allow(dead_code)]
#[path = "e2e/dashboard_steps.rs"]
mod dashboard_steps;
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

    let registered: Vec<_> = iter::<Step>
        .into_iter()
        .filter(|step| step.file.ends_with(file))
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
            found.file.ends_with(file),
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
/// 整文件注册；`scheduled_steps/create.rs` 按需 4 条计划创建步骤与
/// `search_steps.rs` 1 条搜索步骤。占位符语义覆盖 string / i64 / usize、f64 小数
/// 与无占位符直命中；删掉任一新注册即红。
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

    assert_steps_registered_in_rstest_bdd(
        "scheduled_steps/create.rs",
        4,
        &[
            (
                StepKeyword::When,
                "创建订阅计划 金额 800 币种 \"CNY\" 账户 \"现金\" 起始日期 \"2026-02-01\"",
            ),
            (
                StepKeyword::When,
                "创建订阅计划 金额 3000 币种 \"CNY\" 账户 \"现金\" 起始日期 \"2026-02-01\" 备注 \"视频会员\"",
            ),
            (
                StepKeyword::When,
                "创建分期计划 总额 12000 期数 6 账户 \"现金\" 起始日期 \"2026-02-01\" 备注 \"笔记本分期\"",
            ),
            (
                StepKeyword::When,
                "创建定时转账计划 金额 2000 从 \"工资户\" 到 \"储蓄户\" 期数 12 起始日期 \"2026-02-01\" 备注 \"月度储蓄\"",
            ),
        ],
    );

    assert_steps_registered_in_rstest_bdd(
        "search_steps.rs",
        1,
        &[(StepKeyword::When, "搜索 \"缴费专户\"")],
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
