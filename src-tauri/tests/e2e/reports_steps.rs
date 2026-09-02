//! 报表 e2e 步骤定义。
//!
//! 商户消费排行（issue #192）：排行口径由核心函数 `merchant_shares_rows`
//! （命令层同款注入）查询：`expense_net`（毛支出 − 退款）按商户聚合、本位币口径。
//! 交易夹具走与真实写路径一致的行为层（`create_transaction_internal`），
//! 复用商户/交易步骤模块的既有步骤。
//!
//! 日期筛选范围（issue #266 / #389）：范围由核心函数 `query_report_date_range`
//! （命令层同款注入）查询；「今年/去年/前年/明年」等相对日期记号以场景冻结的
//! 本地今日推算（同预算步骤先例），场景在任何日期运行都成立。
//!
//! 分类份额年份联动（issue #376）：口径由核心函数 `category_shares_rows`
//! （命令层同款注入）查询：`expense_net` 净值、本位币口径，可选年份过滤；
//! 带分类交易夹具走与真实写路径一致的行为层，退款复用既有步骤（继承原分类）。

use chrono::{Datelike, Months, NaiveDate};
use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::reports::{
    category_shares_rows, merchant_shares_rows, query_report_date_range,
};
use tauri_app_lib::commands::transactions::create_transaction_internal;
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 夹具工具
// ---------------------------------------------------------------------------

/// 本地今日（与命令层 `report_year_range` 注入口径一致）。
/// 每个场景首次调用时冻结，之后整个场景复用同一 today（同预算步骤先例）。
fn scenario_today(world: &mut LedgerWorld) -> NaiveDate {
    *world
        .frozen_today
        .get_or_insert_with(|| chrono::Local::now().date_naive())
}

/// 相对年份记号 → 距冻结今日的年偏移（年范围场景夹具与断言用）。
fn relative_year_offset(token: &str) -> i32 {
    match token {
        "前年" => -2,
        "去年" => -1,
        "今年" => 0,
        "明年" => 1,
        other => panic!("未知相对年份记号 '{other}'（支持 前年/去年/今年/明年）"),
    }
}

/// 相对年份记号 → 实际年份（以冻结今日为基准）。
fn resolve_year_token(token: &str, today: NaiveDate) -> i64 {
    let offset = relative_year_offset(token);
    if offset >= 0 {
        i64::from(today.year() + offset)
    } else {
        // add_months 不接受负数，过去年份用减月反推（月内日期不变，无跨年歧义）。
        let months = u32::try_from(-offset * 12).expect("年偏移过大");
        i64::from(
            today
                .checked_sub_months(Months::new(months))
                .expect("反推年份溢出")
                .year(),
        )
    }
}

/// 相对日期记号 → 实际日期字符串（YYYY-MM-DD）。
/// 支持纯相对年份（前年/去年/今年/明年，默认年中 06-15，与夹具同日）或直接 YYYY-MM-DD。
fn resolve_date_token(token: &str, today: NaiveDate) -> String {
    if let Some((yt, rest)) = token.split_once('-')
        && matches!(yt, "前年" | "去年" | "今年" | "明年")
    {
        let y = resolve_year_token(yt, today);
        return format!("{y}-{rest}");
    }
    match token {
        "前年" | "去年" | "今年" | "明年" => {
            let y = resolve_year_token(token, today);
            format!("{y}-06-15")
        }
        exact => exact.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 某相对年份（前年/去年/今年/明年）固定年中日期（06-15）的一笔支出夹具。
#[given(expr = "{word}有一笔支出 {int} 到账户 {string}")]
fn create_expense_in_relative_year(
    world: &mut LedgerWorld,
    year_token: String,
    amount: i64,
    account_name: String,
) {
    let year = resolve_year_token(&year_token, scenario_today(world));
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: format!("{year}-06-15"),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建，提交点置脏/到期检查。
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(
        result.is_ok(),
        "创建 {year_token}支出失败: {:?}",
        result.err()
    );
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 创建带商户的跨币种交易（本位币折算由写路径 `convert_to_native` 完成）。
#[when(
    expr = "创建交易 类型 {string} 金额 {int} 币种 {string} 到账户 {string} 日期 {string} 商户 {string}"
)]
fn create_txn_with_merchant_currency(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    currency: String,
    account_name: String,
    date: String,
    merchant_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: currency,
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 查询指定年份的商户消费排行（命令层同款核心函数注入）。
#[when(expr = "查询 {int} 年商户排行")]
fn query_merchant_shares(world: &mut LedgerWorld, year: i64) {
    world.last_merchant_shares =
        merchant_shares_rows(&world_conn!(world), year).expect("查询商户排行失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 排行行数断言。
#[then(expr = "商户排行应为 {int} 行")]
fn check_merchant_ranking_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.last_merchant_shares.len(),
        n,
        "排行行数不符：实际 {:?}",
        world
            .last_merchant_shares
            .iter()
            .map(|s| (s.merchant_name.as_str(), s.amount_cents))
            .collect::<Vec<_>>()
    );
}

/// 排行第 {index} 名断言：商户名（现名，改名即时生效）+ 本位币净支出，
/// 顺序即排行顺序（净额降序）。
#[then(expr = "商户排行第 {int} 名应为 {string} 金额 {int}")]
fn check_merchant_ranking_row(world: &mut LedgerWorld, index: usize, name: String, amount: i64) {
    let share = world
        .last_merchant_shares
        .get(index - 1)
        .unwrap_or_else(|| panic!("商户排行第 {index} 名不存在"));
    assert_eq!(share.merchant_name, name, "排行第 {index} 名商户不符");
    assert_eq!(share.amount_cents, amount, "商户 '{name}' 净支出不符");
}

/// 商户契约回归「名字字典」（issue #223）：排行响应序列化后不应再含指定字段
/// （icon/color 已退役；排行行只含名称与金额）。
#[then(expr = "商户排行响应 JSON 不含字段 {string}")]
fn check_merchant_shares_json_not_contain_field(world: &mut LedgerWorld, field: String) {
    assert!(
        !world.last_merchant_shares.is_empty(),
        "商户排行为空，无法校验响应字段契约"
    );
    for s in &world.last_merchant_shares {
        let json = serde_json::to_value(s).expect("商户排行行序列化失败");
        assert!(
            json.get(&field).is_none(),
            "商户排行响应不应含字段 '{field}'，实际: {json}"
        );
    }
}

// ---------------------------------------------------------------------------
// 日期筛选范围（issue #266 / #389）
// ---------------------------------------------------------------------------

/// 查询报表日期筛选范围（命令层同款核心函数注入）。
#[when(expr = "查询报表日期范围")]
fn query_date_range_step(world: &mut LedgerWorld) {
    world.last_date_range =
        Some(query_report_date_range(&world_conn!(world)).expect("查询报表日期范围失败"));
}

/// 日期范围断言：两端以相对记号（前年/去年/今年/明年）或实际日期表述，
/// 由冻结今日推算实际日期后比对。
#[then(expr = "报表日期范围应为 {string} 到 {string}")]
fn check_report_date_range(world: &mut LedgerWorld, min_token: String, max_token: String) {
    let today = scenario_today(world);
    let range = world.last_date_range.as_ref().expect("未查询报表日期范围");
    let actual = (range.min_date.as_deref(), range.max_date.as_deref());
    let expected_min = resolve_date_token(&min_token, today);
    let expected_max = resolve_date_token(&max_token, today);
    assert_eq!(
        actual,
        (Some(expected_min.as_str()), Some(expected_max.as_str())),
        "报表日期范围不符：期望 {min_token}({expected_min}) 到 {max_token}({expected_max})"
    );
}

/// 空库或软删后无交易时的日期范围断言（双 None / null）。
#[then(expr = "报表日期范围应为空")]
fn check_report_date_range_empty(world: &mut LedgerWorld) {
    let range = world.last_date_range.as_ref().expect("未查询报表日期范围");
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (None, None),
        "空库报表日期范围应为双 null"
    );
}

// ---------------------------------------------------------------------------
// 分类份额年份联动（issue #376）
// ---------------------------------------------------------------------------

/// 支出分类名 → id（夹具由既有步骤「存在支出分类」插入，同预算步骤的查找方式）。
fn expense_category_id(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND kind='expense' AND is_deleted=0",
        params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("支出分类 '{name}' 不存在: {e}"))
}

/// 创建带分类的交易（行为层落库，与真实写路径一致；退款继承原分类由 Writer 保证）。
#[when(expr = "创建交易 类型 {string} 金额 {int} 分类 {string} 到账户 {string} 日期 {string}")]
fn create_txn_with_category(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    category_name: String,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: Some(expense_category_id(&world_conn!(world), &category_name)),
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 查询指定年份的支出分类份额（命令层同款核心函数注入，年份联动口径）。
#[when(expr = "查询 {int} 年分类份额")]
fn query_category_shares(world: &mut LedgerWorld, year: i64) {
    world.last_category_shares =
        category_shares_rows(&world_conn!(world), "expense", None, Some(year))
            .expect("查询分类份额失败");
}

/// 缺省年份查询（全时段口径）：既有调用方不回归的回归锁定。
#[when(expr = "查询分类份额 全时段")]
fn query_category_shares_all_time(world: &mut LedgerWorld) {
    world.last_category_shares =
        category_shares_rows(&world_conn!(world), "expense", None, None).expect("查询分类份额失败");
}

/// 分类份额行数断言。
#[then(expr = "分类份额应为 {int} 行")]
fn check_category_shares_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.last_category_shares.len(),
        n,
        "分类份额行数不符：实际 {:?}",
        world
            .last_category_shares
            .iter()
            .map(|s| (s.category_name.as_str(), s.amount_cents))
            .collect::<Vec<_>>()
    );
}

/// 分类份额第 {index} 名断言：分类名（现名，未分类行为「未分类」）+ 本位币净额，
/// 顺序即净额降序。
#[then(expr = "分类份额第 {int} 名应为 {string} 金额 {int}")]
fn check_category_shares_row(world: &mut LedgerWorld, index: usize, name: String, amount: i64) {
    let share = world
        .last_category_shares
        .get(index - 1)
        .unwrap_or_else(|| panic!("分类份额第 {index} 名不存在"));
    assert_eq!(share.category_name, name, "分类份额第 {index} 名分类不符");
    assert_eq!(share.amount_cents, amount, "分类 '{name}' 净额不符");
}
