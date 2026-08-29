//! 定时计划步骤的跨主题私有辅助（issue #263 拆分约定：跨主题 helper 收此）。

use tauri_app_lib::scheduled_transactions::execute_occurrence;

use crate::world::LedgerWorld;

/// 执行期次并记录结果：成功回填 last_transaction_id，失败记录 last_error。
pub fn execute_occurrence_step(world: &mut LedgerWorld, occ_id: &str) {
    world.last_occurrence_id = Some(occ_id.to_string());
    match execute_occurrence(&world_conn!(world), occ_id) {
        Ok(txn_id) => {
            world.last_transaction_id = Some(txn_id);
            world.last_error = None;
        }
        Err(e) => {
            world.last_transaction_id = None;
            world.last_error = Some(e.to_string());
        }
    }
}
