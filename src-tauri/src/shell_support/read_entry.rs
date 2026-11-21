use rusqlite::Connection;

use ledger_infra::db::{DbReadHandle, DbWriteHandle};
use ledger_infra::error::Result;

pub async fn read_entry<T, F>(span: &'static str, db: DbReadHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    db.run(span, f).await
}

pub async fn read_entry_on_write<T, F>(span: &'static str, db: DbWriteHandle, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
{
    db.run_raw(span, f).await
}

#[cfg(test)]
mod tests {
    use ledger_infra::db::DbState;
    use ledger_infra::error::AppError;
    use ledger_infra::test_utils::GATED_TIMEOUT;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tauri_app_lib::test_support::{ScratchDir, open, seed_account};

    use super::*;

    fn fixture() -> DbState {
        let conn = Arc::new(Mutex::new(open()));
        DbState {
            conn: Arc::clone(&conn),
            read_conn: conn,
        }
    }

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

    #[test]
    fn read_entry_returns_while_long_write_job_in_flight() {
        let dir = ScratchDir::new("read-entry");
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
