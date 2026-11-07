//! NSH 根组件与启动结果视图模型（issue #2017 骨架 / issue #2018 交易表格首屏）。
//!
//! 装配入口可注入（docs/research/nsh-testing-capabilities.md §3.1：lib + bin
//! 双靶，bin 薄启动）：bin（`main.rs`）解析启动参数后经 [`set_book_dir`] 注入
//! 账本目录；headless 组件测试以同一入口注入测试库路径后起真实视图树
//! （`Root::of::<App>`），生产与测试同一条装配路径。
//!
//! 引导在组件任务的后台线程执行（`spawn` + `blocking`：SQLite 是阻塞 IO，不占
//! 引擎线程）；就绪态承载交易表格首屏——DataTable 渲染真实交易数据、分页条
//! 点击翻页（100 行/页服务端 offset 分页，走域读路径）、忙碌指示与空态。行/格
//! 文本由装配层（[`crate::assembly`]）在数据到达时一次性产出，渲染期零格式化；
//! 会话句柄（`DbState`）住进程级单例 [`DB`]，页读复用成对建连的只读侧。
//!
//! 界面主题跟随系统深浅色（registry 缺省带主题双态令牌，ADR-0145 立桩验收
//! 项）：文字色取令牌（裸 text 的默认前景是黑——暗色桌面不可见，先例 #2017
//! 修复），表格部件经 Tailwind 类取主题令牌。文案仅 zh-CN（spec #2015 实施
//! 决策）。

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use ledger_infra::db::DbState;
use ledger_infra::error::AppError;
use nsh_base::{
    DataTableAlign, DataTableCell, DataTableClasses, DataTableColumn, DataTableContextMenu,
    DataTableRows, PopoverEvent, button, data_table, input, pagination, popover,
};
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;

use crate::assembly::{
    self, AccountTable, AmountSemantic, ColumnAlign, CurrencyTable, MerchantTable, RowCells,
    RowContext,
};
use crate::boot;
use crate::items::AddItemTarget;
use crate::nav_model::ViewName;
use crate::{
    accounts, backup_entry, context_menu, danger, dashboard, items, modal, multi_select, nav,
    nav_model, overlay, period_row, pinyin_select, popconfirm, reports, scheduled, search, sidebar,
    signals, tx_filter, txn_form, views,
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

/// 金额语义色值表（旧壳 `packages/theme/src/semantic-colors.ts`
/// `KIND_SEMANTIC_COLORS` 的逐值镜像，亮/暗两态自适应——色值只在此处与旧壳
/// 各出现一份，NSH 侧不另立第二份色板：色型闭集住装配层
/// [`assembly::AmountSemantic`]，色值映射住本函数）。
pub(crate) fn amount_color(semantic: AmountSemantic) -> AdaptiveColor {
    match semantic {
        AmountSemantic::Expense => AdaptiveColor::new(rgb(0xd03050), rgb(0xe88080)),
        AmountSemantic::Income => AdaptiveColor::new(rgb(0x18a058), rgb(0x63e2b7)),
        AmountSemantic::Refund => AdaptiveColor::new(rgb(0x2080f0), rgb(0x63a8f2)),
        AmountSemantic::Transfer => AdaptiveColor::new(rgb(0x722ed1), rgb(0xb37feb)),
        AmountSemantic::Buy => AdaptiveColor::new(rgb(0xeb2f96), rgb(0xff85c0)),
        AmountSemantic::Sell => AdaptiveColor::new(rgb(0x13c2c2), rgb(0x5cdbd3)),
        AmountSemantic::Convert => AdaptiveColor::new(rgb(0xd48806), rgb(0xf0c060)),
        AmountSemantic::Split => AdaptiveColor::new(rgb(0x2f54eb), rgb(0x85a5ff)),
        AmountSemantic::Dividend => AdaptiveColor::new(rgb(0x0d9488), rgb(0x5eead4)),
    }
}

/// 启动装配入口：账本目录（bin 注入一次；headless 测试按场景注入）。槽位
/// 形态的进程级单例（headless 多场景串行测试需可复位，见
/// [`reset_book_assembly`]）。
static BOOK_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 就绪后的账本库会话（成对建连，读页复用只读侧；进程级单例与 [`BOOK_DIR`]
/// 同形——boot 成功先于 `StartupView::Ready` 呈现，视图读取时恒已就位）。
static DB: Mutex<Option<std::sync::Arc<DbState>>> = Mutex::new(None);

/// 装配槽位守卫（毒化时取回内值：槽位本身不含不变量，锁毒化不影响正确性）。
fn book_dir_slot() -> MutexGuard<'static, Option<PathBuf>> {
    BOOK_DIR.lock().unwrap_or_else(|p| p.into_inner())
}

/// 会话句柄槽位守卫（crate 内消费：交易表单写任务与 lib 测试经它取句柄）。
pub(crate) fn db_slot() -> MutexGuard<'static, Option<std::sync::Arc<DbState>>> {
    DB.lock().unwrap_or_else(|p| p.into_inner())
}

/// 会话句柄只读出口（crate 内消费：lib 测试的接线断言经它取句柄直读首页）。
#[cfg(test)]
pub(crate) fn session_handle() -> Option<std::sync::Arc<DbState>> {
    db_slot().clone()
}

/// 注入账本目录（幂等：先注入者优先，与接缝注册点同形）。bin 启动时调用一次；
/// 未注入时 [`App`] 呈现缺参失败态（不静默退窗，窗口即错误呈现面）。
pub fn set_book_dir(dir: PathBuf) {
    let mut slot = book_dir_slot();
    if slot.is_none() {
        *slot = Some(dir);
    }
}

/// lib 测试的装配串行锁（跨模块共享）：装配入口、会话句柄、导航状态与失效
/// 信号订阅者都是进程级单例——引擎类 lib 单测（tests / nav::tests /
/// dashboard::tests）跨模块互斥防串扰（#2039 概览探针长用例与既有探针并行
/// 即互踩装配单例，收口单锁）。毒化取回内值（槽位本身不含不变量，测试
/// panic 后续用例照常串行）。headless 集成测试目标有独立 SERIAL，两个测试
/// 二进制互不串扰。
#[doc(hidden)]
#[cfg(test)]
pub(crate) fn test_assembly_guard() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// 重置装配入口与会话句柄（headless 测试接缝）：同一进程内的多场景串行测试
/// 先复位再注入各自的测试库路径（生产 bin 只注入一次，不消费本函数；串行
/// 由 [`test_assembly_guard`] 保证，复位时上一场景的引擎已撤销）。失效信号
/// 订阅者与导航状态属同一批复位点（上一场景视图的订阅者不得响应下一场景
/// 的广播）。
#[doc(hidden)]
pub fn reset_book_assembly() {
    *book_dir_slot() = None;
    *db_slot() = None;
    signals::reset_for_tests();
    nav::reset_for_tests();
    tx_filter::reset_for_tests();
    reports::reset_for_tests();
    scheduled::reset_for_tests();
    crate::cross_book::reset_default_data_dir();
}

/// 启动结果三态（窗口呈现的全部状态）。
#[derive(Clone, Debug, PartialEq)]
pub enum StartupView {
    /// 未指定账本目录（bin 缺参）：呈现用法。
    MissingArg,
    /// 引导中（域栈在后台线程打开账本库）。
    Loading,
    /// 就绪：交易表格首屏（读路径直调域服务的立桩靶标，issue #2018）。
    Ready,
    /// 引导失败：码化错误的用户可见文案（zh-CN 原样呈现）。
    Failed { message: String },
}

/// 执行启动竖井（引导域栈 → 会话就位），返回窗口呈现态。
///
/// 阻塞（SQLite IO）；组件内在 `blocking` 任务里调用。就绪时会话句柄进 [`DB`]
/// 单例，列表页读经它复用只读连接（不随启动竖井释放）。
pub fn run_startup() -> StartupView {
    let Some(book_dir) = book_dir_slot().clone() else {
        return StartupView::MissingArg;
    };
    match boot::boot_ledger(&book_dir) {
        Ok(db) => {
            *db_slot() = Some(std::sync::Arc::new(db));
            StartupView::Ready
        }
        Err(e) => StartupView::Failed {
            message: e.to_string(),
        },
    }
}

/// 启动竖井的呈现文案（缺参 / 引导中 / 失败三态；就绪态不走此路径——表格
/// 视图承载，见 [`TransactionsView`]）。
pub fn headline_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "未指定账本库目录".to_owned(),
        StartupView::Loading => "正在打开账本库…".to_owned(),
        StartupView::Ready => String::new(),
        StartupView::Failed { message } => format!("启动失败：{message}"),
    }
}

/// 次行文案（用法提示 / 账本目录回显，验证立桩的可见凭据）。
pub fn secondary_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "用法：nsh-app <账本目录>".to_owned(),
        _ => match book_dir_slot().as_ref() {
            Some(dir) => dir.display().to_string(),
            None => String::new(),
        },
    }
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

/// 自绘格（金额语义色，issue #2019）：金额列按行语义色型着色（旧壳金额列
/// 着色口径），其余列平文——格文本仍取自数据（`cell.text`），本函数只补色。
/// 掩码态照常着色（掩码隐藏数字，不隐藏方向，旧壳隐私保留项同口径）。交易
/// 列表与搜索视图（#2040）共用。
pub(crate) fn render_cell(rows_signal: Signal<Vec<RowCells>>) -> impl Fn(&DataTableCell) -> Text {
    move |cell: &DataTableCell| {
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
            Some(semantic) if cell.key == "amount" => label.text_color(amount_color(semantic)),
            _ => label,
        }
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
///
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
    let columns = computed(move || elastic_columns(window().width.0 - sidebar::SIDEBAR_WIDTH));
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
    let amount_min_text = signal(String::new());
    let amount_max_text = signal(String::new());
    // 行删除态（issue #2022）：待确认删除的交易 id（Some = 危险确认对话框开）
    // 与删除失败呈现（码化错误文案，不静默；下次删除尝试开始时清空）。
    let confirm_target = signal(None::<String>);
    let delete_error = signal(None::<String>);
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

    // 主题令牌（双态自适应，registry 缺省带主题双态令牌）；缺失时回退亮灰/
    // 中灰（缺省主题 registry 缺失时的可读兜底，非双态）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    // 加入物品成功反馈条位的前景色（#2044；tokens 绑定后取值）。
    let item_feedback_fg = fg;
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());

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
                            },
                        )
                        .map(|result| PageRead::new(result.items, result.total))
                    } else {
                        // 搜索态：关键词 + 金额区间 + 期间快照（裁决后期间对搜索
                        // 命中面生效，旧壳搜索页同口径）；账户/商户/分类/类型维度
                        // 不在搜索面（域读路径不支持，裁决定型为规格）。
                        boot::transaction_search(
                            &db,
                            &query,
                            page_no,
                            amount_min,
                            amount_max,
                            state.range.as_ref(),
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
                .popup_class("bg-background border border-border rounded-md shadow-md p-1")
                .trigger(
                    view().test_id("tx-add").child(
                        button()
                            .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(text("记一笔").text_size(px(13.0)).text_color(rgb(0xffffff)))
                            .child(text("▾").text_size(px(11.0)).text_color(rgb(0xffffff))),
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
                                        "w-full px-2 py-1 rounded-md text-left text-foreground hover:bg-accent",
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
                                        "w-full px-2 py-1 rounded-md text-left text-foreground hover:bg-accent",
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
                                        "w-full px-2 py-1 rounded-md text-left text-foreground hover:bg-accent",
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
    // 金额区间（元输入，搜索面维度）+ 清除筛选（禁用态随筛选激活判定）。
    // 分类维度无手动控件（旧壳同口径，下钻只读入口）；隐藏投资相关开关属视图
    // 偏好不入筛选（ADR-0136 偏好与筛选分家，随视图偏好面接入）。控件全部经
    // 封装族消费（视图不直连框架基元，ADR-0118）；弹层抑制上报自含于封装。
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
                    "bg-background rounded-md border border-border px-2 py-1 \
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
                    "bg-background rounded-md border border-border px-2 py-1 \
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
            // 清除筛选（旧壳 transactions.filter.clear 同词表）：禁用态随筛选
            // 激活判定（无保留状态不可点——旧壳 disabled=!filtersActive 同位）。
            // test_id 落在按钮内文本上（headless press/enabled 按 id 锚点命中
            // 文本、物理命中到内层按钮，tx-form-submit 先例同形）。
            button()
                .disabled(move || !search_filters_active())
                .accessibility_label("清除筛选")
                .app_class(
                    "px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40",
                )
                .child(
                    text("清除筛选")
                        .text_size(px(12.0))
                        .text_color(fg)
                        .test_id("tx-filter-clear"),
                )
                .on_press(move |_: &()| reset_filters()),
        );

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
                            "flex-1 bg-background rounded-md border border-border px-2 py-1 \
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
            view()
                .test_id("tx-table")
                .flex_1()
                .child(
                    data_table()
                        .columns(columns)
                        .data(move || rows_to_data(&rows.get()))
                        .render_cell(render_cell(rows))
                        // 行右键菜单（App* 封装族：上下文菜单，#2022）：目标行
                        // 由右键所在行固定（fixed-target，旧壳同位）——派生行
                        // （购买项续行）的 id 同为交易本体 id，任一行右键删除的
                        // 都是整笔交易。菜单项闭集立桩只接「删除」（文案与旧壳
                        // transactions.menu.delete 同词表）；弹出与抑制上报归
                        // context_menu 封装，选中只回写确认目标（对话框呈现与
                        // 写入执行在下方两单点）。
                        .on_context_menu(move |e: &DataTableContextMenu| {
                            let Some(row) = e.row else {
                                return;
                            };
                            // 行寻址经装配行 id（派生行——购买项续行——id 同为
                            // 交易本体 id，任一行右键编辑/删除的都是整笔交易），
                            // 再按 id 反查原始行供编辑回填。
                            let assembled = rows.get();
                            let Some(cell) = assembled.get(row as usize) else {
                                return;
                            };
                            if let Some(tx) = tx_items
                                .get()
                                .0
                                .iter()
                                .find(|t| t.id == cell.id)
                                .cloned()
                            {
                                let id = tx.id.clone();
                                let mut items = Vec::new();
                                // 编辑（#2034 基础收支，#2035 转账/借出/退款，
                                // #2036 出资分解/购买项行全量开放）：目标行随右
                                // 键固定。编辑开放面（#2036 表单③后全量开放）：
                                // 收入/支出/转账/退款行开放——#2035 的子行守卫
                                // （分解/购买项行不开放）随表单③子行编辑能力解
                                // 除：分解行进出资分解族、购买项行进购买项族、
                                // 并存行进出资分解族叠加购买项区（edit_dispatch
                                // 单点分派，旧壳 supportsRowEdit 口径的更新在平
                                // 价核对表留痕）。界面只读 kind 仍不开放。
                                let editable = matches!(
                                    tx.kind,
                                    ledger_transaction::TransactionKind::Income
                                        | ledger_transaction::TransactionKind::Expense
                                        | ledger_transaction::TransactionKind::Transfer
                                        | ledger_transaction::TransactionKind::Refund
                                );
                                if editable {
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
                                        form.open_edit(
                                            &tx,
                                            &currencies.get(),
                                            label,
                                            &accounts.get().0,
                                        );
                                    }));
                                }
                                // 加入物品目标随右键所在行固定（克隆在退款分支
                                // 之前——退款菜单闭包 move 持有 tx 的先例同位）。
                                let item_tx = tx.clone();
                                // 退款（#2035）：无分解支出行开放（分解行不开放——
                                // 退款表单未支持分解，旧壳 #1861 口径；购买项行保留——
                                // 退款新建独立行不触碰原行子行）。原交易随右键所在行
                                // 固定（fixed-target，旧壳同位）。
                                if tx.kind == ledger_transaction::TransactionKind::Expense
                                    && tx.fundings.is_empty()
                                {
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
                                // 行交易的物品 → 置灰（溯源唯一的入口侧呈现，
                                // 禁令事实源在域创建守卫）。
                                if matches!(
                                    item_tx.kind,
                                    ledger_transaction::TransactionKind::Expense
                                ) {
                                    let has_item =
                                        items::has_item_for_tx(&items_snap.get().0, &item_tx.id);
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
                                context_menu::open_context_menu(items, Point::new(e.x, e.y));
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
                ),
        )
        .child(
            // 加入物品成功反馈条位（#2044）：旧壳 message.success 的条位对应
            // 物（NSH 无 toast——#2038 裁决）；物品来源列呈现由失效信号驱动
            // 列表重读承担。
            when(
                move || item_feedback.get().is_some(),
                move || {
                    text(move || item_feedback.get().unwrap_or_default())
                        .text_size(px(12.0))
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
                    .text_size(px(12.0))
                    .text_color(
                        tokens
                            .color("destructive")
                            .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into()),
                    )
                    .test_id("tx-delete-error")
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
                                    .text_size(px(12.0))
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
                                .text_size(px(12.0))
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
                                            "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
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

// ---------------------------------------------------------------------------
// 预算视图（#2033 NSH 迁移②——封装族热身）：预算管理各分类预算 + 执行进度
// 呈现。域读写直调 `ledger-budget` 等域 crate 读路径（[`crate::boot`]，不经
// IPC），失效信号接 [`crate::signals`] 预算族（写路径后广播、订阅重读）；
// PinyinSelect / Modal / Popconfirm 三封装件首消费面（[`crate::pinyin_select`]
// / [`crate::modal`] / [`crate::popconfirm`]，视图不直连框架基元，ADR-0118）。
// 金额与进度口径归装配层预算族（[`crate::assembly`]，渲染期零格式化）。
// ---------------------------------------------------------------------------

/// 周期单选行（创建表单与编辑弹窗共用形态，词表旧壳 budget.period）：两个
/// radio（nsh-base 行为件），锚点前缀由调用方给（headless 选择器用）。
fn budget_period_radio(
    id_prefix: &'static str,
    period: Signal<ledger_budget::BudgetPeriod>,
    fg: AdaptiveColor,
) -> View {
    let option =
        |value: ledger_budget::BudgetPeriod, id_suffix: &'static str, label: &'static str| {
            view().flex_row().items_center().gap_1().child(
                nsh_base::radio()
                    .checked(move || period.get() == value)
                    .on_change(move |_: &bool| period.set(value))
                    .child(
                        text(label)
                            .text_size(px(13.0))
                            .text_color(fg)
                            .test_id(format!("{}{}", id_prefix, id_suffix).as_str()),
                    ),
            )
        };
    view()
        .flex_row()
        .gap_3()
        .child(option(
            ledger_budget::BudgetPeriod::Monthly,
            "-period-monthly",
            "按月",
        ))
        .child(option(
            ledger_budget::BudgetPeriod::Yearly,
            "-period-yearly",
            "按年",
        ))
}

/// 表单字段标签（弱化小字，旧壳 NFormItem label 的本壳形态）。
pub(crate) fn field_label(label: &'static str, muted: AdaptiveColor) -> impl IntoElement {
    text(label).text_size(px(12.0)).text_color(muted)
}

/// 进度条（本视图自绘：轨道 + 按百分比宽度的指示条，颜色随超支红/正常绿——
/// 旧壳 NProgress status 语义，色值取金额语义色表同源两值）。入参是响应式
/// 闭包：宽度与颜色随信号重算（汇总卡信号驱动，表行按格重绘静态闭包）。
fn progress_bar(
    percent: impl Fn() -> f32 + Copy + 'static,
    over_budget: impl Fn() -> bool + Copy + 'static,
    show_label: bool,
) -> View {
    let color = move || {
        if over_budget() {
            amount_color(assembly::AmountSemantic::Expense)
        } else {
            amount_color(assembly::AmountSemantic::Income)
        }
    };
    let bar = view().flex_row().items_center().gap_2().child(
        view()
            .w(px(160.0))
            .h(px(6.0))
            .rounded_full()
            .bg(hsla(0.0, 0.0, 0.5, 0.15))
            .child(
                view()
                    .h(px(6.0))
                    .w(move || px(160.0 * (percent().clamp(0.0, 100.0) / 100.0)))
                    .rounded_full()
                    .bg(color),
            ),
    );
    if show_label {
        bar.child(
            text(move || format!("{}%", percent() as i64))
                .text_size(px(11.0))
                .test_id("budget-progress-percent"),
        )
    } else {
        bar
    }
}

/// 预算视图（导航骨架预算分支的真实组件，#2033）：
///
/// - **新增预算卡**：分类（拼音可搜下拉，数据源 = 支出分类树选项）+ 周期
///   （按月/按年单选，缺省月度与后端缺省一致）+ 金额（元输入，整数分落定）
///   + 添加；必填与正数前置拦截（旧壳同词表），域码化错误条位呈现。
/// - **年度汇总卡**：年化总额 / 本年已支 + 进度条 + 说明（旧壳 #1948 形态）。
/// - **预算执行表**：分类（路径名或「未分类」回退）/ 周期 / 预算 / 已支出 /
///   进度 / 状态 / 操作（编辑 + 删除，删除经确认气泡）。
///
/// 数据流：装载（进度 + 年度汇总 + 支出分类字典一次读齐）→ 装配层一次性产出
/// 展示格；写路径（创建/编辑/删除）成功后广播预算失效信号，订阅重读——
/// 「失效信号→列表重读」的消费接线点（删除即红条目钉在
/// tests/app_headless.rs）。写重活走 blocking 后台线程，任务挂视图实例
/// owner（弹窗子树卸载不取消，交易删除先例同形）。
#[component]
pub(crate) fn BudgetView() -> impl IntoElement {
    let view_owner = Owner::current();
    let loading = signal(true);
    let error = signal(None::<String>);
    // 装配产物（渲染期零格式化：行格文本已是一次性装配结果）。
    let rows = signal(Vec::<assembly::BudgetRowCells>::new());
    let summary = signal(None::<assembly::BudgetSummaryCells>);
    // 支出分类树选项（拼音可搜下拉的数据源）。
    let options = signal(Vec::<assembly::CategoryOption>::new());

    // 创建表单态。
    let category_id = signal(None::<String>);
    let period = signal(ledger_budget::BudgetPeriod::Monthly);
    let amount_text = signal(String::new());
    let create_error = signal(None::<String>);
    // 编辑弹窗态（目标行 + 回填字段；显示由「意图非空」派生，关闭统一置回
    // None 终态——旧壳 ModalIntent 编排的本壳等价形态）。
    let edit_target = signal(None::<assembly::BudgetRowCells>);
    let edit_amount = signal(String::new());
    let edit_period = signal(ledger_budget::BudgetPeriod::Monthly);
    let edit_error = signal(None::<String>);
    // 删除确认态（气泡目标预算 id；按行匹配的气泡一次只开一个）。
    let confirm_id = signal(None::<String>);
    let delete_error = signal(None::<String>);

    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    let danger_fg = tokens
        .color("destructive")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());

    // 装载（首屏与失效信号重读共用一条路径）：进度行集 + 年度汇总 + 支出分类
    // 字典一次读齐（小表聚合，重读成本可忽略）；金额隐私开关随装载取设备态
    //（轻量设置项，同步读）。阻塞读走后台线程（SQLite IO 不占引擎线程）。
    let load = move || {
        loading.set(true);
        error.set(None);
        let db = db_slot().clone();
        let privacy = amount_privacy_enabled();
        spawn(async move {
            let Some(db) = db else {
                error.set(Some("内部错误：账本库会话缺失".to_owned()));
                loading.set(false);
                return;
            };
            let outcome = blocking(move || -> ledger_infra::error::Result<(
                Vec<ledger_budget::BudgetProgress>,
                ledger_budget::BudgetAnnualSummary,
                Vec<ledger_categories::Category>,
            )> {
                let progress = boot::budget_progress(&db)?;
                let annual = boot::budget_annual_summary(&db)?;
                let categories = boot::category_rows(&db)?;
                Ok((progress, annual, categories))
            })
            .await;
            match outcome {
                Ok(Ok((progress, annual, category_rows))) => {
                    let table = assembly::CategoryTable::from_categories(&category_rows);
                    rows.set(assembly::assemble_budget_rows(&progress, &table, privacy));
                    summary.set(Some(assembly::assemble_budget_summary(&annual, privacy)));
                    options.set(assembly::expense_category_options(&category_rows));
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
            loading.set(false);
        });
    };

    // 失效信号订阅（预算写路径成功后广播 → 本视图重读）：订阅句柄随视图实例
    // 存亡（句柄移入 effect 闭包，交易视图同形——直放 let 会在体结束时 Drop）。
    let reload_subscription = signals::subscribe_budgets_changed(load);
    effect(move || {
        let _held_for_view_lifetime: &signals::BudgetsChangedSubscription = &reload_subscription;
    });

    // 下钻意图投递即弃（预算视图无定位形态——旧壳预算路由不读 focus 同语义；
    // 读一次封闸语义由 take_intent 单点承担）。
    effect(move || {
        let _current_landed = nav::store().current.get();
        let _ = nav::take_intent();
    });

    // 首屏装载（组件任务运行一次；boot 成功先于本视图呈现，DB 恒已就位）。
    load();

    // 创建预算（前置拦截 → 域公开写入口直调 → 成功广播失效信号并清表单）。
    let perform_create = move || {
        let cents = assembly::budget_yuan_to_cents(&amount_text.get());
        let category = category_id.get();
        let failure = match (category.as_ref(), cents) {
            (None, _) | (_, None) => Some("请填写分类和金额".to_owned()),
            (Some(_), Some(c)) if c <= 0 => Some("预算金额必须为正数".to_owned()),
            _ => None,
        };
        if let Some(message) = failure {
            create_error.set(Some(message));
            return;
        }
        create_error.set(None);
        let input = ledger_budget::BudgetInput {
            category_id: category.unwrap_or_default(),
            period: Some(period.get_untracked()),
            amount_cents: cents.unwrap_or_default(),
            start_date: boot::today_string(),
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_budget(db, &input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        category_id.set(None);
                        amount_text.set(String::new());
                        period.set(ledger_budget::BudgetPeriod::Monthly);
                        // 失效信号广播（「写路径后刷新」的广播接线点，删除
                        // 即红条目钉在 tests/app_headless.rs）。
                        signals::notify_budgets_invalidated();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => {
                        create_error.set(Some("创建失败: 内部错误：创建任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 编辑保存（金额与周期一并提交；分类不可改，改法为删旧建新——旧壳同
    // 语义；保存成功关窗 + 广播失效信号）。
    let perform_update = move || {
        let Some(target) = edit_target.get_untracked() else {
            return;
        };
        let cents = assembly::budget_yuan_to_cents(&edit_amount.get());
        let failure = match cents {
            None => Some("预算金额必须为正数".to_owned()),
            Some(c) if c <= 0 => Some("预算金额必须为正数".to_owned()),
            Some(_) => None,
        };
        if let Some(message) = failure {
            edit_error.set(Some(message));
            return;
        }
        edit_error.set(None);
        let next_period = edit_period.get_untracked();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_budget(
                        db,
                        &target.id,
                        cents.unwrap_or_default(),
                        Some(next_period),
                    ),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        edit_target.set(None);
                        signals::notify_budgets_invalidated();
                    }
                    Ok(Err(e)) => edit_error.set(Some(format!("更新失败: {e}"))),
                    Err(_) => {
                        edit_error.set(Some("更新失败: 内部错误：保存任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 删除执行（气泡确认后；成功广播失效信号，失败条位呈现不静默）。
    let perform_delete = move |id: String| {
        delete_error.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_budget(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => signals::notify_budgets_invalidated(),
                    Ok(Err(e)) => delete_error.set(Some(format!("删除失败: {e}"))),
                    Err(_) => {
                        delete_error.set(Some("删除失败: 内部错误：删除任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 表格自绘格（进度条 / 状态色 / 操作列）：格文本仍是装配产物，本闭包只
    // 补形态与交互（编辑回填、删除目标回写）。行标识用 Arc<str>（Copy）——
    // 封装件闭包界要求 Copy 捕获。
    let render_budget_cell =
        move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
            let row_index = cell.row as usize;
            match cell.key.as_ref() {
                "progress" => {
                    let snapshot = rows.get_untracked();
                    let row = snapshot.get(row_index);
                    let percent = row.map(|r| r.percent).unwrap_or(0.0);
                    let over = row.map(|r| r.over_budget).unwrap_or(false);
                    progress_bar(move || percent, move || over, true).into_element()
                }
                "status" => {
                    let snapshot = rows.get_untracked();
                    let row = snapshot.get(row_index);
                    let over = row.map(|r| r.over_budget).unwrap_or(false);
                    let status = row.map(|r| r.status).unwrap_or("");
                    text(status)
                        .text_size(px(12.0))
                        .text_color(if over {
                            amount_color(assembly::AmountSemantic::Expense)
                        } else {
                            amount_color(assembly::AmountSemantic::Income)
                        })
                        .into_element()
                }
                "actions" => {
                    let snapshot = rows.get_untracked();
                    let Some(row) = snapshot.get(row_index) else {
                        return view().into_element();
                    };
                    let row = row.clone();
                    let row_id: std::rc::Rc<str> = row.id.as_str().into();
                    let delete_id = row_id.clone();
                    let open_id = row_id.clone();
                    view()
                        .flex_row()
                        .gap_1()
                        .child(
                            nsh_base::button()
                                .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent")
                                .child(
                                    text("编辑")
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id("budget-edit"),
                                )
                                .on_press(move |_: &()| {
                                    edit_amount
                                        .set(assembly::budget_cents_to_yuan_text(row.amount_cents));
                                    edit_period.set(row.period);
                                    edit_error.set(None);
                                    edit_target.set(Some(row.clone()));
                                }),
                        )
                        .child(
                            // 轻量确认气泡（App* 封装族 Popconfirm 首消费）：删除
                            // 目标按行回写，气泡开合与确认归封装（抑制上报自含）。
                            popconfirm::popconfirm(
                                "budget-del",
                                move || confirm_id.get().as_deref() == Some(&*open_id),
                                move |open: bool| {
                                    if open {
                                        confirm_id.set(Some(row_id.to_string()));
                                    } else {
                                        confirm_id.set(None);
                                    }
                                },
                                popconfirm::PopconfirmSpec {
                                    message: "确认删除？".to_owned(),
                                    confirm_label: "删除".to_owned(),
                                    cancel_label: "取消".to_owned(),
                                },
                                // 触发件用纯视图（nsh-base Button 自含按压语义
                                // 会吞掉外层按压——封装的按压源是外层包裹视图）。
                                view()
                                    .app_class(
                                        "px-2 py-1 rounded-md text-foreground hover:bg-accent",
                                    )
                                    .child(
                                        text("删除")
                                            .text_size(px(12.0))
                                            .text_color(fg)
                                            .test_id("budget-delete"),
                                    ),
                                move || perform_delete(delete_id.to_string()),
                            ),
                        )
                        .into_element()
                }
                _ => text(cell.text.clone()).number_of_lines(1).into_element(),
            }
        };

    // 预算执行表列（装配列定义 → 宿主列；进度列吸收富余宽）。
    let columns = computed(move || {
        let width = window().width.0 - sidebar::SIDEBAR_WIDTH;
        let mut columns: Vec<nsh_base::DataTableColumn> = assembly::BUDGET_COLUMNS
            .iter()
            .map(|def| {
                let mut column = nsh_base::DataTableColumn::default();
                column.key = def.key.into();
                column.title = Some(def.title.into());
                column.width = Some(Points(def.width));
                column.align = match def.align {
                    assembly::ColumnAlign::Start => nsh_base::DataTableAlign::Start,
                    assembly::ColumnAlign::End => nsh_base::DataTableAlign::End,
                };
                column
            })
            .collect();
        let suggested: f32 = assembly::BUDGET_COLUMNS.iter().map(|def| def.width).sum();
        let extra = (width - suggested).max(0.0);
        if extra > 0.0
            && let Some(progress) = columns.iter_mut().find(|col| col.key == "progress")
            && let Some(w) = progress.width
        {
            progress.width = Some(Points(w.0 + extra));
        }
        columns
    });

    // 创建表单行（分类 / 周期 / 金额 / 添加）。
    let create_form = view()
        .flex_row()
        .items_center()
        .gap_3()
        .flex_wrap()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("分类", muted))
                .child(pinyin_select::pinyin_select(
                    "budget-category",
                    move || options.get(),
                    category_id,
                    "支出分类",
                    false,
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("周期", muted))
                .child(budget_period_radio("budget", period, fg)),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("金额", muted))
                .child(
                    input()
                        .test_id("budget-amount")
                        .value(move || amount_text.get())
                        .placeholder("0.00")
                        .optional_label("预算金额")
                        .text_color(fg)
                        .w(px(120.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |text: &str| amount_text.set(text.to_owned())),
                ),
        )
        .child(
            nsh_base::button()
                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                .child(
                    text("添加")
                        .text_size(px(13.0))
                        .text_color(rgb(0xffffff))
                        .test_id("budget-add"),
                )
                .on_press(move |_: &()| perform_create()),
        );

    // 年度汇总卡体（汇总在册才呈现——取数失败不渲染空卡，错误由条位呈现）。
    let summary_card = when(
        move || summary.get().is_some(),
        move || {
            view()
                .flex_col()
                .gap_2()
                .app_class("rounded-md border border-border p-3")
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            text("本年度预算")
                                .text_size(px(13.0))
                                .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                .text_color(fg),
                        )
                        .child(
                            text(move || summary.get().map(|c| c.status).unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(move || {
                                    if summary.get().map(|c| c.over_budget).unwrap_or(false) {
                                        amount_color(assembly::AmountSemantic::Expense)
                                    } else {
                                        amount_color(assembly::AmountSemantic::Income)
                                    }
                                }),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_end()
                        .gap_8()
                        .child(
                            view()
                                .flex_col()
                                .child(text("年度预算总额").text_size(px(11.0)).text_color(muted))
                                .child(
                                    text(move || {
                                        summary
                                            .get()
                                            .map(|c| c.annual_total.clone())
                                            .unwrap_or_default()
                                    })
                                    .text_size(px(22.0))
                                    .text_color(fg)
                                    .test_id("budget-summary-total"),
                                ),
                        )
                        .child(
                            view()
                                .flex_col()
                                .child(text("本年已支").text_size(px(11.0)).text_color(muted))
                                .child(
                                    text(move || {
                                        summary.get().map(|c| c.spent.clone()).unwrap_or_default()
                                    })
                                    .text_size(px(18.0))
                                    .text_color(fg)
                                    .test_id("budget-summary-spent"),
                                ),
                        ),
                )
                .child(progress_bar(
                    move || summary.get().map(|c| c.percent).unwrap_or(0.0),
                    move || summary.get().map(|c| c.over_budget).unwrap_or(false),
                    false,
                ))
                .child(
                    text("月预算按 ×12 年化计入总额；未设预算的分类支出不计入已支。")
                        .text_size(px(11.0))
                        .text_color(muted),
                )
        },
    )
    .otherwise(view);

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名（内容区标题，同容器壳形态）。
            view().px_4().py_2().child(
                text("预算")
                    .text_size(px(16.0))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(12.0))
                        .text_color(fg)
                        .test_id("budget-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 内容（表卡占剩余高度——DataTable 虚拟化按自身高度裁行，不进
            // 滚动容器；窄窗下的整页滚动随窄窗横切票）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                    .child(
                        // 新增预算卡。
                        view()
                            .flex_col()
                            .gap_2()
                            .app_class("rounded-md border border-border p-3")
                            .child(
                                text("新增预算")
                                    .text_size(px(13.0))
                                    .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            )
                            .child(create_form)
                            .child(
                                // 创建错误/提示条位（前置拦截与域错误同位；下次
                                // 尝试开始时清空）。
                                when(
                                    move || create_error.get().is_some(),
                                    move || {
                                        text(move || {
                                            create_error.get().unwrap_or_default()
                                        })
                                        .text_size(px(12.0))
                                        .text_color(danger_fg)
                                        .test_id("budget-create-error")
                                    },
                                )
                                .otherwise(view),
                            ),
                    )
                    .child(summary_card)
                    .child(
                        // 预算执行卡：表格 + 删除失败条位 + 空态。
                        view()
                            .flex_1()
                            .flex_col()
                            .gap_2()
                            .app_class("rounded-md border border-border p-3")
                            .child(
                                text("预算执行")
                                    .text_size(px(13.0))
                                    .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            )
                            .child(
                                view()
                                    .test_id("budget-table")
                                    .flex_1()
                                    .child(
                                        data_table()
                                            .columns(columns)
                                            .data(move || {
                                                let mut data = nsh_base::DataTableRows::new(rows.get().len());
                                                data = data
                                                    .texts("category", rows.get().iter().map(|r| r.category.as_str()))
                                                    .texts("period", rows.get().iter().map(|r| r.period_label))
                                                    .texts("amount", rows.get().iter().map(|r| r.amount.as_str()))
                                                    .texts("spent", rows.get().iter().map(|r| r.spent.as_str()))
                                                    .texts("progress", rows.get().iter().map(|_| ""))
                                                    .texts("status", rows.get().iter().map(|_| ""))
                                                    .texts("actions", rows.get().iter().map(|_| ""));
                                                data
                                            })
                                            .render_cell(move |cell: &nsh_base::DataTableCell| {
                                                render_budget_cell(cell)
                                            })
                                            .loading(move || loading.get())
                                            .loading_view(text("正在加载…").text_color(muted))
                                            .empty(text("暂无数据").text_color(muted))
                                            .classes(table_classes())
                                            .flex_1(),
                                    ),
                            )
                            .child(
                                when(
                                    move || delete_error.get().is_some(),
                                    move || {
                                        text(move || delete_error.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(danger_fg)
                                        .test_id("budget-delete-error")
                                    },
                                )
                                .otherwise(view),
                            ),
                    ),
        )
        .child(
            // 编辑弹窗（App* 封装族 Modal 首消费）：分类只读行 + 周期 + 金额
            // + 取消/保存；ESC / ✕ / 取消三路径关闭不写（弹层语义归封装）。
            modal::app_modal(
                move || edit_target.get().is_some(),
                || "编辑预算".to_owned(),
                move || {
                    edit_target.set(None);
                },
                move || {
                    let category = edit_target
                        .get()
                        .map(|row| row.category.clone())
                        .unwrap_or_default();
                    view()
                        .flex_col()
                        .gap_3()
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("分类", muted))
                                // 分类只读（改法为删旧建新，旧壳同语义）。
                                .child(text(category).text_size(px(13.0)).text_color(fg)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("周期", muted))
                                .child(budget_period_radio("budget-edit", edit_period, fg)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("金额", muted))
                                .child(
                                    input()
                                        .test_id("budget-edit-amount")
                                        .value(move || edit_amount.get())
                                        .placeholder("0.00")
                                        .optional_label("预算金额")
                                        .text_color(fg)
                                        .w(px(160.0))
                                        .app_class(
                                            "bg-background rounded-md border border-border px-2 py-1 \
                                             placeholder:text-muted-foreground",
                                        )
                                        .on_change(move |text: &str| {
                                            edit_amount.set(text.to_owned())
                                        }),
                                ),
                        )
                        .child(
                            when(
                                move || edit_error.get().is_some(),
                                move || {
                                    text(move || edit_error.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(danger_fg)
                                        .test_id("budget-edit-error")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            view()
                                .flex_row()
                                .justify_end()
                                .gap_2()
                                .child(
                                    nsh_base::button()
                                        .app_class(
                                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                                        )
                                        .child(
                                            text("取消")
                                                .text_size(px(13.0))
                                                .text_color(fg)
                                                .test_id("budget-cancel"),
                                        )
                                        .on_press(move |_: &()| edit_target.set(None)),
                                )
                                .child(
                                    nsh_base::button()
                                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                                        .child(
                                            text("保存")
                                                .text_size(px(13.0))
                                                .text_color(rgb(0xffffff))
                                                .test_id("budget-save"),
                                        )
                                        .on_press(move |_: &()| perform_update()),
                                ),
                        )
                },
            ),
        )
}

/// 视图宿主（导航骨架的内容区，#2032）：已迁移的交易列表与预算视图挂真实
/// 组件，三容器挂「更多」页签壳，其余未迁移视图共享反应式占位壳（标题随
/// 当前视图，下钻意图消费键落位；迁移票以自己的真实组件替换对应分支）。
/// 视图宿主（导航骨架的内容区，#2032）：已迁移的交易列表与预算视图挂真实
/// 组件，三容器挂「更多」页签壳，其余未迁移视图共享反应式占位壳（标题随
/// 当前视图，下钻意图消费键落位；迁移票以自己的真实组件替换对应分支）。
fn view_host(nav: nav::NavStore) -> impl IntoElement {
    // 当前视图信号（Copy 句柄移入分支谓词闭包，'static 无借用逃逸）。
    let current = nav.current;
    let is = move |name: ViewName| move || current.get() == name;
    let group_more = |gid: nav_model::GroupId| move || views::group_more_view(gid);
    // 分支序随迁移系列推进：交易列表 / 预算（#2033）/ 账户（#2041）/
    // 定时（#2042）/ 概览（#2039）/ 搜索（#2040）/ 物品（#2044）/ 报表
    //（#2048）/ 储蓄目标（#2056）/ 保单（#2050）/ 商户（#2052）/ 跨账本汇
    // 总与 AI 提示词（#2055）挂真实组件，三容器挂「更多」页签壳，其余未
    // 迁移视图共享占位。
    when(is(ViewName::Transactions), transactions_view).otherwise(move || {
        when(is(ViewName::Budget), budget_view).otherwise(move || {
            when(is(ViewName::Accounts), accounts::accounts_view).otherwise(move || {
                when(is(ViewName::Scheduled), scheduled::scheduled_view).otherwise(move || {
                    when(is(ViewName::Items), items::items_view).otherwise(move || {
                        when(is(ViewName::Dashboard), dashboard::dashboard_view).otherwise(
                            move || {
                                when(is(ViewName::Search), search::search_view).otherwise(
                                    move || {
                                        when(is(ViewName::Reports), reports::reports_view)
                                            .otherwise(move || {
                                                when(
                                                    is(ViewName::SavingsGoals),
                                                    crate::savings::savings_goals_view,
                                                )
                                                .otherwise(move || {
                                                    when(
                                                        is(ViewName::Policies),
                                                        crate::policies::policies_view,
                                                    )
                                                    .otherwise(move || {
                                                        when(
                                                            is(ViewName::Merchants),
                                                            crate::merchants::merchants_view,
                                                        )
                                                        .otherwise(move || {
                                                            when(
                                                                is(ViewName::Ai),
                                                                crate::ai_prompt::ai_prompt_view,
                                                            )
                                                            .otherwise(move || {
                                                                when(
is(ViewName::CrossBookSummary),
crate::cross_book::cross_book_summary_view,
)
.otherwise(move || {
when(
is(ViewName::BookkeepingMore),
group_more(nav_model::GroupId::Bookkeeping),
)
.otherwise(move || {
when(
is(ViewName::AssetsMore),
group_more(nav_model::GroupId::Assets),
)
.otherwise(move || {
when(
is(ViewName::InsightsMore),
group_more(nav_model::GroupId::Insights),
)
.otherwise(move || {
// 未迁移视图反应式占位壳：标题随当前视图；意图消费时机 = 落位
//（effect 键 current，同轮落地）。
let focused = signal(None::<nav::NavIntent>);
effect(move || {
let _current = nav.current.get();
focused.set(nav::take_intent());
});
views::placeholder_shell_current(focused)
})
})
})
})
                                                            })
                                                        })
                                                    })
                                                })
                                            })
                                    },
                                )
                            },
                        )
                    })
                })
            })
        })
    })
}

/// 立桩根组件：就绪前居中呈现启动三态，就绪后承载导航骨架（侧栏 + 内容区）。
#[component]
pub fn App() -> impl IntoElement {
    // 导航状态与筛选会话态挂根组件实例（根永生，信号存活与进程同步；后续消费
    // 经同一句柄；筛选会话态「视图卸载不丢」依赖根实例所有权，#2037），键位带
    // 绑定随根实例存亡（引擎线程派发，含抑制闸门）。
    let nav = nav::store();
    tx_filter::store();
    // 报表页会话态（#2048）同样挂根实例（首调初始化；离开报表视图重挂不丢
    // ——旧壳 reports-session 存活期 = 应用会话同语义）。
    reports::store();
    nav::bind_shortcuts();
    let state = signal(StartupView::Loading);
    // 主行/次行前景取主题令牌（双态自适应，registry 缺省带主题双态令牌）：
    // 裸 text 的默认前景是黑（ResolvedTextStyle::default 的 React Native
    // 惯例），暗色桌面上黑字贴暗底不可见——窗口文字必须显式带色。令牌缺失
    // 时回退亮灰/中灰（缺省主题 registry 缺失时的可读兜底，非双态）。
    let tokens = nsh_core::element::tokens();
    tracing::debug!(
        foreground = ?tokens.color("foreground"),
        muted = ?tokens.color("muted-foreground"),
        "启动视图主题令牌"
    );
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    spawn(async move {
        // blocking 的 JoinHandle：启动竖井不 panic（失败走 Result）；JoinError 仅在
        // 任务异常退出时出现，归入失败态呈现（不 panic 引擎线程）。
        let outcome = blocking(run_startup)
            .await
            .unwrap_or_else(|_| StartupView::Failed {
                message: "内部错误：启动任务异常退出".to_owned(),
            });
        state.set(outcome);
    });
    view().flex_1().flex_col().app_class("bg-background").child(
        // 启动三态与导航骨架互斥呈现：就绪前居中三态，就绪后侧栏 + 内容区。
        // 单一分支承载内容区（侧栏与视图宿主同分支），不双挂。
        when(
            move || matches!(state.get(), StartupView::Ready),
            move || {
                view()
                    .flex_1()
                    .flex_row()
                    .child(sidebar::sidebar_view())
                    .child(view_host(nav))
            },
        )
        .otherwise(move || {
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    text(move || headline_text(&state.get()))
                        .text_size(px(24.0))
                        .text_color(fg)
                        .test_id("startup-headline"),
                )
                .child(
                    text(move || secondary_text(&state.get()))
                        .text_size(px(12.0))
                        .text_color(muted),
                )
        }),
    )
}

#[cfg(test)]
mod shortcut_binding_tests {
    use nsh_core::keymap::{KeyBinding, Keystroke, When};

    /// 可编辑焦点闸（ADR-0141 决策 4 三闸门之一）的绑定侧钉子：四个裸键皆为
    /// 文本编辑键，`When::Auto` 缺省绑定的 `in_text = false`——文本输入持有
    /// 键盘焦点期间引擎不派发（keymap.find 的 editing 判定）。本测钉住本壳
    /// 「以 Auto 绑定」的选择；UI 级证明（搜索框焦点期间快捷键静默）随 #2023
    /// 搜索接入落地。裸键语义（拒绝一切修饰键）由引擎 keystroke 精确匹配
    /// （含修饰位）承担，一并钉住。
    #[test]
    fn bare_shortcut_bindings_do_not_fire_while_editing() {
        for text in ["[", "]", ",", "."] {
            let stroke = Keystroke::parse_with(text, false).unwrap();
            assert!(
                stroke.edits_text(),
                "{text} 是文本编辑键（输入焦点期间抑制的前提）"
            );
            let binding = KeyBinding::new(0, stroke, When::Auto, false);
            assert!(!binding.in_text, "When::Auto 绑定不在编辑中派发：{text}");
            let mut with_modifier = stroke;
            with_modifier.modifiers.shift = true;
            assert_ne!(
                with_modifier, stroke,
                "带修饰键的同名键是另一 keystroke，不命中"
            );
        }
    }
}
