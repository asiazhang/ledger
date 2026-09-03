use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::accounts::{
    AccountBalanceAdjustInput, AccountUpdateInput, adjust_account_balance,
    delete_account as delete_account_domain, update_account,
};
use tauri_app_lib::db::{balance::compute_balance, device_id, new_uuid, now_iso};
use tauri_app_lib::transaction::delete_transaction_internal;

use crate::common::query_accounts_by_name;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 共享辅助（步骤函数体共用的落库/查询形状）
// ---------------------------------------------------------------------------

/// 直插一个账户行并注册名称→id 映射（「创建账户…初始余额」与「存在账户…初始余额」
/// 两个步骤共用同一落库形状）。
fn insert_account_and_register(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    let id = new_uuid();
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
            params![id, name, kind, currency, initial_balance, now, now, 1, device_id()],
        )
        .unwrap();
    world.account_name_to_id.insert(name, id);
}

/// 未删除的隐藏账户（黑洞账户）名单（存在/不残留两个对称断言共用同一查询）。
fn hidden_account_names(world: &LedgerWorld) -> Vec<String> {
    let conn = world_conn!(world);
    let mut stmt = conn
        .prepare("SELECT name FROM accounts WHERE is_deleted=0 AND is_hidden=1")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// When：编辑账户 / 余额调整（ADR-0026 黑洞转账）
// ---------------------------------------------------------------------------

/// 带初始余额的账户前置（余额调整场景用）：仅为「创建账户…初始余额」的 Given 语义别名
/// （cucumber 不跨 given/when 匹配，缺失本步骤时场景整体被静默跳过）。
#[given(expr = "存在账户 {string} 类型 {string} 币种 {string} 初始余额 {int}")]
fn create_account_with_initial_balance(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    insert_account_and_register(world, name, kind, currency, initial_balance);
}

/// 缺失币种的黑洞账户场景用：补一条 1:1 汇率（MVP 多币种汇率 1:1，本位币折算所需）。
#[given(expr = "存在汇率 {string} 兑本位币 {float}")]
fn ensure_exchange_rate(world: &mut LedgerWorld, code: String, rate: f64) {
    world_conn!(world)
        .execute(
            "INSERT INTO exchange_rates (id, base_code, quote_code, rate, priced_at, source, updated_at, version, device_id) \
             VALUES (?1, ?2, 'CNY', ?3, '2026-01-01T00:00:00Z', 'manual', ?4, 1, ?5)",
            params![new_uuid(), code, rate, now_iso(), device_id()],
        )
        .unwrap();
}

#[when(expr = "修改账户 {string} 名称为 {string}")]
fn rename_account(world: &mut LedgerWorld, name: String, new_name: String) {
    let id = world.account_id(&name);
    world.last_error = update_account(
        &world_conn!(world),
        &id,
        AccountUpdateInput {
            name: Some(new_name),
            currency_code: None,
        },
    )
    .err()
    .map(|e| e.to_string());
}

/// 币种修改失败场景用：错误记入 `world.last_error`（应返回错误步骤断言）。
#[when(expr = "尝试修改账户 {string} 币种为 {string}")]
fn try_change_currency(world: &mut LedgerWorld, name: String, currency: String) {
    let id = world.account_id(&name);
    world.last_error = update_account(
        &world_conn!(world),
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: Some(currency),
        },
    )
    .err()
    .map(|e| e.to_string());
}

#[when(expr = "调整账户 {string} 余额至 {int} 日期 {string}")]
fn adjust_balance(world: &mut LedgerWorld, name: String, target: i64, date: String) {
    let id = world.account_id(&name);
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032），提交点置脏/回滚不置脏。
    match world.db.write(|conn| {
        adjust_account_balance(
            conn,
            &id,
            &AccountBalanceAdjustInput {
                target_balance_cents: target,
                date,
                note: None,
            },
        )
    }) {
        Ok((tx_id, _)) => {
            world.last_transaction_id = Some(tx_id);
            world.last_error = None;
        }
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

/// 调整产生的转账就是普通 transfer：删除即撤销调整（ADR-0026 可逆性）。
#[when(expr = "删除上一笔交易")]
fn delete_last_transaction(world: &mut LedgerWorld) {
    let tx_id = world
        .last_transaction_id
        .clone()
        .expect("场景中应先产生一笔交易");
    delete_transaction_internal(&world_conn!(world), &tx_id).expect("删除交易失败");
}

/// 注入「余额调整中途失败」：调整的交易写入（行为层创建入口 → Writer 落库）
/// 被 BEFORE INSERT 触发器 RAISE(ABORT) 挡下，而黑洞账户 ensure 在其之前已插入——
/// 纯测试侧注入（spec #169 / #310 定案同款），检验外层事务壳持有回滚、
/// 同事务即建的黑洞账户不残留（issue #310）。
#[when(expr = "注入余额调整交易写入失败触发器")]
fn inject_adjust_tx_failure_trigger(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute(
            "CREATE TRIGGER block_adjust_tx BEFORE INSERT ON transactions \
             BEGIN SELECT RAISE(ABORT, '测试注入：余额调整写入失败'); END",
            [],
        )
        .expect("注入触发器失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "账户列表应包含黑洞账户 {string}")]
fn check_black_hole_exists(world: &mut LedgerWorld, name: String) {
    let names = hidden_account_names(world);
    assert!(
        names.contains(&name),
        "应包含黑洞账户 '{}'，实际 {:?}",
        name,
        names
    );
}

/// 黑洞账户不残留断言（issue #310 回滚场景用）：按币种指名的黑洞账户不应存在——
/// 调整失败整体回滚后，同事务即建的黑洞账户不得残留（种子预置的 CNY/HKD 不受影响）。
#[then(expr = "账户列表不应包含黑洞账户 {string}")]
fn check_black_hole_absent(world: &mut LedgerWorld, name: String) {
    let names = hidden_account_names(world);
    assert!(
        !names.contains(&name),
        "不应存在黑洞账户 '{}'，实际 {:?}",
        name,
        names
    );
}

/// 直接落库一笔分红交易（余额口径测试用 fixture）。
///
/// 分红/拆股属投资层路径，writer::normalize 显式拒绝（issue #72 计划），
/// 行为层创建路径不开放；余额口径只关心该行存在，故直接 INSERT。
#[when(expr = "直接写入分红交易 金额 {int} 到账户 {string} 日期 {string}")]
fn insert_dividend_row(world: &mut LedgerWorld, amount: i64, account_name: String, date: String) {
    let id = new_uuid();
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,\
             created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'dividend',?2,'CNY',?2,?3,?4,?5,?5,1,?6,0)",
            params![
                id,
                amount,
                world.account_id(&account_name),
                date,
                now,
                device_id()
            ],
        )
        .unwrap();
}

#[when(expr = "创建账户 {string} 类型 {string} 币种 {string} 初始余额 {int}")]
fn create_account(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    insert_account_and_register(world, name, kind, currency, initial_balance);
}

#[when(expr = "删除账户 {string}")]
fn delete_account(world: &mut LedgerWorld, name: String) {
    let id = world.account_id(&name);
    // 软删除走真实领域路径（IPC/HTTP 共用 accounts::delete_account，含存在性守卫）。
    delete_account_domain(&world_conn!(world), &id).expect("删除账户失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "账户列表应包含 {int} 条记录")]
fn check_account_count(world: &mut LedgerWorld, expected: i64) {
    let count: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_deleted=0 AND is_hidden=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "账户数量不匹配");
}

#[then(expr = "{string} 账户余额应为 {int}")]
fn check_balance(world: &mut LedgerWorld, name: String, expected: i64) {
    let id = world.account_id(&name);
    let balance = compute_balance(&world_conn!(world), &id).unwrap();
    assert_eq!(balance, expected, "账户 '{}' 余额不匹配", name);
}

#[then(expr = "账户列表应包含 {string}")]
fn check_account_exists(world: &mut LedgerWorld, name: String) {
    let accounts = query_accounts_by_name(&world_conn!(world));
    assert!(
        accounts.contains(&name),
        "账户列表应包含 '{}'，但实际为 {:?}",
        name,
        accounts
    );
}

#[then(expr = "账户列表不应包含 {string}")]
fn check_account_not_exists(world: &mut LedgerWorld, name: String) {
    let accounts = query_accounts_by_name(&world_conn!(world));
    assert!(
        !accounts.contains(&name),
        "账户列表不应包含 '{}'，但实际为 {:?}",
        name,
        accounts
    );
}
