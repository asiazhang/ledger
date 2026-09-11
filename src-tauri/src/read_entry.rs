//! 壳层统一读入口（ADR-0104，spec #1009 批次①）：连接句柄与读闭包进，其余全部
//! 内化——[`crate::db::run_db`]（执行线程与 span 传播，ADR-0069，组合而非替代）→
//! 锁行（锁失败映射，与 [`crate::db::write`] 同形）。
//!
//! 读命令的手抄仪式（克隆连接句柄 → 送阻塞线程池 → 锁失败映射 → span 归因）
//! 收敛进本入口一处实现；读命令壳退回到它该有的样子：解包 + 一行调用。命名与
//! [`crate::write_entry`] 对仗，保持「只留一种可模仿形态」（ADR-0069 立项动机之一）。
//!
//! 语义锚（与迁移前逐字节一致，ADR-0104 决策 4）：
//! - **执行线程**：闭包在 tauri 全局运行时的阻塞线程池执行（`run_db` 组合语义，
//!   事件循环线程与 tokio worker 不被 DB 调用占用）；
//! - **锁失败映射**：`conn.lock().map_err(|e| AppError::Db(e.to_string()))?`
//!   体内单点——锁中毒归一化为 [`crate::error::AppError::Db`]；
//! - **span 归因串**保留 `&'static str` 参数：IPC 传命令名字面量、HTTP 传
//!   `"METHOD /path"` 端点键，SQL 日志逐字节不变（ADR-0009 / ADR-0068 零感知）；
//! - **结果证据**：闭包业务错误原样传播、闭包 panic 归一化为
//!   [`crate::error::AppError::Io`]（与 `run_db`/ADR-0069 先例同形）。
//!
//! 读路径无置脏维度（ADR-0032 置脏豁免单点不动，ADR-0104 关联）。域层读路径
//! （ADR-0033 接缝）不纳入——本入口壳层专用。

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::run_db;
use crate::error::{AppError, Result};

/// 壳层统一读入口（ADR-0104 决策 2）：组合 `run_db`（阻塞线程池 + span 传播）
/// → 锁行（锁失败映射体内单点）→ 读闭包。
///
/// - `span`：SQL 归因串（`&'static str`，IPC 命令名 / HTTP 端点键，语义同
///   [`run_db`] 的 `command` 参数）；
/// - 闭包业务错误原样传播、闭包 panic 归一化为 [`crate::error::AppError::Io`]
///   （与 [`run_db`]/ADR-0069 先例同形）；锁中毒归一化为
///   [`crate::error::AppError::Db`]（与 [`crate::db::write`] 同形）。
pub async fn read_entry<T, F>(span: &'static str, conn: Arc<Mutex<Connection>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    run_db(span, move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        f(&conn)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::test_support::{open, seed_account};
    use std::sync::Arc;

    /// 内存库夹具（统一测试工厂建库，ADR-0084 决策 3/7）。
    fn fixture() -> Arc<Mutex<Connection>> {
        Arc::new(Mutex::new(open()))
    }

    /// 闭包执行：闭包在阻塞线程池线程跑完（不在调用线程内联，run_db 组合语义），
    /// Ok 值原样带回 await 点。
    #[test]
    fn closure_executes_and_returns_value() {
        let conn = fixture();
        let caller = std::thread::current().id();
        let value = tauri::async_runtime::block_on(read_entry("test", conn, move |_conn| {
            assert_ne!(
                std::thread::current().id(),
                caller,
                "闭包应在阻塞线程池线程执行（run_db 组合语义）"
            );
            Ok(41 + 1)
        }))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, 42);
    }

    /// 闭包拿到可用连接：读已提交的真实行（连接机制内化但真实可用）。
    #[test]
    fn closure_receives_usable_connection_and_reads_persisted_row() {
        let conn = fixture();
        {
            let guard = conn.lock().expect("锁应可获取");
            seed_account(&guard, "acct-1", "测试账户", "cash", "CNY", 12345);
        }
        let balance: i64 = tauri::async_runtime::block_on(read_entry("test", conn, move |conn| {
            conn.query_row(
                "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-1'",
                [],
                |r| r.get(0),
            )
            .map_err(AppError::from)
        }))
        .expect("读取应成功");
        assert_eq!(balance, 12345, "闭包应能读到已种入的行");
    }

    /// 业务错误原样传播（不二次包装）。
    #[test]
    fn business_error_propagates_verbatim() {
        let conn = fixture();
        let err = tauri::async_runtime::block_on(read_entry::<(), _>("test", conn, move |_conn| {
            Err(AppError::Invalid("boom".into()))
        }))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Invalid(ref m) if m == "boom"),
            "业务错误应原样传播，实际 {err:?}"
        );
    }

    /// 锁中毒：持锁线程 panic 使互斥体中毒，入口归一化为 AppError::Db
    ///（与 db::write 同形，ADR-0104 决策 4）。
    #[test]
    fn poisoned_lock_maps_to_db_error() {
        let conn = fixture();
        let poisoner = conn.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("锁应可获取");
            panic!("持锁 panic 使互斥体中毒");
        })
        .join();
        assert!(conn.lock().is_err(), "互斥体应已中毒");

        let err =
            tauri::async_runtime::block_on(read_entry::<(), _>("test", conn, move |_conn| Ok(())))
                .unwrap_err();
        assert!(
            matches!(err, AppError::Db(_)),
            "锁中毒应归一化为 AppError::Db，实际 {err:?}"
        );
    }

    /// 闭包 panic → JoinError 归一化为 AppError::Io（ADR-0069 先例同形）。
    #[test]
    fn closure_panic_normalizes_to_io_error() {
        let conn = fixture();
        let err = tauri::async_runtime::block_on(read_entry::<(), _>(
            "test",
            conn,
            move |_conn| -> Result<()> { panic!("闭包内崩溃") },
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Io(_)),
            "panic 应归一化为 AppError::Io，实际 {err:?}"
        );
    }
}
