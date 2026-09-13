//! `investment` 命令测试共享脚手架（issue #257 纯移动自原 tests.rs 顶部，
//! 抽取仅限本测试模块内部，跨模块合并见 issue #250）。
//!
//! 建库与跨域重复夹具（账户 / 标的字典 / 汇率 / 价格与汇率历史）已上收统一
//! 测试工厂 `tauri_app_lib::test_support`（spec #728 / ADR-0084，域迁移票 #755）：
//! 调用点直接用 `open` / `seed_account` / `seed_instrument` / `seed_exchange_rate`
//! / `seed_price_history` / `seed_fx_rate_history`。本薄皮按准入规则（ADR-0084
//! 决策 1）只留域特有形态：带市场 / 来源 / 基金类型的标的种子与交易输入构造器
//! （域语义，非 DB 夹具）；种子簿记戳一律引用工厂 [`FIXED_NOW`]，零字面量。

use rusqlite::{Connection, params};

use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use tauri_app_lib::test_support::FIXED_NOW;

// 既有测试经域根 glob（`super::super::*`）消费的旧壳 mod.rs 私有 use 绑定，
// 随 #401 域归位改由共享脚手架再导出（import 更新，断言与场景不变）；
// PnlFilter / TrendRange 已由域根模型再导出承载（#422），不再经脚手架转发。
pub(crate) use crate::{InstrumentListFilter, InstrumentListResult};
pub(crate) use ledger_infra::error::AppError;

/// 种入一个标的行（市场 / 类型显式的域内变体形态），panic 形态。库层约束断言
/// （同码同类型 UNIQUE 冲突等）需拿 `Err` 的用 [`try_insert_instrument_with_market`]。
pub(super) fn insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
) {
    try_insert_instrument_with_market(conn, id, symbol, name, currency, market, kind).unwrap();
}

/// [`insert_instrument_with_market`] 的可失败形态：断言库层约束（UNIQUE 冲突等）
/// 的测试需要 `Err` 而非 panic（种子统一 unwrap，不表达预期失败）。
pub(super) fn try_insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
) -> rusqlite::Result<String> {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,?6,?3,?4,?5,?7,?7,1,'test')",
        params![id, symbol, name, currency, market, kind, FIXED_NOW],
    )?;
    Ok(id.to_string())
}

/// 种入带来源标记的标的行（来源是域语义：'eastmoney' 同步 / 'manual' 手动，
/// 随行终身不变——ADR-0036「非同步即手动」）：来源标记与复用语义测试用。
/// 全位置参数保持列显式（ADR-0084 决策 4 同款理由），故显式 allow
/// `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
pub(super) fn insert_instrument_with_source(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
    source: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
          VALUES (?1,?2,?6,?3,?4,?5,?8,?8,1,'test',?7)",
        params![id, symbol, name, currency, market, kind, source, FIXED_NOW],
    )
    .unwrap();
}

/// 插入场外基金标的（type='fund'，市场 unknown——ADR-0038：基金无交易所市场
/// 概念，报价币种 CNY）。自 fund_trade 本地副本迁入（#755 裸 SQL 清零）。
pub(super) fn insert_fund_instrument(conn: &Connection, id: &str, symbol: &str, name: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'fund',?3,'CNY','unknown',?4,?4,1,'test')",
        params![id, symbol, name, FIXED_NOW],
    )
    .unwrap();
}

pub(super) fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

pub(super) fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

/// 基金转换输入构造器（ADR-0099）：转出腿 = instrument_id / quantity / out_amount，
/// 转入腿 = to_instrument_id / to_quantity / in_amount；行金额占位 0（服务端按
/// FIFO 消耗算定结转成本后写入锚点）。
#[allow(clippy::too_many_arguments)]
pub(super) fn make_convert_input(
    account_id: &str,
    instrument_id: &str,
    to_instrument_id: &str,
    quantity: f64,
    to_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Convert,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents: None,
        fee_cents: Some(fee_cents),
        to_instrument_id: Some(to_instrument_id.into()),
        to_quantity: Some(to_quantity),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(in_amount_cents),
        idempotency_key: None,
    }
}

/// 份额调整输入构造器（ADR-0106 / issue #1049）：无现金腿——金额 0、无单价、
/// 无手续费；`delta` 为带符号份额增量 Δ（`+` = 折算/结转/送股、`−` = 缩股）。
pub(super) fn make_split_input(
    account_id: &str,
    instrument_id: &str,
    delta: f64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Split,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(delta),
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

/// 现金分红输入构造器（issue #1078 / ADR-0109）：现金腿 = `amount_cents`，
/// 标的必填；无份额 / 单价 / 手续费。`currency` 须与到账账户币种一致（写路径守卫），
/// 到账账户可为任意在用账户（本构造器默认投资账户）。
pub(super) fn make_dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    currency: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: currency.into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: None,
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

/// 日期可指定的 buy/sell 输入（数量推算测试需要错开周采样日）。
pub(super) fn make_trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}
