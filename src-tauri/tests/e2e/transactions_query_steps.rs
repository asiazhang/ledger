use std::collections::HashSet;

use cucumber::{then, when};
use rusqlite::params;

use ledger_infra::db::new_uuid;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::list_transactions_internal;
use ledger_transaction::{TransactionInput, TransactionListFilter};

use crate::common::query_all_transactions;
use crate::step_inputs::{parse_kind, plain_input};
use crate::step_verbs::create_transaction_verb;
use crate::world::LedgerWorld;

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 分类 {string}")]
fn create_txn_with_category(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    category_name: String,
) {
    let account_id = world.account_id(&account_name);
    let input = TransactionInput {
        category_id: Some(world.category_id(&category_name)),
        ..plain_input(parse_kind(&kind), amount, &account_id, &date)
    };
    create_transaction_verb(world, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

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
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

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
    world.txn.transactions_list = result.items;
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
            kinds: Some(vec![
                TransactionKind::parse(&kind)
                    .unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
            ]),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("kind（经集合参数）过滤后 page={page}"),
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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

fn parse_kinds(raw: &str) -> Vec<TransactionKind> {
    raw.split(',')
        .map(|k| TransactionKind::parse(k).unwrap_or_else(|e| panic!("非法 kind: {k}（{e}）")))
        .collect()
}

#[when(expr = "播种 9 类交易各带分类与无分类 日期 {string} 到账户 {string} 分类 {string}")]
fn seed_kinds_with_and_without_category(
    world: &mut LedgerWorld,
    date: String,
    account_name: String,
    category_name: String,
) {
    let account_id = world.account_id(&account_name);
    let category_id = world.category_id(&category_name);
    let conn = world_conn!(world);
    for kind in TransactionKind::ALL {
        for cat in [Some(category_id.as_str()), None] {
            let id = new_uuid();
            conn.execute(
                "INSERT INTO transactions \
                 (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
                 category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
                 VALUES (?1,?2,100,'CNY',100,?3,NULL,?4,NULL,NULL,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
                params![id, kind.as_str(), account_id, cat, date],
            )
            .unwrap();
        }
    }
    world.txn.transactions_list = query_all_transactions(&conn);
}

#[then(expr = "分页查询 类型集合 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_kinds(
    world: &mut LedgerWorld,
    kinds_raw: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let kinds = parse_kinds(&kinds_raw);
    assert_paged(
        world,
        TransactionListFilter {
            kinds: Some(kinds),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("类型集合 '{kinds_raw}' page={page}"),
    );
}

#[then(
    expr = "分页查询 分类 {string} 类型集合 {string} page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
fn check_page_category_kinds(
    world: &mut LedgerWorld,
    category_name: String,
    kinds_raw: String,
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
            kinds: Some(parse_kinds(&kinds_raw)),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("分类 '{category_name}' + 类型集合 '{kinds_raw}' page={page}"),
    );
}

#[then(expr = "列表行应同时含类型 {string} 与 {string}")]
fn check_rows_contain_both_kinds(world: &mut LedgerWorld, kind_a: String, kind_b: String) {
    let parse =
        |k: &str| TransactionKind::parse(k).unwrap_or_else(|e| panic!("非法 kind: {k}（{e}）"));
    let (a, b) = (parse(&kind_a), parse(&kind_b));
    assert!(
        world.txn.transactions_list.iter().any(|t| t.kind == a),
        "列表应含 {kind_a} 行"
    );
    assert!(
        world.txn.transactions_list.iter().any(|t| t.kind == b),
        "列表应含 {kind_b} 行"
    );
}

#[then(
    expr = "分页查询 类型集合 {string} 仅无分类 page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
fn check_page_kinds_uncategorized(
    world: &mut LedgerWorld,
    kinds_raw: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let kinds = parse_kinds(&kinds_raw);
    assert_paged(
        world,
        TransactionListFilter {
            kinds: Some(kinds),
            uncategorized_only: Some(true),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("类型集合 '{kinds_raw}' + 仅无分类 page={page}"),
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
    world.txn.transactions_list = result.items;
}

#[then(expr = "缺省查询应覆盖全部 {int} 类交易")]
fn check_default_covers_all_kinds(world: &mut LedgerWorld, expected_kinds: i64) {
    let all = TransactionKind::ALL;
    assert_eq!(
        all.len() as i64,
        expected_kinds,
        "kind 闭集数与步骤参数不一致（闭集演化后请同步步骤参数）"
    );
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("缺省查询失败");
    for kind in all {
        assert!(
            result.items.iter().any(|t| t.kind == kind),
            "缺省查询（无 kinds 参数）应包含 {kind} 行——无 kinds 参数返回全部 kind 的契约被破坏"
        );
    }
    world.txn.transactions_list = result.items;
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
