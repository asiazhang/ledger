use std::path::PathBuf;

use cucumber::{then, when};

use tauri_app_lib::accounts::create_account;
use tauri_app_lib::backup::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, BackupKind, SkipReason, backup_db_to,
    expected_schema_version, get_state, read_backup_kind, restore_db_from, set_state,
};
use tauri_app_lib::categories::{
    CategoryInput, create_category, delete_category as delete_category_domain,
};
use tauri_app_lib::currencies::ExchangeRateInput;
use tauri_app_lib::db::{new_uuid, now_iso, open_connection};
use tauri_app_lib::investment::{create_exchange_rate, create_instrument, create_market_price};
use tauri_app_lib::item::cost;
use tauri_app_lib::item::domain::{create_item, delete_item, dispose_item, update_item};
use tauri_app_lib::models::{
    AccountInput, AccountType, InstrumentInput, InstrumentType, ItemDisposeInput, ItemInput,
    MarketPriceInput, TransactionInput,
};
use tauri_app_lib::settings::{self, SettingKey};
use tauri_app_lib::transaction::TransactionBatch;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{delete_transaction_internal, update_transaction_internal};

use crate::world::LedgerWorld;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ledger-e2e-backup-{name}-{}.db", new_uuid()))
}

fn temp_safety_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-safety-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "备份数据库到临时文件")]
fn backup_to_temp(world: &mut LedgerWorld) {
    let target = temp_path("backup.zip");
    let result = backup_db_to(&world_conn!(world), &target, "0.2.0", BackupKind::Manual);
    assert!(result.is_ok(), "备份失败: {:?}", result.err());
    world.last_backup_path = Some(target);
}

/// 真实走自动备份触发入口（前置业务写已置脏、开关默认开启），产物落到本场景
/// 独立临时目录并复用（日界门场景据同目录产物计数区分「跳过/新增」）。
#[when(expr = "自动备份数据库到临时目录")]
fn auto_backup_to_temp(world: &mut LedgerWorld) {
    let dir = world.auto_backup_dir.clone().unwrap_or_else(|| {
        let dir = std::env::temp_dir().join(format!("ledger-e2e-auto-backup-{}", new_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    });
    world.auto_backup_dir = Some(dir.clone());
    let outcome = tauri_app_lib::backup::run_due_backup(
        &world_conn!(world),
        Some(dir.to_str().unwrap()),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert!(
        matches!(outcome, AttemptOutcome::Performed { .. }),
        "自动备份应执行，实际 {outcome:?}"
    );
    if let AttemptOutcome::Performed { path } = outcome {
        world.last_auto_backup_path = Some(PathBuf::from(path));
    }
}

#[when(expr = "删除全部交易")]
fn delete_all_txns(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute_batch("UPDATE transactions SET is_deleted=1")
        .unwrap();
}

#[when(expr = "从备份恢复到临时数据库")]
fn restore_to_temp(world: &mut LedgerWorld) {
    let backup = world.last_backup_path.clone().expect("尚未备份");
    let db_path = temp_path("restored.db");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    world.restored_db_path = Some(db_path);
    std::fs::remove_dir_all(&safety_dir).ok();
}

#[when(expr = "尝试从更高 schema 版本恢复")]
fn try_newer_restore(world: &mut LedgerWorld) {
    // 构造一个 schema 版本更高的库文件作为"备份"。
    let newer = temp_path("newer.db");
    {
        let conn = open_connection(&newer).unwrap();
        conn.execute_batch("PRAGMA user_version = 999").unwrap();
    }
    let db_path = temp_path("target.db");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    world.last_error = match restore_db_from(&newer, &db_path, &safety_dir, expected) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
    std::fs::remove_dir_all(&safety_dir).ok();
    std::fs::remove_file(&newer).ok();
    std::fs::remove_file(&db_path).ok();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "备份文件应存在")]
fn backup_exists(world: &mut LedgerWorld) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    assert!(p.exists(), "备份文件不存在: {}", p.display());
}

#[then(expr = "备份包应包含 {string} 与 {string}")]
fn backup_contains(world: &mut LedgerWorld, a: String, b: String) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    let file = std::fs::File::open(p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&a), "缺少条目 {a}: {names:?}");
    assert!(names.contains(&b), "缺少条目 {b}: {names:?}");
}

#[then(expr = "备份包内的数据库应包含 {int} 条交易")]
fn backup_db_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    let file = std::fs::File::open(p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut db_entry = archive.by_name("ledger.db").unwrap();
    let out = temp_path("extract.db");
    let mut out_f = std::fs::File::create(&out).unwrap();
    std::io::copy(&mut db_entry, &mut out_f).unwrap();
    drop(out_f);
    let conn = open_connection(&out).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "备份包内交易数量不匹配");
    std::fs::remove_file(&out).ok();
}

#[then(expr = "恢复的数据库应包含 {int} 条交易")]
fn restored_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.restored_db_path.as_ref().expect("尚未恢复");
    let conn = open_connection(p).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "恢复出的交易数量不匹配");
}

// ---------------------------------------------------------------------------
// 备份产物来源标记（issue #127）
// ---------------------------------------------------------------------------

#[then(expr = "备份元数据来源应为 {string}")]
fn backup_meta_kind_manual(world: &mut LedgerWorld, expected: String) {
    let p = world.last_backup_path.as_ref().expect("尚未手动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "手动备份元数据来源不匹配"
    );
}

#[then(expr = "自动备份元数据来源应为 {string}")]
fn backup_meta_kind_auto(world: &mut LedgerWorld, expected: String) {
    let p = world.last_auto_backup_path.as_ref().expect("尚未自动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "自动备份元数据来源不匹配"
    );
}

// ---------------------------------------------------------------------------
// 脏标记挂钩（issue #126）
// ---------------------------------------------------------------------------

#[then(expr = "自动备份脏标记应为真")]
fn auto_backup_dirty(world: &mut LedgerWorld) {
    let state = get_state(&world_conn!(world)).unwrap();
    assert!(state.dirty, "业务写库成功后脏标记应为真");
}

#[then(expr = "自动备份脏标记应为假")]
fn auto_backup_clean(world: &mut LedgerWorld) {
    let state = get_state(&world_conn!(world)).unwrap();
    assert!(!state.dirty, "未发生业务写库时脏标记应为默认假");
}

#[when(expr = "删除最近创建的交易")]
fn delete_last_transaction(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可删除的交易");
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）删除，成功即置脏。
    world
        .db
        .write(|conn| delete_transaction_internal(conn, &id))
        .unwrap();
}

/// 快进跨过本地日界（issue #386）：把上次备份锚点拨到昨天（本地日期，取本地昨天
/// 正午换算 UTC 以避开时制切换窗口），使下一次「自动备份数据库到临时目录」必到期
/// （既有链式场景逐段验证多次写入的置脏）。经模块 `set_state` 写回，锚点格式由模块保证。
#[when(expr = "距离上次自动备份已过一天")]
fn fast_forward_backup_due(world: &mut LedgerWorld) {
    use chrono::TimeZone;
    let conn = world_conn!(world);
    let mut state = get_state(&conn).unwrap();
    if state.last_backup_at.is_some() {
        let yesterday_noon = chrono::Local
            .from_local_datetime(
                &(chrono::Local::now().date_naive() - chrono::Days::new(1))
                    .and_hms_opt(12, 0, 0)
                    .expect("合法时刻"),
            )
            .earliest()
            .expect("本地昨天正午应可解析");
        state.last_backup_at = Some(tauri_app_lib::db::iso_at(
            yesterday_noon.with_timezone(&chrono::Utc),
        ));
        set_state(&conn, &state).expect("回拨备份锚点");
    }
}

// ---------------------------------------------------------------------------
// 自动备份日界门：本地自然日每天最多一次，三入口统一（issue #386）
// ---------------------------------------------------------------------------

/// 同日已自动备份后再次到期触发：断言静默跳过（原因可辨）且锚点不前移。
#[when(expr = "再次到期触发自动备份因日界门静默跳过")]
fn due_trigger_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = tauri_app_lib::backup::run_due_backup(
        &world_conn!(world),
        Some(
            world
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日二次到期触发应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

/// 同日已自动备份后退出兜底：断言静默跳过且锚点不前移（原「不受每日约束」豁免取消）。
#[when(expr = "退出兜底因日界门静默跳过")]
fn exit_fallback_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = tauri_app_lib::backup::run_exit_backup(
        &world_conn!(world),
        Some(
            world
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日退出兜底应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

/// 模拟跨本地日后的到期触发（时间注入，BackupTrigger 接口面，与调度线程同构）：
/// 注入时刻取「现在 + 1 天」——锚点仍是今天，判定为跨日恢复备份；产物文件名
/// 时间戳随注入时刻自然不同，同目录产物计数可区分两次备份（避开秒级同妙覆盖）。
#[when(expr = "跨日后触发自动备份数据库到临时目录")]
fn auto_backup_next_day_to_temp(world: &mut LedgerWorld) {
    let dir = world.auto_backup_dir.clone().expect("尚未自动备份");
    let outcome = tauri_app_lib::backup::run_due_backup(
        &world_conn!(world),
        Some(dir.to_str().unwrap()),
        "0.2.0",
        chrono::Utc::now() + chrono::Duration::days(1),
    );
    assert!(
        matches!(outcome, AttemptOutcome::Performed { .. }),
        "跨本地日后有变动应恢复备份，实际 {outcome:?}"
    );
    if let AttemptOutcome::Performed { path } = outcome {
        world.last_auto_backup_path = Some(PathBuf::from(path));
    }
}

/// 同日已自动备份且列表为空（目录被清空）后首次兜底：断言静默跳过且锚点不前移。
#[when(expr = "首次兜底因日界门静默跳过")]
fn first_fallback_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = tauri_app_lib::backup::run_first_backup(
        &world_conn!(world),
        Some(
            world
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日首次兜底应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

/// 清空自动备份目录内的产物（模拟用户删光备份），目录本身保留。
#[when(expr = "清空自动备份目录")]
fn clear_auto_backup_dir(world: &mut LedgerWorld) {
    let dir = world.auto_backup_dir.as_ref().expect("尚未自动备份");
    for entry in std::fs::read_dir(dir).expect("列目录") {
        std::fs::remove_file(entry.expect("读目录项").path()).expect("删除产物");
    }
}

/// 统计本场景自动备份目录内的自动产物数量（受管前缀识别，混入手动文件不计数）。
#[then(expr = "备份目录内自动备份产物数量应为 {int}")]
fn auto_backup_product_count(world: &mut LedgerWorld, expected: i64) {
    let dir = world.auto_backup_dir.as_ref().expect("尚未自动备份");
    let count = std::fs::read_dir(dir)
        .expect("列目录")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(AUTO_BACKUP_PREFIX)
        })
        .count() as i64;
    assert_eq!(count, expected, "自动备份产物数量不匹配");
}

/// 设置写入（`app_settings`，经 settings 模块单点收口，普通锁不走出入口）——
/// ADR-0032 的豁免路径：不置脏。
#[when(expr = "写入一项设置")]
fn write_a_setting(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    settings::set(&conn, SettingKey::AutoBackupEnabled, &false).expect("写入设置");
}

// ---------------------------------------------------------------------------
// 参考数据写路径置脏（issue #243，ADR-0032 写入口接管）
// ---------------------------------------------------------------------------

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建账户，成功即置脏。
#[when(expr = "创建账户 {string} 类型 {string} 币种 {string}")]
fn create_account_via_entry(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    let input = AccountInput {
        name,
        kind: kind.parse::<AccountType>().expect("非法账户类型"),
        currency_code: currency,
        initial_balance_cents: None,
    };
    world
        .db
        .write(|conn| create_account(conn, input))
        .expect("创建账户失败");
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建分类，成功即置脏。
#[when(expr = "创建分类 {string} 类型 {string}")]
fn create_category_via_entry(world: &mut LedgerWorld, name: String, kind: String) {
    let input = CategoryInput {
        name,
        kind,
        parent_id: None,
        icon: None,
    };
    world
        .db
        .write(|conn| create_category(conn, input))
        .expect("创建分类失败");
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）软删分类，成功即置脏。
#[when(expr = "删除分类 {string}")]
fn delete_category_via_entry(world: &mut LedgerWorld, name: String) {
    let id: String = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT id FROM categories WHERE name=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .expect("分类不存在")
    };
    world
        .db
        .write(|conn| delete_category_domain(conn, &id))
        .expect("删除分类失败");
}

/// 尝试把最近创建的交易改为非法金额（金额必须大于 0）：修改事务内失败回滚，
/// 写入口闭包失败不置脏（ADR-0032）。错误记入 last_error 供「应返回错误」断言。
#[when(expr = "尝试把最近创建的交易修改为非法金额")]
fn update_last_transaction_invalid_amount(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let input = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: "acc-any".into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    world.last_error = match world
        .db
        .write(|conn| update_transaction_internal(conn, &id, input))
    {
        Ok(_) => Some(String::from("预期失败但成功了")),
        Err(e) => Some(e.to_string()),
    };
}

// ---------------------------------------------------------------------------
// 批量导入写路径置脏（issue #245，ADR-0032 写入口接管）
// ---------------------------------------------------------------------------

/// 与 HTTP 批量导入端点同形态：经连接层统一写入口（ADR-0032，issue #245）跑一次
/// 批量导入（dedup=true，各行日期互异不撞去重身份），提交点置脏；步骤内断言整批
/// 成功（逐条结果均 success）。
#[when(expr = "批量导入 {int} 笔支出各 {int} 分到账户 {string}")]
fn batch_import_expenses_via_entry(
    world: &mut LedgerWorld,
    count: usize,
    cents: i64,
    account: String,
) {
    let account_id = world.account_id(&account);
    let inputs: Vec<TransactionInput> = (0..count)
        .map(|i| TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: cents,
            currency_code: "CNY".into(),
            account_id: account_id.clone(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            // 日期互异：dedup=true 时同内容行会被判重复，这里保证每行身份唯一。
            date: format!("2026-02-{:02}", i + 1),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        })
        .collect();
    let results = world
        .db
        .write(|conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入失败")
        .results;
    assert!(
        results.iter().all(|r| r.success),
        "批量导入应整批成功: {results:?}"
    );
}

/// 与 HTTP 批量导入端点同形态：批次含一行退款引用不存在的原支出交易——这是
/// 非单行 `Invalid` 的硬错误（`AppError::NotFound`），整批回滚；写入口闭包失败
/// 不置脏（ADR-0032）。错误记入 last_error 供「应返回错误」断言。
#[when(expr = "批量导入两笔交易但退款行引用不存在的原支出交易")]
fn batch_import_rollback_via_entry(world: &mut LedgerWorld) {
    let account_id = world.account_id("现金");
    let expense = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 1500,
        currency_code: "CNY".into(),
        account_id: account_id.clone(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let refund = TransactionInput {
        policy_id: None,
        kind: TransactionKind::Refund,
        amount_cents: 500,
        // 占位值即可：退款归一化以原支出覆盖账户/币种，但原支出不存在先行报错。
        currency_code: "CNY".into(),
        account_id,
        refund_of_transaction_id: Some(String::from("tx-no-such")),
        ..expense.clone()
    };
    world.last_error = match world
        .db
        .write(|conn| TransactionBatch::run(conn, vec![expense, refund], true))
    {
        Ok(_) => Some(String::from("预期失败但成功了")),
        Err(e) => Some(e.to_string()),
    };
}

// ---------------------------------------------------------------------------
// 物品写路径置脏（issue #244，ADR-0032 写入口接管）
// ---------------------------------------------------------------------------

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建物品并关联最近创建的
/// 购买交易（满足溯源守卫，后端以交易值带出日期/成本/币种，入参占位值被覆盖），
/// 成功即置脏。
#[when(expr = "创建物品 {string} 关联最近创建的购买交易")]
fn create_item_via_entry(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .last_transaction_id
        .clone()
        .expect("没有可关联的购买交易");
    let input = ItemInput {
        name,
        purchase_date: "1970-01-01".into(), // 占位：后端以交易值覆盖带出
        total_cost_cents: 1,
        currency_code: "CNY".into(),
        note: None,
        purchase_transaction_id: Some(tx_id),
    };
    let id = world
        .db
        .write(|conn| create_item(conn, input, &mut || {}))
        .expect("创建物品失败");
    world.last_item_id = Some(id);
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）修改最近创建的物品的备注
/// （其余字段读现值保持不变，溯源保持；空字符串规为清除），成功即置脏。
#[when(expr = "修改最近创建的物品备注为 {string}")]
fn update_last_item_note_via_entry(world: &mut LedgerWorld, note: String) {
    let id = world.last_item_id.clone().expect("没有已创建的物品");
    // 读现值构造入参：本步骤只改备注，其余字段原样保留（不与创建场景数据耦合）。
    let (name, purchase_date, total_cost_cents, currency_code): (String, String, i64, String) = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT name, purchase_date, total_cost_cents, currency_code FROM items \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("读物品现值失败")
    };
    let input = ItemInput {
        name,
        purchase_date,
        total_cost_cents,
        currency_code,
        note: if note.is_empty() { None } else { Some(note) },
        purchase_transaction_id: None, // None = 维持既有溯源
    };
    world
        .db
        .write(|conn| update_item(conn, &id, input, &mut || {}))
        .expect("修改物品失败");
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）以今天为处置日处置最近
/// 创建的物品（不填残值），成功即置脏。
#[when(expr = "今天处置最近创建的物品")]
fn dispose_last_item_today_via_entry(world: &mut LedgerWorld) {
    let id = world.last_item_id.clone().expect("没有已创建的物品");
    let input = ItemDisposeInput {
        disposal_date: cost::today().format("%Y-%m-%d").to_string(),
        residual_value_cents: None,
    };
    world
        .db
        .write(|conn| dispose_item(conn, &id, input, &mut || {}))
        .expect("处置物品失败");
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）软删除最近创建的物品，
/// 成功即置脏。
#[when(expr = "软删除最近创建的物品")]
fn delete_last_item_via_entry(world: &mut LedgerWorld) {
    let id = world.last_item_id.clone().expect("没有已创建的物品");
    world
        .db
        .write(|conn| delete_item(conn, &id, &mut || {}))
        .expect("软删除物品失败");
}

// ---------------------------------------------------------------------------
// 市场数据写路径置脏（issue #244，ADR-0032 写入口接管）
// ---------------------------------------------------------------------------

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）写入一条汇率，成功即置脏。
#[when(expr = "写入汇率 {string} 兑 {string} 为 {float}")]
fn write_exchange_rate_via_entry(world: &mut LedgerWorld, base: String, quote: String, rate: f64) {
    let input = ExchangeRateInput {
        base_code: base,
        quote_code: quote,
        rate,
        priced_at: now_iso(),
        source: None,
    };
    world
        .db
        .write(|conn| create_exchange_rate(conn, input))
        .expect("写入汇率失败");
}

/// 新建标的的共用实现（新增性写入与同名信息更新两条步骤同款）。
fn create_instrument_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    let input = InstrumentInput {
        symbol,
        kind: InstrumentType::Stock,
        name: Some(name),
        currency_code: currency,
        market: None,
    };
    world
        .db
        .write(|conn| create_instrument(conn, input))
        .expect("新建标的失败");
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）新建标的，成功即置脏。
#[when(expr = "新建标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    create_instrument_entry(world, symbol, name, currency);
}

/// 同名标的再建（名称有变 → 走 create_instrument 的信息更新分支），成功即置脏：
/// 钉住「标的信息更新也算市场数据写入」。
#[when(expr = "再次新建标的 {string} 名称 {string} 币种 {string}")]
fn recreate_instrument_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    create_instrument_entry(world, symbol, name, currency);
}

/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）写入一条标的现价，成功即置脏。
#[when(expr = "写入标的 {string} 现价 {int} 币种 {string}")]
fn write_market_price_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    price: i64,
    currency: String,
) {
    let instrument_id: String = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT id FROM instruments WHERE symbol=?1 LIMIT 1",
            rusqlite::params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在")
    };
    let input = MarketPriceInput {
        instrument_id,
        price_cents: price,
        currency_code: currency,
        priced_at: now_iso(),
        source: None,
    };
    world
        .db
        .write(|conn| create_market_price(conn, input))
        .expect("写入行情失败");
}

#[then(expr = "恢复的数据库自动备份状态应为「未脏且已重新计时」")]
fn restored_auto_backup_state_reset(world: &mut LedgerWorld) {
    let p = world.restored_db_path.as_ref().expect("尚未恢复");
    let conn = open_connection(p).unwrap();
    let state = get_state(&conn).unwrap();
    assert!(!state.dirty, "恢复后脏标记应被重置为假");
    assert!(
        state.last_backup_at.is_some(),
        "恢复后上次备份锚点应重新计时"
    );
}
