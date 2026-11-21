use ledger_savings_goal::{
    SavingsGoalInput, SavingsGoalPaceSource, SavingsGoalProgress, SavingsGoalUpdateInput,
};
use tauri::Manager;
use tauri_app_lib::commands::accounts::list_accounts;
use tauri_app_lib::commands::savings_goal::{
    create_savings_goal, savings_goal_progress, update_savings_goal,
};
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new("savings-goal-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

#[tokio::test]
async fn create_goal_returns_success_and_bound_account_is_readable() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: Some("2027-06-30".into()),
        },
    )
    .await
    .expect("创建目标应返回成功");

    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1);
    let row = &progress[0];
    assert_eq!(row.goal.id, goal_id);

    let accounts = list_accounts(app.state())
        .await
        .expect("账户域读命令应成功");
    let bound = accounts
        .iter()
        .find(|a| a.id == row.goal.account_id)
        .expect("目标绑定的专属账户应经账户域读命令可读");
    assert_eq!(bound.name, "买车基金");
    assert_eq!(
        bound.kind,
        ledger_accounts::AccountType::Other,
        "专属账户应为 other 类型"
    );
}

#[tokio::test]
async fn create_goal_rejects_non_positive_amount_with_code() {
    let (app, _dir) = app_with_db();
    let err = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "零元目标".into(),
            target_amount_cents: 0,
            deadline: None,
        },
    )
    .await
    .expect_err("非正目标金额应被拒绝");
    assert!(
        err.is_code("savings-goal.target-amount-positive"),
        "应报码化错误 savings-goal.target-amount-positive，实际 {err:?}"
    );

    let progress = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert!(progress.is_empty(), "拒绝创建不应留下目标行");
    let accounts = list_accounts(app.state())
        .await
        .expect("账户域读命令应成功");
    assert!(
        !accounts.iter().any(|a| a.name == "零元目标"),
        "拒绝创建不应建出专属账户"
    );
}

#[tokio::test]
async fn update_goal_returns_success_and_account_is_renamed() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");

    update_savings_goal(
        app.state(),
        app.clone(),
        goal_id,
        SavingsGoalUpdateInput {
            name: "换车基金".into(),
            target_amount_cents: 400_000,
            deadline: Some("2027-12-31".into()),
            planned_monthly_cents: Some(30_000),
        },
    )
    .await
    .expect("编辑目标应返回成功");

    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1);
    let account_id = progress[0].goal.account_id.clone();

    let accounts = list_accounts(app.state())
        .await
        .expect("账户域读命令应成功");
    let bound = accounts
        .iter()
        .find(|a| a.id == account_id)
        .expect("目标绑定的专属账户应可读");
    assert_eq!(bound.name, "换车基金", "改名后账户列表同步更新");
}

#[tokio::test]
async fn update_goal_rejects_non_positive_amount_with_code() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");

    let err = update_savings_goal(
        app.state(),
        app.clone(),
        goal_id,
        SavingsGoalUpdateInput {
            name: "买车基金".into(),
            target_amount_cents: 0,
            deadline: None,
            planned_monthly_cents: None,
        },
    )
    .await
    .expect_err("非正目标金额应被拒绝");
    assert!(
        err.is_code("savings-goal.target-amount-positive"),
        "应报码化错误 savings-goal.target-amount-positive，实际 {err:?}"
    );

    let progress = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1, "拒绝编辑不应删除目标");
}

#[tokio::test]
async fn progress_command_wires_projection() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 1_200_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");

    update_savings_goal(
        app.state(),
        app.clone(),
        goal_id.clone(),
        SavingsGoalUpdateInput {
            name: "买车基金".into(),
            target_amount_cents: 1_200_000,
            deadline: None,
            planned_monthly_cents: Some(50_000),
        },
    )
    .await
    .expect("编辑目标应返回成功");

    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    let row = &progress[0];
    assert_eq!(row.pace_monthly_cents, Some(50_000), "节奏 = 手填计划月存");
    assert_eq!(row.pace_source, Some(SavingsGoalPaceSource::Manual));
    assert!(row.eta_months.is_some(), "无截止 + 有节奏 → ETA 月数在场");
    assert!(row.eta_month.is_some(), "预计年月在场");

    update_savings_goal(
        app.state(),
        app.clone(),
        goal_id,
        SavingsGoalUpdateInput {
            name: "买车基金".into(),
            target_amount_cents: 1_200_000,
            deadline: Some("2030-12-31".into()),
            planned_monthly_cents: None,
        },
    )
    .await
    .expect("编辑目标应返回成功");

    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    let row = &progress[0];
    assert_eq!(row.pace_monthly_cents, None, "节奏为零");
    assert_eq!(row.pace_source, None);
    assert!(
        row.required_monthly_cents.is_some(),
        "所需月存是纯目标参数算术，节奏为零照算"
    );
    assert_eq!(row.pace_delta_cents, None, "无节奏不虚构差值");
    assert_eq!(row.eta_months, None, "有截止日不正推 ETA");
}

use ledger_infra::db::DbState;
use tauri_app_lib::commands::accounts::delete_account;
use tauri_app_lib::commands::savings_goal::{
    archive_savings_goal, delete_savings_goal, unarchive_savings_goal,
};

#[tokio::test]
async fn archive_unarchive_roundtrip_wires_progress_status() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");

    archive_savings_goal(app.state(), app.clone(), goal_id.clone())
        .await
        .expect("归档应返回成功");
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(
        progress[0].goal.status,
        ledger_savings_goal::SavingsGoalStatus::Archived
    );

    unarchive_savings_goal(app.state(), app.clone(), goal_id.clone())
        .await
        .expect("取消归档应返回成功");
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(
        progress[0].goal.status,
        ledger_savings_goal::SavingsGoalStatus::Active
    );

    let accounts = list_accounts(app.state()).await.expect("账户读命令应成功");
    let account_id = progress[0].goal.account_id.clone();
    assert!(accounts.iter().any(|a| a.id == account_id));
}

#[tokio::test]
async fn delete_goal_guards_balance_then_cascades_account() {
    let (app, _dir) = app_with_db();
    let goal_id = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    let account_id = progress[0].goal.account_id.clone();

    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().expect("种子写入锁应可取");
        tauri_app_lib::test_support::seed_account(
            &guard,
            "src",
            "活期卡",
            "bank",
            "CNY",
            2_000_000,
        );
        ledger_transaction::create(&guard, transfer_input("src", &account_id, 100_000))
            .expect("转账应成功");
    }

    let err = delete_savings_goal(app.state(), app.clone(), goal_id.clone())
        .await
        .expect_err("余额非零应被拒绝");
    assert!(
        err.is_code("savings-goal.delete-balance-nonzero"),
        "应报码化错误 savings-goal.delete-balance-nonzero，实际 {err:?}"
    );
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1, "拒绝删除后目标原样");

    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().expect("种子写入锁应可取");
        ledger_transaction::create(
            &guard,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Expense,
                account_id: Some(account_id.clone()),
                funding: Vec::new(),
                ..transfer_input("src", &account_id, 100_000)
            },
        )
        .expect("支出应成功");
    }
    delete_savings_goal(app.state(), app.clone(), goal_id.clone())
        .await
        .expect("余额为零应可删除");
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert!(progress.is_empty(), "删除后目标退出进度读数");
    let accounts = list_accounts(app.state()).await.expect("账户读命令应成功");
    assert!(
        !accounts.iter().any(|a| a.id == account_id),
        "删除后专属账户经账户域读命令再读不存在"
    );
}

#[tokio::test]
async fn delete_account_of_active_goal_is_rejected_by_shell_orchestration() {
    let (app, _dir) = app_with_db();
    let _ = create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    let goal_account = progress[0].goal.account_id.clone();

    let err = delete_account(app.state(), app.clone(), goal_account.clone())
        .await
        .expect_err("在用目标的专属账户应被拒绝");
    assert!(
        err.is_code("savings-goal.account-in-use"),
        "应报码化错误 savings-goal.account-in-use，实际 {err:?}"
    );

    let plain = {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().expect("种子写入锁应可取");
        ledger_accounts::create_account(
            &guard,
            ledger_accounts::AccountInput {
                name: "普通卡".into(),
                kind: ledger_accounts::AccountType::Bank,
                currency_code: "CNY".into(),
                initial_balance_cents: None,
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .expect("建普通账户应成功")
    };
    delete_account(app.state(), app.clone(), plain)
        .await
        .expect("非目标账户删除应成功");
}

fn transfer_input(from: &str, to: &str, amount_cents: i64) -> ledger_transaction::TransactionInput {
    ledger_transaction::TransactionInput {
        funding: Vec::new(),
        kind: ledger_transaction::TransactionKind::Transfer,
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
    }
}
