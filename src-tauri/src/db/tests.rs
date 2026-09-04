//! `db` 模块的单元测试，按行为主题拆为子模块（#260，纯移动）：
//! - `migrations`：迁移集合自校验、`init_db` 幂等与种子、schema 约束
//!   （含 V010 price/fx history 周唯一约束与旧备份升级路径）；
//! - `balance_cache`：V017 余额缓存迁移回填 == 实时计算（issue #491）；
//! - `holding`：净值视图 `v_holdings` 折算语义与交易本位币折算；
//! - `perf`：耗时分级边界、perf trace 接线（ADR-0009）与聚合覆盖索引；
//! - `dirty_marker`：连接层统一写入口 `db::write` 置脏语义（ADR-0032）；
//! - `run_db`：统一 DB 调用 helper `db::run_db`（形状乙，spec #498 / #501）；
//! - `encryption`：SQLCipher 引擎基座（issue #569 / ADR-0075）——依赖切换
//!   不变量（未设密钥保持明文）、建连密钥缝、文件头探测三态。

mod balance_cache;
mod common;
mod dirty_marker;
mod encryption;
mod holding;
mod migrations;
mod perf;
mod run_db;
