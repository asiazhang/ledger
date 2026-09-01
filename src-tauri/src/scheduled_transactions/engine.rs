use chrono::Datelike;
use rusqlite::{Connection, OptionalExtension};

use crate::db::query::{query_all, query_one};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer;

use super::models::*;

/// 默认预生成窗口大小（期次数）。
const WINDOW_SIZE: i64 = 12;

// ---------------------------------------------------------------------------
// 计划管理
// ---------------------------------------------------------------------------

/// 创建定时交易计划（核心 + 扩展表 + 预生成期次）。
pub fn create_plan(conn: &Connection, input: CreateScheduledInput) -> Result<String> {
    if input.amount_cents <= 0 {
        return Err(AppError::coded(
            "scheduled-plan.amount-positive",
            "金额必须大于 0",
        ));
    }
    if let Some(ref to_acc) = input.to_account_id
        && to_acc == &input.account_id
    {
        return Err(AppError::coded(
            "scheduled-plan.same-account",
            "转出账户不能等于转入账户",
        ));
    }
    // 定时转账（issue #203，词汇表 ScheduledTransfer 边界）：核心交易域转账交易是
    // 单金额单币种，转出与转入账户必须同币种，不一致在创建入口显式拒绝。
    if input.kind == ScheduledKind::ScheduledTransfer
        && let Some(ref to_acc) = input.to_account_id
    {
        let account_currency = |id: &str, code: &str, missing: &str| -> Result<String> {
            conn.query_row(
                "SELECT currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .map_err(|_| AppError::coded_not_found(code, missing))
        };
        let from_currency = account_currency(
            &input.account_id,
            "scheduled-plan.from-account-not-found",
            "转出账户不存在",
        )?;
        let to_currency = account_currency(
            to_acc,
            "scheduled-plan.to-account-not-found",
            "转入账户不存在",
        )?;
        if from_currency != to_currency {
            return Err(AppError::coded(
                "scheduled-plan.currency-mismatch",
                "转出账户与转入账户币种不一致，定时转账不支持跨币种",
            ));
        }
    }
    // 商户收口（issue #190 / ADR-0028）：installment/subscription 可携带商户，
    // 携带的商户必须存在且未软删除（软删商户不可再被新计划选择，与交易写入
    // 共用 writer 接缝的校验）；scheduled_transfer 不使用商户（用 to_account_id
    // 表示本方账户间转账），行为层拒绝携带。
    match input.kind {
        ScheduledKind::ScheduledTransfer => {
            if input.merchant_id.is_some() {
                return Err(AppError::coded(
                    "scheduled-plan.transfer-merchant-forbidden",
                    "定时转账不能携带商户",
                ));
            }
        }
        ScheduledKind::Installment | ScheduledKind::Subscription => {
            writer::validate_merchant_active(conn, input.merchant_id.as_deref())?;
        }
    }
    // 保单引用准入（issue #362 / ADR-0051 决策 2）：保费协议 = 订阅形态，只有
    // 订阅可携带保单引用，分期/定时转账携带即在行为层显式拒绝；携带的保单必须
    // 存在且未软删除（软删保单不可被新协议选择，共用 Writer 接缝的保单校验）。
    if input.policy_id.is_some() {
        if input.kind != ScheduledKind::Subscription {
            return Err(AppError::coded(
                "scheduled-plan.policy-subscription-only",
                "只有订阅形态协议可挂保单",
            ));
        }
        writer::validate_policy_active(conn, input.policy_id.as_deref())?;
    }

    let id = new_uuid();
    let now = now_iso();
    let kind_str = input.kind.to_string();

    conn.execute(
        "INSERT INTO scheduled_transactions \
         (id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'active',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,1,?14,0)",
        rusqlite::params![
            id,
            kind_str,
            input.account_id,
            input.category_id,
            input.amount_cents,
            input.currency_code,
            input.recurrence_type.to_string(),
            input.recurrence_interval,
            input.recurrence_day,
            input.start_date,
            input.note,
            now,
            now,
            device_id(),
        ],
    )?;

    match input.kind {
        ScheduledKind::Installment => {
            let total = input.total_amount_cents.ok_or_else(|| {
                AppError::coded(
                    "scheduled-plan.installment-total-required",
                    "分期计划必须指定总金额",
                )
            })?;
            let total_occ = input.total_occurrences.ok_or_else(|| {
                AppError::coded(
                    "scheduled-plan.installment-occurrences-required",
                    "分期计划必须指定总期数",
                )
            })?;
            if total < total_occ {
                return Err(AppError::coded(
                    "scheduled-plan.installment-total-too-small",
                    "总金额不能小于期数",
                ));
            }
            conn.execute(
                "INSERT INTO installment_plans (scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences) \
                 VALUES (?1,?2,?3,?4)",
                rusqlite::params![id, input.merchant_id, total, total_occ],
            )?;
        }
        ScheduledKind::Subscription => {
            conn.execute(
                "INSERT INTO subscription_plans (scheduled_transaction_id,merchant_id,policy_id) VALUES (?1,?2,?3)",
                rusqlite::params![id, input.merchant_id, input.policy_id],
            )?;
        }
        ScheduledKind::ScheduledTransfer => {
            let to_acc = input.to_account_id.ok_or_else(|| {
                AppError::coded(
                    "scheduled-plan.to-account-required",
                    "定时转账必须指定目标账户",
                )
            })?;
            conn.execute(
                "INSERT INTO scheduled_transfer_plans (scheduled_transaction_id,to_account_id,total_occurrences) \
                 VALUES (?1,?2,?3)",
                rusqlite::params![id, to_acc, input.total_occurrences],
            )?;
        }
    }

    expand_occurrences(conn, &id)?;

    Ok(id)
}

/// 更新计划状态（暂停/恢复/取消）。
pub fn update_plan_status(conn: &Connection, id: &str, new_status: ScheduledStatus) -> Result<()> {
    let st: ScheduledTransaction = query_one(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| AppError::coded_not_found("scheduled-plan.not-found", "定时计划不存在"))?;

    let current: ScheduledStatus = st.status.parse()?;
    match (current, new_status) {
        (ScheduledStatus::Active, ScheduledStatus::Paused)
        | (ScheduledStatus::Paused, ScheduledStatus::Active)
        | (ScheduledStatus::Active, ScheduledStatus::Cancelled)
        | (ScheduledStatus::Paused, ScheduledStatus::Cancelled) => {}
        (_, ScheduledStatus::Completed) => {
            return Err(AppError::coded(
                "scheduled-plan.manual-complete-forbidden",
                "不能手动将计划设为 completed",
            ));
        }
        _ => {
            let from = format!("{current:?}");
            let to = format!("{new_status:?}");
            return Err(AppError::codedp(
                "scheduled-plan.status-transition-forbidden",
                format!("不允许从 {from} 转换到 {to}"),
                &[&from, &to],
            ));
        }
    }

    let status_str = new_status.to_string();
    let now = now_iso();
    conn.execute(
        "UPDATE scheduled_transactions SET status=?2, updated_at=?3, version=version+1, device_id=?4 WHERE id=?1",
        rusqlite::params![id, status_str, now, device_id()],
    )?;

    // 取消时把所有 pending 期次置为 cancelled
    if new_status == ScheduledStatus::Cancelled {
        conn.execute(
            "UPDATE scheduled_transaction_occurrences SET status='cancelled', updated_at=?2, version=version+1, device_id=?3 \
             WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0",
            rusqlite::params![id, now, device_id()],
        )?;
    }

    Ok(())
}

/// 编辑订阅计划的非金额字段（issue #162，ADR-0023 决策三；商户 issue #190）。
///
/// 仅允许备注、分类、扣款账户、商户；请求携带金额字段时显式拒绝并提示
/// 「改价 = 取消旧计划 + 新建」。编辑不改任何已生成的期次与交易：
/// 期次执行时从计划读取 account_id / category_id / note / 商户（金额取自期次行），
/// 因此编辑天然只影响未来期次。商户为**全量替换**语义：提交值与扩展表当前值
/// 相同视为保持历史引用（软删商户照常保留），变更时校验新商户在用。
pub fn update_subscription(conn: &Connection, input: UpdateSubscriptionInput) -> Result<()> {
    if input.amount_cents || input.total_amount_cents {
        return Err(AppError::coded(
            "scheduled-plan.edit-amount-forbidden",
            "订阅金额不可编辑：改价 = 取消旧计划 + 新建（按新金额重建计划，保留两段真实价格历史）",
        ));
    }

    let st: ScheduledTransaction = query_one(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![&input.id],
    )?
    .ok_or_else(|| AppError::coded_not_found("scheduled-plan.not-found", "定时计划不存在"))?;

    if st.kind != ScheduledKind::Subscription {
        return Err(AppError::coded(
            "scheduled-plan.edit-subscription-only",
            "仅订阅计划支持编辑非金额字段",
        ));
    }

    // 商户（issue #190）：与 writer 编辑路径同语义——提交值与当前引用相同视为
    // 保持历史引用（软删商户照常保留），变更时校验新商户在用（不可选软删商户）。
    let current_merchant: Option<String> = conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![&input.id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if input.merchant_id != current_merchant {
        writer::validate_merchant_active(conn, input.merchant_id.as_deref())?;
    }

    conn.execute(
        "UPDATE scheduled_transactions SET account_id=?2, category_id=?3, note=?4, \
         updated_at=?5, version=version+1, device_id=?6 WHERE id=?1",
        rusqlite::params![
            &input.id,
            &input.account_id,
            &input.category_id,
            &input.note,
            now_iso(),
            device_id(),
        ],
    )?;
    conn.execute(
        "UPDATE subscription_plans SET merchant_id=?2 WHERE scheduled_transaction_id=?1",
        rusqlite::params![&input.id, input.merchant_id],
    )?;

    // 保单引用不在编辑面（issue #362）：订阅编辑只影响未来期次的非核心字段，
    // 「这段协议属于哪张保单」是身份要素、创建后不可改（改挂保单无业务场景；
    // 价变重建走「取消旧协议 + 按新金额重建」，新段携带引用）。
    Ok(())
}

/// 获取计划完整详情（含扩展字段和期次）。
pub fn get_plan_detail(conn: &Connection, id: &str) -> Result<ScheduledTransactionDetail> {
    let core: ScheduledTransaction = query_one(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| AppError::coded_not_found("scheduled-plan.not-found", "定时计划不存在"))?;

    let extension = match core.kind {
        ScheduledKind::Installment => {
            let ext: InstallmentPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences \
                 FROM installment_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.installment-ext-not-found",
                    "分期扩展信息不存在",
                )
            })?;
            serde_json::to_value(ext).unwrap_or_default()
        }
        ScheduledKind::Subscription => {
            let ext: SubscriptionPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,merchant_id,policy_id \
                 FROM subscription_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.subscription-ext-not-found",
                    "订阅扩展信息不存在",
                )
            })?;
            serde_json::to_value(ext).unwrap_or_default()
        }
        ScheduledKind::ScheduledTransfer => {
            let ext: ScheduledTransferPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,to_account_id,total_occurrences \
                 FROM scheduled_transfer_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.transfer-ext-not-found",
                    "定时转账扩展信息不存在",
                )
            })?;
            serde_json::to_value(ext).unwrap_or_default()
        }
    };

    let pending_occurrences: Vec<ScheduledTransactionOccurrence> = query_all(
        conn,
        "SELECT id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transaction_occurrences \
         WHERE scheduled_transaction_id=?1 AND is_deleted=0 AND status='pending' \
         ORDER BY scheduled_date ASC",
        rusqlite::params![id],
    )?;

    let (completed_occurrences, completed_amount_cents): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(amount_cents),0) FROM scheduled_transaction_occurrences \
         WHERE scheduled_transaction_id=?1 AND status='completed' AND is_deleted=0",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    // issue #205：期次详情弹窗需要全量期次列表（含 failed 可重试、cancelled
    // 历史等各状态）；新增返回字段，不改动既有字段口径。
    let occurrences: Vec<ScheduledTransactionOccurrence> = query_all(
        conn,
        "SELECT id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transaction_occurrences \
         WHERE scheduled_transaction_id=?1 AND is_deleted=0 \
         ORDER BY scheduled_date ASC",
        rusqlite::params![id],
    )?;

    Ok(ScheduledTransactionDetail {
        core,
        extension,
        pending_occurrences,
        completed_occurrences,
        completed_amount_cents,
        occurrences,
    })
}

/// 列出所有非软删除的定时计划（含类型特有字段）。
pub fn list_plans(conn: &Connection) -> Result<Vec<ScheduledTransactionWithExt>> {
    let cores: Vec<ScheduledTransaction> = query_all(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE is_deleted=0 ORDER BY created_at DESC",
        [],
    )?;

    let mut results = Vec::with_capacity(cores.len());
    for core in cores {
        let (merchant_id, policy_id, total_amount_cents, total_occurrences, to_account_id) =
            match core.kind {
                ScheduledKind::Installment => {
                    let ext: InstallmentPlan = query_one(
                        conn,
                        "SELECT scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences \
                         FROM installment_plans WHERE scheduled_transaction_id=?1",
                        rusqlite::params![core.id],
                    )?
                    .unwrap_or(InstallmentPlan {
                        scheduled_transaction_id: core.id.clone(),
                        merchant_id: None,
                        total_amount_cents: 0,
                        total_occurrences: 0,
                    });
                    (
                        ext.merchant_id,
                        None,
                        Some(ext.total_amount_cents),
                        Some(ext.total_occurrences),
                        None,
                    )
                }
                ScheduledKind::Subscription => {
                    let ext: SubscriptionPlan = query_one(
                        conn,
                        "SELECT scheduled_transaction_id,merchant_id,policy_id \
                         FROM subscription_plans WHERE scheduled_transaction_id=?1",
                        rusqlite::params![core.id],
                    )?
                    .unwrap_or(SubscriptionPlan {
                        scheduled_transaction_id: core.id.clone(),
                        merchant_id: None,
                        policy_id: None,
                    });
                    (ext.merchant_id, ext.policy_id, None, None, None)
                }
                ScheduledKind::ScheduledTransfer => {
                    let ext: ScheduledTransferPlan = query_one(
                        conn,
                        "SELECT scheduled_transaction_id,to_account_id,total_occurrences \
                         FROM scheduled_transfer_plans WHERE scheduled_transaction_id=?1",
                        rusqlite::params![core.id],
                    )?
                    .unwrap_or(ScheduledTransferPlan {
                        scheduled_transaction_id: core.id.clone(),
                        to_account_id: String::new(),
                        total_occurrences: None,
                    });
                    (
                        None,
                        None,
                        None,
                        ext.total_occurrences,
                        Some(ext.to_account_id),
                    )
                }
            };
        results.push(ScheduledTransactionWithExt {
            core,
            merchant_id,
            policy_id,
            total_amount_cents,
            total_occurrences,
            to_account_id,
        });
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// 期次展开
// ---------------------------------------------------------------------------

/// 预生成下一批期次。对于有上限的计划（如 installment）生成所有剩余期次；
/// 对于无限循环的计划生成有限窗口。
pub fn expand_occurrences(conn: &Connection, st_id: &str) -> Result<Vec<String>> {
    let st: ScheduledTransaction = query_one(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![st_id],
    )?
    .ok_or_else(|| AppError::coded_not_found("scheduled-plan.not-found", "定时计划不存在"))?;

    if st.status != "active" {
        return Ok(vec![]);
    }

    let (total_occurrences, is_installment) = match st.kind {
        ScheduledKind::Installment => {
            let ext: InstallmentPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences \
                 FROM installment_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![st_id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.installment-ext-not-found",
                    "分期扩展信息不存在",
                )
            })?;
            (Some(ext.total_occurrences), true)
        }
        ScheduledKind::ScheduledTransfer => {
            let ext: ScheduledTransferPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,to_account_id,total_occurrences \
                 FROM scheduled_transfer_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![st_id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.transfer-ext-not-found",
                    "定时转账扩展信息不存在",
                )
            })?;
            (ext.total_occurrences, false)
        }
        ScheduledKind::Subscription => (None, false),
    };

    let existing_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scheduled_transaction_occurrences \
         WHERE scheduled_transaction_id=?1 AND is_deleted=0",
        rusqlite::params![st_id],
        |r| r.get(0),
    )?;

    if let Some(total) = total_occurrences
        && existing_count >= total
    {
        return Ok(vec![]);
    }

    let max_to_create = if let Some(total) = total_occurrences {
        total - existing_count
    } else {
        WINDOW_SIZE
    };

    if max_to_create <= 0 {
        return Ok(vec![]);
    }

    // 确定起始日期：取已有最后一期的下一期，或 start_date
    let start = if existing_count > 0 {
        let last_date: String = conn.query_row(
            "SELECT scheduled_date FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND is_deleted=0 \
             ORDER BY scheduled_date DESC LIMIT 1",
            rusqlite::params![st_id],
            |r| r.get(0),
        )?;
        advance_date(&st, &last_date, 1)?
    } else {
        st.start_date.clone()
    };

    let now = now_iso();
    let mut ids = Vec::new();

    if is_installment {
        let total = total_occurrences.unwrap();
        let ext: InstallmentPlan = query_one(
            conn,
            "SELECT scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences \
             FROM installment_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![st_id],
        )?
        .ok_or_else(|| {
            AppError::coded_not_found(
                "scheduled-plan.installment-ext-not-found",
                "分期扩展信息不存在",
            )
        })?;

        let base = ext.total_amount_cents / ext.total_occurrences;
        let tail = ext.total_amount_cents - base * ext.total_occurrences;

        for i in existing_count..total {
            let occ_id = new_uuid();
            let date = advance_date(&st, &st.start_date, i)?;
            let amount = if i == total - 1 { base + tail } else { base };

            conn.execute(
                "INSERT INTO scheduled_transaction_occurrences \
                 (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
                 created_at,updated_at,version,device_id,is_deleted) \
                 VALUES (?1,?2,?3,'pending',NULL,?4,?5,?5,1,?6,0)",
                rusqlite::params![occ_id, st_id, date, amount, now, device_id()],
            )?;
            ids.push(occ_id);
        }
    } else {
        for i in 0..max_to_create {
            let occ_id = new_uuid();
            let date = advance_date(&st, &start, i)?;

            conn.execute(
                "INSERT INTO scheduled_transaction_occurrences \
                 (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
                 created_at,updated_at,version,device_id,is_deleted) \
                 VALUES (?1,?2,?3,'pending',NULL,?4,?5,?5,1,?6,0)",
                rusqlite::params![occ_id, st_id, date, st.amount_cents, now, device_id()],
            )?;
            ids.push(occ_id);
        }
    }

    Ok(ids)
}

/// 计算日期加上 offset 期次后的日期。
fn advance_date(st: &ScheduledTransaction, from: &str, offset: i64) -> Result<String> {
    let date = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|e| {
        AppError::codedp(
            "scheduled-plan.date-invalid",
            format!("日期格式错误 {from}: {e}"),
            &[from, &e.to_string()],
        )
    })?;

    let interval = st.recurrence_interval.max(1);
    let result = match st.recurrence_type.as_str() {
        "daily" => date + chrono::Duration::days(interval * offset),
        "weekly" => date + chrono::Duration::days(7 * interval * offset),
        "monthly" => {
            let months = interval * offset;
            let mut m = date.month() as i64 + months;
            let mut y = date.year() as i64 + (m - 1) / 12;
            m = ((m - 1) % 12) + 1;
            if m <= 0 {
                m += 12;
                y -= 1;
            }
            let day = st.recurrence_day.map(|d| d as u32).unwrap_or(date.day());
            let max_day = days_in_month(y, m as u32);
            chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, day.min(max_day))
                .ok_or_else(|| AppError::Invalid("无效日期".into()))?
        }
        "yearly" => {
            let y = date.year() + (interval * offset) as i32;
            let day = st.recurrence_day.map(|d| d as u32).unwrap_or(date.day());
            let max_day = days_in_month(y as i64, date.month());
            chrono::NaiveDate::from_ymd_opt(y, date.month(), day.min(max_day))
                .ok_or_else(|| AppError::Invalid("无效日期".into()))?
        }
        _ => return Err(AppError::Invalid("未知周期类型".into())),
    };

    Ok(result.format("%Y-%m-%d").to_string())
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// ---------------------------------------------------------------------------
// 期次执行
// ---------------------------------------------------------------------------

/// 执行一条 pending/failed 期次：生成 Transaction 并回填。
///
/// 事务边界（issue #230 / ADR-0033 决策 #6，引擎是登记的**事务自持唯一合法例外**，
/// 继续直调 Writer 接缝、不经行为层编排入口）：[`writer::normalize`] 是只读校验，
/// 保持事务外——业务错误（如非默认币种缺汇率）期次停留 pending 可重试，现状不变；
/// 其后从 CAS 置 processing 起包到计划完成检查，任何失败整体 ROLLBACK、期次回原
/// 状态可重试，不再出现期次滞留 processing 而交易未落库的中间态。调用方仅 IPC
/// 命令一处（`db.write`，autocommit），无嵌套问题，自持 BEGIN/COMMIT/ROLLBACK
/// 即可，无需行为层的嵌套感知分支。
pub fn execute_occurrence(conn: &Connection, occurrence_id: &str) -> Result<String> {
    tracing::info!(occurrence_id = %occurrence_id, "开始执行定时交易期次");
    let occ: ScheduledTransactionOccurrence = query_one(
        conn,
        "SELECT id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transaction_occurrences WHERE id=?1 AND is_deleted=0",
        rusqlite::params![occurrence_id],
    )?
    .ok_or_else(|| AppError::coded_not_found("scheduled-occurrence.not-found", "期次不存在"))?;

    if occ.status != "pending" && occ.status != "failed" {
        tracing::warn!(occurrence_id = %occurrence_id, status = %occ.status, "期次不能执行，状态不匹配");
        return Err(AppError::codedp(
            "scheduled-occurrence.not-executable",
            format!("期次状态为 {}，不能执行", occ.status),
            &[&occ.status],
        ));
    }

    let st: ScheduledTransaction = query_one(
        conn,
        "SELECT id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted \
         FROM scheduled_transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![occ.scheduled_transaction_id],
    )?
    .ok_or_else(|| {
        AppError::coded_not_found("scheduled-occurrence.plan-not-found", "关联定时计划不存在")
    })?;

    if st.status != "active" {
        return Err(AppError::coded(
            "scheduled-plan.not-active",
            "关联计划未处于活跃状态",
        ));
    }

    let (kind, to_account_id, category_id, merchant_id, policy_id) = match st.kind {
        ScheduledKind::Installment | ScheduledKind::Subscription => {
            // 复制计划的商户到流水（issue #190 / ADR-0028）：installment/subscription
            // 每期生成的交易带上计划扩展表的 merchant_id（沿用原 counterparty 复制语义）；
            // 同时复制保单引用（issue #362 / ADR-0051 决策 2，同机制）：保费协议
            // （订阅形态）每期生成的交易携带扩展表的 policy_id——协议对保单的引用
            // 是历史引用（创建时已校验在用），保单随后被软删时照常复制（与商户
            // 「保持历史引用」同一语义），而不是让期次执行失败。分期不持保单引用
            // （准入守卫在 create_plan），恒 None。
            let (merchant_id, policy_id) = match st.kind {
                ScheduledKind::Installment => (
                    query_one::<InstallmentPlan, _>(
                        conn,
                        "SELECT scheduled_transaction_id,merchant_id,total_amount_cents,total_occurrences \
                         FROM installment_plans WHERE scheduled_transaction_id=?1",
                        rusqlite::params![st.id],
                    )?
                    .ok_or_else(|| {
                        AppError::coded_not_found(
                            "scheduled-plan.installment-ext-not-found",
                            "分期扩展信息不存在",
                        )
                    })?
                    .merchant_id,
                    None,
                ),
                ScheduledKind::Subscription => {
                    let ext = subscription_ext(conn, &st.id)?;
                    (ext.merchant_id, ext.policy_id)
                }
                ScheduledKind::ScheduledTransfer => unreachable!(),
            };
            (
                TransactionKind::Expense,
                None,
                st.category_id.clone(),
                merchant_id,
                policy_id,
            )
        }
        ScheduledKind::ScheduledTransfer => {
            let ext: ScheduledTransferPlan = query_one(
                conn,
                "SELECT scheduled_transaction_id,to_account_id,total_occurrences \
                 FROM scheduled_transfer_plans WHERE scheduled_transaction_id=?1",
                rusqlite::params![st.id],
            )?
            .ok_or_else(|| {
                AppError::coded_not_found(
                    "scheduled-plan.transfer-ext-not-found",
                    "定时转账扩展信息不存在",
                )
            })?;
            (
                TransactionKind::Transfer,
                Some(ext.to_account_id),
                None,
                None,
                None,
            )
        }
    };

    // 经 Writer 接缝归一化（issue #59 / spec #52）：
    // - 本位币金额由 Amount 接缝 convert_to_native 折算，修复非默认币种定时交易
    //   把原始金额当作 amount_native_cents 落库的 bug（故事 3/17/23）；
    // - normalize 为只读校验+折算，放在 CAS 锁定**之前**：业务错误（如非默认币种
    //   缺汇率）直接返回、期次保持 pending 可重试，不会滞留 processing；
    // - id 与审计字段（created_at/updated_at/version/device_id/is_deleted）由
    //   insert_row 生成，与手动创建/导入共用同一写入权威，列清单不在此重复。
    // 商户复制语义（issue #190 / ADR-0028）：期次生成交易复制计划的商户引用。
    // `existing_merchant_id` 传同值——计划对商户的引用是历史引用（创建计划时已校验
    // 商户在用），期次只是把该引用复制到流水：计划商户随后被软删时，历史引用照常
    // 保留、继续复制（与 writer 编辑路径「保持历史引用跳过校验」同一语义），
    // 而不是让期次执行失败。
    let norm = writer::normalize(
        conn,
        &writer::Input {
            kind,
            amount_cents: occ.amount_cents,
            currency_code: st.currency_code.clone(),
            account_id: st.account_id.clone(),
            to_account_id,
            category_id,
            merchant_id: merchant_id.clone(),
            existing_merchant_id: merchant_id,
            // 保单引用复制（issue #362 / ADR-0051 决策 2）：保费协议每期生成交易
            // 携带扩展表 policy_id；`existing_policy_id` 传同值——协议对保单的引用
            // 是历史引用（创建时已校验在用），保单随后被软删时照常复制、期次不失败。
            policy_id: policy_id.clone(),
            existing_policy_id: policy_id,
            refund_of_transaction_id: None,
            note: st.note.clone(),
            date: occ.scheduled_date.clone(),
        },
    )?;

    // 事务自持：从 CAS 置 processing 起包到计划完成检查（事务边界见本函数 doc）。
    let now = now_iso();
    conn.execute("BEGIN", [])?;
    match execute_within_transaction(conn, occurrence_id, &occ.status, &norm, &st.id, &now) {
        Ok(txn_id) => match conn.execute("COMMIT", []) {
            Ok(_) => {
                tracing::info!(occurrence_id = %occurrence_id, transaction_id = %txn_id, "定时交易期次执行成功");
                Ok(txn_id)
            }
            // COMMIT 失败：尽力回滚清理残留，再上抛提交错误（与行为层编排入口同款）。
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        // 中途失败：整体回滚，期次回原状态可重试；ROLLBACK 自身失败不遮蔽业务错误。
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}

/// 读取订阅计划扩展行（期次执行引用复制用）；缺失即错（核心行存在而扩展行
/// 缺失属数据损坏）。
fn subscription_ext(conn: &Connection, st_id: &str) -> Result<SubscriptionPlan> {
    query_one::<SubscriptionPlan, _>(
        conn,
        "SELECT scheduled_transaction_id,merchant_id,policy_id \
         FROM subscription_plans WHERE scheduled_transaction_id=?1",
        rusqlite::params![st_id],
    )?
    .ok_or_else(|| {
        AppError::coded_not_found(
            "scheduled-plan.subscription-ext-not-found",
            "订阅扩展信息不存在",
        )
    })
}

/// 期次落库协议本体（无事务语义，由 [`execute_occurrence`] 自持事务包裹）：
/// CAS 置 processing → 交易行落库 → 回填 transaction_id → 计划完成检查。
fn execute_within_transaction(
    conn: &Connection,
    occurrence_id: &str,
    expected_status: &str,
    norm: &writer::NormalizedRow,
    plan_id: &str,
    now: &str,
) -> Result<String> {
    // 锁定期次: CAS update status -> processing
    let updated = conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='processing', updated_at=?2, version=version+1, device_id=?3 \
         WHERE id=?1 AND status=?4 AND is_deleted=0",
        rusqlite::params![occurrence_id, now, device_id(), expected_status],
    )?;
    if updated == 0 {
        tracing::warn!(occurrence_id = %occurrence_id, "期次 CAS 冲突，已被其他设备执行");
        return Err(AppError::coded(
            "scheduled-occurrence.conflict",
            "期次已被其他设备执行",
        ));
    }

    let txn_id = writer::insert_row(conn, norm)?;

    // 回填 transaction_id
    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='completed', transaction_id=?2, updated_at=?3, version=version+1, device_id=?4 \
         WHERE id=?1",
        rusqlite::params![occurrence_id, txn_id, now, device_id()],
    )?;

    // 检查计划是否应标记为 completed
    check_and_complete_plan(conn, plan_id)?;

    Ok(txn_id)
}

/// 检查计划是否所有期次已完成，如果是则标记为 completed。
fn check_and_complete_plan(conn: &Connection, st_id: &str) -> Result<()> {
    let kind: ScheduledKind = conn.query_row(
        "SELECT kind FROM scheduled_transactions WHERE id=?1",
        rusqlite::params![st_id],
        |r| r.get(0),
    )?;

    let total_occurrences: Option<i64> = match kind {
        ScheduledKind::Installment => Some(conn.query_row(
            "SELECT total_occurrences FROM installment_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![st_id],
            |r| r.get(0),
        )?),
        ScheduledKind::ScheduledTransfer => conn.query_row(
            "SELECT total_occurrences FROM scheduled_transfer_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![st_id],
            |r| r.get(0),
        )?,
        ScheduledKind::Subscription => None,
    };

    if let Some(total) = total_occurrences {
        let completed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND status='completed' AND is_deleted=0",
            rusqlite::params![st_id],
            |r| r.get(0),
        )?;
        if completed >= total {
            let now = now_iso();
            conn.execute(
                "UPDATE scheduled_transactions SET status='completed', updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
                rusqlite::params![st_id, now, device_id()],
            )?;
        }
    }

    Ok(())
}
