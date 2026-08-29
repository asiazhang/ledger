//! 物品（Item）命令层（issue #115 / #117 / #118 / #120 / #122 / spec #113 / ADR-0014）：创建、
//! 列出、编辑、处置、软删除物品与「在用物品每天成本合计」聚合。
//!
//! 组织方式镜像 `commands::categories`：命令外壳（`create_item` / `list_items` /
//! `delete_item` / `dispose_item`）+ `*_internal` 复用函数（BDD seam，验收：BDD 场景调用本层内部函数）。
//!
//! 接缝约定：
//! - 金额折算走 Amount 接缝（`transaction::amount::convert_to_native`），不另写口径；
//! - 每天使用成本走 `item::cost` 接缝（DailyUsageCost 单一权威），列表不重算口径；
//! - 写入成功后经 `notify` 回调发出 `ledger:changed` 粗粒度失效信号（回调注入式，
//!   仿 `commands::sync` 的 emit 注入先例：生产路径发 `events::LEDGER_CHANGED`，
//!   BDD/测试注入记录闭包断言「写后发信号」这一外部可观察行为）；
//! - 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域
//!   零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

#[cfg(test)]
mod tests;

use rusqlite::{Connection, OptionalExtension};
use tauri::State;

use crate::db::query::{query_all, query_one};
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::item::cost;
use crate::models::{
    Item, ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemStatus, ItemWithDailyCost,
};
use crate::transaction::amount::{self, TransactionKind};

/// 按 `id` 读未删除物品（多命令共用的前检）：不存在（或已软删除）返回 `None`。
fn get_item_by_id(conn: &Connection, id: &str) -> Result<Option<Item>> {
    query_one(
        conn,
        "SELECT id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted \
         FROM items WHERE id=?1 AND is_deleted=0",
        [id],
    )
}

/// 列出全部未删除物品，每件附「已用天数」与「每天成本」。
///
/// 目标日按状态取：在用 → 今天（本地时区日历日）；已处置 → 处置日
/// （T1 骨架尚无处置入口，#120 接线后可达，此处口径先行对齐 `item::cost`）。
/// 排序按创建先后（created_at 升序），保证列表稳定。
pub fn list_items_internal(conn: &Connection) -> Result<Vec<ItemWithDailyCost>> {
    let items: Vec<Item> = query_all(
        conn,
        "SELECT id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted \
         FROM items WHERE is_deleted=0 ORDER BY created_at, id",
        [],
    )?;
    items
        .into_iter()
        .map(|item| {
            let usage = daily_usage(&item)?;
            Ok(ItemWithDailyCost {
                item,
                used_days: usage.days,
                numerator_cents: usage.numerator_cents,
                per_day_cents: usage.per_day_cents,
            })
        })
        .collect()
}

/// 以物品自身字段（分子口径：总成本 − 残值，下限 0）向指定目标日计算，
/// 列表缺省口径与自选参考日重算共用（口径全在 `item::cost` 接缝）。
fn usage_to(item: &Item, target_date: chrono::NaiveDate) -> Result<cost::DailyUsageCost> {
    cost::calculate(
        item.total_cost_cents,
        parse_date(&item.purchase_date)?,
        target_date,
        item.residual_value_cents,
    )
}

/// 按物品生命周期状态计算每天使用成本（在用 → 今天；已处置 → 处置日），
/// 列表与单件详情共用同一口径（`item::cost` 接缝）。
fn daily_usage(item: &Item) -> Result<cost::DailyUsageCost> {
    match item.status {
        ItemStatus::InUse => usage_to(item, cost::today()),
        ItemStatus::Disposed => {
            let disposal_date = item
                .disposal_date
                .as_deref()
                .ok_or_else(|| AppError::Invalid(format!("已处置物品缺少处置日期: {}", item.id)))?;
            usage_to(item, parse_date(disposal_date)?)
        }
    }
}

/// 计算单件物品的每天使用成本（issue #121 自选参考日重算）：
/// `reference_date` 缺省 → 沿用列表口径（在用 → 今天；已处置 → 处置日，见 [`daily_usage`]）；
/// 提供参考日 → 覆盖目标日（在用/已处置均生效，分子口径不变：总成本 − 残值，下限 0），
/// 支持未来日期（预览「用满 N 天」的摊薄）。参考日早于购买日或不可解析报错，
/// 口径全部收敛在 `item::cost` 接缝，本函数只做读取与缺省目标日选择。
pub fn calculate_item_cost_internal(
    conn: &Connection,
    id: &str,
    reference_date: Option<&str>,
) -> Result<ItemDailyCost> {
    let item =
        get_item_by_id(conn, id)?.ok_or_else(|| AppError::NotFound(format!("物品不存在: {id}")))?;
    let usage = match reference_date {
        Some(date) => usage_to(&item, parse_date(date)?),
        None => daily_usage(&item),
    }?;
    Ok(ItemDailyCost {
        used_days: usage.days,
        numerator_cents: usage.numerator_cents,
        per_day_cents: usage.per_day_cents,
    })
}

/// 创建一件在用物品：溯源守卫 → 校验 → 金额折算本位币（Amount 接缝）→
/// 落库（生成 `id` 与审计字段）→ 成功后调用 `notify`（生产路径发 `ledger:changed`）。
///
/// 溯源守卫（issue #207，ADR-0025 创建唯一入口）：不关联购买交易的创建请求
/// 直接拒绝——物品只能经交易右键「加入物品」+ 确认弹窗创建，无溯源物品会让
/// 每天成本口径失真；列仍可空是为修改语义（None = 保留）与外键清理保留，
/// 必填仅约束创建时刻。
///
/// 其余校验：名称非空、总成本 > 0、购买日期可解析（YYYY-MM-DD）；
/// 币种折算经 [`amount::convert_to_native`]（无汇率即报错，不静默混币种）。
pub fn create_item_internal(
    conn: &Connection,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    // 溯源守卫：创建必须关联购买交易（修改路径不受此限，None = 保留既有溯源）。
    if input.purchase_transaction_id.is_none() {
        return Err(AppError::Invalid(
            "物品必须关联一笔购买交易创建：请在交易页右键一笔支出交易，选择「加入物品」".into(),
        ));
    }
    // 关联购买交易：校验存在且为 expense，自动带出日期/成本/币种（覆盖同名入参）。
    let effective = apply_purchase_link(conn, &input)?;
    let (name, purchase_date, cost_native_cents) = validate_and_convert(conn, &effective)?;

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO items \
         (id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,'in_use',NULL,NULL,?7,?8,?9,?10,1,?11,0)",
        rusqlite::params![
            id,
            name,
            purchase_date,
            effective.total_cost_cents,
            effective.currency_code,
            cost_native_cents,
            input.purchase_transaction_id,
            input.note,
            now,
            now,
            device_id(),
        ],
    )?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 创建/修改共用的入参校验与归一化：名称非空、总成本 > 0、购买日期可解析
/// （成本计算依赖日历日期）、币种可按 Amount 接缝折算本位币。
/// 返回归一化后的名称、规范化日期串（YYYY-MM-DD）与本位币成本，调用方直接落库。
fn validate_and_convert(conn: &Connection, input: &ItemInput) -> Result<(String, String, i64)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Invalid("物品名称不能为空".into()));
    }
    if input.total_cost_cents <= 0 {
        return Err(AppError::Invalid("物品总成本必须大于 0".into()));
    }
    let purchase_date = parse_date(&input.purchase_date)?;
    let cost_native_cents =
        amount::convert_to_native(conn, input.total_cost_cents, &input.currency_code)?;
    Ok((
        name.to_string(),
        purchase_date.format("%Y-%m-%d").to_string(),
        cost_native_cents,
    ))
}

/// 按 `id` 修改物品字段（名称/购买日期/总成本/币种/备注/关联购买交易，
/// issue #117 / #119）。
///
/// 保留审计字段：`id` / `created_at` / `status` / 处置相关字段 / `is_deleted`
/// 均不动；`version` 递增、`updated_at` / `device_id` 刷新（同 Writer 接缝的
/// `update_row` 约定）。金额折算走 Amount 接缝，成功后调用 `notify`
/// （生产路径发 `ledger:changed`）。物品不存在（或已软删除）→ [`AppError::NotFound`]。
///
/// 关联购买交易语义：入参提供新交易 → 校验并自动带出（覆盖日期/成本/币种，
/// 替换溯源）；入参为 `None` → 保留既有溯源不动（溯源只增不减，不随编辑丢失）。
pub fn update_item_internal(
    conn: &Connection,
    id: &str,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    let existing = get_item_by_id(conn, id)?;
    let Some(existing) = existing else {
        return Err(AppError::NotFound("物品不存在".into()));
    };

    // 关联购买交易语义：新关联或换关（与既有指针不同）→ 校验并自动带出
    // （覆盖日期/成本/币种）；维持既有关联或不带关联 → 不重新带出，
    // 手动调整的成本/日期照常生效（溯源只增不减，None 不等于取消关联）。
    let effective = match &input.purchase_transaction_id {
        Some(tx_id) if Some(tx_id.as_str()) != existing.purchase_transaction_id.as_deref() => {
            apply_purchase_link(conn, &input)?
        }
        _ => input.clone(),
    };
    let link = input
        .purchase_transaction_id
        .clone()
        .or(existing.purchase_transaction_id);

    let (name, purchase_date, cost_native_cents) = validate_and_convert(conn, &effective)?;

    // 已处置物品的购买日期不得晚于处置日，否则列表/详情读取时成本口径报错（不可达状态）。
    if let Some(disposal_date) = &existing.disposal_date
        && parse_date(&purchase_date)? > parse_date(disposal_date)?
    {
        return Err(AppError::Invalid(format!(
            "购买日期 {purchase_date} 晚于处置日期 {disposal_date}，请先调整处置日期"
        )));
    }

    let updated = conn.execute(
        "UPDATE items \
         SET name=?2, purchase_date=?3, total_cost_cents=?4, currency_code=?5, \
         cost_native_cents=?6, purchase_transaction_id=?7, note=?8, updated_at=?9, \
         version=version+1, device_id=?10 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            name,
            purchase_date,
            effective.total_cost_cents,
            effective.currency_code,
            cost_native_cents,
            link,
            input.note,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(())
}

/// 处置物品（issue #120）：置 `status='disposed'` 并记录处置日期（必填）与
/// 可选残值。已处置物品的每天成本由 [`daily_usage`] 摊到处置日，
/// 分子 = 总成本 − 残值（口径全部在 `item::cost` 接缝，本函数只写状态字段）。
///
/// 校验：物品存在且未删除；处置日期可解析、不早于购买日期且不晚于今天（否则列表读取时
/// 成本口径报错或分母虚增，均为不可达/错误状态）；残值存在时必须 ≥ 0
/// （残值 ≥ 成本合法，分子下限 0）。
/// 对已处置物品再次处置 = 修正处置信息（更新日期与残值，版本递增）。
/// 成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn dispose_item_internal(
    conn: &Connection,
    id: &str,
    input: ItemDisposeInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    let Some(existing) = get_item_by_id(conn, id)? else {
        return Err(AppError::NotFound(format!("物品不存在: {id}")));
    };

    let disposal_date = parse_date(&input.disposal_date)?;
    let purchase_date = parse_date(&existing.purchase_date)?;
    if disposal_date < purchase_date {
        return Err(AppError::Invalid(format!(
            "处置日期 {disposal_date} 早于购买日期 {purchase_date}，无法处置"
        )));
    }
    if disposal_date > cost::today() {
        // 「已处置」语义上当日之后才成立：未来日期按录入错误拒绝，
        // 避免每天成本按未来处置日摊（分母虚增）。
        return Err(AppError::Invalid(format!(
            "处置日期 {disposal_date} 不能晚于今天"
        )));
    }
    if input.residual_value_cents.is_some_and(|v| v < 0) {
        return Err(AppError::Invalid("残值不能为负".into()));
    }

    let updated = conn.execute(
        "UPDATE items SET status='disposed', disposal_date=?2, residual_value_cents=?3, \
         updated_at=?4, version=version+1, device_id=?5 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            disposal_date.format("%Y-%m-%d").to_string(),
            input.residual_value_cents,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    // 处置成功 → 通知调用方发出失效信号（生产为 ledger:changed）。
    notify();
    Ok(())
}

/// 软删除物品（`is_deleted=1`，不物理移除）：标准列表（`WHERE is_deleted=0`）
/// 自动过滤。不校验引用（物品当前无下游引用）。不存在（含已删除）的 id 返回
/// `AppError::NotFound`。成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn delete_item_internal(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM items WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound(format!("物品不存在: {id}")));
    }
    conn.execute(
        "UPDATE items SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    // 删除成功 → 通知调用方发出失效信号（生产为 ledger:changed）。
    notify();
    Ok(())
}

/// 解析关联购买交易并自动带出（issue #119）：入参带交易 id 时校验交易
/// 存在、未删除且为 `expense`，用交易值覆盖入参的购买日期/总成本/币种
/// （自动带出）；不带关联时原样返回。返回有效入参，调用方继续走统一校验。
fn apply_purchase_link(conn: &Connection, input: &ItemInput) -> Result<ItemInput> {
    let Some(tx_id) = &input.purchase_transaction_id else {
        return Ok(input.clone());
    };
    let (date, cost_cents, currency) = resolve_purchase_link(conn, tx_id)?;
    Ok(ItemInput {
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency,
        ..input.clone()
    })
}

/// 查验关联购买交易：存在（未删除）且为 `expense`，返回
/// （交易日期，金额分，币种）。不存在/已删除 → 参数错误；非 expense → 参数错误；
/// 该交易已被其他未删除物品关联 → 参数错误（溯源唯一，创建与换关两条路径共用本守卫：
/// 同一笔购买只能对应一件物品，避免每天成本被重复计算；软删除物品不占坑，可重新创建）。
fn resolve_purchase_link(conn: &Connection, tx_id: &str) -> Result<(String, i64, String)> {
    let taken: bool = conn
        .query_row(
            "SELECT 1 FROM items WHERE purchase_transaction_id=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![tx_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(AppError::Invalid(format!(
            "该购买交易已创建过物品，不能重复创建（溯源唯一）: {tx_id}"
        )));
    }
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT kind, date, amount_cents, currency_code FROM transactions \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((kind, date, amount_cents, currency)) = row else {
        return Err(AppError::Invalid(format!("关联的购买交易不存在: {tx_id}")));
    };
    if kind != TransactionKind::Expense.as_str() {
        return Err(AppError::Invalid(format!(
            "关联的交易必须是支出类型（实际: {kind}）"
        )));
    }
    Ok((date, amount_cents, currency))
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

/// 全部在用物品「每天成本合计」（issue #122 dashboard 汇总卡）：conn 级聚合，
/// 复用 [`list_items_internal`] 的逐件口径快照（分子/天数均经 `item::cost` 接缝），
/// 本函数只做筛选（仅 `in_use`）、本位币折算（Amount 接缝）与求和，不另写口径表达式。
///
/// 合计口径 = Σ 各在用物品分子（折本位币）÷ 各自天数：每件物品每天都在发生的
/// 持有开销直接相加（「每天成本合计」），**不是** Σ分子 ÷ Σ天数（那是按天数加权的均值，
/// 不回答「每天合计花多少」）。缺汇率的币种错误上抛（与 `dashboard_overview` 同款，
/// 不静默以零计入）；分子为 0（残值 ≥ 成本）的物品计件但不贡献金额。
pub fn item_daily_total_internal(conn: &Connection) -> Result<ItemDailyTotal> {
    let mut per_day_total = 0f64;
    let mut item_count = 0u64;
    for entry in list_items_internal(conn)? {
        if entry.item.status != ItemStatus::InUse {
            continue;
        }
        let numerator_native =
            amount::convert_to_native(conn, entry.numerator_cents, &entry.item.currency_code)?;
        per_day_total += numerator_native as f64 / entry.used_days as f64;
        item_count += 1;
    }
    Ok(ItemDailyTotal {
        native_currency: amount::default_currency_code().to_string(),
        per_day_cents: per_day_total,
        item_count,
    })
}

/// 计算单件物品的每天使用成本（issue #121，只读命令不发失效信号）：
/// `reference_date` 缺省/为 null 时沿用列表口径（在用 → 今天；已处置 → 处置日）。
#[tauri::command]
pub fn calculate_item_cost(
    db: State<'_, DbState>,
    id: String,
    reference_date: Option<String>,
) -> Result<ItemDailyCost> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    calculate_item_cost_internal(&conn, &id, reference_date.as_deref())
}

/// 全部在用物品每天成本合计（issue #122，只读聚合不发失效信号），
/// 供 dashboard 汇总卡展示（默认币种）。
#[tauri::command]
pub fn item_daily_total(db: State<'_, DbState>) -> Result<ItemDailyTotal> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    item_daily_total_internal(&conn)
}

#[tauri::command]
pub fn create_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ItemInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        create_item_internal(conn, input, &mut || {
            // 物品是独立领域（非参考数据，ADR-0014）：直接发通用失效信号，
            // 物品 store 与消费界面订阅后自动重拉。
            crate::events::emit_ledger_changed(&app);
        })
    })
}

#[tauri::command]
pub fn update_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        update_item_internal(conn, &id, input, &mut || {
            // 与 create_item 同款：独立领域写入 → 通用失效信号（issue #117）。
            crate::events::emit_ledger_changed(&app);
        })
    })
}

#[tauri::command]
pub fn dispose_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemDisposeInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        dispose_item_internal(conn, &id, input, &mut || {
            // 物品是独立领域（非参考数据，ADR-0014）：直接发通用失效信号。
            crate::events::emit_ledger_changed(&app);
        })
    })
}

#[tauri::command]
pub fn delete_item(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        delete_item_internal(conn, &id, &mut || {
            // 物品是独立领域（非参考数据，ADR-0014）：直接发通用失效信号。
            crate::events::emit_ledger_changed(&app);
        })
    })
}
