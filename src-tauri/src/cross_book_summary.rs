use rusqlite::Connection;
use serde::Serialize;

use ledger_infra::db::book_registry::Book;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::db::encryption::probe_file_kind;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{open_connection_readonly_in, schema_version};
use ledger_infra::error::Result;
use ledger_investment as investment;
use ledger_transaction::amount;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossBookBookStatus {
    Included,
    Locked,
    NotInitialized,
    SchemaMismatch,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossBookBookRow {
    pub id: String,
    pub name: String,
    pub status: CrossBookBookStatus,
}

#[derive(Debug, Serialize)]
pub struct CrossBookInvestmentSummary {
    pub target_currency: String,
    pub converted: bool,
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    pub books: Vec<CrossBookBookRow>,
}

#[derive(Debug, Clone, Default)]
pub struct BookInvestmentReading {
    pub native_currency: String,
    pub holding_totals: Vec<investment::CurrencyHoldingTotals>,
    pub cumulative_pnl: Vec<investment::CurrencyCumulativePnl>,
    pub investable_assets: Option<(String, i64)>,
}

pub fn read_book_investment(conn: &Connection) -> Result<BookInvestmentReading> {
    ensure_transaction(conn, || {
        let holding_totals = investment::query_holdings_summary_by_currency(conn)?;
        let cumulative_pnl = investment::query_cumulative_pnl_summary(conn)?;
        let investable_assets_cents = investment::query_investable_assets_cents(conn)?;
        let native_currency = amount::default_currency_code(conn)?;
        Ok(BookInvestmentReading {
            native_currency: native_currency.clone(),
            holding_totals,
            cumulative_pnl,
            investable_assets: Some((native_currency, investable_assets_cents)),
        })
    })
}

pub fn read_active_book_and_merge(
    mut readings: Vec<BookInvestmentReading>,
    rows: Vec<CrossBookBookRow>,
    conn: &Connection,
) -> Result<CrossBookInvestmentSummary> {
    ensure_transaction(conn, || {
        readings.push(read_book_investment(conn)?);
        let target_currency = amount::default_currency_code(conn)?;
        let totals = merge_readings(&readings, &target_currency, &mut |cents, currency| {
            amount::convert_to_native_current(conn, cents, currency)
        })?;
        Ok(CrossBookInvestmentSummary {
            target_currency,
            converted: totals.converted,
            market_value_cents: totals.market_value_cents,
            unrealized_pnl_cents: totals.unrealized_pnl_cents,
            cumulative_pnl_cents: totals.cumulative_pnl_cents,
            investable_assets_cents: totals.investable_assets_cents,
            books: rows,
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTotals {
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    pub converted: bool,
}

pub fn merge_readings(
    readings: &[BookInvestmentReading],
    target_currency: &str,
    convert: &mut dyn FnMut(i64, &str) -> Result<i64>,
) -> Result<MergedTotals> {
    let mut totals = MergedTotals {
        market_value_cents: 0,
        unrealized_pnl_cents: 0,
        cumulative_pnl_cents: 0,
        investable_assets_cents: 0,
        converted: readings
            .iter()
            .any(|r| r.native_currency != target_currency),
    };
    let mut add = |sum: &mut i64, cents: i64, currency: &str| -> Result<()> {
        if currency == target_currency {
            *sum += cents;
        } else {
            totals.converted = true;
            *sum += convert(cents, currency)?;
        }
        Ok(())
    };
    for reading in readings {
        for group in &reading.holding_totals {
            if let Some(cents) = group.market_value_cents {
                add(&mut totals.market_value_cents, cents, &group.currency_code)?;
            }
            if let Some(cents) = group.unrealized_pnl_cents {
                add(
                    &mut totals.unrealized_pnl_cents,
                    cents,
                    &group.currency_code,
                )?;
            }
        }
        for group in &reading.cumulative_pnl {
            add(
                &mut totals.cumulative_pnl_cents,
                group.cumulative_pnl_cents,
                &group.currency_code,
            )?;
        }
        if let Some((currency, cents)) = &reading.investable_assets {
            add(&mut totals.investable_assets_cents, *cents, currency)?;
        }
    }
    Ok(totals)
}

pub fn collect_other_books(
    books: &[Book],
    active_id: &str,
    active_schema_version: i64,
) -> (Vec<CrossBookBookRow>, Vec<BookInvestmentReading>) {
    let mut rows = Vec::with_capacity(books.len());
    let mut readings = Vec::new();
    for book in books {
        if book.id == active_id {
            rows.push(CrossBookBookRow {
                id: book.id.clone(),
                name: book.name.clone(),
                status: CrossBookBookStatus::Included,
            });
            continue;
        }
        let (status, reading) = read_other_book(book, active_schema_version);
        rows.push(CrossBookBookRow {
            id: book.id.clone(),
            name: book.name.clone(),
            status,
        });
        if let Some(reading) = reading {
            readings.push(reading);
        }
    }
    (rows, readings)
}

fn read_other_book(
    book: &Book,
    active_schema_version: i64,
) -> (CrossBookBookStatus, Option<BookInvestmentReading>) {
    let db_path = book.dir.join(DB_FILE_NAME);
    let kind = match probe_file_kind(&db_path) {
        Ok(kind) => kind,
        Err(_) => return (CrossBookBookStatus::Unreadable, None),
    };
    match kind {
        DbFileKind::Empty => (CrossBookBookStatus::NotInitialized, None),
        DbFileKind::Encrypted => (CrossBookBookStatus::Locked, None),
        DbFileKind::Plaintext => {
            let conn = match open_connection_readonly_in(&book.dir) {
                Ok(conn) => conn,
                Err(_) => return (CrossBookBookStatus::Unreadable, None),
            };
            let version = match schema_version(&conn) {
                Ok(v) => v,
                Err(_) => return (CrossBookBookStatus::Unreadable, None),
            };
            if version != active_schema_version {
                return (CrossBookBookStatus::SchemaMismatch, None);
            }
            match read_book_investment(&conn) {
                Ok(reading) => (CrossBookBookStatus::Included, Some(reading)),
                Err(_) => (CrossBookBookStatus::Unreadable, None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot_probe::{self, InjectionOutcome};
    use crate::test_support::{ScratchDir, open_file, seed_account, seed_exchange_rate};

    #[test]
    fn active_book_merge_shares_snapshot_between_readings_and_fx() {
        let dir = ScratchDir::new("cross-book-fx-read-snapshot");
        let conn = open_file(dir.path());
        seed_account(&conn, "acc-cny", "投资账户", "investment", "CNY", 200_000);
        ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
        seed_exchange_rate(&conn, "USD", "CNY", 7.0);
        let other_book = BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![investment::CurrencyHoldingTotals {
                currency_code: "USD".into(),
                market_value_cents: Some(1_000_000),
                unrealized_pnl_cents: Some(200_000),
            }],
            cumulative_pnl: vec![],
            investable_assets: Some(("USD".into(), 500_000)),
        };

        let before = read_active_book_and_merge(vec![other_book], vec![], &conn).unwrap();

        snapshot_probe::arm(
            &conn,
            dir.path(),
            "FROM exchange_rates",
            &[
                "UPDATE exchange_rates SET rate = 14.0 WHERE base_code = 'USD' AND quote_code = 'CNY'",
            ],
        );
        let other_book = BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![investment::CurrencyHoldingTotals {
                currency_code: "USD".into(),
                market_value_cents: Some(1_000_000),
                unrealized_pnl_cents: Some(200_000),
            }],
            cumulative_pnl: vec![],
            investable_assets: Some(("USD".into(), 500_000)),
        };
        let after = read_active_book_and_merge(vec![other_book], vec![], &conn).unwrap();

        let outcome = snapshot_probe::outcome();
        assert!(
            outcome != InjectionOutcome::NotFired,
            "探针未命中 merge 的汇率取数（marker 漂移或未臂装），断言失去意义：{outcome:?}"
        );
        assert!(before.converted, "他本 USD 组应触发折算口径标注");
        assert_eq!(
            before.market_value_cents, after.market_value_cents,
            "合并持仓市值必须与基线同时点——活动本读数与汇率折算同快照"
        );
        assert_eq!(
            before.investable_assets_cents, after.investable_assets_cents,
            "可投资资产必须与基线同时点"
        );
        assert_eq!(
            before.target_currency, after.target_currency,
            "目标币种不应漂移"
        );
    }

    #[test]
    fn read_book_investment_is_one_snapshot_under_concurrent_write() {
        let dir = ScratchDir::new("cross-book-read-snapshot");
        let conn = open_file(dir.path());
        seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 200_000);
        ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
        seed_exchange_rate(&conn, "CNY", "CNY", 1.0);

        let before = read_book_investment(&conn).unwrap();

        snapshot_probe::arm(
            &conn,
            dir.path(),
            "is_hidden=0",
            &[
                "UPDATE account_balance_cache SET balance_cents = balance_cents + 5555 WHERE account_id = 'acc-inv'",
            ],
        );
        let after = read_book_investment(&conn).unwrap();

        let outcome = snapshot_probe::outcome();
        assert!(
            outcome != InjectionOutcome::NotFired,
            "探针未命中现金腿（marker 漂移或未臂装），断言失去意义：{outcome:?}"
        );
        assert!(
            before.investable_assets.is_some(),
            "种子应产出可投资资产读数（否则对拍空转）"
        );
        assert_eq!(
            before.investable_assets, after.investable_assets,
            "可投资资产必须与基线同时点——注入写要么整体进快照、要么整体不进"
        );
        assert_eq!(
            before.native_currency, after.native_currency,
            "本位币不应漂移"
        );
    }

    #[test]
    fn same_currency_adds_directly_without_conversion() {
        let readings = vec![
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![totals("CNY", Some(120_000), Some(20_000))],
                cumulative_pnl: vec![cumulative("CNY", 45_000)],
                investable_assets: Some(("CNY".into(), 170_000)),
            },
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![totals("CNY", Some(1_000), Some(-500))],
                cumulative_pnl: vec![cumulative("CNY", 500)],
                investable_assets: Some(("CNY".into(), 1_500)),
            },
        ];
        let merged = merge_readings(&readings, "CNY", &mut |_cents, cur| {
            panic!("同币种不应触发折算，实际折算 {cur}")
        })
        .unwrap();
        assert_eq!(
            merged,
            MergedTotals {
                market_value_cents: 121_000,
                unrealized_pnl_cents: 19_500,
                cumulative_pnl_cents: 45_500,
                investable_assets_cents: 171_500,
                converted: false,
            }
        );
    }

    #[test]
    fn mixed_currencies_convert_and_flag() {
        let readings = vec![
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![
                    totals("CNY", Some(100_000), Some(10_000)),
                    totals("USD", None, Some(2_000)),
                ],
                cumulative_pnl: vec![cumulative("USD", 3_000)],
                investable_assets: Some(("CNY".into(), 110_000)),
            },
            BookInvestmentReading {
                native_currency: "USD".into(),
                holding_totals: vec![totals("USD", Some(12_000), Some(2_000))],
                cumulative_pnl: vec![cumulative("HKD", 700)],
                investable_assets: Some(("USD".into(), 14_000)),
            },
        ];
        let merged = merge_readings(&readings, "CNY", &mut |cents, _cur| Ok(cents * 7)).unwrap();
        assert!(merged.converted);
        assert_eq!(merged.market_value_cents, 100_000 + 12_000 * 7);
        assert_eq!(merged.unrealized_pnl_cents, 10_000 + (2_000 + 2_000) * 7);
        assert_eq!(merged.cumulative_pnl_cents, 3_000 * 7 + 700 * 7);
        assert_eq!(merged.investable_assets_cents, 110_000 + 14_000 * 7);
    }

    #[test]
    fn empty_readings_yield_zero_totals() {
        let merged = merge_readings(&[], "CNY", &mut |cents, _cur| Ok(cents * 7)).unwrap();
        assert_eq!(
            merged,
            MergedTotals {
                market_value_cents: 0,
                unrealized_pnl_cents: 0,
                cumulative_pnl_cents: 0,
                investable_assets_cents: 0,
                converted: false,
            }
        );
    }

    #[test]
    fn conversion_error_propagates() {
        let readings = vec![BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![totals("USD", Some(12_000), None)],
            cumulative_pnl: vec![],
            investable_assets: None,
        }];
        let err = merge_readings(&readings, "CNY", &mut |_cents, _cur| {
            Err(ledger_infra::error::AppError::Invalid("boom".into()))
        })
        .unwrap_err();
        assert!(matches!(err, ledger_infra::error::AppError::Invalid(_)));
    }

    #[test]
    fn base_currency_divergence_flags_converted_even_without_amount_conversion() {
        let readings = vec![BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![totals("CNY", Some(100_000), Some(10_000))],
            cumulative_pnl: vec![],
            investable_assets: Some(("CNY".into(), 110_000)),
        }];
        let merged = merge_readings(&readings, "CNY", &mut |_cents, _cur| {
            panic!("金额全部同币种，不应触发折算闭包")
        })
        .unwrap();
        assert!(merged.converted, "位币不一致应置折算口径标注");
        assert_eq!(merged.investable_assets_cents, 110_000);
    }

    fn totals(
        currency: &str,
        market: Option<i64>,
        unrealized: Option<i64>,
    ) -> investment::CurrencyHoldingTotals {
        investment::CurrencyHoldingTotals {
            currency_code: currency.into(),
            market_value_cents: market,
            unrealized_pnl_cents: unrealized,
        }
    }

    fn cumulative(currency: &str, cents: i64) -> investment::CurrencyCumulativePnl {
        investment::CurrencyCumulativePnl {
            currency_code: currency.into(),
            cumulative_pnl_cents: cents,
        }
    }
}
