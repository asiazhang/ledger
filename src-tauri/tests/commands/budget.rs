//! 预算命令面集成测试（issue #1948，壳三件套 + 接线证明，ADR-0087 /
//! CONTEXT-testing「壳三件套」「接线证明」）。
//!
//! 断言面只有一步可观察接线证明——年度汇总读命令读到预算域聚合的结果：造数经
//! 域层公开写入口（CONTEXT-testing 造数纪律），域结果细节（年化算术、父子去重、
//! 未设预算分类不进汇总、超标判定）归域单测权威层（`ledger-budget` 的 tests），
//! 此处不越层。

use ledger_budget::{BudgetInput, BudgetPeriod};
use ledger_categories::CategoryInput;
use ledger_infra::db::DbState;
use tauri::Manager;
use tauri_app_lib::commands::budget::budget_annual_summary;
use tauri_app_lib::test_support::ScratchDir;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

/// mock 应用 + 产品文件库（惰性门面路径，ADR-0125 决策 1/4）：返回值第二元是
/// 暂存目录 guard，调用方持有到用例结束（仓库的测试暂存目录纪律）。
fn app_with_db() -> (AppHandle, ScratchDir) {
    let app = tauri::test::mock_app();
    // 写路径副作用与交易域接缝接线（与测试工厂 / 生产启动同形，幂等，进程级）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new("budget-commands-it");
    app.manage(ledger_infra::db::open_db_in(&dir).expect("文件库应可建"));
    (app.handle().clone(), dir)
}

/// 接线证明：建预算（域公开入口）后，年度汇总读命令的年度预算总额按 `monthly × 12`
/// 变化——壳层命令确实进到预算域聚合，且该聚合正是财务自由度分母的同一函数。
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
