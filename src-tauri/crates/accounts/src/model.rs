//! 账户领域模型（#419 随域归位）：账户类型枚举、账户实体与入参、账户余额读模型 DTO。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布），
//! 消费方经 `accounts` 域路径逐类型显式 import。余额计算引擎与余额读查询
//! 已自 `db::balance` 迁入本域 [`super::balance`]（ADR-0071）。

use std::fmt;
use std::str::FromStr;

use ledger_infra::db::query::FromRow;
use ledger_infra::error::AppError;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Cash,
    Bank,
    Credit,
    Ewallet,
    Investment,
    Debt,
    Receivable,
    Other,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountType::Cash => write!(f, "cash"),
            AccountType::Bank => write!(f, "bank"),
            AccountType::Credit => write!(f, "credit"),
            AccountType::Ewallet => write!(f, "ewallet"),
            AccountType::Investment => write!(f, "investment"),
            AccountType::Debt => write!(f, "debt"),
            AccountType::Receivable => write!(f, "receivable"),
            AccountType::Other => write!(f, "other"),
        }
    }
}

impl FromStr for AccountType {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cash" => Ok(AccountType::Cash),
            "bank" => Ok(AccountType::Bank),
            "credit" => Ok(AccountType::Credit),
            "ewallet" => Ok(AccountType::Ewallet),
            "investment" => Ok(AccountType::Investment),
            "debt" => Ok(AccountType::Debt),
            "receivable" => Ok(AccountType::Receivable),
            "other" => Ok(AccountType::Other),
            _ => Err(AppError::codedp(
                "account.type-unknown",
                format!("未知账户类型: {s}"),
                &[s],
            )),
        }
    }
}

impl ToSql for AccountType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for AccountType {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    /// 黑洞账户标志：对用户侧列表/余额/下拉选择器隐藏，但交易仍参与交易列表与报表。
    pub is_hidden: bool,
    /// 信用额度（整数分，仅信用卡；`None` = 未设置）。档案字段，不参与余额与净资产。
    pub credit_limit_cents: Option<i64>,
    /// 账单日（1–31 的每月第 N 日声明值，仅信用卡；`None` = 未设置）。
    pub statement_day: Option<i64>,
    /// 还款日（1–31 的每月第 N 日声明值，仅信用卡；`None` = 未设置）。
    pub due_day: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
    /// 信用卡档案字段（仅 `credit` 账户；缺省/`null` = 未设置，三个字段彼此独立）。
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

/// 账户编辑入参（IPC `update_account` / HTTP `PUT /api/v1/accounts/{id}`）。
/// `type` 不可改：账户类型参与 kind→符号矩阵（余额方向），改动会重写历史交易
/// 的余额归属（ADR-0026 同期决策，Q3）；`initial_balance_cents` 不在此改，
/// 归余额调整（见参考数据与设置域 BalanceAdjustment）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountUpdateInput {
    pub name: Option<String>,
    /// 仅无交易账户可改（有交易时改币种会使历史折算口径错乱，后端拒绝）。
    pub currency_code: Option<String>,
    /// 信用卡档案字段三态：键缺席 = 不改、`null` = 清空、给值 = 落定该值
    /// （区分器 [`ledger_infra::serde_util::double_option`]：serde 默认把
    /// 「键缺席」与「值为 `null`」折叠成同一 `None`，原理与用法见其模块文档）。
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub credit_limit_cents: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub statement_day: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub due_day: Option<Option<i64>>,
}

/// 信用卡档案字段三件套——**不是** wire 形态（wire 与库列都是三个扁平字段），而是
/// 守卫与范围校验的单点载体：三条写入路径（本地创建 / 本地编辑 / 同步重放）消费同一
/// 入口，规则只写一遍（spec #1327 / ADR-0119）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CreditTerms {
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

impl CreditTerms {
    /// 自三个扁平字段装配（入参序：信用额度 / 账单日 / 还款日）。
    pub fn new(
        credit_limit_cents: Option<i64>,
        statement_day: Option<i64>,
        due_day: Option<i64>,
    ) -> Self {
        Self {
            credit_limit_cents,
            statement_day,
            due_day,
        }
    }

    /// 三字段是否全未设置——「未携带信用卡档案」的判据。
    pub fn is_unset(&self) -> bool {
        self.credit_limit_cents.is_none() && self.statement_day.is_none() && self.due_day.is_none()
    }

    /// 携带守卫 + 范围校验（单点）：
    /// - 非空即要求账户类型为 `credit`，否则拒绝——额度与账单日是信用卡的档案，
    ///   挂到现金或负债账户上会让账户类型与属性失去对应关系；
    /// - 信用额度须为正整数分（`0` 与负数非法，`None` 才是「未设置」）；
    /// - 账单日 / 还款日须落在 1–31（月末钳制不在存储层，见 [`Account`] 字段注释）。
    pub fn validate_for(&self, kind: AccountType) -> Result<(), AppError> {
        if self.is_unset() {
            return Ok(());
        }
        if kind != AccountType::Credit {
            return Err(AppError::coded(
                "account.credit-attribute-not-applicable",
                "信用额度、账单日与还款日仅信用卡账户可填写",
            ));
        }
        if self.credit_limit_cents.is_some_and(|limit| limit <= 0) {
            return Err(AppError::coded(
                "account.credit-limit-invalid",
                "信用额度必须大于 0",
            ));
        }
        for (day, code, label) in [
            (
                self.statement_day,
                "account.statement-day-invalid",
                "账单日",
            ),
            (self.due_day, "account.due-day-invalid", "还款日"),
        ] {
            if let Some(day) = day.filter(|d| !(1..=31).contains(d)) {
                let value = day.to_string();
                return Err(AppError::codedp(
                    code,
                    format!("{label}必须在 1 到 31 之间: {value}"),
                    &[value.as_str()],
                ));
            }
        }
        Ok(())
    }
}

/// 余额调整入参（IPC `adjust_account_balance`）：把余额校准到目标值，
/// 机制为生成一笔与黑洞账户的转账（ADR-0026）。
#[derive(Debug, Deserialize)]
pub struct AccountBalanceAdjustInput {
    pub target_balance_cents: i64,
    /// 调整交易日期（YYYY-MM-DD，对账常补记过去日期）。
    pub date: String,
    /// 调整交易备注；缺省后端补「余额调整」。
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountBalance {
    pub account: Account,
    pub balance_cents: i64,
}

/// 余额缓存审计差异行（issue #491 / ADR-0067）：缓存缺失记 None（回填前）。
#[derive(Debug, Serialize, ToSchema)]
pub struct BalanceCacheDrift {
    pub account_id: String,
    pub account_name: String,
    pub cached_cents: Option<i64>,
    pub actual_cents: i64,
}

/// 余额缓存审计报告（issue #491 / ADR-0067）：修复已完成后的差异快照。
#[derive(Debug, Serialize, ToSchema)]
pub struct BalanceCacheAudit {
    pub accounts_checked: usize,
    pub drifts: Vec<BalanceCacheDrift>,
    pub repaired: bool,
}

impl FromRow for Account {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Account {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            currency_code: row.get(3)?,
            initial_balance_cents: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
            is_deleted: row.get::<_, i64>(9)? != 0,
            is_hidden: row.get::<_, i64>(10)? != 0,
            credit_limit_cents: row.get(11)?,
            statement_day: row.get(12)?,
            due_day: row.get(13)?,
        })
    }
}
