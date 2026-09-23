//! 储蓄目标命令面集成测试（spec #1750 / issue #1751，壳三件套 + 接线证明，
//! ADR-0087 / CONTEXT-testing「壳三件套」「接线证明」）。
//!
//! 直调命令函数（`#[tokio::test]`）：断言面只有参数解包的成功 / 错误码形态与
//! 一步可观察接线证明——创建返回成功且专属账户经账户域读命令可读、拒绝路径
//! 再读不存在。域结果细节（进度算术、达成判定、联动矩阵）归域单测权威层
//!（`ledger-savings-goal` 的 tests），此处不越层。

use ledger_savings_goal::{SavingsGoalInput, SavingsGoalProgress};
use tauri::Manager;
use tauri_app_lib::commands::accounts::list_accounts;
use tauri_app_lib::commands::savings_goal::{create_savings_goal, savings_goal_progress};
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

/// mock 应用 + 产品文件库（惰性门面路径，ADR-0125 决策 1/4）：返回值第二元是
/// 暂存目录 guard，调用方持有到用例结束（仓库的测试暂存目录纪律）。
fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    // 写路径副作用与交易域接缝接线（与测试工厂 / 生产启动同形，幂等，进程级）：
    // 本套件经产品建缝拿文件库连接（不入测试工厂，ADR-0084 决策 3），建库单点
    // 的注册不覆盖本处，落库前显式接线。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    // 库走产品开库入口（open_db_in：建连 + 迁移成对，ScratchDir guard 随返回持有，
    // 用例结束整棵删除）——建库两行序的测试侧唯一入口是 test_support 工厂，文件库
    // 形态走产品入口（ADR-0084 决策 3 / sync_channel 先例）。
    let dir = ScratchDir::new("savings-goal-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

/// 接线证明（AC4）：创建目标返回成功，专属账户经账户域读命令可读（other 类型、
/// 与目标 1:1 绑定——绑定在进度读数的 account_id 上可见）。
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

    // 进度读命令可见该目标，且绑定指向专属账户
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1);
    let row = &progress[0];
    assert_eq!(row.goal.id, goal_id);

    // 专属账户经账户域读命令可读：other 类型、名字随目标
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

/// 壳三件套（错误码）+ 接线证明负向面：非正目标金额被码化错误拒绝，
/// 再读不存在——无目标行、无专属账户。
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
