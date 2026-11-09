//! 交易表单弹窗（issue #2034 表单①基础收支 + issue #2035 表单②转账·借出·
//! 退款 + issue #2036 表单③出资分解·购买项下单）：同一表单语义承载六形态
//! ——基础收支（单账户收入/支出）、转账（双账户对转）、借贷（transfer 的
//! 派生视角，方向切换）、退款（关联原支出，继承与金额钳制语义以域为唯一真
//! 源）、出资分解（多账户分摊出资，Σ == 交易金额，契约互斥无主账户位）、
//! 购买项下单（订单 + 商品行，仅 expense）。kind 切换字段族重排，编辑回填
//! 复用同表单（含子行回填）。
//!
//! **形态裁决**：弹窗挂交易列表视图入口（工具行「记一笔」类型菜单七项——
//! 记支出/记收入/记转账 + 分隔 + 借出/借入 + 分隔 + 出资分解/购买项下单；
//! 借贷入口预置方向、表单内可切四向。后两项是表单③首建入口——旧壳创建菜
//! 单闭集五项、无此两入口（录入侧未做，分解/购买项行纠错 = 删除重建或
//! HTTP 契约重导），本票按 spec #2031 表单③纵切授权在新壳首发录入形态）；
//! 行右键「编辑」开放面 = 收入/支出/转账/退款行全量开放（#2035 仅无子行行
//! 开放；表单③支持子行编辑后子行守卫解除：分解行进出资分解族、购买项行进
//! 购买项族、并存行进出资分解族叠加购买项区——旧壳 supportsRowEdit 口径的
//! 更新在平价核对表留痕；退款行的分解是读侧派生不落库，编辑无静默清空面、
//! 随本票开放，旧壳 refund 行仅删除、链接透传保关联）；行右键「退款」仅对
//! 无分解的支出行开放——旧壳 #1861 口径（退款表单未支持显式分解覆盖；投资
//! 形态随投资票扩展）。呈现用 [`crate::modal`] 编辑类模态（新增与编辑同形
//! 态，编辑回填初值——旧壳「编辑复用同表单」语义）。
//!
//! 状态与所有权：表单信号束住调用方视图实例（与预算视图表单态同形），本模
//! 块只提供信号束、开关动作与弹窗挂载面；写路径经 [`crate::boot`] 域公开写
//! 入口直调（不经 IPC），重活走 `blocking` 后台线程、任务挂**视图实例
//! owner**（弹窗子树随关窗卸载不取消写任务，交易删除先例同形）；成功广播
//! [`crate::signals`] 交易失效信号（列表重读——「写→失效信号→列表重读」的
//! 广播接线点，删除即红条目钉在 tests/app_headless.rs）。
//!
//! 装配纪律：wire 字段拼装按形态收口 [`crate::assembly`] 五装配器单点（基础
//! 收支 / 转账借贷 / 退款 / 出资分解 / 购买项下单，per-kind 矩阵 / 元转分 /
//! 日期转换 / 合计一致性不散表单）；金额红态走判定族
//! [`crate::assembly::judge_amount_text`]（红框 + 提交禁用，ADR-0058 两件同
//! 发）；业务类校验（账户未选 / 同账户 / 金额非正 / 换算兜底 / 合计不一致）
//! 与域码化错误走错误条位（旧壳 toast 同语义的条位呈现）。币种随所选账户
//! 推导锁定只读（转账 = 转出账户币种，ADR-0134 决策 5；退款 = 原支出币种，
//! 域继承；出资分解 = 首条出资账户币种——域准入要求逐条与交易币种一致；购
//! 买项下单 = 付款账户币种）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::input;
use nsh_core::prelude::*;

use crate::{
    assembly::{self, BasicFormSubmit, CategoryOption, CurrencyTable, MerchantRef},
    boot, combo_select, date_select, modal, pinyin_select, select, signals,
};
use ledger_infra::error::AppError;

/// 表单锚点前缀（headless `id:` 选择器用；各字段锚点 = 前缀 + 后缀）。
pub(crate) const FORM_ID: &str = "tx-form";
const AMOUNT_ID: &str = "tx-form-amount";
const CURRENCY_ID: &str = "tx-form-currency";
const ACCOUNT_PREFIX: &str = "tx-form-account";
const TO_ACCOUNT_PREFIX: &str = "tx-form-to-account";
const CATEGORY_PREFIX: &str = "tx-form-category";
const MERCHANT_PREFIX: &str = "tx-form-merchant";
const POLICY_PREFIX: &str = "tx-form-policy";
const DATE_PREFIX: &str = "tx-form-date";
const NOTE_ID: &str = "tx-form-note";
const SUBMIT_ID: &str = "tx-form-submit";
const ERROR_ID: &str = "tx-form-error";
/// 表单数据载荷锚点（回填断言用：弹窗内只读字段行）。
const MERCHANT_LABEL_ID: &str = "tx-form-merchant-label";
/// 退款形态只读面锚点（原交易快照行 / 锁定账户行）。
const REFUND_TARGET_ID: &str = "tx-form-refund-original";
const REFUND_ACCOUNT_ID: &str = "tx-form-refund-account";
/// 转账两端币种不一致的错误条位锚点（存量脏行编辑回填态）。
const TO_CURRENCY_ERROR_ID: &str = "tx-form-to-currency-error";

/// 出资项子行区锚点前缀（表单③；行内锚点 = 前缀 + 行键 + 后缀，键随进程
/// 内递增计数器分配、删除不重排）。
const FUND_ACCOUNT_PREFIX: &str = "tx-form-fund-account";
const FUND_AMOUNT_PREFIX: &str = "tx-form-fund-amount";
const FUND_LABEL_PREFIX: &str = "tx-form-fund-label";
const FUND_REMOVE_PREFIX: &str = "tx-form-fund-remove";
/// 加行钮 / 行容器 / 合计行锚点。
const FUNDING_ADD_ID: &str = "tx-form-funding-add";
const FUND_ROW_ID_PREFIX: &str = "tx-form-fund-row";
const FUNDING_TOTAL_ID: &str = "tx-form-funding-total";

/// 购买项商品行区锚点前缀（形态同出资项子行区）。
const PURCHASE_NAME_PREFIX: &str = "tx-form-purchase-name";
const PURCHASE_QTY_PREFIX: &str = "tx-form-purchase-qty";
const PURCHASE_CAT_PREFIX: &str = "tx-form-purchase-cat";
const PURCHASE_PRICE_PREFIX: &str = "tx-form-purchase-price";
const PURCHASE_REMOVE_PREFIX: &str = "tx-form-purchase-remove";
const PURCHASE_ADD_ID: &str = "tx-form-purchase-add";
const PURCHASE_ROW_ID_PREFIX: &str = "tx-form-purchase-row";
const PURCHASE_SUBTOTAL_PREFIX: &str = "tx-form-purchase-subtotal";

/// 表单形态族（字段族重排的选择器）：kind 切换 = 关闭重开（入口单点表达，
/// 旧壳 #150 同语义），族随入口/编辑分派锁定，表单存续期恒定——借贷变体的
/// 方向切换是族内状态，不改族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormFamily {
    /// 基础收支（单账户收入/支出）。
    Basic,
    /// 转账（双账户对转，无方向）。
    Transfer,
    /// 借贷（transfer 派生视角，方向切换；提交产物与转账同构，ADR-0053）。
    Lending,
    /// 退款（关联原支出，继承语义归域）。
    Refund,
    /// 出资分解（多账户分摊出资，Σ == 交易金额；无主账户位——契约互斥，
    /// ADR-0138 决策 2；可叠加购买项区承载并存行编辑）。
    Funding,
    /// 购买项下单（订单 + 商品行，仅 expense；主账户必选，与分解族互斥形态）。
    Purchase,
}

/// 「记一笔」入口闭集（类型菜单七项的载荷；借贷入口预置方向——旧壳
/// LENDING_CREATE_DIRECTIONS = 借出/借入，收回/还款经表单内方向切换到达；
/// 出资分解/购买项下单是表单③首建入口，spec #2031 授权——旧壳创建菜单无
/// 此两项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateEntry {
    /// 记支出。
    Expense,
    /// 记收入。
    Income,
    /// 记转账。
    Transfer,
    /// 借出（预置方向）。
    Lend,
    /// 借入（预置方向）。
    Borrow,
    /// 出资分解（表单③；kind 锁定支出，字段族无主账户位）。
    Funding,
    /// 购买项下单（表单③；kind 锁定支出，订单 + 商品行）。
    Purchase,
}

/// 出资项子行状态（每行独立信号束；删除 = 从行集移除、行键恒定不重排——
/// 渲染 keyed 随键卸载，headless 锚点随行键稳定寻址）。
#[derive(Clone)]
pub(crate) struct FundingRowState {
    /// 行键（进程内递增；渲染 keyed 与 headless 锚点共用）。
    pub key: u64,
    /// 出资账户。
    pub account_id: Signal<Option<String>>,
    /// 出资金额原始文本。
    pub amount_text: Signal<String>,
    /// 扣款标签。
    pub label: Signal<String>,
}

impl PartialEq for FundingRowState {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.account_id.get() == other.account_id.get()
            && self.amount_text.get() == other.amount_text.get()
            && self.label.get() == other.label.get()
    }
}

/// 购买项商品行状态（每行独立信号束；形态同 [`FundingRowState`]）。
#[derive(Clone)]
pub(crate) struct PurchaseRowState {
    /// 行键（进程内递增；渲染 keyed 与 headless 锚点共用）。
    pub key: u64,
    /// 商品名称。
    pub name: Signal<String>,
    /// 件数原始文本。
    pub quantity_text: Signal<String>,
    /// 商品分类（可选）。
    pub category_id: Signal<Option<String>>,
    /// 单价原始文本（可空）。
    pub price_text: Signal<String>,
}

impl PartialEq for PurchaseRowState {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.name.get() == other.name.get()
            && self.quantity_text.get() == other.quantity_text.get()
            && self.category_id.get() == other.category_id.get()
            && self.price_text.get() == other.price_text.get()
    }
}

/// 退款目标快照（原支出行的呈现面快照，fixed-target 入口随行携带）：编辑面
/// 的原支出行不可得（多半不在当前页）时不呈现原交易行，提交引用仍随行透传
/// （关联语义不受呈现面影响）。
#[derive(Clone, Default)]
pub(crate) struct RefundTarget(pub(crate) Option<ledger_transaction::Transaction>);

impl PartialEq for RefundTarget {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.id == b.id
                    && a.amount_cents == b.amount_cents
                    && a.currency_code == b.currency_code
                    && a.account_id == b.account_id
                    && a.category_id == b.category_id
                    && a.date == b.date
            }
            _ => false,
        }
    }
}

/// 表单参考数据源行集投影（signal 值域要求 `PartialEq`——nsh 反应式等值通
/// 知；域行类型未实现全量相等，投影比较按表单消费字段全等：任一消费字段
/// 不同即不等，语义等同「表单源变化即通知」）。
#[derive(Clone, Default)]
pub(crate) struct TxItems(pub(crate) Vec<ledger_transaction::Transaction>);

impl PartialEq for TxItems {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.kind == b.kind
                    && a.amount_cents == b.amount_cents
                    && a.currency_code == b.currency_code
                    && a.account_id == b.account_id
                    && a.to_account_id == b.to_account_id
                    && a.refund_of_transaction_id == b.refund_of_transaction_id
                    && a.category_id == b.category_id
                    && a.merchant_id == b.merchant_id
                    && a.policy_id == b.policy_id
                    && a.note == b.note
                    && a.date == b.date
                    && a.source_order_no == b.source_order_no
            })
    }
}

/// 账户源投影（id / 名 / 币种 / 类型 / 归档位——候选面、币种推导与借贷侧别
/// 过滤的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct FormAccounts(pub(crate) Vec<ledger_accounts::Account>);

impl PartialEq for FormAccounts {
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

/// 分类源投影（id / kind / 父引用 / 名 / 序——两树选项面的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct FormCategories(pub(crate) Vec<ledger_categories::Category>);

impl PartialEq for FormCategories {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.kind == b.kind
                    && a.parent_id == b.parent_id
                    && a.name == b.name
                    && a.sort_order == b.sort_order
            })
    }
}

/// 商户源投影（id / 名 / 软删位——建议面与软删兜底解析的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct FormMerchants(pub(crate) Vec<ledger_merchants::Merchant>);

impl PartialEq for FormMerchants {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(&other.0)
                .all(|(a, b)| a.id == b.id && a.name == b.name && a.is_deleted == b.is_deleted)
    }
}

/// 保单源投影（id / 保单号 / 险种——选项面的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct FormPolicies(pub(crate) Vec<ledger_policy::Policy>);

impl PartialEq for FormPolicies {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.policy_number == b.policy_number
                    && a.product_name == b.product_name
            })
    }
}

/// 表单信号束（住调用方视图实例；Copy 句柄可随意移入闭包）。缺省关窗、基础
/// 族、支出 kind、今天日期。
#[derive(Clone, Copy)]
pub(crate) struct TxnFormState {
    /// 弹窗开合（受控；关窗统一置回——模态封装消费）。
    pub open: Signal<bool>,
    /// 表单形态族（字段族重排选择器；入口/编辑分派锁定）。
    pub family: Signal<FormFamily>,
    /// 提交 kind（创建入口锁定 / 编辑按行锁定；表单内无切换控件——旧壳
    /// #150「类型由入口单点表达，中途换类型 = 关闭重开」同语义）。
    pub kind: Signal<ledger_transaction::TransactionKind>,
    /// 创建入口 kind 词（标题「记一笔 · {词}」静态随入口——旧壳 createTitle
    /// 读入口意图同语义，表单内方向切换不重写标题）。
    pub entry_label: Signal<&'static str>,
    /// 编辑目标交易 id（None = 创建模式）。
    pub editing_id: Signal<Option<String>>,
    /// 编辑透传锚点：来源订单号（表单无输入位，全字段替换下缺省即静默清除，
    /// 旧壳同语义——编辑路径原样回传行上原值）。
    pub source_order_no: Signal<Option<String>>,
    /// 编辑原商户 id（「未改动」语义的比对锚点；None = 创建模式）。
    pub editing_merchant_id: Signal<Option<String>>,
    /// 金额原始文本（判定族消费的原样输入，不拦截不静默丢弃）。
    pub amount_text: Signal<String>,
    /// 金额红态已启用（失焦或提交尝试后生效——空值不抢红，ADR-0058 决策 2）。
    pub amount_marked: Signal<bool>,
    /// 账户（基础收支记账账户 / 转账借贷转出账户 / 退款继承原支出账户的锁
    /// 定呈现面）。
    pub account_id: Signal<Option<String>>,
    /// 转入账户（转账/借贷双账户对；基础收支与退款不用）。
    pub to_account_id: Signal<Option<String>>,
    /// 借贷方向（借贷变体表单状态；方向切换处置见
    /// [`assembly::lending_direction_swap`]）。
    pub lending_direction: Signal<assembly::LendingFormDirection>,
    /// 分类（可选；None = 无分类交易。基础收支族消费）。
    pub category_id: Signal<Option<String>>,
    /// 商户输入文本（自由文本语义的原样承载；基础收支与借贷族渲染输入位）。
    pub merchant_typed: Signal<String>,
    /// 商户选定建议项 id（键入即失效——[`combo_select`] 双态值语义）。
    pub merchant_selected: Signal<Option<String>>,
    /// 保单（可选挂单。基础收支族消费；退款不挂单 ADR-0051 决策 4）。
    pub policy_id: Signal<Option<String>>,
    /// 备注。
    pub note: Signal<String>,
    /// 交易日期（缺省今天，旧壳 NDatePicker 缺省同语义）。
    pub date: Signal<chrono::NaiveDate>,
    /// 退款原支出引用（提交面：创建随目标、编辑透传行上原值——关联语义不
    /// 可被编辑破坏）。
    pub refund_of_id: Signal<Option<String>>,
    /// 退款目标快照（原交易呈现面；见 [`RefundTarget`]）。
    pub refund_target: Signal<RefundTarget>,
    /// 退款币种（继承原支出——创建随目标、编辑随行，只读呈现）。
    pub refund_currency: Signal<String>,
    /// 出资项子行（表单③出资分解族；创建入口默认单行、编辑随行回填）。
    pub funding_rows: Signal<Vec<FundingRowState>>,
    /// 购买项商品行（表单③购买项族默认空行集；出资分解族编辑并存行时回填）。
    pub purchase_rows: Signal<Vec<PurchaseRowState>>,
    /// 子行键计数器（递增不回收；两区共用）。
    pub row_seq: Signal<u64>,
    /// 提交错误条位（业务校验与域码化错误同位呈现；下次尝试开始时清空）。
    pub error: Signal<Option<String>>,
}

/// 表单数据源束（调用方视图持有的参考数据信号；装载刷新与表单共享同一来源）。
#[derive(Clone, Copy)]
pub(crate) struct TxnFormSources {
    /// 可见账户行（含归档——候选面在挂载时滤归档，编辑面补值消费全行）。
    pub accounts: Signal<FormAccounts>,
    /// 在用分类行（[`crate::boot::category_rows`] 口径）。
    pub categories: Signal<FormCategories>,
    /// 含软删全量商户行（编辑面软删兜底的解析来源）。
    pub merchants: Signal<FormMerchants>,
    /// 未删除保单行（[`crate::boot::policy_rows`] 口径）。
    pub policies: Signal<FormPolicies>,
    /// 币种字典（退款原交易金额文本的展示换算消费）。
    pub currencies: Signal<CurrencyTable>,
}

impl TxnFormState {
    /// 表单信号束（缺省关窗、基础族、支出 kind、今天日期）。
    pub(crate) fn new() -> Self {
        Self {
            open: signal(false),
            family: signal(FormFamily::Basic),
            kind: signal(ledger_transaction::TransactionKind::Expense),
            entry_label: signal("支出"),
            editing_id: signal(None),
            source_order_no: signal(None),
            editing_merchant_id: signal(None),
            amount_text: signal(String::new()),
            amount_marked: signal(false),
            account_id: signal(None),
            to_account_id: signal(None),
            lending_direction: signal(assembly::LendingFormDirection::Lend),
            category_id: signal(None),
            merchant_typed: signal(String::new()),
            merchant_selected: signal(None),
            policy_id: signal(None),
            note: signal(String::new()),
            date: signal(chrono::Local::now().date_naive()),
            refund_of_id: signal(None),
            refund_target: signal(RefundTarget(None)),
            refund_currency: signal("CNY".to_owned()),
            funding_rows: signal(Vec::new()),
            purchase_rows: signal(Vec::new()),
            row_seq: signal(0),
            error: signal(None),
        }
    }

    /// 新出资项空行（键取递增计数器；缺省金额空、账户未选）。
    fn new_funding_row(&self) -> FundingRowState {
        let key = self.row_seq.get();
        self.row_seq.set(key + 1);
        FundingRowState {
            key,
            account_id: signal(None),
            amount_text: signal(String::new()),
            label: signal(String::new()),
        }
    }

    /// 新购买项空行（键取递增计数器；各字段空初值——与表单其他字段一致，
    /// 忘填由装配器 fail fast 阻拦）。
    fn new_purchase_row(&self) -> PurchaseRowState {
        let key = self.row_seq.get();
        self.row_seq.set(key + 1);
        PurchaseRowState {
            key,
            name: signal(String::new()),
            quantity_text: signal(String::new()),
            category_id: signal(None),
            price_text: signal(String::new()),
        }
    }

    /// 全字段回默认态（open_* 三入口共享——旧壳「开窗重建实例」同语义）。
    fn reset(&self) {
        self.family.set(FormFamily::Basic);
        self.kind.set(ledger_transaction::TransactionKind::Expense);
        self.entry_label.set("支出");
        self.editing_id.set(None);
        self.source_order_no.set(None);
        self.editing_merchant_id.set(None);
        self.amount_text.set(String::new());
        self.amount_marked.set(false);
        self.account_id.set(None);
        self.to_account_id.set(None);
        self.lending_direction
            .set(assembly::LendingFormDirection::Lend);
        self.category_id.set(None);
        self.merchant_typed.set(String::new());
        self.merchant_selected.set(None);
        self.policy_id.set(None);
        self.note.set(String::new());
        self.date.set(chrono::Local::now().date_naive());
        self.refund_of_id.set(None);
        self.refund_target.set(RefundTarget(None));
        self.refund_currency.set("CNY".to_owned());
        self.funding_rows.set(Vec::new());
        self.purchase_rows.set(Vec::new());
        self.error.set(None);
    }

    /// 打开新增表单（kind 由入口锁定；全部字段回默认态）。
    pub(crate) fn open_create(&self, entry: CreateEntry) {
        self.reset();
        match entry {
            CreateEntry::Expense => {}
            CreateEntry::Income => {
                self.kind.set(ledger_transaction::TransactionKind::Income);
                self.entry_label.set("收入");
            }
            CreateEntry::Transfer => {
                self.family.set(FormFamily::Transfer);
                self.kind.set(ledger_transaction::TransactionKind::Transfer);
                self.entry_label.set("转账");
            }
            CreateEntry::Lend => {
                self.family.set(FormFamily::Lending);
                self.kind.set(ledger_transaction::TransactionKind::Transfer);
                self.entry_label.set(assembly::lending_direction_label(
                    assembly::LendingFormDirection::Lend,
                ));
            }
            CreateEntry::Borrow => {
                self.family.set(FormFamily::Lending);
                self.kind.set(ledger_transaction::TransactionKind::Transfer);
                self.lending_direction
                    .set(assembly::LendingFormDirection::Borrow);
                self.entry_label.set(assembly::lending_direction_label(
                    assembly::LendingFormDirection::Borrow,
                ));
            }
            CreateEntry::Funding => {
                // 出资分解（表单③）：kind 锁定支出，初始单行（直接填两账户
                // 分摊只需一次加行）。
                self.family.set(FormFamily::Funding);
                self.entry_label.set("出资分解");
                let row = self.new_funding_row();
                self.funding_rows.set(vec![row]);
            }
            CreateEntry::Purchase => {
                // 购买项下单（表单③）：kind 锁定支出，初始单行（打开即录商
                // 品；删至零行保存 = 无明细的普通支出，增删行是自由形态）。
                self.family.set(FormFamily::Purchase);
                self.entry_label.set("购买项下单");
                let row = self.new_purchase_row();
                self.purchase_rows.set(vec![row]);
            }
        }
        self.open.set(true);
    }

    /// 打开退款表单（fixed-target 入口：原支出由右键所在行固定；金额缺省
    /// 回填原交易金额——「金额钳制」的入口形态，可改；日期缺省今天，旧壳
    /// useRefundForm 同语义）。
    pub(crate) fn open_create_refund(
        &self,
        target: &ledger_transaction::Transaction,
        currencies: &CurrencyTable,
    ) {
        self.reset();
        self.family.set(FormFamily::Refund);
        self.kind.set(ledger_transaction::TransactionKind::Refund);
        self.entry_label.set("退款");
        self.refund_of_id.set(Some(target.id.clone()));
        self.refund_target.set(RefundTarget(Some(target.clone())));
        self.refund_currency.set(target.currency_code.clone());
        // 金额回填：分 → 元文本按原交易币种小数位（判定必为合法、不显红态）。
        let fmt = currencies.format_of(&target.currency_code);
        self.amount_text.set(assembly::cents_to_amount_text(
            target.amount_cents,
            fmt.decimal_places,
        ));
        self.account_id.set(target.account_id.clone());
        self.open.set(true);
    }

    /// 打开编辑表单（回填既有行全部业务字段；kind 与形态族随行分派——转账
    /// 行按两端账户类型派生借贷方向进借贷变体，旧壳 TransactionForm 分派同
    /// 语义）。`merchant_label` 是原商户的显示名（含软删全量口径解析；None
    /// = 行上无商户引用）；`accounts` 是可见账户行集（借贷方向派生的账户类
    /// 型解析源）。界面只读 kind（convert/split/dividend 等）不开放本入口，
    /// 防御性 no-op 不开窗。
    pub(crate) fn open_edit(
        &self,
        tx: &ledger_transaction::Transaction,
        currencies: &CurrencyTable,
        merchant_label: Option<String>,
        accounts: &[ledger_accounts::Account],
    ) {
        let table = assembly::AccountTable::from_accounts(accounts.iter());
        let Some((family, direction)) = edit_dispatch(tx, &table) else {
            return;
        };
        self.reset();
        self.kind.set(tx.kind);
        self.editing_id.set(Some(tx.id.clone()));
        self.source_order_no.set(tx.source_order_no.clone());
        self.editing_merchant_id.set(tx.merchant_id.clone());
        // 金额回填：分 → 元文本按行币种小数位（不手写 /100；旧壳 centsToYuan
        // 同口径），判定必为合法、不显红态。
        let fmt = currencies.format_of(&tx.currency_code);
        self.amount_text.set(assembly::cents_to_amount_text(
            tx.amount_cents,
            fmt.decimal_places,
        ));
        self.account_id.set(tx.account_id.clone());
        self.note.set(tx.note.clone().unwrap_or_default());
        // 日期回填：行上 ISO 文本解析；解析失败保持今天（数据侧契约文本恒可
        // 解析，防御兜底不静默造错值）。
        self.date.set(
            chrono::NaiveDate::parse_from_str(&tx.date, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::Local::now().date_naive()),
        );
        self.family.set(family);
        self.to_account_id.set(tx.to_account_id.clone());
        // 商户随行保留（普通转账表单无输入位；基础收支/分解/订单族回填输入位）。
        self.merchant_selected.set(tx.merchant_id.clone());
        self.merchant_typed.set(merchant_label.unwrap_or_default());
        match family {
            FormFamily::Basic => {
                self.category_id.set(tx.category_id.clone());
                self.policy_id.set(tx.policy_id.clone());
            }
            FormFamily::Transfer => {
                // 普通转账编辑：无输入位，选定面保持行上原值——提交经
                // merchant_ref 命中「未改动」语义（ADR-0092 决策 5 形态退化
                // 不静默清数据）；输入文本回空避免双态值语义混入。
                self.merchant_typed.set(String::new());
            }
            FormFamily::Lending => {
                self.lending_direction
                    .set(direction.unwrap_or(assembly::LendingFormDirection::Lend));
            }
            FormFamily::Refund => {
                // 原支出引用透传（关联语义不可被编辑破坏）；原交易快照呈现
                // 面不可得（原支出行多半不在当前页），不渲染原交易行。
                self.refund_of_id.set(tx.refund_of_transaction_id.clone());
                self.refund_currency.set(tx.currency_code.clone());
            }
            FormFamily::Funding => {
                // 出资项子行回填（逐行账户/金额/标签；金额分→元文本按行币种
                // 小数位）；并存购买项一并回填（分解行可携带购买项，ADR-0138
                // 决策 15 呈现形态）；分类/保单随行。
                self.category_id.set(tx.category_id.clone());
                self.policy_id.set(tx.policy_id.clone());
                self.funding_rows.set(
                    tx.fundings
                        .iter()
                        .map(|f| {
                            let row = self.new_funding_row();
                            row.account_id.set(Some(f.account_id.clone()));
                            row.amount_text.set(assembly::cents_to_amount_text(
                                f.amount_cents,
                                fmt.decimal_places,
                            ));
                            row.label.set(f.label.clone().unwrap_or_default());
                            row
                        })
                        .collect(),
                );
                self.backfill_purchase_rows(tx, fmt.decimal_places);
            }
            FormFamily::Purchase => {
                // 订单族：主账户/分类/商户/保单随行，商品行逐行回填。
                self.category_id.set(tx.category_id.clone());
                self.policy_id.set(tx.policy_id.clone());
                self.backfill_purchase_rows(tx, fmt.decimal_places);
            }
        }
        self.error.set(None);
        self.open.set(true);
    }

    /// 商品行回填（表单③两族共用；空行集不建行）。
    fn backfill_purchase_rows(&self, tx: &ledger_transaction::Transaction, decimal_places: u32) {
        self.purchase_rows.set(
            tx.purchases
                .iter()
                .map(|p| {
                    let row = self.new_purchase_row();
                    row.name.set(p.name.clone());
                    row.quantity_text.set(p.quantity.to_string());
                    row.category_id.set(p.category_id.clone());
                    row.price_text.set(
                        p.unit_price_cents
                            .map(|cents| assembly::cents_to_amount_text(cents, decimal_places))
                            .unwrap_or_default(),
                    );
                    row
                })
                .collect(),
        );
    }

    /// 关窗（ESC / ✕ / 取消 / 成功四路径归一）。
    pub(crate) fn close(&self) {
        self.open.set(false);
    }
}

/// 编辑分派（纯函数，旧壳 TransactionForm 形态识别的单点重生）：交易行 +
/// 账户表 →（形态族，借贷方向预置）。收入/支出按子行形态分流——分解行进
/// 出资分解族、购买项行进购买项族、并存行进出资分解族（叠加购买项区）、无
/// 子行进基础族（表单③开放子行编辑后的全形态分派）；转账按两端账户类型
/// 派生借贷方向——派生非 none 进借贷变体（方向随派生），否则普通转账族；
/// 退款进退款族（读侧派生分解不落库，无静默清空面）；界面只读 kind 返回
/// None（入口开放面守卫的镜像，防御双落点——调用方不开窗）。
fn edit_dispatch(
    tx: &ledger_transaction::Transaction,
    accounts: &assembly::AccountTable,
) -> Option<(FormFamily, Option<assembly::LendingFormDirection>)> {
    use ledger_transaction::TransactionKind as K;
    match tx.kind {
        K::Income | K::Expense => {
            if !tx.fundings.is_empty() {
                Some((FormFamily::Funding, None))
            } else if !tx.purchases.is_empty() {
                Some((FormFamily::Purchase, None))
            } else {
                Some((FormFamily::Basic, None))
            }
        }
        K::Transfer => match assembly::derive_lending_direction(tx, accounts) {
            assembly::LendingDirection::None => Some((FormFamily::Transfer, None)),
            derived => Some((FormFamily::Lending, Some(form_direction(derived)))),
        },
        K::Refund => Some((FormFamily::Refund, None)),
        _ => None,
    }
}

/// 方向派生五态 → 借贷表单方向（表单分派专用；none 已在分派侧分流）。
fn form_direction(direction: assembly::LendingDirection) -> assembly::LendingFormDirection {
    match direction {
        assembly::LendingDirection::Lend => assembly::LendingFormDirection::Lend,
        assembly::LendingDirection::Collect => assembly::LendingFormDirection::Collect,
        assembly::LendingDirection::Borrow => assembly::LendingFormDirection::Borrow,
        assembly::LendingDirection::Repay => assembly::LendingFormDirection::Repay,
        assembly::LendingDirection::None => assembly::LendingFormDirection::Lend,
    }
}

/// 账户候选投影（可见 − 归档 + 编辑面补值——已存值恰是归档账户或越侧/越币
/// 种存量脏行时补回选项并标「已归档」，值原样保留，spec #1952 / ADR-0142 决
/// 策 4）。`keep` 是形态过滤器：基础收支/转账转出恒真，借贷按方向侧别，转
/// 账转入按币种收窄。
pub(crate) fn pinned_account_options(
    rows: &[ledger_accounts::Account],
    selected: Option<&str>,
    keep: impl Fn(&ledger_accounts::Account) -> bool,
) -> Vec<CategoryOption> {
    let mut options: Vec<CategoryOption> = rows
        .iter()
        .filter(|a| !a.is_archived && keep(a))
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && !options.iter().any(|o| o.id == id)
        && let Some(row) = rows.iter().find(|a| a.id == id)
    {
        options.push(CategoryOption {
            id: row.id.clone(),
            label: format!("{}（已归档）", row.name),
        });
    }
    options
}

/// 商户语义推导（表单选择器双态值 → [`MerchantRef`] 闭集）：选定建议项按
/// 「未改动 / 改选」分流（后端 existing_merchant_id unchanged 语义）；无选定
/// 按文本分流（空 = 不挂，非空 = 自由文本即建/复用——域内单点）。纯函数。
pub(crate) fn merchant_ref(
    editing_merchant_id: Option<&str>,
    selected: Option<&str>,
    typed: &str,
) -> MerchantRef {
    let typed = typed.trim();
    match (editing_merchant_id, selected) {
        (Some(orig), Some(id)) if id == orig => MerchantRef::Keep(orig.to_owned()),
        (orig, Some(id)) => {
            let _ = orig;
            MerchantRef::Selected(id.to_owned())
        }
        (_, None) if typed.is_empty() => MerchantRef::None,
        (_, None) => MerchantRef::NewName(typed.to_owned()),
    }
}

/// 普通转账的商户语义（表单无输入位——创建恒不挂，编辑原样携带行上原值，
/// ADR-0092 决策 5 形态退化不静默清数据）。
fn transfer_merchant(editing_merchant_id: Option<&str>) -> MerchantRef {
    match editing_merchant_id {
        Some(id) => MerchantRef::Keep(id.to_owned()),
        None => MerchantRef::None,
    }
}

/// 金额红态判定（纯函数）：判定非 `Ok` 且已标记（失焦或提交尝试）才红——
/// 旧壳 useFieldErrors 时机语义；初始为空不抢红（ADR-0058 决策 2）。
fn amount_error_state(judgment: assembly::AmountJudgment, marked: bool) -> bool {
    !matches!(judgment, assembly::AmountJudgment::Ok(_)) && marked
}

/// 金额红态判定（信号读取面）。
fn amount_has_error(state: &TxnFormState) -> bool {
    amount_error_state(
        assembly::judge_amount_text(&state.amount_text.get()),
        state.amount_marked.get(),
    )
}

/// 提交装配（形态族分派 → 对应装配器收口）：业务校验 fail fast 词表 / wire
/// 矩阵全部归 [`crate::assembly`] 三装配器，本面只搬运表单状态。错误为
/// zh-CN 词表文本（错误条位呈现）。
fn assemble_input(
    state: &TxnFormState,
    sources: &TxnFormSources,
) -> Result<ledger_transaction::TransactionInput, String> {
    let currency = if state.family.get() == FormFamily::Refund {
        state.refund_currency.get()
    } else {
        selected_currency(state, sources)
    };
    match state.family.get() {
        FormFamily::Basic => {
            let submit = BasicFormSubmit {
                kind: state.kind.get(),
                amount_text: state.amount_text.get(),
                account_id: state.account_id.get(),
                category_id: state.category_id.get(),
                merchant: merchant_ref(
                    state.editing_merchant_id.get().as_deref(),
                    state.merchant_selected.get().as_deref(),
                    &state.merchant_typed.get(),
                ),
                policy_id: state.policy_id.get(),
                note: state.note.get(),
                date: state.date.get(),
                source_order_no: state.source_order_no.get(),
            };
            assembly::assemble_basic_transaction(submit, currency)
                .map_err(|e| e.as_text().to_owned())
        }
        FormFamily::Transfer | FormFamily::Lending => {
            let merchant = if state.family.get() == FormFamily::Transfer {
                transfer_merchant(state.editing_merchant_id.get().as_deref())
            } else {
                merchant_ref(
                    state.editing_merchant_id.get().as_deref(),
                    state.merchant_selected.get().as_deref(),
                    &state.merchant_typed.get(),
                )
            };
            let submit = assembly::TransferFormSubmit {
                amount_text: state.amount_text.get(),
                account_id: state.account_id.get(),
                to_account_id: state.to_account_id.get(),
                merchant,
                note: state.note.get(),
                date: state.date.get(),
                source_order_no: state.source_order_no.get(),
            };
            assembly::assemble_transfer_transaction(submit, currency)
                .map_err(|e| e.as_text().to_owned())
        }
        FormFamily::Refund => {
            let submit = assembly::RefundFormSubmit {
                amount_text: state.amount_text.get(),
                refund_of_transaction_id: state.refund_of_id.get(),
                account_id: state.account_id.get(),
                note: state.note.get(),
                date: state.date.get(),
                source_order_no: state.source_order_no.get(),
            };
            assembly::assemble_refund_transaction(submit, currency)
                .map_err(|e| e.as_text().to_owned())
        }
        FormFamily::Funding => {
            let submit = assembly::FundingFormSubmit {
                kind: state.kind.get(),
                amount_text: state.amount_text.get(),
                rows: state
                    .funding_rows
                    .get()
                    .iter()
                    .map(|row| assembly::FundingRowSubmit {
                        account_id: row.account_id.get(),
                        amount_text: row.amount_text.get(),
                        label: row.label.get(),
                    })
                    .collect(),
                purchases: purchase_submits(state),
                category_id: state.category_id.get(),
                merchant: merchant_ref(
                    state.editing_merchant_id.get().as_deref(),
                    state.merchant_selected.get().as_deref(),
                    &state.merchant_typed.get(),
                ),
                policy_id: state.policy_id.get(),
                note: state.note.get(),
                date: state.date.get(),
                source_order_no: state.source_order_no.get(),
            };
            assembly::assemble_funding_transaction(submit, currency)
                .map_err(|e| e.as_text().to_owned())
        }
        FormFamily::Purchase => {
            let submit = assembly::PurchaseFormSubmit {
                amount_text: state.amount_text.get(),
                account_id: state.account_id.get(),
                rows: purchase_submits(state),
                category_id: state.category_id.get(),
                merchant: merchant_ref(
                    state.editing_merchant_id.get().as_deref(),
                    state.merchant_selected.get().as_deref(),
                    &state.merchant_typed.get(),
                ),
                policy_id: state.policy_id.get(),
                note: state.note.get(),
                date: state.date.get(),
                source_order_no: state.source_order_no.get(),
            };
            assembly::assemble_purchase_transaction(submit, currency)
                .map_err(|e| e.as_text().to_owned())
        }
    }
}

/// 商品行表单状态 → 提交载荷（表单③两族共用）。
fn purchase_submits(state: &TxnFormState) -> Vec<assembly::PurchaseRowSubmit> {
    state
        .purchase_rows
        .get()
        .iter()
        .map(|row| assembly::PurchaseRowSubmit {
            name: row.name.get(),
            quantity_text: row.quantity_text.get(),
            category_id: row.category_id.get(),
            price_text: row.price_text.get(),
        })
        .collect()
}

/// 提交执行（前置校验 → 域公开写入口直调 → 成功广播失效信号；失败错误条位）。
/// 任务挂 `view_owner`（弹窗子树卸载不取消，交易删除先例同形）。
fn perform_submit(
    state: TxnFormState,
    sources: TxnFormSources,
    view_owner: Owner,
) -> impl Fn() + Copy + 'static {
    move || {
        // 域码化错误 zh-CN 原样呈现（错误条位；下次尝试开始时清空）。
        state.error.set(None);
        state.amount_marked.set(true);
        // 装配器 fail fast（账户/同账户/金额业务校验与换算兜底；红态拦截是
        // 第一道防线，错误条位是本拦截的呈现位）。
        let input = match assemble_input(&state, &sources) {
            Ok(input) => input,
            Err(text) => {
                state.error.set(Some(text));
                return;
            }
        };
        let editing = state.editing_id.get();
        let was_edit = editing.is_some();
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match (db.as_ref(), editing.as_deref()) {
                    (Some(db), None) => boot::create_transaction(db, input)
                        .map(|_: ledger_transaction::TransactionWrite| ()),
                    (Some(db), Some(id)) => boot::update_transaction(db, id, input)
                        .map(|_: ledger_infra::signals::WriteEvidence| ()),
                    (None, _) => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        state.close();
                        // 失效信号广播（「写路径后刷新」的广播接线点；即建
                        // 商户证据经列表重读的商户行集刷新自然覆盖——旧壳
                        // write_entry 参考失效信号的壳内等价物）。删除即红
                        // 条目钉在 tests/app_headless.rs。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => {
                        let prefix = if was_edit {
                            "保存失败"
                        } else {
                            "记账失败"
                        };
                        state.error.set(Some(format!("{prefix}: {e}")));
                    }
                    Err(_) => state
                        .error
                        .set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    }
}

/// 币种随所选账户推导（基础收支/订单记账币种 = 账户币种；转账记账币种 = 转
/// 出账户币种，ADR-0134 决策 5；出资分解 = 首条出资账户币种——域准入要求逐
/// 条与交易币种一致，多账户跨币种不做；无账户缺省 CNY——默认币种缺 key 回默
/// 认 CNY 同口径，有账户上下文的提交必选账户）。
fn selected_currency(state: &TxnFormState, sources: &TxnFormSources) -> String {
    let account_id = if state.family.get() == FormFamily::Funding {
        state
            .funding_rows
            .get()
            .first()
            .and_then(|row| row.account_id.get())
    } else {
        state.account_id.get()
    };
    account_id
        .as_deref()
        .and_then(|id| {
            sources
                .accounts
                .get()
                .0
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.currency_code.clone())
        })
        .unwrap_or_else(|| "CNY".to_owned())
}

/// 表单字段标签（弱化小字；预算视图同形小工具——封装层尚无 form 原语，
/// 不为此立新封装件）。
fn field_label(label: &'static str, muted: AdaptiveColor) -> impl IntoElement {
    text(label).text_size(px(12.0)).text_color(muted)
}

/// 弹窗标题（创建「记一笔 · {入口词}」/ 编辑「编辑交易」——旧壳词表同形）。
fn modal_title(editing: bool, entry_label: &str) -> String {
    if editing {
        "编辑交易".to_owned()
    } else {
        format!("记一笔 · {entry_label}")
    }
}

/// 提交按钮文案（创建 = 记{类型词}——借贷随当前方向；编辑 = 保存修改——
/// 旧壳词表同形）。纯函数（信号读取面归调用方）。
fn submit_label(
    family: FormFamily,
    kind: ledger_transaction::TransactionKind,
    direction: assembly::LendingFormDirection,
    editing: bool,
) -> String {
    if editing {
        return "保存修改".to_owned();
    }
    match family {
        FormFamily::Basic => match kind {
            ledger_transaction::TransactionKind::Income => "记收入",
            _ => "记支出",
        }
        .to_owned(),
        FormFamily::Transfer => "记转账".to_owned(),
        FormFamily::Lending => format!("记{}", assembly::lending_direction_label(direction)),
        FormFamily::Refund => "记退款".to_owned(),
        FormFamily::Funding => "记支出".to_owned(),
        FormFamily::Purchase => "记订单".to_owned(),
    }
}

/// 方向切换钮的锚点（headless `id:` 选择器）。
fn direction_test_id(direction: assembly::LendingFormDirection) -> &'static str {
    match direction {
        assembly::LendingFormDirection::Lend => "tx-form-dir-lend",
        assembly::LendingFormDirection::Collect => "tx-form-dir-collect",
        assembly::LendingFormDirection::Borrow => "tx-form-dir-borrow",
        assembly::LendingFormDirection::Repay => "tx-form-dir-repay",
    }
}

/// 表单弹窗挂载面（调用方视图树内消费；`view_owner` 是调用方视图实例 owner
/// ——写任务经它挂载，弹窗子树卸载不取消）。
pub(crate) fn transaction_form_modal(
    state: TxnFormState,
    sources: TxnFormSources,
    view_owner: Owner,
) -> impl IntoElement {
    // 账户候选面（基础收支记账账户 / 转账借贷转出账户共用锚点）：可见 − 归
    // 档；借贷变体按方向转出侧过滤（资金侧 or 应收/负债侧随方向响应式）。
    let account_options = computed(move || {
        let rows = sources.accounts.get().0;
        if state.family.get() == FormFamily::Lending {
            let (from_side, _) = assembly::lending_direction_sides(state.lending_direction.get());
            pinned_account_options(&rows, state.account_id.get().as_deref(), |a| {
                assembly::account_matches_side(Some(a.kind), from_side)
            })
        } else {
            pinned_account_options(&rows, state.account_id.get().as_deref(), |_| true)
        }
    });
    // 转入账户候选面：转账与转出账户同币种（跨币种转账不做，ADR-0134 决策
    // 4；未选转出前不收窄——两端选择顺序自由）；借贷按方向转入侧过滤。补值
    // 覆盖归档与存量脏行（币种不一致/越侧的历史行原样保留，见 pinned 计）。
    let to_account_options = computed(move || {
        let rows = sources.accounts.get().0;
        match state.family.get() {
            FormFamily::Lending => {
                let (_, to_side) = assembly::lending_direction_sides(state.lending_direction.get());
                pinned_account_options(&rows, state.to_account_id.get().as_deref(), |a| {
                    assembly::account_matches_side(Some(a.kind), to_side)
                })
            }
            _ => {
                let narrowed = state.account_id.get().is_some();
                let currency = selected_currency(&state, &sources);
                pinned_account_options(&rows, state.to_account_id.get().as_deref(), |a| {
                    !narrowed || a.currency_code == currency
                })
            }
        }
    });
    // 分类候选面：随 kind 过滤（支出/收入两树，路径名口径单点在装配层）。
    let category_options = computed(move || {
        let kind = match state.kind.get() {
            ledger_transaction::TransactionKind::Income => "income",
            _ => "expense",
        };
        assembly::category_kind_options(kind, &sources.categories.get().0)
    });
    // 商户建议面：在用商户（软删不进新选择）+ 编辑面软删兜底选项承载原 id
    // （裸 uuid 不可读，提交按「未改动」语义原样保留——旧壳同语义）。
    let merchant_options = computed(move || {
        let rows = sources.merchants.get().0;
        let mut options: Vec<CategoryOption> = rows
            .iter()
            .filter(|m| !m.is_deleted)
            .map(|m| CategoryOption {
                id: m.id.clone(),
                label: m.name.clone(),
            })
            .collect();
        if let Some(orig) = state.editing_merchant_id.get()
            && !rows.iter().any(|m| m.id == orig && !m.is_deleted)
            && rows.iter().any(|m| m.id == orig)
        {
            options.insert(
                0,
                CategoryOption {
                    id: orig,
                    label: "（已删除商户）".to_owned(),
                },
            );
        }
        options
    });
    // 保单选项面：未删除保单（保单号为主、险种辅佐——保单号是用户认单的自然
    // 键）+ 编辑面软删兜底选项（旧壳同语义）。
    let policy_options = computed(move || {
        let rows = sources.policies.get().0;
        let mut options: Vec<CategoryOption> = rows
            .iter()
            .map(|p| CategoryOption {
                id: p.id.clone(),
                label: format!("{}（{}）", p.policy_number, p.product_name),
            })
            .collect();
        if let Some(orig) = state.policy_id.get()
            && !rows.iter().any(|p| p.id == orig)
        {
            options.insert(
                0,
                CategoryOption {
                    id: orig,
                    label: "（已删除保单）".to_owned(),
                },
            );
        }
        options
    });
    // 币种展示（基础收支/转账随账户推导锁定只读——后端 Writer 守卫是唯一权
    // 威，前端只读呈现；退款随继承原支出只读）。
    let currency_text = move || {
        if state.family.get() == FormFamily::Refund {
            state.refund_currency.get()
        } else {
            selected_currency(&state, &sources)
        }
    };
    // 金额红态（判定族单点；红框 + 提交禁用两件同发，ADR-0058 决策 1）。
    let amount_error = move || amount_has_error(&state);
    // 两端币种不一致（转账形态的存量脏行编辑回填态：候选过滤已挡住新增路
    // 径）——即时红显阻止提交，保存前须把两端改为同币种（后端 Writer 守卫
    // 是契约兜底；旧壳 ADR-0134 决策 5 同语义，字段红显的文案条位是本壳等
    // 价呈现位）。
    let to_end_currency_error = computed(move || {
        if state.family.get() != FormFamily::Transfer {
            return false;
        }
        let rows = sources.accounts.get().0;
        let currency_of = |id: &str| {
            rows.iter()
                .find(|a| a.id == id)
                .map(|a| a.currency_code.clone())
        };
        let from = state.account_id.get().as_deref().and_then(currency_of);
        let to = state.to_account_id.get().as_deref().and_then(currency_of);
        matches!((from, to), (Some(f), Some(t)) if f != t)
    });
    // 编辑态商户口显示名（回填断言锚点；含软删全量口径解析）。
    let merchant_label = move || {
        state.editing_merchant_id.get().as_deref().and_then(|id| {
            sources
                .merchants
                .get()
                .0
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.name.clone())
        })
    };
    // 退款原交易快照行文本（原交易：日期 · 金额 · 分类路径 · 账户名；快照
    // 不在场不渲染——fixed-target 入口必有，编辑面多半不可得）。
    let refund_target_text = move || {
        let target = state.refund_target.get().0.clone()?;
        let fmt = sources.currencies.get().format_of(&target.currency_code);
        let amount = assembly::format_amount_basic(target.amount_cents, &fmt);
        let categories =
            assembly::CategoryTable::from_categories(sources.categories.get().0.iter());
        let category = categories
            .path_of(target.category_id.as_deref())
            .unwrap_or_else(|| "-".to_owned());
        let account = target
            .account_id
            .as_deref()
            .and_then(|id| {
                sources
                    .accounts
                    .get()
                    .0
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| a.name.clone())
            })
            .unwrap_or_else(|| "-".to_owned());
        Some(format!(
            "{} · {amount} · {category} · {account}",
            target.date
        ))
    };
    // 退款锁定账户名（继承原支出账户——后端 Writer 继承是唯一权威，本行只
    // 读呈现；账户不在可见面（归档/软删）回退占位词，不裸露 uuid）。
    let refund_account_text = move || {
        state
            .account_id
            .get()
            .as_deref()
            .and_then(|id| {
                sources
                    .accounts
                    .get()
                    .0
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| a.name.clone())
            })
            .unwrap_or_else(|| "由原交易决定".to_owned())
    };

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

    let submit = perform_submit(state, sources, view_owner);

    // 方向切换（借贷变体）：反向交换两端 / 越侧清除，决策半边在装配层
    // （lending_direction_swap），本面只搬运信号。
    let set_direction = move |next: assembly::LendingFormDirection| {
        let type_of = |id: &str| {
            sources
                .accounts
                .get()
                .0
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.kind)
        };
        let (from, to) = assembly::lending_direction_swap(
            state.lending_direction.get(),
            next,
            state.account_id.get().as_deref(),
            state.to_account_id.get().as_deref(),
            type_of,
        );
        state.account_id.set(from);
        state.to_account_id.set(to);
        state.lending_direction.set(next);
    };

    modal::app_modal(
        move || state.open.get(),
        move || modal_title(state.editing_id.get().is_some(), state.entry_label.get()),
        move || state.close(),
        move || {
            let family = state.family.get();
            // 金额占位随形态（退款取专属词表 transactions.refund.amount）。
            let amount_placeholder: &str = if family == FormFamily::Refund {
                "退款金额"
            } else {
                "金额"
            };
            let body = view()
                .test_id(FORM_ID)
                .flex_col()
                .gap_2()
                // 金额（原始文本承载；判定错误红框）+ 币种（推导/继承锁定）。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label("金额", muted))
                        .child(
                            input()
                                .test_id(AMOUNT_ID)
                                .value(move || state.amount_text.get())
                                .placeholder(amount_placeholder)
                                .optional_label("交易金额")
                                .text_color(fg)
                                .w(px(160.0))
                                .app_class(
                                    "bg-background rounded-md border border-border px-2 py-1 placeholder:text-muted-foreground",
                                )
                                .on_change(move |value: &str| state.amount_text.set(value.to_owned()))
                                .on_blur(move || state.amount_marked.set(true)),
                        )
                        .child(
                            // 锚点落在包裹视图上（headless 组合选择器 id: >> text:
                            // 以后代命中；先例 date_select 触发件同形）。
                            view()
                                .test_id(CURRENCY_ID)
                                .child(text(currency_text).text_size(px(13.0)).text_color(muted)),
                        ),
                );
            // 形态字段族（kind 切换字段族重排；族随入口/编辑锁定）。
            let body = match family {
                FormFamily::Basic => body
                    // 账户（可搜下拉；币种随之推导）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("账户", muted))
                            .child(pinyin_select::pinyin_select(
                                ACCOUNT_PREFIX,
                                move || account_options.get(),
                                state.account_id,
                                "选择账户",
                                false,
                            )),
                    )
                    // 分类（可搜下拉、可清空——无分类交易合法）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("分类", muted))
                            .child(pinyin_select::pinyin_select(
                                CATEGORY_PREFIX,
                                move || category_options.get(),
                                state.category_id,
                                "选择分类",
                                true,
                            )),
                    )
                    // 商户（自由文本可搜输入：选择在用商户或直接输入新名称）。
                    .child(merchant_row(state, merchant_options, fg, muted))
                    // 编辑态商户锚点（回填断言面：创建态不渲染）。
                    .child(merchant_label_row(merchant_label, muted))
                    // 保单（普通下拉、可清空；选项闭集归视图）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("保单", muted))
                            .child(select::app_select(
                                POLICY_PREFIX,
                                move || policy_options.get(),
                                state.policy_id,
                                "选择保单（可选）",
                                true,
                            )),
                    ),
                FormFamily::Transfer => body
                    // 转出账户（可搜下拉；币种随之推导）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("转出账户", muted))
                            .child(pinyin_select::pinyin_select(
                                ACCOUNT_PREFIX,
                                move || account_options.get(),
                                state.account_id,
                                "选择转出账户",
                                false,
                            )),
                    )
                    // 转入账户（候选与转出同币种；存量脏行回填时红显条位拦截）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("转入账户", muted))
                            .child(pinyin_select::pinyin_select(
                                TO_ACCOUNT_PREFIX,
                                move || to_account_options.get(),
                                state.to_account_id,
                                "目标账户",
                                false,
                            )),
                    )
                    // 两端币种不一致条位（旧壳字段红显 + 禁用提交的本壳呈现位；
                    // 提交按钮禁用随该态同发）。
                    .child(
                        when(
                            move || to_end_currency_error.get(),
                            move || {
                                text("转出账户和转入账户的币种不一致")
                                    .text_size(px(12.0))
                                    .text_color(danger_fg)
                                    .test_id(TO_CURRENCY_ERROR_ID)
                            },
                        )
                        .otherwise(view),
                    ),
                FormFamily::Lending => body
                    // 方向切换钮（四向，键序即展示序——旧壳 LENDING_FORM_DIRECTIONS
                    // 同序；方向切换的已选账户处置归装配层决策半边）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("方向", muted))
                            .child(
                                view().flex_row().gap_1().children(
                                    [
                                        assembly::LendingFormDirection::Lend,
                                        assembly::LendingFormDirection::Collect,
                                        assembly::LendingFormDirection::Borrow,
                                        assembly::LendingFormDirection::Repay,
                                    ]
                                    .map(|d| {
                                        nsh_base::button()
                                            .selected(move || state.lending_direction.get() == d)
                                            .app_class(
                                                "px-2 py-1 rounded-md border border-border \
                                             text-foreground hover:bg-accent selected:bg-accent",
                                            )
                                            .child(
                                                text(assembly::lending_direction_label(d))
                                                    .text_size(px(13.0))
                                                    .text_color(fg)
                                                    .test_id(direction_test_id(d)),
                                            )
                                            .on_press(move |_: &()| set_direction(d))
                                    }),
                                ),
                            ),
                    )
                    // 转出账户（候选随方向转出侧过滤）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("转出账户", muted))
                            .child(pinyin_select::pinyin_select(
                                ACCOUNT_PREFIX,
                                move || account_options.get(),
                                state.account_id,
                                "选择转出账户",
                                false,
                            )),
                    )
                    // 转入账户（候选随方向转入侧过滤）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("转入账户", muted))
                            .child(pinyin_select::pinyin_select(
                                TO_ACCOUNT_PREFIX,
                                move || to_account_options.get(),
                                state.to_account_id,
                                "目标账户",
                                false,
                            )),
                    )
                    // 商户（借贷可关联商户——ADR-0092，与支出表单同款自由文本）。
                    .child(merchant_row(state, merchant_options, fg, muted))
                    // 编辑态商户锚点（回填断言面：创建态不渲染）。
                    .child(merchant_label_row(merchant_label, muted)),
                FormFamily::Refund => body
                    // 原交易（fixed-target 快照在场时呈现只读信息行——旧壳
                    // RefundForm「原交易」NText 同位；编辑面快照不可得不渲染）。
                    .child(
                        when(
                            move || refund_target_text().is_some(),
                            move || {
                                text(move || {
                                    format!("原交易：{}", refund_target_text().unwrap_or_default())
                                })
                                .text_size(px(11.0))
                                .text_color(muted)
                                .test_id(REFUND_TARGET_ID)
                            },
                        )
                        .otherwise(view),
                    )
                    // 账户（继承原支出账户，锁定只读——旧壳 disabled PinyinSelect
                    // 「由原交易决定」同语义；显示文本取可见面账户名）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("账户", muted))
                            .child(
                                // 锚点落在包裹视图上（同上——组合选择器后代命中）。
                                view().test_id(REFUND_ACCOUNT_ID).child(
                                    text(refund_account_text).text_size(px(13.0)).text_color(fg),
                                ),
                            ),
                    ),
                FormFamily::Funding => body
                    // 分类（可搜下拉、可清空——无分类交易合法；交易行分类是
                    // 统计归属，报表按单归一类）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("分类", muted))
                            .child(pinyin_select::pinyin_select(
                                CATEGORY_PREFIX,
                                move || category_options.get(),
                                state.category_id,
                                "选择分类",
                                true,
                            )),
                    )
                    // 商户 + 编辑态商户锚点 + 保单（与基础收支族同款）。
                    .child(merchant_row(state, merchant_options, fg, muted))
                    .child(merchant_label_row(merchant_label, muted))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("保单", muted))
                            .child(select::app_select(
                                POLICY_PREFIX,
                                move || policy_options.get(),
                                state.policy_id,
                                "选择保单（可选）",
                                true,
                            )),
                    )
                    // 出资项子行区（多账户分摊：无主账户位——契约互斥；合计
                    // 一致性实时红显，提交阻拦在装配器）。
                    .child(funding_rows_section(state, sources, muted, fg)),
                FormFamily::Purchase => body
                    // 账户（订单付款账户，主账户必选——与分解族互斥形态）。
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("账户", muted))
                            .child(pinyin_select::pinyin_select(
                                ACCOUNT_PREFIX,
                                move || account_options.get(),
                                state.account_id,
                                "选择账户",
                                false,
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("分类", muted))
                            .child(pinyin_select::pinyin_select(
                                CATEGORY_PREFIX,
                                move || category_options.get(),
                                state.category_id,
                                "选择分类",
                                true,
                            )),
                    )
                    .child(merchant_row(state, merchant_options, fg, muted))
                    .child(merchant_label_row(merchant_label, muted))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("保单", muted))
                            .child(select::app_select(
                                POLICY_PREFIX,
                                move || policy_options.get(),
                                state.policy_id,
                                "选择保单（可选）",
                                true,
                            )),
                    )
                    // 商品行区（订单 + 商品行：逐行名称/件数/分类/单价，行小
                    // 计呈现；价格存而不显示在列表——小计只在表单内提示）。
                    .child(purchase_rows_section(state, sources, muted, fg)),
            };
            body
                // 日期（可键入 + 日历点选）。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label("日期", muted))
                        .child(date_select::date_select(DATE_PREFIX, state.date)),
                )
                // 备注。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label("备注", muted))
                        .child(
                            input()
                                .test_id(NOTE_ID)
                                .value(move || state.note.get())
                                .placeholder("备注（可选）")
                                .optional_label("交易备注")
                                .text_color(fg)
                                .flex_1()
                                .app_class(
                                    "bg-background rounded-md border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |value: &str| state.note.set(value.to_owned())),
                        ),
                )
                // 错误条位（业务校验与域码化错误同位；下次尝试开始时清空）。
                .child(
                    when(
                        move || state.error.get().is_some(),
                        move || {
                            text(move || state.error.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(danger_fg)
                                .test_id(ERROR_ID)
                        },
                    )
                    .otherwise(view),
                )
                // 动作行：取消（显式关闭不写）+ 提交（红态/两端币种不一致禁用）。
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
                                    text("取消").text_size(px(13.0)).text_color(fg).test_id("tx-form-cancel"),
                                )
                                .on_press(move |_: &()| state.close()),
                        )
                        .child(
                            nsh_base::button()
                                .disabled(move || amount_error() || to_end_currency_error.get())
                                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                                .child(
                                    text(move || {
                                        submit_label(
                                            state.family.get(),
                                            state.kind.get(),
                                            state.lending_direction.get(),
                                            state.editing_id.get().is_some(),
                                        )
                                    })
                                        .text_size(px(13.0))
                                        .text_color(rgb(0xffffff))
                                        .test_id(SUBMIT_ID),
                                )
                                .on_press(move |_: &()| submit()),
                        ),
                )
        },
    )
}

/// 出资项子行区（表单③出资分解族）：区标题 + 逐行（账户下拉 / 金额 / 标签 /
/// 删除）+ 合计行（Σ 与交易金额不一致红显）+ 加行钮。行键稳定不重排（删除
/// 中间行其余行锚点不变）；每行账户候选 = 可见账户 − 归档 + 行内补值（编辑
/// 面已存归档账户标「已归档」——与主账户选择器同口径）。
fn funding_rows_section(
    state: TxnFormState,
    sources: TxnFormSources,
    muted: AdaptiveColor,
    fg: AdaptiveColor,
) -> View {
    // 合计行文本（Σ 出资项与主金额；主金额换算失败按 0 呈现，红态已由金额
    // 红框表达；不一致红显——提交阻拦在装配器 fail fast）。
    let total = computed(move || {
        let rows = state.funding_rows.get();
        let sum: i64 = rows
            .iter()
            .filter_map(|row| assembly::amount_text_to_cents(&row.amount_text.get()))
            .sum();
        let main = assembly::amount_text_to_cents(&state.amount_text.get()).unwrap_or(0);
        let fmt = sources
            .currencies
            .get()
            .format_of(&selected_currency(&state, &sources));
        (
            format!(
                "出资合计 {} · 交易金额 {}",
                assembly::format_amount_basic(sum, &fmt),
                assembly::format_amount_basic(main, &fmt)
            ),
            sum != main,
        )
    });
    // 行集 keyed 渲染（键 = 行键，删除中间行其余行不重挂——锚点随键稳定，
    // headless 断言按行键寻址）。
    let rows_view = each(
        move || state.funding_rows.get(),
        |row: &FundingRowState| row.key,
        move |item: ReadSignal<FundingRowState>, _index: ReadSignal<usize>| {
            let row = item.get();
            // 行内账户候选（每行独立补值：已存值不在可见面时补回并标「已归档」）。
            let row_for_options = row.clone();
            let account_options = move || {
                pinned_account_options(
                    &sources.accounts.get().0,
                    row_for_options.account_id.get().as_deref(),
                    |_| true,
                )
            };
            let row_for_remove = row.clone();
            let rows_for_remove = state;
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .test_id(format!("{}-{}", FUND_ROW_ID_PREFIX, row.key).as_str())
                .child(pinyin_select::pinyin_select(
                    format!("{}-{}", FUND_ACCOUNT_PREFIX, row.key),
                    account_options,
                    row.account_id,
                    "出资账户",
                    false,
                ))
                .child(
                    input()
                        .test_id(format!("{}-{}", FUND_AMOUNT_PREFIX, row.key).as_str())
                        .value(move || row.amount_text.get())
                        .placeholder("金额")
                        .optional_label("出资金额")
                        .text_color(fg)
                        .w(px(110.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| row.amount_text.set(value.to_owned())),
                )
                .child(
                    input()
                        .test_id(format!("{}-{}", FUND_LABEL_PREFIX, row.key).as_str())
                        .value(move || row.label.get())
                        .placeholder("标签（可选）")
                        .optional_label("扣款标签")
                        .text_color(fg)
                        .w(px(110.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| row.label.set(value.to_owned())),
                )
                .child(
                    nsh_base::button()
                        .accessibility_label("删除出资项")
                        .app_class("px-1 py-0.5 rounded-md text-muted-foreground hover:bg-accent")
                        .child(text("✕").text_size(px(11.0)).text_color(muted).test_id(
                            format!("{}-{}", FUND_REMOVE_PREFIX, row_for_remove.key).as_str(),
                        ))
                        .on_press(move |_: &()| {
                            let key = row_for_remove.key;
                            rows_for_remove
                                .funding_rows
                                .update(|list| list.retain(|r| r.key != key));
                        }),
                )
        },
    );
    view()
        .flex_col()
        .gap_1()
        .child(field_label("出资项", muted))
        .child(rows_view)
        .child(
            // 合计行（一致中性 / 不一致红——可纠错的即时呈现，提交阻拦在装配
            // 器 fail fast 的错误条位）。
            text(move || total.get().0)
                .text_size(px(11.0))
                .test_id(format!("{}-text", FUNDING_TOTAL_ID).as_str())
                .text_color(move || {
                    if total.get().1 {
                        tokens()
                            .color("destructive")
                            .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into())
                    } else {
                        muted
                    }
                }),
        )
        .child(
            nsh_base::button()
                .app_class(
                    "px-2 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                )
                .child(
                    text("添加出资项")
                        .text_size(px(12.0))
                        .text_color(fg)
                        .test_id(FUNDING_ADD_ID),
                )
                .on_press(move |_: &()| {
                    let row = state.new_funding_row();
                    state.funding_rows.update(|list| list.push(row));
                }),
        )
}

/// 购买项商品行区（表单③购买项族）：区标题 + 逐行（名称 / 件数 / 分类 /
/// 单价 / 行小计 / 删除）+ 加行钮。行键稳定不重排；行小计 = 单价 × 件数
/// （价格可空——ADR-0138 决策 10：价格存而不显示、不进金额口径，小计只在
/// 表单内提示，不与订单实付勾稽）。
fn purchase_rows_section(
    state: TxnFormState,
    sources: TxnFormSources,
    muted: AdaptiveColor,
    fg: AdaptiveColor,
) -> View {
    // 商品行 keyed 渲染（键 = 行键，同出资项区——锚点随键稳定）。
    let rows_view = each(
        move || state.purchase_rows.get(),
        |row: &PurchaseRowState| row.key,
        move |item: ReadSignal<PurchaseRowState>, _index: ReadSignal<usize>| {
            let row = item.get();
            // 行小计（单价与件数均合法才有值；解析失败不呈现，提交阻拦在装
            // 配器 fail fast 的错误条位）。
            let row_for_subtotal = row.clone();
            let subtotal = move || {
                let price = assembly::amount_text_to_cents(&row_for_subtotal.price_text.get())?;
                let qty: i64 = row_for_subtotal.quantity_text.get().trim().parse().ok()?;
                if qty <= 0 {
                    return None;
                }
                let fmt = sources
                    .currencies
                    .get()
                    .format_of(&selected_currency(&state, &sources));
                Some(format!(
                    "小计 {}",
                    assembly::format_amount_basic(price * qty, &fmt)
                ))
            };
            // 商品分类候选（支出树；行内独立选择器）。
            let category_options =
                move || assembly::category_kind_options("expense", &sources.categories.get().0);
            let row_for_remove = row.clone();
            let rows_for_remove = state;
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .test_id(format!("{}-{}", PURCHASE_ROW_ID_PREFIX, row.key).as_str())
                .child(
                    input()
                        .test_id(format!("{}-{}", PURCHASE_NAME_PREFIX, row.key).as_str())
                        .value(move || row.name.get())
                        .placeholder("商品名称")
                        .optional_label("商品名称")
                        .text_color(fg)
                        .w(px(140.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| row.name.set(value.to_owned())),
                )
                .child(
                    input()
                        .test_id(format!("{}-{}", PURCHASE_QTY_PREFIX, row.key).as_str())
                        .value(move || row.quantity_text.get())
                        .placeholder("件数")
                        .optional_label("商品件数")
                        .text_color(fg)
                        .w(px(60.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| row.quantity_text.set(value.to_owned())),
                )
                .child(pinyin_select::pinyin_select(
                    format!("{}-{}", PURCHASE_CAT_PREFIX, row.key),
                    category_options,
                    row.category_id,
                    "分类（可选）",
                    true,
                ))
                .child(
                    input()
                        .test_id(format!("{}-{}", PURCHASE_PRICE_PREFIX, row.key).as_str())
                        .value(move || row.price_text.get())
                        .placeholder("单价（可选）")
                        .optional_label("商品单价")
                        .text_color(fg)
                        .w(px(90.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| row.price_text.set(value.to_owned())),
                )
                .child(
                    text(move || subtotal().unwrap_or_default())
                        .text_size(px(11.0))
                        .text_color(muted)
                        .test_id(format!("{}-{}", PURCHASE_SUBTOTAL_PREFIX, row.key).as_str()),
                )
                .child(
                    nsh_base::button()
                        .accessibility_label("删除商品")
                        .app_class("px-1 py-0.5 rounded-md text-muted-foreground hover:bg-accent")
                        .child(text("✕").text_size(px(11.0)).text_color(muted).test_id(
                            format!("{}-{}", PURCHASE_REMOVE_PREFIX, row_for_remove.key).as_str(),
                        ))
                        .on_press(move |_: &()| {
                            let key = row_for_remove.key;
                            rows_for_remove
                                .purchase_rows
                                .update(|list| list.retain(|r| r.key != key));
                        }),
                )
        },
    );
    view()
        .flex_col()
        .gap_1()
        .child(field_label("商品", muted))
        .child(rows_view)
        .child(
            nsh_base::button()
                .app_class(
                    "px-2 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                )
                .child(
                    text("添加商品")
                        .text_size(px(12.0))
                        .text_color(fg)
                        .test_id(PURCHASE_ADD_ID),
                )
                .on_press(move |_: &()| {
                    let row = state.new_purchase_row();
                    state.purchase_rows.update(|list| list.push(row));
                }),
        )
}

/// 商户输入行（基础收支与借贷变体共用；自由文本可搜输入）。
fn merchant_row(
    state: TxnFormState,
    merchant_options: Memo<Vec<CategoryOption>>,
    _fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label("商户", muted))
        .child(combo_select::combo_select(
            MERCHANT_PREFIX,
            move || merchant_options.get(),
            state.merchant_typed,
            state.merchant_selected,
            "选择商户，可直接输入新名称",
        ))
}

/// 编辑态商户锚点行（回填断言面：创建态不渲染）。
fn merchant_label_row(
    merchant_label: impl Fn() -> Option<String> + Copy + 'static,
    muted: AdaptiveColor,
) -> impl IntoElement {
    when(
        move || merchant_label().is_some(),
        move || {
            text(move || format!("原商户：{}", merchant_label().unwrap_or_default()))
                .text_size(px(11.0))
                .text_color(muted)
                .test_id(MERCHANT_LABEL_ID)
        },
    )
    .otherwise(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merchant_ref_maps_selector_state_to_semantics() {
        // 创建面：选定 → Selected；文本 → NewName；空 → None。
        assert_eq!(
            merchant_ref(None, Some("m1"), ""),
            MerchantRef::Selected("m1".to_owned())
        );
        assert_eq!(
            merchant_ref(None, None, "新商户"),
            MerchantRef::NewName("新商户".to_owned())
        );
        assert_eq!(
            merchant_ref(None, None, "   "),
            MerchantRef::None,
            "纯空白 = 不挂"
        );
        // 编辑面：未改动 → Keep（后端 unchanged 语义跳过在用校验，软删历史
        // 引用保留）；改选 → Selected。
        assert_eq!(
            merchant_ref(Some("m-orig"), Some("m-orig"), "原名"),
            MerchantRef::Keep("m-orig".to_owned())
        );
        assert_eq!(
            merchant_ref(Some("m-orig"), Some("m-other"), "原名"),
            MerchantRef::Selected("m-other".to_owned())
        );
        // 编辑面清空 = 不挂（移除商户引用是合法编辑）。
        assert_eq!(merchant_ref(Some("m-orig"), None, ""), MerchantRef::None);
        // 编辑面改文本（哪怕重输同名）= 自由文本语义（后端在用名精确匹配
        // 复用——同名命中回原 id 无害，软删同名即建，与旧壳同口径）。
        assert_eq!(
            merchant_ref(Some("m-orig"), None, "改名"),
            MerchantRef::NewName("改名".to_owned())
        );
    }

    #[test]
    fn transfer_merchant_keeps_original_without_input_face() {
        // 普通转账表单无商户输入位：创建恒不挂、编辑原样携带（Keep）。
        assert_eq!(transfer_merchant(None), MerchantRef::None);
        assert_eq!(
            transfer_merchant(Some("m-1")),
            MerchantRef::Keep("m-1".to_owned())
        );
    }

    #[test]
    fn submit_labels_and_titles_follow_old_shell_vocabulary() {
        use ledger_transaction::TransactionKind as K;
        let lend = assembly::LendingFormDirection::Lend;
        // 基础收支创建：记支出/记收入；编辑：保存修改。
        assert_eq!(
            submit_label(FormFamily::Basic, K::Expense, lend, false),
            "记支出"
        );
        assert_eq!(
            submit_label(FormFamily::Basic, K::Income, lend, false),
            "记收入"
        );
        assert_eq!(modal_title(false, "支出"), "记一笔 · 支出");
        assert_eq!(modal_title(false, "收入"), "记一笔 · 收入");
        // 转账/借贷/退款创建词表；借贷随当前方向。
        assert_eq!(
            submit_label(FormFamily::Transfer, K::Transfer, lend, false),
            "记转账"
        );
        assert_eq!(modal_title(false, "转账"), "记一笔 · 转账");
        for (direction, word) in [
            (assembly::LendingFormDirection::Lend, "借出"),
            (assembly::LendingFormDirection::Collect, "收回"),
            (assembly::LendingFormDirection::Borrow, "借入"),
            (assembly::LendingFormDirection::Repay, "还款"),
        ] {
            assert_eq!(
                submit_label(FormFamily::Lending, K::Transfer, direction, false),
                format!("记{word}")
            );
            assert_eq!(modal_title(false, word), format!("记一笔 · {word}"));
        }
        assert_eq!(
            submit_label(FormFamily::Refund, K::Refund, lend, false),
            "记退款"
        );
        // 编辑态恒「编辑交易 / 保存修改」（入口词不参与）。
        assert_eq!(
            submit_label(FormFamily::Basic, K::Expense, lend, true),
            "保存修改"
        );
        assert_eq!(modal_title(true, "借入"), "编辑交易");
    }

    #[test]
    fn edit_dispatch_routes_family_by_kind_and_lending_shape() {
        use ledger_transaction::TransactionKind as K;
        fn account(id: &str, kind: ledger_accounts::AccountType) -> ledger_accounts::Account {
            ledger_accounts::Account {
                id: id.to_owned(),
                name: id.to_owned(),
                kind,
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
        fn row(id: &str, kind: K) -> ledger_transaction::Transaction {
            ledger_transaction::Transaction {
                id: id.to_owned(),
                kind,
                amount_cents: 10_000,
                currency_code: "CNY".to_owned(),
                amount_native_cents: 10_000,
                account_id: Some("a-cash".to_owned()),
                to_account_id: None,
                funding_account_id: None,
                category_id: None,
                merchant_id: None,
                policy_id: None,
                refund_of_transaction_id: None,
                fx_rate_used: None,
                fx_rate_source: None,
                note: None,
                source_order_no: None,
                date: "2026-02-01".to_owned(),
                created_at: String::new(),
                updated_at: String::new(),
                version: 1,
                device_id: String::new(),
                is_deleted: false,
                fundings: Vec::new(),
                purchases: Vec::new(),
                source: None,
                convert: None,
            }
        }
        let table = assembly::AccountTable::from_accounts(
            [
                account("a-cash", ledger_accounts::AccountType::Cash),
                account("a-recv", ledger_accounts::AccountType::Receivable),
                account("a-bank", ledger_accounts::AccountType::Bank),
            ]
            .iter(),
        );
        // 收入/支出 → 基础族。
        assert_eq!(
            edit_dispatch(&row("t", K::Expense), &table),
            Some((FormFamily::Basic, None))
        );
        assert_eq!(
            edit_dispatch(&row("t", K::Income), &table),
            Some((FormFamily::Basic, None))
        );
        // 资金 → 应收 = 借出派生：进借贷变体、方向随派生。
        let mut lending = row("t", K::Transfer);
        lending.to_account_id = Some("a-recv".to_owned());
        assert_eq!(
            edit_dispatch(&lending, &table),
            Some((
                FormFamily::Lending,
                Some(assembly::LendingFormDirection::Lend)
            ))
        );
        // 资金 → 资金 = 普通转账族（无方向）。
        let mut plain = row("t", K::Transfer);
        plain.to_account_id = Some("a-bank".to_owned());
        assert_eq!(
            edit_dispatch(&plain, &table),
            Some((FormFamily::Transfer, None))
        );
        // 退款 → 退款族。
        assert_eq!(
            edit_dispatch(&row("t", K::Refund), &table),
            Some((FormFamily::Refund, None))
        );
        // 界面只读 kind 不开放编辑入口（防御双落点）。
        assert_eq!(edit_dispatch(&row("t", K::Convert), &table), None);
        assert_eq!(edit_dispatch(&row("t", K::Buy), &table), None);
    }

    #[test]
    fn amount_error_gates_on_marked_flag() {
        // 时机语义：判定错误但未标记（未失焦/未尝试）不红——初始为空不抢红。
        let parse_error = assembly::judge_amount_text("abc");
        assert!(!amount_error_state(parse_error, false));
        assert!(amount_error_state(parse_error, true));
        let ok = assembly::judge_amount_text("25");
        assert!(!amount_error_state(ok, true), "合法值不红（即便已标记）");
        let empty = assembly::judge_amount_text("");
        assert!(!amount_error_state(empty, false), "空值未标记不红");
        assert!(amount_error_state(empty, true), "空值在提交尝试后红");
    }
}
