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
use ledger_infra::db::tx_scope::ensure_transaction;
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
///
/// **读快照一致性（issue #1699）**：四段读数（持仓合计 / 累计收益 / 可投资资产
/// 两腿 / 本位币）整体收进同一读事务（嵌套感知）——写提交落在语句之间会让
/// 同屏合计与可投资资产不同时点。覆盖活动本与非活动本两条调用路径。
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

/// 活动本读数与合并的单点（ADR-0114 决策 3 的第④步，壳层编排）：活动本读数、
/// 目标本位币、当期汇率折算与逐本合并一次给出。
///
/// **读快照一致性（issue #1699，证据点「活动本读数与汇率折算跨口径」）**：整段
/// 收进同一读事务（嵌套感知）——[`read_book_investment`] 的四段读数、目标本位币
/// 与 merge 的逐笔当期汇率取数同快照，写提交落在其间不会「读数旧、汇率新」。
/// 非活动本读数各自已收本内快照（`read_book_investment` 内接线）；跨库合天然
/// 逐本时点（单事务跨不了库文件，结构边界如实保留，不假装跨本一致）。
///
/// 从命令闭包抽出的动机：接线要能在**命令线程之外**被探针测试直接驱动
/// （`trace_v2` 回调与被测读闭包同线程才可见臂装状态），命令壳只留一行调用。
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
    use crate::test_support::snapshot_probe::{self, InjectionOutcome};
    use crate::test_support::{ScratchDir, open_file, seed_account, seed_exchange_rate};

    /// 读快照一致性·证据点「活动本读数与汇率折算跨口径」（issue #1699）：
    /// `read_active_book_and_merge` 的活动本读数、目标本位币与 merge 的逐笔当期
    /// 汇率取数必须同快照。
    ///
    /// 探针选点：活动本纯 CNY（内层 `read_book_investment` 零汇率查询），他本读数
    /// 由参数合入（USD 组只在 merge 折算）——首个 `FROM exchange_rates` 即 merge 的
    /// 折算取数，臂装契约①（不得命中首条语句）与「首中即外层窗口」同时成立：
    /// - 读闭包无快照保护（红）：注入的汇率改写提交，merge 按新汇率折算，合并结果
    ///   与基线漂移（同屏里活动本读数与折算汇率不同时点）；
    /// - 读闭包收进读事务（绿）：注入写被挡住，合并结果与基线一致。
    #[test]
    fn active_book_merge_shares_snapshot_between_readings_and_fx() {
        let dir = ScratchDir::new("cross-book-fx-read-snapshot");
        let conn = open_file(dir.path());
        // 活动本：纯 CNY（默认币即 CNY，现金腿同币短路，内层不读汇率表）。
        seed_account(&conn, "acc-cny", "投资账户", "investment", "CNY", 200_000);
        ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
        // 当期汇率：供 merge 把他本 USD 组折到目标币种 CNY。
        seed_exchange_rate(&conn, "USD", "CNY", 7.0);
        // 他本读数（参数面）：USD 持仓组 + 可投资资产，只经 merge 折算。
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

    /// 读快照一致性（issue #1699）：`read_book_investment` 的四段读数必须同快照。
    /// 探针在现金腿（账户列表，全闭包首个 `is_hidden=0` 命中）开始前于另一连接
    /// 提交余额缓存写——两次读数之间库内唯一变动就是这笔注入写，基线对拍即判据：
    /// - 读闭包无快照保护（红）：持仓/累计读旧、可投资资产读新，对拍变红；
    /// - 读闭包收进读事务（绿）：注入写被挡住，两次读数逐字段相等。
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
