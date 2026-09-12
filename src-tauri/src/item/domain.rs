//! 物品（Item）域 API（issue #115 / #117 / #118 / #120 / #121 / #122 / spec #113 /
//! ADR-0014）：创建、修改、处置、删除、列表、单件每天成本与「在用物品每天
//! 成本合计」聚合的单一权威——写路径的校验与归一化、读路径的口径接线。
//!
//! 组织方式（阶段 1 域目录化，#397 / ADR-0056）：原 `commands::item` 的
//! `*_internal` 复用函数整体归位，接口正名为域语言短名（`create_item` /
//! `list_items` / `update_item` / `dispose_item` / `delete_item` /
//! `calculate_item_cost` / `item_daily_total`）；IPC 壳层（`commands::item`）
//! 只做参数解包、统一写入口与信号发射，转调本模块。
//!
//! 接缝约定：
//! - 金额折算走 Amount 接缝（`transaction::amount::convert_to_native`），不另写口径；
//! - 每天使用成本走 `item::cost` 接缝（DailyUsageCost 单一权威），列表不重算口径；
//! - 溯源守卫（创建唯一入口的准入接缝）独立在 [`guard`]：
//!   创建/换关路径经其解析关联购买交易并自动带出；
//! - 写入成功后经 `notify` 回调发出 `ledger:changed` 粗粒度失效信号（回调注入式，
//!   仿行情同步域 `sync` 的 emit 注入先例：生产路径经信号映射单点 `signals::emit_for`
//!   判定发射，ADR-0044 决策 8——notify 只是发射钩子不再是决策点；BDD/测试注入
//!   记录闭包断言「写后发信号」这一外部可观察行为）；
//! - 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域
//!   零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::command::{ItemCommand, ItemCommandRow, record_local};
use super::cost;
use super::guard;
use super::guard::apply_purchase_link;
use super::model::{
    Item, ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemSourceDisplay,
    ItemStatus, ItemWithDailyCost,
};
use crate::db::query::{query_all, query_one};
use crate::db::tx_scope::ensure_transaction;
use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::transaction::amount;
use ledger_sync_protocol::device::device_id;

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
pub fn list_items(conn: &Connection) -> Result<Vec<ItemWithDailyCost>> {
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

/// 交易列表来源反查（spec #704 / issue #708，词汇表「来源列」展示层溯源反查）：
/// 按溯源指针（`purchase_transaction_id`）批量取指向给定交易的未删除物品
/// （物品名 + 已处置标志），供核心交易域按页填充来源列——展示层 join，不落
/// 任何数据级反向引用（物品域 source_transaction_id 词条边界：禁令针对数据
/// 模型，不针对展示）。软删物品不返回（跳转会落空，且溯源唯一守卫只看未删除
/// 物品，该交易可再次建物品——来源列同口径视为无来源）。溯源唯一（创建守卫）
/// 使一交易至多一物品，返回仍为 Vec 供调用方按映射消费。
pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<ItemSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT id, purchase_transaction_id, name, status FROM items \
             WHERE purchase_transaction_id IN ({placeholders}) AND is_deleted=0",
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
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
            let disposal_date = item.disposal_date.as_deref().ok_or_else(|| {
                AppError::codedp(
                    "item.disposal-date-missing",
                    format!("已处置物品缺少处置日期: {}", item.id),
                    &[&item.id],
                )
            })?;
            usage_to(item, parse_date(disposal_date)?)
        }
    }
}

/// 计算单件物品的每天使用成本（issue #121 自选参考日重算）：
/// `reference_date` 缺省 → 沿用列表口径（在用 → 今天；已处置 → 处置日，见 [`daily_usage`]）；
/// 提供参考日 → 覆盖目标日（在用/已处置均生效，分子口径不变：总成本 − 残值，下限 0），
/// 支持未来日期（预览「用满 N 天」的摊薄）。参考日早于购买日或不可解析报错，
/// 口径全部收敛在 `item::cost` 接缝，本函数只做读取与缺省目标日选择。
pub fn calculate_item_cost(
    conn: &Connection,
    id: &str,
    reference_date: Option<&str>,
) -> Result<ItemDailyCost> {
    let item = get_item_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("item.not-found", format!("物品不存在: {id}"), &[id])
    })?;
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
pub fn create_item(
    conn: &Connection,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let id = ensure_transaction(conn, || create_within_transaction(conn, &input))?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 创建协议本体（无事务语义，由 [`ensure_transaction`] 包裹）：溯源守卫 → 校验
/// → 源端折算 → 落库 + op 产出（随同一事务提交/回滚）。
fn create_within_transaction(conn: &Connection, input: &ItemInput) -> Result<String> {
    // 溯源守卫：创建必须关联购买交易（修改路径不受此限，None = 保留既有溯源）。
    if input.purchase_transaction_id.is_none() {
        return Err(guard::link_required_error());
    }
    // 关联购买交易：校验存在且为 expense，自动带出日期/成本/币种（覆盖同名入参）。
    let effective = apply_purchase_link(conn, input)?;
    let (name, purchase_date, cost_native_cents) = validate_and_convert(conn, &effective)?;
    let row = ItemCommandRow {
        name,
        purchase_date,
        total_cost_cents: effective.total_cost_cents,
        currency_code: effective.currency_code,
        cost_native_cents,
        purchase_transaction_id: input.purchase_transaction_id.clone(),
        note: input.note.clone(),
    };
    let id = write_create(conn, &new_uuid(), &row)?;
    // op 产出接缝（issue #860）：创建成功后随同一事务追加 op。
    record_local(
        conn,
        ItemCommand::Create {
            id: id.clone(),
            row,
        },
    )?;
    Ok(id)
}

/// 物品行落库协议（本地创建与重放共用，无 op 产出）：插入在用行（处置字段空）。
fn write_create(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO items \
         (id,name,purchase_date,total_cost_cents,currency_code,cost_native_cents,status, \
         disposal_date,residual_value_cents,purchase_transaction_id,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,'in_use',NULL,NULL,?7,?8,?9,?10,1,?11,0)",
        rusqlite::params![
            id,
            row.name,
            row.purchase_date,
            row.total_cost_cents,
            row.currency_code,
            row.cost_native_cents,
            row.purchase_transaction_id,
            row.note,
            now,
            now,
            device_id(conn)?,
        ],
    )?;
    Ok(id.to_string())
}

/// 创建/修改共用的入参校验与归一化：名称非空、总成本 > 0、购买日期可解析
/// （成本计算依赖日历日期）、币种可按 Amount 接缝折算本位币。
/// 返回归一化后的名称、规范化日期串（YYYY-MM-DD）与本位币成本，调用方直接落库。
fn validate_and_convert(conn: &Connection, input: &ItemInput) -> Result<(String, String, i64)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::coded("item.name-required", "物品名称不能为空"));
    }
    if input.total_cost_cents <= 0 {
        return Err(AppError::coded(
            "item.cost-positive",
            "物品总成本必须大于 0",
        ));
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
pub fn update_item(
    conn: &Connection,
    id: &str,
    input: ItemInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || update_within_transaction(conn, id, &input))?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(())
}

/// 修改协议本体（无事务语义，由 [`ensure_transaction`] 包裹）：带出/校验 →
/// 落库 + op 产出（随同一事务提交/回滚）。
fn update_within_transaction(conn: &Connection, id: &str, input: &ItemInput) -> Result<()> {
    let existing = get_item_by_id(conn, id)?;
    let Some(existing) = existing else {
        return Err(AppError::codedp_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
            &[id],
        ));
    };

    // 关联购买交易语义：新关联或换关（与既有指针不同）→ 校验并自动带出
    // （覆盖日期/成本/币种）；维持既有关联或不带关联 → 不重新带出，
    // 手动调整的成本/日期照常生效（溯源只增不减，None 不等于取消关联）。
    let effective = match &input.purchase_transaction_id {
        Some(tx_id) if Some(tx_id.as_str()) != existing.purchase_transaction_id.as_deref() => {
            apply_purchase_link(conn, input)?
        }
        _ => input.clone(),
    };
    let link = input
        .purchase_transaction_id
        .clone()
        .or(existing.purchase_transaction_id);

    let (name, purchase_date, cost_native_cents) = validate_and_convert(conn, &effective)?;

    // 已处置物品的购买日期不得晚于处置日，否则列表/详情读取时成本口径报错（不可达状态）。
    if let Some(disposal_date) = &existing.disposal_date {
        ensure_purchase_not_after_disposal(&purchase_date, disposal_date)?;
    }

    let row = ItemCommandRow {
        name,
        purchase_date,
        total_cost_cents: effective.total_cost_cents,
        currency_code: effective.currency_code,
        cost_native_cents,
        purchase_transaction_id: link,
        note: input.note.clone(),
    };
    write_update_row(conn, id, &row)?;
    // op 产出接缝（issue #860）：修改成功后随同一事务追加 op。
    record_local(
        conn,
        ItemCommand::Update {
            id: id.to_string(),
            row,
        },
    )
}

/// 物品行更新协议（本地修改与重放共用，无 op 产出）：替换编辑面字段
/// （status / 处置字段 / is_deleted 不动）。
fn write_update_row(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let updated = conn.execute(
        "UPDATE items \
         SET name=?2, purchase_date=?3, total_cost_cents=?4, currency_code=?5, \
         cost_native_cents=?6, purchase_transaction_id=?7, note=?8, updated_at=?9, \
         version=version+1, device_id=?10 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            row.name,
            row.purchase_date,
            row.total_cost_cents,
            row.currency_code,
            row.cost_native_cents,
            row.purchase_transaction_id,
            row.note,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
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
pub fn dispose_item(
    conn: &Connection,
    id: &str,
    input: ItemDisposeInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    ensure_transaction(conn, || {
        let date = write_dispose(conn, id, &input)?;
        record_local(
            conn,
            ItemCommand::Dispose {
                id: id.to_string(),
                disposal_date: date,
                residual_value_cents: input.residual_value_cents,
            },
        )
    })?;
    // 处置成功 → 通知调用方发出失效信号（生产为 ledger:changed）。
    notify();
    Ok(())
}

/// 物品处置协议（本地处置与重放共用，无 op 产出）：校验 + 状态流转，返回
/// 规范化后的处置日期（YYYY-MM-DD）。
fn write_dispose(conn: &Connection, id: &str, input: &ItemDisposeInput) -> Result<String> {
    let Some(existing) = get_item_by_id(conn, id)? else {
        return Err(AppError::coded_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
        ));
    };

    let disposal_date = parse_date(&input.disposal_date)?;
    let purchase_date = parse_date(&existing.purchase_date)?;
    if disposal_date < purchase_date {
        return Err(AppError::codedp(
            "item.disposal-before-purchase",
            format!("处置日期 {disposal_date} 早于购买日期 {purchase_date}，无法处置"),
            &[&disposal_date.to_string(), &purchase_date.to_string()],
        ));
    }
    if disposal_date > cost::today() {
        // 「已处置」语义上当日之后才成立：未来日期按录入错误拒绝，
        // 避免每天成本按未来处置日摊（分母虚增）。
        return Err(AppError::codedp(
            "item.disposal-in-future",
            format!("处置日期 {disposal_date} 不能晚于今天"),
            &[&disposal_date.to_string()],
        ));
    }
    if input.residual_value_cents.is_some_and(|v| v < 0) {
        return Err(AppError::coded("item.residual-negative", "残值不能为负"));
    }

    let date = disposal_date.format("%Y-%m-%d").to_string();
    let updated = conn.execute(
        "UPDATE items SET status='disposed', disposal_date=?2, residual_value_cents=?3, \
         updated_at=?4, version=version+1, device_id=?5 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            &date,
            input.residual_value_cents,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    Ok(date)
}

/// 软删除物品（`is_deleted=1`，不物理移除）：标准列表（`WHERE is_deleted=0`）
/// 自动过滤。不校验引用（物品当前无下游引用）。不存在（含已删除）的 id 返回
/// `AppError::NotFound`。成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn delete_item(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, ItemCommand::Delete { id: id.to_string() })
    })?;
    // 删除成功 → 通知调用方发出失效信号（生产为 ledger:changed）。
    notify();
    Ok(())
}

/// 物品软删协议（本地删除与重放共用，无 op 产出）：存在性检查 + 软删。
fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM items WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "item.not-found",
            format!("物品不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE items SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

/// 已处置物品的购买日期不得晚于处置日（本地修改与重放共用的不可达状态守卫，
/// 否则列表/详情读取时成本口径报错）。
fn ensure_purchase_not_after_disposal(purchase_date: &str, disposal_date: &str) -> Result<()> {
    let purchase = parse_date(purchase_date)?;
    let disposal = parse_date(disposal_date)?;
    if purchase > disposal {
        return Err(AppError::codedp(
            "item.purchase-after-disposal",
            format!("购买日期 {purchase_date} 晚于处置日期 {disposal_date}，请先调整处置日期"),
            &[purchase_date, disposal_date],
        ));
    }
    Ok(())
}

/// 解析 YYYY-MM-DD 日期字符串；非法格式报错（物品成本计算依赖日历日期）。
fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "item.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}

/// 重放执行：创建（溯源守卫原样生效：关联交易缺失/被占 → 挂起待裁决；
/// 折算结果随命令携带，不重折算——ADR-0091 决策 3）。
pub(crate) fn replay_create(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let Some(tx_id) = &row.purchase_transaction_id else {
        return Err(guard::link_required_error());
    };
    guard::resolve_purchase_link(conn, tx_id)?;
    write_create(conn, id, row)?;
    Ok(())
}

/// 重放执行：修改（换关时复验溯源守卫；处置先后守卫与本地同语义）。
pub(crate) fn replay_update(conn: &Connection, id: &str, row: &ItemCommandRow) -> Result<()> {
    let existing = get_item_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("item.not-found", format!("物品不存在: {id}"), &[id])
    })?;
    // 换关（与既有指针不同）→ 校验新关联（存在/expense/溯源唯一）。
    if let Some(tx_id) = &row.purchase_transaction_id
        && Some(tx_id.as_str()) != existing.purchase_transaction_id.as_deref()
    {
        guard::resolve_purchase_link(conn, tx_id)?;
    }
    // 已处置物品的购买日期不得晚于处置日（与本地修改同一不可达状态守卫）。
    if let Some(disposal_date) = &existing.disposal_date {
        ensure_purchase_not_after_disposal(&row.purchase_date, disposal_date)?;
    }
    write_update_row(conn, id, row)
}

/// 重放执行：处置（同一协议含全部日期/残值守卫）。
pub(crate) fn replay_dispose(conn: &Connection, id: &str, input: &ItemDisposeInput) -> Result<()> {
    write_dispose(conn, id, input)?;
    Ok(())
}

/// 重放执行：软删除（同一协议含存在性检查）。
pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

/// 全部在用物品「每天成本合计」（issue #122 dashboard 汇总卡）：conn 级聚合，
/// 复用 [`list_items`] 的逐件口径快照（分子/天数均经 `item::cost` 接缝），
/// 本函数只做筛选（仅 `in_use`）、本位币折算（Amount 接缝）与求和，不另写口径表达式。
///
/// 合计口径 = Σ 各在用物品分子（折本位币）÷ 各自天数：每件物品每天都在发生的
/// 持有开销直接相加（「每天成本合计」），**不是** Σ分子 ÷ Σ天数（那是按天数加权的均值，
/// 不回答「每天合计花多少」）。缺汇率的币种错误上抛（与 `dashboard_overview` 同款，
/// 不静默以零计入）；分子为 0（残值 ≥ 成本）的物品计件但不贡献金额。
pub fn item_daily_total(conn: &Connection) -> Result<ItemDailyTotal> {
    let mut per_day_total = 0f64;
    let mut item_count = 0u64;
    for entry in list_items(conn)? {
        if entry.item.status != ItemStatus::InUse {
            continue;
        }
        let numerator_native =
            amount::convert_to_native(conn, entry.numerator_cents, &entry.item.currency_code)?;
        per_day_total += numerator_native as f64 / entry.used_days as f64;
        item_count += 1;
    }
    Ok(ItemDailyTotal {
        native_currency: amount::default_currency_code(conn)?,
        per_day_cents: per_day_total,
        item_count,
    })
}

// ---------------------------------------------------------------------------
// 交易×物品接缝实现（spec #1086 / issue #1092）：来源列③物品反查
// ---------------------------------------------------------------------------

/// 来源列③物品反查实现（核心交易域 `transaction::seams::source` 注册点，#1092）：委托
/// [`source_display_by_transaction_ids`] 并映射为核心交易域来源模型（kind = Item、
/// entity = 物品 id、展示名 = 物品名、已处置 → Disposed 标注——口径零变化，
/// spec #704）；溯源指针为空的行跳过（与迁移前调用方 filter_map 同口径）。
fn item_source_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, crate::transaction::TransactionSource>> {
    Ok(source_display_by_transaction_ids(conn, transaction_ids)?
        .into_iter()
        .filter_map(|row| {
            let txn_id = row.purchase_transaction_id.clone()?;
            Some((
                txn_id,
                crate::transaction::TransactionSource {
                    kind: crate::transaction::TransactionSourceKind::Item,
                    entity_id: row.id.clone(),
                    display_name: row.name.clone(),
                    status: row
                        .is_disposed
                        .then_some(crate::transaction::TransactionSourceStatus::Disposed),
                },
            ))
        })
        .collect())
}

/// 注册物品反查实现（幂等：进程级一次，重复注册保留首次）。调用点在壳层启动
/// 接线与测试建库单点，与生产同形；业务代码不直接调用。
pub fn install_source_hook() {
    crate::transaction::seams::source::register_item_source_resolver(item_source_resolver);
}
