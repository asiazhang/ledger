//! 定时转账页签（issue #2043 定时②；旧壳 `TransfersPane.vue` 的重生——
//! ScheduledPlanList 计划清单模块的转账形态薄适配：计划清单 + 计划表单 +
//! 期次详情弹层）。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：
//! - **清单**：定时转账计划行集（域读路径过滤转账形态）+ 逐行详情读出
//!   「下期转账 = 最早 pending 期次」（仅选取不现场推算日期，旧壳
//!   earliestPendingOccurrence 同口径）；状态过滤（进行中 / 已暂停 /
//!   已取消 / 已完成——WITH_COMPLETED 键集：一次性执行后 completed 计划
//!   恢复可见可查，#309 显式可见变化口径同源）；纯前端过滤不产生请求
//!   （旧壳 filteredRows 同口径）。
//! - **新建定时转账**（旧壳 ModalIntent 单成员意图的本壳等价形态：显示由
//!   「意图非空」派生，关闭统一清回 None 终态）：共享表单件消费
//!   （[`crate::scheduled_subscription`]：账户下拉 / 币种随账户推导 / 周期
//!   三字段 / 开始日 / 备注形态），转账形态真差异留本页签——转入账户候选
//!   按转出账户币种过滤并排除转出账户本身（转出 = 转入被域显式拒绝，词汇
//!   表 ScheduledTransfer 边界），切换转出账户清空币种不再匹配的转入选中
//!   （旧壳 watcher 同语义，[`to_account_clear_needed`] 纯函数单点）；无
//!   商户字段（旧壳 #203 口径：转账不挂商户，本方账户用 to_account_id 承
//!   载，域显式拒绝携带商户）；总期数三态——留空 = 无限循环、1 = 一次性、
//!   N = 有限期数（旧壳 totalOccurrencesPlaceholder 同口径，金额格带
//!   「× N期」尾饰）。
//! - **行操作**：期次（详情）恒可用、暂停限 active、恢复限 paused、取消限
//!   「未结束」带确认气泡（转账形态取消确认文案：旧壳
//!   transferCancelConfirm）；编辑是订阅适配器自建描述符（ADR-0041 决策
//!   7），本页签不呈现。
//! - **计划来源落点**：`pending_focus` 在本页签挂载时消费开窗（详情弹窗按
//!   id 独立取数——已取消计划照常可开）并回报清闸——页签切换不复开（旧壳
//!   usePlanFocusLanding 时序的页签侧）。
//!
//! 写路径：直调域公开写入口（[`crate::boot`]，不经 IPC，旧壳 scheduled 族
//! 命令同一执行协议）；清单重拉归动作回调（旧壳「计划写入刻意零信号、消
//! 费面按回调刷新」同口径）；期次执行落交易行后由视图层手动广播交易失效
//! 信号（交易列表重读，同订阅页签口径）。
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
use crate::assembly::{self, AccountTable, CategoryOption, ColumnAlign, CurrencyTable};
use crate::boot;
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::pinyin_select;
use crate::popconfirm;
use crate::scheduled as sched;
use crate::scheduled_plan_detail;
use crate::scheduled_subscription::{
    account_options_of, action_button, currency_of_account, derive_currency_text, field_label,
    form_row,
};
use crate::signals;
use crate::{nav, txn_form};

// ---------------------------------------------------------------------------
// 词表（旧壳 packages/i18n zh-CN scheduled.json 逐条平价；i18n 框架级方案不在
// 本票，字面承载）。
// ---------------------------------------------------------------------------
pub(crate) const TRANSFER_LIST_TITLE: &str = "定时转账清单";
pub(crate) const CREATE_TRANSFER_TITLE: &str = "新建定时转账";
pub(crate) const CREATE_TRANSFER_OPEN_LABEL: &str = "新建转账";
pub(crate) const CREATE_TRANSFER_SUBMIT_LABEL: &str = "创建转账";
pub(crate) const TRANSFER_CANCEL_CONFIRM_TEXT: &str =
    "取消后不再自动转账，已生成的交易与历史期次保留。确认取消？";
pub(crate) const MSG_TRANSFER_CREATED: &str = "已创建定时转账";
pub(crate) const MSG_SELECT_FROM_ACCOUNT: &str = "请选择转出账户";
pub(crate) const MSG_SELECT_TO_ACCOUNT: &str = "请选择转入账户";
/// 总期数文本输入的下限（旧壳 NInputNumber min 1 的提交通道兜底文案：文本
/// 输入在本壳的明文案对应形态）。
pub(crate) const MSG_TOTAL_OCC_MIN: &str = "总期数不小于 1";

// ---------------------------------------------------------------------------
// 行集快照与装配（清单格文本的纯 Rust 单测面：渲染期零格式化纪律）。
// ---------------------------------------------------------------------------

/// 定时转账计划行可克隆快照（视图局部）：域读路径 [`crate::boot::plan_rows`]
/// 按 kind 过滤转账计划 + 逐行详情读出的最早 pending 期次投影——清单表单消
/// 费的字段全集在装载批一次性投影持有（[`crate::scheduled::SubRowSnap`] 转
/// 账形态同构面，to_account / total_occurrences 换位）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TransferRowSnap {
    pub id: String,
    pub note: Option<String>,
    /// 转出账户（计划核心账户）。
    pub account_id: String,
    /// 转入账户（转账扩展表；域在档协议必有值）。
    pub to_account_id: Option<String>,
    pub amount_cents: i64,
    pub currency_code: String,
    pub recurrence_type: String,
    pub recurrence_interval: i64,
    pub start_date: String,
    pub status: String,
    /// 总期数（None = 无限循环；1 = 一次性；N = 有限期数）。
    pub total_occurrences: Option<i64>,
    /// 详情命令失败：与「无数据」区分，不静默（下期转账占位「加载失败」，
    /// 旧壳 detailFailed 同口径）。
    pub detail_failed: bool,
    /// 最早 pending 期次（日期, 金额分）；无 pending（预生成窗口之外）= None。
    pub next: Option<(String, i64)>,
}

/// 行集投影 [`TransferRowSnap`] 的集合容器（装载批一次性投影）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TransferRowsSnap(pub Vec<TransferRowSnap>);

/// 装载批一次读齐的回执（转账行集 + 币种 / 账户参考数据；complexity 门禁
/// 的具名面）。
type TransferLoadBatch = Result<
    (
        Vec<TransferRowSnap>,
        Vec<ledger_currencies::Currency>,
        Vec<ledger_accounts::Account>,
    ),
    ledger_infra::error::AppError,
>;

/// 装载批投影（转账行快照构造单点；扩展字段在计划行持有侧投影）。
fn transfer_row_snap_of(
    plan: &ledger_scheduled::ScheduledTransactionWithExt,
    detail_failed: bool,
    next: Option<(String, i64)>,
) -> TransferRowSnap {
    TransferRowSnap {
        id: plan.core.id.clone(),
        note: plan.core.note.clone(),
        account_id: plan.core.account_id.clone(),
        to_account_id: plan.to_account_id.clone(),
        amount_cents: plan.core.amount_cents,
        currency_code: plan.core.currency_code.clone(),
        recurrence_type: plan.core.recurrence_type.clone(),
        recurrence_interval: plan.core.recurrence_interval,
        start_date: plan.core.start_date.clone(),
        status: plan.core.status.clone(),
        total_occurrences: plan.total_occurrences,
        detail_failed,
        next,
    }
}

/// 列表行格单元格（渲染期零格式化：文本与行操作可用性已是一次性装配产物）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TransferRowCells {
    pub id: String,
    pub note: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub recurrence: String,
    pub start_date: String,
    pub status: String,
    pub next: String,
    pub actions: crate::scheduled::SubRowActions,
}

/// 清单装配上下文（参考数据表的只读投影 + 隐私快照；转账无商户/分类面）。
pub(crate) struct TransferListContext<'a> {
    pub currencies: &'a CurrencyTable,
    pub accounts: &'a AccountTable,
    pub privacy_enabled: bool,
}

/// 列定义（旧壳桌面档九列一字不动：备注/转出账户/转入账户/金额/周期/开始日/
/// 状态/下期转账/操作；金额右对齐。宽度建议值与账户表先例同量级，富余宽给
/// 备注列由宿主映射分配）。
pub(crate) const TRANSFER_COLUMNS: &[assembly::ColumnDef] = &[
    assembly::ColumnDef {
        key: "note",
        title: "备注",
        width: 150.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "from",
        title: "转出账户",
        width: 95.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "to",
        title: "转入账户",
        width: 95.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "amount",
        title: "金额",
        width: 90.0,
        align: ColumnAlign::End,
    },
    assembly::ColumnDef {
        key: "recurrence",
        title: "周期",
        width: 70.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "start_date",
        title: "开始日",
        width: 90.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "status",
        title: "状态",
        width: 70.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "next",
        title: "下期转账",
        width: 140.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "actions",
        title: "操作",
        width: 110.0,
        align: ColumnAlign::Start,
    },
];

/// 定时转账计划行 → 列表格文本（旧壳桌面档九列 render 的装配层收口）：
/// 转出入账户名解析消费账户表（未知 id 回退原始 id，旧壳 accountName 同口
/// 径）；转入在档协议必有值，缺席呈现「—」；金额格带有限期数的「× N期」
/// 尾饰（旧壳 occurrencesSuffix 同词）。
pub(crate) fn assemble_transfer_rows(
    rows: &[TransferRowSnap],
    ctx: &TransferListContext<'_>,
) -> Vec<TransferRowCells> {
    rows.iter()
        .map(|row| {
            let fmt = ctx.currencies.format_of(&row.currency_code);
            let base = sched::amount_text(row.amount_cents, &fmt, ctx.privacy_enabled);
            let amount = match row.total_occurrences {
                Some(total) => format!("{base}{}", occurrences_suffix(total)),
                None => base,
            };
            let next = match (&row.next, row.detail_failed) {
                (_, true) => sched::DETAIL_FAILED_TEXT.to_owned(),
                (Some((date, cents)), _) => {
                    format!(
                        "{date} · {}",
                        sched::amount_text(*cents, &fmt, ctx.privacy_enabled)
                    )
                }
                (None, _) => sched::NEXT_PLACEHOLDER.to_owned(),
            };
            TransferRowCells {
                id: row.id.clone(),
                note: row.note.clone().unwrap_or_default(),
                from: ctx
                    .accounts
                    .name_of(Some(row.account_id.as_str()))
                    .unwrap_or(row.account_id.as_str())
                    .to_owned(),
                to: row
                    .to_account_id
                    .as_deref()
                    .map(|id| ctx.accounts.name_of(Some(id)).unwrap_or(id).to_owned())
                    .unwrap_or_else(|| sched::NEXT_PLACEHOLDER.to_owned()),
                amount,
                recurrence: sched::recurrence_label(&row.recurrence_type, row.recurrence_interval),
                start_date: row.start_date.clone(),
                status: sched::status_label(&row.status),
                next,
                actions: sched::lifecycle_actions_of(&row.status),
            }
        })
        .collect()
}

/// 周期后缀（旧壳 `scheduled.column.occurrencesSuffix` = 「× {n}期」的金额
/// 格尾饰；与金额间一个空格，旧壳 amountText 拼接同形）。
pub(crate) fn occurrences_suffix(total_occurrences: i64) -> String {
    format!(" × {total_occurrences}期")
}

// ---------------------------------------------------------------------------
// 转入账户选项纯函数（转账形态真差异的离线单测面）。
// ---------------------------------------------------------------------------

/// 转入账户候选（旧壳 toAccountOptions 同口径）：基础 = 可选择账户面（归档
/// 不进新选择）；再按转出账户币种过滤并排除转出账户本身（转出 = 转入被域
/// 拒绝）；未选转出账户时为全部可选账户。
pub(crate) fn to_account_options_of(
    accounts: &[ledger_accounts::Account],
    from_id: Option<&str>,
) -> Vec<CategoryOption> {
    let from_currency = from_id.and_then(|id| currency_of_account(id, accounts));
    account_options_of(accounts, None)
        .into_iter()
        .filter(|option| {
            if Some(option.id.as_str()) == from_id {
                return false;
            }
            match &from_currency {
                Some(currency) => {
                    currency_of_account(&option.id, accounts).is_some_and(|c| c == *currency)
                }
                None => true,
            }
        })
        .collect()
}

/// 切换转出账户后是否清空转入选中（旧壳 watch(fromAccountId) 同语义的纯函
/// 数裁定面）：两侧账户都在表且币种不再一致时需要清空；任一侧缺席不清。
pub(crate) fn to_account_clear_needed(
    accounts: &[ledger_accounts::Account],
    from_id: Option<&str>,
    to_id: Option<&str>,
) -> bool {
    let (Some(from_id), Some(to_id)) = (from_id, to_id) else {
        return false;
    };
    let Some(from_currency) = currency_of_account(from_id, accounts) else {
        return false;
    };
    let Some(to_currency) = currency_of_account(to_id, accounts) else {
        return false;
    };
    to_currency != from_currency
}

// ---------------------------------------------------------------------------
// 定时转账页签组件（转账清单卡 + 新建弹窗 + 行操作 + 详情弹层）。
// ---------------------------------------------------------------------------

pub(crate) fn transfers_pane(pending_focus: Signal<Option<String>>) -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 装载批与数据面 ============================
    let rows = signal(TransferRowsSnap::default());
    let currencies = signal(CurrencyTable::default());
    // 参考数据原始行集（表单选项面与账户名解析的消费字段投影，交易表单
    // 先例同形的投影新类型）。
    let accounts_raw = signal(txn_form::FormAccounts::default());
    let privacy = signal(false);
    let active_filter = signal("active".to_owned());
    let loading = signal(true);
    let error = signal(None::<String>);
    let status = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批（清单 + 逐行详情扩展 + 账户参考数据一次读齐）：清单/参考任一
    // 失败升级为装载错误条位；逐行详情失败记行级 detail_failed 不拖垮整单。
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
                let outcome = blocking(move || -> TransferLoadBatch {
                    let plans = boot::plan_rows(&db)?;
                    let mut transfer_rows = Vec::new();
                    for plan in plans.iter().filter(|p| {
                        p.core.kind == ledger_scheduled::ScheduledKind::ScheduledTransfer
                    }) {
                        // 逐行详情扩展：最早 pending 期次（仅选取不现场推算
                        // 日期）；详情命令失败记行级 detailFailed 不静默。
                        let detail = boot::scheduled_plan_detail(&db, &plan.core.id);
                        let (detail_failed, next) = match detail {
                            Ok(detail) => (
                                false,
                                detail
                                    .pending_occurrences
                                    .iter()
                                    .min_by_key(|occ| occ.scheduled_date.clone())
                                    .map(|occ| (occ.scheduled_date.clone(), occ.amount_cents)),
                            ),
                            Err(_) => (true, None),
                        };
                        transfer_rows.push(transfer_row_snap_of(plan, detail_failed, next));
                    }
                    Ok((
                        transfer_rows,
                        boot::currency_rows(&db)?,
                        boot::account_rows(&db)?,
                    ))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((transfer_rows, curr_rows, acct_rows))) => {
                        rows.set(TransferRowsSnap(transfer_rows));
                        currencies.set(CurrencyTable::from_currencies(&curr_rows));
                        accounts_raw.set(txn_form::FormAccounts(acct_rows));
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
    // 详情重试 / 展开成功的回报（changed）：清单同步重拉 + 交易失效信号
    // 广播（期次执行落交易行 → 交易列表重读见行；接缝线与订阅页签同位）。
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
        let ctx = TransferListContext {
            currencies: &currencies.get(),
            accounts: &accounts_table,
            privacy_enabled: privacy.get(),
        };
        assemble_transfer_rows(&filtered_rows.get(), &ctx)
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

    // ============================ 新建定时转账弹窗 ============================
    let create_open = signal(false);
    let create_note = signal(String::new());
    let create_from_account = signal(None::<String>);
    let create_to_account = signal(None::<String>);
    let create_amount = signal(String::new());
    let create_recurrence = signal(Some("monthly".to_owned()));
    let create_interval_text = signal("1".to_owned());
    let create_total_occ_text = signal(String::new());
    let create_start_date = signal(chrono::Local::now().date_naive());
    let create_error = signal(None::<String>);

    let reset_create = move || {
        create_note.set(String::new());
        create_from_account.set(None);
        create_to_account.set(None);
        create_amount.set(String::new());
        create_recurrence.set(Some("monthly".to_owned()));
        create_interval_text.set("1".to_owned());
        create_total_occ_text.set(String::new());
        create_start_date.set(chrono::Local::now().date_naive());
        create_error.set(None);
    };

    // 币种随转出账户推导（只读展示，旧壳 disabled AppSelect 同位）。
    let create_currency_text = computed(move || {
        derive_currency_text(create_from_account.get().as_deref(), &accounts_raw.get().0)
    });

    // 切换转出账户后清空币种不再匹配的转入选中（防跨币种提交；旧壳
    // watch(fromAccountId) 同语义，纯函数裁定面 [`to_account_clear_needed`]）。
    {
        let accounts_body = accounts_raw;
        let to_account = create_to_account;
        let from_account = create_from_account;
        effect(move || {
            let from = from_account.get();
            if to_account_clear_needed(
                &accounts_body.get().0,
                from.as_deref(),
                to_account.get().as_deref(),
            ) {
                to_account.set(None);
            }
        });
    }

    let open_create = move || {
        reset_create();
        create_open.set(true);
    };
    let close_create = move || {
        create_open.set(false);
        reset_create();
    };

    // 新建提交（旧壳 create 校验面 submitCreate 编排的对应形态）：校验
    // （转出必选 → 转入必选 → 金额 > 0 → 周期间隔 >= 1 → 总期数留空或
    //  >= 1）→ 建档写接缝；无商户面（旧壳 scheduled_transfer 自带：商户
    // 解析显式跳过，域显式拒绝携带）→ 成功反馈 + 弹窗复位关闭 + 清单重
    // 拉；失败错误条位呈现不重置草稿（旧壳弹窗保持打开同语义）。
    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let Some(from_account_id) = create_from_account.get() else {
            create_error.set(Some(MSG_SELECT_FROM_ACCOUNT.to_owned()));
            return;
        };
        let Some(to_account_id) = create_to_account.get() else {
            create_error.set(Some(MSG_SELECT_TO_ACCOUNT.to_owned()));
            return;
        };
        let Some(amount_cents) = assembly::amount_text_to_cents(&create_amount.get()) else {
            create_error.set(Some(sched::MSG_AMOUNT_POSITIVE.to_owned()));
            return;
        };
        if amount_cents <= 0 {
            create_error.set(Some(sched::MSG_AMOUNT_POSITIVE.to_owned()));
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
        // 总期数三态：留空 = 无限循环（None）；填写 = 正整数（旧壳
        // NInputNumber min 1 的文本输入通道对应面）。
        let total_occ = match create_total_occ_text.get().trim() {
            "" => None,
            text => match text.parse::<i64>().ok().filter(|n| *n >= 1) {
                Some(total) => Some(total),
                None => {
                    create_error.set(Some(MSG_TOTAL_OCC_MIN.to_owned()));
                    return;
                }
            },
        };
        let note = create_note.get().trim().to_owned();
        let input = ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::ScheduledTransfer,
            account_id: from_account_id.clone(),
            category_id: None,
            amount_cents,
            currency_code: currency_of_account(&from_account_id, &accounts_raw.get_untracked().0)
                .unwrap_or_default(),
            recurrence_type,
            recurrence_interval: interval,
            recurrence_day: None,
            start_date: create_start_date.get_untracked().to_string(),
            note: (!note.is_empty()).then_some(note),
            // 定时转账不挂商户（旧壳 #203 口径；携带即被域显式拒绝）。
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: total_occ,
            to_account_id: Some(to_account_id),
        };
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_scheduled_plan(db, input),
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
                        status.set(Some(MSG_TRANSFER_CREATED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => create_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 视图树 ============================
    // 列宿主映射（装配列 → nsh-base 列；富余宽全给备注列——订阅页签同形）。
    let columns = computed(move || transfer_columns(adaptive::content_width()));

    // 取消确认的目标计划 id（按行匹配的气泡一次只开一个）。
    let confirm_cancel = signal(None::<String>);

    // 行格自绘（行操作列按钮组 + 行锚点；下期转账列带行级 test 锚点——旧壳
    // next-transfer-{id} 同位）。
    let render_cell = make_action_renderer(
        move || assembled.get(),
        detail_open_id,
        confirm_cancel,
        perform_status,
    );

    // 清单卡（标题 + 状态过滤按钮 + 新建转账 + 表）。
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
                    text(TRANSFER_LIST_TITLE)
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
                                .test_id(format!("transfer-filter-{key_owned}").as_str()),
                        )
                        .on_press(move |_: &()| active_filter.set(key.to_owned()))
                }))
                .child(
                    button()
                        .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                        .child(
                            text(CREATE_TRANSFER_OPEN_LABEL)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(primary_fg)
                                .test_id("transfer-create-open"),
                        )
                        .on_press(move |_: &()| open_create()),
                ),
        )
        .child(
            view().test_id("transfer-table").flex_1().child(
                data_table()
                    .columns(columns)
                    .data(move || {
                        // 追踪读（DataTable 的 data prop 是响应式闭包）。渲染
                        // 期零格式化：文本已是一次性装配产物。
                        let snapshot = assembled.get();
                        DataTableRows::new(snapshot.len())
                            .texts("note", snapshot.iter().map(|r| r.note.as_str()))
                            .texts("from", snapshot.iter().map(|r| r.from.as_str()))
                            .texts("to", snapshot.iter().map(|r| r.to.as_str()))
                            .texts("amount", snapshot.iter().map(|r| r.amount.as_str()))
                            .texts("recurrence", snapshot.iter().map(|r| r.recurrence.as_str()))
                            .texts("start_date", snapshot.iter().map(|r| r.start_date.as_str()))
                            .texts("status", snapshot.iter().map(|r| r.status.as_str()))
                            .texts("next", snapshot.iter().map(|r| r.next.as_str()))
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

    // 新建定时转账弹窗内容。
    let create_form_fields = move || {
        view()
            .flex_col()
            .gap_2()
            .child(form_row("备注", muted, input()
                .test_id("transfer-note")
                .value(move || create_note.get())
                .placeholder("转账用途，如：月度储蓄")
                .optional_label("转账备注")
                .text_color(fg)
                .w(px(280.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| create_note.set(value.to_owned()))))
            .child(form_row("转出账户", muted, pinyin_select::pinyin_select(
                "transfer-from-account",
                move || account_options_of(&accounts_raw.get().0, None),
                create_from_account,
                "选择转出账户",
                false,
            )))
            .child(form_row("转入账户", muted, pinyin_select::pinyin_select(
                "transfer-to-account",
                move || {
                    // 转入候选：同币种过滤 + 排除转出账户本身（纯函数单点）。
                    to_account_options_of(
                        &accounts_raw.get().0,
                        create_from_account.get_untracked().as_deref(),
                    )
                },
                create_to_account,
                "选择转入账户（与转出同币种）",
                false,
            )))
            .child(form_row("金额", muted, view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(input()
                    .test_id("transfer-amount")
                    .value(move || create_amount.get())
                    .placeholder("每期金额")
                    .optional_label("每期金额")
                    .text_color(fg)
                    .w(px(160.0))
                    .app_class(
                        "bg-background rounded-md border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |value: &str| create_amount.set(value.to_owned())))
                .child(
                    // 币种随转出账户推导（只读展示；拆 derive_currency_text 即红）。
                    text(move || create_currency_text.get())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(muted)
                        .test_id("transfer-currency"),
                )))
            .child(form_row("重复", muted, view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(text("每").text_size(px(font_size::SECONDARY)).text_color(fg))
                .child(input()
                    .test_id("transfer-interval")
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
                    "transfer-recurrence-type",
                    sched::recurrence_options,
                    create_recurrence,
                    "周期类型",
                    false,
                ))))
            .child(form_row("总期数", muted, input()
                .test_id("transfer-total-occurrences")
                .value(move || create_total_occ_text.get())
                .placeholder("留空为无限循环")
                .optional_label("总期数")
                .text_color(fg)
                .w(px(200.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| create_total_occ_text.set(value.to_owned()))))
            .child(form_row("开始日", muted, date_select::date_select(
                "transfer-start", create_start_date,
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
                                .test_id("transfer-create-error")
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
                                    .test_id("transfer-create-cancel"),
                            )
                            .on_press(move |_: &()| close_create()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                            .child(
                                text(CREATE_TRANSFER_SUBMIT_LABEL)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(primary_fg)
                                    .test_id("transfer-create"),
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
                        .test_id("transfer-error")
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
                        .test_id("transfer-status")
                },
            )
            .otherwise(view),
        )
        .child(
            // 新建定时转账弹窗。
            modal::app_modal(
                move || create_open.get(),
                || CREATE_TRANSFER_TITLE.to_owned(),
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
// 模块级 helper（行操作渲染爪 + 列宿主映射）。
// ---------------------------------------------------------------------------

/// 行格自绘器（清单表「下期转账」列带行级锚点 + 操作列按钮组，旧壳
/// PlanRowActions 透传同位：期次恒可用、暂停限 active、恢复限 paused、取消
/// 限「未结束」带确认气泡（目标一次只开一个）、无编辑描述符；动作锚点 =
/// `transfer-{key}-{rowId}`）。拆行操作任一接线（期次目标 / 状态写路径 /
/// 气泡确认）即红（headless「删除即红」条目的实拆点）。
fn make_action_renderer(
    assembled: impl Fn() -> Vec<TransferRowCells> + Copy + 'static,
    detail_open_id: Signal<Option<String>>,
    confirm_cancel: Signal<Option<String>>,
    perform_status: impl Fn(String, ScheduledStatus) + Clone + 'static,
) -> impl Fn(&nsh_base::DataTableCell) -> nsh_core::element::AnyElement {
    move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let Some(row) = assembled().get(index).cloned() else {
            return text(cell.text.clone()).into_element();
        };
        match cell.key.as_ref() {
            "next" => text(cell.text.clone())
                .text_size(px(font_size::SECONDARY))
                .number_of_lines(1)
                .test_id(format!("transfer-next-{}", row.id).as_str())
                .into_element(),
            "actions" => {
                let fg = crate::theme::color("foreground");
                let danger_fg = crate::theme::color("destructive");
                let id = row.id.clone();
                let mut buttons: Vec<nsh_core::element::AnyElement> = vec![action_button(
                    "期次",
                    format!("transfer-detail-{id}"),
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
                        format!("transfer-pause-{id}"),
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
                        format!("transfer-resume-{id}"),
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
                            "transfer-cancel",
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
                                message: TRANSFER_CANCEL_CONFIRM_TEXT.to_owned(),
                                confirm_label: "确定".to_owned(),
                                cancel_label: "取消".to_owned(),
                            },
                            view()
                                .app_class("px-2 py-1 rounded-md text-destructive hover:bg-accent")
                                .child(
                                    text("取消")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id(format!("transfer-cancel-{id}").as_str()),
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

/// 列宿主映射（[`TRANSFER_COLUMNS`] → nsh-base 列；订阅页签同形——富余宽
/// 全给备注列）。
fn transfer_columns(width: f32) -> Vec<DataTableColumn> {
    // 表格外围定宽扣减（内容列 p-3 + 分区卡 p-3 + 描边 2pt，订阅页签同值）。
    const TRANSFER_TABLE_WIDTH_PAD: f32 = 12.0 * 2.0 + 12.0 * 2.0 + 2.0;
    let mut columns: Vec<DataTableColumn> = TRANSFER_COLUMNS
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
    let suggested: f32 = TRANSFER_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - TRANSFER_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(note) = columns.iter_mut().find(|column| column.key == "note")
    {
        note.width = note.width.map(|w| Points(w.0 + extra));
    }
    columns
}

// ---------------------------------------------------------------------------
// lib 单测（转入账户选项 / 清装与/ 装配口径：渲染期零格式化的兜底面）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(total_occurrences: Option<i64>) -> TransferRowSnap {
        TransferRowSnap {
            id: "p-1".to_owned(),
            note: Some("月度储蓄".to_owned()),
            account_id: "a-1".to_owned(),
            to_account_id: Some("a-2".to_owned()),
            amount_cents: 3_000,
            currency_code: "CNY".to_owned(),
            recurrence_type: "monthly".to_owned(),
            recurrence_interval: 1,
            start_date: "2026-01-15".to_owned(),
            status: "active".to_owned(),
            total_occurrences,
            detail_failed: false,
            next: Some(("2026-02-15".to_owned(), 300)),
        }
    }

    fn acc(id: &str, name: &str, currency: &str, archived: bool) -> ledger_accounts::Account {
        ledger_accounts::Account {
            id: id.to_owned(),
            name: name.to_owned(),
            currency_code: currency.to_owned(),
            is_archived: archived,
            is_deleted: false,
            is_hidden: false,
            kind: ledger_accounts::AccountType::Cash,
            initial_balance_cents: 0,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            created_at: String::new(),
            updated_at: String::new(),
            version: 0,
            device_id: String::new(),
        }
    }

    fn empty_ctx(privacy: bool) -> TransferListContext<'static> {
        let currency = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        TransferListContext {
            currencies: Box::leak(Box::new(CurrencyTable::from_currencies([&currency]))),
            accounts: Box::leak(Box::new(AccountTable::default())),
            privacy_enabled: privacy,
        }
    }

    fn named_ctx(privacy: bool) -> TransferListContext<'static> {
        let currency = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        TransferListContext {
            currencies: Box::leak(Box::new(CurrencyTable::from_currencies([&currency]))),
            accounts: Box::leak(Box::new(AccountTable::from_accounts(&[
                acc("a-1", "转出钱包", "CNY", false),
                acc("a-2", "储蓄罐", "CNY", false),
            ]))),
            privacy_enabled: privacy,
        }
    }

    #[test]
    fn transfer_cells_cover_nine_desktop_columns() {
        let keys: Vec<&str> = TRANSFER_COLUMNS.iter().map(|c| c.key).collect();
        assert_eq!(
            keys,
            vec![
                "note",
                "from",
                "to",
                "amount",
                "recurrence",
                "start_date",
                "status",
                "next",
                "actions"
            ],
            "桌面档九列一字不动（回归红线）"
        );
    }

    #[test]
    fn assemble_transfer_rows_fills_cells_and_next_transfer() {
        let cells = assemble_transfer_rows(&[snap(Some(3))], &named_ctx(false));
        assert_eq!(cells.len(), 1);
        let cell = &cells[0];
        assert_eq!(cell.id, "p-1");
        assert_eq!(cell.note, "月度储蓄");
        assert_eq!(cell.from, "转出钱包");
        assert_eq!(cell.to, "储蓄罐");
        assert_eq!(
            cell.amount, "¥30 × 3期",
            "金额格跟进 ×期数尾饰（有限总期数）"
        );
        assert_eq!(cell.recurrence, "每月");
        assert_eq!(cell.start_date, "2026-01-15");
        assert_eq!(cell.status, "进行中");
        assert_eq!(
            cell.next, "2026-02-15 · ¥3",
            "下期转账 = 最早 pending 期次日期 · 金额"
        );
    }

    #[test]
    fn transfer_amount_has_no_suffix_when_unlimited() {
        let mut row = snap(None);
        row.to_account_id = None;
        let cells = assemble_transfer_rows(&[row], &empty_ctx(false));
        assert_eq!(cells[0].amount, "¥30", "无限循环无期数尾饰");
        assert_eq!(cells[0].to, "—", "转入在档协议必有值，缺席呈现「—」");
    }

    #[test]
    fn transfer_next_charge_or_detail_failure_slots_are_stable() {
        let mut row = snap(None);
        row.next = None;
        let cells = assemble_transfer_rows(&[row.clone()], &empty_ctx(false));
        assert_eq!(cells[0].next, "—", "无 pending 期次占位（不现场推算日期）");
        row.detail_failed = true;
        let cells = assemble_transfer_rows(&[row], &empty_ctx(false));
        assert_eq!(
            cells[0].next, "加载失败",
            "详情命令失败区分于「无数据」不静默"
        );
    }

    #[test]
    fn amount_masks_when_privacy_enabled() {
        let cells = assemble_transfer_rows(&[snap(Some(2))], &empty_ctx(true));
        assert_eq!(
            cells[0].amount, "•••• × 2期",
            "隐私掩码恒形（期数尾饰保留）"
        );
    }

    #[test]
    fn transfer_row_actions_exclude_edit() {
        // 主动作集：期次恒可用、暂停限 active、恢复限 paused、取消限「未结束」，
        // 编辑（订阅适配器自建描述符）排除。
        for status in ["active", "paused", "cancelled", "completed"] {
            assert!(
                !assemble_transfer_rows(&[snap(None)], &empty_ctx(false))[0]
                    .actions
                    .can_edit,
                "转账行无编辑动作：{status}"
            );
        }
        let cells = assemble_transfer_rows(&[snap(None)], &empty_ctx(false));
        assert!(cells[0].actions.can_pause);
        assert!(cells[0].actions.can_cancel);
        assert!(!cells[0].actions.can_resume);
    }

    #[test]
    fn to_account_options_filter_same_currency_and_exclude_from() {
        let accounts = vec![
            acc("a-1", "现金", "CNY", false),
            acc("a-2", "储蓄", "CNY", false),
            acc("a-3", "美元户", "USD", false),
            acc("a-4", "旧卡", "CNY", true),
        ];
        // 未选转出：全部可选择账户（归档不进）。
        let options = to_account_options_of(&accounts, None);
        assert_eq!(
            options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec!["a-1", "a-2", "a-3"],
            "未选转出 = 全可选择账户面（归档不进新选择）"
        );
        // 选定转出 a-1：同币种（CNY）、排除 a-1 本身、归档不进。
        let options = to_account_options_of(&accounts, Some("a-1"));
        assert_eq!(
            options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec!["a-2"],
            "转入候选 = 同币种 + 排除转出本身"
        );
    }

    #[test]
    fn to_account_clear_needed_matches_old_shell_watcher() {
        let accounts = vec![
            acc("a-1", "现金", "CNY", false),
            acc("a-2", "储蓄", "CNY", false),
            acc("a-3", "美元户", "USD", false),
        ];
        assert!(
            !to_account_clear_needed(&accounts, None, Some("a-3")),
            "未选转出不清"
        );
        assert!(
            !to_account_clear_needed(&accounts, Some("a-1"), None),
            "未选转入不清"
        );
        assert!(!to_account_clear_needed(
            &accounts,
            Some("a-1"),
            Some("a-2")
        ));
        assert!(
            to_account_clear_needed(&accounts, Some("a-1"), Some("a-3")),
            "币种不再匹配才清"
        );
    }

    #[test]
    fn occurrences_suffix_matches_old_shell_template() {
        assert_eq!(occurrences_suffix(3), " × 3期");
        assert_eq!(occurrences_suffix(1), " × 1期", "一次性转账同样带尾饰");
    }
}
