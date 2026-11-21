use chrono::Datelike;
use rusqlite::{Connection, OptionalExtension};

use super::command::{
    ScheduledCommand, occurrence_transaction_id, record_local, transaction_landed,
};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::db::tx_scope::{ensure_transaction, hold_transaction};
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::NormalizedTransaction;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::write::writer;

use super::models::*;

const WINDOW_SIZE: i64 = 12;

pub fn create_plan(conn: &Connection, input: CreateScheduledInput) -> Result<String> {
    ensure_transaction(conn, || {
        let id = new_uuid();
        create_protocol(conn, &id, &input)?;
        record_local(
            conn,
            ScheduledCommand::CreatePlan {
                id: id.clone(),
                input,
            },
        )?;
        Ok(id)
    })
}

pub(crate) fn create_protocol(
    conn: &Connection,
    id: &str,
    input: &CreateScheduledInput,
) -> Result<()> {
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
    if input.policy_id.is_some() {
        if input.kind != ScheduledKind::Subscription {
            return Err(AppError::coded(
                "scheduled-plan.policy-subscription-only",
                "只有订阅形态协议可挂保单",
            ));
        }
        if input.merchant_id.is_some() {
            return Err(AppError::coded(
                "scheduled-plan.policy-merchant-forbidden",
                "保单缴费协议不挂商户：保费归属走保单引用，付款对象由保司承担",
            ));
        }
        writer::validate_policy_active(conn, input.policy_id.as_deref())?;
    }

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
            &input.account_id,
            &input.category_id,
            input.amount_cents,
            &input.currency_code,
            input.recurrence_type.to_string(),
            input.recurrence_interval,
            input.recurrence_day,
            &input.start_date,
            &input.note,
            now,
            now,
            device_id(conn)?,
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
                rusqlite::params![id, &input.merchant_id, total, total_occ],
            )?;
        }
        ScheduledKind::Subscription => {
            conn.execute(
                "INSERT INTO subscription_plans (scheduled_transaction_id,merchant_id,policy_id) VALUES (?1,?2,?3)",
                rusqlite::params![id, &input.merchant_id, &input.policy_id],
            )?;
        }
        ScheduledKind::ScheduledTransfer => {
            let to_acc = input.to_account_id.clone().ok_or_else(|| {
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

    expand_occurrences_protocol(conn, id)?;

    Ok(())
}

pub fn update_plan_status(conn: &Connection, id: &str, new_status: ScheduledStatus) -> Result<()> {
    ensure_transaction(conn, || {
        update_plan_status_protocol(conn, id, new_status)?;
        record_local(
            conn,
            ScheduledCommand::UpdatePlanStatus {
                id: id.to_string(),
                new_status,
            },
        )
    })
}

pub(crate) fn update_plan_status_protocol(
    conn: &Connection,
    id: &str,
    new_status: ScheduledStatus,
) -> Result<()> {
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
        rusqlite::params![id, status_str, now, device_id(conn)?],
    )?;

    if new_status == ScheduledStatus::Cancelled {
        conn.execute(
            "UPDATE scheduled_transaction_occurrences SET status='cancelled', updated_at=?2, version=version+1, device_id=?3 \
             WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0",
            rusqlite::params![id, now, device_id(conn)?],
        )?;
    }

    Ok(())
}

pub fn update_subscription(conn: &Connection, input: UpdateSubscriptionInput) -> Result<()> {
    ensure_transaction(conn, || {
        update_subscription_protocol(conn, &input)?;
        record_local(
            conn,
            ScheduledCommand::UpdateSubscription {
                id: input.id.clone(),
                account_id: input.account_id.clone(),
                category_id: input.category_id.clone(),
                note: input.note.clone(),
                merchant_id: input.merchant_id.clone(),
            },
        )
    })
}

pub(crate) fn update_subscription_protocol(
    conn: &Connection,
    input: &UpdateSubscriptionInput,
) -> Result<()> {
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

    let current_policy_id: Option<String> = conn
        .query_row(
            "SELECT policy_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![&input.id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if current_policy_id.is_some() && input.merchant_id.is_some() {
        return Err(AppError::coded(
            "scheduled-plan.policy-merchant-forbidden",
            "保单缴费协议不挂商户：保费归属走保单引用，付款对象由保司承担",
        ));
    }
    let current_merchant: Option<String> = conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            rusqlite::params![&input.id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if input.merchant_id.as_ref() != current_merchant.as_ref() {
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
            device_id(conn)?,
        ],
    )?;
    conn.execute(
        "UPDATE subscription_plans SET merchant_id=?2 WHERE scheduled_transaction_id=?1",
        rusqlite::params![&input.id, input.merchant_id],
    )?;

    Ok(())
}

pub fn get_plan_detail(conn: &Connection, id: &str) -> Result<ScheduledTransactionDetail> {
    ensure_transaction(conn, || {
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
    })
}

pub fn list_plans(conn: &Connection) -> Result<Vec<ScheduledTransactionWithExt>> {
    ensure_transaction(conn, || {
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
            let ext = match core.kind {
                ScheduledKind::Installment => {
                    let plan: InstallmentPlan = query_one(
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
                    PlanExtProjection {
                        merchant_id: plan.merchant_id,
                        total_amount_cents: Some(plan.total_amount_cents),
                        total_occurrences: Some(plan.total_occurrences),
                        ..Default::default()
                    }
                }
                ScheduledKind::Subscription => {
                    let plan: SubscriptionPlan = query_one(
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
                    PlanExtProjection {
                        merchant_id: plan.merchant_id,
                        policy_id: plan.policy_id,
                        ..Default::default()
                    }
                }
                ScheduledKind::ScheduledTransfer => {
                    let plan: ScheduledTransferPlan = query_one(
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
                    PlanExtProjection {
                        to_account_id: Some(plan.to_account_id),
                        total_occurrences: plan.total_occurrences,
                        ..Default::default()
                    }
                }
            };
            results.push(ScheduledTransactionWithExt {
                core,
                merchant_id: ext.merchant_id,
                policy_id: ext.policy_id,
                total_amount_cents: ext.total_amount_cents,
                total_occurrences: ext.total_occurrences,
                to_account_id: ext.to_account_id,
            });
        }
        Ok(results)
    })
}

pub fn expand_occurrences(conn: &Connection, st_id: &str) -> Result<Vec<String>> {
    ensure_transaction(conn, || {
        let ids = expand_occurrences_protocol(conn, st_id)?;
        if !ids.is_empty() {
            record_local(
                conn,
                ScheduledCommand::ExpandOccurrences {
                    plan_id: st_id.to_string(),
                },
            )?;
        }
        Ok(ids)
    })
}

pub(crate) fn expand_occurrences_protocol(conn: &Connection, st_id: &str) -> Result<Vec<String>> {
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

    if let Some(total) = total_occurrences
        && is_installment
    {
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
                rusqlite::params![occ_id, st_id, date, amount, now, device_id(conn)?],
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
                rusqlite::params![occ_id, st_id, date, st.amount_cents, now, device_id(conn)?],
            )?;
            ids.push(occ_id);
        }
    }

    Ok(ids)
}

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
            chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, day.min(max_day)).ok_or_else(
                || AppError::coded("scheduled-plan.occurrence-date-invalid", "无效日期"),
            )?
        }
        "yearly" => {
            let y = date.year() + (interval * offset) as i32;
            let day = st.recurrence_day.map(|d| d as u32).unwrap_or(date.day());
            let max_day = days_in_month(y as i64, date.month());
            chrono::NaiveDate::from_ymd_opt(y, date.month(), day.min(max_day)).ok_or_else(|| {
                AppError::coded("scheduled-plan.occurrence-date-invalid", "无效日期")
            })?
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

    let (kind, category_id, ext) = match st.kind {
        ScheduledKind::Installment | ScheduledKind::Subscription => {
            let ext = match st.kind {
                ScheduledKind::Installment => {
                    let plan = query_one::<InstallmentPlan, _>(
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
                    })?;
                    PlanExtProjection {
                        merchant_id: plan.merchant_id,
                        ..Default::default()
                    }
                }
                ScheduledKind::Subscription => {
                    let ext = subscription_ext(conn, &st.id)?;
                    PlanExtProjection {
                        merchant_id: ext.merchant_id,
                        policy_id: ext.policy_id,
                        ..Default::default()
                    }
                }
                ScheduledKind::ScheduledTransfer => {
                    return Err(AppError::coded(
                        "scheduled-plan.kind-mismatch",
                        "计划形态与期次执行路径不一致（内部不一致）",
                    ));
                }
            };
            (TransactionKind::Expense, st.category_id.clone(), ext)
        }
        ScheduledKind::ScheduledTransfer => {
            let plan: ScheduledTransferPlan = query_one(
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
                None,
                PlanExtProjection {
                    to_account_id: Some(plan.to_account_id),
                    total_occurrences: plan.total_occurrences,
                    ..Default::default()
                },
            )
        }
    };

    let norm = writer::normalize(
        conn,
        &writer::Input {
            kind,
            amount_cents: occ.amount_cents,
            currency_code: st.currency_code.clone(),
            account_id: Some(st.account_id.clone()),
            to_account_id: ext.to_account_id,
            funding_account_id: None,
            funding: Vec::new(),
            existing_funding: Vec::new(),
            purchases: Vec::new(),
            existing_purchases: Vec::new(),
            source_order_no: None,
            category_id,
            merchant_id: ext.merchant_id.clone(),
            existing_merchant_id: ext.merchant_id,
            policy_id: ext.policy_id.clone(),
            existing_policy_id: ext.policy_id,
            refund_of_transaction_id: None,
            note: st.note.clone(),
            date: occ.scheduled_date.clone(),
            fx_rate: None,
            fx_edit_baseline: None,
        },
    )?;

    let now = now_iso();
    let txn_id = hold_transaction(conn, || {
        execute_within_transaction(
            conn,
            occurrence_id,
            &occ.status,
            &norm,
            &st.id,
            &occ.scheduled_date,
            &now,
        )
    })?;
    tracing::info!(occurrence_id = %occurrence_id, transaction_id = %txn_id, "定时交易期次执行成功");
    Ok(txn_id)
}

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

#[derive(Default)]
struct PlanExtProjection {
    merchant_id: Option<String>,
    policy_id: Option<String>,
    total_amount_cents: Option<i64>,
    total_occurrences: Option<i64>,
    to_account_id: Option<String>,
}

fn execute_within_transaction(
    conn: &Connection,
    occurrence_id: &str,
    expected_status: &str,
    norm: &writer::NormalizedRow,
    plan_id: &str,
    scheduled_date: &str,
    now: &str,
) -> Result<String> {
    let updated = conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='processing', updated_at=?2, version=version+1, device_id=?3 \
         WHERE id=?1 AND status=?4 AND is_deleted=0",
        rusqlite::params![occurrence_id, now, device_id(conn)?, expected_status],
    )?;
    if updated == 0 {
        tracing::warn!(occurrence_id = %occurrence_id, "期次 CAS 冲突，已被其他设备执行");
        return Err(AppError::coded(
            "scheduled-occurrence.conflict",
            "期次已被其他设备执行",
        ));
    }

    let landing_id = occurrence_transaction_id(plan_id, scheduled_date);
    let landed = transaction_landed(conn, &landing_id)?;
    if !landed {
        writer::insert_row_with_id(conn, &landing_id, norm)?;
        record_local(
            conn,
            ScheduledCommand::ExecuteOccurrence {
                plan_id: plan_id.to_string(),
                scheduled_date: scheduled_date.to_string(),
                row: NormalizedTransaction::from(norm),
            },
        )?;
    }

    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='completed', transaction_id=?2, updated_at=?3, version=version+1, device_id=?4 \
         WHERE id=?1",
        rusqlite::params![occurrence_id, landing_id, now, device_id(conn)?],
    )?;

    check_and_complete_plan(conn, plan_id)?;

    Ok(landing_id)
}

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
                rusqlite::params![st_id, now, device_id(conn)?],
            )?;
        }
    }

    Ok(())
}
