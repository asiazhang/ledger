//! 币种领域模型（#418 随域归位）：币种实体 + 汇率实体。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布）：
//! 汇率是币种参考数据的从属实体（`exchange_rates` 以 base/quote 双外键挂在
//! `currencies` 上），随币种域走；投资域是汇率的消费方与录入入口，经域路径
//! 显式 import 消费。域内类型经 `currencies` 逐类型再导出，禁止 glob。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i64,
}

impl FromRow for Currency {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Currency {
            code: row.get(0)?,
            name: row.get(1)?,
            symbol: row.get(2)?,
            decimal_places: row.get(3)?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExchangeRate {
    pub id: String,
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRateInput {
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
}

impl FromRow for ExchangeRate {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ExchangeRate {
            id: row.get(0)?,
            base_code: row.get(1)?,
            quote_code: row.get(2)?,
            rate: row.get(3)?,
            priced_at: row.get(4)?,
            source: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
        })
    }
}
