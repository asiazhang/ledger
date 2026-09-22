//! 读快照探针（issue #1699 / #1702）：确定性制造「写提交落在读闭包两条语句之间」的
//! 并发现场，供多语句读一致性的红/绿测试共用（ADR-0084 决策 1 准入：交易 /
//! 投资 / 壳层 / 仪表盘 / 实物资产 / 定时 / 保单七域同体消费，#1699 七处 + #1702 九处）。
//!
//! **机制**：在 reader 连接上注册 `trace_v2` 的 `SQLITE_TRACE_STMT` 回调（语句
//! 开始执行前触发），目标语句（[`marker`] 子串命中）开始前，于**另一条连接**上
//! 原子提交一个注入写事务（[`hold_transaction`] 壳）——落点即「两语句之间」：
//! - 读闭包无快照保护（红）：注入写成功提交，后续语句读到新数据，先读的语句
//!   读到旧数据 → 同屏口径自相矛盾，各用例断言变红；
//! - 读闭包收进读事务（绿）：rollback journal 下读事务持 SHARED 锁横跨语句，
//!   注入写的 COMMIT 拿不到 EXCLUSIVE，busy 超时后放弃——数据未变，口径仍自洽。
//!
//! **单槽与线程局部**（两处既有事实，改前先读）：
//! - SQLite `sqlite3_trace_v2` 是 `(mask, fn)` 单槽：本注册**顶掉**建连收尾安装的
//!   PROFILE 耗时 hook（`perf_trace`）——仅测试连接、仅本用例存活期，perf 日志
//!   缺席无行为影响，不回装；
//! - 回调只收裸 `fn` 指针，状态经线程局部传递（先例：`perf_trace` 的阈值）。
//!   测试线程与用例一一对应，臂装/结局互不串台。
//!
//! **现场要求**：文件库双连接（[`super::open_file`]；内存库按连接隔离，第二连接
//! 是另一个空库）。注入连接取 100ms busy_timeout——读事务挡住提交时测试线程只
//! 等这一拍，不随生产写连接的 5s 容让长挂。
//!
//! **断言契约**：用例读闭包调用后取 [`outcome`] 并断言 ≠ [`InjectionOutcome::NotFired`]
//! ——marker 子串随 SQL 文本漂移即红，防探针静默失敏；一致性断言（总量=分量和 /
//! 与基线同时点）归各用例，形态由各口径自行裁剪。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::trace::{TraceEvent, TraceEventCodes};

use ledger_infra::db::connection::DB_FILE_NAME;
use ledger_infra::db::open_connection;
use ledger_infra::db::tx_scope::hold_transaction;

/// 注入结局（用例判探针命中与否；具体分支只作诊断，不锁死断言——未来隔离级别
/// 变化只要口径仍自洽，用例应保持绿）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionOutcome {
    /// 注入写事务成功提交（读闭包无快照保护时的窗口形态）。
    Committed,
    /// 注入写被读事务挡住、busy 超时后放弃（读事务收口后的形态；数据未变）。
    Blocked,
    /// 注入执行失败（非 busy 的语句错误等——用例应当排查而非放过）。
    Failed(String),
    /// 未命中：marker 从未匹配，或尚未运行被测读闭包。
    NotFired,
}

struct Plan {
    db_dir: PathBuf,
    marker: String,
    statements: Vec<String>,
    fired: bool,
    outcome: InjectionOutcome,
}

thread_local! {
    // 裸 fn 指针回调只能经线程局部传态（先例：perf_trace 阈值）；测试线程与
    // 用例一一对应，arm 覆写、outcome 取走即清。
    static PLAN: RefCell<Option<Plan>> = const { RefCell::new(None) };
}

/// 臂装探针：在 `reader` 上注册 STMT 回调，并登记「marker 命中时在
/// `db_dir/ledger.db` 上原子提交 `statements`」的注入计划。
///
/// marker 是目标语句 SQL 的子串（大小写敏感、单行连续），须满足两条：
/// ① 与被测读闭包**首条**语句不匹配（首条之前读事务尚未取 SHARED 锁，注入会整体
///    落在快照外、绿侧断言失去意义）；② 命中点在其之后仍有被测读语句（否则没有
///   「之后读到新数据」的观察面）。一次性闩存——至多注入一次。
pub fn arm(reader: &Connection, db_dir: &Path, marker: &str, statements: &[&str]) {
    PLAN.with(|slot| {
        *slot.borrow_mut() = Some(Plan {
            db_dir: db_dir.to_path_buf(),
            marker: marker.to_string(),
            statements: statements.iter().map(|s| (*s).to_string()).collect(),
            fired: false,
            outcome: InjectionOutcome::NotFired,
        });
    });
    // trace_v2 单槽：STMT 注册顶掉建连收尾的 PROFILE 耗时 hook（见模块文档，
    // 测试连接可接受）。
    reader.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, Some(on_stmt));
}

/// 取注入结局并解除武装。未臂装或 marker 未命中返回 [`InjectionOutcome::NotFired`]
/// （用例的命中断言即靠它变红）。
pub fn outcome() -> InjectionOutcome {
    PLAN.with(|slot| slot.borrow_mut().take())
        .map(|plan| plan.outcome)
        .unwrap_or(InjectionOutcome::NotFired)
}

/// STMT 回调：marker 命中且未触发过时，先执行注入写、再放行目标语句——写落在
/// 两语句之间。回调内不开嵌套 PLAN 借用（注入不触线程局部），rusqlite 侧
/// `catch_unwind` 吞 panic，故本函数不 panic。
fn on_stmt(event: TraceEvent<'_>) {
    let TraceEvent::Stmt(_, sql) = event else {
        return;
    };
    PLAN.with(|slot| {
        let mut plan_ref = slot.borrow_mut();
        let Some(plan) = plan_ref.as_mut() else {
            return;
        };
        if plan.fired || !sql.contains(&plan.marker) {
            return;
        }
        plan.fired = true;
        plan.outcome = inject(&plan.db_dir, &plan.statements);
    });
}

/// 在库目录上开一条独立连接并原子提交注入写：BEGIN 壳经 `hold_transaction`
/// （原生事务语句唯一住址守门 NATIVE_TX_STMT，测试源不手写 BEGIN/COMMIT），
/// 短 busy_timeout 保证读事务挡道时本线程限时退出。
fn inject(db_dir: &Path, statements: &[String]) -> InjectionOutcome {
    let conn = match open_connection(db_dir.join(DB_FILE_NAME)) {
        Ok(conn) => conn,
        Err(e) => return InjectionOutcome::Failed(e.to_string()),
    };
    if let Err(e) = conn.busy_timeout(Duration::from_millis(100)) {
        return InjectionOutcome::Failed(e.to_string());
    }
    match hold_transaction(&conn, || {
        for sql in statements {
            conn.execute(sql, [])?;
        }
        Ok(())
    }) {
        Ok(()) => InjectionOutcome::Committed,
        Err(e) => {
            let msg = e.to_string();
            // busy 归类靠错误文本（AppError::Db 透传 rusqlite 文案：
            // "database is locked" 等）；其余归 Failed 供用例排查。
            if msg.contains("locked") || msg.contains("busy") {
                InjectionOutcome::Blocked
            } else {
                InjectionOutcome::Failed(msg)
            }
        }
    }
}
