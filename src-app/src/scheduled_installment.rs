//! 分期页签（issue #2043 定时②；旧壳 `InstallmentsPane.vue` 的重生——
//! ScheduledPlanList 计划清单模块的分期形态薄适配：计划清单 + 计划表单 +
//! 期次详情弹层）。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：
//! - **清单**：分期计划行集（域读路径过滤分期形态）+ 逐行详情读出的进度
//!   （已完成期次数与已还金额实时汇总，来自既有详情命令的
//!   completed_occurrences / completed_amount_cents，不持久化、不推算——
//!   旧壳 installment ext 同口径）；状态过滤（进行中 / 已暂停 / 已取消 /
//!   已完成——WITH_COMPLETED 键集：#309 显式可见变化之二在此收口的等价
//!   面——按 Plan Lifecycle 自然完成的分期计划恢复可见可查）；纯前端过滤
//!   不产生请求（旧壳 filteredRows 同口径）。
//! - **新建分期**（旧壳 ModalIntent 单成员意图的本壳等价形态）：共享表单
//!   件（账户/分类/商户输入即建/币种随账户推导/周期三字段/开始日/备注）
//!   消费 [`crate::scheduled_subscription`] 单点 + 商户「输入即建」走
//!   （[`crate::boot::create_merchant_by_name`] 按名 find-or-create 单点）；
//!   分期形态真差异留本页签——总金额 + 期数 + 每期金额实时预览（含尾差
//!   文案，口径唯一来源 = [`installment_schedule`]：floor 均分、尾差进最
//!   后一期，与域 `expand_occurrences` 的分期分支同口径；总金额 < 期数被
//!   域显式拒绝，前端先行前置拦截同文案）。
//! - **进度呈现**（渲染期零格式化）：进度列 = 期数维度百分比条（已完成
//!   期数 / 总期数，总期数缺席兜底 0，旧壳 progressPercentage 同口径）+
//!   已还汇总文案（旧壳 scheduled.progress.repaid 同词表，装配层产出）；
//!   详情失败呈现「加载失败」不与无数据混淆。
//! - **行操作**：期次（详情）恒可用、暂停限 active、恢复限 paused、取消限
//!   「未结束」带确认气泡（分期形态取消确认文案：旧壳
//!   installmentCancelConfirm）；编辑是订阅适配器自建描述符（ADR-0041 决
//!   策 7），本页签不呈现。
//! - **计划来源落点**：`pending_focus` 在本页签挂载时消费开窗并回报清闸
//!   （旧壳 usePlanFocusLanding 时序的页签侧）。
//!
//! 写路径：直调域公开写入口（[`crate::boot`]，不经 IPC）；清单重拉归动作
//! 回调（旧壳「计划写入刻意零信号」同口径）；期次执行落交易行后由视图层
//! 手动广播交易失效信号（同订阅页签口径）。
//!
//! 兄弟模块：[`crate::scheduled`]（视图壳与词表）、
//! [`crate::scheduled_subscription`]（共享表单元语言落点）、
//! [`crate::scheduled_plan_detail`]（期次详情弹层，三页签通用）。
//!
//! 界面文案仅 zh-CN；码化错误 zh 原样呈现（反馈条位对应旧壳 toast）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use ledger_scheduled::ScheduledStatus;

use crate::adaptive;
use crate::assembly::{
    self, AccountTable, CategoryTable, ColumnAlign, CurrencyTable, MerchantTable,
};
use crate::boot;
use crate::combo_select;
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::pinyin_select;
use crate::popconfirm;
use crate::scheduled as sched;
use crate::scheduled_plan_detail;
use crate::scheduled_subscription::{
    MerchantRefOf, account_options_of, action_button, currency_of_account, derive_currency_text,
    field_label, form_row, merchant_options_active, merchant_ref_of,
};
use crate::signals;
use crate::{nav, txn_form};

// ---------------------------------------------------------------------------
// 词表（旧壳 packages/i18n zh-CN scheduled.json 逐条平价；i18n 框架级方案不在
// 本票，字面承载）。
// ---------------------------------------------------------------------------
pub(crate) const INST_LIST_TITLE: &str = "分期清单";
pub(crate) const CREATE_INST_TITLE: &str = "新建分期";
pub(crate) const CREATE_INST_OPEN_LABEL: &str = "新建分期";
pub(crate) const CREATE_INST_SUBMIT_LABEL: &str = "创建分期";
pub(crate) const INST_CANCEL_CONFIRM_TEXT: &str =
    "取消后不再自动扣款，已生成的交易与历史期次保留。确认取消？";
pub(crate) const MSG_INST_CREATED: &str = "已创建分期计划";
pub(crate) const MSG_TOTAL_POSITIVE: &str = "请输入大于 0 的总金额";
pub(crate) const MSG_PERIODS_MIN: &str = "请输入不小于 1 的期数";
pub(crate) const MSG_TOTAL_BELOW_PERIODS: &str = "总金额不能小于期数（每期至少 1 分）";

// ---------------------------------------------------------------------------
// 分期口径纯函数（旧壳 `packages/utils/src/installment.ts` 的重生：口径唯一
// 来源 = 域 `expand_occurrences` 的分期分支——floor 均分、尾差进最后一期；
// 本壳只做预览呈现，禁止另立第二套分期金额表达式）。
// ---------------------------------------------------------------------------

/// 按总额与期数计算每期金额与最后一期（含尾差）：非法输入（非正整数）= None
/// （预览静默回空，提交校验面独立拦截——旧壳 schedule computed try/catch 同
/// 语义）。
pub(crate) fn installment_schedule(total_cents: i64, periods: i64) -> Option<(i64, i64)> {
    if total_cents <= 0 || periods <= 0 {
        return None;
    }
    let per_period_cents = total_cents / periods;
    let last_period_cents = total_cents - per_period_cents * (periods - 1);
    Some((per_period_cents, last_period_cents))
}

/// 每期金额预览文案：整除时末期与每期一致，不提尾差；不整除时明确标注末期
/// 含尾差（旧壳 scheduled.preview.perPeriod / withLast 同词表）。日期与金额
/// 的装配期一次性产出（渲染期零格式化）。
pub(crate) fn preview_text(
    total_cents: i64,
    periods: i64,
    fmt: &assembly::CurrencyFormat,
) -> String {
    let Some((per, last)) = installment_schedule(total_cents, periods) else {
        return String::new();
    };
    let per_text = assembly::format_amount_basic(per, fmt);
    if last == per {
        format!("每期 {per_text}")
    } else {
        let last_text = assembly::format_amount_basic(last, fmt);
        format!("每期 {per_text} · 末期 {last_text}（含尾差）")
    }
}

// ---------------------------------------------------------------------------
// 行集快照与装配（清单格文本的纯 Rust 单测面）。
// ---------------------------------------------------------------------------

/// 分期计划行可克隆快照（视图局部）：域读路径 [`crate::boot::plan_rows`]
/// 按 kind 过滤分期计划 + 逐行详情读出的已完成汇总（期数与金额，不持久化
/// 不推算）——清单表单消费的字段全集在装载批一次性投影持有。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstRowSnap {
    pub id: String,
    pub note: Option<String>,
    pub merchant_id: Option<String>,
    pub account_id: String,
    pub category_id: Option<String>,
    /// 计划总金额（分期扩展表；域校验必有值）。
    pub total_amount_cents: i64,
    pub amount_cents: i64,
    pub currency_code: String,
    pub recurrence_type: String,
    pub recurrence_interval: i64,
    pub start_date: String,
    pub status: String,
    /// 总期数（域校验必有值）。
    pub total_occurrences: i64,
    /// 详情命令失败：与「无数据」区分，不静默（进度占位「加载失败」，
    /// 旧壳 detailFailed 同口径）。
    pub detail_failed: bool,
    /// 已完成期次数（详情命令实时汇总，不持久化）。
    pub completed_count: i64,
    /// 已完成期次金额合计（详情命令实时汇总，不持久化）。
    pub completed_amount_cents: i64,
}

/// 行集投影 [`InstRowSnap`] 的集合容器（装载批一次性投影）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct InstRowsSnap(pub Vec<InstRowSnap>);

/// 装载批一次读齐的回执（分期行集 + 币种 / 分类 / 账户 / 商户参考数据；
/// complexity 门禁的具名面）。
type InstLoadBatch = Result<
    (
        Vec<InstRowSnap>,
        Vec<ledger_currencies::Currency>,
        Vec<ledger_categories::Category>,
        Vec<ledger_accounts::Account>,
        Vec<ledger_merchants::Merchant>,
    ),
    ledger_infra::error::AppError,
>;

/// 装载批投影（分期行快照构造单点；进度汇总随详情读一并投影）。
fn inst_row_snap_of(
    plan: &ledger_scheduled::ScheduledTransactionWithExt,
    detail_failed: bool,
    completed_count: i64,
    completed_amount_cents: i64,
) -> InstRowSnap {
    InstRowSnap {
        id: plan.core.id.clone(),
        note: plan.core.note.clone(),
        merchant_id: plan.merchant_id.clone(),
        account_id: plan.core.account_id.clone(),
        category_id: plan.core.category_id.clone(),
        total_amount_cents: plan.total_amount_cents.unwrap_or(0),
        amount_cents: plan.core.amount_cents,
        currency_code: plan.core.currency_code.clone(),
        recurrence_type: plan.core.recurrence_type.clone(),
        recurrence_interval: plan.core.recurrence_interval,
        start_date: plan.core.start_date.clone(),
        status: plan.core.status.clone(),
        total_occurrences: plan.total_occurrences.unwrap_or(0),
        detail_failed,
        completed_count,
        completed_amount_cents,
    }
}

/// 列表行格单元格（渲染期零格式化）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstRowCells {
    pub id: String,
    pub note: String,
    pub merchant: String,
    pub category: String,
    pub account: String,
    pub total: String,
    /// 已还汇总文案（装配期一次性产出）。
    pub progress: String,
    /// 期数维度进度百分比（0–100，条宽消费面）。
    pub progress_percent: f32,
    pub recurrence: String,
    pub start_date: String,
    pub status: String,
    pub actions: crate::scheduled::SubRowActions,
}

/// 清单装配上下文（参考数据表的只读投影 + 隐私快照）。
pub(crate) struct InstListContext<'a> {
    pub currencies: &'a CurrencyTable,
    pub categories: &'a CategoryTable,
    pub accounts: &'a AccountTable,
    pub merchants: &'a MerchantTable,
    pub privacy_enabled: bool,
}

/// 列定义（旧壳桌面档十列一字不动：备注/商户/分类/扣款账户/总金额/进度/
/// 周期/开始日/状态/操作；总金额右对齐。宽度建议值收紧到 1280 视口内容区
/// 全列在屏（十列合计 1030pt ≤ 表区可用 1054pt——操作列不被右缘裁切，旧
/// 壳 naive-ui 自动宽度同效果），富余宽给备注列由宿主映射分配）。
pub(crate) const INST_COLUMNS: &[assembly::ColumnDef] = &[
    assembly::ColumnDef {
        key: "note",
        title: "备注",
        width: 140.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "merchant",
        title: "商户",
        width: 90.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "category",
        title: "分类",
        width: 100.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "account",
        title: "扣款账户",
        width: 90.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "total",
        title: "总金额",
        width: 80.0,
        align: ColumnAlign::End,
    },
    assembly::ColumnDef {
        key: "progress",
        title: "进度",
        width: 210.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "recurrence",
        title: "周期",
        width: 65.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "start_date",
        title: "开始日",
        width: 85.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "status",
        title: "状态",
        width: 65.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "actions",
        title: "操作",
        width: 105.0,
        align: ColumnAlign::Start,
    },
];

/// 分期计划行 → 列表格文本（旧壳桌面档十列 render 的装配层收口）：商户名
/// 解析消费含软删全量商户表（历史计划照常显示）;扣款账户未知 id 回退原始
/// id；进度文案 = 「已还 {paid} / {total} · {count}/{occ} 期」（旧壳
/// scheduled.progress.repaid 同词表），详情失败呈现「加载失败」。
pub(crate) fn assemble_inst_rows(
    rows: &[InstRowSnap],
    ctx: &InstListContext<'_>,
) -> Vec<InstRowCells> {
    rows.iter()
        .map(|row| {
            let fmt = ctx.currencies.format_of(&row.currency_code);
            let merchant = row
                .merchant_id
                .as_deref()
                .and_then(|id| ctx.merchants.name_of(Some(id)))
                .unwrap_or(sched::NEXT_PLACEHOLDER)
                .to_owned();
            let category = ctx
                .categories
                .path_of(row.category_id.as_deref())
                .unwrap_or_else(|| sched::NEXT_PLACEHOLDER.to_owned());
            let account = ctx
                .accounts
                .name_of(Some(row.account_id.as_str()))
                .unwrap_or(row.account_id.as_str())
                .to_owned();
            let total = sched::amount_text(row.total_amount_cents, &fmt, ctx.privacy_enabled);
            let progress = if row.detail_failed {
                sched::DETAIL_FAILED_TEXT.to_owned()
            } else {
                format!(
                    "已还 {} / {} · {}/{} 期",
                    sched::amount_text(row.completed_amount_cents, &fmt, ctx.privacy_enabled),
                    sched::amount_text(row.total_amount_cents, &fmt, ctx.privacy_enabled),
                    row.completed_count,
                    row.total_occurrences
                )
            };
            InstRowCells {
                id: row.id.clone(),
                note: row.note.clone().unwrap_or_default(),
                merchant,
                category,
                account,
                total,
                progress,
                progress_percent: progress_percent(
                    row.completed_count,
                    Some(row.total_occurrences),
                ),
                recurrence: sched::recurrence_label(&row.recurrence_type, row.recurrence_interval),
                start_date: row.start_date.clone(),
                status: sched::status_label(&row.status),
                actions: sched::lifecycle_actions_of(&row.status),
            }
        })
        .collect()
}

/// 进度百分比（期数维度：已完成期数 / 总期数；总期数异常兜底 0；旧壳
/// progressPercentage 同口径）。
pub(crate) fn progress_percent(completed_count: i64, total: Option<i64>) -> f32 {
    let total = total.unwrap_or(0);
    if total <= 0 {
        return 0.0;
    }
    ((completed_count.max(0) as f32 / total as f32) * 100.0).min(100.0)
}

// ---------------------------------------------------------------------------
// 分期页签组件（分期清单卡 + 新建弹窗 + 行操作 + 详情弹层）。
// ---------------------------------------------------------------------------

pub(crate) fn installments_pane(pending_focus: Signal<Option<String>>) -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 装载批与数据面 ============================
    let rows = signal(InstRowsSnap::default());
    let currencies = signal(CurrencyTable::default());
    let categories = signal(CategoryTable::default());
    // 参考数据原始行集（表单选项面的消费字段投影，交易表单先例同形）。
    let category_rows = signal(txn_form::FormCategories::default());
    let accounts_raw = signal(txn_form::FormAccounts::default());
    let merchants = signal(MerchantTable::default());
    let merchants_raw = signal(txn_form::FormMerchants::default());
    let privacy = signal(false);
    let active_filter = signal("active".to_owned());
    let loading = signal(true);
    let error = signal(None::<String>);
    let status = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批（清单 + 逐行详情扩展 + 参考数据一次读齐）：清单/参考任一失败
    // 升级为装载错误条位；逐行详情失败记行级 detail_failed 不拖垮整单。
    // 竞态 latest-wins（搜索视图先例），后发不落位。
    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || -> InstLoadBatch {
                    let plans = boot::plan_rows(&db)?;
                    let mut inst_rows = Vec::new();
                    for plan in plans
                        .iter()
                        .filter(|p| p.core.kind == ledger_scheduled::ScheduledKind::Installment)
                    {
                        // 逐行详情扩展：已完成期次数与金额实时汇总（既有
                        // 详情命令的 completed_occurrences / completed_
                        // amount_cents，不持久化不推算）；详情命令失败记
                        // 行级 detailFailed 不静默。
                        let (detail_failed, complete) =
                            match boot::scheduled_plan_detail(&db, &plan.core.id) {
                                Ok(detail) => (
                                    false,
                                    (detail.completed_occurrences, detail.completed_amount_cents),
                                ),
                                Err(_) => (true, (0, 0)),
                            };
                        inst_rows.push(inst_row_snap_of(
                            plan,
                            detail_failed,
                            complete.0,
                            complete.1,
                        ));
                    }
                    Ok((
                        inst_rows,
                        boot::currency_rows(&db)?,
                        boot::category_rows(&db)?,
                        boot::account_rows(&db)?,
                        boot::merchant_rows(&db)?,
                    ))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((inst_rows, curr_rows, cat_rows, acct_rows, merch_rows))) => {
                        rows.set(InstRowsSnap(inst_rows));
                        currencies.set(CurrencyTable::from_currencies(&curr_rows));
                        categories.set(CategoryTable::from_categories(&cat_rows));
                        accounts_raw.set(txn_form::FormAccounts(acct_rows));
                        merchants.set(MerchantTable::from_merchants(&merch_rows));
                        merchants_raw.set(txn_form::FormMerchants(merch_rows));
                        category_rows.set(txn_form::FormCategories(cat_rows));
                        privacy.set(crate::transactions::amount_privacy_enabled());
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // ============================ 计划详情弹层与失效刷新 ============================
    let detail_open_id = signal(None::<String>);
    // 详情重试 / 展开成功的回报（changed）：清单同步重拉 + 交易失效信号广播。
    let reload_after_change = move || {
        reload();
        signals::notify_transactions_invalidated();
    };

    // ============================ 挂载（首屏装载 + focus 开窗回报） ============================
    let detail_open_id_landing = detail_open_id;
    effect(move || {
        let _landed = nav::store().current.get();
        reload();
        let focus = pending_focus.get_untracked();
        if let Some(id) = focus {
            detail_open_id_landing.set(Some(id));
            pending_focus.set(None);
        }
    });

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    let danger_fg = crate::theme::color("destructive");
    // 主按钮前景 = primary-foreground 令牌（亮暗双态随表；旧裸白直调退役）。
    let primary_fg = crate::theme::color("primary-foreground");
    // 进度条正常色（#2033 预算 → #2039 概览同款取舍：正常态用账本语义绿）。
    let progress_fg = crate::transactions::amount_color(assembly::AmountSemantic::Income);
    // 进度条轨道色 = muted 令牌（弱化底语义角色；消费方零本地色纪律）。
    let progress_track = crate::theme::color("muted");

    // ============================ 清单呈现 ============================
    // 状态过滤（纯前端过滤，不产生请求——旧壳 filteredRows 同口径）。
    let filtered_rows = computed(move || {
        let filter = active_filter.get();
        rows.get()
            .0
            .iter()
            .filter(|row| row.status == filter)
            .cloned()
            .collect::<Vec<_>>()
    });

    // 行格单元（渲染期零格式化）。
    let assembled = computed(move || {
        let accounts_table =
            AccountTable::from_accounts(accounts_raw.get().0.iter().collect::<Vec<_>>());
        let ctx = InstListContext {
            currencies: &currencies.get(),
            categories: &categories.get(),
            accounts: &accounts_table,
            merchants: &merchants.get(),
            privacy_enabled: privacy.get(),
        };
        assemble_inst_rows(&filtered_rows.get(), &ctx)
    });

    // ============================ 行动作执行（写路径 + 反馈 + 重拉） ============================
    let perform_status = move |plan_id: String, new_status: ScheduledStatus| {
        status.set(None);
        let feedback = match new_status {
            ScheduledStatus::Active => sched::MSG_RESUMED.to_owned(),
            ScheduledStatus::Paused => sched::MSG_PAUSED.to_owned(),
            ScheduledStatus::Cancelled | ScheduledStatus::Completed => {
                sched::MSG_CANCELLED.to_owned()
            }
        };
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_scheduled_plan_status(db, &plan_id, new_status),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(feedback));
                        reload();
                    }
                    Ok(Err(e)) => status.set(Some(format!("操作失败: {e}"))),
                    Err(_) => status.set(Some("操作失败: 内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 新建分期弹窗 ============================
    let create_open = signal(false);
    let create_total = signal(String::new());
    let create_periods_text = signal(String::new());
    let create_note = signal(String::new());
    let create_account = signal(None::<String>);
    let create_category = signal(None::<String>);
    let create_merchant_typed = signal(String::new());
    let create_merchant_selected = signal(None::<String>);
    let create_recurrence = signal(Some("monthly".to_owned()));
    let create_interval_text = signal("1".to_owned());
    let create_start_date = signal(chrono::Local::now().date_naive());
    let create_error = signal(None::<String>);

    let reset_create = move || {
        create_total.set(String::new());
        create_periods_text.set(String::new());
        create_note.set(String::new());
        create_account.set(None);
        create_category.set(None);
        create_merchant_typed.set(String::new());
        create_merchant_selected.set(None);
        create_recurrence.set(Some("monthly".to_owned()));
        create_interval_text.set("1".to_owned());
        create_start_date.set(chrono::Local::now().date_naive());
        create_error.set(None);
    };

    // 币种随账户推导（只读展示，旧壳 disabled AppSelect 同位）。
    let create_currency_text = computed(move || {
        derive_currency_text(create_account.get().as_deref(), &accounts_raw.get().0)
    });

    // 每期金额预览：总额与期数均合法时给出（含尾差文案，装配层单点），
    // 否则空。拆 preview_text 接线即红（headless 负向条目钉点）。
    let preview = computed(move || {
        let Some(total_cents) = assembly::amount_text_to_cents(&create_total.get()) else {
            return String::new();
        };
        let Ok(periods) = create_periods_text.get().trim().parse::<i64>() else {
            return String::new();
        };
        let fmt = currencies.get().format_of(&derive_currency_text(
            create_account.get().as_deref(),
            &accounts_raw.get().0,
        ));
        preview_text(total_cents, periods, &fmt)
    });

    let open_create = move || {
        reset_create();
        create_open.set(true);
    };
    let close_create = move || {
        create_open.set(false);
        reset_create();
    };

    // 新建分期提交（旧壳 create 校验面 + 商户解析在提交流程收口的同位形态：
    // 校验（账户必选 → 总额 > 0 → 期数 >= 1 → 总额 >= 期数）→ 每期 floor
    // 口径（amount_cents 存每期金额，与期次生成一致）→ 商户语义解析与建档
    // 在同一个 blocking 批内完成 → 成功反馈 + 弹窗复位关闭 + 清单重拉；失
    // 败错误条位呈现不重置草稿（旧壳弹窗保持打开同语义）。
    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let Some(account_id) = create_account.get() else {
            create_error.set(Some(sched::MSG_SELECT_ACCOUNT.to_owned()));
            return;
        };
        let Some(total_cents) = assembly::amount_text_to_cents(&create_total.get()) else {
            create_error.set(Some(MSG_TOTAL_POSITIVE.to_owned()));
            return;
        };
        if total_cents <= 0 {
            create_error.set(Some(MSG_TOTAL_POSITIVE.to_owned()));
            return;
        }
        let Some(total_occurrences) = create_periods_text
            .get()
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 1)
        else {
            create_error.set(Some(MSG_PERIODS_MIN.to_owned()));
            return;
        };
        if total_cents < total_occurrences {
            create_error.set(Some(MSG_TOTAL_BELOW_PERIODS.to_owned()));
            return;
        }
        let Some(interval) = create_interval_text
            .get()
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 1)
        else {
            create_error.set(Some(sched::MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        let Some(recurrence_id) = create_recurrence.get() else {
            create_error.set(Some(sched::MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        let Ok(recurrence_type) = recurrence_id.parse::<ledger_scheduled::RecurrenceType>() else {
            create_error.set(Some(sched::MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        // 每期金额预览口径的提交同款（floor 均分；amount_cents 存每期金额）。
        let (per_period_cents, _) = installment_schedule(total_cents, total_occurrences)
            .or_else(|| {
                create_error.set(Some(MSG_TOTAL_BELOW_PERIODS.to_owned()));
                None
            })
            .unwrap_or((0, 0));
        let merchant = merchant_ref_of(
            None,
            create_merchant_selected.get_untracked().as_deref(),
            &create_merchant_typed.get_untracked(),
        );
        let note = create_note.get().trim().to_owned();
        let input = ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::Installment,
            account_id: account_id.clone(),
            category_id: create_category.get_untracked(),
            amount_cents: per_period_cents,
            currency_code: currency_of_account(&account_id, &accounts_raw.get_untracked().0)
                .unwrap_or_default(),
            recurrence_type,
            recurrence_interval: interval,
            recurrence_day: None,
            start_date: create_start_date.get_untracked().to_string(),
            note: (!note.is_empty()).then_some(note),
            // 商户在 blocking 批内按语义解析（订阅页签同单点）；先落空占
            // 位，建档前由内部分支覆写。
            merchant_id: None,
            policy_id: None,
            total_amount_cents: Some(total_cents),
            total_occurrences: Some(total_occurrences),
            to_account_id: None,
        };
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        // 商户解析单点（旧壳 resolveMerchantRef 的写时收口）：
                        // Keep / Selected 直接携 id（域在用校验），NewName 走
                        // 按名 find-or-create；None 不挂商户。
                        let resolved = match merchant.clone() {
                            MerchantRefOf::None => None,
                            MerchantRefOf::Keep(id) | MerchantRefOf::Selected(id) => Some(id),
                            MerchantRefOf::NewName(name) => {
                                Some(boot::create_merchant_by_name(db, &name)?)
                            }
                        };
                        boot::create_scheduled_plan(
                            db,
                            ledger_scheduled::CreateScheduledInput {
                                merchant_id: resolved,
                                ..input
                            },
                        )
                    }
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        // 成功闭环（旧壳 onSubmitted 原子动作）：关窗 + 草稿
                        // 复位 + 清单刷新。
                        create_open.set(false);
                        reset_create();
                        status.set(Some(MSG_INST_CREATED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => create_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 视图树 ============================
    // 列宿主映射（装配列 → nsh-base 列；富余宽全给备注列）。
    let columns = computed(move || inst_columns(adaptive::content_width()));

    // 取消确认的目标计划 id（按行匹配的气泡一次只开一个）。
    let confirm_cancel = signal(None::<String>);

    // 行格自绘（行操作列按钮组 + 行锚点；进度列带行级 test 锚点——旧壳
    // inst-progress-{id} 同位：进度条 + 已还文案）。
    let render_cell = make_action_renderer(
        move || assembled.get(),
        detail_open_id,
        confirm_cancel,
        perform_status,
        progress_fg,
        progress_track,
    );

    // 新建商户下拉选项源（在用商户；订阅页签同口径）。
    let create_merchant_options = move || merchant_options_active(&merchants_raw.get().0);

    // 清单卡（标题 + 状态过滤按钮 + 新建分期 + 表）。
    let list_card = view()
        .flex_1()
        .flex_col()
        .gap_2()
        .app_class("rounded-md border border-border p-3")
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(INST_LIST_TITLE)
                        .text_size(px(font_size::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                )
                .child(view().flex_1())
                .children(sched::plan_status_filters().into_iter().map(|(key, label)| {
                    let key_owned = key.to_owned();
                    button()
                        .app_class(
                            "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                        )
                        .child(
                            text(label)
                                .text_size(px(font_size::CAPTION))
                                .test_id(format!("inst-filter-{key_owned}").as_str()),
                        )
                        .on_press(move |_: &()| active_filter.set(key.to_owned()))
                }))
                .child(
                    button()
                        .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                        .child(
                            text(CREATE_INST_OPEN_LABEL)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(primary_fg)
                                .test_id("inst-create-open"),
                        )
                        .on_press(move |_: &()| open_create()),
                ),
        )
        .child(
            view().test_id("inst-table").flex_1().child(
                data_table()
                    .columns(columns)
                    .data(move || {
                        // 追踪读（DataTable 的 data prop 是响应式闭包）。
                        let snapshot = assembled.get();
                        DataTableRows::new(snapshot.len())
                            .texts("note", snapshot.iter().map(|r| r.note.as_str()))
                            .texts("merchant", snapshot.iter().map(|r| r.merchant.as_str()))
                            .texts("category", snapshot.iter().map(|r| r.category.as_str()))
                            .texts("account", snapshot.iter().map(|r| r.account.as_str()))
                            .texts("total", snapshot.iter().map(|r| r.total.as_str()))
                            .texts("progress", snapshot.iter().map(|r| r.progress.as_str()))
                            .texts("recurrence", snapshot.iter().map(|r| r.recurrence.as_str()))
                            .texts("start_date", snapshot.iter().map(|r| r.start_date.as_str()))
                            .texts("status", snapshot.iter().map(|r| r.status.as_str()))
                            .texts("actions", snapshot.iter().map(|_| ""))
                    })
                    .render_cell(render_cell)
                    .loading(move || loading.get())
                    .loading_view(text("正在加载…").text_color(muted))
                    .empty(text("暂无数据").text_color(muted))
                    .classes(crate::transactions::table_classes())
                    .flex_1(),
            ),
        );

    // 新建分期弹窗内容（旧壳布局对齐：总金额/期数/每期金额预览在备注与
    // 账户之前，其余字段与订阅表单同款排布）。
    let create_form_fields = move || {
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("总金额", muted))
                    .child(input()
                        .test_id("inst-total")
                        .value(move || create_total.get())
                        .placeholder("分期总金额")
                        .optional_label("分期总金额")
                        .text_color(fg)
                        .w(px(160.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| create_total.set(value.to_owned())))
                    .child(
                        // 币种随账户推导（只读展示；拆 derive_currency_text 即红）。
                        text(move || create_currency_text.get())
                            .text_size(px(font_size::SECONDARY))
                            .text_color(muted)
                            .test_id("inst-currency"),
                    ),
            )
            .child(form_row("期数", muted, input()
                .test_id("inst-periods")
                .value(move || create_periods_text.get())
                .placeholder("总期数")
                .optional_label("总期数")
                .text_color(fg)
                .w(px(160.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| create_periods_text.set(value.to_owned()))))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("每期金额", muted))
                    .child(
                        text(move || preview.get())
                            .text_size(px(font_size::SECONDARY))
                            .text_color(fg)
                            .test_id("inst-preview"),
                    ),
            )
            .child(form_row("备注", muted, input()
                .test_id("inst-note")
                .value(move || create_note.get())
                .placeholder("分期用途，如：手机分期")
                .optional_label("分期备注")
                .text_color(fg)
                .w(px(280.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| create_note.set(value.to_owned()))))
            .child(form_row("扣款账户", muted, pinyin_select::pinyin_select(
                "inst-account",
                move || account_options_of(&accounts_raw.get().0, None),
                create_account,
                "选择账户",
                false,
            )))
            .child(form_row("分类", muted, pinyin_select::pinyin_select(
                "inst-category",
                move || crate::assembly::expense_category_options(&category_rows.get().0),
                create_category,
                "选择分类",
                true,
            )))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("商户", muted))
                    .child(combo_select::combo_select(
                        "inst-merchant",
                        create_merchant_options,
                        create_merchant_typed,
                        create_merchant_selected,
                        "选择商户，可直接输入新名称",
                    )),
            )
            .child(form_row("重复", muted, view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(text("每").text_size(px(font_size::SECONDARY)).text_color(fg))
                .child(input()
                    .test_id("inst-interval")
                    .value(move || create_interval_text.get())
                    .placeholder("1")
                    .optional_label("周期间隔")
                    .text_color(fg)
                    .w(px(90.0))
                    .app_class(
                        "bg-background rounded-md border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |value: &str| create_interval_text.set(value.to_owned())))
                .child(crate::select::app_select(
                    "inst-recurrence-type",
                    sched::recurrence_options,
                    create_recurrence,
                    "周期类型",
                    false,
                ))))
            .child(form_row("开始日", muted, date_select::date_select(
                "inst-start", create_start_date,
            )))
            .child(
                view()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .child(field_label("", muted))
                    .child(when(
                        move || create_error.get().is_some(),
                        move || {
                            text(move || format!("创建失败: {}", create_error.get().unwrap_or_default()))
                                .text_size(px(font_size::CAPTION))
                                .text_color(danger_fg)
                                .test_id("inst-create-error")
                        },
                    ).otherwise(view)),
            )
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        button()
                            .app_class(
                                "px-3 py-2 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .child(
                                text("取消")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(fg)
                                    .test_id("inst-create-cancel"),
                            )
                            .on_press(move |_: &()| close_create()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                            .child(
                                text(CREATE_INST_SUBMIT_LABEL)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(primary_fg)
                                    .test_id("inst-create"),
                            )
                            .on_press(move |_: &()| perform_create()),
                    ),
            )
    };

    view()
        .flex_1()
        .flex_col()
        .gap_3()
        .app_class("bg-background")
        .child(
            // 装载失败条位（码化错误 zh 原样，不静默）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败: {}", error.get().unwrap_or_default()))
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("inst-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(list_card)
        .child(
            // 操作反馈条位（成功/失败共用一条，新动作开始清回空）。
            when(
                move || status.get().is_some(),
                move || {
                    text(move || status.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(fg)
                        .test_id("inst-status")
                },
            )
            .otherwise(view),
        )
        .child(
            // 新建分期弹窗。
            modal::app_modal(
                move || create_open.get(),
                || CREATE_INST_TITLE.to_owned(),
                close_create,
                create_form_fields,
            ),
        )
        .child(
            // 期次详情弹层（三页签通用；重试 / 展开成功回调重拉与失效广播）。
            scheduled_plan_detail::plan_detail_modal(
                detail_open_id,
                currencies,
                privacy,
                reload_after_change,
            ),
        )
}

// ---------------------------------------------------------------------------
// 模块级 helper（行操作渲染爪 + 列宿主映射 + 进度条）。
// ---------------------------------------------------------------------------

/// 行格自绘器（清单表「进度」列带行级锚点 + 进度条自绘（预算/概览先例同形
/// 的轨道 + 指示条，旧壳 NProgress type=line 消费同位）+ 操作列按钮组：
/// 期次恒可用、暂停限 active、恢复限 paused、取消限「未结束」带确认气泡、
/// 无编辑描述符；动作锚点 = `inst-{key}-{rowId}`）。拆行操作任一接线（期
/// 次目标 / 状态写路径 / 气泡确认）即红。
fn make_action_renderer(
    assembled: impl Fn() -> Vec<InstRowCells> + Copy + 'static,
    detail_open_id: Signal<Option<String>>,
    confirm_cancel: Signal<Option<String>>,
    perform_status: impl Fn(String, ScheduledStatus) + Clone + 'static,
    progress_fg: nsh_core::element::AdaptiveColor,
    progress_track: nsh_core::element::AdaptiveColor,
) -> impl Fn(&nsh_base::DataTableCell) -> nsh_core::element::AnyElement {
    move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let Some(row) = assembled().get(index).cloned() else {
            return text(cell.text.clone()).into_element();
        };
        match cell.key.as_ref() {
            "progress" => {
                // 进度条自绘（旧壳 NProgress type=line 本壳等价形态：轨道
                // 定宽 + 按百分比宽度的指示条，颜色语义绿——禁用竖版进度）。
                let fg = crate::theme::color("foreground");
                view()
                    .test_id(format!("inst-progress-{}", row.id).as_str())
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        // 轨道色 = muted 令牌（弱化底语义角色，亮暗双态随表；
                        // 旧中性洗色 hsla 直调退役）。轨道/指示条全圆 = 圆角阶
                        // rounded-full；轨道定宽与 6pt 高为进度条组件内部基线
                        //（规格未成文，DESIGN.md §6 现状基线）。
                        view()
                            .w(px(160.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(progress_track)
                            .child(
                                view()
                                    .h(px(6.0))
                                    .w(move || {
                                        px(160.0 * (row.progress_percent.clamp(0.0, 100.0) / 100.0))
                                    })
                                    .rounded_full()
                                    .bg(progress_fg),
                            ),
                    )
                    .child(
                        text(cell.text.clone())
                            .text_size(px(font_size::CAPTION))
                            .text_color(fg),
                    )
                    .into_element()
            }
            "actions" => {
                let fg = crate::theme::color("foreground");
                let danger_fg = crate::theme::color("destructive");
                let id = row.id.clone();
                let mut buttons: Vec<nsh_core::element::AnyElement> = vec![action_button(
                    "期次",
                    format!("inst-detail-{id}"),
                    {
                        let id = id.clone();
                        let open_id = detail_open_id;
                        move || open_id.set(Some(id.clone()))
                    },
                    fg,
                )];
                if row.actions.can_pause {
                    buttons.push(action_button(
                        "暂停",
                        format!("inst-pause-{id}"),
                        {
                            let id = id.clone();
                            let perform = perform_status.clone();
                            move || perform(id.clone(), ScheduledStatus::Paused)
                        },
                        fg,
                    ));
                }
                if row.actions.can_resume {
                    buttons.push(action_button(
                        "恢复",
                        format!("inst-resume-{id}"),
                        {
                            let id = id.clone();
                            let perform = perform_status.clone();
                            move || perform(id.clone(), ScheduledStatus::Active)
                        },
                        fg,
                    ));
                }
                if row.actions.can_cancel {
                    // 轻量确认气泡（旧壳 AppPopconfirm 消费同位；开合闭包
                    // 各自持有行 id 克隆——move 进闭包后不再外用原值）。
                    let popconfirm_id = id.clone();
                    buttons.push(
                        popconfirm::popconfirm(
                            "inst-cancel",
                            {
                                let popconfirm_id = popconfirm_id.clone();
                                move || confirm_cancel.get().as_deref() == Some(&popconfirm_id)
                            },
                            move |open: bool| {
                                if open {
                                    confirm_cancel.set(Some(popconfirm_id.clone()));
                                } else {
                                    confirm_cancel.set(None);
                                }
                            },
                            popconfirm::PopconfirmSpec {
                                message: INST_CANCEL_CONFIRM_TEXT.to_owned(),
                                confirm_label: "确定".to_owned(),
                                cancel_label: "取消".to_owned(),
                            },
                            view()
                                .app_class("px-2 py-1 rounded-md text-destructive hover:bg-accent")
                                .child(
                                    text("取消")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id(format!("inst-cancel-{id}").as_str()),
                                ),
                            {
                                let id = id.clone();
                                let perform = perform_status.clone();
                                move || perform(id.clone(), ScheduledStatus::Cancelled)
                            },
                        )
                        .into_element(),
                    );
                }
                view().flex_row().gap_1().children(buttons).into_element()
            }
            _ => text(cell.text.clone()).number_of_lines(1).into_element(),
        }
    }
}

/// 列宿主映射（[`INST_COLUMNS`] → nsh-base 列；订阅页签同形——富余宽全给
/// 备注列）。
fn inst_columns(width: f32) -> Vec<DataTableColumn> {
    // 表格外围定宽扣减（内容列 p-3 + 分区卡 p-3 + 描边 2pt，订阅页签同值）。
    const INST_TABLE_WIDTH_PAD: f32 = 12.0 * 2.0 + 12.0 * 2.0 + 2.0;
    let mut columns: Vec<DataTableColumn> = INST_COLUMNS
        .iter()
        .map(|def| {
            let mut column = DataTableColumn::default();
            column.key = def.key.into();
            column.title = Some(def.title.into());
            column.width = Some(Points(def.width));
            column.align = match def.align {
                ColumnAlign::Start => DataTableAlign::Start,
                ColumnAlign::End => DataTableAlign::End,
            };
            column
        })
        .collect();
    let suggested: f32 = INST_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - INST_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(note) = columns.iter_mut().find(|column| column.key == "note")
    {
        note.width = note.width.map(|w| Points(w.0 + extra));
    }
    columns
}

// ---------------------------------------------------------------------------
// lib 单测（每期 floor 口径 / 预览文案 / 进度装配：渲染期零格式化的兜底面）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(completed: (i64, i64)) -> InstRowSnap {
        InstRowSnap {
            id: "p-1".to_owned(),
            note: Some("手机分期".to_owned()),
            merchant_id: Some("m-1".to_owned()),
            account_id: "a-1".to_owned(),
            category_id: Some("c-1".to_owned()),
            total_amount_cents: 30_000,
            amount_cents: 10_000,
            currency_code: "CNY".to_owned(),
            recurrence_type: "monthly".to_owned(),
            recurrence_interval: 1,
            start_date: "2026-01-15".to_owned(),
            status: "active".to_owned(),
            total_occurrences: 3,
            detail_failed: false,
            completed_count: completed.0,
            completed_amount_cents: completed.1,
        }
    }

    fn empty_ctx(privacy: bool) -> InstListContext<'static> {
        let currency = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        InstListContext {
            currencies: Box::leak(Box::new(CurrencyTable::from_currencies([&currency]))),
            categories: Box::leak(Box::new(assembly::CategoryTable::default())),
            accounts: Box::leak(Box::new(AccountTable::default())),
            merchants: Box::leak(Box::new(assembly::MerchantTable::default())),
            privacy_enabled: privacy,
        }
    }

    fn fmt_cny() -> assembly::CurrencyFormat {
        let currency = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        CurrencyTable::from_currencies([&currency]).format_of("CNY")
    }

    #[test]
    fn installment_schedule_floors_and_puts_reminder_in_last_period() {
        // 100 分 / 3 期旧壳 34/34/32 期次生成口径。
        assert_eq!(installment_schedule(100, 3), Some((33, 34)));
        assert_eq!(
            installment_schedule(30_000, 3),
            Some((10_000, 10_000)),
            "整除时末期与每期一致"
        );
        assert_eq!(installment_schedule(0, 3), None, "非正总额非法");
        assert_eq!(installment_schedule(100, 0), None, "非正期数非法");
    }

    #[test]
    fn preview_text_matches_old_shell_wording() {
        assert_eq!(
            preview_text(10_000, 2, &fmt_cny()),
            "每期 ¥50",
            "整除不提尾差"
        );
        assert_eq!(
            preview_text(100, 3, &fmt_cny()),
            "每期 ¥0.33 · 末期 ¥0.34（含尾差）",
            "不整除明确标注末期含尾差"
        );
        assert_eq!(preview_text(0, 3, &fmt_cny()), "", "非法输入预览空");
        assert_eq!(preview_text(100, 0, &fmt_cny()), "");
    }

    #[test]
    fn inst_cells_cover_ten_desktop_columns() {
        let keys: Vec<&str> = INST_COLUMNS.iter().map(|c| c.key).collect();
        assert_eq!(
            keys,
            vec![
                "note",
                "merchant",
                "category",
                "account",
                "total",
                "progress",
                "recurrence",
                "start_date",
                "status",
                "actions"
            ],
            "桌面档十列一字不动（回归红线）"
        );
    }

    #[test]
    fn assemble_inst_rows_fills_cells_and_progress() {
        let cells = assemble_inst_rows(&[snap((2, 20_000))], &empty_ctx(false));
        assert_eq!(cells.len(), 1);
        let cell = &cells[0];
        assert_eq!(cell.id, "p-1");
        assert_eq!(cell.total, "¥300", "总金额按币种小数位呈现");
        assert_eq!(
            cell.progress, "已还 ¥200 / ¥300 · 2/3 期",
            "进度 = 已还汇总（实时面，不持久化不推算）"
        );
        assert!(
            (cell.progress_percent - 200.0_f32 / 3.0).abs() < 1e-4,
            "期数维度百分比"
        );
        assert_eq!(cell.recurrence, "每月");
        assert_eq!(cell.start_date, "2026-01-15");
        assert_eq!(cell.status, "进行中");
        assert!(cell.actions.can_pause && cell.actions.can_cancel);
        assert!(
            !cell.actions.can_edit,
            "分期行无编辑动作（ADR-0041 决策 7）"
        );
    }

    #[test]
    fn inst_progress_detail_failure_is_load_failed_not_empty() {
        let mut row = snap((0, 0));
        row.detail_failed = true;
        let cells = assemble_inst_rows(&[row], &empty_ctx(false));
        assert_eq!(
            cells[0].progress, "加载失败",
            "详情命令失败区分于「无数据」不静默"
        );
    }

    #[test]
    fn progress_text_masks_when_privacy_enabled() {
        let cells = assemble_inst_rows(&[snap((2, 20_000))], &empty_ctx(true));
        assert_eq!(
            cells[0].progress, "已还 •••• / •••• · 2/3 期",
            "隐私掩码恒形（期数维度仍可读）"
        );
    }

    #[test]
    fn progress_percent_edges() {
        assert_eq!(progress_percent(0, None), 0.0, "总期数缺席兜底 0");
        assert_eq!(progress_percent(0, Some(0)), 0.0);
        assert_eq!(progress_percent(3, Some(3)), 100.0);
        assert_eq!(progress_percent(5, Some(3)), 100.0, "钳制 100");
        assert!((progress_percent(1, Some(3)) - 100.0 / 3.0).abs() < 1e-4);
    }
}
