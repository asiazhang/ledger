//! 作用域会话接缝（issue #1275）：标的信息同步编排获取数据库连接的唯一通道。
//!
//! 编排（[`super::incremental`]）需要读写库时经本接缝**短暂取一次连接**、用完
//! 即还；抓取网络数据的闭包签名里没有连接句柄，编排路径在类型上取不到连接
//! ——「持着连接做网络 I/O」从纪律约束（ADR-0069 决策 4）变成编译期写不出来
//! 的形状（父 spec #1274，本票为预重构：行为零变化）。接缝形态与进度发射器
//!（[`crate::progress::ProgressEmitter`]）同款：域定义 trait，实现在消费边界
//!（壳层）提供并接线——生产由命令壳把统一写入口持有的连接包成会话交给编排，
//! 域 crate 不依赖壳层、不引入并发原语（连接与锁归实现侧所有，ADR-0112 决策 5
//! 的反转形态）。

use rusqlite::Connection;

use ledger_infra::error::Result;

/// 作用域会话：把一次数据库读写限定在连接的作用域内。
///
/// 实现约定：
/// - 连接只在 [`with_connection`](Self::with_connection) 闭包体内可见，闭包
///   返回即回收（作用域只到闭包边界），实现不得让闭包外的代码再触到它；
/// - 闭包的业务 [`Result`] 原样传播，实现不包装、不吞错（与连接层统一写入口
///   ADR-0032 的错误契约一致）。
pub trait ScopedSession {
    /// 取一次连接执行一次读写。取连接的方式与时机（直通已持连接 / 锁内短暂
    /// 获取）由实现决定，编排对此无感知。
    fn with_connection<R, F>(&self, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>;
}
