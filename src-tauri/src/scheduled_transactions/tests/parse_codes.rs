//! 定时计划域闭集解析与期次日期守卫的码契约（ADR-0050 码化收口，#1072）。
//!
//! 四个 `FromStr` 闭集解析覆盖 DB 读边界（`FromSql` / `parse`）：未知值报稳定码
//! 与插值参数，`message` 逐字保留（ADR-0050 只增不改）。四个码在当前 wire/DB
//! 路径不可达前端（经 serde/rusqlite 错误类型扁平化后只承载 `message`），断言
//! 在构造点——与 `account.type-unknown` 同形（ADR-0108 决策 4 已知边界）。
//!
//! 期次日期守卫（`advance_date`）另断言用户可见路径：`recurrence_day` 未在入口
//! 校验，月度计划的 0 日落在 `from_ymd_opt` 的 None 分支，错误按码上报。

use std::str::FromStr;

use super::super::*;
use crate::error::AppError;

/// 断言错误同时满足：稳定码、`kind` 归类、message 逐字、插值参数顺序。
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

/// 期次日期守卫（#1072）：月度计划给出闭集内的周期类型但非法 `recurrence_day`
/// （0 日），建档展开期次时报码化「无效日期」——用户可见条件不留裸 `Invalid`。
#[test]
fn occurrence_date_guard_reports_coded_invalid_date() {
    let conn = crate::test_support::open();
    crate::test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
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
