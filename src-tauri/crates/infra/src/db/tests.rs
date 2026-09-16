//! `db` 模块的单元测试，按行为主题拆为子模块（#260，纯移动）：
//! - `migrations`：迁移集合自校验、`init_db` 幂等与种子、schema 约束
//!   （含 V010 price/fx history 周唯一约束）；
//! - `holding`：净值视图 `v_holdings` 折算语义与交易本位币折算；
//! - `perf`：耗时分级边界、perf trace 接线（ADR-0009）与聚合覆盖索引；
//! - `dirty_marker`：连接层统一写入口 `db::write` 置脏语义（ADR-0032）；
//! - `run_db`：统一 DB 调用 helper `db::run_db`（形状乙，spec #498 / #501）；
//! - `schema_guard`：schema 漂移守卫机制（内存参照库方向性 diff，
//!   issue #992 / ADR-0100）；
//! - `integrity`：完整性检查失败的码契约（ADR-0050 收口，#1072）；
//! - `tx_scope`：事务作用域原语 `db::tx_scope::{ensure_transaction, hold_transaction}`
//!   （嵌套感知「保证处于事务中」与无条件自持事务壳，ADR-0033 决策 2 /
//!   issue #1013、#1014）；
//! - `lock_probe`：持锁时长探针（issue #1276 守门③）：超阈值记 warn、不静默。
//! - `readonly`：只读连接与成对 DbState 读槽（读路径独立只读连接，
//!   issue #1280 / ADR-0117）。
//! - `facade`：异步 DB 门面（写读两线程、作业通道、panic 回滚与连接不可信语义，
//!   ADR-0125 决策 1–3 / issue #1408；换连承接——成对换连在门面下成立、换连后
//!   新连接重置不可信标记，同 ADR 决策 2 / issue #1409）。

mod common;
mod dirty_marker;
mod facade;
mod holding;
mod integrity;
mod lock_probe;
mod migrations;
mod perf;
mod readonly;
mod run_db;
mod schema_guard;
mod tx_scope;
