//! 加密转换三形态 BDD 步骤（issue #570/#571 / ADR-0075）。
//!
//! 与 data_location.feature 同一文件级接缝：真临时目录驱动真实文件系统，
//! 每个 scenario 干净的目录现场，只断言外部可见行为（转换往返、启动探测
//! 接管、失败原子性、口令错误可重试、关闭/改口令对称形态、搬迁后仍为密
//! 文库）。步骤直调 db 基础设施的实现接缝（`enable_encryption_for_file`
//! / `disable_encryption_for_file` / `change_passphrase_for_file` /
//! `unlock_db_file` / `boot` / `relocate_with_key`），与真实 IPC 命令同一
//! 实现（先例：BDD 直调命令层内部函数）。

use cucumber::{given, then, when};
use rusqlite::Connection;

use tauri_app_lib::db::data_location::relocate_with_key;
use tauri_app_lib::db::data_location::{self, DB_FILE_NAME};
use tauri_app_lib::db::encryption::{
    DbFileKind, change_passphrase_for_file, disable_encryption_for_file,
    enable_encryption_for_file, probe_file_kind, reset_encrypted_db_file, unlock_db_file,
};
use tauri_app_lib::db::{init_db, new_uuid, open_connection, open_connection_with_passphrase};
use tauri_app_lib::error::AppError;
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::create_transaction_internal;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 场景现场与本地辅助
// ---------------------------------------------------------------------------

fn ensure_dir(world: &mut LedgerWorld) {
    if world.enc_dir.is_none() {
        let dir = std::env::temp_dir().join(format!("ledger-e2e-enc-{}", new_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        world.enc_dir = Some(dir.clone());
        // 同步登记到 DataLocation 场景现场：生目录断言类步骤（"生效目录应为
        // 默认数据目录"）复用 data_location_steps 的既有定义，不重复声明。
        world.dl_default_dir = Some(dir);
    }
}

fn db_path(world: &LedgerWorld) -> std::path::PathBuf {
    world.enc_dir.as_ref().unwrap().join(DB_FILE_NAME)
}

/// 在文件库中建账户与 N 条交易（经 Writer/行为层接缝，含余额缓存行不变量）。
fn seed_db(conn: &Connection, count: usize) {
    let account_id = new_uuid();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![account_id, "现金"],
    )
    .unwrap();
    tauri_app_lib::accounts::balance::refresh_account_balances(conn, &[account_id.as_str()])
        .unwrap();
    for i in 0..count {
        let input = TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: account_id.clone(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: Some(format!("加密种子交易 {i}")),
            date: "2026-03-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        };
        create_transaction_internal(conn, input).unwrap();
    }
}

fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn count_transactions_in_file(db: &std::path::Path, passphrase: Option<&str>) -> i64 {
    let conn = match passphrase {
        Some(p) => open_connection_with_passphrase(db, p).unwrap(),
        None => open_connection(db).unwrap(),
    };
    count_transactions(&conn)
}

/// 断言库中种子交易完整：数量一致且种子备注逐条在（转换往返不丢内容）。
fn assert_seed_rows(conn: &Connection, count: usize) {
    assert_eq!(count_transactions(conn) as usize, count, "交易数不符");
    let notes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE note LIKE '加密种子交易 %'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notes as usize, count, "种子交易内容应完整保留");
}

fn code_of(err: &AppError) -> Option<&str> {
    match err {
        AppError::Coded { code, .. } => Some(code),
        _ => None,
    }
}

/// 抽取交易表内容快照（排序后拼接），供转换前后一致性比对。
fn transactions_fingerprint(conn: &Connection) -> String {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, amount_cents, date, note FROM transactions WHERE is_deleted = 0 ORDER BY id",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(format!(
                "{}|{}|{}|{}|{}",
                r.get::<_, String>(0).unwrap(),
                r.get::<_, String>(1).unwrap(),
                r.get::<_, i64>(2).unwrap(),
                r.get::<_, String>(3).unwrap(),
                r.get::<_, Option<String>>(4).unwrap().unwrap_or_default(),
            ))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "默认数据目录中已有一个含 {int} 条交易的明文库")]
fn given_plaintext_db(world: &mut LedgerWorld, count: usize) {
    ensure_dir(world);
    let mut conn = open_connection(db_path(world)).unwrap();
    init_db(&mut conn).unwrap();
    seed_db(&conn, count);
}

#[given(expr = "默认数据目录中有一个凭主口令 {string} 加密且含 {int} 条交易的密文库")]
fn given_encrypted_db(world: &mut LedgerWorld, passphrase: String, count: usize) {
    ensure_dir(world);
    let path = db_path(world);
    {
        let mut conn = open_connection(&path).unwrap();
        init_db(&mut conn).unwrap();
        seed_db(&conn, count);
    }
    enable_encryption_for_file(&path, &passphrase).unwrap();
    // 场景现场收尾：.bak 副本不入现场（它是转换的副产物，后续场景自己断言）。
    std::fs::remove_file(path.with_extension("db.bak")).unwrap();
}

#[given(expr = "记录当前库文件字节")]
fn given_record_bytes(world: &mut LedgerWorld) {
    world.enc_db_bytes = Some(std::fs::read(db_path(world)).unwrap());
}

#[given(expr = "指针文件指向空的目标目录")]
fn given_pointer_to_empty_target(world: &mut LedgerWorld) {
    ensure_dir(world);
    let default_dir = world.enc_dir.clone().unwrap();
    let target = std::env::temp_dir().join(format!("ledger-e2e-enc-target-{}", new_uuid()));
    std::fs::create_dir_all(&target).unwrap();
    data_location::write_pointer(&default_dir, &target).unwrap();
    world.enc_target_dir = Some(target);
}

#[given(expr = "数据目录不可写")]
fn given_dir_readonly(world: &mut LedgerWorld) {
    let dir = world.enc_dir.clone().unwrap();
    // Unix 权限位（非 root 下真实生效；root 绕过权限检查，不可依赖）。
    // 先置只读再进入转换，转换结束后由 Then 侧恢复权限。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "执行启动引导")]
fn when_boot(world: &mut LedgerWorld) {
    let default_dir = world.enc_dir.clone().unwrap();
    world.last_boot = Some(data_location::boot(&default_dir));
}

#[when(expr = "用主口令 {string} 开启加密")]
fn when_enable_encryption(world: &mut LedgerWorld, passphrase: String) {
    world.enc_last_error = None;
    if let Err(e) = enable_encryption_for_file(&db_path(world), &passphrase) {
        world.enc_last_error = Some(e);
    }
}

#[when(expr = "用当前主口令 {string} 关闭加密")]
fn when_disable_encryption(world: &mut LedgerWorld, passphrase: String) {
    world.enc_last_error = None;
    if let Err(e) = disable_encryption_for_file(&db_path(world), &passphrase) {
        world.enc_last_error = Some(e);
    }
}

#[when(expr = "用旧口令 {string} 与新口令 {string} 修改主口令")]
fn when_change_passphrase(world: &mut LedgerWorld, current: String, new_pass: String) {
    world.enc_last_error = None;
    if let Err(e) = change_passphrase_for_file(&db_path(world), &current, &new_pass) {
        world.enc_last_error = Some(e);
    }
}

#[when(expr = "以主口令 {string} 解锁")]
fn when_unlock(world: &mut LedgerWorld, passphrase: String) {
    world.enc_last_error = None;
    match unlock_db_file(&db_path(world), &passphrase) {
        Ok(conn) => world.enc_conn = Some(conn),
        Err(e) => world.enc_last_error = Some(e),
    }
}

#[when(expr = "以主口令 {string} 再次解锁")]
fn when_unlock_again(world: &mut LedgerWorld, passphrase: String) {
    when_unlock(world, passphrase);
}

#[when(expr = "以主口令 {string} 解锁并补做等待中的搬迁")]
fn when_unlock_and_relocate(world: &mut LedgerWorld, passphrase: String) {
    world.enc_last_error = None;
    match unlock_db_file(&db_path(world), &passphrase) {
        Ok(conn) => world.enc_conn = Some(conn),
        Err(e) => {
            world.enc_last_error = Some(e);
            return;
        }
    }
    let default_dir = world.enc_dir.clone().unwrap();
    let target = world.enc_target_dir.clone().unwrap();
    if let Err(e) = relocate_with_key(
        &default_dir.join(DB_FILE_NAME),
        &target.join(DB_FILE_NAME),
        &passphrase,
    ) {
        world.enc_last_error = Some(e);
    }
}

#[when(expr = "执行忘记口令重置")]
fn when_reset_forgotten(world: &mut LedgerWorld) {
    world.enc_last_error = None;
    match reset_encrypted_db_file(&db_path(world)) {
        Ok(conn) => world.enc_conn = Some(conn),
        Err(e) => world.enc_last_error = Some(e),
    }
}

#[when(expr = "不带口令打开 .bak 密文副本")]
fn when_open_bak_without_key(world: &mut LedgerWorld) {
    let bak = db_path(world).with_extension("db.bak");
    world.enc_last_error = (|| -> Result<i64, AppError> {
        let conn = open_connection(&bak)?;
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )?)
    })()
    .err();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "库文件应探测为明文库")]
fn then_probe_plaintext(world: &mut LedgerWorld) {
    assert_eq!(
        probe_file_kind(&db_path(world)).unwrap(),
        DbFileKind::Plaintext,
        "当前库文件应探测为明文库"
    );
}

#[then(expr = "库文件应探测为密文库")]
fn then_probe_encrypted(world: &mut LedgerWorld) {
    assert_eq!(
        probe_file_kind(&db_path(world)).unwrap(),
        DbFileKind::Encrypted,
        "转换后的库文件应为密文库"
    );
}

#[then(expr = "引导不应等待解锁")]
fn then_no_deferred(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    assert!(
        boot.deferred_relocation.is_none(),
        "明文库引导不应等待解锁后搬迁"
    );
}

#[then(expr = "引导不应发生回退")]
fn then_no_fallback(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    assert!(
        boot.fallback_reason.is_none(),
        "不应发生回退，实际: {:?}",
        boot.fallback_reason
    );
}

#[then(expr = "转换应成功")]
fn then_convert_ok(world: &mut LedgerWorld) {
    assert!(
        world.enc_last_error.is_none(),
        "转换应成功，实际: {:?}",
        world.enc_last_error
    );
}

#[then(expr = "转换应失败")]
fn then_convert_failed(world: &mut LedgerWorld) {
    assert!(
        world.enc_last_error.is_some(),
        "预期转换失败（目录不可写），实际成功"
    );
    // 恢复目录权限，便于临时目录清理与后续断言。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = world.enc_dir.clone().unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[then(expr = "原库文件应保留为 .bak 明文副本")]
fn then_bak_preserved(world: &mut LedgerWorld) {
    let bak = db_path(world).with_extension("db.bak");
    assert!(bak.exists(), "原明文库应保留为 .bak 副本");
    assert_eq!(
        probe_file_kind(&bak).unwrap(),
        DbFileKind::Plaintext,
        ".bak 副本应为明文库"
    );
}

#[then(expr = "原库文件应保留为 .bak 密文副本")]
fn then_bak_preserved_encrypted(world: &mut LedgerWorld) {
    let bak = db_path(world).with_extension("db.bak");
    assert!(bak.exists(), "原密文库应保留为 .bak 副本");
    assert_eq!(
        probe_file_kind(&bak).unwrap(),
        DbFileKind::Encrypted,
        ".bak 副本应为密文库"
    );
}

#[then(expr = "明文打开当前库应包含 {int} 条交易且内容完整")]
fn then_open_plaintext(world: &mut LedgerWorld, count: usize) {
    let conn = open_connection(db_path(world)).unwrap();
    assert_seed_rows(&conn, count);
}

#[then(expr = "凭主口令 {string} 打开 .bak 副本应包含 {int} 条交易且内容完整")]
fn then_open_bak_with_passphrase(world: &mut LedgerWorld, passphrase: String, count: usize) {
    let bak = db_path(world).with_extension("db.bak");
    let conn = open_connection_with_passphrase(&bak, &passphrase).unwrap();
    assert_seed_rows(&conn, count);
}

#[then(expr = "转换失败错误码应为 {string}")]
fn then_convert_failed_with_code(world: &mut LedgerWorld, code: String) {
    let error = world.enc_last_error.as_ref().expect("预期转换失败");
    assert_eq!(
        code_of(error),
        Some(code.as_str()),
        "错误码不匹配，实际: {error}"
    );
}

#[then(expr = "凭主口令 {string} 打开当前库应包含 {int} 条交易且内容完整")]
fn then_open_with_passphrase(world: &mut LedgerWorld, passphrase: String, count: usize) {
    let path = db_path(world);
    let conn = open_connection_with_passphrase(&path, &passphrase).unwrap();
    // 内容完整：与转换前 seed 的确定性内容比对（同序同值）。
    assert_seed_rows(&conn, count);
}

#[then(expr = "当前库文件字节应保持不变且仍为明文库")]
fn then_bytes_unchanged_and_plaintext(world: &mut LedgerWorld) {
    let current = std::fs::read(db_path(world)).unwrap();
    assert_eq!(
        current,
        *world.enc_db_bytes.as_ref().expect("未记录字节快照"),
        "失败场景中原库文件字节不应被改动"
    );
    assert_eq!(
        probe_file_kind(&db_path(world)).unwrap(),
        DbFileKind::Plaintext,
        "失败后原库应仍为明文库"
    );
}

#[then(expr = "当前库文件字节应保持不变")]
fn then_bytes_unchanged(world: &mut LedgerWorld) {
    let current = std::fs::read(db_path(world)).unwrap();
    assert_eq!(
        current,
        *world.enc_db_bytes.as_ref().expect("未记录字节快照"),
        "失败重试不得改动库文件"
    );
}

#[then(expr = "原库仍能以明文打开且包含 {int} 条交易")]
fn then_still_plaintext_readable(world: &mut LedgerWorld, count: usize) {
    assert_eq!(
        count_transactions_in_file(&db_path(world), None) as usize,
        count,
        "原库应仍能以明文打开且数据完整"
    );
}

#[then(expr = "目录中不应残留转换临时文件或 .bak 副本")]
fn then_no_leftovers(world: &mut LedgerWorld) {
    let dir = world.enc_dir.clone().unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|name| !name.starts_with(".ledger.db.")),
        "不应残留转换临时文件: {names:?}"
    );
    assert!(
        !db_path(world).with_extension("db.bak").exists(),
        "失败时不应产生 .bak 副本"
    );
}

#[then(expr = "重置应成功")]
fn then_reset_ok(world: &mut LedgerWorld) {
    assert!(
        world.enc_last_error.is_none(),
        "重置应成功，实际: {:?}",
        world.enc_last_error
    );
}

#[then(expr = "重置后的新库应为不含交易的明文空库")]
fn then_reset_fresh_empty(world: &mut LedgerWorld) {
    let conn = world.enc_conn.as_ref().expect("重置后应有新库连接");
    assert_eq!(count_transactions(conn), 0, "重置后的新库应为空库");
}

#[then(expr = "原密文库应保留为 .bak 密文副本")]
fn then_bak_encrypted_preserved(world: &mut LedgerWorld) {
    let bak = db_path(world).with_extension("db.bak");
    assert!(bak.exists(), "原密文库应保留为 .bak 副本");
    assert_eq!(
        probe_file_kind(&bak).unwrap(),
        DbFileKind::Encrypted,
        ".bak 副本应为密文库（头部为密文，无密钥不可读）"
    );
}

#[then(expr = ".bak 密文副本凭原主口令 {string} 仍可打开且包含 {int} 条交易")]
fn then_bak_recoverable_with_passphrase(world: &mut LedgerWorld, passphrase: String, count: usize) {
    let bak = db_path(world).with_extension("db.bak");
    let conn = open_connection_with_passphrase(&bak, &passphrase).unwrap();
    assert_eq!(
        count_transactions(&conn) as usize,
        count,
        "日后想起口令时，副本数据应可完整救回"
    );
}

#[then(expr = "打开应失败且错误为文件不可读")]
fn then_bak_unreadable_without_key(world: &mut LedgerWorld) {
    let error = world
        .enc_last_error
        .as_ref()
        .expect("无密钥打开密文副本应失败");
    let text = error.to_string();
    assert!(
        text.contains("not a database"),
        "无密钥读取密文库应报文件不可读（not a database），实际: {text}"
    );
}

#[then(expr = "解锁应成功且打开的库应包含 {int} 条交易")]
fn then_unlocked_with_count(world: &mut LedgerWorld, count: usize) {
    assert!(
        world.enc_last_error.is_none(),
        "解锁应成功，实际: {:?}",
        world.enc_last_error
    );
    let conn = world.enc_conn.as_ref().expect("解锁后应有连接");
    assert_eq!(count_transactions(conn) as usize, count);
}

#[then(expr = "解锁应失败且错误码为 {string}")]
fn then_unlock_failed_with_code(world: &mut LedgerWorld, code: String) {
    let error = world.enc_last_error.as_ref().expect("预期解锁失败");
    assert_eq!(
        code_of(error),
        Some(code.as_str()),
        "错误码不匹配，实际: {error}"
    );
}

#[then(expr = "凭主口令 {string} 可再次打开当前库")]
fn then_reopen_with_passphrase(world: &mut LedgerWorld, passphrase: String) {
    let conn = open_connection_with_passphrase(db_path(world), &passphrase).unwrap();
    assert!(count_transactions(&conn) >= 0, "凭主口令可再次打开并读取");
}

#[then(expr = "引导应等待解锁后搬迁到目标目录")]
fn then_deferred_relocation(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let target = world.enc_target_dir.as_ref().unwrap();
    assert_eq!(
        boot.deferred_relocation.as_deref(),
        Some(target.as_path()),
        "引导应携带等待解锁后搬迁的目标目录"
    );
    assert!(
        boot.fallback_reason.is_none(),
        "推迟搬迁不是回退，不应携带回退警示"
    );
}

#[then(expr = "搬迁应成功")]
fn then_relocate_ok(world: &mut LedgerWorld) {
    assert!(
        world.enc_last_error.is_none(),
        "搬迁应成功，实际: {:?}",
        world.enc_last_error
    );
}

#[then(expr = "目标目录的库应探测为密文库")]
fn then_target_encrypted(world: &mut LedgerWorld) {
    let target = world.enc_target_dir.as_ref().unwrap();
    assert_eq!(
        probe_file_kind(&target.join(DB_FILE_NAME)).unwrap(),
        DbFileKind::Encrypted,
        "搬迁后的目标库应仍为密文库（加密状态随文件走）"
    );
}

#[then(expr = "凭主口令 {string} 打开目标目录的库应包含 {int} 条交易且内容完整")]
fn then_target_content(world: &mut LedgerWorld, passphrase: String, count: usize) {
    let target = world.enc_target_dir.as_ref().unwrap();
    let conn = open_connection_with_passphrase(target.join(DB_FILE_NAME), &passphrase).unwrap();
    assert_eq!(count_transactions(&conn) as usize, count);
    // 与源库快照逐行比对：搬迁前后内容一致。
    let source = open_connection_with_passphrase(db_path(world), &passphrase).unwrap();
    assert_eq!(
        transactions_fingerprint(&conn),
        transactions_fingerprint(&source),
        "搬迁后目标库交易内容应与源库一致"
    );
}

#[then(expr = "源目录的密文库应原样保留")]
fn then_source_preserved(world: &mut LedgerWorld) {
    let path = db_path(world);
    assert!(path.exists(), "搬迁后源库应原样保留（旧位置永不删除）");
    assert_eq!(
        probe_file_kind(&path).unwrap(),
        DbFileKind::Encrypted,
        "源库应保持密文形态"
    );
}

// ---------------------------------------------------------------------------
// 原位重引导计划（issue #644 / ADR-0080）：重启命令消费的引导序列内核
// ---------------------------------------------------------------------------

#[when(expr = "制定重引导计划")]
fn when_plan_reboot(world: &mut LedgerWorld) {
    let default_dir = world.enc_dir.clone().expect("尚未准备加密场景目录");
    let plan = tauri_app_lib::db::boot::plan_boot(&default_dir);
    world.last_boot = Some(plan.boot);
    world.enc_last_plan = Some(plan.disposition.map_err(|e| e.to_string()));
}

#[then(expr = "重引导后生效目录应为目标目录")]
fn then_plan_dir_is_target(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未制定重引导计划");
    let target = world.enc_target_dir.as_ref().expect("尚未配置搬迁目标目录");
    assert_eq!(&boot.db_dir, target, "重引导后生效目录应切换到搬迁目标目录");
}

#[then(expr = "重引导处置应等待解锁")]
fn then_plan_awaits_unlock(world: &mut LedgerWorld) {
    let plan = world.enc_last_plan.as_ref().expect("尚未制定重引导计划");
    assert_eq!(
        plan.as_ref().expect("重引导计划不应失败"),
        &tauri_app_lib::db::boot::BootDisposition::AwaitUnlock,
        "重引导后密文库应推进到等待解锁（与启动同序列）"
    );
}

#[then(expr = "重引导处置应就绪建连")]
fn then_plan_ready(world: &mut LedgerWorld) {
    let plan = world.enc_last_plan.as_ref().expect("尚未制定重引导计划");
    assert_eq!(
        plan.as_ref().expect("重引导计划不应失败"),
        &tauri_app_lib::db::boot::BootDisposition::OpenPlaintext,
        "重引导后明文库应就绪建连（与启动同序列）"
    );
}
