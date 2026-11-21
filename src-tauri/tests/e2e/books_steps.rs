use std::path::Path;

use cucumber::{then, when};
use rusqlite::params;

use ledger_infra::db::book_registry::{self, Book, BookRegistry, RegistryRead};
use ledger_infra::db::boot::BootDisposition;
use ledger_infra::db::data_location::{self, DB_FILE_NAME};
use ledger_infra::db::encryption::{enable_encryption_for_file, unlock_db_file};
use ledger_infra::db::{DbState, open_connection_readonly_with_passphrase, open_db_in};
use ledger_reports::monthly_summary_rows;

use crate::common::query_accounts_by_name;
use crate::step_inputs::expense_input;
use crate::world::LedgerWorld;

fn default_dir(world: &LedgerWorld) -> std::path::PathBuf {
    world
        .boot
        .dl_default_dir
        .clone()
        .expect("未准备默认数据目录现场")
}

fn registered_book_by_name(default_dir: &Path, name: &str) -> Book {
    let registry = match book_registry::read_registry(default_dir) {
        RegistryRead::Resolved(registry) => registry,
        RegistryRead::Unconfigured => BookRegistry::single_default(default_dir),
        RegistryRead::Corrupt(reason) => panic!("注册表损坏，无法解析账本 '{name}'：{reason}"),
    };
    registry
        .books
        .into_iter()
        .find(|book| book.name == name)
        .unwrap_or_else(|| panic!("账本 '{name}' 未登记，先经新建账本步骤铺垫"))
}

fn replan_and_open(world: &mut LedgerWorld) {
    let dir = default_dir(world);
    let plan = ledger_infra::db::boot::plan_boot(&dir);
    let disposition = plan.disposition.map_err(|e| e.to_string());
    world.boot.last_boot = Some(plan.boot);
    world.boot.dl_conn = None;
    let plaintext = matches!(
        disposition.as_ref(),
        Ok(&BootDisposition::OpenPlaintext) | Ok(&BootDisposition::NormalizePlaintext)
    );
    if plaintext {
        let db_dir = world
            .boot
            .last_boot
            .as_ref()
            .expect("引导结果未登记")
            .db_dir
            .clone();
        if disposition.as_ref() == Ok(&BootDisposition::NormalizePlaintext) {
            ledger_infra::db::encryption::normalize_plaintext_db_file(&db_dir.join(DB_FILE_NAME))
                .expect("外来形态明文库归一化失败");
        }
        let opened = open_db_in(&db_dir);
        assert!(
            opened.is_ok(),
            "引导后打开当前账本库失败: {:?}",
            opened.err()
        );
        world.boot.dl_conn = Some(opened.expect("已断言成功"));
    }
    world.boot.book_last_disposition = Some(disposition);
}

fn seed_book_expenses(conn: &rusqlite::Connection, account: &str, count: usize) {
    let id = match conn.query_row(
        "SELECT id FROM accounts WHERE name=?1 AND is_deleted=0",
        params![account],
        |r| r.get::<_, String>(0),
    ) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => ledger_accounts::create_account(
            conn,
            ledger_accounts::AccountInput {
                name: account.into(),
                kind: ledger_accounts::AccountType::Cash,
                currency_code: "CNY".into(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .expect("创建账户失败"),
        Err(e) => panic!("查询账户 '{account}' 失败: {e}"),
    };
    for i in 0..count {
        let input = expense_input(1000 + i as i64, &id, "2026-03-01");
        ledger_transaction::create_transaction_internal(conn, input).expect("写入种子支出失败");
    }
}

fn current_book_account_names(world: &LedgerWorld) -> Vec<String> {
    let state = world.boot.dl_conn.as_ref().expect("当前账本无已打开连接");
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    query_accounts_by_name(&conn)
}

#[when(expr = "新建账本 {string}")]
fn when_create_book(world: &mut LedgerWorld, name: String) {
    let dir = default_dir(world);
    let created = book_registry::create_book_entry(&dir, &name);
    assert!(
        created.is_ok(),
        "新建账本 '{name}' 失败: {:?}",
        created.err()
    );
}

#[when(expr = "切换到账本 {string} 并原位重引导")]
fn when_switch_book(world: &mut LedgerWorld, name: String) {
    let dir = default_dir(world);
    let book = registered_book_by_name(&dir, &name);
    let switched = book_registry::switch_active_book(&dir, &book.id);
    assert!(
        switched.is_ok(),
        "切换到账本 '{name}' 失败: {:?}",
        switched.err()
    );
    replan_and_open(world);
}

#[when(expr = "执行引导并打开当前账本")]
fn when_boot_and_open(world: &mut LedgerWorld) {
    replan_and_open(world);
}

#[when(expr = "在当前账本记 {int} 条支出到账户 {string}")]
fn when_record_expenses(world: &mut LedgerWorld, count: usize, account: String) {
    let state = world
        .boot
        .dl_conn
        .as_ref()
        .expect("当前账本无已打开连接（引导未就绪建连）");
    let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    ledger_infra::db::write_locked(&guard, |conn| {
        seed_book_expenses(conn, &account, count);
        Ok(())
    })
    .expect("记账失败");
}

#[when(expr = "关闭当前账本连接")]
fn when_close_connection(world: &mut LedgerWorld) {
    world.boot.dl_conn = None;
}

#[when(expr = "用主口令 {string} 开启当前账本加密")]
fn when_enable_encryption(world: &mut LedgerWorld, passphrase: String) {
    let db_dir = world
        .boot
        .last_boot
        .as_ref()
        .expect("尚未执行引导")
        .db_dir
        .clone();
    enable_encryption_for_file(&db_dir.join(DB_FILE_NAME), &passphrase)
        .expect("开启当前账本加密失败");
}

#[when(expr = "以主口令 {string} 解锁当前账本")]
fn when_unlock_book(world: &mut LedgerWorld, passphrase: String) {
    let db_dir = world
        .boot
        .last_boot
        .as_ref()
        .expect("尚未执行引导")
        .db_dir
        .clone();
    let unlocked = unlock_db_file(&db_dir.join(DB_FILE_NAME), &passphrase);
    assert!(unlocked.is_ok(), "解锁当前账本失败: {:?}", unlocked.err());
    let write_conn = unlocked.expect("已断言成功");
    let read_conn =
        open_connection_readonly_with_passphrase(db_dir.join(DB_FILE_NAME), &passphrase)
            .expect("解锁读连接应可开");
    world.boot.dl_conn = Some(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(write_conn)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_conn)),
    });
}

#[when(expr = "查询账本清单")]
fn when_query_book_list(world: &mut LedgerWorld) {
    let dir = default_dir(world);
    let boot = world.boot.last_boot.clone();
    world.boot.book_last_list = Some(data_location::gather_book_list(&dir, boot.as_ref()));
}

#[when(expr = "查询当前账本月度汇总 期间 {string} 到 {string}")]
fn when_query_book_monthly_summary(world: &mut LedgerWorld, from: String, to: String) {
    let state = world.boot.dl_conn.as_ref().expect("当前账本无已打开连接");
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    world.report.last_monthly_summary =
        monthly_summary_rows(&conn, 0, Some(&from), Some(&to)).expect("查询月度汇总失败");
}

#[then(expr = "当前账本应为 {string}")]
fn then_current_book_is(world: &mut LedgerWorld, name: String) {
    let boot = world.boot.last_boot.as_ref().expect("尚未执行引导");
    let active = boot
        .registry
        .as_ref()
        .expect("注册表不可读（引导已回退），无当前账本信息")
        .active()
        .expect("活动账本指针不可解析");
    assert_eq!(active.name, name, "当前账本不符");
}

#[then(expr = "引导处置应等待解锁（落解锁屏）")]
fn then_disposition_awaits_unlock(world: &mut LedgerWorld) {
    let disposition = world
        .boot
        .book_last_disposition
        .as_ref()
        .expect("尚未执行引导");
    assert_eq!(
        disposition.as_ref().ok(),
        Some(&BootDisposition::AwaitUnlock),
        "密文库切换应落解锁屏，实际 {disposition:?}"
    );
}

#[then(expr = "清单应包含 {int} 个账本")]
fn then_list_contains(world: &mut LedgerWorld, count: usize) {
    let list = world
        .boot
        .book_last_list
        .as_ref()
        .expect("尚未查询账本清单");
    assert_eq!(
        list.books.len(),
        count,
        "账本清单条数不符：{:?}",
        list.books
    );
}

#[then(expr = "清单的活动账本应为 {string}")]
fn then_list_active_is(world: &mut LedgerWorld, name: String) {
    let list = world
        .boot
        .book_last_list
        .as_ref()
        .expect("尚未查询账本清单");
    let active_id = list
        .active_id
        .as_ref()
        .expect("清单无活动指针（注册表损坏）");
    let active = list
        .books
        .iter()
        .find(|book| book.id == *active_id)
        .unwrap_or_else(|| panic!("活动指针 {active_id} 悬空"));
    assert_eq!(active.name, name, "清单活动账本不符");
}

#[then(expr = "当前账本应有账户 {string}")]
fn then_book_has_account(world: &mut LedgerWorld, name: String) {
    let names = current_book_account_names(world);
    assert!(
        names.iter().any(|n| n == &name),
        "当前账本应有账户 '{name}'，实际 {names:?}"
    );
}

#[then(expr = "当前账本不应有账户 {string}")]
fn then_book_lacks_account(world: &mut LedgerWorld, name: String) {
    let names = current_book_account_names(world);
    assert!(
        !names.iter().any(|n| n == &name),
        "当前账本不应有账户 '{name}'，实际 {names:?}"
    );
}
