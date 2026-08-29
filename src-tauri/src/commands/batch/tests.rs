//! `TransactionBatch` 模块的单元测试：断言**外部行为**——`run` 的返回值
//! （success / duplicate / id / error）与实际落库行数/内容；不断言内部实现
//! （事务 BEGIN/COMMIT 写法、SQL 字符串、去重分支结构）。
//!
//! 原命令模块中批量写入/`compute_dedup_hash` 相关测试随重构迁入本模块（issue #53 / #63 / #66），改用
//! `TransactionBatch::run` 断言外部行为；`transactions` 模块遗留的旧 `batch_*`
//! 直调 `create_transaction_internal` 测试（全部有效落库/转账缺目标账户/零金额）已随
//! #66 处理——零金额校验迁入本模块以 `run` 外部行为覆盖，其余被本模块既有
//! 测试与 `transaction::writer` 模块测试共同取代（通用 kind 归一化语义已收口到
//! Writer 接缝）。单条写入
//! （`create_transaction_internal`）与删除/修改（`delete_transaction_internal`/
//! `update_transaction_internal`）的测试仍留在 `transactions` 模块。
//!
//! #259 按行为主题拆为子模块（纯移动）：
//! - `dedup`：内容哈希、去重身份判定（`dedup_identity`）与槽位联动；
//! - `batch_create`：批量写入、幂等键语义与批次汇总日志。

mod batch_create;
mod common;
mod dedup;
