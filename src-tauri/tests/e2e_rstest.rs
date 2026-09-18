//! BDD e2e 目标（rstest-bdd 通道，spec #1494 / ticket #1495）：把 `.feature` 场景
//! 绑成标准测试运行器里的**独立测试**——`scenarios!` 自动发现场景，测试世界以
//! rstest fixture 单点提供，场景失败由测试运行器置红（不再依赖自建 runner）。
//!
//! 本目标是增量迁移的**新通道**，与旧 cucumber 目标（`tests/e2e.rs`，
//! `harness = false`）并存：
//! - 账户域全部场景（accounts.feature）在本目标全绿，旧目标行为零变化；
//! - 账户域消费的步骤函数改为**双注册**（同一函数同时挂 cucumber 与 rstest-bdd
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
#[path = "e2e/step_inputs.rs"]
mod step_inputs;
#[allow(dead_code)]
#[path = "e2e/step_verbs.rs"]
mod step_verbs;
#[allow(dead_code)]
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;

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
}
