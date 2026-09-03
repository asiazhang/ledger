use std::collections::HashSet;

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::db::new_uuid;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{TransactionInput, TransactionListFilter};
use tauri_app_lib::transaction::{create_transaction_internal, list_transactions_internal};

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// When：创建带分类引用的交易（issue #377 分类下钻场景用）
// ---------------------------------------------------------------------------

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 分类 {string}")]
fn create_txn_with_category(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    category_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: Some(world.category_id(&category_name)),
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
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建，提交点置脏/到期检查。
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(result.is_ok(), "创建带分类交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// 服务端分页（issue-32：items + total，offset 页码模式）
// ---------------------------------------------------------------------------

/// 同日批量导入：直接写库模拟"同一批导入每批一个时间戳"（全部行 created_at 相同，
/// 只有 id tiebreaker 能区分排序），用于验证确定性排序下翻页无重复无遗漏。
#[when(expr = "批量导入 {int} 笔同日交易 日期 {string} 到账户 {string}")]
fn batch_import_same_day(world: &mut LedgerWorld, count: i64, date: String, account_name: String) {
    let account_id = world.account_id(&account_name);
    for i in 0..count {
        let id = new_uuid();
        let amount = (i + 1) * 100;
        world_conn!(world)
            .execute(
                "INSERT INTO transactions \
                 (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
                 category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
                 VALUES (?1,'expense',?2,'CNY',?2,?3,NULL,NULL,NULL,NULL,?4,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
                params![id, amount, account_id, date],
            )
            .unwrap();
    }
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 执行分页查询并断言当前页条数与 total，快照 items 供后续步骤使用。
/// 四个 `check_page*` step 共用（避免重复的断言形状）。
fn assert_paged(
    world: &mut LedgerWorld,
    filter: TransactionListFilter,
    expected_count: i64,
    expected_total: i64,
    label: &str,
) {
    let result = list_transactions_internal(&world_conn!(world), &filter).expect("分页查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected_count,
        "{label} 返回条数不匹配"
    );
    assert_eq!(result.total, expected_total, "{label} total 不匹配");
    world.transactions_list = result.items;
}

#[then(expr = "分页查询 page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page(
    world: &mut LedgerWorld,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("page={page} page_size={page_size}"),
    );
}

#[then(expr = "分页查询 账户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_account(
    world: &mut LedgerWorld,
    account_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let account_id = world.account_id(&account_name);
    assert_paged(
        world,
        TransactionListFilter {
            account_id: Some(account_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("账户 '{account_name}' page={page}"),
    );
}

#[then(expr = "分页查询 涉及账户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_involving_account(
    world: &mut LedgerWorld,
    account_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let account_id = world.account_id(&account_name);
    assert_paged(
        world,
        TransactionListFilter {
            involving_account_id: Some(account_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("涉及账户 '{account_name}' page={page}"),
    );
}

#[then(expr = "分页查询 kind {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_kind(
    world: &mut LedgerWorld,
    kind: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            kind: Some(
                TransactionKind::parse(&kind)
                    .unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
            ),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("kind 过滤后 page={page}"),
    );
}

#[then(
    expr = "分页查询 日期 {string} 至 {string} page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
fn check_page_date(
    world: &mut LedgerWorld,
    from: String,
    to: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            from: Some(from.clone()),
            to: Some(to.clone()),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("日期区间 [{from}, {to}] page={page}"),
    );
}

#[then(expr = "分页查询 商户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_merchant(
    world: &mut LedgerWorld,
    merchant_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let merchant_id = world.merchant_id(&merchant_name);
    assert_paged(
        world,
        TransactionListFilter {
            merchant_id: Some(merchant_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("商户 '{merchant_name}' page={page}"),
    );
}

#[then(
    expr = "分页查询 商户 {string} 涉及账户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn check_page_merchant_involving_account(
    world: &mut LedgerWorld,
    merchant_name: String,
    account_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let merchant_id = world.merchant_id(&merchant_name);
    let account_id = world.account_id(&account_name);
    assert_paged(
        world,
        TransactionListFilter {
            merchant_id: Some(merchant_id),
            involving_account_id: Some(account_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("商户 '{merchant_name}' + 涉及账户 '{account_name}' page={page}"),
    );
}

#[then(
    expr = "分页查询 商户 {string} 日期 {string} 至 {string} page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn check_page_merchant_date(
    world: &mut LedgerWorld,
    merchant_name: String,
    from: String,
    to: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let merchant_id = world.merchant_id(&merchant_name);
    assert_paged(
        world,
        TransactionListFilter {
            merchant_id: Some(merchant_id),
            from: Some(from.clone()),
            to: Some(to.clone()),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("商户 '{merchant_name}' 日期 {from}..{to} page={page}"),
    );
}

#[then(expr = "分页查询 分类 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_category(
    world: &mut LedgerWorld,
    category_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let category_id = world.category_id(&category_name);
    assert_paged(
        world,
        TransactionListFilter {
            category_id: Some(category_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("分类 '{category_name}' page={page}"),
    );
}

#[then(expr = "分页查询 仅无分类 page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_uncategorized(
    world: &mut LedgerWorld,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            uncategorized_only: Some(true),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("仅无分类 page={page}"),
    );
}

#[then(expr = "缺省查询 应返回 {int} 条 total {int}")]
fn check_default(world: &mut LedgerWorld, expected_count: i64, expected_total: i64) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("缺省查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected_count,
        "缺省查询应返回全部"
    );
    assert_eq!(result.total, expected_total, "缺省查询 total 不匹配");
    world.transactions_list = result.items;
}

#[then(expr = "读取 limit {int} 应返回 {int} 条")]
fn check_limit(world: &mut LedgerWorld, limit: i64, expected: i64) {
    let result = list_transactions_internal(
        &world_conn!(world),
        &TransactionListFilter {
            limit: Some(limit),
            ..Default::default()
        },
    )
    .expect("limit 查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected,
        "limit={limit} 返回条数不匹配"
    );
}

#[then(expr = "翻页 page_size {int} 应覆盖全部 {int} 条无重复无遗漏")]
fn check_pages_cover_all(world: &mut LedgerWorld, page_size: i64, expected_total: i64) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut page: usize = 1;
    loop {
        let result = list_transactions_internal(
            &world_conn!(world),
            &TransactionListFilter {
                page: Some(page),
                page_size: Some(page_size as usize),
                ..Default::default()
            },
        )
        .expect("翻页查询失败");
        assert_eq!(result.total, expected_total, "total 应保持过滤后总数");
        for t in &result.items {
            assert!(seen.insert(t.id.clone()), "翻页出现重复交易: {}", t.id);
        }
        if (result.items.len() as i64) < page_size {
            break;
        }
        page += 1;
    }
    assert_eq!(
        seen.len() as i64,
        expected_total,
        "翻页覆盖条数不匹配（有遗漏）"
    );
}
