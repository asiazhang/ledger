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
