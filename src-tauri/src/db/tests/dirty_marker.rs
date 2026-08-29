//! 连接层统一写入口 `db::write` 的置脏语义测试（ADR-0032）：成功置脏、失败不置脏、
//! 闭包内自管事务延迟到提交点，以及目录未配置时不记备份锚点。

use crate::error::AppError;

use super::common::{dirty_state, write_test_state};

// ---------------------------------------------------------------------------
// 连接层统一写入口 db::write（ADR-0032）
// ---------------------------------------------------------------------------

/// 闭包成功且已提交（autocommit）→ 单点置脏；目录未配置时到期检查静默跳过
/// （不记备份锚点）。
#[test]
fn write_ok_marks_dirty() {
    let state = write_test_state();
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    state.write(|_conn| Ok(())).expect("写入口成功");
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
    let err = state
        .write(|_conn| Err::<(), AppError>(AppError::Invalid("boom".into())))
        .unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(!dirty_state(&state).dirty, "闭包失败不应置脏");
}

/// 闭包内部自行 BEGIN 且未提交就返回 Ok → is_autocommit 为假，写入口不在
/// 未提交点置脏；回滚后既无数据也无置脏（提交点语义：置脏只发生在提交点）。
#[test]
fn write_inside_open_transaction_defers_to_commit_point() {
    let state = write_test_state();
    state
        .write(|conn| {
            conn.execute("BEGIN", [])?;
            // 任意一笔真实写（未提交）：用调度状态 KV，避开业务表外键。
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from("2026-01-01T00:00:00Z")),
            )?;
            Ok(())
        })
        .expect("闭包成功");
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
    state
        .write(|conn| {
            conn.execute("BEGIN", [])?;
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from("2026-01-01T00:00:00Z")),
            )?;
            conn.execute("COMMIT", [])?;
            Ok(())
        })
        .expect("闭包成功");
    assert!(dirty_state(&state).dirty, "提交点应置脏");
}
