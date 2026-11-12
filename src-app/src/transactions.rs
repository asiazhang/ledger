//! 交易表格视图与表格装配共用件族（issue #2018 首屏，#2073 自 app.rs 归位）。
//!
//! 视图本体 [`TransactionsView`]：DataTable 渲染真实交易数据、分页条点击翻页
//! （100 行/页服务端 offset 分页，走域读路径）、搜索/筛选/删除确认/加入物品
//! 与交易表单接线。共用件族是「行集进表格」的呈现半边（`app.rs` 立桩期与根
//! 组件同址，消费面遍及各平表视图）：行装配消费 [`rows_to_data`]、自绘格
//! [`render_cell`]（交互载荷经 [`CellActions`] 分发）、列定义 [`table_columns`]
//! 与弹性列宽 [`elastic_columns`]、部件主题 [`table_classes`]、金额语义色
//! [`amount_color`]、金额隐私开关 [`amount_privacy_enabled`]（设备轻量设置项
//! 读侧）与搜索框共用常量。文案仅 zh-CN（spec #2015）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::Arc;
use std::time::Duration;

use ledger_infra::error::AppError;
use nsh_base::{
    DataTableAlign, DataTableCell, DataTableClasses, DataTableColumn, DataTableContextMenu,
    DataTableRows, PopoverEvent, button, data_table, input, pagination, popover,
};
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;

use crate::app::db_slot;
use crate::assembly::{
    self, AccountAtom, AccountAtoms, AccountTable, AmountSemantic, ColumnAlign, CurrencyTable,
    MerchantTable, RowCells, RowContext, SourceJump,
};
use crate::boot;
use crate::items::AddItemTarget;
use crate::nav_model;
use crate::{
    backup_entry, context_menu, danger, investments_form, items, multi_select, nav, overlay,
    period_row, pinyin_select, signals, tx_filter, tx_view_preferences, txn_form,
};

/// 金额隐私模式开关的存储键（设备轻量设置项，与旧壳同键同值形态：旧壳
/// `@ledger/money` 的 `AMOUNT_PRIVACY_STORAGE_KEY`，值为 JSON 布尔字面量
/// `"true"` / `"false"`）。轻量设置项域语义见
/// docs/contexts/CONTEXT-reference-settings.md「金额隐私模式」。
const AMOUNT_PRIVACY_STORAGE_KEY: &str = "amount_privacy_enabled";

/// 金额隐私开关的设备态读取（组件任务内同步读；解析镜像旧壳 `loadLocal`
/// 的 JSON 布尔语义：缺失/非法归默认关）。立桩无设置面——开关的写入口随
/// 设置面票接入（旧壳入口：侧栏标题行眼睛图标），本壳消费同一域来源、
/// 同一存储键。crate 内消费面：交易/预算/概览视图的装配入口（#2039）。
pub(crate) fn amount_privacy_enabled() -> bool {
    nsh_core::storage::Storage::get_item(AMOUNT_PRIVACY_STORAGE_KEY)
        .is_some_and(|raw| raw.trim() == "true")
}

/// 装载批次读侧载荷（[`assembly::RowContext`] 的读侧对称形态）：一次页读同
/// 批取回的行集、总数与装配参考数据束。行集先落位、参考数据与边界逐臂补齐
/// 的线穿以结构承载——新增同批读维度改本结构一处，不再嵌套扩元组。
struct PageRead {
    /// 本页交易行集（列表态与搜索态同一字段，搜索态即命中页）。
    items: Vec<ledger_transaction::Transaction>,
    /// 未删除物品行集（#2044 「加入物品」行菜单项的溯源唯一准入判定面；
    /// 装载批同批读取，物品写后由失效信号广播驱动重读）。
    item_rows: Vec<ledger_item::ItemWithDailyCost>,
    /// 满足当前口径的总数（页与总数共用同一 WHERE）。
    total: i64,
    /// 数据期间边界（读失败退化为 None＝不钳制，旧壳 silent 同口径）。
    date_range: Option<ledger_reports::DateRange>,
    /// 币种字典行（金额展示参数的来源）。
    currency_rows: Vec<ledger_currencies::Currency>,
    /// 在用分类行（分类路径表来源）。
    category_rows: Vec<ledger_categories::Category>,
    /// 在用且未隐藏账户行（账户格与借贷派生来源）。
    pub(crate) account_rows: Vec<ledger_accounts::Account>,
    /// 含软删全量商户行（商户格来源）。
    pub(crate) merchant_rows: Vec<ledger_merchants::Merchant>,
    /// 未删除保单行（#2034 表单保单选项面）。
    pub(crate) policy_rows: Vec<ledger_policy::Policy>,
}

impl PageRead {
    /// 行集与总数先落位，参考数据与边界随后逐臂补齐（币种读失败即整批
    /// 失败，边界读失败保 None 不拖垮页呈现——顺序与错误传播语义不变）。
    fn new(items: Vec<ledger_transaction::Transaction>, total: i64) -> Self {
        Self {
            items,
            item_rows: Vec::new(),
            total,
            date_range: None,
            currency_rows: Vec::new(),
            category_rows: Vec::new(),
            account_rows: Vec::new(),
            merchant_rows: Vec::new(),
            policy_rows: Vec::new(),
        }
    }
}

/// 金额语义 → 主题令牌键（色型闭集住装配层 [`assembly::AmountSemantic`]，
/// 键名闭集住本函数；色值住 theme.json 令牌表——issue #2100 收编后本模块
/// 零本地色常量）。
fn amount_token_name(semantic: AmountSemantic) -> &'static str {
    match semantic {
        AmountSemantic::Expense => "expense",
        AmountSemantic::Income => "income",
        AmountSemantic::Refund => "refund",
        AmountSemantic::Transfer => "transfer",
        AmountSemantic::Buy => "buy",
        AmountSemantic::Sell => "sell",
        AmountSemantic::Convert => "convert",
        AmountSemantic::Split => "split",
        AmountSemantic::Dividend => "dividend",
    }
}

/// 金额语义色（主题令牌消费，亮/暗两态自适应；读取经
/// [`crate::theme::color`] 单点，缺失落可读兜底）。
pub(crate) fn amount_color(semantic: AmountSemantic) -> AdaptiveColor {
    crate::theme::color(amount_token_name(semantic))
}

/// 总数文案（与旧壳 zh-CN 词表同文案：transactions.list.total）。
fn total_text(total: i64) -> String {
    format!("共 {total} 条")
}

/// 搜索态总数文案（与旧壳 zh-CN 词表同文案：search.hitCount；列表态仍用
/// [`total_text`]——搜索语义变了，计数词随之换，与旧壳两词并存同形）。
pub(crate) fn hit_total_text(total: i64) -> String {
    format!("命中 {total} 条")
}

/// 搜索输入防抖时长（毫秒）：与旧壳 `SEARCH_DEBOUNCE_MS`（issue #1402「搜索
/// 输入防抖」跨域不变量）同值同语义——等待成功优于即刻失败（AGENTS.md 体验
/// 取舍），快输入不逐键打库。交易列表与搜索视图（#2040）同一时长单源。
pub(crate) const SEARCH_DEBOUNCE_MS: u64 = 300;

/// 搜索框占位文案（与旧壳 search.keywordPlaceholder 同文案）。
pub(crate) const SEARCH_PLACEHOLDER: &str =
    "输入关键字开始搜索（备注、账户名、商户名，支持多关键字）";

/// 装配行集 → 宿主表格数据（按列装配，渲染期零格式化——格文本已是装配产物）。
pub(crate) fn rows_to_data(rows: &[RowCells]) -> DataTableRows {
    DataTableRows::new(rows.len())
        .texts("date", rows.iter().map(|r| r.date.as_str()))
        .texts("kind", rows.iter().map(|r| r.kind.as_str()))
        .texts("category", rows.iter().map(|r| r.category.as_str()))
        .texts("merchant", rows.iter().map(|r| r.merchant.as_str()))
        .texts("account", rows.iter().map(|r| r.account.as_str()))
        .texts("source", rows.iter().map(|r| r.source.as_str()))
        .texts("note", rows.iter().map(|r| r.note.as_str()))
        .texts("amount", rows.iter().map(|r| r.amount.as_str()))
}

/// 来源格跳转分发闭包（载荷形态别名，消 type-complexity）。
type SourceJumpDispatch = Arc<dyn Fn(ledger_transaction::TransactionSourceKind, &str)>;

/// 表格级行为委托处理器闭集（#2061 的分发端；旧壳 transaction-table-actions
/// 的本壳对应形态——装配行带交互载荷，自绘格按格键命中、载荷分发）：视图
/// 装配一次传入 [`render_cell`]；只读消费方（搜索视图，模块注「先平价只读」）
/// 传 None——自绘格退回纯文本形态，不渲染可点样式、不挂空接缝。
#[derive(Clone)]
pub(crate) struct CellActions {
    /// 账户原子下钻（账户 id）。
    pub account: Arc<dyn Fn(&str)>,
    /// 商户下钻（商户 id）。
    pub merchant: Arc<dyn Fn(&str)>,
    /// 来源跳转（来源种类 + 实体 id）。
    pub source: SourceJumpDispatch,
    /// 备注复制（备注原文）。
    pub copy_note: Arc<dyn Fn(&str)>,
}

/// 可点原子色（主题强调色；accounts/merchants 可点格先例同源，primary
/// 令牌经 [`crate::theme::color`] 单点消费）。
fn accent_color() -> AdaptiveColor {
    crate::theme::color("primary")
}

/// 来源种类 → 导航域种类（装配载荷是域闭集，跳转目标计算吃导航域闭集——
/// 映射归 UI 层，与列对齐枚举映射同形）。
fn source_nav_kind(kind: ledger_transaction::TransactionSourceKind) -> nav_model::SourceKind {
    match kind {
        ledger_transaction::TransactionSourceKind::InstallmentPlan => {
            nav_model::SourceKind::InstallmentPlan
        }
        ledger_transaction::TransactionSourceKind::Subscription => {
            nav_model::SourceKind::Subscription
        }
        ledger_transaction::TransactionSourceKind::ScheduledTransfer => {
            nav_model::SourceKind::ScheduledTransfer
        }
        ledger_transaction::TransactionSourceKind::Policy => nav_model::SourceKind::Policy,
        ledger_transaction::TransactionSourceKind::Item => nav_model::SourceKind::Item,
        ledger_transaction::TransactionSourceKind::Instrument => nav_model::SourceKind::Instrument,
    }
}

/// 自绘格（金额语义色，issue #2019；格级交互格，#2061）：金额列按行语义色型
/// 着色（旧壳金额列着色口径），账户/商户/来源/备注四格在委托处理器在场时按
/// 装配载荷渲染可点原子（accent 色文本 + test_id + on_press 分发——accounts
/// 视图可点格先例同形），其余列平文——格文本仍取自数据（`cell.text`），本函数
/// 只补色与交互。只读消费方（actions None）或载荷缺席 → 纯文本形态（与既有
/// 呈现零差异）。掩码态照常着色（掩码隐藏数字，不隐藏方向，旧壳隐私保留项
/// 同口径）。交易列表（交互）与搜索视图（只读）共用。
pub(crate) fn render_cell(
    rows_signal: Signal<Vec<RowCells>>,
    actions: Option<CellActions>,
) -> impl Fn(&DataTableCell) -> nsh_core::element::AnyElement {
    let accent = accent_color();
    move |cell: &DataTableCell| {
        // 交互格（#2061）：委托处理器在场且装配行带载荷才走可点形态；命中
        // 失败（只读视图 / 占位「-」/ 续行空格）落回纯文本路径。
        if let Some(actions) = actions.as_ref()
            && let Some(element) = interactive_cell(cell, &rows_signal, actions, accent)
        {
            return element;
        }
        let semantic = rows_signal
            .get()
            .get(cell.row as usize)
            .map(|r| r.amount_semantic);
        // 末端对齐列文本右对齐：表格对自绘格返回元素强制 `.flex_1()`（撑满
        // 整格），格容器的 `justify_end`（列 align=End）随之失效——文本贴格
        // 左沿，金额列读感为「列过宽」。文本对齐在自绘侧补齐（列头走组件
        // 缺省路径，不受此影响）。
        let align_end = assembly::COLUMNS
            .iter()
            .find(|def| cell.key == def.key)
            .is_some_and(|def| def.align == ColumnAlign::End);
        // 单行截断（对齐 nsh-base 默认格路径的单行约束，view.rs 默认分支
        // `.number_of_lines(1)`；自绘格路径组件只补 flex/min_w，不自动加）：
        // DataTable 固定行高虚拟滚动，长文本自动换行即溢出行框，叠到相邻行
        // 难以阅读（首屏实测：来源列派生文本与下一行重叠）。截断语义随组件
        // 缺省：单行、末端省略号。
        let label = text(cell.text.clone()).number_of_lines(1);
        let label = if align_end { label.text_right() } else { label };
        match semantic {
            Some(semantic) if cell.key == "amount" => {
                label.text_color(amount_color(semantic)).into_element()
            }
            _ => label.into_element(),
        }
    }
}

/// 交互格渲染（#2061）：命中可点载荷 → 可点原子元素（accent 色文本 +
/// test_id + on_press 分发；accounts 视图名称列先例同形）；未命中（该格无
/// 载荷：占位「-」/ 续行空格）返回 None → 纯文本路径。单行截断与纯文本路径
/// 同纪律（虚拟滚动固定行高，长文本不换行）。
fn interactive_cell(
    cell: &DataTableCell,
    rows_signal: &Signal<Vec<RowCells>>,
    actions: &CellActions,
    accent: AdaptiveColor,
) -> Option<nsh_core::element::AnyElement> {
    let row = rows_signal.get().get(cell.row as usize)?.clone();
    // 分发闭集按格键取 Arc 克隆（on_press 闭包 'static——挂到元素随存活）。
    let dispatch = actions.clone();
    // 账户原子元素：带 id（可下钻）= accent 色可点件；占位「-」= 平文。
    let account_atom_element = |atom: &AccountAtom| -> nsh_core::element::AnyElement {
        match &atom.account_id {
            Some(id) => {
                let id = id.clone();
                let account = dispatch.account.clone();
                view()
                    .test_id(format!("tx-cell-account-{id}").as_str())
                    .on_press(move |_: &PressEvent| account(&id))
                    .child(
                        text(atom.text.clone())
                            .text_color(accent)
                            .number_of_lines(1),
                    )
                    .into_element()
            }
            None => text(atom.text.clone()).number_of_lines(1).into_element(),
        }
    };
    match cell.key.as_ref() {
        "account" => {
            let atoms = row.account_atoms?;
            Some(match atoms {
                // 单占位（无 id）不走交互形态：落回纯文本路径（与既有呈现零差异）。
                AccountAtoms::Single(ref atom) if atom.account_id.is_none() => return None,
                atoms => match atoms {
                    AccountAtoms::Single(atom) => account_atom_element(&atom),
                    AccountAtoms::Two { from, to } => view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(account_atom_element(&from))
                        .child(text("→").number_of_lines(1))
                        .child(account_atom_element(&to))
                        .into_element(),
                    AccountAtoms::Funding { first, marker } => view()
                        .flex_row()
                        .items_center()
                        .child(account_atom_element(&first))
                        .child(text(marker).number_of_lines(1))
                        .into_element(),
                },
            })
        }
        "merchant" => {
            let id = row.merchant_id?;
            let press_id = id.clone();
            let merchant = dispatch.merchant.clone();
            Some(
                view()
                    .test_id(format!("tx-cell-merchant-{id}").as_str())
                    .on_press(move |_: &PressEvent| merchant(&press_id))
                    .child(
                        text(cell.text.to_string())
                            .text_color(accent)
                            .number_of_lines(1),
                    )
                    .into_element(),
            )
        }
        "source" => {
            let jump = row.source_jump?;
            let SourceJump {
                kind,
                entity_id,
                name,
                status,
            } = jump;
            let press_entity = entity_id.clone();
            let source = dispatch.source.clone();
            Some(
                view()
                    .flex_row()
                    .items_center()
                    .child(
                        view()
                            .test_id(format!("tx-cell-source-{entity_id}").as_str())
                            .on_press(move |_: &PressEvent| source(kind, &press_entity))
                            .child(text(name).text_color(accent).number_of_lines(1)),
                    )
                    // 状态标注不可点（文本尾段，旧壳状态徽章平铺镜像同位）。
                    .children(status.map(|label| text(format!(" {label}")).number_of_lines(1)))
                    .into_element(),
            )
        }
        "note" => {
            let note = row.copy_note?;
            let press_note = note;
            let copy_note = dispatch.copy_note.clone();
            Some(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(text(cell.text.to_string()).number_of_lines(1))
                    .child(
                        view()
                            .test_id("tx-cell-copy-note")
                            .optional_label("复制备注")
                            .on_press(move |_: &PressEvent| copy_note(&press_note))
                            .child(text("复制").text_xs().text_color(accent)),
                    )
                    .into_element(),
            )
        }
        _ => None,
    }
}

/// 列定义映射（装配层产物 → 宿主列）：UI 层只做映射、不产列。
fn table_columns() -> Vec<DataTableColumn> {
    assembly::COLUMNS
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
        .collect()
}

/// 弹性列宽分配：列组件无弹性列机制（列宽恒取建议值，总宽不足行宽时右侧
/// 留空——首屏实测金额列右侧约 1/3 空黑，读感「金额列过宽」），在应用侧
/// 分配：富余宽度全给备注列（旧壳备注列即弹性列，[`assembly::COLUMNS`]
/// 注释同口径），其余列按装配建议宽度。`width` 是表格可用宽（pt，取窗口
/// 内容宽——导航骨架起扣侧栏宽）；窄于建议总宽时全列取建议值（横向滚动语义
/// 不变）。
pub(crate) fn elastic_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns = table_columns();
    let suggested: f32 = assembly::COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - suggested).max(0.0);
    if extra > 0.0
        && let Some(note) = columns.iter_mut().find(|col| col.key == "note")
        && let Some(w) = note.width
    {
        note.width = Some(Points(w.0 + extra));
    }
    columns
}

/// 表格部件主题（Tailwind 类取主题令牌，跟随系统深浅色）：文字色逐部件显式
/// 带色（裸 text 默认前景黑，暗色桌面不可见——#2017 先例），行悬停与空态取
/// 弱化面。交易列表与搜索视图（#2040）共用同一套表格映射（列集 / 自绘格 /
/// 弹性列宽 / 部件主题）。
pub(crate) fn table_classes() -> DataTableClasses {
    let mut classes = DataTableClasses::default();
    classes.root = Some("bg-background text-foreground".into());
    // 单元格水平留白（组件缺省零 padding，金额列值贴窗口右缘、日期列贴左
    // 缘——首屏实测）：列头与格各 px-3，两侧呼吸感同源。
    classes.header_cell = Some("text-muted-foreground px-3".into());
    classes.cell = Some("text-foreground px-3".into());
    classes.row_hover = Some("bg-accent".into());
    classes.empty = Some("text-muted-foreground".into());
    classes
}

/// 交易表格首屏（issue #2018）：DataTable 渲染真实交易数据 + 分页条点击翻页
/// （100 行/页服务端 offset 分页，走域读路径）+ 忙碌指示 + 空态。
/// 时间维度行（issue #2021 / ADR-0141）：期间步进器 `< 期间标签 >` + 期间
/// 选择弹层。期间状态 = 日期区间快照（None = 「全部」默认态，与旧壳同）；
/// 步进/芯片换算与钳制归 [`period`] 纯决策单点，UI 只消费；翻页与周期步进
/// 两族快捷键（ADR-0140/0141 裸键 `[` `]` `,` `.`）语义等同点击对应控件、
/// 共用 Overlay Suppression 闸门（[`overlay`]，弹层开着不派发；可编辑焦点
/// 闸由引擎 keymap `When::Auto` 承担——四键皆为文本编辑键，输入焦点期间不
/// 派发；桌面立桩无触控轴）。
#[component]
pub(crate) fn TransactionsView() -> impl IntoElement {
    let total = signal(0i64);
    let rows = signal(Vec::<RowCells>::new());
    let loading = signal(true);
    let error = signal(None::<String>);
    // 响应式列集：表格可用宽取窗口内容宽（导航骨架起扣侧栏宽；ambient::window
    // 随窗口缩放重算）——备注列吸收富余宽（弹性列，旧壳同语义），其余列按
    // 装配建议宽度。
    let columns = computed(move || elastic_columns(crate::adaptive::content_width()));
    // 搜索态：输入文本（受控）、结果呈现态（旧壳 searched 语义——搜索结果落位
    // 后才切搜索态呈现，键入中底栏仍呈上一装载态）、防抖调度序号（新输入作废
    // 在途定时器；金额区间输入共用同一防抖——旧壳搜索页单防抖同形）与装载序号
    //（后发作废先发，与旧壳 Loadable invalidate 同义：竞态后发覆盖先发，迟到
    // 结果不落位）。
    let search_text = signal(String::new());
    let searched = signal(false);
    let search_seq = signal(0u64);
    let load_seq = signal(0u64);
    // 筛选会话态（#2037）：进程级单例信号束（会话内保留——视图卸载重挂不丢，
    // 旧壳 ADR-0094 同语义；零写盘，冷启动回默认）。金额区间输入属搜索面维度
    //（旧壳搜索页同口径），组件本地态不进会话保留；文本元 → 分换算在装载时机
    //（[assembly::amount_text_to_cents] 换算家族单点）。
    let filters = tx_filter::store();
    // 隐藏投资相关流水（#2062 / ADR-0136 视图偏好）：设备级持久行集裁决的
    // 会话镜像（真源在存储，读写单点归 [`crate::tx_view_preferences`]；不入
    // 筛选会话态——清除筛选与 ESC 复位不触达）。列表态装载时机读值（搜索
    // 态不携带：域搜索读路径无此维度，ADR-0136 决策 4 搜索面不动）。
    let hide_investment = tx_view_preferences::store();
    let amount_min_text = signal(String::new());
    let amount_max_text = signal(String::new());
    // 行删除态（issue #2022）：待确认删除的交易 id（Some = 危险确认对话框开）
    // 与删除失败呈现（码化错误文案，不静默；下次删除尝试开始时清空）。
    let confirm_target = signal(None::<String>);
    let delete_error = signal(None::<String>);
    // 备注复制反馈（#2061）：成功/失败两态文案（None = 无反馈；ai_prompt 先例
    // 同形——旧壳 toast 的条位等价，#2038 裁决）。
    let copy_feedback = signal(None::<String>);
    // 物品快照（#2044）：装载批同批读取——「加入物品」行菜单项的溯源唯一
    // 准入判定面（hasItem 按溯源指针比对，旧壳 items store 同口径）。
    let items_snap = signal(items::ItemsSnap::default());
    // 加入物品弹窗目标交易（None = 不呈现）与成功反馈条位（NSH 无 toast 的
    // 条位对应物）。
    let add_item_target = signal(AddItemTarget::default());
    let item_feedback = signal(None::<String>);
    // 交易表单族（#2034）：表单信号束与参考数据源（与列表装载共享同一读批；
    // 编辑回填锚点：当前页原始行集——弹窗开合与字段状态住表单信号束）。
    let form = txn_form::TxnFormState::new();
    let tx_items = signal(txn_form::TxItems(Vec::new()));
    let form_accounts = signal(txn_form::FormAccounts(Vec::new()));
    let form_categories = signal(txn_form::FormCategories(Vec::new()));
    let form_merchants = signal(txn_form::FormMerchants(Vec::new()));
    let form_policies = signal(txn_form::FormPolicies(Vec::new()));
    let form_currencies = signal(assembly::CurrencyTable::default());
    // 投资记账表单·买卖族（#2046）：表单信号束与参考数据源独立成束（旧壳
    // InvestmentForm 挂 TransactionForm 的同面本壳分件；明细取数随表单自身
    // 装载批次，不入列表读批——明细域数据不同源）。入口归属见 investments_form
    // 模块头：买入/卖出行编辑入口住户投资明细页签（⑯ 落位装配），此处挂载
    // 弹窗 + 行菜单分流接缝先行（行集闭集不含 buy/sell 的死面注释保留）。
    let trade_form = investments_form::TradeFormState::new();
    let trade_sources = investments_form::TradeFormSources::new();
    // 「记一笔」类型菜单弹层开合（入口单点表达 kind：弹层两项——记支出/
    // 记收入；弹层开合上报 Overlay Suppression，期间弹层同形）。
    let add_menu_open = signal(false);
    let add_menu_token = overlay::OverlayToken::new();
    // 备份最小入口（#2038）：入口状态束挂视图实例（工具区按钮 + 反馈状态
    // 条 + 覆盖确认对话框共用同一信号束，backup_entry 模块注）。
    let backup = backup_entry::BackupEntryState::new();
    // 时间维度行状态：期间快照住筛选会话态（#2037 起，旧壳 dateFrom/dateTo 同
    // 维度的会话内保留）；数据期间边界原始日期对（None = 在途/失败 → 钳制退化
    // 为不钳制，旧壳 silent 同口径）；快照区间是唯一事实源，游标不落状态（步进
    // 前从当前区间反推，ADR-0141 决策 2）。
    let boundary_dates = signal(None::<(Option<String>, Option<String>)>);
    // 视图实例 owner（组件体运行时的 current）：删除任务经它挂到组件实例，
    // 不随对话框子树卸载被取消（见 perform_delete 注）。
    let view_owner = Owner::current();

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let crate::views::ViewPalette {
        fg,
        muted,
        danger_fg,
    } = crate::views::view_palette();
    // 加入物品成功反馈条位的前景色（#2044；与列表前景同源）。
    let item_feedback_fg = fg;

    // 装载一页（首屏、翻页、换期间、筛选与搜索共用一条路径）：判态——关键词
    // 或金额区间任一在场 = 搜索态直调域统一模糊搜索（与旧壳 `search_transactions`
    // 命令同一域函数，不经 IPC；携金额区间与期间快照），否则 = 列表态读交易页
    //（携筛选全维度 + 期间快照，默认 kind 收窄）。两态分页与总数语义同旧壳
    //（页码重置、服务端 offset 分页、total = 满足口径的总数）。阻塞读走后台
    // 线程（SQLite IO 不占引擎线程）；分页条装载期间禁用（单飞——等待成功优于
    // 即刻失败，无并发页读交错）。币种字典、在用分类、可见账户、含软删商户、
    // 未删除保单与行集同批读取：字典读失败不静默降级成无符号金额（用户无感知
    // 的错示比暂缓呈现更差），与行集一并走错误呈现。数据期间边界读失败不拖垮
    // 页呈现：钳制退化为不钳制（旧壳 silent 同口径），保留旧值到下次成功替换。
    // 表单参考数据源与原始行集随装载一并落位（#2034：编辑回填与选项面的同一
    // 事实源）。
    let load = move |page_no: u32| {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let query = search_text.get().trim().to_owned();
        let amount_min = assembly::amount_text_to_cents(&amount_min_text.get());
        let amount_max = assembly::amount_text_to_cents(&amount_max_text.get());
        // 搜索态判据：关键词或金额区间任一在场（旧壳搜索页 hasQuery 同口径）；
        // 期间不判态——列表态的期间过滤照旧，搜索态下期间对命中面生效（搜索 ×
        // 期间裁决，#2037）。筛选维度快照在调用时机取值（装载即以当下筛选态
        // 读页——与旧壳 load 装配请求参数同形）。
        let searching = !query.is_empty() || amount_min.is_some() || amount_max.is_some();
        let state = filters.snapshot();
        let privacy = amount_privacy_enabled();
        // 视图偏好值在引擎线程先取（blocking 任务内不得触碰信号，隐私开关
        // 同纪律）：列表态载荷携带，搜索态不携带（ADR-0136 决策 4）。
        let hide_investment_related = hide_investment.get_untracked();
        let db = db_slot().clone();
        // 任务挂视图实例 owner（perform_delete 先例同形）：处理器可能运行在
        // 即将卸载的子树 owner 下——筛选条清除钮/弹层选项按下后随状态翻转即
        // 时卸载（#2037 起多了一类「按下即自卸载」的触发面），直呼 spawn 会
        // 把装载任务挂到子树 owner 上，随卸载被取消——装载静默蒸发、loading
        // 永不落位（修复前 headless 清除旅程曾确定性复现本竞态）。经
        // [`Owner::run`] 让任务归组件实例，存活到装载完成；本包裹属竞态消除
        // 而非「删除即红」型接线——删除后的复现取决于引擎调度时序（任务是否
        // 赢得卸载前的执行窗口），不设拆除断言。
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                // 币种字典、在用分类、可见账户、含软删商户、未删除保单与行集同批读取：
                // 字典读失败不静默降级成无符号金额（用户无感知的错示比暂缓呈现更差），
                // 与行集一并走错误呈现。数据期间边界读失败不拖垮页呈现：钳制退化为不钳制
                // （旧壳 silent 同口径），保留旧值到下次成功替换。表单参考数据源与原始
                // 行集随装载一并落位（#2034：编辑回填与选项面的同一事实源）。
                let outcome = blocking(move || {
                    let page_read = if !searching {
                        // 列表态：筛选维度 + 期间快照 + 默认 kind 收窄（投资 kind
                        // 排除是读接缝单点，ADR-0135）。
                        boot::transaction_page(
                            &db,
                            page_no,
                            &boot::TxListQuery {
                                range: state.range.clone(),
                                kinds: tx_filter::resolve_request_kinds(state.kinds.as_deref()),
                                involving_account_id: state.involving_account_id.clone(),
                                merchant_id: state.merchant_id.clone(),
                                category_id: state.category.as_ref().and_then(|c| match c {
                                    tx_filter::CategoryFilter::Exact(id) => Some(id.clone()),
                                    tx_filter::CategoryFilter::UncategorizedOnly => None,
                                }),
                                uncategorized_only: matches!(
                                    state.category,
                                    Some(tx_filter::CategoryFilter::UncategorizedOnly)
                                ),
                                // 视图偏好先决收窄（ADR-0136 决策 1/3）：偏好不是筛选
                                // 维度，不入会话态（snapshot 不含它）、不参与清除/ESC；
                                // 行集 = 偏好收窄 ∩ 既有筛选，下钻不让位（服务端
                                // total 同口径）。删除本行即红：开关切换行集不变
                                // （headless 旅程测试）。
                                hide_investment_related,
                            },
                        )
                        .map(|result| PageRead::new(result.items, result.total))
                    } else {
                        // 搜索态：关键词 + 金额区间 + 期间快照（裁决后期间对搜索
                        // 命中面生效，旧壳搜索页同口径）；账户/商户/分类/类型维度
                        // 不在搜索面（域读路径不支持，裁决定型为规格）。
                        boot::transaction_search(
                            &db,
                            page_no,
                            &boot::TxSearchQuery {
                                query: query.clone(),
                                amount_min_cents: amount_min,
                                amount_max_cents: amount_max,
                                range: state.range.clone(),
                            },
                        )
                        .map(|result| PageRead::new(result.items, result.total))
                    };
                    page_read
                        .and_then(|mut read| {
                            read.date_range = boot::report_date_range(&db).ok();
                            boot::currency_rows(&db).map(|rows| {
                                read.currency_rows = rows;
                                read
                            })
                        })
                        .and_then(|mut read| {
                            boot::category_rows(&db).map(|rows| {
                                read.category_rows = rows;
                                read
                            })
                        })
                        .and_then(|mut read| {
                            boot::account_rows(&db).map(|rows| {
                                read.account_rows = rows;
                                read
                            })
                        })
                        .and_then(|mut read| {
                            boot::merchant_rows(&db).map(|rows| {
                                read.merchant_rows = rows;
                                read
                            })
                        })
                        .and_then(|mut read| {
                            boot::policy_rows(&db).map(|rows| {
                                read.policy_rows = rows;
                                read
                            })
                        })
                        .and_then(|mut read| {
                            boot::item_rows(&db).map(|rows| {
                                read.item_rows = rows;
                                read
                            })
                        })
                })
                .await;
                // 过期装载不落位（后发已接管 rows/loading，先发不得回写）。
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok(PageRead {
                        items,
                        item_rows,
                        total: total_count,
                        date_range,
                        currency_rows,
                        category_rows,
                        account_rows,
                        merchant_rows,
                        policy_rows,
                    })) => {
                        // 表单参考数据源与编辑回填锚点（原始行集）随装载落位
                        //（#2034：与列表同一读批的同一事实源）。
                        tx_items.set(txn_form::TxItems(items.clone()));
                        items_snap.set(items::ItemsSnap(item_rows));
                        form_accounts.set(txn_form::FormAccounts(account_rows.clone()));
                        form_categories.set(txn_form::FormCategories(category_rows.clone()));
                        form_merchants.set(txn_form::FormMerchants(merchant_rows.clone()));
                        form_policies.set(txn_form::FormPolicies(policy_rows));
                        let currencies = CurrencyTable::from_currencies(&currency_rows);
                        form_currencies.set(currencies.clone());
                        let categories = assembly::CategoryTable::from_categories(&category_rows);
                        let accounts = AccountTable::from_accounts(&account_rows);
                        let merchants = MerchantTable::from_merchants(&merchant_rows);
                        let ctx = RowContext {
                            currencies: &currencies,
                            categories: &categories,
                            accounts: &accounts,
                            merchants: &merchants,
                            privacy_enabled: privacy,
                        };
                        rows.set(assembly::assemble_rows(&items, &ctx));
                        total.set(total_count);
                        boundary_dates.set(date_range.map(|dr| (dr.min_date, dr.max_date)));
                        searched.set(searching);
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读页任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 失效信号订阅（进程内失效信号→列表重读的接线点，#2022）：广播到达时
    // 以删后页码决策（保持当前页 / 本页删空回退一页，旧壳 afterRowDelete
    // ADR-0045 同语义）重读。订阅句柄随视图实例存亡；生产进程一视图一订阅。
    let reload_after_delete = move || {
        let target = assembly::page_after_row_delete(filters.page.get(), total.get())
            .unwrap_or_else(|| filters.page.get());
        filters.set_page(target);
        load(target);
    };
    // 订阅随组件实例存亡：组件体只跑一次、普通绑定不入 owner 图，体结束即
    // 释放——订阅句柄直放 let 会在体结束时 Drop 撤销订阅、广播落空（库已删
    // 而列表永不刷新）。与上方 overlay_token 同形：句柄移入 effect 闭包
    // （effect 不追踪读、只跑一次，闭包被组件 owner 持有，随组件卸载释放、
    // Drop 撤销订阅）。
    let reload_subscription = signals::subscribe_transactions_changed(reload_after_delete);
    effect(move || {
        let _held_for_view_lifetime: &signals::TransactionsChangedSubscription =
            &reload_subscription;
    });

    // 确认后的删除执行（域公开写入口直调，不经 IPC）：写连接侧在后台线程删
    // 除（SQLite 阻塞 IO 不占引擎线程），成功即广播交易数据失效（进程内失效
    // 信号——「失效信号→列表重读」的广播接线点，删除即红条目钉在
    // tests/app_headless.rs）；失败呈现码化错误文案，不静默（对话框已随确认
    // 关闭，错误在表格下方条位呈现，旧壳 toast 同语义）。
    let perform_delete = move |id: String| {
        delete_error.set(None);
        let db = db_slot().clone();
        // 任务挂视图实例 owner：确认 handler 运行在对话框子树的 owner 下
        // （监听器随注册它的视图存亡），而关窗即卸载对话框子树——直呼 spawn
        // 会把任务挂到子树 owner 上，随关窗被取消，写入静默蒸发（广播不发、
        // 列表不刷、错误不呈现）。视图体捕获自身 Owner，经 [`Owner::run`] 让
        // 任务归组件实例，存活到删除完成。
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_transaction(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => signals::notify_transactions_invalidated(),
                    Ok(Err(e)) => delete_error.set(Some(format!("删除失败: {e}"))),
                    Err(_) => {
                        delete_error.set(Some("删除失败: 内部错误：删除任务异常退出".to_owned()))
                    }
                }
            })
        });
    };

    // 危险确认结局（确认/取消/ESC 三路径）：关窗归一，确认才执行删除（删除
    // 任务经 perform_delete 挂视图实例 owner，关窗卸载对话框子树不影响）。
    let on_danger_outcome = move |outcome: danger::DangerOutcome| {
        let id = confirm_target.get_untracked();
        confirm_target.set(None);
        if outcome == danger::DangerOutcome::Confirmed
            && let Some(id) = id
        {
            perform_delete(id);
        }
    };

    // 搜索输入调度（旧壳 scheduleSearch 同形）：文本变化 → 页码归 1 → 防抖后
    // 装载第一页；期间再输入则序号失配、在途定时器自弃（与旧壳 clearTimeout
    // 同义）。回车立即装载（on_submit，旧壳 onEnter 同语义——不等防抖）。
    let schedule_search = move |text: &str| {
        search_text.set(text.to_owned());
        filters.set_page(1);
        search_seq.update(|seq| *seq += 1);
        let seq = search_seq.get();
        set_timeout(Duration::from_millis(SEARCH_DEBOUNCE_MS), move || {
            if search_seq.get() == seq {
                load(1);
            }
        });
    };

    // 金额区间输入调度（搜索面维度，旧壳搜索页同形：关键词/金额/日期同一
    // watcher 防抖查询）：文本变化 → 页码归 1 → 共用搜索防抖定时器（序号失配
    // 自弃）。元 → 分换算在装载时机（[assembly::amount_text_to_cents] 单点）；
    // 金额任一端在场即入搜索态（旧壳「无关键字仅筛选」同语义）。
    let schedule_amount = move || {
        filters.set_page(1);
        search_seq.update(|seq| *seq += 1);
        let seq = search_seq.get();
        set_timeout(Duration::from_millis(SEARCH_DEBOUNCE_MS), move || {
            if search_seq.get() == seq {
                load(1);
            }
        });
    };
    let on_amount_min_change = move |text: &str| {
        amount_min_text.set(text.to_owned());
        schedule_amount();
    };
    let on_amount_max_change = move |text: &str| {
        amount_max_text.set(text.to_owned());
        schedule_amount();
    };

    // 翻页动作单点（快捷键语义走同一决策半边，ADR-0140 决策 2）：分页条点击
    // 不走本入口（点击无抑制闸门——快捷键三闸门只辖键盘派发面）；越界由
    // [`assembly::next_page`] 决策半边幂等拦截。
    let flip_page = move |step: assembly::PageStep| {
        // Overlay Suppression 闸门（两族快捷键共用）：弹层开着不派发。
        // 「删除即红」负向条目钉在 headless 抑制测试（tests/app_headless.rs）。
        if overlay::has_open_overlay() {
            return;
        }
        let Some(target) =
            assembly::next_page(filters.page.get(), assembly::total_pages(total.get()), step)
        else {
            return;
        };
        filters.set_page(target);
        load(target);
    };

    // 时间维度行（共用形态抽离 `period_row`，#2040）：期间快照住筛选会话态
    //（#2037 起），应用回调 = 翻页归零 + 重读第一页（同值守卫的 changed
    // 才重读——旧壳 setFilter 同值守卫同口径）。步进动作给 `,`/`.` 快捷键
    // 与行内按钮共用；步进与弹层开合的抑制上报归 period_row 模块。
    let (period_row_view, step_period) = period_row::period_row(
        "period",
        true,
        true,
        filters.range,
        boundary_dates,
        move |changed: bool| {
            filters.set_page(1);
            if changed {
                load(1);
            }
        },
    );

    // ----------------------------------------------------------------------
    // 筛选条（#2037）：会话态 + 交互桥 + 类型/清除/ESC 处理器。
    // ----------------------------------------------------------------------

    // 下钻落位消费（#2037，骨架票导航接缝的第一个真实消费方）：挂载即读一次
    // （读一次封闸归 [nav::take_intent]；页签往复/重复落位不重放）。筛选载荷
    // 全量替换会话态（旧壳「新 query 全量替换旧 query」同构，[tx_filter::drilled]
    // 纯决策单点）；focus 在交易列表无实体定位形态（旧壳无对应消费面），读一
    // 次丢弃以防误投后续落位。消费须先于交互桥初始化与首屏装载——落位筛选与
    // 首拉同轮生效。
    if let Some(intent) = nav::take_intent()
        && let Some(drill) = intent.tx_filters.as_ref()
    {
        filters.apply_drill(drill);
    }

    // 账户/商户下拉交互桥（挂载时自会话态初始化；改动经桥接 effect 归一）。
    // 会话态信号是唯一事实源；交互桥是封装件（信号受控形态）的写入面——挂载
    // 顺序：下钻消费 → 桥初始化（落位筛选直接呈现）。
    let account_sel = signal(filters.account.get_untracked());
    let merchant_sel = signal(filters.merchant.get_untracked());

    // 类型多选读写面（回调受控封装，无中间信号）：读 = 会话态集合的**响应式**
    // 查询（触发件回显与 ✕ 可见性随勾选即时刷新——封装内 when/文本闭包经本
    // 闭包追踪会话态信号）；写 = 切换后归一（[tx_filter::normalize_kind_selection]，
    // 空集/满选 ≡ 默认）+ 同值守卫 + 统一出口（翻页归零 + 重读第一页，旧壳
    // setFilter 同形）。
    let kind_selected = move |id: &str| {
        ledger_transaction::TransactionKind::parse(id).is_ok_and(|kind| {
            filters
                .kinds
                .get()
                .is_some_and(|kinds| kinds.contains(&kind))
        })
    };
    let on_kind_toggle = move |id: &str| {
        let Ok(kind) = ledger_transaction::TransactionKind::parse(id) else {
            return;
        };
        let mut next = filters.kinds.get_untracked().unwrap_or_default();
        if next.contains(&kind) {
            next.retain(|k| *k != kind);
        } else {
            next.push(kind);
        }
        let normalized = tx_filter::normalize_kind_selection(&next);
        if filters.kinds.get_untracked() == normalized {
            return;
        }
        filters.kinds.set(normalized);
        filters.set_page(1);
        load(1);
    };
    let on_kind_clear = move || {
        if filters.kinds.get_untracked().is_none() {
            return;
        }
        filters.kinds.set(None);
        filters.set_page(1);
        load(1);
    };

    // 桥接 effect（账户/商户两维）：交互桥信号 → 会话态 + 统一出口（翻页归零
    // + 重读第一页）。同值不动作（信号等值守卫——挂载初始化不产生额外装载，
    // 重置路径先归一会话态再归零桥时桥接跳过）。删除任一桥接即红：headless
    // 账户/商户筛选旅程（设筛选 → 行集变化 → 清除）断言用户可见行集。
    let bridge_account = move || {
        effect(move || {
            let value = account_sel.get();
            if filters.account.get_untracked() != value {
                filters.account.set(value);
                filters.set_page(1);
                load(1);
            }
        });
    };
    bridge_account();
    let bridge_merchant = move || {
        effect(move || {
            let value = merchant_sel.get();
            if filters.merchant.get_untracked() != value {
                filters.merchant.set(value);
                filters.set_page(1);
                load(1);
            }
        });
    };
    bridge_merchant();

    // —— 格级行为委托（#2061）：装配载荷 → 用户可观察行为的分发闭集 ——

    // 账户/商户格自下钻：下钻载荷全量替换会话态（[tx_filter::drilled] 单点——
    // 旧壳 URL query「新 query 全量替换旧 query」同构），筛选下拉呈现随落位
    // 同步（挂载下钻消费 = apply_drill 后桥自会话态初始化；本路径显式回写桥
    // 信号，同值守卫兜底不重装载）；搜索态（关键词/金额，组件本地态）随下钻
    // 归零——旧壳下钻落交易列表页即弃搜索态；装载由本路径直呼（不经防抖）。
    let self_drill = move |drill: tx_filter::DrillFilters| {
        search_text.set(String::new());
        amount_min_text.set(String::new());
        amount_max_text.set(String::new());
        filters.apply_drill(&drill);
        account_sel.set(drill.account.clone());
        merchant_sel.set(drill.merchant.clone());
        load(1);
    };

    // 来源格跳转：目标计算收口导航域深模块（收纳/关闭两轴谓词由导航状态注入
    // ——旧壳 sidebar-order / feature-toggles 两 store 注入同位），落位经导航
    // 单点入口（下钻/跳转经 resolve_source_jump 造 NavRequest 后调 open 的
    // 既有接缝，消费不自建）。
    let jump_source = move |kind: ledger_transaction::TransactionSourceKind, entity_id: &str| {
        let store = nav::store();
        nav::open(nav_model::resolve_source_jump(
            source_nav_kind(kind),
            entity_id,
            |v| store.is_view_contained(v),
            |v| store.is_closed(v),
        ));
    };

    // 备注复制：剪贴板显式写 + 回读校验两态反馈（ai_prompt 先例同形——同一
    // 应用进程的自写自读；旧壳 NoteCopyButton 语义迁移，成功词表
    // transactions.list.copyNoteOk 同文案）。
    let copy_note = {
        let owner = view_owner;
        move |note: &str| {
            let note = note.to_owned();
            owner.run(move || {
                spawn(async move {
                    set_clipboard(&note);
                    let readback = clipboard_string().await;
                    let matched = readback == note;
                    if !matched {
                        tracing::warn!("剪贴板回读与备注原文不一致，复制反馈呈现失败态");
                    }
                    copy_feedback.set(Some(if matched {
                        "备注已复制".to_owned()
                    } else {
                        "复制失败".to_owned()
                    }));
                });
            });
        }
    };

    let cell_actions = CellActions {
        account: Arc::new(move |id: &str| {
            self_drill(tx_filter::DrillFilters {
                account: Some(id.to_owned()),
                ..tx_filter::DrillFilters::default()
            })
        }),
        merchant: Arc::new(move |id: &str| {
            self_drill(tx_filter::DrillFilters {
                merchant: Some(id.to_owned()),
                ..tx_filter::DrillFilters::default()
            })
        }),
        source: Arc::new(jump_source),
        copy_note: Arc::new(copy_note),
    };

    // 筛选激活判定（清除按钮禁用态与空态分型的判定输入；旧壳 filtersActive
    // 同口径：页码不算筛选，金额输入属搜索面保留态一并计入）。响应式读
    //（空态文案与按钮禁用随筛选变化即时刷新）。
    let search_filters_active = move || {
        tx_filter::filters_active(&tx_filter::TxFilterState {
            kinds: filters.kinds.get(),
            involving_account_id: filters.account.get(),
            merchant_id: filters.merchant.get(),
            category: filters.category.get(),
            range: filters.range.get(),
            page: filters.page.get_untracked(),
        }) || !amount_min_text.get().trim().is_empty()
            || !amount_max_text.get().trim().is_empty()
    };

    // 清除全部筛选（旧壳 resetFilters 同语义：清全部维度 + 页大小回默认[本壳
    // 固定页径] + 翻页归零 + 重拉；金额输入与搜索桥一并归零——搜索桥归零经
    // 桥接 effect 跳过同值动作）。幂等：无保留状态时按钮禁用不可达，ESC 路径
    // 用下方守卫拦。
    let reset_filters = move || {
        account_sel.set(None);
        merchant_sel.set(None);
        amount_min_text.set(String::new());
        amount_max_text.set(String::new());
        filters.reset();
        load(1);
    };

    // 视图偏好切换（#2062 / ADR-0136）：写偏好单点（落盘 + 信号镜像，与筛选
    // 会话态分家——不入 filters、清除/ESC 不触达）+ 重拉行集（行集 = 偏好收窄
    // ∩ 既有筛选；旧壳 onHideInvestmentChange 走 refresh 统一出口同构：翻页归
    // 零 + 重拉）。开关受控态与按压替身两写面同源（settings 开关行同形，不另
    // 立第二笔写路径）。
    let toggle_hide_investment = move |next: bool| {
        tx_view_preferences::set(next);
        filters.set_page(1);
        load(1);
    };

    // 两族快捷键（ADR-0140 / ADR-0141 裸键闭集）：裸键语义 = 拒绝一切修饰键
    // （引擎 keystroke 精确匹配含修饰位，`shift+[` 等不命中）；`When::Auto`
    // 缺省（四键皆文本编辑键，输入焦点期间引擎不派发——可编辑焦点闸）；
    // 处理器体即「闸门 → 决策 → 派发」三步，决策半边在纯函数单点。
    on_key("[", move |_| flip_page(assembly::PageStep::Prev));
    on_key("]", move |_| flip_page(assembly::PageStep::Next));
    on_key(",", move |_| {
        // Overlay Suppression 闸门（与翻页族共用同一判定，删除任一处即红）。
        if overlay::has_open_overlay() {
            return;
        }
        step_period(-1);
    });
    on_key(".", move |_| {
        if overlay::has_open_overlay() {
            return;
        }
        step_period(1);
    });
    // ESC 复位（旧壳 registerViewReset(resetFilters) 同语义：窗口行为守卫在
    // 无弹层 ESC 时消费——本壳等价形态 = 抑制闸门 + 幂等守卫）。弹层开着时
    // ESC 属弹层关闭面（闸门拦截，删除即红钉在 headless ESC 旅程测试）；无
    // 保留状态（筛选全默认 + 第 1 页 + 金额输入空）时幂等无操作（旧壳
    // isAtDefault 同口径，金额输入属搜索面保留态一并计入）。
    on_key("escape", move |_| {
        if overlay::has_open_overlay() {
            return;
        }
        let at_default = tx_filter::is_at_default(&filters.snapshot())
            && amount_min_text.get_untracked().trim().is_empty()
            && amount_max_text.get_untracked().trim().is_empty();
        if at_default {
            return;
        }
        reset_filters();
    });

    // 首屏装载（组件任务运行一次；boot 成功先于本视图呈现，DB 恒已就位）。
    load(1);

    // 弹层开合上报（注册表单一通道：追踪受控 open 信号，全路径覆盖）。
    // 「记一笔」类型菜单（#2034 表单入口）；期间弹层的上报已随共用期间行
    // 收进 period_row 模块。
    effect(move || {
        let open = add_menu_open.get();
        add_menu_token.report(open);
    });

    // 时间维度行（共用期间行，#2040 起抽离 `period_row` 模块）：步进器 +
    // 期间选择弹层（预设闭集；直达日历面板随迁移系列票补齐）。快照住筛选
    // 会话态，应用回调见上方 builder 接线；行尾「记一笔」入口（#2034）：
    // 类型由入口单点表达——弹层两项（记支出/记收入），弹窗内不提供切换
    //（旧壳 #150 同语义；转账/借出/退款随表单②、投资随表单③）。
    let period_row = period_row_view
        .child(view().flex_1())
        .child(
            // 备份最小入口（#2038）：工具区次级动作（旧壳设置页备份卡「另存
            // 为…」路径的最小化——NSH 无设置面，保存对话框是唯一落点选择
            // 方式；备份流程与反馈归 backup_entry 模块）。
            backup_entry::backup_button(backup, view_owner),
        )
        .child(
            // 「记一笔」入口（#2034）：按压即开类型菜单弹层（抑制上报由上方
            // effect 单通道承担）；菜单项点击落表单弹窗（kind 入口锁定）。
            popover()
                .open(move || Some(add_menu_open.get()))
                .on_open_change(move |e: &PopoverEvent| {
                    if let PopoverEvent::OpenChange(open, _) = *e {
                        add_menu_open.set(open);
                    }
                })
                .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-1")
                .trigger(
                    view().test_id("tx-add").child(
                        button()
                            .app_class("h-8 px-3 rounded-(--radius) bg-primary hover:opacity-90")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
                                text("记一笔")
                                    .text_sm()
                                    .text_color(crate::theme::color("primary-foreground")),
                            )
                            .child(
                                text("▾")
                                    .text_xs()
                                    .text_color(crate::theme::color("primary-foreground")),
                            ),
                    ),
                )
                .child(
                    // 类型菜单五项（#2035 表单②）：记支出/记收入/记转账 + 分隔线
                    // + 借出/借入（借贷入口预置方向，表单内可切四向——旧壳
                    // LENDING_CREATE_DIRECTIONS 闭集同形；退款不入菜单，入口由
                    // 交易条目右键菜单承接，旧壳同口径）+ 分隔线 + 出资分解/
                    // 购买项下单（表单③首建入口——旧壳创建菜单闭集五项无此
                    // 两项，spec #2031 表单③纵切授权在新壳首发录入形态）。
                    view()
                        .flex_col()
                        .gap_1()
                        .children(
                            [
                                (txn_form::CreateEntry::Expense, "记支出", "tx-add-expense"),
                                (txn_form::CreateEntry::Income, "记收入", "tx-add-income"),
                                (txn_form::CreateEntry::Transfer, "记转账", "tx-add-transfer"),
                            ]
                            .map(|(entry, label, anchor)| {
                                button()
                                    .app_class(
                                        "w-full px-2 py-1 rounded-(--radius) text-left text-foreground hover:bg-accent",
                                    )
                                    .child(text(label).test_id(anchor))
                                    .on_press(move |_: &()| {
                                        add_menu_open.set(false);
                                        form.open_create(entry);
                                    })
                            }),
                        )
                        .child(view().h(px(1.0)).w_full().app_class("bg-border"))
                        .children(
                            [
                                (txn_form::CreateEntry::Lend, "借出", "tx-add-lend"),
                                (txn_form::CreateEntry::Borrow, "借入", "tx-add-borrow"),
                            ]
                            .map(|(entry, label, anchor)| {
                                button()
                                    .app_class(
                                        "w-full px-2 py-1 rounded-(--radius) text-left text-foreground hover:bg-accent",
                                    )
                                    .child(text(label).test_id(anchor))
                                    .on_press(move |_: &()| {
                                        add_menu_open.set(false);
                                        form.open_create(entry);
                                    })
                            }),
                        )
                        .child(view().h(px(1.0)).w_full().app_class("bg-border"))
                        .children(
                            [
                                (txn_form::CreateEntry::Funding, "出资分解", "tx-add-funding"),
                                (txn_form::CreateEntry::Purchase, "购买项下单", "tx-add-purchase"),
                            ]
                            .map(|(entry, label, anchor)| {
                                button()
                                    .app_class(
                                        "w-full px-2 py-1 rounded-(--radius) text-left text-foreground hover:bg-accent",
                                    )
                                    .child(text(label).test_id(anchor))
                                    .on_press(move |_: &()| {
                                        add_menu_open.set(false);
                                        form.open_create(entry);
                                    })
                            }),
                        ),
                ),
        );

    // 筛选条（#2037，旧壳交易页筛选行 + 搜索页金额输入的合流形态）：账户
    //（拼音可搜，涉及账户语义，可清除）/ 商户（拼音可搜，含软删商户历史交易
    // 口径，可清除）/ 类型（多选，可选集四通用 kind，空选满选归一默认）/
    // 金额区间（元输入，搜索面维度）+ 隐藏投资相关流水开关（#2062：视图
    // 偏好不入筛选，ADR-0136 偏好与筛选分家——写偏好单点而非会话态，清除
    // 筛选与 ESC 复位不触达，也不参与清除按钮禁用态）+ 清除筛选（禁用态随
    // 筛选激活判定）。分类维度无手动控件（旧壳同口径，下钻只读入口）。控件
    // 全部经封装族消费（视图不直连框架基元，ADR-0118）；弹层抑制上报自含于封装。
    let filter_row = view()
        .flex_row()
        .items_center()
        .flex_wrap()
        .gap_2()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(pinyin_select::pinyin_select(
            "tx-filter-account",
            move || {
                txn_form::pinned_account_options(
                    &form_accounts.get().0,
                    filters.account.get_untracked().as_deref(),
                    |_| true,
                )
            },
            account_sel,
            "账户",
            true,
        ))
        .child(pinyin_select::pinyin_select(
            "tx-filter-merchant",
            move || assembly::merchant_filter_options(&form_merchants.get().0),
            merchant_sel,
            "商户",
            true,
        ))
        .child(multi_select::multi_select(
            "tx-filter-kind",
            assembly::kind_filter_options,
            kind_selected,
            on_kind_toggle,
            on_kind_clear,
            "类型",
        ))
        .child(
            input()
                .test_id("tx-amount-min")
                .value(move || amount_min_text.get())
                .placeholder("最低金额（元）")
                .text_color(fg)
                .w(px(130.0))
                .app_class(
                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(on_amount_min_change)
                .on_submit(move |_: &str| {
                    // 回车立即装载（旧壳搜索页 @keyup.enter 同语义）。
                    filters.set_page(1);
                    search_seq.update(|seq| *seq += 1);
                    load(1);
                }),
        )
        .child(
            input()
                .test_id("tx-amount-max")
                .value(move || amount_max_text.get())
                .placeholder("最高金额（元）")
                .text_color(fg)
                .w(px(130.0))
                .app_class(
                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(on_amount_max_change)
                .on_submit(move |_: &str| {
                    filters.set_page(1);
                    search_seq.update(|seq| *seq += 1);
                    load(1);
                }),
        )
        .child(
            // 视图偏好开关（#2062 / ADR-0136）：设备级持久裁决落筛选栏（旧壳
            // NSwitch + 文案 span 同位）。行 test_id = headless 按压替身
            //（switch 行为件本体不暴露 testid 面，settings 开关行先例同形）
            //——替身按压写向 toggle_hide_investment 同一执行通道，与开关
            // on_change 同源。无障碍：开关名即文案，行按钮名同（rows 无障碍
            // 读法平价）。
            view()
                .test_id("tx-filter-hide-investment")
                .role(nsh_core::AccessibilityRole::Button)
                .optional_label("隐藏投资相关流水")
                .app_class("flex items-center gap-2 rounded-(--radius)")
                .on_press(move |_: &PressEvent| {
                    let next = !hide_investment.get_untracked();
                    toggle_hide_investment(next);
                })
                .child(
                    view()
                        .flex_row()
                        .app_class("items-center gap-2")
                        .child(
                            nsh_base::switch()
                                .accessibility_label("隐藏投资相关流水")
                                .checked(move || hide_investment.get())
                                .on_change(move |value: &bool| toggle_hide_investment(*value)),
                        )
                        .child(text("隐藏投资相关流水").text_sm().text_color(fg)),
                ),
        )
        .child(
            // 清除筛选（旧壳 transactions.filter.clear 同词表）：禁用态随筛选
            // 激活判定（无保留状态不可点——旧壳 disabled=!filtersActive 同位）。
            // test_id 落在按钮内文本上（headless press/enabled 按 id 锚点命中
            // 文本、物理命中到内层按钮，tx-form-submit 先例同形）。
            button()
                .disabled(move || !search_filters_active())
                .accessibility_label("清除筛选")
                .app_class(
                    "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent disabled:opacity-40",
                )
                .child(
                    text("清除筛选").text_sm().text_color(fg).test_id("tx-filter-clear"),
                )
                .on_press(move |_: &()| reset_filters()),
        );

    // 行菜单单点（#2057 卡片形态与表格行右键共用同一菜接缝）：目标行随调用
    // 固定（fixed-target，旧壳同位）——派生行（购买项续行）的 id 同为交易本体 id，
    // 任一路径编辑/删除的都是整笔交易。载荷菜单项闭集（编辑/退款/加入物品/删除）
    // 与写路径零改动，只把入口从表格右键扩为卡片「⋯」（菜单接缝复用）。
    let open_row_menu = move |cell: &assembly::RowCells, at: Point<Points>| {
        if let Some(tx) = tx_items.get().0.iter().find(|t| t.id == cell.id).cloned() {
            let id = tx.id.clone();
            let mut items = Vec::new();
            // 编辑（#2034 基础收支，#2035 转账/借出/退款，
            // #2036 出资分解/购买项行全量开放，#2046 投资买
            // 卖行接缝）：目标行随右键固定。编辑开放面（#2036
            // 表单③后全量开放）：收入/支出/转账/退款行开放
            // ——#2035 的子行守卫（分解/购买项行不开放）随表
            // 单③子行编辑能力解除：分解行进出资分解族、购买
            // 项行进购买项族、并存行进出资分解族叠加购买项区
            //（edit_dispatch 单点分派，旧壳 supportsRowEdit
            // 口径的更新在平价核对表留痕）。界面只读 kind 仍
            // 不开放。买入/卖出行的「编辑」进投资买卖族（旧
            // 壳 issue #180 起 supportsRowEdit 同判定，主列
            // 表 ADR-0135 默认排除 investment kind——入口
            // 对用户即在投资明细页签（⑯ 装配），本分面为编
            // 译可达的接缝面）。
            let editable = matches!(
                tx.kind,
                ledger_transaction::TransactionKind::Income
                    | ledger_transaction::TransactionKind::Expense
                    | ledger_transaction::TransactionKind::Transfer
                    | ledger_transaction::TransactionKind::Refund
            );
            if matches!(
                tx.kind,
                ledger_transaction::TransactionKind::Buy
                    | ledger_transaction::TransactionKind::Sell
            ) {
                let trade_form = trade_form;
                let trade_sources = trade_sources;
                let tx = tx.clone();
                items.push(MenuItem::new("编辑").on_select(move || {
                    trade_form.begin_edit((&tx).into(), trade_sources, view_owner);
                }));
            } else if editable {
                let form = form;
                let currencies = form_currencies;
                let merchants = form_merchants;
                let accounts = form_accounts;
                let tx = tx.clone();
                items.push(MenuItem::new("编辑").on_select(move || {
                    let label = merchants
                        .get()
                        .0
                        .iter()
                        .find(|m| Some(&m.id) == tx.merchant_id.as_ref())
                        .map(|m| m.name.clone());
                    form.open_edit(&tx, &currencies.get(), label, &accounts.get().0);
                }));
            }
            // 加入物品目标随右键所在行固定（克隆在退款分支
            // 之前——退款菜单闭包 move 持有 tx 的先例同位）。
            let item_tx = tx.clone();
            // 退款（#2035）：无分解支出行开放（分解行不开放——
            // 退款表单未支持分解，旧壳 #1861 口径；购买项行保留——
            // 退款新建独立行不触碰原行子行）。原交易随右键所在行
            // 固定（fixed-target，旧壳同位）。
            if tx.kind == ledger_transaction::TransactionKind::Expense && tx.fundings.is_empty() {
                let form = form;
                let currencies = form_currencies;
                items.push(MenuItem::new("退款").on_select(move || {
                    form.open_create_refund(&tx, &currencies.get());
                }));
            }
            // 加入物品（#2044 / 旧壳 transactions.menu.addItem
            // 同词表）：仅 expense 行（溯源必为支出购买；
            // ADR-0025 创建唯一入口）。hasItem（旧壳 items
            // store 溯源指针比对同口径）：快照内存在指向本
            // 行交易的物品 →　置灰（溯源唯一的入口侧呈现，
            // 禁令事实源在域创建守卫）。
            if matches!(item_tx.kind, ledger_transaction::TransactionKind::Expense) {
                let has_item = items::has_item_for_tx(&items_snap.get().0, &item_tx.id);
                items.push(
                    MenuItem::new("加入物品")
                        .disabled(has_item)
                        .on_select(move || {
                            add_item_target.set(AddItemTarget(Some(item_tx.clone())));
                        }),
                );
            }
            let confirm = confirm_target;
            items.push(MenuItem::new("删除").on_select(move || {
                confirm.set(Some(id.clone()));
            }));
            context_menu::open_context_menu(items, at);
        }
    };
    // 卡片菜单入口（窄窗卡片「⋯」→ 行菜单单点）：目标交易随卡片固定（id 反查
    // 装配行首行——派生行同 id，任一行右键的语义同位）。
    let card_menu = move |card: &assembly::TransactionCard, at: Point<Points>| {
        let assembled = rows.get_untracked();
        if let Some(cell) = assembled.iter().find(|r| r.id == card.id) {
            open_row_menu(cell, at);
        }
    };

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(period_row)
        .child(
            // 备份入口弹层与反馈挂载面（#2038）：覆盖确认对话框 + 反馈状态
            // 条（入口下方条位，仅在有反馈时呈现；同一状态束）。
            backup_entry::backup_entry_overlays(backup, view_owner),
        )
        .child(filter_row)
        .child(
            // 搜索框（issue #2023）：标准可编辑文本输入控件（nsh_core `Input`，
            // Linux 宿主为 GTK 原生输入，天然经 GTK IME 管线——中文候选框/焦点
            // 行为归平台输入法栈，#2023 人工验收面；headless 断言面只覆盖搜索
            // 行为本身）。placeholder 与回车立即搜索与旧壳搜索页同形。
            view()
                .flex_row()
                .px_4()
                .py_2()
                .app_class("border-b border-border")
                .child(
                    input()
                        .test_id("tx-search")
                        .value(move || search_text.get())
                        .placeholder(SEARCH_PLACEHOLDER)
                        .optional_label("搜索交易")
                        .text_color(fg)
                        .app_class(
                            "flex-1 bg-background rounded-(--radius) border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(schedule_search)
                        .on_submit(move |_: &str| {
                            // 作废在途防抖（序号失配自弃）后立即装载。
                            search_seq.update(|seq| *seq += 1);
                            filters.set_page(1);
                            load(1);
                        }),
                ),
        )
        .child(
            // 断点双渲染（#2057 / ADR-0088 决策 9）：窄窗移动档 = 卡片形态
            // （tx_cards::transaction_cards——卡片格文本与宽形态同一装配产
            // 物，TransactionCardList 口径），桌面档表格一字不动（迁移系列
            // 红线）。空态分型三档与表格同词表同判据（list_empty_text 单
            // 点）；卡片「⋯」与行右键共用同一菜单单点 open_row_menu。
            when(
                crate::adaptive::is_narrow,
                move || {
                    when(
                        move || rows.get().is_empty(),
                        move || {
                            view().flex_1().items_center().justify_center().child(
                                text(assembly::list_empty_text(
                                    searched.get(),
                                    search_filters_active(),
                                ))
                                .text_sm()
                                .text_color(muted),
                            )
                        },
                    )
                    .otherwise(move || crate::tx_cards::transaction_cards(rows, card_menu))
                },
            )
            .otherwise(move || {
                view()
                    .test_id("tx-table")
                    .flex_1()
                .child(
                    data_table()
                        .columns(columns)
                        .data(move || rows_to_data(&rows.get()))
                        // 格级行为委托（#2061）：自绘格按装配载荷分发（账户/商户
                        // 自下钻、来源跳转、备注复制）；删除本接线即红——headless
                        // 四动作旅程测试逐项断言用户可观察结果。
                        .render_cell(render_cell(rows, Some(cell_actions.clone())))
                        // 行右键菜单（App* 封装族：上下文菜单，#2022）：菜单项
                        // 闭集与写路径归 open_row_menu 单点（#2057 起与卡片
                        // 「⋯」共用同一接缝）——目标行由右键所在行固定（fixed-
                        // target，旧壳同位），派生行 id 同交易本体。
                        .on_context_menu(move |e: &DataTableContextMenu| {
                            let Some(row) = e.row else {
                                return;
                            };
                            let assembled = rows.get();
                            if let Some(cell) = assembled.get(row as usize) {
                                open_row_menu(cell, Point::new(e.x, e.y));
                            }
                        })
                        .loading(move || loading.get())
                        .loading_view(text("正在加载…").text_color(muted))
                        // 空态分型三档与旧壳同源（[assembly::list_empty_text]
                        // 纯决策单点）：搜索态无命中「无匹配结果」
                        //（search.noResults）；列表态筛选无结果「没有符合条件的
                        // 交易」（list.emptyFiltered，#2037 筛选激活判定）；
                        // 列表态空库「暂无数据」（list.empty）。
                        .empty(
                            text(move || {
                                assembly::list_empty_text(searched.get(), search_filters_active())
                            })
                            .text_color(muted),
                        )
                        .classes(table_classes())
                        .flex_1(),
                    )
            }),
        )
        .child(
            // 加入物品成功反馈条位（#2044）：旧壳 message.success 的条位对应
            // 物（NSH 无 toast——#2038 裁决）；物品来源列呈现由失效信号驱动
            // 列表重读承担。
            when(
                move || item_feedback.get().is_some(),
                move || {
                    text(move || item_feedback.get().unwrap_or_default())
                        .text_xs()
                        .text_color(item_feedback_fg)
                        .test_id("tx-add-item-status")
                        .app_class("px-4 py-1")
                },
            ),
        )
        .child(
            // 删除失败呈现（#2022）：码化错误文案 zh-CN 原样，不静默；下次
            // 删除尝试开始时清空（旧壳 toast 同语义的条位呈现）。
            when(
                move || delete_error.get().is_some(),
                move || {
                    text(move || {
                        format!("删除失败: {}", delete_error.get().unwrap_or_default())
                    })
                    .text_xs()
                    .text_color(danger_fg)
                    .test_id("tx-delete-error")
                    .app_class("px-4 py-1")
                },
            ),
        )
        .child(
            // 备注复制反馈条位（#2061）：旧壳 message.success/error 的条位对应
            // 物（ai_prompt 先例同形）；反馈驻留至下次复制（无自动消失——
            // NSH 无 toast 超时机制，驻留条位是既有先例口径）。
            when(
                move || copy_feedback.get().is_some(),
                move || {
                    text(move || copy_feedback.get().unwrap_or_default())
                        .text_xs()
                        .text_color(item_feedback_fg)
                        .test_id("tx-copy-note-status")
                        .app_class("px-4 py-1")
                },
            ),
        )
        .child(
            // 危险确认对话框（App* 封装族首批件，ADR-0078 分级语义）：交易删除
            // 不可逆（确认文案自述「删除后不可恢复」）→ error 级形态；抑制上报
            // 与 ESC/遮罩语义归封装（danger_confirm），选中菜单只回写确认目标。
            danger::danger_confirm(
                move || confirm_target.get().is_some(),
                danger::DangerConfirmSpec {
                    level: danger::DangerLevel::Error,
                    title: "删除交易".to_owned(),
                    lead: Some("确认删除该条交易？".to_owned()),
                    strong_warning: Some("删除后不可恢复。".to_owned()),
                    detail: None,
                    confirm_label: "删除".to_owned(),
                    cancel_label: "取消".to_owned(),
                },
                on_danger_outcome,
            ),
        )
        .child(
            // 交易表单弹窗（#2034）：新增/编辑共用同表单语义；信号束住视图
            // 实例，写任务经视图 owner 挂载（弹窗子树卸载不取消）；成功广播
            // 交易失效信号（列表重读）。
            txn_form::transaction_form_modal(
                form,
                txn_form::TxnFormSources {
                    accounts: form_accounts,
                    categories: form_categories,
                    merchants: form_merchants,
                    policies: form_policies,
                    currencies: form_currencies,
                },
                view_owner,
            ),
        )
        .child(items::add_item_modal(add_item_target, form_currencies, item_feedback))
        .child(
            // 投资记账表单·买卖族弹窗（#2046）：行右键「编辑」分流（入口归属
            // 与先取明细再开窗的编排语义归 investments_form 模块头）；装载失
            // 败条位（失败不开窗，旧壳「先取再开窗」同口径——NSH 无 toast 的
            // 条位对应物）。
            investments_form::trade_form_modal(trade_form, trade_sources, view_owner),
        )
        .child(
            when(
                move || trade_form.load_error.get().is_some(),
                move || {
                    text(move || trade_form.load_error.get().unwrap_or_default())
                        .text_xs()
                        .text_color(item_feedback_fg)
                        .test_id("tx-trade-edit-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 底栏：读失败优先呈现错误；否则总数（搜索态「命中 N 条」、列表态
            // 「共 N 条」——旧壳搜索页与交易页同形）+ 分页条（单页收起，分页条
            // 点击与边界幂等归 Pagination 组件；搜索态翻页携关键词重搜，同
            // load 模式感知路径）。搜索态即使 0 命中也呈现总数（旧壳搜索页
            // searched 即显示 hitCount）。
            when(
                move || error.get().is_some() || searched.get() || total.get() > 0,
                move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .app_class("border-t border-border")
                        .child(
                            when(
                                move || error.get().is_some(),
                                move || {
                                    text(move || {
                                        format!("加载失败：{}", error.get().unwrap_or_default())
                                    })
                                    .text_xs()
                                    .text_color(fg)
                                    .test_id("tx-error")
                                },
                            )
                            .otherwise(move || {
                                text(move || {
                                    if searched.get() {
                                        hit_total_text(total.get())
                                    } else {
                                        total_text(total.get())
                                    }
                                })
                                .text_xs()
                                .text_color(muted)
                                .test_id("tx-total")
                            }),
                        )
                        .child(
                            when(
                                move || {
                                    error.get().is_none()
                                        && assembly::total_pages(total.get()) > 1
                                },
                                move || {
                                    view().test_id("tx-pagination").flex_row().child(
                                        pagination()
                                        .accessibility_label("交易分页")
                                        .previous_accessibility_label("上一页")
                                        .next_accessibility_label("下一页")
                                        .page(move || filters.page.get())
                                        .total_pages(move || assembly::total_pages(total.get()))
                                        .disabled(move || loading.get())
                                        .item_class(
                                            "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
                                        )
                                        .on_change(move |&picked: &u32| {
                                            filters.set_page(picked);
                                            load(picked);
                                        }),
                                    )
                                },
                            )
                            .otherwise(view),
                        )
                },
            )
            .otherwise(view),
        )
}
