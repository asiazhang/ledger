// 测试整体豁免（ADR-0060）：BDD 测试 crate（harness=false）经 cfg(test) 放行六件套，
// 生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use cucumber::World;

#[macro_use]
#[path = "e2e/world.rs"]
mod world;

#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;
#[path = "e2e/backup_steps.rs"]
mod backup_steps;
#[path = "e2e/books_steps.rs"]
mod books_steps;
#[path = "e2e/budget_steps.rs"]
mod budget_steps;
#[path = "e2e/categories_steps.rs"]
mod categories_steps;
#[path = "e2e/common.rs"]
mod common;
#[path = "e2e/dashboard_steps.rs"]
mod dashboard_steps;
#[path = "e2e/data_location_steps.rs"]
mod data_location_steps;
#[path = "e2e/encryption_steps.rs"]
mod encryption_steps;
#[path = "e2e/financial_freedom_steps.rs"]
mod financial_freedom_steps;
#[path = "e2e/fund_trade_steps.rs"]
mod fund_trade_steps;
#[path = "e2e/instruments_steps.rs"]
mod instruments_steps;
#[path = "e2e/insurers_steps.rs"]
mod insurers_steps;
#[path = "e2e/investment_migration_steps.rs"]
mod investment_migration_steps;
#[path = "e2e/investment_trend_steps.rs"]
mod investment_trend_steps;
#[path = "e2e/items_common.rs"]
mod items_common;
#[path = "e2e/items_cost_steps.rs"]
mod items_cost_steps;
#[path = "e2e/items_create_steps.rs"]
mod items_create_steps;
#[path = "e2e/items_dispose_steps.rs"]
mod items_dispose_steps;
#[path = "e2e/items_provenance_steps.rs"]
mod items_provenance_steps;
#[path = "e2e/items_update_steps.rs"]
mod items_update_steps;
#[path = "e2e/log_level_steps.rs"]
mod log_level_steps;
#[path = "e2e/manual_quote_steps.rs"]
mod manual_quote_steps;
#[path = "e2e/merchants_steps.rs"]
mod merchants_steps;
#[path = "e2e/migration_steps.rs"]
mod migration_steps;
#[path = "e2e/physical_asset_disposal_steps.rs"]
mod physical_asset_disposal_steps;
#[path = "e2e/physical_asset_updates_steps.rs"]
mod physical_asset_updates_steps;
#[path = "e2e/physical_assets_steps.rs"]
mod physical_assets_steps;
#[path = "e2e/policies_steps.rs"]
mod policies_steps;
#[path = "e2e/policy_agreement_steps.rs"]
mod policy_agreement_steps;
#[path = "e2e/policy_stats_steps.rs"]
mod policy_stats_steps;
#[path = "e2e/reports_steps.rs"]
mod reports_steps;
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;
#[path = "e2e/search_steps.rs"]
mod search_steps;
#[path = "e2e/startup_failure_steps.rs"]
mod startup_failure_steps;
#[path = "e2e/step_inputs.rs"]
mod step_inputs;
#[path = "e2e/step_verbs.rs"]
mod step_verbs;
#[path = "e2e/sync_steps.rs"]
mod sync_steps;
#[path = "e2e/transactions_edit_steps.rs"]
mod transactions_edit_steps;
#[path = "e2e/transactions_policy_steps.rs"]
mod transactions_policy_steps;
#[path = "e2e/transactions_query_steps.rs"]
mod transactions_query_steps;
#[path = "e2e/transactions_source_steps.rs"]
mod transactions_source_steps;
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;

/// 「目录置只读（0o555）触发转换失败」手段是否可用（issue #793）：仅非 root
/// Unix 成立——root 凭 CAP_DAC_OVERRIDE 无视权限位，非 Unix 无权限位可依；
/// 不可用时 @non-root-only 场景显式跳过，不假红（失败路径由 Linux 非 root
/// CI 覆盖；与 #791 的 db 单测 readonly_trigger_available 守卫同根源同策略）。
/// 经 /proc/self 属主读有效 uid（零新依赖）；无 /proc 的 Unix 平台按非 root
/// 处理（票面针对 Linux root）。
fn readonly_trigger_available() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata("/proc/self") {
            Ok(meta) => meta.uid() != 0,
            Err(_) => true,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[tokio::main]
async fn main() {
    // @non-root-only 场景（encryption.feature 两个转换失败原子性场景）靠
    // Unix 权限位触发失败路径，root 环境显式跳过不假红（issue #793，与
    // #791 db 单测守卫同策略）。
    if !readonly_trigger_available() {
        eprintln!(
            "root/非 Unix 环境：跳过 @non-root-only 场景（目录只读触发手段依赖非 root，失败路径由 Linux 非 root CI 覆盖，issue #793）"
        );
    }
    world::LedgerWorld::filter_run("tests/e2e/features", |_, _, scenario| {
        readonly_trigger_available() || !scenario.tags.iter().any(|tag| tag == "non-root-only")
    })
    .await;
}
