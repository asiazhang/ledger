//! 多账本（Book）跨模块用户旅程 BDD 步骤（issue #835 / ADR-0089）。
//!
//! 与 data_location / encryption 两份 feature 同一文件级接缝：真临时目录驱动
//! 真实文件系统，每个 scenario 干净的目录现场，只断言外部可见行为（哪本账本
//! 生效、各自记账互不可见、切回数据原样、密文库落解锁屏、解锁进对应账本、
//! 升级后报表口径不变）。场景现场复用 data_location_steps 的默认目录与旧格式
//! 指针步骤（同一引导组现场字段）；切换与建连消费启动/重引导共用序列内核
//! （`boot::plan_boot` + `db::open_db_in`），登记变更消费引导内核公开函数
//! （`book_registry::create_book_entry` / `switch_active_book`）——测试无应用
//! 运行时，不经壳层命令，与真实 IPC 命令同一实现接缝（先例：BDD 直调命令层
//! 内部函数）。记账经账户域公开创建入口 + L1 输入工厂 + 行为层写入接缝；
//! 报表经命令层同款注入 `monthly_summary_rows` 查询，断言复用 reports_steps
//! 的既有月度汇总步骤（共享层文本，不复制断言）。

use std::path::Path;

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::db::book_registry::{self, Book, BookRegistry, RegistryRead};
use tauri_app_lib::db::boot::BootDisposition;
use tauri_app_lib::db::data_location::{self, DB_FILE_NAME};
use tauri_app_lib::db::encryption::{enable_encryption_for_file, unlock_db_file};
use tauri_app_lib::db::{DbState, open_db_in};
use tauri_app_lib::reports::monthly_summary_rows;

use crate::common::query_accounts_by_name;
use crate::step_inputs::expense_input;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 场景现场与本地辅助
// ---------------------------------------------------------------------------

/// 当前场景的默认应用数据目录（复用 DataLocation 场景现场字段：语义同一——
/// 引导组的默认目录，生目录与旧格式指针 Given 步骤亦复用同字段）。
fn default_dir(world: &LedgerWorld) -> std::path::PathBuf {
    world
        .boot
        .dl_default_dir
        .clone()
        .expect("未准备默认数据目录现场")
}

/// 按展示名解析已登记账本（清单渲染 → 选择的用户路径）：读注册表最新落盘态，
/// 未配置折叠出厂默认账本；损坏与未登记都是场景文本错误（panic）。
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

/// 原位重引导（切换账本的下半场 / 出厂启动）：启动与重引导共用的序列内核
/// 解析生效目录并做处置判定；处置为就绪建连才打开库连接（密文库落解锁屏，
/// 无业务连接，与壳层连接换入同判据）。每次重引导先丢弃旧连接（换入语义）。
fn replan_and_open(world: &mut LedgerWorld) {
    let dir = default_dir(world);
    let plan = tauri_app_lib::db::boot::plan_boot(&dir);
    let disposition = plan.disposition.map_err(|e| e.to_string());
    world.boot.last_boot = Some(plan.boot);
    world.boot.dl_conn = None;
    if disposition.as_ref() == Ok(&BootDisposition::OpenPlaintext) {
        let db_dir = world
            .boot
            .last_boot
            .as_ref()
            .expect("引导结果未登记")
            .db_dir
            .clone();
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

/// 在当前账本库中确保账户存在（名字已登记则复用，否则经账户域公开创建入口
/// 补建——余额缓存行不变量由产品代码保证，#763 旁路归零）并落 count 条支出
///（L1 工具 + 行为层接缝，金额与日期口径与共享种子助手一致）。
fn seed_book_expenses(conn: &rusqlite::Connection, account: &str, count: usize) {
    let id = match conn.query_row(
        "SELECT id FROM accounts WHERE name=?1 AND is_deleted=0",
        params![account],
        |r| r.get::<_, String>(0),
    ) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => tauri_app_lib::accounts::create_account(
            conn,
            tauri_app_lib::accounts::AccountInput {
                name: account.into(),
                kind: tauri_app_lib::accounts::AccountType::Cash,
                currency_code: "CNY".into(),
                initial_balance_cents: Some(0),
            },
        )
        .expect("创建账户失败"),
        Err(e) => panic!("查询账户 '{account}' 失败: {e}"),
    };
    for i in 0..count {
        let input = expense_input(1000 + i as i64, &id, "2026-03-01");
        tauri_app_lib::transaction::create_transaction_internal(conn, input)
            .expect("写入种子支出失败");
    }
}

/// 当前账本的用户侧账户名清单（未删未隐藏；黑洞种子账户不参与可见性断言）。
fn current_book_account_names(world: &LedgerWorld) -> Vec<String> {
    let state = world.boot.dl_conn.as_ref().expect("当前账本无已打开连接");
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    query_accounts_by_name(&conn)
}

// ---------------------------------------------------------------------------
// When：登记变更与原位重引导（旅程主干）
// ---------------------------------------------------------------------------

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
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    seed_book_expenses(&conn, &account, count);
}

#[when(expr = "关闭当前账本连接")]
fn when_close_connection(world: &mut LedgerWorld) {
    // 密文转换前置：丢弃连接句柄，转换后不留指向旧明文文件的悬空连接。
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
    // 解锁成功即引导序列的连接换入：解锁连接成为当前账本连接。
    world.boot.dl_conn = Some(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(unlocked.expect("已断言成功"))),
    });
}

// ---------------------------------------------------------------------------
// When：清单与报表查询
// ---------------------------------------------------------------------------

#[when(expr = "查询账本清单")]
fn when_query_book_list(world: &mut LedgerWorld) {
    let dir = default_dir(world);
    // 与真实命令一致：可变性与回退警示来自启动期已登记的引导结果。
    let boot = world.boot.last_boot.clone();
    world.boot.book_last_list = Some(data_location::gather_book_list(&dir, boot.as_ref()));
}

#[when(expr = "查询当前账本月度汇总 期间 {string} 到 {string}")]
fn when_query_book_monthly_summary(world: &mut LedgerWorld, from: String, to: String) {
    let state = world.boot.dl_conn.as_ref().expect("当前账本无已打开连接");
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    // 期间口径下年份不参与（reports_steps 同款：年份占位传 0）；
    // 结果入报表组快照，断言复用既有月度汇总步骤。
    world.report.last_monthly_summary =
        monthly_summary_rows(&conn, 0, Some(&from), Some(&to)).expect("查询月度汇总失败");
}

// ---------------------------------------------------------------------------
// Then：当前账本、处置、清单与账户可见性断言
// ---------------------------------------------------------------------------

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
