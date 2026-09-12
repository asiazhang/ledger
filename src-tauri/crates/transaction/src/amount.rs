//! Amount 接缝（issue #54 / spec #52）：交易金额口径的单一权威。
//!
//! 三块职责：
//! - [`TransactionKind`] 枚举：9 种交易类型的模块内真源（DB/wire 边界的小写字符串
//!   映射经 `as_str` / `parse` 收口，serde 以小写字符串序列化，wire 格式不变）。
//! - kind→度量矩阵：[`signed_amount`]（行级/展示）与四个 SQL 片段 builder
//!   （服务端聚合）由同一 [`coefficient`] 矩阵驱动，二者口径恒一致。
//! - [`convert_to_native`]：raw → 本位币折算，基准为全局默认币种
//!   （[`default_currency_code`]），不依赖 per-account 币种，避免跨账户漂移。
//!
//! 本模块为行为保持的 expand 步骤：消费方已接线（`transaction::writer::normalize`
//! 经 `convert_to_native` 折算本位币，服务端聚合经 SQL 片段 builder，余额/报表/预算
//! 消费矩阵口径），语义由测试锁定。

use std::fmt::Write as _;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use ledger_infra::closed_set::closed_set;
use ledger_infra::error::{AppError, Result};

// ---------------------------------------------------------------------------
// TransactionKind 枚举
// ---------------------------------------------------------------------------

closed_set! {
/// 交易类型真源（issue #73）。与 `transactions.kind` 的 CHECK 约束（V001）一一对应：
///
/// | kind | 含义 |
/// |------|------|
/// | [`TransactionKind::Income`] | 收入 |
/// | [`TransactionKind::Expense`] | 支出 |
/// | [`TransactionKind::Transfer`] | 转账（`account_id` 转出、`to_account_id` 转入） |
/// | [`TransactionKind::Refund`] | 退款（关联原支出交易） |
/// | [`TransactionKind::Buy`] | 买入证券（减少现金，扩展表记持仓） |
/// | [`TransactionKind::Sell`] | 卖出证券（增加现金） |
/// | [`TransactionKind::Dividend`] | 现金分红 |
/// | [`TransactionKind::Split`] | 拆股/送股（现金影响恒为 0） |
/// | [`TransactionKind::Convert`] | 基金转换（同一投资账户内两标的互换、无现金腿，六度量系数全 0） |
///
/// 五份表示（enum / `ALL` / `as_str` / `parse` / `Display`）由 `closed_set!`
/// 宏同体派生（ADR-0108）：字符串字面量每变体只出现一次，漏登/漏臂漂移
/// 不可表达。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionKind {
    Income => "income",
    Expense => "expense",
    Transfer => "transfer",
    Refund => "refund",
    Buy => "buy",
    Sell => "sell",
    Dividend => "dividend",
    Split => "split",
    Convert => "convert",
}
err_label = "交易类型",
err_code = "transaction.kind-unknown",
}

// rusqlite：从 `transactions.kind` 列直接读为枚举（DB 边界：TEXT 列经 [`TransactionKind::parse`]
// 严格映射，未知值即 FromSql 错误——DB CHECK 约束（V001）保证正常数据不可达）。
impl rusqlite::types::FromSql for TransactionKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        TransactionKind::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
// （income/expense/transfer/refund/buy/sell/dividend/split）。内联 schema，
// 消费方（如 [`super::model::Transaction`]）字段直接嵌入、无需注册组件。
impl PartialSchema for TransactionKind {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(TransactionKind::ALL.map(|k| k.as_str().to_string())))
                .description(Some(
                    "交易类型（闭集，小写字符串，与 transactions.kind 的 CHECK 约束一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for TransactionKind {}

// serde：以与 `transactions.kind` 同形的小写字符串序列化（wire 格式即小写字符串，
// 与裸 String 时代的 JSON 形状一致）；反序列化复用 [`TransactionKind::parse`]，
// 未知值报错文案与 parse 同源（serde 包装后附位置信息）。
impl Serialize for TransactionKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TransactionKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TransactionKind::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// 具名度量
// ---------------------------------------------------------------------------

/// 账户现金流量的归因端点：转出侧（`account_id`）、转入侧（`to_account_id`）
/// 或出资侧（`funding_account_id`，ADR-0096）。`account_flow` 度量对 transfer 的
/// 符号由转出/转入侧决定；出资侧只承载 buy/sell 的现金腿。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferSide {
    /// 转出账户侧：现金流出（−）。
    Out,
    /// 转入账户侧：现金流入（+）。
    In,
    /// 出资账户侧（ADR-0096）：buy/sell 的结算现金端点——结算账户 =
    /// 出资账户 ?? 投资账户；buy 记 −、sell 记 +，kind 矩阵符号不变、
    /// 归因端点可变。非 buy/sell 行不经出资端（准入收口，防御性记 0）。
    Funding,
}

/// 具名金额度量。每种度量对每种 kind 的符号见 [`coefficient`] 矩阵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    /// 账户现金流动：某账户视角下的现金出入（余额口径）。
    /// 转账按 [`TransferSide`] 取号，其余 kind 与侧无关。
    AccountFlow(TransferSide),
    /// 支出净额 = 毛支出 − 退款；投资类（buy/sell）不计入经营收支。
    ExpenseNet,
    /// 收入净额 = 收入 + 分红（dividend 计入收入）。
    IncomeNet,
    /// 退款毛额：独立成列可看，供毛值/净值并存展示。
    RefundGross,
    /// 保单已缴保费（挂单 `expense` 之和）：保单视角统计口径（issue #363 /
    /// ADR-0051 决策 6）。挂单准入（行为层，issue #361）保证只有 expense/income
    /// 可携带 policy_id，故本度量恒不含退款冲减——保单现金流入记 income 挂单
    /// 而非 refund（ADR-0051 决策 4），保费口径不被冲小。
    PolicyPremium,
    /// 保单现金流入（挂单 `income` 之和）：理赔/退保/满期返还统一记 income 挂单
    /// （ADR-0051 决策 4），三者记账语义无差别。
    PolicyInflow,
}

/// kind→度量系数矩阵（单一真源）：-1 / 0 / +1。
///
/// Rust 助手 [`signed_amount`] 与 SQL 片段 builder 均由此驱动；
/// 修改任何口径只改这里，两侧行为同步变化。
fn coefficient(kind: TransactionKind, measure: Measure) -> i64 {
    match measure {
        // 账户现金流动：buy/sell 的符号与端点无关（矩阵符号不变，归因端点可变，
        // ADR-0096）——转出侧带出资守卫后只对未出资行生效，出资侧只对 buy/sell
        // 生效（见 [`account_flow_expr`] 的归因规则）。非 buy/sell 行不经出资端
        // （准入收口在行为层，防御性记 0）。
        Measure::AccountFlow(side) => match kind {
            TransactionKind::Income | TransactionKind::Refund | TransactionKind::Dividend => {
                match side {
                    TransferSide::Funding => 0,
                    _ => 1,
                }
            }
            TransactionKind::Sell => 1,
            TransactionKind::Expense => match side {
                TransferSide::Funding => 0,
                _ => -1,
            },
            TransactionKind::Buy => -1,
            TransactionKind::Transfer => match side {
                TransferSide::Out => -1,
                TransferSide::In => 1,
                TransferSide::Funding => 0,
            },
            // convert 无现金腿（ADR-0099）：两侧标的互换不涉任何账户现金，
            // 三个侧别恒记 0——落账前后全部账户余额（含黑洞）不变。
            TransactionKind::Split | TransactionKind::Convert => 0,
        },
        Measure::ExpenseNet => match kind {
            TransactionKind::Expense => 1,
            TransactionKind::Refund => -1,
            TransactionKind::Income
            | TransactionKind::Transfer
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::IncomeNet => match kind {
            TransactionKind::Income | TransactionKind::Dividend => 1,
            TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::RefundGross => match kind {
            TransactionKind::Refund => 1,
            TransactionKind::Income
            | TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::PolicyPremium => match kind {
            TransactionKind::Expense => 1,
            TransactionKind::Income
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
        Measure::PolicyInflow => match kind {
            TransactionKind::Income => 1,
            TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Refund
            | TransactionKind::Buy
            | TransactionKind::Sell
            | TransactionKind::Dividend
            | TransactionKind::Split
            | TransactionKind::Convert => 0,
        },
    }
}

/// 行级/展示用有符号金额：`coefficient(kind, measure) × amount_native_cents`。
///
/// 输入应为本位币金额（`amount_native_cents`）；
/// `split` 对现金度量恒为 0，buy/sell 不进 expense_net/income_net。
pub fn signed_amount(kind: TransactionKind, amount_native_cents: i64, measure: Measure) -> i64 {
    coefficient(kind, measure) * amount_native_cents
}

// ---------------------------------------------------------------------------
// SQL 片段 builder（服务端聚合）
// ---------------------------------------------------------------------------

/// 由 coefficient 矩阵生成 `CASE ... END` 片段：按系数分组 kind，
/// 输出对 `alias.amount_native_cents` 的有符号表达式。
///
/// 只负责 kind→符号，不含 `is_deleted` 等过滤，过滤条件由调用方 WHERE 决定。
fn kind_case_expr(alias: &str, measure: Measure) -> String {
    let amount_col = format!("{alias}.amount_native_cents");
    let kind_col = format!("{alias}.kind");
    let mut pos: Vec<&'static str> = Vec::new();
    let mut neg: Vec<&'static str> = Vec::new();
    for kind in TransactionKind::ALL {
        match coefficient(kind, measure) {
            1 => pos.push(kind.as_str()),
            -1 => neg.push(kind.as_str()),
            _ => {}
        }
    }
    let mut expr = String::from("CASE");
    if !pos.is_empty() {
        let _ = write!(
            expr,
            " WHEN {kind_col} IN ({list}) THEN {amount_col}",
            list = quote_list(&pos)
        );
    }
    if !neg.is_empty() {
        let _ = write!(
            expr,
            " WHEN {kind_col} IN ({list}) THEN -{amount_col}",
            list = quote_list(&neg)
        );
    }
    expr.push_str(" ELSE 0 END");
    expr
}

fn quote_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// `account_flow` 聚合片段。转账符号按 `side` 取：
/// 转出侧 join `t.account_id`、转入侧 join `t.to_account_id` 后分别求和相加。
///
/// 出资账户归因规则单条在此落库为 SQL（ADR-0096 决策 2，全库唯一直 incarnation）：
/// 结算账户 = 出资账户 ?? 投资账户（`account_id`）——带出资账户的 buy/sell，其现金
/// 腿归出资侧（[`TransferSide::Funding`] 端点，join `funding_account_id`），转出侧
/// （投资账户端）对带出资账户的 buy/sell 记 0（钱不经手）；未填出资账户时矩阵符号
/// 原样记在投资账户上。转入侧不需要守卫：buy/sell 行恒无 `to_account_id`——
/// 该不变量由投资域 prepare / 重放装配的 forbidden 守卫拒绝携带保证
/// （`trade.buy-to-account-forbidden` / `trade.sell-to-account-forbidden`，
/// issue #1187），本表达式不另设第二份判定。
pub fn account_flow_expr(alias: &str, side: TransferSide) -> String {
    let matrix = kind_case_expr(alias, Measure::AccountFlow(side));
    match side {
        TransferSide::Out => format!(
            "CASE WHEN {alias}.kind IN ('buy','sell') \
             AND {alias}.funding_account_id IS NOT NULL THEN 0 ELSE ({matrix}) END"
        ),
        TransferSide::In | TransferSide::Funding => matrix,
    }
}

/// `expense_net` 聚合片段（毛支出 − 退款）。
pub fn expense_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::ExpenseNet)
}

/// `income_net` 聚合片段（收入 + 分红）。
pub fn income_net_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::IncomeNet)
}

/// `refund_gross` 聚合片段（退款毛额）。
pub fn refund_gross_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::RefundGross)
}

/// `policy_premium` 聚合片段（保单已缴保费，issue #363）：挂单 `expense` 之和。
pub fn policy_premium_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyPremium)
}

/// `policy_inflow` 聚合片段（保单现金流入，issue #363）：挂单 `income` 之和。
pub fn policy_inflow_expr(alias: &str) -> String {
    kind_case_expr(alias, Measure::PolicyInflow)
}

/// 毛支出聚合片段 = `expense_net + refund_gross`（spec #52 净值关系恒等式：
/// `expense_net = expense_gross − refund_gross`）。
///
/// 毛值不作为独立度量进入矩阵，而由两个具名度量经恒等式导出，
/// 毛值/净值口径由同一矩阵驱动、永不漂移（月度汇总毛值三列用，见 issue #57）。
pub fn expense_gross_expr(alias: &str) -> String {
    format!(
        "({} + {})",
        expense_net_expr(alias),
        refund_gross_expr(alias)
    )
}

/// 对度量有贡献（系数非 0）的 kind 字符串列表，矩阵驱动。
/// 供聚合 SQL 的 `WHERE kind IN (...)` 行过滤使用，与聚合片段出自同一矩阵，
/// 避免手写 kind 清单漂移（如 income_net 必须含 dividend）。
pub fn contributing_kinds(measure: Measure) -> Vec<&'static str> {
    TransactionKind::ALL
        .into_iter()
        .filter(|k| coefficient(*k, measure) != 0)
        .map(|k| k.as_str())
        .collect()
}

/// 带引号的贡献 kind 清单（如 `'expense','refund'`），可直接内插进
/// `WHERE kind IN (...)`。供聚合 SQL 行过滤用，与 [`contributing_kinds`]
/// 同源，消费方不再各自手拼 SQL 字面量（budget / reports 共用）。
pub fn contributing_kinds_sql(measure: Measure) -> String {
    quote_list(&contributing_kinds(measure))
}

// ---------------------------------------------------------------------------
// 本位币折算
// ---------------------------------------------------------------------------

/// 全局默认（本位）币种基准：`amount_native_cents` 的折算基准，读账本级设置
/// （LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）：存储落
/// `app_settings`（ADR-0017，读写协议归币种域），缺 key / 缺表回默认 CNY（行为
/// 免费正确），随多端同步分发、全设备强制一致。读取自 #1092 起经交易×币种接缝
/// （[`super::base_currency_seam`]）：注册点在本域，实现由币种域启动时装入，
/// 本域对币种域零直接依赖。模块内其余口径不变。
pub fn default_currency_code(conn: &Connection) -> Result<String> {
    super::base_currency_seam::current_base_currency(conn)
}

/// 查询货币对当前汇率（正查失败则反查取倒数）。
///
/// 私有依赖（spec #52）：旧壳层同名查询已随 issue #60 接线删除，
/// 本函数即其收口后的单一实现（Writer 接缝落地时已统一）。
fn lookup_exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::codedp(
        "fx.rate-missing",
        format!("未找到 {base_code} -> {quote_code} 的汇率（正反向均无）"),
        &[base_code, quote_code],
    ))
}

/// 将原始币种金额折算为**全局默认币种**金额（四舍五入到分）。
///
/// - 币种与默认币种相同 → 1:1 原样返回。
/// - 基准为 [`default_currency_code`]，**与账户币种无关**：
///   各账户的交易统一折算到同一本位币，避免跨账户漂移。
/// - 正反向汇率均无 → 报错，不静默混币种。
pub fn convert_to_native(conn: &Connection, amount_cents: i64, currency_code: &str) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, &target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}
