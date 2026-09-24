//! 储蓄目标命令面集成测试（spec #1750 / issue #1751，壳三件套 + 接线证明，
//! ADR-0087 / CONTEXT-testing「壳三件套」「接线证明」）。
//!
//! 直调命令函数（`#[tokio::test]`）：断言面只有参数解包的成功 / 错误码形态与
//! 一步可观察接线证明——创建返回成功且专属账户经账户域读命令可读、拒绝路径
//! 再读不存在。域结果细节（进度算术、达成判定、联动矩阵）归域单测权威层
//!（`ledger-savings-goal` 的 tests），此处不越层。

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

/// 接线证明（AC1）：编辑目标返回成功，专属账户经账户域读命令可读出新名——
/// 改名联动落在账户列表（各下拉同一参考来源）上一步可观察。
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

    // 进度读命令可见编辑后的目标（读出的绑定指向同一专属账户）
    let progress: Vec<SavingsGoalProgress> = savings_goal_progress(app.state())
        .await
        .expect("进度读命令应成功");
    assert_eq!(progress.len(), 1);
    let account_id = progress[0].goal.account_id.clone();

    // 专属账户经账户域读命令读出新名（账户列表 / 各下拉同一参考来源）
    let accounts = list_accounts(app.state())
        .await
        .expect("账户域读命令应成功");
    let bound = accounts
        .iter()
        .find(|a| a.id == account_id)
        .expect("目标绑定的专属账户应可读");
    assert_eq!(bound.name, "换车基金", "改名后账户列表同步更新");
}

/// 壳三件套（错误码）+ 接线证明负向面：编辑目标金额非正数（与创建同校验）被
/// 码化错误拒绝，再读目标仍在且金额未变（拒绝零落库）。
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

/// 接线证明（issue #1753）：进度读命令携带双向推算——手填节奏 + 无截止 ETA、
/// 关联计划节奏优先（经定时计划命令挂计划后节奏来源切到 Plan）。域算术细节
/// 归域单测，此处只证接线一步可观察。
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

    // 手填节奏（编辑命令，issue #1752）：读命令应回显节奏与 ETA
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

    // 有截止反推（编辑命令设截止日 + 挂计划的两分支归域单测权威层）：清节奏后
    // 读命令仍给出所需月存（纯目标参数算术）、差值缺席。
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

// ---------------------------------------------------------------------------
// 生命周期守卫（issue #1754）：归档 / 取消归档 / 删除守卫 + 账户禁删壳层编排。
// ---------------------------------------------------------------------------

use ledger_infra::db::DbState;
use tauri_app_lib::commands::accounts::delete_account;
use tauri_app_lib::commands::savings_goal::{
    archive_savings_goal, delete_savings_goal, unarchive_savings_goal,
};

/// 接线证明（AC 归档）：归档 / 取消归档命令往返——进度读命令 status 字段可见
/// archived / active 切换；专属账户全程可读（归档不删账户）。
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

    // 账户侧读命令全程可见专属账户（归档不删任何东西）
    let accounts = list_accounts(app.state()).await.expect("账户读命令应成功");
    let account_id = progress[0].goal.account_id.clone();
    assert!(accounts.iter().any(|a| a.id == account_id));
}

/// 壳三件套（错误码）+ 接线证明负向面：余额非零删除被码化拒绝（引导先转出），
/// 目标与专属账户原样；真实支出清零后删除成功，专属账户经账户域读命令再读
/// 不存在、目标退出进度读数。
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

    // 造数走真实 transfer 蓄水（余额缓存由产品写路径维护）：命令测试无独立写
    // 命令面向目标账户，经 DbState 连接直驱交易域公开入口。
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

    // 清零（真实支出冲销余额）后删除：目标退出进度读数、专属账户再读不存在
    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().expect("种子写入锁应可取");
        ledger_transaction::create(
            &guard,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Expense,
                account_id: account_id.clone(),
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

/// 接线证明（AC 账户侧删除被拒·壳层编排，删除编排调用本测试即红）：删除在用
/// 目标的专属账户被码化拒绝（账户域不反向依赖目标域，守卫经壳层编排落在删除
/// 账户命令入口）；普通账户（非目标绑定）照常删除成功。
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

    // 普通账户（非目标绑定）照常可删——守卫只拦目标绑定账户
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

/// 造数：真实 transfer 输入（公开写入口，域单测同款全字段形态）。
fn transfer_input(from: &str, to: &str, amount_cents: i64) -> ledger_transaction::TransactionInput {
    ledger_transaction::TransactionInput {
        kind: ledger_transaction::TransactionKind::Transfer,
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
    }
}
