//! 作用域会话接缝（issue #1275；async 形态 ADR-0125 决策 5 / issue #1412）：标的信息
//! 同步编排获取数据库连接的唯一通道。
//!
//! 编排（[`super::incremental`]）需要读写库时经本接缝**短暂取一次连接**、用完
//! 即还；抓取网络数据的闭包签名里没有连接句柄，编排路径在类型上取不到连接
//! ——「持着连接做网络 I/O」从纪律约束（ADR-0069 决策 4）变成编译期写不出来
//! 的形状（父 spec #1274，#1275 预重构：行为零变化）。
//!
//! **async 形态**（ADR-0125 决策 5 / issue #1412）：一进一出——取连接作业是
//! `await`（作业经异步 DB 门面在 DB 线程执行，编排 `await` 结果），闭包体内
//! 仍是同步 rusqlite 操作。接缝因此成为「行情域异步化」与「DB 形态」之间的
//! 稳定边界：编排侧只认本 trait，底层实现可以是阻塞线程池（过渡态）或异步
//! DB 门面（目标态），零分叉。作业闭包要求 `Send + 'static`（门面作业形态），
//! 编排现场的可变状态（进度回调 / 写入见证 / 通道束）进不了闭包——需要带回
//! 的结果由闭包返回、在 `await` 之后回填编排现场。
//!
//! 接缝形态与进度发射器（[`crate::progress::ProgressEmitter`]）同款：域定义
//! trait，实现在消费边界提供并接线——生产 = [`FacadeWriteSession`]（命令壳侧
//! 与域内后台车道各持门面写句柄构造，[`DbWriteHandle::run_raw`] 裸作业：逐段
//! autocommit、提交点置脏归收尾裁决点，ADR-0073 / #1276 语义），域 crate 不依赖
//! 壳层（连接与锁归实现侧所有，ADR-0112 决策 5 的反转形态）。

use std::future::Future;

use ledger_infra::db::DbWriteHandle;
use ledger_infra::error::Result;

/// 作用域会话：把一次数据库读写限定在连接的作用域内。
///
/// 实现约定：
/// - 连接只在 [`with_connection`](Self::with_connection) 闭包体内可见，闭包
///   返回即回收（作用域只到闭包边界），实现不得让闭包外的代码再触到它；
/// - 取连接的方式与时机由实现决定（生产 = 门面写槽裸作业），编排对此无感知；
///   作业在实现的执行体（DB 线程）上运行，**网络等待不得进入闭包**——闭包是
///   同步 rusqlite 形态，作业期间的连接为整个作业独占（ADR-0069 决策 4）；
/// - 闭包的业务 [`Result`] 原样传播，实现不包装、不吞错（与连接层统一写入口
///   ADR-0032 的错误契约一致）。
pub trait ScopedSession {
    /// 取一次连接执行一次读写：作业闭包进，`await` 结果出（一进一出）。
    fn with_connection<R, F>(&self, use_connection: F) -> impl Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static;
}

use rusqlite::Connection;

/// 门面写槽裸作业会话（生产实现，issue #1412）：把 [`DbWriteHandle`] 的写槽
/// **裸作业**（[`DbWriteHandle::run_raw`]）包成作用域会话交给编排。
///
/// 「裸」的含义与迁移前分段取连接逐字一致（ADR-0125 决策 2 / issue #1410）：
/// 作业在门面写 DB 线程执行、不经连接层统一写入口的提交点后置动作——逐段
/// autocommit，置脏与信号由分段入口在收尾裁决点恰好一次触发。命令壳侧
///（`sync_instrument_info`）与域内后台两条车道（历史补全 / 每日现价刷新）
/// 各持句柄构造，后台车道的连接取用自此不再直锁连接槽。
pub struct FacadeWriteSession {
    handle: DbWriteHandle,
    /// 作业的 SQL 归因串（`&'static str`，语义同门面作业的 `command` 参数）。
    span: &'static str,
}

impl FacadeWriteSession {
    /// 由门面写句柄构造会话（消费边界接线点：命令壳与后台车道）。
    pub fn new(handle: DbWriteHandle, span: &'static str) -> Self {
        Self { handle, span }
    }
}

impl ScopedSession for FacadeWriteSession {
    fn with_connection<R, F>(&self, use_connection: F) -> impl Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.handle.run_raw(self.span, use_connection)
    }
}
