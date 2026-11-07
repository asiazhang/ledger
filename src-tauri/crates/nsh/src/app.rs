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
use crate::{context_menu, danger, overlay, period, signals};

/// 金额隐私模式开关的存储键（设备轻量设置项，与旧壳同键同值形态：旧壳
/// `@ledger/money` 的 `AMOUNT_PRIVACY_STORAGE_KEY`，值为 JSON 布尔字面量
/// `"true"` / `"false"`）。轻量设置项域语义见
/// docs/contexts/CONTEXT-reference-settings.md「金额隐私模式」。
const AMOUNT_PRIVACY_STORAGE_KEY: &str = "amount_privacy_enabled";

/// 金额隐私开关的设备态读取（组件任务内同步读；解析镜像旧壳 `loadLocal`
/// 的 JSON 布尔语义：缺失/非法归默认关）。立桩无设置面——开关的写入口随
/// 设置面票接入（旧壳入口：侧栏标题行眼睛图标），本壳消费同一域来源、
/// 同一存储键。
fn amount_privacy_enabled() -> bool {
    nsh_core::storage::Storage::get_item(AMOUNT_PRIVACY_STORAGE_KEY)
        .is_some_and(|raw| raw.trim() == "true")
}

/// 装载批次读侧载荷（[`assembly::RowContext`] 的读侧对称形态）：一次页读同
/// 批取回的行集、总数与装配参考数据束。行集先落位、参考数据与边界逐臂补齐
/// 的线穿以结构承载——新增同批读维度改本结构一处，不再嵌套扩元组。
struct PageRead {
    /// 本页交易行集（列表态与搜索态同一字段，搜索态即命中页）。
    items: Vec<ledger_transaction::Transaction>,
    /// 满足当前口径的总数（页与总数共用同一 WHERE）。
    total: i64,
    /// 数据期间边界（读失败退化为 None＝不钳制，旧壳 silent 同口径）。
    date_range: Option<ledger_reports::DateRange>,
    /// 币种字典行（金额展示参数的来源）。
    currency_rows: Vec<ledger_currencies::Currency>,
    /// 在用分类行（分类路径表来源）。
    category_rows: Vec<ledger_categories::Category>,
    /// 在用且未隐藏账户行（账户格与借贷派生来源）。
    account_rows: Vec<ledger_accounts::Account>,
    /// 含软删全量商户行（商户格来源）。
    merchant_rows: Vec<ledger_merchants::Merchant>,
}

impl PageRead {
    /// 行集与总数先落位，参考数据与边界随后逐臂补齐（币种读失败即整批
    /// 失败，边界读失败保 None 不拖垮页呈现——顺序与错误传播语义不变）。
    fn new(items: Vec<ledger_transaction::Transaction>, total: i64) -> Self {
        Self {
            items,
            total,
            date_range: None,
            currency_rows: Vec::new(),
            category_rows: Vec::new(),
            account_rows: Vec::new(),
            merchant_rows: Vec::new(),
        }
    }
}

/// 金额语义色值表（旧壳 `packages/theme/src/semantic-colors.ts`
/// `KIND_SEMANTIC_COLORS` 的逐值镜像，亮/暗两态自适应——色值只在此处与旧壳
/// 各出现一份，NSH 侧不另立第二份色板：色型闭集住装配层
/// [`assembly::AmountSemantic`]，色值映射住本函数）。
fn amount_color(semantic: AmountSemantic) -> AdaptiveColor {
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

fn db_slot() -> MutexGuard<'static, Option<std::sync::Arc<DbState>>> {
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

/// 重置装配入口与会话句柄（headless 测试接缝）：同一进程内的多场景串行测试
/// 先复位再注入各自的测试库路径（生产 bin 只注入一次，不消费本函数；串行
/// 由测试侧的 SERIAL 锁保证，复位时上一场景的引擎已撤销）。失效信号订阅者
/// 属同一批复位点（上一场景视图的订阅者不得响应下一场景的广播）。
#[doc(hidden)]
pub fn reset_book_assembly() {
    *book_dir_slot() = None;
    *db_slot() = None;
    signals::reset_for_tests();
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
fn hit_total_text(total: i64) -> String {
    format!("命中 {total} 条")
}

/// 搜索输入防抖时长（毫秒）：与旧壳 `SEARCH_DEBOUNCE_MS`（issue #1402「搜索
/// 输入防抖」跨域不变量）同值同语义——等待成功优于即刻失败（AGENTS.md 体验
/// 取舍），快输入不逐键打库。
const SEARCH_DEBOUNCE_MS: u64 = 300;

/// 搜索框占位文案（与旧壳 search.keywordPlaceholder 同文案）。
const SEARCH_PLACEHOLDER: &str = "输入关键字开始搜索（备注、账户名、商户名，支持多关键字）";

/// 搜索无结果空态文案（与旧壳 search.noResults 同文案；列表态空库仍是
/// 「暂无数据」——空态分型与旧壳两档同源同形）。
const SEARCH_EMPTY_TEXT: &str = "无匹配结果";

/// 「今天」（本地时区日历日，期间预设与边界抬升的相对点，与旧壳本地
/// 日历同口径）。非响应式：跨期翻转随下次重算呈现（分钟级时钟不立桩）。
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 期间标签文案（旧壳 quickTimeRange.periodLabel 同词表）：有游标呈期间，
/// 无游标（「全部」）呈「选择期间」（弹层入口的召唤文案）。
fn period_label_text(cursor: Option<crate::period::NaturalPeriod>) -> String {
    cursor
        .as_ref()
        .map(period::format_period_label)
        .unwrap_or_else(|| "选择期间".to_owned())
}

/// 芯片点亮判定（UI 侧适配：读快照区间 → 高亮派生）。
fn match_preset_now(range: Option<period::PeriodRange>, preset: period::Preset) -> bool {
    let (from, to) = match range {
        Some(r) => (Some(r.from), Some(r.to)),
        None => (None, None),
    };
    period::match_preset(from.as_deref(), to.as_deref(), today()) == Some(preset)
}

/// 芯片稳定锚点（headless 选择器用 id: 前缀，research doc §3.5）。
fn preset_test_id(preset: period::Preset) -> &'static str {
    match preset {
        period::Preset::All => "period-chip-all",
        period::Preset::Month => "period-chip-month",
        period::Preset::Quarter => "period-chip-quarter",
        period::Preset::Year => "period-chip-year",
        period::Preset::LastYear => "period-chip-last-year",
    }
}

/// 装配行集 → 宿主表格数据（按列装配，渲染期零格式化——格文本已是装配产物）。
fn rows_to_data(rows: &[RowCells]) -> DataTableRows {
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
/// 掩码态照常着色（掩码隐藏数字，不隐藏方向，旧壳隐私保留项同口径）。
fn render_cell(rows_signal: Signal<Vec<RowCells>>) -> impl Fn(&DataTableCell) -> Text {
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
/// 内容宽）；窄于建议总宽时全列取建议值（横向滚动语义不变）。
fn elastic_columns(width: f32) -> Vec<DataTableColumn> {
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
/// 弱化面。
fn table_classes() -> DataTableClasses {
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
fn TransactionsView() -> impl IntoElement {
    let page = signal(1u32);
    let total = signal(0i64);
    let rows = signal(Vec::<RowCells>::new());
    let loading = signal(true);
    let error = signal(None::<String>);
    // 响应式列集：表格可用宽取窗口内容宽（根布局无水平 padding，表格区横向
    // 即窗口宽；ambient::window 随窗口缩放重算）——备注列吸收富余宽（弹性
    // 列，旧壳同语义），其余列按装配建议宽度。
    let columns = computed(move || elastic_columns(window().width.0));
    // 搜索态：输入文本（受控）、结果呈现态（旧壳 searched 语义——搜索结果落位
    // 后才切搜索态呈现，键入中底栏仍呈上一装载态）、防抖调度序号（新输入作废
    // 在途定时器）与装载序号（后发作废先发，与旧壳 Loadable invalidate 同义：
    // 竞态后发覆盖先发，迟到结果不落位）。
    let search_text = signal(String::new());
    let searched = signal(false);
    let search_seq = signal(0u64);
    let load_seq = signal(0u64);
    // 行删除态（issue #2022）：待确认删除的交易 id（Some = 危险确认对话框开）
    // 与删除失败呈现（码化错误文案，不静默；下次删除尝试开始时清空）。
    let confirm_target = signal(None::<String>);
    let delete_error = signal(None::<String>);
    // 时间维度行状态：期间快照（None = 全部）与数据期间边界原始日期对
    // （None = 在途/失败 → 钳制退化为不钳制，旧壳 silent 同口径）；快照区间
    // 是唯一事实源，游标不落状态（步进前从当前区间反推，ADR-0141 决策 2）。
    let range = signal(None::<period::PeriodRange>);
    let boundary_dates = signal(None::<(Option<String>, Option<String>)>);
    let period_open = signal(false);
    // 弹层开合上报注册表（Overlay Suppression，ADR-0035 原生壳重建）：token
    // 实例作用域持有，随 open 信号显式上报（effect 追踪信号，覆盖全部关闭
    // 路径——受控关闭不产生 Popover 事件，只听事件会漏报），组件卸载时
    // Drop 自动撤销。
    let overlay_token = overlay::OverlayToken::new();
    // 视图实例 owner（组件体运行时的 current）：删除任务经它挂到组件实例，
    // 不随对话框子树卸载被取消（见 perform_delete 注）。
    let view_owner = Owner::current();

    // 主题令牌（双态自适应，registry 缺省带主题双态令牌）；缺失时回退亮灰/
    // 中灰（缺省主题 registry 缺失时的可读兜底，非双态）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());

    // 期间派生（纯函数单点，UI 只消费）：游标、当前游标单位下的边界、步进
    // 可达性。
    let cursor = computed(move || {
        let r = range.get();
        period::range_to_period(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
        )
    });
    let boundary = computed(move || {
        let unit = cursor.get()?.unit;
        let (min, max) = boundary_dates.get()?;
        Some(period::derive_boundary(
            unit,
            min.as_deref(),
            max.as_deref(),
            today(),
        ))
    });
    let can_step_prev = computed(move || {
        let r = range.get();
        period::can_step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            -1,
            boundary.get().as_ref(),
        )
    });
    let can_step_next = computed(move || {
        let r = range.get();
        period::can_step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            1,
            boundary.get().as_ref(),
        )
    });

    // 装载一页（首屏、翻页、换期间与搜索共用一条路径）：输入 trim 后判态
    // ——空 = 列表态读交易页（携期间快照，全库或期间过滤口径），非空 = 搜索
    // 态直调域统一模糊搜索（与旧壳 `search_transactions` 命令同一域函数，
    // 不经 IPC）；两态分页与总数语义同旧壳（页码重置、服务端 offset 分页、
    // total = 满足口径的总数）。阻塞读走后台线程（SQLite IO 不占引擎线程）；
    // 分页条装载期间禁用（单飞——等待成功优于即刻失败，无并发页读交错）。
    // 币种字典、在用分类与数据期间边界随装载一并刷新（字典行数小、分类小
    // 表、边界是单行聚合，重读成本可忽略；写路径落地后天然跟随数据外扩/收
    // 窄）；金额隐私开关随装载取设备态（轻量设置项，存储读是同步操作，组件
    // 任务内直读）。
    let load = move |page_no: u32| {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let query = search_text.get().trim().to_owned();
        let searching = !query.is_empty();
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        let range = range.get();
        spawn(async move {
            let Some(db) = db else {
                error.set(Some("内部错误：账本库会话缺失".to_owned()));
                loading.set(false);
                return;
            };
            // 币种字典、在用分类、可见账户、含软删商户与行集同批读取：字典读失败
            // 不静默降级成无符号金额（用户无感知的错示比暂缓呈现更差），与行集
            // 一并走错误呈现。数据期间边界读失败不拖垮页呈现：钳制退化为不钳制
            // （旧壳 silent 同口径），保留旧值到下次成功替换。
            let outcome = blocking(move || {
                let page_read = if query.is_empty() {
                    boot::transaction_page(&db, page_no, range.as_ref())
                        .map(|result| PageRead::new(result.items, result.total))
                } else {
                    boot::transaction_search(&db, &query, page_no)
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
            })
            .await;
            // 过期装载不落位（后发已接管 rows/loading，先发不得回写）。
            if load_seq.get() != seq {
                return;
            }
            match outcome {
                Ok(Ok(PageRead {
                    items,
                    total: total_count,
                    date_range,
                    currency_rows,
                    category_rows,
                    account_rows,
                    merchant_rows,
                })) => {
                    let currencies = CurrencyTable::from_currencies(&currency_rows);
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
    };

    // 失效信号订阅（进程内失效信号→列表重读的接线点，#2022）：广播到达时
    // 以删后页码决策（保持当前页 / 本页删空回退一页，旧壳 afterRowDelete
    // ADR-0045 同语义）重读。订阅句柄随视图实例存亡；生产进程一视图一订阅。
    let reload_after_delete = move || {
        let target =
            assembly::page_after_row_delete(page.get(), total.get()).unwrap_or_else(|| page.get());
        page.set(target);
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
        page.set(1);
        search_seq.update(|seq| *seq += 1);
        let seq = search_seq.get();
        set_timeout(Duration::from_millis(SEARCH_DEBOUNCE_MS), move || {
            if search_seq.get() == seq {
                load(1);
            }
        });
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
            assembly::next_page(page.get(), assembly::total_pages(total.get()), step)
        else {
            return;
        };
        page.set(target);
        load(target);
    };

    // 周期步进动作（ADR-0141 决策 2：游标反推、边界钳制、越界幂等全在
    // [`period::step_current`] 单点；换期间后回第 1 页）。点击路径（步进器
    // 按钮）无抑制闸门（三闸门只辖键盘派发面，先例分页条点击）；快捷键路径
    // 在下方 step_period 内先行闸检。
    let apply_step = move |delta: i32| {
        let r = range.get();
        let Some(next) = period::step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            delta,
            boundary.get().as_ref(),
        ) else {
            return;
        };
        range.set(Some(next));
        page.set(1);
        load(1);
    };
    let step_period = move |delta: i32| {
        // Overlay Suppression 闸门（与翻页族共用同一判定，删除任一处即红）。
        if overlay::has_open_overlay() {
            return;
        }
        apply_step(delta);
    };

    // 预设选择动作单点（弹层芯片点击）：写精确自然周期快照并关弹层，回
    // 第 1 页（「全部」= 无过滤区间；离开无过滤默认态仅在确认选定后发生）。
    let apply_preset = move |preset: period::Preset| {
        range.set(period::preset_range(preset, today()));
        page.set(1);
        load(1);
        period_open.set(false);
    };

    // 两族快捷键（ADR-0140 / ADR-0141 裸键闭集）：裸键语义 = 拒绝一切修饰键
    // （引擎 keystroke 精确匹配含修饰位，`shift+[` 等不命中）；`When::Auto`
    // 缺省（四键皆文本编辑键，输入焦点期间引擎不派发——可编辑焦点闸）；
    // 处理器体即「闸门 → 决策 → 派发」三步，决策半边在纯函数单点。
    on_key("[", move |_| flip_page(assembly::PageStep::Prev));
    on_key("]", move |_| flip_page(assembly::PageStep::Next));
    on_key(",", move |_| step_period(-1));
    on_key(".", move |_| step_period(1));

    // 首屏装载（组件任务运行一次；boot 成功先于本视图呈现，DB 恒已就位）。
    load(1);

    // 弹层开合上报（注册表单一通道：追踪受控 open 信号，全路径覆盖）。
    effect(move || {
        let open = period_open.get();
        overlay_token.report(open);
    });

    // 时间维度行（issue #2021 / ADR-0141）：期间步进器 `< 期间标签 >`，标签
    // 点击打开期间选择弹层（预设闭集；直达日历面板随迁移系列票补齐）。步进
    // 按钮置灰 = 无游标或越出数据期间边界（可达性在 computed 单点）；键位
    // 字符内嵌步进按钮（ADR-0141 决策 5 修订形态）。
    let period_row = view()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(
            button()
                .disabled(move || !can_step_prev.get())
                .accessibility_label("上一个周期")
                .app_class(
                    "px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40",
                )
                .flex_row()
                .items_center()
                .gap_1()
                .on_press({
                    let apply = apply_step;
                    move || apply(-1)
                })
                .child(text("‹").text_color(fg))
                .child(text(",").text_size(px(11.0)).text_color(muted)),
        )
        .child(
            // 期间标签 → 期间选择弹层（Overlay Suppression 的注册弹层：
            // on_open_change 只回写受控 open 信号，注册表上报由上方 effect
            // 单通道承担；token 随组件卸载撤销）。
            popover()
                .open(move || Some(period_open.get()))
                .on_open_change(move |e: &PopoverEvent| {
                    // 只做信号回写（开合上报住上方 effect）。
                    if let PopoverEvent::OpenChange(open, _) = *e {
                        period_open.set(open);
                    }
                })
                .popup_class("bg-background border border-border rounded-md shadow-md p-1")
                .trigger(
                    // test_id 载体视图（组件元素无 test_id 槽，先例 tx-pagination 的
                    // 载体形态）；headless 经 id: 锚点命中载体、物理命中到内层按钮。
                    view().test_id("period-label").child(
                        button()
                            .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(text(move || period_label_text(cursor.get())).text_color(fg))
                            .child(text("▾").text_size(px(11.0)).text_color(muted)),
                    ),
                )
                .child(
                    // 芯片 test_id 落在按钮内文本上（headless press 只认可按元素
                    // 本身或其内部文本，包裹视图不命中，nsh-testing Screen::pressable）。
                    view()
                        .flex_col()
                        .gap_1()
                        .children(period::PRESETS.iter().copied().map(|preset| {
                            button()
                                .selected(move || match_preset_now(range.get(), preset))
                                .app_class(
                                    "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                                )
                                .child(text(period::preset_label(preset)).test_id(preset_test_id(preset)))
                                .on_press({
                                    let apply = apply_preset;
                                    move || apply(preset)
                                })
                        })),
                ),
        )
        .child(
            button()
                .disabled(move || !can_step_next.get())
                .accessibility_label("下一个周期")
                .app_class(
                    "px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40",
                )
                .flex_row()
                .items_center()
                .gap_1()
                .on_press({
                    let apply = apply_step;
                    move || apply(1)
                })
                .child(text("›").text_color(fg))
                .child(text(".").text_size(px(11.0)).text_color(muted)),
        );

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(period_row)
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
                            page.set(1);
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
                            if let Some(id) = rows.get().get(row as usize).map(|r| r.id.clone()) {
                                let confirm = confirm_target;
                                context_menu::open_context_menu(
                                    vec![MenuItem::new("删除").on_select(move || {
                                        confirm.set(Some(id.clone()));
                                    })],
                                    Point::new(e.x, e.y),
                                );
                            }
                        })
                        .loading(move || loading.get())
                        .loading_view(text("正在加载…").text_color(muted))
                        // 空态分型与旧壳同源：搜索态无命中「无匹配结果」
                        //（search.noResults），列表态空库「暂无数据」（list.empty）。
                        .empty(
                            text(move || {
                                if searched.get() {
                                    SEARCH_EMPTY_TEXT
                                } else {
                                    "暂无数据"
                                }
                            })
                            .text_color(muted),
                        )
                        .classes(table_classes())
                        .flex_1(),
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
                                        .page(move || page.get())
                                        .total_pages(move || assembly::total_pages(total.get()))
                                        .disabled(move || loading.get())
                                        .item_class(
                                            "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
                                        )
                                        .on_change(move |&picked: &u32| {
                                            page.set(picked);
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

/// 立桩根组件：就绪前居中呈现启动三态，就绪后承载交易表格首屏。
#[component]
pub fn App() -> impl IntoElement {
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
        // 启动三态与表格首屏互斥呈现：就绪前居中三态，就绪后整窗表格。
        // 单一分支承载 TransactionsView，不双挂（双实例即双页读与双底栏）。
        when(
            move || matches!(state.get(), StartupView::Ready),
            transactions_view,
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
