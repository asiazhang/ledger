use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rusqlite::Connection;

use ledger_infra::db::DbWriteHandle;
use ledger_infra::db::{probe_lock_hold, run_db};
use ledger_infra::error::{AppError, Result};
use ledger_infra::events::SignalEmitter;
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Silent(T),
    Evidenced(T, WriteEvidence),
}

pub async fn write_entry<T, F>(
    span: &'static str,
    db: DbWriteHandle,
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    f: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<Outcome<T>> + Send + 'static,
{
    let (value, evidence) = db
        .run(span, move |conn| match f(conn)? {
            Outcome::Silent(value) => Ok((value, WriteEvidence::None)),
            Outcome::Evidenced(value, evidence) => Ok((value, evidence)),
        })
        .await?;
    if let Some(emitter) = emitter {
        emit_for(emitter, op, evidence);
    }
    Ok(value)
}

pub struct SegmentLock<'a> {
    conn: &'a Arc<Mutex<Connection>>,
}

impl<'a> SegmentLock<'a> {
    pub fn new(db: &'a DbWriteHandle) -> SegmentLock<'a> {
        SegmentLock {
            conn: db.write_slot(),
        }
    }

    pub fn with_connection<R, F>(&self, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let hold_started = Instant::now();
        let conn = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_connection(&conn);
        probe_lock_hold(hold_started.elapsed());
        result
    }
}

#[derive(Debug)]
pub struct SegmentedFailure {
    pub error: AppError,
    pub evidence: WriteEvidence,
}

impl From<AppError> for SegmentedFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            evidence: WriteEvidence::None,
        }
    }
}

fn segmented_normalize<T>(
    result: std::result::Result<Outcome<T>, SegmentedFailure>,
) -> (Result<T>, WriteEvidence) {
    match result {
        Ok(Outcome::Silent(value)) => (Ok(value), WriteEvidence::None),
        Ok(Outcome::Evidenced(value, evidence)) => (Ok(value), evidence),
        Err(failure) => (Err(failure.error), failure.evidence),
    }
}

fn segmented_needs_dirty_job<T>(result: &Result<T>, evidence: &WriteEvidence) -> bool {
    result.is_ok() || evidence.price_written()
}

fn segmented_emit_after_verdict(
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    evidence: WriteEvidence,
) {
    if let Some(emitter) = emitter {
        emit_for(emitter, op, evidence);
    }
}

pub async fn write_entry_segmented<T, F>(
    span: &'static str,
    db: DbWriteHandle,
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    f: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&SegmentLock<'_>) -> std::result::Result<Outcome<T>, SegmentedFailure>
        + Send
        + 'static,
{
    let (result, evidence) = run_db(span, move || {
        let lock = SegmentLock::new(&db);
        let (result, evidence) = segmented_normalize(f(&lock));
        if segmented_needs_dirty_job(&result, &evidence) {
            db.run_blocking(span, |_| Ok(()))?;
        }
        Ok((result, evidence))
    })
    .await?;
    segmented_emit_after_verdict(emitter, op, evidence);
    result
}

pub async fn write_entry_segmented_async<T, Fut>(
    span: &'static str,
    db: DbWriteHandle,
    emitter: Option<&dyn SignalEmitter>,
    op: WriteOp,
    f: Fut,
) -> Result<T>
where
    Fut: Future<Output = std::result::Result<Outcome<T>, SegmentedFailure>> + Send,
{
    let (result, evidence) = segmented_normalize(f.await);
    if segmented_needs_dirty_job(&result, &evidence) {
        db.run(span, |_| Ok(())).await?;
    }
    segmented_emit_after_verdict(emitter, op, evidence);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_infra::db::DbState;
    use ledger_infra::error::AppError;
    use ledger_infra::events::{BACKUPS_CHANGED, LEDGER_CHANGED, PRICES_CHANGED};
    use ledger_infra::signals::{WriteEvidence, WriteOp};
    use ledger_infra::test_utils::{GATED_TIMEOUT, GatedEmitter};
    use rusqlite::params;
    use std::sync::{Arc, Mutex};

    fn fixture() -> (DbState, GatedEmitter) {
        let conn = crate::test_support::open();
        let conn = Arc::new(Mutex::new(conn));
        (
            DbState {
                conn: Arc::clone(&conn),
                read_conn: conn,
            },
            GatedEmitter::gated(),
        )
    }

    #[test]
    fn silent_closure_executes_and_returns_value() {
        let (state, _emitter) = fixture();
        let caller = std::thread::current().id();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            None,
            WriteOp::CreateAccount,
            move |_conn| {
                assert_ne!(
                    std::thread::current().id(),
                    caller,
                    "闭包应在门面写 DB 线程执行，不在调用线程内联"
                );
                assert_eq!(
                    std::thread::current().name(),
                    Some("db-write"),
                    "写作业应在写 DB 线程上执行"
                );
                Ok(Outcome::Silent(41 + 1))
            },
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, 42);
    }

    #[test]
    fn closure_receives_usable_connection_and_write_persists() {
        let (state, _emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            None,
            WriteOp::CreateCategory,
            move |conn| {
                conn.execute(
                    "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        "cat-1",
                        "测试",
                        "expense",
                        crate::test_support::FIXED_NOW,
                        crate::test_support::FIXED_NOW,
                        1,
                        "device-1"
                    ],
                )
                .map_err(AppError::from)?;
                Ok(Outcome::Silent(()))
            },
        ))
        .expect("写入应成功");
        let count: i64 = state
            .conn
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

    #[test]
    fn evidenced_evidence_reaches_signal_mapping() {
        let (state, emitter) = fixture();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            Some(&emitter),
            WriteOp::SyncInstrumentInfo,
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

        let (state, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            Some(&emitter),
            WriteOp::SyncInstrumentInfo,
            move |_conn| Ok(Outcome::Evidenced(1, WriteEvidence::PriceWritten(false))),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert!(emitter.posted().is_empty(), "零变化不广播（映射单点判定）");
    }

    #[test]
    fn identity_drives_static_signal_row() {
        let (state, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
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

    #[test]
    fn none_emitter_skips_emission() {
        let (state, _emitter) = fixture();
        let value = tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            None,
            WriteOp::CreateAccount,
            move |_conn| Ok(Outcome::Silent("id")),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(value, "id");
    }

    #[test]
    fn business_error_propagates_verbatim_without_emission() {
        let (state, emitter) = fixture();
        let err = tauri::async_runtime::block_on(write_entry::<(), _>(
            "test",
            state.write_handle(),
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

    #[test]
    fn closure_panic_normalizes_to_io_error_without_emission() {
        let (state, emitter) = fixture();
        let err = tauri::async_runtime::block_on(write_entry::<(), _>(
            "test",
            state.write_handle(),
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

    #[test]
    fn black_hole_evidence_drives_conditional_signal() {
        let (state, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
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

        let (state, emitter) = fixture();
        tauri::async_runtime::block_on(write_entry(
            "test",
            state.write_handle(),
            Some(&emitter),
            WriteOp::PruneBackups,
            move |_conn| Ok(Outcome::Silent(())),
        ))
        .expect("入口应传播闭包的 Ok 值");
        assert_eq!(emitter.posted(), vec![BACKUPS_CHANGED]);
    }

    #[test]
    fn segmented_entry_holds_no_lock_during_closure_wait() {
        let state = fixture().0;
        let conn = Arc::clone(&state.conn);
        let handle = state.write_handle();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            tauri::async_runtime::block_on(write_entry_segmented(
                "test",
                handle,
                None,
                WriteOp::SyncInstrumentInfo,
                move |lock| {
                    lock.with_connection(|c| {
                        c.execute(
                            "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                             VALUES ('cat-seg-1', '段一', 'expense', '2026-01-10T00:00:00Z', '2026-01-10T00:00:00Z', 1, 'test')",
                            [],
                        )
                        .map_err(AppError::from)?;
                        Ok(())
                    })?;
                    entered_tx.send(()).expect("通知等待点应成功");
                    release_rx
                        .recv_timeout(GATED_TIMEOUT)
                        .expect("测试应放行网络等待");
                    lock.with_connection(|c| {
                        c.execute(
                            "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                             VALUES ('cat-seg-2', '段二', 'expense', '2026-01-10T00:00:00Z', '2026-01-10T00:00:00Z', 1, 'test')",
                            [],
                        )
                        .map_err(AppError::from)?;
                        Ok(())
                    })?;
                    Ok(Outcome::Evidenced(
                        "done",
                        WriteEvidence::PriceWritten(true),
                    ))
                },
            ))
        });

        entered_rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("闭包应到达网络等待点");
        {
            let guard = conn
                .try_lock()
                .expect("网络等待期间连接锁不应被持有（分段取锁核心性质）");
            let count: i64 = guard
                .query_row(
                    "SELECT count(*) FROM categories WHERE id = 'cat-seg-1'",
                    [],
                    |r| r.get(0),
                )
                .expect("读回应成功");
            assert_eq!(count, 1, "先头段的写入应已提交且对立即可读");
        }
        release_tx.send(()).expect("放行应成功");
        let value = worker.join().expect("分段写入口应跑完").expect("应成功");
        assert_eq!(value, "done");
        let guard = conn.lock().expect("收尾锁应可取");
        let count: i64 = guard
            .query_row(
                "SELECT count(*) FROM categories WHERE id LIKE 'cat-seg-%'",
                [],
                |r| r.get(0),
            )
            .expect("读回应成功");
        assert_eq!(count, 2, "两段写入都应落库");
    }

    #[test]
    fn segmented_entry_verdict_dirty_on_success_and_skip_on_zero_write_failure() {
        ledger_infra::db::register_after_commit_hook(ledger_backup::after_commit_hook);
        fn dirty_of(conn: &Connection) -> bool {
            ledger_backup::get_state(conn)
                .expect("调度状态应可读")
                .dirty
        }
        fn reset_dirty(conn: &Connection) {
            use ledger_infra::settings::SettingKey;
            ledger_infra::settings::set(conn, SettingKey::AutoBackupDirty, &false)
                .expect("重置脏标记应成功");
        }

        let (state, _emitter) = fixture();
        {
            let guard = state.conn.lock().expect("锁应可取");
            reset_dirty(&guard);
        }
        tauri::async_runtime::block_on(write_entry_segmented(
            "test",
            state.write_handle(),
            None,
            WriteOp::SyncInstrumentInfo,
            move |lock| {
                lock.with_connection(|c| {
                    c.execute("UPDATE app_settings SET value = value WHERE key = 'x'", [])
                        .map_err(AppError::from)?;
                    Ok(())
                })?;
                Ok(Outcome::Silent(()))
            },
        ))
        .expect("分段写入应成功");
        assert!(
            dirty_of(&state.conn.lock().expect("锁应可取")),
            "成功收尾应经提交点置脏恰好生效"
        );

        let (state, emitter) = fixture();
        {
            let guard = state.conn.lock().expect("锁应可取");
            reset_dirty(&guard);
        }
        let err = tauri::async_runtime::block_on(write_entry_segmented::<(), _>(
            "test",
            state.write_handle(),
            Some(&emitter),
            WriteOp::SyncInstrumentInfo,
            move |_lock| Err(AppError::Invalid("中途失败".into()).into()),
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Invalid(ref m) if m == "中途失败"),
            "业务错误应原样传播，实际 {err:?}"
        );
        assert!(
            !dirty_of(&state.conn.lock().expect("锁应可取")),
            "零写入失败收尾不应置脏（与整段形态同构）"
        );
        assert!(emitter.posted().is_empty(), "零写入失败不应发信号");
    }

    #[test]
    fn segmented_entry_failure_with_writes_marks_dirty_and_emits_signal() {
        crate::db::register_after_commit_hook(ledger_backup::after_commit_hook);
        fn dirty_of(conn: &Connection) -> bool {
            ledger_backup::get_state(conn)
                .expect("调度状态应可读")
                .dirty
        }

        let (state, emitter) = fixture();
        {
            let guard = state.conn.lock().expect("锁应可取");
            use ledger_infra::settings::SettingKey;
            ledger_infra::settings::set(&guard, SettingKey::AutoBackupDirty, &false)
                .expect("重置脏标记应成功");
        }
        let err = tauri::async_runtime::block_on(write_entry_segmented::<(), _>(
            "test",
            state.write_handle(),
            Some(&emitter),
            WriteOp::SyncInstrumentInfo,
            move |lock| {
                lock.with_connection(|c| {
                    c.execute(
                        "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                         VALUES ('cat-seg-fail', '段', 'expense', '2026-01-10T00:00:00Z', '2026-01-10T00:00:00Z', 1, 'test')",
                        [],
                    )
                    .map_err(AppError::from)?;
                    Ok(())
                })?;
                Err(SegmentedFailure {
                    error: AppError::Invalid("中途失败".into()),
                    evidence: WriteEvidence::PriceWritten(true),
                })
            },
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::Invalid(ref m) if m == "中途失败"),
            "业务错误应原样传播，实际 {err:?}"
        );
        assert!(
            dirty_of(&state.conn.lock().expect("锁应可取")),
            "实际写过的失败收尾应置脏（#1277：实际写过即置脏）"
        );
        assert_eq!(
            emitter.posted(),
            vec![PRICES_CHANGED],
            "实际写过的失败收尾应发价格失效信号（成败同判）"
        );
    }
}
