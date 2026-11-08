//! 投资视图·明细页签（issue #2047 NSH 迁移⑯；spec #2031 系列 16/26，投资
//! 3/3；旧壳 `InvestmentLedgerTab.vue` 的重生）。
//!
//! **行读接缝**：服务端 offset 分页直调域读入口 [`boot::investment_transaction_page`]
//! （旧壳 `list_investment_transactions` 命令同一域函数，不经 IPC）——五种
//! 投资 kind（buy/sell/convert/split/dividend）交易行的投资投影按 kind 分
//! 形态渲染（装配快照：行/格预计算、渲染期零格式化），过滤维（类型子集 /
//! 账户涉及语义 / 标的深链）与页码住会话槽（[`crate::investments::store`]
//! 明细段，旧壳 investments-session detail 段同射程：会话内保留、冷启动回
//! 默认）。页大小固定 20（旧壳默认档同值；页大小档位选择器缺席——旧壳
//! `pageSizes` 档位带随主列表同源，缺件留痕随交付报告）。
//!
//! **筛选三维**：类型多选（[`crate::multi_select`] 弹层，选项闭集 = 五种投
//! 资 kind 与域行集闭包同源、维度内取或）、账户单选（涉及账户语义：账户端
//! ∪ 出资端——语义单点在域读路径；候选 = 可选择投资账户 + 选中归档补值
//! 「（已归档）」，spec #1952 同款）、清除筛选（#1807 主列表同款判定）；任
//! 一应用值实际变化 → 翻页归零 + 重拉（同值守卫 effect 内化，同值重设不触
//! 发）。标的维度无手动控件：仅下钻落点写入（旧壳 `?instrument=` 只读承
//! 载同语义）。日期维度缺席（旧壳 QuickTimeRange 芯片 + 期间直达面板，头
//! 件不在本壳就有——缺件留痕随交付报告）。
//!
//! **行操作弹窗族**（旧壳 ADR-0135 决策 4 / issue #1781 同权复用）：行右键
//! 菜单（编辑 buy/sell、只读详情 convert/split/dividend、删除）——编辑经
//! [`crate::investments_form`]（**先取明细再开窗**：弹窗行适配投影
//! [`investments_form::trade_row_tx`]），只读详情载荷 = 明细行投影自带
//!（扩展行随行带出，不再取数，「enum 闭集承载弹窗内分派」）；软删走既有域
//! 删 ([`boot`] delete_transaction 前例接缝——级联与码化守卫照常，不新增错
//! 误码），删除成功后列表重读（issue #893 同款空页自愈兜底）。「⋯」常显操
//! 作列缺席：NSH 迁移期只做宽屏主形态（spec #2031 窄窗横切票协同）——行
//! 右键菜单承担同一行操作闭集（桌面平台行可达，触控轴随窄窗票落地）。
//!
//! **创建入口落位**（#2046 偏差 1 的消费面）：页签头部「记买入/记卖出」
//!（旧壳 ADR-0135 决策 5，类型由入口单点表达）；[`investments_form::TradeFormState::open_create`]
//! 是 ⑮ 预建接缝的第一用户（弹窗复用同件、装配器零重迁）。
//!
//! **下钻落点**：持仓页签行「标的代码」链接（ADR-0135 决策 6 持仓下钻，本
//! 票已接 [`crate::investments_holdings`]）与骨架导航接缝
//!（[`crate::nav::NavIntent::investments_drill`] 载荷消费）——落本页签时筛
//! 选维覆盖（账户 + 标的深链维度，「参数在场永远赢、缺席维度保留保留态」
//! 旧壳同口径）并翻页归零；消费时点 = 落位（effect 键当前视图/激活页签，
//! [`crate::nav::take_intent`] 读一次封闸）。
//!
//! 金额/单价/费用列走统一展示格式化（[`crate::assembly`]，隐私掩码自动生
//! 效）；行投影不携带币种呈现（混合币种列表不暗示同币种，但单价格带标的
//! 币符号——价格列口径随标的字典，旧壳 formatPrice?ccy 同语义按注册表出
//! 现符号）。渲染期零格式化纪律——本页签全部格文本一次性产出进快照。
//! 界面文案仅 zh-CN（旧壳 investments.json ledger 段 + transactions 复用词
//! 表逐条平价；字面闭集承载）。码化错误 zh 原样呈现（错误条位，不给半截数
//! 字）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::DataTableRows;
use nsh_base::{
    DataTableAlign, DataTableCell, DataTableClasses, DataTableColumn, DataTableContextMenu,
    data_table, pagination,
};
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;

use crate::assembly::CategoryOption;
use crate::investments::PanePalette;
use crate::{
    app, assembly, boot, context_menu, danger, investments, investments_form, modal, multi_select,
    pinyin_select, signals,
};
use ledger_transaction::TransactionKind;

// ---------------------------------------------------------------------------
// 词表（旧壳 investments.json ledger 段 + transactions 复用词表逐条平价）
// ---------------------------------------------------------------------------

pub(crate) const COL_DATE: &str = "日期";
pub(crate) const COL_KIND: &str = "类型";
pub(crate) const COL_INSTRUMENT: &str = "标的";
pub(crate) const COL_QUANTITY: &str = "数量";
pub(crate) const COL_AMOUNT: &str = "金额";
pub(crate) const COL_PRICE: &str = "单价";
pub(crate) const COL_FEE: &str = "手续费";
pub(crate) const COL_ACCOUNT: &str = "账户";
pub(crate) const COL_FUNDING_ACCOUNT: &str = "出资账户";
/// 总数前缀（旧壳 investments.ledger.total 共 {n} 条；主列表同构）。
pub(crate) const TOTAL_TEXT_FMT: &str = "共 {n} 条";
pub(crate) const LEDGER_EMPTY: &str = "暂无投资交易";
pub(crate) const LEDGER_EMPTY_FILTERED: &str = "当前筛选无匹配交易";
pub(crate) const KIND_FILTER_PLACEHOLDER: &str = "类型";
pub(crate) const ACCOUNT_FILTER_PLACEHOLDER: &str = "账户";
/// 清除筛选（旧壳 transactions.filter.clear 同词复用）。
pub(crate) const CLEAR_FILTERS: &str = "清除筛选";
/// 创建入口（旧壳 investments.ledger.createBuy / createSell 同词表）。
pub(crate) const CREATE_BUY: &str = "记买入";
pub(crate) const CREATE_SELL: &str = "记卖出";
/// 行菜单词（旧壳 transaction-row-menu 单点复用：编辑/详情/删除同词）。
pub(crate) const MENU_EDIT: &str = "编辑";
pub(crate) const MENU_DETAIL: &str = "详情";
pub(crate) const MENU_DELETE: &str = "删除";
/// 删除确认（旧壳 transactions.deleteDialog / list.deleted 同词复用）。
const DELETE_TITLE: &str = "删除交易";
const DELETE_CONFIRM_CONTENT: &str = "确认删除该条交易？删除后不可恢复。";
const DELETE_CONFIRM: &str = "删除";
const DELETE_CANCEL: &str = "取消";
const DELETED: &str = "已删除";
/// 失败条位前缀（码化错误 zh 原样；旧壳 list.deleteFailed / detail.loadFailed
/// 同词）。
const DELETE_FAILED_PREFIX: &str = "删除失败: ";
pub(crate) const LOAD_FAILED_PREFIX: &str = "加载失败: ";
const MSG_INTERNAL: &str = "内部错误：读任务异常退出";
/// 只读详情弹窗标题（旧壳 transactions.detail.title 同词）。
pub(crate) const DETAIL_TITLE: &str = "交易详情";

/// 类型筛选选项闭集（明细节 = 行集闭包同源：五种投资 kind——旧壳
/// LEDGER_TAB_KINDS 同一闭集；label 经 kind_label 闭集序渲染）。
pub(crate) const LEDGER_TAB_KINDS: [TransactionKind; 5] = [
    TransactionKind::Buy,
    TransactionKind::Sell,
    TransactionKind::Convert,
    TransactionKind::Split,
    TransactionKind::Dividend,
];

/// 类型多选选项（旧壳 kindValue 按闭集序渲染同语义）。
fn kind_filter_options() -> Vec<CategoryOption> {
    LEDGER_TAB_KINDS
        .iter()
        .map(|kind| CategoryOption {
            id: kind.as_str().to_owned(),
            label: assembly::kind_label(*kind, assembly::LendingDirection::None).to_owned(),
        })
        .collect()
}

/// 类型多选选中归一（值序不作为展示序——选项闭集序由 multi_select 承担；
/// 翻页归零由追踪 effect 对实际变化承担）。
fn kind_signature(kinds: &[TransactionKind]) -> Vec<String> {
    let mut ids: Vec<String> = kinds.iter().map(|k| k.as_str().to_owned()).collect();
    ids.sort();
    ids
}

// ---------------------------------------------------------------------------
// 行快照与格文本（装配层：渲染期零格式化——本页签格文本一次产出）
// ---------------------------------------------------------------------------

/// 明细行快照（旧壳 `InvestmentTransactionRow` 行的格投影闭集；kind 专属
/// 载荷在装配闭集内化成分格文本——渲染期只消费 String/bool）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LedgerRowSnap {
    pub(crate) id: String,
    pub(crate) date_text: String,
    /// 类型词（kind 闭集标签；类型列）。
    pub(crate) kind_word: &'static str,
    pub(crate) kind: TransactionKind,
    /// 标的格（convert 行「转出 → 转入」双腿；其余「代码 · 名称」/ 裸码）。
    pub(crate) instrument_text: String,
    /// 数量格（convert 双腿「→」；split 带符号 Δ；dividend 「-」）。
    pub(crate) quantity_text: String,
    /// 金额格（convert 行读转出金额确认单权威；split 「-」；其余行金额锚
    /// 点；纯数字不带币符——旧壳混币列表不暗示同币种）。
    pub(crate) amount_text: Option<String>,
    /// 单价格（仅 buy/sell 成交单价；万分之一元刻度展示;其余「-」）。
    pub(crate) price_text: String,
    /// 手续费格（仅 buy/sell；其余「-」）。
    pub(crate) fee_text: String,
    pub(crate) account_name: String,
    pub(crate) funding_account_name: String,
    /// 只读详情载荷（只读 kind 弹窗再投影消费；行集投影自带，不再取数）。
    pub(crate) detail: Option<DetailPayload>,
    /// 可编辑行（buy/sell——旧壳 supportsRowEdit 同判定单口径）。
    pub(crate) editable: bool,
    /// 编辑回填锚（弹窗行适配投影，旧壳 ledgerRowToModalRow 的重生同消费
    /// 面——先取明细再开窗的载荷由行投影携带，列表呈现永不读它的任何字段
    /// 。只读 kind 为 None（编辑入口随行缺席）。
    pub(crate) edit_anchor: Option<investments_form::TradeEditAnchor>,
}

/// 只读详情载荷（convert / split / dividend 行的弹窗再投影；全部文本一并
/// 装配产出——弹窗渲染期零格式化）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DetailPayload {
    Convert {
        from_symbol: String,
        to_symbol: String,
        out_shares_text: String,
        in_shares_text: String,
        out_amount_text: String,
        in_amount_text: String,
    },
    Split {
        delta_shares_text: String,
    },
    Dividend {
        amount_text: String,
        account_name: String,
        date_text: String,
    },
}

/// 只读 kind 词（详情弹窗行义项；kind_label 闭集同源）。
impl DetailPayload {
    pub(crate) fn kind_word(&self) -> &'static str {
        match self {
            DetailPayload::Convert { .. } => "转换",
            DetailPayload::Split { .. } => "份额调整",
            DetailPayload::Dividend { .. } => "分红",
        }
    }
}

/// 数量格（旧壳 quantityCell 单点）；隐私掩码高速相伴（数量也是量值——
/// 隐私态统一掩码，全壳同源）。
fn quantity_cell(privacy: bool, row: &ledger_investment::InvestmentTransactionRow) -> String {
    if privacy {
        return assembly::AMOUNT_PRIVACY_MASK.to_owned();
    }
    if let Some(trade) = row.trade.as_ref() {
        return assembly::format_quantity_text_f64(trade.quantity, false);
    }
    if let Some(convert) = row.convert.as_ref() {
        return format!(
            "{} → {}",
            assembly::format_quantity_text_f64(convert.quantity, false),
            assembly::format_quantity_text_f64(convert.to_quantity, false)
        );
    }
    if let Some(split) = row.split.as_ref() {
        let delta = split.delta_quantity;
        return if delta > 0.0 {
            format!("+{}", assembly::format_quantity_text_f64(delta, false))
        } else {
            assembly::format_quantity_text_f64(delta, false)
        };
    }
    "-".to_owned()
}

/// 金额格（旧壳 amountCell 单点：convert 行读转出金额确认单权威；split 行
/// 无现金腿「-」——「None 渲染 -，不以 0 伪装已知为零」旧壳同口径；其余
/// 行金额锚点纯数字）。
fn amount_cell(privacy: bool, row: &ledger_investment::InvestmentTransactionRow) -> Option<String> {
    if privacy {
        return Some(assembly::AMOUNT_PRIVACY_MASK.to_owned());
    }
    match row.convert.as_ref() {
        Some(convert) => Some(assembly::cents_to_amount_text(convert.out_amount_cents, 2)),
        None if row.split.is_some() => None,
        None => Some(assembly::cents_to_amount_text(row.amount_cents, 2)),
    }
}

fn cents_text(privacy: bool, cents: i64) -> String {
    if privacy {
        assembly::AMOUNT_PRIVACY_MASK.to_owned()
    } else {
        assembly::cents_to_amount_text(cents, 2)
    }
}

/// 只读详情载荷投影（旧壳三只读详情组件消费面的本壳最小实体：convert 呈
/// 现「A → B」两侧标的/份额/金额（确认单权威）/账户端；split 呈现带符号
/// Δ 与调整日/账户；dividend 呈现归属金额（本位币行投影——折算口径单点
/// 在写路径已落定）/到账账户/日期）。旧壳 OrderDetail 分支：订单区随
/// #1930 已退役详单（明细行集闭包内 supportedRowEdit 判定不落 order 意图
/// ——本页签零死角）。
fn assemble_detail_payload(
    privacy: bool,
    row: &ledger_investment::InvestmentTransactionRow,
    account_name_of: impl Fn(&str) -> Option<String> + Copy,
) -> Option<DetailPayload> {
    match row.kind {
        TransactionKind::Convert => row.convert.as_ref().map(|convert| DetailPayload::Convert {
            from_symbol: row.symbol.clone(),
            to_symbol: convert.to_symbol.clone(),
            out_shares_text: assembly::format_quantity_text_f64(convert.quantity, privacy),
            in_shares_text: assembly::format_quantity_text_f64(convert.to_quantity, privacy),
            out_amount_text: cents_text(privacy, convert.out_amount_cents),
            in_amount_text: cents_text(privacy, convert.in_amount_cents),
        }),
        TransactionKind::Split => row.split.as_ref().map(|split| {
            let delta = split.delta_quantity;
            let delta_text = if delta > 0.0 {
                format!("+{}", assembly::format_quantity_text_f64(delta, privacy))
            } else {
                assembly::format_quantity_text_f64(delta, privacy)
            };
            DetailPayload::Split {
                delta_shares_text: delta_text,
            }
        }),
        TransactionKind::Dividend => Some(DetailPayload::Dividend {
            amount_text: if privacy {
                assembly::AMOUNT_PRIVACY_MASK.to_owned()
            } else {
                assembly::cents_to_amount_text(row.amount_native_cents, 2)
            },
            account_name: account_name_of(&row.account_id).unwrap_or_else(|| "-".to_owned()),
            date_text: row.date.clone(),
        }),
        _ => None,
    }
}

/// 行集装配（域读 → 行快照闭集；账户名映射与隐私开关是装配输入——装载
/// 时机读取，隐私不响应式）。单价格的展示币种 = 标的币种（价格随行币种
/// 投影——旧壳价直接无边币；NSH 沿持仓现价格同口径带符号）。
pub(crate) fn assemble_ledger_rows(
    rows: &[ledger_investment::InvestmentTransactionRow],
    account_name_of: impl Fn(&str) -> Option<String> + Copy,
    privacy: bool,
) -> Vec<LedgerRowSnap> {
    rows.iter()
        .map(|row| {
            let price_fmt =
                crate::assembly::CurrencyTable::default().format_of(row.currency_code.as_str());
            let price_text = row
                .trade
                .as_ref()
                .map(|trade| {
                    if privacy {
                        assembly::AMOUNT_PRIVACY_MASK.to_owned()
                    } else {
                        assembly::format_price_text(trade.price_cents, &price_fmt)
                    }
                })
                .unwrap_or_else(|| "-".to_owned());
            let fee_text = row
                .trade
                .as_ref()
                .map(|trade| {
                    if privacy {
                        assembly::AMOUNT_PRIVACY_MASK.to_owned()
                    } else {
                        assembly::cents_to_amount_text(trade.fee_cents, 2)
                    }
                })
                .unwrap_or_else(|| "-".to_owned());
            let instrument_text = match row.convert.as_ref() {
                Some(convert) => format!("{} → {}", row.symbol, convert.to_symbol),
                None => match row
                    .instrument_name
                    .as_deref()
                    .filter(|n| !n.trim().is_empty())
                {
                    Some(name) => format!("{} · {}", row.symbol, name),
                    None => row.symbol.clone(),
                },
            };
            let payload = assemble_detail_payload(privacy, row, account_name_of);
            LedgerRowSnap {
                id: row.id.clone(),
                date_text: row.date.clone(),
                kind_word: assembly::kind_label(row.kind, assembly::LendingDirection::None),
                kind: row.kind,
                instrument_text,
                quantity_text: quantity_cell(privacy, row),
                amount_text: amount_cell(privacy, row),
                price_text,
                fee_text,
                account_name: account_name_of(&row.account_id).unwrap_or_else(|| "-".to_owned()),
                funding_account_name: row
                    .funding_account_id
                    .as_deref()
                    .and_then(account_name_of)
                    .unwrap_or_else(|| "-".to_owned()),
                detail: payload,
                editable: matches!(row.kind, TransactionKind::Buy | TransactionKind::Sell),
                edit_anchor: Some(investments_form::TradeEditAnchor {
                    id: row.id.clone(),
                    kind: row.kind,
                    amount_cents: row.amount_cents,
                    currency_code: row.currency_code.clone(),
                    account_id: Some(row.account_id.clone()),
                    funding_account_id: row.funding_account_id.clone(),
                    note: row.note.clone(),
                    source_order_no: row.source_order_no.clone(),
                    date: row.date.clone(),
                }),
            }
        })
        .collect()
}

/// 空态分型（旧壳 two 型同口径：默认空账本 / 筛选无匹配两端分类；筛选激
/// 活判定含深链带入的标的维）。纯函数、headless 空态断言对口。
pub(crate) fn ledger_empty_text(
    total: i64,
    first_page_present: bool,
    filters_active: bool,
) -> &'static str {
    if filters_active {
        LEDGER_EMPTY_FILTERED
    } else if first_page_present && total == 0 {
        LEDGER_EMPTY
    } else {
        LEDGER_EMPTY_FILTERED
    }
}

// ---------------------------------------------------------------------------
// 列定义（旧壳 NDataTable columns 等值档；唯一弹性轴 = 标的列（旧壳
// 弹性列同位——NSH 宽屏总宽可容纳：建议宽和 < 内容区，不做水平滚动）
// ---------------------------------------------------------------------------

const DATE_W: f32 = 105.0;
const KIND_W: f32 = 90.0;
const INSTRUMENT_W: f32 = 200.0;
const QUANTITY_W: f32 = 130.0;
const AMOUNT_W: f32 = 120.0;
const PRICE_W: f32 = 110.0;
const FEE_W: f32 = 100.0;
const ACCOUNT_W: f32 = 110.0;
const FUNDING_W: f32 = 110.0;

/// 有效页数（≥1；服务端分页同语义同构——旧壳 ADR-0008 هم形；页大小 = 本
/// 页签固定档 [`boot::LEDGER_TAB_PAGE_SIZE`]）。
pub(crate) fn ledger_total_pages(total: i64) -> u32 {
    let count = total.max(0) as usize;
    count.div_ceil(boot::LEDGER_TAB_PAGE_SIZE).max(1) as u32
}

fn ledger_columns() -> Vec<DataTableColumn> {
    let col = |key: &'static str, title: &'static str, width: f32, align: DataTableAlign| {
        let mut col = DataTableColumn::default();
        col.key = key.into();
        col.title = Some(title.into());
        col.width = Some(Points(width));
        col.align = align;
        col
    };
    vec![
        col("date", COL_DATE, DATE_W, DataTableAlign::Start),
        col("kind", COL_KIND, KIND_W, DataTableAlign::Start),
        col(
            "instrument",
            COL_INSTRUMENT,
            INSTRUMENT_W,
            DataTableAlign::Start,
        ),
        col("quantity", COL_QUANTITY, QUANTITY_W, DataTableAlign::End),
        col("amount", COL_AMOUNT, AMOUNT_W, DataTableAlign::End),
        col("price", COL_PRICE, PRICE_W, DataTableAlign::End),
        col("fee", COL_FEE, FEE_W, DataTableAlign::End),
        col("account", COL_ACCOUNT, ACCOUNT_W, DataTableAlign::Start),
        col(
            "funding",
            COL_FUNDING_ACCOUNT,
            FUNDING_W,
            DataTableAlign::Start,
        ),
    ]
}

/// 行快照 → 格文本（按列 key 消费快照字段；渲染期零格式化——全部文本已
/// 是装配产物）。金额列 None → "-"（旧壳 None 渲染「-」口径）。
fn cell_text_of(snap: &LedgerRowSnap, key: &str) -> String {
    match key {
        "date" => snap.date_text.clone(),
        "kind" => snap.kind_word.to_owned(),
        "instrument" => snap.instrument_text.clone(),
        "quantity" => snap.quantity_text.clone(),
        "amount" => snap.amount_text.clone().unwrap_or_else(|| "-".to_owned()),
        "price" => snap.price_text.clone(),
        "fee" => snap.fee_text.clone(),
        "account" => snap.account_name.clone(),
        "funding" => snap.funding_account_name.clone(),
        _ => String::new(),
    }
}

/// 行快照 → 宿主表格数据（投资行集投影；sort= 无——服务端 date 倒序直
/// 出，旧壳 remote 分页同语义）。
fn rows_to_data(rows: &[LedgerRowSnap]) -> DataTableRows {
    DataTableRows::new(rows.len())
        .texts("date", rows.iter().map(|r| r.date_text.as_str()))
        .texts("kind", rows.iter().map(|r| r.kind_word))
        .texts(
            "instrument",
            rows.iter().map(|r| r.instrument_text.as_str()),
        )
        .texts("quantity", rows.iter().map(|r| r.quantity_text.as_str()))
        .texts("amount", rows.iter().map(amount_data_text))
        .texts("price", rows.iter().map(|r| r.price_text.as_str()))
        .texts("fee", rows.iter().map(|r| r.fee_text.as_str()))
        .texts("account", rows.iter().map(|r| r.account_name.as_str()))
        .texts(
            "funding",
            rows.iter().map(|r| r.funding_account_name.as_str()),
        )
}

/// 金额数据格文本（None → "-"——旧壳 None 渲染「-」；数据面出口避免在迭代
/// 内落临时引用）。
fn amount_data_text(row: &LedgerRowSnap) -> String {
    row.amount_text.clone().unwrap_or_else(|| "-".to_owned())
}
/// 表格部件主题（交易列表 table_classes 同形映射；行悬停 + 空态弱化面）。
fn table_classes() -> DataTableClasses {
    let mut classes = DataTableClasses::default();
    classes.root = Some("bg-background text-foreground".into());
    classes.header_cell = Some("text-muted-foreground px-3".into());
    classes.cell = Some("text-foreground px-3".into());
    classes.row_hover = Some("bg-accent".into());
    classes.empty = Some("text-muted-foreground".into());
    classes
}

/// 筛选签名（同值守卫的比较锚；二维空集合归一层——过滤面只有类型/账户/
/// 标的深链）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterSignature {
    kinds: Vec<String>,
    account_id: Option<String>,
    instrument_id: Option<String>,
}

impl FilterSignature {
    fn of(
        kinds: &[TransactionKind],
        account_id: Option<String>,
        instrument_id: Option<String>,
    ) -> Self {
        Self {
            kinds: kind_signature(kinds),
            account_id,
            instrument_id,
        }
    }
}

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

/// 明细页签本体（旧壳 InvestmentLedgerTab 的本壳等价：筛选三维 + 服务端分
/// 页明细表 + 行右键操作弹窗族 + 创建入口两钮 + 创建与只读详情弹窗挂面）。
/// 「删除即红」负向条目钉在 headless：页签激活装载 / 筛选重拉 / 创建入口
/// 接线 / 交易失效重读 / 删除重读。
#[component]
pub(crate) fn LedgerPane() -> impl IntoElement {
    let palette = investments::pane_palette();
    let session = investments::store();
    let view_owner = Owner::current();

    // 行集快照束（失败错误条 + 重试；隐私装载时机读取；账户候选与名映射
    // 随列表读批同批装载——参考数据读失败不静默降级成「-」[账户名缺失显
    // 示降级面在装配层]）。
    let rows = signal(Vec::<LedgerRowSnap>::new());
    let total = signal(0_i64);
    let error = signal(None::<String>);
    let loading = signal(false);
    let accounts = signal(investments_form::Accounts(Vec::new()));
    #[allow(unused_assignments)]
    let privacy = signal(false);

    // 行操作弹窗族（#2046 偏差 1 消费：买卖表单族——创建与编辑共用弹窗、
    // 只读详情弹窗、删除确认目标与反馈条位。意图互不串扰）。
    let trade_form = investments_form::TradeFormState::new();
    let trade_sources = investments_form::TradeFormSources::new();

    let delete_target = signal(None::<String>);
    let delete_error = signal(None::<String>);
    let delete_feedback = signal(None::<String>);
    let detail_target = signal(None::<DetailPayload>);

    // 账户名映射（行装配消费面； portfolios 快照既作图例又作行装配）。
    let account_name_of = move |id: &str| {
        accounts
            .get_untracked()
            .0
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
    };

    // 装载（页签激活边 + 筛选变化 + 失效信号 + 重试共用一条路径；分页单
    // 飞——装载期间分页条禁用（等待成功优于即刻失败）。空页自愈（issue
    // #893 同款：空页 + 尚有数据 + 非第一页 → 回退一页重拉，本响应不落数
    // 据——不渲染空页）。
    let load = move || {
        loading.set(true);
        error.set(None);
        let privacy_enabled = app::amount_privacy_enabled();
        // 会话快照进闭包（低频信号值拉过先例）。
        let page_now = session.detail_page.get_untracked().max(1);
        let kinds = session.detail_kinds.get_untracked();
        let filter = ledger_investment::InvestmentTransactionListFilter {
            page: Some(page_now),
            page_size: Some(boot::LEDGER_TAB_PAGE_SIZE),
            kinds: (!kinds.is_empty()).then(|| kinds.clone()),
            account_id: session.detail_account_id.get_untracked(),
            instrument_id: session.detail_instrument_id.get_untracked(),
            from: None,
            to: None,
        };
        let filter_account = filter.account_id.clone();
        let filter_instrument = filter.instrument_id.clone();
        let tracked_session = session;
        let db = app::db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                loading.set(false);
                error.set(Some(format!("{LOAD_FAILED_PREFIX}{MSG_INTERNAL}")));
                return;
            };
            let outcome = blocking(
                move || -> ledger_infra::error::Result<
                    (
                        ledger_investment::InvestmentTransactionListResult,
                        Vec<ledger_accounts::Account>,
                    ),
                > {
                    let page = boot::investment_transaction_page(&db, &filter)?;
                    let account_rows = boot::account_rows(&db)?;
                    Ok((page, account_rows))
                },
            )
            .await;
            loading.set(false);
            match outcome {
                Ok(Ok((page, account_rows))) => {
                    // 空页自愈（issue #893 同款）。
                    if page.items.is_empty()
                        && page.total > 0
                        && tracked_session.detail_page.get_untracked() > 1
                    {
                        tracked_session
                            .detail_page
                            .set(tracked_session.detail_page.get_untracked() - 1);
                        return;
                    }
                    privacy.set(privacy_enabled);
                    accounts.set(investments_form::Accounts(account_rows));
                    rows.set(assemble_ledger_rows(
                        &page.items,
                        account_name_of,
                        privacy_enabled,
                    ));
                    total.set(page.total);
                    error.set(None);
                }
                Ok(Err(e)) => error.set(Some(format!("{LOAD_FAILED_PREFIX}{e}"))),
                Err(_) => error.set(Some(format!("{LOAD_FAILED_PREFIX}{MSG_INTERNAL}"))),
            }
            let _ = (filter_account, filter_instrument);
        });
    };

    // 筛选三维任一实际变化 → 翻页归零 + 重拉（同值守卫；同值重设与首载
    // 不触发。删除本 effect 接线即红：筛选变化不刷新，headless 筛选用例红。
    // 装载与「deep-link 覆盖」共用本 effect——下钻落点直接写会话槽即天然
    // 走到本重拉）。
    let tracked = std::rc::Rc::new(std::cell::RefCell::new(None::<FilterSignature>));
    let tracked_for_effect = tracked;
    effect(move || {
        let current = FilterSignature::of(
            session.detail_kinds.get().as_slice(),
            session.detail_account_id.get(),
            session.detail_instrument_id.get(),
        );
        let mut prev = tracked_for_effect.borrow_mut();
        if prev.as_ref() == Some(&current) {
            return;
        }
        *prev = Some(current);
        drop(prev);
        session.detail_page.set(1);
        load();
    });

    // 页签落位装载（visited-once keep-mounted：激活边即重拉——旧壳明细页
    // 签 display 缺省 'if' 重挂重拉语义，会话内保留的是选择不是快照；删除
    // 接线即红钉 headless 激活装载用例）。
    let was_active = signal(false);
    effect(move || {
        let active = session.tab.get() == investments::InvestmentsTab::Ledger;
        if active && !was_active.get_untracked() {
            load();
        }
        was_active.set(active);
    });

    // 失效信号（交易写路径统一接线点 → 明细重读；句柄随 pane 实例存亡；
    // 删除接线即红钉 lib 探针：买卖写后明细不刷新——旧壳 PRICES 涨跌外部
    // 无关线，本面交易族单点承载）。
    let tx_sub = signals::subscribe_transactions_changed(load);
    effect(move || {
        let held: &signals::TransactionsChangedSubscription = &tx_sub;
        let _ = held;
    });

    // —— 筛选行 ——
    // 类型多选（multi_select 调用方归一——归一序由选项闭集序承担）。
    let on_kind_toggle = move |id: &str| {
        let mut next = session.detail_kinds.get_untracked();
        if let Some(kind) = LEDGER_TAB_KINDS.iter().copied().find(|k| k.as_str() == id) {
            if next.contains(&kind) {
                next.retain(|kept| *kept != kind);
            } else {
                // 归一序按闭集序重排（旧壳 kindValue 同构）。
                next.push(kind);
                next.sort_by_key(|k| {
                    LEDGER_TAB_KINDS
                        .iter()
                        .position(|closed| closed == k)
                        .unwrap_or_default()
                });
            }
        }
        session.detail_kinds.set(next);
    };
    let on_kind_clear = move || session.detail_kinds.set(Vec::new());

    // 筛选激活判定（清除筛选按钮的准入 + 空态分型，旧壳 filtersActive 同
    // 判定——含深链带入的标的维）。
    let filters_active = move || {
        !session.detail_kinds.get_untracked().is_empty()
            || session.detail_account_id.get_untracked().is_some()
            || session.detail_instrument_id.get_untracked().is_some()
    };

    let filter_row = move || {
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                view()
                    .test_id("ledger-kind-filter")
                    .child(multi_select::multi_select(
                        "ledger-kind",
                        kind_filter_options,
                        move |id| {
                            session
                                .detail_kinds
                                .get_untracked()
                                .iter()
                                .any(|k| k.as_str() == id)
                        },
                        on_kind_toggle,
                        on_kind_clear,
                        KIND_FILTER_PLACEHOLDER,
                    )),
            )
            .child(
                view()
                    .test_id("ledger-account-filter")
                    .child(pinyin_select::pinyin_select(
                        "ledger-account",
                        move || {
                            pinned_account_options(
                                &accounts.get().0,
                                session.detail_account_id.get_untracked(),
                            )
                        },
                        session.detail_account_id,
                        ACCOUNT_FILTER_PLACEHOLDER,
                        true,
                    )),
            )
            .child(
                // 清除筛选（#1807 主列表同款判定：任一明细维度激活即可用；清明
                // 细筛选 + 翻页归零，不切页签、不动页大小）。
                view()
                    .test_id("ledger-clear-filters")
                    .disabled(move || !filters_active())
                    .on_press(move |_: &PressEvent| {
                        session.detail_kinds.set(Vec::new());
                        session.detail_account_id.set(None);
                        session.detail_instrument_id.set(None);
                    })
                    .rounded_md()
                    .px_2()
                    .py_1()
                    .app_class("hover:bg-accent disabled:opacity-40")
                    .child(
                        text(CLEAR_FILTERS)
                            .text_size(px(12.0))
                            .text_color(palette.fg),
                    ),
            )
            .child(
                // 创建入口（#2046 偏差 1 的消费面）：类型由入口单点表达（旧壳
                // ADR-0135 决策 5）；提交走既有创建编排与 TransactionInput 装配
                // 接缝（装配器零重迁）。
                view()
                    .flex_row()
                    .gap_2()
                    .child(
                        view()
                            .on_press({
                                move |_: &PressEvent| {
                                    trade_form.open_create(
                                        TransactionKind::Buy,
                                        trade_sources,
                                        view_owner,
                                    );
                                }
                            })
                            .test_id("ledger-create-buy")
                            .rounded_md()
                            .px_3()
                            .py_1()
                            .app_class("bg-primary hover:opacity-90")
                            .child(
                                text(CREATE_BUY)
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xffffff)),
                            ),
                    )
                    .child(
                        view()
                            .on_press({
                                move |_: &PressEvent| {
                                    trade_form.open_create(
                                        TransactionKind::Sell,
                                        trade_sources,
                                        view_owner,
                                    );
                                }
                            })
                            .test_id("ledger-create-sell")
                            .rounded_md()
                            .px_3()
                            .py_1()
                            .app_class("bg-primary hover:opacity-90")
                            .child(
                                text(CREATE_SELL)
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xffffff)),
                            ),
                    ),
            )
    };

    // —— 行右键菜单分派（旧壳 transaction-row-menu 复用闭集：除 refund 外
    // 可编辑行首项「编辑」、界面只读 kind 仅「详情」、其余行含「删除」——
    // 行激活闭集单口径 supportsRowEdit，本页签无第二套判定）。
    let on_context_menu = {
        move |e: &DataTableContextMenu| {
            let Some(row_index) = e.row else {
                return;
            };
            let Some(snap) = rows.get_untracked().get(row_index as usize).cloned() else {
                return;
            };
            let mut items: Vec<MenuItem> = Vec::new();
            if let (true, Some(anchor_f)) = (snap.editable, snap.edit_anchor.clone()) {
                items.push(MenuItem::new(MENU_EDIT).on_select(move || {
                    // 先取明细再开窗（编排语义归 investments_form 单点）；
                    // 行载荷 = 明细行适配投影（ledgerRowToModalRow 重生，随
                    // 行携带——防全字段替换误抹备注/订单号）。
                    trade_form.begin_edit(anchor_f.clone(), trade_sources, view_owner);
                }));
            } else if let Some(payload_f) = snap.detail.clone() {
                items.push(MenuItem::new(MENU_DETAIL).on_select(move || {
                    detail_target.set(Some(payload_f.clone()));
                }));
            }
            let confirm_id = snap.id.clone();
            items.push(MenuItem::new(MENU_DELETE).on_select(move || {
                delete_target.set(Some(confirm_id.clone()));
            }));
            context_menu::open_context_menu(items, Point::new(e.x, e.y));
        }
    };

    // —— 删除执行链（危险确认结局归一：确认才执行，删后重读；任务挂视
    // 图实例 owner，关窗卸载对话框子树不影响——旧壳 afterRowDelete 同位）。
    let delete_owner = view_owner;
    let on_danger_outcome = move |outcome: danger::DangerOutcome| {
        let id = delete_target.get_untracked();
        delete_target.set(None);
        if outcome == danger::DangerOutcome::Confirmed
            && let Some(id) = id
        {
            perform_delete(id, delete_owner, delete_error, delete_feedback, load);
        }
    };

    // —— 表体（列闭集在 otherwise 闭包内重复构造——组件 Fn 闭包不可重复
    // 消费同一 Vec 先例 trend_chart_of 同形） ——
    let body = when(
        move || error.get().is_some(),
        move || {
            investments::retry_row(
                move || error.get().unwrap_or_default(),
                "ledger-error",
                "ledger-retry",
                palette,
                load,
            )
        },
    )
    .otherwise(move || {
        view()
            .flex_col()
            .gap_2()
            .child(filter_row())
            .child(
                // 反馈条位（删除成功提示；NSH 无 toast——条位语义）。
                when(
                    move || delete_feedback.get().is_some(),
                    move || {
                        text(move || delete_feedback.get().unwrap_or_default())
                            .text_size(px(12.0))
                            .text_color(palette.fg)
                            .test_id("ledger-delete-status")
                    },
                )
                .otherwise(view),
            )
            .child(
                when(
                    move || delete_error.get().is_some(),
                    move || {
                        text(move || {
                            format!(
                                "{DELETE_FAILED_PREFIX}{}",
                                delete_error.get().unwrap_or_default()
                            )
                        })
                        .text_size(px(12.0))
                        .text_color(palette.danger)
                        .test_id("ledger-delete-error")
                    },
                )
                .otherwise(view),
            )
            .child(
                // 明细表（DataTable；ρ列闭集 = 旧壳 columns 词表，唯一弹
                // 性轴暂不分配——总宽 < 内容区。行右键菜单见 on_context_menu。
                data_table()
                    .columns(ledger_columns())
                    .data(move || rows_to_data(&rows.get()))
                    .render_cell(ledger_cell_renderer(rows, palette))
                    .on_context_menu(on_context_menu)
                    .loading(move || loading.get())
                    .loading_view(text("正在加载…").text_color(palette.muted))
                    .empty(
                        text(move || {
                            ledger_empty_text(total.get(), !rows.get().is_empty(), filters_active())
                        })
                        .text_color(palette.muted)
                        .test_id("ledger-empty"),
                    )
                    .classes(table_classes())
                    .flex_1(),
            )
            .child(
                // 服务端分页（旧壳 ADR-0008 同形：「共 N 条」前缀 + 分页条）。
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        text(move || TOTAL_TEXT_FMT.replace("{n}", &total.get().to_string()))
                            .text_size(px(12.0))
                            .text_color(palette.muted)
                            .test_id("ledger-total"),
                    )
                    .child(
                        when(
                            move || ledger_total_pages(total.get()) > 1,
                            move || {
                                view().test_id("ledger-pagination").flex_row().child(
                                    pagination()
                                        .accessibility_label("投资明细分页")
                                        .previous_accessibility_label("上一页")
                                        .next_accessibility_label("下一页")
                                        .page(move || session.detail_page.get() as u32)
                                        .total_pages(move || ledger_total_pages(total.get()))
                                        .disabled(move || loading.get())
                                        .item_class(
                                            "px-2 py-1 rounded-md text-foreground hover:bg-accent \
                                             selected:bg-accent disabled:opacity-40",
                                        )
                                        .on_change(move |&picked: &u32| {
                                            let page = picked.max(1) as usize;
                                            if page == session.detail_page.get_untracked() {
                                                return;
                                            }
                                            session.detail_page.set(page);
                                            load();
                                        }),
                                )
                            },
                        )
                        .otherwise(view),
                    ),
            )
    });

    scroll().flex_1().child(
        view().flex_col().child(
            // 卡容器（旧壳 NSpace vertical + filter 表的等价形态）。
            view()
                .test_id("ledger-card")
                .flex_col()
                .gap_3()
                .rounded_lg()
                .mx_4()
                .my_2()
                .p_4()
                .app_class("border border-border bg-background")
                .child(
                    text(investments::LEDGER_TAB_LABEL)
                        .text_size(px(15.0))
                        .text_color(palette.fg)
                        .test_id("ledger-title"),
                )
                .child(body)
                // 危险确认对话框挂面（error 级形态；抑制上报与 ESC
                // /遮罩语义归封装——#2022 交易删除同形）。
                .child(danger::danger_confirm(
                    move || delete_target.get().is_some(),
                    danger::DangerConfirmSpec {
                        level: danger::DangerLevel::Error,
                        title: DELETE_TITLE.to_owned(),
                        lead: Some(DELETE_CONFIRM_CONTENT.to_owned()),
                        strong_warning: Some("删除后不可恢复。".to_owned()),
                        detail: None,
                        confirm_label: DELETE_CONFIRM.to_owned(),
                        cancel_label: DELETE_CANCEL.to_owned(),
                    },
                    on_danger_outcome,
                ))
                // 买卖表单弹窗（#2046 族复用：创建与编辑同弹窗语义；
                // 装配器与先取再开窗编排零重迁）。
                .child(investments_form::trade_form_modal(
                    trade_form,
                    trade_sources,
                    view_owner,
                ))
                // 参考装载/编辑明细失败条位（参考先载再开窗 / 先取再
                // 开窗失败不开窗的呈现位；下次尝试开始时清空——旧壳
                // toast 的条位对应物）。
                .child(
                    when(
                        move || trade_form.load_error.get().is_some(),
                        move || {
                            text(move || trade_form.load_error.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(palette.danger)
                                .test_id("ledger-trade-load-error")
                                .app_class("px-4 py-1")
                        },
                    )
                    .otherwise(view),
                )
                // 只读详情弹窗（convert / split / dividend）。
                .child(detail_modal(palette, detail_target)),
        ),
    )
}

/// 删除执行（危险确认通过线）：旧壳 delete_transaction 同一路径（软删编排 + 级联与码化守卫照常）——成功 = 反馈条位 + 以当前状态重读（页尾空集时由 load 的空页自愈回退一页，同主列表 afterRowDelete 可观察结果）；失败码化错误条位不静默；任务挂视图实例。
fn perform_delete(
    id: String,
    view_owner: Owner,
    delete_error: Signal<Option<String>>,
    delete_feedback: Signal<Option<String>>,
    reload: impl Fn() + Copy + 'static,
) {
    let db = app::db_slot().clone();
    view_owner.run(move || {
        spawn(async move {
            let Some(db) = db else {
                delete_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let target = id.clone();
            let outcome = blocking(move || boot::delete_transaction(&db, &target)).await;
            match outcome {
                Ok(Ok(())) => {
                    delete_error.set(None);
                    delete_feedback.set(Some(DELETED.to_owned()));
                    reload();
                }
                Ok(Err(e)) => delete_error.set(Some(e.to_string())),
                Err(_) => delete_error.set(Some("内部错误：写入任务异常退出".to_owned())),
            }
        });
    });
}

/// 自绘格（投资行全平文：类型列语义词随 kind 闭集，tag 色面缺席——旧壳
/// KIND_TAG_TYPE 与本壳 kind 权重色单点统一预告，缺件留痕最低形态平文）。
/// 对齐语义：数值列右对齐（旧壳 align: right 同构），其余贴左。
fn ledger_cell_renderer(
    rows: Signal<Vec<LedgerRowSnap>>,
    palette: PanePalette,
) -> impl Fn(&DataTableCell) -> Text + 'static {
    move |cell: &DataTableCell| {
        let snap = rows.get_untracked().get(cell.row as usize).cloned();
        let text_value = snap
            .map(|s| cell_text_of(&s, cell.key.as_ref()))
            .unwrap_or_default();
        let mut label = text(text_value).number_of_lines(1);
        if cell.key == "quantity"
            || cell.key == "amount"
            || cell.key == "price"
            || cell.key == "fee"
        {
            label = label.text_right();
        }
        label.text_color(palette.fg)
    }
}

/// 账户候选投影（可选择投资账户 − 归档 + 选中归档补值「（已归档）」，值
/// 原样保留——盈亏账户候选同款先例）。
fn pinned_account_options(
    accounts: &[ledger_accounts::Account],
    selected: Option<String>,
) -> Vec<CategoryOption> {
    let mut options: Vec<CategoryOption> = accounts
        .iter()
        .filter(|a| a.kind == ledger_accounts::AccountType::Investment && !a.is_archived)
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && options.iter().all(|o| o.id != id)
        && let Some(row) = accounts.iter().find(|a| a.id == id)
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

/// 只读详情词行（convert「A → B」两侧 / split Δ / dividend 金额与到账账户
/// ——旧壳三只读详情组件消费面的本壳最小实体弹窗行集）。
fn detail_rows(payload: &DetailPayload) -> Vec<(&'static str, String)> {
    match payload {
        DetailPayload::Convert {
            from_symbol,
            to_symbol,
            out_shares_text,
            in_shares_text,
            out_amount_text,
            in_amount_text,
        } => vec![
            ("转出标的", from_symbol.clone()),
            ("转入标的", to_symbol.clone()),
            ("转出份额", out_shares_text.clone()),
            ("转入份额", in_shares_text.clone()),
            ("转出金额", out_amount_text.clone()),
            ("转入金额", in_amount_text.clone()),
        ],
        DetailPayload::Split { delta_shares_text } => {
            vec![("份额变动", delta_shares_text.clone())]
        }
        DetailPayload::Dividend {
            amount_text,
            account_name,
            date_text,
        } => vec![
            ("金额", amount_text.clone()),
            ("到账账户", account_name.clone()),
            ("日期", date_text.clone()),
        ],
    }
}

/// 只读详情弹窗（旧壳 AppModal preset card 同形态；载荷 = 明细行投影自带，
/// 扩展非再取数——showable 关闭语义归 modal 封装）。
fn detail_modal(
    palette: PanePalette,
    detail_target: Signal<Option<DetailPayload>>,
) -> impl IntoElement {
    let fg = palette.fg;
    let muted = palette.muted;
    modal::app_modal(
        move || detail_target.get().is_some(),
        move || DETAIL_TITLE.to_owned(),
        move || detail_target.set(None),
        move || {
            let payload = detail_target.get();
            let kind_word = payload.as_ref().map(|p| p.kind_word().to_owned());
            let rows = payload.map(|p| detail_rows(&p)).unwrap_or_default();
            view()
                .test_id("ledger-detail")
                .flex_col()
                .gap_2()
                .child(
                    text(kind_word.unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(muted)
                        .test_id("ledger-detail-kind"),
                )
                .children(rows.into_iter().map(|(label, value)| {
                    let label_id = format!("ledger-detail-{}", label);
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .test_id(label_id.as_str())
                        .child(text(label).text_size(px(12.0)).text_color(muted))
                        .child(text(value).text_size(px(13.0)).text_color(fg))
                }))
        },
    )
}
