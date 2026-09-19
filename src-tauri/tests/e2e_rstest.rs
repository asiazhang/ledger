//! BDD e2e 目标（rstest-bdd 通道，spec #1494 / ticket #1495）：把 `.feature` 场景
//! 绑成标准测试运行器里的**独立测试**——`scenarios!` 自动发现场景，测试世界以
//! rstest fixture 单点提供，场景失败由测试运行器置红（不再依赖自建 runner）。
//!
//! 本目标是增量迁移的**新通道**，与旧 cucumber 目标（`tests/e2e.rs`，
//! `harness = false`）并存：
//! - 已迁入的 feature（accounts.feature，#1495；transactions_write.feature，#1497；
//!   transactions_edit.feature / transactions_query.feature，#1498；
//!   transactions_policy.feature，#1499）在本目标全绿，旧目标行为零变化；
//! - 已迁入域消费的步骤函数改为**双注册**（同一函数同时挂 cucumber 与 rstest-bdd
//!   属性宏），函数体与断言唯一，不复制；数据表步骤因两种 macro 的入参形态不同，
//!   抽共享实现 + 两侧注册适配器（`migration_steps::批量导入交易`、
//!   `transactions_policy_steps::批量导入挂单交易`），适配形态见
//!   `docs/verification/1499-transactions-policy-dual-registration.md` 与
//!   `docs/verification/1498-transactions-edit-query-dual-registration.md`；
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
// （world / common / step_inputs / step_verbs）按整文件并入，消费者却是已迁移的
// 步骤域子集——未并入的域在本目标里暂时无人调用。逐域迁移完成后本目标即全量目标，
// 该 allow 随最后一个域并入一并删除（届时 `-D warnings` 重新覆盖这些模块）。
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
#[path = "e2e/policies_steps.rs"]
mod policies_steps;
#[allow(dead_code)]
#[path = "e2e/step_inputs.rs"]
mod step_inputs;
#[allow(dead_code)]
#[path = "e2e/step_verbs.rs"]
mod step_verbs;
#[allow(dead_code)]
#[path = "e2e/transactions_edit_steps.rs"]
mod transactions_edit_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_policy_steps.rs"]
mod transactions_policy_steps;
#[allow(dead_code)]
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;

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
