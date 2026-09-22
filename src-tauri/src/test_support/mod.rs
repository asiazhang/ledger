//! 统一测试数据库工厂与共享断言库（spec #728 / issue #751 / ADR-0084）。
//!
//! 建库与种子知识的唯一入口：`open()` 吸收 `db::open_in_memory()` + `db::init_db()`
//! 两行序（外键 + perf hook + 迁移 + 种子）；`seed::*` 吸收跨域重复的实体列清单与
//! 投资铺垫组合；`assert::*` 吸收「缓存回填 == 实时计算」不变式对拍（ADR-0067 的
//! 测试保障面收归唯一维护点）与通用白盒行读取器。以 `&Connection` 为中心的自由
//! 函数集，不引入包装类型与 builder 链（ADR-0084 决策 3）。
//!
//! **准入规则**（ADR-0084 决策 1）：工厂只收跨 ≥2 域重复的夹具；单域特有的种子、
//! 输入构造器与读取器留域薄皮（如 `make_input` 等域语义构造器）。守门豁免清单
//! 即此规则的机器面（守门票 #752）。
//!
//! **可见性**（ADR-0084 决策 2）：`pub mod` + `#[doc(hidden)]`，先例 `test_utils.rs`
//! ——外部集成测试（`tests/api_server/`）链接非 `#[cfg(test)]` 构建的 lib，看不到
//! `#[cfg(test)]` 模块，这是仓库已验证的共享机制；`#[doc(hidden)]` 使其不进入文档。
//!
//! **固定时刻**（ADR-0084 决策 5）：[`FIXED_NOW`] 是夹具簿记戳（created_at/updated_at
//! 等非行为输入列）的统一发放值，种子内部发放、调用点零字面量；域时刻（价格/汇率
//! 序列点、预算窗口、定时触发点等测试行为输入）不吸收——需要时间推进的种子参数
//! 显式收时刻。守门票（#752）禁 `FIXED_NOW` 值的字面量出现在工厂之外；禁令清单
//! 自本文件 `pub const` 字符串常量闭集提取（#1468）——本文件即固定时刻登记处，
//! 非固定时刻的字符串常量不住此处。
//!
//! **本版收编清单**（吸收体 → 工厂成员，供按域迁移票 #753–#757 核对）：
//!
//! - 建库两行序（transaction/investment/reports/policy/merchants/item/sync/
//!   scheduled_transactions/db 等域薄皮与 `tests/api_server/common.rs` 逐字重复）→ [`open`]；
//! - `transaction/tests/common.rs` 与 `investment/tests/common.rs` 的 `insert_account`
//!   同体函数 → [`seed_account`]（全位置归一签名，ADR-0084 决策 4）；
//! - `db/tests/common.rs` 与 `investment/tests/common.rs` 的 `insert_instrument` 同体
//!   函数 → [`seed_instrument`]；
//! - `db/tests/common.rs`、`investment/tests/trend.rs` 的 price/fx 周采样插入同体函数
//!   → [`seed_price_history`] / [`seed_fx_rate_history`]；
//! - `investment/tests/common.rs` 的 `insert_rate_1_1` / `insert_rate` 及 transaction、
//!   api_server 各处 `exchange_rates` 同形状插入 → [`seed_exchange_rate`]；
//! - `transaction/tests/common.rs`（及 batch_create.rs 逐字副本）的
//!   `setup_investment_account` 与 `tests/api_server/investment_migration.rs` 的
//!   `seed_investment_account` → [`seed_investment_setup`]；
//! - `transaction/tests/balance_cache.rs` 与 `db/tests/balance_cache.rs` 各自维护的
//!   「回填 == 实时」对拍断言 → [`assert_balance_cache_matches_realtime`]（本票唯一
//!   的既有调用改动：两域旧断言体删除改调共享版本）。
//!
//! **#956 追加**（通道线格式替身）：命令面集成测试与 BDD 步骤层各自手工复制的
//! 「构造合法通道段 + 归并 manifest」（含各自的 `sha256_hex`）→
//! [`publish_raw_segment`]（`test_support::channel`）。共享物是**通道线格式契约**
//! （字节级成帧），不是建库/种子/默认值集，ADR-0086 决策 9 不破；由此新增一条
//! `test_support → sync_engine` 测试专用边（登记处：ADR-0084 迁移状态段），
//! 该边由测试豁免路径消费、不进产品依赖图。
//!
//! **#1433 追加**（源码扫描掩码器具）：守门家族三处同型的词法掩码与四处手写
//! 花括号配对上收 [`scan`] 单一维护点（`mask_non_code` + `matching_brace_end`），
//! 壳层信号/连接槽/同步触发/行情接缝守门、命令面扫描测试与 market-sync 车道
//! 守门共用；与 TS 侧 `check-structure.ts::maskNonCode` 双源登记，共享语料
//! 夹具防漂移。登记处：ADR-0084 修订注记、CONTEXT-testing 词条。
//!
//! **#1645 追加**（测试暂存目录 guard）：Rust 测试「只建不删」的真临时目录
//! 夹具（散在 5 个编译单元约 70 处）收编 [`scratch`]——`ScratchDir`（目录）与
//! `ScratchFile`（散文件目录化形态），构造即建 `ledger-test-{tag}-{uuid}/`，
//! drop（含 panic unwind）整棵删除，手写 remove_dir_all/cleanup 收尾随迁移
//! 退役；前缀归一 `ledger-test-`，残留可一条 rm 识别清理。infra / backup /
//! sync-engine / ledger-perf 经既有 dev-dependency 环消费，无新依赖边。
//! 登记处：ADR-0084 修订注记、CONTEXT-testing「测试暂存目录」。
//!
//! **#1699 追加**（读快照探针 + 文件库建库）：多语句读闭包「写提交落在两语句
//! 之间」的确定性注入器具收编 [`snapshot_probe`]——`trace_v2` STMT 回调在目标
//! 语句开始前于另一连接原子提交注入写，供交易 / 投资 / 壳层三域七处探针测试共用
//! （决策 1 准入：≥2 域同体消费）；配套 [`open_file`]（文件库建库，内存库按
//! 连接隔离撑不起双连接现场）。登记处：
//! ADR-0084 修订注记、CONTEXT-testing「读快照探针」词条。
//!
//! 说明：集成测试 `tests/api_server/` 链接的是非 `#[cfg(test)]` 构建的 lib，
//! 因此本模块不能仅以 `#[cfg(test)]` 编译；对生产二进制的影响只是一些未使用的
//! 测试辅助函数（可被编译器消除）。
//!
// C 类豁免（ADR-0060）：仅测试用——本模块被集成测试以非 cfg(test) 构建链接，
// 无法经 crate 根 cfg(test) 豁免覆盖，故文件级放行六件套；生产路径不得消费本模块。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

mod assert;
pub mod channel;
pub mod s3;
pub mod scan;
pub mod scratch;
mod seed;
pub mod snapshot_probe;
#[cfg(test)]
mod tests;

pub use assert::{
    assert_balance_cache_matches_realtime, extract_check_in_literals, read_scalar_i64,
};
pub use channel::publish_raw_segment;
pub use s3::{
    S3Addressing, S3Deny, S3Gate, S3ObservedRequest, S3Stub, S3StubConfig, spawn_s3_stub,
};
pub use scratch::{ScratchDir, ScratchFile};
pub use seed::{
    seed_account, seed_exchange_rate, seed_exchange_rate_with_source, seed_fx_history_weeks,
    seed_fx_rate_history, seed_instrument, seed_investment_setup, seed_price_history,
};

use std::future::Future;
use std::path::Path;

use rusqlite::Connection;

/// 夹具簿记戳的统一固定时刻（ADR-0084 决策 5）：种子的 created_at/updated_at 等
/// 非行为输入列由工厂内部发放此值，调用点不再出现默认时刻字面量；域时刻是测试的
/// 行为输入，经种子参数显式传入、默认值引用本常量。
pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

/// 同步测试驱动 async 域接缝的唯一入口（ADR-0125 决策 5/7）：域编排 async 化后
///（issue #1412/#1413），同步 `#[test]` 经全局运行时（tauri::async_runtime）把
/// async 调用驱动到完成——先用例 investment 域的 `add_fund_by_code_with` /
/// `fetch_stock_quote_for_add`（issue #1413）。跨 ≥2 域复用（investment /
/// sync-engine），按准入规则收编；测试线程不在任何运行时上下文内，驱动安全。
pub fn block_on<F: Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

/// 零配置打开已初始化的内存测试库：`db::open_in_memory()`（外键 + perf hook）+
/// `db::init_db()`（迁移 + 默认种子）两行序的唯一承载（ADR-0084 决策 3：建库的
/// 全部现状就是内存库 + 迁移，无配置项）。加密是 BDD 场景，不入本工厂；文件库
/// 建库见 [`open_file`]（issue #1699 读快照探针的双连接现场）。
///
/// 建库经 [`ledger_infra::db::open_in_memory_initialized`]：工厂仍是唯一的建库入口，
/// 但迁移链不再逐用例重放，改由进程内固定的模板产物还原（spec #1086 / issue #1514，
/// 口径与独立性不变，见该函数文档）。接口形态（零配置 `open() -> Connection`）与
/// 全部既有调用点不变。
pub fn open() -> Connection {
    install_test_wiring();
    ledger_infra::db::open_in_memory_initialized().expect("打开并初始化内存测试库")
}

/// 打开已初始化的**文件库**测试连接（issue #1699 读快照探针）：建库知识与接线
/// 与 [`open`] 共享同两处单点（[`install_test_wiring`] + 基础设施建连入口），只把
/// 库形态换成文件库（`db::open_connection_in`：迁移 + 种子 + 建连收尾）。
///
/// 用途边界：**双连接现场**（跨连接的读快照一致性探针）必须用文件库——内存库按
/// 连接隔离，第二连接是另一个空库；单连接用例仍走 [`open`]（模板还原更快）。
pub fn open_file(db_dir: &Path) -> Connection {
    install_test_wiring();
    ledger_infra::db::open_connection_in(db_dir).expect("打开并初始化文件测试库")
}

/// 测试接线单点（[`open`] / [`open_file`] 共享；测试库与生产同形，幂等、先装者
/// 优先）：
/// - 提交点后置动作（spec #1086 / issue #1088）：连接层写入口的副作用实现由域侧
///   提供，建库单点负责注册；
/// - 写后即时同步（#1089）：op 产出单点在协议 crate，响应闭包（去抖合流）由同步域
///   提供；调度未拉起时信号投递仍是零动作；
/// - 写路径副作用接缝（issue #1090 / #1091）：余额刷新实现由账户域、计划来源解析
///   实现由定时计划域提供；期次落账置脏（#1090）实现由备份域（#1091 起为
///   `ledger-backup` crate）提供、定时计划域注册点对装，追补触发（#1091 挂载点④）
///   实现由定时计划域提供、备份域注册点对装——两个域互相零直接依赖，接线都在本单点；
/// - 交易域接缝（issue #1092 / #1180）：六向实现经组合入口一次装入。
fn install_test_wiring() {
    ledger_backup::install_after_commit_hook();
    ledger_sync_engine::trigger::install_after_write_hook();
    ledger_accounts::balance::install_balance_refresh_hook();
    ledger_scheduled::install_plan_source_hook();
    ledger_scheduled::auto_run::register_after_occurrence_hook(
        ledger_backup::occurrence_dirty_hook,
    );
    ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
    crate::transaction_wiring::install_all();
}
