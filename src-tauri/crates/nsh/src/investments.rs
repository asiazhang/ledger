//! 投资视图（issue #2045 NSH 迁移⑭ + #2046 迁移⑮；旧壳 `InvestmentsView.vue`
//! 页签容器：概览/盈亏随⑭已迁、持仓随⑮，ledger/instruments 随⑯）。
//!
//! 页签容器（旧壳 InvestmentsView 前三页签——ledger/instruments 随 ⑯）：
//! visited-once keep-mounted 页签容器单点重建
//!（同 [`crate::settings`] 形态）——旧壳 display-directive="show" + 页签 key
//! 纪律的本壳对应物：已到访页签 display 隐藏保持挂载，切页签不重挂重拉；
//! 删 display 切换接线即红（headless keep-mounted 探针）。
//!
//! 会话槽（旧壳 investments-session store 的重生子集：已迁三页签射程内的
//! 瞬态——当前页签 + 走势预设区间 + 盈亏两表页码 + 持仓筛选三维与页码）：
//! 进程级信号单例，同一会话内离开投资视图再回来回原样；冷启动回默认（默认
//! 页签「概览」+ 走势「1 年」+ 页码 1）；保留全程零写盘。旧壳持仓筛选三维
//! 与页码随 issue #1192 住会话 store，本壳会话槽同射程直接承载；明细/标的
//! 筛选三维与其页解随各自页签的归属票就位（不扩射程之外的形状）。
//!
//! 读路径直调域公开读入口（[`crate::boot`]，不经 IPC——与旧壳
//! investment_overview / realized_pnl_summary / portfolio_value_trend /
//! money_weighted_return_summary 命令同一域函数）；ADR-0131 全页折本位币
//! 单值，装配层只消费不另立口径。投资失效信号订阅（[`crate::signals`] 投资
//! 族）驱动重读。
//!
//! 界面文案仅 zh-CN（旧壳 investments.json 词表原样承载）；隐私掩码与全壳
//! 同源（设备轻量设置项，装载时机读取）。码化错误 zh 原样呈现
//!（fx.rate-missing 等域码化错误透传卡内警告 + 重试，不给半截数字）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

use crate::app::{amount_privacy_enabled, db_slot};
use crate::assembly::{self, AMOUNT_PRIVACY_MASK, CategoryOption, CurrencyFormat};
use crate::boot;
use crate::nav;
use crate::pinyin_select;
use crate::plot_line;
use crate::plot_line::PlotLineSeries;
use crate::popover::popover_tip;
use crate::select;
use crate::signals;
use crate::views;

// ---------------------------------------------------------------------------
// 词表（旧壳 packages/i18n zh-CN investments.json 逐条平价；字面闭集承载）。
// ---------------------------------------------------------------------------

pub(crate) const OVERVIEW_TITLE: &str = "投资概览";
pub(crate) const INVESTABLE_ASSETS: &str = "可投资资产";
pub(crate) const INVESTABLE_ASSETS_TIP: &str =
    "投资账户现金 + 持仓市值；隐藏账户与负债都不计入，缺现价的持仓跳过；它是财务自由度的分子。";
pub(crate) const CURRENCY_LABEL_FMT: &str = "本位币 {currency}";
pub(crate) const CASH_LEG: &str = "投资账户现金";
pub(crate) const HOLDINGS_LEG: &str = "持仓市值";
pub(crate) const TOTALS_TITLE: &str = "投资合计";
pub(crate) const MARKET_VALUE_LABEL: &str = "总市值";
pub(crate) const UNREALIZED_PNL_LABEL: &str = "持仓收益";
pub(crate) const CUMULATIVE_PNL_LABEL: &str = "累计收益";
pub(crate) const MARKET_VALUE_TIP: &str =
    "各持仓按「数量 × 最新价格」计算市值的合计；缺现价或缺汇率的持仓不计入；不含投资账户里的现金。";
pub(crate) const UNREALIZED_PNL_TIP: &str = "当前市值减去持仓成本（FIFO 批次成本，含买入手续费）的未实现盈亏，随行情变动；卖出后才转为已实现盈亏。";
pub(crate) const CUMULATIVE_PNL_TIP: &str = "持仓收益（未实现）＋已实现盈亏＋累计分红。";
/// 概览作用域句（旧壳 investments.concepts.scopeOverview 同词表；tipTemplate
/// "{body}{scope}" 同形拼接，概览页签口径 = 隐藏账户排除）。
pub(crate) const OVERVIEW_SCOPE_SUFFIX: &str = "概览页此处为全账本口径：金额折算到全局默认币种（本位币）后合并为单个数值，不随搜索或账户过滤收窄；隐藏账户排除（同可投资资产口径）。";
pub(crate) const RETRY: &str = "重试";
pub(crate) const MISSING_PRICE_NOTE_FMT: &str = "另有 {count} 只持仓因缺现价或折算汇率未计入。";
pub(crate) const NO_ACCOUNT_NOTE: &str =
    "还没有投资账户；在「账户」页新建投资账户后，这里会显示你的可投资资产。";

pub(crate) const TREND_CARD_TITLE: &str = "组合走势";
pub(crate) const TREND_MODE_PORTFOLIO: &str = "组合市值";
pub(crate) const TREND_TIP: &str = "每个采样点按「当时持有份额 × 当周价格」折算到本位币的历史市值；跨币种用同期历史汇率；周采样，缺价或缺汇率的标的沿用最近一个有价 / 有汇率的周；不含现金账户。";
pub(crate) const TREND_EMPTY_NO_POINTS: &str = "当前区间内暂无采样点，可尝试更长的区间。";
pub(crate) const TREND_EMPTY_BACKFILL_RUNNING: &str = "历史数据补全中，完成后即可查看走势。";
pub(crate) const TREND_EMPTY_BACKFILL_PROGRESS_FMT: &str =
    "历史数据补全中（{done}/{total}），完成后即可查看走势。";
pub(crate) const TREND_EMPTY_RETRY_PENDING: &str = "历史补全暂未成功，后台稍后会自动重试。";
pub(crate) const TREND_EMPTY_NO_DATA: &str =
    "没有可采集的历史数据（如查无此码、新基金未披露净值或已退市）。";
/// 币种口径标注（旧壳 investments.trend.captionPortfolio 同词表）。
pub(crate) fn trend_caption(currency: &str) -> String {
    format!("单位：{currency}（本位币）")
}

pub(crate) const PNL_TAB_LABEL: &str = "盈亏";
/// 明细页签名（旧壳 investments.tabs.ledger 同词表）。
pub(crate) const LEDGER_TAB_LABEL: &str = "明细";
/// 标的页签名（旧壳 investments.tabs.instruments 同词表）。
pub(crate) const INSTRUMENTS_TAB_LABEL: &str = "标的";
/// 明细页签 URL 值（旧壳 LEDGER_TAB_URL_VALUE 同词；下钻载荷 tab 值域锚）。
pub(crate) const LEDGER_TAB_URL_VALUE: &str = "detail";
pub(crate) const PNL_FILTER_ACCOUNT: &str = "全部账户";
pub(crate) const PNL_FILTER_INSTRUMENT_PLACEHOLDER: &str = "搜索标的（代码/名称/拼音）";
pub(crate) const PNL_BY_YEAR: &str = "按年度汇总";
pub(crate) const PNL_BY_ACCOUNT: &str = "按账户汇总";
pub(crate) const PNL_BY_MWR: &str = "资金加权收益率";
pub(crate) const PNL_TOTAL: &str = "全账";
pub(crate) const PNL_NOT_COMPUTABLE: &str = "无法计算";
pub(crate) const PNL_EMPTY: &str = "暂无盈亏数据";
pub(crate) const PNL_EMPTY_TABLE: &str = "暂无数据";
pub(crate) const PNL_COL_ACCOUNT: &str = "账户";
pub(crate) const PNL_COL_YEAR: &str = "年度";
pub(crate) const PNL_COL_CURRENCY: &str = "币种";
pub(crate) const PNL_COL_REALIZED_PNL: &str = "已实现盈亏";
pub(crate) const PNL_COL_DIVIDEND: &str = "现金分红";
pub(crate) const PNL_COL_REALIZED_GAIN: &str = "已实现收益";
pub(crate) const PNL_COL_MWR: &str = "资金加权收益率";
pub(crate) const PNL_MWR_TIP: &str = "按实际现金流与当前市值解出的年化内部收益率；建仓时点未知的持仓改给「未年化」，即累计收益除以累计投入，两者不可互算；不随搜索或标的筛选收窄；缺价或数据不足显示「-」。";
pub(crate) const PNL_MWR_CUMULATIVE_TIP: &str =
    "未年化：累计收益除以累计投入（含建仓时点未知的持仓，不给年化）";
/// 折算口径披露句（旧壳 investments.pnl.calibrationNote 同词表直取）。
pub(crate) const PNL_CALIBRATION_NOTE: &str = "两表金额已折算为本位币：每腿按其实现时点所在周的汇率折算（汇率缺失时取就近周），跨币种已合计，供跨年、跨账户直接比较。";
/// 内部错误兜底文案（读任务异常退出/会话缺失；boot 读面同词）。
pub(crate) const MSG_INTERNAL: &str = "内部错误：读任务异常退出";

// ---------------------------------------------------------------------------
// 会话槽（旧壳 investments-session store 的本票射程子集）
// ---------------------------------------------------------------------------

/// 页签闭集（本票两页签：持仓随 ⑮、ledger/instruments 随 ⑯ 再扩）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvestmentsTab {
    /// 概览（默认；旧壳 INVESTMENTS_DEFAULT_TAB 同源）。
    Overview,
    /// 盈亏。
    Pnl,
    /// 持仓（#2046）。
    Holdings,
    /// 明细（#2047）。
    Ledger,
    /// 标的（#2047）。
    Instruments,
}

impl InvestmentsTab {
    /// 页签名（旧壳 investments.tabs.* 同词表）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            InvestmentsTab::Overview => "概览",
            InvestmentsTab::Pnl => PNL_TAB_LABEL,
            InvestmentsTab::Holdings => "持仓",
            InvestmentsTab::Ledger => LEDGER_TAB_LABEL,
            InvestmentsTab::Instruments => INSTRUMENTS_TAB_LABEL,
        }
    }

    /// headless 锚点片段。
    pub(crate) fn id_str(self) -> &'static str {
        match self {
            InvestmentsTab::Overview => "overview",
            InvestmentsTab::Pnl => "pnl",
            InvestmentsTab::Holdings => "holdings",
            InvestmentsTab::Ledger => "ledger",
            InvestmentsTab::Instruments => "instruments",
        }
    }
}

/// 页签全集（顺序 = 旧壳 InvestmentsView 五位全序；ledger/instruments 随 ⑯。
const TABS: [InvestmentsTab; 5] = [
    InvestmentsTab::Overview,
    InvestmentsTab::Pnl,
    InvestmentsTab::Holdings,
    InvestmentsTab::Ledger,
    InvestmentsTab::Instruments,
];

/// 走势预设区间闭集（旧壳 TREND_RANGE_PRESETS；1 月/3 月/1 年/3 年/5 年/全部）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrendPreset {
    OneMonth,
    ThreeMonths,
    OneYear,
    ThreeYears,
    FiveYears,
    All,
}

/// 全部预设（旧壳 TREND_RANGE_PRESETS 同序）。
pub(crate) const TREND_PRESETS: [TrendPreset; 6] = [
    TrendPreset::OneMonth,
    TrendPreset::ThreeMonths,
    TrendPreset::OneYear,
    TrendPreset::ThreeYears,
    TrendPreset::FiveYears,
    TrendPreset::All,
];

impl TrendPreset {
    /// 预设标签（旧壳 investments.trend.range* 同词表）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            TrendPreset::OneMonth => "1 月",
            TrendPreset::ThreeMonths => "3 月",
            TrendPreset::OneYear => "1 年",
            TrendPreset::ThreeYears => "3 年",
            TrendPreset::FiveYears => "5 年",
            TrendPreset::All => "全部",
        }
    }

    /// 预设 → 月数（「全部」不设界）。
    fn months(self) -> Option<i64> {
        match self {
            TrendPreset::OneMonth => Some(1),
            TrendPreset::ThreeMonths => Some(3),
            TrendPreset::OneYear => Some(12),
            TrendPreset::ThreeYears => Some(36),
            TrendPreset::FiveYears => Some(60),
            TrendPreset::All => None,
        }
    }
}

/// 会话态句柄（Copy；信号挂首次 [`store`] 调用所在组件实例——生产路径契约：
/// 投资视图组件体最先调用。视图卸载重挂不变：进程槽承担会话内保留）。
#[derive(Clone, Copy)]
pub(crate) struct InvestmentsSession {
    /// 当前页签（默认概览）。
    pub tab: Signal<InvestmentsTab>,
    /// 走势预设区间（默认「1 年」）。
    pub trend_preset: Signal<TrendPreset>,
    /// 盈亏·按年汇总页码（1 起；客户端切片的展示切片状态）。
    pub pnl_year_page: Signal<usize>,
    /// 盈亏·按账户汇总页码（1 起；与按年页码彼此独立）。
    pub pnl_account_page: Signal<usize>,
    /// 持仓·搜索输入回显值（即时，未经防抖；旧壳 holdingsSearchInput 同射程）。
    pub holdings_search_input: Signal<String>,
    /// 持仓·搜索应用值（防抖后参与过滤；旧壳 holdingsSearch 同射程）。
    pub holdings_search: Signal<String>,
    /// 持仓·账户过滤（None = 全部默认态；旧壳 holdingsAccountId 同射程）。
    pub holdings_account_id: Signal<Option<String>>,
    /// 持仓·排序（None = 默认标的代码字母序；旧壳 holdingsSorter 同射程）。
    pub holdings_sorter: Signal<Option<HoldingsSorter>>,
    /// 持仓·页码（1 起；客户端切片的展示切片状态）。
    pub holdings_page: Signal<usize>,
    /// 明细·页码（1 起；服务端分页，⑯）。
    pub detail_page: Signal<usize>,
    /// 明细·类型子集（空集合 = 不过滤 = 默认态；闭集 = 五种投资 kind，⑯）。
    pub detail_kinds: Signal<Vec<ledger_transaction::TransactionKind>>,
    /// 明细·账户过滤（None = 全部默认态；涉及账户语义，⑯）。
    pub detail_account_id: Signal<Option<String>>,
    /// 明细·标的深链维（None = 默认态；仅下钻落点写入，无手动控件，⑯）。
    pub detail_instrument_id: Signal<Option<String>>,
    /// 标的页签·anchor 定位意图（None = 默认态；仅下钻落点写入，消费后即
    /// 复位——读一次，⑯）。
    pub instruments_focus: Signal<Option<String>>,
}

/// 持仓排序列闭集（旧壳 HoldingsSortColumn 同闭集）：市值 / 持仓收益（未实现
/// 盈亏）两列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldingsSortColumn {
    /// 市值列。
    MarketValue,
    /// 持仓收益列。
    UnrealizedPnl,
}

/// 持仓排序状态（旧壳 HoldingsSorter 同形）：列 + 方向（descend = 降序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HoldingsSorter {
    pub(crate) column: HoldingsSortColumn,
    pub(crate) descend: bool,
}

/// 持仓页客户端切片页大小（旧壳 HOLDINGS_PAGE_SIZE 单源同值：固定 20 不设
/// 选择器，issue #912）。
pub(crate) const HOLDINGS_PAGE_SIZE: usize = 20;

static SESSION: std::sync::Mutex<Option<InvestmentsSession>> = std::sync::Mutex::new(None);

fn session_slot() -> std::sync::MutexGuard<'static, Option<InvestmentsSession>> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// 投资页会话槽（惰性初始化：冷启动回默认——默认页签「概览」+ 走势「1 年」
/// + 页码 1；旧壳 investments-session 默认态同源）。
pub(crate) fn store() -> InvestmentsSession {
    let mut slot = session_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = InvestmentsSession {
        tab: signal(InvestmentsTab::Overview),
        trend_preset: signal(TrendPreset::OneYear),
        pnl_year_page: signal(1),
        pnl_account_page: signal(1),
        holdings_search_input: signal(String::new()),
        holdings_search: signal(String::new()),
        holdings_account_id: signal(None),
        holdings_sorter: signal(None),
        holdings_page: signal(1),
        detail_page: signal(1),
        detail_kinds: signal(Vec::new()),
        detail_account_id: signal(None),
        detail_instrument_id: signal(None),
        instruments_focus: signal(None),
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
// 预设区间换算（旧壳 usePortfolioTrend.ts 的 toTrendRange 重生，纯函数）
// ---------------------------------------------------------------------------

/// 预设区间 → 查询区间（纯函数）：起点 = today 减去对应月数、同日；该日在
/// 目标月不存在时钳到目标月最后一日（月末溢出钳制）；终点不设界（域内裁剪
/// 到今天为止）；「全部」不设起止。
pub(crate) fn to_trend_range(
    preset: TrendPreset,
    today: chrono::NaiveDate,
) -> (Option<String>, Option<String>) {
    use chrono::Datelike;
    let Some(months) = preset.months() else {
        return (None, None);
    };
    let total = i64::from(today.year()) * 12 + i64::from(today.month() - 1) - months;
    let target_year = total.div_euclid(12);
    let target_month = total.rem_euclid(12) as u32 + 1;
    let day = today.day().min(days_in_month(target_year, target_month));
    let start = chrono::NaiveDate::from_ymd_opt(target_year as i32, target_month, day);
    (start.map(|d| d.format("%Y-%m-%d").to_string()), None)
}

/// 某月天数（month 1-12；闰年按外部日历）。
fn days_in_month(year: i64, month: u32) -> u32 {
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && is_leap_year(year) {
        29
    } else {
        MONTH_DAYS[(month - 1) as usize]
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// 装配快照与格文本（装配层：渲染期零格式化——两页签全部格文本一次性产出；
// 纯函数离线单测面）
// ---------------------------------------------------------------------------

/// 涨跌色消费面（旧壳 pnlSemanticColor 同语义：红涨绿跌、正/零归涨）。
pub(crate) fn pnl_semantic(cents: i64) -> assembly::PnlSemantic {
    if cents >= 0 {
        assembly::PnlSemantic::Gain
    } else {
        assembly::PnlSemantic::Loss
    }
}

/// 概览页签读数快照（域 InvestmentOverview 的 Clone/PartialEq 镜像）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct OverviewSnap {
    pub native_currency: String,
    pub investable_assets_cents: i64,
    pub investment_cash_cents: i64,
    pub holdings_market_value_cents: i64,
    pub total_market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub missing_price_holding_count: i64,
    pub has_investment_account: bool,
}

impl OverviewSnap {
    pub(crate) fn of_domain(o: &ledger_investment::InvestmentOverview) -> Self {
        Self {
            native_currency: o.native_currency.clone(),
            investable_assets_cents: o.investable_assets_cents,
            investment_cash_cents: o.investment_cash_cents,
            holdings_market_value_cents: o.holdings_market_value_cents,
            total_market_value_cents: o.total_market_value_cents,
            unrealized_pnl_cents: o.unrealized_pnl_cents,
            cumulative_pnl_cents: o.cumulative_pnl_cents,
            missing_price_holding_count: o.missing_price_holding_count,
            has_investment_account: o.has_investment_account,
        }
    }
}

/// 金额格式化（隐私置位即掩码；口径与全壳 formatAmount 掩码同源；币种
/// 展示参数 = 本位币的字典投影）。
pub(crate) fn format_amount_privacy(cents: i64, fmt: &CurrencyFormat, privacy: bool) -> String {
    if privacy {
        AMOUNT_PRIVACY_MASK.to_owned()
    } else {
        assembly::format_amount_basic(cents, fmt)
    }
}

/// 各腿独立占比（旧壳 sharePct 字面，spec #1684 用户故事 27）：总额 ≤ 0 不
/// 产段宽；钳 [0, 100] 防异常输入出负宽/超宽。展示派生纯图形——不复述
/// 「两腿之和 = 可投资资产」口径（后端两腿失配时比例条如实出现缺口而非被
/// 余量抹平），不产生任何用户可见数字。
pub(crate) fn share_pct(leg_cents: i64, total_cents: i64) -> f64 {
    if total_cents <= 0 {
        return 0.0;
    }
    ((leg_cents as f64 / total_cents as f64) * 100.0).clamp(0.0, 100.0)
}

/// 概览页签的格装配闭集（privacy 快照进，文本全部一次性产出）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OverviewCells {
    pub hero_text: String,
    pub currency_label: String,
    pub cash_leg_text: String,
    pub holdings_leg_text: String,
    /// 构成比例条可见性（可投资资产为 0 或隐私开启整条不渲染——空轨道易被
    /// 误读为残缺，隐私时段宽绕过统一格式化函数按最严口径一并隐藏；旧壳
    /// spec #1684 显式裁量的本壳等价。两处隐藏的都只是图形——金额、图例与
    /// 引导语义照常）。
    pub show_composition: bool,
    pub cash_share_pct: f64,
    pub holdings_share_pct: f64,
    pub market_value_text: String,
    pub unrealized_pnl_text: String,
    pub cumulative_pnl_text: String,
    pub unrealized_pnl_semantic: Option<assembly::PnlSemantic>,
    pub cumulative_pnl_semantic: Option<assembly::PnlSemantic>,
    /// 缺价持仓显式说明（>0 才出——不虚增也不静默低估）。
    pub missing_price_note: Option<String>,
    /// 无投资账户引导句（数字照常 0，不隐藏功能）。
    pub no_account_note: Option<String>,
}

pub(crate) fn assemble_overview(
    overview: &OverviewSnap,
    currencies: &crate::assembly::CurrencyTable,
    privacy: bool,
) -> OverviewCells {
    let fmt = currencies.format_of(&overview.native_currency);
    OverviewCells {
        hero_text: format_amount_privacy(overview.investable_assets_cents, &fmt, privacy),
        currency_label: CURRENCY_LABEL_FMT.replace("{currency}", &overview.native_currency),
        cash_leg_text: format!(
            "{} {}",
            CASH_LEG,
            format_amount_privacy(overview.investment_cash_cents, &fmt, privacy)
        ),
        holdings_leg_text: format!(
            "{} {}",
            HOLDINGS_LEG,
            format_amount_privacy(overview.holdings_market_value_cents, &fmt, privacy)
        ),
        show_composition: !privacy && overview.investable_assets_cents > 0,
        cash_share_pct: share_pct(
            overview.investment_cash_cents,
            overview.investable_assets_cents,
        ),
        holdings_share_pct: share_pct(
            overview.holdings_market_value_cents,
            overview.investable_assets_cents,
        ),
        market_value_text: format_amount_privacy(overview.total_market_value_cents, &fmt, privacy),
        unrealized_pnl_text: format_amount_privacy(overview.unrealized_pnl_cents, &fmt, privacy),
        cumulative_pnl_text: format_amount_privacy(overview.cumulative_pnl_cents, &fmt, privacy),
        unrealized_pnl_semantic: Some(pnl_semantic(overview.unrealized_pnl_cents)),
        cumulative_pnl_semantic: Some(pnl_semantic(overview.cumulative_pnl_cents)),
        missing_price_note: (overview.missing_price_holding_count > 0).then(|| {
            MISSING_PRICE_NOTE_FMT
                .replace("{count}", &overview.missing_price_holding_count.to_string())
        }),
        no_account_note: (!overview.has_investment_account).then(|| NO_ACCOUNT_NOTE.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// 盈亏页签：快照与格文本装配
// ---------------------------------------------------------------------------

/// 盈亏页客户端切片页大小（旧壳 PNL_PAGE_SIZE 单源同值：固定 8 不设选择器，
/// 两表共享）。
pub(crate) const PNL_PAGE_SIZE: usize = 8;

/// 按年行快照（域 YearPnl 镜像：Clone/PartialEq 约束面）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct YearPnlSnap {
    pub year: String,
    pub realized_pnl_cents: Option<i64>,
    pub dividend_cents: Option<i64>,
    pub realized_gain_cents: Option<i64>,
}

/// 按账户行快照（域 AccountPnl 镜像）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AccountPnlSnap {
    pub account_id: String,
    pub account_name: String,
    pub realized_pnl_cents: Option<i64>,
    pub dividend_cents: Option<i64>,
    pub realized_gain_cents: Option<i64>,
}

/// 盈亏汇总快照（域 RealizedPnlSummary 镜像；本位币 = 首行行集透传的两表
/// 共用基准，两表金额格式化面）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PnlSnap {
    pub native_currency: String,
    pub by_year: Vec<YearPnlSnap>,
    pub by_account: Vec<AccountPnlSnap>,
}

impl PnlSnap {
    pub(crate) fn of_domain(s: &ledger_investment::RealizedPnlSummary) -> Self {
        Self {
            native_currency: s
                .by_year
                .first()
                .map(|r| r.native_currency.clone())
                .or_else(|| s.by_account.first().map(|r| r.native_currency.clone()))
                .unwrap_or_default(),
            by_year: s
                .by_year
                .iter()
                .map(|r| YearPnlSnap {
                    year: r.year.clone(),
                    realized_pnl_cents: r.realized_pnl_cents,
                    dividend_cents: r.dividend_cents,
                    realized_gain_cents: r.realized_gain_cents,
                })
                .collect(),
            by_account: s
                .by_account
                .iter()
                .map(|r| AccountPnlSnap {
                    account_id: r.account_id.clone(),
                    account_name: r.account_name.clone(),
                    realized_pnl_cents: r.realized_pnl_cents,
                    dividend_cents: r.dividend_cents,
                    realized_gain_cents: r.realized_gain_cents,
                })
                .collect(),
        }
    }
}

/// 单值行格（两张单值表共用形态，旧壳 realizedGainColumn 单点同款）：主值
///（已实现盈亏 + 现金分红，逐腿按事件周汇率折算——后端折算好的分，前端零
/// 算术）；任一腿缺料（三项全 None）→ 格内只显式「无法计算」，不出部分
/// 数字、不以零计入（旧壳同规则：半截数字由装配面排除）。隐私态主值与两腿
/// 一并掩码（方向不再着色，与全壳掩码同源）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RealizedGainCells {
    /// 三项全 None = 显式「无法计算」；数值不渲染。
    pub computable: bool,
    pub gain_text: Option<String>,
    pub gain_semantic: Option<assembly::PnlSemantic>,
    pub realized_leg_text: Option<String>,
    pub dividend_leg_text: Option<String>,
}

pub(crate) fn assemble_pnl_row(
    realized_pnl_cents: Option<i64>,
    dividend_cents: Option<i64>,
    realized_gain_cents: Option<i64>,
    fmt: &CurrencyFormat,
    privacy: bool,
) -> RealizedGainCells {
    let computable =
        realized_pnl_cents.is_some() && dividend_cents.is_some() && realized_gain_cents.is_some();
    let gain_semantic = realized_gain_cents.map(pnl_semantic);
    match (computable, privacy) {
        (true, false) => RealizedGainCells {
            computable: true,
            gain_text: Some(format_amount_privacy(
                realized_gain_cents.unwrap_or(0),
                fmt,
                false,
            )),
            gain_semantic,
            realized_leg_text: Some(format_amount_privacy(
                realized_pnl_cents.unwrap_or(0),
                fmt,
                false,
            )),
            dividend_leg_text: Some(format_amount_privacy(
                dividend_cents.unwrap_or(0),
                fmt,
                false,
            )),
        },
        (true, true) => RealizedGainCells {
            computable: true,
            gain_text: Some(AMOUNT_PRIVACY_MASK.to_owned()),
            gain_semantic,
            realized_leg_text: Some(AMOUNT_PRIVACY_MASK.to_owned()),
            dividend_leg_text: Some(AMOUNT_PRIVACY_MASK.to_owned()),
        },
        (false, _) => RealizedGainCells {
            computable: false,
            gain_text: None,
            gain_semantic: None,
            realized_leg_text: None,
            dividend_leg_text: None,
        },
    }
}

/// 主值文本（不可计算行只给「无法计算」，不给半截数字）。
pub(crate) fn realized_gain_text(cells: &RealizedGainCells) -> String {
    if cells.computable {
        cells.gain_text.clone().unwrap_or_default()
    } else {
        PNL_NOT_COMPUTABLE.to_owned()
    }
}

/// 副行两腿文本（不可计算行返回 None——格内只留主值标注；旧壳 subLine 同
/// 形 `{已实现盈亏} X · {现金分红} Y`）。
pub(crate) fn realized_legs_text(cells: &RealizedGainCells) -> Option<String> {
    if !cells.computable {
        return None;
    }
    Some(format!(
        "{} {} · {} {}",
        PNL_COL_REALIZED_PNL,
        cells.realized_leg_text.clone().unwrap_or_default(),
        PNL_COL_DIVIDEND,
        cells.dividend_leg_text.clone().unwrap_or_default()
    ))
}

/// 按年页行集（后端升序返回，此处倒转为最新年在前——旧壳 yearRows 同口径：
/// 第 1 页 = 最近年份是分页可用的前提，看盈亏先看最近表现）。纯函数、可
/// 离线单测。
pub(crate) fn year_rows_desc(rows: &[YearPnlSnap]) -> Vec<YearPnlSnap> {
    let mut sorted: Vec<YearPnlSnap> = rows.to_vec();
    sorted.sort_by(|a, b| b.year.cmp(&a.year));
    sorted
}

/// 有效页数（≥1；行集空 = 一页）。
pub(crate) fn total_pages(row_count: usize) -> usize {
    row_count.div_ceil(PNL_PAGE_SIZE).max(1)
}

/// 客户端切片分页（旧壳「两表行集一次全量拉取，翻页只是内存展示切片」同
/// 口径；不发请求不参数化查询）：返回本页切片；页码越界取末页（读出口整
/// 面，调用方在装载完成时写回钳制值）。
pub(crate) fn page_slice<T: Clone>(rows: &[T], page: usize) -> Vec<T> {
    let valid = total_pages(rows.len());
    let page = page.max(1).min(valid);
    let start = (page - 1) * PNL_PAGE_SIZE;
    rows.iter()
        .skip(start)
        .take(PNL_PAGE_SIZE)
        .cloned()
        .collect()
}

/// 收益率行快照（域 MwrRow 镜像 + 口径枚举）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MwrRowSnap {
    /// 行作用域文本（账户行 = 账户名；全账行 = 「全账」）。
    pub scope: String,
    pub currency_code: String,
    pub basis: MwrBasis,
    pub rate: Option<f64>,
    /// 账户行 id（headless 行键；全账行 None）。
    pub account_id: Option<String>,
}

/// 收益率口径（域 MwrBasis 镜像：两口径不可互算，行自带标记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MwrBasis {
    /// 年化内部收益率（XIRR）。
    Annualized,
    /// 未年化（累计收益 ÷ 累计投入；建仓时点未知）。
    Cumulative,
}

impl MwrBasis {
    fn of_domain(basis: ledger_investment::MwrBasis) -> Self {
        match basis {
            ledger_investment::MwrBasis::Annualized => MwrBasis::Annualized,
            ledger_investment::MwrBasis::Cumulative => MwrBasis::Cumulative,
        }
    }
}

/// MWR 行集装配（旧壳 mwrRows 同口径）：账户行 = by_account 随账户筛选收窄
///（客户端过滤，行集本就全量返回）；全账行 = total 全集按币种分组、不跨
/// 币种折算、不随筛选收窄。行序 = 账户行在前、全账行在后（旧壳拼接序）。
pub(crate) fn assemble_mwr_rows(
    summary: &ledger_investment::MoneyWeightedReturnSummary,
    account_filter: Option<&str>,
    scope_of_account: impl Fn(&str) -> Option<String>,
) -> Vec<MwrRowSnap> {
    let mut rows = Vec::new();
    for row in &summary.by_account {
        if let Some(filter) = account_filter
            && row.account_id != filter
        {
            continue;
        }
        let scope = scope_of_account(&row.account_id).unwrap_or_else(|| row.account_name.clone());
        rows.push(MwrRowSnap {
            scope,
            currency_code: row.currency_code.clone(),
            basis: MwrBasis::of_domain(row.basis),
            rate: row.rate,
            account_id: Some(row.account_id.clone()),
        });
    }
    for row in &summary.total {
        rows.push(MwrRowSnap {
            scope: PNL_TOTAL.to_owned(),
            currency_code: row.currency_code.clone(),
            basis: MwrBasis::of_domain(row.basis),
            rate: row.rate,
            account_id: None,
        });
    }
    rows
}

/// 收益率行文本（旧壳 renderMwrRateCell 三态单点同款）：可计算 = 百分数
///（formatRate 口径）+ 未年化角标「*」（ADR-0115 防误读的在场标注）；无解
/// = 「无法计算」显式标注不猜解。
pub(crate) fn rate_text_of(row: &MwrRowSnap) -> String {
    match row.rate {
        Some(rate) => {
            let marker = if row.basis == MwrBasis::Cumulative {
                "*"
            } else {
                ""
            };
            format!("{}{marker}", format_rate(rate))
        }
        None => PNL_NOT_COMPUTABLE.to_owned(),
    }
}

/// 百分数展示（旧壳 formatRate 同口径：小数 → 百分数，两位小数、非零恒带
/// 符号、零不带符号）。纯函数、可离线单测。
pub(crate) fn format_rate(rate: f64) -> String {
    let percents = rate * 100.0;
    let fixed = format!("{percents:.2}");
    if rate > 0.0 {
        format!("+{fixed}%")
    } else {
        format!("{fixed}%")
    }
}

// ---------------------------------------------------------------------------
// 走势卡：快照与空态三态装配
// ---------------------------------------------------------------------------

/// 走势快照（域 PortfolioValueTrend 的消费面镜像：周采样序列 + 补全三态）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TrendSnap {
    /// 序列（周点：周一 ISO 日期标签；分位币「分」值；缺价周不出点——
    /// 曲线缺值跨越语义随 wrapper）。
    pub dates: Vec<String>,
    pub values: Vec<Option<i64>>,
    pub currency_code: String,
    /// 补全三态（仅空序列且库内有通道无历史序列标的时在场，域读投影直出）。
    pub backfill: Option<ledger_investment::TrendBackfillState>,
    pub backfill_done: Option<usize>,
    pub backfill_total: Option<usize>,
}

impl TrendSnap {
    /// 采样点数（两列等长以少者计——域保证等长，此为越界兜底）。
    pub(crate) fn len(&self) -> usize {
        self.dates.len().min(self.values.len())
    }

    /// 序列是否为空（空态判定）。
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 空态补全说明句（旧壳 emptyExtra 三态同词表；不指向同步按钮）。
    pub(crate) fn empty_note(&self) -> String {
        match self.backfill {
            Some(ledger_investment::TrendBackfillState::Running) => {
                match (self.backfill_done, self.backfill_total) {
                    (Some(done), Some(total)) => TREND_EMPTY_BACKFILL_PROGRESS_FMT
                        .replace("{done}", &done.to_string())
                        .replace("{total}", &total.to_string()),
                    _ => TREND_EMPTY_BACKFILL_RUNNING.to_owned(),
                }
            }
            Some(ledger_investment::TrendBackfillState::RetryPending) => {
                TREND_EMPTY_RETRY_PENDING.to_owned()
            }
            Some(ledger_investment::TrendBackfillState::NoData) => TREND_EMPTY_NO_DATA.to_owned(),
            // 无补全字段（区间裁剪 / 持仓与价格周错开等）：既有空态文案的
            // 中性改写（旧壳同口径）。
            None => TREND_EMPTY_NO_POINTS.to_owned(),
        }
    }
}

/// 组合走势装载（域读 → 快照；序列、币种与补全状态一次产出）。
pub(crate) fn assemble_trend(trend: &ledger_investment::PortfolioValueTrend) -> TrendSnap {
    TrendSnap {
        dates: trend.points.iter().map(|p| p.date.clone()).collect(),
        values: trend
            .points
            .iter()
            .map(|p| Some(p.market_value_cents))
            .collect(),
        currency_code: trend.currency_code.clone(),
        backfill: trend.backfill.as_ref().map(|b| b.state),
        backfill_done: trend.backfill.as_ref().and_then(|b| b.done),
        backfill_total: trend.backfill.as_ref().and_then(|b| b.total),
    }
}

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

/// 投资视图（导航骨架投资分支的真实组件）：页签容器（概览/盈亏/持仓）+
/// 价格过期提示条（#2046 视图层横切位，旧壳 InvestmentsView 告警位同位）
/// + 三页签本体。「删除即红」负向条目钉在 tests/app_headless.rs 与 lib 探针。
#[component]
pub(crate) fn InvestmentsView() -> impl IntoElement {
    let session = store();
    // 落位意图消费（读一次封闸由 take_intent 单点承担；#2047 起两道通道：
    // investments_drill 持仓下钻 → 落明细页签 + 筛选维覆盖；focus 标的定位
    // → 落标的页签 + anchor 定位意图。删除本接线即红：钉 headless 下钻定位
    // 用例与 lib 探针）。effect 键当前视图——落位与意图同轮落地。
    effect(move || {
        let _landed = nav::store().current.get();
        let Some(intent) = nav::take_intent() else {
            return;
        };
        if let Some(drill) = intent.investments_drill {
            // 明细下钻：页签指向收窄（仅 detail 在场，旧壳 tab=detail 同词）；
            // 筛选维在场永远赢、缺席维度保留保留态；翻页归零（筛选变化即换
            // 页面集——旧壳 store 内化同口径）。
            if drill.tab == LEDGER_TAB_URL_VALUE {
                if drill.account_id.is_some() {
                    session.detail_account_id.set(drill.account_id);
                }
                if drill.instrument_id.is_some() {
                    session.detail_instrument_id.set(drill.instrument_id);
                }
                session.detail_page.set(1);
                session.tab.set(InvestmentsTab::Ledger);
            }
        } else if let Some(focus) = intent.focus {
            // 标的定位（spec #704 / #1907）：单标的走势退役后焦点面归标的
            // 页签——anchor 分页定位让行必在当前页（无效 focus 在 anchor
            // 查询内回落普通页码，不提供落空的跳转）。
            session.instruments_focus.set(Some(focus));
            session.tab.set(InvestmentsTab::Instruments);
        }
    });

    // 到访标记（visited-once keep-mounted：先到的页签保持挂载，页签切换不
    // 重挂重拉——旧壳 display-directive="show" + key 纪律的本壳对应物）。
    let visited: Signal<Vec<bool>> = signal({
        let mut flags = vec![false; TABS.len()];
        flags[0] = true; // 默认首签挂载即呈现
        flags
    });
    effect(move || {
        let current = session.tab.get();
        if let Some(index) = TABS.iter().position(|tab| *tab == current) {
            visited.update(|flags: &mut Vec<bool>| flags[index] = true);
        }
    });

    let palette = views::view_palette();
    let tab_bar = view()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(nsh_base::tabs().indicator_class("bg-accent").child(
            view().flex_row().gap_2().children(TABS.map(|tab| {
                let is_active = move || session.tab.get() == tab;
                nsh_base::tab()
                    .selected(is_active)
                    .accessibility_label(tab.label())
                    .app_class("px-2 py-1 rounded-md selected:bg-accent")
                    .child(
                        text(tab.label())
                            .text_size(px(13.0))
                            .text_color(move || {
                                if is_active() {
                                    palette.fg
                                } else {
                                    palette.muted
                                }
                            })
                            .test_id(format!("inv-tab-{}", tab.id_str()).as_str()),
                    )
                    .on_press(move |_: &()| session.tab.set(tab))
            })),
        ));

    // 页签宿主闭集（visited-once keep-mounted；激活切换只变 display 不重挂
    // ——顺序呈现，卸载重建即红钉 headless keep-mounted 探针）。
    let content_host: View = view()
        .flex_1()
        .min_h(px(0.0))
        .app_class("overflow-hidden")
        .children(TABS.map(|tab| keep_mounted_pane(tab, session, visited)));

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background overflow-hidden")
        .child(
            view().px_4().py_2().child(
                text("投资")
                    .text_size(px(16.0))
                    .text_color(palette.fg)
                    .test_id("view-title"),
            ),
        )
        .child(tab_bar)
        .child(
            // 价格过期提示条（#2046）：本地水位检查（零网络），有过期或持仓
            // 缺现价才出场；同步实际写价后随失效信号自动消失（旧壳 staleness
            // alert 同位同语义）。
            crate::investment_sync::staleness_alert(),
        )
        .child(content_host)
}

/// keep-mounted 页签宿主：未到访不挂载（不预载）；已到访宿主随激活状态
/// display 切换（隐藏样式，退出布局流）。
fn keep_mounted_pane(
    tab: InvestmentsTab,
    session: InvestmentsSession,
    visited: Signal<Vec<bool>>,
) -> impl IntoElement {
    let index = TABS.iter().position(|t| *t == tab).unwrap_or(0);
    when(
        move || visited.get()[index],
        move || {
            let pane = match tab {
                InvestmentsTab::Overview => overview_pane().into_element(),
                InvestmentsTab::Pnl => pnl_pane().into_element(),
                InvestmentsTab::Holdings => {
                    crate::investments_holdings::holdings_pane().into_element()
                }
                InvestmentsTab::Ledger => crate::investments_ledger::ledger_pane().into_element(),
                InvestmentsTab::Instruments => {
                    crate::investments_instruments::instruments_pane().into_element()
                }
            };
            view()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .style_when(move || session.tab.get() != tab, hide_when_inactive)
                .child(pane)
        },
    )
    .otherwise(view)
}

/// 不活动页签宿主的隐藏样式（display none；独立 fn 让删除映射即编译红）。
fn hide_when_inactive(
    style: nsh_core::element::StyleRefinement,
) -> nsh_core::element::StyleRefinement {
    use nsh_core::element::Styled;
    style.hidden()
}

/// 主题令牌调色板（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
#[derive(Clone, Copy)]
pub(crate) struct PanePalette {
    pub(crate) fg: nsh_core::element::AdaptiveColor,
    pub(crate) muted: nsh_core::element::AdaptiveColor,
    pub(crate) danger: nsh_core::element::AdaptiveColor,
    /// 盈亏涨跌色（红涨绿跌；dashboard investment_stat 同色值，红涨与交易
    /// 语义色正交——词汇表「盈亏涨跌色」）。
    pub(crate) gain: nsh_core::element::AdaptiveColor,
    pub(crate) loss: nsh_core::element::AdaptiveColor,
    /// 口径说明触发点信息蓝。
    pub(crate) info: nsh_core::element::AdaptiveColor,
}

pub(crate) fn pane_palette() -> PanePalette {
    let tokens = nsh_core::element::tokens();
    PanePalette {
        fg: tokens
            .color("foreground")
            .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into()),
        muted: tokens
            .color("muted-foreground")
            .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into()),
        danger: tokens
            .color("destructive")
            .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into()),
        gain: nsh_core::element::AdaptiveColor::new(rgb(0xd03050), rgb(0xe88080)),
        loss: nsh_core::element::AdaptiveColor::new(rgb(0x18a058), rgb(0x63e2b7)),
        info: nsh_core::element::AdaptiveColor::new(rgb(0x2080f0), rgb(0x63a8f2)),
    }
}

/// 语义色 → 适配色（涨跌色消费面：涨/跌/中性三态归本单点，不外溢到市值）。
pub(crate) fn pnl_color(
    semantic: Option<assembly::PnlSemantic>,
    palette: &PanePalette,
    neutral: &nsh_core::element::AdaptiveColor,
) -> nsh_core::element::AdaptiveColor {
    match semantic {
        Some(assembly::PnlSemantic::Gain) => palette.gain,
        Some(assembly::PnlSemantic::Loss) => palette.loss,
        None => *neutral,
    }
}

/// 概念说明触发点（常驻 ⓘ 形态；文案归 popover_tip 调用方）。
pub(crate) fn info_dot(info: nsh_core::element::AdaptiveColor) -> impl IntoElement {
    text("ⓘ").text_size(px(11.0)).text_color(info)
}

/// 卡容器（旧壳 NCard 的本壳等价形态：边框 + 圆角 + 内边距，reports.rs
/// 先例同形）。
pub(crate) fn card_container(test_id: &'static str) -> View {
    view()
        .test_id(test_id)
        .flex_col()
        .gap_3()
        .rounded_lg()
        .mx_4()
        .my_2()
        .p_4()
        .app_class("border border-border bg-background")
}

/// 卡内错误行 + 重试（重试 = 重发同一条读命令，不产生任何写入；旧壳卡内
/// warning + retry 同语义）。
pub(crate) fn retry_row(
    message: impl Fn() -> String + Copy + 'static,
    error_id: &'static str,
    retry_id: &'static str,
    palette: PanePalette,
    retry: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            text(message)
                .text_size(px(12.0))
                .text_color(palette.danger)
                .test_id(error_id),
        )
        .child(
            view()
                .on_press(move |_: &PressEvent| retry())
                .rounded_md()
                .px_1()
                .py_0p5()
                .app_class("hover:bg-accent")
                .child(
                    text(RETRY)
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .test_id(retry_id),
                ),
        )
}

/// 图例色点。
fn legend_dot(color: nsh_core::element::AdaptiveColor) -> impl IntoElement {
    view().w(px(8.0)).h(px(8.0)).rounded_sm().bg(color)
}

// ---------------------------------------------------------------------------
// 概览页签
// ---------------------------------------------------------------------------

/// 概览页签：投资概览卡（可投资资产 Hero + 构成比例条与图例 + 投资合计
/// 三项 + 缺价/无账户说明）+ 组合走势卡（走势图 + 区间切换 + 空态三态）。
/// 卡面按子视图变量展开（减少嵌套深度；测试锚点见 `tests/app_headless.rs`
/// 投资视图组与 lib 装配单测）。
fn overview_pane() -> impl IntoElement {
    let palette = pane_palette();
    let session = store();
    let cells = signal(None::<OverviewCells>);
    let error = signal(None::<String>);
    let privacy = signal(false);
    let trend = signal(None::<TrendSnap>);
    let trend_error = signal(None::<String>);
    // 币种字典（y 轴刻度与本位币标注的折算面；随概览批一并读，旧壳
    // reference store 口径）。声明提前至信号束（format_y 闭包消费面）。
    let currencies = signal(crate::assembly::CurrencyTable::default());

    // 概览读数装载（首屏 + 失效信号重读共用；阻塞读走后台线程，SQLite IO
    // 不占引擎线程；隐私开关装载时机读取）。
    let load_overview = move || {
        error.set(None);
        let privacy_enabled = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                error.set(Some(MSG_INTERNAL.to_owned()));
                return;
            };
            let outcome = blocking(
                move || -> ledger_infra::error::Result<
                    (OverviewSnap, Vec<ledger_currencies::Currency>),
                > {
                    let overview = boot::investment_overview(&db)?;
                    let currencies = boot::currency_rows(&db)?;
                    Ok((OverviewSnap::of_domain(&overview), currencies))
                },
            )
            .await;
            match outcome {
                Ok(Ok((snap, currency_rows))) => {
                    privacy.set(privacy_enabled);
                    cells.set(Some(assemble_overview(
                        &snap,
                        &crate::assembly::CurrencyTable::from_currencies(&currency_rows),
                        privacy_enabled,
                    )));
                    currencies.set(crate::assembly::CurrencyTable::from_currencies(
                        &currency_rows,
                    ));
                    error.set(None);
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some(MSG_INTERNAL.to_owned())),
            }
        });
    };

    // 走势装载（预设变化重读；失效信号重读——每次进出现拉，保留的是选择
    // 不是快照，旧壳同口径）。
    let load_trend = move || {
        trend_error.set(None);
        let preset = session.trend_preset.get_untracked();
        let (start, end) = to_trend_range(preset, chrono::Local::now().date_naive());
        let range = ledger_investment::TrendRange {
            start_date: start,
            end_date: end,
        };
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                trend_error.set(Some(MSG_INTERNAL.to_owned()));
                return;
            };
            let outcome = blocking(move || boot::portfolio_value_trend(&db, &range)).await;
            match outcome {
                Ok(Ok(t)) => {
                    trend.set(Some(assemble_trend(&t)));
                    trend_error.set(None);
                }
                Ok(Err(e)) => trend_error.set(Some(e.to_string())),
                Err(_) => trend_error.set(Some(MSG_INTERNAL.to_owned())),
            }
        });
    };

    // 预设切换：真变化才重拉（同值守卫；删除接线即红钉 headless 区间切换
    // 用例）。
    let on_preset_change = move |next: TrendPreset| {
        let changed = session.trend_preset.get_untracked() != next;
        session.trend_preset.set(next);
        if changed {
            load_trend();
        }
    };

    // 失效信号（投资族：价格/持仓侧数据变化 → 两卡重读；写路径广播接线归
    // ⑮/⑯ 写路径票，消费面先行）。句柄随视图实例存亡。
    let invest_sub = signals::subscribe_investments_changed(move || {
        load_overview();
        load_trend();
    });
    effect(move || {
        let held: &signals::InvestmentsChangedSubscription = &invest_sub;
        let _ = held;
    });

    // 首屏装载。
    load_overview();
    load_trend();

    // 币种字典（y 轴刻度与走势币种标注的折算面；两卡装载批一并读，成本
    // 可忽略，旧壳 reference store 口径）。
    // y 轴刻度文本化闭包（金额口径单源 assembly；隐私态统一掩码）。
    let format_y = move |v: f64| {
        if privacy.get_untracked() {
            return AMOUNT_PRIVACY_MASK.to_owned();
        }
        let native = trend
            .get_untracked()
            .map(|s| s.currency_code)
            .unwrap_or_default();
        assembly::format_amount_f64(v, &currencies.get_untracked().format_of(&native))
    };

    // ---- Hero 段（首屏焦点）：标签（常驻 ⓘ）+ 本位币标注 + 大号数字 ----
    let hero = view()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            text(INVESTABLE_ASSETS)
                                .text_size(px(12.0))
                                .text_color(palette.muted)
                                .test_id("overview-investable-assets"),
                        )
                        .child(popover_tip(
                            "overview-investable-assets-info",
                            vec![INVESTABLE_ASSETS_TIP.to_owned()],
                            info_dot(palette.info),
                        )),
                )
                .child(
                    text(move || {
                        cells
                            .get()
                            .map(|c| c.currency_label.clone())
                            .unwrap_or_default()
                    })
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .test_id("overview-currency-label"),
                ),
        )
        .child(
            text(move || cells.get().map(|c| c.hero_text.clone()).unwrap_or_default())
                .text_size(px(28.0))
                .text_color(palette.fg)
                .test_id("overview-hero-value"),
        );

    // ---- 图例（色点 + 两腿标签与格式化金额；比例条 aria-hidden 等价信息） ----
    let legend = view()
        .flex_row()
        .gap_4()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(legend_dot(cash_leg_color()))
                .child(
                    text(move || {
                        cells
                            .get()
                            .map(|c| c.cash_leg_text.clone())
                            .unwrap_or_default()
                    })
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .test_id("overview-cash-leg"),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(legend_dot(palette.fg))
                .child(
                    text(move || {
                        cells
                            .get()
                            .map(|c| c.holdings_leg_text.clone())
                            .unwrap_or_default()
                    })
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .test_id("overview-holdings-leg"),
                ),
        );

    // ---- 投资合计三项（盈亏两数着涨跌色，总市值中性） ----
    let totals = view()
        .flex_col()
        .gap_1()
        .child(
            text(TOTALS_TITLE)
                .text_size(px(12.0))
                .text_color(palette.muted)
                .test_id("overview-totals-title"),
        )
        .child(
            view()
                .flex_row()
                .gap_8()
                .child(view().flex_col().child(overview_stat(
                    MARKET_VALUE_LABEL,
                    "overview-total-market-value",
                    cells.read_only(),
                    || None,
                    MARKET_VALUE_TIP,
                    palette,
                )))
                .child(overview_stat(
                    UNREALIZED_PNL_LABEL,
                    "overview-unrealized-pnl",
                    cells.read_only(),
                    move || cells.get().and_then(|c| c.unrealized_pnl_semantic),
                    UNREALIZED_PNL_TIP,
                    palette,
                ))
                .child(overview_stat(
                    CUMULATIVE_PNL_LABEL,
                    "overview-cumulative-pnl",
                    cells.read_only(),
                    move || cells.get().and_then(|c| c.cumulative_pnl_semantic),
                    CUMULATIVE_PNL_TIP,
                    palette,
                )),
        );

    // ---- 缺价说明与无账户引导（条件出场） ----
    let missing_note = when(
        move || cells.get().is_some_and(|c| c.missing_price_note.is_some()),
        move || {
            text(move || {
                cells
                    .get()
                    .and_then(|c| c.missing_price_note.clone())
                    .unwrap_or_default()
            })
            .text_size(px(12.0))
            .text_color(palette.muted)
            .test_id("overview-missing-price")
        },
    )
    .otherwise(view);
    let no_account_note = when(
        move || cells.get().is_some_and(|c| c.no_account_note.is_some()),
        move || {
            text(move || {
                cells
                    .get()
                    .and_then(|c| c.no_account_note.clone())
                    .unwrap_or_default()
            })
            .text_size(px(12.0))
            .text_color(palette.muted)
            .test_id("overview-no-account")
        },
    )
    .otherwise(view);

    // ---- 概览卡（报错整卡警告 + 重试；数据就绪后呈现） ----
    let overview_body = view()
        .flex_col()
        .gap_3()
        .child(hero)
        .child(composition_row(cells.read_only(), palette))
        .child(legend)
        .child(totals)
        .child(missing_note)
        .child(no_account_note);
    let overview_card = card_container("investment-overview")
        .child(
            text(OVERVIEW_TITLE)
                .text_size(px(15.0))
                .text_color(palette.fg)
                .test_id("overview-title"),
        )
        .child(
            when(
                move || error.get().is_some(),
                move || {
                    retry_row(
                        move || error.get().unwrap_or_default(),
                        "overview-error",
                        "overview-retry",
                        palette,
                        load_overview,
                    )
                },
            )
            .otherwise(move || overview_body.clone().into_element()),
        );

    // ---- 组合走势卡 ----
    // 图区每次组装现建（闭包内重建元素——Fn 闭包不可重复消费同一元素）。
    let trend_chart_of = move || {
        view()
            .test_id("trend-chart")
            .h(px(320.0))
            .child(plot_line::plot_line(
                move || {
                    let snapshot = trend.get().unwrap_or_default();
                    PlotLineSeries {
                        labels: snapshot.dates.clone(),
                        values: snapshot
                            .values
                            .iter()
                            .map(|cents| cents.map(|c| c as f64))
                            .collect(),
                    }
                },
                320.0,
                format_y,
            ))
    };
    let trend_body = when(
        move || trend_error.get().is_some(),
        move || {
            retry_row(
                move || trend_error.get().unwrap_or_default(),
                "trend-error",
                "trend-retry",
                palette,
                load_trend,
            )
        },
    )
    .otherwise(move || {
        when(
            move || trend.get().is_some_and(|s| !s.is_empty()),
            move || {
                view()
                    .flex_col()
                    .gap_2()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(TREND_MODE_PORTFOLIO)
                                    .text_size(px(12.0))
                                    .text_color(palette.muted),
                            )
                            .child(popover_tip(
                                "trend-concept",
                                vec![TREND_TIP.to_owned()],
                                info_dot(palette.info),
                            ))
                            .child(
                                text(move || {
                                    trend
                                        .get()
                                        .map(|s| trend_caption(&s.currency_code))
                                        .unwrap_or_default()
                                })
                                .text_size(px(12.0))
                                .text_color(palette.muted)
                                .test_id("trend-currency"),
                            ),
                    )
                    .child(trend_chart_of())
            },
        )
        .otherwise(move || {
            view().flex_col().child(
                text(move || {
                    trend
                        .get()
                        .map(|s| s.empty_note())
                        .unwrap_or_else(|| TREND_EMPTY_NO_POINTS.to_owned())
                })
                .text_size(px(13.0))
                .text_color(palette.muted)
                .test_id("trend-empty"),
            )
        })
    });
    let trend_card = card_container("overview-trend-card")
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(TREND_CARD_TITLE)
                        .text_size(px(15.0))
                        .text_color(palette.fg)
                        .test_id("trend-card-title"),
                )
                .child(
                    view().flex_row().gap_1().children(TREND_PRESETS.map(|p| {
                        let selected = move || session.trend_preset.get() == p;
                        nsh_base::button()
                            .selected(selected)
                            .app_class(
                                "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                            )
                            .child(
                                text(p.label())
                                    .text_size(px(11.0))
                                    .text_color(move || {
                                        if selected() {
                                            palette.fg
                                        } else {
                                            palette.muted
                                        }
                                    })
                                    .test_id(format!("trend-range-{}", p.label()).as_str()),
                            )
                            .on_press(move |_: &()| on_preset_change(p))
                    })),
                ),
        )
        .child(trend_body);

    // 页滚动承载（旧壳 NSpace vertical 页列；总高超视口时页滚动）。
    scroll()
        .flex_1()
        .child(view().flex_col().child(overview_card).child(trend_card))
}

/// 构成比例条轨道宽（pt；纯图形、不显示百分数——等价信息归图例文本）。
const COMPOSITION_TRACK_PT: f32 = 240.0;

/// 两段式构成比例条（段宽 = 各腿独立占比，钳 [0, 100]——两段各自独立按
/// 「各腿 ÷ 可投资资产」计算，不做 100−x 余量法，不复述「两腿之和 = 可
/// 投资资产」的口径不变量：后端两腿失配时比例条如实出现缺口而非被余量
/// 抹平——旧壳 spec #1684 同裁决的本壳等价）。可投资资产为 0 或隐私开启
/// 时整条缺席（调用方 cells 装配门）。
fn composition_row(
    cells: nsh_core::element::ReadSignal<Option<OverviewCells>>,
    palette: PanePalette,
) -> View {
    let cash_color = cash_leg_color();
    let holdings_color = palette.fg;
    view()
        .flex_row()
        .h(px(10.0))
        .w(px(COMPOSITION_TRACK_PT))
        .rounded_sm()
        .test_id("overview-composition-bar")
        .child(
            when(
                move || cells.get().is_some_and(|c| c.show_composition),
                move || {
                    let cash = cells.get().map(|c| c.cash_share_pct).unwrap_or_default();
                    let holdings = cells
                        .get()
                        .map(|c| c.holdings_share_pct)
                        .unwrap_or_default();
                    view()
                        .flex_row()
                        .h(px(10.0))
                        .child(
                            view()
                                .h(px(10.0))
                                .w(px(COMPOSITION_TRACK_PT * cash as f32 / 100.0))
                                .rounded_sm()
                                .bg(cash_color)
                                .test_id("overview-bar-cash"),
                        )
                        .child(
                            view()
                                .h(px(10.0))
                                .w(px(COMPOSITION_TRACK_PT * holdings as f32 / 100.0))
                                .rounded_sm()
                                .bg(holdings_color)
                                .test_id("overview-bar-holdings"),
                        )
                },
            )
            .otherwise(view),
        )
}

/// 两腿图例色源（现金 = 三级灰、持仓 = 主文本色——旧壳 investment-overview
/// css 中性两阶灰阶，随主题整体换装）。
fn cash_leg_color() -> nsh_core::element::AdaptiveColor {
    nsh_core::element::AdaptiveColor::new(
        nsh_core::color::hsla(0.0, 0.0, 0.55, 1.0),
        nsh_core::color::hsla(0.0, 0.0, 0.65, 1.0),
    )
}

/// 投资合计单格（标签 + 口径说明 + 数值着涨跌色或中性；数值文本随同一
/// 卡面信号响应式，失效重读后随信号更新）。
fn overview_stat(
    label: &'static str,
    test_id: &'static str,
    cells: nsh_core::element::ReadSignal<Option<OverviewCells>>,
    semantic: impl Fn() -> Option<assembly::PnlSemantic> + Copy + 'static,
    tip_body: &'static str,
    palette: PanePalette,
) -> impl IntoElement {
    let fg = palette.fg;
    view()
        .flex_col()
        .gap_1()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(text(label).text_size(px(12.0)).text_color(palette.muted))
                .child(popover_tip(
                    format!("{test_id}-info"),
                    vec![format!("{tip_body}{OVERVIEW_SCOPE_SUFFIX}")],
                    info_dot(palette.info),
                )),
        )
        .child(
            text(move || {
                cells
                    .get()
                    .map(|c| match label {
                        MARKET_VALUE_LABEL => c.market_value_text.clone(),
                        UNREALIZED_PNL_LABEL => c.unrealized_pnl_text.clone(),
                        _ => c.cumulative_pnl_text.clone(),
                    })
                    .unwrap_or_default()
            })
            .text_size(px(18.0))
            .text_color(move || pnl_color(semantic(), &palette, &fg))
            .test_id(format!("{test_id}-value").as_str()),
        )
}

// ---------------------------------------------------------------------------
// 盈亏页签
// ---------------------------------------------------------------------------

/// 盈亏页签：账户/标的筛选 + 按年/按账户两张单值表（客户端切片分页）+
/// 资金加权收益率卡。行集一次全量拉取，翻页是展示切片；筛选变化页码归零
/// 并重查。
fn pnl_pane() -> impl IntoElement {
    let palette = pane_palette();
    let session = store();

    // 筛选（旧壳 useRealizedPnl 同口径：视图作用域瞬态，不进会话槽——离开
    // 投资视图再回来为全新装载同果）。
    let account_id = signal(None::<String>);
    let instrument_id = signal(None::<String>);
    let account_options = signal(Vec::<CategoryOption>::new());
    let instrument_options = signal(Vec::<CategoryOption>::new());
    let account_names = signal(Vec::<(String, String)>::new());
    let summary = signal(None::<PnlSnap>);
    let mwr = signal(Vec::<MwrRowSnap>::new());
    let error = signal(None::<String>);
    let privacy = signal(false);
    // 币种字典（两表金额格的格式化面；随汇总批一并读，成本可忽略）。
    let pnl_currencies = signal(crate::assembly::CurrencyTable::default());

    // 汇总装载（首屏、筛选变化、失效信号共用；账户/标的筛选与两腿折算、
    // fx.rate-missing 码化上抛全在域内同一读事务收快照）。装载完成时两表
    // 页码钳到有效区间（越界回落写回——旧壳 clampedPage 读出口对账同口径；
    // 回设不触发重读，纯展示切片）。
    let load_summary = move || {
        error.set(None);
        let privacy_enabled = amount_privacy_enabled();
        let filter = ledger_investment::PnlFilter {
            account_id: account_id.get_untracked(),
            instrument_id: instrument_id.get_untracked(),
        };
        let names = account_names.get_untracked();
        // PnlFilter 无 Clone（域 Deserialize 载荷类型）；字段级克隆成对传入
        // 阻塞闭包（载荷面不复制，比照需用面）。
        let filter_account = filter.account_id.clone();
        let filter_instrument = filter.instrument_id.clone();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                error.set(Some(MSG_INTERNAL.to_owned()));
                return;
            };
            // 汇总与收益率两条命令独立（旧壳两 Loadable 并行同口径）；失败
            // 各自呈现，不互相拖垮。
            let outcome = blocking(
                move || -> ledger_infra::error::Result<(
                    PnlSnap,
                    ledger_investment::MoneyWeightedReturnSummary,
                    Vec<ledger_currencies::Currency>,
                )> {
                    let filter = ledger_investment::PnlFilter {
                        account_id: filter_account.clone(),
                        instrument_id: filter_instrument.clone(),
                    };
                    let summary = boot::realized_pnl_summary(&db, &filter)?;
                    let mwr = boot::money_weighted_return_summary(&db)?;
                    let currencies = boot::currency_rows(&db)?;
                    Ok((PnlSnap::of_domain(&summary), mwr, currencies))
                },
            )
            .await;
            match outcome {
                Ok(Ok((summary_snap, mwr_raw, currency_rows))) => {
                    pnl_currencies.set(crate::assembly::CurrencyTable::from_currencies(
                        &currency_rows,
                    ));
                    let mwr_rows =
                        assemble_mwr_rows(&mwr_raw, filter.account_id.as_deref(), |id| {
                            names.iter().find(|(k, _)| k == id).map(|(_, v)| v.clone())
                        });
                    privacy.set(privacy_enabled);
                    summary.set(Some(summary_snap.clone()));
                    mwr.set(mwr_rows);
                    error.set(None);
                    session.pnl_year_page.set(
                        session
                            .pnl_year_page
                            .get_untracked()
                            .max(1)
                            .min(total_pages(summary_snap.by_year.len())),
                    );
                    session.pnl_account_page.set(
                        session
                            .pnl_account_page
                            .get_untracked()
                            .max(1)
                            .min(total_pages(summary_snap.by_account.len())),
                    );
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some(MSG_INTERNAL.to_owned())),
            }
        });
    };

    // 参考装载：可选择面投资账户（selectableInvestmentAccountOptions 同口
    // 径——投资账户、剔除归档）与标的候选面（全集一次装载后经统一模糊搜索
    // 语义本地过滤，与汇总两表无先后依赖）。
    let load_reference = move || {
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else { return };
            let outcome = blocking(
                move || -> ledger_infra::error::Result<
                    (Vec<ledger_accounts::Account>, Vec<ledger_investment::Instrument>),
                > { Ok((boot::account_rows(&db)?, boot::instrument_rows(&db)?)) },
            )
            .await;
            let Ok(Ok((accounts, instruments))) = outcome else {
                return;
            };
            account_options.set(
                accounts
                    .iter()
                    .filter(|a| {
                        a.kind == ledger_accounts::AccountType::Investment && !a.is_archived
                    })
                    .map(|a| CategoryOption {
                        id: a.id.clone(),
                        label: a.name.clone(),
                    })
                    .collect::<Vec<_>>(),
            );
            account_names.set(
                accounts
                    .iter()
                    .map(|a| (a.id.clone(), a.name.clone()))
                    .collect::<Vec<_>>(),
            );
            instrument_options.set(
                instruments
                    .iter()
                    .map(|i| CategoryOption {
                        id: i.id.clone(),
                        label: match i.name.as_deref().filter(|n| !n.is_empty()) {
                            Some(name) => format!("{} · {}", i.symbol, name),
                            None => i.symbol.clone(),
                        },
                    })
                    .collect(),
            );
        });
    };

    // 筛选写路径（下拉封装直写信号，变化经本 effect 收口：任一应用值实际
    // 变化即页码归零 + 重查；同值重设与首载（None → (None, None) 相等）不
    // 触发。删除本 effect 接线即红：筛选永不重查，headless 筛选用例红）。
    let tracked = std::rc::Rc::new(std::cell::RefCell::new((None, None)));
    let tracked_for_effect = tracked.clone();
    effect(move || {
        let current = (account_id.get(), instrument_id.get());
        if *tracked_for_effect.borrow() != current {
            *tracked_for_effect.borrow_mut() = current.clone();
            session.pnl_year_page.set(1);
            session.pnl_account_page.set(1);
            load_summary();
        }
    });

    // 失效信号（投资族 → 重查；句柄随视图实例存亡）。
    let invest_sub = signals::subscribe_investments_changed(move || {
        load_summary();
    });
    effect(move || {
        let held: &signals::InvestmentsChangedSubscription = &invest_sub;
        let _ = held;
    });

    // 页签落位装载。
    effect(move || {
        let _landed = nav::store().current.get();
        load_reference();
        load_summary();
    });

    // 座行集切片（页码取会话槽；装载完成时已写回钳制值，切片处仅兜底钳）。
    let years = move || {
        summary
            .get()
            .map(|s| year_rows_desc(&s.by_year))
            .unwrap_or_default()
    };
    let account_rows = move || {
        summary
            .get()
            .map(|s| s.by_account.clone())
            .unwrap_or_default()
    };
    let year_page = move || {
        // 页码读持有响应式订阅（切页即行集切换——2 042 收尾的 get_untracked
        // 追重先例不得再现）。
        let _tracked = session.pnl_year_page.get();
        session
            .pnl_year_page
            .get_untracked()
            .max(1)
            .min(total_pages(years().len()))
    };
    let account_page = move || {
        let _tracked = session.pnl_account_page.get();
        session
            .pnl_account_page
            .get_untracked()
            .max(1)
            .min(total_pages(account_rows().len()))
    };
    let on_year_page = move |page: usize| session.pnl_year_page.set(page.max(1));
    let on_account_page = move |page: usize| session.pnl_account_page.set(page.max(1));

    // 账户候选（可选面 + 归档补值：选中项不在可选择面时补回选项头部并标
    // 「已归档」——旧壳 pinArchivedAccountOption 同判定，值原样保留）。
    let account_options_with_pin = move || {
        let mut options = account_options.get();
        let selected = account_id.get_untracked();
        if let Some(id) = selected
            && options.iter().all(|o| o.id != id)
        {
            let name = account_names
                .get_untracked()
                .iter()
                .find(|(k, _)| *k == id)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| id.clone());
            options.insert(
                0,
                CategoryOption {
                    label: format!("{name}（已归档）"),
                    id,
                },
            );
        }
        options
    };

    // 视图组装（页滚动承载；旧壳 NSpace vertical 同形态）。
    let filter_row = view()
        .flex_row()
        .items_center()
        .gap_4()
        .child(
            view()
                .test_id("pnl-filter-account")
                .child(select::app_select(
                    "pnl-account-filter",
                    account_options_with_pin,
                    account_id,
                    PNL_FILTER_ACCOUNT,
                    true,
                )),
        )
        .child(
            view()
                .test_id("pnl-filter-instrument")
                .child(pinyin_select::pinyin_select(
                    "pnl-instrument-filter",
                    move || instrument_options.get(),
                    instrument_id,
                    PNL_FILTER_INSTRUMENT_PLACEHOLDER,
                    true,
                )),
        );
    let note = text(PNL_CALIBRATION_NOTE)
        .text_size(px(11.0))
        .text_color(palette.muted)
        .test_id("pnl-calibration-note");
    let error_line = when(
        move || error.get().is_some(),
        move || {
            retry_row(
                move || error.get().unwrap_or_default(),
                "pnl-error",
                "pnl-retry",
                palette,
                load_summary,
            )
        },
    )
    .otherwise(view);
    let year_table = pnl_table(
        PNL_BY_YEAR,
        "pnl-year-table",
        PNL_COL_YEAR,
        move || {
            let privacy = privacy.get_untracked();
            let table = pnl_currencies.get_untracked();
            let fmt = summary
                .get_untracked()
                .map(|s| table.format_of(&s.native_currency))
                .unwrap_or_default();
            page_slice(&years(), year_page())
                .into_iter()
                .map(|row| {
                    let cells = assemble_pnl_row(
                        row.realized_pnl_cents,
                        row.dividend_cents,
                        row.realized_gain_cents,
                        &fmt,
                        privacy,
                    );
                    (row.year.clone(), cells)
                })
                .collect::<Vec<_>>()
        },
        move || total_pages(years().len()) as u32,
        move || year_page() as u32,
        on_year_page,
        palette,
    );
    let account_table = pnl_table(
        PNL_BY_ACCOUNT,
        "pnl-account-table",
        PNL_COL_ACCOUNT,
        move || {
            let privacy = privacy.get_untracked();
            let table = pnl_currencies.get_untracked();
            let fmt = summary
                .get_untracked()
                .map(|s| table.format_of(&s.native_currency))
                .unwrap_or_default();
            page_slice(&account_rows(), account_page())
                .into_iter()
                .map(|row| {
                    let cells = assemble_pnl_row(
                        row.realized_pnl_cents,
                        row.dividend_cents,
                        row.realized_gain_cents,
                        &fmt,
                        privacy,
                    );
                    (row.account_name.clone(), cells)
                })
                .collect::<Vec<_>>()
        },
        move || total_pages(account_rows().len()) as u32,
        move || account_page() as u32,
        on_account_page,
        palette,
    );
    let body = view()
        .flex_col()
        .gap_3()
        .child(
            view()
                .flex_row()
                .gap_3()
                .child(year_table)
                .child(account_table),
        )
        .child(mwr_card(palette, mwr.read_only()));
    scroll().flex_1().child(
        view()
            .flex_col()
            .mx_4()
            .my_2()
            .gap_3()
            .child(filter_row)
            .child(note)
            .child(error_line)
            .child(
                when(
                    move || summary.get().is_some(),
                    move || body.clone().into_element(),
                )
                .otherwise(move || {
                    // 命令失败已在错误条呈现；装载中/无数据给空态句。
                    view().test_id("pnl-empty").child(
                        text(PNL_EMPTY)
                            .text_size(px(13.0))
                            .text_color(palette.muted),
                    )
                }),
            ),
    )
}

/// MWR 卡（账户级随筛选收窄 + 全账级恒全账本口径；与金额口径并列、互不
/// 换算。列 = 账户 | 币种 | 资金加权收益率；重拉由失效信号驱动接缝内化）。
fn mwr_card(palette: PanePalette, mwr: nsh_core::element::ReadSignal<Vec<MwrRowSnap>>) -> View {
    view()
        .test_id("pnl-mwr-card")
        .flex_col()
        .gap_2()
        .rounded_lg()
        .p_3()
        .app_class("border border-border bg-background")
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    text(PNL_BY_MWR)
                        .text_size(px(13.0))
                        .text_color(palette.fg)
                        .test_id("pnl-mwr-title"),
                )
                .child(popover_tip(
                    "pnl-mwr-info",
                    vec![PNL_MWR_TIP.to_owned()],
                    info_dot(palette.info),
                )),
        )
        .child(
            when(
                move || mwr.get().is_empty(),
                move || {
                    text(PNL_EMPTY_TABLE)
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .test_id("pnl-mwr-empty")
                },
            )
            .otherwise(move || {
                view()
                    .flex_col()
                    .gap_1()
                    .child(
                        view()
                            .flex_row()
                            .gap_6()
                            .child(
                                text(PNL_COL_ACCOUNT)
                                    .text_size(px(12.0))
                                    .text_color(palette.muted),
                            )
                            .child(
                                text(PNL_COL_CURRENCY)
                                    .text_size(px(12.0))
                                    .text_color(palette.muted),
                            )
                            .child(
                                text(PNL_COL_MWR)
                                    .text_size(px(12.0))
                                    .text_color(palette.muted),
                            ),
                    )
                    .child(each(
                        move || mwr.get(),
                        |r: &MwrRowSnap| {
                            format!(
                                "{}|{}|{}",
                                r.account_id.clone().unwrap_or_default(),
                                r.scope,
                                r.currency_code
                            )
                        },
                        move |item: nsh_core::element::ReadSignal<MwrRowSnap>,
                              _index: nsh_core::element::ReadSignal<usize>| {
                            mwr_row_view(item, palette)
                        },
                    ))
            }),
        )
}

/// MWR 单行：作用域 | 币种 | 收益率（可计算着涨跌色、「无法计算」中性
/// 不猜解；未年化行带「*」角标——解释文案点按可达）。
fn mwr_row_view(
    item: nsh_core::element::ReadSignal<MwrRowSnap>,
    palette: PanePalette,
) -> impl IntoElement {
    let row = item.get_untracked();
    let id_suffix = row.account_id.clone().unwrap_or_else(|| "total".to_owned());
    let rate_id = format!("pnl-mwr-{}-{}", id_suffix, row.currency_code);
    view()
        .flex_row()
        .items_center()
        .gap_6()
        .child(
            text(row.scope.clone())
                .text_size(px(13.0))
                .text_color(palette.fg)
                .test_id(format!("{rate_id}-scope").as_str()),
        )
        .child(
            text(row.currency_code.clone())
                .text_size(px(13.0))
                .text_color(palette.muted),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    text(move || {
                        let snapshot = item.get();
                        rate_text_of(&snapshot)
                    })
                    .text_size(px(13.0))
                    .test_id(rate_id.as_str())
                    .text_color(move || {
                        let snapshot = item.get();
                        let semantic = snapshot
                            .rate
                            .map(|rate| pnl_semantic(if rate < 0.0 { -1 } else { 0 }));
                        pnl_color(semantic, &palette, &palette.fg)
                    }),
                )
                .child(
                    when(
                        move || item.get().basis == MwrBasis::Cumulative,
                        move || {
                            popover_tip(
                                format!("{rate_id}-cumulative-tip"),
                                vec![PNL_MWR_CUMULATIVE_TIP.to_owned()],
                                info_dot(palette.info),
                            )
                        },
                    )
                    .otherwise(view),
                ),
        )
}

/// 单值表（行名 | 已实现收益：主值 + 副行两腿拆解——逐腿透传前端零算术；
/// 客户端切片分页——分页条只供多页时呈现，单页收起）。
#[allow(clippy::too_many_arguments)]
fn pnl_table(
    title: &'static str,
    test_id: &'static str,
    head_col: &'static str,
    rows_of: impl Fn() -> Vec<(String, RealizedGainCells)> + Copy + 'static,
    total_pages_of: impl Fn() -> u32 + Copy + 'static,
    page_of: impl Fn() -> u32 + Copy + 'static,
    on_page: impl Fn(usize) + Copy + 'static,
    palette: PanePalette,
) -> View {
    view()
        .test_id(test_id)
        .flex_1()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .p_3()
        .app_class("border border-border bg-background")
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(text(title).text_size(px(13.0)).text_color(palette.fg))
                // 主值列口径说明挂表头（旧壳 ConceptLabel 列头同位）。
                .child(popover_tip(
                    format!("{test_id}-gain-info"),
                    vec![format!("{}：已实现盈亏 + 现金分红，即这一年（这个账户）已到手的得失与现金收入；不含仍在持仓里的浮动盈亏（本页只算已到手的部分）。分红按到账日归年，含红利再投的分红腿（同日买入抬高的是持仓成本，不在本表）与迁移来源的货基按年收益。", PNL_COL_REALIZED_GAIN)],
                    info_dot(palette.info),
                )),
        )
        .child(
            // 列头（旧壳 NDataTable columns 词表）。
            view()
                .flex_row()
                .gap_4()
                .child(
                    text(head_col)
                        .text_size(px(12.0))
                        .text_color(palette.muted),
                )
                .child(
                    text(PNL_COL_REALIZED_GAIN)
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .flex_1(),
                ),
        )
        .child(
            when(
                move || rows_of().is_empty(),
                move || {
                    text(PNL_EMPTY_TABLE)
                        .text_size(px(13.0))
                        .text_color(palette.muted)
                        .test_id(format!("{test_id}-empty").as_str())
                },
            )
            .otherwise(move || {
                view()
                    .flex_col()
                    .gap_1()
                    .child(each(
                        move || {
                            rows_of()
                                .into_iter()
                                .map(|(row_name, cells)| {
                                    // 格文本预装配（渲染期零格式化纪律——本行
                                    // 全部文本与着色面一次产出，元素闭包只消
                                    // 费快照值，不持引用）。
                                    (row_name, RealizedGainRowView {
                                        gain_text: realized_gain_text(&cells),
                                        gain_color: match cells.gain_semantic {
                                            Some(assembly::PnlSemantic::Gain) => RowGainColor::Gain,
                                            Some(assembly::PnlSemantic::Loss) => RowGainColor::Loss,
                                            None => RowGainColor::Neutral,
                                        },
                                        legs_present: realized_legs_text(&cells).is_some(),
                                        legs_text: realized_legs_text(&cells)
                                            .unwrap_or_default(),
                                    })
                                })
                                .collect::<Vec<_>>()
                        },
                        |(row_name, _): &(String, RealizedGainRowView)| row_name.clone(),
                        move |item: nsh_core::element::ReadSignal<(String, RealizedGainRowView)>,
                              _index: nsh_core::element::ReadSignal<usize>| {
                            let (row_name, view_cells) = item.get_untracked();
                            let gain_color = match view_cells.gain_color {
                                RowGainColor::Gain => palette.gain,
                                RowGainColor::Loss => palette.loss,
                                RowGainColor::Neutral => palette.fg,
                            };
                            view()
                                .flex_col()
                                .gap_0()
                                .min_h(px(36.0))
                                .items_start()
                                .child(
                                    // 行名 + 主值（可计算着涨跌色、「无法计算」
                                    // 中性；headless 锚点行键）。
                                    view()
                                        .flex_row()
                                        .gap_4()
                                        .child(
                                            text(row_name.clone())
                                                .text_size(px(13.0))
                                                .text_color(palette.fg)
                                                .test_id(
                                                    format!("{test_id}-name-{}", row_name)
                                                        .as_str(),
                                                ),
                                        )
                                        .child(
                                            view().flex_col().child(
                                                text(view_cells.gain_text.clone())
                                                    .text_size(px(13.0))
                                                    .text_color(gain_color)
                                                    .test_id(
                                                        format!("{test_id}-value-{}", row_name)
                                                            .as_str(),
                                                    ),
                                            ),
                                        ),
                                )
                                .child(
                                    // 副行两腿拆解（不可计算行不复现）。
                                    when(
                                        move || view_cells.legs_present,
                                        move || {
                                            text(view_cells.legs_text.clone())
                                                .text_size(px(11.0))
                                                .text_color(palette.muted)
                                        },
                                    )
                                    .otherwise(view),
                                )
                        },
                    ))
                    .child(
                        // 客户端切片分页条（单页收起——旧壳 paginate-single-
                        // page = false 同口径）。
                        when(
                            move || total_pages_of() > 1,
                            move || {
                                view().test_id(format!("{test_id}-pagination").as_str()).child(
                                    nsh_base::pagination()
                                        .accessibility_label("盈亏汇总分页")
                                        .previous_accessibility_label("上一页")
                                        .next_accessibility_label("下一页")
                                        .page(page_of)
                                        .total_pages(total_pages_of)
                                        .on_change(move |&picked: &u32| {
                                            on_page(picked as usize);
                                        }),
                                )
                            },
                        )
                        .otherwise(view),
                    )
            }),
        )
}

/// 单值行注视快照（each 行闭包的 owned 载体——涨跌色枚举化避免跨闭包
/// 引用；文本/两腿拆解均为一次性装配产物）。
#[derive(Debug, Clone, PartialEq)]
struct RealizedGainRowView {
    gain_text: String,
    gain_color: RowGainColor,
    legs_present: bool,
    legs_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowGainColor {
    Gain,
    Loss,
    Neutral,
}

// lib 单测（词表纯函数与装配口径：渲染期零格式化的兜底面——两页签汇总格
// 文本、折算口径与分页切片；测试整体豁免 ADR-0060 经 crate 根放行）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cny_table() -> crate::assembly::CurrencyTable {
        let cny = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        crate::assembly::CurrencyTable::from_currencies([&cny])
    }

    fn overview_snap() -> OverviewSnap {
        OverviewSnap {
            native_currency: "CNY".to_owned(),
            investable_assets_cents: 12_345,
            investment_cash_cents: 6_789,
            holdings_market_value_cents: 5_556,
            total_market_value_cents: 5_556,
            unrealized_pnl_cents: -1_234,
            cumulative_pnl_cents: 876,
            missing_price_holding_count: 2,
            has_investment_account: true,
        }
    }

    #[test]
    fn overview_cells_format_amounts_and_notes() {
        let cells = assemble_overview(&overview_snap(), &cny_table(), false);
        assert_eq!(cells.hero_text, "¥123.45");
        assert_eq!(cells.currency_label, "本位币 CNY");
        assert_eq!(cells.cash_leg_text, "投资账户现金 ¥67.89");
        assert_eq!(cells.holdings_leg_text, "持仓市值 ¥55.56");
        assert_eq!(cells.market_value_text, "¥55.56");
        assert_eq!(cells.unrealized_pnl_text, "-¥12.34");
        assert_eq!(cells.cumulative_pnl_text, "¥8.76");
        assert_eq!(
            cells.missing_price_note.as_deref(),
            Some("另有 2 只持仓因缺现价或折算汇率未计入。")
        );
        assert_eq!(cells.no_account_note, None, "有账户不出引导句");
    }

    #[test]
    fn overview_cells_privacy_and_zero_and_empty_book() {
        // 隐私态：金额全掩码、构成条一并隐藏（图形和金额同严格口径）。
        let cells = assemble_overview(&overview_snap(), &cny_table(), true);
        for text in [
            cells.hero_text.as_str(),
            cells.cash_leg_text.split(' ').nth(1).unwrap(),
            cells.holdings_leg_text.split(' ').nth(1).unwrap(),
            cells.market_value_text.as_str(),
            cells.unrealized_pnl_text.as_str(),
            cells.cumulative_pnl_text.as_str(),
        ] {
            assert_eq!(text, assembly::AMOUNT_PRIVACY_MASK, "隐私态金额统一掩码");
        }
        assert!(!cells.show_composition, "隐私态不渲染构成条");
        // 可投资资产 0：不成段，但金额、图例与引导照常（空轨道不渲染）。
        let empty = OverviewSnap {
            native_currency: "CNY".to_owned(),
            ..Default::default()
        };
        let cells = assemble_overview(&empty, &cny_table(), false);
        assert_eq!(cells.hero_text, "¥0");
        assert!(!cells.show_composition, "可投资资产 0 不渲染构成条");
        assert!(
            cells.no_account_note.is_some_and(|n| n == NO_ACCOUNT_NOTE),
            "没有投资账户出引导句"
        );
    }

    #[test]
    fn share_pct_each_leg_optional_and_clamped() {
        // 两段各自独立（不做 100−x 余量法）：两腿之和失配时缺口如实保留。
        assert!(share_pct(30, 100) - 30.0 < 1e-4);
        assert!(share_pct(60, 100) - 60.0 < 1e-4);
        assert!(share_pct(40, 50) - 80.0 < 1e-4);
        assert_eq!(share_pct(40, 0), 0.0, "总额 ≤ 0 不产段宽");
        assert_eq!(share_pct(-10, 100), 0.0, "钳下界");
        assert_eq!(share_pct(120, 100), 100.0, "钳上界");
    }

    fn pnl_domain_rows() -> PnlSnap {
        PnlSnap {
            native_currency: "CNY".to_owned(),
            by_year: vec![
                YearPnlSnap {
                    year: "2025".to_owned(),
                    realized_pnl_cents: Some(1_000),
                    dividend_cents: Some(500),
                    realized_gain_cents: Some(1_500),
                },
                YearPnlSnap {
                    year: "2024".to_owned(),
                    realized_pnl_cents: Some(-300),
                    dividend_cents: Some(100),
                    realized_gain_cents: Some(-200),
                },
                // 缺料行（任一腿兜底窗口内无点）：三项全 None。
                YearPnlSnap {
                    year: "2023".to_owned(),
                    realized_pnl_cents: None,
                    dividend_cents: Some(70),
                    realized_gain_cents: None,
                },
            ],
            by_account: vec![AccountPnlSnap {
                account_id: "a-1".to_owned(),
                account_name: "证券".to_owned(),
                realized_pnl_cents: Some(700),
                dividend_cents: Some(600),
                realized_gain_cents: Some(1_300),
            }],
        }
    }

    #[test]
    fn pnl_year_rows_desc_and_page_slice() {
        let rows = year_rows_desc(&pnl_domain_rows().by_year);
        assert_eq!(
            rows.iter().map(|r| r.year.as_str()).collect::<Vec<_>>(),
            vec!["2025", "2024", "2023"],
            "最新年在前（第 1 页 = 最近年份是分页可用的前提）"
        );
        // 客户端切片：页大小 8 收口（行少不切），越界取末页。
        assert_eq!(page_slice(&rows, 1).len(), 3);
        assert_eq!(
            page_slice(&rows, 99).last().map(|r| r.year.as_str()),
            Some("2023")
        );
        assert_eq!(total_pages(8), 1);
        assert_eq!(total_pages(9), 2);
    }

    #[test]
    fn pnl_row_cells_main_value_and_legs() {
        let row = &year_rows_desc(&pnl_domain_rows().by_year)[0];
        let cells = assemble_pnl_row(
            row.realized_pnl_cents,
            row.dividend_cents,
            row.realized_gain_cents,
            &cny_table().format_of("CNY"),
            false,
        );
        assert!(cells.computable);
        assert_eq!(realized_gain_text(&cells), "¥15");
        // 副行两腿拆解（前端零算术——两腿均为后端折算好的分）。
        assert_eq!(
            realized_legs_text(&cells).as_deref(),
            Some("已实现盈亏 ¥10 · 现金分红 ¥5")
        );
        assert_eq!(cells.gain_semantic, Some(assembly::PnlSemantic::Gain));
        // 缺料行：整行「无法计算」，不出部分数字（另一腿真实值与 0 都不出）。
        let missing = &year_rows_desc(&pnl_domain_rows().by_year)[2];
        let cells = assemble_pnl_row(
            missing.realized_pnl_cents,
            missing.dividend_cents,
            missing.realized_gain_cents,
            &cny_table().format_of("CNY"),
            false,
        );
        assert!(!cells.computable);
        assert_eq!(realized_gain_text(&cells), PNL_NOT_COMPUTABLE);
        assert_eq!(realized_legs_text(&cells), None);
        // 隐私态：主值与两腿一并掩码（图形与金额同严格口径）。
        let cells = assemble_pnl_row(
            row.realized_pnl_cents,
            row.dividend_cents,
            row.realized_gain_cents,
            &cny_table().format_of("CNY"),
            true,
        );
        assert_eq!(
            cells.gain_text.as_deref(),
            Some(assembly::AMOUNT_PRIVACY_MASK)
        );
        assert_eq!(
            cells.realized_leg_text.as_deref(),
            Some(assembly::AMOUNT_PRIVACY_MASK)
        );
    }

    #[test]
    fn mwr_rows_assemble_with_filter_and_total() {
        let summary = ledger_investment::MoneyWeightedReturnSummary {
            by_instrument: Vec::new(),
            by_account: vec![
                ledger_investment::AccountMwr {
                    account_id: "a-1".to_owned(),
                    account_name: "证券".to_owned(),
                    currency_code: "CNY".to_owned(),
                    basis: ledger_investment::MwrBasis::Annualized,
                    rate: Some(0.1234),
                },
                ledger_investment::AccountMwr {
                    account_id: "a-2".to_owned(),
                    account_name: "美元户".to_owned(),
                    currency_code: "USD".to_owned(),
                    basis: ledger_investment::MwrBasis::Cumulative,
                    rate: None,
                },
            ],
            total: vec![ledger_investment::CurrencyMwr {
                currency_code: "CNY".to_owned(),
                basis: ledger_investment::MwrBasis::Annualized,
                rate: Some(-0.05),
            }],
        };
        // 无筛选：账户行在前、全账行在后。
        let rows = assemble_mwr_rows(&summary, None, |_| None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].scope, "证券");
        assert_eq!(rows[1].scope, "美元户");
        assert_eq!(rows[2].scope, PNL_TOTAL);
        // 账户筛选：同 id 行收窄、全账行不随筛选收窄。
        let rows = assemble_mwr_rows(&summary, Some("a-1"), |_| None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].scope, "证券");
        assert_eq!(rows[1].scope, PNL_TOTAL);
        // 行文本：年化带两位百分数；未年化带角标；无解显式「无法计算」。
        assert_eq!(format_rate(0.1234), "+12.34%");
        assert_eq!(format_rate(-0.05), "-5.00%");
        assert_eq!(format_rate(0.0), "0.00%");
        assert_eq!(rate_text_of(&rows[0]), "+12.34%");
        assert_eq!(rate_text_of(&rows[1]), "-5.00%", "筛选后全账行位次前移");
    }

    #[test]
    fn trend_range_presets_match_old_shell_math() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        // 「全部」不设起止。
        assert_eq!(to_trend_range(TrendPreset::All, today), (None, None));
        // 1 年：同月同日回退一年。
        assert_eq!(
            to_trend_range(TrendPreset::OneYear, today).0.as_deref(),
            Some("2025-03-15")
        );
        // 1 月：月末溢出钳制（31 号回 28 天月落到 2 月最后一日）。
        let mar31 = chrono::NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        assert_eq!(
            to_trend_range(TrendPreset::OneMonth, mar31).0.as_deref(),
            Some("2026-02-28")
        );
        // 5 年跨闰铸牢：非闰年 2 月 28 日 → 五年前落同月 28 日。
        let feb28 = chrono::NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
        assert_eq!(
            to_trend_range(TrendPreset::FiveYears, feb28).0.as_deref(),
            Some("2021-02-28")
        );
    }

    #[test]
    fn trend_snap_empty_note_follows_backfill_states() {
        let mut trend = TrendSnap::default();
        assert_eq!(
            trend.empty_note(),
            TREND_EMPTY_NO_POINTS,
            "无补全字段给中性改写"
        );
        trend.backfill = Some(ledger_investment::TrendBackfillState::Running);
        trend.backfill_done = Some(2);
        trend.backfill_total = Some(5);
        assert_eq!(
            trend.empty_note(),
            "历史数据补全中（2/5），完成后即可查看走势。"
        );
        trend.backfill_done = None;
        trend.backfill_total = None;
        assert_eq!(trend.empty_note(), TREND_EMPTY_BACKFILL_RUNNING);
        trend.backfill = Some(ledger_investment::TrendBackfillState::RetryPending);
        assert_eq!(trend.empty_note(), TREND_EMPTY_RETRY_PENDING);
        trend.backfill = Some(ledger_investment::TrendBackfillState::NoData);
        assert_eq!(trend.empty_note(), TREND_EMPTY_NO_DATA);
    }
}

// ---------------------------------------------------------------------------
// 失效信号重读探针（lib 模块单测；headless 引擎内驱动——失效信号广播必须
// 发生在引擎 UI 线程，集成测试线程触不到 thread_local 订阅者表，探针组件
// 在引擎线程内经真实按钮触发广播，消费面挂真实投资视图；dashboard/tests.rs
// 先例同形）。
//
// 「删除即红」负向条目：删 [`InvestmentsView`]（经 [`overview_pane`]）内
// 的投资族订阅接线任一 → 本测红（库中已变而卡面仍旧）；删 [`crate::signals`]
// 的广播转发 → 探针按钮失去生产语义。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod probe {
    use nsh_core::prelude::*;
    use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
    use nsh_testing::Headless;

    use crate::app::{reset_book_assembly, run_startup, set_book_dir, test_assembly_guard};
    use crate::signals;

    /// 探针根组件：投资失效信号广播按钮 + 真实投资视图（概览页签默认首签）。
    #[component]
    fn InvestmentsProbeApp() -> impl IntoElement {
        view()
            .flex_1()
            .flex_col()
            .app_class("bg-background")
            .child(
                view()
                    .test_id("probe-broadcast-investments")
                    .on_press(move |_: &()| signals::notify_investments_invalidated())
                    .child(text("投资失效")),
            )
            .child(super::investments_view())
    }

    /// 探针造数交易输入的最小基座（具名字段覆盖，其余零值）。
    fn probe_input() -> ledger_transaction::TransactionInput {
        ledger_transaction::TransactionInput {
            kind: ledger_transaction::TransactionKind::Expense,
            amount_cents: 0,
            currency_code: String::new(),
            account_id: None,
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            source_order_no: None,
            date: String::new(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
        }
    }

    /// 投资失效信号重读：卡片首次读 ¥1099（现金腿 ¥999 + 持仓市值腿
    /// ¥100）；后台录一笔现价翻倍（¥100 → ¥200）→ 广播投资失效 → 两卡
    /// 重读（Hero ¥1099 → ¥1199）。「删除即红」负向条目见模块头注。
    #[test]
    fn overview_rereads_on_investments_invalidation_signal() {
        let _guard = test_assembly_guard();
        reset_book_assembly();
        let dir = std::env::temp_dir().join(format!(
            "nsh-app-invest-probe-{}",
            ledger_infra::ids::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ledger.db"), b"").unwrap();
        {
            let db = crate::boot::boot_ledger(&dir).unwrap();
            use ledger_accounts::{AccountInput, AccountType};
            use ledger_investment::{InstrumentInput, InstrumentType, MarketPriceInput};
            let invest = {
                let conn = db.conn.lock().unwrap();
                ledger_accounts::create_account(
                    &conn,
                    AccountInput {
                        name: "证券".to_owned(),
                        kind: AccountType::Investment,
                        currency_code: "CNY".to_owned(),
                        initial_balance_cents: Some(100_000),
                        credit_limit_cents: None,
                        statement_day: None,
                        due_day: None,
                    },
                )
                .unwrap()
            };
            let instrument = {
                let conn = db.conn.lock().unwrap();
                let id = ledger_investment::create_instrument(
                    &conn,
                    InstrumentInput {
                        symbol: "TST".to_owned(),
                        kind: InstrumentType::Stock,
                        name: Some("测试标的".to_owned()),
                        currency_code: "CNY".to_owned(),
                        market: None,
                    },
                )
                .unwrap();
                ledger_investment::create_market_price(
                    &conn,
                    MarketPriceInput {
                        instrument_id: id.clone(),
                        price_cents: 1_000_000, // 万分之一元刻度：单价 ¥100
                        currency_code: "CNY".to_owned(),
                        priced_at: crate::boot::today_string(),
                        source: Some("manual".to_owned()),
                    },
                )
                .unwrap();
                id
            };
            {
                let conn = db.conn.lock().unwrap();
                ledger_transaction::create(
                    &conn,
                    ledger_transaction::TransactionInput {
                        kind: ledger_transaction::TransactionKind::Buy,
                        amount_cents: 0,
                        currency_code: "CNY".to_owned(),
                        account_id: Some(invest),
                        instrument_id: Some(instrument),
                        quantity: Some(1.0),
                        price_cents: Some(10_000), // 买入成本 ¥1（刻度差价为测试面既定）
                        fee_cents: Some(0),
                        date: crate::boot::today_string(),
                        note: Some("买入".to_owned()),
                        ..probe_input()
                    },
                )
                .unwrap();
            }
        }

        set_book_dir(dir.clone());
        assert!(matches!(run_startup(), crate::app::StartupView::Ready));

        let mut app = Headless::start(EngineConfig {
            viewport: nsh_engine::Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
            environment: Environment {
                os: Os::Linux,
                idiom: Idiom::Desktop,
                ..Environment::default()
            },
            ui_sets: vec![nsh_base::registry()],
            ..EngineConfig::new(Root::of::<InvestmentsProbeApp>())
        })
        .unwrap();
        app.wait_for("id:overview-hero-value").unwrap();
        app.wait_for("text:¥1099").unwrap();

        // 后台行情写：现价翻倍（万分之一元 2_000_000 = ¥200）→ 持仓市值腿
        // ¥200，可投资资产 = 999 现金腿 + 200 持仓腿 = ¥1199（现金腿不变，
        // 市值腿随价变——读面包含全部四段取数的同一快照）。
        {
            let db_session = crate::app::session_handle().unwrap();
            let conn = db_session.conn.lock().unwrap();
            ledger_investment::create_market_price(
                &conn,
                ledger_investment::MarketPriceInput {
                    instrument_id: ledger_investment::list_instruments(&conn, &Default::default())
                        .unwrap()
                        .items
                        .first()
                        .map(|i| i.id.clone())
                        .unwrap(),
                    price_cents: 2_000_000,
                    currency_code: "CNY".to_owned(),
                    priced_at: crate::boot::today_string(),
                    source: Some("manual".to_owned()),
                },
            )
            .unwrap();
        }
        app.press("id:probe-broadcast-investments").unwrap();
        app.wait_for("text:¥1199").unwrap();
    }
}
