//! 连接层统一写入口（`write_locked`，已持锁形态）的置脏语义测试（ADR-0032）：
//! 成功置脏、失败不置脏、闭包内自管事务延迟到提交点，以及目录未配置时不记备份锚点。

use crate::error::AppError;
use tauri_app_lib::test_support::FIXED_NOW;

use super::common::{dirty_state, write_test_state};

// ---------------------------------------------------------------------------
// 连接层统一写入口（write_locked，测试面自取槽锁后直呼——取锁便捷形态已退役，
// issue #1438；置脏语义与门面作业同源同一实现，ADR-0125 决策 2）
// ---------------------------------------------------------------------------

/// 闭包成功且已提交（autocommit）→ 单点置脏；目录未配置时到期检查静默跳过
/// （不记备份锚点）。
#[test]
fn write_ok_marks_dirty() {
    let state = write_test_state();
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |_conn| Ok(())).expect("写入口成功");
    }
    assert!(dirty_state(&state).dirty, "闭包成功后应置脏");
    assert_eq!(
        dirty_state(&state).last_backup_at,
        None,
        "目录未配置不应记录备份锚点"
    );
}

/// 闭包失败 → 不置脏（回滚语义：失败闭包不该留下置脏痕迹）。
#[test]
fn write_err_does_not_mark_dirty() {
    let state = write_test_state();
    let err = {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |_conn| {
            Err::<(), AppError>(AppError::Invalid("boom".into()))
        })
        .unwrap_err()
    };
    assert!(err.to_string().contains("boom"));
    assert!(!dirty_state(&state).dirty, "闭包失败不应置脏");
}

/// 闭包内部自行 BEGIN 且未提交就返回 Ok → is_autocommit 为假，写入口不在
/// 未提交点置脏；回滚后既无数据也无置脏（提交点语义：置脏只发生在提交点）。
#[test]
fn write_inside_open_transaction_defers_to_commit_point() {
    let state = write_test_state();
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |conn| {
            conn.execute("BEGIN", [])?;
            // 任意一笔真实写（未提交）：用调度状态 KV，避开业务表外键；
            // 时刻值为夹具簿记，引用工厂固定时刻常量（ADR-0084 决策 5）。
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from(FIXED_NOW)),
            )?;
            Ok(())
        })
        .expect("闭包成功");
    }
    assert!(!dirty_state(&state).dirty, "未提交不置脏");
    {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("ROLLBACK", []).expect("回滚");
        let hit: Option<String> = crate::settings::get(
            &conn,
            crate::settings::SettingKey::AutoBackupNextDueAt,
            None,
        )
        .unwrap();
        assert_eq!(hit, None, "回滚后写入应消失");
    }
    assert!(!dirty_state(&state).dirty, "回滚后仍不置脏");
}

/// 闭包内部自行 BEGIN/COMMIT 后返回 Ok → 已回到提交点（is_autocommit），
/// 写入口在该点单点置脏（交易修改路径的形态）。
#[test]
fn write_closure_committing_own_tx_marks_dirty() {
    let state = write_test_state();
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |conn| {
            conn.execute("BEGIN", [])?;
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from(FIXED_NOW)),
            )?;
            conn.execute("COMMIT", [])?;
            Ok(())
        })
        .expect("闭包成功");
    }
    assert!(dirty_state(&state).dirty, "提交点应置脏");
}

/// 已持锁形态（ADR-0125 决策 2 / issue #1408）：异步 DB 门面线程持槽锁后经
/// `db::write_locked` 执行——提交点置脏、未提交（显式事务在途）不置脏两条判据
/// 在门面同款调用形态下逐条成立。
#[test]
fn write_locked_marks_dirty_only_at_commit_point() {
    let state = write_test_state();
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |_conn| Ok(())).expect("已持锁形态成功");
    }
    assert!(dirty_state(&state).dirty, "已持锁形态的提交点应置脏");

    // 未提交就返回（事务在途）→ 不置脏；回滚后仍不置脏（提交点复核同源）。
    let state = write_test_state();
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        guard.execute("BEGIN", []).expect("开事务");
        crate::db::write_locked(&guard, |_conn| Ok(())).expect("未提交成功");
        guard.execute("ROLLBACK", []).expect("回滚");
    }
    assert!(!dirty_state(&state).dirty, "未提交不置脏（提交点复核）");
}
