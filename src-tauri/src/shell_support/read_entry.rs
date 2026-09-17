//! 壳层机制（spec #1086 P5 / #1108 壳层收敛）：本模块只被壳层消费，正住址即
//! 壳层根包；曾暂住 `ledger-infra::shell_support`（ADR-0111 决策 2 / #1130），
//! #1108 迁回，不得被基础设施或域引用。
//!
//! 壳层统一读入口（ADR-0104，spec #1009 批次①）：连接句柄与读闭包进，其余全部
//! 内化——[`ledger_infra::db::DbReadHandle`]（异步 DB 门面的**读类型化句柄**，
//! ADR-0125 决策 1/4，issue #1410）→ 门面读 DB 线程执行（连接槽锁由门面为整个
//! 作业持有，取用独占收在门面内，ADR-0125 决策 1）。
//!
//! 读命令的手抄仪式（克隆连接句柄 → 送阻塞线程池 → 锁失败映射 → span 归因）自
//! ADR-0125 起改道：调用点取门面读句柄交给本入口，仪式仍收在本入口一处实现；
//! 读命令壳退回到它该有的样子：解包 + 一行调用。命名与
//! [`crate::shell_support::write_entry`] 对仗，保持「只留一种可模仿形态」（ADR-0069 立项动机之一）。
//!
//! 语义锚（与迁移前逐字节一致，ADR-0104 决策 4）：
//! - **执行线程**：闭包在**门面读 DB 线程**执行（ADR-0125 决策 1；事件循环线程与
//!   tokio worker 不被 DB 调用占用）；读侧与写侧各持一条线程，长写事务不把读排在
//!   后面（ADR-0117 语义原样，读路径不进写者闸门）；
//! - **锁失败映射**：门面取槽锁失败（含锁中毒）归一化为
//!   [`ledger_infra::error::AppError::Db`]（与迁移前同一归一化口径）；
//! - **span 归因串**保留 `&'static str` 参数：IPC 传命令名字面量、HTTP 传
//!   `"METHOD /path"` 端点键，SQL 日志逐字节不变（ADR-0009 / ADR-0068 零感知）；
//! - **结果证据**：闭包业务错误原样传播、闭包 panic 归一化为
//!   [`ledger_infra::error::AppError::Io`]（与 ADR-0069 先例同形；门面侧
//!   `catch_unwind` + 未提交事务回滚，ADR-0125 决策 3）。
//!
//! 读路径无置脏维度（ADR-0032 置脏豁免单点不动，ADR-0104 关联）。域层读路径
//! （ADR-0033 接缝）不纳入——本入口壳层专用。

use rusqlite::Connection;

use ledger_infra::db::{DbReadHandle, DbWriteHandle};
use ledger_infra::error::Result;

/// 壳层统一读入口（ADR-0104 决策 2；取用形态经 ADR-0125 决策 1/4 更替为门面读
/// 句柄，issue #1410）：把读闭包交给门面读句柄，在门面读 DB 线程上执行。
///
/// - `span`：SQL 归因串（`&'static str`，IPC 命令名 / HTTP 端点键，语义同
///   门面作业的 `command` 参数）；
/// - 闭包业务错误原样传播、闭包 panic 归一化为 [`ledger_infra::error::AppError::Io`]
///   （与 ADR-0069 先例同形）；锁中毒归一化为 [`ledger_infra::error::AppError::Db`]。
pub async fn read_entry<T, F>(span: &'static str, db: DbReadHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    db.run(span, f).await
}

/// 壳层统一读入口 · **写槽形态**（ADR-0117 甄别结论，issue #1410）：读形态但闭包
/// 内含惰性写的命令（缓存自愈 UPSERT / 拼音惰性回填 / 余额漂移修复，ADR-0117
/// 「存量读路径只读甄别结果」逐命令留痕）必须走写连接——走只读读连接会在只读
/// 约束上报错。
///
/// 作业走门面写槽**裸作业**：不经连接层统一写入口的提交点后置动作（与迁移前
/// `read_entry` 消费写槽逐字一致——缓存属派生数据，修复不置脏、不发信号；
/// ADR-0067 / ADR-0032）。
pub async fn read_entry_on_write<T, F>(span: &'static str, db: DbWriteHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    db.run_raw(span, f).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{open, seed_account};
    use ledger_infra::db::DbState;
    use ledger_infra::error::AppError;
    use ledger_infra::test_utils::GATED_TIMEOUT;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// 内存库夹具（统一测试工厂建库，ADR-0084 决策 3/7）：写读两槽同指一连接
    /// （与 `DbState::open_in_memory` 同形），读句柄经具名访问器取——
    /// 测试世界构造保持「裸库 + 状态」原形（ADR-0125 决策 4）。
    fn fixture() -> DbState {
        let conn = Arc::new(Mutex::new(open()));
        DbState {
            conn: Arc::clone(&conn),
            read_conn: conn,
        }
    }

    /// 闭包执行：闭包在**门面读 DB 线程**跑完（不在调用线程内联），Ok 值原样
    /// 带回 await 点（线程名是可观察坐标，ADR-0125 决策 1）。
    #[test]
    fn closure_executes_and_returns_value() {
        let state = fixture();
        let caller = std::thread::current().id();
        let value =
            tauri::async_runtime::block_on(read_entry("test", state.read_handle(), move |_conn| {
                assert_ne!(
                    std::thread::current().id(),
                    caller,
                    "闭包应在门面读 DB 线程执行，不在调用线程内联"
                );
                assert_eq!(
                    std::thread::current().name(),
                    Some("db-read"),
                    "读作业应在读 DB 线程上执行（读侧与写侧不共线程）"
                );
                Ok(41 + 1)
            }))
            .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, 42);
    }

    /// 闭包拿到可用连接：读已提交的真实行（连接机制内化但真实可用）。
    #[test]
    fn closure_receives_usable_connection_and_reads_persisted_row() {
        let state = fixture();
        {
            let guard = state.conn.lock().expect("锁应可获取");
            seed_account(&guard, "acct-1", "测试账户", "cash", "CNY", 12345);
        }
        let balance: i64 =
            tauri::async_runtime::block_on(read_entry("test", state.read_handle(), move |conn| {
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
        let state = fixture();
        let err = tauri::async_runtime::block_on(read_entry::<(), _>(
            "test",
            state.read_handle(),
            move |_conn| Err(AppError::Invalid("boom".into())),
        ))
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
        let state = fixture();
        let poisoner = state.conn.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("锁应可获取");
            panic!("持锁 panic 使互斥体中毒");
        })
        .join();
        assert!(state.conn.lock().is_err(), "互斥体应已中毒");

        let err = tauri::async_runtime::block_on(read_entry::<(), _>(
            "test",
            state.read_handle(),
            move |_conn| Ok(()),
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Db(_)),
            "锁中毒应归一化为 AppError::Db，实际 {err:?}"
        );
    }

    /// 闭包 panic → 归一化为 AppError::Io（ADR-0069 先例同形；门面侧 catch_unwind
    /// 拦下并回滚未提交事务，ADR-0125 决策 3）。
    #[test]
    fn closure_panic_normalizes_to_io_error() {
        let state = fixture();
        let err = tauri::async_runtime::block_on(read_entry::<(), _>(
            "test",
            state.read_handle(),
            move |_conn| -> Result<()> { panic!("闭包内崩溃") },
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Io(_)),
            "panic 应归一化为 AppError::Io，实际 {err:?}"
        );
    }

    /// 读入口不进写者闸门（ADR-0117 语义 / ADR-0125 决策 1 的实现形态，issue #1410）：
    /// 长写作业在途（门面写线程整段持写槽）时，读入口仍限时返回。
    ///
    /// **负向判据**（ADR-0087 断言强度）：把读入口改回写者闸门（`db.run_write` 或与
    /// 写侧共线程 / 共连接）→ 读作业排到长写作业之后，本测试在限时内收不到结果即红。
    /// 夹具必须是**成对文件库**（写读两槽分属两连接）：内存库两槽同指一连接，
    /// 表达不了读侧独立性（facade 侧同款取舍）。
    #[test]
    fn read_entry_returns_while_long_write_job_in_flight() {
        let dir = std::env::temp_dir().join(format!(
            "ledger-read-entry-{}",
            ledger_infra::db::new_uuid()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录应可建");
        let state = ledger_infra::db::open_db_in(&dir).expect("文件库应成对打开");

        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let write = state.write_handle();
        let writer = std::thread::spawn(move || {
            tauri::async_runtime::block_on(write.run("test", move |_conn| {
                entered_tx.send(()).expect("通知在途应成功");
                release_rx
                    .recv_timeout(GATED_TIMEOUT)
                    .expect("测试应放行长写作业");
                Ok(())
            }))
            .expect("长写作业应成功");
        });
        entered_rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("长写作业应在途");

        let read = state.read_handle();
        let (read_tx, read_rx) = std::sync::mpsc::channel::<std::time::Duration>();
        let reader = std::thread::spawn(move || {
            let started = Instant::now();
            tauri::async_runtime::block_on(read_entry("test", read, |_conn| Ok(())))
                .expect("读应成功");
            read_tx
                .send(Instant::now() - started)
                .expect("回报读耗时应成功");
        });
        let elapsed = read_rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("长写作业在途时读入口应限时返回（读路径不得排在写者闸门之后，ADR-0117）");
        assert!(
            elapsed < GATED_TIMEOUT,
            "读应在限时内返回，实际耗时 {elapsed:?}"
        );

        release_tx.send(()).expect("放行长写作业应成功");
        writer.join().expect("长写作业线程应正常收尾");
        reader.join().expect("读线程应正常收尾");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
