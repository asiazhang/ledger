//! 壳层统一写入口（ADR-0073，spec #523）：连接句柄、发射器、写操作身份、业务闭包进，
//! 其余全部内化——[`crate::db::run_db`]（执行线程与 span 传播，ADR-0069，组合而非
//! 替代）→ [`crate::db::write`]（锁失败映射、事务、提交点置脏，ADR-0032）→
//! [`crate::signals::signals_for`]（映射单点，ADR-0044）→ 发射。
//!
//! 写命令的六行仪式（克隆连接句柄 → 送阻塞线程池 → 锁失败映射 → 开事务置脏 →
//! 发信号）收敛进本入口一处实现；写操作身份（[`crate::signals::WriteOp`]）作为参数
//! 随闭包流动，两壳「命令 → 身份」声明表消亡为源码扫描派生物（守门见
//! `signals_cross_check`，ADR-0073 决策 5）。命令壳退回到它该有的样子：
//! 解包 + 一行调用。
//!
//! 语义锚（与迁移前逐字节一致）：
//! - **发射时序**：事务提交成功后发射（写失败早退不发）；发射走既有投递机制
//!   （`SignalEmitter::post`，ADR-0054 主线程非阻塞投递），发射失败静默忽略、
//!   不影响写结果（ADR-0044）。「发不发、发哪个」判定单点仍是
//!   [`crate::signals::signals_for`]（穷尽 match，身份合法性唯一裁决者）；
//! - **结果证据**：经 [`Outcome`] 包装随闭包返回必达（ADR-0073 决策 2）——
//!   [`Outcome::Silent`]（零证据）/ [`Outcome::Evidenced`]（携带
//!   [`crate::signals::WriteEvidence`]），条件信号（价格写入 / 黑洞即建 / 商户即建）
//!   的「条件」一半由证据承载，「条件信号身份误用静默入口」的漂移被类型消灭；
//! - **发射器参数归一**（ADR-0073 决策 3）：`Option<&dyn SignalEmitter>`——
//!   [`tauri::AppHandle`] 即该接缝的生产实现（ADR-0054），IPC 壳透传 `Some(&app)`；
//!   HTTP 壳从 `EmitterSlot` 解包（`slot.as_deref()`）；`None` 跳过发射正是两侧
//!   共有的既有测试态语义。走 `db::write` 但映射为零信号的写命令仍统一传
//!   `Some`——生产不借用测试态语义，未来给零信号身份补信号时天然生效；
//! - **span 归因串**保留 `&'static str` 参数：IPC 传命令名字面量、HTTP 传
//!   `"METHOD /path"` 端点键，SQL 日志逐字节不变（ADR-0009 / ADR-0068 零感知）；
//!   归因串与身份的漂移由扫描守门顺带核对（ADR-0073 决策 4/5）。
//!
//! 入口零豁免概念（ADR-0073 决策 6）：不设 bypass-dirty 参数，置脏豁免仍由
//! [`crate::db::write`]/`settings.rs` 内层单点裁决（ADR-0032）；Restore 路径与
//! 其余不经 `db::write` 的声明写命令不经本入口（例外白名单登记，见
//! `signals_cross_check`）。域层写路径（ADR-0033 接缝）不纳入——本入口壳层专用。

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::run_db;
use crate::db::write as db_write;
use crate::error::Result;
use crate::events::SignalEmitter;
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 写闭包的结果证据包装（ADR-0073 决策 2）：[`write_entry`] 的业务闭包统一返回
/// `Result<Outcome<T>>`——单形态，证据只能从返回值来，「携带证据」是类型要求
/// 而非调用方自觉。零证据命令的代价是一个 [`Outcome::Silent`] 构造器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    /// 零证据：信号完全由写操作身份的静态映射行决定（含刻意零信号身份）。
    Silent(T),
    /// 携带结果证据：条件信号的「条件」一半（价格写入 / 黑洞即建 / 商户即建，
    /// 见 [`WriteEvidence`]），由映射单点与身份合成「发不发」。
    Evidenced(T, WriteEvidence),
}

/// 壳层统一写入口（ADR-0073 决策 1）：按序组合 `run_db`（阻塞线程池 + span
/// 传播）→ `db::write`（锁失败映射、事务、提交点置脏）→ `signals_for` → 发射。
///
/// - `span`：SQL 归因串（`&'static str`，IPC 命令名 / HTTP 端点键，语义同
///   [`run_db`] 的 `command` 参数）；
/// - `emitter`：`None` 跳过发射（两侧既有测试态语义）；
/// - 闭包业务错误原样传播、闭包 panic 归一化为 [`crate::error::AppError::Io`]
///   （与 [`run_db`]/ADR-0069 先例同形）；写失败早退不发信号；
/// - 发生在写事务提交成功之后、调用线程上（与迁移前壳层「await 后发射」
///   逐点同位）。
pub async fn write_entry<T, F>(
    span: &'static str,
    conn: Arc<Mutex<Connection>>,
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    f: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<Outcome<T>> + Send + 'static,
{
    let (value, evidence) = run_db(span, move || {
        db_write(&conn, |conn| match f(conn)? {
            Outcome::Silent(value) => Ok((value, WriteEvidence::None)),
            Outcome::Evidenced(value, evidence) => Ok((value, evidence)),
        })
    })
    .await?;
    // 事务提交成功后发射（映射单点判定，ADR-0044）：emit_for = signals_for +
    // emit_all，发射失败静默忽略，不影响写结果。
    if let Some(emitter) = emitter {
        emit_for(emitter, op, evidence);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbState;
    use crate::error::AppError;
    use crate::events::{BACKUPS_CHANGED, LEDGER_CHANGED, PRICES_CHANGED};
    use crate::signals::{WriteEvidence, WriteOp};
    use crate::test_utils::GatedEmitter;
    use std::sync::Arc;

    /// 内存库 + 闸门式假发射器的测试夹具。
    fn fixture() -> (Arc<Mutex<Connection>>, GatedEmitter) {
        let db = DbState::open_in_memory().expect("内存库应可打开");
        (db.conn, GatedEmitter::gated())
    }

    /// 静默闭包：Ok 值带回 await 点，闭包在阻塞线程池执行（run_db 组合语义）。
    #[test]
    fn silent_closure_executes_and_returns_value() {
        let (conn, _emitter) = fixture();
        let caller = std::thread::current().id();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            None,
            WriteOp::CreateAccount,
            move |_conn| {
                assert_ne!(
                    std::thread::current().id(),
                    caller,
                    "闭包应在阻塞线程池线程执行（run_db 组合语义）"
                );
                Ok(Outcome::Silent(41 + 1))
            },
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, 42);
    }

    /// 闭包拿到可用连接：写入落库（连接机制内化但真实可用）。
    #[test]
    fn closure_receives_usable_connection_and_write_persists() {
        let (conn, _emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            conn.clone(),
            None,
            WriteOp::CreateCategory,
            move |conn| {
                conn.execute(
                    "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                     VALUES ('cat-1', '测试', 'expense', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, 'device-1')",
                    [],
                )
                .map_err(AppError::from)?;
                Ok(Outcome::Silent(()))
            },
        ))
        .expect("写入应成功");
        let count: i64 = conn
            .lock()
            .expect("锁应可获取")
            .query_row(
                "SELECT count(*) FROM categories WHERE id = 'cat-1'",
                [],
                |r| r.get(0),
            )
            .expect("查询应成功");
        assert_eq!(count, 1, "闭包内的写入应已提交落库");
    }

    /// 证据传递：Evidenced + 条件证据为真 → 按身份与证据发价格信号；
    /// 条件为假 → 零信号（「发不发」判定在映射单点，入口只传递）。
    #[test]
    fn evidenced_evidence_reaches_signal_mapping() {
        let (conn, emitter) = fixture();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            Some(&emitter),
            WriteOp::SyncHoldingPrices,
            move |_conn| {
                Ok(Outcome::Evidenced(
                    "done",
                    WriteEvidence::PriceWritten(true),
                ))
            },
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, "done");
        assert_eq!(
            emitter.posted(),
            vec![PRICES_CHANGED],
            "价格写入应发价格信号"
        );

        // 同身份、证据为假 → 零信号。
        let (conn, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            Some(&emitter),
            WriteOp::SyncHoldingPrices,
            move |_conn| Ok(Outcome::Evidenced(1, WriteEvidence::PriceWritten(false))),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert!(emitter.posted().is_empty(), "零变化不广播（映射单点判定）");
    }

    /// 身份传递：静态映射行身份（无证据）按 signals_for 发对应信号。
    #[test]
    fn identity_drives_static_signal_row() {
        let (conn, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            Some(&emitter),
            WriteOp::CreateAccount,
            move |_conn| Ok(Outcome::Silent("id")),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(
            emitter.posted(),
            vec![LEDGER_CHANGED],
            "参考写入身份应发参考失效信号"
        );
    }

    /// 发射器 None：跳过发射（两侧既有测试态语义），写结果不受影响。
    #[test]
    fn none_emitter_skips_emission() {
        let (conn, _emitter) = fixture();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            None,
            WriteOp::CreateAccount,
            move |_conn| Ok(Outcome::Silent("id")),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, "id");
    }

    /// 业务错误原样传播（不二次包装），且失败早退不发信号。
    #[test]
    fn business_error_propagates_verbatim_without_emission() {
        let (conn, emitter) = fixture();
        let err = tauri::async_runtime::block_on(write_entry::<(), _>(
            "test",
            conn,
            Some(&emitter),
            WriteOp::CreateAccount,
            move |_conn| Err(AppError::Invalid("boom".into())),
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Invalid(ref m) if m == "boom"),
            "业务错误应原样传播，实际 {err:?}"
        );
        assert!(emitter.posted().is_empty(), "写失败不应发信号");
    }

    /// 闭包 panic → JoinError 归一化为 AppError::Io（ADR-0069 先例同形），不发信号。
    #[test]
    fn closure_panic_normalizes_to_io_error_without_emission() {
        let (conn, emitter) = fixture();
        let err = tauri::async_runtime::block_on(write_entry::<(), _>(
            "test",
            conn,
            Some(&emitter),
            WriteOp::PruneBackups,
            move |_conn| -> Result<Outcome<()>> { panic!("闭包内崩溃") },
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Io(_)),
            "panic 应归一化为 AppError::Io，实际 {err:?}"
        );
        assert!(emitter.posted().is_empty(), "写失败不应发信号");
    }

    /// 证据形状直通：Evidenced 的证据（非默认 None）随闭包返回到达映射单点
    /// ——黑洞即建证据驱动账户域条件信号（ADR-0044 决策 4）。
    #[test]
    fn black_hole_evidence_drives_conditional_signal() {
        let (conn, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            Some(&emitter),
            WriteOp::AdjustAccountBalance,
            move |_conn| {
                Ok(Outcome::Evidenced(
                    "tx-1",
                    WriteEvidence::BlackHoleCreated(true),
                ))
            },
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(
            emitter.posted(),
            vec![LEDGER_CHANGED],
            "黑洞即建应发参考失效信号"
        );

        // 备份域身份对照：静态行 BackupsChanged（身份 → 信号，与证据无关）。
        let (conn, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            conn,
            Some(&emitter),
            WriteOp::PruneBackups,
            move |_conn| Ok(Outcome::Silent(())),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(emitter.posted(), vec![BACKUPS_CHANGED]);
    }
}
