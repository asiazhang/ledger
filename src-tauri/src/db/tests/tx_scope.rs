//! 事务作用域原语 `db::tx_scope::{ensure_transaction, hold_transaction}` 的行为
//! 单测（issue #1013 / #1014，ADR-0033 决策 2）：`ensure` 锁定嵌套加入外层 /
//! 自持失败整体回滚 / COMMIT 失败尽力回滚清理 / ROLLBACK 自身失败不遮蔽原错误；
//! `hold` 锁定无条件自持提交 / 中途失败整体回滚 / 已在事务中自然 BEGIN 报错 /
//! 失败语义与 ensure 同款（COMMIT 失败清理、ROLLBACK 不遮蔽）。断言对准数据终态
//! 与报错内容，不断言事务写法（ADR-0033 测试定案）；失败注入用纯测试侧手段
//! （SQLite 触发器 RAISE(ABORT) / RAISE(ROLLBACK)、延迟外键），产品代码零 hook。

use super::super::tx_scope::{ensure_transaction, hold_transaction};
use crate::error::AppError;
use crate::test_support;

/// 嵌套加入外层（嵌套模式，issue #310 合法使用者语义）：连接已在事务中时，原语
/// 不再 BEGIN/COMMIT——Ok 与 Err 都原样透传，回滚权归外层持有者。
#[test]
fn nested_calls_join_outer_transaction_and_leave_rollback_to_holder() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE t(x INTEGER)", []).unwrap();

    conn.execute("BEGIN", []).unwrap();
    // Ok 分支：原语不得自行提交——连接仍处于外层事务。
    ensure_transaction(&conn, || {
        conn.execute("INSERT INTO t VALUES (1)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap();
    assert!(!conn.is_autocommit(), "嵌套模式不得自行提交");

    // Err 分支：失败直接返回错误，原语不回滚——外层事务仍在原样进行。
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

    // 外层持有者回滚 → 全部消失（回滚归外层持有者）。
    conn.execute("ROLLBACK", []).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "外层回滚应清掉嵌套模式的全部写入");
}

/// 自持失败整体回滚：autocommit 连接上自持事务，中途失败（BEFORE INSERT 触发器
/// RAISE(ABORT) 挡下第二条写入，纯测试侧注入，先例 spec #169）整体回滚——先落
/// 的第一条写入不得残留，连接回到 autocommit，无中间态泄漏。
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

/// COMMIT 失败尽力回滚清理：`f` 全部成功但 COMMIT 被库层拒绝（延迟外键在提交点
/// 集中检查，纯测试侧注入），原语尽力 ROLLBACK 清理残留后上抛提交错误——连接
/// 回到 autocommit、违规行不残留。
#[test]
fn commit_failure_rolls_back_and_surfaces_commit_error() {
    // 建连收尾单点已开启外键（finish_open）。
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
        // 延迟外键：语句期不检查，COMMIT 期集中检查——`f` 本身全程「成功」。
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

/// ROLLBACK 自身失败不遮蔽原错误：触发器 RAISE(ROLLBACK) 使 SQLite 隐式回滚整个
/// 事务（纯测试侧注入，先例 ADR-0033 测试定案的 RAISE 家族）——事务已不存在时原语
/// 的 ROLLBACK 自身必然失败（no transaction active，被尽力回滚忽略），上抛的仍是
/// 原错误。注：现代 SQLite 对小事务的 pending query / 第二连接共享锁不再令显式
/// ROLLBACK 失败（实测 2026-09），RAISE(ROLLBACK) 是确定性注入手段。
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
        // RAISE(ROLLBACK)：语句错误 + 事务被 SQLite 隐式回滚——原语的 ROLLBACK
        // 将自身失败（no transaction active），不得改写或遮蔽本错误。
        conn.execute("INSERT INTO blocked VALUES (2)", [])
            .map_err(AppError::from)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Db(ref m) if m.contains("测试注入：隐式回滚")),
        "ROLLBACK 自身失败不得遮蔽原错误，实际 {err:?}"
    );
    // 隐式回滚已清场：先落的写入一并消失，连接回到 autocommit。
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM landed", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "隐式回滚应清掉事务内全部写入");
    assert!(conn.is_autocommit(), "事务已被隐式回滚关闭");
}

/// `hold_transaction` 无条件自持提交：autocommit 连接上自持 BEGIN/COMMIT，
/// `f` 成功即落库、连接回到 autocommit。批量导入 / 引擎期次 / 余额调整的外层
/// 事务壳语义（issue #1014）——不检测连接事务状态、无条件 BEGIN。
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

/// `hold_transaction` 中途失败整体回滚：与 `ensure` 自持分支同款失败语义——
/// `f` 中途失败整体回滚（先落的写入不残留），连接回到 autocommit。
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

/// `hold_transaction` 已在事务中：自然 BEGIN 报错、不加码化错误，`f` 不执行、
/// 外层事务原样——回滚权仍归外层持有者（无事务状态查询，协议知识不泄漏）。
#[test]
fn hold_transaction_on_open_transaction_fails_with_natural_begin_error() {
    let conn = test_support::open();
    conn.execute("CREATE TABLE t(x INTEGER)", []).unwrap();
    conn.execute("BEGIN", []).unwrap();

    let err = hold_transaction::<()>(&conn, || {
        panic!("已在事务中：BEGIN 失败应先于 f 执行，f 不得被调用");
    })
    .unwrap_err();
    // 自然报错：信息来自 SQLite（AppError::Db），不是本仓码化错误构造器。
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

/// `hold_transaction` COMMIT 失败尽力回滚清理：与 `ensure` 同款——`f` 全程成功但
/// COMMIT 被库层拒绝（延迟外键），原语尽力 ROLLBACK 清理残留后上抛提交错误。
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

/// `hold_transaction` ROLLBACK 自身失败不遮蔽原错误：与 `ensure` 同款——触发器
/// RAISE(ROLLBACK) 使 SQLite 隐式回滚整个事务，事务已不存在时原语的 ROLLBACK 自身
/// 失败（被忽略），上抛的仍是原错误。
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
