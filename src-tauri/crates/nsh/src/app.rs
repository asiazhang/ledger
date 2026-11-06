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
use nsh_base::{
    DataTableAlign, DataTableCell, DataTableClasses, DataTableColumn, DataTableRows, PopoverEvent,
    button, data_table, input, pagination, popover,
};
use nsh_core::prelude::*;

use crate::assembly::{
    self, AccountTable, AmountSemantic, ColumnAlign, CurrencyTable, MerchantTable, RowCells,
    RowContext,
};
use crate::boot;
use crate::{overlay, period};

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
/// 由测试侧的 SERIAL 锁保证，复位时上一场景的引擎已撤销）。
#[doc(hidden)]
pub fn reset_book_assembly() {
    *book_dir_slot() = None;
    *db_slot() = None;
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
        let label = text(cell.text.clone());
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

/// 表格部件主题（Tailwind 类取主题令牌，跟随系统深浅色）：文字色逐部件显式
/// 带色（裸 text 默认前景黑，暗色桌面不可见——#2017 先例），行悬停与空态取
/// 弱化面。
fn table_classes() -> DataTableClasses {
    let mut classes = DataTableClasses::default();
    classes.root = Some("bg-background text-foreground".into());
    classes.header_cell = Some("text-muted-foreground".into());
    classes.cell = Some("text-foreground".into());
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
    // 搜索态：输入文本（受控）、结果呈现态（旧壳 searched 语义——搜索结果落位
    // 后才切搜索态呈现，键入中底栏仍呈上一装载态）、防抖调度序号（新输入作废
    // 在途定时器）与装载序号（后发作废先发，与旧壳 Loadable invalidate 同义：
    // 竞态后发覆盖先发，迟到结果不落位）。
    let search_text = signal(String::new());
    let searched = signal(false);
    let search_seq = signal(0u64);
    let load_seq = signal(0u64);
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
                        .map(|result| (result.items, result.total))
                } else {
                    boot::transaction_search(&db, &query, page_no)
                        .map(|result| (result.items, result.total))
                };
                page_read
                    .and_then(|(items, total)| {
                        let date_range = boot::report_date_range(&db).ok();
                        boot::currency_rows(&db)
                            .map(|currency_rows| (items, total, date_range, currency_rows))
                    })
                    .and_then(|(items, total, date_range, currency_rows)| {
                        boot::category_rows(&db)
                            .map(|rows| (items, total, date_range, currency_rows, rows))
                    })
                    .and_then(|(items, total, date_range, currency_rows, category_rows)| {
                        boot::account_rows(&db).map(|account_rows| {
                            (
                                items,
                                total,
                                date_range,
                                currency_rows,
                                category_rows,
                                account_rows,
                            )
                        })
                    })
                    .and_then(
                        |(items, total, date_range, currency_rows, category_rows, account_rows)| {
                            boot::merchant_rows(&db).map(|merchant_rows| {
                                (
                                    items,
                                    total,
                                    date_range,
                                    currency_rows,
                                    category_rows,
                                    account_rows,
                                    merchant_rows,
                                )
                            })
                        },
                    )
            })
            .await;
            // 过期装载不落位（后发已接管 rows/loading，先发不得回写）。
            if load_seq.get() != seq {
                return;
            }
            match outcome {
                Ok(Ok((
                    items,
                    total_count,
                    date_range,
                    currency_rows,
                    category_rows,
                    account_rows,
                    merchant_rows,
                ))) => {
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
                        .columns(table_columns())
                        .data(move || rows_to_data(&rows.get()))
                        .render_cell(render_cell(rows))
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
