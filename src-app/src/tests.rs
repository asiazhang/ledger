//! 壳行为单测（issue #2017 骨架 / issue #2018 首屏，ADR-0087 断言归属：引导与
//! 读接线归本壳）：引导序列（处置四态收窄 + 成对建连）、交易页读（读路径直调
//! 域服务）与装配层对 DB 造数行集的分页边界装配。
//!
//! 造数纪律：**不经根包 test_support 统一工厂**——立桩时工厂链接旧壳 tauri
//!（GTK3，与 NSH 的 GTK4 同进程符号互踩，src-app/Cargo.toml 头注；spec #2015
//! AC 原文的「统一测试数据库工厂造数」当时在本壳不可达）；#2083 切轨后
//! tauri 清零、互踩消失，不经工厂走产品开库入口的分层纪律保留。改用同纪律
//! 的分层形态：
//! 建库走**产品开库入口**（[`nsh_app::boot::boot_ledger`]，e2e world 同款分层
//! 形态，ADR-0056 决策 5 例外口径）；造数经域层公开写入口（账户 + 交易写入
//! 协议，CONTEXT-testing「公开写入口（测试侧）」纪律），余额刷新接缝已随域栈
//! 引导接线（[`crate::wiring`]）。账本目录落 `std::env::temp_dir` 下的唯一
//! 暂存目录（infra 身份工厂发 UUID，测试互不串库），进程退出由 OS 回收
//! （骨架测试无 ScratchDir 需求——不建库则无文件，与 BDD 世界持久目录同容忍）。
//!
//! 造数形态（暂存目录、日期步进、账户与最小支出输入、支出系列入口）与
//! headless 组件测试共享同一份 `tests/support/seed.rs`（两侧 `#[path]` 载入，
//! issue #2027；本面薄壳只负责以 [`crate::boot::boot_ledger`] 开库）。

use chrono::Datelike;
use ledger_accounts::{AccountInput, AccountType};
use ledger_currencies::Currency;
use ledger_transaction::{TransactionInput, TransactionKind};
use std::fs;
use std::path::Path;

use crate::app::{StartupView, headline_text, run_startup, set_book_dir};
use crate::boot::{
    CODE_AWAITING_UNLOCK, CODE_BOOK_MISSING, PickedBookDisposition, account_rows, boot_ledger,
    category_rows, confirm_create_picked_book, currency_rows, merchant_rows,
    resolve_picked_book_dir, transaction_page, transaction_search,
};

/// 造数助手自共享 `tests/support/seed.rs`（issue #2027）载入；
/// `scratch_book_dir` / `expense_input` 以 pub(crate) 再导出，供 nav 等模块
/// 测试复用（原 pub(crate) 本地定义收拢后的接缝不变）。
#[path = "../tests/support/seed.rs"]
mod seed;

/// 测试 logger 助手（issue #2109）同自共享 `tests/support/` 载入：headless
/// 测试进程缺省无 logger、框架错误出口 `log::error!` 静默丢弃（坑、用法与
/// 让位语义见模块文档；本面无进程级消费点，进程级接线自测住集成面）。
#[path = "../tests/support/test_logger.rs"]
mod test_logger;

pub(crate) use seed::{
    create_empty_book_file, expense_input, make_book_dir as scratch_book_dir, seed_cash_account,
    test_date,
};

/// 串行锁：接缝注册与 OnceLock 装配入口是进程级单例，用例间串行防串扰
/// （crate 级共享锁 [`crate::app::test_assembly_guard`]，与 nav::tests /
/// dashboard::tests 探针互斥——装配单例跨模块唯一）。
fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::app::test_assembly_guard()
}

/// 经域层公开写入口在同一账户上造一列支出（余额刷新接缝的行文证据；插入序
/// = 日期升序，供分页排序断言）。目录级薄壳：开库走本面产品入口，造数形态
/// 归共享助手（[`seed`]，issue #2027）。
fn seed_expense_series(dir: &Path, count: usize) {
    let db = boot_ledger(dir).unwrap();
    seed::seed_expense_series(&db, count);
}

/// 经域层公开写入口造一列带指定中文备注的支出（同一账户「测试现金」，日期
/// 递增；搜索读接线断言的原文命中造数面）。
fn seed_expense_notes(dir: &Path, notes: &[&str]) {
    let db = boot_ledger(dir).unwrap();
    seed::seed_expense_notes(&db, notes);
}

/// 一笔收入输入（维度筛选读路径的造数入口：无分类无商户）。
fn income_input(account: &str, date: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Income,
        amount_cents,
        currency_code: "CNY".to_owned(),
        account_id: Some(account.to_owned()),
        note: Some(note.to_owned()),
        date: date.to_owned(),
        ..expense_input(account, date, amount_cents, note)
    }
}

/// 一笔转账输入（转出 → 转入双腿，维度筛选读路径的造数入口）。
fn transfer_input(
    from: &str,
    to: &str,
    date: &str,
    amount_cents: i64,
    note: &str,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents,
        currency_code: "CNY".to_owned(),
        account_id: Some(from.to_owned()),
        to_account_id: Some(to.to_owned()),
        note: Some(note.to_owned()),
        date: date.to_owned(),
        ..expense_input(from, date, amount_cents, note)
    }
}

/// 一笔退款输入（指向原支出行，维度筛选读路径的造数入口）。
fn refund_input(
    account: &str,
    date: &str,
    amount_cents: i64,
    refund_of: &str,
    note: &str,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Refund,
        refund_of_transaction_id: Some(refund_of.to_owned()),
        ..expense_input(account, date, amount_cents, note)
    }
}

/// 一笔买入输入（投资 kind，默认载荷排除断言的造数入口；非 DB 夹具语义与
/// 域测试 `make_buy_input` 同形）。
fn buy_input(account: &str, instrument_id: &str, date: &str) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".to_owned(),
        account_id: Some(account.to_owned()),
        instrument_id: Some(instrument_id.to_owned()),
        quantity: Some(1.0),
        price_cents: Some(10_000),
        fee_cents: Some(0),
        date: date.to_owned(),
        ..expense_input(account, date, 0, "买入")
    }
}

/// 在指定日期造一笔指定金额的现金支出（期间维度读路径的造数入口）。
fn seed_expense_on_date(dir: &Path, date: &str, amount_cents: i64, note: &str) {
    let db = boot_ledger(dir).unwrap();
    seed::seed_expense_on_date(&db, date, amount_cents, note);
}

/// 首页读断言（空库与造数库共用）：total 与当前页行集。
fn assert_first_page(dir: &Path, expected_total: i64, expected_rows: usize) {
    let db = boot_ledger(dir).unwrap();
    let page = transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(
        page.total, expected_total,
        "total = 满足过滤条件的未删除总数"
    );
    assert_eq!(
        page.items.len(),
        expected_rows,
        "首页行数 = min(total, 每页行数)"
    );
}

#[test]
fn boot_opens_empty_book_and_reads_first_page_empty() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("empty");
    create_empty_book_file(&dir);
    assert_first_page(&dir, 0, 0);
}

#[test]
fn boot_reads_seeded_transactions_via_domain_write_entries() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("seeded");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 3);
    let db = boot_ledger(&dir).unwrap();
    let page = transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(
        page.total, 3,
        "total = 公开写入口写入的未删除笔数（读路径直调域服务）"
    );
    assert_eq!(page.items.len(), 3);
    assert_eq!(
        page.items[0].note.as_deref(),
        Some("造数第 2 笔"),
        "排序 date DESC：最新在前"
    );
}

#[test]
fn page_reads_offset_paging_boundaries_with_deterministic_order() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("paged");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 250);
    let db = boot_ledger(&dir).unwrap();

    let page1 = transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(page1.total, 250, "total 与页无关：满足过滤条件的总数");
    assert_eq!(page1.items.len(), 100, "首页整 100 行");
    assert_eq!(
        page1.items[0].date,
        test_date(249),
        "排序 date DESC，首页首行最新"
    );
    assert_eq!(
        page1.items[99].date,
        test_date(150),
        "首页末行 = 第 151 笔写入序"
    );

    let page2 = transaction_page(&db, 2, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(page2.items.len(), 100);
    assert_eq!(
        page2.items[0].date,
        test_date(149),
        "offset = (页码-1) × 每页行数"
    );
    assert_eq!(page2.items[99].date, test_date(50));

    let last = transaction_page(&db, 3, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(last.items.len(), 50, "末页 50 行（250 = 100 + 100 + 50）");
    assert_eq!(last.items[0].date, test_date(49));
    assert_eq!(last.items[49].date, test_date(0), "末页末行最早");

    let beyond = transaction_page(&db, 4, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(beyond.items.len(), 0, "越界页空行集");
    assert_eq!(beyond.total, 250, "越界页 total 仍返回");
}

/// 搜索读接线（issue #2023）：读路径直调核心交易域统一模糊搜索服务——中文
/// 备注原文连续子串命中，与旧壳 `search_transactions` 命令同一域函数（同一
/// 账本库同结果）。删除搜索读路径任一环（域函数调用 / 只读连接选择）即红。
#[test]
fn search_reads_hit_transactions_with_raw_text_semantics() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("search");
    create_empty_book_file(&dir);
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);
    let db = boot_ledger(&dir).unwrap();

    let result = transaction_search(
        &db,
        1,
        &crate::boot::TxSearchQuery {
            query: "火锅".to_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.total, 1, "命中总数 = 备注原文命中的未删除笔数");
    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result.items[0].note.as_deref(),
        Some("元旦火锅聚餐"),
        "原文连续子串命中（非前缀、非全词）"
    );

    // 空查询走域空结果语义（视图层空输入 = 列表态不调搜索——呈现切换归
    // headless 断言面，本用例只钉域函数自身的既有契约）。
    let empty = transaction_search(&db, 1, &crate::boot::TxSearchQuery::default()).unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.items.is_empty());
}

/// 期间限定读路径（issue #2021）：日期区间过滤进 TransactionListFilter 的
/// from/to（含边界字典序），total 与行集同 WHERE；自然周期区间换算归
/// period 纯函数（本测锚定读路径的过滤半边）。
#[test]
fn page_reads_filter_by_period_range_with_inclusive_bounds() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("period-filter");
    create_empty_book_file(&dir);
    seed_expense_on_date(&dir, "2026-01-31", 1_100, "一月末");
    seed_expense_on_date(&dir, "2026-03-01", 2_200, "三月初");
    seed_expense_on_date(&dir, "2026-03-31", 3_300, "三月末");
    seed_expense_on_date(&dir, "2026-04-01", 4_400, "四月初");
    let db = boot_ledger(&dir).unwrap();

    // 2026 年 3 月自然月（含边界）：只命中三月两笔，边界日期在内。
    let march = crate::boot::transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            range: Some(crate::period::PeriodRange {
                from: "2026-03-01".to_owned(),
                to: "2026-03-31".to_owned(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(march.total, 2, "total 与行集共用同一 WHERE 子句");
    assert_eq!(
        march
            .items
            .iter()
            .map(|t| t.note.as_deref())
            .collect::<Vec<_>>(),
        [Some("三月末"), Some("三月初")],
        "期间内按 date DESC 排序"
    );

    // 「全部」（None）：不过滤。
    let all = crate::boot::transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(all.total, 4);
}

/// 维度筛选读路径（issue #2037）：涉及账户/商户/分类（含「仅无分类」三态）/
/// 类型集合逐维过滤进 TransactionListFilter（维度间 AND、维度内取或），默认
/// 载荷显式携带四通用 kind（投资 kind 的排除是本读接缝单点，ADR-0135）。
/// 删除任一维度投影（含 kinds 默认）即红。
#[test]
fn page_reads_filter_dimensions_and_defaults_to_main_kinds() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("dim-filter");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let (cash, bank, invest, merchant, category) = {
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let cash = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "现金".to_owned(),
                kind: AccountType::Cash,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let bank = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "银行卡".to_owned(),
                kind: AccountType::Bank,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        // 投资账户（买入的账户类型守卫要求投资账户：trade.buy-account-not-investment）。
        let invest = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "证券户".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let merchant = ledger_merchants::create_merchant(
            &conn,
            ledger_merchants::MerchantInput {
                name: "链商超".to_owned(),
            },
        )
        .unwrap();
        let category = ledger_categories::create_category(
            &conn,
            ledger_categories::CategoryInput {
                name: "餐饮".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
        let expense = ledger_transaction::create(&conn, {
            let mut input = expense_input(&cash, "2026-01-05", 1_100, "超市买菜");
            input.merchant_id = Some(merchant.clone());
            input.category_id = Some(category.clone());
            input
        })
        .unwrap();
        ledger_transaction::create(&conn, income_input(&bank, "2026-01-06", 9_000, "工资"))
            .unwrap();
        ledger_transaction::create(
            &conn,
            transfer_input(&cash, &bank, "2026-01-07", 2_000, "转存"),
        )
        .unwrap();
        ledger_transaction::create(
            &conn,
            refund_input(&cash, "2026-01-08", 500, &expense.id, "部分退款"),
        )
        .unwrap();
        // 投资 kind（买入）：不在主列表口径，默认载荷必须排除（ADR-0135）。
        let instrument = ledger_investment::create_instrument(
            &conn,
            ledger_investment::InstrumentInput {
                symbol: "TEST".to_owned(),
                kind: ledger_investment::InstrumentType::Stock,
                name: Some("测试标的".to_owned()),
                currency_code: "USD".to_owned(),
                market: None,
            },
        )
        .unwrap();
        ledger_transaction::create(&conn, buy_input(&invest, &instrument, "2026-01-09")).unwrap();
        (cash, bank, invest, merchant, category)
    };

    // 默认载荷：四通用 kind（买入排除）→ 4 笔。
    let default = transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(default.total, 4, "投资 kind 不进主列表（默认载荷显式排除）");

    // 类型子集：维度内取或。
    let expense_only = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            kinds: vec![TransactionKind::Expense],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(expense_only.total, 1);

    // 涉及账户：转出 ∪ 转入 ∪ 出资三端命中；与默认 kind 排除 AND（银行卡
    // 上的买入不因账户命中而进主列表）。
    let involving_cash = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            involving_account_id: Some(cash.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(involving_cash.total, 3, "现金：支出 + 转账转出 + 退款");
    let involving_bank = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            involving_account_id: Some(bank.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        involving_bank.total, 2,
        "银行卡：收入 + 转账转入（买入被默认 kind 排除）"
    );

    // 显式类型集合与账户维度组合（AND）：银行卡上的买入可见。
    let invest_buys = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            involving_account_id: Some(invest),
            kinds: vec![TransactionKind::Buy],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(invest_buys.total, 1, "显式携带投资 kind 时买入可见");

    // 商户维度。
    let merchant_rows = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            merchant_id: Some(merchant),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        merchant_rows.total, 2,
        "支出 + 退款（退款继承原商户，域语义）"
    );

    // 分类维度三态。
    let categorized = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            category_id: Some(category),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        categorized.total, 2,
        "支出 + 退款（退款继承原分类，域语义）"
    );
    let uncategorized = transaction_page(
        &db,
        1,
        &crate::boot::TxListQuery {
            uncategorized_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        uncategorized.total, 2,
        "收入 + 转账无分类（退款继承原分类不算无分类）"
    );
}

/// 搜索读路径的金额/期间筛选（issue #2037 搜索 × 期间裁决的读接缝半边）：
/// 关键词与金额区间/期间区间 AND 组合（域统一模糊搜索的可选筛选直传）；
/// 空关键词 + 仅有筛选 = 仅筛选查询（旧壳搜索页「无关键字仅筛选」同语义）。
#[test]
fn search_reads_with_amount_and_period_filters() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("search-filter");
    create_empty_book_file(&dir);
    seed_expense_on_date(&dir, "2026-01-05", 1_100, "一月火锅");
    seed_expense_on_date(&dir, "2026-02-05", 8_800, "二月火锅");
    seed_expense_on_date(&dir, "2026-03-05", 4_400, "三月火锅");
    let db = boot_ledger(&dir).unwrap();

    // 关键词 × 金额区间（含边界）。
    let ranged = transaction_search(
        &db,
        1,
        &crate::boot::TxSearchQuery {
            query: "火锅".to_owned(),
            amount_min_cents: Some(4_400),
            amount_max_cents: Some(8_800),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        ranged.total, 2,
        "金额下下含边界：8800 与 4400 命中，1100 排除"
    );

    // 关键词 × 期间：裁决后期间对搜索命中面生效（旧壳搜索页同口径）。
    let february = transaction_search(
        &db,
        1,
        &crate::boot::TxSearchQuery {
            query: "火锅".to_owned(),
            range: Some(crate::period::PeriodRange {
                from: "2026-02-01".to_owned(),
                to: "2026-02-28".to_owned(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(february.total, 1, "期间快照过滤搜索命中面");

    // 空关键词 + 仅金额筛选 = 仅筛选查询（全部支出里按金额过滤）。
    let filter_only = transaction_search(
        &db,
        1,
        &crate::boot::TxSearchQuery {
            amount_min_cents: Some(5_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(filter_only.total, 1);
    assert_eq!(filter_only.items[0].note.as_deref(), Some("二月火锅"));
}

/// 数据期间边界读路径（issue #2021 / ADR-0141 钳制输入）：报表域日期极值，
/// 空库双 None；与旧壳 report_date_range 同一域函数。
#[test]
fn report_date_range_spans_seeded_data() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("date-range");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let empty = crate::boot::report_date_range(&db).unwrap();
    assert_eq!(
        (empty.min_date.as_deref(), empty.max_date.as_deref()),
        (None, None),
        "空库双 None"
    );
    drop(db);
    seed_expense_on_date(&dir, "2025-08-15", 5_500, "历史");
    seed_expense_on_date(&dir, "2026-02-20", 6_600, "最新");
    let db = boot_ledger(&dir).unwrap();
    let range = crate::boot::report_date_range(&db).unwrap();
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (Some("2025-08-15"), Some("2026-02-20")),
        "起点为最早流水日期，终点为最新流水日期"
    );
}

#[test]
fn currency_dict_reads_seeded_reference_data() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("currencies");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let rows: Vec<Currency> = currency_rows(&db).unwrap();
    let cny = rows
        .iter()
        .find(|c| c.code == "CNY")
        .expect("种子币种字典含 CNY");
    assert_eq!(cny.symbol, "¥");
    assert_eq!(cny.decimal_places, 2);
}

/// 装配层 × DB 造数行集 × 分页边界（issue #2018 AC2 的合一面）：250 笔造数的
/// 首末页行集经 [`crate::assembly::assemble_rows`] 装配成格文本，首/末页首末
/// 行的类型标签、金额（种子币种 CNY 符号）与日期逐列可读（删除装配任一环或
/// 读路径分页任一环即红）。列定义自身（无 DB 依赖）归 assembly 单测，不在此
/// 重复。
#[test]
fn page_reads_assemble_into_cell_texts_across_page_boundaries() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("assembled");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 250);
    let db = boot_ledger(&dir).unwrap();
    let currencies = crate::assembly::CurrencyTable::from_currencies(&currency_rows(&db).unwrap());
    let categories = crate::assembly::CategoryTable::default();
    let ctx = crate::assembly::RowContext {
        currencies: &currencies,
        categories: &categories,
        accounts: &crate::assembly::AccountTable::default(),
        merchants: &crate::assembly::MerchantTable::default(),
        privacy_enabled: false,
    };

    let first = crate::assembly::assemble_rows(
        &transaction_page(&db, 1, &crate::boot::TxListQuery::default())
            .unwrap()
            .items,
        &ctx,
    );
    assert_eq!(first.len(), crate::assembly::PAGE_SIZE);
    assert_eq!(first[0].date, test_date(249));
    assert_eq!(first[0].kind, "支出", "类型标签首版 = kind 直译");
    assert_eq!(
        first[0].amount, "¥25",
        "金额基础格式化（种子 CNY 符号 + 去尾零）"
    );
    assert_eq!(first[99].date, test_date(150), "首页末行边界");

    let last = crate::assembly::assemble_rows(
        &transaction_page(&db, 3, &crate::boot::TxListQuery::default())
            .unwrap()
            .items,
        &ctx,
    );
    assert_eq!(last.len(), 50, "末页 50 行");
    assert_eq!(last[0].date, test_date(49));
    assert_eq!(last[49].date, test_date(0), "末页末行 = 最早一笔");
    assert_eq!(last[49].amount, "¥25");
}

/// 分类路径 × DB 造数行集（issue #2019 分类路径 AC 的接线面）：经域公开写入口
/// 建父子分类，支出行引用子分类，`category_rows` 读接缝（在用口径）+ 装配层
/// 路径表产出「父 > 子」格文本；软删分类不在投影行集，引用落 `-`。删除读
/// 接缝（`category_rows`）或装配消费任一环即红。
#[test]
fn category_rows_assemble_into_path_cells_active_only() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("categories");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let parent = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "餐饮".to_owned(),
            kind: "expense".to_owned(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let child = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "午餐".to_owned(),
            kind: "expense".to_owned(),
            parent_id: Some(parent.clone()),
            icon: None,
        },
    )
    .unwrap();
    let account = ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: "测试现金".to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    let make_expense =
        |date: &str, category_id: Option<String>, note: Option<String>| TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 2_500,
            currency_code: "CNY".to_owned(),
            account_id: Some(account.clone()),
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note,
            source_order_no: None,
            date: date.to_owned(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
        };
    // 三笔各占一天（排序 date DESC：行序 = 写入序倒序，断言与 created_at
    // 同秒无关）。
    ledger_transaction::create(
        &conn,
        make_expense("2026-01-15", Some(child.clone()), Some("买咖啡".to_owned())),
    )
    .unwrap();
    ledger_transaction::create(&conn, make_expense("2026-01-16", None, None)).unwrap();
    // 软删分类：不在投影行集，历史引用落 `-`。
    let gone = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "已删分类".to_owned(),
            kind: "expense".to_owned(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    ledger_categories::delete_category(&conn, &gone).unwrap();
    ledger_transaction::create(&conn, make_expense("2026-01-17", Some(gone.clone()), None))
        .unwrap();
    drop(conn);

    let currencies = crate::assembly::CurrencyTable::from_currencies(&currency_rows(&db).unwrap());
    let categories = crate::assembly::CategoryTable::from_categories(&category_rows(&db).unwrap());
    let ctx = crate::assembly::RowContext {
        currencies: &currencies,
        categories: &categories,
        accounts: &crate::assembly::AccountTable::from_accounts(&account_rows(&db).unwrap()),
        merchants: &crate::assembly::MerchantTable::from_merchants(&merchant_rows(&db).unwrap()),
        privacy_enabled: false,
    };
    let rows = crate::assembly::assemble_rows(
        &transaction_page(&db, 1, &crate::boot::TxListQuery::default())
            .unwrap()
            .items,
        &ctx,
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].category, "-",
        "软删分类的引用不在在用投影行集（最新一笔）"
    );
    assert_eq!(rows[1].category, "-", "无分类引用");
    assert_eq!(rows[1].note, "-", "无备注");
    assert_eq!(rows[2].category, "餐饮 > 午餐", "子分类为「父 > 子」路径");
    assert_eq!(rows[2].note, "买咖啡");
}

/// 账户与商户读接缝（#2020 账户格 / 借贷派生 / 商户格的参考数据面）：账户读
/// **在用可见**口径（排除软删与隐藏——旧壳 accountMap 同口径），商户读**含软删
/// 全量**（历史交易显示，issue #191）。删除读接缝（account_rows / merchant_rows）
/// 或装配消费任一环即红（呈现侧断言归 headless 集成测试）。
#[test]
fn account_and_merchant_rows_read_with_reference_visibility_rules() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("refs");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let visible = ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: "现金".to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    // 黑洞账户（隐藏，不在 list_accounts 投影）：域公开入口即建路径。
    ledger_accounts::ensure_black_hole_account(&conn, "CNY").unwrap();
    let merchant = ledger_merchants::create_merchant(
        &conn,
        ledger_merchants::MerchantInput {
            name: "京东".to_owned(),
        },
    )
    .unwrap();
    ledger_merchants::delete_merchant(&conn, &merchant).unwrap();
    drop(conn);

    let accounts = account_rows(&db).unwrap();
    assert_eq!(
        accounts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        [visible.as_str()],
        "账户投影只收在用且未隐藏（黑洞不在表）"
    );
    let merchants = merchant_rows(&db).unwrap();
    assert_eq!(
        merchants
            .iter()
            .map(|m| (m.id.as_str(), m.is_deleted))
            .collect::<Vec<_>>(),
        [(merchant.as_str(), true)],
        "商户投影含软删全量（历史交易显示口径）"
    );
}

#[test]
fn boot_missing_book_file_fails_coded() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("missing");
    let err = match boot_ledger(&dir) {
        Ok(_) => panic!("库文件缺失应报错，实际引导成功"),
        Err(e) => e,
    };
    assert!(
        err.is_code(CODE_BOOK_MISSING),
        "库文件缺失报码化错误，实际：{err}"
    );
    // 文案重写（ADR-0146 决策 4）：正式呈现文案，立桩期表述清零；防假空态
    // 内核保留（不自动新建的立场写进文案），路径随参数透出。
    let message = err.to_string();
    assert!(
        message.contains(dir.join("ledger.db").to_string_lossy().as_ref()),
        "缺失路径随文案透出：{message}"
    );
    assert!(
        message.contains("不会自动新建账本库"),
        "防假空态内核在文案：{message}"
    );
    assert!(!message.contains("立桩"), "立桩表述已清零：{message}");
}

#[test]
fn boot_page_aligned_foreign_header_lands_awaiting_unlock_and_locks_gate() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("locked");
    // 两整页非明文魔数字节（页对齐 = 基础设施「密文库落盘形态」判别）：
    // 引导层分类为 AwaitUnlock，产出等待解锁码化信号（呈现层落锁定视图）。
    fs::write(dir.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    let err = match boot_ledger(&dir) {
        Ok(_) => panic!("密文库应落等待解锁信号，实际引导成功"),
        Err(e) => e,
    };
    assert!(
        err.is_code(CODE_AWAITING_UNLOCK),
        "密文库报等待解锁码化信号，实际：{err}"
    );
    // 引导记录落槽（解锁 / 重置编排的路径读源）+ 锁定门置锁（呈现层据码落
    // 锁定视图；备份调度 / 同步触发的门检在锁定期间空转）。
    let record = crate::boot::boot_record().expect("密文库引导记录应落槽");
    assert_eq!(record.db_dir, dir, "引导记录登记密文库目录");
    assert!(crate::boot::encryption_locked(), "密文库引导后锁定门应置锁");
    // 复位门槽，不泄入后续用例（串行守卫内手动复位；headless 场景经
    // reset_book_assembly 同批复位）。
    crate::boot::reset_encryption_gate_for_tests();
}

// —— 选中目录现场闭集处置（ADR-0146 决策 3 单点；分支语义归本层，UI 接线归
// headless 旅程——删任一层断言各自变红，两层断言不重复对准同一实现形状）。——

/// 两本注册表现场（活动本 b1 + 追加他本；目录选择闭集用例的共用前置）。
fn seed_two_book_registry(root: &Path, active_dir: &Path, other: (&str, &Path)) {
    use ledger_infra::db::book_registry::{Book, BookRegistry, RegistryOrigin, write_registry};
    write_registry(
        root,
        &BookRegistry {
            books: vec![
                Book {
                    id: "b1".to_owned(),
                    name: "活动账本".to_owned(),
                    dir: active_dir.to_path_buf(),
                },
                Book {
                    id: other.0.to_owned(),
                    name: "备用账本".to_owned(),
                    dir: other.1.to_path_buf(),
                },
            ],
            active_id: "b1".to_owned(),
            pending_relocation: None,
            origin: RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
}

/// 注册表活动本读取（用例断言助手；解析失败即 panic）。
fn active_book_of(root: &Path) -> ledger_infra::db::book_registry::Book {
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    match read_registry(root) {
        RegistryRead::Resolved(registry) => registry.active().unwrap().clone(),
        other => panic!("注册表应可解析，实际：{other:?}"),
    }
}

/// 现场闭集①（注册表在册其他本）：切活动指针，直接判开；已是活动本幂等
/// （零改写零错误）。
#[test]
fn picked_registered_dir_switches_active_pointer_and_is_idempotent() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let active = scratch_book_dir("t4-active");
    let other = scratch_book_dir("t4-other");
    create_empty_book_file(&other);
    seed_two_book_registry(&root, &active, ("b2", &other));
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &other).unwrap(),
            PickedBookDisposition::Open
        ),
        "在册目录直接判开，不问新建"
    );
    assert_eq!(active_book_of(&root).id, "b2", "活动指针切至在册其他本");
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &other).unwrap(),
            PickedBookDisposition::Open
        ),
        "已是活动本再选同目录幂等判开"
    );
    assert_eq!(active_book_of(&root).id, "b2", "幂等重选不改写指针");
}

/// F3（ADR-0146 决策 3⑤ + 决策 4；④随 #2088 反转为登记+打开，修订注记在
/// ADR）：选中**在册但**损坏残留的本 → 先分类后切指针：码化错误上抛且注册
/// 表零写入（先切指针再打开失败会把指针移走，该失败态无选择器出口——应用
/// 内不可恢复）。旧序「在册优先于文件处置」钉死用例随语义翻转改造：断言
/// 错误呈现 + 注册表内容不变。
#[test]
fn picked_registered_corrupt_dir_reports_coded_error_without_registry_change() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let active = scratch_book_dir("t4-active");
    let other = scratch_book_dir("t4-other");
    // 损坏残留夹具：非魔数且非页对齐（probe 计密文形态、无密文落盘形态 →
    // Unreadable）。
    fs::write(other.join("ledger.db"), vec![0xABu8; 20]).unwrap();
    seed_two_book_registry(&root, &active, ("b2", &other));
    let err = resolve_picked_book_dir(&root, &other).unwrap_err();
    assert!(
        err.is_code(ledger_infra::db::boot::BOOT_DB_UNREADABLE),
        "在册损坏残留报既有损坏码化错误，实际：{err}"
    );
    assert_eq!(active_book_of(&root).id, "b1", "注册表零写入：指针未切");
}

/// 现场闭集②（有库未登记）：登记新条目（uuid、展示名取目录名）+ 置活动。
#[test]
fn picked_unregistered_book_dir_registers_and_activates() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let active = scratch_book_dir("t4-active");
    let picked = scratch_book_dir("t4-picked");
    create_empty_book_file(&picked);
    let other = scratch_book_dir("t4-other");
    seed_two_book_registry(&root, &active, ("b2", &other));
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &picked).unwrap(),
            PickedBookDisposition::Open
        ),
        "有库未登记判开"
    );
    use ledger_infra::db::book_registry::RegistryRead;
    let RegistryRead::Resolved(registry) = ledger_infra::db::book_registry::read_registry(&root)
    else {
        panic!("注册表应可解析")
    };
    assert_eq!(registry.books.len(), 3, "新条目已登记");
    let picked_name = picked.file_name().unwrap().to_string_lossy().to_string();
    let entry = registry
        .books
        .iter()
        .find(|book| book.dir == picked)
        .expect("新条目指向选中目录");
    assert_eq!(entry.name, picked_name, "展示名取目录名");
    assert_eq!(registry.active_id, entry.id, "新条目即活动本");
}

/// 现场闭集③（无库 → 新建意图）：确认前零写入；确认完成动作建库（建连
/// 迁移单点）+ 登记（未配置根折叠为「默认账本」条目，ADR-0146 决策 5）。
#[test]
fn picked_dir_without_book_asks_create_then_confirm_creates_and_registers() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let picked = scratch_book_dir("t4-new");
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &picked).unwrap(),
            PickedBookDisposition::ConfirmCreate(ref path) if path == &picked,
        ),
        "无库目录返回新建意图并明示路径"
    );
    assert!(!picked.join("ledger.db").exists(), "确认前零写入");
    assert!(
        !root.join("data_location.json").exists(),
        "确认前注册表零写入"
    );
    confirm_create_picked_book(&root, &picked).unwrap();
    assert!(
        picked.join("ledger.db").is_file(),
        "建库走建连迁移单点（新装语义）"
    );
    let book = active_book_of(&root);
    assert_eq!(book.dir, picked, "登记指向新建目录");
    assert_eq!(
        book.name,
        picked.file_name().unwrap().to_string_lossy().to_string(),
        "任意目录登记展示名取目录名"
    );
    // 决策 5 的首本形态：选中目录即注册表根本身（未配置）→ 折叠为「默认账
    // 本」条目（与词汇表存量折叠路径同形，id 稳定派生）。
    let root2 = scratch_book_dir("t4-root2");
    confirm_create_picked_book(&root2, &root2).unwrap();
    assert!(root2.join("ledger.db").is_file(), "默认目录建库");
    let folded = active_book_of(&root2);
    assert_eq!(folded.dir, root2);
    assert_eq!(
        folded.name,
        ledger_infra::db::book_registry::DEFAULT_BOOK_NAME,
        "未配置根折叠为「默认账本」条目（决策 5）"
    );
}

/// 现场闭集④（密文库，#2088 反转后语义）：登记新条目 + 置活动指针 + 判开
///（调用方打开后引导落锁定视图解锁——旧「码化错误 + 零写入」用例随解锁路
/// 径落地改造）。删登记换入（退回码化拦截）→ 判开断言红。
#[test]
fn picked_encrypted_dir_registers_and_activates_then_lands_awaiting_unlock() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let active = scratch_book_dir("t4-active");
    let picked = scratch_book_dir("t4-encrypted");
    fs::write(picked.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    let other = scratch_book_dir("t4-other");
    seed_two_book_registry(&root, &active, ("b2", &other));
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &picked).unwrap(),
            PickedBookDisposition::Open
        ),
        "密文库目录判开（登记 + 置活动）"
    );
    assert_ne!(
        active_book_of(&root).id,
        "b1",
        "活动指针已切至新登记的密文本"
    );
    // 打开后的引导：等待解锁信号 + 门置锁（呈现层落锁定视图）。
    let err = match boot_ledger(&picked) {
        Ok(_) => panic!("密文库应落等待解锁信号，实际引导成功"),
        Err(e) => e,
    };
    assert!(
        err.is_code(CODE_AWAITING_UNLOCK),
        "密文库报等待解锁码化信号，实际：{err}"
    );
    crate::boot::reset_encryption_gate_for_tests();
}

/// 现场闭集⑤（损坏残留）：既有损坏码化错误，注册表零写入（不自动换目录
/// 重试）。
#[test]
fn picked_corrupt_dir_reports_coded_error_without_registry_change() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-root");
    let active = scratch_book_dir("t4-active");
    let picked = scratch_book_dir("t4-corrupt");
    // 损坏残留夹具：非魔数且非页对齐（probe 计密文形态、无密文落盘形态 →
    // Unreadable；短字节串会落 Empty=新装语义，不构成损坏残留）。
    fs::write(picked.join("ledger.db"), vec![0xABu8; 20]).unwrap();
    let other = scratch_book_dir("t4-other");
    seed_two_book_registry(&root, &active, ("b2", &other));
    let err = resolve_picked_book_dir(&root, &picked).unwrap_err();
    assert!(
        err.is_code(ledger_infra::db::boot::BOOT_DB_UNREADABLE),
        "损坏残留报既有损坏码化错误，实际：{err}"
    );
    assert_eq!(active_book_of(&root).id, "b1", "注册表零写入");
}

/// F2（ADR-0146 决策 3，分支②在损坏现场的容错）：注册表损坏时查册按
/// 「未注册」容错——有库未登记目录 → 登记新条目 + 置活动 + 判开，写覆盖
/// 损坏文件（与首启新建腿 create_default_book「告示知情后覆盖损坏指针」
/// 裁决同向：引导回退告示已提供知情）。查册损坏上抛（registry-corrupt
/// 死路文案指不可达的设置面）→ 本用例红。
#[test]
fn picked_dir_at_corrupt_registry_tolerates_as_unregistered_and_opens() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-corrupt-root");
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    let picked = scratch_book_dir("t4-corrupt-picked");
    create_empty_book_file(&picked);
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &picked).unwrap(),
            PickedBookDisposition::Open
        ),
        "损坏现场有库未登记目录：登记 + 判开"
    );
    let book = active_book_of(&root);
    assert_eq!(book.dir, picked, "损坏文件被覆盖：选中目录登记为活动本");
    assert_eq!(
        book.name,
        picked.file_name().unwrap().to_string_lossy().to_string(),
        "展示名取目录名"
    );
}

/// F4（决策 3 分支③完成动作换序）：先建库后登记——建库失败 → 注册表零
/// 写入（确认对话可重试或换目录重选，不留「已登记但无库」的悬死现场）。
/// 旧序「登记先行」下建库失败已写注册表 → 本用例红。
#[test]
fn confirm_create_failed_open_leaves_registry_untouched() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-f4-root");
    let picked = scratch_book_dir("t4-f4-picked");
    // 建库必败现场：ledger.db 是目录（Connection::open 拒绝打开目录）。
    fs::create_dir(picked.join("ledger.db")).unwrap();
    assert!(confirm_create_picked_book(&root, &picked).is_err());
    assert!(
        !root.join("data_location.json").exists(),
        "建库失败注册表零写入"
    );
}

/// F2（ADR-0146 决策 3，分支③在损坏现场的容错）：损坏根 + 空目录 → 新建
/// 意图 → 确认后折叠登记（默认目录折叠为「默认账本」条目，同未配置根）+
/// 建库打开。确认后注册表可解析（损坏文件被覆盖）。
#[test]
fn picked_empty_dir_at_corrupt_registry_confirms_create_folds_default_book() {
    let _guard = serial_guard();
    let root = scratch_book_dir("t4-corrupt-fold");
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    // 空目录即损坏根自身（默认数据目录无库，引导回退现场）。
    assert!(
        matches!(
            resolve_picked_book_dir(&root, &root).unwrap(),
            PickedBookDisposition::ConfirmCreate(ref path) if path == &root,
        ),
        "损坏根无库：新建意图"
    );
    confirm_create_picked_book(&root, &root).unwrap();
    assert!(root.join("ledger.db").is_file(), "确认后建库");
    let folded = active_book_of(&root);
    assert_eq!(folded.dir, root, "折叠登记指向默认目录");
    assert_eq!(
        folded.name,
        ledger_infra::db::book_registry::DEFAULT_BOOK_NAME,
        "损坏现场折叠为「默认账本」条目（与出厂逃生舱折叠语义同向）"
    );
}

/// 立桩竖井的端到端视图态（装配入口注入 → 引导 → 会话就位 → 首页读）：headless
/// 组件测试的前置语义在 lib 侧先钉一半（startup 装配与三态文案），窗口呈现
/// 接线归 `tests/app_headless.rs`。
#[test]
fn run_startup_without_book_dir_renders_usage() {
    let _guard = serial_guard();
    // 缺席路径显式复位（headless 集成测试与本文件内的注入用例都可能先行运行，
    // 进程级槽位必须回到空态，缺席断言才与用例顺序无关）。
    crate::app::reset_book_assembly();
    assert_eq!(run_startup(), StartupView::MissingArg);
    assert_eq!(headline_text(&StartupView::MissingArg), "未指定账本库目录");
    assert_eq!(headline_text(&StartupView::Loading), "正在打开账本库…");
    assert_eq!(
        headline_text(&StartupView::Ready),
        "",
        "就绪态不走启动文案路径（表格视图承载）"
    );
    assert_eq!(
        headline_text(&StartupView::Failed {
            message: "账本库文件不存在".to_owned(),
            can_pick_book_dir: false,
        }),
        "启动失败：账本库文件不存在"
    );
}

/// 首启引导现场（ADR-0146 决策 2，spec #2090 T2 #2092）：数据根已注入、
/// 注册表未配置且解析落点（默认数据目录）无库 → 启动竖井呈现引导态（非
/// 缺参态），且**不静默建库**——建库必须经用户确认（对旧壳静默新建的有意
/// 偏离）。删缺参臂的引导映射（`Ok(None)` 回退 MissingArg）→ 本用例红
///（「删除即变红」，ADR-0087）。
#[test]
fn run_startup_unconfigured_without_book_renders_onboarding() {
    let _guard = serial_guard();
    crate::app::reset_book_assembly();
    let root = scratch_book_dir("onboarding");
    crate::cross_book::set_default_data_dir(root.clone());
    let view = run_startup();
    assert_eq!(
        view,
        StartupView::Onboarding {
            fallback_reason: None
        },
        "未配置且无库呈现首启引导态（干净出厂现场无回退原因）"
    );
    assert!(
        !root.join("ledger.db").exists(),
        "引导态不静默建库（建库须经确认对话）"
    );
    assert_eq!(headline_text(&view), "欢迎使用 Ledger");
    assert_eq!(
        crate::app::secondary_text(&view),
        "尚未找到账本库，新建一本账即可开始记账。"
    );
}

/// 损坏回退且无库的引导现场（ADR-0146 决策 2）：注册表损坏回退默认目录、
/// 落点无库 → 引导态携带回退原因（引导界面损坏告示的呈现源；缺参态与干净
/// 出厂现场均不带）。删引导记录落槽（回退原因不随 `Ok(None)` 落槽）→ 本
/// 用例红。
#[test]
fn run_startup_corrupt_registry_without_book_onboarding_carries_fallback_reason() {
    let _guard = serial_guard();
    crate::app::reset_book_assembly();
    let root = scratch_book_dir("onboarding-corrupt");
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    crate::cross_book::set_default_data_dir(root);
    match run_startup() {
        StartupView::Onboarding {
            fallback_reason: Some(reason),
        } => assert!(
            !reason.is_empty(),
            "回退原因随引导态携带（告示数据源）：{reason}"
        ),
        other => panic!("损坏回退无库应呈现带告示的引导态，实际：{other:?}"),
    }
}

/// F1 现场（ADR-0146 决策 4）：注册表已配置且活动本指向默认数据目录自身、
/// 库文件缺失（库被外部删除的升级现场）→ book-missing 失败态（带目录选择
/// 出口），**不进首启引导**——引导现场 = 注册表未配置（含损坏回退）且解析
/// 落点无库（决策 2）；已解析活动条目缺库一律 fail loud（决策 4）。删
/// `boot_ledger_via_registry` 的注册表状态信号分流（退回路径等值判定）→
/// 本用例呈现引导态即红。
#[test]
fn run_startup_configured_active_book_at_default_dir_missing_fails_loud() {
    let _guard = serial_guard();
    crate::app::reset_book_assembly();
    let root = scratch_book_dir("onboarding-at-default");
    // 注册表已配置：唯一活动本指向默认数据目录自身，库文件缺失。
    ledger_infra::db::book_registry::write_registry(
        &root,
        &ledger_infra::db::book_registry::BookRegistry {
            books: vec![ledger_infra::db::book_registry::Book {
                id: "b1".to_owned(),
                name: "活动账本".to_owned(),
                dir: root.clone(),
            }],
            active_id: "b1".to_owned(),
            pending_relocation: None,
            origin: ledger_infra::db::book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    crate::cross_book::set_default_data_dir(root);
    match run_startup() {
        StartupView::Failed {
            message,
            can_pick_book_dir,
        } => {
            assert!(
                message.contains("账本库文件不存在"),
                "book-missing 失败态呈现：{message}"
            );
            assert!(can_pick_book_dir, "book-missing 失败态挂目录选择出口");
        }
        other => panic!("已配置+活动本=默认目录+缺库应 fail loud，实际：{other:?}"),
    }
}

/// 会话句柄读取（app 槽位的 crate 内测试出口，随头注可见性）。
fn nsh_app_session() -> std::sync::Arc<ledger_infra::db::DbState> {
    crate::app::session_handle().expect("就绪时会话句柄已就位")
}

/// 注入装配入口后的启动竖井：引导成功 → 会话句柄就位 → 首页读得真实行集
/// （删除任一环——注入 / 引导 / 会话句柄 / 首页读——本用例即红，ADR-0087 接线
/// 断言；窗口呈现侧的对应断言归 `tests/app_headless.rs`）。
#[test]
fn run_startup_with_seeded_book_places_session_and_reads_first_page() {
    let _guard = serial_guard();
    let dir = scratch_book_dir("startup");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 2);
    set_book_dir(dir.clone());
    assert_eq!(run_startup(), StartupView::Ready, "引导成功呈现就绪态");
    let db = nsh_app_session();
    let page = crate::boot::transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(page.total, 2, "会话句柄可直读首页（读路径直调域服务）");
    assert_eq!(page.items.len(), 2);
}

/// 删除写接缝（issue #2022）：[`crate::boot::delete_transaction`] 经核心交易域
/// 公开删除入口直调（软删编排同处一个事务，余额重算由已接线接缝执行）——删
/// 后读路径不再投影该笔；不存在的 id 报码化 NotFound（`transaction.not-found`）。
/// 删除域语义归核心交易域单测，本用例只钉壳写接线的两个消费契约。
#[test]
fn delete_transaction_writes_via_domain_entry_and_reports_coded_not_found() {
    use crate::boot::delete_transaction;
    use ledger_infra::error::AppError;

    let _guard = serial_guard();
    let dir = scratch_book_dir("delete-write-seam");
    create_empty_book_file(&dir);
    seed_expense_notes(&dir, &["要删除的一笔", "要留下的一笔"]);
    let db = boot_ledger(&dir).unwrap();
    let gone_id = {
        let conn = db
            .conn
            .lock()
            .map_err(|e| AppError::Db(e.to_string()))
            .unwrap();
        ledger_transaction::list_transactions(
            &conn,
            &ledger_transaction::TransactionListFilter {
                page: Some(1),
                page_size: Some(100),
                ..Default::default()
            },
        )
        .unwrap()
        .items
        .into_iter()
        .find(|tx| tx.note.as_deref() == Some("要删除的一笔"))
        .expect("造数行集含要删除的一笔")
        .id
    };

    delete_transaction(&db, &gone_id).expect("存在的 id 删除成功");

    let page = transaction_page(&db, 1, &crate::boot::TxListQuery::default()).unwrap();
    assert_eq!(page.total, 1, "删除后 total = 未删除笔数");
    assert!(
        page.items.iter().all(|tx| tx.id != gone_id),
        "软删行不在投影行集"
    );

    let err = delete_transaction(&db, "no-such-id").expect_err("不存在的 id 报错");
    assert!(
        err.is_code("transaction.not-found"),
        "不存在 id 报码化 NotFound，实际：{err}"
    );
}

/// 壳写路径置脏接线（issue #2087）：壳写函数族经壳层统一写入口直锁形态
/// （[`crate::shell_support::write_entry_locked`] → 连接层 [`ledger_infra::db::write_locked`]
/// 已持锁形态）执行，备份域提交点钩子（置脏 + 写时顺带到期检查，ADR-0032）
/// 结构性生效。对照组钉住判定面：同闭包形状的域直调（不经写入口）不置脏——
/// 置脏宿主是写入口单点，不是任意写。删除写入口收拢（boot 写函数改回直锁）
/// 或删备份域接线（[`crate::wiring`] 的 `install_after_commit_hook`）任一环
/// → 脏标记不翻转 → 本测红（ADR-0087 接线断言）。余额刷新回归断言随行：
/// 写入 / 删除后 V017 缓存行跟随（写路径接缝行为不因收拢回退）。
#[test]
fn shell_transaction_writes_reach_after_commit_dirty_marker() {
    use crate::boot::{account_balance_rows, create_transaction, transaction_page};
    use ledger_infra::error::AppError;

    let _guard = serial_guard();
    let dir = scratch_book_dir("write-entry-dirty");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    // 测试隔离：目录镜像置空——写时顺带到期检查因目录未配置静默跳过（不清
    // 脏），本测只钉置脏面；lib 测试进程内镜像本就未配置，显式置空防泄漏。
    ledger_backup::shared_prefs().set_dir(None);
    let account = seed_cash_account(&db);
    let dirty = |db: &ledger_infra::db::DbState| {
        let conn = db
            .read_conn
            .lock()
            .map_err(|e| AppError::Db(e.to_string()))
            .unwrap();
        ledger_backup::get_state(&conn).unwrap().dirty
    };

    // 对照组：域直调（同闭包形状，不经壳写入口）——不置脏。
    {
        let conn = db
            .conn
            .lock()
            .map_err(|e| AppError::Db(e.to_string()))
            .unwrap();
        ledger_transaction::create(
            &conn,
            expense_input(&account, &test_date(0), 100, "域直调对照"),
        )
        .unwrap();
    }
    assert!(!dirty(&db), "域直调（不经写入口）不应置脏");

    // 壳写入口：create → 提交点置脏翻转。
    create_transaction(&db, expense_input(&account, &test_date(1), 200, "壳写入口"))
        .expect("壳写路径创建成功");
    assert!(dirty(&db), "壳写路径应经写入口提交点置脏（备份域钩子生效）");

    // 删除路径同接线：先显式清脏（消除 create 段遗留标记，删除后置脏才可判
    // 定），再经壳写入口删除——脏标记重新翻转（清脏后不重翻即删入口脱线）。
    let id = transaction_page(&db, 1, &crate::boot::TxListQuery::default())
        .unwrap()
        .items
        .into_iter()
        .find(|tx| tx.note.as_deref() == Some("壳写入口"))
        .expect("行集含壳写入口一笔")
        .id;
    {
        let conn = db
            .conn
            .lock()
            .map_err(|e| AppError::Db(e.to_string()))
            .unwrap();
        ledger_infra::settings::set(
            &conn,
            ledger_infra::settings::SettingKey::AutoBackupDirty,
            &false,
        )
        .unwrap();
    }
    assert!(!dirty(&db), "前置清脏应生效");
    crate::boot::delete_transaction(&db, &id).expect("壳写路径删除成功");
    assert!(dirty(&db), "删除路径同经写入口提交点置脏");

    // 余额刷新回归：V017 缓存行 = 0 − 100（直调）+ 200（壳写）− 200（壳删）。
    let rows = account_balance_rows(&db).unwrap();
    assert_eq!(rows.len(), 1, "单账户行集");
    assert_eq!(
        rows[0].balance_cents, -100,
        "余额刷新接缝不因写入口收拢回退"
    );
}

// ---------------------------------------------------------------------------
// 预算读写接缝（#2033）：读路径（进度 / 年度汇总）与写路径（创建 / 编辑 /
// 删除）直调 `ledger-budget` 域入口，不经 IPC——删除任一环本组用例即红
// （ADR-0087 接线断言）。预算域语义（口径 / 校验 / 同步命令）归域单测，本组
// 只钉壳侧接线的消费契约：调用贯通、会话选择（写侧 conn）、码化错误透传。
// ---------------------------------------------------------------------------

/// 造一个支出分类（域公开写入口）。
fn seed_expense_category(db: &ledger_infra::db::DbState, name: &str) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: name.to_owned(),
            kind: "expense".to_owned(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap()
}

#[test]
fn budget_write_and_read_seams_round_trip_through_domain_entries() {
    use crate::boot::{
        budget_annual_summary, budget_progress, create_budget, delete_budget, update_budget,
    };
    use ledger_budget::{BudgetInput, BudgetPeriod};

    let _guard = serial_guard();
    let dir = scratch_book_dir("budget-seam");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let category = seed_expense_category(&db, "餐饮");

    // 创建：域公开写入口贯通（写侧 conn）。
    let id = create_budget(
        &db,
        &BudgetInput {
            category_id: category.clone(),
            period: Some(BudgetPeriod::Monthly),
            amount_cents: 50_000,
            start_date: crate::boot::today_string(),
        },
    )
    .unwrap();

    // 进度读：行集含该预算，分类名与金额直出（spent=0 无支出）。
    let rows = budget_progress(&db).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].budget.id, id);
    assert_eq!(rows[0].budget.amount_cents, 50_000);
    assert_eq!(rows[0].category_name, "餐饮");
    assert_eq!(rows[0].spent_cents, 0);
    assert!(!rows[0].over_budget);

    // 本月一笔分类支出 → 进度与年度汇总同刻更新（读路径贯通）。
    {
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let account = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "现金".to_owned(),
                kind: AccountType::Cash,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let mut input = expense_input(&account, &crate::boot::today_string(), 12_000, "本月支出");
        input.category_id = Some(category.clone());
        ledger_transaction::create(&conn, input).unwrap();
    }
    let rows = budget_progress(&db).unwrap();
    assert_eq!(rows[0].spent_cents, 12_000, "进度读路径贯通（本月净支出）");
    assert!(!rows[0].over_budget, "12_000 < 50_000 未超支");

    // 年度汇总：年化总额 = 50000 × 12，已支 = 本年累计。
    let summary = budget_annual_summary(&db).unwrap();
    assert_eq!(summary.annual_budget_total_cents, 600_000);
    assert_eq!(summary.spent_cents, 12_000);
    assert!(!summary.over_budget);

    // 编辑：金额与周期一并落定（写接缝贯通）。
    update_budget(&db, &id, 30_000, Some(BudgetPeriod::Yearly)).unwrap();
    let rows = budget_progress(&db).unwrap();
    assert_eq!(rows[0].budget.amount_cents, 30_000);
    assert_eq!(rows[0].budget.period, BudgetPeriod::Yearly);

    // 删除：软删后行集离场。
    delete_budget(&db, &id).unwrap();
    assert!(budget_progress(&db).unwrap().is_empty());

    // 不存在的 id：域删除幂等（UPDATE 静默 0 行仍 Ok——预算域语义，壳侧透传）。
    delete_budget(&db, "no-such-budget").unwrap();
}

#[test]
fn budget_create_surfaces_domain_validation_as_coded_error() {
    use crate::boot::create_budget;
    use ledger_budget::{BudgetInput, BudgetPeriod};

    let _guard = serial_guard();
    let dir = scratch_book_dir("budget-seam-validate");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let category = seed_expense_category(&db, "文具");

    // 金额非正：域校验拒绝，码化错误原样上抛（视图层呈现面）。
    let err = create_budget(
        &db,
        &BudgetInput {
            category_id: category,
            period: Some(BudgetPeriod::Monthly),
            amount_cents: 0,
            start_date: crate::boot::today_string(),
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("预算金额必须为正数"),
        "域校验错误透传，实际：{err}"
    );
}

// ---------------------------------------------------------------------------
// 备份最小入口（#2038）：默认文件名生成面（旧壳 `defaultBackupFileName` 同
// 格式的生成单点，backup_entry 模块）。格式闭集两态：指纹在场携标识段、
// 缺失退化旧命名（ADR-0016 受管命名；前后端各持一份前缀常量的取舍下，本壳
// 生成面与旧壳前端逐字同形，受管判定仍归备份域与旧壳前端）。
// ---------------------------------------------------------------------------

#[test]
fn default_backup_file_name_carries_fingerprint_segment() {
    use crate::backup_entry::default_backup_file_name;
    use chrono::TimeZone;

    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 9, 14, 5, 9)
        .unwrap();
    assert_eq!(
        default_backup_file_name(now, Some("fp-abc")),
        "ledger-backup-20261109-140509-fp-abc.db.zip",
        "指纹在场：时间戳后带标识段（旧壳 defaultBackupFileName 同格式）"
    );
}

#[test]
fn default_backup_file_name_degrades_without_fingerprint() {
    use crate::backup_entry::default_backup_file_name;
    use chrono::TimeZone;

    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 9, 14, 5, 9)
        .unwrap();
    assert_eq!(
        default_backup_file_name(now, None),
        "ledger-backup-20261109-140509.db.zip",
        "指纹缺失（空库未固化）：退化为无标识旧命名"
    );
}

// ---------------------------------------------------------------------------
// 概览视图读接缝（#2039）：净资产总览（dashboard 域单点取数）、本月收支
// （报表域月度汇总）、投资概览（投资域持仓行 + 累计收益折本位币单值）与
// 财务自由度（投资域聚合，分子分母同事务）直调域入口——与旧壳
// dashboard_overview / monthly_summary / list_holdings /
// cumulative_pnl_native_total / financial_freedom 命令消费的同一域函数，
// 不经 IPC。建库走产品开库入口、造数经域公开写入口（模块头造数纪律）。
// ---------------------------------------------------------------------------

/// 造一个指定币种指定类型的账户（概览读接缝的造数入口；返回账户 id）。
fn seed_account_with_currency(
    db: &ledger_infra::db::DbState,
    name: &str,
    kind: AccountType,
    currency_code: &str,
    initial_balance_cents: i64,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: name.to_owned(),
            kind,
            currency_code: currency_code.to_owned(),
            initial_balance_cents: Some(initial_balance_cents),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap()
}

#[test]
fn dashboard_overview_and_monthly_seams_read_seeded_book() {
    use crate::boot::{dashboard_overview, monthly_summary_rows};

    let _guard = serial_guard();
    let dir = scratch_book_dir("dashboard-seam");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();

    // 净资产 = Σ 折本位币余额（现金腿）。期初 100_000 分 + 本月收入 5_000 −
    // 本月支出 2_000 = 103_000（月初余额经期初余额进账，折算 CNY 恒等）。
    let cash = seed_account_with_currency(&db, "现金", AccountType::Cash, "CNY", 100_000);
    let today = crate::boot::today_string();
    {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(&conn, income_input(&cash, &today, 5_000, "收入")).unwrap();
        ledger_transaction::create(&conn, expense_input(&cash, &today, 2_000, "支出")).unwrap();
    }

    // 净资产总览（dashboard 域读探针直调；缓存指纹命中与否对读数无感）。
    let overview = dashboard_overview(&db).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(
        overview.net_worth_cents, 103_000,
        "净资产 = 账户折本位币余额合计（含期初余额）"
    );
    assert_eq!(overview.accounts_balance_cents, 103_000);
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.physical_assets_value_cents, 0);

    // 本月收支（报表域月度汇总直调，当年行集含当月）：收入 5_000 / 毛支出
    // 2_000 / 退款 0——展示层净支出 = 毛支出 − 退款（装配层推导，域行集不重复）。
    let year = chrono::Local::now().year();
    let rows = monthly_summary_rows(&db, i64::from(year)).unwrap();
    let month_key = format!("{year}-{}", chrono::Local::now().format("%m"));
    let current = rows.iter().find(|row| row.month == month_key);
    let current = current.unwrap_or_else(|| panic!("当年行集含当月 {month_key}"));
    assert_eq!(current.income_cents, 5_000);
    assert_eq!(current.expense_cents, 2_000);
    assert_eq!(current.refund_cents, 0);
}

// ---------------------------------------------------------------------------
// 报表视图读接缝（#2048）：期间口径三张报表直调报表域入口——与旧壳
// monthly_summary / category_shares / merchant_shares 命令的 from/to 期间
// 形态消费的同一域函数，不经 IPC。建库走产品开库入口、造数经域公开写入口
// （模块头造数纪律）。
// ---------------------------------------------------------------------------

#[test]
fn report_period_seams_read_seeded_book() {
    use crate::boot::{category_shares_rows, merchant_shares_report, monthly_summary_for_period};
    use crate::period::PeriodRange;

    let _guard = serial_guard();
    let dir = scratch_book_dir("report-seam");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let cash = ledger_accounts::create_account(
        &db.conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap(),
        AccountInput {
            name: "现金".to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    let merchant = ledger_merchants::create_merchant(
        &db.conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap(),
        ledger_merchants::MerchantInput {
            name: "甲城".to_owned(),
        },
    )
    .unwrap();

    // 2026 固定日期两月（期间口径口径面：双月按月分组、期间过滤只含期内）。
    {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(&conn, expense_input(&cash, "2026-03-05", 4_000, "午餐"))
            .unwrap();
        ledger_transaction::create(&conn, expense_input(&cash, "2026-03-06", 800, "通勤")).unwrap();
        ledger_transaction::create(&conn, income_input(&cash, "2026-03-07", 8_000, "进账"))
            .unwrap();
        let mut input = expense_input(&cash, "2026-01-20", 2_500, "追加");
        input.merchant_id = Some(merchant);
        ledger_transaction::create(&conn, input).unwrap();
    }

    let range = PeriodRange {
        from: "2026-03-01".to_owned(),
        to: "2026-03-31".to_owned(),
    };

    // 月度汇总（期间内按月分布）：单行 3 月，收入 8000 / 毛支出 4800 / 退款 0。
    let monthly = monthly_summary_for_period(&db, &range).unwrap();
    assert_eq!(monthly.len(), 1, "1 月行不进期间口径");
    assert_eq!(monthly[0].month, "2026-03");
    assert_eq!(monthly[0].income_cents, 8_000);
    assert_eq!(monthly[0].expense_cents, 4_800);

    // 分类份额（未分类匡入同口径）：期间内单行「未分类」4800（本测试
    // 造数不带分类——结构与数值由 headless 分类旅程另钉）。
    let shares = category_shares_rows(&db, &range).unwrap();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].category_id, "");
    assert_eq!(shares[0].category_name, "未分类");
    assert_eq!(shares[0].amount_cents, 4_800);

    // 商户排行（期间 + topN）：商户交易在 1 月、不在 3 月期间 → 空集 + 合计 0。
    let report = merchant_shares_report(&db, &range, Some(5)).unwrap();
    assert!(
        report.rows.is_empty(),
        "期间过滤商户排行：{:?}",
        report.rows
    );
    assert_eq!(report.total_cents, 0);
    let q1 = PeriodRange {
        from: "2026-01-01".to_owned(),
        to: "2026-01-31".to_owned(),
    };
    let report_q1 = merchant_shares_report(&db, &q1, Some(5)).unwrap();
    assert_eq!(report_q1.rows.len(), 1);
    assert_eq!(report_q1.rows[0].merchant_name, "甲城");
    assert_eq!(report_q1.rows[0].amount_cents, 2_500);
    assert_eq!(report_q1.total_cents, 2_500, "全量合计与 topN 截断无关");
}

#[test]
fn portfolio_and_freedom_seams_read_holdings_and_budget() {
    use crate::boot::{cumulative_pnl_native_total, financial_freedom, portfolio_rows};
    use ledger_investment::{InstrumentInput, InstrumentType, MarketPriceInput};

    let _guard = serial_guard();
    let dir = scratch_book_dir("portfolio-seam");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();

    // 投资账户 + 标的 + 现价 + 买入：价格刻度万分之一元（ADR-0038）——现价
    // 1_000_000 = ¥100，1 份持仓市值 = 10_000 分；买入价 10_000 = ¥1（成本腿
    // 100 分，未实现盈亏 9_900 分，累计收益读数随之钉住）。期初现金 100 分
    // 恰好覆盖买入现金腿（买入后余额归零——可投资资产现金腿 0，不拖累分子）。
    let invest = seed_account_with_currency(&db, "证券", AccountType::Investment, "CNY", 100);
    let today = crate::boot::today_string();
    let instrument = {
        let conn = db.conn.lock().unwrap();
        let id = ledger_investment::create_instrument(
            &conn,
            InstrumentInput {
                symbol: "TST".to_owned(),
                kind: InstrumentType::Stock,
                name: Some("测试标的".to_owned()),
                currency_code: "CNY".to_owned(),
                market: None,
            },
        )
        .unwrap();
        ledger_investment::create_market_price(
            &conn,
            MarketPriceInput {
                instrument_id: id.clone(),
                price_cents: 1_000_000,
                currency_code: "CNY".to_owned(),
                priced_at: today.clone(),
                source: Some("manual".to_owned()),
            },
        )
        .unwrap();
        id
    };
    {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(&conn, buy_input(&invest, &instrument, &today)).unwrap();
    }

    // 持仓行集（v_holdings 直调，逐行折本位币软形态已算）。
    let holdings = portfolio_rows(&db).unwrap();
    assert_eq!(holdings.len(), 1, "持仓行集贯通");
    assert_eq!(holdings[0].quantity, 1.0);
    assert_eq!(holdings[0].market_value_cents, Some(10_000));
    assert_eq!(
        holdings[0].native_market_value_cents,
        Some(10_000),
        "CNY 账户当期恒等折算"
    );

    // 累计收益（未实现腿 9_900 = 市值 10_000 − 成本 100；折本位币单值 + 基准币种）。
    let cumulative = cumulative_pnl_native_total(&db).unwrap();
    assert_eq!(cumulative.native_currency, "CNY");
    assert_eq!(cumulative.total_cents, 9_900);

    // 财务自由度：月度预算 3_125（年化分母 37_500），分子 = 投资账户现金 0
    // + 持仓市值 10_000 = 10_000；比例 = 分子×3%÷分母×100 = 0.8（恰在一位
    // 小数刻度上，无舍入边）。
    let category = seed_expense_category(&db, "餐饮");
    {
        use ledger_budget::{BudgetInput, BudgetPeriod};
        let conn = db.conn.lock().unwrap();
        ledger_budget::crud::create_budget(
            &conn,
            &BudgetInput {
                category_id: category,
                period: Some(BudgetPeriod::Monthly),
                amount_cents: 3_125,
                start_date: today,
            },
        )
        .unwrap();
    }
    let freedom = financial_freedom(&db).unwrap();
    assert_eq!(
        freedom.numerator_cents, 10_000,
        "分子 = 持仓市值腿（现金腿已归零）"
    );
    assert_eq!(freedom.denominator_cents, 37_500, "分母 = 月预算 ×12");
    assert!(
        (freedom.ratio - 0.8).abs() < 1e-9,
        "比例一位小数刻度，实值 {:?}",
        freedom.ratio
    );
    assert_eq!(freedom.native_currency, "CNY");
}

/// bin 启动接线钉（#2077，源码扫描守门：main 不进 headless 测试进程，删接线
/// 或换序扫描即红——#959/#961 先例形态）：`logging::init` 的文件日志面解析日
/// 志目录消费跨账本数据根槽位，main 必须先注入数据根再初始化日志，且两处
/// 接线调用都在。
#[test]
fn main_wires_data_root_before_logging_init() {
    let main_rs = include_str!("main.rs");
    let data_root_pos = main_rs
        .find("cross_book::default_data_dir_from_env()")
        .expect("main 应注入跨账本注册表数据根");
    let logging_pos = main_rs
        .find("logging::init()")
        .expect("main 应接线日志初始化（stderr + 日滚文件双输出）");
    assert!(
        data_root_pos < logging_pos,
        "数据根注入必须先于日志初始化（文件日志目录解析消费数据根槽位）"
    );
}
