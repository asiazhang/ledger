//! 订阅页签（issue #2042；旧壳 `SubscriptionsPane.vue` 的重生——计划清单 +
//! 计划表单 + 花费面板 + 计划详情弹层的订阅形态薄适配）。
//!
//! 定时②（#2043）起兼**兄弟页签的共享件落点**：[`account_options_of`]（账户
//! 下拉与归档补值）、[`currency_of_account`] / [`derive_currency_text`]（币
//! 种随账户推导，ADR-0134 决策 5）、[`merchant_ref_of`] /
//! [`merchant_options_active`]（商户五分支语义与在用选项面）与
//! [`field_label`] / [`form_row`] / [`action_button`]（表单行与行操作按钮
//! 形态）归本模块单点，分期 / 定时转账页签消费不自建。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：
//! - **清单**：订阅计划行集（域读路径过滤订阅形态）+ 逐行详情读出「下期
//!   扣款 = 最早 pending 期次」（仅选取不现场推算日期，旧壳
//!   earliestPendingOccurrence 同口径）；状态过滤（进行中 / 已暂停 / 已取消，
//!   订阅无「已完成」键集——#309 时期现状，纯前端过滤不产生请求）；清单
//!   加载失败/生命周期变更的反馈与重拉时序走动作回调（旧壳「计划写入刻意
//!   零信号、消费面按回调刷新」同口径）。
//! - **新建订阅**（旧壳 ModalIntent 单成员意图的 NSH 等价形态：显示由
//!   「意图非空」派生，关闭统一清回 None 终态）：计划表单装配纯 Rust 化
//!   ——公共草稿字段（备注/账户/分类/商户/币种随账户推导/周期三字段/开始日）、
//!   商户解析（「输入即建 + 重名兜底」走 [`crate::boot::create_merchant_by_name`]
//!   按名 find-or-create 单点，旧壳 resolveMerchantRef 的重名竞态处理在域内
//!   收敛）、金额校验（账户必选、金额 > 0）、元转分走换算接缝
//!   [`assembly::amount_text_to_cents`]；币种随所选账户推导只读展示（旧壳
//!   useAccountCurrency 机械推导，ADR-0134 决策 5 的前端收敛——杜绝新增
//!   「交易币种 != 账户币种」的脏计划）。
//! - **编辑订阅**（ADR-0023 决策三）：仅非金额字段（备注/账户/分类/商户），
//!   弹窗无金额输入；提交走订阅编辑写接缝，携带金额即被域显式拒绝；挂保单
//!   的缴费协议不显示商户（issue #713 / ADR-0082 决策 2）；编辑不改已生成的
//!   期次与交易，只影响未来。编辑实例的账户下拉取「这份草稿的当前值」补值
//!   口径（旧壳 spec #1952 / ADR-0142 决策 4：归档账户补回选项面），软删原
//!   商户以「（已删除商户）」兜底选项承载原 id（提交时按「未改动」语义原样
//!   保留）。商户为全量替换语义。
//! - **行操作**（旧壳 PlanRowActions / rowActions 状态矩阵同形）：期次
//!   （详情）恒可用、暂停限 active、恢复限 paused、取消限「未结束」带确认
//!   气泡、编辑限「未结束」无确认；动作回复 = 反馈条位 + 清单与花费重拉。
//! - **计划来源落点**：`pending_focus`（计划 id）在本页签挂载时消费开窗
//!   （详情弹窗按 id 独立取数——已取消计划照常可开）并回报清闸——页签切换
//!   不复开（旧壳 usePlanFocusLanding「读 id → 开窗 → 回报」时序的页签侧）。
//!
//! 兄弟模块：[`crate::scheduled`]（视图壳与词表）、
//! [`crate::scheduled_plan_detail`]（期次详情弹层）、[`crate::scheduled_spend`]
//! （花费面板）。
//!
//! 界面文案仅 zh-CN；码化错误 zh 原样呈现（反馈条位对应旧壳 toast）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::scheduled::SubRowCells;
use ledger_scheduled::ScheduledStatus;

use crate::adaptive;
use crate::assembly::{
    self, AccountTable, CategoryOption, CategoryTable, ColumnAlign, CurrencyTable, MerchantTable,
};
use crate::boot;
use crate::combo_select;
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::pinyin_select;
use crate::popconfirm;
use crate::scheduled::{
    CANCEL_CONFIRM_TEXT, CREATE_SUB_SUBMIT_LABEL, CREATE_SUB_TITLE, EDIT_SUB_TITLE,
    MSG_AMOUNT_POSITIVE, MSG_CANCELLED, MSG_CREATED, MSG_INTERVAL_MIN, MSG_PAUSED, MSG_RESUMED,
    MSG_SAVED, MSG_SELECT_ACCOUNT, SUB_COLUMNS, SUB_LIST_TITLE, assemble_sub_rows,
    recurrence_options, subscription_status_filters,
};
use crate::scheduled_spend::{SpendPanelState, assemble_spend, spend_panel};
use crate::signals;
use crate::{nav, txn_form};

// ---------------------------------------------------------------------------
// 装载批与快照容器（读闭包返回值，复杂度聚合单点；账户块先例同形）。
// ---------------------------------------------------------------------------

/// 装载批：行集 + 参考数据束 + 花费总览（花费读失败独立降级为面板失败态，
/// 不拖垮清单——旧壳花费面板独立 reload 同口径）。
type SubLoads = ledger_infra::error::Result<(
    Vec<crate::scheduled::SubRowSnap>,
    Vec<ledger_currencies::Currency>,
    Vec<ledger_categories::Category>,
    Vec<ledger_accounts::Account>,
    Vec<ledger_merchants::Merchant>,
    Result<ledger_scheduled::SubscriptionSpendOverview, ledger_infra::error::AppError>,
)>;

/// 行集快照容器。
type RowsSnap = crate::scheduled::SubRowsSnap;

/// 计划快照（视图信号持有用；域行无 Clone 的部分字段在装载批一次性投影，
/// [`crate::scheduled::SubRowSnap`]）。
type SubRowSnap = crate::scheduled::SubRowSnap;

/// 装载批投影（计划的订阅行快照构造单点）。
fn sub_row_snap_of(
    plan: &ledger_scheduled::ScheduledTransactionWithExt,
    detail_failed: bool,
    next: Option<(String, i64)>,
) -> SubRowSnap {
    SubRowSnap {
        id: plan.core.id.clone(),
        note: plan.core.note.clone(),
        merchant_id: plan.merchant_id.clone(),
        policy_id: plan.policy_id.clone(),
        account_id: plan.core.account_id.clone(),
        category_id: plan.core.category_id.clone(),
        amount_cents: plan.core.amount_cents,
        currency_code: plan.core.currency_code.clone(),
        recurrence_type: plan.core.recurrence_type.clone(),
        recurrence_interval: plan.core.recurrence_interval,
        start_date: plan.core.start_date.clone(),
        status: plan.core.status.clone(),
        detail_failed,
        next,
    }
}

/// 账户下拉选项（旧壳 accountOptions / usePinnedAccountOptions 同口径）：
/// 可选择账户排除归档（归档不进新选择），编辑面按「这份草稿的当前值」补值
/// ——pinned 账户即使已归档也回选项面（spec #1952 / ADR-0142 决策 4）。
pub(crate) fn account_options_of(
    accounts: &[ledger_accounts::Account],
    pinned: Option<&str>,
) -> Vec<CategoryOption> {
    accounts
        .iter()
        .filter(|a| !a.is_archived || Some(a.id.as_str()) == pinned)
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect()
}

/// 账户 id → 币种（币种随账户推导的取数单点；未选择 = 无币种文案）。
pub(crate) fn currency_of_account(
    account_id: &str,
    accounts: &[ledger_accounts::Account],
) -> Option<String> {
    accounts
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| a.currency_code.clone())
}

/// 币种随账户推导的展示文本（未选择账户时占位空串；旧壳 disabled AppSelect
/// 空值同口径）。
pub(crate) fn derive_currency_text(
    account_id: Option<&str>,
    accounts: &[ledger_accounts::Account],
) -> String {
    account_id
        .and_then(|id| currency_of_account(id, accounts))
        .unwrap_or_default()
}

/// 商户选择器双态值 → 商户语义闭集（旧壳 resolveMerchantRef 的五分支收口，
/// [`assembly::MerchantRef`] 同构的灯光面）：编辑面原值保留单列一个变体
///（提交值与原商户相同视为保持历史引用，软删商户照常保留）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MerchantRefOf {
    /// 不挂商户。
    None,
    /// 编辑面原样保留。
    Keep(String),
    /// 用户选定在用商户（id 提交，域在用校验）。
    Selected(String),
    /// 自由文本（写时间经 [`crate::boot::create_merchant_by_name`] 按名
    /// find-or-create：精确匹配在用名复用、未命中即建）。
    NewName(String),
}

/// 表单选择器状态 → 商户语义（双态判定单点）。
pub(crate) fn merchant_ref_of(
    editing: Option<&str>,
    selected: Option<&str>,
    typed: &str,
) -> MerchantRefOf {
    match selected {
        Some(id) => match editing {
            Some(orig) if orig == id => MerchantRefOf::Keep(orig.to_owned()),
            _ => MerchantRefOf::Selected(id.to_owned()),
        },
        None if typed.trim().is_empty() => MerchantRefOf::None,
        None => MerchantRefOf::NewName(typed.trim().to_owned()),
    }
}

// ---------------------------------------------------------------------------
// 订阅页签组件（订阅清单卡 + 新建/编辑弹窗 + 行操作 + 花费面板 + 详情弹层）。
// ---------------------------------------------------------------------------

pub(crate) fn subscriptions_pane(pending_focus: Signal<Option<String>>) -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 装载批与数据面 ============================
    let rows = signal(RowsSnap::default());
    let currencies = signal(CurrencyTable::default());
    let categories = signal(CategoryTable::default());
    // 参考数据原始行集（表单选项面的消费字段投影：txn_form 先例同形之名
    // 投影新类型——域行无 PartialEq，信号持有的字段闭集投影单点）。
    let category_rows = signal(txn_form::FormCategories::default());
    let accounts_raw = signal(txn_form::FormAccounts::default());
    let merchants = signal(MerchantTable::default());
    let merchants_raw = signal(txn_form::FormMerchants::default());
    let privacy = signal(false);
    let spending = signal(SpendPanelState::default());
    let active_filter = signal("active".to_owned());
    let loading = signal(true);
    let error = signal(None::<String>);
    // 操作反馈条位（旧壳 message.success/error 的条位对应物：新动作开始时
    // 清回空；create/edit 的错误归各自弹窗条位，反馈条位承载成功与行动作
    // 结果）。
    let status = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批（清单 + 逐行详情扩展 + 参考数据 + 花费总览一次读齐）：清单
    // / 参考任一失败升级为装载错误条位（清单口径缺失不静默回退）；逐行
    // 详情失败记行级 detail_failed 不拖垮整单（旧壳 Loadable 行内同口径）；
    // 花费读失败独立呈现失败态面板（旧壳花费 loadFailed 同口径）。竞态
    // latest-wins（搜索视图先例），后发不落位。
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
                let outcome = blocking(move || -> SubLoads {
                    let plans = boot::plan_rows(&db)?;
                    let mut sub_rows = Vec::new();
                    for plan in plans
                        .iter()
                        .filter(|p| p.core.kind == ledger_scheduled::ScheduledKind::Subscription)
                    {
                        // 逐行详情扩展：最早 pending 期次（仅选取不现场推算
                        // 日期）；详情命令失败记行级 detailFailed，与「无
                        // 数据」区分不静默。
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
                        sub_rows.push(sub_row_snap_of(plan, detail_failed, next));
                    }
                    Ok((
                        sub_rows,
                        boot::currency_rows(&db)?,
                        boot::category_rows(&db)?,
                        boot::account_rows(&db)?,
                        boot::merchant_rows(&db)?,
                        boot::subscription_spend_overview(&db),
                    ))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((sub_rows, curr_rows, cat_rows, acct_rows, merch_rows, spend))) => {
                        rows.set(crate::scheduled::SubRowsSnap(sub_rows));
                        currencies.set(CurrencyTable::from_currencies(&curr_rows));
                        categories.set(CategoryTable::from_categories(&cat_rows));
                        accounts_raw.set(txn_form::FormAccounts(acct_rows));
                        merchants.set(MerchantTable::from_merchants(&merch_rows));
                        merchants_raw.set(txn_form::FormMerchants(merch_rows.clone()));
                        category_rows.set(txn_form::FormCategories(cat_rows.clone()));
                        privacy.set(crate::transactions::amount_privacy_enabled());
                        match spend {
                            Ok(overview) => {
                                let privacy_enabled = privacy.get_untracked();
                                let table = currencies.get_untracked();
                                spending.set(SpendPanelState::settled(assemble_spend(
                                    &overview,
                                    &table,
                                    privacy_enabled,
                                )));
                            }
                            Err(e) => {
                                // 花费读失败不静默混算（ADR-0023）：面板
                                // 独立呈现失败态，清单照常。
                                tracing::warn!(error = %e, "订阅花费读失败，面板呈现失败态");
                                spending.set(SpendPanelState::failed());
                            }
                        }
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
    // 详情重试 / 展开成功的回报（changed）：清单与花费同步重拉 + 交易失效
    // 信号广播（期次执行落交易行 → 交易列表重读见行；「删除即红」条目钉
    // 在 headless：删本回调的重拉接线 → 弹窗外清单的「下期扣款」行值不
    // 重读即红；删广播接线 → 交易列表不刷新即红）。
    let reload_after_change = move || {
        reload();
        signals::notify_transactions_invalidated();
    };

    // ============================ 挂载（首屏装载 + focus 开窗回报） ============================
    // 来源落点的 focus 开窗：弹窗按 id 独立取数（不受清单状态过滤影响——
    // 已取消计划照常可开），开即回报清闸（页签切换不复开，视图侧信号暂存
    // 清回 None）。
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

    // 行格单元（渲染期零格式化：文本与行操作可用性已是一次性装配产物；
    // 参考数据表快照进入上下文，商户/分类/账户名解析在装配层收口）。
    let assembled = computed(move || {
        let accounts_table =
            AccountTable::from_accounts(accounts_raw.get().0.iter().collect::<Vec<_>>());
        let ctx = crate::scheduled::SubListContext {
            currencies: &currencies.get(),
            categories: &categories.get(),
            accounts: &accounts_table,
            merchants: &merchants.get(),
            privacy_enabled: privacy.get(),
        };
        assemble_sub_rows(&filtered_rows.get(), &ctx)
    });

    // ============================ 行动作执行（写路径 + 反馈 + 重拉） ============================
    // 生命周期变更（暂停/恢复/取消）：写接缝成功 → 状态反馈（旧壳 paused /
    // resumed / cancelled 同词表）+ 清单与花费重拉；失败呈现「操作失败: …」
    // 不重拉（旧壳 toast 同语义）。
    let perform_status = move |plan_id: String, new_status: ledger_scheduled::ScheduledStatus| {
        status.set(None);
        let feedback = match new_status {
            ledger_scheduled::ScheduledStatus::Active => MSG_RESUMED.to_owned(),
            ledger_scheduled::ScheduledStatus::Paused => MSG_PAUSED.to_owned(),
            ledger_scheduled::ScheduledStatus::Cancelled => MSG_CANCELLED.to_owned(),
            ledger_scheduled::ScheduledStatus::Completed => MSG_CANCELLED.to_owned(),
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

    // ============================ 新建订阅弹窗 ============================
    // 显示由「意图非空」派生（旧壳 ModalIntent 单成员闭集的等价形态）；关闭
    // 统一清回 None 终态并复位草稿。
    let create_open = signal(false);
    let create_note = signal(String::new());
    let create_account = signal(None::<String>);
    let create_category = signal(None::<String>);
    let create_merchant_typed = signal(String::new());
    let create_merchant_selected = signal(None::<String>);
    let create_amount = signal(String::new());
    let create_recurrence = signal(Some("monthly".to_owned()));
    let create_interval_text = signal("1".to_owned());
    let create_start_date = signal(chrono::Local::now().date_naive());
    let create_error = signal(None::<String>);

    let reset_create = move || {
        create_note.set(String::new());
        create_account.set(None);
        create_category.set(None);
        create_merchant_typed.set(String::new());
        create_merchant_selected.set(None);
        create_amount.set(String::new());
        create_recurrence.set(Some("monthly".to_owned()));
        create_interval_text.set("1".to_owned());
        create_start_date.set(chrono::Local::now().date_naive());
        create_error.set(None);
    };

    // 币种随账户推导（只读展示）。
    let create_currency_text = computed(move || {
        derive_currency_text(create_account.get().as_deref(), &accounts_raw.get().0)
    });

    let open_create = move || {
        reset_create();
        create_open.set(true);
    };
    let close_create = move || {
        create_open.set(false);
        reset_create();
    };

    // 新建提交（旧壳 create 校验面 + useScheduledPlanForm submitCreate 编排
    // 的对应形态）：校验（账户必选 → 金额 > 0 → 周期间隔 >= 1）→ 商户解析
    // 与计划建档在同一个 blocking 批内完成（商户按名 find-or-create 后建档
    // ——旧壳先 api.createMerchant 再 createScheduledTransaction 的两步在
    // 本壳的同一线程一次收口，语义不变）→ 成功反馈 + 弹窗复位关闭 + 清单
    // 与花费重拉；失败错误条位呈现不重置草稿（旧壳弹窗保持打开同语义）。
    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let Some(account_id) = create_account.get() else {
            create_error.set(Some(MSG_SELECT_ACCOUNT.to_owned()));
            return;
        };
        let Some(amount_cents) = assembly::amount_text_to_cents(&create_amount.get()) else {
            create_error.set(Some(MSG_AMOUNT_POSITIVE.to_owned()));
            return;
        };
        if amount_cents <= 0 {
            create_error.set(Some(MSG_AMOUNT_POSITIVE.to_owned()));
            return;
        }
        let Some(interval) = create_interval_text
            .get()
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 1)
        else {
            create_error.set(Some(MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        let Some(recurrence_id) = create_recurrence.get() else {
            create_error.set(Some(MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        let Ok(recurrence_type) = recurrence_id.parse::<ledger_scheduled::RecurrenceType>() else {
            create_error.set(Some(MSG_INTERVAL_MIN.to_owned()));
            return;
        };
        let merchant = merchant_ref_of(
            None,
            create_merchant_selected.get().as_deref(),
            &create_merchant_typed.get(),
        );
        let note = create_note.get().trim().to_owned();
        let input = ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::Subscription,
            account_id: account_id.clone(),
            category_id: create_category.get_untracked(),
            amount_cents,
            currency_code: currency_of_account(&account_id, &accounts_raw.get_untracked().0)
                .unwrap_or_default(),
            recurrence_type,
            recurrence_interval: interval,
            recurrence_day: None,
            start_date: create_start_date.get_untracked().to_string(),
            note: (!note.is_empty()).then_some(note),
            // 商户在 blocking 批内按语义解析（见下方闭包）；先落空占位，
            // 建档前由内部分支覆写。
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        };
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        // 商户解析单点（旧壳 resolveMerchantRef 的写时收口）：
                        // Keep / Selected 直接携 id（域在用校验），NewName 走
                        // 按名 find-or-create（精确匹配在用名复用、未命中
                        // 即建）；None 不挂商户。
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
                        // 复位 + 清单刷新 + 花费刷新（清单先于花费，同一
                        // 批内完成——重拉时序与旧壳一致）。
                        reset_create();
                        create_open.set(false);
                        status.set(Some(MSG_CREATED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => create_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 编辑订阅弹窗 ============================
    // 目标按计划 id 持有（弹窗内容自 rows 快照按 id 反查原始行，并发删除 =
    // 弹窗关闭不呈现半态）；显示由「目标非空」派生。
    let edit_target = signal(None::<String>);
    let edit_note = signal(String::new());
    let edit_account = signal(None::<String>);
    let edit_category = signal(None::<String>);
    let edit_merchant_typed = signal(String::new());
    let edit_merchant_selected = signal(None::<String>);
    // 被编辑计划的当前商户 id（供商户语义的「未改动」判定）与原商户显示名
    //（含软删兜底：裸 uuid 不可读）。
    let edit_current_merchant_id = signal(None::<String>);
    let edit_current_merchant_label = signal(String::new());
    // 挂保单的缴费协议（行反查回填；编辑弹窗隐藏商户字段，提交也保持置空
    // 语义不回挂商户）。
    let edit_has_policy = signal(false);
    let edit_error = signal(None::<String>);

    // 行反查（快照内单点）；行缺席（并发删除）直接关弹窗，不呈现半态。
    let snap_of = move |plan_id: &str| -> Option<SubRowSnap> {
        rows.get_untracked()
            .0
            .iter()
            .find(|row| row.id == plan_id)
            .cloned()
    };

    // 编辑商户下拉选项（旧壳 editMerchantOptions 同口径）：基础 = 在用商户
    //（软删不进新选择）+ 原商户软删时追加兜底选项承载原 id——裸 uuid 不可
    // 读，提交时按「未改动」语义原样保留。
    let edit_merchant_options = move || -> Vec<CategoryOption> {
        let snapshot = merchants_raw.get_untracked();
        edit_merchant_options_of(
            &snapshot.0,
            edit_current_merchant_id.get_untracked().as_deref(),
            &edit_current_merchant_label.get_untracked(),
        )
    };

    // 新建商户下拉选项源（在用商户；旧壳 merchantOptions 同口径）。
    let create_merchant_options = move || merchant_options_active(&merchants_raw.get().0);

    let open_edit = move |plan_id: String| {
        let Some(row) = snap_of(&plan_id) else {
            return;
        };
        edit_note.set(row.note.clone().unwrap_or_default());
        edit_account.set(Some(row.account_id.clone()));
        edit_category.set(row.category_id.clone());
        edit_current_merchant_id.set(row.merchant_id.clone());
        let label = row
            .merchant_id
            .as_deref()
            .and_then(|id| {
                merchants_raw
                    .get_untracked()
                    .0
                    .iter()
                    .find(|m| m.id == id)
                    .map(|m| m.name.clone())
            })
            .unwrap_or_default();
        edit_current_merchant_label.set(label);
        edit_merchant_typed.set(
            // 软删商户仍可解析名称（历史引用照常显示）；解析不到（行缺席）
            // 退空文本。
            row.merchant_id
                .as_deref()
                .and_then(|id| {
                    merchants_raw
                        .get_untracked()
                        .0
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.name.clone())
                })
                .unwrap_or_default(),
        );
        edit_merchant_selected.set(row.merchant_id.clone());
        edit_has_policy.set(row.policy_id.is_some());
        edit_error.set(None);
        edit_target.set(Some(row.id.clone()));
    };

    let close_edit = move || {
        edit_target.set(None);
        edit_current_merchant_id.set(None);
        edit_current_merchant_label.set(String::new());
        edit_has_policy.set(false);
        edit_error.set(None);
    };

    // 编辑保存（仅非金额字段；金额不可编辑哨兵在域内显式拒绝）：校验 →
    // 商户语义（Keep/Selected/NewName 与新建同单点）→ 编辑写接缝 → 反馈 +
    // 清单与花费重拉。挂保单的缴费协议不发送商户（issue #713 / ADR-0082
    // 决策 2：提交空 = 保持置空态）。
    let perform_edit = move || {
        edit_error.set(None);
        status.set(None);
        let Some(plan_id) = edit_target.get_untracked() else {
            close_edit();
            return;
        };
        let Some(account_id) = edit_account.get_untracked() else {
            edit_error.set(Some(MSG_SELECT_ACCOUNT.to_owned()));
            return;
        };
        let note = edit_note.get_untracked().trim().to_owned();
        // 挂保单的行：商户语义固定不挂（原值置空态）；普通行：双态解析。
        let merchant = if edit_has_policy.get_untracked() {
            MerchantRefOf::None
        } else {
            merchant_ref_of(
                edit_current_merchant_id.get_untracked().as_deref(),
                edit_merchant_selected.get_untracked().as_deref(),
                &edit_merchant_typed.get_untracked(),
            )
        };
        let input = ledger_scheduled::UpdateSubscriptionInput {
            id: plan_id.clone(),
            account_id: account_id.clone(),
            category_id: edit_category.get_untracked(),
            note: (!note.is_empty()).then_some(note),
            // 商户在 blocking 批内按语义解析（与新建同法）。
            merchant_id: None,
            amount_cents: false,
            total_amount_cents: false,
        };
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        let resolved = match merchant.clone() {
                            MerchantRefOf::None => None,
                            MerchantRefOf::Keep(id) | MerchantRefOf::Selected(id) => Some(id),
                            MerchantRefOf::NewName(name) => {
                                Some(boot::create_merchant_by_name(db, &name)?)
                            }
                        };
                        boot::update_scheduled_subscription(
                            db,
                            ledger_scheduled::UpdateSubscriptionInput {
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
                    Ok(Ok(())) => {
                        close_edit();
                        status.set(Some(MSG_SAVED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => edit_error.set(Some(format!("保存失败: {e}"))),
                    Err(_) => edit_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 视图树 ============================
    // 列宿主映射（装配列 → nsh-base 列；富余宽全给备注列——旧壳备注列弹性
    // 同语义，账户表先例同形）。总宽不得超出表格实测可用宽（列总和超出视
    // 口时尾列缺席即行动作双入口灭失——#2041 账户列映射同条约束）。
    let columns = computed(move || sub_columns(adaptive::content_width()));

    // 取消确认的目标计划 id（按行匹配的气泡一次只开一个，预算视图先例同形）。
    let confirm_cancel = signal(None::<String>);

    // 行格自绘（行操作列按钮组 + 行锚点；其余列文本是装配产物直接呈现，
    // 下期扣款列带行级 test 锚点——旧壳 next-charge-{id} 同位）。
    let render_cell = make_action_renderer(
        move || assembled.get(),
        detail_open_id,
        confirm_cancel,
        open_edit,
        perform_status,
    );

    // 清单卡（标题 + 状态过滤按钮 + 新建订阅 + 表）。
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
                    text(SUB_LIST_TITLE)
                        .text_size(px(font_size::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                )
                .child(view().flex_1())
                .children(subscription_status_filters().into_iter().map(|(key, label)| {
                    let key_owned = key.to_owned();
                    button()
                        .app_class(
                            "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                        )
                        .child(
                            text(label)
                                .text_size(px(font_size::CAPTION))
                                .test_id(format!("sub-filter-{key_owned}").as_str()),
                        )
                        .on_press(move |_: &()| active_filter.set(key.to_owned()))
                }))
                .child(
                    button()
                        .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                        .child(
                            text("新建订阅")
                                .text_size(px(font_size::SECONDARY))
                                .text_color(primary_fg)
                                .test_id("sub-create-open"),
                        )
                        .on_press(move |_: &()| open_create()),
                ),
        )
        .child(
            view().test_id("sub-table").flex_1().child(
                data_table()
                    .columns(columns)
                    .data(move || {
                        // 追踪读（DataTable 的 data prop 是响应式闭包；get_untracked
                        // 不建追踪依赖，行集变更后表格不重绘）。渲染期零格式化：
                        // 文本与行操作可用性已是一次性装配产物。
                        let snapshot = assembled.get();
                        DataTableRows::new(snapshot.len())
                            .texts("note", snapshot.iter().map(|r| r.note.as_str()))
                            .texts("merchant", snapshot.iter().map(|r| r.merchant.as_str()))
                            .texts("category", snapshot.iter().map(|r| r.category.as_str()))
                            .texts("account", snapshot.iter().map(|r| r.account.as_str()))
                            .texts("amount", snapshot.iter().map(|r| r.amount.as_str()))
                            .texts(
                                "recurrence",
                                snapshot.iter().map(|r| r.recurrence.as_str()),
                            )
                            .texts(
                                "start_date",
                                snapshot.iter().map(|r| r.start_date.as_str()),
                            )
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

    // 新建订阅弹窗内容（旧壳布局对齐记一笔表单：字段行距 + 宽度档；NSH
    // 沿交易表单与账户表的字段行形态）。
    let create_form_fields = move || {
        view()
            .flex_col()
            .gap_2()
            .child(form_row("备注", muted, input()
                .test_id("sub-note")
                .value(move || create_note.get())
                .placeholder("服务名称，如：视频会员")
                .optional_label("订阅备注")
                .text_color(fg)
                .w(px(280.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| create_note.set(value.to_owned()))))
            .child(form_row("扣款账户", muted, pinyin_select::pinyin_select(
                "sub-account",
                move || account_options_of(&accounts_raw.get().0, None),
                create_account,
                "选择账户",
                false,
            )))
            .child(form_row("分类", muted, pinyin_select::pinyin_select(
                "sub-category",
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
                        "sub-merchant",
                        create_merchant_options,
                        create_merchant_typed,
                        create_merchant_selected,
                        "选择商户，可直接输入新名称",
                    )),
            )
            .child(form_row("金额", muted, view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(input()
                    .test_id("sub-amount")
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
                    // 币种随账户推导（只读展示，旧壳 disabled AppSelect 同位）。
                    text(move || create_currency_text.get())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(muted)
                        .test_id("sub-currency"),
                )))
            .child(form_row("重复", muted, view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(text("每").text_size(px(font_size::SECONDARY)).text_color(fg))
                .child(input()
                    .test_id("sub-interval")
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
                    "sub-recurrence-type",
                    recurrence_options,
                    create_recurrence,
                    "周期类型",
                    false,
                ))))
            .child(form_row("开始日", muted, date_select::date_select(
                "sub-start", create_start_date,
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
                                .test_id("sub-create-error")
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
                                    .test_id("sub-create-cancel"),
                            )
                            .on_press(move |_: &()| close_create()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                            .child(
                                text(CREATE_SUB_SUBMIT_LABEL)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(primary_fg)
                                    .test_id("sub-create"),
                            )
                            .on_press(move |_: &()| perform_create()),
                    ),
            )
    };

    // 编辑订阅弹窗内容（仅非金额字段；挂保单的缴费协议不显示商户字段）。
    let edit_form_fields = move || {
        view()
            .flex_col()
            .gap_2()
            .child(form_row("备注", muted, input()
                .test_id("sub-edit-note")
                .value(move || edit_note.get())
                .placeholder("服务名称")
                .optional_label("订阅备注")
                .text_color(fg)
                .w(px(280.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| edit_note.set(value.to_owned()))))
            .child(form_row("扣款账户", muted, pinyin_select::pinyin_select(
                "sub-edit-account",
                move || {
                    // 编辑实例的账户下拉取这份草稿的当前值补值（spec #1952 /
                    // ADR-0142 决策 4：归档账户补回选项面）。
                    account_options_of(
                        &accounts_raw.get().0,
                        edit_account.get_untracked().as_deref(),
                    )
                },
                edit_account,
                "选择账户",
                false,
            )))
            .child(form_row("分类", muted, pinyin_select::pinyin_select(
                "sub-edit-category",
                move || crate::assembly::expense_category_options(&category_rows.get().0),
                edit_category,
                "选择分类",
                true,
            )))
            .child(when(
                // 挂保单的缴费协议不显示商户（issue #713 / ADR-0082 决策 2）：
                // 付款对象语义由保单的保司承担，后端对非空提交显式拒绝。
                move || !edit_has_policy.get(),
                move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label("商户", muted))
                        .child(combo_select::combo_select(
                            "sub-edit-merchant",
                            edit_merchant_options,
                            edit_merchant_typed,
                            edit_merchant_selected,
                            "选择商户，可直接输入新名称",
                        ))
                },
            ).otherwise(view))
            .child(
                view()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .child(field_label("", muted))
                    .child(when(
                        move || edit_error.get().is_some(),
                        move || {
                            text(move || format!("保存失败: {}", edit_error.get().unwrap_or_default()))
                                .text_size(px(font_size::CAPTION))
                                .text_color(danger_fg)
                                .test_id("sub-edit-error")
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
                                    .test_id("sub-edit-cancel"),
                            )
                            .on_press(move |_: &()| close_edit()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-2 rounded-md bg-primary hover:opacity-90")
                            .child(
                                text("保存")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(primary_fg)
                                    .test_id("sub-edit-save"),
                            )
                            .on_press(move |_: &()| perform_edit()),
                    ),
            )
    };

    view()
        .flex_1()
        .flex_col()
        .gap_3()
        .app_class("bg-background")
        .child(
            // 装载失败条位（码化错误 zh 原样，不静默——清单口径缺失不回退）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败: {}", error.get().unwrap_or_default()))
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("sub-error")
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
                        .test_id("sub-status")
                },
            )
            .otherwise(view),
        )
        .child(spend_panel(spending))
        .child(
            // 新建订阅弹窗。
            modal::app_modal(
                move || create_open.get(),
                || CREATE_SUB_TITLE.to_owned(),
                close_create,
                create_form_fields,
            ),
        )
        .child(
            // 编辑订阅弹窗（仅非金额字段）。
            modal::app_modal(
                move || edit_target.get().is_some(),
                || EDIT_SUB_TITLE.to_owned(),
                close_edit,
                edit_form_fields,
            ),
        )
        .child(
            // 期次详情弹层（三页签通用；重试 / 展开成功回调重拉与失效广播）。
            crate::scheduled_plan_detail::plan_detail_modal(
                detail_open_id,
                currencies,
                privacy,
                reload_after_change,
            ),
        )
}

// ---------------------------------------------------------------------------
// 模块级 helper（装配证据的闭包支撑面 + 选项投影纯函数）。
// ---------------------------------------------------------------------------

/// 表单字段标签（字阶 SECONDARY 档 + muted 弱化——次要信息不靠缩字号，
/// 交易表单同行形态）。空标签 = 占位空（错误条位
/// 不配标签——与行位对齐）。
pub(crate) fn field_label(
    label: &'static str,
    muted: nsh_core::element::AdaptiveColor,
) -> nsh_core::element::AnyElement {
    if label.is_empty() {
        view().w(px(64.0)).into_element()
    } else {
        text(label)
            .text_size(px(font_size::SECONDARY))
            .text_color(muted)
            .w(px(64.0))
            .text_right()
            .into_element()
    }
}

/// 表单行（标签 + 控件；标签宽固定，控件端对齐——交易表单先行例同形）。
pub(crate) fn form_row(
    label: &'static str,
    muted: nsh_core::element::AdaptiveColor,
    control: impl IntoElement,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label(label, muted))
        .child(control)
}

/// 行操作小按钮（动作文案 + 行级锚点；按压回调归调用方动作单点）。
pub(crate) fn action_button(
    label: &'static str,
    anchor: String,
    on_press: impl Fn() + Clone + 'static,
    fg: nsh_core::element::AdaptiveColor,
) -> nsh_core::element::AnyElement {
    button()
        .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent")
        .child(
            text(label)
                .text_size(px(font_size::CAPTION))
                .text_color(fg)
                .test_id(anchor.as_str()),
        )
        .on_press(move |_: &()| on_press())
        .into_element()
}

/// 行格自绘器（清单表「下期扣款」列带行级锚点 + 操作列按钮组，旧壳
/// PlanRowActions 的重生：期次恒可用、暂停限 active、恢复限 paused、取消限
/// 「未结束」带确认气泡（目标一次只开一个，预算视图先例同形）、编辑限
/// 「未结束」、全不可用时零按钮；动作锚点 = `sub-{key}-{rowId}`，旧壳
/// `op-${key}-${rowId}` 同位）。拆行操作任一接线（期次目标 / 编辑回填 / 状态写路径 / 气泡确认）即红
/// （headless 「删除即红」条目的实拆点——逐项已实拆验证）。
fn make_action_renderer(
    assembled: impl Fn() -> Vec<SubRowCells> + Copy + 'static,
    detail_open_id: Signal<Option<String>>,
    confirm_cancel: Signal<Option<String>>,
    open_edit: impl Fn(String) + Clone + 'static,
    perform_status: impl Fn(String, ledger_scheduled::ScheduledStatus) + Clone + 'static,
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
                .test_id(format!("sub-next-{}", row.id).as_str())
                .into_element(),
            "actions" => {
                let fg = crate::theme::color("foreground");
                let danger_fg = crate::theme::color("destructive");
                let id = row.id.clone();
                let mut buttons: Vec<nsh_core::element::AnyElement> = vec![action_button(
                    "期次",
                    format!("sub-detail-{id}"),
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
                        format!("sub-pause-{id}"),
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
                        format!("sub-resume-{id}"),
                        {
                            let id = id.clone();
                            let perform = perform_status.clone();
                            move || perform(id.clone(), ScheduledStatus::Active)
                        },
                        fg,
                    ));
                }
                if row.actions.can_cancel {
                    // 轻量确认气泡（旧壳 AppPopconfirm 消费同位，确认/取消按钮
                    // 文案同 NPopconfirm 缺省）：目标计划 id 按行回写，开合与确
                    // 认归封装；确认即取消（不升危险确认——已生成交易与历史期
                    // 次保留，旧壳 confirm 文案如实）。行 id 克隆进各闭包。
                    let popconfirm_id = id.clone();
                    buttons.push(
                        popconfirm::popconfirm(
                            "sub-cancel",
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
                                message: CANCEL_CONFIRM_TEXT.to_owned(),
                                confirm_label: "确定".to_owned(),
                                cancel_label: "取消".to_owned(),
                            },
                            // 触发件用纯视图（nsh-base Button 自含按压语义会
                            // 否掉外层按压——封装的按压源是外层包裹视图，预算
                            // 视图先例同形）；锚照旧落行内。触发件文色用危险性
                            // 面（旧壳 type=error quaternary 同位）。
                            view()
                                .app_class("px-2 py-1 rounded-md text-destructive hover:bg-accent")
                                .child(
                                    text("取消")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id(format!("sub-cancel-{id}").as_str()),
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
                if row.actions.can_edit {
                    buttons.push(action_button(
                        "编辑",
                        format!("sub-edit-{id}"),
                        {
                            let id = id.clone();
                            let open = open_edit.clone();
                            move || open(id.clone())
                        },
                        fg,
                    ));
                }
                view().flex_row().gap_1().children(buttons).into_element()
            }
            _ => text(cell.text.clone()).number_of_lines(1).into_element(),
        }
    }
}

/// 列宿主映射（[`SUB_COLUMNS`] → nsh-base 列；账户列映射同形——富余宽全给备注列，旧壳备注列弹性同语义）。
fn sub_columns(width: f32) -> Vec<DataTableColumn> {
    // 表格外围定宽扣减（内容列 p-3 + 分区卡 p-3 + 描边 2pt，账户表先例同）。
    const SUB_TABLE_WIDTH_PAD: f32 = 12.0 * 2.0 + 12.0 * 2.0 + 2.0;
    let mut columns: Vec<DataTableColumn> = SUB_COLUMNS
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
    let suggested: f32 = SUB_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - SUB_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(note) = columns.iter_mut().find(|column| column.key == "note")
    {
        note.width = note.width.map(|w| Points(w.0 + extra));
    }
    columns
}

// ---------------------------------------------------------------------------
// 选项投影纯函数（商户 / 编辑兜底；lib 单测面——「输入即建 + 软删兜底」
// 选项口径的离线双面）。
// ---------------------------------------------------------------------------

/// 商户下拉选项（在用商户；旧壳 merchantOptions 同口径——软删不进新选择，
/// 历史计划行的商户名走含软删全量表的解析口径）。纯投影。
pub(crate) fn merchant_options_active(rows: &[ledger_merchants::Merchant]) -> Vec<CategoryOption> {
    rows.iter()
        .filter(|m| !m.is_deleted)
        .map(|m| CategoryOption {
            id: m.id.clone(),
            label: m.name.clone(),
        })
        .collect()
}

/// 编辑商户下拉选项（旧壳 editMerchantOptions 同口径）：基础 = 在用商户
/// + 原商户软删时追加兜底选项（label「（已删除商户）」，value 原商户 id
///   ——提交时按「未改动」语义原样保留）。纯投影。
pub(crate) fn edit_merchant_options_of(
    rows: &[ledger_merchants::Merchant],
    incoming_merchant: Option<&str>,
    incoming_label: &str,
) -> Vec<CategoryOption> {
    let mut base = merchant_options_active(rows);
    if let Some(id) = incoming_merchant
        && let Some(row) = rows.iter().find(|m| m.id == id)
        && row.is_deleted
    {
        base.push(CategoryOption {
            id: id.to_owned(),
            label: if incoming_label.is_empty() {
                "（已删除商户）".to_owned()
            } else {
                format!("{incoming_label}（已删除商户）")
            },
        });
    }
    base
}

// ---------------------------------------------------------------------------
// lib 单测（商户双态值语义 / 账户选项 / 币种推导 / 编辑兜底）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(id: &str, name: &str, deleted: bool) -> ledger_merchants::Merchant {
        ledger_merchants::Merchant {
            id: id.to_owned(),
            name: name.to_owned(),
            is_deleted: deleted,
            created_at: String::new(),
            updated_at: String::new(),
            version: 0,
            device_id: String::new(),
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

    #[test]
    fn merchant_ref_of_follows_old_shell_five_branches() {
        assert_eq!(merchant_ref_of(None, None, ""), MerchantRefOf::None);
        assert_eq!(
            merchant_ref_of(None, Some("m-1"), "新名"),
            MerchantRefOf::Selected("m-1".to_owned()),
            "选定建议项优先于文本"
        );
        assert_eq!(
            merchant_ref_of(None, None, "红旗超市"),
            MerchantRefOf::NewName("红旗超市".to_owned()),
            "自由文本即建"
        );
        assert_eq!(
            merchant_ref_of(Some("m-1"), Some("m-1"), ""),
            MerchantRefOf::Keep("m-1".to_owned()),
            "编辑页未改选 = 保持历史引用（软删兜底同款）"
        );
        assert_eq!(
            merchant_ref_of(Some("m-1"), Some("m-2"), ""),
            MerchantRefOf::Selected("m-2".to_owned()),
            "改选在用商户"
        );
        assert_eq!(
            merchant_ref_of(Some("m-1"), None, "新名"),
            MerchantRefOf::NewName("新名".to_owned()),
            "编辑页退回自由文本"
        );
        assert_eq!(
            merchant_ref_of(None, None, "  "),
            MerchantRefOf::None,
            "空白文本 = 未挂"
        );
    }

    #[test]
    fn account_options_exclude_archived_unless_pinned() {
        let accounts = vec![
            acc("a-1", "现金", "CNY", false),
            acc("a-2", "旧卡", "CNY", true),
        ];
        let identifiers = account_options_of(&accounts, None);
        assert_eq!(identifiers.len(), 1, "归档不进新选择");
        assert_eq!(
            account_options_of(&accounts, Some("a-2")).len(),
            2,
            "编辑面补值把归档账户补回选项面"
        );
    }

    #[test]
    fn currency_is_derived_from_selected_account() {
        let accounts = vec![acc("a-1", "现金", "USD", false)];
        assert_eq!(
            derive_currency_text(Some("a-1"), &accounts),
            "USD",
            "币种随账户推导（ADR-0134 决策 5）"
        );
        assert_eq!(
            derive_currency_text(Some("a-x"), &accounts),
            "",
            "未命中账户（软删隐藏口径外）不兜币种偏好"
        );
        assert_eq!(derive_currency_text(None, &accounts), "");
    }

    #[test]
    fn merchant_options_active_exclude_deleted_rows() {
        let rows = vec![ms("m-1", "视频平台", false), ms("m-2", "软删平台", true)];
        let options = merchant_options_active(&rows);
        assert_eq!(options.len(), 1, "软删不进新选择");
        assert_eq!(options[0].label, "视频平台");
    }

    #[test]
    fn edit_options_carry_deleted_merchant_fallback() {
        let rows = vec![ms("m-1", "视频平台", false), ms("m-2", "软删平台", true)];
        let options = edit_merchant_options_of(&rows, Some("m-2"), "软删平台");
        assert_eq!(options.len(), 2, "在用商户 + 软删原商户兜底选项");
        assert_eq!(options.last().unwrap().id, "m-2");
        assert!(
            options.last().unwrap().label.contains("（已删除商户）"),
            "兜底选项承载原商户名（裸 uuid 不可读）"
        );
        // 在用原商户不出兜底选项（选项面已含）。
        let options = edit_merchant_options_of(&rows, Some("m-1"), "视频平台");
        assert_eq!(options.len(), 1, "兜底选项只针对软删原商户");
    }
}
