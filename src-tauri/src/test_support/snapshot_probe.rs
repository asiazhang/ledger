use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::trace::{TraceEvent, TraceEventCodes};

use ledger_infra::db::connection::DB_FILE_NAME;
use ledger_infra::db::open_connection;
use ledger_infra::db::tx_scope::hold_transaction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionOutcome {
    Committed,
    Blocked,
    Failed(String),
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
    static PLAN: RefCell<Option<Plan>> = const { RefCell::new(None) };
}

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
    reader.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, Some(on_stmt));
}

pub fn outcome() -> InjectionOutcome {
    PLAN.with(|slot| slot.borrow_mut().take())
        .map(|plan| plan.outcome)
        .unwrap_or(InjectionOutcome::NotFired)
}

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
            if msg.contains("locked") || msg.contains("busy") {
                InjectionOutcome::Blocked
            } else {
                InjectionOutcome::Failed(msg)
            }
        }
    }
}
