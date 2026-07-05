use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub currency_code: String,
    pub initial_balance_cents: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CategoryInput {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    pub id: i64,
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: i64,
    pub to_account_id: Option<i64>,
    pub category_id: Option<i64>,
    pub note: Option<String>,
    pub date: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct TransactionInput {
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: i64,
    pub to_account_id: Option<i64>,
    pub category_id: Option<i64>,
    pub note: Option<String>,
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Budget {
    pub id: i64,
    pub category_id: i64,
    pub period: String,
    pub amount_cents: i64,
    pub start_date: String,
}

#[derive(Debug, Deserialize)]
pub struct BudgetInput {
    pub category_id: i64,
    pub period: Option<String>,
    pub amount_cents: i64,
    pub start_date: String,
}

#[derive(Debug, Serialize)]
pub struct AccountBalance {
    pub account: Account,
    pub balance_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct MonthlySummary {
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct CategoryShare {
    pub category_id: i64,
    pub category_name: String,
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct BudgetProgress {
    pub budget: Budget,
    pub category_name: String,
    pub spent_cents: i64,
    pub over_budget: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportedRow {
    pub date: String,
    pub amount_cents: i64,
    pub note: String,
    pub category_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub path: String,
}
