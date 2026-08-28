//! 物品（Item）命令层（issue #115 / spec #113 / ADR-0014）：创建与列出在用物品。
//!
//! 组织方式镜像 `commands::categories`：命令外壳（`create_item` / `list_items`）
//! + `*_internal` 复用函数（BDD seam，issue #115 验收：BDD 场景调用本层内部函数）。
//!
//! 接缝约定：
//! - 金额折算走 Amount 接缝（`transaction::amount::convert_to_native`），不另写口径；
//! - 每天使用成本走 `item::cost` 接缝（DailyUsageCost 单一权威），列表不重算口径；
//! - 写入成功后经 `notify` 回调发出 `ledger:changed` 粗粒度失效信号（回调注入式，
//!   仿 `commands::sync` 的 emit 注入先例：生产路径发 `events::LEDGER_CHANGED`，
//!   BDD/测试注入记录闭包断言「写后发信号」这一外部可观察行为）。

#[cfg(test)]
mod tests;

use rusqlite::Connection;
use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::item::cost;
use crate::models::{Item, ItemInput, ItemStatus, ItemWithDailyCost};
use crate::transaction::amount;

/// 列出全部未删除物品，每件附「已用天数」与「每天成本」。
///
/// 目标日按状态取：在用 → 今天（本地时区日历日）；已处置 → 处置日
/// （T1 骨架尚无处置入口，#120 接线后可达，此处口径先行对齐 `item::cost`）。
/// 排序按创建先后（created_at 升序），保证列表稳定。
pub fn list_items_internal(conn: &Connection) -> Result<Vec<ItemWithDailyCost>> {
    let items: Vec<Item> = query_all(
        conn,
        "SELECT id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,note,created_at,updated_at,version,device_id,is_deleted \
         FROM items WHERE is_deleted=0 ORDER BY created_at, id",
        [],
    )?;
    items
        .into_iter()
        .map(|item| {
            let usage = match item.status {
                ItemStatus::InUse => cost::calculate_to_today(
                    item.total_cost_cents,
                    parse_date(&item.purchase_date)?,
                    item.residual_value_cents,
                )?,
                ItemStatus::Disposed => {
                    let disposal_date = item.disposal_date.as_deref().ok_or_else(|| {
                        AppError::Invalid(format!("已处置物品缺少处置日期: {}", item.id))
                    })?;
                    cost::calculate(
                        item.total_cost_cents,
                        parse_date(&item.purchase_date)?,
                        parse_date(disposal_date)?,
                        item.residual_value_cents,
                    )?
                }
            };
            Ok(ItemWithDailyCost {
                item,
                used_days: usage.days,
                per_day_cents: usage.per_day_cents,
            })
        })
        .collect()
}

/// 创建一件在用物品：校验 → 金额折算本位币（Amount 接缝）→ 落库（生成
/// `id` 与审计字段）→ 成功后调用 `notify`（生产路径发 `ledger:changed`）。
///
/// 校验：名称非空、总成本 > 0、购买日期可解析（YYYY-MM-DD）；
/// 币种折算经 [`amount::convert_to_native`]（无汇率即报错，不静默混币种）。
pub fn create_item_internal(
    conn: &Connection,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Invalid("物品名称不能为空".into()));
    }
    if input.total_cost_cents <= 0 {
        return Err(AppError::Invalid("物品总成本必须大于 0".into()));
    }
    parse_date(&input.purchase_date)?; // 校验可解析性（成本计算依赖日历日期）
    let cost_native_cents =
        amount::convert_to_native(conn, input.total_cost_cents, &input.currency_code)?;

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO items \
         (id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,'in_use',NULL,NULL,?7,?8,?9,1,?10,0)",
        rusqlite::params![
            id,
            name,
            input.purchase_date,
            input.total_cost_cents,
            input.currency_code,
            cost_native_cents,
            input.note,
            now,
            now,
            device_id(),
        ],
    )?;
    // 脏标记挂钩（issue #126）：落库成功即置脏，到期则写时顺带触发备份。
    crate::auto_backup::on_write(conn);
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 解析 YYYY-MM-DD 日期字符串；非法格式报错（物品成本计算依赖日历日期）。
fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::Invalid(format!("日期格式无效，应为 YYYY-MM-DD: {s}")))
}

#[tauri::command]
pub fn list_items(db: State<'_, DbState>) -> Result<Vec<ItemWithDailyCost>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    list_items_internal(&conn)
}

#[tauri::command]
pub fn create_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ItemInput,
) -> Result<String> {
    let id = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        create_item_internal(&conn, input, &mut || {
            // 物品是独立领域（非参考数据，ADR-0014）：直接发通用失效信号，
            // 物品 store 与消费界面订阅后自动重拉。
            crate::events::emit_ledger_changed(&app);
        })?
    };
    Ok(id)
}
