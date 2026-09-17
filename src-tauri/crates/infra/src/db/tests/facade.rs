//! 异步 DB 门面核心测试（ADR-0125 决策 1–3，issue #1408）：作业在 DB 线程执行
//!（与调用线程不同）、结果与错误原样传播、提交点后置动作在提交后由 DB 线程触发、
//! 读侧独立于写侧（长写不挡读）、跨线程 span / dispatcher 归因不漂移、作业 panic
//! 不毒化互斥体且回滚未提交事务、连接不可信后后续作业 fail-loud、停机与探针语义。
//!
//! 换连承接（ADR-0125 决策 2 / 决策 3，issue #1409）：成对换连在门面下成立
//!（换连与在途作业互斥、对后续作业立即可见，写读两槽各自断言）、换连后新连接
//! 重置连接不可信标记（决策 3 的「退役 / 重建」）。
//!
//! 失败注入口径（ADR-0087 断言强度）：SQLite 无确定性的「回滚失败」注入手段——
//! 探针实证（本票实施期实测，2026-09）：第二连接持 SHARED 锁时写事务 `ROLLBACK`
//! 成功、`PRAGMA query_only = ON` 在途事务上 `ROLLBACK` 成功、中途
//! `PRAGMA journal_mode = OFF` 后 `ROLLBACK` 成功、只读连接上 `BEGIN` + 失败写后
//! `ROLLBACK` 成功（rusqlite 0.40 / bundled SQLCipher）。故「回滚失败 → 连接不可信
//! → 后续作业 fail-loud」一判据经门面内仅测试构建可见的注入开关驱动
//!（`facade::force_next_recovery_failure`）；「连接状态不可信」（槽锁中毒，迁移期
//! 真实可达：直锁调用点的闭包 panic 毒化互斥体）另用真实注入路径覆盖。

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
use tauri_app_lib::test_support::FIXED_NOW;
use tracing_subscriber::layer::SubscriberExt;

use super::common::{dirty_state, write_test_state};

/// 文件库成对 [`DbState`] 夹具（读写两槽各一条连接）：内存库两槽同指一连接，
/// 表达不了「读侧独立于写侧」，故读侧独立性用文件库钉住（readonly 测试同款建库）。
fn file_state(tag: &str) -> (std::path::PathBuf, DbState) {
    crate::db::register_after_commit_hook(ledger_backup::after_commit_hook);
    let dir =
        std::env::temp_dir().join(format!("ledger-db-facade-{tag}-{}", crate::db::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = crate::db::open_db_in(&dir).unwrap();
    (dir, state)
}

/// 调度状态 KV 写入（作业内写入探针，避开业务表外键）。
fn set_next_due(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    crate::settings::set(
        conn,
        SettingKey::AutoBackupNextDueAt,
        &Some(String::from(FIXED_NOW)),
    )
}

/// 「连接不可信后一律 fail-loud」的共同判据（两种触发原因共用）：后续作业报**同一
/// 错误**（字符串逐字相同）且作业体不执行；`runs_expected` 是触发作业自身是否跑过
/// 作业体的期望值。
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

// ---------------------------------------------------------------------------
// 限时等待与放弃（ADR-0125 决策 8 豁免台账退役，issue #1415）
// ---------------------------------------------------------------------------

/// 正向：槽空闲时限时等待作业照常执行并原样带回结果（时限不是「延迟」）。
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

/// 时限内拿不到槽 → 放弃本轮，且**作业体零执行**（ADR-0125 决策 8 豁免台账退役的
/// 核心判据，issue #1415）：调用方拿到 [`LockOutcome::Abandoned`]。
///
/// 负向（ADR-0087，删除即变红）：取掉限时放弃语义（改回无界等待）→ 本测试的
/// `run_raw_blocking_within` 会一直等到测试放锁才返回，`Ran` 分支断言红；
/// 「排队中可放弃」被削弱成「等到底再执行」时，作业体执行次数断言同样红。
#[test]
fn timed_job_abandons_without_running_body_when_slot_is_busy() {
    let (_dir, state) = file_state("timed-abandon");
    let facade = DbFacade::start(&state).expect("门面应启动");
    let hold = state.conn.lock().expect("测试应拿到写槽");
    let runs = Arc::new(AtomicUsize::new(0));
    let first_attempt = Arc::clone(&runs);
    // 作业只能进队：投递线程在槽锁（测试持有）与作业通道（DB 线程 blocked 在等
    // 槽锁）之间——调用方自己不得在持锁状态下等作业，故投递给独立线程。
    let outcome = std::thread::scope(|scope| {
        let call = scope.spawn(|| {
            let started = Instant::now();
            let outcome = facade.run_write_raw_blocking_within(
                "test",
                Duration::from_millis(50),
                move |_conn| {
                    first_attempt.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            );
            (outcome, started.elapsed())
        });
        // 等过时限：调用方应在时限附近返回（而不是等槽释放）。
        let (outcome, elapsed) = call.join().expect("投递线程应正常返回");
        // 槽仍在测试手里：被放弃的作业不得在槽释放后就地补跑。
        assert_eq!(
            runs.load(Ordering::SeqCst),
            0,
            "排队中被放弃的作业不得执行作业体（零副作用）"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "等槽期间到达时限就应弃权返回，不应等到槽释放（实际 {elapsed:?}）"
        );
        outcome
    });
    assert!(
        matches!(outcome, LockOutcome::Abandoned),
        "槽被占用超过时限应放弃本轮，实际 {outcome:?}"
    );

    // 放锁后重投同一形态：本轮正常执行（「下一轮重试」成立）。
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

// ---------------------------------------------------------------------------
// 正向：执行线程、结果传播、提交点后置动作、读侧独立性、span 归因
// ---------------------------------------------------------------------------

/// 写作业在写 DB 线程执行（不在调用线程内联），Ok 值原样带回 await 点。
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

/// 读作业在读 DB 线程执行（读侧与写侧不共线程，ADR-0117 的实现形态）。
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

/// 业务错误原样传播（不二次包装），写 / 读两侧同判。
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

/// 提交点后置动作（置脏 + 写时到期检查）由 DB 线程在作业提交后触发：写作业成功
/// 即置脏，目录未配置时到期检查静默跳过（不记备份锚点）。
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

/// 写作业失败（事务回滚）→ 不置脏；未提交就返回（显式事务仍打开）同样不置脏
/// ——autocommit 复核语义与取锁形态同源（同一实现）。
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
    // 回滚作业自身在提交点成功 → 按 ADR-0032 置脏（置脏判据是「成功且回到提交点」，
    // 不是「库内容有净变化」）；关键是未提交的写入不残留。
    let pending: Option<String> =
        tauri::async_runtime::block_on(facade.run_write("test", |conn| {
            crate::settings::get(conn, SettingKey::AutoBackupNextDueAt, None)
        }))
        .expect("读回作业应成功");
    assert_eq!(pending, None, "未提交的写入应随回滚消失");
}

/// 读作业不触碰置脏维度（读路径无写后置动作，ADR-0104 / ADR-0032 置脏豁免）。
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

/// 读侧独立于写侧（ADR-0117 语义在门面形态下原样）：写作业在途（占住写线程与
/// 写槽）时，读作业照常被读线程服务并即时返回——长写事务不把读排在后面。
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

/// 调用方上下文传播：调用点已有活动 span + 线程局部 dispatcher（HTTP handlers 在
/// tower_http 请求 span 内运行的形态）时，作业在该 span 内执行、事件路由到调用方
/// dispatcher——门面形态下 SQL 归因不漂移（与 `run_db` 同款显式带入）。
#[test]
fn caller_span_and_dispatch_propagate_into_job() {
    // 建库（迁移会发射 SQL 事件）在建捕获层之前完成，捕获面只含本测试的作业事件。
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

/// 调用点无 span（IPC 异步命令形态）时重建 `command` span 兜底，维持既有 SQL
/// 耗时归因口径（lib.rs 异步命令归因约定）。
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

// ---------------------------------------------------------------------------
// panic 语义：不毒化互斥体、回滚未提交事务、恢复失败即连接不可信
// ---------------------------------------------------------------------------

/// 作业 panic：错误经通道 fail-loud 上报（归一化为 `AppError::Io`，与 `run_db`
/// 的 JoinError 先例同形），DB 线程不死、**互斥体不中毒**——`catch_unwind` 位于
/// 连接槽守卫作用域之内（ADR-0125 决策 3）。写读两侧 panic 后门面都继续服务。
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

/// 作业在未提交事务中 panic → 整体回滚：连接回到提交点、panic 前的写入不残留、
/// 不置脏；后续作业可再开事务并正常提交（提交点置脏照常）。
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

/// 槽锁中毒 = 连接状态不可信（迁移期真实可达：直锁调用点的闭包 panic 毒化互斥体，
/// ADR-0125 背景所述退化现场）：首个作业 fail-loud 报错并标记，后续作业一律报
/// **同一错误**且作业体不执行——即便槽锁中毒标记被清除（连接本身未坏）也不静默
/// 恢复服务，判定依据是「连接已被标记不可信」而不是「槽当下能不能锁上」。
#[test]
fn untrusted_connection_fails_loud_for_all_later_jobs() {
    let state = write_test_state();
    // 真实注入路径：另一线程持写槽 panic → 互斥体中毒。
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
    // 清除互斥体中毒标记（连接本身未坏，槽可再次取用）：门面仍须拒绝服务——
    // 不可信状态一旦标记即持续，不随槽状态波动静默恢复。
    state.conn.clear_poison();
    assert!(
        state.conn.lock().is_ok(),
        "清除中毒标记后槽应可锁（后续判据据此排除「只是锁不上」的解释）"
    );
    assert_later_jobs_fail_loud(&facade, &first, &runs, 0);
}

/// panic 恢复失败（回滚失败 / 回滚后连接仍未回到提交点）→ 标记连接不可信：该作业
/// 与后续作业一律 fail-loud 报同一错误，作业体不执行（ADR-0125 决策 3）。
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
                // 注入：本次作业 panic 后的恢复失败（SQLite 无确定性回滚失败手段）。
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

// ---------------------------------------------------------------------------
// 停机语义与作业占用探针
// ---------------------------------------------------------------------------

/// 门面句柄可被共享：`Send + Sync`（壳层应用状态要求 `Send + Sync + 'static`，
/// `Arc` 跨线程共享同理）——调用方改道票（#1410）把门面接线进壳层的前提，编译期钉住。
#[test]
fn facade_handle_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<DbFacade>();
}

/// 门面句柄（ADR-0125 决策 1/4，issue #1410）按槽解析门面：安装过进程级门面的
/// 槽解析到那一份（写句柄的作业在写线程、读句柄的作业在读线程）；未安装的槽
/// 惰性拉起（测试世界形态），且不被误判为进程级安装。
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

    // 未安装的槽：句柄惰性拉起门面（测试世界与命令层直呼同形），不产生固定条目。
    let lazy_state = write_test_state();
    let lazy = lazy_state.write_handle();
    tauri::async_runtime::block_on(lazy.run("test", |_conn| Ok(()))).expect("惰性门面作业应成功");
    assert!(
        !crate::db::facade_installed(&lazy_state.conn),
        "惰性拉起的门面不是进程级安装——「不静默恢复」的账不落在它身上"
    );
}

/// 停机：投停机消息并 join 两条 DB 线程（槽句柄随之释放），停机后投递 fail-loud
/// 报错（不静默丢弃作业）。
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

/// `Drop` 与显式停机同义：门面句柄释放即两条线程退出并释放槽句柄（测试收尾
/// 不泄漏线程）。
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

/// 作业占用 DB 线程时长超过阈值即记 warn（阈值与告警口径与既有持锁探针同源，
/// ADR-0125 决策 2）：门面形态下量纲从「持锁时长」变为「作业占用 DB 线程时长」。
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

// ---------------------------------------------------------------------------
// 换连承接：四条路径的成对换连在门面下成立（ADR-0125 决策 2 / 决策 3，issue #1409）
// ---------------------------------------------------------------------------

/// 给定连接上是否存在该账户（断言「哪个库可见」用：迁移自带种子账户，按 id 判定
/// 不受其干扰；测试侧只读数、不写表）。
fn has_account(conn: &rusqlite::Connection, id: &str) -> bool {
    conn.query_row("SELECT count(*) FROM accounts WHERE id = ?1", [id], |r| {
        r.get::<_, i64>(0)
    })
    .expect("账户存在性查询应成功")
        > 0
}

/// 经读作业断言账户存在性（门面下读槽的可观察面）。
fn facade_has_account(facade: &DbFacade, id: &str) -> bool {
    // 作业闭包需 `'static`：账户 id 随闭包一并带走。
    let id = id.to_string();
    tauri::async_runtime::block_on(facade.run_read("test", move |conn| Ok(has_account(conn, &id))))
        .expect("读作业应成功")
}

/// 换连承接 · 成对换连在门面下成立（ADR-0125 决策 2，issue #1409）：门面运行期间
/// 经成对换连原语换库，门面后续的**读作业**（读槽）与**写作业**（写槽）都应看到
/// 新库——门面线程为每个作业重新取槽，换连与在途作业共用槽互斥体，故换连对门面
/// 立即可见。
///
/// 负向判据（ADR-0087，删除即变红）：成对原语退化为「只换写槽」→ 读作业仍读到旧
/// 库，本测试的读断言红；退化为「只换读槽」→ 写作业仍落在旧库，写槽断言红。
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

    // 新库：独立目录成对建连（产品建连缝：迁移在写连接上完成、读连接只读同库），
    // 种子种在换入前的裸连接上。
    let new_dir = std::env::temp_dir().join(format!(
        "ledger-db-facade-swap-new-{}",
        crate::db::new_uuid()
    ));
    std::fs::create_dir_all(&new_dir).unwrap();
    let new_conn = crate::db::open_connection_in(&new_dir).expect("新库写连接应建成");
    let new_read = crate::db::open_connection_readonly_in(&new_dir).expect("新库读连接应建成");
    tauri_app_lib::test_support::seed_account(&new_conn, "acct-new", "新库", "cash", "CNY", 2222);

    state.swap_pair(new_conn, new_read).expect("成对换连应成功");

    // 读槽：换连后读作业必须走读槽看到新库（漏换读槽 → 仍读到「旧库」，此处红）。
    assert!(
        facade_has_account(&facade, "acct-new"),
        "换连后读作业应看到新库（成对原语漏换读槽即红）"
    );
    assert!(
        !facade_has_account(&facade, "acct-old"),
        "换连后读作业不应再看到旧库（两槽应一致指向新库）"
    );
    // 写槽：换连后写作业必须落在新库（漏换写槽 → 写进旧库）。
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
    // 跨槽一致：写槽的写入经读槽可见——两槽同指新库（成对换连的可观察结果）。
    assert!(
        facade_has_account(&facade, "acct-post"),
        "读槽应看到写槽落在新库的写入（两槽一致指向新库）"
    );
}

/// 换连后新连接重置不可信标记（ADR-0125 决策 2 承接 / 决策 3「是否退役或重建该连接
/// 由实施票按现场判定」，issue #1409 判定为换连即重建）：连接被标记不可信后经成对
/// 换连换入新连接，门面写侧与读侧都恢复服务——标记是**连接**级的，旧连接的失败态
/// 不被新连接继承。
///
/// 负向判据（ADR-0087，删除即变红）：去掉 DB 线程取锁后的换连代次比对（复位调用点）
/// → 换连后门面仍报旧错误，本测试的「恢复服务」断言红。两条 DB 线程各自的标记都
/// 先被真实打上（写侧、读侧各触发一次恢复失败），故任一条线程漏掉复位都变红。
#[test]
fn swap_pair_resets_untrusted_mark_for_new_connection() {
    let state = write_test_state();
    let facade = DbFacade::start(&state).expect("门面应启动");
    let runs = Arc::new(AtomicUsize::new(0));
    // 写侧触发「连接不可信」：作业 panic 后回滚失败（SQLite 无确定性回滚失败手段，
    // 用门面内仅测试构建可见的注入开关；口径与上文的同族判据一致）。
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
    // 读侧同样打上「连接不可信」（读线程的标记独立于写线程）：注入开关在回滚之前
    // 短路，故这里不必先开事务——判据只关心「标记已被打上」。
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

    // 成对换连：槽换成新的一对连接（换连原语沿用 ADR-0117 决策 3，不改门面作业）。
    state
        .swap_pair(
            tauri_app_lib::test_support::open(),
            tauri_app_lib::test_support::open(),
        )
        .expect("成对换连应成功");

    // 新连接服务：旧连接的失败态不继承——写侧、读侧各自线程的标记都须复位。
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

/// 读槽单槽换出（恢复 / 整库转换路径的 `placeholderize_read_conn`，issue #1280 /
/// ADR-0117 决策 3）在门面下的承接（issue #1409）：读槽换出也抬高换连代次——
/// 读作业既须看到换出后的占位槽（换出对门面立即可见），也须复位读线程的不可信标记。
///
/// 负向判据（ADR-0087，删除即变红）：读槽替换不抬高代次（或门面不比对该槽代次）
/// → 读侧仍报旧的不可信错误，本测试的「读侧恢复服务」断言红。
#[test]
fn read_slot_only_swap_out_is_observed_and_resets_read_mark() {
    let (_dir, state) = file_state("swap-out-read");
    let facade = DbFacade::start(&state).expect("门面应启动");
    // 读侧打上「连接不可信」（读线程的标记独立于写线程）。
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

    // 读槽单槽换出（占位内存库）：与恢复 / 转换路径同款调用点。
    state.placeholderize_read_conn().expect("读槽换出应成功");

    // 读侧恢复服务，且用的确是换出后的占位库（无业务表）——换出对门面可见。
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
