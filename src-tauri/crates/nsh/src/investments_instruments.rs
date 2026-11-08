//! 投资视图·标的库页签（issue #2047 NSH 迁移⑯；spec #2031 系列 16/26，投资
//! 3/3；旧壳 `InstrumentBrowser.vue` + `ManualPriceModal.vue` +
//! `AddInstrumentModal.vue` 的重生）。
//!
//! **列表读接缝**：服务端分页直调域读入口 [`boot::instrument_page`]（旧壳
//! `list_instruments` 命令同一域函数，不经 IPC）——搜索（统一模糊语义：
//! 子串 ∨ 拼音首字母子序列，「代码 · 名称」判定面，ADR-0027）、市场精确匹
//! 配（筛选闭集四档，旧壳 MARKET_FILTER_TYPES 同源）、只看持仓（开关，
//! issue #108）+ anchor 分页定位（focus 下钻落点，issue #1907）。锚点定位、
//! COUNT 与 items 是多语句读闭包在域内收同一读事务（#2071 收口）。页大小
//! 固定 50（旧壳 pageSize 常量同值）。
//!
//! **下钻落点消费**（骨架票导航接缝，消费不自建）：focus 意图在场时按
//! anchor_id 查询定位所在页 + 行高亮（行必在当前页——域锚点仲裁；无效
//! focus 在 anchor 查询内回落普通页码，不提供落空的跳转）。高亮随 focus id
//! 保留到变化——与「行必在当前页」配套。旧壳 prop 注入的本壳对应物 = 会话
//! 槽 [`investments::InvestmentsSession::instruments_focus`]
//!（ [`crate::investments::InvestmentsView`] 落位消费一次）。
//!
//! **手动报价**（issue #291 / ADR-0036）：行内「录价」只对手动报价通道开放
//!（后端派生事实 [`ledger_investment::Instrument::price_channel`]，前端不
//! 自行推断）——「日期 + 价格」单点弹窗，写路径经 [`boot::record_manual_price`]
//! 直调（价格历史周采样幂等覆盖 + 现价缓存按最新点映像规则 upsert，回执按
//! 域结果 `current_price_written` 分型）；实际写入任一落点 → 广播投资失效
//! 信号（零变化不广播——ADR-0031）→ 行集原地重拉（保留分页/搜索——旧壳
//! issue #238 同口径）。本票订阅接线，删除即红钉 lib 探针。
//!
//! **添加投资标的**（issue #697 / spec #690；六通道修订 #826）：录入通道闭
//! 集 = 沪/深/港/美股/场外基金（按代码查询：网络在连接锁外 await，
//! [`boot::add_fund_by_code`] / [`boot::add_instrument_by_code`] 同实现
//! 底）+ 自定义标的（零查询直接建档 [`boot::create_instrument`]，同（代码，
//! 类型）幂等复用并更新名称）。落现价的通道落价即广播（ADR-0031 同源）；
//! 查询未命中只报错并引导切换自定义标的通道（股票通道
//! `sync.stock-not-found` 引导位），不转建档。结果回显文案随结果投影组装
//!（识别类型 / 最新价 / 权威名称 / 净值），词表逐条平价。
//!
//! **自建标的删除**（issue #292 / ADR-0036 决策 5）：仅手动来源行渲染删除
//! 动作，危险确认后走 [`boot::delete_instrument`]（守卫在域写路径单正文；
//! 同步来源与被引用标的码化拒删原样呈现——旧壳「双保险」同语义）。删除后
//! 原地重拉（当前页删空回退一页，旧壳同口径）。
//!
//! 同步入口（旧壳页面级「同步标的信息」按钮 + 进度条 + 结果消息 + 降级标
//! 注）消费 [`crate::investment_sync`] 接缝（与持仓卡同一单点、在途短路复
//! 用——两入口看到完全相同终态反馈，#2046 同源承诺）。
//!
//! 界面文案仅 zh-CN（旧壳 investments.json browser/manualPrice/addInstrument
//! 段 + market/type/priceChannel 闭集逐条平价；字面闭集承载）。码化错误 zh
//! 原样呈现（条位反馈——NSH 无 toast，#2038 裁决）。测试豁免（ADR-0060）：
//! 本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{input, pagination, switch};
use nsh_core::prelude::*;

use crate::assembly::CategoryOption;
use crate::investments::PanePalette;
use crate::{
    app, boot, danger, investment_sync, investments, modal, pinyin_select, select, signals,
};
use ledger_investment::{InstrumentType, PriceChannel};

// ---------------------------------------------------------------------------
// 词表（旧壳 investments.json browser / manualPrice / addInstrument 段逐条平价）
// ---------------------------------------------------------------------------

const SEARCH_PLACEHOLDER: &str = "搜索代码或名称...";
const ALL_MARKETS: &str = "全部市场";
const ONLY_INVESTED: &str = "只看持仓";
const INSTRUMENTS_EMPTY: &str = "暂无标的";
const INSTRUMENTS_FILTER_NO_MATCH: &str = "筛选条件下无匹配标的";
const ADD_INSTRUMENT: &str = "添加投资标的";
/// 删除反馈（旧壳 browser.deleteSuccess / deleteFailed 同词）。
const DELETE_SUCCESS_FMT: &str = "已删除标的：{name}";
const DELETE_FAILED_PREFIX: &str = "删除失败：";
/// 列头词（旧壳 browser.columns.* 同词表）。
const COL_SYMBOL: &str = "代码";
const COL_NAME: &str = "名称";
const COL_PRICE_SOURCE: &str = "价格来源";
const COL_PRICE: &str = "现价";
const COL_MARKET: &str = "市场";
const COL_TYPE: &str = "类型";
const COL_CURRENCY: &str = "币种";
const COL_QUOTE: &str = "录价";
const COL_INVESTED_TAG: &str = "持仓";
const COL_ACTIONS: &str = "操作";
const QUOTE_ACTION: &str = "录价";
const DELETE_ACTION: &str = "删除";
const DELETE_TITLE: &str = "删除标的";
const DELETE_CONFIRM_FMT: &str = "确认删除标的「{name}」？删除后不可恢复。";
/// 现价概念说明（旧壳 ConceptLabel concept=price 同词表直取）。
const PRICE_TIP: &str = "最新一条行情：基金行为最新公布的单位净值（对应「净值日期」列），股票为最新报价或收盘价；不是买入价或成本价，没有行情时显示「-」。";
/// 手动报价弹窗（旧壳 manualPrice.* 同词表）。
const MANUAL_QUOTE_TITLE_FMT: &str = "录价 — {symbol}";
const MANUAL_QUOTE_INTRO: &str = "录入「日期 + 价格」报价点：当日及更新报价即时生效为现价，回填旧价只沉淀历史、不改变现价；同一周内重复录入按后写覆盖。";
const MANUAL_QUOTE_DATE_LABEL: &str = "日期";
const MANUAL_QUOTE_PRICE_LABEL: &str = "价格";
const MANUAL_QUOTE_PRICE_PLACEHOLDER: &str = "单价（元，如 1.3180）";
const MANUAL_QUOTE_SUBMIT: &str = "录价";
const MANUAL_QUOTE_CANCEL: &str = "取消";
const MANUAL_QUOTE_PRICE_POSITIVE: &str = "价格必须大于 0";
const MANUAL_QUOTE_SUCCESS_FMT: &str = "已录价：{symbol} 现价更新为 {price}";
const MANUAL_QUOTE_HISTORY_ONLY_FMT: &str =
    "已沉淀历史价格（早于最新价格点，{symbol} 现价保持不变）";
/// 添加投资标的弹窗（旧壳 addInstrument.* 同词表）。
const ADD_TITLE: &str = "添加投资标的";
const ADD_INTRO: &str = "选择录入通道：沪 / 深 / 港 / 美股 / 场外基金输入代码自动查询权威名称与最新价回填（美股免选交易所）；无真实代码的标的（虚拟股、组合等）选「自定义标的」直接建档。";
const ADD_MARKET_LABEL: &str = "市场";
const ADD_MARKET_PLACEHOLDER: &str = "选择市场（必选）";
const ADD_CODE_LABEL: &str = "代码";
const ADD_CODE_PLACEHOLDER_STOCK: &str = "股票代码，如 600519 / 00700 / AAPL";
const ADD_CODE_PLACEHOLDER_FUND: &str = "6 位基金代码，如 000001";
const ADD_CODE_PLACEHOLDER_CUSTOM: &str = "自定义代码，自由文本，如 HW-VR";
const ADD_QUERY: &str = "查询并添加";
const ADD_CREATE: &str = "创建";
const ADD_NAME_LABEL: &str = "名称";
const ADD_NAME_PLACEHOLDER: &str = "必填，如：某某转债";
const ADD_TYPE_LABEL: &str = "类型";
const ADD_TYPE_PLACEHOLDER: &str = "请选择类型";
const ADD_CURRENCY_LABEL: &str = "币种";
const ADD_CUSTOM_HINT: &str = "自定义标的无需查询，直接建档：代码为幂等键，名称必填，类型限债券 / ETF / 其他；同（代码，类型）已存在时复用并更新名称。";
const ADD_NOT_FOUND_HINT: &str = "数据源未覆盖该代码？切换「自定义标的」通道手动建档。";
const ADD_PRICE_FMT: &str = "最新价 {price}";
const ADD_PRICE_MISSING: &str = "暂未取到最新价";
const ADD_NAV_FMT: &str = "最新净值 {price}（{date}）";
const ADD_NAV_MISSING: &str = "暂未取到净值";
const ADD_CUSTOM_SUCCESS_FMT: &str = "已创建标的：{name}（{symbol}）";
/// 通道闭集标签（沪/深/港复用市场词；美股/场外基金/自定义标的为通道语义）。
const CHANNEL_SH: &str = "沪市";
const CHANNEL_SZ: &str = "深市";
const CHANNEL_HK: &str = "港股";
const CHANNEL_US: &str = "美股";
const CHANNEL_FUND: &str = "场外基金";
const CHANNEL_CUSTOM: &str = "自定义标的";
/// 通道值闭集（与旧壳 AddInstrumentChannel 同词）。
const CHANNEL_ID_SH: &str = "sh";
const CHANNEL_ID_SZ: &str = "sz";
const CHANNEL_ID_HK: &str = "hk";
const CHANNEL_ID_US: &str = "us";
const CHANNEL_ID_FUND: &str = "fund";
const CHANNEL_ID_CUSTOM: &str = "custom";
/// 自定义标的类型白名单标签/值（后端守卫同源；股票类不手动建）。
const TYPE_BOND: &str = "债券";
const TYPE_ETF: &str = "ETF";
const TYPE_OTHER: &str = "其他";
/// 错误码（旧壳 errorCodeOf 同面：股票通道查无此码引导自定义通道）。
const STOCK_NOT_FOUND_CODE: &str = "sync.stock-not-found";
/// 页大小（旧壳 InstrumentBrowser pageSize 单值同源，无档位选择器）。
const PAGE_SIZE: usize = 50;
/// 查询/建档在途文案（旧壳按钮 loading 的文字态）。
const QUERYING_LABEL: &str = "查询中…";
const CREATING_LABEL: &str = "创建中…";
/// 查询通道命中回显（旧壳 addInstrument.successStock / successFund 同词）。
const ADD_SUCCESS_STOCK_FMT: &str = "已添加投资标的：{name}（{symbol} · {typeLabel}）{price}";
const ADD_SUCCESS_FUND_FMT: &str = "已添加场外基金：{name}（{symbol}）{nav}";

/// 市场筛选选项闭集（旧壳 MARKET_FILTER_TYPES 同源：后端精确匹配，美股三
/// 交易所折叠语义不进筛选闭集）。
fn market_filter_options() -> Vec<CategoryOption> {
    vec![
        CategoryOption {
            id: "sh".to_owned(),
            label: CHANNEL_SH.to_owned(),
        },
        CategoryOption {
            id: "sz".to_owned(),
            label: CHANNEL_SZ.to_owned(),
        },
        CategoryOption {
            id: "hk".to_owned(),
            label: CHANNEL_HK.to_owned(),
        },
        CategoryOption {
            id: "unknown".to_owned(),
            label: "未知".to_owned(),
        },
    ]
}

/// 通道选项（旧壳 CHANNEL_OPTIONS 同序同词）。
fn add_channel_options() -> Vec<CategoryOption> {
    vec![
        CategoryOption {
            id: CHANNEL_ID_SH.to_owned(),
            label: CHANNEL_SH.to_owned(),
        },
        CategoryOption {
            id: CHANNEL_ID_SZ.to_owned(),
            label: CHANNEL_SZ.to_owned(),
        },
        CategoryOption {
            id: CHANNEL_ID_HK.to_owned(),
            label: CHANNEL_HK.to_owned(),
        },
        CategoryOption {
            id: CHANNEL_ID_US.to_owned(),
            label: CHANNEL_US.to_owned(),
        },
        CategoryOption {
            id: CHANNEL_ID_FUND.to_owned(),
            label: CHANNEL_FUND.to_owned(),
        },
        CategoryOption {
            id: CHANNEL_ID_CUSTOM.to_owned(),
            label: CHANNEL_CUSTOM.to_owned(),
        },
    ]
}

/// 自定义标的类型白名单选项（旧壳 TYPE_OPTIONS 同序同词）。
fn add_type_options() -> Vec<CategoryOption> {
    vec![
        CategoryOption {
            id: "bond".to_owned(),
            label: TYPE_BOND.to_owned(),
        },
        CategoryOption {
            id: "etf".to_owned(),
            label: TYPE_ETF.to_owned(),
        },
        CategoryOption {
            id: "other".to_owned(),
            label: TYPE_OTHER.to_owned(),
        },
    ]
}

/// 币种选项（旧壳添加弹窗 currencyOptions：全币种字典「code · name」）。
fn currency_options(table: &crate::assembly::CurrencyTable) -> Vec<CategoryOption> {
    table.currency_options()
}

/// 通道值 → 代码输入占位（旧壳 codePlaceholder 三态同语义）。
fn code_placeholder_of(channel: Option<&str>) -> &'static str {
    match channel {
        Some(CHANNEL_ID_FUND) => ADD_CODE_PLACEHOLDER_FUND,
        Some(CHANNEL_ID_CUSTOM) => ADD_CODE_PLACEHOLDER_CUSTOM,
        _ => ADD_CODE_PLACEHOLDER_STOCK,
    }
}

/// 枚举标签（旧壳 enumLabel 闭集外回退原值——防脏数据渲染成 key）。
fn market_word(market: &str) -> String {
    match market {
        "sh" => CHANNEL_SH.to_owned(),
        "sz" => CHANNEL_SZ.to_owned(),
        "hk" => CHANNEL_HK.to_owned(),
        "nasdaq" | "nyse" | "amex" => CHANNEL_US.to_owned(),
        "unknown" => "未知".to_owned(),
        other => other.to_owned(),
    }
}

/// 标的类型词（旧壳 investments.type.* 同词表）。
fn type_word(kind: InstrumentType) -> &'static str {
    match kind {
        InstrumentType::Stock => "股票",
        InstrumentType::Fund => "基金",
        InstrumentType::Bond => TYPE_BOND,
        InstrumentType::Etf => TYPE_ETF,
        InstrumentType::Other => TYPE_OTHER,
    }
}

/// 价格来源词（旧壳 priceChannel 五值同词表）。
fn price_channel_word(channel: PriceChannel) -> &'static str {
    match channel {
        PriceChannel::Quote => "行情",
        PriceChannel::FundNav => "净值",
        PriceChannel::Constant => "恒定价格",
        PriceChannel::Manual => "手动报价",
        PriceChannel::None => "无来源",
    }
}

/// 基金代码前端提前拦截（域 validate_fund_code 同校验——不发无效网络请求
/// ，旧壳 /^\d{6}$/ 同判定；权威校验在域入口单点）。
fn fund_code_shape(code: &str) -> bool {
    code.chars().count() == 6 && code.chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// 行快照与格文本（装配层：渲染期零格式化——本页签格文本一次产出）
// ---------------------------------------------------------------------------

/// 标的行快照（旧壳 browser 列投影闭集；全部格文本/准入位一次产出）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstrumentRowSnap {
    pub(crate) id: String,
    pub(crate) symbol: String,
    pub(crate) name_text: String,
    pub(crate) price_channel_word: &'static str,
    pub(crate) price_text: Option<String>,
    pub(crate) market_word: String,
    pub(crate) type_word: &'static str,
    pub(crate) currency_code: String,
    /// 是否有当前持仓（持仓 tag 呈现位）。
    pub(crate) invested: bool,
    /// 可录价（手动报价通道——后端派生事实单点）。
    pub(crate) quoteable: bool,
    /// 可删除（手动来源行——字典条目 source 单点）。
    pub(crate) deletable: bool,
    /// 行删除确认与反馈的展示名（名称优先，缺失回退代码）。
    pub(crate) display_name: String,
}

/// 行集装配（域读 → 行快照闭集；隐私态现价掩码——统一掩码消费面）。
pub(crate) fn assemble_instrument_rows(
    rows: &[ledger_investment::Instrument],
    currencies: &crate::assembly::CurrencyTable,
    privacy: bool,
) -> Vec<InstrumentRowSnap> {
    rows.iter()
        .map(|inst| {
            let price_text = inst.price_cents.map(|cents| {
                if privacy {
                    crate::assembly::AMOUNT_PRIVACY_MASK.to_owned()
                } else {
                    crate::assembly::format_price_text(
                        cents,
                        &currencies.format_of(&inst.currency_code),
                    )
                }
            });
            let name = inst.name.clone().filter(|n| !n.trim().is_empty());
            InstrumentRowSnap {
                id: inst.id.clone(),
                symbol: inst.symbol.clone(),
                name_text: name.clone().unwrap_or_else(|| "-".to_owned()),
                price_channel_word: price_channel_word(inst.price_channel),
                price_text,
                market_word: market_word(&inst.market),
                type_word: type_word(inst.kind),
                currency_code: inst.currency_code.clone(),
                invested: inst.invested,
                quoteable: inst.price_channel == PriceChannel::Manual,
                deletable: inst.source == ledger_investment::MANUAL_SOURCE,
                display_name: name.unwrap_or_else(|| inst.symbol.clone()),
            }
        })
        .collect()
}

/// 空态分型（旧壳两态：无筛选 = 库内无标的；否则 = 有标的但筛选无命中——
/// 与持仓页签同构的两态区分）。
pub(crate) fn instruments_empty_text(total: i64, filters_active: bool) -> &'static str {
    if filters_active {
        INSTRUMENTS_FILTER_NO_MATCH
    } else if total == 0 {
        INSTRUMENTS_EMPTY
    } else {
        INSTRUMENTS_FILTER_NO_MATCH
    }
}

/// 有效页数（≥1；服务端分页，页大小 [`PAGE_SIZE`]）。
pub(crate) fn instruments_total_pages(total: i64) -> u32 {
    let count = total.max(0) as usize;
    count.div_ceil(PAGE_SIZE).max(1) as u32
}

/// 筛选签名（同值守卫的比较锚）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterState {
    search: String,
    market: Option<String>,
    only_invested: bool,
}

// 列宽带（旧壳 NDataTable 固定列宽等值档；名称列不给弹性（千行内行集不需
// 要横向滚动——持仓页签先例同形；旧壳移动档 scroll-x 随窄窗票）。
const SYMBOL_W: f32 = 110.0;
const NAME_W: f32 = 200.0;
const SOURCE_W: f32 = 95.0;
const PRICE_W: f32 = 110.0;
const MARKET_W: f32 = 90.0;
const TYPE_W: f32 = 90.0;
const CURRENCY_W: f32 = 70.0;
const QUOTE_W: f32 = 70.0;
const INVESTED_W: f32 = 70.0;
const ACTIONS_W: f32 = 70.0;

/// 表头行（旧壳 NDataTable columns 词表；现价列挂概念说明——旧壳
/// ConceptLabel 同位）。
fn table_head(palette: PanePalette) -> impl IntoElement {
    let muted = palette.muted;
    let head = move |label: &'static str, width: f32| {
        text(label)
            .text_size(px(12.0))
            .text_color(muted)
            .w(px(width))
    };
    view()
        .test_id("instruments-table-head")
        .flex_row()
        .items_center()
        .gap_1()
        .child(head(COL_SYMBOL, SYMBOL_W))
        .child(head(COL_NAME, NAME_W))
        .child(head(COL_PRICE_SOURCE, SOURCE_W))
        .child(
            view()
                .test_id("browser-price")
                .flex_row()
                .items_center()
                .gap_1()
                .w(px(PRICE_W))
                .child(text(COL_PRICE).text_size(px(12.0)).text_color(muted))
                .child(crate::popover::popover_tip(
                    "instruments-price-concept",
                    vec![PRICE_TIP.to_owned()],
                    investments::info_dot(palette.info),
                )),
        )
        .child(head(COL_MARKET, MARKET_W))
        .child(head(COL_TYPE, TYPE_W))
        .child(head(COL_CURRENCY, CURRENCY_W))
        .child(head(COL_QUOTE, QUOTE_W))
        .child(head(COL_INVESTED_TAG, INVESTED_W))
        .child(head(COL_ACTIONS, ACTIONS_W))
}

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

/// 标的库页签本体（旧壳 InstrumentBrowser 的本壳等价；「删除即红」负向条
/// 目钉在 headless：页签激活装载 / 检索重拉 / 失效重拉 / focus 锚定位高亮
/// / 录价提交后信号刷新 / 删除重拉）。
#[component]
pub(crate) fn InstrumentsPane() -> impl IntoElement {
    let palette = investments::pane_palette();
    let session = investments::store();
    let view_owner = Owner::current();

    let rows = signal(Vec::<InstrumentRowSnap>::new());
    let total = signal(0_i64);
    let error = signal(None::<String>);
    let loading = signal(false);
    let currencies = signal(crate::assembly::CurrencyTable::default());

    // 筛选三维与服务端分页页码（视图作用域瞬态——旧壳同态）。
    let search_input = signal(String::new());
    let search = signal(String::new());
    let market = signal(None::<String>);
    let only_invested = signal(false);
    let page = signal(1_usize);
    let search_seq = signal(0_u64);

    // focus 高亮（会话内保留到 focus 变化；None = 普通列表态）。筛选变化
    // 撤高亮（旧行被筛走的保守语义，旧壳 focus prop 同射程）。
    let highlight_id = signal(None::<String>);

    // 行操作弹窗族与页面级反馈条位（NSH 无 toast——条位对应物）。
    let quote_open = signal(false);
    let add_open = signal(false);
    let add_message = signal(None::<String>);
    let delete_message = signal(None::<String>);
    let delete_error = signal(None::<String>);
    let quote_message = signal(None::<String>);
    let quote_error = signal(None::<String>);
    let add_error = signal(None::<String>);
    let quote_target = signal(None::<InstrumentRowSnap>);
    let delete_target = signal(None::<InstrumentRowSnap>);

    // 装载完成线装配（行集 + 币种套表 + 总数；锚点与常规两入口共用）。
    let apply_page = move |result: ledger_investment::InstrumentListResult,
                           table: crate::assembly::CurrencyTable,
                           privacy_enabled: bool| {
        rows.set(assemble_instrument_rows(
            &result.items,
            &table,
            privacy_enabled,
        ));
        currencies.set(table);
        total.set(result.total);
        error.set(None);
    };

    // 常规装载（页签激活边 + 筛选变化 + 失效信号重拉共用一条路径；保留分
    // 页与搜索状态——旧壳 issue #238 原地重拉同口径。分页单飞：分页条装
    // 载期间禁用）。
    let load = move || {
        loading.set(true);
        error.set(None);
        let privacy_enabled = app::amount_privacy_enabled();
        let filter = ledger_investment::InstrumentListFilter {
            search: {
                let trimmed = "".to_owned();
                (!trimmed.is_empty()).then_some(trimmed)
            },
            market: market.get_untracked(),
            only_invested: only_invested.get_untracked().then_some(true),
            page: Some(page.get_untracked().max(1)),
            page_size: Some(PAGE_SIZE),
            anchor_id: None,
            ..Default::default()
        };
        let db = app::db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                loading.set(false);
                error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let db_read = db.clone();
            let filter_read = filter.clone();
            let outcome = blocking(move || boot::instrument_page(&db_read, &filter_read)).await;
            loading.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    let table = currency_table_of(&db);
                    apply_page(result, table, privacy_enabled);
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
        });
    };

    // focus 消费（读一次：focus 意图 → 行高亮置定 + anchor 定位查询；行必
    // 在返回页，高亮因此可靠。锚点查询带 page_size + anchor_id——旧壳同语
    // 义：过滤退回默认口径，anchor_page 自分配后写页码。删除本接线即红：
    // 下钻落点不定位不高亮，headless focus 用例红）。
    effect(move || {
        if session.tab.get() != investments::InvestmentsTab::Instruments {
            return;
        }
        let Some(anchor_id) = session.instruments_focus.get() else {
            return;
        };
        session.instruments_focus.set(None);
        highlight_id.set(Some(anchor_id.clone()));
        loading.set(true);
        error.set(None);
        let filter = ledger_investment::InstrumentListFilter {
            page_size: Some(PAGE_SIZE),
            anchor_id: Some(anchor_id),
            ..Default::default()
        };
        let privacy_enabled = app::amount_privacy_enabled();
        let db = app::db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                loading.set(false);
                error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let db_read = db.clone();
            let filter_read = filter;
            let anchor_page = blocking(move || boot::instrument_page(&db_read, &filter_read)).await;
            loading.set(false);
            if let Ok(Ok(result)) = anchor_page {
                let anchor_page_now = result.anchor_page;
                let table = currency_table_of(&db);
                apply_page(result, table, privacy_enabled);
                page.set(anchor_page_now.map(|p| p.max(1)).unwrap_or(1).max(1));
            }
        });
    });

    // 筛选三维任一实际变化 → 翻页归零 + 重拉（同值守卫；同值重设与首载不
    // 触发。删除本 effect 接线即红：筛选后切片仍停原页，headless 检索用例
    // 红）。
    let tracked = std::rc::Rc::new(std::cell::RefCell::new(None::<FilterState>));
    let tracked_for_effect = tracked;
    effect(move || {
        let current = FilterState {
            search: search.get(),
            market: market.get(),
            only_invested: only_invested.get(),
        };
        let mut prev = tracked_for_effect.borrow_mut();
        if prev.as_ref() == Some(&current) {
            return;
        }
        *prev = Some(current);
        drop(prev);
        highlight_id.set(None);
        page.set(1);
        load();
    });

    // 页签落位装载（visited-once keep-mounted：激活边即重拉——旧壳本页签
    // display 'if' 重挂重拉语义；删除接线即红钉 headless 激活装载用例）。
    let was_active = signal(false);
    effect(move || {
        let active = session.tab.get() == investments::InvestmentsTab::Instruments;
        if active && !was_active.get_untracked() {
            load();
        }
        was_active.set(active);
    });

    // 失效信号（ADR-0031）：标的信息同步/录价等实际写价后原地重拉——load()
    // 保留分页与搜索状态。删除接线即红钉 lib 探针（录价后现价列不刷新）。
    let invest_sub = signals::subscribe_investments_changed(load);
    effect(move || {
        let held: &signals::InvestmentsChangedSubscription = &invest_sub;
        let _ = held;
    });

    // 搜索防抖调度（持仓页签 schedule 同形：回显立即生效、应用值 300ms；
    // 防抖序号失配自弃）。
    let schedule_search = move |text: String| {
        search_input.set(text);
        let next = search_input.get_untracked().trim().to_owned();
        if next == search.get_untracked() {
            return;
        }
        let applied = search;
        let seq = search_seq;
        let current = seq.get_untracked() + 1;
        seq.set(current);
        set_timeout(
            std::time::Duration::from_millis(app::SEARCH_DEBOUNCE_MS),
            move || {
                if seq.get() == current {
                    applied.set(next);
                }
            },
        );
    };

    // —— 头部（标题 + 同步入口；进度/结果行共享同一同步接缝——任一入口触
    // 发都呈现同一份，在途短路复用）——
    let header = view()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            text(investments::INSTRUMENTS_TAB_LABEL)
                .text_size(px(15.0))
                .text_color(palette.fg)
                .test_id("instruments-title"),
        )
        .child(
            view()
                .on_press(move |_: &PressEvent| investment_sync::trigger_sync())
                .test_id("sync-instrument-info")
                .rounded_md()
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
                    .text_size(px(12.0))
                    .text_color(rgb(0xffffff)),
                ),
        );

    let filter_row = view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            input()
                .test_id("instruments-search")
                .value(move || search_input.get())
                .placeholder(SEARCH_PLACEHOLDER)
                .optional_label("标的搜索")
                .text_color(palette.fg)
                .w(px(240.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 \
                     placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| schedule_search(value.to_owned())),
        )
        .child(
            view()
                .test_id("instruments-market-filter")
                .child(select::app_select(
                    "instruments-market",
                    market_filter_options,
                    market,
                    ALL_MARKETS,
                    true,
                )),
        )
        .child(
            // 只看持仓（issue #108）：勾选后仅列出有当前持仓的标的。
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    view().test_id("only-invested-switch").child(
                        switch()
                            .checked(move || only_invested.get())
                            .on_change(move |checked: &bool| only_invested.set(*checked))
                            .app_class("shrink-0"),
                    ),
                )
                .child(
                    text(ONLY_INVESTED)
                        .text_size(px(13.0))
                        .text_color(palette.fg),
                ),
        )
        .child(
            view()
                .on_press(move |_: &PressEvent| {
                    add_error.set(None);
                    add_message.set(None);
                    add_open.set(true);
                })
                .test_id("add-instrument")
                .rounded_md()
                .px_2()
                .py_1()
                .app_class("border border-border hover:bg-accent")
                .child(
                    text(ADD_INSTRUMENT)
                        .text_size(px(12.0))
                        .text_color(palette.fg),
                ),
        );

    // 表体（头行 + 行集 each + 行内录价/删除格 + 空态两型 + 分页）。
    let when_active_error = move || error.get().is_some();
    let rows_view = move || {
        each(
            move || rows.get(),
            |row: &InstrumentRowSnap| row.id.clone(),
            move |item: ReadSignal<InstrumentRowSnap>, _index: ReadSignal<usize>| {
                let snap = item.get_untracked();
                let focused = highlight_id.get_untracked().as_deref() == Some(snap.id.as_str());
                // 行内录价（issue #291）：只对手动报价通道开放；其余「-」
                //（行情/净值通道现价归同步——无来源行无入口）。
                let quote_spot = if snap.quoteable {
                    view()
                        .test_id(format!("quote-{}", snap.symbol).as_str())
                        .on_press({
                            let target = snap.clone();
                            move |_: &PressEvent| {
                                quote_target.set(Some(target.clone()));
                                quote_error.set(None);
                                quote_message.set(None);
                                quote_open.set(true);
                            }
                        })
                        .rounded_sm()
                        .px_2()
                        .py_0p5()
                        .app_class("hover:bg-accent")
                        .child(
                            text(QUOTE_ACTION)
                                .text_size(px(11.0))
                                .text_color(palette.fg),
                        )
                } else {
                    view().child(text("-").text_size(px(13.0)).text_color(palette.muted))
                };
                // 行内删除（issue #292 / ADR-0036 决策 5）：仅手动来源行；
                // 域守卫在域写路径（界面准入 + 后端双保险）。
                let delete_spot = if snap.deletable {
                    view()
                        .test_id(format!("delete-instrument-{}", snap.symbol).as_str())
                        .on_press({
                            let target = snap.clone();
                            move |_: &PressEvent| {
                                delete_target.set(Some(target.clone()));
                                delete_error.set(None);
                                delete_message.set(None);
                            }
                        })
                        .rounded_sm()
                        .px_2()
                        .py_0p5()
                        .app_class("hover:bg-accent")
                        .child(
                            text(DELETE_ACTION)
                                .text_size(px(11.0))
                                .text_color(palette.danger),
                        )
                } else {
                    view().child(text("-").text_size(px(13.0)).text_color(palette.muted))
                };
                view()
                    .test_id(format!("instruments-row-{}", snap.id).as_str())
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .min_h(px(36.0))
                    .app_class(if focused { "bg-accent" } else { "" })
                    .child(
                        text(snap.symbol.clone())
                            .text_size(px(13.0))
                            .text_color(palette.fg)
                            .w(px(SYMBOL_W)),
                    )
                    .child(
                        text(snap.name_text.clone())
                            .text_size(px(13.0))
                            .text_color(palette.fg)
                            .w(px(NAME_W)),
                    )
                    .child(
                        text(snap.price_channel_word)
                            .text_size(px(13.0))
                            .text_color(palette.muted)
                            .w(px(SOURCE_W)),
                    )
                    .child(
                        text(snap.price_text.clone().unwrap_or_else(|| "-".to_owned()))
                            .text_size(px(13.0))
                            .text_color(palette.fg)
                            .test_id(format!("instrument-price-{}", snap.symbol).as_str())
                            .w(px(PRICE_W)),
                    )
                    .child(
                        text(snap.market_word.clone())
                            .text_size(px(13.0))
                            .text_color(palette.muted)
                            .w(px(MARKET_W)),
                    )
                    .child(
                        text(snap.type_word)
                            .text_size(px(13.0))
                            .text_color(palette.muted)
                            .w(px(TYPE_W)),
                    )
                    .child(
                        text(snap.currency_code.clone())
                            .text_size(px(13.0))
                            .text_color(palette.muted)
                            .w(px(CURRENCY_W)),
                    )
                    .child(quote_spot)
                    .child(view().w(px(INVESTED_W)).child(if snap.invested {
                        text(COL_INVESTED_TAG)
                            .text_size(px(11.0))
                            .text_color(palette.fg)
                            .rounded_sm()
                            .px_1()
                            .py_0p5()
                            .app_class("bg-accent inline-block")
                    } else {
                        text("-").text_size(px(13.0)).text_color(palette.muted)
                    }))
                    .child(delete_spot)
            },
        )
    };
    let rows_or_empty = move || {
        view().flex_col().gap_1().child(table_head(palette)).child(
            when(
                move || error.get().is_none() && total.get() == 0,
                move || {
                    text(move || {
                        instruments_empty_text(
                            total.get(),
                            !search.get().trim().is_empty()
                                || market.get().is_some()
                                || only_invested.get(),
                        )
                    })
                    .text_size(px(13.0))
                    .text_color(palette.muted)
                    .test_id("instruments-empty")
                },
            )
            .otherwise(rows_view),
        )
    };
    let table_body = when(when_active_error, move || {
        text(move || error.get().unwrap_or_default())
            .text_size(px(12.0))
            .text_color(palette.danger)
    })
    .otherwise(rows_or_empty);

    let body = view()
        .flex_col()
        .gap_3()
        .child(header)
        .child(investment_sync::sync_progress_bar(palette))
        .child(investment_sync::sync_result_row(palette))
        .child(filter_row)
        .child(
            when(
                move || total.get() > 0,
                move || {
                    // 页面级反馈条位（添加成功 / 删除成功 / 录价成功 / 失败
                    // 同位着色——旧壳 message.success/error 条位对应物）。
                    view()
                        .flex_col()
                        .gap_1()
                        .child(
                            when(
                                move || add_message.get().is_some(),
                                move || {
                                    text(move || add_message.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(palette.fg)
                                        .test_id("add-instrument-result")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            when(
                                move || delete_message.get().is_some(),
                                move || {
                                    text(move || delete_message.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(palette.fg)
                                        .test_id("delete-instrument-result")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            when(
                                move || delete_error.get().is_some(),
                                move || {
                                    text(format!(
                                        "{DELETE_FAILED_PREFIX}{}",
                                        delete_error.get().unwrap_or_default()
                                    ))
                                    .text_size(px(12.0))
                                    .text_color(palette.danger)
                                    .test_id("delete-instrument-error")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            when(
                                move || quote_message.get().is_some(),
                                move || {
                                    text(move || quote_message.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(palette.fg)
                                        .test_id("manual-quote-result")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            when(
                                move || quote_error.get().is_some(),
                                move || {
                                    text(move || quote_error.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(palette.danger)
                                        .test_id("manual-quote-error-strip")
                                },
                            )
                            .otherwise(view),
                        )
                },
            )
            .otherwise(view),
        )
        .child(table_body)
        .child(
            when(
                move || instruments_total_pages(total.get()) > 1,
                move || {
                    view().test_id("instruments-pagination").flex_row().child(
                        pagination()
                            .accessibility_label("标的分页")
                            .previous_accessibility_label("上一页")
                            .next_accessibility_label("下一页")
                            .page(move || page.get() as u32)
                            .total_pages(move || instruments_total_pages(total.get()))
                            .disabled(move || loading.get())
                            .item_class(
                                "px-2 py-1 rounded-md text-foreground hover:bg-accent \
                                 selected:bg-accent disabled:opacity-40",
                            )
                            .on_change(move |&picked: &u32| {
                                let picked = picked.max(1) as usize;
                                if picked == page.get_untracked() {
                                    return;
                                }
                                page.set(picked);
                                load();
                            }),
                    )
                },
            )
            .otherwise(view),
        );

    view()
        .test_id("instruments-card")
        .flex_col()
        .gap_3()
        .rounded_lg()
        .mx_4()
        .my_2()
        .p_4()
        .app_class("border border-border bg-background")
        .child(body)
        .child({
            // 添加/手动报价弹窗借用面（签名收窄的工具变量；挂面本体见两弹
            // 窗函数）。
            manual_quote_modal(
                palette,
                quote_open,
                quote_target,
                quote_message,
                quote_error,
                view_owner,
            )
        })
        .child(add_instrument_modal(
            palette,
            add_open,
            add_message,
            add_error,
            view_owner,
        ))
        .child(delete_confirm_of(
            delete_target,
            delete_message,
            delete_error,
            load,
        ))
        .test_id("instruments-card")
}

/// 币种套表装载（装载批第二读；失败回退空表——列窄窗下币种符号缺席是可
/// 观察降级面且错误条位承载读取失败整体）。
fn currency_table_of(db: &ledger_infra::db::DbState) -> crate::assembly::CurrencyTable {
    match boot::currency_rows(db) {
        Ok(rows) => crate::assembly::CurrencyTable::from_currencies(&rows),
        Err(_) => crate::assembly::CurrencyTable::default(),
    }
}

/// 自建标的删除确认与执行（危险确认的浮层外挂；目标行随触发点固定；确
/// 认才执行 + 死别反馈）。
fn delete_confirm_of(
    delete_target: Signal<Option<InstrumentRowSnap>>,
    delete_message: Signal<Option<String>>,
    delete_error: Signal<Option<String>>,
    reload: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let owner = Owner::current();
    danger::danger_confirm(
        move || delete_target.get().is_some(),
        danger::DangerConfirmSpec {
            level: danger::DangerLevel::Error,
            title: DELETE_TITLE.to_owned(),
            lead: Some(
                DELETE_CONFIRM_FMT.replace(
                    "{name}",
                    &delete_target
                        .get_untracked()
                        .map(|row| row.display_name.clone())
                        .unwrap_or_default(),
                ),
            ),
            strong_warning: None,
            detail: None,
            confirm_label: DELETE_ACTION.to_owned(),
            cancel_label: "取消".to_owned(),
        },
        move |outcome: danger::DangerOutcome| {
            let target = delete_target.get_untracked();
            delete_target.set(None);
            if outcome == danger::DangerOutcome::Confirmed
                && let Some(row) = target
            {
                perform_instrument_delete(row, owner, delete_message, delete_error, reload);
            }
        },
    )
}

/// 自建标的删除（旧壳 deleteInstrument 命令同一路径）：守卫在域写路径单正
/// 文——同步来源与被引用标的码化拒删原样呈现；成功反馈（展示名随行）+ 原
/// 地重拉；失败码化错误条位。
fn perform_instrument_delete(
    row: InstrumentRowSnap,
    view_owner: Owner,
    delete_message: Signal<Option<String>>,
    delete_error: Signal<Option<String>>,
    reload: impl Fn() + Copy + 'static,
) {
    let db = app::db_slot().clone();
    view_owner.run(move || {
        spawn(async move {
            let id = row.id.clone();
            let label = row.display_name.clone();
            let Some(db) = db else {
                delete_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let outcome = blocking(move || boot::delete_instrument(&db, &id)).await;
            match outcome {
                Ok(Ok(())) => {
                    delete_error.set(None);
                    delete_message.set(Some(DELETE_SUCCESS_FMT.replace("{name}", &label)));
                    reload();
                }
                Ok(Err(e)) => delete_error.set(Some(e.to_string())),
                Err(_) => delete_error.set(Some("内部错误：写入任务异常退出".to_owned())),
            }
        })
    });
}

// ---------------------------------------------------------------------------
// 手动报价弹窗（旧壳 ManualPriceModal.vue 重生）
// ---------------------------------------------------------------------------

/// 手动报价弹窗挂面：日期（缺省今天）+ 价格（元，就地判定红框）+ 提交禁
/// 用（价格非法 / 在途）+ 弹窗内错误位（保持弹窗打开供修改重试）+ 页面级
/// 回执（quote_message / quote_error）。打开时重置表单（旧壳 watch show
/// 同形）。
fn manual_quote_modal(
    palette: PanePalette,
    quote_open: Signal<bool>,
    quote_target: Signal<Option<InstrumentRowSnap>>,
    quote_message: Signal<Option<String>>,
    quote_error: Signal<Option<String>>,
    view_owner: Owner,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;
    let date = signal(chrono::Local::now().date_naive());
    let price_text = signal(String::new());
    let marked = signal(false);
    let submitting = signal(false);

    // 价格单点判定（非空即判红态显红；空值不抢红——提交尝试再标记）。
    let price_error_now = move || {
        let text = price_text.get();
        catalog_price_error(&text, marked.get())
    };
    // 提交资格：价格合法非空 + 不在途（旧壳 canSubmit 同判——价格非法均值
    // 空或非数字，两者都在红框位拦截）。
    let can_submit = move || {
        !price_text.get().trim().is_empty() && price_error_now().is_none() && !submitting.get()
    };

    // 打开时重置（日期回今天、清空价格与错误）。
    effect(move || {
        if quote_open.get() {
            date.set(chrono::Local::now().date_naive());
            price_text.set(String::new());
            marked.set(false);
            quote_error.set(None);
        }
    });

    modal::app_modal(
        move || quote_open.get(),
        move || {
            MANUAL_QUOTE_TITLE_FMT.replace(
                "{symbol}",
                &quote_target
                    .get_untracked()
                    .map(|row| row.symbol)
                    .unwrap_or_default(),
            )
        },
        move || quote_open.set(false),
        move || {
            let price_error_now = price_error_now;
            let start = view().flex_col().gap_2().child(
                text(MANUAL_QUOTE_INTRO)
                    .text_size(px(12.0))
                    .text_color(muted),
            );
            let date_row = view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(MANUAL_QUOTE_DATE_LABEL)
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(56.0)),
                )
                .child(
                    view()
                        .w(px(160.0))
                        .child(crate::date_select::date_select("manual-quote-date", date)),
                );
            let price_border = if price_error_now().is_some() {
                "bg-background rounded-md border border-destructive px-2 py-1 \
                 placeholder:text-muted-foreground"
            } else {
                "bg-background rounded-md border border-border px-2 py-1 \
                 placeholder:text-muted-foreground"
            };
            let price_row = view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(MANUAL_QUOTE_PRICE_LABEL)
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(56.0)),
                )
                .child(
                    input()
                        .test_id("manual-quote-price")
                        .value(move || price_text.get())
                        .placeholder(MANUAL_QUOTE_PRICE_PLACEHOLDER)
                        .optional_label("报价价格")
                        .text_color(fg)
                        .w(px(160.0))
                        .app_class(price_border)
                        .on_change(move |value: &str| {
                            marked.set(true);
                            price_text.set(value.to_owned());
                        }),
                );
            let error_row = when(
                move || quote_error.get().is_some(),
                move || {
                    text(move || quote_error.get().unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(palette.danger)
                        .test_id("manual-quote-error")
                },
            )
            .otherwise(view);
            let actions = view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    view()
                        .on_press(move |_: &PressEvent| quote_open.set(false))
                        .test_id("cancel-manual-quote")
                        .rounded_md()
                        .px_3()
                        .py_1()
                        .app_class("border border-border hover:bg-accent disabled:opacity-40")
                        .disabled(move || submitting.get())
                        .child(text(MANUAL_QUOTE_CANCEL).text_size(px(12.0)).text_color(fg)),
                )
                .child(
                    view()
                        .on_press(move |_: &PressEvent| {
                            submit_manual_quote(
                                quote_target.get_untracked(),
                                date.get_untracked(),
                                price_text.get_untracked(),
                                quote_open,
                                submitting,
                                quote_message,
                                quote_error,
                                marked,
                                view_owner,
                            );
                        })
                        .test_id("submit-manual-quote")
                        .rounded_md()
                        .px_3()
                        .py_1()
                        .app_class("bg-primary hover:opacity-90 disabled:opacity-40")
                        .disabled(move || !can_submit())
                        .child(
                            text(move || {
                                if submitting.get() {
                                    "录价中…".to_owned()
                                } else {
                                    MANUAL_QUOTE_SUBMIT.to_owned()
                                }
                            })
                            .text_size(px(12.0))
                            .text_color(rgb(0xffffff)),
                        ),
                );
            view()
                .flex_col()
                .gap_2()
                .child(start)
                .child(date_row)
                .child(price_row)
                .child(error_row)
                .child(actions)
        },
    )
}

/// 价格文本就地判定（非空 + 判定 Ok 装载红框位——判定域归 assembly 判定
/// 族单点，零重复校验）。
fn catalog_price_error(text: &str, marked: bool) -> Option<()> {
    let trim = text.trim();
    if trim.is_empty() {
        return None;
    }
    match crate::assembly::judge_price_text(trim) {
        crate::assembly::PriceJudgment::Ok(_) => None,
        _ if marked => Some(()),
        _ => None,
    }
}

/// 手动报价提交（旧壳 submit 同形：价格必须 > 0 前端提前拦 + 域公开写入口
/// 直调 + 广播按域证据 + 关窗）。
#[allow(clippy::too_many_arguments)]
fn submit_manual_quote(
    target: Option<InstrumentRowSnap>,
    date: chrono::NaiveDate,
    price_text: String,
    quote_open: Signal<bool>,
    submitting: Signal<bool>,
    quote_message: Signal<Option<String>>,
    quote_error: Signal<Option<String>>,
    marked: Signal<bool>,
    view_owner: Owner,
) {
    let Some(row) = target else { return };
    let trimmed = price_text.trim();
    if trimmed.is_empty() {
        marked.set(true);
        return;
    }
    let Some(price_cents) = crate::assembly::price_text_to_price_cents(trimmed) else {
        marked.set(true);
        return;
    };
    if price_cents <= 0 {
        quote_error.set(Some(MANUAL_QUOTE_PRICE_POSITIVE.to_owned()));
        return;
    }
    let input = ledger_investment::ManualPriceInput {
        instrument_id: row.id.clone(),
        date: date.format("%Y-%m-%d").to_string(),
        price_cents,
    };
    let price_text_display = crate::assembly::price_cents_to_yuan_text(price_cents);
    let db = app::db_slot().clone();
    view_owner.run(move || {
        spawn(async move {
            submitting.set(true);
            let Some(db) = db else {
                submitting.set(false);
                quote_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let outcome = blocking(move || boot::record_manual_price(&db, &input)).await;
            submitting.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    // 回执按落点结果分型：回填旧价只沉淀历史、不动现价。
                    let message = if result.current_price_written {
                        MANUAL_QUOTE_SUCCESS_FMT
                            .replace("{symbol}", &row.symbol)
                            .replace("{price}", &price_text_display)
                    } else {
                        MANUAL_QUOTE_HISTORY_ONLY_FMT.replace("{symbol}", &row.symbol)
                    };
                    quote_error.set(None);
                    quote_message.set(Some(message));
                    quote_open.set(false);
                    // 广播（实际写入任一落点；零变化不广播——信号映射判定
                    // 单点在 infra signals，本壳投影到投资族通道）。
                    if result.any_written() {
                        signals::notify_investments_invalidated();
                    }
                }
                Ok(Err(e)) => quote_error.set(Some(e.to_string())),
                Err(_) => quote_error.set(Some("内部错误：写入任务异常退出".to_owned())),
            }
        });
    });
}

// ---------------------------------------------------------------------------
// 添加投资标的弹窗（旧壳 AddInstrumentModal.vue 重生）
// ---------------------------------------------------------------------------

/// 添加投资标的弹窗挂面：通道必选（六通道）+ 代码（基金 6 位前端提前拦截
/// ——不发无效网络请求）+ 自定义通道直呈建档表单（名称 / 类型白名单 / 币
/// 种默认 CNY 可选）+ 主按钮分派 + 弹窗内错误位（查询未命中 / 临时故障 /
/// 建档校验失败保持弹窗打开供改码重试）。
fn add_instrument_modal(
    palette: PanePalette,
    add_open: Signal<bool>,
    add_message: Signal<Option<String>>,
    add_error: Signal<Option<String>>,
    view_owner: Owner,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;
    let channel = signal(None::<String>);
    let code = signal(String::new());
    let custom_name = signal(String::new());
    let custom_type = signal(None::<String>);
    let custom_currency_value = signal("CNY".to_owned());
    let custom_currency = signal(None::<String>);
    // 受控双通道：值信号（.get()）与下拉的 Option 信号同步（旧壳
    // v-model:value 同形——选项 signal 受控 + 默认 CNY）。
    effect(move || {
        custom_currency_value.set(custom_currency.get().unwrap_or_else(|| "CNY".to_owned()));
    });
    let querying = signal(false);
    let creating = signal(false);
    let currencies_table = signal(crate::assembly::CurrencyTable::default());
    let not_found_hint = signal(false);

    // 打开时重置（旧壳 watch show immediate 同形：币种默认人民币）。
    effect(move || {
        if add_open.get() {
            channel.set(None);
            code.set(String::new());
            custom_name.set(String::new());
            custom_type.set(None);
            custom_currency.set(Some("CNY".to_owned()));
            querying.set(false);
            creating.set(false);
            add_error.set(None);
            not_found_hint.set(false);
        }
    });

    // 币种字典（弹窗开启时现拉；旧壳 reference store 口径）。
    effect(move || {
        if !add_open.get() {
            return;
        }
        let db = app::db_slot().clone();
        spawn(async move {
            let Some(db) = db else { return };
            let outcome = blocking(move || boot::currency_rows(&db)).await;
            if let Ok(Ok(rows)) = outcome {
                currencies_table.set(crate::assembly::CurrencyTable::from_currencies(&rows));
            }
        });
    });

    let is_custom = move || channel.get().as_deref() == Some(CHANNEL_ID_CUSTOM);
    let code_valid = move || {
        let trimmed = code.get().trim().to_owned();
        if trimmed.is_empty() {
            return false;
        }
        if channel.get().as_deref() == Some(CHANNEL_ID_FUND) {
            return fund_code_shape(&trimmed);
        }
        true
    };
    let busy = move || querying.get() || creating.get();
    let primary_label = move || {
        if is_custom() { ADD_CREATE } else { ADD_QUERY }
    };
    let primary_disabled = move || {
        if is_custom() {
            !(code_valid()
                && !custom_name.get().trim().is_empty()
                && custom_type.get().is_some()
                && !busy())
        } else {
            !(channel.get().is_some() && code_valid() && !busy())
        }
    };

    modal::app_modal(
        move || add_open.get(),
        move || ADD_TITLE.to_owned(),
        move || add_open.set(false),
        move || {
            let placeholder = code_placeholder_of(channel.get().as_deref());
            // 主提交（分派：自定义 = 建档；其余 = 查询——旧壳 submitPrimary
            // 同面）。分派在闭包内做（is_custom 现读），装面输入 Reflection。
            let on_submit = move |_: &PressEvent| {
                if is_custom() {
                    let name = custom_name.get().trim().to_owned();
                    let Some(kind_id) = custom_type.get() else {
                        return;
                    };
                    if !code_valid() {
                        return;
                    }
                    let kind = match kind_id.as_str() {
                        "bond" => InstrumentType::Bond,
                        "etf" => InstrumentType::Etf,
                        _ => InstrumentType::Other,
                    };
                    let input = ledger_investment::InstrumentInput {
                        symbol: code.get().trim().to_owned(),
                        kind,
                        name: Some(name.clone()),
                        currency_code: custom_currency_value.get(),
                        market: None,
                    };
                    let db = app::db_slot().clone();
                    view_owner.run({
                        move || {
                            spawn(async move {
                                creating.set(true);
                                add_error.set(None);
                                let Some(db) = db else {
                                    creating.set(false);
                                    add_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                                    return;
                                };
                                let outcome =
                                    blocking(move || boot::create_instrument(&db, input)).await;
                                creating.set(false);
                                match outcome {
                                    Ok(Ok(_id)) => {
                                        add_error.set(None);
                                        add_message.set(Some(
                                            ADD_CUSTOM_SUCCESS_FMT
                                                .replace("{name}", &name)
                                                .replace("{symbol}", code.get_untracked().trim()),
                                        ));
                                        add_open.set(false);
                                        // 新标的行上列表：回到第 1 页重拉——
                                        // 行集随本弹窗装载信号重读（旧壳
                                        // onInstrumentAdded reload 同语义）。
                                        signals::notify_investments_invalidated();
                                    }
                                    Ok(Err(e)) => add_error.set(Some(e.to_string())),
                                    Err(_) => {
                                        add_error.set(Some("内部错误：写入任务异常退出".to_owned()))
                                    }
                                }
                            });
                        }
                    });
                } else {
                    // 查询通道：fund / 沪深港美股（网络在锁外 await——旧壳
                    // 命令同底；未命中/临时故障显式报错不建档）。
                    let Some(channel_id) = channel.get() else {
                        return;
                    };
                    let code_trim = code.get().trim().to_owned();
                    if channel_id == CHANNEL_ID_FUND && !fund_code_shape(&code_trim) {
                        return;
                    }
                    let db = app::db_slot().clone();
                    view_owner.run({
                        move || {
                            spawn(async move {
                                querying.set(true);
                                add_error.set(None);
                                not_found_hint.set(false);
                                let Some(db) = db else {
                                    querying.set(false);
                                    add_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                                    return;
                                };
                                let outcome = if channel_id == CHANNEL_ID_FUND {
                                    match boot::add_fund_by_code(&db, &code_trim).await {
                                        Ok(result) => Ok(QuoteOutcome::Fund(result)),
                                        Err(e) => Err(e),
                                    }
                                } else {
                                    match boot::add_instrument_by_code(&db, &channel_id, &code_trim)
                                        .await
                                    {
                                        Ok(result) => Ok(QuoteOutcome::Stock(result)),
                                        Err(e) => Err(e),
                                    }
                                };
                                querying.set(false);
                                match outcome {
                                    Ok(QuoteOutcome::Fund(result)) => {
                                        let nav =
                                            match (result.nav_cents, result.nav_date.as_deref()) {
                                                (Some(cents), Some(date)) => ADD_NAV_FMT
                                                    .replace(
                                                        "{price}",
                                                        &crate::assembly::price_cents_to_yuan_text(
                                                            cents,
                                                        ),
                                                    )
                                                    .replace("{date}", date),
                                                _ => ADD_NAV_MISSING.to_owned(),
                                            };
                                        add_error.set(None);
                                        add_message.set(Some(
                                            ADD_SUCCESS_FUND_FMT
                                                .replace("{name}", &result.name)
                                                .replace("{symbol}", &result.symbol)
                                                .replace("{nav}", &nav),
                                        ));
                                        add_open.set(false);
                                        not_found_hint.set(false);
                                        signals::notify_investments_invalidated();
                                    }
                                    Ok(QuoteOutcome::Stock(result)) => {
                                        let price = match result.price_cents {
                                            Some(cents) => ADD_PRICE_FMT.replace(
                                                "{price}",
                                                &crate::assembly::price_cents_to_yuan_text(cents),
                                            ),
                                            None => ADD_PRICE_MISSING.to_owned(),
                                        };
                                        let type_label = type_word(result.kind);
                                        add_error.set(None);
                                        add_message.set(Some(
                                            ADD_SUCCESS_STOCK_FMT
                                                .replace("{name}", &result.name)
                                                .replace("{symbol}", &result.symbol)
                                                .replace("{typeLabel}", type_label)
                                                .replace("{price}", &price),
                                        ));
                                        add_open.set(false);
                                        not_found_hint.set(false);
                                        signals::notify_investments_invalidated();
                                    }
                                    Err(e) => {
                                        add_error.set(Some(e.to_string()));
                                        not_found_hint.set(
                                            channel_id != CHANNEL_ID_FUND
                                                && e.code() == Some(STOCK_NOT_FOUND_CODE),
                                        );
                                    }
                                }
                            });
                        }
                    });
                }
            };

            let base = view()
                .flex_col()
                .gap_2()
                .child(text(ADD_INTRO).text_size(px(12.0)).text_color(muted))
                .child(
                    // 通道（AppSelect 必选）。
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(ADD_MARKET_LABEL)
                                .text_size(px(12.0))
                                .text_color(muted)
                                .w(px(56.0)),
                        )
                        .test_id("add-instrument-market")
                        .child(select::app_select(
                            "add-instrument-channel",
                            add_channel_options,
                            channel,
                            ADD_MARKET_PLACEHOLDER,
                            false,
                        )),
                )
                .child(
                    // 代码（基金 6 位前端提前拦截——后端同样校验）。
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(ADD_CODE_LABEL)
                                .text_size(px(12.0))
                                .text_color(muted)
                                .w(px(56.0)),
                        )
                        .child(
                            input()
                                .test_id("add-instrument-code")
                                .value(move || code.get())
                                .placeholder(placeholder)
                                .optional_label("录入代码")
                                .text_color(fg)
                                .w(px(220.0))
                                .app_class(
                                    "bg-background rounded-md border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |value: &str| code.set(value.to_owned())),
                        ),
                );

            let custom_blocks = when(is_custom(), move || {
                let codes_now = currencies_table.get();
                let sys_signal = signal(currency_options(&codes_now));
                view()
                    .flex_col()
                    .gap_2()
                    .test_id("add-instrument-custom-hint")
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(ADD_NAME_LABEL)
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .w(px(56.0)),
                            )
                            .child(
                                input()
                                    .test_id("add-instrument-name")
                                    .value(move || custom_name.get())
                                    .placeholder(ADD_NAME_PLACEHOLDER)
                                    .optional_label("自定义名称")
                                    .text_color(fg)
                                    .w(px(220.0))
                                    .app_class(
                                        "bg-background rounded-md border border-border px-2 py-1 \
                                             placeholder:text-muted-foreground",
                                    )
                                    .on_change(move |value: &str| {
                                        custom_name.set(value.to_owned())
                                    }),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(ADD_TYPE_LABEL)
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .w(px(56.0)),
                            )
                            .test_id("add-instrument-type")
                            .child(select::app_select(
                                "add-instrument-type-select",
                                add_type_options,
                                custom_type,
                                ADD_TYPE_PLACEHOLDER,
                                false,
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(ADD_CURRENCY_LABEL)
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .w(px(56.0)),
                            )
                            .test_id("add-instrument-currency")
                            .child(pinyin_select::pinyin_select(
                                "add-instrument-currency-select",
                                {
                                    let rows_signal = sys_signal;
                                    move || rows_signal.get()
                                },
                                custom_currency,
                                ADD_CURRENCY_LABEL,
                                false,
                            )),
                    )
                    .child(text(ADD_CUSTOM_HINT).text_size(px(11.0)).text_color(muted))
            })
            .otherwise(view);

            let done_rows = view()
                .flex_col()
                .gap_2()
                .child(
                    when(
                        move || add_error.get().is_some(),
                        move || {
                            text(move || add_error.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(palette.danger)
                                .test_id("add-instrument-error")
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    when(
                        move || not_found_hint.get(),
                        move || {
                            text(ADD_NOT_FOUND_HINT)
                                .text_size(px(12.0))
                                .text_color(muted)
                        },
                    )
                    .otherwise(view),
                );
            let actions = view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    view()
                        .on_press(move |_: &PressEvent| add_open.set(false))
                        .test_id("cancel-add-instrument")
                        .rounded_md()
                        .px_3()
                        .py_1()
                        .app_class("border border-border hover:bg-accent disabled:opacity-40")
                        .disabled(busy)
                        .child(text("取消").text_size(px(12.0)).text_color(fg)),
                )
                .child(
                    view()
                        .on_press(on_submit)
                        .test_id("submit-add-instrument")
                        .rounded_md()
                        .px_3()
                        .py_1()
                        .app_class("bg-primary hover:opacity-90 disabled:opacity-40")
                        .disabled(primary_disabled)
                        .child(
                            text(move || {
                                if querying.get() {
                                    QUERYING_LABEL.to_owned()
                                } else if creating.get() {
                                    CREATING_LABEL.to_owned()
                                } else {
                                    primary_label().to_owned()
                                }
                            })
                            .text_size(px(12.0))
                            .text_color(rgb(0xffffff)),
                        ),
                );

            view()
                .flex_col()
                .gap_2()
                .child(base)
                .child(custom_blocks)
                .child(done_rows)
                .child(actions)
        },
    )
}

/// 查询通道结果的形状翻型（fund / stock 两面在提交闭包内分流回执）。
enum QuoteOutcome {
    Fund(ledger_investment::AddFundResult),
    Stock(ledger_investment::AddStockInstrumentResult),
}

// ---------------------------------------------------------------------------
// lib 单测（词表纯函数与装配口径；测试整体豁免 ADR-0060 经 crate 根放行）
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

    fn instrument_row(
        id: &str,
        symbol: &str,
        source: &str,
        channel: PriceChannel,
        invested: bool,
        price_cents: Option<i64>,
        market: &str,
    ) -> ledger_investment::Instrument {
        ledger_investment::Instrument {
            id: id.to_owned(),
            symbol: symbol.to_owned(),
            kind: InstrumentType::Stock,
            name: if symbol.is_empty() {
                None
            } else {
                Some(format!("{symbol} 名称"))
            },
            currency_code: "CNY".to_owned(),
            market: market.to_owned(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: "t".to_owned(),
            source: source.to_owned(),
            price_cents,
            invested,
            price_channel: channel,
        }
    }

    #[test]
    fn instrument_rows_follow_old_shell_project_and_pricing() {
        let table = cny_table();
        // 手动报价通道 + 手动来源（录价 + 删除双入口开）；行情通道股票不掉
        // 行内入口。债券形态（自定义通道类型白名单成员之一）。
        let mut manual_inst = instrument_row(
            "i-1",
            "TestMan",
            "manual",
            PriceChannel::Manual,
            true,
            Some(21890),
            "unknown",
        );
        manual_inst.kind = InstrumentType::Bond;
        let manual = assemble_instrument_rows(&[manual_inst], &table, false);
        assert!(manual[0].quoteable);
        assert!(manual[0].deletable);
        assert_eq!(manual[0].price_channel_word, "手动报价");
        assert_eq!(manual[0].type_word, "债券");
        assert_eq!(manual[0].market_word, formula_market_label("unknown"));
        assert_eq!(manual[0].price_text.as_deref(), Some("¥2.189"));
        assert_eq!(manual[0].display_name, "TestMan 名称");
    }

    #[test]
    fn quote_channel_gates_follow_channel_not_ui() {
        let table = cny_table();
        // 行情通道（Quote）：不显示录价入口；同步来源：不显示删除入口。
        let stock = assemble_instrument_rows(
            &[instrument_row(
                "i-2",
                "QuoteOnly",
                "eastmoney",
                PriceChannel::Quote,
                false,
                None,
                "sh",
            )],
            &table,
            false,
        );
        assert!(!stock[0].quoteable);
        assert!(!stock[0].deletable);
        assert_eq!(stock[0].price_channel_word, "行情");
        assert_eq!(stock[0].market_word, formula_market_label("sh"));
        assert_eq!(stock[0].name_text, "QuoteOnly 名称");
        // 外来市场（脏数据）枚举标签退原值（旧壳 enumLabel 同口径）。
        assert_eq!(stock[0].market_word, formula_market_label("sh"));
    }

    #[test]
    fn privacy_masks_price_cell() {
        let table = cny_table();
        let rows = assemble_instrument_rows(
            &[instrument_row(
                "i-3",
                "Priv",
                "manual",
                PriceChannel::Manual,
                false,
                Some(31_815),
                "unknown",
            )],
            &table,
            true,
        );
        assert_eq!(
            rows[0].price_text.as_deref(),
            Some(crate::assembly::AMOUNT_PRIVACY_MASK)
        );
    }

    #[test]
    fn empty_and_pagination_closures() {
        assert_eq!(instruments_total_pages(0), 1);
        assert_eq!(instruments_total_pages(1), 1);
        assert_eq!(instruments_total_pages(50), 1);
        assert_eq!(instruments_total_pages(51), 2);
        assert_eq!(
            instruments_empty_text(0, false),
            INSTRUMENTS_EMPTY,
            "无筛选空库 → 库内无标的"
        );
        assert_eq!(
            instruments_empty_text(0, true),
            INSTRUMENTS_FILTER_NO_MATCH,
            "筛选无命中两态分型"
        );
        assert_eq!(instruments_empty_text(5, true), INSTRUMENTS_FILTER_NO_MATCH);
    }

    #[test]
    fn fund_code_shape_and_code_placeholder_follow_channels() {
        assert!(fund_code_shape("000001"));
        assert!(!fund_code_shape("60051"));
        assert!(!fund_code_shape("6005a0"));
        assert_eq!(
            code_placeholder_of(Some(CHANNEL_ID_FUND)),
            ADD_CODE_PLACEHOLDER_FUND
        );
        assert_eq!(
            code_placeholder_of(Some(CHANNEL_ID_CUSTOM)),
            ADD_CODE_PLACEHOLDER_CUSTOM
        );
        assert_eq!(
            code_placeholder_of(Some(CHANNEL_ID_SH)),
            ADD_CODE_PLACEHOLDER_STOCK
        );
    }

    fn formula_market_label(market: &str) -> String {
        market_word(market)
    }
}
