//! 储蓄目标域单测（spec #1750 / issue #1751 / #1753，权威层——ADR-0087）：
//! 创建联动（自动建户、other 类型、1:1 绑定、金额码化守卫）、蓄水进度口径
//! （进度 = 专属账户余额，余额缓存、读时派生达成态）与双向推算矩阵（无截止
//! ETA / 有截止所需月存 / 节奏为零不虚构时点——节奏来源闭集二值，issue #1753）。
//!
//! 建库走统一测试数据库工厂（ADR-0084）；造数走公开写入口与工厂种子——进度
//! 变化必须由真实流水写入驱动（余额缓存由产品写路径维护，测试不直写缓存）。

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

/// 推算的固定「今天」：全部推算断言的确定性时点（2026-09-23）。
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
}

/// 造数：指向 `to` 账户的在用定时转账计划（公开写入口，issue #203 形态）——
/// 周期与间隔由调用方给定（折算矩阵用），金额按计划币种。返回计划 id。
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
/**
 * 编辑入参工厂：四字段全量替换形态（名称 / 目标金额 / 截止日 / 手填计划月存）。
 * 计划月存可空（None = 清除），截止日可空（None = 无截止日）。
 */
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

/// 进度行别名：断言处少一排泛型噪音。
type SavingsGoalProgressRow = super::model::SavingsGoalProgress;

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// 蓄水 = 真实转账（公开写入口，ADR-0086 纪律——余额缓存由产品写路径刷新）。
fn transfer(conn: &Connection, from: &str, to: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            kind: TransactionKind::Transfer,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: from.into(),
            to_account_id: Some(to.into()),
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
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
        },
    )
    .expect("转账应成功");
}

/// 取出 = 真实支出（同上，进度如实回落）。
fn spend(conn: &Connection, account_id: &str, amount_cents: i64) {
    ledger_transaction::create(
        conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
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
        },
    )
    .expect("支出应成功");
}

/// 创建联动（AC1）：目标创建成功 → 专属账户经账户域读命令可读（other 类型、
/// 与目标 1:1 绑定、币种 = 账本本位币）；两个目标各绑各的账户（无共享池）。
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

    // 专属账户经账户域读命令可读：other 类型、与目标 1:1 绑定
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

    // 1:1：第二个目标绑定第二个专属账户（共享资金池不在范围，spec #1750）
    create_savings_goal(&conn, &goal_input("教育金", 100_000, None)).expect("第二个目标创建应成功");
    let rows = list_savings_goal_progress(&conn, today()).expect("进度读取应成功");
    assert_eq!(rows.len(), 2);
    let account_ids: std::collections::HashSet<&str> =
        rows.iter().map(|r| r.goal.account_id.as_str()).collect();
    assert_eq!(account_ids.len(), 2, "每个目标绑定各自专属账户");
}

/// 编辑·改名联动（AC1）：目标名权威、账户名随动只读——改名后目标读数与
/// 专属账户名同步更新，绑定不变（1 目标 : 1 账户）；名称未变的编辑照常成功。
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

    // 目标读数与账户列表同步更新（账户域读命令 = 账户列表与各下拉的同一来源）
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

    // 名称未变的编辑照常成功（目标额随之更新）
    update_savings_goal(
        &conn,
        &row.goal.id,
        &update_input("换车基金", 600_000, None, None),
    )
    .expect("名称未变的编辑应成功");
    assert_eq!(only_progress(&conn).goal.target_amount_cents, 600_000);
}

/// 编辑·进度即时生效（AC2）：目标额 / 截止日编辑即时反映到进度读数——改小目标额
/// 立即呈现达成态（读时派生、不落库），截止日可设可清，改回大值即退出达成。
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

    // 改小目标额 → 下一次读数立即达成
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

    // 截止日清除（全量替换：None = 无截止日），达成态不受影响
    update_savings_goal(
        &conn,
        &goal_id,
        &update_input("买车基金", 200_000, None, None),
    )
    .expect("清除截止日应成功");
    let row = only_progress(&conn);
    assert_eq!(row.goal.deadline, None, "截止日可清空");
    assert!(row.achieved);

    // 截止日回设
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

    // 目标额改回大值 → 退出达成（达成是纯展示态，无「已达成」残留）
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

/// 编辑守卫矩阵（AC4 / AC5）：非法输入全部码化拒绝且零落库——编辑尝试后目标行
/// 四字段与基线逐字相同（名称 trim 后空白同空名拒绝，与创建同款目标额校验）。
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

    // （场景, 名称, 目标额, 计划月存, 期望错误码）——截止日非法形态无校验（与创建同）
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

    // 零落库：非法编辑全部拒绝后，基线四字段逐字原样（含未携带改写的截止日）
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

/// 手填「计划月存」可设置与清除（AC3，可空列）：设置后读数回显、清除后归零位。
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

/// 编辑不存在的目标：码化 NotFound（不静默成功、不新建行）。
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
/// 创建守卫（AC1）：目标金额非正数被码化错误拒绝，且零落库——无目标行、
/// 无专属账户（守卫先行 + 同一事务，失败不留任何中间态）。
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

/// 进度口径（AC2）：进度 = 专属账户余额（余额缓存）——初始已存 0、还差 = 目标额；
/// 转入上涨、达成 = 余额 ≥ 目标额、超额为带符号差值；支出如实回落、达成撤销。
#[test]
fn progress_follows_goal_account_balance() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 2_000_000);
    create_savings_goal(&conn, &goal_input("买车基金", 500_000, None)).expect("创建目标应成功");
    let goal_account_id = only_progress(&conn).goal.account_id;

    // 初始：已存 0、还差 = 目标额、未达成
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 0);
    assert_eq!(row.remaining_cents, 500_000);
    assert!(!row.achieved);
    assert_eq!(row.currency_code, "CNY");

    // 蓄水 = 真实转账：进度随之上涨
    transfer(&conn, "src", &goal_account_id, 200_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 200_000, "已存 = 专属账户余额");
    assert_eq!(row.remaining_cents, 300_000);
    assert!(!row.achieved);

    // 补足到目标额：达成态点亮、还差归零
    transfer(&conn, "src", &goal_account_id, 300_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 500_000);
    assert_eq!(row.remaining_cents, 0);
    assert!(row.achieved, "余额 ≥ 目标额即达成");

    // 超额存入：达成保持，还差为带符号负差值（不钳制、无百分比口径）
    transfer(&conn, "src", &goal_account_id, 50_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 550_000);
    assert_eq!(row.remaining_cents, -50_000);
    assert!(row.achieved);

    // 取出 = 真实支出：进度如实回落、达成撤销（无「扣回进度」特殊语义）
    spend(&conn, &goal_account_id, 100_000);
    let row = only_progress(&conn);
    assert_eq!(row.saved_cents, 450_000);
    assert_eq!(row.remaining_cents, 50_000);
    assert!(!row.achieved, "余额跌破目标额即退出达成态");
}

// ---------------------------------------------------------------------------
// 双向推算（issue #1753）：节奏来源闭集二值（计划折算优先 / 手填兜底 / 皆无为零
// 不虚构）、无截止正推 ETA、有截止反推所需月存与落后 / 超前差值。
// ---------------------------------------------------------------------------

/// 无截止 + 计划节奏（AC1 计划分支）：节奏取在用定时转账计划折算月存，ETA =
/// 还差 N 个月（上取整）+ 预计年月（今天所在月 + N）。
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

/// 无截止 + 手填节奏（AC3 无计划分支）：无在用计划时节奏取手填「计划月存」；
/// 有计划时计划优先（闭集二值先计划后手填）；两者皆无 → 节奏与推算全部缺席，
/// 不虚构时点（AC3 双断言的后端侧：时点字段为 None，引导归界面）。
#[test]
fn projection_pace_source_closed_set_and_zero_pace_no_fabrication() {
    let conn = setup();
    create_savings_goal(&conn, &goal_input("买车基金", 1_200_000, None)).expect("创建目标应成功");

    // 两者皆无：节奏为零——推算字段全部缺席（不虚构时点、不虚构月存）
    let row = only_progress(&conn);
    assert_eq!(row.pace_monthly_cents, None, "无计划且未手填 = 节奏为零");
    assert_eq!(row.pace_source, None);
    assert_eq!(row.eta_months, None, "节奏为零不虚构 ETA 月数");
    assert_eq!(row.eta_month, None, "节奏为零不虚构预计年月");
    assert_eq!(row.required_monthly_cents, None);
    assert_eq!(row.pace_delta_cents, None);

    // 手填节奏（无计划）：来源 Manual
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

    // 挂上计划：计划优先于手填（闭集二值的优先序）
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

/// 周期折算矩阵（AC1：月 / 周 / 间隔折算比照订阅花费先例）：年付 ÷12、周付
/// ×52÷12、日付 ×30、间隔均摊 ÷N；逐计划折算后舍入到分再求和；无关联计划
/// 的目标不受他目标计划污染（读口径单点按 to_account 过滤）。
#[test]
fn projection_plan_pace_recurrence_conversion_matrix() {
    let conn = setup();
    tauri_app_lib::test_support::seed_account(&conn, "src", "活期卡", "bank", "CNY", 9_999_999);
    create_savings_goal(&conn, &goal_input("买车基金", 10_000_000, None)).expect("创建目标应成功");
    let goal_account = only_progress(&conn).goal.account_id;

    // 四条在用计划分别折算：周 10000×52/12=43333.33→43333；年 120000÷12=10000；
    // 每 3 月 90000÷3=30000；日 1000×30=30000 → 合计 113333。
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

    // 读口径按 to_account 过滤：另一目标（无关联计划）不被上述计划污染
    create_savings_goal(&conn, &goal_input("教育金", 500_000, None)).expect("第二个目标创建应成功");
    let rows = list_savings_goal_progress(&conn, today()).expect("进度读取应成功");
    let other = rows.iter().find(|r| r.goal.name == "教育金").unwrap();
    assert_eq!(
        other.pace_monthly_cents, None,
        "他目标的手填值缺席 = 节奏为零（不共享他目标的计划节奏）"
    );
    assert_eq!(other.eta_months, None, "节奏为零不虚构 ETA");
}

/// 计划暂停 / 取消后不再计入（AC1）：在用（active）之外的暂停 / 取消计划退出
/// 节奏闭集——暂停后回退手填节奏，取消同理；恢复在用即重新计入。
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

    // 暂停 → 回退手填 50_000（Manual）
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

    // 恢复在用 → 计划重新优先；随后取消（active → cancelled 合法转换）→ 不再计入
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

/// 有截止反推（AC2 / AC4）：所需月存 = 剩余 ÷ 剩余整月数（上取整），落后 / 超前
/// 差值 = 节奏 − 所需；随真实蓄水上涨，所需与差值随之变化（差值随进度变化）。
/// 节奏为零时所需照算（纯目标参数算术）、差值缺席（无节奏可比）。
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

    // 今天 2026-09-23 → 截止 2027-06-30 剩 9 个整月；剩余 1_000_000 → 所需 111_112
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

    // 再蓄 400_000 → 已存 600_000、剩余 600_000 → 所需 66_667、差值转为超前（随进度变化）
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

    // 节奏清零（清除手填、暂停计划）：所需照算、差值缺席
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

    // 达成（余额 ≥ 目标额）：推算字段整体退场、达成态由状态列表达
    transfer(&conn, "src", &goal_account, 800_000);
    let row = only_progress(&conn);
    assert!(row.achieved);
    assert_eq!(row.eta_months, None);
    assert_eq!(row.required_monthly_cents, None);
    assert_eq!(row.pace_delta_cents, None);
}

/// 截止日已过（剩余 > 0）：不反推所需月存、不虚构差值（推算字段全部缺席）。
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

/// 截止落在今天所在月（月差 0）：按 1 个整月计（截止当月仍须把剩余存完）——
/// 所需月存 = 剩余本身；该约定与「截止日已过不反推」由本测试两分支显式钉住。
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

/// 读快照一致性（issue #1699 / #1702 纪律，删除接线即红）：目标行 × 余额 × 关联
/// 计划节奏是同屏口径的多语句读闭包。探针在关联计划读取开始前于另一连接把
/// 计划金额翻倍——
/// - 读闭包无快照保护（红）：余额读旧、节奏读新（翻倍），同屏推算基于异时点
///   数据；
/// - 读闭包收进读事务（绿）：注入写被挡住，节奏与基线同时点。
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

    // 探针：关联计划读取（`FROM scheduled_transactions`，全闭包唯一命中）开始前，
    // 另一连接提交计划金额翻倍。
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

// ---------------------------------------------------------------------------
// 生命周期守卫（issue #1754）：归档 / 取消归档、删除守卫矩阵（余额非零码化拒绝 /
// 余额为零级联软删）、在用目标专属账户禁删守卫。
// ---------------------------------------------------------------------------

use super::{
    archive_savings_goal, delete_savings_goal, ensure_account_not_goal_bound,
    unarchive_savings_goal,
};

/// 归档 / 取消归档（AC 归档）：归档后目标行仍在读数中但状态转 archived，
/// 专属账户与余额原样；取消归档恢复 active。归档不删任何东西。
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

/// 归档不删任何东西（AC 归档不删账户 / 流水 / 计划）：账户仍可读、历史交易仍在、
/// 关联在用计划不受影响（节奏来源仍为 Plan——目标域不改定时计划域状态）。
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

/// 归档不存在的目标：码化 NotFound。
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

/// 删除守卫（AC 删目标·余额非零）：专属账户余额非零被码化拒绝并引导先转出，
/// 零落库——目标行与专属账户原样。
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

    // 零落库：目标行与专属账户都在
    let row = only_progress(&conn);
    assert_eq!(row.goal.id, goal_id, "拒绝删除后目标行原样");
    let accounts = ledger_accounts::list_accounts(&conn).expect("账户读命令应成功");
    assert!(
        accounts.iter().any(|a| a.id == account_id),
        "拒绝删除后专属账户原样"
    );
}

/// 删除守卫（AC 删目标·余额为零）：目标与专属账户级联软删，历史交易仍可读
///（真实蓄水 + 真实支出清零后删除，流水保留）。删除后禁删守卫自然放行。
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

    // 目标与专属账户软删、历史交易保留可查
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

    // 删除后守卫放行（该账户已无在用目标绑定）
    ensure_account_not_goal_bound(&conn, &account_id).expect("已删目标的账户守卫应放行");
}

/// 删除不存在的目标：码化 NotFound，零落库。
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

/// 账户禁删守卫（AC 账户侧删除被拒·域单测面）：命中在用目标绑定 → 码化拒绝；
/// 普通账户（非目标绑定）放行。
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
