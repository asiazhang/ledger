use std::str::FromStr;

use super::super::*;
use ledger_infra::error::AppError;

fn assert_coded(err: &AppError, code: &str, message: &str, params: &[&str]) {
    let wire = serde_json::to_value(err).unwrap();
    assert_eq!(wire["kind"], "Invalid", "码化参数错误 kind 保持 Invalid");
    assert_eq!(wire["code"], code);
    assert_eq!(err.to_string(), message, "message 逐字保留（ADR-0050）");
    if params.is_empty() {
        assert!(
            wire.get("params").is_none(),
            "无参错误 params 应缺席: {wire}"
        );
    } else {
        assert_eq!(wire["params"], serde_json::json!(params));
    }
}

#[test]
fn scheduled_kind_parse_rejects_unknown_with_code() {
    let err = ScheduledKind::from_str("bogus").unwrap_err();
    assert_coded(
        &err,
        "scheduled-plan.kind-unknown",
        "未知定时交易类型: bogus",
        &["bogus"],
    );
}

#[test]
fn scheduled_status_parse_rejects_unknown_with_code() {
    let err = ScheduledStatus::from_str("bogus").unwrap_err();
    assert_coded(
        &err,
        "scheduled-plan.status-unknown",
        "未知计划状态: bogus",
        &["bogus"],
    );
}

#[test]
fn recurrence_type_parse_rejects_unknown_with_code() {
    let err = RecurrenceType::from_str("bogus").unwrap_err();
    assert_coded(
        &err,
        "scheduled-plan.recurrence-unknown",
        "未知周期类型: bogus",
        &["bogus"],
    );
}

#[test]
fn occurrence_status_parse_rejects_unknown_with_code() {
    let err = OccurrenceStatus::from_str("bogus").unwrap_err();
    assert_coded(
        &err,
        "scheduled-occurrence.status-unknown",
        "未知期次状态: bogus",
        &["bogus"],
    );
}

#[test]
fn occurrence_date_guard_reports_coded_invalid_date() {
    let conn = tauri_app_lib::test_support::open();
    tauri_app_lib::test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    let err = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: None,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: Some(0),
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap_err();
    assert_coded(
        &err,
        "scheduled-plan.occurrence-date-invalid",
        "无效日期",
        &[],
    );
}
