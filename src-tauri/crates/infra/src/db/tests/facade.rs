use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tracing::Level;

use crate::db::DbState;
use crate::db::facade::DbFacade;
use crate::db::job_gate::LockOutcome;
use crate::error::AppError;
use crate::settings::SettingKey;
use crate::test_utils::{CaptureLayer, GATED_TIMEOUT, capture_events, ensure_global_max_level};
use tauri_app_lib::test_support::{FIXED_NOW, ScratchDir};
use tracing_subscriber::layer::SubscriberExt;

use super::common::{dirty_state, write_test_state};

fn file_state(tag: &str) -> (ScratchDir, DbState) {
    crate::db::register_after_commit_hook(ledger_backup::after_commit_hook);
    let dir = ScratchDir::new(&format!("db-facade-{tag}"));
    let state = crate::db::open_db_in(&dir).unwrap();
    (dir, state)
}

fn set_next_due(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    crate::settings::set(
        conn,
        SettingKey::AutoBackupNextDueAt,
        &Some(String::from(FIXED_NOW)),
    )
}

fn assert_later_jobs_fail_loud(
    facade: &DbFacade,
    first: &AppError,
    runs: &Arc<AtomicUsize>,
    runs_expected: usize,
) {
    for _ in 0..2 {
        let counter = Arc::clone(runs);
        let later = tauri::async_runtime::block_on(facade.run_write("test", move |_conn| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
        .unwrap_err();
        assert_eq!(
            later.to_string(),
            first.to_string(),
            "后续作业应 fail-loud 报同一错误（不静默恢复）"
        );
    }
    assert_eq!(
        runs.load(Ordering::SeqCst),
        runs_expected,
        "连接不可信后作业体一律不得执行"
    );
}

#[test]
fn timed_job_runs_and_returns_value_when_slot_is_free() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let outcome =
        facade.run_write_raw_blocking_within("test", Duration::from_secs(2), |_conn| Ok(41 + 1));
    match outcome {
        LockOutcome::Ran(Ok(value)) => assert_eq!(value, 42, "限时等待作业应原样带回结果"),
        other => panic!("槽空闲时作业应执行，实际 {other:?}"),
    }
}

#[test]
fn timed_job_abandons_without_running_body_when_slot_is_busy() {
    let (_dir, state) = file_state("timed-abandon");
    let facade = DbFacade::start(&state).expect("门面应启动");
    let hold = state.conn.lock().expect("测试应拿到写槽");
    let runs = Arc::new(AtomicUsize::new(0));
    let first_attempt = Arc::clone(&runs);
    let outcome = {
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let facade = &facade;
            scope.spawn(move || {
                let started = Instant::now();
                let outcome = facade.run_write_raw_blocking_within(
                    "test",
                    Duration::from_millis(50),
                    move |_conn| {
                        first_attempt.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                );
                let _ = done_tx.send((outcome, started.elapsed()));
            });
            let (outcome, elapsed) = done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("弃权应在时限附近返回：作业体若未被局限时放弃，这里拿不到结果");
            assert!(
                elapsed < Duration::from_secs(1),
                "等槽期间到达时限就应弃权返回，不应等到槽释放（实际 {elapsed:?}）"
            );
            outcome
        })
    };
    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "排队中被放弃的作业不得执行作业体（零副作用）"
    );
    assert!(
        matches!(outcome, LockOutcome::Abandoned),
        "槽被占用超过时限应放弃本轮，实际 {outcome:?}"
    );

    drop(hold);
    let retried = Arc::clone(&runs);
    match facade.run_write_raw_blocking_within("test", Duration::from_secs(2), move |_conn| {
        retried.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }) {
        LockOutcome::Ran(Ok(())) => {}
        other => panic!("放锁后的重试应正常执行，实际 {other:?}"),
    }
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "重试作业应恰好执行一次（被放弃的那次不得补跑）"
    );
}

#[test]
fn write_job_runs_on_write_thread_and_returns_value() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let caller = std::thread::current().id();
    let observed = Arc::new(Mutex::new(None::<(std::thread::ThreadId, Option<String>)>));
    let observer = Arc::clone(&observed);
    let value = tauri::async_runtime::block_on(facade.run_write("test", move |_conn| {
        *observer.lock().unwrap() = Some((
            std::thread::current().id(),
            std::thread::current().name().map(str::to_string),
        ));
        Ok(41 + 1)
    }))
    .expect("门面应传播作业的 Ok 值");
    assert_eq!(value, 42);
    let (thread_id, thread_name) = observed.lock().unwrap().clone().expect("作业应已执行");
    assert_ne!(
        thread_id, caller,
        "门面作业应在 DB 线程执行，不在调用线程内联"
    );
    assert_eq!(
        thread_name.as_deref(),
        Some("db-write"),
        "写作业应在写 DB 线程上执行"
    );
}

#[test]
fn read_job_runs_on_read_thread() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let observed = Arc::new(Mutex::new(None::<Option<String>>));
    let observer = Arc::clone(&observed);
    tauri::async_runtime::block_on(facade.run_read("test", move |_conn| {
        *observer.lock().unwrap() = Some(std::thread::current().name().map(str::to_string));
        Ok(())
    }))
    .expect("读作业应成功");
    assert_eq!(
        observed.lock().unwrap().clone().flatten().as_deref(),
        Some("db-read"),
        "读作业应在读 DB 线程上执行"
    );
}

#[test]
fn business_error_propagates_verbatim() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let write_err = tauri::async_runtime::block_on(
        facade.run_write::<(), _>("test", |_conn| Err(AppError::Invalid("写侧失败".into()))),
    )
    .unwrap_err();
    assert!(
        matches!(write_err, AppError::Invalid(ref m) if m == "写侧失败"),
        "写作业错误应原样传播，实际 {write_err:?}"
    );
    let read_err = tauri::async_runtime::block_on(
        facade.run_read::<(), _>("test", |_conn| Err(AppError::NotFound("读侧失败".into()))),
    )
    .unwrap_err();
    assert!(
        matches!(read_err, AppError::NotFound(ref m) if m == "读侧失败"),
        "读作业错误应原样传播，实际 {read_err:?}"
    );
}

#[test]
fn write_job_triggers_after_commit_hook() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    tauri::async_runtime::block_on(facade.run_write("test", |_conn| Ok(()))).expect("作业应成功");
    let after = dirty_state(&state);
    assert!(after.dirty, "作业提交后应由 DB 线程触发置脏");
    assert_eq!(after.last_backup_at, None, "目录未配置不应记录备份锚点");
}

#[test]
fn write_job_marks_dirty_only_at_commit_point() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let err = tauri::async_runtime::block_on(
        facade.run_write::<(), _>("test", |_conn| Err(AppError::Invalid("boom".into()))),
    )
    .unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(!dirty_state(&state).dirty, "作业失败不应置脏");

    tauri::async_runtime::block_on(facade.run_write("test", |conn| {
        conn.execute("BEGIN", []).map_err(AppError::from)?;
        set_next_due(conn)?;
        Ok(())
    }))
    .expect("未提交就返回的作业应成功");
    assert!(
        !dirty_state(&state).dirty,
        "未提交（事务在途）不置脏——提交点复核"
    );
    tauri::async_runtime::block_on(facade.run_write("test", |conn| {
        conn.execute("ROLLBACK", []).map_err(AppError::from)?;
        Ok(())
    }))
    .expect("回滚作业应成功");
    let pending: Option<String> =
        tauri::async_runtime::block_on(facade.run_write("test", |conn| {
            crate::settings::get(conn, SettingKey::AutoBackupNextDueAt, None)
        }))
        .expect("读回作业应成功");
    assert_eq!(pending, None, "未提交的写入应随回滚消失");
}

#[test]
fn read_job_does_not_mark_dirty() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let tables: i64 = tauri::async_runtime::block_on(facade.run_read("test", |conn| {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |r| r.get(0),
        )
        .map_err(AppError::from)
    }))
    .expect("读作业应成功");
    assert!(tables > 0, "读作业应能读到真实 schema");
    assert!(!dirty_state(&state).dirty, "读作业不应置脏");
}

#[test]
fn read_job_is_not_blocked_by_in_flight_write_job() {
    let (_dir, state) = file_state("independent");
    let facade = DbFacade::start(&state).expect("门面应启动");
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let released = Arc::new(AtomicBool::new(false));
    let released_in_read = Arc::clone(&released);

    let write_job = facade.run_write("test", move |_conn| {
        entered_tx.send(()).expect("通知在途应成功");
        release_rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("测试应放行写作业");
        Ok(())
    });
    let read_job = async {
        entered_rx
            .recv_timeout(GATED_TIMEOUT)
            .expect("写作业应进入在途（占住写槽）");
        let value = facade
            .run_read("test", |conn| {
                conn.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
                    [],
                    |r| r.get(0),
                )
                .map_err(AppError::from)
            })
            .await
            .expect("写作业在途时读作业应可服务");
        assert!(
            !released_in_read.load(Ordering::SeqCst),
            "读应在写作业仍在途时即返回（读侧不经过写者闸门）"
        );
        release_tx.send(()).expect("放行写作业应成功");
        Ok::<i64, AppError>(value)
    };
    let (write_result, read_result) =
        tauri::async_runtime::block_on(async { tokio::join!(write_job, read_job) });
    released.store(true, Ordering::SeqCst);
    write_result.expect("写作业应成功");
    assert!(read_result.expect("读作业应成功") > 0);
}

#[test]
fn caller_span_and_dispatch_propagate_into_job() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    ensure_global_max_level();
    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let observed = Arc::new(Mutex::new(None::<String>));
    let observer = Arc::clone(&observed);
    let span = tracing::info_span!("request");
    let _entered = span.enter();
    tauri::async_runtime::block_on(facade.run_write("test", move |_conn| {
        tracing::info!("门面作业内标记事件");
        *observer.lock().unwrap() = tracing::Span::current()
            .metadata()
            .map(|m| m.name().to_string());
        Ok(())
    }))
    .expect("作业应成功");

    assert_eq!(
        observed.lock().unwrap().as_deref(),
        Some("request"),
        "作业应携带调用方 span 执行（跨线程显式带入）"
    );
    let events = captured.lock().unwrap().clone();
    let marker: Vec<_> = events
        .iter()
        .filter(|e| {
            e.fields
                .iter()
                .any(|(k, v)| k == "message" && v.contains("门面作业内标记事件"))
        })
        .collect();
    assert!(
        !marker.is_empty(),
        "作业内事件应路由到调用方 dispatcher，实际: {events:?}"
    );
    assert!(
        marker
            .iter()
            .all(|e| e.current_span.as_deref() == Some("request")),
        "作业内事件应归因到调用方 span（request），实际: {events:?}"
    );
}

#[test]
fn command_span_rebuilt_when_caller_has_none() {
    ensure_global_max_level();
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let observed = Arc::new(Mutex::new(None::<String>));
    let observer = Arc::clone(&observed);
    tauri::async_runtime::block_on(facade.run_write("test", move |_conn| {
        *observer.lock().unwrap() = tracing::Span::current()
            .metadata()
            .map(|m| m.name().to_string());
        Ok(())
    }))
    .expect("作业应成功");
    assert_eq!(
        observed.lock().unwrap().as_deref(),
        Some("command"),
        "调用点无 span 时应重建 command span（归因兜底）"
    );
}

#[test]
fn job_panic_reports_error_and_facade_keeps_serving() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");

    let err = tauri::async_runtime::block_on(
        facade.run_write::<(), _>("test", |_conn| -> crate::error::Result<()> {
            panic!("写作业内崩溃")
        }),
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Io(ref m) if m.contains("写作业内崩溃")),
        "作业 panic 应归一化为 AppError::Io 并携带 panic 载荷，实际 {err:?}"
    );
    assert!(
        state.conn.lock().is_ok(),
        "panic 在守卫作用域内被拦下：连接互斥体不应中毒（ADR-0125 决策 3）"
    );
    assert_eq!(
        tauri::async_runtime::block_on(facade.run_write("test", |_conn| Ok(7)))
            .expect("panic 后写侧仍应服务"),
        7,
        "写作业 panic 后门面应继续服务（不静默重启、不退役线程）"
    );

    let read_err = tauri::async_runtime::block_on(
        facade.run_read::<(), _>("test", |_conn| -> crate::error::Result<()> {
            panic!("读作业内崩溃")
        }),
    )
    .unwrap_err();
    assert!(
        matches!(read_err, AppError::Io(_)),
        "读作业 panic 同样归一化为 AppError::Io，实际 {read_err:?}"
    );
    assert_eq!(
        tauri::async_runtime::block_on(facade.run_read("test", |_conn| Ok(9)))
            .expect("panic 后读侧仍应服务"),
        9
    );
}

#[test]
fn job_panic_rolls_back_uncommitted_transaction() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let err = tauri::async_runtime::block_on(facade.run_write::<(), _>(
        "test",
        |conn| -> crate::error::Result<()> {
            conn.execute("BEGIN", []).map_err(AppError::from)?;
            set_next_due(conn)?;
            panic!("事务在途时崩溃");
        },
    ))
    .unwrap_err();
    assert!(matches!(err, AppError::Io(_)), "实际 {err:?}");
    assert!(
        !dirty_state(&state).dirty,
        "panic 作业未到提交点：回滚后不应置脏"
    );

    let pending: Option<String> =
        tauri::async_runtime::block_on(facade.run_write("test", |conn| {
            crate::settings::get(conn, SettingKey::AutoBackupNextDueAt, None)
        }))
        .expect("后续作业应成功");
    assert_eq!(pending, None, "panic 前未提交的写入应随回滚消失");

    tauri::async_runtime::block_on(facade.run_write("test", |conn| {
        conn.execute("BEGIN", []).map_err(AppError::from)?;
        set_next_due(conn)?;
        conn.execute("COMMIT", []).map_err(AppError::from)?;
        Ok(())
    }))
    .expect("连接回到提交点后应可再开事务并提交");
    assert!(
        dirty_state(&state).dirty,
        "提交点应置脏（autocommit 复核不变）"
    );
}

#[test]
fn untrusted_connection_fails_loud_for_all_later_jobs() {
    let state = write_test_state();
    let poisoner = Arc::clone(&state.conn);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.lock().expect("锁应可获取");
        panic!("持锁 panic 使互斥体中毒");
    })
    .join();
    assert!(state.conn.lock().is_err(), "互斥体应已中毒");

    let facade = DbFacade::start(&state).expect("门面应启动");
    let runs = Arc::new(AtomicUsize::new(0));
    let first = {
        let runs = Arc::clone(&runs);
        tauri::async_runtime::block_on(facade.run_write("test", move |_conn| {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
        .unwrap_err()
    };
    assert!(
        matches!(first, AppError::Db(_)),
        "连接不可信应报 Db 错误（与锁中毒归一化同形），实际 {first:?}"
    );
    state.conn.clear_poison();
    assert!(
        state.conn.lock().is_ok(),
        "清除中毒标记后槽应可锁（后续判据据此排除「只是锁不上」的解释）"
    );
    assert_later_jobs_fail_loud(&facade, &first, &runs, 0);
}

#[test]
fn panic_recovery_failure_marks_connection_untrusted() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let runs = Arc::new(AtomicUsize::new(0));
    let first = {
        let runs = Arc::clone(&runs);
        tauri::async_runtime::block_on(facade.run_write::<(), _>(
            "test",
            move |conn| -> crate::error::Result<()> {
                conn.execute("BEGIN", []).map_err(AppError::from)?;
                crate::db::facade::force_next_recovery_failure();
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("注入 panic：恢复失败");
            },
        ))
        .unwrap_err()
    };
    assert!(
        matches!(first, AppError::Db(ref m) if m.contains("回滚失败")),
        "恢复失败应作为该作业的错误 fail-loud 上报，实际 {first:?}"
    );
    assert_later_jobs_fail_loud(&facade, &first, &runs, 1);
}

#[test]
fn facade_handle_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<DbFacade>();
}

#[test]
fn handles_resolve_process_level_facade_and_route_by_direction() {
    let state = write_test_state();
    crate::db::install_facade(&state.slots()).expect("安装应成功");
    assert!(
        crate::db::facade_installed(&state.conn),
        "安装过的写槽应被判为进程级门面"
    );

    let thread_name = || std::thread::current().name().map(str::to_string);
    let write = state.write_handle();
    let read = state.read_handle();
    let write_thread =
        tauri::async_runtime::block_on(write.run_raw("test", move |_conn| Ok(thread_name())))
            .expect("写句柄作业应成功");
    let read_thread =
        tauri::async_runtime::block_on(read.run("test", move |_conn| Ok(thread_name())))
            .expect("读句柄作业应成功");
    assert_eq!(
        write_thread.as_deref(),
        Some("db-write"),
        "写句柄的作业应在写 DB 线程执行"
    );
    assert_eq!(
        read_thread.as_deref(),
        Some("db-read"),
        "读句柄的作业应在读 DB 线程执行（读侧不进写者闸门，ADR-0117）"
    );

    let lazy_state = write_test_state();
    let lazy = lazy_state.write_handle();
    tauri::async_runtime::block_on(lazy.run("test", |_conn| Ok(()))).expect("惰性门面作业应成功");
    assert!(
        !crate::db::facade_installed(&lazy_state.conn),
        "惰性拉起的门面不是进程级安装——「不静默恢复」的账不落在它身上"
    );
}

#[test]
fn shutdown_joins_workers_and_rejects_later_jobs() {
    let state = write_test_state();
    let baseline = Arc::strong_count(&state.conn);
    let mut facade = DbFacade::start(&state).expect("门面应启动");
    assert_eq!(
        Arc::strong_count(&state.conn),
        baseline + 2,
        "两条门面线程各持一个槽句柄（内存库写读两槽同指一连接）"
    );
    tauri::async_runtime::block_on(facade.run_write("test", |_conn| Ok(()))).expect("作业应成功");
    facade.shutdown();
    assert!(
        facade.workers_finished(),
        "停机后两条 DB 线程都应已退出（join 完成）"
    );
    assert_eq!(
        Arc::strong_count(&state.conn),
        baseline,
        "线程退出后应释放槽句柄"
    );
    let err = tauri::async_runtime::block_on(facade.run_write::<(), _>("test", |_conn| Ok(())))
        .unwrap_err();
    assert!(
        matches!(err, AppError::Io(_)),
        "停机后投递应 fail-loud 报错，实际 {err:?}"
    );
}

#[test]
fn drop_joins_workers() {
    let state = write_test_state();
    let baseline = Arc::strong_count(&state.conn);
    let facade = DbFacade::start(&state).expect("门面应启动");
    assert_eq!(Arc::strong_count(&state.conn), baseline + 2);
    drop(facade);
    assert_eq!(
        Arc::strong_count(&state.conn),
        baseline,
        "Drop 应停机并 join 两条线程（在途作业跑完为止）"
    );
}

#[test]
fn slow_job_warns_past_probe_threshold() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let events = capture_events(|| {
        tauri::async_runtime::block_on(facade.run_write("test", |_conn| {
            std::thread::sleep(Duration::from_millis(1100));
            Ok(())
        }))
        .expect("慢作业应成功（探针只记日志、不改变行为）");
    });
    assert!(
        events
            .iter()
            .any(|e| e.level == Level::WARN && e.fields.iter().any(|(k, _)| k == "hold_ms")),
        "作业占用 DB 线程超阈值应记 warn，实际捕获: {events:?}"
    );
}

#[test]
fn budgeted_slow_job_stays_silent_within_registered_budget() {
    const PREFIX: &str = "facade-budget-probe.";
    crate::db::register_lock_hold_budget(PREFIX, Duration::from_secs(30));

    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let events = capture_events(|| {
        tauri::async_runtime::block_on(facade.run_write("facade-budget-probe.slow-job", |_conn| {
            std::thread::sleep(Duration::from_millis(1100));
            Ok(())
        }))
        .expect("预算内慢作业应成功（探针只记日志、不改变行为）");
    });
    assert!(
        events.iter().all(|e| e.level != Level::WARN),
        "未超登记预算的作业不应记 warn，实际捕获: {events:?}"
    );
}

fn has_account(conn: &rusqlite::Connection, id: &str) -> bool {
    conn.query_row("SELECT count(*) FROM accounts WHERE id = ?1", [id], |r| {
        r.get::<_, i64>(0)
    })
    .expect("账户存在性查询应成功")
        > 0
}

fn facade_has_account(facade: &DbFacade, id: &str) -> bool {
    let id = id.to_string();
    tauri::async_runtime::block_on(facade.run_read("test", move |conn| Ok(has_account(conn, &id))))
        .expect("读作业应成功")
}

#[test]
fn facade_jobs_observe_paired_swap_in_both_slots() {
    let (_old_dir, state) = file_state("swap-old");
    let facade = DbFacade::start(&state).expect("门面应启动");
    tauri::async_runtime::block_on(facade.run_write("test", |conn| {
        tauri_app_lib::test_support::seed_account(conn, "acct-old", "旧库", "cash", "CNY", 1111);
        Ok(())
    }))
    .expect("旧库写作业应成功");
    assert!(
        facade_has_account(&facade, "acct-old"),
        "换连前读作业应看到旧库的账户"
    );

    let new_dir = ScratchDir::new("db-facade-swap-new");
    let new_conn = crate::db::open_connection_in(&new_dir).expect("新库写连接应建成");
    let new_read = crate::db::open_connection_readonly_in(&new_dir).expect("新库读连接应建成");
    tauri_app_lib::test_support::seed_account(&new_conn, "acct-new", "新库", "cash", "CNY", 2222);

    state.swap_pair(new_conn, new_read).expect("成对换连应成功");

    assert!(
        facade_has_account(&facade, "acct-new"),
        "换连后读作业应看到新库（成对原语漏换读槽即红）"
    );
    assert!(
        !facade_has_account(&facade, "acct-old"),
        "换连后读作业不应再看到旧库（两槽应一致指向新库）"
    );
    tauri::async_runtime::block_on(facade.run_write("test", |conn| {
        tauri_app_lib::test_support::seed_account(conn, "acct-post", "换连后", "cash", "CNY", 3333);
        Ok(())
    }))
    .expect("换连后写作业应成功");
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            has_account(&guard, "acct-new") && has_account(&guard, "acct-post"),
            "换连后写作业应落在新库（成对原语漏换写槽即写进旧库）"
        );
        assert!(!has_account(&guard, "acct-old"), "换连后写槽不应再指向旧库");
    }
    assert!(
        facade_has_account(&facade, "acct-post"),
        "读槽应看到写槽落在新库的写入（两槽一致指向新库）"
    );
}

#[test]
fn swap_pair_resets_untrusted_mark_for_new_connection() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let runs = Arc::new(AtomicUsize::new(0));
    let write_first = {
        let runs = Arc::clone(&runs);
        tauri::async_runtime::block_on(facade.run_write::<(), _>(
            "test",
            move |conn| -> crate::error::Result<()> {
                conn.execute("BEGIN", []).map_err(AppError::from)?;
                crate::db::facade::force_next_recovery_failure();
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("注入 panic：恢复失败");
            },
        ))
        .unwrap_err()
    };
    assert!(
        matches!(write_first, AppError::Db(ref m) if m.contains("回滚失败")),
        "恢复失败应作为该作业的错误上报，实际 {write_first:?}"
    );
    assert_later_jobs_fail_loud(&facade, &write_first, &runs, 1);
    let read_first = tauri::async_runtime::block_on(facade.run_read::<(), _>(
        "test",
        |_conn| -> crate::error::Result<()> {
            crate::db::facade::force_next_recovery_failure();
            panic!("注入 panic：读侧恢复失败");
        },
    ))
    .unwrap_err();
    assert!(
        matches!(read_first, AppError::Db(ref m) if m.contains("回滚失败")),
        "读侧恢复失败应作为该作业的错误上报，实际 {read_first:?}"
    );
    for _ in 0..2 {
        let later =
            tauri::async_runtime::block_on(facade.run_read::<(), _>("test", |_conn| Ok(())))
                .unwrap_err();
        assert_eq!(
            later.to_string(),
            read_first.to_string(),
            "读侧连接不可信后应 fail-loud 报同一错误"
        );
    }

    state
        .swap_pair(
            tauri_app_lib::test_support::open(),
            tauri_app_lib::test_support::open(),
        )
        .expect("成对换连应成功");

    assert_eq!(
        tauri::async_runtime::block_on(facade.run_write("test", |_conn| Ok(7)))
            .expect("换连后写侧应恢复服务"),
        7,
        "换连换入的新连接不应继承写侧旧连接的不可信标记"
    );
    assert_eq!(
        tauri::async_runtime::block_on(facade.run_read("test", |conn| {
            conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
                .map_err(AppError::from)
        }))
        .expect("换连后读侧应恢复服务"),
        1,
        "换连换入的新连接不应继承读侧旧连接的不可信标记"
    );
}

#[test]
fn read_slot_only_swap_out_is_observed_and_resets_read_mark() {
    let (_dir, state) = file_state("swap-out-read");
    let facade = DbFacade::start(&state).expect("门面应启动");
    let read_first = tauri::async_runtime::block_on(facade.run_read::<(), _>(
        "test",
        |_conn| -> crate::error::Result<()> {
            crate::db::facade::force_next_recovery_failure();
            panic!("注入 panic：读侧恢复失败");
        },
    ))
    .unwrap_err();
    assert!(
        matches!(read_first, AppError::Db(_)),
        "读侧恢复失败应上报 Db 错误，实际 {read_first:?}"
    );

    state.placeholderize_read_conn().expect("读槽换出应成功");

    let placeholder_error =
        tauri::async_runtime::block_on(facade.run_read::<(), _>("test", |conn| {
            conn.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))?;
            Ok(())
        }))
        .expect_err("占位库无业务表，读作业应报错（换出对门面立即可见）");
    assert!(
        matches!(placeholder_error, AppError::Db(_)),
        "占位槽上的读应报 Db 错误，实际 {placeholder_error:?}"
    );
    assert_eq!(
        tauri::async_runtime::block_on(facade.run_read("test", |_conn| Ok(11)))
            .expect("读槽换出后读侧应恢复服务（不继承旧的不可信标记）"),
        11,
        "读槽换出应复位读线程的不可信标记"
    );
}
