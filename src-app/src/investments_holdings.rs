//! 投资视图·持仓页签（issue #2046 NSH 迁移⑮；spec #2031 系列 15/26，投资
//! 2/3；旧壳 `HoldingsOverview.vue` + `usePortfolioOverview.ts` +
//! `useHoldingsFilter.ts` + `PortfolioStatsCards.vue` 本页签消费形态的重生）。
//!
//! 数据装载（旧壳 list_holdings + 持仓标的字典拼装接缝）：读路径直调域读
//! 入口（[`crate::boot`]，旧壳 list_holdings / list_instruments /
//! money_weighted_return_summary / cumulative_pnl_native_total 命令同一域
//! 函数，不经 IPC）——持仓行 + 标的字典 + 账户名录 + 标的收益率
//! （by_instrument 行）+ 累计收益·折本位币单值 + 币种字典一次装载（阻塞读
//! 走后台，SQLite IO 不占引擎线程）；累计收益是**全账本口径、不随过滤收窄**
//!（已实现腿来自平仓匹配，无法归到某行可见持仓——旧壳 usePortfolioOverview
//! 注记同词），其读失败码化上抛只降级累计卡（警告 + 重试 = 重发同一条读
//! 命令），不拖累持仓行装配（两路独立 Loadable 同语义）；行集失败整卡
//! 错误条 + 重试（旧壳卡内 warning + retry 同语义）。
//!
//! 过滤 / 排序 / 分页（旧壳 useHoldingsFilter 三维闭集 + issue #912 分页）：
//! 搜索（300ms 防抖统一模糊搜索语义）、账户单选（可选择面 + 选中归档补值，
//! spec #1952）、市值/收益列头升降序（缺价行恒排末尾与方向无关、无排序状态
//! 默认标的代码字母序）与客户端切片分页（固定 20 行/页不设选择器、三维任一
//! 实际变化即翻页归零、恢复越界回写钳制）全部在壳客户端完成（`list_holdings`
//! 契约不动）；三维状态与页码住会话槽（[`crate::investments::store`]，旧壳
//! issue #1192 会话保留语义同射程）。合计三卡随过滤子集收窄（issue #1797
//! 折本位币单值）、累计卡保留全账本口径——合计表达式与首页概览卡同源
//!（[`assembly::assemble_portfolio_stats_for_pairs`]，旧壳 rowStatCardValues
//! 单点不再复制第二份）。
//!
//! 落位装载与到访重拉：visited-once keep-mounted 容器不随页签切换重挂——
//! 旧壳本页签 display-directive 缺省 'if'（每次进页签重新挂载重新取数），
//! 本壳以「激活边即重拉」对应（会话内保留的是选择不是快照，数据照常现拉，
//! 旧壳同口径）。价格失效信号（[`crate::signals`] 投资族）驱动重读
//!（ADR-0031，同步实际写价后行集与合计现值化）——删除接线即红钉在 lib 探针。
//!
//! 缺价行引导（issue #1193）：缺价行只读**后端派生的价格通道事实**分流
//!（词汇表「价格通道」，前端不自行推断）——行情 / 净值通道 → 触发既有
//!「同步标的信息」接缝（[`crate::investment_sync`] 同一单点、在途短路
//! 复用）；手动报价通道行内不设引导——录价入口与页面级回执住标的库页签
//!（#2047 ⑯ 交付）；无通道不给引导（无可落地的既有入口）。
//!
//! 界面文案仅 zh-CN（旧壳 investments.json holdings 段 + concepts 段同词
//! 直取；旧壳列头下钻链接 InstrumentLink 已随 #2047 ⑯ 落位——标的代码
//! 下钻链接，删除即红钉见 [`render_symbol_link`]）；隐私态金额/数量统一掩码（全壳
//! AMOUNT_PRIVACY_MASK，装载时机读取，#2045 同口径）；码化错误 zh 原样
//! 呈现（卡内错误条 + 重试）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{input, pagination};
use nsh_core::prelude::*;

use crate::adaptive;
use crate::assembly::{CategoryOption, CurrencyTable, PnlSemantic};
use crate::design::font_size;
use crate::investments::{HoldingsSortColumn, HoldingsSorter};
use crate::{
    app, assembly, boot, investment_sync, investments, popover, select, signals, transactions,
};

// ---------------------------------------------------------------------------
// 词表（旧壳 investments.json holdings 段 + concepts 段逐条平价；字面闭集承载）
// ---------------------------------------------------------------------------

pub(crate) const HOLDINGS_TITLE: &str = "当前持仓";
pub(crate) const HOLDINGS_EMPTY: &str = "暂无持仓";
pub(crate) const HOLDINGS_FILTER_NO_MATCH: &str = "筛选条件下无匹配持仓";
pub(crate) const HOLDINGS_SEARCH_PLACEHOLDER: &str = "搜索标的（代码/名称/拼音）";
pub(crate) const HOLDINGS_FILTER_ACCOUNT: &str = "全部账户";
pub(crate) const COL_SYMBOL: &str = "标的";
pub(crate) const COL_NAME: &str = "名称";
pub(crate) const COL_ACCOUNT: &str = "账户";
pub(crate) const COL_QUANTITY: &str = "数量";
pub(crate) const COL_COST: &str = "成本";
pub(crate) const COL_PRICE: &str = "现价";
pub(crate) const COL_NAV_DATE: &str = "净值日期";
pub(crate) const COL_MARKET_VALUE: &str = "市值";
pub(crate) const COL_UNREALIZED_PNL: &str = "持仓收益";
pub(crate) const COL_MWR: &str = "资金加权收益率";
/// 合计三卡标签（旧壳 PortfolioStatsCards 消费 concepts.marketValue 等同词）。
pub(crate) const STATS_MARKET_VALUE_LABEL: &str = "总市值";
pub(crate) const STATS_UNREALIZED_PNL_LABEL: &str = "持仓收益";
/// 成本概念说明（investments.concepts.costTip 同词表）。
pub(crate) const COST_TIP: &str = "当前剩余持仓按 FIFO 剩余批次成本合计，含买入手续费；已卖出部分的买入成本不计入，故不是累计投入；币种为买入时的成本币种。";
/// 价格概念说明（investments.concepts.priceTip 同词表）。
pub(crate) const PRICE_TIP: &str = "最新一条行情：基金行为最新公布的单位净值（对应「净值日期」列），股票为最新报价或收盘价；不是买入价或成本价，没有行情时显示「-」。";
/// 市值概念说明（investments.concepts.marketValueTip 同词表）。
pub(crate) const MARKET_VALUE_TIP: &str =
    "各持仓按「数量 × 最新价格」计算市值的合计；缺现价或缺汇率的持仓不计入；不含投资账户里的现金。";
/// 收益概念说明（investments.concepts.unrealizedPnlTip 同词表）。
pub(crate) const UNREALIZED_PNL_TIP: &str = "当前市值减去持仓成本（FIFO 批次成本，含买入手续费）的未实现盈亏，随行情变动；卖出后才转为已实现盈亏。";
/// 收益率概念说明（investments.concepts.mwrTip 同词表）。
pub(crate) const MWR_TIP: &str = "按实际现金流与当前市值解出的年化内部收益率；建仓时点未知的持仓改给「未年化」，即累计收益除以累计投入，两者不可互算；不随搜索或标的筛选收窄；缺价或数据不足显示「-」。";
/// 累计收益概念说明（investments.concepts.cumulativePnlTip 同词表）。
pub(crate) const CUMULATIVE_PNL_TIP: &str = "持仓收益（未实现）＋已实现盈亏＋累计分红。";
/// 合计卡过滤子集作用域句（investments.concepts.scopeFiltered 同词表；
/// tipTemplate「{body}{scope}」同形拼接——市值/收益两卡共用）。
pub(crate) const STATS_SCOPE_FILTERED: &str = "金额折算到全局默认币种（本位币）后合并为单个数值；随当前搜索与账户过滤收窄；隐藏账户照常计入。";
/// 累计卡全账本作用域句（investments.concepts.scopeWholeLedger 同词表）。
pub(crate) const STATS_SCOPE_WHOLE_LEDGER: &str = "金额折算到全局默认币种（本位币）后合并为单个数值；覆盖全账本的持仓，不随搜索或过滤收窄；隐藏账户照常计入。";
/// 缺价行同步引导（investments.holdings.missingPrice.sync 同词表）。
pub(crate) const MISSING_PRICE_SYNC: &str = "同步";
/// 收益率未年化角标（旧壳 renderMwrRateCell 单点同词）。
const MWR_CUMULATIVE_MARKER: &str = "*";
/// 收益率「无法计算」（旧壳 pnl 词表「无法计算」同词复用）。
const MWR_NOT_COMPUTABLE: &str = "无法计算";
/// 收益率未年化解释（旧壳概念双轴：角标行的解释文案随列头说明共同场）。
const MWR_CUMULATIVE_TIP: &str = "未年化：累计收益除以累计投入（含建仓时点未知的持仓，不给年化）";

// ---------------------------------------------------------------------------
// 行快照与格文本（装配层：渲染期零格式化——本页签格文本一次产出）
// ---------------------------------------------------------------------------

/// 持仓概览行快照（旧壳 PortfolioRow 的本壳等价：Holding 行叠加标的字典与
/// 账户的展示信息；全部格文本/语义面一次产出）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoldingRowSnap {
    /// 行身份键（账户 + 标的：v_holdings 一行 = 一个账户一只标的，同一标的
    /// 跨账户持仓各行互不撞车；旧壳 priceGuideRowKey 同拼法）。
    pub(crate) row_id: String,
    pub(crate) account_id: String,
    pub(crate) instrument_id: String,
    /// 标的代码（标的字典缺行 → None，展示降级「-」，旧壳同口径）。
    pub(crate) symbol: Option<String>,
    pub(crate) instrument_name: Option<String>,
    /// 账户名（参考数据缺失 → None，展示「-」）。
    pub(crate) account_name: Option<String>,
    /// 搜索判定文本（「代码 · 名称」等价文本；无名称退化裸代码——旧壳
    /// holdingsSearchLabel 同拼法，与标的搜索同规格）。
    search_label: String,
    /// 数量文本（至多 4 位小数去零、隐私掩码）。
    pub(crate) quantity_text: String,
    /// 成本文本（FIFO 剩余批次成本，成本币种格式化）。
    pub(crate) cost_text: String,
    /// 现价文本（缺价 None；万分之一元刻度展示口径单点在装配层）。
    pub(crate) price_text: Option<String>,
    /// 缺价标志（缺价行引导的呈现位）。
    pub(crate) price_missing: bool,
    /// 净值日期（仅基金行携带；其余呈现「-」）。
    pub(crate) nav_date_text: String,
    pub(crate) market_value_text: Option<String>,
    pub(crate) unrealized_pnl_text: Option<String>,
    pub(crate) unrealized_pnl_semantic: Option<PnlSemantic>,
    /// 收益率格（一态闭集：可计算值 + 未年化标记 / 「无法计算」）。
    pub(crate) mwr_text: String,
    pub(crate) mwr_semantic: Option<PnlSemantic>,
    pub(crate) mwr_cumulative: bool,
    /// 合计卡消费面（两列行级对：账户币值 + 折本位币值；缺料三态在合计
    /// 装配闭集内收口）。
    pub(crate) market_value_cents: Option<i64>,
    pub(crate) native_market_value_cents: Option<i64>,
    pub(crate) unrealized_pnl_cents: Option<i64>,
    pub(crate) native_unrealized_pnl_cents: Option<i64>,
    /// 价格写入通道（标的字典透传的后端派生事实，issue #1060）。
    pub(crate) price_channel: Option<ledger_investment::PriceChannel>,
}

/// 持仓行集装配（域读 → 行快照闭集；收益率按（账户 × 标的）行 id 匹配，
/// 缺失行「无法计算」不猜解）。隐私开关是装配输入（装载时机读取）。
pub(crate) fn assemble_holding_rows(
    holdings: &[ledger_investment::Holding],
    instruments: &[ledger_investment::Instrument],
    accounts: &[ledger_accounts::Account],
    mwr_rows: &[ledger_investment::InstrumentMwr],
    currencies: &CurrencyTable,
    privacy: bool,
) -> Vec<HoldingRowSnap> {
    holdings
        .iter()
        .map(|h| {
            let inst = instruments.iter().find(|i| i.id == h.instrument_id);
            let account = accounts.iter().find(|a| a.id == h.account_id);
            let symbol = inst.map(|i| i.symbol.clone());
            let name = inst
                .and_then(|i| i.name.clone())
                .filter(|n| !n.trim().is_empty());
            let search_label = match (&symbol, &name) {
                (Some(sym), Some(n)) => format!("{sym} · {n}"),
                (Some(sym), None) => sym.clone(),
                (None, Some(n)) => n.clone(),
                (None, None) => String::new(),
            };
            // 市值/收益列展示币种 = 账户币（账户缺失回退成本币种，旧壳
            // valueCurrencyCode 同口径）。
            let value_currency = account
                .map(|a| a.currency_code.clone())
                .unwrap_or_else(|| h.cost_currency_code.clone());
            let value_fmt = currencies.format_of(&value_currency);
            let cost_fmt = currencies.format_of(&h.cost_currency_code);
            let mwr_row = mwr_rows
                .iter()
                .find(|row| row.account_id == h.account_id && row.instrument_id == h.instrument_id);
            let mwr_cumulative =
                mwr_row.is_some_and(|r| r.basis == ledger_investment::MwrBasis::Cumulative);
            let (mwr_text, mwr_semantic) = match mwr_row.and_then(|r| r.rate) {
                Some(rate) => {
                    let marker = if mwr_cumulative {
                        MWR_CUMULATIVE_MARKER
                    } else {
                        ""
                    };
                    (
                        Some(format!("{}{marker}", investments::format_rate(rate))),
                        Some(investments::pnl_semantic(if rate < 0.0 { -1 } else { 0 })),
                    )
                }
                None => (None, None),
            };
            HoldingRowSnap {
                row_id: format!("{}-{}", h.account_id, h.instrument_id),
                account_id: h.account_id.clone(),
                instrument_id: h.instrument_id.clone(),
                symbol,
                instrument_name: name,
                account_name: account.map(|a| a.name.clone()),
                search_label,
                quantity_text: assembly::format_quantity_text_f64(h.quantity, privacy),
                cost_text: if privacy {
                    crate::assembly::AMOUNT_PRIVACY_MASK.to_owned()
                } else {
                    assembly::format_amount_basic(h.cost_basis_cents, &cost_fmt)
                },
                price_text: h.latest_price_cents.map(|cents| {
                    if privacy {
                        return crate::assembly::AMOUNT_PRIVACY_MASK.to_owned();
                    }
                    // 现价是价格列（万分之一元刻度）；展示币种 = 价格币
                    //（缺省回落成本币——参考表未命中占位降级，旧壳同口径）。
                    let fmt = currencies.format_of(
                        h.latest_price_currency_code
                            .as_deref()
                            .unwrap_or(&h.cost_currency_code),
                    );
                    assembly::format_price_text(cents, &fmt)
                }),
                price_missing: h.latest_price_cents.is_none(),
                nav_date_text: h.latest_nav_date.clone().unwrap_or_else(|| "-".to_owned()),
                market_value_text: h.market_value_cents.map(|cents| {
                    if privacy {
                        crate::assembly::AMOUNT_PRIVACY_MASK.to_owned()
                    } else {
                        assembly::format_amount_basic(cents, &value_fmt)
                    }
                }),
                unrealized_pnl_text: h.unrealized_pnl_cents.map(|cents| {
                    if privacy {
                        crate::assembly::AMOUNT_PRIVACY_MASK.to_owned()
                    } else {
                        assembly::format_amount_basic(cents, &value_fmt)
                    }
                }),
                unrealized_pnl_semantic: h
                    .unrealized_pnl_cents
                    .filter(|_| !privacy)
                    .map(investments::pnl_semantic),
                mwr_text: mwr_text.unwrap_or_else(|| MWR_NOT_COMPUTABLE.to_owned()),
                mwr_semantic,
                mwr_cumulative,
                market_value_cents: h.market_value_cents,
                native_market_value_cents: h.native_market_value_cents,
                unrealized_pnl_cents: h.unrealized_pnl_cents,
                native_unrealized_pnl_cents: h.native_unrealized_pnl_cents,
                price_channel: inst.map(|i| i.price_channel),
            }
        })
        .collect()
}

/// 行搜索判定（旧壳 holdingMatchesSearch 同口径：统一模糊搜索语义、空/空白
/// 恒命中；查询级组合逻辑住 [`ledger_transaction::query_matches_text`] 单点
/// ——#2073 摘除「建单选项喂 pinyin filter_options」中转绕行）。
pub(crate) fn holding_matches_search(snap: &HoldingRowSnap, query: &str) -> bool {
    ledger_transaction::query_matches_text(query.trim(), &snap.search_label)
}

/// 行集过滤（旧壳 filterHoldings 同形状：账户过滤 + 搜索 AND 组合，保序）。
pub(crate) fn filter_holdings(
    rows: &[HoldingRowSnap],
    account_id: Option<&str>,
    search: &str,
) -> Vec<HoldingRowSnap> {
    rows.iter()
        .filter(|row| {
            account_id.map(|a| row.account_id == a).unwrap_or(true)
                && holding_matches_search(row, search)
        })
        .cloned()
        .collect()
}

/// 行集排序（旧壳 sortHoldings 同口径）：None = 默认标的代码字母序（缺代码
/// 行恒排末尾）；列头升降序——缺价行（金额 null）恒排末尾与方向无关，双侧
/// 均有值时**数值直比**（多币种混合无精确语义，已接受代价，旧壳
/// comparePortfolioRows 注记同词）。
pub(crate) fn sort_holdings(
    rows: Vec<HoldingRowSnap>,
    sorter: Option<HoldingsSorter>,
) -> Vec<HoldingRowSnap> {
    let mut sorted = rows;
    match sorter {
        None => sorted.sort_by(symbol_order),
        Some(sorter) => sorted.sort_by(move |a, b| sort_key(a, b, sorter)),
    }
    sorted
}

/// 列排序三态比较（空值层级单独一层）：缺价行（金额 None）恒殿后——与方向
/// 无关（旧壳 nullsLast 同义：nullsLast 包裹在方向比较外层，方向只反转数
/// 值段）；双侧均有值时**数值直比**（跨币种无精确语义，已接受代价，旧壳
/// comparePortfolioRows 注记同词）。
fn sort_key(a: &HoldingRowSnap, b: &HoldingRowSnap, sorter: HoldingsSorter) -> std::cmp::Ordering {
    let (av, bv) = match sorter.column {
        HoldingsSortColumn::MarketValue => (a.market_value_cents, b.market_value_cents),
        HoldingsSortColumn::UnrealizedPnl => (a.unrealized_pnl_cents, b.unrealized_pnl_cents),
    };
    match (av, bv) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => {
            if sorter.descend {
                y.cmp(&x)
            } else {
                x.cmp(&y)
            }
        }
    }
}

/// 默认代码字母序：缺代码行恒排末尾。
fn symbol_order(a: &HoldingRowSnap, b: &HoldingRowSnap) -> std::cmp::Ordering {
    match (&a.symbol, &b.symbol) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// 有效页数（≥1；行集空 = 一页——旧壳 pageCount 同口径；换算住
/// [`assembly::total_pages_of`] 单点，#2073）。
pub(crate) fn holdings_total_pages(row_count: usize) -> usize {
    crate::assembly::total_pages_of(row_count as i64, investments::HOLDINGS_PAGE_SIZE) as usize
}

/// 客户端切片（页码越界取末页——读出口整面，切片处仅兜底钳）。
pub(crate) fn holdings_page_slice(rows: &[HoldingRowSnap], page: usize) -> Vec<HoldingRowSnap> {
    let valid = holdings_total_pages(rows.len());
    let page = page.max(1).min(valid);
    let start = (page - 1) * investments::HOLDINGS_PAGE_SIZE;
    rows.iter()
        .skip(start)
        .take(investments::HOLDINGS_PAGE_SIZE)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

/// 装载批次六面（行集/参考/折算字典 + 累计独立两路错误面）。
struct LoadAll {
    holdings: Vec<ledger_investment::Holding>,
    instruments: Vec<ledger_investment::Instrument>,
    accounts: Vec<ledger_accounts::Account>,
    mwr_by_instrument: Vec<ledger_investment::InstrumentMwr>,
    currencies: Vec<ledger_currencies::Currency>,
    cumulative: Option<CumulativeSnap>,
    cumulative_error: Option<ledger_infra::error::AppError>,
}

/// 累计收益·折本位币单值的消费快照（域类型未 Clone——快照承载 Clone 字段，
/// 合计装配时按值重构成域类型，口径零漂移）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CumulativeSnap {
    total_cents: i64,
    native_currency: String,
}

/// 搜索输入防抖时长（旧壳 SEARCH_DEBOUNCE_MS 同值同源，app.rs 单点）。
const SEARCH_DEBOUNCE_MS: u64 = 300;

/// 账户源投影（id / 名 / 币种 / 类型 / 归档位——过滤候选面的消费字段；域
/// 行未实现全量相等，信号语义「源变化即通知」按消费字段全等比较——
/// txn_form FormAccounts 同形先例）。
#[derive(Clone, Default)]
struct Accounts(Vec<ledger_accounts::Account>);

impl PartialEq for Accounts {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.name == b.name
                    && a.currency_code == b.currency_code
                    && a.kind == b.kind
                    && a.is_archived == b.is_archived
            })
    }
}

/// 列宽带（旧壳 NDataTable sumFixedColumnWidths 各固定列宽等值档；名称并入
/// 标的列后总宽取窄窗 1024 内旁收）。唯一弹性轴合并说明：旧壳名称列唯一
/// 弹性，本壳名称并标的列（代码 · 名称上下叠行），不另设弹性列——千行内
/// 行集不需要横向滚动。
const SYMBOL_W: f32 = 230.0;
const ACCOUNT_W: f32 = 80.0;
const QUANTITY_W: f32 = 95.0;
const COST_W: f32 = 100.0;
const PRICE_W: f32 = 95.0;
const NAV_DATE_W: f32 = 95.0;
const MARKET_VALUE_W: f32 = 105.0;
const UNREALIZED_PNL_W: f32 = 115.0;
const MWR_W: f32 = 120.0;

/// 持仓卡（旧壳 HoldingsOverview 卡形态：标题头 +「同步标的信息」入口 +
/// 确定进度 + 结果/降级行 + 搜索/账户过滤 + 合计三卡（过滤子集 + 累计全
/// 账本）+ 行表客户端切片分页 + 缺价行同步引导 + 两态空态）。删除即红负向
/// 条目钉在 headless（激活边装载）与 lib 失效探针。（组件函数名
/// PascalCase——宏生成 `holdings_pane()` 构造件，同名会与原函数相撞。）
#[component]
pub(crate) fn HoldingsPane() -> impl IntoElement {
    let palette = investments::pane_palette();
    let session = investments::store();

    // 装载快照束（行集 + 参考面 + 折算字典与累计独立两路错误面）。账户源包
    // 装在投影（域行未实现全量相等——按候选面消费字段比较，语义同：「源变
    // 通知」，txn_form FormAccounts 同形先例）。
    let rows = signal(None::<Vec<HoldingRowSnap>>);
    let error = signal(None::<String>);
    let privacy = signal(false);
    let currencies = signal(CurrencyTable::default());
    let cumulative = signal(None::<CumulativeSnap>);
    let cumulative_error = signal(None::<String>);
    let accounts = signal(Accounts::default());
    // 搜索防抖序号（pane 存续期常驻；定时器闭包序号失配自弃——app.rs 同形先例）。
    let search_seq = signal(0_u64);

    // 装载（首屏激活 + 失效信号重读 + 累计重试共用；隐私开关装载时机读取）。
    let load = move || {
        error.set(None);
        let privacy_enabled = transactions::amount_privacy_enabled();
        let db = app::db_slot().clone();
        spawn(async move {
            // DbState 克隆件拥有地进阻塞闭包（`blocking` 要求 'static）——
            // 借用面溢出即编译红，投资污污同形先例。
            let Some(db) = db else {
                error.set(Some(investments::MSG_INTERNAL.to_owned()));
                return;
            };
            let outcome = blocking(move || -> ledger_infra::error::Result<LoadAll> {
                // 行集 + 参考面一次装载；累计收益命令独立（错误只降级累计卡，
                // 旧壳两路独立 Loadable 同语义）。
                let holdings = boot::portfolio_rows(&db)?;
                let instruments = boot::instrument_rows(&db)?;
                let accounts = boot::account_rows(&db)?;
                let mwr = boot::money_weighted_return_summary(&db)?;
                let currencies = boot::currency_rows(&db)?;
                let cumulative = boot::cumulative_pnl_native_total(&db);
                let (cumulative, cumulative_error) = match cumulative {
                    Ok(c) => (
                        Some(CumulativeSnap {
                            total_cents: c.total_cents,
                            native_currency: c.native_currency.clone(),
                        }),
                        None,
                    ),
                    Err(e) => (None, Some(e)),
                };
                Ok(LoadAll {
                    holdings,
                    instruments,
                    accounts,
                    mwr_by_instrument: mwr.by_instrument,
                    currencies,
                    cumulative,
                    cumulative_error,
                })
            })
            .await;
            match outcome {
                Ok(Ok(all)) => {
                    // 装配与快照双用账户行集（快照面先克隆，装配面消费原集）。
                    let accounts_rows = all.accounts.clone();
                    accounts.set(Accounts(accounts_rows));
                    let table = CurrencyTable::from_currencies(&all.currencies);
                    rows.set(Some(assemble_holding_rows(
                        &all.holdings,
                        &all.instruments,
                        &all.accounts,
                        &all.mwr_by_instrument,
                        &table,
                        privacy_enabled,
                    )));
                    currencies.set(table);
                    privacy.set(privacy_enabled);
                    error.set(None);
                    cumulative.set(all.cumulative);
                    cumulative_error.set(all.cumulative_error.map(|e| e.to_string()));
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some(investments::MSG_INTERNAL.to_owned())),
            }
        });
    };

    // 到访激活边重拉（visited-once keep-mounted：保持挂载，激活边对应旧壳
    // display 'if' 的重挂重拉语义——会话内保留的是选择不是快照；删除接线
    // 即红钉在 headless 激活装载用例）。
    let was_active = signal(false);
    effect(move || {
        let active = session.tab.get() == investments::InvestmentsTab::Holdings;
        if active && !was_active.get_untracked() {
            load();
        }
        was_active.set(active);
    });

    // 失效信号（投资族 → 行集 + 合计重读；句柄随 pane 实例存亡；删除接线
    // 即红钉在 lib 失效探针）。
    let invest_sub = signals::subscribe_investments_changed(load);
    effect(move || {
        let held: &signals::InvestmentsChangedSubscription = &invest_sub;
        let _ = held;
    });

    // 派生行集（过滤 → 排序，一次派生三处消费：表体切片 / 合计卡 / 空态判定）。
    let derived = computed(move || {
        let rows_now = rows.get().unwrap_or_default();
        sort_holdings(
            filter_holdings(
                &rows_now,
                session.holdings_account_id.get().as_deref(),
                &session.holdings_search.get(),
            ),
            session.holdings_sorter.get(),
        )
    });

    // 合计三卡格（过滤子集口径；累计全账本口径随命令透传——快照重构成域
    // 类型消费同一装配闭集；排序重排不触及合计，由派生等值跳过）。
    let stats = computed(move || {
        let visible = derived.get();
        let market_pairs: Vec<(Option<i64>, Option<i64>)> = visible
            .iter()
            .map(|r| (r.market_value_cents, r.native_market_value_cents))
            .collect();
        let unreal_pairs: Vec<(Option<i64>, Option<i64>)> = visible
            .iter()
            .map(|r| (r.unrealized_pnl_cents, r.native_unrealized_pnl_cents))
            .collect();
        let cumulative_domain =
            cumulative
                .get()
                .map(|snap| ledger_investment::CumulativePnlNativeTotal {
                    total_cents: snap.total_cents,
                    native_currency: snap.native_currency.clone(),
                });
        assembly::assemble_portfolio_stats_for_pairs(
            &market_pairs,
            &unreal_pairs,
            cumulative_domain.as_ref(),
            cumulative_error.get(),
            &currencies.get(),
            privacy.get(),
        )
    });

    // 三维任一实际变化 → 翻页归零（同值守卫；页码是展示切片不是第四维——
    // 旧壳 useHoldingsFilter 翻页归零。搜索应用值参与变化判定，回显不触发。
    // 删除本接线即红：筛选后切片仍停原页空切片）。
    let tracked_filter = std::rc::Rc::new(std::cell::RefCell::new(None::<FilterSignature>));
    let tracked_for_effect = tracked_filter;
    effect(move || {
        let current = FilterSignature::of(
            session.holdings_search.get(),
            session.holdings_account_id.get(),
            session.holdings_sorter.get(),
        );
        let mut prev = tracked_for_effect.borrow_mut();
        if prev.as_ref() == Some(&current) {
            return;
        }
        *prev = Some(current);
        drop(prev);
        session.holdings_page.set(1);
    });

    // 页码钳制回写（旧壳恢复越界钳到有效范围并写回保留态同口径；切片处仅
    // 兜底钳，不二次写回）。
    let derived_for_clamp = derived;
    effect(move || {
        let valid = holdings_total_pages(derived_for_clamp.get().len());
        let page = session.holdings_page.get().max(1);
        if page > valid {
            session.holdings_page.set(valid);
        }
    });

    // 头部：标题 + 同步入口（旧壳 card header「同步标的信息」同位；按钮
    // loading 文字形态 + 在途短路复用同一接缝——触发面单点归 investment_sync）。
    let header = view()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            text(HOLDINGS_TITLE)
                .text_size(px(font_size::TITLE))
                .text_color(palette.fg)
                .test_id("holdings-title"),
        )
        .child(
            view()
                .on_press(move |_: &PressEvent| investment_sync::trigger_sync())
                .rounded_lg()
                .px_2()
                .py_1()
                .app_class("bg-primary hover:opacity-90")
                .disabled(move || investment_sync::seam().syncing.get())
                .child(
                    text(move || {
                        if investment_sync::seam().syncing.get() {
                            investment_sync::SYNCING_LABEL
                        } else {
                            investment_sync::SYNC_LABEL
                        }
                    })
                    .text_size(px(font_size::CAPTION))
                    .text_color(crate::theme::color("primary-foreground"))
                    .test_id("holdings-sync-button"),
                ),
        );

    // 卡内主体（错误条 / 装载态 / 行集就绪三态；错误与重试共用 load）。
    let body = when(
        move || error.get().is_some(),
        move || {
            investments::retry_row(
                move || error.get().unwrap_or_default(),
                "holdings-error",
                "holdings-retry",
                palette,
                load,
            )
        },
    )
    .otherwise(move || {
        when(
            move || rows.get().is_none(),
            move || {
                text("正在加载持仓…")
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted)
                    .test_id("holdings-loading")
            },
        )
        .otherwise(move || {
            view()
                .flex_col()
                .gap_3()
                .child(investment_sync::sync_progress_bar(palette))
                .child(investment_sync::sync_result_row(palette))
                .child(filter_row(palette, session, accounts, search_seq))
                .child(holding_stats_cards(palette, stats, load))
                .child(holdings_table_section(palette, session, derived))
        })
    });

    scroll().flex_1().child(
        view().flex_col().child(
            // 卡容器（旧壳 NCard size=small 本壳等价形态）。
            view()
                .test_id("holdings-card")
                .flex_col()
                .gap_3()
                .rounded_lg()
                .mx_4()
                .my_2()
                .p_4()
                .app_class("border border-border bg-background")
                .child(header)
                .child(body),
        ),
    )
}

/// 三维过滤签名（同值守卫的比较锚；信号值的 owned 镜像）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterSignature {
    search: String,
    account_id: Option<String>,
    sorter_column: Option<HoldingsSortColumn>,
    sorter_descend: Option<bool>,
}

impl FilterSignature {
    fn of(search: String, account_id: Option<String>, sorter: Option<HoldingsSorter>) -> Self {
        Self {
            search,
            account_id,
            sorter_column: sorter.map(|s| s.column),
            sorter_descend: sorter.map(|s| s.descend),
        }
    }
}

/// 账户过滤候选面（可选择面：投资账户 − 归档，#1830 口径 + 选中归档补值
/// 「（已归档）」——旧壳 accountOptions 收口同判定，值原样保留）。
fn account_options_with_pin(
    accounts: &Accounts,
    session: investments::InvestmentsSession,
) -> Vec<CategoryOption> {
    let rows = &accounts.0;
    let selected = session.holdings_account_id.get_untracked();
    let mut options: Vec<CategoryOption> = rows
        .iter()
        .filter(|a| a.kind == ledger_accounts::AccountType::Investment && !a.is_archived)
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && options.iter().all(|o| o.id != id)
        && let Some(row) = rows.iter().find(|a| a.id == id)
    {
        options.insert(
            0,
            CategoryOption {
                id: row.id.clone(),
                label: format!("{}（已归档）", row.name),
            },
        );
    }
    options
}

/// 搜索输入防抖调度（旧壳 scheduleSearch 同形）：回显立即生效、应用值延迟
/// 300ms（期间再输入序号失配自弃，与旧壳 clearTimeout 同义）；同值不重排
/// 定时器（重复输入同一有效值不产生新翻转——旧壳 setSearch 同守卫）。
/// 防抖序号信号宿 pane 存续期（调度内现建信号随重挂重建，守卫失效——
/// app.rs 搜索防抖同一形态：序号信号常驻，定时器闭包持快照比对）。
fn schedule_holdings_search(
    session: investments::InvestmentsSession,
    seq: Signal<u64>,
    text: String,
) {
    session.holdings_search_input.set(text);
    let next = session.holdings_search_input.get_untracked();
    if next == session.holdings_search.get_untracked() {
        return;
    }
    // 定时器闭包序号守卫（app.rs 附防抖同一形态；定时器闭包持序号快照）。
    let applied = session.holdings_search;
    let current = seq.get_untracked() + 1;
    seq.set(current);
    set_timeout(
        std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
        move || {
            if seq.get() == current {
                applied.set(session.holdings_search_input.get_untracked());
                // 应用值写入后页码归零的接线由任一维实际变化 effect 统一承担
                // （本调度只负责应用值；翻页归零与重查都在同一 effect）。
            }
        },
    );
}

/// 过滤行（搜索框 + 账户单选；行集为空不渲染——旧壳 rows.length > 0 判定，
/// 空态由表体段收口）。
/// 过滤行（搜索框 + 账户单选；行集为空不渲染——旧壳 rows.length > 0 判定，
/// 空态由表体段收口）。
fn filter_row(
    palette: investments::PanePalette,
    session: investments::InvestmentsSession,
    accounts: Signal<Accounts>,
    search_seq: Signal<u64>,
) -> impl IntoElement {
    let account_options = move || account_options_with_pin(&accounts.get(), session);
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            input()
                .test_id("holdings-search")
                .value(move || session.holdings_search_input.get())
                .placeholder(HOLDINGS_SEARCH_PLACEHOLDER)
                .optional_label("持仓搜索")
                .text_color(palette.fg)
                .w(px(240.0))
                .app_class(
                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                // 搜索输入：回显写回显值，防抖调度闭包持序号信号（应用值
                // 300ms 后写会话应用值；期间再输入序号失配自弃）。
                .on_change(move |value: &str| {
                    schedule_holdings_search(session, search_seq, value.to_owned())
                }),
        )
        .child(
            view()
                .test_id("holdings-account-filter")
                .child(select::app_select(
                    "holdings-account-filter-select",
                    account_options,
                    session.holdings_account_id,
                    HOLDINGS_FILTER_ACCOUNT,
                    true,
                )),
        )
}

/// 合计三卡（过滤子集口径的市值/收益 + 全账本口径的累计）：单值、缺料警告
/// 与重试、缺现价计数小字。重试 = 重发同一条读命令（旧壳 retry 同口径，不
/// 产生任何写入）；旧壳 PortfolioStatsCards test-id 前缀 total- 同形。
fn holding_stats_cards(
    palette: investments::PanePalette,
    stats: Memo<assembly::PortfolioStatsCells>,
    retry: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;
    let danger = palette.danger;
    let gain = palette.gain;
    let loss = palette.loss;
    let neutral = fg;
    let cell_of = move |field: &'static str| {
        let cells = stats.get();
        match field {
            STATS_MARKET_VALUE_LABEL => cells.market_value,
            STATS_UNREALIZED_PNL_LABEL => cells.unrealized_pnl,
            _ => cells.cumulative_pnl,
        }
    };
    let stat = move |label: &'static str,
                     tip_body: &'static str,
                     scope: &'static str,
                     test_id: &'static str,
                     pnl: Option<(AdaptiveColor, AdaptiveColor)>| {
        view()
            .test_id(test_id)
            .flex_col()
            .gap_1()
            .flex_1()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        text(label)
                            .text_size(px(font_size::CAPTION))
                            .text_color(muted),
                    )
                    .child(popover::popover_tip(
                        format!("{test_id}-info"),
                        vec![format!("{tip_body}{scope}")],
                        investments::info_dot(palette.info),
                    )),
            )
            .child(
                // 警告态优先（缺折算汇率/命令报错 → 整卡警告 + 重试，不给半
                // 截数字）。
                when(
                    move || cell_of(label).warning.is_some(),
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(move || cell_of(label).warning.unwrap_or_default())
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(danger)
                                    .test_id(format!("{test_id}-warning").as_str()),
                            )
                            .child(
                                view()
                                    .on_press(move |_: &PressEvent| retry())
                                    .rounded_lg()
                                    .px_1()
                                    .py_0p5()
                                    .app_class("hover:bg-accent")
                                    .child(
                                        text("重试")
                                            .text_size(px(font_size::CAPTION))
                                            .text_color(muted)
                                            .test_id(format!("{test_id}-retry").as_str()),
                                    ),
                            )
                    },
                )
                .otherwise(move || {
                    view()
                        .flex_col()
                        .gap_1()
                        .child(
                            text(move || {
                                cell_of(label)
                                    .text
                                    .clone()
                                    .unwrap_or_else(|| "-".to_owned())
                            })
                            .text_size(px(font_size::LEADING))
                            .text_color(move || match (cell_of(label).pnl, pnl) {
                                (Some(PnlSemantic::Gain), Some((gain_c, _))) => gain_c,
                                (Some(PnlSemantic::Loss), Some((_, loss_c))) => loss_c,
                                _ => neutral,
                            })
                            .test_id(format!("{test_id}-value").as_str()),
                        )
                        .child(
                            when(
                                move || cell_of(label).missing_price_note.is_some(),
                                move || {
                                    text(move || {
                                        cell_of(label).missing_price_note.unwrap_or_default()
                                    })
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(muted)
                                    .test_id(format!("{test_id}-missing-price").as_str())
                                },
                            )
                            .otherwise(view),
                        )
                }),
            )
    };
    // 合计三卡 = 响应式统计行（#2057：窄窗纵向堆叠，旧壳 PortfolioStatsCards
    // 移动档 NGrid cols=1 同构；子项两档共用同一构造器）。
    adaptive::responsive_row(24.0, move || {
        vec![
            stat(
                STATS_MARKET_VALUE_LABEL,
                MARKET_VALUE_TIP,
                STATS_SCOPE_FILTERED,
                "holdings-total-market-value",
                None,
            )
            .into_element(),
            stat(
                STATS_UNREALIZED_PNL_LABEL,
                UNREALIZED_PNL_TIP,
                STATS_SCOPE_FILTERED,
                "holdings-total-unrealized-pnl",
                Some((gain, loss)),
            )
            .into_element(),
            stat(
                "累计收益",
                CUMULATIVE_PNL_TIP,
                STATS_SCOPE_WHOLE_LEDGER,
                "holdings-total-cumulative-pnl",
                Some((gain, loss)),
            )
            .into_element(),
        ]
    })
}

/// 行表段（旧壳 NDataTable 列形态的等宽自绘：列头同构布局、列头点击排序的
/// 两列受控、行格预装配零渲染期格式化、分页条单页收起；空态两型先行——
/// 「暂无持仓」与「筛选条件下无匹配持仓」可分辨）。
fn holdings_table_section(
    palette: investments::PanePalette,
    session: investments::InvestmentsSession,
    derived: Memo<Vec<HoldingRowSnap>>,
) -> impl IntoElement {
    // 空态判定（旧壳 rows.length === 0 → 暂无持仓；filteredRows.length === 0
    // → 筛选无命中——两型不叠出）。
    let bare = move || holdings_table_all(derived, session).0;
    when(bare, move || {
        text(HOLDINGS_EMPTY)
            .text_size(px(font_size::SECONDARY))
            .text_color(palette.muted)
            .test_id("holdings-empty")
    })
    .otherwise(move || {
        when(
            move || derived.get().is_empty(),
            move || {
                text(HOLDINGS_FILTER_NO_MATCH)
                    .text_size(px(font_size::SECONDARY))
                    .text_color(palette.muted)
                    .test_id("holdings-no-match")
            },
        )
        .otherwise(move || holdings_table_body(palette, session, derived))
    })
}

/// 全集行数快照（空态判定输入）——供 bare 判定的辅助形状。
fn holdings_table_all(
    derived: Memo<Vec<HoldingRowSnap>>,
    session: investments::InvestmentsSession,
) -> (bool, usize) {
    let rows = derived.get();
    let bare = rows.is_empty()
        && session.holdings_search.get().trim().is_empty()
        && session.holdings_account_id.get().is_none();
    (bare, rows.len())
}

/// 表体（列头 + 行集切片 + 分页条）。列头点击排序的两列受控（旧壳
/// `sorter: true` + 受控 sortOrder 的本壳呈现位；其余列非排序列——旧壳
/// columns 闭集同形）。
fn holdings_table_body(
    palette: investments::PanePalette,
    session: investments::InvestmentsSession,
    derived: Memo<Vec<HoldingRowSnap>>,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;

    // 派生行集切片（页码经兜底钳——派生行集在钳制回写后与切片一致；此处
    // 只读消费）。
    let page_rows = move || {
        let all = derived.get();
        let valid = holdings_total_pages(all.len());
        let page = session.holdings_page.get().max(1).min(valid);
        holdings_page_slice(&all, page)
    };

    // 列头工厂（文本列头 / 可排序列头两形态；指标符「↑」/「↓」随排序态）。
    // 概念说明列头（成本/现价，旧壳 ConceptLabel 同位；testid 与旧壳
    // holdings-cost / holdings-price 同锚）。
    let head_concept =
        move |label: &'static str, tip_body: &'static str, test_id: &'static str, width: f32| {
            view()
                .test_id(test_id)
                .flex_row()
                .items_center()
                .gap_1()
                .w(px(width))
                .child(
                    text(label)
                        .text_size(px(font_size::CAPTION))
                        .text_color(muted),
                )
                .child(popover::popover_tip(
                    format!("{test_id}-info"),
                    vec![tip_body.to_owned()],
                    investments::info_dot(palette.info),
                ))
        };
    let head_text = move |label: &'static str, width: f32| {
        text(label)
            .text_size(px(font_size::CAPTION))
            .text_color(muted)
            .w(px(width))
    };
    let sorter_indicator = move |column: HoldingsSortColumn| {
        let sorter = session.holdings_sorter.get();
        match sorter {
            Some(sorter) if sorter.column == column => {
                if sorter.descend {
                    " ↓"
                } else {
                    " ↑"
                }
            }
            _ => "",
        }
    };
    let head_sortable = move |label: &'static str,
                              column: HoldingsSortColumn,
                              width: f32,
                              test_id: &'static str| {
        let session = session;
        let selected = move || {
            session
                .holdings_sorter
                .get()
                .is_some_and(|sorter| sorter.column == column)
        };
        view()
            .on_press(move |_: &PressEvent| toggle_sorter(session, column))
            .rounded_sm()
            .px_0p5()
            .py_0p5()
            .app_class("hover:bg-accent")
            .test_id(test_id)
            .w(px(width))
            .child(
                text(move || format!("{label}{}", sorter_indicator(column)))
                    .text_size(px(font_size::CAPTION))
                    .text_color(move || if selected() { fg } else { muted }),
            )
    };

    view()
        .flex_col()
        .gap_1()
        // 列头（旧壳 NDataTable columns 词表；标的列合并代码 + 名称两行——
        // 旧壳名称列唯一弹性列的本壳窄窗等价形态）。行表列宽见列宽带常量。
        .child(
            view()
                .test_id("holdings-table-head")
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    // 标的 / 名称合并列（代码上、名称下两行头；行格同构）。
                    view()
                        .w(px(SYMBOL_W))
                        .flex_col()
                        .gap_0p5()
                        .child(head_text(COL_SYMBOL, SYMBOL_W))
                        .child(head_text(COL_NAME, 0.0)),
                )
                .child(head_text(COL_ACCOUNT, ACCOUNT_W))
                .child(head_text(COL_QUANTITY, QUANTITY_W))
                .child(head_concept(COL_COST, COST_TIP, "holdings-cost", COST_W))
                .child(head_concept(
                    COL_PRICE,
                    PRICE_TIP,
                    "holdings-price",
                    PRICE_W,
                ))
                .child(head_text(COL_NAV_DATE, NAV_DATE_W))
                .child(head_sortable(
                    COL_MARKET_VALUE,
                    HoldingsSortColumn::MarketValue,
                    MARKET_VALUE_W,
                    "holdings-col-market-value",
                ))
                .child(head_sortable(
                    COL_UNREALIZED_PNL,
                    HoldingsSortColumn::UnrealizedPnl,
                    UNREALIZED_PNL_W,
                    "holdings-col-unrealized-pnl",
                ))
                .child(
                    // 收益率列头（与金额口径并列、互不换算——概念说明挂列
                    // 头 ⓘ，旧壳 ConceptLabel 列同位）。
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .w(px(MWR_W))
                        .child(head_text(COL_MWR, 0.0))
                        .child(popover::popover_tip(
                            "holdings-mwr-info",
                            vec![MWR_TIP.to_owned(), MWR_CUMULATIVE_TIP.to_owned()],
                            investments::info_dot(palette.info),
                        )),
                ),
        )
        .child(row_body(palette, page_rows))
        .child(
            // 客户端切片分页条（单页收起——旧壳 paginate-single-page=false 同
            // 口径；页码钳制回写在装载派生处，切片处仅读）。
            when(
                move || holdings_total_pages(derived.get().len()) > 1,
                move || {
                    view().test_id("holdings-pagination").child(
                        pagination()
                            .accessibility_label("持仓分页")
                            .previous_accessibility_label("上一页")
                            .next_accessibility_label("下一页")
                            .page(move || clamp_page(derived, session) as u32)
                            .total_pages(move || holdings_total_pages(derived.get().len()) as u32)
                            .on_change(move |&picked: &u32| {
                                session.holdings_page.set(picked.max(1) as usize);
                            }),
                    )
                },
            )
            .otherwise(view),
        )
}

/// 标的代码下钻链接（持仓下钻，ADR-0135 决策 6 / 旧壳 InstrumentLink 的
/// 重生）：点击跳投资明细页签按该账户该标的过滤（该账户该标的的投资交易
/// 历史）；载荷 None = 缺码「-」纯文本不可点击。消费导航单点接缝（请求带
/// investments_drill 载荷），删除接线即红钉 headless 持仓下钻用例。
fn render_symbol_link(
    drill: Option<crate::nav_model::InvestmentsDrill>,
    label: String,
    accent: nsh_core::element::AdaptiveColor,
    fg: nsh_core::element::AdaptiveColor,
) -> nsh_core::element::View {
    match drill {
        Some(drill) => view()
            .on_press(move |_: &PressEvent| {
                crate::nav::open(
                    crate::nav_model::NavRequest::bare(crate::nav_model::ViewName::Investments)
                        .with_investments_drill(drill.clone()),
                );
            })
            .child(
                text(label)
                    .text_size(px(font_size::SECONDARY))
                    .text_color(accent),
            ),
        None => view().child(
            text(label)
                .text_size(px(font_size::SECONDARY))
                .text_color(fg),
        ),
    }
}

fn clamp_page(
    derived: Memo<Vec<HoldingRowSnap>>,
    session: investments::InvestmentsSession,
) -> usize {
    let valid = holdings_total_pages(derived.get().len());
    session.holdings_page.get().max(1).min(valid)
}

/// 列头排序点击（旧壳 NDataTable 受控 sorter 的状态机：none → 列升序 →
/// 同列降序 → 退 none；换列从升序起。纯会话槽写入，不触发重拉——排序是
/// 客户端重排不是查询）。
fn toggle_sorter(session: investments::InvestmentsSession, column: HoldingsSortColumn) {
    let current = session.holdings_sorter.get_untracked();
    let next = match current {
        None => Some(HoldingsSorter {
            column,
            descend: false,
        }),
        Some(sorter) if sorter.column == column && !sorter.descend => Some(HoldingsSorter {
            column,
            descend: true,
        }),
        _ => None,
    };
    session.holdings_sorter.set(next);
}

/// 行体（each 键 = row_id：账户 + 标的，同一标的跨账户持仓各行不撞车），
/// 行格预装配零渲染期格式化。单元格闭包只消费快照 owned 值，不持引用。
fn row_body(
    palette: investments::PanePalette,
    page_rows: impl Fn() -> Vec<HoldingRowSnap> + Copy + 'static,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;
    // 下钻强调色（旧壳 InstrumentLink accentColor 同语义：主题 primary 令牌
    // 经 theme::color 单点消费，缺失落可读兑底——accounts.rs 账户名下钻链接
    // 同款先例）。
    let accent = crate::theme::color("primary");
    let gain = palette.gain;
    let loss = palette.loss;
    each(
        page_rows,
        |row: &HoldingRowSnap| row.row_id.clone(),
        move |item: ReadSignal<HoldingRowSnap>, _index: ReadSignal<usize>| {
            let row = item.get_untracked();
            let row_id = row.row_id.clone();
            // 缺价行同步引导锚点（行身份键预派生；when 条件随快照预判定，闭
            // 包不持引用）。
            let guide_id = format!("holdings-row-{}-missing-price-sync", row_id);
            // 收益率语义色（可计算着涨跌色，「无法计算」中性不猜解）。
            let semantic_color = match row.mwr_semantic {
                Some(PnlSemantic::Gain) => gain,
                Some(PnlSemantic::Loss) => loss,
                _ => fg,
            };
            // 缺价行引导（issue #1193）：只读后端派生的价格通道事实分流。
            // 行情/净值通道（Quote/FundNav）给行内「同步」（触发归同一同步
            // 接缝单点，在途短路复用）；手动报价通道行内不设引导（录价入口
            // 住标的库页签，#2047 ⑯）；无通道/未知不给。向外冒用也随快照预判定，
            // 不在闭包里重新读字典。
            let price_guide_sync = row.price_missing
                && matches!(
                    row.price_channel,
                    Some(
                        ledger_investment::PriceChannel::Quote
                            | ledger_investment::PriceChannel::FundNav
                    )
                );
            // 标的代码下钻链接（ADR-0135 决策 6 / 旧壳 InstrumentLink）：载荷
            // 预派生（row 快照 owned 值 + 常量页签词），不持任何借用；缺码
            // 无链接（纯「-」，不可点击）。
            let link_drill = row
                .symbol
                .as_ref()
                .map(|_| crate::nav_model::InvestmentsDrill {
                    tab: investments::LEDGER_TAB_URL_VALUE,
                    account_id: Some(row.account_id.clone()),
                    instrument_id: Some(row.instrument_id.clone()),
                });
            let link_symbol_text = row.symbol.clone().unwrap_or_else(|| "-".to_owned());
            let symbol_cell = render_symbol_link(link_drill, link_symbol_text, accent, fg)
                .test_id(format!("holdings-row-{}-symbol", row_id).as_str());
            view()
                .test_id(format!("holdings-row-{row_id}").as_str())
                .flex_row()
                .items_center()
                .gap_1()
                .min_h(px(36.0))
                .child(
                    // 标的 / 名称合并列（代码上、名称下；代码缺失「-」，
                    // 名称缺失同降级——旧壳 render 同口径）。
                    view()
                        .w(px(SYMBOL_W))
                        .flex_col()
                        .gap_0p5()
                        .child(symbol_cell)
                        .child(
                            text(
                                row.instrument_name
                                    .clone()
                                    .unwrap_or_else(|| "-".to_owned()),
                            )
                            .text_size(px(font_size::CAPTION))
                            .text_color(muted)
                            .test_id(format!("holdings-row-{}-name", row_id).as_str()),
                        ),
                )
                .child(
                    text(row.account_name.clone().unwrap_or_else(|| "-".to_owned()))
                        .text_size(px(font_size::SECONDARY))
                        .text_color(fg)
                        .w(px(ACCOUNT_W)),
                )
                .child(
                    text(row.quantity_text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(fg)
                        .w(px(QUANTITY_W)),
                )
                .child(
                    text(row.cost_text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(fg)
                        .w(px(COST_W)),
                )
                .child(
                    // 现价：缺价「-」+ 通道可行时行内同步引导（缺价行引导
                    // 动作与卡顶同步钮同一接缝；锚点预派生，条件随快照预判
                    // 定，行键不持引用）。
                    view()
                        .w(px(PRICE_W))
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            text(row.price_text.clone().unwrap_or_else(|| "-".to_owned()))
                                .text_size(px(font_size::SECONDARY))
                                .text_color(fg),
                        )
                        .child(
                            when(
                                move || price_guide_sync,
                                move || {
                                    view()
                                        .on_press(move |_: &PressEvent| {
                                            investment_sync::trigger_sync()
                                        })
                                        .rounded_sm()
                                        .px_1()
                                        .py_0p5()
                                        .app_class("hover:bg-accent")
                                        .child(
                                            text(MISSING_PRICE_SYNC)
                                                .text_size(px(font_size::CAPTION))
                                                .text_color(palette.info)
                                                .test_id(guide_id.as_str()),
                                        )
                                },
                            )
                            .otherwise(view),
                        ),
                )
                .child(
                    text(row.nav_date_text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(muted)
                        .w(px(NAV_DATE_W)),
                )
                .child(
                    text(
                        row.market_value_text
                            .clone()
                            .unwrap_or_else(|| "-".to_owned()),
                    )
                    .text_size(px(font_size::SECONDARY))
                    .text_color(fg)
                    .w(px(MARKET_VALUE_W)),
                )
                .child(
                    // 未实现盈亏：缺价「-」（不可着色不猜零）；有值红涨绿跌。
                    text(
                        row.unrealized_pnl_text
                            .clone()
                            .unwrap_or_else(|| "-".to_owned()),
                    )
                    .text_size(px(font_size::SECONDARY))
                    .text_color(match row.unrealized_pnl_semantic {
                        Some(PnlSemantic::Gain) => gain,
                        Some(PnlSemantic::Loss) => loss,
                        _ => fg,
                    })
                    .w(px(UNREALIZED_PNL_W)),
                )
                .child(
                    // 收益率：可计算着涨跌色 + 未年化角标（含义归列头说明）；
                    // 无解「无法计算」中性不猜解。
                    text(row.mwr_text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(semantic_color)
                        .test_id(format!("holdings-row-{}-mwr", row_id).as_str())
                        .w(px(MWR_W)),
                )
        },
    )
}

// ---------------------------------------------------------------------------
// 单测（词表纯函数与装配口径：渲染期零格式化兜底——行格文本、过滤排序、
// 分页切片与合计三卡；测试整体豁免 ADR-0060 经 crate 根放行）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cny_table() -> CurrencyTable {
        let cny = ledger_currencies::Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        CurrencyTable::from_currencies([&cny])
    }

    /// 行域样本（#2045 headless 造数同刻度：TST 1 份、成本 ¥1、现价 ¥1、
    /// 市值 ¥1、收益 ¥0；MWR 可计算 +0.05 未年化）。
    fn holding() -> ledger_investment::Holding {
        ledger_investment::Holding {
            id: "h-1".to_owned(),
            account_id: "acc-1".to_owned(),
            instrument_id: "ins-1".to_owned(),
            quantity: 1.0,
            cost_basis_cents: 100,
            cost_currency_code: "CNY".to_owned(),
            latest_price_cents: Some(10_000),
            latest_price_currency_code: Some("CNY".to_owned()),
            latest_nav_date: None,
            market_value_cents: Some(100),
            unrealized_pnl_cents: Some(0),
            native_market_value_cents: Some(100),
            native_unrealized_pnl_cents: Some(0),
            updated_at: String::new(),
        }
    }

    fn instrument() -> ledger_investment::Instrument {
        ledger_investment::Instrument {
            id: "ins-1".to_owned(),
            symbol: "TST".to_owned(),
            kind: ledger_investment::InstrumentType::Stock,
            name: Some("测试标的".to_owned()),
            currency_code: "CNY".to_owned(),
            market: "unknown".to_owned(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            source: "manual".to_owned(),
            price_cents: Some(10_000),
            invested: true,
            price_channel: ledger_investment::PriceChannel::Manual,
        }
    }

    fn account() -> ledger_accounts::Account {
        ledger_accounts::Account {
            id: "acc-1".to_owned(),
            name: "证券".to_owned(),
            kind: ledger_accounts::AccountType::Investment,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: 0,
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            is_deleted: false,
            is_hidden: false,
            is_archived: false,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        }
    }

    fn mwr_row() -> ledger_investment::InstrumentMwr {
        ledger_investment::InstrumentMwr {
            account_id: "acc-1".to_owned(),
            instrument_id: "ins-1".to_owned(),
            currency_code: "CNY".to_owned(),
            basis: ledger_investment::MwrBasis::Cumulative,
            rate: Some(0.05),
        }
    }

    fn rows() -> Vec<HoldingRowSnap> {
        assemble_holding_rows(
            &[holding()],
            &[instrument()],
            &[account()],
            &[mwr_row()],
            &cny_table(),
            false,
        )
    }

    #[test]
    fn holding_row_cells_match_old_shell_vocabulary() {
        let row = &rows()[0];
        assert_eq!(row.symbol.as_deref(), Some("TST"));
        assert_eq!(row.instrument_name.as_deref(), Some("测试标的"));
        assert_eq!(row.search_label, "TST · 测试标的", "搜索判定文本同拼法");
        assert_eq!(row.account_name.as_deref(), Some("证券"));
        assert_eq!(row.quantity_text, "1", "数量去零");
        assert_eq!(row.cost_text, "¥1");
        assert_eq!(row.price_text.as_deref(), Some("¥1"), "现价万位刻度");
        assert_eq!(row.nav_date_text, "-", "非基金行净值日期缺省「-」");
        assert_eq!(row.market_value_text.as_deref(), Some("¥1"));
        assert_eq!(row.unrealized_pnl_text.as_deref(), Some("¥0"));
        assert_eq!(
            row.unrealized_pnl_semantic,
            Some(PnlSemantic::Gain),
            "零收益归涨（红涨绿跌：正/零归涨）"
        );
        assert_eq!(row.mwr_text, "+5.00%*", "未年化行带角标");
        assert_eq!(row.mwr_semantic, Some(PnlSemantic::Gain));
        assert!(row.mwr_cumulative);
        assert!(!row.price_missing);
    }

    #[test]
    fn holding_row_privacy_masks_all_amount_cells() {
        let rows = assemble_holding_rows(
            &[holding()],
            &[instrument()],
            &[account()],
            &[mwr_row()],
            &cny_table(),
            true,
        );
        let row = &rows[0];
        for text in [
            row.quantity_text.as_str(),
            row.cost_text.as_str(),
            row.price_text.clone().unwrap_or_default().as_str(),
            row.market_value_text.clone().unwrap_or_default().as_str(),
            row.unrealized_pnl_text.clone().unwrap_or_default().as_str(),
        ] {
            assert_eq!(text, assembly::AMOUNT_PRIVACY_MASK, "隐私态统一掩码");
        }
        assert!(row.unrealized_pnl_semantic.is_none(), "掩码下不着色");
    }

    #[test]
    fn holding_missing_price_row_degrades_and_channels_guide() {
        let mut no_price = holding();
        no_price.latest_price_cents = None;
        no_price.market_value_cents = None;
        no_price.native_market_value_cents = None;
        no_price.unrealized_pnl_cents = None;
        no_price.native_unrealized_pnl_cents = None;
        let rows = assemble_holding_rows(
            &[no_price],
            &[instrument()],
            &[account()],
            &[mwr_row()],
            &cny_table(),
            false,
        );
        let row = &rows[0];
        assert!(row.price_missing);
        assert_eq!(row.price_text, None);
        assert_eq!(row.market_value_text, None);
        assert_eq!(row.unrealized_pnl_text, None, "缺价行市值/收益双双缺料");
        // 手动报价通道行内不设引导（录价入口住标的库页签，#2047 ⑯）；
        // 行情/净值通道给同步引导（引导分流读通道事实）。
        assert_eq!(
            row.price_channel,
            Some(ledger_investment::PriceChannel::Manual)
        );

        // 行情通道（market sh）→ 同步引导在位。
        let mut quote_inst = instrument();
        quote_inst.market = "sh".to_owned();
        quote_inst.price_channel = ledger_investment::PriceChannel::Quote;
        let rows = assemble_holding_rows(
            &[holding()],
            &[quote_inst],
            &[account()],
            &[mwr_row()],
            &cny_table(),
            false,
        );
        assert_eq!(
            rows[0].price_channel,
            Some(ledger_investment::PriceChannel::Quote),
            "引导分流只读后端派生事实，前端不自行按类型×市场推断"
        );
    }

    #[test]
    fn holding_search_filter_and_sort_match_old_shell() {
        // 搜索：判定文本「代码 · 名称」，统一模糊语义命中；空白恒命中。
        let rows = rows();
        assert!(holding_matches_search(&rows[0], "TST"));
        assert!(holding_matches_search(&rows[0], "测试"));
        assert!(holding_matches_search(&rows[0], "  "));
        assert!(!holding_matches_search(&rows[0], "ZZZ"));
        // 过滤：账户 AND 搜索，保序。
        let filtered = filter_holdings(&rows, Some("acc-1"), "TST");
        assert_eq!(filtered.len(), 1);
        assert!(filter_holdings(&rows, Some("other"), "").is_empty());
        // 排序：默认代码字母序（缺代码行殿后）；两列缺价行殿后与方向无关。
        let make = |symbol: Option<String>, market_value: Option<i64>| HoldingRowSnap {
            row_id: format!("r-{}", symbol.clone().unwrap_or_default()),
            account_id: "acc-1".to_owned(),
            instrument_id: "ins-1".to_owned(),
            symbol,
            instrument_name: None,
            account_name: None,
            search_label: String::new(),
            quantity_text: String::new(),
            cost_text: String::new(),
            price_text: None,
            price_missing: market_value.is_none(),
            nav_date_text: "-".to_owned(),
            market_value_text: None,
            unrealized_pnl_text: None,
            unrealized_pnl_semantic: None,
            mwr_text: MWR_NOT_COMPUTABLE.to_owned(),
            mwr_semantic: None,
            mwr_cumulative: false,
            market_value_cents: market_value,
            native_market_value_cents: market_value,
            unrealized_pnl_cents: market_value,
            native_unrealized_pnl_cents: market_value,
            price_channel: None,
        };
        let mixed = vec![
            make(Some("Z".to_owned()), None),
            make(None, Some(300)),
            make(Some("B".to_owned()), Some(100)),
            make(Some("A".to_owned()), Some(200)),
        ];
        let default_sorted = sort_holdings(mixed.clone(), None);
        assert_eq!(
            default_sorted
                .iter()
                .map(|r| r.symbol.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["A", "B", "Z", ""],
            "None = 默认标的代码字母序（缺代码行殿后）"
        );
        let sorted = sort_holdings(
            mixed.clone(),
            Some(HoldingsSorter {
                column: HoldingsSortColumn::MarketValue,
                descend: false,
            }),
        );
        assert_eq!(
            sorted
                .iter()
                .map(|r| r.market_value_cents)
                .collect::<Vec<_>>(),
            vec![Some(100), Some(200), Some(300), None],
            "缺价行殿后与方向无关"
        );
        let sorted_desc = sort_holdings(
            mixed,
            Some(HoldingsSorter {
                column: HoldingsSortColumn::UnrealizedPnl,
                descend: true,
            }),
        );
        assert_eq!(
            sorted_desc
                .iter()
                .map(|r| r.market_value_cents)
                .collect::<Vec<_>>(),
            vec![Some(300), Some(200), Some(100), None],
            "降序直比、缺价行仍殿后"
        );
    }

    #[test]
    fn holdings_pagination_slices_with_page_size_twenty() {
        let rows: Vec<HoldingRowSnap> = (0..25).map(|i| make_row(format!("S{:02}", i))).collect();
        assert_eq!(holdings_total_pages(25), 2);
        assert_eq!(holdings_total_pages(0), 1, "空行集一页");
        let page1 = holdings_page_slice(&rows, 1);
        assert_eq!(page1.len(), 20);
        assert_eq!(page1[0].symbol.as_deref(), Some("S00"));
        let page2 = holdings_page_slice(&rows, 2);
        assert_eq!(page2.len(), 5);
        assert_eq!(page2[0].symbol.as_deref(), Some("S20"));
        // 页码越界取末页（切片处兜底钳）。
        assert_eq!(holdings_page_slice(&rows, 99).len(), 5);
    }

    fn make_row(symbol: String) -> HoldingRowSnap {
        HoldingRowSnap {
            row_id: format!("r-{symbol}"),
            account_id: "acc-1".to_owned(),
            instrument_id: "ins-1".to_owned(),
            symbol: Some(symbol),
            instrument_name: None,
            account_name: None,
            search_label: String::new(),
            quantity_text: String::new(),
            cost_text: String::new(),
            price_text: None,
            price_missing: true,
            nav_date_text: "-".to_owned(),
            market_value_text: None,
            unrealized_pnl_text: None,
            unrealized_pnl_semantic: None,
            mwr_text: MWR_NOT_COMPUTABLE.to_owned(),
            mwr_semantic: None,
            mwr_cumulative: false,
            market_value_cents: Some(0),
            native_market_value_cents: Some(0),
            unrealized_pnl_cents: Some(0),
            native_unrealized_pnl_cents: Some(0),
            price_channel: None,
        }
    }

    #[test]
    fn holdings_stats_cards_follow_filtered_subset_and_whole_ledger_cumulative() {
        // 装配单点 = assemble_portfolio_stats_for_pairs（#2045 概览同源）；
        // 此处钉持仓页签消费形状：两列行级对 + 累计独立 + 口径句。
        let visible = rows();
        let market_pairs: Vec<(Option<i64>, Option<i64>)> = visible
            .iter()
            .map(|r| (r.market_value_cents, r.native_market_value_cents))
            .collect();
        let unreal_pairs: Vec<(Option<i64>, Option<i64>)> = visible
            .iter()
            .map(|r| (r.unrealized_pnl_cents, r.native_unrealized_pnl_cents))
            .collect();
        let cumulative = ledger_investment::CumulativePnlNativeTotal {
            total_cents: 12_000,
            native_currency: "CNY".to_owned(),
        };
        let cells = assembly::assemble_portfolio_stats_for_pairs(
            &market_pairs,
            &unreal_pairs,
            Some(&cumulative),
            None,
            &cny_table(),
            false,
        );
        assert_eq!(cells.market_value.text.as_deref(), Some("¥1"));
        assert_eq!(cells.unrealized_pnl.text.as_deref(), Some("¥0"));
        assert_eq!(cells.cumulative_pnl.text.as_deref(), Some("¥120"));
        assert!(cells.cumulative_pnl.warning.is_none());
        assert!(
            crate::investments_holdings::STATS_SCOPE_FILTERED.contains("随当前搜索与账户过滤收窄"),
            "市值/收益两卡挂过滤子集作用域句"
        );
        assert!(
            STATS_SCOPE_WHOLE_LEDGER.contains("不随搜索或过滤收窄"),
            "累计卡挂全账本作用域句"
        );
        // 累计命令报错 → 累计卡警告不给半截数字（两列值照常）。
        let cells = assembly::assemble_portfolio_stats_for_pairs(
            &market_pairs,
            &unreal_pairs,
            None,
            Some("fx.rate-missing".to_owned()),
            &cny_table(),
            false,
        );
        assert!(
            cells.cumulative_pnl.warning.is_some(),
            "累计读失败仅降级累计卡"
        );
        // 该分支的格式锚缺缺省（native_currency 仅随就绪命令透传）——分量
        // 命令未就绪/报错时两列值按缺省格式呈现（旧壳 getCurrency("") 降级
        // 裸数字同口径）。
        assert_eq!(cells.market_value.text.as_deref(), Some("1"));
    }
}

// ---------------------------------------------------------------------------
// lib 探针（headless 引擎内驱动；investments.rs 探针同形——失效信号广播
// 必须发生在引擎 UI 线程，集成测试线程触不到 thread_local 订阅者表，探针
// 组件在引擎线程内经真实按钮触发广播，消费面挂真实投资视图）。
//
// 「删除即红」负向条目：删 [`crate::investments::investments_view`] /
// [`holdings_pane`] 内的投资族订阅接线（`invest_sub` effect）任一 → 本探
// 针红（库中已写价而持仓卡仍旧）；删 [`crate::signals`] 投资族广播转发 →
// 探针按钮失去生产语义。staleness 探针同政治：删 [`staleness_alert`] 内的
// 失效重查订阅 → 同步写价后提示不再收起（本探针红）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod probe {
    use nsh_core::prelude::*;
    use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
    use nsh_testing::Headless;

    use crate::app::{reset_book_assembly, run_startup, set_book_dir, test_assembly_guard};
    use crate::investments;
    use crate::signals;

    /// 探针根组件：投资失效广播按钮 + 真实投资视图（预置落位持仓页签）。
    #[component]
    fn HoldingsProbeApp() -> impl IntoElement {
        // 会话槽预置落位持仓页签（探针不走侧栏切换，直达面积）。
        investments::store()
            .tab
            .set(investments::InvestmentsTab::Holdings);
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
            .child(investments::investments_view())
    }

    /// 探针造数账本（持仓无效信号重读探针惯例）：投资账户 + 标的 TST 现价
    /// ¥1（万分之一元 10_000）+ 买入 1 份@¥1 → 持仓行集：市值 ¥1、成本
    /// ¥1、现价 ¥1。
    fn seed_probe_book(dir: &std::path::Path, stale: bool) {
        let db = crate::boot::boot_ledger(dir).unwrap();
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
                    // 行情通道（Quote）：staleness 计数面 = 有通道标的；无
                    // 通道行「同步修不了」不在过期计数内（域单点裁决，前端
                    // 不另修口径）。
                    market: Some("sh".to_owned()),
                },
            )
            .unwrap();
            if !stale {
                ledger_investment::create_market_price(
                    &conn,
                    MarketPriceInput {
                        instrument_id: id.clone(),
                        price_cents: 10_000,
                        currency_code: "CNY".to_owned(),
                        priced_at: crate::boot::today_string(),
                        source: Some("manual".to_owned()),
                    },
                )
                .unwrap();
            }
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
                    fee_cents: Some(0),
                    price_cents: Some(10_000),
                    date: crate::boot::today_string(),
                    note: Some("买入".to_owned()),
                    ..probe_input()
                },
            )
            .unwrap();
        }
    }

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

    /// 持仓失效信号重读：合计市值卡首读 ¥1；后台录现价 ¥2 → 广播投资失效
    /// → 行集/合计现值化（¥2）。「删除即红」见模块头注。
    #[test]
    fn holdings_reread_on_investments_invalidation_signal() {
        let _guard = test_assembly_guard();
        reset_book_assembly();
        let dir = std::env::temp_dir().join(format!(
            "nsh-app-holdings-probe-{}",
            ledger_infra::ids::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ledger.db"), b"").unwrap();
        seed_probe_book(&dir, false);

        set_book_dir(dir);
        assert!(matches!(run_startup(), crate::app::StartupView::Ready));

        let mut app = Headless::start(EngineConfig {
            viewport: nsh_engine::Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
            environment: Environment {
                os: Os::Linux,
                idiom: Idiom::Desktop,
                ..Environment::default()
            },
            ui_sets: vec![nsh_base::registry()],
            ..EngineConfig::new(Root::of::<HoldingsProbeApp>())
        })
        .unwrap();
        app.wait_for("id:holdings-total-market-value-value")
            .unwrap();
        app.wait_for("text:¥1").unwrap();

        // 后台行情写：现价翻倍（万分之一元 20_000 = ¥2）。
        {
            let db_session = crate::app::session_handle().unwrap();
            let conn = db_session.conn.lock().unwrap();
            let instrument_id: String = conn
                .query_row("SELECT id FROM instruments LIMIT 1", [], |r| r.get(0))
                .unwrap();
            ledger_investment::create_market_price(
                &conn,
                ledger_investment::MarketPriceInput {
                    instrument_id,
                    price_cents: 20_000,
                    currency_code: "CNY".to_owned(),
                    priced_at: crate::boot::today_string(),
                    source: Some("manual".to_owned()),
                },
            )
            .unwrap();
        }
        app.press("id:probe-broadcast-investments").unwrap();
        app.wait_for("text:¥2").unwrap();
    }

    /// staleness 提示条收起（过度提示纽面：提示随失效信号重查自动消失）：
    /// 无价持仓 → 提示条在屏（ landlord 缺现价）；后台手动写价（水线变新）
    /// → 广播失效 → 重查计数归零 → 提示条收起。「删除即红」见模块头注。
    #[test]
    fn staleness_alert_clears_when_price_writes_refresh_waterline() {
        let _guard = test_assembly_guard();
        reset_book_assembly();
        let dir = std::env::temp_dir().join(format!(
            "nsh-app-staleness-probe-{}",
            ledger_infra::ids::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ledger.db"), b"").unwrap();
        // stale = true：不写初始价（持仓缺现价 = 水位缺失入选）。
        seed_probe_book(&dir, true);

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
            ..EngineConfig::new(Root::of::<HoldingsProbeApp>())
        })
        .unwrap();
        app.wait_for("id:staleness-alert").unwrap();
        app.wait_for("id:staleness-message").unwrap();
        assert!(
            app.shows("有 1 个标的的行情 / 净值可能已过期（超过 7 天未更新，或持仓缺现价）。"),
            "缺现价持仓入选提示：{:?}",
            app.screen().texts()
        );

        // 后台手动写价（水线变新）→ 广播失效 → 重查。
        {
            let db_session = crate::app::session_handle().unwrap();
            let conn = db_session.conn.lock().unwrap();
            let instrument_id: String = conn
                .query_row("SELECT id FROM instruments LIMIT 1", [], |r| r.get(0))
                .unwrap();
            ledger_investment::create_market_price(
                &conn,
                ledger_investment::MarketPriceInput {
                    instrument_id,
                    price_cents: 10_000,
                    currency_code: "CNY".to_owned(),
                    priced_at: crate::boot::today_string(),
                    source: Some("manual".to_owned()),
                },
            )
            .unwrap();
        }
        app.press("id:probe-broadcast-investments").unwrap();
        app.wait_for_gone("id:staleness-alert", std::time::Duration::from_secs(5))
            .unwrap();
    }
}
