//! `policy` 域单元测试：建档校验语义、软删保留与失效信号回调
//! （BDD 场景外的快速反馈；外部可观察行为的验收在 BDD `policies.feature`）。

use rusqlite::Connection;

use super::model::PolicyInput;
use crate::{create_insurer, create_policy, delete_policy, list_policies, update_policy};

fn conn() -> Connection {
    // 建库两行序经统一测试工厂承载（spec #728 / issue #754 / ADR-0084 决策 7）；
    // 工厂住根包，经 dev-dependency 测试环消费（#1100，ledger-transaction 同款）。
    tauri_app_lib::test_support::open()
}

fn input(insurer_id: &str) -> PolicyInput {
    PolicyInput {
        insurer_id: insurer_id.into(),
        policy_number: "P2026-001".into(),
        product_name: "重疾险".into(),
        start_date: "2026-01-01".into(),
        end_date: Some("2036-01-01".into()),
        coverage_amount_cents: Some(30_000_000),
        coverage_currency_code: Some("CNY".into()),
        note: Some(" 50 万保额 ".into()),
    }
}

fn seed_insurer(conn: &Connection, name: &str) -> String {
    create_insurer(conn, crate::InsurerInput { name: name.into() }).expect("创建保司失败")
}

fn create_ok(conn: &Connection, input: PolicyInput) -> String {
    create_policy(conn, input, &mut || {}).expect("创建保单失败")
}

#[test]
fn 创建保单并读回全字段() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    let id = create_ok(&conn, input(&insurer_id));
    let list = list_policies(&conn).unwrap();
    assert_eq!(list.len(), 1);
    let policy = &list[0];
    assert_eq!(policy.id, id);
    assert_eq!(policy.insurer_id, insurer_id);
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
    let insurer_id = seed_insurer(&conn, "平安保险");
    let mut input = input(&insurer_id);
    input.end_date = None;
    input.coverage_amount_cents = None;
    // 保额缺省时币种忽略存空（成对原子）
    input.coverage_currency_code = Some("USD".into());
    input.note = Some("   ".into());
    create_ok(&conn, input);
    let policy = &list_policies(&conn).unwrap()[0];
    assert_eq!(policy.end_date, None);
    assert_eq!(policy.coverage_amount_cents, None);
    assert_eq!(policy.coverage_currency_code, None);
    assert_eq!(policy.note, None);
}

#[test]
fn 创建成功发失效信号_失败不发() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    let mut signals = 0;
    let ok_input = input(&insurer_id);
    create_policy(&conn, ok_input, &mut || signals += 1).unwrap();
    assert_eq!(signals, 1);

    let mut signals_err = 0;
    let bad = input("不存在的保司");
    let err = create_policy(&conn, bad, &mut || signals_err += 1).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
    assert_eq!(signals_err, 0);
}

#[test]
fn 编辑保单_审计字段保留() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    let id = create_ok(&conn, input(&insurer_id));
    let created_at = list_policies(&conn).unwrap()[0].created_at.clone();

    let insurer2 = seed_insurer(&conn, "太平洋保险");
    let mut input = input(&insurer2);
    input.policy_number = "P2026-002".into();
    input.product_name = "医疗险".into();
    input.start_date = "2026-02-01".into();
    input.end_date = None;
    input.coverage_amount_cents = None;
    input.coverage_currency_code = None;
    input.note = None;
    update_policy(&conn, &id, input, &mut || {}).unwrap();

    let policy = &list_policies(&conn).unwrap()[0];
    assert_eq!(policy.insurer_id, insurer2);
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
    let insurer_id = seed_insurer(&conn, "平安保险");
    let id = create_ok(&conn, input(&insurer_id));
    delete_policy(&conn, &id, &mut || {}).unwrap();

    assert!(list_policies(&conn).unwrap().is_empty());
    // 库内行保留：is_deleted=1，保司引用等列原样（历史引用保留不置空，ADR-0051 决策 5）
    let (is_deleted, kept_insurer, kept_number): (i64, String, String) = conn
        .query_row(
            "SELECT is_deleted, insurer_id, policy_number FROM policies WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(is_deleted, 1);
    assert_eq!(kept_insurer, insurer_id);
    assert_eq!(kept_number, "P2026-001");
}

#[test]
fn 已删保单再编辑再删均报不存在() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    let id = create_ok(&conn, input(&insurer_id));
    delete_policy(&conn, &id, &mut || {}).unwrap();

    let err = update_policy(&conn, &id, input(&insurer_id), &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保单不存在"));
    let err = delete_policy(&conn, &id, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保单不存在"));
}

#[test]
fn 编辑时保司未变_软删保司维持历史引用可继续编辑() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    let id = create_ok(&conn, input(&insurer_id));
    // 建档后保司被软删：未换保司的编辑 = 维持历史引用（同 Writer 接缝语义）
    crate::delete_insurer(&conn, &insurer_id).unwrap();
    let mut keep_input = input(&insurer_id);
    keep_input.product_name = "医疗险".into();
    update_policy(&conn, &id, keep_input, &mut || {}).unwrap();
    assert_eq!(list_policies(&conn).unwrap()[0].product_name, "医疗险");

    // 换成另一个软删保司 = 新档案选择，仍被拒
    let insurer2 = seed_insurer(&conn, "已退保保司");
    crate::delete_insurer(&conn, &insurer2).unwrap();
    let mut switch_input = input(&insurer2);
    switch_input.product_name = "医疗险".into();
    let err = update_policy(&conn, &id, switch_input, &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
}

#[test]
fn 建档校验各分支() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");

    let cases: Vec<(PolicyInput, &str)> = vec![
        (input("不存在的保司"), "保险公司不存在或已删除"),
        (
            {
                let mut i = input(&insurer_id);
                i.policy_number = "  ".into();
                i
            },
            "保单号不能为空",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.product_name = "".into();
                i
            },
            "险种名称不能为空",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.start_date = "2026/01/01".into();
                i
            },
            "日期格式无效",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.end_date = Some("2025-12-31".into());
                i
            },
            "早于起日",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.coverage_amount_cents = Some(0);
                i
            },
            "保额必须大于 0",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.coverage_amount_cents = Some(100);
                i.coverage_currency_code = None;
                i
            },
            "填写保额时必须选择保额币种",
        ),
        (
            {
                let mut i = input(&insurer_id);
                i.coverage_amount_cents = Some(100);
                i.coverage_currency_code = Some("XYZ".into());
                i
            },
            "未知币种",
        ),
    ];
    for (input, needle) in cases {
        let err = create_policy(&conn, input, &mut || {}).unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "错误应包含 '{needle}'，实际 '{err}'"
        );
    }
    assert!(list_policies(&conn).unwrap().is_empty(), "校验失败不落库");
}

#[test]
fn 软删保司不可再被新档案选择() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "已退保保司");
    crate::delete_insurer(&conn, &insurer_id).unwrap();
    let err = create_policy(&conn, input(&insurer_id), &mut || {}).unwrap_err();
    assert!(err.to_string().contains("保险公司不存在或已删除"));
}

// ---------------------------------------------------------------------------
// 保单视角统计（issue #363）：实时推导、不落库（BDD 场景外的快速反馈；
// 含协议期次的下期扣款日推导由 BDD `policy_stats.feature` 验收）
// ---------------------------------------------------------------------------

use crate::policy_stats;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

fn insert_account(conn: &Connection, id: &str) {
    // 统计世界脚手架账户：工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）。
    tauri_app_lib::test_support::seed_account(conn, id, id, "cash", "CNY", 0);
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
        funding_account_id: None,
        note: None,
        date: date.into(),
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

fn today(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn 统计_挂单保费与流入实时合计且软删流水不计入() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    insert_account(&conn, "acc-stat");
    let policy_id = create_ok(&conn, input(&insurer_id));

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
    ledger_transaction::delete_transaction_internal(&conn, &removed).unwrap();

    let stats = policy_stats(&conn, today(2026, 6, 1)).unwrap();
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
    let insurer_id = seed_insurer(&conn, "平安保险");

    let build = |number: &str, start: &str, end: Option<&str>| {
        let mut i = input(&insurer_id);
        i.policy_number = number.into();
        i.start_date = start.into();
        i.end_date = end.map(String::from);
        i
    };
    create_ok(&conn, build("P-EXPIRED", "2019-01-01", Some("2020-01-01")));
    create_ok(&conn, build("P-FUTURE", "2026-01-01", Some("2999-01-01")));
    create_ok(&conn, build("P-LIFETIME", "2026-01-01", None));
    create_ok(&conn, build("P-TODAY", "2019-01-01", Some("2026-06-01")));

    let stats = policy_stats(&conn, today(2026, 6, 1)).unwrap();
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
    // 边界：止日 == today 按「早于今天」严格判定 → 保障中
    assert!(!by_number("P-TODAY").is_expired, "止日为 today 不算已到期");
}

#[test]
fn 统计_软删保单不产生统计行且不串其他保单() {
    let conn = conn();
    let insurer_id = seed_insurer(&conn, "平安保险");
    insert_account(&conn, "acc-stat");
    let kept = create_ok(&conn, input(&insurer_id));
    let removed = {
        let mut i = input(&insurer_id);
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
    delete_policy(&conn, &removed, &mut || {}).unwrap();

    let stats = policy_stats(&conn, today(2026, 6, 1)).unwrap();
    assert_eq!(stats.len(), 1, "软删保单不产生统计行");
    assert_eq!(stats[0].policy_id, kept);
    assert_eq!(stats[0].total_paid_native_cents, 111, "已删保单流水不串入");
}

// ---------------------------------------------------------------------------
// 读快照一致性探针（issue #1702）：写提交落在语句之间时，同屏口径必须仍互相
// 自洽。探针机制见 `tauri_app_lib::test_support::snapshot_probe`。
// ---------------------------------------------------------------------------

use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{ScratchDir, open_file};

/// 统计的保单行与逐保单合计必须同快照（issue #1702）：保单基础行、保费/流入
/// 聚合、下期扣款日、本位币是多语句读闭包（两段 join 形态）。探针在保费聚合
/// 读取开始前于另一连接把挂单流水金额翻倍——
/// - 读闭包无快照保护（红）：统计相对基线漂移（合计读新、保单行读旧，
///   「列表有此保单而合计对不上」不同时点）；
/// - 读闭包收进读事务（绿）：注入写被挡住，行与合计同见一套数。
#[test]
fn policy_stats_rows_and_sums_share_one_snapshot() {
    let dir = ScratchDir::new("policy-stats-read-snapshot");
    let conn = open_file(dir.path());
    let insurer_id = seed_insurer(&conn, "平安保险");
    let policy_id = create_ok(&conn, input(&insurer_id));
    insert_account(&conn, "acc-stats");
    create_transaction_internal(
        &conn,
        linked_input(
            "acc-stats",
            TransactionKind::Expense,
            300,
            "2026-02-01",
            &policy_id,
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        linked_input(
            "acc-stats",
            TransactionKind::Income,
            50,
            "2026-03-01",
            &policy_id,
        ),
    )
    .unwrap();

    let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let before = policy_stats(&conn, today).unwrap();
    assert_eq!(
        before.len(),
        1,
        "种子应产出恰好一行统计（否则口径断言空转）"
    );
    assert_eq!(
        before[0].total_paid_native_cents, 300,
        "种子保费合计应就位（否则口径断言空转）"
    );
    assert_eq!(before[0].total_inflow_native_cents, 50);

    // 探针：保费聚合读取（`FROM transactions t JOIN policies p`，位于保单行
    // 之后、全闭包首次命中）开始前，另一连接提交流水翻倍。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transactions t JOIN policies p",
        &[
            "UPDATE transactions SET amount_native_cents = amount_native_cents * 2",
            "UPDATE transactions SET amount_cents = amount_cents * 2",
        ],
    );
    let after = policy_stats(&conn, today).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中保费聚合（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before[0].total_paid_native_cents, after[0].total_paid_native_cents,
        "保费合计必须与基线同时点（保单行读旧、合计读新即漂移）"
    );
    assert_eq!(
        before[0].total_inflow_native_cents, after[0].total_inflow_native_cents,
        "流入合计必须与基线同时点"
    );
    assert_eq!(
        (
            before[0].policy_id.as_str(),
            before[0].native_currency.as_str()
        ),
        (
            after[0].policy_id.as_str(),
            after[0].native_currency.as_str()
        ),
        "统计行身份与折算基准不应漂移"
    );
}
