//! `db` 模块的单元测试，按行为主题拆为子模块（#260，纯移动）：
//! - `migrations`：迁移集合自校验、`init_db` 幂等与种子、schema 约束
//!   （含 V010 price/fx history 周唯一约束与旧备份升级路径）；
//! - `balance_cache`：V017 余额缓存迁移回填 == 实时计算（issue #491）；
//! - `search_cache`：进程内搜索候选缓存快照/失效/连接身份（issue #493）；
//! - `holding`：净值视图 `v_holdings` 折算语义与交易本位币折算；
//! - `perf`：耗时分级边界、perf trace 接线（ADR-0009）与聚合覆盖索引；
//! - `dirty_marker`：连接层统一写入口 `db::write` 置脏语义（ADR-0032）。

mod balance_cache;
mod common;
mod dirty_marker;
mod holding;
mod migrations;
mod perf;
mod search_cache;
