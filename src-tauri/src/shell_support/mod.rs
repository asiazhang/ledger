//! 壳层机制（spec #1086 P5 / issue #1108 壳层收敛）：只被壳层消费的机制
//! 收进本显式分组，正住址即壳层根包。
//!
//! 住址沿革：四模块曾随基础设施归位暂住 `ledger-infra::shell_support`
//! （ADR-0111 决策 2 / #1130），#1108 起迁回壳层正住址——基础设施不再承载
//! 任何只被壳层消费的机制，本组不得反向被基础设施或域引用。
//!
//! 成员（四个，全部只被壳层消费）：
//! - [`read_entry`]：壳层统一读入口（ADR-0104，spec #1009 批次①）；
//! - [`write_entry`]：壳层统一写入口（ADR-0073，spec #523）；
//! - [`redact`]：IPC 载荷脱敏（ADR-0075 后果条款，issue #1087 首位成员）；
//! - [`logger`]：日志初始化、滚动清理与运行期档位接管（spec #608 / #611）。
//!
//! 依赖口径：四模块消费的基础设施机制（`db` / `error` / `events` / `signals` /
//! `settings` / `test_utils`）直接以 `ledger_infra::…` 引用——壳层对基础设施的
//! 合法依赖（壳 → 基础设施，ADR-0056），不再经根包再导出中转。
//! 接缝契约——发射时序、锁失败归一化、`Outcome` 证据形态、组合语义
//! （ADR-0073 / ADR-0104）——逐条不变。
//!
//! 分组边界：失效信号投递机制（`ledger_infra::events`）与 `DbState` 的消费方
//! 跨出壳层（备份域、同步域），**不在本组**，留在基础设施共享接缝。

pub mod logger;
pub mod read_entry;
pub mod redact;
pub mod write_entry;
