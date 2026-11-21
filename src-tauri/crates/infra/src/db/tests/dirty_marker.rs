use crate::error::AppError;
use tauri_app_lib::test_support::FIXED_NOW;

use super::common::{dirty_state, write_test_state};

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

#[test]
fn write_inside_open_transaction_defers_to_commit_point() {
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

#[test]
fn write_locked_marks_dirty_only_at_commit_point() {
    let state = write_test_state();
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |_conn| Ok(())).expect("已持锁形态成功");
    }
    assert!(dirty_state(&state).dirty, "已持锁形态的提交点应置脏");

    let state = write_test_state();
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        guard.execute("BEGIN", []).expect("开事务");
        crate::db::write_locked(&guard, |_conn| Ok(())).expect("未提交成功");
        guard.execute("ROLLBACK", []).expect("回滚");
    }
    assert!(!dirty_state(&state).dirty, "未提交不置脏（提交点复核）");
}
