//! 保单（Policy）命令层（issue #360 / spec #358 / ADR-0051）：创建、列出、编辑、
//! 软删除保单（静态档案 CRUD）。缴费协议与保单视角统计由后续票承接，不在本模块。
//!
//! 组织方式镜像 `commands::item`：命令外壳（`create_policy` / `list_policies` /
//! `update_policy` / `delete_policy`）+ `*_internal` 复用函数（BDD seam，
//! 验收：BDD 场景调用本层内部函数）。
//!
//! 接缝约定：
//! - 保单是静态档案，**不进任何金额口径**：保额纯展示，不走 Amount 接缝折算、
//!   不参与聚合（ADR-0051 决策 6 的镜像约束——统计未来按流水推导，不经保单行）；
//! - 保司复用商户字典（ADR-0028）：建档/编辑校验保司为在用商户（软删商户不可
//!   再被新档案选择，与交易侧写入口径一致）；
//! - 写入成功后经 `notify` 回调发出 `ledger:changed` 粗粒度失效信号（回调注入式，
//!   生产路径经信号映射单点 `signals::emit_for` 判定发射，ADR-0044 决策 8）；
//! - 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）。
//!
//! 删除语义（ADR-0051 决策 5）：软删除，已删保单不进列表，其上历史引用保留
//! 不置空——保单是档案非字典（与商户字典行刻意不同）。

#[cfg(test)]
mod tests;

use rusqlite::{Connection, OptionalExtension};
use tauri::State;

use crate::db::query::{query_all, query_one};
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Policy, PolicyInput};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 保单全列清单（读路径共用，与 `FromRow` 的列名约定一致）。
const POLICY_COLUMNS: &str = "id,merchant_id,policy_number,product_name,start_date,end_date,\
     coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted";

/// 按 `id` 读未删除保单（多命令共用的前检）：不存在（或已软删除）返回 `None`。
fn get_policy_by_id(conn: &Connection, id: &str) -> Result<Option<Policy>> {
    query_one(
        conn,
        &format!("SELECT {POLICY_COLUMNS} FROM policies WHERE id=?1 AND is_deleted=0"),
        [id],
    )
}

/// 列出全部未删除保单，排序按创建先后（created_at 升序），保证列表稳定。
/// 已删保单不进列表；到期状态不在此推导（展示层由保障期间即时推导，不持久化）。
pub fn list_policies_internal(conn: &Connection) -> Result<Vec<Policy>> {
    query_all(
        conn,
        &format!(
            "SELECT {POLICY_COLUMNS} FROM policies WHERE is_deleted=0 ORDER BY created_at, id"
        ),
        [],
    )
}

/// 创建一张保单：校验 → 落库（生成 `id` 与审计字段）→ 成功后调用 `notify`
/// （生产路径发 `ledger:changed`）。校验语义见 [`validate_input`]。
pub fn create_policy_internal(
    conn: &Connection,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let normalized = validate_input(conn, &input)?;

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO policies \
         (id,merchant_id,policy_number,product_name,start_date,end_date,\
         coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,1,?11,0)",
        rusqlite::params![
            id,
            normalized.merchant_id,
            normalized.policy_number,
            normalized.product_name,
            normalized.start_date,
            normalized.end_date,
            normalized.coverage_amount_cents,
            normalized.coverage_currency_code,
            normalized.note,
            now,
            device_id(),
        ],
    )?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 按 `id` 编辑保单静态要素（全量替换）：保留审计字段（`id` / `created_at` /
/// `is_deleted`），`version` 递增、`updated_at` / `device_id` 刷新（同 Writer
/// 接缝的 `update_row` 约定）。不存在（或已软删除）→ [`AppError::NotFound`]。
/// 成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn update_policy_internal(
    conn: &Connection,
    id: &str,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    get_policy_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("policy.not-found", format!("保单不存在: {id}"), &[id])
    })?;

    let normalized = validate_input(conn, &input)?;

    let updated = conn.execute(
        "UPDATE policies SET merchant_id=?2, policy_number=?3, product_name=?4, start_date=?5, \
         end_date=?6, coverage_amount_cents=?7, coverage_currency_code=?8, note=?9, \
         updated_at=?10, version=version+1, device_id=?11 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            normalized.merchant_id,
            normalized.policy_number,
            normalized.product_name,
            normalized.start_date,
            normalized.end_date,
            normalized.coverage_amount_cents,
            normalized.coverage_currency_code,
            normalized.note,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    notify();
    Ok(())
}

/// 软删除保单（`is_deleted=1`，不物理移除）：标准列表（`WHERE is_deleted=0`）
/// 自动过滤；库内行与既有引用列**原样保留、不置空**（ADR-0051 决策 5：档案的
/// 历史语义不可毁）。不存在（含已删除）的 id → [`AppError::NotFound`]。
/// 成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn delete_policy_internal(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM policies WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "policy.not-found",
            format!("保单不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE policies SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    notify();
    Ok(())
}

// ---------------------------------------------------------------------------
// 校验与归一化（创建/编辑共用）
// ---------------------------------------------------------------------------

/// 校验结果（归一化后）：trim 非空、日期规范化、保额/币种成对落定。
struct NormalizedInput {
    merchant_id: String,
    policy_number: String,
    product_name: String,
    start_date: String,
    end_date: Option<String>,
    coverage_amount_cents: Option<i64>,
    coverage_currency_code: Option<String>,
    note: Option<String>,
}

/// 创建/编辑共用的入参校验与归一化：
/// - 保司必须为在用商户（软删商户不可再被新档案选择，历史引用不受影响）；
/// - 保单号/险种名称 trim 非空；
/// - 起止日可解析（YYYY-MM-DD），止日存在时不得早于起日（止日可空 = 长期/终身）；
/// - 保额与币种成对：保额存在时必须 > 0 且币种必填、须存在于币种表；
///   保额缺省时币种忽略存空（两者原子，不产生半挂状态）；
/// - 备注 trim，空串归 `None`。
fn validate_input(conn: &Connection, input: &PolicyInput) -> Result<NormalizedInput> {
    // 保司：在用商户（ADR-0028 软删语义 + ADR-0051 决策 7）。
    let merchant_active: bool = conn
        .query_row(
            "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
            rusqlite::params![input.merchant_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !merchant_active {
        return Err(AppError::codedp(
            "policy.merchant-not-found",
            format!("保险公司不存在或已删除: {}", input.merchant_id),
            &[&input.merchant_id],
        ));
    }

    let policy_number = input.policy_number.trim();
    if policy_number.is_empty() {
        return Err(AppError::coded("policy.number-required", "保单号不能为空"));
    }
    let product_name = input.product_name.trim();
    if product_name.is_empty() {
        return Err(AppError::coded(
            "policy.product-required",
            "险种名称不能为空",
        ));
    }

    let start_date = parse_date(&input.start_date)?;
    let end_date = match &input.end_date {
        Some(raw) => {
            let date = parse_date(raw)?;
            if date < start_date {
                return Err(AppError::codedp(
                    "policy.end-before-start",
                    format!("保障期间止日 {raw} 早于起日 {}", input.start_date),
                    &[raw, &input.start_date],
                ));
            }
            Some(date.format("%Y-%m-%d").to_string())
        }
        None => None,
    };

    let (coverage_amount_cents, coverage_currency_code) = match input.coverage_amount_cents {
        Some(cents) => {
            if cents <= 0 {
                return Err(AppError::coded("policy.amount-positive", "保额必须大于 0"));
            }
            let code = input.coverage_currency_code.as_deref().ok_or_else(|| {
                AppError::coded("policy.currency-required", "填写保额时必须选择保额币种")
            })?;
            let known: bool = conn
                .query_row(
                    "SELECT 1 FROM currencies WHERE code=?1",
                    rusqlite::params![code],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            if !known {
                return Err(AppError::codedp(
                    "policy.currency-not-found",
                    format!("未知币种: {code}"),
                    &[code],
                ));
            }
            (Some(cents), Some(code.to_string()))
        }
        // 保额缺省 → 币种忽略存空（成对原子，不产生只有币种的半挂状态）。
        None => (None, None),
    };

    Ok(NormalizedInput {
        merchant_id: input.merchant_id.clone(),
        policy_number: policy_number.to_string(),
        product_name: product_name.to_string(),
        start_date: start_date.format("%Y-%m-%d").to_string(),
        end_date,
        coverage_amount_cents,
        coverage_currency_code,
        note: input
            .note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

/// 解析 YYYY-MM-DD 日期字符串；非法格式报错（保障期间依赖日历日期）。
fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "policy.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}

// ---------------------------------------------------------------------------
// 命令外壳（IPC 面，ADR-0047：build.rs 扫描注解生成清单）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_policies(db: State<'_, DbState>) -> Result<Vec<Policy>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    list_policies_internal(&conn)
}

#[tauri::command]
pub fn create_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PolicyInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        create_policy_internal(conn, input, &mut || {
            // 保单是独立领域（ADR-0051，同物品先例）：复用 `ledger:changed` 同名
            // 事件，保单 store 订阅后自动重拉。发不发由映射单点判定（ADR-0044）。
            emit_for(&app, WriteOp::CreatePolicy, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn update_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PolicyInput,
) -> Result<()> {
    db.write(|conn| {
        update_policy_internal(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::UpdatePolicy, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn delete_policy(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    db.write(|conn| {
        delete_policy_internal(conn, &id, &mut || {
            emit_for(&app, WriteOp::DeletePolicy, WriteEvidence::None);
        })
    })
}
