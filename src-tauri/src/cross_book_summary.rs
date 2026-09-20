//! 跨账本投资汇总（CrossBookInvestmentSummary，ADR-0114 / issue #1196）：壳层编排。
//!
//! 跨本聚合不属投资域（域是单本语义，且域不得依赖壳/引导层，ADR-0112）；按
//! ADR-0114 决策与实施前核查的接缝归属，编排落在本模块：壳层枚举注册表 →
//! 逐本只读建连（活动本复用进程内读连接）→ 调投资域 `&Connection` 读函数 →
//! 内存合并 → 按主账本（＝当前活动账本，ADR-0114 决策 3）本位币折算与标注。
//!
//! 逐本状态闭集（ADR-0114 决策 5，2026-09-14 修订）：活动本恒计入；非活动本
//! 按文件形态分派——密文库（进程内无逐本解锁能力，ADR-0117 决策 2 口令协议
//! 禁止解锁窗外读钥匙串）标注「未解锁、未计入」；只登记未建库标注「尚未初始
//! 化」；明文库先比 `user_version` 与活动本是否一致（活动本建连必经迁移，其版
//! 本即当前应用 schema 版本），不一致标注「版本不一致」，一致才经投资域读函数
//! 取数；打开或读取失败标注「读取失败」。四类排除逐本可见，不静默少算、不阻
//! 塞其他账本出数。
//!
//! 折算口径（ADR-0114 决策 3）：全部金额带币种进合并器，币种＝目标币种（活动
//! 本本位币）直接相加，否则经活动本当期汇率表折算（当期入口
//! `convert_to_native_current`，缺汇
//! 率码化错误上抛——与既有本内折算同一「不静默返回缺料数字」口径）；发生过任
//! 意折算即置 `converted`，界面据此标注折算口径。
//!
//! 只读承诺：本模块不产生任何写路径、不持有活动本连接以外的长生命周期连接，
//! 逐本连接在取数后即弃（作用域结束 drop）。

use rusqlite::Connection;
use serde::Serialize;

use ledger_infra::db::book_registry::Book;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::db::encryption::probe_file_kind;
use ledger_infra::db::{open_connection_readonly_in, schema_version};
use ledger_infra::error::Result;
use ledger_investment as investment;
use ledger_transaction::amount;

/// 逐本状态闭集（serde 字面量与前端 i18n 键一一对应）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossBookBookStatus {
    /// 计入。
    Included,
    /// 加密库且进程内未解锁（非活动密文本恒此态）。
    Locked,
    /// 只登记未建库（库文件缺失或空文件）。
    NotInitialized,
    /// 明文库 schema 版本与活动本不一致（旧版待升级或库来自更新版本）。
    SchemaMismatch,
    /// 只读打开或读取失败（文件损坏等）。
    Unreadable,
}

/// 逐本行：注册表序呈现，含活动本。
#[derive(Debug, Clone, Serialize)]
pub struct CrossBookBookRow {
    pub id: String,
    pub name: String,
    pub status: CrossBookBookStatus,
}

/// 汇总载荷（折算全部在后端完成，前端不出现第二份口径——仪表盘同款纪律）。
#[derive(Debug, Serialize)]
pub struct CrossBookInvestmentSummary {
    /// 折算目标＝主账本（当前活动账本）本位币。
    pub target_currency: String,
    /// 是否发生过当期汇率折算（界面据此标注口径）。
    pub converted: bool,
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    /// 逐本状态行（注册表序，含活动本）。
    pub books: Vec<CrossBookBookRow>,
}

/// 单本投资口径原始读数（金额带币种，折算前）。
#[derive(Debug, Clone, Default)]
pub struct BookInvestmentReading {
    /// 本内默认币种（本位币基准）：逐本位币与目标币是否一致的判定源。
    pub native_currency: String,
    /// 持仓市值/持仓收益按币种分组（账户币口径）。
    pub holding_totals: Vec<investment::CurrencyHoldingTotals>,
    /// 累计收益按币种分组（域读投影原样）。
    pub cumulative_pnl: Vec<investment::CurrencyCumulativePnl>,
    /// 可投资资产：域分子函数折本内默认币种后的 (币种, 分)。
    pub investable_assets: Option<(String, i64)>,
}

/// 单本读数（投资域 `&Connection` 纯读组合）：持仓合计 + 累计收益 + 可投资资产。
/// 任一读失败原样上抛（调用方决定逐本降级还是整体失败）。
pub fn read_book_investment(conn: &Connection) -> Result<BookInvestmentReading> {
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
}

/// 合并结果（折算后，目标币种口径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTotals {
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    pub converted: bool,
}

/// 内存合并器（纯函数，折算经注入闭包）：币种＝目标币种直接相加，否则折算并置
/// `converted`；任一计入本的本位币≠目标币同样置 `converted`——位币不一致时
/// 「按当期汇率折算」是口径事实（ADR-0114 决策 3），与本次实际是否发生折算无关。
/// 空值组按域空值语义跳过、不以零计入。
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

/// 逐本探测与读取（非活动本；阻塞文件 IO，调用方负责投放阻塞线程池）。
/// 返回注册表序的全量状态行（活动本恒 `Included`）与非活动本的可计入读数。
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

/// 单本（非活动）分派：探测文件形态 → 密文=未解锁、空=未初始化、明文=比版本后
/// 读取；打开/读取失败逐本降级为 `Unreadable`，不上抛、不阻塞其他本。
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
        // 进程内没有逐本解锁路径（ADR-0114 决策 5 / ADR-0117 决策 2）。
        DbFileKind::Encrypted => (CrossBookBookStatus::Locked, None),
        DbFileKind::Plaintext => {
            // 只读打开、不迁移（迁移是写；旧版本等先切换进入再升级）。
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

    /// 同币种直加：目标币种金额直接求和，`converted` 恒 false。
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

    /// 异币种折算：经注入闭包折算到目标币种并置 `converted`；空值组跳过。
    #[test]
    fn mixed_currencies_convert_and_flag() {
        let readings = vec![
            BookInvestmentReading {
                native_currency: "CNY".into(),
                // CNY 本：直加；NULL 市值组跳过未实现列。
                holding_totals: vec![
                    totals("CNY", Some(100_000), Some(10_000)),
                    totals("USD", None, Some(2_000)),
                ],
                cumulative_pnl: vec![cumulative("USD", 3_000)],
                investable_assets: Some(("CNY".into(), 110_000)),
            },
            BookInvestmentReading {
                native_currency: "USD".into(),
                // USD 本：全部折算（×7）。
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

    /// 空读数（全部排除的现场）：全零合计、未折算，不报错。
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

    /// 折算闭包的错误原样上抛（缺汇率现场由命令层经当期入口
    /// `convert_to_native_current` 命中
    /// `fx.rate-missing`），合并不吞错。
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

    /// 位币不一致本身就是折算口径事实：计入本的本位币≠目标币时，即使本次金额
    /// 全部同币种、零折算发生，也置 `converted`（ADR-0114 决策 3 标注义务）。
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
