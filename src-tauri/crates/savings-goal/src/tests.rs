use chrono::NaiveDate;
use rusqlite::Connection;

use super::model::{
    SavingsGoalInput, SavingsGoalPaceSource, SavingsGoalStatus, SavingsGoalUpdateInput,
};
use super::{create_savings_goal, list_savings_goal_progress, update_savings_goal};
use ledger_accounts::AccountType;
use ledger_transaction::{TransactionInput, TransactionKind};

fn setup() -> Connection {
    tauri_app_lib::test_support::open()
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
}

fn create_transfer_plan(
    conn: &Connection,
    from: &str,
    to: &str,
    amount_cents: i64,
    recurrence_type: &str,
    recurrence_interval: i64,
) -> String {
    ledger_scheduled::create_plan(
        conn,
        ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::ScheduledTransfer,
            account_id: from.into(),
            category_id: None,
            amount_cents,
            currency_code: "CNY".into(),
            recurrence_type: recurrence_type.parse().unwrap(),
            recurrence_interval,
            recurrence_day: None,
            start_date: "2026-09-01".into(),
            note: None,
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: Some(to.into()),
        },
    )
    .unwrap()
}

fn goal_input(name: &str, target_amount_cents: i64, deadline: Option<&str>) -> SavingsGoalInput {
    SavingsGoalInput {
        name: name.to_string(),
        target_amount_cents,
        deadline: deadline.map(str::to_string),
    }
}
fn update_input(
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
    planned_monthly_cents: Option<i64>,
) -> SavingsGoalUpdateInput {
    SavingsGoalUpdateInput {
        name: name.to_string(),
        target_amount_cents,
        deadline: deadline.map(str::to_string),
        planned_monthly_cents,
    }
}

fn only_progress(conn: &Connection) -> SavingsGoalProgressRow {
    let rows = list_savings_goal_progress(conn, today()).expect("进度读取应成功");
    assert_eq!(rows.len(), 1, "夹具应只有一个目标");
    rows.into_iter().next().unwrap()
}

type SavingsGoalProgressRow = super::model::SavingsGoalProgress;

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn transfer(conn: &Connection, from: &str, to: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            funding: Vec::new(),
            kind: TransactionKind::Transfer,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: Some(from.into()),
            to_account_id: Some(to.into()),
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            source_order_no: None,
            note: None,
            date: "2026-07-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
            purchases: Vec::new(),
        },
    )
    .expect("转账应成功");
}

fn spend(conn: &Connection, account_id: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            funding: Vec::new(),
            kind: TransactionKind::Expense,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: Some(account_id.into()),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            source_order_no: None,
            note: None,
            date: "2026-07-02".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
            purchases: Vec::new(),
        },
    )
    .expect("支出应成功");
}

#[test]
fn create_goal_builds_bound_other_account() {
    let conn = setup();
    let goal_id = create_savings_goal(&conn, &goal_input("买车基金", 500_000, Some("2027-06-30")))
        .expect("创建目标应成功");

    let row = only_progress(&conn);
    assert_eq!(row.goal.id, goal_id);
    assert_eq!(row.goal.name, "买车基金");
    assert_eq!(row.goal.target_amount_cents, 500_000);
    assert_eq!(row.goal.deadline.as_deref(), Some("2027-06-30"));
    assert_eq!(row.goal.status, SavingsGoalStatus::Active);

    let accounts = ledger_accounts::list_accounts(&conn).expect("账户域读命令应成功");
    let bound: Vec<_> = accounts
        .iter()
        .filter(|a| a.id == row.goal.account_id)
        .collect();
    assert_eq!(bound.len(), 1, "目标绑定的账户应在账户域可读");
    let account = bound[0];
    assert_eq!(account.name, "买车基金");
    assert_eq!(account.kind, AccountType::Other);
    assert_eq!(
        account.currency_code, "CNY",
        "币种 = 账本本位币（缺省 CNY）"
    );

    create_savings_goal(&conn, &goal_input("教育金", 100_000, None)).expect("第二个目标创建应成功");
    let rows = list_savings_goal_progress(&conn, today()).expect("进度读取应成功");
    assert_eq!(rows.len(), 2);
    let account_ids: std::collections::HashSet<&str> =
        rows.iter().map(|r| r.goal.account_id.as_str()).collect();
    assert_eq!(account_ids.len(), 2, "每个目标绑定各自专属账户");
}

#[test]
fn edit_goal_renames_bound_account() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let before = only_progress(&conn);
    let account_id = before.goal.account_id.clone();

    update_savings_goal(
        &conn,
        &before.goal.id,
        &update_input("换车基金", 500_000, None, None),
    )
    .expect("编辑目标应成功");

    let row = only_progress(&conn);
    assert_eq!(row.goal.name, "换车基金");
    assert_eq!(
        row.goal.account_id, account_id,
        "改名不改绑定（1 目标 : 1 账户）"
    );
    let accounts = ledger_accounts::list_accounts(&conn).expect("账户域读命令应成功");
    let bound = accounts
        .iter()
        .find(|a| a.id == account_id)
        .expect("专属账户应可读");
    assert_eq!(bound.name, "换车基金", "专属账户名随目标联动");

    update_savings_goal(
        &conn,
        &row.goal.id,
        &update_input("换车基金", 600_000, None, None),
    )
    .expect("名称未变的编辑应成功");
    assert_eq!(only_progress(&conn).goal.target_amount_cents, 600_000);
}

#[test]
fn edit_goal_amount_and_deadline_reflect_in_progress() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, Some("2027-06-30")))
        .expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id;
    let account_id = only_progress(&conn).goal.account_id;
    transfer(&conn, "src", &account_id, 300_000);

    let row = only_progress(&conn);
    assert!(!row.achieved, "已存 300000 < 目标 500000 未达成");

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 200_000, Some("2027-06-30"), None),
    )
    .expect("改小目标额应成功");
    let row = only_progress(&conn);
    assert_eq!(row.goal.target_amount_cents, 200_000);
    assert!(row.achieved, "改小目标额后立即呈现达成态");
    assert_eq!(row.remaining_cents, -100_000, "还差为带符号差值");

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 200_000, None, None),
    )
    .expect("清除截止日应成功");
    let row = only_progress(&conn);
    assert_eq!(row.goal.deadline, None, "截止日可清空");
    assert!(row.achieved);

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 200_000, Some("2028-12-31"), None),
    )
    .expect("回设截止日应成功");
    assert_eq!(
        only_progress(&conn).goal.deadline.as_deref(),
        Some("2028-12-31")
    );

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 600_000, Some("2028-12-31"), None),
    )
    .expect("调大目标额应成功");
    let row = only_progress(&conn);
    assert!(!row.achieved);
    assert_eq!(row.remaining_cents, 300_000);
}

#[test]
fn edit_goal_rejects_invalid_input_matrix() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, Some("2027-06-30")))
        .expect("创建目标应成功");
    update_savings_goal(
        &conn,
        &only_progress(&conn).goal.id,
        &update_input("买车基金", 500_000, Some("2027-06-30"), Some(50_000)),
    )
    .expect("基线编辑应成功");
    let baseline = only_progress(&conn);

    let matrix = [
        (
            "目标额为零",
            "买车基金",
            0_i64,
            None,
            "savings-goal.target-amount-positive",
        ),
        (
            "目标额为负",
            "买车基金",
            -1,
            None,
            "savings-goal.target-amount-positive",
        ),
        ("名称为空", "", 500_000, None, "savings-goal.name-required"),
        (
            "名称全空白",
            "   ",
            500_000,
            None,
            "savings-goal.name-required",
        ),
        (
            "计划月存为零",
            "买车基金",
            500_000,
            Some(0),
            "savings-goal.planned-monthly-positive",
        ),
        (
            "计划月存为负",
            "买车基金",
            500_000,
            Some(-100),
            "savings-goal.planned-monthly-positive",
        ),
    ];
    for (label, name, target, planned, expected_code) in matrix {
        let err = match update_savings_goal(
            &conn,
            &baseline.goal.id,
            &update_input(name, target, None, planned),
        ) {
            Err(e) => e,
            Ok(()) => panic!("[{label}] 应被码化拒绝"),
        };
        assert!(
            err.is_code(expected_code),
            "[{label}] 应报 {expected_code}，实际 {err:?}"
        );
    }

    let after = only_progress(&conn);
    assert_eq!(after.goal.name, baseline.goal.name);
    assert_eq!(
        after.goal.target_amount_cents,
        baseline.goal.target_amount_cents
    );
    assert_eq!(after.goal.deadline, baseline.goal.deadline);
    assert_eq!(
        after.goal.planned_monthly_cents,
        baseline.goal.planned_monthly_cents
    );
}

#[test]
fn edit_goal_sets_and_clears_planned_monthly() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id;
    assert_eq!(
        only_progress(&conn).goal.planned_monthly_cents,
        None,
        "缺省未设置"
    );

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 500_000, None, Some(50_000)),
    )
    .expect("设置计划月存应成功");
    assert_eq!(
        only_progress(&conn).goal.planned_monthly_cents,
        Some(50_000)
    );

    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 500_000, None, None),
    )
    .expect("清除计划月存应成功");
    assert_eq!(
        only_progress(&conn).goal.planned_monthly_cents,
        None,
        "清除后归空"
    );
}

#[test]
fn edit_missing_goal_reports_not_found() {
    let conn = setup();
    let err = update_savings_goal(
        &conn,
        "no-such-goal",
        &update_input("幽灵", 100, None, None),
    )
    .expect_err("不存在的目标应被拒绝");
    assert!(
        err.is_code("savings-goal.not-found"),
        "应报码化错误 savings-goal.not-found，实际 {err:?}"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM goals"),
        0,
        "拒绝不应新建目标行"
    );
}
#[test]
fn create_goal_rejects_non_positive_target_amount() {
    let conn = setup();
    let accounts_before = count(&conn, "SELECT COUNT(*) FROM accounts WHERE is_deleted=0");

    for amount in [0_i64, -1] {
        let err = create_savings_goal(&conn, &goal_input("负数目标", amount, None))
            .expect_err("非正目标金额应被拒绝");
        assert!(
            err.is_code("savings-goal.target-amount-positive"),
            "应报码化错误 savings-goal.target-amount-positive，实际 {err:?}"
        );
    }

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM goals WHERE is_deleted=0"),
        0,
        "拒绝创建不应留下目标行"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM accounts WHERE is_deleted=0"),
        accounts_before,
        "拒绝创建不应建出专属账户"
    );
}

#[test]
fn progress_follows_goal_account_balance() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_account_id = only_progress(&conn).goal.account_id;

    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 0);
    assert_eq!(row.remaining_cents, 500_000);
    assert!(!row.achieved);
    assert_eq!(row.currency_code, "CNY");

    transfer(&conn, "src", &goal_account_id, 200_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 200_000, "已存 = 专属账户余额");
    assert_eq!(row.remaining_cents, 300_000);
    assert!(!row.achieved);

    transfer(&conn, "src", &goal_account_id, 300_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 500_000);
    assert_eq!(row.remaining_cents, 0);
    assert!(row.achieved, "余额 ≥ 目标额即达成");

    transfer(&conn, "src", &goal_account_id, 50_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 550_000);
    assert_eq!(row.remaining_cents, -50_000);
    assert!(row.achieved);

    spend(&conn, &goal_account_id, 100_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 450_000);
    assert_eq!(row.remaining_cents, 50_000);
    assert!(!row.achieved, "余额跌破目标额即退出达成态");
}

#[test]
fn projection_no_deadline_uses_linked_plan_pace() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 1_200_000, None)).expect("创建目标应成功");
    let goal_account = only_progress(&conn).goal.account_id;
    create_transfer_plan(&conn, "src", &goal_account, 100_000, "monthly", 1);

    let row = only_progress(&conn);
    assert_eq!(
        row.pace_monthly_cents,
        Some(100_000),
        "节奏 = 计划每期金额折算月存"
    );
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Plan));
    assert_eq!(
        row.eta_months,
        Some(12),
        "还差 1_200_000 ÷ 100_000 = 12 个月"
    );
    assert_eq!(
        row.eta_month,
        Some("2027-09".to_string()),
        "预计年月 = 2026-09 + 12 月"
    );
    assert_eq!(row.required_monthly_cents, None, "无截止日不反推所需月存");
    assert_eq!(row.pace_delta_cents, None);
}

#[test]
fn projection_pace_source_closed_set_and_zero_pace_no_fabrication() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("买车基金", 1_200_000, None)).expect("创建目标应成功");

    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, None, "无计划且未手填 = 节奏为零");
    assert_eq!(row.pace_source, None);
    assert_eq!(row.eta_months, None, "节奏为零不虚构 ETA 月数");
    assert_eq!(row.eta_month, None, "节奏为零不虚构预计年月");
    assert_eq!(row.required_monthly_cents, None);
    assert_eq!(row.pace_delta_cents, None);

    update_savings_goal(
        &conn,
        &row.goal.id,
        &update_input("买车基金", 1_200_000, None, Some(50_000)),
    )
    .expect("手填计划月存应成功");
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, Some(50_000), "无计划时节奏取手填值");
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Manual));
    assert_eq!(row.eta_months, Some(24));
    assert_eq!(row.eta_month, Some("2028-09".to_string()));

    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    let goal_account = only_progress(&conn).goal.account_id;
    create_transfer_plan(&conn, "src", &goal_account, 100_000, "monthly", 1);
    let row = only_progress(&conn);
    assert_eq!(
        row.pace_monthly_cents,
        Some(100_000),
        "有计划时节奏取计划折算"
    );
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Plan));
    assert_eq!(row.eta_months, Some(12), "ETA 按计划节奏重算");
}

#[test]
fn projection_plan_pace_recurrence_conversion_matrix() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 9_999_999);
    create_savings_goal(&conn, &goal_input("买车基金", 10_000_000, None)).expect("创建目标应成功");
    let goal_account = only_progress(&conn).goal.account_id;

    create_transfer_plan(&conn, "src", &goal_account, 10_000, "weekly", 1);
    create_transfer_plan(&conn, "src", &goal_account, 120_000, "yearly", 1);
    create_transfer_plan(&conn, "src", &goal_account, 90_000, "monthly", 3);
    create_transfer_plan(&conn, "src", &goal_account, 1_000, "daily", 1);
    let row = only_progress(&conn);
    assert_eq!(
        row.pace_monthly_cents,
        Some(43_333 + 10_000 + 30_000 + 30_000),
        "逐计划先折算再舍入、最后求和（订阅花费先例同款舍入口径）"
    );

    create_savings_goal(&conn, &goal_input("教育金", 500_000, None)).expect("第二个目标创建应成功");
    let rows = list_savings_goal_progress(&conn, today()).expect("进度读取应成功");
    let other = rows.iter().find(|r| r.goal.name == "教育金").unwrap();
    assert_eq!(
        other.pace_monthly_cents, None,
        "他目标的手填值缺席 = 节奏为零（不共享他目标的计划节奏）"
    );
    assert_eq!(other.eta_months, None, "节奏为零不虚构 ETA");
}

#[test]
fn projection_excludes_paused_and_cancelled_plans() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 1_200_000, None)).expect("创建目标应成功");
    let goal_account = only_progress(&conn).goal.account_id;
    let plan_id = create_transfer_plan(&conn, "src", &goal_account, 100_000, "monthly", 1);
    update_savings_goal(
        &conn,
        &only_progress(&conn).goal.id,
        &update_input("买车基金", 1_200_000, None, Some(50_000)),
    )
    .expect("手填节奏应成功");
    assert_eq!(only_progress(&conn).pace_monthly_cents, Some(100_000));

    ledger_scheduled::update_plan_status(
        &conn,
        &plan_id,
        ledger_scheduled::ScheduledStatus::Paused,
    )
    .unwrap();
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, Some(50_000), "暂停计划不再计入");
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Manual));
    assert_eq!(row.eta_months, Some(24), "ETA 随节奏回退重算");

    ledger_scheduled::update_plan_status(
        &conn,
        &plan_id,
        ledger_scheduled::ScheduledStatus::Active,
    )
    .unwrap();
    assert_eq!(
        only_progress(&conn).pace_monthly_cents,
        Some(100_000),
        "恢复在用重新计入"
    );
    ledger_scheduled::update_plan_status(
        &conn,
        &plan_id,
        ledger_scheduled::ScheduledStatus::Cancelled,
    )
    .unwrap();
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, Some(50_000), "取消计划不再计入");
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Manual));
}

#[test]
fn projection_deadline_required_monthly_and_delta_tracks_progress() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 5_000_000);
    create_savings_goal(&conn, &goal_input("上大学", 1_200_000, Some("2027-06-30")))
        .expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id;
    let goal_account = only_progress(&conn).goal.account_id;
    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("上大学", 1_200_000, Some("2027-06-30"), Some(100_000)),
    )
    .expect("手填节奏应成功");
    transfer(&conn, "src", &goal_account, 200_000);

    let row = only_progress(&conn);
    assert_eq!(
        row.required_monthly_cents,
        Some(111_112),
        "1_000_000 ÷ 9 上取整"
    );
    assert_eq!(
        row.pace_delta_cents,
        Some(100_000 - 111_112),
        "节奏 10 万 < 所需 = 落后"
    );
    assert_eq!(row.eta_months, None, "有截止日不正推 ETA");

    transfer(&conn, "src", &goal_account, 400_000);
    let row = only_progress(&conn);
    assert_eq!(
        row.required_monthly_cents,
        Some(66_667),
        "600_000 ÷ 9 上取整"
    );
    assert_eq!(
        row.pace_delta_cents,
        Some(100_000 - 66_667),
        "节奏所需差值随蓄水转正（超前）"
    );

    ledger_scheduled::update_plan_status(
        &conn,
        &create_transfer_plan(&conn, "src", &goal_account, 100_000, "monthly", 1),
        ledger_scheduled::ScheduledStatus::Cancelled,
    )
    .unwrap();
    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("上大学", 1_200_000, Some("2027-06-30"), None),
    )
    .expect("清除手填节奏应成功");
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, None, "节奏为零");
    assert_eq!(
        row.required_monthly_cents,
        Some(66_667),
        "所需月存是纯目标参数算术，不依赖节奏"
    );
    assert_eq!(row.pace_delta_cents, None, "无节奏不虚构差值");

    transfer(&conn, "src", &goal_account, 800_000);
    let row = only_progress(&conn);
    assert!(row.achieved);
    assert_eq!(row.eta_months, None);
    assert_eq!(row.required_monthly_cents, None);
    assert_eq!(row.pace_delta_cents, None);
}

#[test]
fn projection_past_deadline_projects_nothing() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("旧目标", 1_200_000, Some("2026-01-31")))
        .expect("创建目标应成功");
    update_savings_goal(
        &conn,
        &only_progress(&conn).goal.id,
        &update_input("旧目标", 1_200_000, Some("2026-01-31"), Some(100_000)),
    )
    .expect("手填节奏应成功");
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, Some(100_000), "节奏照常回显");
    assert_eq!(row.required_monthly_cents, None, "截止日已过不虚构所需月存");
    assert_eq!(row.pace_delta_cents, None);
    assert_eq!(row.eta_months, None, "有截止日不走无截止正推");
}

#[test]
fn projection_deadline_in_current_month_counts_one_month() {
    let conn = setup();
    create_savings_goal(
        &conn,
        &goal_input("当月目标", 1_200_000, Some("2026-09-30")),
    )
    .expect("创建目标应成功");
    let row = only_progress(&conn);
    assert_eq!(
        row.required_monthly_cents,
        Some(1_200_000),
        "截止当月（2026-09-30 vs 今天 2026-09-23）按 1 个整月计：所需 = 剩余"
    );
    assert_eq!(row.pace_delta_cents, None, "节奏为零不虚构差值");
}

#[test]
fn progress_and_linked_plan_share_one_snapshot() {
    use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
    use tauri_app_lib::test_support::{ScratchDir, open_file};

    let dir = ScratchDir::new("savings-goal-read-snapshot");
    let conn = open_file(dir.path());
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 1_200_000, None)).expect("创建目标应成功");
    let goal_account = only_progress(&conn).goal.account_id;
    create_transfer_plan(&conn, "src", &goal_account, 100_000, "monthly", 1);

    let before = only_progress(&conn);
    assert_eq!(
        before.pace_monthly_cents,
        Some(100_000),
        "种子节奏应为计划折算 100000（否则口径断言空转）"
    );

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM scheduled_transactions",
        &[
            "UPDATE scheduled_transactions SET amount_cents = amount_cents*2 WHERE kind='scheduled_transfer'",
        ],
    );
    let after = only_progress(&conn);

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中关联计划读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before.pace_monthly_cents, after.pace_monthly_cents,
        "节奏必须与基线同时点（计划金额落在语句间翻倍即漂移）"
    );
    assert_eq!(
        after.eta_months, before.eta_months,
        "ETA 由同快照的余额与节奏派生，两者异快照即口径矛盾"
    );
}

use super::{
    archive_savings_goal, delete_savings_goal, ensure_account_can_archive,
    ensure_account_not_goal_bound, unarchive_savings_goal,
};

#[test]
fn archive_exits_default_state_and_unarchive_restores() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id.clone();
    let account_id = only_progress(&conn).goal.account_id.clone();
    transfer(&conn, "src", &account_id, 200_000);

    archive_savings_goal(&conn, &goal_id).expect("归档应成功");

    let row = only_progress(&conn);
    assert_eq!(
        row.goal.status,
        SavingsGoalStatus::Archived,
        "归档后状态 archived"
    );
    assert_eq!(row.saved_cents, 200_000, "归档不动余额（账户 / 流水原样）");

    unarchive_savings_goal(&conn, &goal_id).expect("取消归档应成功");
    let row = only_progress(&conn);
    assert_eq!(
        row.goal.status,
        SavingsGoalStatus::Active,
        "取消归档恢复进行中"
    );
    assert_eq!(row.saved_cents, 200_000);
}

#[test]
fn archive_deletes_nothing() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let account_id = only_progress(&conn).goal.account_id.clone();
    transfer(&conn, "src", &account_id, 200_000);
    create_transfer_plan(&conn, "src", &account_id, 100_000, "monthly", 1);
    let tx_before = count(
        &conn,
        &format!(
            "SELECT COUNT(*) FROM transactions \
             WHERE account_id='{account_id}' OR to_account_id='{account_id}'"
        ),
    );

    archive_savings_goal(&conn, &only_progress(&conn).goal.id).expect("归档应成功");

    assert_eq!(
        count(
            &conn,
            &format!(
                "SELECT COUNT(*) FROM transactions \
                 WHERE account_id='{account_id}' OR to_account_id='{account_id}'"
            ),
        ),
        tx_before,
        "归档不删流水"
    );
    let accounts = ledger_accounts::list_accounts(&conn).expect("账户读命令应成功");
    assert!(
        accounts.iter().any(|a| a.id == account_id),
        "归档不删专属账户"
    );
    let row = only_progress(&conn);
    assert_eq!(
        row.pace_source,
        Some(SavingsGoalPaceSource::Plan),
        "关联计划不受归档影响"
    );
    assert_eq!(row.pace_monthly_cents, Some(100_000));
}

#[test]
fn archive_missing_goal_reports_not_found() {
    let conn = setup();
    let err = archive_savings_goal(&conn, "no-such-goal").expect_err("不存在目标应被拒绝");
    assert!(
        err.is_code("savings-goal.not-found"),
        "应报码化 NotFound，实际 {err:?}"
    );
    let err = unarchive_savings_goal(&conn, "no-such-goal").expect_err("同上");
    assert!(err.is_code("savings-goal.not-found"), "实际 {err:?}");
}

#[test]
fn delete_goal_rejects_nonzero_balance() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id.clone();
    let account_id = only_progress(&conn).goal.account_id.clone();
    transfer(&conn, "src", &account_id, 200_000);

    let err = delete_savings_goal(&conn, &goal_id).expect_err("余额非零应被拒绝");
    assert!(
        err.is_code("savings-goal.delete-balance-nonzero"),
        "应报码化错误 savings-goal.delete-balance-nonzero（文案引导先转出），实际 {err:?}"
    );

    let row = only_progress(&conn);
    assert_eq!(row.goal.id, goal_id, "拒绝删除后目标行原样");
    let accounts = ledger_accounts::list_accounts(&conn).expect("账户读命令应成功");
    assert!(
        accounts.iter().any(|a| a.id == account_id),
        "拒绝删除后专属账户原样"
    );
}

#[test]
fn delete_goal_zero_balance_cascades_account_soft_delete() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_id = only_progress(&conn).goal.id.clone();
    let account_id = only_progress(&conn).goal.account_id.clone();
    transfer(&conn, "src", &account_id, 200_000);
    spend(&conn, &account_id, 200_000);
    assert_eq!(only_progress(&conn).saved_cents, 0, "夹具应清零余额");
    let tx_before = count(
        &conn,
        &format!(
            "SELECT COUNT(*) FROM transactions \
             WHERE account_id='{account_id}' OR to_account_id='{account_id}'"
        ),
    );
    assert!(tx_before > 0, "夹具应留下历史交易");

    delete_savings_goal(&conn, &goal_id).expect("余额为零应可删除");

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM goals WHERE is_deleted=0"),
        0,
        "目标行软删"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM goals WHERE is_deleted=1"),
        1,
        "软删而非物理删除"
    );
    assert_eq!(
        count(
            &conn,
            &format!("SELECT COUNT(*) FROM accounts WHERE id='{account_id}' AND is_deleted=0"),
        ),
        0,
        "级联软删专属账户"
    );
    assert_eq!(
        count(
            &conn,
            &format!("SELECT COUNT(*) FROM accounts WHERE id='{account_id}' AND is_deleted=1"),
        ),
        1,
        "专属账户为软删而非删除数据"
    );
    assert_eq!(
        count(
            &conn,
            &format!(
                "SELECT COUNT(*) FROM transactions \
                 WHERE account_id='{account_id}' OR to_account_id='{account_id}'"
            ),
        ),
        tx_before,
        "历史交易保留可查"
    );
    assert!(
        list_savings_goal_progress(&conn, today())
            .expect("进度读取应成功")
            .is_empty()
    );

    ensure_account_not_goal_bound(&conn, &account_id).expect("已删目标的账户守卫应放行");
}

#[test]
fn delete_missing_goal_reports_not_found() {
    let conn = setup();
    let err = delete_savings_goal(&conn, "no-such-goal").expect_err("不存在目标应被拒绝");
    assert!(err.is_code("savings-goal.not-found"), "实际 {err:?}");
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM accounts WHERE is_deleted=1"),
        0
    );
}

#[test]
fn account_guard_rejects_bound_account_only() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "plain", "普通卡", "bank", "CNY", 0);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let account_id = only_progress(&conn).goal.account_id.clone();

    let err = ensure_account_not_goal_bound(&conn, &account_id).expect_err("在用目标绑定应被拒");
    assert!(
        err.is_code("savings-goal.account-in-use"),
        "应报码化错误 savings-goal.account-in-use，实际 {err:?}"
    );

    ensure_account_not_goal_bound(&conn, "plain").expect("非目标账户应放行");
}

#[test]
fn archive_guard_rejects_bound_account_only_and_points_at_goal_archive() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "plain", "普通卡", "bank", "CNY", 0);
    create_savings_goal(&conn, &goal_input("买房基金", 500_000, None)).expect("创建目标应成功");
    let account_id = only_progress(&conn).goal.account_id.clone();

    let err = ensure_account_can_archive(&conn, &account_id).expect_err("在用目标绑定应被拒");
    assert!(
        err.is_code("account.goal-bound-cannot-archive"),
        "应报码化错误 account.goal-bound-cannot-archive，实际 {err:?}"
    );
    assert!(
        err.to_string().contains("归档"),
        "文案应指向「归档目标本身」，实际 {err}"
    );

    ensure_account_can_archive(&conn, "plain").expect("非目标账户应放行");
}
