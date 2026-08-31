//! 投资域判定谓词薄模块（issue #239）：只收「持仓标的」判定谓词一个常量，
//! 不放函数、不放查询——消费方各自拼装自己的 SQL。
//!
//! 「持仓标的」（InvestedInstrument，ADR-0015 决策 1）的判定谓词单点定义于此，
//! 同一口径驱动四处：`list_instruments` 的 `invested` 派生列、标的页「只看持仓」
//! 过滤（`only_invested`）、增量同步（`sync_holding_prices`）的标的收集与统计、
//! 盈亏页持仓概览。未来任何「持仓标的」消费方（如定投提醒）只需复用本常量。
//!
//! 与 `v_holdings` 视图（迁移内的第二份 SQL 编码，已随发布冻结、只增不改）的
//! 一致性由绑定测试钉住（本域 `tests/predicates.rs`，先例：周键 ↔ week_start
//! 生成列绑定测试），不靠注释声明。
//!
//! 与 as-of 持仓推算接缝（`holdings.rs`）刻意不合并为「投资域口径模块」：
//! SQL 谓词片段与 Rust 纯函数的消费方、测试方式、变更节奏不同
//!（grilling 定案 2026-08-29）。

/// 有当前持仓的判定谓词（口径与 `v_holdings` 视图一致：批次剩余数量 > 0
/// 且排除软删除账户的批次）。
///
/// # 别名契约
///
/// 谓词以 `i` 引用外层 `instruments` 行：引用本常量的外层查询**必须**以 `i`
/// 作为 instruments 表别名（如 `FROM instruments i WHERE {INVESTED_EXISTS}`）。
/// 违反契约在 prepare 期即报「no such column」类错误，不会静默通过。
pub(crate) const INVESTED_EXISTS: &str = "EXISTS (SELECT 1 FROM security_lots l WHERE l.instrument_id=i.id \
     AND l.remaining_quantity > 0 \
     AND l.account_id IN (SELECT id FROM accounts WHERE is_deleted = 0))";
