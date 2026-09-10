use std::path::Path;

use rusqlite::Connection;
use rusqlite::params;

use tauri_app_lib::db::{open_connection, open_connection_with_passphrase};
use tauri_app_lib::error::AppError;
use tauri_app_lib::transaction::{Transaction, TransactionInput};

use crate::step_inputs::expense_input;
use crate::world::LedgerWorld;

/// 断言最近一次操作记录的错误信息包含指定片段（多个 `*_steps` 模块共用的 seam 断言）。
pub fn assert_last_error_contains(world: &LedgerWorld, needle: &str) {
    match &world.last_error {
        Some(msg) => assert!(
            msg.contains(needle),
            "错误消息不匹配: 期望包含 '{needle}', 实际 '{msg}'"
        ),
        None => panic!("预期错误但未发生"),
    }
}

/// 错误断言路径的 When 侧捕获（与 [`assert_last_error_contains`] 成对）：行为层
/// 写入结果记入 `world.last_error`。臂形固定为两臂——码化错误记 message，其余
/// （意外成功、非码化错误）一律记「预期失败但成功了」，由后续「应返回错误」
/// 断言变红；与 write/edit 步骤既有逐处内联 match 同形，断言语义不变。
pub fn capture_expected_error<T>(world: &mut LedgerWorld, result: Result<T, AppError>) {
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 统计未删除交易数（backup/encryption/data_location 文件级步骤共用的内联
/// COUNT 收编，#763）：连接形态，断言侧直接消费。
pub fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// 打开库文件（可选主口令）并统计未删除交易数（[#count_transactions] 的文件
/// 形态）：备份恢复产物与加密转换副本的「应包含 N 条交易」断言共用。
pub fn count_transactions_in_file(db: &Path, passphrase: Option<&str>) -> i64 {
    let conn = match passphrase {
        Some(p) => open_connection_with_passphrase(db, p).unwrap(),
        None => open_connection(db).unwrap(),
    };
    count_transactions(&conn)
}

/// 在文件库中经账户域公开创建入口建现金账户（余额缓存行由产品代码保证，
/// #763 旁路归零）并落 count 条带备注支出（L1 工厂 + 行为层接缝），返回账户
/// id 供调用方注册。引导组四文件（backup/encryption/data_location/
/// startup_failure）共用的种子形状：数量与日期各文件自选，备注按序号编码。
pub fn seed_account_with_expenses(
    conn: &Connection,
    account: &str,
    note_prefix: &str,
    count: usize,
    amount_base: i64,
    date: &str,
) -> String {
    let id = tauri_app_lib::accounts::create_account(
        conn,
        tauri_app_lib::accounts::AccountInput {
            name: account.into(),
            kind: tauri_app_lib::accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
        },
    )
    .unwrap();
    for i in 0..count {
        let input = TransactionInput {
            note: Some(format!("{note_prefix} {i}")),
            ..expense_input(amount_base + i as i64, &id, date)
        };
        tauri_app_lib::transaction::create_transaction_internal(conn, input).unwrap();
    }
    id
}

/// 按标的代码取 id（场景内代码唯一；不存在即 panic——场景文本错误）。买入/卖出/
/// 明细断言步骤共用（#761 收编：原 write/edit/policy 三文件内联同款 SELECT）。
pub fn instrument_id_by_symbol(conn: &Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .expect("标的不存在，先铺垫 Given 存在标的")
}

/// 查询全部未删除交易，按日期倒序（与 `list_transactions_internal` 的确定性排序一致，id 为 tiebreaker）。
pub fn query_all_transactions(conn: &Connection) -> Vec<Transaction> {
    let mut stmt = conn
        .prepare(
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
             to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
             version,device_id,is_deleted,merchant_id,policy_id \
             FROM transactions WHERE is_deleted=0 ORDER BY date DESC, created_at DESC, id DESC",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok(Transaction {
            id: r.get(0)?,
            kind: r.get(1)?,
            amount_cents: r.get(2)?,
            currency_code: r.get(3)?,
            amount_native_cents: r.get(4)?,
            account_id: r.get(5)?,
            to_account_id: r.get(6)?,
            funding_account_id: r.get(7)?,
            category_id: r.get(8)?,
            refund_of_transaction_id: r.get(9)?,
            note: r.get(10)?,
            date: r.get(11)?,
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
            version: r.get(14)?,
            device_id: r.get(15)?,
            is_deleted: r.get::<_, i64>(16)? != 0,
            merchant_id: r.get(17)?,
            policy_id: r.get(18)?,
            // 步骤侧直读快照不做来源反查：来源契约断言一律走列表命令（transactions_source_steps）。
            source: None,
            // 转换扩展同规：直读快照不反查，展示口径断言走列表命令。
            convert: None,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// 查询所有未删除、未隐藏账户名称（用户侧视角）。
pub fn query_accounts_by_name(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM accounts WHERE is_deleted=0 AND is_hidden=0 ORDER BY created_at")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}
