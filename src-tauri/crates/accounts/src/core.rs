//! 账户域核心逻辑（issue #91 域内收口，#404 自命令壳层迁入）：CRUD / 幂等创建 /
//! 软删除 / 余额清单 / 黑洞账户与余额调整编排。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。
//!
//! 余额调整的交易写入经行为层创建编排入口（issue #310，ADR-0033）：本模块只
//! 持有外层事务壳与领域组装（方向/差额/缺省备注），不直调 Writer 接缝。

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::{ensure_transaction, hold_transaction};
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, OptionalExtension};

use super::command::{AccountCommand, AccountCommandRow, record_local};
use super::model::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountType,
    AccountUpdateInput, BalanceCacheAudit, BalanceCacheDrift,
};
use crate::balance::refresh_account_balances;

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>> {
    crate::balance::list_accounts_with_visibility(conn, false)
}

// ---------------------------------------------------------------------------
// 交易×账户接缝实现（spec #1086 / issue #1092）：出资账户视图
// ---------------------------------------------------------------------------

/// 出资准入的现金类账户类型闭集（ADR-0096 决策 6）：cash / bank / credit /
/// ewallet / other。投资账户与 receivable / debt 排除——非自有现金与再挂一层
/// 的出资无记账语义。类型词汇的单一来源是本域 [`AccountType`]，闭集映射随
/// #1092 接缝反转住本域实现侧（原 `transaction::funding` 的 FUNDING_ALLOWED_TYPES
/// 迁入，准入判定本身仍归核心交易域）。
const FUNDING_CASH_LIKE_TYPES: [AccountType; 5] = [
    AccountType::Cash,
    AccountType::Bank,
    AccountType::Credit,
    AccountType::Ewallet,
    AccountType::Other,
];

/// 出资账户视图实现（核心交易域 `transaction::funding` 注册点，issue #1092）：
/// 读在用账户的类型/币种并投影为准入判读所需的最小视图；不存在或已软删除返回
/// `None`（缺行的码化 NotFound 归交易域准入语义，与商户/保单同款）。
fn funding_account_view(
    conn: &Connection,
    id: &str,
) -> Result<Option<ledger_transaction::funding::FundingAccountView>> {
    let (account_type, currency_code): (AccountType, String) = match conn.query_row(
        "SELECT type, currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(ledger_transaction::funding::FundingAccountView {
        class: if FUNDING_CASH_LIKE_TYPES.contains(&account_type) {
            ledger_transaction::funding::FundingAccountClass::CashLike
        } else {
            ledger_transaction::funding::FundingAccountClass::Ineligible
        },
        type_display: account_type.to_string(),
        currency_code,
    }))
}

/// 注册出资账户视图实现（幂等：进程级一次，重复注册保留首次）。调用点在壳层
/// 启动接线与测试建库单点，与生产同形；业务代码不直接调用。
pub fn install_funding_account_hook() {
    ledger_transaction::funding::register_funding_account_lookup_hook(funding_account_view);
}

/// AI 侧完整账户列表：不过滤 `is_hidden`，返回含 `is_hidden` 字段的完整列表。
pub fn list_accounts_for_api(conn: &Connection) -> Result<Vec<Account>> {
    crate::balance::list_accounts_with_visibility(conn, true)
}

pub fn create_account(conn: &Connection, input: AccountInput) -> Result<String> {
    ensure_transaction(conn, || create_within_transaction(conn, &input))
}

/// 创建协议本体（无事务语义，由 [`ensure_transaction`] 包裹）：落库 + 缓存行 +
/// op 产出（issue #860）——写与副作用全部成功后才追加 op，随同一事务提交/回滚。
fn create_within_transaction(conn: &Connection, input: &AccountInput) -> Result<String> {
    let row = AccountCommandRow {
        name: input.name.clone(),
        kind: input.kind,
        currency_code: input.currency_code.clone(),
        initial_balance_cents: input.initial_balance_cents.unwrap_or(0),
        is_hidden: false,
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    record_local(
        conn,
        AccountCommand::Create {
            id: id.clone(),
            row,
        },
    )?;
    Ok(id)
}

/// 账户落库协议（本地创建与重放共用，无 op 产出）：插入行 + 建余额缓存行
/// （issue #491 / ADR-0067）。调用方保证处于写事务内。
fn write_create(conn: &Connection, id: &str, row: &AccountCommandRow) -> Result<String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,0,?9)",
        rusqlite::params![
            id,
            row.name,
            row.kind,
            row.currency_code,
            row.initial_balance_cents,
            now,
            now,
            device_id(conn)?,
            row.is_hidden,
        ],
    )?;
    // 余额缓存写路径（issue #491 / ADR-0067）：新账户建缓存行（初始余额 + 零流水）。
    refresh_account_balances(conn, &[id])?;
    Ok(id.to_string())
}

/// 按自然键（name + type + currency_code）幂等创建账户：已存在（未删除）时返回已有 id，
/// 不重复插入、不报错。供 HTTP 导入 API 使用。
pub fn create_account_idempotent(conn: &Connection, input: AccountInput) -> Result<String> {
    if let Some(id) = find_account_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_account(conn, input)
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
pub fn delete_account(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, AccountCommand::Delete { id: id.to_string() })
    })
}

/// 账户软删协议（本地删除与重放共用，无 op 产出）：存在性检查 + 软删 + 缓存
/// touch。调用方保证处于写事务内。
fn write_delete(conn: &Connection, id: &str) -> Result<()> {
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
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    // 余额缓存写路径：touch 缓存行时间戳，净资产读探针指纹即时感知账户删除。
    refresh_account_balances(conn, &[id])?;
    Ok(())
}

/// 按 `id` 读取单个未删除账户；不存在或已软删除返回 `AppError::NotFound`。
pub fn get_account(conn: &Connection, id: &str) -> Result<Account> {
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
pub fn update_account(conn: &Connection, id: &str, input: AccountUpdateInput) -> Result<()> {
    ensure_transaction(conn, || {
        let (name, currency_code) = write_update(conn, id, &input)?;
        record_local(
            conn,
            AccountCommand::Update {
                id: id.to_string(),
                name,
                currency_code,
            },
        )
    })
}

/// 账户编辑协议（本地修改与重放共用，无 op 产出）：解决 + 校验 + 落库，返回
/// 解决后的落定值（名称非空、币种为实际生效值）。调用方保证处于写事务内。
fn write_update(
    conn: &Connection,
    id: &str,
    input: &AccountUpdateInput,
) -> Result<(String, String)> {
    let existing = get_account(conn, id)?;
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
        rusqlite::params![id, &name, &currency_code, now_iso(), device_id(conn)?],
    )?;
    // 余额缓存写路径：touch 缓存行时间戳（币种改动影响净资产折算口径，读探针需即时感知）。
    refresh_account_balances(conn, &[id])?;
    Ok((name, currency_code))
}

/// 重放执行：创建（同事务内由同步引擎包裹；不产出 op）。
pub(crate) fn replay_create(conn: &Connection, id: &str, row: &AccountCommandRow) -> Result<()> {
    write_create(conn, id, row)?;
    Ok(())
}

/// 重放执行：修改——携带的即解决后的落定值，以同一协议复验依赖（币种锁定、
/// 币种存在）后落库。
pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    currency_code: &str,
) -> Result<()> {
    write_update(
        conn,
        id,
        &AccountUpdateInput {
            name: Some(name.to_string()),
            currency_code: Some(currency_code.to_string()),
        },
    )?;
    Ok(())
}

/// 重放执行：软删除（同一协议含存在性检查：引用失败挂起待裁决）。
pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
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
pub fn ensure_black_hole_account(conn: &Connection, currency_code: &str) -> Result<(String, bool)> {
    if let Some(id) = find_black_hole_account(conn, currency_code)? {
        return Ok((id, false));
    }
    // 复用账户落库协议（与 create_account 同一 INSERT + 缓存行不变量），
    // op 载荷与落库行同源构造（无第二事实源漂移）。
    let row = AccountCommandRow {
        name: format!("无({currency_code})"),
        kind: AccountType::Other,
        currency_code: currency_code.to_string(),
        initial_balance_cents: 0,
        is_hidden: true,
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    // op 产出接缝（issue #860）：黑洞即建也是账户写——随调用方事务追加创建 op，
    // 否则对端重放本笔调整交易时账户缺失而挂起（外键依赖失败）。
    record_local(
        conn,
        AccountCommand::Create {
            id: id.clone(),
            row,
        },
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
pub fn adjust_account_balance(
    conn: &Connection,
    id: &str,
    input: &AccountBalanceAdjustInput,
) -> Result<(String, bool)> {
    let account = get_account(conn, id)?;
    if account.is_hidden {
        return Err(AppError::coded(
            "account.black-hole-adjust-unsupported",
            "黑洞账户不支持余额调整",
        ));
    }
    // 余额调整取数（五出口之一，issue #491）：读缓存而非实时聚合。
    let current = crate::balance::cached_balance(conn, id)?;
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
    // 余额调整外层事务壳（issue #310）：黑洞账户 ensure 与交易写入必须同事务。
    // 无条件自持原语归基础设施 `db::tx_scope::hold_transaction`（issue #1014 / #1003
    // 定案 3/4）：中途失败整体回滚、COMMIT 失败尽力清理，失败语义与行为层编排入口统一。
    hold_transaction(conn, || {
        let (black_hole_id, created) = ensure_black_hole_account(conn, &account.currency_code)?;
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
                funding_account_id: None,
                note: Some(input.note.clone().unwrap_or_else(|| "余额调整".to_string())),
                date: input.date.clone(),
                instrument_id: None,
                quantity: None,
                price_cents: None,
                fee_cents: None,
                to_instrument_id: None,
                to_quantity: None,
                out_amount_cents: None,
                in_amount_cents: None,
                idempotency_key: None,
            },
        )?
        .id;
        Ok((tx_id, created))
    })
}

/// 手动审计命令领域逻辑（issue #491 / ADR-0067）：全账户实时重算 vs 余额缓存，
/// 逐账户比对→修复（整体重算回写）→差异报告。唯一允许绕过 db::write 的缓存修复
/// 写入（与设置/恢复同列豁免形态）：缓存为派生数据，修复不置脏、不发信号。
pub fn audit_balance_cache(conn: &Connection) -> Result<BalanceCacheAudit> {
    let accounts = crate::balance::list_accounts_with_visibility(conn, true)?;
    let mut drifts = Vec::new();
    for account in &accounts {
        let actual = crate::balance::compute_balance(conn, &account.id)?;
        let cached = crate::balance::cached_balance_optional(conn, &account.id)?;
        if cached != Some(actual) {
            drifts.push(BalanceCacheDrift {
                account_id: account.id.clone(),
                account_name: account.name.clone(),
                cached_cents: cached,
                actual_cents: actual,
            });
        }
    }
    let repaired = !drifts.is_empty();
    if repaired {
        crate::balance::refresh_all_account_balances(conn)?;
    }
    Ok(BalanceCacheAudit {
        accounts_checked: accounts.len(),
        drifts,
        repaired,
    })
}

/// 账户余额清单（conn 级）：`include_hidden` 为 true 时含黑洞账户。
/// 余额读模型 SQL 收口在 [`crate::balance`]（ADR-0071）。
/// 域内薄委托：对外扁平签名（mod.rs 再导出）保持不变，清单口径单一来源在余额模块。
pub fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    crate::balance::list_account_balances_with_visibility(conn, include_hidden)
}

/// AI 侧余额清单：含黑洞账户。
pub fn list_account_balances_for_api(conn: &Connection) -> Result<Vec<AccountBalance>> {
    crate::balance::list_account_balances_with_visibility(conn, true)
}
