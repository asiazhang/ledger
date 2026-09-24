//! 投资明细列表读命令测试（ADR-0135 / issue #1778，命名对齐源码 ledger_tab 模块）：
//! 四维过滤逐维断言（账户涉及语义含出资/到账端、标的含 convert 两腿命中、kind 子集、
//! 日期区间）、排序与 offset 分页 + total、软删排除、各 kind 投影字段逐项断言、
//! 页与总数的读快照探针（多语句读闭包纪律，#1699 / #1702）。

use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{create_transaction_internal, delete_transaction_internal};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{ScratchDir, open, open_file, seed_account, seed_instrument};

/// 五种投资 kind 各造一行（buy/sell/convert/split/dividend）+ 一行通用 expense
/// （行集闭包的反证），全部 CNY 同币种免汇率铺垫。
fn seed_five_kinds(conn: &rusqlite::Connection) -> Vec<String> {
    seed_account(conn, "acc-it", "投资户", "investment", "CNY", 0);
    seed_account(conn, "acc-bank", "银行卡", "bank", "CNY", 0);
    seed_instrument(conn, "inst-a", "AAPL", "苹果", "CNY", "unknown");
    seed_instrument(conn, "inst-b", "MSFT", "微软", "CNY", "unknown");
    // 买入建仓（数量 10 @ 10 元 + 手续费 1 元）→ 卖出 5 → 转换 2（A→B）→ 份额调整 +2。
    let buy =
        create_transaction_internal(conn, make_buy_input("acc-it", "inst-a", 10.0, 100_000, 100))
            .unwrap()
            .id;
    let sell =
        create_transaction_internal(conn, make_sell_input("acc-it", "inst-a", 5.0, 100_000, 50))
            .unwrap()
            .id;
    let convert = create_transaction_internal(
        conn,
        make_convert_input("acc-it", "inst-a", "inst-b", 2.0, 1.0, 2_100, 2_000, 0),
    )
    .unwrap()
    .id;
    let split = create_transaction_internal(conn, make_split_input("acc-it", "inst-a", 2.0))
        .unwrap()
        .id;
    // 现金分红：现金腿到账银行卡（非投资账户）——到账账户端的造数前提。
    let dividend =
        create_transaction_internal(conn, make_dividend_input("acc-bank", "inst-a", 300, "CNY"))
            .unwrap()
            .id;
    // 通用 kind（expense）：投资明细行集闭包的反证行。
    create_transaction_internal(
        conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 100,
            currency_code: "CNY".into(),
            account_id: "acc-bank".into(),
            ..plain_expense_input()
        },
    )
    .unwrap();
    vec![buy, sell, convert, split, dividend]
}

/// 通用 expense 行的中性底座（金额/账户/币种显式，其余缺省）。
fn plain_expense_input() -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: String::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-03-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
    }
}

/// 各 kind 投影字段逐项断言（行投影是本命令的核心交付）：公共字段（行身份、
/// 行金额锚点、账户端、归属标的）恒在场；kind 专属载荷按形态携带。
#[test]
fn projection_covers_all_five_kinds_with_fields() {
    let conn = open();
    let ids = seed_five_kinds(&conn);

    let result =
        list_investment_transactions(&conn, &InvestmentTransactionListFilter::default()).unwrap();
    assert_eq!(
        result.total, 5,
        "行集闭包在五种投资 kind 内（expense 不入集）"
    );
    assert_eq!(result.items.len(), 5);

    let buy = result
        .items
        .iter()
        .find(|r| r.kind == TransactionKind::Buy)
        .expect("买入行应在场");
    assert_eq!(buy.id, ids[0]);
    assert_eq!(buy.date, "2026-01-10");
    assert_eq!(buy.account_id, "acc-it", "投资账户端");
    assert_eq!(buy.funding_account_id, None, "未带出资账户");
    assert_eq!(buy.instrument_id, "inst-a");
    assert_eq!(buy.symbol, "AAPL");
    assert_eq!(buy.instrument_name.as_deref(), Some("苹果"));
    assert_eq!(buy.instrument_type, "stock");
    let trade = buy.trade.as_ref().expect("buy 行应有买卖载荷");
    assert_eq!(trade.quantity, 10.0);
    assert_eq!(trade.price_cents, 100_000, "单价存万分之一元");
    assert_eq!(trade.fee_cents, 100);
    assert!(buy.convert.is_none() && buy.split.is_none());

    let sell = result
        .items
        .iter()
        .find(|r| r.kind == TransactionKind::Sell)
        .expect("卖出行应在场");
    let trade = sell.trade.as_ref().expect("sell 行应有买卖载荷");
    assert_eq!(trade.quantity, 5.0);
    assert_eq!(trade.price_cents, 100_000);
    assert_eq!(trade.fee_cents, 50);
    assert!(sell.convert.is_none() && sell.split.is_none());

    let convert = result
        .items
        .iter()
        .find(|r| r.kind == TransactionKind::Convert)
        .expect("转换行应在场");
    assert_eq!(
        convert.instrument_id, "inst-a",
        "公共标的地 = 转出腿（A → B 的 A）"
    );
    let leg = convert.convert.as_ref().expect("convert 行应有转入腿载荷");
    assert_eq!(leg.to_instrument_id, "inst-b");
    assert_eq!(leg.to_symbol, "MSFT");
    assert_eq!(leg.to_quantity, 1.0);
    assert_eq!(leg.out_amount_cents, 2_100, "转出金额（确认单口径）");
    assert_eq!(leg.in_amount_cents, 2_000, "转入金额（确认单口径）");
    assert!(
        convert.amount_cents != leg.out_amount_cents,
        "行金额锚点是结转成本，列表展示金额读本载荷（见 TransactionConvert 行金额锚点）"
    );
    assert!(convert.trade.is_none() && convert.split.is_none());

    let split_row = result
        .items
        .iter()
        .find(|r| r.kind == TransactionKind::Split)
        .expect("份额调整行应在场");
    let leg = split_row.split.as_ref().expect("split 行应有 Δ 载荷");
    assert_eq!(leg.delta_quantity, 2.0, "带符号份额增量 Δ 原样投影");
    assert!(split_row.trade.is_none() && split_row.convert.is_none());

    let dividend = result
        .items
        .iter()
        .find(|r| r.kind == TransactionKind::Dividend)
        .expect("分红行应在场");
    assert_eq!(dividend.amount_cents, 300, "现金腿金额");
    assert_eq!(
        dividend.account_id, "acc-bank",
        "到账账户即分红行的账户端（任意在用账户）"
    );
    assert_eq!(dividend.instrument_id, "inst-a", "分红归属标的不缺席");
    assert!(
        dividend.trade.is_none() && dividend.convert.is_none() && dividend.split.is_none(),
        "dividend 无 kind 专属载荷（现金腿与到账账户在公共字段）"
    );
}

/// 账户过滤的涉及账户语义（ADR-0135 决策 3）：投资账户 ∪ 出资账户（buy/sell，
/// ADR-0096）∪ 到账账户（dividend 行的账户端）；投资 kind 无转入侧（写入守卫恒拒）。
#[test]
fn account_filter_hits_funding_and_arrival_ends() {
    let conn = open();
    seed_account(&conn, "acc-f1", "券商户", "investment", "CNY", 0);
    seed_account(&conn, "acc-fund", "出资卡", "bank", "CNY", 50_000);
    seed_instrument(&conn, "inst-f", "TSLA", "特斯拉", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        TransactionInput {
            funding_account_id: Some("acc-fund".into()),
            ..make_buy_input("acc-f1", "inst-f", 3.0, 200_000, 0)
        },
    )
    .unwrap();
    // 分红到账出资卡（非投资账户）——到账账户端命中前提。
    create_transaction_internal(&conn, make_dividend_input("acc-fund", "inst-f", 500, "CNY"))
        .unwrap();

    let f = |account_id: &str| InvestmentTransactionListFilter {
        account_id: Some(account_id.into()),
        ..Default::default()
    };
    let by_funding = list_investment_transactions(&conn, &f("acc-fund")).unwrap();
    assert_eq!(
        by_funding.total, 2,
        "出资端（buy）∪ 到账端（dividend）双命中"
    );
    assert_eq!(by_funding.items.len(), 2);

    let by_investment = list_investment_transactions(&conn, &f("acc-f1")).unwrap();
    assert_eq!(by_investment.total, 1, "投资账户端命中 buy 行");
    assert_eq!(by_investment.items[0].kind, TransactionKind::Buy);
}

/// 标的过滤两腿命中口径（与时点持仓推算认 convert 两腿对齐）：转出腿或 convert
/// 转入腿任一命中即算。
#[test]
fn instrument_filter_matches_convert_both_legs() {
    let conn = open();
    seed_account(&conn, "acc-inst", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "AAPL", "苹果", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "MSFT", "微软", "CNY", "unknown");
    seed_instrument(&conn, "inst-c", "ORCL", "甲骨文", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inst", "inst-a", 10.0, 100_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input("acc-inst", "inst-a", "inst-b", 4.0, 3.0, 4_000, 3_800, 0),
    )
    .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-inst", "inst-c", 1.0, 100_000, 0))
        .unwrap();

    let f = |instrument_id: &str| InvestmentTransactionListFilter {
        instrument_id: Some(instrument_id.into()),
        ..Default::default()
    };
    assert_eq!(
        list_investment_transactions(&conn, &f("inst-a"))
            .unwrap()
            .total,
        2,
        "转出腿命中（buy + convert）"
    );
    assert_eq!(
        list_investment_transactions(&conn, &f("inst-b"))
            .unwrap()
            .total,
        1,
        "convert 转入腿命中即算"
    );
    assert_eq!(
        list_investment_transactions(&conn, &f("inst-c"))
            .unwrap()
            .total,
        1
    );
    // 无任何流水命中的标的：空集而非报错。
    seed_instrument(&conn, "inst-d", "NVDA", "英伟达", "CNY", "unknown");
    let empty = list_investment_transactions(&conn, &f("inst-d")).unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.items.is_empty());
}

/// 类型子集多选（维度内取或、与其余维度 AND 组合）；空集合视为未携带；
/// 通用 kind 不入行集闭包（传了也只是空集，不报错）。
#[test]
fn kind_subset_filter_and_empty_means_unfiltered() {
    let conn = open();
    seed_five_kinds(&conn);

    let f = |kinds: &[TransactionKind]| InvestmentTransactionListFilter {
        kinds: Some(kinds.to_vec()),
        ..Default::default()
    };
    let only_buy = list_investment_transactions(&conn, &f(&[TransactionKind::Buy])).unwrap();
    assert_eq!(only_buy.total, 1);
    assert_eq!(only_buy.items[0].kind, TransactionKind::Buy);

    let buy_and_dividend = list_investment_transactions(
        &conn,
        &f(&[TransactionKind::Buy, TransactionKind::Dividend]),
    )
    .unwrap();
    assert_eq!(buy_and_dividend.total, 2, "维度内取或");

    let all = list_investment_transactions(&conn, &f(&[])).unwrap();
    assert_eq!(all.total, 5, "空集合视为未携带（不过滤）");

    let generic = list_investment_transactions(&conn, &f(&[TransactionKind::Expense])).unwrap();
    assert_eq!(
        generic.total, 0,
        "行集闭包在五种投资 kind 内，通用 kind 恒空集"
    );
}

/// 日期区间双端有界（含边界日）。
#[test]
fn date_range_filter_is_bounded_both_ends() {
    let conn = open();
    seed_five_kinds(&conn);

    let f = |from: Option<&str>, to: Option<&str>| InvestmentTransactionListFilter {
        from: from.map(str::to_string),
        to: to.map(str::to_string),
        ..Default::default()
    };
    let february = list_investment_transactions(&conn, &f(Some("2026-02-01"), None)).unwrap();
    assert_eq!(february.total, 3, "convert/split/dividend 落在 2 月");
    let january = list_investment_transactions(&conn, &f(None, Some("2026-01-20"))).unwrap();
    assert_eq!(january.total, 2, "buy/sell 落在 1 月（to 含边界日）");
    let mid =
        list_investment_transactions(&conn, &f(Some("2026-01-20"), Some("2026-02-01"))).unwrap();
    assert_eq!(
        mid.total, 3,
        "双端有界：sell + convert + split（均含边界日）"
    );
}

/// 排序 date 倒序（与主列表同构）+ offset 分页返回 items + total；
/// 同日行按 created_at、id 同秒 tiebreaker 确定性翻页（无重复无遗漏）。
#[test]
fn sort_desc_and_offset_pagination_cover_all_without_overlap() {
    let conn = open();
    let ids = seed_five_kinds(&conn);

    let result = list_investment_transactions(
        &conn,
        &InvestmentTransactionListFilter {
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.total, 5, "total 恒为满足条件的总数，与页无关");
    // 首页两条：date 最大的 dividend（02-10）与 02-01 两条之一（同日 tiebreak 不断言序）。
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].kind, TransactionKind::Dividend);

    let page2 = list_investment_transactions(
        &conn,
        &InvestmentTransactionListFilter {
            page: Some(2),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    let page3 = list_investment_transactions(
        &conn,
        &InvestmentTransactionListFilter {
            page: Some(3),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.items.len(), 2);
    assert_eq!(page3.items.len(), 1, "末页返回余数行");

    // 翻页无重复无遗漏：三页并集 = 全部 id。
    let mut page_ids: Vec<String> = result
        .items
        .iter()
        .chain(&page2.items)
        .chain(&page3.items)
        .map(|r| r.id.clone())
        .collect();
    page_ids.sort();
    let mut expected = ids;
    expected.sort();
    assert_eq!(page_ids, expected, "三页并集应恰为全部五行");

    // 超范围页码：空页、total 不变。
    let beyond = list_investment_transactions(
        &conn,
        &InvestmentTransactionListFilter {
            page: Some(99),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(beyond.items.len(), 0);
    assert_eq!(beyond.total, 5);
}

/// 软删行排除（is_deleted=0）：删除后的买入不再出现在明细列表，total 同步回落。
#[test]
fn soft_deleted_rows_are_excluded() {
    let conn = open();
    seed_account(&conn, "acc-del", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-del", "DELL", "戴尔", "CNY", "unknown");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-del", "inst-del", 1.0, 100_000, 0),
    )
    .unwrap()
    .id;

    let before =
        list_investment_transactions(&conn, &InvestmentTransactionListFilter::default()).unwrap();
    assert_eq!(before.total, 1);

    delete_transaction_internal(&conn, &buy_id).unwrap();
    let after =
        list_investment_transactions(&conn, &InvestmentTransactionListFilter::default()).unwrap();
    assert_eq!(after.total, 0, "软删行排除");
    assert!(after.items.is_empty());
}

/// 页与总数必须同快照（#1699 / #1702 同根因的多语句读闭包）：探针在 items 语句
/// 开始前于另一连接把一行买入置软删——
/// - 读闭包无快照保护（红）：COUNT 读旧值（1）、items 读新值（0 行），页与总数错位；
/// - 读闭包收进读事务（绿）：注入写被挡住，COUNT 与 items 同见一套数。
#[test]
fn page_and_total_share_one_snapshot() {
    let dir = ScratchDir::new("investment-ledger-tab-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-snap", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-snap", "AAPL", "苹果", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-snap", "inst-snap", 1.0, 100_000, 0),
    )
    .unwrap();

    // 探针：items 语句（以投影列首段为 marker，与 COUNT 语句区分）开始前，另一
    // 连接提交软删写。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT t.id, t.kind",
        &["UPDATE transactions SET is_deleted=1 WHERE kind='buy'"],
    );
    let result = list_investment_transactions(
        &conn,
        &InvestmentTransactionListFilter {
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中 items 语句（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        result.items.len() as i64,
        result.total,
        "页与总数必须同快照（COUNT 与 items 之间注入写不得造成错位）"
    );
    assert_eq!(
        result.total, 1,
        "注入写应整体落在快照之外（否则口径断言空转）"
    );
}
