use ledger_accounts::{AccountInput, AccountType};
use ledger_savings_goal::SavingsGoalInput;
use tauri::Manager;
use tauri_app_lib::commands::accounts::{
    archive_account, create_account, list_accounts, unarchive_account,
};
use tauri_app_lib::commands::savings_goal::{create_savings_goal, savings_goal_progress};
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new("account-archive-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

async fn create_plain_account(app: &AppHandle, name: &str) -> String {
    create_account(
        app.state(),
        app.clone(),
        AccountInput {
            name: name.into(),
            kind: AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("创建账户应返回成功")
}

async fn archived_flag(app: &AppHandle, id: &str) -> bool {
    let accounts = list_accounts(app.state())
        .await
        .expect("账户域读命令应成功");
    accounts
        .iter()
        .find(|a| a.id == id)
        .expect("账户应经读命令可读")
        .is_archived
}

#[tokio::test]
async fn archive_account_rejects_goal_bound_account_with_code() {
    let (app, _dir) = app_with_db();
    create_savings_goal(
        app.state(),
        app.clone(),
        SavingsGoalInput {
            name: "买房基金".into(),
            target_amount_cents: 100_000,
            deadline: None,
        },
    )
    .await
    .expect("创建目标应返回成功");
    let progress = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    let account_id = progress[0].goal.account_id.clone();

    let err = archive_account(app.state(), app.clone(), account_id.clone())
        .await
        .expect_err("绑定在用储蓄目标的账户归档应被拒绝");
    assert!(
        err.is_code("account.goal-bound-cannot-archive"),
        "应报码化错误 account.goal-bound-cannot-archive，实际 {err:?}"
    );
    assert!(
        err.to_string().contains("归档"),
        "文案应指向「归档目标本身」，实际 {err}"
    );

    assert!(!archived_flag(&app, &account_id).await, "被拒归档不应落库");
}

#[tokio::test]
async fn archive_and_unarchive_account_roundtrip_via_commands() {
    let (app, _dir) = app_with_db();
    let account_id = create_plain_account(&app, "旧银行卡").await;
    assert!(!archived_flag(&app, &account_id).await, "新建账户未归档");

    archive_account(app.state(), app.clone(), account_id.clone())
        .await
        .expect("普通账户归档应成功");
    assert!(
        archived_flag(&app, &account_id).await,
        "归档后读命令披露已归档"
    );

    unarchive_account(app.state(), app.clone(), account_id.clone())
        .await
        .expect("恢复应成功");
    assert!(
        !archived_flag(&app, &account_id).await,
        "恢复后读命令回到未归档"
    );
}
