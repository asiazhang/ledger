//! 账户域核心逻辑（issue #91）：CRUD / 幂等创建 / 软删除 / 余额清单。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。
//!
//! 余额调整的交易写入经行为层创建编排入口（issue #310，ADR-0033）：本模块只
//! 持有外层事务壳与领域组装（方向/差额/缺省备注），不直调 Writer 接缝。

use rusqlite::{Connection, OptionalExtension};

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
    TransactionInput,
};
use crate::transaction::amount::TransactionKind;
use crate::transaction::create_transaction_internal;

fn list_accounts_with_visibility(conn: &Connection, include_hidden: bool) -> Result<Vec<Account>> {
    let where_clause = if include_hidden {
        "is_deleted=0"
    } else {
        "is_deleted=0 AND is_hidden=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden \
             FROM accounts WHERE {where_clause} ORDER BY created_at"
        ),
        [],
    )
}

pub fn list_accounts_internal(conn: &Connection) -> Result<Vec<Account>> {
    list_accounts_with_visibility(conn, false)
}

/// AI 侧完整账户列表：不过滤 `is_hidden`，返回含 `is_hidden` 字段的完整列表。
pub fn list_accounts_for_api_internal(conn: &Connection) -> Result<Vec<Account>> {
    list_accounts_with_visibility(conn, true)
}

pub fn create_account_internal(conn: &Connection, input: AccountInput) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

/// 按自然键（name + type + currency_code）幂等创建账户：已存在（未删除）时返回已有 id，
/// 不重复插入、不报错。供 HTTP 导入 API 使用。
pub fn create_account_idempotent_internal(
    conn: &Connection,
    input: AccountInput,
) -> Result<String> {
    if let Some(id) = find_account_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_account_internal(conn, input)
}

fn find_account_by_natural_key(conn: &Connection, input: &AccountInput) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts \
         WHERE name=?1 AND type=?2 AND currency_code=?3 AND is_deleted=0 LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![
        input.name,
        input.kind,
        input.currency_code
    ])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// 软删除账户（`is_deleted=1`）。不校验引用（与 UI 行为一致：删除有交易的账户后
/// 历史交易仍保留）。不存在的 id 返回 `AppError::NotFound`（HTTP 侧映射 404）。
/// IPC 与 HTTP 端点共用本函数。
pub fn delete_account_internal(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "account.not-found",
            format!("账户不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 按 `id` 读取单个未删除账户；不存在或已软删除返回 `AppError::NotFound`。
pub fn get_account_internal(conn: &Connection, id: &str) -> Result<Account> {
    query_all(
        conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden \
         FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::codedp_not_found("account.not-found", format!("账户不存在: {id}"), &[id]))
}

/// 编辑账户（`name` / `currency_code` 可选字段，未传保持原值）。
///
/// 边界（ADR-0026 同期决策）：
/// - `type` 不可改（参与 kind→符号矩阵，改动会重写历史交易的余额归属）；
/// - `currency_code` 仅无交易账户可改（有交易时改币种使历史折算口径错乱）；
/// - `initial_balance_cents` 不在此改，归余额调整（见 ADR-0026）。
pub fn update_account_internal(
    conn: &Connection,
    id: &str,
    input: AccountUpdateInput,
) -> Result<()> {
    let existing = get_account_internal(conn, id)?;
    let name = match input.name {
        Some(ref n) => {
            let trimmed = n.trim();
            if trimmed.is_empty() {
                return Err(AppError::coded("account.name-required", "账户名称不能为空"));
            }
            trimmed.to_string()
        }
        None => existing.name.clone(),
    };
    let currency_code = match input.currency_code {
        Some(ref code) if code != &existing.currency_code => {
            let referenced: bool = conn
                .query_row(
                    "SELECT 1 FROM transactions WHERE (account_id=?1 OR to_account_id=?1) AND is_deleted=0 LIMIT 1",
                    rusqlite::params![id],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            if referenced {
                return Err(AppError::coded(
                    "account.currency-locked",
                    "账户已有交易，不能修改币种（会使历史交易折算口径错乱）",
                ));
            }
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM currencies WHERE code=?1",
                    rusqlite::params![code],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            if !exists {
                return Err(AppError::codedp(
                    "account.currency-unknown",
                    format!("未知币种: {code}"),
                    &[code.as_str()],
                ));
            }
            code.clone()
        }
        _ => existing.currency_code.clone(),
    };
    conn.execute(
        "UPDATE accounts SET name=?2, currency_code=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?1",
        rusqlite::params![id, name, currency_code, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 查找指定币种的黑洞账户（未删除且 `is_hidden=1`，取最早创建的一个）。
fn find_black_hole_account(conn: &Connection, currency_code: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts WHERE is_deleted=0 AND is_hidden=1 AND currency_code=?1 \
         ORDER BY created_at LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![currency_code])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// 确保指定币种的黑洞账户存在；缺失则按种子同形创建（`无(XXX)`、type=`other`、
/// `is_hidden=1`，见 AI 导入域 BlackHoleAccount）。返回 `(id, 是否新建)`。
pub fn ensure_black_hole_account_internal(
    conn: &Connection,
    currency_code: &str,
) -> Result<(String, bool)> {
    if let Some(id) = find_black_hole_account(conn, currency_code)? {
        return Ok((id, false));
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,'other',?3,0,?4,?5,1,?6,0,1)",
        rusqlite::params![
            id,
            format!("无({currency_code})"),
            currency_code,
            now,
            now,
            device_id()
        ],
    )?;
    Ok((id, true))
}

/// 余额调整（ADR-0026）：把账户余额校准到目标值，机制为生成一笔与黑洞账户
/// 之间的转账——目标余额 − 当前实时余额 = Δ，Δ>0 从「无」转入、Δ<0 转出至「无」，
/// 交易币种取目标账户自身币种；删除该转账即撤销调整。
///
/// 返回 `(新交易 id, 是否新建了黑洞账户)`：新建属参考表变更，命令层据此发
/// `ledger:changed` 信号（交易类写入本身不触发，与既有约定一致）。
///
/// 事务自管（BEGIN/COMMIT/ROLLBACK，薄 wrapper 边界，ADR-0032）：置脏不在此处，
/// 由调用方写入口在提交点（闭包 Ok 后 `is_autocommit()` 复核）单点承接。
///
/// 写入路径（issue #310）：交易落库经行为层创建编排入口——本函数持有外层事务
/// （黑洞账户 ensure 与交易写入必须同事务），入口嵌套感知检测到已在事务中则加入、
/// 不自持提交，失败直接返回错误、回滚归本函数的外层事务壳。不直调 Writer 接缝，
/// 「写一笔交易」的事务协议只在行为层三入口（及 ADR-0033 登记的引擎例外）可达。
pub fn adjust_account_balance_internal(
    conn: &Connection,
    id: &str,
    input: &AccountBalanceAdjustInput,
) -> Result<(String, bool)> {
    let account = get_account_internal(conn, id)?;
    if account.is_hidden {
        return Err(AppError::coded(
            "account.black-hole-adjust-unsupported",
            "黑洞账户不支持余额调整",
        ));
    }
    let current = crate::db::balance::compute_balance(conn, id)?;
    let delta = input
        .target_balance_cents
        .checked_sub(current)
        .ok_or_else(|| AppError::coded("account.balance-overflow", "目标余额溢出"))?;
    if delta == 0 {
        return Err(AppError::coded(
            "account.balance-no-change",
            "余额已等于目标值，无需调整",
        ));
    }
    conn.execute("BEGIN", [])?;
    let res = (|| -> Result<(String, bool)> {
        let (black_hole_id, created) =
            ensure_black_hole_account_internal(conn, &account.currency_code)?;
        let (account_id, to_account_id) = if delta > 0 {
            (black_hole_id.clone(), id.to_string())
        } else {
            (id.to_string(), black_hole_id.clone())
        };
        // 方向（delta 正负定转出/转入）、差额绝对值与缺省备注是余额调整自身的
        // 领域知识，在此组装为「半空」`TransactionInput`（与场景无关的可选字段
        // 一律 None，行为层对其有明确的跳过/拒绝语义）；写入协议（校验/归一化/
        // 落库顺序/事务边界）交行为层创建编排入口（issue #310）：外层事务已在，
        // 嵌套感知加入、不自持提交，黑洞账户与交易同事务由本函数事务壳保证。
        let tx_id = create_transaction_internal(
            conn,
            TransactionInput {
                kind: TransactionKind::Transfer,
                amount_cents: delta.abs(),
                currency_code: account.currency_code.clone(),
                account_id,
                to_account_id: Some(to_account_id),
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: Some(input.note.clone().unwrap_or_else(|| "余额调整".to_string())),
                date: input.date.clone(),
                instrument_id: None,
                quantity: None,
                price_cents: None,
                fee_cents: None,
                idempotency_key: None,
            },
        )?
        .id;
        Ok((tx_id, created))
    })();
    match res {
        Ok((tx_id, created)) => {
            conn.execute("COMMIT", [])?;
            Ok((tx_id, created))
        }
        Err(e) => {
            conn.execute("ROLLBACK", [])?;
            Err(e)
        }
    }
}

/// 账户余额清单（conn 级）：`include_hidden` 为 true 时含黑洞账户。
/// `pub(crate)` 供 dashboard 净资产聚合复用同一口径（issue #142）。
pub(crate) fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    let accounts = list_accounts_with_visibility(conn, include_hidden)?;
    let balances = crate::db::balance::compute_all_balances_with_visibility(conn, include_hidden)?;
    Ok(accounts
        .into_iter()
        .map(|a| {
            let balance_cents = balances.get(&a.id).copied().unwrap_or(0);
            AccountBalance {
                balance_cents,
                account: a,
            }
        })
        .collect())
}

/// AI 侧余额清单：含黑洞账户。
pub fn list_account_balances_for_api_internal(conn: &Connection) -> Result<Vec<AccountBalance>> {
    list_account_balances_with_visibility(conn, true)
}
