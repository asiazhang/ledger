//! BDD e2e 目标（rstest-bdd 通道，spec #1494 / ticket #1495）：把 `.feature` 场景
//! 绑成标准测试运行器里的**独立测试**——`scenarios!` 自动发现场景，测试世界以
//! rstest fixture 单点提供，场景失败由测试运行器置红（不再依赖自建 runner）。
//!
//! 本目标是增量迁移的**新通道**，与旧 cucumber 目标（`tests/e2e.rs`，
//! `harness = false`）并存：
//! - 已迁移域的全部场景在本目标全绿，旧目标行为零变化。当前已绑域：账户
//!   （#1495）、物品与实物资产（#1500，items_* / physical_asset*）；
//! - 已迁移域消费的步骤函数改为**双注册**（同一函数同时挂 cucumber 与 rstest-bdd
//!   属性宏），函数体与断言唯一，不复制；
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
#[path = "e2e/physical_asset_disposal_steps.rs"]
mod physical_asset_disposal_steps;
#[allow(dead_code)]
#[path = "e2e/physical_asset_updates_steps.rs"]
mod physical_asset_updates_steps;
#[allow(dead_code)]
#[path = "e2e/physical_assets_steps.rs"]
mod physical_assets_steps;
#[allow(dead_code)]
#[path = "e2e/step_inputs.rs"]
mod step_inputs;
#[allow(dead_code)]
#[path = "e2e/step_verbs.rs"]
mod step_verbs;
#[allow(dead_code)]
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;
// 物品与实物资产域消费的共享步骤文件：汇率夹具 `存在汇率 X 兑 Y 为 R` 住
// `scheduled_steps/occurrence.rs`（跨域共享），故按整模块并入其父模块——本票只
// 为被消费的那一条步骤补 rstest-bdd 注册，其余定时计划步骤归 ticket #1506。
#[allow(dead_code)]
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;

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
}

/// 运行时注册表断言（ticket #1497 AC1/AC3）：`transactions_write_steps.rs` 整文件
/// 22 条步骤（Given 1 + When 12 + Then 9）都已进入新目标的 rstest-bdd 注册表，
/// 且改写后的占位符能按真实语义匹配真实场景文本（`{<名>:string}` 剥引号、
/// `{<名>:i64}` 解析整数）。「只留 cucumber 注册」（删掉任一新注册）即红。
///
/// 逐条模式的全等覆盖（feature 步骤行 ↔ 注册模式、无漏改 / 无歧义）由静态覆盖
/// 守门兜底（`bun scripts/check-e2e-step-coverage.ts`，ticket #1510 / AC2）；
/// 本断言只担运行时那半：宏确实把注册发进了目标注册表、且查找能命中。
#[test]
fn transactions_write_steps_are_registered_in_rstest_bdd() {
    use rstest_bdd::{Step, StepKeyword, StepText, find_step_with_metadata, iter};

    let registered: Vec<_> = iter::<Step>
        .into_iter()
        .filter(|step| step.file.ends_with("transactions_write_steps.rs"))
        .collect();
    assert_eq!(
        registered.len(),
        22,
        "本文件在新目标的注册条数不符（缺注册或多注册）：{:#?}",
        registered
            .iter()
            .map(|step| step.pattern.as_str())
            .collect::<Vec<_>>()
    );

    // 占位符语义抽样：string 剥引号、i64 解析整数、可选后缀（备注）与无占位符。
    let samples = [
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
    ];
    for (keyword, text) in samples {
        let found = find_step_with_metadata(keyword, StepText::from(text))
            .unwrap_or_else(|| panic!("新目标未匹配到步骤文本：{text}"));
        assert!(
            found.file.ends_with("transactions_write_steps.rs"),
            "步骤文本 {text} 未命中本文件的注册：{}",
            found.file
        );
    }
}
