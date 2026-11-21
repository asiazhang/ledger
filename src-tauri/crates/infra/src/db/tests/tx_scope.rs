use super::super::tx_scope::{ensure_transaction, hold_transaction};
use crate::error::AppError;
use tauri_app_lib::test_support;

#[test]
fn nested_calls_join_outer_transaction_and_leave_rollback_to_holder() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE t(x INTEGER)", []).unwrap();

    conn.execute("BEGIN", []).unwrap();
    ensure_transaction(&conn, || {
        conn.execute("INSERT INTO t VALUES (1)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap();
    assert!(!conn.is_autocommit(), "嵌套模式不得自行提交");

    let err = ensure_transaction::<()>(&conn, || {
        conn.execute("INSERT INTO t VALUES (2)", [])
            .map_err(AppError::from)?;
        Err(AppError::Invalid("嵌套失败".into()))
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Invalid(ref m) if m == "嵌套失败"),
        "嵌套失败应原样透传，实际 {err:?}"
    );
    assert!(!conn.is_autocommit(), "嵌套失败不得回滚外层事务");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2, "两次写入都还在外层事务内，未被原语裁决");

    conn.execute("ROLLBACK", []).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "外层回滚应清掉嵌套模式的全部写入");
}

#[test]
fn self_held_failure_rolls_back_everything() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE landed(x INTEGER)", []).unwrap();
    conn.execute("CREATE TABLE blocked(x INTEGER)", []).unwrap();
    conn.execute(
        "CREATE TRIGGER guard BEFORE INSERT ON blocked \
         BEGIN SELECT RAISE(ABORT, '测试注入：写失败'); END",
        [],
    )
    .unwrap();

    let err = ensure_transaction(&conn, || {
        conn.execute("INSERT INTO landed VALUES (1)", [])
            .map_err(AppError::from)?;
        conn.execute("INSERT INTO blocked VALUES (2)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("测试注入：写失败")),
        "中途失败错误应上抛，实际 {err:?}"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM landed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "自持事务中途失败应整体回滚，第一条写入不得残留");
    assert!(
        conn.is_autocommit(),
        "失败后应回到 autocommit（无残留事务）"
    );
}

#[test]
fn commit_failure_rolls_back_and_surfaces_commit_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE parent(id INTEGER PRIMARY KEY)", [])
        .unwrap();
    conn.execute(
        "CREATE TABLE child(id INTEGER PRIMARY KEY, pid INTEGER NOT NULL \
         REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)",
        [],
    )
    .unwrap();

    let err = ensure_transaction(&conn, || {
        conn.execute("INSERT INTO child (id, pid) VALUES (1, 999)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("FOREIGN KEY")),
        "应上抛 COMMIT 阶段的外键错误，实际 {err:?}"
    );
    assert!(
        conn.is_autocommit(),
        "COMMIT 失败后应尽力回滚清理，回到 autocommit"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM child", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "违规行应随尽力回滚清理，不残留");
}

#[test]
fn rollback_failure_does_not_mask_original_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE landed(x INTEGER)", []).unwrap();
    conn.execute("CREATE TABLE blocked(x INTEGER)", []).unwrap();
    conn.execute(
        "CREATE TRIGGER guard BEFORE INSERT ON blocked \
         BEGIN SELECT RAISE(ROLLBACK, '测试注入：隐式回滚'); END",
        [],
    )
    .unwrap();

    let err = ensure_transaction(&conn, || {
        conn.execute("INSERT INTO landed VALUES (1)", [])
            .map_err(AppError::from)?;
        conn.execute("INSERT INTO blocked VALUES (2)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("测试注入：隐式回滚")),
        "ROLLBACK 自身失败不得遮蔽原错误，实际 {err:?}"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM landed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "隐式回滚应清掉事务内全部写入");
    assert!(conn.is_autocommit(), "事务已被隐式回滚关闭");
}

#[test]
fn hold_transaction_commits_unconditionally_on_autocommit() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE t(x INTEGER)", []).unwrap();

    let v = hold_transaction(&conn, || {
        conn.execute("INSERT INTO t VALUES (1)", [])
            .map_err(AppError::from)?;
        Ok("done")
    })
    .unwrap();
    assert_eq!(v, "done");
    assert!(
        conn.is_autocommit(),
        "无条件自持成功后应提交回到 autocommit"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "成功写入应已提交落库");
}

#[test]
fn hold_transaction_rolls_back_everything_on_failure() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE landed(x INTEGER)", []).unwrap();
    conn.execute("CREATE TABLE blocked(x INTEGER)", []).unwrap();
    conn.execute(
        "CREATE TRIGGER guard BEFORE INSERT ON blocked \
         BEGIN SELECT RAISE(ABORT, '测试注入：写失败'); END",
        [],
    )
    .unwrap();

    let err = hold_transaction(&conn, || {
        conn.execute("INSERT INTO landed VALUES (1)", [])
            .map_err(AppError::from)?;
        conn.execute("INSERT INTO blocked VALUES (2)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("测试注入：写失败")),
        "中途失败错误应上抛，实际 {err:?}"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM landed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "无条件自持中途失败应整体回滚，第一条写入不得残留");
    assert!(conn.is_autocommit(), "失败后应回到 autocommit");
}

#[test]
fn hold_transaction_on_open_transaction_fails_with_natural_begin_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE t(x INTEGER)", []).unwrap();
    conn.execute("BEGIN", []).unwrap();

    let err = hold_transaction::<()>(&conn, || {
        panic!("已在事务中：BEGIN 失败应先于 f 执行，f 不得被调用");
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("within a transaction")),
        "已在事务中应自然 BEGIN 报错，实际 {err:?}"
    );
    assert!(!conn.is_autocommit(), "报错不得动外层事务");
    conn.execute("INSERT INTO t VALUES (1)", []).unwrap();
    conn.execute("ROLLBACK", []).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "外层事务仍由持有者掌控，回滚清场");
}

#[test]
fn hold_transaction_commit_failure_rolls_back_and_surfaces_commit_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE parent(id INTEGER PRIMARY KEY)", [])
        .unwrap();
    conn.execute(
        "CREATE TABLE child(id INTEGER PRIMARY KEY, pid INTEGER NOT NULL \
         REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)",
        [],
    )
    .unwrap();

    let err = hold_transaction(&conn, || {
        conn.execute("INSERT INTO child (id, pid) VALUES (1, 999)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("FOREIGN KEY")),
        "应上抛 COMMIT 阶段的外键错误，实际 {err:?}"
    );
    assert!(conn.is_autocommit(), "COMMIT 失败后应尽力回滚清理");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM child", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "违规行应随尽力回滚清理，不残留");
}

#[test]
fn hold_transaction_rollback_failure_does_not_mask_original_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE landed(x INTEGER)", []).unwrap();
    conn.execute("CREATE TABLE blocked(x INTEGER)", []).unwrap();
    conn.execute(
        "CREATE TRIGGER guard BEFORE INSERT ON blocked \
         BEGIN SELECT RAISE(ROLLBACK, '测试注入：隐式回滚'); END",
        [],
    )
    .unwrap();

    let err = hold_transaction(&conn, || {
        conn.execute("INSERT INTO landed VALUES (1)", [])
            .map_err(AppError::from)?;
        conn.execute("INSERT INTO blocked VALUES (2)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("测试注入：隐式回滚")),
        "ROLLBACK 自身失败不得遮蔽原错误，实际 {err:?}"
    );
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM landed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "隐式回滚应清掉事务内全部写入");
    assert!(conn.is_autocommit(), "事务已被隐式回滚关闭");
}
