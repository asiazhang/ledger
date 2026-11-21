use ledger_budget::{BudgetInput, BudgetPeriod};
use ledger_categories::CategoryInput;
use ledger_infra::db::DbState;
use tauri::Manager;
use tauri_app_lib::commands::budget::budget_annual_summary;
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new("budget-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

#[tokio::test]
async fn annual_summary_reads_annualized_budget_through_command() {
    let (app, _dir) = app_with_db();

    let before = budget_annual_summary(app.state())
        .await
        .expect("年度汇总读命令应成功");
    assert_eq!(before.annual_budget_total_cents, 0, "未设预算时总额为零");
    assert_eq!(before.spent_cents, 0);
    assert!(!before.over_budget);

    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().expect("种子写入锁应可取");
        let category_id = ledger_categories::create_category(
            &guard,
            CategoryInput {
                name: "餐饮".into(),
                kind: "expense".into(),
                parent_id: None,
                icon: None,
            },
        )
        .expect("建支出分类应成功");
        ledger_budget::create_budget(
            &guard,
            &BudgetInput {
                category_id,
                period: Some(BudgetPeriod::Monthly),
                amount_cents: 50_000,
                start_date: "2026-01-01".into(),
            },
        )
        .expect("建预算应成功");
    }

    let after = budget_annual_summary(app.state())
        .await
        .expect("年度汇总读命令应成功");
    assert_eq!(
        after.annual_budget_total_cents,
        50_000 * 12,
        "月预算按 ×12 年化进年度预算总额"
    );
    assert_eq!(after.spent_cents, 0, "无流水时本年已支为零");
    assert!(!after.over_budget);
}
