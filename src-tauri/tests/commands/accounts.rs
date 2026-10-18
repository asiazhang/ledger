//! 账户归档命令面集成测试（spec #1952 / ADR-0142 / issue #1953，壳三件套 +
//! 接线证明，ADR-0087 / CONTEXT-testing「壳三件套」「接线证明」）。
//!
//! 直调命令函数（`#[tokio::test]`）：断言面只有参数解包的成功 / 错误码形态与
//! 一步可观察接线（归档标志经账户读命令可见）——「绑定储蓄目标的账户拒绝归档」
//! 的守卫住壳层编排（`archive_account` 闭包内先经目标域校验），删除该调用本
//! 测试即红。域结果细节（幂等短路、余额口径不变、重放协议）归域单测权威层
//!（`ledger-accounts` 的 tests），此处不越层。

use ledger_accounts::{AccountInput, AccountType};
use ledger_savings_goal::SavingsGoalInput;
use tauri::Manager;
use tauri_app_lib::commands::accounts::{
    archive_account, create_account, list_accounts, unarchive_account,
};
use tauri_app_lib::commands::savings_goal::{create_savings_goal, savings_goal_progress};
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

/// mock 应用 + 产品文件库（惰性门面路径，ADR-0125 决策 1/4）：返回值第二元是
/// 暂存目录 guard，调用方持有到用例结束（仓库的测试暂存目录纪律）。
fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    // 写路径副作用与交易域接缝接线（与测试工厂 / 生产启动同形，幂等，进程级）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new("account-archive-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

/// 建一个普通（现金）账户，返回其 id（命令面成功即建出）。
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

/// 读某一账户的归档标志（经账户读命令，不经库内细节）。
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

/// 壳三件套（错误码）+ 接线证明负向面：绑定在用储蓄目标的账户被码化拒绝
///（`account.goal-bound-cannot-archive`，文案指向「归档目标本身」），账户仍
/// 未归档——移除 `archive_account` 闭包内的目标域守卫本测试即红。
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

    // 拒绝零落库：账户仍可读、仍未归档
    assert!(!archived_flag(&app, &account_id).await, "被拒归档不应落库");
}

/// 接线证明（端到端一步可观察）：普通账户归档成功 → 读命令披露已归档；
/// 恢复成功 → 读命令回到未归档（新命令的注册与调用面接线全链）。
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
