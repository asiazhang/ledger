// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。

//! 保单视图（issue #2050 NSH 迁移⑲；旧壳 `PoliciesView.vue` + `src/policy/`
//! 组件树的重生）：保单清单（保司显示映射含软删 / 保障期间 / 到期态 / 保额
//! 纯展示 / 保单视角统计三列）+ 新建与编辑共表单弹层 + 缴费协议（新建可折叠
//! 可选、编辑模式协议历史分段与添加 / 改价）+ 软删轻量确认气泡 + 来源跳转
//! 行高亮。
//!
//! 职责边界（按旧壳核实平价）：清单卡（装配产物渲染、零新增列）+ 表单弹层
//!（保司「选择在用保司或输入即建」双态值语义——解析收口住保存时的按名
//! find-or-create 域接缝，语义与商户接缝同构但保司是保险域自有字典
//! ADR-0082，两字典不合并参数化；保单编辑为全量替换；保额与币种成对——
//! 清保额即清币种，不产生只有币种的半挂状态）+ 缴费协议（订阅形态 + 保单
//! 引用不挂商户，ADR-0051 决策 2：保费归属唯一事实是保单引用，付款对象语
//! 义由保单的保司承担；改价 = 取消旧协议 + 按新金额重建——订阅既有语义；
//! 分段历史 = 价格分段真相含已取消段）+ 删除确认守卫（轻量确认气泡：旧壳
//! AppPopconfirm 同形态，软删不物理移除、历史引用保留不置空，ADR-0051 决
//! 策 5）。
//!
//! 写路径直调域公开写入口（不经 IPC，ADR-0145 决策 4），成功后重拉清单并
//! 广播交易失效信号（交易列表来源列反查保单——旧壳 ledger:changed 粗粒度语
//! 义的壳内对应物；广播接线删除即红钉在 headless 重读旅程）。失效信号订阅
//! 同语义反方向：挂单流水变化（执行期次 / 删除交易）经交易失效信号订阅重读
//! 保单统计。
//!
//! 界面文案仅 zh-CN（旧壳 policies.* zh-CN 词条原样字面承载）；码化错误 zh
//! 原样呈现；视图本地快照、离开再回 = 全新装载（旧壳 onMounted 同口径）；无
//! toast，动作反馈落状态条位（#2038 裁决）。行聚焦的滚动定位 NSH 无宿主面，
//! 本壳落点 = 高亮呈现（物品视图先例同口径），滚动位随窄窗横切票。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::{AppError, Result as InfraResult};
use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::{FontWeight, SharedString};
use nsh_core::prelude::*;

use crate::app::{db_slot, field_label, table_classes};
use crate::assembly::{self, CurrencyTable, POLICY_COLUMNS};
use crate::boot;
use crate::combo_select;
use crate::date_select;
use crate::modal;
use crate::nav_model::ViewName;
use crate::pinyin_select;
use crate::popconfirm;
use crate::select;
use crate::{nav, sidebar, signals};
use ledger_policy::{Policy, PolicyInput, PolicyStats};
use ledger_scheduled::ScheduledTransactionWithExt;

// ---------------------------------------------------------------------------
// 词表（旧壳 policies.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const LIST_CARD_TITLE: &str = "保单列表";
const NEW_BUTTON: &str = "新建保单";
const EMPTY_GUIDE: &str = "暂无保单：点击「新建保单」建立第一份合同档案。";
const ROW_EDIT: &str = "编辑";
const ROW_DELETE: &str = "删除";
const ROW_CANCEL: &str = "取消";
const DELETE_CONFIRM: &str = "删除后不再显示在列表，历史记录保留。确认删除该保单？";
const MSG_CREATED: &str = "保单已创建";
const MSG_SAVED: &str = "保单已保存";
const MSG_DELETED: &str = "保单已删除";
const MSG_SAVE_FAILED: &str = "保存失败";
const MSG_DELETE_FAILED: &str = "删除失败";
const FORM_TITLE_CREATE: &str = "新建保单";
const FORM_TITLE_EDIT: &str = "编辑保单";
const LABEL_INSURER: &str = "保险公司";
const LABEL_NUMBER: &str = "保单号";
const LABEL_PRODUCT: &str = "险种名称";
const LABEL_PERIOD: &str = "保障期间";
const LABEL_COVERAGE: &str = "保额";
const LABEL_NOTE: &str = "备注";
const PLACEHOLDER_INSURER: &str = "选择保司或输入新名称";
const PLACEHOLDER_NUMBER: &str = "保单号";
const PLACEHOLDER_PRODUCT: &str = "如：重大疾病保险";
const PLACEHOLDER_COVERAGE: &str = "保额（元，可选）";
const PLACEHOLDER_CURRENCY: &str = "币种";
const PLACEHOLDER_NOTE: &str = "备注（可选）";
const PLACEHOLDER_RECURRENCE: &str = "每年";
const MSG_NUMBER_REQUIRED: &str = "保单号不能为空";
const MSG_PRODUCT_REQUIRED: &str = "险种名称不能为空";
const MSG_END_BEFORE_START: &str = "保障期间止日不能早于起日";
const MSG_AMOUNT_INVALID: &str = "保额必须大于 0";
const MSG_CURRENCY_REQUIRED: &str = "填写保额时必须选择保额币种";
const MSG_INSURER_REQUIRED: &str = "请选择或输入保险公司";
const STATS_PAID: &str = "累计已缴保费";
const STATS_INFLOW: &str = "累计现金流入";
const STATS_NEXT_CHARGE: &str = "下期扣款日";
const STATS_EXPIRY: &str = "到期状态";
const EXPIRY_ACTIVE: &str = "保障中";
const EXPIRY_EXPIRED: &str = "已到期";
const AGREEMENT_SECTION: &str = "缴费协议";
const AGREEMENT_TOGGLE_HINT: &str =
    "可选：同时创建订阅形态缴费协议，每期自动生成带本保单引用的保费流水";
const AGREEMENT_ON: &str = "已开启";
const AGREEMENT_OFF: &str = "已关闭";
const AGREEMENT_AMOUNT: &str = "每期金额";
const AGREEMENT_AMOUNT_PLACEHOLDER: &str = "每期保费（元）";
const AGREEMENT_RECURRENCE: &str = "扣款频率";
const AGREEMENT_ACCOUNT: &str = "扣款账户";
const AGREEMENT_ACCOUNT_PLACEHOLDER: &str = "选择扣款账户";
const AGREEMENT_START_DATE: &str = "起始日";
const AGREEMENT_ADD: &str = "添加缴费协议";
const AGREEMENT_REBUILD: &str = "改价";
const AGREEMENT_REBUILD_CONFIRM: &str = "取消旧协议并重建";
const AGREEMENT_REBUILD_HINT: &str =
    "改价 = 取消旧协议 + 按新金额重建：新协议自下期扣款日起生效，价格历史分为两段。";
const AGREEMENT_CREATE: &str = "创建";
const AGREEMENT_SEG_AMOUNT: &str = "每期金额";
const AGREEMENT_SEG_RECURRENCE: &str = "周期";
const AGREEMENT_SEG_START: &str = "起始日";
const AGREEMENT_SEG_STATUS: &str = "状态";
const SEGMENTS_EMPTY: &str = "暂无缴费协议";
const MSG_ACCOUNT_REQUIRED: &str = "请选择扣款账户";
const MSG_AGREEMENT_AMOUNT_INVALID: &str = "每期保费必须大于 0";
const MSG_AGREEMENT_CREATED: &str = "缴费协议已创建";
const MSG_AGREEMENT_REBUILT: &str = "已取消旧协议并按新金额重建";
const MSG_AGREEMENT_FAILED: &str = "操作失败";
const MSG_SEGMENTS_FAILED: &str = "协议历史加载失败";
const EVERY_WORD: &str = "每";
const PERIOD_DIVIDER: &str = "~";

/// 装载占位与任务缺省错误文案（旧壳 loading / 异常分支同口径）。
const LOADING_PLACEHOLDER: &str = "正在加载…";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";

/// 统计行缺失 / 无活跃协议等「—」占位（旧壳 policyStatAmountText 等同口径）。
const DASH_PLACEHOLDER: &str = "—";

/// 本位币基准读取失败的软回退（旧壳缺省 CNY 同口径）。
const BASE_CURRENCY_FALLBACK: &str = "CNY";

/// 改价目标不在场的码化错误码（不可达兜底：改价入口只在活跃段在场时呈现；
/// 码化错误构造器家族的归位面，呈现 zh 原样）。
const CODE_REBUILD_TARGET_MISSING: &str = "nsh-shell.policy-rebuild-target-missing";

/// 空保司名的码化错误码（不可达兜底：保存入口先拦空名；域内同名守卫兜底）。
const CODE_INSURER_NAME_REQUIRED: &str = "nsh-shell.policy-insurer-name-required";

/// 聚焦行 test_id 锚点（headless「删除即红」：来源跳转接线 / 高亮行的锚点）。
const POLICY_ROW_FOCUS_ANCHOR: &str = "policy-row-focus";

/// 聚焦行高亮类（旧壳 `.policy-row-focus` 琥珀底的 NSH 对应形态——主题
/// accent 透明底，双态自适应）。
const POLICY_ROW_FOCUS_CLASS: &str = "bg-accent/25";

/// 到期态推导的本地回落（统计行未加载窗口——同一规则：止日非空且早于今天
/// → 已到期；止日空 = 长期/终身恒 false。可推导的状态不落库，ADR-0051）。
fn local_expired(end_date: Option<&str>, today: &str) -> bool {
    end_date.is_some_and(|end| end < today)
}

/// 「今天」（本地时区日历日文本）：到期态本地回落推导的时间基点（旧壳
/// todayStr 行渲染即时取同口径——无日历事件源，写入触发的重拉顺带刷新快照）。
fn today_text() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// 主题令牌单取（缺失回退双态兜底，app.rs 先例同形）。
fn token_or_fallback(key: &'static str, fallback_light: Hsla) -> AdaptiveColor {
    nsh_core::element::tokens()
        .color(key)
        .unwrap_or_else(|| fallback_light.into())
}

fn foreground_of() -> AdaptiveColor {
    token_or_fallback("foreground", hsla(0.0, 0.0, 0.98, 1.0))
}

fn muted_of() -> AdaptiveColor {
    token_or_fallback("muted-foreground", hsla(0.0, 0.0, 0.45, 1.0))
}

fn danger_of() -> AdaptiveColor {
    token_or_fallback("destructive", hsla(0.0, 0.0, 0.98, 1.0))
}

fn success_of() -> AdaptiveColor {
    token_or_fallback("success", hsla(0.42, 0.62, 0.35, 1.0))
}

fn warning_of() -> AdaptiveColor {
    token_or_fallback("warning", hsla(0.10, 0.87, 0.53, 1.0))
}

// ---------------------------------------------------------------------------
// 快照与流形状
// ---------------------------------------------------------------------------

/// 保单 + 统计快照容器（视图信号持有用）：域行与统计行无 `PartialEq`，信号
/// 载荷等值以「行集逐行 id + updated_at + version + 同保单统计四呈现字段」
/// 呈现等值面（物品行快照容器同形，呈现等值即反复重写不通知）。
#[derive(Debug, Clone, Default)]
pub(crate) struct PoliciesSnap {
    pub policies: Vec<Policy>,
    pub stats: Vec<PolicyStats>,
}

impl PartialEq for PoliciesSnap {
    fn eq(&self, other: &Self) -> bool {
        self.policies.len() == other.policies.len()
            && self.stats.len() == other.stats.len()
            && self
                .policies
                .iter()
                .zip(other.policies.iter())
                .all(|(a, b)| {
                    a.id == b.id && a.updated_at == b.updated_at && a.version == b.version
                })
            && self.stats.iter().zip(other.stats.iter()).all(|(a, b)| {
                a.policy_id == b.policy_id
                    && a.total_paid_native_cents == b.total_paid_native_cents
                    && a.total_inflow_native_cents == b.total_inflow_native_cents
                    && a.next_charge_date == b.next_charge_date
                    && a.is_expired == b.is_expired
            })
    }
}

/// 保司字典快照容器（视图信号持有用）：等值面 = id + name + 软删标志。
#[derive(Debug, Clone, Default)]
pub(crate) struct InsurersSnap(pub Vec<ledger_policy::Insurer>);
impl PartialEq for InsurersSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(a, b)| a.id == b.id && a.name == b.name && a.is_deleted == b.is_deleted)
    }
}

/// 账户行集快照容器（视图信号持有用）：等值面 = id + name + 归档标志 + 币种
///（缴费协议扣款账户候选面）。
#[derive(Debug, Clone, Default)]
pub(crate) struct AccountsSnap(pub Vec<ledger_accounts::Account>);
impl PartialEq for AccountsSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(other.0.iter()).all(|(a, b)| {
                a.id == b.id
                    && a.name == b.name
                    && a.is_archived == b.is_archived
                    && a.currency_code == b.currency_code
            })
    }
}

/// 币种字典快照容器（视图信号持有用）：`ledger_currencies::Currency` 无
/// `PartialEq`，等值面 = code + name + symbol + 小数位（表单下拉 / 格式化消
/// 费面同源）。
#[derive(Debug, Clone, Default)]
pub(crate) struct CurrenciesSnap(pub Vec<ledger_currencies::Currency>);
impl PartialEq for CurrenciesSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(other.0.iter()).all(|(a, b)| {
                a.code == b.code
                    && a.name == b.name
                    && a.symbol == b.symbol
                    && a.decimal_places == b.decimal_places
            })
    }
}

/// 协议历史快照容器（视图信号持有用）：等值面 = 计划 id + updated_at + 状
/// 态（添加 / 改价后分段列表刷新，静段的历史段保持等值不触发重建）。
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanSnap(pub Vec<Segment>);
impl PartialEq for PlanSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(a, b)| a.id == b.id && a.created_at == b.created_at && a.status == b.status)
    }
}

/// 表单保司候选行（在用保司；软删行不进新选择面——旧壳 insurerOptions 用
/// reference store 的 active 批）。
fn insurer_options(insurers: &InsurersSnap) -> Vec<assembly::CategoryOption> {
    insurers
        .0
        .iter()
        .filter(|insurer| !insurer.is_deleted)
        .map(|insurer| assembly::CategoryOption {
            id: insurer.id.clone(),
            label: insurer.name.clone(),
        })
        .collect()
}

/// 保单 id → 保司名（含软删显示映射；存量保单照常显示，旧壳 insurerMap 同
/// 口径）。
fn insurer_name_of(insurers: &InsurersSnap, id: &str) -> Option<String> {
    insurers
        .0
        .iter()
        .find(|insurer| insurer.id == id)
        .map(|insurer| insurer.name.clone())
}

/// 缴费协议扣款账户候选（在用账户 + 归档补值——改价场景预填项可能指向已
/// 归档账户，不补值会露出裸 uuid，spec #1952 / ADR-0142 决策 4 同形）。
fn agreement_account_options(
    accounts: &AccountsSnap,
    selected: Option<&str>,
) -> Vec<assembly::CategoryOption> {
    let mut options: Vec<assembly::CategoryOption> = accounts
        .0
        .iter()
        .filter(|account| !account.is_archived)
        .map(|account| assembly::CategoryOption {
            id: account.id.clone(),
            label: account.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && !options.iter().any(|option| option.id == id)
        && let Some(row) = accounts.0.iter().find(|account| account.id == id)
    {
        options.push(assembly::CategoryOption {
            id: row.id.clone(),
            label: format!("{}（已归档）", row.name),
        });
    }
    options
}

/// 协议历史分段视图行（域计划行的呈现投影——域行无 Clone 语义，分段列表与
/// 改价回填消费面只取这些字段，快照容器按 id + created_at + status 等值）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    id: String,
    /// 保单引用（分段归属；订阅形态协议专属——分期 / 定时转账恒缺）。
    policy_id: Option<String>,
    created_at: String,
    status: String,
    amount_cents: i64,
    currency_code: String,
    recurrence_type: String,
    recurrence_interval: i64,
    start_date: String,
    account_id: String,
}

impl From<&ScheduledTransactionWithExt> for Segment {
    fn from(plan: &ScheduledTransactionWithExt) -> Self {
        Self {
            id: plan.core.id.clone(),
            policy_id: plan.policy_id.clone(),
            created_at: plan.core.created_at.clone(),
            status: plan.core.status.clone(),
            amount_cents: plan.core.amount_cents,
            currency_code: plan.core.currency_code.clone(),
            recurrence_type: plan.core.recurrence_type.clone(),
            recurrence_interval: plan.core.recurrence_interval,
            start_date: plan.core.start_date.clone(),
            account_id: plan.core.account_id.clone(),
        }
    }
}

/// 协议历史分段（该保单名下全部订阅形态协议，按创建先后 = 分段先后；排序
/// 单点与旧壳 PolicyAgreementSection 同口径：created_at 升序、id 决胜负）。
fn segments_of(plans: &PlanSnap, policy_id: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = plans
        .0
        .iter()
        // kind 过滤由 policy_id 的存在性承担：协议引用只有订阅形态可携带
        //（域 `CreateScheduledInput.policy_id` 契约——分期 / 定时转账携带
        // 即被行为层拒绝），policy_id 命中即订阅形态的引用。
        .filter(|plan| plan.policy_id.as_deref() == Some(policy_id))
        .cloned()
        .collect();
    segments.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    segments
}

/// 协议历史分段行文案（分段真相一行一读；金额按分段币种原生格式化——纯
/// 展示消费计划本体，无折算面，旧壳 segmentColumns 同口径）。
fn segment_row_text(segment: &Segment, currencies: &CurrencyTable) -> String {
    let fmt = currencies.format_of(&segment.currency_code);
    format!(
        "{} · {} · {} · {}",
        assembly::format_amount_basic(segment.amount_cents, &fmt),
        crate::scheduled::recurrence_label(&segment.recurrence_type, segment.recurrence_interval,),
        segment.start_date,
        crate::scheduled::status_label(&segment.status),
    )
}

/// 逐段固定锚点（headless 对准统计四段的 id：与字段序同位，单摘要多列）。。统计行未加载时合计显示占位不做本地二次聚合；到期
/// 态推导：止日空 = 长期/终身（永不判到期），止日非空消费统计同源
/// is_expired、统计行未加载时回落本地推导——旧壳 expiryText 同口径。
fn summary_lines(
    policy: &Policy,
    stat: Option<&PolicyStats>,
    currencies: &CurrencyTable,
    today: &str,
) -> Vec<(&'static str, String)> {
    let loaded = stat.is_some();
    let fmt = stat
        .map(|s| currencies.format_of(&s.native_currency))
        .unwrap_or_default();
    let native_text = |cents: i64| -> String {
        if loaded {
            assembly::format_amount_basic(cents, &fmt)
        } else {
            DASH_PLACEHOLDER.to_owned()
        }
    };
    let expired = match stat {
        Some(stat) => stat.is_expired,
        None => local_expired(policy.end_date.as_deref(), today),
    };
    // 到期状态文案：止日空 = 长期/终身（永不判到期）；止日非空按到期态二值。
    let expiry_text = if policy.end_date.is_none() {
        "长期/终身".to_owned()
    } else if expired {
        EXPIRY_EXPIRED.to_owned()
    } else {
        EXPIRY_ACTIVE.to_owned()
    };
    vec![
        (
            STATS_PAID,
            native_text(stat.map(|s| s.total_paid_native_cents).unwrap_or(0)),
        ),
        (
            STATS_INFLOW,
            native_text(stat.map(|s| s.total_inflow_native_cents).unwrap_or(0)),
        ),
        (
            STATS_NEXT_CHARGE,
            stat.and_then(|s| s.next_charge_date.clone())
                .unwrap_or_else(|| DASH_PLACEHOLDER.to_owned()),
        ),
        (STATS_EXPIRY, expiry_text),
    ]
}

/// 保单列定义 → 宿主列（UI 层只做映射，不产列；弹性宽度分配同账户先例——
/// 装配列无弹性机制，富余宽度全给险种列，列宽计算不掺装配）。
fn policy_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = POLICY_COLUMNS
        .iter()
        .map(|def| {
            let mut column = DataTableColumn::default();
            column.key = def.key.into();
            column.title = Some(def.title.into());
            column.width = Some(Points(def.width));
            column.align = match def.align {
                assembly::ColumnAlign::Start => DataTableAlign::Start,
                assembly::ColumnAlign::End => DataTableAlign::End,
            };
            column
        })
        .collect();
    let suggested: f32 = POLICY_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - POLICY_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(product) = columns.iter_mut().find(|col| col.key == "product_name")
        && let Some(w) = product.width
    {
        product.width = Some(Points(w.0 + extra));
    }
    columns
}

/// 表格外围定宽扣减（保单列宽计算输入）：内容列 p-4（两侧 16pt）+ 清单卡
/// p-3（两侧 12pt）+ 清单卡描边两侧 1pt——与储蓄目标视图同形。窄窗自适应随
/// 系列尾部横切票（Out of Scope）。
const POLICY_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 弹窗内「标签 + 值」行为件（旧壳 NFormItem label-placement left 同布局）。
fn form_row(label: &'static str, muted: AdaptiveColor, value: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label(label, muted))
        .child(value)
}

/// 弹窗动作按钮行（取消在前、主按钮殿后——账户 / 储蓄先例同款；id 拼出两
/// 锚点：`{id}-cancel` / `{id}-save`，label 由调用方持词表）。
fn modal_buttons(
    id: &'static str,
    confirm_label: &'static str,
    fg: AdaptiveColor,
    on_cancel: impl Fn() + 'static + Copy,
    on_confirm: impl Fn() + 'static + Copy,
) -> View {
    let cancel_id = format!("{id}-cancel");
    let confirm_id = format!("{id}-save");
    view()
        .flex_row()
        .justify_end()
        .gap_2()
        .child(
            button()
                .app_class(
                    "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                )
                .on_press(move |_: &()| on_cancel())
                .child(
                    text(ROW_CANCEL)
                        .text_size(px(13.0))
                        .text_color(fg)
                        .test_id(cancel_id.as_str()),
                ),
        )
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                .on_press(move |_: &()| on_confirm())
                .child(
                    text(confirm_label)
                        .text_size(px(13.0))
                        .text_color(rgb(0xffffff))
                        .test_id(confirm_id.as_str()),
                ),
        )
}

/// 弹窗错误条位（拦截面线，下次尝试开始清空；码化错误 zh 原样呈现不加二级
/// 前缀）。
fn error_line(
    show: impl Fn() -> bool + 'static,
    text_of: impl Fn() -> String + 'static,
    color: AdaptiveColor,
    id: &'static str,
) -> impl IntoElement {
    when(show, move || {
        text(text_of())
            .text_size(px(12.0))
            .text_color(color)
            .test_id(id)
    })
    .otherwise(view)
}

/// 文本输入字段（弹窗标签行；label 与占位由调用方持词表）。
fn text_field(
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: Signal<String>,
    width: f32,
    fg: AdaptiveColor,
) -> impl IntoElement {
    form_row(
        label,
        muted_of(),
        input()
            .test_id(id)
            .value(move || value.get())
            .placeholder(placeholder)
            .optional_label(label)
            .text_color(fg)
            .w(px(width))
            .app_class(
                "bg-background rounded-md border border-border px-2 py-1 \
                 placeholder:text-muted-foreground",
            )
            .on_change(move |text: &str| value.set(text.to_owned())),
    )
}

// ============================================================================
// 保单视图（导航骨架内保单分支的真实组件，#2050；独立路由与资产「更多」
// 收纳页签共用同一组件整体装载——旧壳 /policies 与容器页签同组件同形态）。
// ============================================================================
#[component]
pub(crate) fn PoliciesView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    let policies = signal(PoliciesSnap::default());
    let insurers = signal(InsurersSnap::default());
    let currency_rows = signal(CurrenciesSnap::default());
    let accounts = signal(AccountsSnap::default());
    let base_currency = signal(BASE_CURRENCY_FALLBACK.to_owned());
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);
    let popconfirm_open = signal(None::<String>);
    let status = signal(None::<String>);

    /// 装载批产物（读接缝单点的具名元组）。
    type PolicyLoads = InfraResult<(
        Vec<Policy>,
        Vec<PolicyStats>,
        Vec<ledger_policy::Insurer>,
        Vec<ledger_currencies::Currency>,
        Vec<ledger_accounts::Account>,
        String,
    )>;

    // 装载批：保单清单 + 保单视角统计 + 保司字典（含软删显示映射面）+ 币种
    // 字典 + 可见账户 + 缺省币种，同批读取（旧壳 onMounted 的两批合一）。
    // 竞态 latest-wins（账户视图先例）。
    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || -> PolicyLoads {
                    let rows = boot::policy_rows(&db)?;
                    let stats = boot::policy_stats_rows(&db)?;
                    let insurer_list = boot::insurer_rows(&db, true)?;
                    let currency_list = boot::currency_rows(&db)?;
                    let account_list = boot::account_rows(&db)?;
                    let base = boot::base_currency_code(&db)
                        .unwrap_or_else(|_| BASE_CURRENCY_FALLBACK.to_owned());
                    Ok((rows, stats, insurer_list, currency_list, account_list, base))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((rows, stats, insurer_list, currency_list, account_list, base))) => {
                        policies.set(PoliciesSnap {
                            policies: rows,
                            stats,
                        });
                        insurers.set(InsurersSnap(insurer_list));
                        currency_rows.set(CurrenciesSnap(currency_list));
                        accounts.set(AccountsSnap(account_list));
                        base_currency.set(base);
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some(READ_TASK_FAILED.to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // ============================ 来源跳转落点（行高亮） ============================
    let highlight = signal(None::<String>);
    // 落位 + 意图消费（读一次封闸：「先拿 id 后等数据」——只暂存 id，行渲染
    // 后生效，旧壳 useFocusParam 同语义；软删保单不在列表，高亮无锚空转安
    // 全）。高亮保持到实例消亡：刷新/重进 = 新实例重定位（NSH 重新导航重新
    // 投递 = 旧壳「URL 在场即复现」的进程内对应形态）。独立路由与收纳页签
    // 装载共用本消费点（容器分支真实视图不抢占位 effect 的意图消费——
    // views.rs 例外表含本视图）。
    effect(move || {
        let _landed = nav::store().current.get();
        let intent = nav::take_intent();
        highlight.set(intent.and_then(|intent| intent.focus));
    });

    // ============================ 落位装载与失效信号 ============================
    effect(move || {
        let _landed = nav::store().current.get();
        reload();
    });
    // 失效信号订阅（旧壳 policiesStore `ledger:changed` 失效重拉 + statsById
    // 同生命周期重拉的壳内对应物）：挂单流水 / 协议改动（含本壳写入口广播）
    // → 清单与统计重读。订阅句柄随组件实例存亡（app.rs reload_subscription
    // 同形：直放 let 会在组件体结束时 Drop 撤销）。
    let reload_subscription = signals::subscribe_transactions_changed(reload);
    effect(move || {
        let _held_for_view_lifetime: &signals::TransactionsChangedSubscription =
            &reload_subscription;
    });

    // ============================ 主题令牌 ============================
    let fg = foreground_of();
    let muted = muted_of();
    let danger_fg = danger_of();

    // ============================ 表单弹层状态 ============================
    // 新建 / 编辑共用同一弹层语义（编辑复用同表单回填初值）：`form_open`
    // 控制开合，`editing_id` 区分模式（None = 新建，旧壳 ModalIntent 单成员
    // 意图闭集 + `:key=formSeq` 重建的对应形态——关闭即回终态，字段归信号）。
    let form_open = signal(false);
    let editing_id: Signal<Option<String>> = signal(None);
    let form_insurer_typed = signal(String::new());
    let form_insurer_selected: Signal<Option<String>> = signal(None);
    let form_number = signal(String::new());
    let form_product = signal(String::new());
    let form_start = signal(chrono::Local::now().date_naive());
    let form_end = signal(None::<chrono::NaiveDate>);
    let form_coverage_text = signal(String::new());
    let form_coverage_currency: Signal<Option<String>> = signal(None);
    let form_note = signal(String::new());
    let form_error = signal(None::<String>);
    let is_edit = move || editing_id.get_untracked().is_some();

    // ============================ 缴费协议（新建模式，可折叠可选） ============================
    // 开关默认关（趸交 / 缴清纯档案）；开启后填频率 / 金额 / 扣款账户 / 起始
    // 日（起始日预填保障期间起日，PolicyAgreementFields.reset 同语义）。
    let with_agreement = signal(false);
    let agr_amount = signal(String::new());
    let agr_currency: Signal<Option<String>> = signal(None);
    let agr_recurrence = signal(Some("yearly".to_owned()));
    let agr_interval_text = signal("1".to_owned());
    let agr_account: Signal<Option<String>> = signal(None);
    let agr_start = signal(chrono::Local::now().date_naive());

    // 协议区字段复位（旧壳 reset 同语义：金额恒清空要求显式输入，频次 / 单
    // 位回缺省，起始日靠调用方拨入的预填；币种缺省 = 账本缺省币种）。
    let reset_agreement = move |start: chrono::NaiveDate, default_currency: Option<String>| {
        with_agreement.set(false);
        agr_amount.set(String::new());
        agr_currency.set(default_currency);
        agr_recurrence.set(Some("yearly".to_owned()));
        agr_interval_text.set("1".to_owned());
        agr_account.set(None);
        agr_start.set(start);
    };
    // 保额清空即清币种（成对原子，不产生只有币种的半挂状态——旧壳
    // watch(coverageFilled) 同语义）。
    effect(move || {
        if form_coverage_text.get().trim().is_empty() {
            form_coverage_currency.set(None);
        }
    });

    // ============================ 协议历史（编辑模式） ============================
    let segments = signal(PlanSnap::default());
    let segments_error = signal(None::<String>);
    let seg_mode = signal(None::<String>);
    let seg_amount = signal(String::new());
    let seg_currency: Signal<Option<String>> = signal(None);
    let seg_recurrence = signal(Some("yearly".to_owned()));
    let seg_interval_text = signal("1".to_owned());
    let seg_account: Signal<Option<String>> = signal(None);
    let seg_start = signal(chrono::Local::now().date_naive());
    let seg_old_plan_id = signal(None::<String>);
    let seg_error = signal(None::<String>);

    let close_seg_form = move || {
        seg_mode.set(None);
        seg_old_plan_id.set(None);
        seg_error.set(None);
    };

    // ============================ 行装配（渲染期零格式化） ============================
    let currencies = computed(move || CurrencyTable::from_currencies(currency_rows.get().0.iter()));
    let rows = computed(move || {
        let currency_table = currencies.get();
        let snap = policies.get();
        assembly::assemble_policy_rows(
            &snap.policies,
            &snap.stats,
            &insurers.get().0,
            &currency_table,
            &today_text(),
        )
    });

    // 行快照反查（编辑 / 删除的目标）：域行快照内单点；行缺席（并发软删）
    // 动作静默退场（弹窗不呈现半态）。
    let row_of = move |policy_id: &str| -> Option<Policy> {
        policies
            .get_untracked()
            .policies
            .iter()
            .find(|row| row.id == policy_id)
            .cloned()
    };

    // ============================ 协议历史装载 ============================
    // 编辑弹窗打开时拉一次 + 添加 / 改价成功后重拉（旧壳 watch(policy) load
    // 同生命周期）：保单名下全部计划经 [`boot::plan_rows`] 读取后视图侧按
    // 保单 id 过滤（计划读取口径不定义保单语义，过滤是呈现侧的组合面）。读
    // 失败不阻断静态编辑（协议区呈现失败态，旧壳 message.error 同口径）。
    let load_segments = move |policy_id: String| {
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::plan_rows(db)
                        .map(|plans| PlanSnap(plans.iter().map(Segment::from).collect())),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(snap)) => {
                        segments.set(snap);
                        segments_error.set(None);
                    }
                    Ok(Err(e)) => segments_error.set(Some(format!("{MSG_SEGMENTS_FAILED}: {e}"))),
                    Err(_) => segments_error.set(Some(READ_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 软删（轻量确认气泡锚定行内删除按钮） ============================
    let perform_delete = move |id: String| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_policy(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(MSG_DELETED.to_owned()));
                        // 保单变化改变交易列表来源列反查口径——按旧壳
                        // ledger:changed 粗粒度语义广播交易失效（广播接线删
                        // 除即红：headless 来源列重读旅程）。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => status.set(Some(format!("{MSG_DELETE_FAILED}: {e}"))),
                    Err(_) => status.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 弹窗开关（回填 / 复位） ============================
    let close_form = move || form_open.set(false);
    let open_create = move || {
        form_insurer_typed.set(String::new());
        form_insurer_selected.set(None);
        form_number.set(String::new());
        form_product.set(String::new());
        form_start.set(chrono::Local::now().date_naive());
        form_end.set(None);
        form_coverage_text.set(String::new());
        form_coverage_currency.set(None);
        form_note.set(String::new());
        form_error.set(None);
        reset_agreement(
            chrono::Local::now().date_naive(),
            Some(base_currency.get_untracked()),
        );
        close_seg_form();
        segments.set(PlanSnap::default());
        segments_error.set(None);
        editing_id.set(None);
        form_open.set(true);
    };
    let open_edit = move |policy_id: String| {
        let Some(existing) = row_of(&policy_id) else {
            return;
        };
        // 保司双态值回填：选中 id 原样携带（编辑维持历史引用），文本位保司
        // 显示映射解析名（软删保司照常显示，ADR-0082）。
        form_insurer_selected.set(Some(existing.insurer_id.clone()));
        form_insurer_typed.set(
            insurer_name_of(&insurers.get_untracked(), &existing.insurer_id).unwrap_or_default(),
        );
        form_number.set(existing.policy_number.clone());
        form_product.set(existing.product_name.clone());
        let start = date_select::parse_date_text(&existing.start_date)
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        form_start.set(start);
        form_end.set(
            existing
                .end_date
                .as_deref()
                .and_then(date_select::parse_date_text),
        );
        form_coverage_text.set(
            existing
                .coverage_amount_cents
                .map(|cents| {
                    let dp = currencies
                        .get_untracked()
                        .format_of(&existing.coverage_currency_code.clone().unwrap_or_default())
                        .decimal_places;
                    assembly::cents_to_amount_text(cents, dp)
                })
                .unwrap_or_default(),
        );
        form_coverage_currency.set(
            existing
                .coverage_currency_code
                .clone()
                .or_else(|| Some(base_currency.get_untracked())),
        );
        form_note.set(existing.note.clone().unwrap_or_default());
        form_error.set(None);
        // 协议区随弹窗复位（开关默认关 = 趸交 / 缴清纯档案；起始日预填保障
        // 期间起日——PolicyAgreementFields.reset 的 startDate 预填同语义）。
        reset_agreement(start, Some(base_currency.get_untracked()));
        close_seg_form();
        segments.set(PlanSnap::default());
        segments_error.set(None);
        editing_id.set(Some(policy_id.clone()));
        form_open.set(true);
        load_segments(policy_id);
    };

    // ============================ 保存执行 ============================
    // 客户端必填校验（消息与后端错误码文案同源，双保险防呆），协议区校验
    // 前置于建档（避免半建档状态）——旧壳 save 编排平价；保司解析与建档 /
    // 建协议在同一个 blocking 批内顺序完成（旧壳 resolve-insurer →
    // create_policy → createScheduledTransaction 三步同序；中途失败保留已
    // 落库前序、弹窗不关重试）。新建模式协议区携带保单引用不挂商户
    //（ADR-0082 决策 2：保费归属唯一事实是保单引用，付款对象语义由保单的
    // 保司承担），备注带险种可读（协议与保单分离，订阅清单靠备注可读）。
    let perform_save = move || {
        form_error.set(None);
        status.set(None);
        let insurer_selected = form_insurer_selected.get_untracked();
        let insurer_typed = form_insurer_typed.get_untracked().trim().to_owned();
        if insurer_selected.is_none() && insurer_typed.is_empty() {
            form_error.set(Some(MSG_INSURER_REQUIRED.to_owned()));
            return;
        }
        let number = form_number.get_untracked().trim().to_owned();
        if number.is_empty() {
            form_error.set(Some(MSG_NUMBER_REQUIRED.to_owned()));
            return;
        }
        let product = form_product.get_untracked().trim().to_owned();
        if product.is_empty() {
            form_error.set(Some(MSG_PRODUCT_REQUIRED.to_owned()));
            return;
        }
        let start_date = form_start.get_untracked().to_string();
        let end_date = form_end.get_untracked().map(|date| date.to_string());
        if let Some(end) = end_date.as_deref()
            && end < start_date.as_str()
        {
            form_error.set(Some(MSG_END_BEFORE_START.to_owned()));
            return;
        }
        // 保额与币种成对（填写保额时币种必填；清保额即清币种——旧壳
        // watch(coverageFilled) 的保存态等价面）。
        let coverage_text = form_coverage_text.get_untracked();
        let (coverage_cents, coverage_currency_code) = if coverage_text.trim().is_empty() {
            (None, None)
        } else {
            let Some(cents) =
                assembly::budget_yuan_to_cents(&coverage_text).filter(|cents| *cents > 0)
            else {
                form_error.set(Some(MSG_AMOUNT_INVALID.to_owned()));
                return;
            };
            match form_coverage_currency.get_untracked() {
                Some(code) => (Some(cents), Some(code)),
                None => {
                    form_error.set(Some(MSG_CURRENCY_REQUIRED.to_owned()));
                    return;
                }
            }
        };
        let note = {
            let text = form_note.get_untracked().trim().to_owned();
            (!text.is_empty()).then_some(text)
        };
        let input = PolicyInput {
            insurer_id: String::new(),
            policy_number: number,
            product_name: product,
            start_date,
            end_date,
            coverage_amount_cents: coverage_cents,
            coverage_currency_code,
            note,
        };
        let editing = editing_id.get_untracked();
        let is_update = editing.is_some();
        let with_agreement_create = editing.is_none() && with_agreement.get_untracked();
        // 协议区字段校验前置（先于建档，避免半建档状态）。币种缺省 = 账本
        // 缺省币种（复位时已写入单一来源）；协议字段闭集四项缺一即拦。
        let agreement = if with_agreement_create {
            let Some(account_id) = agr_account.get_untracked() else {
                form_error.set(Some(MSG_ACCOUNT_REQUIRED.to_owned()));
                return;
            };
            let Some(amount_cents) =
                assembly::amount_text_to_cents(&agr_amount.get_untracked()).filter(|c| *c > 0)
            else {
                form_error.set(Some(MSG_AGREEMENT_AMOUNT_INVALID.to_owned()));
                return;
            };
            let Some(recurrence_id) = agr_recurrence.get_untracked() else {
                form_error.set(Some(MSG_AGREEMENT_AMOUNT_INVALID.to_owned()));
                return;
            };
            let Ok(recurrence_type) = recurrence_id.parse::<ledger_scheduled::RecurrenceType>()
            else {
                form_error.set(Some(MSG_AGREEMENT_AMOUNT_INVALID.to_owned()));
                return;
            };
            let Some(interval) = agr_interval_text
                .get_untracked()
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|interval| *interval >= 1)
            else {
                form_error.set(Some(MSG_AGREEMENT_AMOUNT_INVALID.to_owned()));
                return;
            };
            let Some(currency_code) = agr_currency.get_untracked() else {
                form_error.set(Some(MSG_AGREEMENT_AMOUNT_INVALID.to_owned()));
                return;
            };
            let product_name = input.product_name.clone();
            Some((
                product_name,
                ledger_scheduled::CreateScheduledInput {
                    kind: ledger_scheduled::ScheduledKind::Subscription,
                    account_id,
                    category_id: None,
                    amount_cents,
                    currency_code,
                    recurrence_type,
                    recurrence_interval: interval,
                    recurrence_day: None,
                    start_date: agr_start.get_untracked().to_string(),
                    note: None,
                    merchant_id: None,
                    policy_id: None,
                    total_amount_cents: None,
                    total_occurrences: None,
                    to_account_id: None,
                },
            ))
        } else {
            None
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        // 保司解析（保存时单点收口，「输入即建 + 重名复用」
                        // 交互收口住按名 find-or-create 域接缝——选中 id 原
                        // 样携带（编辑路径维持历史引用，同 Writer 接缝的
                        // existing_merchant_id 语义）、输入文本精确命中在用
                        // 保司复用 / 未命中即建；重名陈旧态不再前端逐层兜
                        // 底，本壳保存时数据现读，无 store 缓存竞态面。
                        let insurer_id = match insurer_selected.clone() {
                            Some(id) => id,
                            None => {
                                let name = insurer_typed.trim().to_owned();
                                if name.is_empty() {
                                    return Err(AppError::coded(
                                        CODE_INSURER_NAME_REQUIRED,
                                        "保司名不能为空",
                                    ));
                                }
                                boot::create_insurer_by_name(db, &name)?
                            }
                        };
                        let mut input = input.clone();
                        input.insurer_id = insurer_id;
                        match editing.as_deref() {
                            Some(id) => {
                                boot::update_policy(db, id, input)?;
                                Ok(id.to_owned())
                            }
                            None => {
                                let policy_id = boot::create_policy(db, input.clone())?;
                                if let Some((product_name, agreement_input)) = agreement.clone() {
                                    boot::create_scheduled_plan(
                                        db,
                                        ledger_scheduled::CreateScheduledInput {
                                            policy_id: Some(policy_id.clone()),
                                            note: Some(product_name),
                                            ..agreement_input
                                        },
                                    )?;
                                }
                                Ok(policy_id)
                            }
                        }
                    }
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        close_form();
                        status.set(Some(if is_update {
                            MSG_SAVED.to_owned()
                        } else {
                            MSG_CREATED.to_owned()
                        }));
                        // 保单变化改变交易来源列反查与表单回填面——按旧壳
                        // ledger:changed 粗粒度语义广播交易失效；清单与统计
                        // 重读（重拉接线删除即红）。
                        signals::notify_transactions_invalidated();
                        reload();
                    }
                    Ok(Err(e)) => {
                        form_error.set(Some(format!("{MSG_SAVE_FAILED}: {e}")));
                        // 建档可能已落库而协议建档失败（批内前序保留，旧壳
                        // 同口径不回滚）：清单重读让已落库行即时可见。
                        reload();
                    }
                    Err(_) => form_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 协议区字段组与提交（添加 / 改价） ============================
    // 字段组校验与提交入参的单点（添加 / 改价共用同表字段——旧壳
    // PolicyAgreementFields reset / validate / build 的等价聚合；分配别名闭包
    // 保 follower 语义）。改价 = 取消旧协议 + 按新金额重建（订阅既有语义：
    // 取消成功而重建失败时旧段已停，可经「添加缴费协议」补建，不产生重复
    // 扣缴——旧壳 submit 同口径）。成功后重拉协议历史 + 清单统计（下期扣
    // 款面自然刷新）。
    let agreement_form_input = move || -> Result<ledger_scheduled::CreateScheduledInput, String> {
        let Some(account_id) = seg_account.get_untracked() else {
            return Err(MSG_ACCOUNT_REQUIRED.to_owned());
        };
        let Some(amount_cents) =
            assembly::amount_text_to_cents(&seg_amount.get_untracked()).filter(|cents| *cents > 0)
        else {
            return Err(MSG_AGREEMENT_AMOUNT_INVALID.to_owned());
        };
        let Some(recurrence_id) = seg_recurrence.get_untracked() else {
            return Err(MSG_AGREEMENT_AMOUNT_INVALID.to_owned());
        };
        let Ok(recurrence_type) = recurrence_id.parse::<ledger_scheduled::RecurrenceType>() else {
            return Err(MSG_AGREEMENT_AMOUNT_INVALID.to_owned());
        };
        let Some(interval) = seg_interval_text
            .get_untracked()
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|interval| *interval >= 1)
        else {
            return Err(MSG_AGREEMENT_AMOUNT_INVALID.to_owned());
        };
        let Some(currency_code) = seg_currency.get_untracked() else {
            return Err(MSG_AGREEMENT_AMOUNT_INVALID.to_owned());
        };
        let product_name = editing_id
            .get_untracked()
            .and_then(|id| row_of(&id))
            .map(|row| row.product_name)
            .unwrap_or_default();
        Ok(ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::Subscription,
            account_id,
            category_id: None,
            amount_cents,
            currency_code,
            recurrence_type,
            recurrence_interval: interval,
            recurrence_day: None,
            start_date: seg_start.get_untracked().to_string(),
            note: (!product_name.is_empty()).then_some(product_name),
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        })
    };
    let perform_agreement = move || {
        seg_error.set(None);
        status.set(None);
        let Some(policy_id) = editing_id.get_untracked() else {
            return;
        };
        let input = match agreement_form_input() {
            Ok(input) => input,
            Err(message) => {
                seg_error.set(Some(message));
                return;
            }
        };
        let is_rebuild = seg_mode.get_untracked().as_deref() == Some("rebuild");
        let old_plan_id = seg_old_plan_id.get_untracked();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                // 批内保单引用（blocking 闭包换手同款拷改两用，外层仍持
                // policy_id 承担重拉 / 分段刷新）。
                let batch_policy_id = policy_id.clone();
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        if is_rebuild {
                            // 改价第一跳：取消旧协议（旧段非 active 的守卫归
                            // 域内状态机；目标在场的不可达兑底码化拦场）。
                            let Some(old_id) = old_plan_id.clone() else {
                                return Err(AppError::coded(
                                    CODE_REBUILD_TARGET_MISSING,
                                    "内部错误：改价目标缺失",
                                ));
                            };
                            boot::update_scheduled_plan_status(
                                db,
                                &old_id,
                                ledger_scheduled::ScheduledStatus::Cancelled,
                            )?;
                        }
                        let mut input = input.clone();
                        input.policy_id = Some(batch_policy_id);
                        boot::create_scheduled_plan(db, input)
                    }
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        close_seg_form();
                        status.set(Some(if is_rebuild {
                            MSG_AGREEMENT_REBUILT.to_owned()
                        } else {
                            MSG_AGREEMENT_CREATED.to_owned()
                        }));
                        // 协议变化影响保单统计（下期扣款日）与订阅清单——
                        // 按旧壳 ledger:changed 族语义广播交易失效（协议不落
                        // 交易行，但本清单是交易失效订阅者，天然重读）。
                        signals::notify_transactions_invalidated();
                        reload();
                        load_segments(policy_id);
                    }
                    Ok(Err(e)) => seg_error.set(Some(format!("{MSG_AGREEMENT_FAILED}: {e}"))),
                    Err(_) => seg_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };
    // 无活跃段 → 添加（趸交 / 缴清保单随时可补建周期扣缴）；有活跃段 →
    // 改价（默认取首个活跃段）。
    let active_segment = move || -> Option<Segment> {
        let policy_id = editing_id.get_untracked()?;
        segments_of(&segments.get_untracked(), &policy_id)
            .into_iter()
            .find(|segment| segment.status == "active")
    };
    let open_add = move || {
        seg_amount.set(String::new());
        seg_currency.set(Some(base_currency.get_untracked()));
        seg_recurrence.set(Some("yearly".to_owned()));
        seg_interval_text.set("1".to_owned());
        seg_account.set(None);
        let start = editing_id
            .get_untracked()
            .and_then(|id| row_of(&id))
            .and_then(|row| date_select::parse_date_text(&row.start_date))
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        seg_start.set(start);
        seg_old_plan_id.set(None);
        seg_error.set(None);
        seg_mode.set(Some("add".to_owned()));
    };
    let open_rebuild = move || {
        let Some(old) = active_segment() else {
            return;
        };
        let old_id = old.id.clone();
        seg_amount.set(String::new());
        seg_currency.set(Some(old.currency_code.clone()));
        seg_recurrence.set(Some(old.recurrence_type.clone()));
        seg_interval_text.set(old.recurrence_interval.to_string());
        seg_account.set(Some(old.account_id.clone()));
        let fallback = date_select::parse_date_text(&old.start_date)
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        seg_start.set(fallback);
        seg_old_plan_id.set(Some(old_id.clone()));
        seg_error.set(None);
        seg_mode.set(Some("rebuild".to_owned()));
        // 新段起始日预填旧协议下期扣款日（最早 pending 期次，无 pending 回落
        // 旧起始日，旧壳 nextChargeDate 同语义；读失败不阻断，回落旧起始日）。
        view_owner.run(move || {
            spawn(async move {
                let detail_id = old_id.clone();
                let next = blocking(move || {
                    let guard = db_slot();
                    guard
                        .as_deref()
                        .and_then(|db| boot::scheduled_plan_detail(db, &detail_id).ok())
                        .and_then(|detail| {
                            detail
                                .occurrences
                                .iter()
                                .filter(|occurrence| occurrence.status == "pending")
                                .map(|occurrence| occurrence.scheduled_date.clone())
                                .min()
                        })
                })
                .await
                .ok()
                .flatten();
                if let Some(date_text) = next
                    && let Some(date) = date_select::parse_date_text(&date_text)
                {
                    seg_start.set(date);
                }
            });
        });
    };

    // ============================ 表格装配 ============================
    let columns = computed(move || {
        let width = window().width.0 - sidebar::SIDEBAR_WIDTH;
        policy_columns(width)
    });
    let row_classes = move || {
        let focused = highlight.get();
        rows.get()
            .iter()
            .map(|row| {
                if focused.as_deref() == Some(row.id.as_str()) {
                    POLICY_ROW_FOCUS_CLASS.to_owned()
                } else {
                    String::default()
                }
            })
            .map(SharedString::from)
            .collect::<Vec<SharedString>>()
    };
    let data_of = move || {
        let snapshot = rows.get();
        DataTableRows::new(snapshot.len())
            .texts("insurer", snapshot.iter().map(|r| r.insurer.as_str()))
            .texts(
                "product_name",
                snapshot.iter().map(|r| r.product_name.as_str()),
            )
            .texts(
                "policy_number",
                snapshot.iter().map(|r| r.policy_number.as_str()),
            )
            .texts("period", snapshot.iter().map(|r| r.period.as_str()))
            .texts("expiry", snapshot.iter().map(|r| r.expiry.as_str()))
            .texts("coverage", snapshot.iter().map(|r| r.coverage.as_str()))
            .texts("paid", snapshot.iter().map(|r| r.paid.as_str()))
            .texts("inflow", snapshot.iter().map(|r| r.inflow.as_str()))
            .texts(
                "next_charge",
                snapshot.iter().map(|r| r.next_charge.as_str()),
            )
            .texts("actions", snapshot.iter().map(|_| ""))
    };

    // 自绘格：险种列（聚焦行 test_id 锚点）、到期列（双态语义色——旧壳
    // NTag success / warning 的文本面对应形态）、操作列（编辑 + 删除气泡）
    // 与其余文本列。
    let cell_of = move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let assembled = rows.get_untracked().get(index).cloned();
        match cell.key.as_ref() {
            "product_name" => {
                let focused = assembled.as_ref().is_some_and(|row| {
                    highlight.get_untracked().as_deref() == Some(row.id.as_str())
                });
                let mut cell_view = view();
                if focused {
                    cell_view = cell_view.test_id(POLICY_ROW_FOCUS_ANCHOR);
                }
                cell_view
                    .child(
                        text(cell.text.clone())
                            .text_size(px(13.0))
                            .text_color(fg)
                            .number_of_lines(1),
                    )
                    .into_element()
            }
            "expiry" => {
                let expired = assembled
                    .as_ref()
                    .is_some_and(|row| row.expiry == EXPIRY_EXPIRED);
                text(cell.text.clone())
                    .text_size(px(13.0))
                    .text_color(if expired { warning_of() } else { success_of() })
                    .into_element()
            }
            "actions" => {
                let Some(row) = assembled else {
                    return view().into_element();
                };
                let id = row.id.clone();
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        button()
                            .app_class(
                                "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .on_press({
                                let id = id.clone();
                                move |_: &()| open_edit(id.clone())
                            })
                            .child(
                                text(ROW_EDIT)
                                    .text_size(px(12.0))
                                    .text_color(fg)
                                    .test_id(format!("policy-edit-{id}").as_str()),
                            ),
                    )
                    .child(
                        // 轻量确认气泡（ADR-0078 轻量档：旧壳 AppPopconfirm 同
                        // 形态——软删无恢复入口、历史引用保留，确认文案把去
                        // 向说清楚）。删除即红条目：删本 popconfirm 开合接线
                        // → headless「确认文案永不呈现」变红。
                        popconfirm::popconfirm(
                            "policy-delete",
                            {
                                let id = id.clone();
                                move || popconfirm_open.get().as_deref() == Some(id.as_str())
                            },
                            {
                                let id = id.clone();
                                move |shown: bool| {
                                    popconfirm_open.set(shown.then(|| id.clone()))
                                }
                            },
                            popconfirm::PopconfirmSpec {
                                message: DELETE_CONFIRM.to_owned(),
                                confirm_label: ROW_DELETE.to_owned(),
                                cancel_label: ROW_CANCEL.to_owned(),
                            },
                            // 触发件用纯视图（nsh-base Button 自含按压语义会
                            // 吞掉外层按压——按压源归外层包裹视图，items 先例
                            // 同注）。
                            view()
                                .app_class(
                                    "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(ROW_DELETE)
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id(format!("policy-delete-{id}").as_str()),
                                ),
                            {
                                let id = id.clone();
                                move || perform_delete(id.clone())
                            },
                        ),
                    )
                    .into_element()
            }
            _ => text(cell.text.clone())
                .text_size(px(13.0))
                .text_color(fg)
                .number_of_lines(1)
                .into_element(),
        }
    };

    // ============================ 编辑模式统计摘要 ============================
    let edit_summary_lines = move || -> Option<Vec<(&'static str, String)>> {
        let id = editing_id.get_untracked()?;
        let row = row_of(&id)?;
        let stat = policies
            .get()
            .stats
            .iter()
            .find(|stat| stat.policy_id == id)
            .cloned();
        Some(summary_lines(
            &row,
            stat.as_ref(),
            &currencies.get_untracked(),
            &today_text(),
        ))
    };

    // ============================ 编辑模式协议历史区 ============================
    // 该保单名下协议分段（价格分段真相含已取消段）+ 无活跃段「添加」与有活
    // 跃段「改价」+ 共字段组的内联表单（旧壳 PolicyAgreementSection 平价）。
    let has_active_segment = move || {
        // 响应式读：分段装载 / 重拉后「改价 ⇄ 添加」入口随活跃段在场重渲染。
        editing_id.get().is_some_and(|id| {
            segments_of(&segments.get(), &id)
                .iter()
                .any(|segment| segment.status == "active")
        })
    };
    let agreement_section = move || {
        // 分段表响应式展开：`each` 的行集闭包与空态 `when` 的谓词随协议历史
        // 装载 / 重拉重渲染（弹层内容体的直接读不进外层渲染作用域的追踪面
        // ——按旧壳 v-for/reactive 的组件级追踪形态走 each / when 闭包）。
        // 行集闭包自持（跳内 editing_id / segments 的 Copy 句柄，不借外层局
        // 部——'static 闭包纪律）。
        let rows_of_policy = move || {
            let policy_id = editing_id.get_untracked().unwrap_or_default();
            segments_of(&segments.get(), &policy_id)
        };
        let currencies_of_row = move || currencies.get_untracked();
        let rows_render = move |item: ReadSignal<Segment>, _index: ReadSignal<usize>| {
            let segment = item.get();
            let currencies_now = currencies_of_row();
            view()
                .test_id(format!("policy-seg-row-{}", segment.id).as_str())
                .child(
                    text(segment_row_text(&segment, &currencies_now))
                        .text_size(px(13.0))
                        .text_color(fg)
                        .number_of_lines(1),
                )
        };
        let rows = view()
            .flex_col()
            .gap_1()
            .test_id("policy-agreement-segments")
            .child(
                when(
                    move || !rows_of_policy().is_empty(),
                    move || {
                        each(
                            rows_of_policy,
                            |segment: &Segment| segment.id.clone(),
                            rows_render,
                        )
                    },
                )
                .otherwise(move || text(SEGMENTS_EMPTY).text_size(px(12.0)).text_color(muted)),
            );
        view()
            .flex_col()
            .gap_2()
            .app_class("rounded-md border border-border p-3")
            .child(
                text(AGREEMENT_SECTION)
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg),
            )
            .child(
                text(format!(
                    "{AGREEMENT_SEG_AMOUNT} · {AGREEMENT_SEG_RECURRENCE} · {AGREEMENT_SEG_START} · {AGREEMENT_SEG_STATUS}"
                ))
                .text_size(px(12.0))
                .text_color(muted),
            )
            .child(error_line(
                move || segments_error.get().is_some(),
                move || segments_error.get().unwrap_or_default(),
                danger_fg,
                "policy-agreement-error",
            ))
            .child(rows)
            .child(
                when(has_active_segment, move || {
                    button()
                        .app_class(
                            "px-2 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                        )
                        .on_press(move |_: &()| open_rebuild())
                        .child(
                            text(AGREEMENT_REBUILD)
                                .text_size(px(12.0))
                                .text_color(fg)
                                .test_id("policy-agreement-rebuild-open"),
                        )
                })
                .otherwise(move || {
                    button()
                        .app_class("px-2 py-1 rounded-md bg-primary hover:opacity-90")
                        .on_press(move |_: &()| open_add())
                        .child(
                            text(AGREEMENT_ADD)
                                .text_size(px(12.0))
                                .text_color(rgb(0xffffff))
                                .test_id("policy-agreement-add"),
                        )
                }),
            )
            .child(
                when(
                    move || seg_mode.get().is_some(),
                    move || {
                        let rebuilding =
                            seg_mode.get_untracked().as_deref() == Some("rebuild");
                        let mut inline = view().flex_col().gap_2().test_id("policy-sec-form");
                        if rebuilding {
                            inline = inline.child(
                                text(AGREEMENT_REBUILD_HINT)
                                    .text_size(px(12.0))
                                    .text_color(muted),
                            );
                        }
                        inline = inline
                            .child(agreement_fields(
                                "policy-sec",
                                "policy-sec-start",
                                fg,
                                AgreementDraft {
                                    amount: seg_amount,
                                    currency: seg_currency,
                                    recurrence: seg_recurrence,
                                    interval_text: seg_interval_text,
                                    account: seg_account,
                                    start: seg_start,
                                },
                                move || currencies.get_untracked().currency_options(),
                                move || {
                                    agreement_account_options(
                                        &accounts.get_untracked(),
                                        seg_account.get().as_deref(),
                                    )
                                },
                            ))
                            .child(error_line(
                                move || seg_error.get().is_some(),
                                move || {
                                    format!(
                                        "{MSG_AGREEMENT_FAILED}: {}",
                                        seg_error.get().unwrap_or_default()
                                    )
                                },
                                danger_fg,
                                "policy-sec-error",
                            ))
                            .child(modal_buttons(
                                "policy-sec",
                                if rebuilding {
                                    AGREEMENT_REBUILD_CONFIRM
                                } else {
                                    AGREEMENT_CREATE
                                },
                                fg,
                                close_seg_form,
                                perform_agreement,
                            ));
                        inline
                    },
                )
                .otherwise(view),
            )
    };

    // ============================ 表单弹层内容（静态字段 + 模式分支） ============================
    let form_content = move || {
        let editing = is_edit();
        let mut body = view()
            .flex_col()
            .gap_2()
            .child(form_row(
                LABEL_INSURER,
                muted,
                combo_select::combo_select(
                    "policy-form-insurer",
                    move || insurer_options(&insurers.get_untracked()),
                    form_insurer_typed,
                    form_insurer_selected,
                    PLACEHOLDER_INSURER,
                ),
            ))
            .child(text_field(
                "policy-form-number",
                LABEL_NUMBER,
                PLACEHOLDER_NUMBER,
                form_number,
                220.0,
                fg,
            ))
            .child(text_field(
                "policy-form-product",
                LABEL_PRODUCT,
                PLACEHOLDER_PRODUCT,
                form_product,
                220.0,
                fg,
            ))
            .child(form_row(
                LABEL_PERIOD,
                muted,
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(date_select::date_select("policy-form-start", form_start))
                    .child(text(PERIOD_DIVIDER).text_size(px(13.0)).text_color(muted))
                    .child(date_select::date_option_select("policy-form-end", form_end)),
            ))
            .child(
                // 保额（可选纯展示，不进任何金额口径——ADR-0051）与币种成对；
                // 清保额即清币种（成对校验在保存面收口；旧壳 disabled 视觉抑
                // 制的差异入交付报告）。
                form_row(
                    LABEL_COVERAGE,
                    muted,
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            input()
                                .test_id("policy-form-coverage")
                                .value(move || form_coverage_text.get())
                                .placeholder(PLACEHOLDER_COVERAGE)
                                .optional_label(LABEL_COVERAGE)
                                .text_color(fg)
                                .w(px(140.0))
                                .app_class(
                                    "bg-background rounded-md border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |text: &str| {
                                    form_coverage_text.set(text.to_owned())
                                }),
                        )
                        .child(select::app_select(
                            "policy-form-coverage-currency",
                            move || currencies.get_untracked().currency_options(),
                            form_coverage_currency,
                            PLACEHOLDER_CURRENCY,
                            false,
                        )),
                ),
            )
            .child(text_field(
                "policy-form-note",
                LABEL_NOTE,
                PLACEHOLDER_NOTE,
                form_note,
                240.0,
                fg,
            ));
        if editing {
            // 编辑模式 = 详情摘要（保单视角统计）与协议历史（分段真相）。
            if let Some(lines) = edit_summary_lines() {
                let anchors = [
                    "policy-stats-paid",
                    "policy-stats-inflow",
                    "policy-stats-next",
                    "policy-stats-expiry",
                ];
                let mut summary = view()
                    .test_id("policy-stats-summary")
                    .flex_row()
                    .gap_3()
                    .flex_wrap();
                for ((label, value), anchor) in lines.into_iter().zip(anchors) {
                    summary = summary.child(
                        view()
                            .flex_row()
                            .gap_1()
                            .items_center()
                            .child(text(label).text_size(px(12.0)).text_color(muted))
                            .child(
                                text(value)
                                    .text_size(px(13.0))
                                    .text_color(fg)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .test_id(anchor),
                            ),
                    );
                }
                body = body.child(summary);
            }
            body = body.child(agreement_section());
        } else {
            // 新建模式缴费协议区：可折叠开关（默认关 = 趸交 / 缴清纯档案）+
            // 字段组（随开关露出）。
            body = body
                .child(form_row(
                    AGREEMENT_SECTION,
                    muted,
                    button()
                        .app_class("px-2 py-1 rounded-md bg-primary hover:opacity-90")
                        .on_press(move |_: &()| {
                            with_agreement.update(|opened| *opened = !*opened);
                        })
                        .child(
                            text(move || {
                                if with_agreement.get() {
                                    AGREEMENT_ON
                                } else {
                                    AGREEMENT_OFF
                                }
                            })
                            .text_size(px(12.0))
                            .text_color(rgb(0xffffff))
                            .test_id("policy-agreement-toggle"),
                        ),
                ))
                .child(
                    text(AGREEMENT_TOGGLE_HINT)
                        .text_size(px(12.0))
                        .text_color(muted)
                        .test_id("policy-agreement-hint"),
                )
                .child(
                    when(
                        move || with_agreement.get(),
                        move || {
                            agreement_fields(
                                "policy-agreement",
                                "policy-agreement-start",
                                fg,
                                AgreementDraft {
                                    amount: agr_amount,
                                    currency: agr_currency,
                                    recurrence: agr_recurrence,
                                    interval_text: agr_interval_text,
                                    account: agr_account,
                                    start: agr_start,
                                },
                                move || currencies.get_untracked().currency_options(),
                                move || {
                                    agreement_account_options(
                                        &accounts.get_untracked(),
                                        agr_account.get().as_deref(),
                                    )
                                },
                            )
                        },
                    )
                    .otherwise(view),
                );
        }
        body = body
            .child(error_line(
                move || form_error.get().is_some(),
                move || {
                    format!(
                        "{MSG_SAVE_FAILED}: {}",
                        form_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "policy-form-error",
            ))
            .child(modal_buttons(
                "policy-form",
                "保留",
                fg,
                close_form,
                perform_save,
            ));
        body
    };

    // 评论（视图树）。版本所需注释：旧壳 PoliciesView.vue 的重生。
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::Policies.label())
                    .text_size(px(16.0))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(error_line(
            move || error.get().is_some(),
            move || format!("加载失败：{}", error.get().unwrap_or_default()),
            danger_fg,
            "policy-error",
        ))
        .child(
            // 内容区（储蓄目标视图同形态的字段卡片列；表格虚拟化按自身高度
            // 裁行——根内容列必须 flex_1，卡片弹性高才有确定高来源，#2041 先
            // 例）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .child(
                    // 顶部新建行（旧壳卡片内右对齐同位；新建唯一入口）。
                    view().flex_row().justify_end().child(
                        button()
                            .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                            .on_press(move |_: &()| open_create())
                            .child(
                                text(NEW_BUTTON)
                                    .text_size(px(13.0))
                                    .text_color(rgb(0xffffff))
                                    .test_id("policy-new"),
                            ),
                    ),
                )
                .child(
                    // 清单卡片（卡片自身 flex_1——虚拟化表格取弹性链高度，卡
                    // 片不定高则虚拟化量出零高，先例同注）。
                    view()
                        .flex_1()
                        .flex_col()
                        .gap_2()
                        .app_class("rounded-md border border-border p-3")
                        .child(
                            view().flex_1().child(
                                text(LIST_CARD_TITLE)
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            ),
                        )
                        .child(
                            view().test_id("policy-table").flex_1().child(
                                data_table()
                                    .columns(columns)
                                    .data(data_of)
                                    .row_classes(row_classes)
                                    .render_cell(cell_of)
                                    .loading(move || loading.get())
                                    .loading_view(text(LOADING_PLACEHOLDER).text_color(muted))
                                    .empty(
                                        text(EMPTY_GUIDE)
                                            .text_color(muted)
                                            .test_id("policy-empty-guide"),
                                    )
                                    .classes(table_classes())
                                    .flex_1(),
                            ),
                        ),
                )
                .child(
                    // 操作反馈条位（成功/失败共用一条；新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(fg)
                                .test_id("policy-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(modal::app_modal(
                    move || form_open.get(),
                    move || {
                        if is_edit() {
                            FORM_TITLE_EDIT.to_owned()
                        } else {
                            FORM_TITLE_CREATE.to_owned()
                        }
                    },
                    close_form,
                    form_content,
                )),
        )
} // ---------------------------------------------------------------------------
// 缴费协议字段组（旧壳 PolicyAgreementFields 的对应形态：每期金额 + 扣款频
// 率（每 N {unit}）+ 扣款账户（拼音可搜）+ 起始日；头部锚点由调用方传入，
// 新建区 / 协议历史区两消费面共用同本件）。
// ---------------------------------------------------------------------------
/// 缴费协议草稿信号束（字段组两消费面共用的持有面——信号归调用方，本件
/// 零状态）。
struct AgreementDraft {
    amount: Signal<String>,
    currency: Signal<Option<String>>,
    recurrence: Signal<Option<String>>,
    interval_text: Signal<String>,
    account: Signal<Option<String>>,
    start: Signal<chrono::NaiveDate>,
}

#[allow(clippy::too_many_arguments)]
fn agreement_fields(
    prefix: &'static str,
    start_id: &'static str,
    fg: AdaptiveColor,
    draft: AgreementDraft,
    currency_options: impl Fn() -> Vec<assembly::CategoryOption> + Copy + 'static,
    account_options: impl Fn() -> Vec<assembly::CategoryOption> + Copy + 'static,
) -> View {
    let AgreementDraft {
        amount,
        currency,
        recurrence,
        interval_text,
        account,
        start,
    } = draft;
    view()
        .flex_col()
        .gap_2()
        .child(form_row(
            AGREEMENT_AMOUNT,
            muted_of(),
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    input()
                        .test_id(format!("{prefix}-amount").as_str())
                        .value(move || amount.get())
                        .placeholder(AGREEMENT_AMOUNT_PLACEHOLDER)
                        .optional_label(AGREEMENT_AMOUNT)
                        .text_color(fg)
                        .w(px(140.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |text: &str| amount.set(text.to_owned())),
                )
                .child(select::app_select(
                    std::borrow::Cow::Owned(format!("{prefix}-currency")),
                    currency_options,
                    currency,
                    PLACEHOLDER_CURRENCY,
                    false,
                )),
        ))
        .child(form_row(
            AGREEMENT_RECURRENCE,
            muted_of(),
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(text(EVERY_WORD).text_size(px(13.0)).text_color(fg))
                .child(
                    input()
                        .test_id(format!("{prefix}-interval").as_str())
                        .value(move || interval_text.get())
                        .optional_label(AGREEMENT_RECURRENCE)
                        .text_color(fg)
                        .w(px(56.0))
                        .app_class("bg-background rounded-md border border-border px-2 py-1")
                        .on_change(move |text: &str| interval_text.set(text.to_owned())),
                )
                .child(select::app_select(
                    std::borrow::Cow::Owned(format!("{prefix}-recurrence")),
                    crate::scheduled::recurrence_options,
                    recurrence,
                    PLACEHOLDER_RECURRENCE,
                    false,
                )),
        ))
        .child(form_row(
            AGREEMENT_ACCOUNT,
            muted_of(),
            pinyin_select::pinyin_select(
                format!("{prefix}-account"),
                account_options,
                account,
                AGREEMENT_ACCOUNT_PLACEHOLDER,
                false,
            ),
        ))
        .child(form_row(
            AGREEMENT_START_DATE,
            muted_of(),
            date_select::date_select(start_id, start),
        ))
}
