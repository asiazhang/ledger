use chrono::{Datelike, Months, NaiveDate};
use cucumber::{given, then, when};
use rusqlite::params;

use ledger_reports::{
    category_shares_rows, merchant_shares_report, monthly_summary_rows, query_report_date_range,
};
use ledger_transaction::TransactionInput;
use ledger_transaction::create_transaction_internal;

use crate::common::query_all_transactions;
use crate::step_inputs::{expense_input, parse_kind, plain_input};
use crate::world::LedgerWorld;

fn scenario_today(world: &mut LedgerWorld) -> NaiveDate {
    *world
        .frozen_today
        .get_or_insert_with(|| chrono::Local::now().date_naive())
}

fn relative_year_offset(token: &str) -> i32 {
    match token {
        "前年" => -2,
        "去年" => -1,
        "今年" => 0,
        "明年" => 1,
        other => panic!("未知相对年份记号 '{other}'（支持 前年/去年/今年/明年）"),
    }
}

fn resolve_year_token(token: &str, today: NaiveDate) -> i64 {
    let offset = relative_year_offset(token);
    if offset >= 0 {
        i64::from(today.year() + offset)
    } else {
        let months = u32::try_from(-offset * 12).expect("年偏移过大");
        i64::from(
            today
                .checked_sub_months(Months::new(months))
                .expect("反推年份溢出")
                .year(),
        )
    }
}

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

#[given(expr = "{word}有一笔支出 {int} 到账户 {string}")]
fn create_expense_in_relative_year(
    world: &mut LedgerWorld,
    year_token: String,
    amount: i64,
    account_name: String,
) {
    let year = resolve_year_token(&year_token, scenario_today(world));
    let input = expense_input(
        amount,
        &world.account_id(&account_name),
        &format!("{year}-06-15"),
    );
    let result = world_write!(world, |conn| create_transaction_internal(conn, input));
    assert!(
        result.is_ok(),
        "创建 {year_token}支出失败: {:?}",
        result.err()
    );
    world.txn.last_transaction_id = Some(result.unwrap().id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

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
        currency_code: currency,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        ..plain_input(
            parse_kind(&kind),
            amount,
            &world.account_id(&account_name),
            &date,
        )
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.txn.last_transaction_id = Some(result.unwrap().id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "查询 {int} 年商户排行")]
fn query_merchant_shares(world: &mut LedgerWorld, year: i64) {
    world.report.last_merchant_shares =
        merchant_shares_report(&world_conn!(world), year, None, None, None)
            .expect("查询商户排行失败")
            .rows;
}

#[when(expr = "查询商户排行 期间 {string} 到 {string}")]
fn query_merchant_shares_period(world: &mut LedgerWorld, from: String, to: String) {
    world.report.last_merchant_shares =
        merchant_shares_report(&world_conn!(world), 0, Some(&from), Some(&to), None)
            .expect("查询商户排行失败")
            .rows;
}

#[then(expr = "商户排行应为 {int} 行")]
fn check_merchant_ranking_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.report.last_merchant_shares.len(),
        n,
        "排行行数不符：实际 {:?}",
        world
            .report
            .last_merchant_shares
            .iter()
            .map(|s| (s.merchant_name.as_str(), s.amount_cents))
            .collect::<Vec<_>>()
    );
}

#[then(expr = "商户排行第 {int} 名应为 {string} 金额 {int}")]
fn check_merchant_ranking_row(world: &mut LedgerWorld, index: usize, name: String, amount: i64) {
    let share = world
        .report
        .last_merchant_shares
        .get(index - 1)
        .unwrap_or_else(|| panic!("商户排行第 {index} 名不存在"));
    assert_eq!(share.merchant_name, name, "排行第 {index} 名商户不符");
    assert_eq!(share.amount_cents, amount, "商户 '{name}' 净支出不符");
}

#[then(expr = "商户排行响应 JSON 不含字段 {string}")]
fn check_merchant_shares_json_not_contain_field(world: &mut LedgerWorld, field: String) {
    assert!(
        !world.report.last_merchant_shares.is_empty(),
        "商户排行为空，无法校验响应字段契约"
    );
    for s in &world.report.last_merchant_shares {
        let json = serde_json::to_value(s).expect("商户排行行序列化失败");
        assert!(
            json.get(&field).is_none(),
            "商户排行响应不应含字段 '{field}'，实际: {json}"
        );
    }
}

#[when(expr = "查询报表日期范围")]
fn query_date_range_step(world: &mut LedgerWorld) {
    world.report.last_date_range =
        Some(query_report_date_range(&world_conn!(world)).expect("查询报表日期范围失败"));
}

#[then(expr = "报表日期范围应为 {string} 到 {string}")]
fn check_report_date_range(world: &mut LedgerWorld, min_token: String, max_token: String) {
    let today = scenario_today(world);
    let range = world
        .report
        .last_date_range
        .as_ref()
        .expect("未查询报表日期范围");
    let actual = (range.min_date.as_deref(), range.max_date.as_deref());
    let expected_min = resolve_date_token(&min_token, today);
    let expected_max = resolve_date_token(&max_token, today);
    assert_eq!(
        actual,
        (Some(expected_min.as_str()), Some(expected_max.as_str())),
        "报表日期范围不符：期望 {min_token}({expected_min}) 到 {max_token}({expected_max})"
    );
}

#[then(expr = "报表日期范围应为空")]
fn check_report_date_range_empty(world: &mut LedgerWorld) {
    let range = world
        .report
        .last_date_range
        .as_ref()
        .expect("未查询报表日期范围");
    assert_eq!(
        (range.min_date.as_deref(), range.max_date.as_deref()),
        (None, None),
        "空库报表日期范围应为双 null"
    );
}

fn expense_category_id(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND kind='expense' AND is_deleted=0",
        params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("支出分类 '{name}' 不存在: {e}"))
}

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
        category_id: Some(expense_category_id(&world_conn!(world), &category_name)),
        ..plain_input(
            parse_kind(&kind),
            amount,
            &world.account_id(&account_name),
            &date,
        )
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.txn.last_transaction_id = Some(result.unwrap().id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "查询 {int} 年分类份额")]
fn query_category_shares(world: &mut LedgerWorld, year: i64) {
    world.report.last_category_shares =
        category_shares_rows(&world_conn!(world), "expense", None, Some(year), None, None)
            .expect("查询分类份额失败");
}

#[when(expr = "查询分类份额 全时段")]
fn query_category_shares_all_time(world: &mut LedgerWorld) {
    world.report.last_category_shares =
        category_shares_rows(&world_conn!(world), "expense", None, None, None, None)
            .expect("查询分类份额失败");
}

#[when(expr = "查询分类份额 期间 {string} 到 {string}")]
fn query_category_shares_period(world: &mut LedgerWorld, from: String, to: String) {
    world.report.last_category_shares = category_shares_rows(
        &world_conn!(world),
        "expense",
        None,
        None,
        Some(&from),
        Some(&to),
    )
    .expect("查询分类份额失败");
}

#[when(expr = "查询月度汇总 期间 {string} 到 {string}")]
fn query_monthly_summary_period(world: &mut LedgerWorld, from: String, to: String) {
    world.report.last_monthly_summary =
        monthly_summary_rows(&world_conn!(world), 0, Some(&from), Some(&to))
            .expect("查询月度汇总失败");
}

#[when(expr = "查询 {int} 年月度汇总")]
fn query_monthly_summary_year(world: &mut LedgerWorld, year: i64) {
    world.report.last_monthly_summary =
        monthly_summary_rows(&world_conn!(world), year, None, None).expect("查询月度汇总失败");
}

#[then(expr = "月度汇总应为 {int} 行")]
fn check_monthly_summary_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.report.last_monthly_summary.len(),
        n,
        "月度汇总行数不符：实际 {:?}",
        world.report.last_monthly_summary
    );
}

#[then(expr = "月度汇总第 {int} 行应为月份 {string} 收入 {int} 支出 {int} 退款 {int}")]
fn check_monthly_summary_row(
    world: &mut LedgerWorld,
    index: usize,
    month: String,
    income: i64,
    expense: i64,
    refund: i64,
) {
    let row = world
        .report
        .last_monthly_summary
        .get(index - 1)
        .unwrap_or_else(|| panic!("月度汇总第 {index} 行不存在"));
    assert_eq!(row.month, month, "月度汇总第 {index} 行月份不符");
    assert_eq!(
        (row.income_cents, row.expense_cents, row.refund_cents),
        (income, expense, refund),
        "月度汇总 '{month}' 毛值三列不符"
    );
}

#[then(expr = "分类份额应为 {int} 行")]
fn check_category_shares_len(world: &mut LedgerWorld, n: usize) {
    assert_eq!(
        world.report.last_category_shares.len(),
        n,
        "分类份额行数不符：实际 {:?}",
        world
            .report
            .last_category_shares
            .iter()
            .map(|s| (s.category_name.as_str(), s.amount_cents))
            .collect::<Vec<_>>()
    );
}

#[then(expr = "分类份额第 {int} 名应为 {string} 金额 {int}")]
fn check_category_shares_row(world: &mut LedgerWorld, index: usize, name: String, amount: i64) {
    let share = world
        .report
        .last_category_shares
        .get(index - 1)
        .unwrap_or_else(|| panic!("分类份额第 {index} 名不存在"));
    assert_eq!(share.category_name, name, "分类份额第 {index} 名分类不符");
    assert_eq!(share.amount_cents, amount, "分类 '{name}' 净额不符");
}
