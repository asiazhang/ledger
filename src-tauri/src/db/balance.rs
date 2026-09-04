use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::accounts::{Account, AccountBalance};
use crate::db::query::{FromRow, query_all};
use crate::error::{AppError, Result};
use crate::transaction::amount::{TransferSide, account_flow_expr};

struct AccountBalanceEntry {
    id: String,
    balance_cents: i64,
}

impl FromRow for AccountBalanceEntry {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(AccountBalanceEntry {
            id: row.get(0)?,
            balance_cents: row.get(1)?,
        })
    }
}

/// `account_flow` 对 transfer 的符号由 side 决定，而 side 决定关联列：
/// 转出侧 join `t.account_id`、转入侧 join `t.to_account_id`。
/// 单个与批量余额共用本映射，口径一致性由代码结构保证而非注释约定。
fn join_column(side: TransferSide) -> &'static str {
    match side {
        TransferSide::Out => "t.account_id",
        TransferSide::In => "t.to_account_id",
    }
}

/// 对指定账户（`account_ref` 为 `?1` 参数或 `a.id` 列引用）的
/// `account_flow` 聚合子查询。各 kind 对余额的符号
/// （income/refund/sell/dividend 为 +，expense/buy 为 −，
/// transfer 转出侧 −/转入侧 +，split 恒 0）由 kind→度量矩阵单一真源决定。
fn account_flow_subquery(side: TransferSide, account_ref: &str) -> String {
    format!(
        "(SELECT COALESCE(SUM({expr}),0) FROM transactions t \
         WHERE t.is_deleted=0 AND {col}={account_ref})",
        expr = account_flow_expr("t", side),
        col = join_column(side),
    )
}

/// 计算账户当前余额 = 初始余额 + Σ account_flow（转出侧） + Σ account_flow（转入侧）。
pub fn compute_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_out: i64 = conn.query_row(
        &format!("SELECT {}", account_flow_subquery(TransferSide::Out, "?1")),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_in: i64 = conn.query_row(
        &format!("SELECT {}", account_flow_subquery(TransferSide::In, "?1")),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    Ok(initial + flow_out + flow_in)
}

/// 批量计算所有未删除账户的余额，单条 SQL 查询。
///
/// 原理：初始余额 + 两个 `account_flow` 关联子查询（转出侧/转入侧）
/// 在一条 SQL 内完成汇总，口径与 [`compute_balance`] 完全一致
/// （同一度量片段、同一关联语义），单个与批量结果恒相等。
/// 对 N 个账户保持 O(1) 次数据库往返。
/// UI 侧不包含黑洞账户；AI 对账需要 `include_hidden = true`。
pub fn compute_all_balances(conn: &Connection) -> Result<HashMap<String, i64>> {
    compute_all_balances_with_visibility(conn, false)
}

/// 批量计算未删除账户余额；`include_hidden` 为 true 时含黑洞账户。
pub fn compute_all_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<HashMap<String, i64>> {
    let hidden_clause = if include_hidden {
        ""
    } else {
        "AND a.is_hidden = 0"
    };
    let sql = format!(
        "SELECT a.id,
                a.initial_balance_cents
                + COALESCE({out}, 0)
                + COALESCE({tin}, 0)
         FROM accounts a
         WHERE a.is_deleted = 0 {hidden_clause}",
        out = account_flow_subquery(TransferSide::Out, "a.id"),
        tin = account_flow_subquery(TransferSide::In, "a.id"),
    );
    let entries: Vec<AccountBalanceEntry> = query_all(conn, &sql, [])?;

    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}

/// 账户行可见性读取（conn 级）：`include_hidden` 为 true 时含黑洞账户。
/// 账户余额清单的账户侧来源，可见性口径与余额侧同一开关（#405 自账户壳层
/// 模块下沉至此，与余额计算同址；账户域归位后随迁账户域）。
pub fn list_accounts_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<Account>> {
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

/// 账户余额清单（conn 级）：账户行 × 当前余额，`include_hidden` 为 true 时含黑洞账户。
/// 账户列表/余额页、dashboard 净资产与投资域财务自由度共用的同一口径
/// （#405 自账户壳层模块下沉至此，消除聚合域对壳层的反向依赖；账户域归位 #404 时随迁）。
pub fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    let accounts = list_accounts_with_visibility(conn, include_hidden)?;
    let balances = cached_all_balances(conn)?;
    accounts
        .into_iter()
        .map(|a| {
            let balance_cents = balances.get(&a.id).copied().ok_or_else(|| {
                AppError::codedp(
                    "balance.cache-row-missing",
                    format!("账户 {} 缺少余额缓存行，请执行余额缓存审计修复", a.name),
                    &[a.id.as_str()],
                )
            })?;
            Ok(AccountBalance {
                balance_cents,
                account: a,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 余额持久化缓存（issue #491 / ADR-0066）：写路径整体重算 + 读路径切缓存
// ---------------------------------------------------------------------------

/// 缓存行时间戳：毫秒精度 UTC ISO 时刻（与全库 `now_iso` 同为 UTC，仅精度不同）。
///
/// 秒级精度会让同一秒内的连续写入无法从 MAX(updated_at) 指纹中区分
/// （净资产读探针将误判缓存仍新鲜），故缓存表自带毫秒精度时间戳；
/// 源表 updated_at 的秒级精度是既有冻结约定，不在此改动。
fn now_iso_millis() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// 对给定账户按唯一口径表达式整体重算余额并写入缓存（禁止增量加减）。
///
/// 表达式由 [`account_flow_subquery`] 生成（与 [`compute_balance`] 同一真源），
/// 单条 UPSERT…SELECT 完成；每次调用无条件刷新 `updated_at`（毫秒精度），
/// 即使余额值未变——这是净资产读探针指纹判定「源已变更」的依据之一。
/// 必须在调用方既有写事务内调用（与引发重算的写入同事务，ADR-0066）。
pub fn refresh_account_balances(conn: &Connection, account_ids: &[&str]) -> Result<()> {
    if account_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; account_ids.len()].join(",");
    let sql = refresh_upsert_sql(&format!("WHERE a.id IN ({placeholders})"));
    let now = now_iso_millis();
    let params: Vec<&str> = std::iter::once(now.as_str())
        .chain(account_ids.iter().copied())
        .collect();
    conn.execute(&sql, rusqlite::params_from_iter(params))?;
    Ok(())
}

/// 全账户整体重算并回写缓存：手动审计命令的修复路径（issue #491）。
pub fn refresh_all_account_balances(conn: &Connection) -> Result<()> {
    // WHERE true：INSERT…SELECT 尾接 ON CONFLICT 需 WHERE 子句消歧（SQLite 语法）。
    let sql = refresh_upsert_sql("WHERE true");
    conn.execute(&sql, rusqlite::params![now_iso_millis()])?;
    Ok(())
}

/// 余额缓存整体重算 UPSERT 语句的单一构造点：两个刷新入口共享同一条
/// INSERT…SELECT…ON CONFLICT（口径表达式同源，仅账户筛选范围不同）。
fn refresh_upsert_sql(account_filter: &str) -> String {
    format!(
        "INSERT INTO account_balance_cache (account_id, balance_cents, updated_at) \
         SELECT a.id, \
                a.initial_balance_cents + COALESCE({out}, 0) + COALESCE({tin}, 0), \
                ? \
         FROM accounts a {account_filter} \
         ON CONFLICT(account_id) DO UPDATE SET \
             balance_cents = excluded.balance_cents, \
             updated_at = excluded.updated_at",
        out = account_flow_subquery(TransferSide::Out, "a.id"),
        tin = account_flow_subquery(TransferSide::In, "a.id"),
    )
}

/// 读取单账户缓存余额；缓存行缺失视为不变量破坏（正常路径由迁移回填与
/// 账户创建/写入接缝维护），码化错误上抛引导审计修复，不静默回退实时计算。
pub fn cached_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    cached_balance_optional(conn, account_id)?.ok_or_else(|| {
        AppError::codedp(
            "balance.cache-row-missing",
            "账户缺少余额缓存行，请执行余额缓存审计修复",
            &[account_id],
        )
    })
}

/// 读取单账户缓存余额的 Option 形态：缺失返回 `None` 而非报错。
/// 供审计命令逐户比对（缺失本身就是要报告的差异形态），正常读出口仍走
/// [`cached_balance`] 的码化错误路径。
pub fn cached_balance_optional(conn: &Connection, account_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT balance_cents FROM account_balance_cache WHERE account_id = ?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Into::into)
}

/// 读取全账户缓存余额映射（缓存读侧单一来源）。
fn cached_all_balances(conn: &Connection) -> Result<HashMap<String, i64>> {
    let entries: Vec<AccountBalanceEntry> = query_all(
        conn,
        "SELECT account_id, balance_cents FROM account_balance_cache",
        [],
    )?;
    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}
