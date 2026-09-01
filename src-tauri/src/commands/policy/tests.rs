//! `commands::policy` 单元测试：建档校验语义、软删保留与失效信号回调
//! （BDD 场景外的快速反馈；外部可观察行为的验收在 BDD `policies.feature`）。

use rusqlite::Connection;

use crate::commands::merchants::create_merchant_internal;
use crate::commands::policy::{
    create_policy_internal, delete_policy_internal, list_policies_internal, update_policy_internal,
};
use crate::db::{init_db, open_in_memory};
use crate::models::{MerchantInput, PolicyInput};

fn conn() -> Connection {
    let mut conn = open_in_memory().expect("内存库创建失败");
    init_db(&mut conn).expect("迁移失败");
    conn
}

fn input(merchant_id: &str) -> PolicyInput {
    PolicyInput {
        merchant_id: merchant_id.into(),
        policy_number: "P2026-001".into(),
        product_name: "重疾险".into(),
        start_date: "2026-01-01".into(),
        end_date: Some("2036-01-01".into()),
        coverage_amount_cents: Some(30_000_000),
        coverage_currency_code: Some("CNY".into()),
        note: Some(" 50 万保额 ".into()),
    }
}

fn seed_merchant(conn: &Connection, name: &str) -> String {
    create_merchant_internal(conn, MerchantInput { name: name.into() }).expect("创建商户失败")
}

fn create_ok(conn: &Connection, input: PolicyInput) -> String {
    create_policy_internal(conn, input, &mut || {}).expect("创建保单失败")
}

#[test]
fn 创建保单并读回全字段() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let id = create_ok(&conn, input(&merchant_id));
    let list = list_policies_internal(&conn).unwrap();
    assert_eq!(list.len(), 1);
    let policy = &list[0];
    assert_eq!(policy.id, id);
    assert_eq!(policy.merchant_id, merchant_id);
    assert_eq!(policy.policy_number, "P2026-001");
    assert_eq!(policy.product_name, "重疾险");
    assert_eq!(policy.start_date, "2026-01-01");
    assert_eq!(policy.end_date.as_deref(), Some("2036-01-01"));
    assert_eq!(policy.coverage_amount_cents, Some(30_000_000));
    assert_eq!(policy.coverage_currency_code.as_deref(), Some("CNY"));
    // 备注 trim 后保留；审计字段齐全
    assert_eq!(policy.note.as_deref(), Some("50 万保额"));
    assert!(!policy.created_at.is_empty());
    assert_eq!(policy.version, 1);
    assert!(!policy.is_deleted);
}

#[test]
fn 止日为空建档成功且保额币种成对存空() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let mut input = input(&merchant_id);
    input.end_date = None;
    input.coverage_amount_cents = None;
    // 保额缺省时币种忽略存空（成对原子）
    input.coverage_currency_code = Some("USD".into());
    input.note = Some("   ".into());
    create_ok(&conn, input);
    let policy = &list_policies_internal(&conn).unwrap()[0];
    assert_eq!(policy.end_date, None);
    assert_eq!(policy.coverage_amount_cents, None);
    assert_eq!(policy.coverage_currency_code, None);
    assert_eq!(policy.note, None);
}

#[test]
fn 创建成功发失效信号_失败不发() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let mut signals = 0;
    let ok_input = input(&merchant_id);
    create_policy_internal(&conn, ok_input, &mut || signals += 1).unwrap();
    assert_eq!(signals, 1);

    let mut signals_err = 0;
    let bad = input("不存在的商户");
    let err = create_policy_internal(&conn, bad, &mut || signals_err += 1).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
    assert_eq!(signals_err, 0);
}

#[test]
fn 编辑保单_审计字段保留() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let id = create_ok(&conn, input(&merchant_id));
    let created_at = list_policies_internal(&conn).unwrap()[0].created_at.clone();

    let merchant2 = seed_merchant(&conn, "太平洋保险");
    let mut input = input(&merchant2);
    input.policy_number = "P2026-002".into();
    input.product_name = "医疗险".into();
    input.start_date = "2026-02-01".into();
    input.end_date = None;
    input.coverage_amount_cents = None;
    input.coverage_currency_code = None;
    input.note = None;
    update_policy_internal(&conn, &id, input, &mut || {}).unwrap();

    let policy = &list_policies_internal(&conn).unwrap()[0];
    assert_eq!(policy.merchant_id, merchant2);
    assert_eq!(policy.policy_number, "P2026-002");
    assert_eq!(policy.product_name, "医疗险");
    assert_eq!(policy.end_date, None);
    assert_eq!(policy.coverage_amount_cents, None);
    assert_eq!(policy.version, 2);
    assert_eq!(policy.created_at, created_at, "created_at 保留");
}

#[test]
fn 软删后不进列表且库内行引用保留不置空() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let id = create_ok(&conn, input(&merchant_id));
    delete_policy_internal(&conn, &id, &mut || {}).unwrap();

    assert!(list_policies_internal(&conn).unwrap().is_empty());
    // 库内行保留：is_deleted=1，保司引用等列原样（历史引用保留不置空，ADR-0051 决策 5）
    let (is_deleted, kept_merchant, kept_number): (i64, String, String) = conn
        .query_row(
            "SELECT is_deleted, merchant_id, policy_number FROM policies WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(is_deleted, 1);
    assert_eq!(kept_merchant, merchant_id);
    assert_eq!(kept_number, "P2026-001");
}

#[test]
fn 已删保单再编辑再删均报不存在() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let id = create_ok(&conn, input(&merchant_id));
    delete_policy_internal(&conn, &id, &mut || {}).unwrap();

    let err = update_policy_internal(&conn, &id, input(&merchant_id), &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保单不存在"));
    let err = delete_policy_internal(&conn, &id, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保单不存在"));
}

#[test]
fn 编辑时保司未变_软删保司维持历史引用可继续编辑() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    let id = create_ok(&conn, input(&merchant_id));
    // 建档后保司被软删：未换保司的编辑 = 维持历史引用（同 Writer 接缝语义）
    crate::commands::merchants::delete_merchant_internal(&conn, &merchant_id).unwrap();
    let mut keep_input = input(&merchant_id);
    keep_input.product_name = "医疗险".into();
    update_policy_internal(&conn, &id, keep_input, &mut || {}).unwrap();
    assert_eq!(
        list_policies_internal(&conn).unwrap()[0].product_name,
        "医疗险"
    );

    // 换成另一个软删商户 = 新档案选择，仍被拒
    let merchant2 = seed_merchant(&conn, "已退保保司");
    crate::commands::merchants::delete_merchant_internal(&conn, &merchant2).unwrap();
    let mut switch_input = input(&merchant2);
    switch_input.product_name = "医疗险".into();
    let err = update_policy_internal(&conn, &id, switch_input, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
}

#[test]
fn 建档校验各分支() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");

    let cases: Vec<(PolicyInput, &str)> = vec![
        (input("不存在的商户"), "保险公司不存在或已删除"),
        (
            {
                let mut i = input(&merchant_id);
                i.policy_number = "  ".into();
                i
            },
            "保单号不能为空",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.product_name = "".into();
                i
            },
            "险种名称不能为空",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.start_date = "2026/01/01".into();
                i
            },
            "日期格式无效",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.end_date = Some("2025-12-31".into());
                i
            },
            "早于起日",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.coverage_amount_cents = Some(0);
                i
            },
            "保额必须大于 0",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.coverage_amount_cents = Some(100);
                i.coverage_currency_code = None;
                i
            },
            "填写保额时必须选择保额币种",
        ),
        (
            {
                let mut i = input(&merchant_id);
                i.coverage_amount_cents = Some(100);
                i.coverage_currency_code = Some("XYZ".into());
                i
            },
            "未知币种",
        ),
    ];
    for (input, needle) in cases {
        let err = create_policy_internal(&conn, input, &mut || {}).unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "错误应包含 '{needle}'，实际 '{err}'"
        );
    }
    assert!(
        list_policies_internal(&conn).unwrap().is_empty(),
        "校验失败不落库"
    );
}

#[test]
fn 软删商户不可再被新档案选择() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "已退保保司");
    crate::commands::merchants::delete_merchant_internal(&conn, &merchant_id).unwrap();
    let err = create_policy_internal(&conn, input(&merchant_id), &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
}

// ---------------------------------------------------------------------------
// 保单视角统计（issue #363）：实时推导、不落库（BDD 场景外的快速反馈；
// 含协议期次的下期扣款日推导由 BDD `policy_stats.feature` 验收）
// ---------------------------------------------------------------------------

use crate::commands::policy::policy_stats_internal;
use crate::commands::transactions::create_transaction_internal;
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;

fn insert_account(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        rusqlite::params![id, id],
    )
    .unwrap();
}

fn linked_input(
    account: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
    policy_id: &str,
) -> TransactionInput {
    TransactionInput {
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: account.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: Some(policy_id.into()),
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

fn today(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn 统计_挂单保费与流入实时合计且软删流水不计入() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    insert_account(&conn, "acc-stat");
    let policy_id = create_ok(&conn, input(&merchant_id));

    let tx = |kind, amount, date| linked_input("acc-stat", kind, amount, date, &policy_id);
    // 三笔保费（其中一笔后软删）+ 一笔理赔款 + 一笔不挂单支出（不得串入）
    let removed =
        create_transaction_internal(&conn, tx(TransactionKind::Expense, 100, "2026-01-01"))
            .unwrap()
            .id;
    create_transaction_internal(&conn, tx(TransactionKind::Expense, 300, "2026-02-01")).unwrap();
    create_transaction_internal(&conn, tx(TransactionKind::Income, 50, "2026-03-01")).unwrap();
    create_transaction_internal(&conn, {
        let mut i = tx(TransactionKind::Expense, 999, "2026-04-01");
        i.policy_id = None;
        i
    })
    .unwrap();
    crate::commands::transactions::delete_transaction_internal(&conn, &removed).unwrap();

    let stats = policy_stats_internal(&conn, today(2026, 6, 1)).unwrap();
    assert_eq!(stats.len(), 1);
    let s = &stats[0];
    assert_eq!(s.policy_id, policy_id);
    // 逐笔可对账：软删的 100 不计入，余 300；流入 50
    assert_eq!(s.total_paid_native_cents, 300);
    assert_eq!(s.total_inflow_native_cents, 50);
    assert_eq!(s.native_currency, "CNY");
    assert_eq!(s.next_charge_date, None, "无协议不显示下期扣款日");
    assert!(!s.is_expired);
}

#[test]
fn 统计_到期态由止日与today推导() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");

    let build = |number: &str, start: &str, end: Option<&str>| {
        let mut i = input(&merchant_id);
        i.policy_number = number.into();
        i.start_date = start.into();
        i.end_date = end.map(String::from);
        i
    };
    create_ok(&conn, build("P-EXPIRED", "2019-01-01", Some("2020-01-01")));
    create_ok(&conn, build("P-FUTURE", "2026-01-01", Some("2999-01-01")));
    create_ok(&conn, build("P-LIFETIME", "2026-01-01", None));

    let stats = policy_stats_internal(&conn, today(2026, 6, 1)).unwrap();
    let by_number = |number: &str| {
        let id: String = conn
            .query_row(
                "SELECT id FROM policies WHERE policy_number=?1",
                [number],
                |r| r.get(0),
            )
            .unwrap();
        stats.iter().find(|s| s.policy_id == id).unwrap()
    };
    assert!(by_number("P-EXPIRED").is_expired, "止日已过 → 已到期");
    assert!(!by_number("P-FUTURE").is_expired, "止日未到 → 保障中");
    assert!(
        !by_number("P-LIFETIME").is_expired,
        "止日空 = 长期/终身，永不判到期"
    );
}

#[test]
fn 统计_软删保单不产生统计行且不串其他保单() {
    let conn = conn();
    let merchant_id = seed_merchant(&conn, "平安保险");
    insert_account(&conn, "acc-stat");
    let kept = create_ok(&conn, input(&merchant_id));
    let removed = {
        let mut i = input(&merchant_id);
        i.policy_number = "P-DELETED".into();
        create_ok(&conn, i)
    };
    // 已删保单的挂单流水保留原引用（不置空），但不得串入其他保单统计
    create_transaction_internal(
        &conn,
        linked_input(
            "acc-stat",
            TransactionKind::Expense,
            777,
            "2026-01-01",
            &removed,
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        linked_input(
            "acc-stat",
            TransactionKind::Expense,
            111,
            "2026-01-02",
            &kept,
        ),
    )
    .unwrap();
    delete_policy_internal(&conn, &removed, &mut || {}).unwrap();

    let stats = policy_stats_internal(&conn, today(2026, 6, 1)).unwrap();
    assert_eq!(stats.len(), 1, "软删保单不产生统计行");
    assert_eq!(stats[0].policy_id, kept);
    assert_eq!(stats[0].total_paid_native_cents, 111, "已删保单流水不串入");
}
