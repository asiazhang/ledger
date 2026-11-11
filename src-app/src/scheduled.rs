//! 定时视图（issue #2042 NSH 迁移⑪ + issue #2043 定时②；spec #2031 系列
//! 11/26 + 12/26；旧壳 `ScheduledView.vue` 三形态页签容器的重生）。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：
//! - **页签骨架**：订阅 / 分期 / 定时转账三形态页签（旧壳 `ScheduledFormTab`
//!   同词表），三页签均挂真实组件（订阅 = [#2042]；分期 / 定时转账 = 定时②
//!   #2043 迁入）。页签状态 = 会话槽（进程级信号单例，会话内保留——离开
//!   定时视图再回原页签不丢；不跨启动持久化、零写盘，旧壳 query.tab 的进程
//!   内等价物；[`nav::NavRequest::form_tab`] 意图落位时的消费面）；
//! - **计划来源落点**：落位意图读一次消费（[`crate::nav::take_intent`] 单点
//!   封闸）——形态页签写会话槽、focus（计划 id）暂存给形态页签（页签挂载时
//!   开计划详情弹窗、开即回报清闸，弹窗按 id 独立取数不受清单状态过滤
//!   影响——已取消计划照常可开，旧壳 usePlanFocusLanding 时序的等价收口）；
//!   当前激活页签消费 focus（默认订阅页签；页签切换后由新挂载的页签消费）。
//! - **写路径**：直调域公开写入口（[`crate::boot`]，不经 IPC，旧壳 scheduled
//!   族命令同一执行协议）；期次执行落交易行后由视图层手动广播交易失效信号
//!   （交易列表重读）；清单与花费面板的重拉归动作回调（旧壳「计划写入刻意
//!   零信号、消费面按回调刷新」同口径）。
//!
//! 兄弟模块：[`crate::scheduled_subscription`]（订阅页签本体）、
//! [`crate::scheduled_installment`]（分期页签本体，定时②）、
//! [`crate::scheduled_transfer`]（定时转账页签本体，定时②）、
//! [`crate::scheduled_plan_detail`]（期次详情弹层，三页签通用）、
//! [`crate::scheduled_spend`]（订阅花费面板）。
//!
//! 界面文案仅 zh-CN（spec #2015 实施决策）；码化错误 zh 原样呈现。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::Mutex;
use std::sync::MutexGuard;

use nsh_core::prelude::*;

use crate::assembly::{self, CategoryOption, ColumnAlign, CurrencyTable};
use crate::design::font_size;
use crate::nav;
use crate::nav_model::ScheduledFormTab;

// ---------------------------------------------------------------------------
// 词表（旧壳 packages/i18n zh-CN scheduled.json 逐条平价；i18n 框架级方案不在
// 本票，字面承载）。
// ---------------------------------------------------------------------------
pub(crate) const SUB_LIST_TITLE: &str = "订阅清单";
pub(crate) const CREATE_SUB_TITLE: &str = "新建订阅";
pub(crate) const CREATE_SUB_SUBMIT_LABEL: &str = "创建订阅";
pub(crate) const EDIT_SUB_TITLE: &str = "编辑订阅";
pub(crate) const CANCEL_CONFIRM_TEXT: &str =
    "取消后不再扣款，已生成的交易与历史期次保留。确认取消？";
pub(crate) const MSG_CREATED: &str = "已创建订阅";
pub(crate) const MSG_SAVED: &str = "已保存";
pub(crate) const MSG_PAUSED: &str = "已暂停";
pub(crate) const MSG_RESUMED: &str = "已恢复";
pub(crate) const MSG_CANCELLED: &str = "已取消";
pub(crate) const MSG_SELECT_ACCOUNT: &str = "请选择扣款账户";
pub(crate) const MSG_AMOUNT_POSITIVE: &str = "请输入大于 0 的金额";
/// 周期间隔输入的下限（旧壳 NInputNumber min 1 的提交通道兜底文案：文本输入
/// 在本壳的明文案对应形态）。
pub(crate) const MSG_INTERVAL_MIN: &str = "周期间隔至少为 1";
/// 下一期次无 pending 占位 / 详情失败文案（旧壳 next 占位「—」与
/// detailFailed「加载失败」，旧壳两词并存同形）。
pub(crate) const NEXT_PLACEHOLDER: &str = "—";
pub(crate) const DETAIL_FAILED_TEXT: &str = "加载失败";

// ---------------------------------------------------------------------------
// 形态页签会话槽（旧壳 query.tab 的进程内等价物）：页签不跨启动持久化
//（旧壳 URL query 会话内有效同口径）；headless 多场景串行复位走
// [`reset_for_tests`]（生产 bin 一次初始化不复位）。
// ---------------------------------------------------------------------------
struct ScheduledSession {
    /// 形态页签（None = 默认订阅页签；消费面 = [`ScheduledView`] 的意图叠加
    /// 与页签点击写）。
    form_tab: Signal<Option<ScheduledFormTab>>,
}

impl Clone for ScheduledSession {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for ScheduledSession {}

static SESSION: Mutex<Option<ScheduledSession>> = Mutex::new(None);

fn session_slot() -> MutexGuard<'static, Option<ScheduledSession>> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// 会话槽（惰性初始化：首次消费创建，信号挂首次调用的组件实例——生产路径
/// `ScheduledView` 组件体最先调用）。
fn scheduled_session() -> ScheduledSession {
    let mut slot = session_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = ScheduledSession {
        form_tab: signal(None),
    };
    *slot = Some(s);
    s
}

/// 复位会话槽（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）。
#[doc(hidden)]
pub fn reset_for_tests() {
    *session_slot() = None;
}

// ---------------------------------------------------------------------------
// 词表纯函数（旧壳 utils/scheduled.ts 与 scheduled-plan-list 的重生：状态 /
// 期次状态 / 周期标签与未结束判据全仓单源口径；旧壳的 t() 现取语义在本壳 =
// 字面闭集）。
// ---------------------------------------------------------------------------

/// 计划状态 → 状态标签（旧壳 `scheduledStatusLabel` 同口径；未知值原样
/// 透传——旧壳 default 分支同语义，不静默改名）。
pub(crate) fn status_label(status: &str) -> String {
    match status {
        "active" => "进行中".to_owned(),
        "paused" => "已暂停".to_owned(),
        "cancelled" => "已取消".to_owned(),
        "completed" => "已完成".to_owned(),
        other => other.to_owned(),
    }
}

/// 期次状态 → 状态标签（旧壳 `occurrenceStatusLabel` 同口径；闭集见
/// scheduled 域 `OccurrenceStatus` 枚举）。
pub(crate) fn occurrence_status_label(status: &str) -> String {
    match status {
        "pending" => "待执行".to_owned(),
        "processing" => "执行中".to_owned(),
        "completed" => "已完成".to_owned(),
        "failed" => "失败".to_owned(),
        "cancelled" => "已取消".to_owned(),
        other => other.to_owned(),
    }
}

/// 计划是否「未结束」（旧壳 `isUnfinishedStatus` = active | paused 的唯一
/// 出处：行操作矩阵——取消 / 编辑限本集合）；集合只此一份字面，账户归档的
/// 未结束计划数提示在账户装配层另计（语义同源）。
pub(crate) fn is_unfinished_status(status: &str) -> bool {
    status == "active" || status == "paused"
}

/// 周期选项闭集（旧壳 `scheduledRecurrenceOptions`：每天 / 每周 / 每月 / 每年，
/// #309 后三页签统一词表的闭集，表单下拉共用一张表）。
pub(crate) fn recurrence_options() -> Vec<CategoryOption> {
    [
        ("daily", "每天"),
        ("weekly", "每周"),
        ("monthly", "每月"),
        ("yearly", "每年"),
    ]
    .into_iter()
    .map(|(id, label)| CategoryOption {
        id: id.to_owned(),
        label: label.to_owned(),
    })
    .collect()
}

/// 周期标签（旧壳 `scheduledRecurrenceLabel`：interval = 1 →「每{unit}」、
/// 否则「每{interval}{unit}」；未知类型兜底原值——旧壳同语义）。
pub(crate) fn recurrence_label(recurrence_type: &str, interval: i64) -> String {
    let unit = match recurrence_type {
        "daily" => "天",
        "weekly" => "周",
        "monthly" => "月",
        "yearly" => "年",
        other => return other.to_owned(),
    };
    if interval > 1 {
        format!("每{interval}{unit}")
    } else {
        format!("每{unit}")
    }
}

/// 订阅页签状态过滤选项（旧壳 `STATUS_FILTER_KEYS.subscription` = 无「已完成」
/// 键集：订阅维持 #309 时期现状不越权补齐；标签经 [`status_label`] 单源）。
pub(crate) fn subscription_status_filters() -> Vec<(&'static str, String)> {
    [
        ("active", "进行中"),
        ("paused", "已暂停"),
        ("cancelled", "已取消"),
    ]
    .into_iter()
    .map(|(key, _)| (key, status_label(key)))
    .collect()
}

/// 定时转账 / 分期页签状态过滤选项（旧壳 `STATUS_FILTER_KEYS.scheduled_transfer`
/// / `installment` = WITH_COMPLETED 键集：定时转账一次性执行后 completed、
/// 分期按 Plan Lifecycle 自然完成计划恢复可见可查——#309 显式可见变化之二。
/// 标签经 [`status_label`] 单源；订阅无「已完成」键集走
/// [`subscription_status_filters`]，不用本集越权补齐）。
pub(crate) fn plan_status_filters() -> Vec<(&'static str, String)> {
    [
        ("active", "进行中"),
        ("paused", "已暂停"),
        ("cancelled", "已取消"),
        ("completed", "已完成"),
    ]
    .into_iter()
    .map(|(key, _)| (key, status_label(key)))
    .collect()
}

/// 金额展示（旧壳 `formatAmount` 的隐私口径：privacy 开启时恒形掩码、隐藏
/// 数字不隐藏方向），由 [`assembly::format_amount_basic`] 承载分组与去零。
pub(crate) fn amount_text(
    cents: i64,
    fmt: &assembly::CurrencyFormat,
    privacy_enabled: bool,
) -> String {
    if privacy_enabled {
        assembly::AMOUNT_PRIVACY_MASK.to_owned()
    } else {
        assembly::format_amount_basic(cents, fmt)
    }
}

// ---------------------------------------------------------------------------
// 行集快照与装配（清单格文本的纯 Rust 单测面：渲染期零格式化纪律——列表
// 行格文本在此一次性产出）。
// ---------------------------------------------------------------------------

/// 订阅计划行可克隆快照（视图局部）：域读路径 `boot::plan_rows`（软删滤除在
/// 域内）按 kind 过滤订阅计划 + 逐行详情读出的最早 pending 期次投影——清单
/// 表单消费的字段全集在装载批一次性投影持有。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubRowSnap {
    pub id: String,
    pub note: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub account_id: String,
    pub category_id: Option<String>,
    pub amount_cents: i64,
    pub currency_code: String,
    pub recurrence_type: String,
    pub recurrence_interval: i64,
    pub start_date: String,
    pub status: String,
    /// 详情命令失败：与「无数据」区分，不静默（下期扣款占位「加载失败」，
    /// 旧壳 detailFailed 同口径）。
    pub detail_failed: bool,
    /// 最早 pending 期次（日期, 金额分）；无 pending（预生成窗口之外）= None。
    pub next: Option<(String, i64)>,
}

/// 行集投影 [`SubRowSnap`] 的集合容器（装载批一次性投影，账户块先例同形）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SubRowsSnap(pub Vec<SubRowSnap>);

/// 行操作可用性（旧壳 scheduled-plan-list `rowActions` 按状态矩阵的行级
/// 投影）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SubRowActions {
    pub can_pause: bool,
    pub can_resume: bool,
    /// 取消 / 编辑共享「未结束」集合（isUnfinishedStatus 同出处）。
    pub can_cancel: bool,
    pub can_edit: bool,
}

/// 列表行格单元格（渲染期零格式化：文本已是一次性装配产物）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubRowCells {
    pub id: String,
    pub note: String,
    pub merchant: String,
    pub category: String,
    pub account: String,
    pub amount: String,
    pub recurrence: String,
    pub start_date: String,
    pub status: String,
    pub next: String,
    pub actions: SubRowActions,
}

/// 清单装配上下文（参考数据表的只读投影 + 隐私快照）。
pub(crate) struct SubListContext<'a> {
    pub currencies: &'a CurrencyTable,
    pub categories: &'a assembly::CategoryTable,
    pub accounts: &'a assembly::AccountTable,
    pub merchants: &'a assembly::MerchantTable,
    pub privacy_enabled: bool,
}

/// 列定义（旧壳桌面档十列一字不动：备注/商户/分类/扣款账户/金额/周期/开始日/
/// 状态/下期扣款/操作；金额右对齐。宽度建议值与账户表先例同量级，富余宽给
/// 备注列由宿主映射分配）。
pub(crate) const SUB_COLUMNS: &[assembly::ColumnDef] = &[
    assembly::ColumnDef {
        key: "note",
        title: "备注",
        width: 150.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "merchant",
        title: "商户",
        width: 100.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "category",
        title: "分类",
        width: 110.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "account",
        title: "扣款账户",
        width: 95.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "amount",
        title: "金额",
        width: 80.0,
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
        title: "下期扣款",
        width: 140.0,
        align: ColumnAlign::Start,
    },
    assembly::ColumnDef {
        key: "actions",
        title: "操作",
        width: 150.0,
        align: ColumnAlign::Start,
    },
];

/// 订阅计划行 → 列表格文本（旧壳桌面档十列 render 的装配层收口）：
/// 商户名解析消费含软删全量商户表（改名即时生效，软删商户历史计划显示原名
/// ——旧壳 merchantMap 同口径）；扣款账户无匹配回退原始 id（旧壳
/// accountName 同口径）；分类路径解析失败 / 无分类呈现「—」。
pub(crate) fn assemble_sub_rows(rows: &[SubRowSnap], ctx: &SubListContext<'_>) -> Vec<SubRowCells> {
    rows.iter()
        .map(|row| {
            let fmt = ctx.currencies.format_of(&row.currency_code);
            let merchant = row
                .merchant_id
                .as_deref()
                .and_then(|id| ctx.merchants.name_of(Some(id)))
                .unwrap_or(NEXT_PLACEHOLDER)
                .to_owned();
            let category = ctx
                .categories
                .path_of(row.category_id.as_deref())
                .unwrap_or_else(|| NEXT_PLACEHOLDER.to_owned());
            let account = ctx
                .accounts
                .name_of(Some(row.account_id.as_str()))
                .unwrap_or(row.account_id.as_str())
                .to_owned();
            let amount = amount_text(row.amount_cents, &fmt, ctx.privacy_enabled);
            let next = match (&row.next, row.detail_failed) {
                (_, true) => DETAIL_FAILED_TEXT.to_owned(),
                (Some((date, cents)), _) => {
                    format!(
                        "{date} · {}",
                        amount_text(*cents, &fmt, ctx.privacy_enabled)
                    )
                }
                (None, _) => NEXT_PLACEHOLDER.to_owned(),
            };
            SubRowCells {
                id: row.id.clone(),
                note: row.note.clone().unwrap_or_default(),
                merchant,
                category,
                account,
                amount,
                recurrence: recurrence_label(&row.recurrence_type, row.recurrence_interval),
                start_date: row.start_date.clone(),
                status: status_label(&row.status),
                next,
                actions: row_actions_of(&row.status),
            }
        })
        .collect()
}

/// 行操作可用性投影（旧壳 `rowActions` 状态矩阵的行级闭包面）：期次恒可用、
/// 暂停限 active、恢复限 paused、取消 / 编辑限「未结束」。挂保单的订阅在行
/// 操作集上无特化（商户字段在编辑弹窗隐藏，issue #713 / ADR-0082 决策 2）。
pub(crate) fn row_actions_of(status: &str) -> SubRowActions {
    SubRowActions {
        can_pause: status == "active",
        can_resume: status == "paused",
        can_cancel: is_unfinished_status(status),
        can_edit: is_unfinished_status(status),
    }
}

/// 定时转账 / 分期页签行操作投影（旧壳 scheduled-plan-list `rowActions` 的
/// 生命周期闭集：期次恒可用、暂停限 active、恢复限 paused、取消限「未结束」
/// ——**编辑是订阅适配器自建描述符**（ADR-0041 决策 7），清单模块闭集不
/// 含编辑，故 can_edit 恒 false）。
pub(crate) fn lifecycle_actions_of(status: &str) -> SubRowActions {
    SubRowActions {
        can_edit: false,
        ..row_actions_of(status)
    }
}

// ---------------------------------------------------------------------------
// 视图组件
// ---------------------------------------------------------------------------

/// 定时视图（导航 / 容器的定时落位本体）：三形态页签 + 会话槽 + 意图消费 +
/// 三页签均挂真实组件（订阅 = ⑪，分期 / 定时转账 = ⑫ 定时②迁入）。
#[component]
pub(crate) fn ScheduledView() -> impl IntoElement {
    let session = scheduled_session();
    // 待开的计划 id（来源落点的 focus 暂存；订阅页签挂载时消费开窗并回报
    // 清闸——见 [scheduled_subscription::subscriptions_pane]）。
    let pending_focus: Signal<Option<String>> = signal(None);

    // 落位意图消费（读一次封闸归 [nav::take_intent]；账户视图同形的落位键）。
    // 意图三分：交易筛选载荷不在本视图语义（读一次丢弃，防误投后续落位）；
    // 形态页签叠加写会话槽（页签会话内保留）；focus = 待开的计划 id（暂存给
    // 形态页签：订阅页签消费开计划详情弹窗，旧壳 usePlanFocusLanding 时序的
    // 视图侧收口）。
    effect(move || {
        let _landed = nav::store().current.get();
        let intent = nav::take_intent();
        if let Some(tab) = intent.as_ref().and_then(|i| i.form_tab) {
            session.form_tab.set(Some(tab));
        }
        pending_focus.set(intent.and_then(|i| i.focus));
    });

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");

    // 形态页签（三态闭集；会话槽缺省首签订阅）。页签点击写会话槽（会话内
    // 保留）；nsh-base Tabs 行为件（roving 焦点 + 无障碍页签列表，容器页签
    // 壳先例同形）。
    let active = computed(move || {
        session
            .form_tab
            .get()
            .unwrap_or(ScheduledFormTab::Subscriptions)
    });
    let tab_bar = {
        let tabs = [
            ScheduledFormTab::Subscriptions,
            ScheduledFormTab::Installments,
            ScheduledFormTab::Transfers,
        ]
        .map(|tab| {
            let id = format!("scheduled-tab-{}", tab.as_str());
            let (id, tab) = (id, tab);
            nsh_base::tab()
                .selected(move || active.get() == tab)
                .accessibility_label(form_tab_label(tab))
                .app_class("px-2 py-2 rounded-md selected:bg-accent")
                .child(
                    text(form_tab_label(tab))
                        .text_size(px(font_size::SECONDARY))
                        .text_color(move || if active.get() == tab { fg } else { muted })
                        .test_id(id.as_str()),
                )
                .on_press(move |_: &()| {
                    if active.get_untracked() != tab {
                        session.form_tab.set(Some(tab));
                    }
                })
        });
        view()
            .px_4()
            .py_2()
            .app_class("border-b border-border")
            .child(
                nsh_base::tabs()
                    .indicator_class("bg-accent")
                    .child(view().flex_row().gap_2().child(tabs)),
            )
    };

    // 页签内容：三页签均挂真实组件（分期 / 定时转账由定时② #2043 迁入；
    // 页签形态切换 = 分支重建 → 真实视图重挂全新装载，旧壳 NTabPane 缺省
    // display-directive「if」同口径）。
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(tab_bar)
        .child(
            // 页签的 pending_focus 入参：无 #[component] 的普通函数形态
            //（视图组件带 props 的装配面归普通 fn，子组件生计由调用方持有）。
            when(
                move || active.get() == ScheduledFormTab::Subscriptions,
                move || crate::scheduled_subscription::subscriptions_pane(pending_focus),
            )
            .otherwise(move || {
                when(
                    move || active.get() == ScheduledFormTab::Installments,
                    move || crate::scheduled_installment::installments_pane(pending_focus),
                )
                .otherwise(move || crate::scheduled_transfer::transfers_pane(pending_focus))
            }),
        )
}

/// 形态页签 zh-CN 词表（旧壳 `scheduled.tab.*` 同文案）。
fn form_tab_label(tab: ScheduledFormTab) -> &'static str {
    match tab {
        ScheduledFormTab::Subscriptions => "订阅",
        ScheduledFormTab::Installments => "分期",
        ScheduledFormTab::Transfers => "定时转账",
    }
}

// ---------------------------------------------------------------------------
// lib 单测（词表纯函数与装配口径：渲染期零格式化的兜底面）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(status: &str, amount_cents: i64) -> SubRowSnap {
        SubRowSnap {
            id: "p-1".to_owned(),
            note: Some("视频会员".to_owned()),
            merchant_id: Some("m-1".to_owned()),
            policy_id: None,
            account_id: "a-1".to_owned(),
            category_id: Some("c-1".to_owned()),
            amount_cents,
            currency_code: "CNY".to_owned(),
            recurrence_type: "monthly".to_owned(),
            recurrence_interval: 1,
            start_date: "2026-01-15".to_owned(),
            status: status.to_owned(),
            detail_failed: false,
            next: Some(("2026-02-15".to_owned(), 300)),
        }
    }

    fn empty_ctx(privacy: bool) -> SubListContext<'static> {
        // 币种字典经费表格缺省覆盖 CNY（计划金额的币符与两位小数位都不
        // 是代账本字典的缺省面，测试闭集自给）。
        let currency = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        SubListContext {
            currencies: Box::leak(Box::new(CurrencyTable::from_currencies([&currency]))),
            categories: Box::leak(Box::new(assembly::CategoryTable::default())),
            accounts: Box::leak(Box::new(crate::assembly::AccountTable::default())),
            merchants: Box::leak(Box::new(crate::assembly::MerchantTable::default())),
            privacy_enabled: privacy,
        }
    }

    #[test]
    fn recurrence_options_follow_old_shell_labels() {
        let options = recurrence_options();
        let labels = options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>();
        assert_eq!(labels, vec!["每天", "每周", "每月", "每年"]);
        assert_eq!(options[2].id, "monthly");
    }

    #[test]
    fn recurrence_label_matches_old_shell_forms() {
        assert_eq!(recurrence_label("monthly", 1), "每月");
        assert_eq!(recurrence_label("monthly", 3), "每3月");
        assert_eq!(recurrence_label("yearly", 1), "每年");
        assert_eq!(recurrence_label("weekly", 2), "每2周");
        assert_eq!(
            recurrence_label("mystery", 1),
            "mystery",
            "未知类型兜底原值"
        );
    }

    #[test]
    fn status_and_occurrence_labels_match_old_shell() {
        assert_eq!(status_label("active"), "进行中");
        assert_eq!(status_label("paused"), "已暂停");
        assert_eq!(status_label("cancelled"), "已取消");
        assert_eq!(status_label("completed"), "已完成");
        assert_eq!(status_label("unknown"), "unknown");
        assert_eq!(occurrence_status_label("pending"), "待执行");
        assert_eq!(occurrence_status_label("failed"), "失败");
        assert_eq!(occurrence_status_label("processing"), "执行中");
        assert_eq!(occurrence_status_label("cancelled"), "已取消");
    }

    #[test]
    fn subscription_filter_set_excludes_completed_status() {
        let filters = subscription_status_filters();
        let labels = filters.iter().map(|f| f.1.as_str()).collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["进行中", "已暂停", "已取消"],
            "订阅无「已完成」过滤项（#309 时期现状，不越权补齐）"
        );
    }

    #[test]
    fn unfinished_status_matrix_unchanged() {
        assert!(is_unfinished_status("active"));
        assert!(is_unfinished_status("paused"));
        assert!(!is_unfinished_status("cancelled"));
        assert!(!is_unfinished_status("completed"));
    }

    #[test]
    fn row_actions_follow_status_matrix() {
        assert_eq!(
            row_actions_of("active"),
            SubRowActions {
                can_pause: true,
                can_resume: false,
                can_cancel: true,
                can_edit: true
            }
        );
        assert_eq!(
            row_actions_of("paused"),
            SubRowActions {
                can_pause: false,
                can_resume: true,
                can_cancel: true,
                can_edit: true
            }
        );
        assert_eq!(
            row_actions_of("cancelled"),
            SubRowActions::default(),
            "已取消计划不提供任何动作"
        );
        assert_eq!(
            row_actions_of("completed"),
            SubRowActions::default(),
            "已完成计划不提供任何动作"
        );
    }

    #[test]
    fn assemble_sub_rows_fills_cells_and_next_charge() {
        let rows = vec![snap("active", 3_000)];
        let cells = assemble_sub_rows(&rows, &empty_ctx(false));
        assert_eq!(cells.len(), 1);
        let cell = &cells[0];
        assert_eq!(cell.id, "p-1");
        assert_eq!(cell.note, "视频会员");
        assert_eq!(cell.amount, "¥30", "计划金额按币种小数位呈现");
        assert_eq!(cell.recurrence, "每月");
        assert_eq!(cell.start_date, "2026-01-15");
        assert_eq!(cell.status, "进行中");
        assert_eq!(
            cell.next, "2026-02-15 · ¥3",
            "下期扣款 = 最早 pending 期次日期 · 金额"
        );
    }

    #[test]
    fn sub_row_next_charge_or_detail_failure_slots_are_stable() {
        let mut row = snap("active", 100);
        row.next = None;
        let cells = assemble_sub_rows(&[row.clone()], &empty_ctx(false));
        assert_eq!(cells[0].next, "—", "无 pending 期次占位（不现场推算日期）");
        row.detail_failed = true;
        let cells = assemble_sub_rows(&[row], &empty_ctx(false));
        assert_eq!(
            cells[0].next, "加载失败",
            "详情命令失败区分于「无数据」不静默"
        );
    }

    #[test]
    fn amount_masks_when_privacy_enabled() {
        let rows = vec![snap("active", 3_000)];
        let cells = assemble_sub_rows(&rows, &empty_ctx(true));
        assert_eq!(cells[0].amount, "••••", "隐私掩码恒形");
        assert_eq!(
            cells[0].next, "2026-02-15 · ••••",
            "下期扣款金额同口径掩码（日期仍可读）"
        );
    }

    #[test]
    fn sub_columns_cover_ten_desktop_columns() {
        let keys: Vec<&str> = SUB_COLUMNS.iter().map(|c| c.key).collect();
        assert_eq!(
            keys,
            vec![
                "note",
                "merchant",
                "category",
                "account",
                "amount",
                "recurrence",
                "start_date",
                "status",
                "next",
                "actions"
            ],
            "桌面档十列一字不动（回归红线）"
        );
    }
}
