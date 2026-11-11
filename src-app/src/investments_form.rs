//! 投资记账表单·买卖族（issue #2046 NSH ⑮；spec #2031 系列 15/26，投资
//! 2/3；旧壳 `TransactionForm.vue` 的 `InvestmentForm.vue` 挂载族 +
//! `useInvestmentForm.ts` 的重生）。
//!
//! buy / sell 记账表单，创建与编辑共用同表单语义（旧壳「编辑复用同表单」
//! 同纪律）：金额（基金权威 / 其余自动展示）+ 币种（账户推导锁定）+ 投资
//! 账户 + 出资账户（可选）+ 标的 + 份额（数量）+ 单价（基金反算只读 / 其
//! 余权威）+ 手续费 + 日期 + 备注（旧壳 InvestmentForm 字段族全同构）。
//!
//! **入口归属（偏差留痕）**：旧壳买入/卖出的**创建**入口住投资明细页签
//! （`openCreate(kind)`）——明细页签随⑯ 迁移，届时本模块
//! [`TradeFormState::open_create`] 直达该两入口（接缝零改动，⑯ 只装配不
//! 重迁）；本票交付即具备创建能力但无用户可点创建入口。**编辑**入口在交
//! 易列表行右键菜单（旧壳 issue #180 起 buy/sell 行开放「编辑」）——本票
//! 随交易列表挂载接线开放（[`TradeFormState::begin_edit`] 单点）；旧壳行
//! 菜单开放判定（supportsRowEdit：buy / sell 开编辑；convert / split /
//! dividend 界面只读）平价不动。
//!
//! **先取明细再开窗**（旧壳 TransactionModalState 编排同语义）：编辑先读
//! `security_transactions` 扩展投影（标的/数量/价格/费用 + 标的字典带出字
//! 段），慢取期间不开窗、失败不开窗（旧壳同口径；NSH 无 toast——失败反
//! 馈条位住交易列表视图，[`TradeFormState::load_error`] 信号消费）。
//!
//! **录入权威按标的类型分流**（issue #302 / ADR-0038）：场外基金 = 确认单
//! 金额（权威）+ 份额，单价由（金额 ∓ 手续费）÷ 份额反算只读展示（后端反
//! 算不落 wire）；其余 = 数量 + 单价（成交金额由后端按数量 × 单价 ± 费用
//! 重算，前端展示面自动计算）。判定与换算口径单点在
//! [`crate::assembly::judge_quantity_text`] / [`crate::assembly::judge_price_text`]
//! / [`crate::assembly::assemble_trade_transaction`]——wire 拼装与业务校验
//! fail fast 全归装配器闭集，本面只搬运表单状态（txn_form 五装配器同纪律）。
//!
//! **字段红态 + 提交禁用两件同发**（ADR-0058 决策 1）：数量/价格红框（旧壳
//! NInput status 同位）+ 保存禁用。偏差留痕：旧壳金额/手续费是 NInputNumber
//!（数值输入恒合法、无红态），本壳自由文本承载（非法输入原样保留，数量/
//! 价格同族纪律），金额/手续费一并接入判定红框 + 禁用——禁用条件比旧壳
//! 严格（形状非法提前拦）；业务类校验（账户/标的/非正）保留提交通道错误
//! 条位（旧壳 toast 同语义，词表逐条平价）。
//!
//! **记账币种**（issue #1191 / ADR-0134 决策 5）：buy/sell 记账币种由投资
//! 账户决定，展示值锁定只读（后端 Writer 守卫是唯一权威）。出资账户候选 =
//! 账户同币种（旧壳 #936 / #938 候选准入闭集同源，绑定排除随储蓄目标域读
//! 接入：储蓄目标专属账户不进候选；后端行为层准入是唯一权威，前端过滤是
//! 防呆）。
//!
//! **失效广播**：写成功广播交易族信号（交易列表重读）——旧壳买卖写后持
//! 仓页不主动重拉（后端交易写不 emit PRICES_CHANGED），本壳同口径不扩广播
//! 面（持仓刷新随价格失效信号与激活边重拉承担）。
//!
//! 标的候选面：一次全量装载（分页循环至 total 收核、无静默截断——#2045
//! 偏差 1 同源承诺，⑯ 明细/标的页签票回访远程搜索竞态），拼音二维筛选由
//! [`crate::pinyin_select`] 统一模糊搜索语义本地完成。
//!
//! 界面文案仅 zh-CN（旧壳 transactions.form / investments.form 词表逐条
//! 平价；字面闭集承载）。码化错误 zh 原样呈现（错误条位）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::input;
use nsh_core::prelude::*;

use crate::design::font_size;
use crate::{
    assembly::{self, CategoryOption, CurrencyFormat, CurrencyTable},
    boot, date_select, modal, pinyin_select, signals,
};
use ledger_infra::error::AppError;

/// 表单锚点前缀（headless `id:` 选择器用；旧壳 data-testid `investment-*`
/// 的本壳选择器命名带）。
pub(crate) const FORM_ID: &str = "trade-form";
const AMOUNT_ID: &str = "trade-form-amount";
const AUTO_AMOUNT_ID: &str = "trade-form-auto-amount";
const CURRENCY_ID: &str = "trade-form-currency";
const ACCOUNT_PREFIX: &str = "trade-form-account";
const FUNDING_PREFIX: &str = "trade-form-funding";
const INSTRUMENT_PREFIX: &str = "trade-form-instrument";
const QUANTITY_ID: &str = "trade-form-quantity";
const PRICE_ID: &str = "trade-form-price";
const DERIVED_PRICE_ID: &str = "trade-form-derived-price";
const FEE_ID: &str = "trade-form-fee";
const DATE_PREFIX: &str = "trade-form-date";
const NOTE_ID: &str = "trade-form-note";
const SUBMIT_ID: &str = "trade-form-submit";
const CANCEL_ID: &str = "trade-form-cancel";
const ERROR_ID: &str = "trade-form-error";

// ---------------------------------------------------------------------------
// 词表（旧壳 transactions.form submit 词表 + investments.json form 段逐条平价）
// ---------------------------------------------------------------------------

const LABEL_AMOUNT: &str = "金额";
const LABEL_ACCOUNT: &str = "投资账户";
const LABEL_FUNDING: &str = "出资账户";
const LABEL_INSTRUMENT: &str = "标的";
const LABEL_SHARES: &str = "份额";
const LABEL_QUANTITY: &str = "数量";
const LABEL_UNIT_PRICE_NAV: &str = "单价（净值）";
const LABEL_UNIT_PRICE: &str = "单价";
const LABEL_FEE: &str = "手续费";
const LABEL_DATE: &str = "日期";
const LABEL_NOTE: &str = "备注";
const AMOUNT_PLACEHOLDER: &str = "确认金额（以确认单为准）";
const AMOUNT_AUTO: &str = "自动计算";
const ACCOUNT_PLACEHOLDER: &str = "选择投资账户";
const FUNDING_PLACEHOLDER: &str = "选择出资账户（可选）";
const INSTRUMENT_PLACEHOLDER: &str = "选择标的（输入代码/名称/拼音搜索）";
const SHARES_PLACEHOLDER: &str = "确认份额（以确认单为准）";
const QUANTITY_PLACEHOLDER: &str = "数量";
const DERIVED_PRICE_PLACEHOLDER: &str = "由金额与份额反算";
const UNIT_PRICE_PLACEHOLDER: &str = "单价";
const FEE_PLACEHOLDER: &str = "手续费";
const NOTE_PLACEHOLDER: &str = "备注（可选）";
const SAVE_EDIT: &str = "保存修改";
/// 失败反馈前缀（旧壳 saveFailed / recordFailed 同词表，冒号带空格同形）。
const SAVE_FAILED_PREFIX: &str = "保存失败: ";
const RECORD_FAILED_PREFIX: &str = "记账失败: ";
/// 编辑装载失败条位前缀（交易列表条位呈现，旧壳 toast 的本壳条位对应物）。
const LOAD_FAILED_PREFIX: &str = "取明细失败: ";

/// kind 方向词（旧壳 kind 词表；错误文案的共用轴。
/// [`TradeFormError::as_text`] 以「买入」/「卖出」作 kind 方向词）。
fn kind_word(kind: ledger_transaction::TransactionKind) -> &'static str {
    match kind {
        ledger_transaction::TransactionKind::Sell => "卖出",
        _ => "买入",
    }
}

// ---------------------------------------------------------------------------
// 数据源投影（signal 值域要求 PartialEq——域行未实现全量相等，投影比较按
// 表单消费字段全等：任一消费字段不同即不等，语义同「源变化即通知」；
// txn_form FormAccounts 同形先例）
// ---------------------------------------------------------------------------

/// 账户源投影（id / 名 / 币种 / 类型 / 归档位——候选面、币种推导与出资过
/// 滤的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct Accounts(pub(crate) Vec<ledger_accounts::Account>);

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

/// 标的源投影（id / 代码 / 名 / 类型 / 币种——候选面与基金判定的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct Instruments(pub(crate) Vec<ledger_investment::Instrument>);

impl PartialEq for Instruments {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.symbol == b.symbol
                    && a.name == b.name
                    && a.kind == b.kind
                    && a.currency_code == b.currency_code
            })
    }
}

/// 储蓄目标绑定账户 id 集（出资候选防呆过滤面；旧壳 goalAccountIds 同口径）。
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct GoalAccounts(pub(crate) Vec<String>);

/// 表单数据源束（调用方视图持有；装载刷新与表单共享同一来源。signal 首建
/// 住 `new()`——调用方在视图实例内构造，信号随视图实例常驻。字典面字段句
/// 柄按信号 ID 全等（Signal PartialEq）——组件 Props 约束由字段面比较承载，
/// 「源信号受换成即不通知」由字段自身的投影 PartialEq 承担）。
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TradeFormSources {
    /// 可见账户行（含归档——候选面挂载时滤归档，编辑面补值消费全行）。
    #[allow(unknown_lints, clippy::type_complexity)]
    pub accounts: Signal<Accounts>,
    /// 标的全集。
    pub instruments: Signal<Instruments>,
    /// 币种字典（金额/费用文本换算消费）。
    pub currencies: Signal<CurrencyTable>,
    /// 储蓄目标绑定账户 id 集。
    pub goal_accounts: Signal<GoalAccounts>,
}

impl Default for TradeFormSources {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeFormSources {
    pub(crate) fn new() -> Self {
        Self {
            accounts: signal(Accounts::default()),
            instruments: signal(Instruments::default()),
            currencies: signal(CurrencyTable::default()),
            goal_accounts: signal(GoalAccounts::default()),
        }
    }
}

/// 参考四面装载批（编辑与创建两入口共用的同一阻塞批；工装件，私密）。
struct FormReferences {
    accounts: Vec<ledger_accounts::Account>,
    instruments: Vec<ledger_investment::Instrument>,
    currencies: Vec<ledger_currencies::Currency>,
    goal_accounts: Vec<String>,
}

/// 编辑回填锚（[#TradeFormState::begin_edit] 的行载荷面）：交易列表原始
/// 行与明细页签行适配投影同走这里（明细载荷只有行投影自己携带的回填锚
/// 字段——消费面 = kind / 金额锚点 / 账户端 / 可选出资账户 / 备注 / 记账
/// 币种 / 日期 / 来源订单号，旧壳 ledgerRowToModalRow 的重生同消费面）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TradeEditAnchor {
    pub(crate) id: String,
    pub(crate) kind: ledger_transaction::TransactionKind,
    pub(crate) amount_cents: i64,
    pub(crate) currency_code: String,
    pub(crate) account_id: Option<String>,
    pub(crate) funding_account_id: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) source_order_no: Option<String>,
    pub(crate) date: String,
}

impl From<&ledger_transaction::Transaction> for TradeEditAnchor {
    fn from(tx: &ledger_transaction::Transaction) -> Self {
        Self {
            id: tx.id.clone(),
            kind: tx.kind,
            amount_cents: tx.amount_cents,
            currency_code: tx.currency_code.clone(),
            account_id: tx.account_id.clone(),
            funding_account_id: tx.funding_account_id.clone(),
            note: tx.note.clone(),
            source_order_no: tx.source_order_no.clone(),
            date: tx.date.clone(),
        }
    }
}

/// 编辑装载批（明细 + 行回填锚 + 参考四面；工装件，私密）。
struct EditLoad {
    trade: ledger_investment::TransactionTrade,
    anchor: TradeEditAnchor,
    references: FormReferences,
}

/// 阻塞批：参考四面一次装载（编辑与创建两入口共用）。
fn load_form_references(
    db: Option<&ledger_infra::db::DbState>,
) -> ledger_infra::error::Result<FormReferences> {
    let Some(db) = db else {
        return Err(AppError::coded(
            boot::CODE_SESSION_MISSING,
            "内部错误：账本库会话缺失",
        ));
    };
    let accounts = boot::account_rows(db)?;
    let instruments = boot::instrument_rows(db)?;
    let currencies = boot::currency_rows(db)?;
    let goals = boot::savings_goal_progress_rows(db)?;
    Ok(FormReferences {
        accounts,
        instruments,
        currencies,
        goal_accounts: goals.into_iter().map(|g| g.goal.account_id).collect(),
    })
}

/// 阻塞批：编辑装载（买卖明细 + 参考四面 + 行回填锚）。
fn load_edit_bundle(
    db: Option<&ledger_infra::db::DbState>,
    tx_id: &str,
    anchor: TradeEditAnchor,
) -> ledger_infra::error::Result<EditLoad> {
    let Some(db) = db else {
        return Err(AppError::coded(
            boot::CODE_SESSION_MISSING,
            "内部错误：账本库会话缺失",
        ));
    };
    let trade = boot::transaction_trade(db, tx_id)?;
    let references = load_form_references(Some(db))?;
    Ok(EditLoad {
        trade,
        anchor,
        references,
    })
}

// ---------------------------------------------------------------------------
// 表单信号束
// ---------------------------------------------------------------------------

/// 表单信号束（缺省关窗、买入 kind、今天日期；Copy 句柄可随意移入闭包；
/// 状态住调用方视图实例，同 txn_form 同一所有权形态；字典面字段句柄按信号
/// ID 全等（组件 Props 约束面——字段面 égalité由各信号值摊）。
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TradeFormState {
    /// 弹窗开合（受控）。
    pub open: Signal<bool>,
    /// 提交 kind（创建入口锁定 / 编辑随行；表单内无切换控件——族随入口/
    /// 行锁定，旧壳 #150「类型由入口单点表达」同语义）。
    pub kind: Signal<ledger_transaction::TransactionKind>,
    /// 编辑目标交易 id（None = 创建模式）。
    pub editing_id: Signal<Option<String>>,
    /// 编辑透传锚点：来源订单号（表单无输入位，全字段替换下缺省即静默清
    /// 除——旧壳 #1862 同语义，编辑路径原样回传行上原值不静默丢失）。
    pub source_order_no: Signal<Option<String>>,
    /// 投资账户（必选；币种随之推导锁定）。
    pub account_id: Signal<Option<String>>,
    /// 可选出资账户（None = 维持余额买卖语义；编辑回填随全字段替换不被
    /// 静默丢失）。
    pub funding_account_id: Signal<Option<String>>,
    /// 标的（必选）。
    pub instrument_id: Signal<Option<String>>,
    /// 编辑面基金形态锁（None = 创建模式随标的字典响应式；Some = 编辑锁定，
    /// 防候选面未命中时基金判定翻车的带出字段）。
    fund_locked: Signal<Option<bool>>,
    /// 确认单金额原始文本（基金形态权威输入；非基金形态恒空）。
    pub amount_text: Signal<String>,
    /// 数量/份额原始文本（判定族消费的原样输入）。
    pub quantity_text: Signal<String>,
    /// 单价原始文本（非基金形态权威输入；基金恒空——反算只读展示）。
    pub price_text: Signal<String>,
    /// 手续费原始文本（可空；空 = fee_cents 不填而非 0，旧壳同语义）。
    pub fee_text: Signal<String>,
    /// 备注。
    pub note: Signal<String>,
    /// 交易日期（缺省今天）。
    pub date: Signal<chrono::NaiveDate>,
    /// 字段红态标记位（失焦 / 提交尝试后生效——空值不抢红，ADR-0058 决策 2）。
    amount_marked: Signal<bool>,
    quantity_marked: Signal<bool>,
    price_marked: Signal<bool>,
    fee_marked: Signal<bool>,
    /// 提交错误条位（业务校验与域码化错误同位；下次尝试开始时清空）。
    pub error: Signal<Option<String>>,
    /// 编辑装载失败条位（交易列表视图消费；下次尝试清空）。
    pub load_error: Signal<Option<String>>,
}

impl Default for TradeFormState {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeFormState {
    /// 表单信号束（缺省关窗、买入 kind、今天日期）。
    pub(crate) fn new() -> Self {
        Self {
            open: signal(false),
            kind: signal(ledger_transaction::TransactionKind::Buy),
            editing_id: signal(None),
            source_order_no: signal(None),
            account_id: signal(None),
            funding_account_id: signal(None),
            instrument_id: signal(None),
            fund_locked: signal(None),
            amount_text: signal(String::new()),
            quantity_text: signal(String::new()),
            price_text: signal(String::new()),
            fee_text: signal(String::new()),
            note: signal(String::new()),
            date: signal(chrono::Local::now().date_naive()),
            amount_marked: signal(false),
            quantity_marked: signal(false),
            price_marked: signal(false),
            fee_marked: signal(false),
            error: signal(None),
            load_error: signal(None),
        }
    }

    /// 打开编辑表单（**先取明细再开窗**：阻塞批读买卖明细 + 参考四面；成
    /// 功回填并开窗，失败不开窗 + [`Self::load_error`] 上抛交易列表条位。
    /// 取数任务挂 `view_owner`——调用方视图实例 owner，取数期间弹窗未挂
    /// 载无取消竞态，旧壳「先取再开窗」同口径）。交易列表原始行与明细页签
    /// 行适配投影同走 [`TradeEditAnchor`]（明细页签行只有行投影自己携带的
    /// 回填锚字段——弹窗行适配消费面同源）。
    pub(crate) fn begin_edit(
        self,
        anchor: TradeEditAnchor,
        sources: TradeFormSources,
        view_owner: Owner,
    ) {
        self.load_error.set(None);
        let tx_id = anchor.id.clone();
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome =
                    blocking(move || load_edit_bundle(db.clone().as_deref(), &tx_id, anchor)).await;
                let bundle = match outcome {
                    Ok(Ok(bundle)) => bundle,
                    Ok(Err(e)) => {
                        self.load_error
                            .set(Some(format!("{LOAD_FAILED_PREFIX}{e}")));
                        return;
                    }
                    Err(_) => {
                        self.load_error.set(Some(format!(
                            "{LOAD_FAILED_PREFIX}内部错误：读任务异常退出"
                        )));
                        return;
                    }
                };
                let EditLoad {
                    trade,
                    anchor,
                    references,
                } = bundle;
                // 行上币种决定金额/费用文本的换算小数位（账户币 = 记账币种
                // ，旧壳 centsToYuan 同口径）——decimals 在参考四面仍 owned
                // 时算好带回填，避免回填再读源信号的次序耦合。
                let decimals = CurrencyTable::from_currencies(&references.currencies)
                    .format_of(&anchor.currency_code)
                    .decimal_places;
                self.apply_references(references, sources);
                self.backfill_edit(trade, anchor, decimals);
            })
        });
    }

    /// 打开创建表单（#2047 明细页签「记买入/记卖出」入口落位；kind 由入口
    /// 单点表达、弹窗内无切换——旧壳 #150 同语义）。**参考先载再开窗**：参
    /// 考四面（账户/标的/币种/储蓄目标绑定账户）与编辑装载同一个阻塞批、
    /// 成功后开窗；失败不开窗 + [`Self::load_error`] 上抛明细页签条位（同
    /// 旧壳 create 编排「弹窗组件明细取数不经视图」同口径——明细取数经本
    /// 表单自身批装载，⑯ 消费面）。任务挂 `view_owner`——调用方视图实例
    /// owner，取数期间弹窗未挂载无取消竞态。
    pub(crate) fn open_create(
        self,
        kind: ledger_transaction::TransactionKind,
        sources: TradeFormSources,
        view_owner: Owner,
    ) {
        self.load_error.set(None);
        self.kind.set(kind);
        self.editing_id.set(None);
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || load_form_references(db.clone().as_deref())).await;
                match outcome {
                    Ok(Ok(references)) => {
                        self.apply_references(references, sources);
                        self.error.set(None);
                        self.open.set(true);
                    }
                    Ok(Err(e)) => {
                        self.load_error
                            .set(Some(format!("{LOAD_FAILED_PREFIX}{e}")));
                    }
                    Err(_) => self.load_error.set(Some(format!(
                        "{LOAD_FAILED_PREFIX}内部错误：读任务异常退出"
                    ))),
                }
            });
        });
    }

    /// 参考四面进源信号束（编辑与创建两入口的装载完成线；delete 删除接线
    /// 即红钉 lib 探针——候选面为空则表单不可用）。
    fn apply_references(self, references: FormReferences, sources: TradeFormSources) {
        sources.accounts.set(Accounts(references.accounts));
        sources.instruments.set(Instruments(references.instruments));
        sources
            .currencies
            .set(CurrencyTable::from_currencies(&references.currencies));
        sources
            .goal_accounts
            .set(GoalAccounts(references.goal_accounts));
    }

    /// 编辑回填（issue #180 旧壳 useInvestmentForm 回填段同族）：全部业务
    /// 字段随行。数量/价格文本回填不显红态（存储值在本仓录入路径下必在各
    /// 自精度口径内；极端历史数据如实红显），金额（基金形态）与费用同理。
    /// 基金判定随明细带出类型锁定（不依赖候选面命中）——编辑回填后选择框
    /// 直接显示「代码 · 名称」，不依赖远程搜索候选（旧壳同语义，候选面本
    /// 就全集现成）。
    fn backfill_edit(
        self,
        trade: ledger_investment::TransactionTrade,
        tx: TradeEditAnchor,
        decimals: u32,
    ) {
        self.kind.set(tx.kind);
        self.editing_id.set(Some(tx.id.clone()));
        self.source_order_no.set(tx.source_order_no.clone());
        self.account_id.set(tx.account_id.clone());
        self.funding_account_id.set(tx.funding_account_id.clone());
        self.instrument_id.set(Some(trade.instrument_id.clone()));
        self.fund_locked.set(Some(trade.instrument_type == "fund"));
        self.quantity_text.set(format!("{}", trade.quantity));
        if trade.instrument_type == "fund" {
            // 基金形态：确认单金额是权威输入（回填行上金额），单价不回填
            // ——展示值由反算闭包按金额/份额/费用现算（存储同公式）。
            self.amount_text
                .set(assembly::cents_to_amount_text(tx.amount_cents, decimals));
            self.price_text.set(String::new());
        } else {
            // 其余形态：单价回填（价格列万分之一元刻度 → 元文本单点换算），
            // 金额恒空（展示面自动计算）。
            self.amount_text.set(String::new());
            self.price_text
                .set(assembly::price_cents_to_yuan_text(trade.price_cents));
        }
        self.fee_text.set(
            trade
                .fee_cents
                .map(|cents| assembly::cents_to_amount_text(cents, decimals))
                .unwrap_or_default(),
        );
        self.note.set(tx.note.clone().unwrap_or_default());
        self.date.set(
            chrono::NaiveDate::parse_from_str(&tx.date, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::Local::now().date_naive()),
        );
        self.error.set(None);
        self.open.set(true);
    }

    /// 关窗（ESC / ✕ / 取消 / 成功四路径归一）。
    pub(crate) fn close(&self) {
        self.open.set(false);
    }
}

// ---------------------------------------------------------------------------
// 判定与派生（浅层派生归本面闭集；换算与装配归 assembly 单点）
// ---------------------------------------------------------------------------

/// 可选金额字段红态（判定非 `Ok` 且已标记；可选形态空值放行——费用不填
/// 合法，金额在非基金形态无输入面）。
fn optional_amount_error(text: String, marked: bool) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    !matches!(
        assembly::judge_amount_text(&text),
        assembly::AmountJudgment::Ok(_)
    ) && marked
}

/// 是否场外基金（录入权威分流的开关）：编辑面按明细带出类型锁定；创建面
/// 随标的字典响应式（选中标的未命中字典按非基金——不稳定判定不猜测）。
fn is_fund_computed(state: &TradeFormState, sources: &TradeFormSources) -> Memo<bool> {
    let state = *state;
    let sources = *sources;
    computed(move || {
        if let Some(locked) = state.fund_locked.get() {
            return locked;
        }
        let Some(id) = state.instrument_id.get() else {
            return false;
        };
        sources
            .instruments
            .get()
            .0
            .iter()
            .find(|i| i.id == id)
            .is_some_and(|i| i.kind == ledger_investment::InstrumentType::Fund)
    })
}

/// 任意字段处于错误态 → 保存按钮禁用（红框 + 提交禁用两件同发，ADR-0058
/// 决策 1）。判定装配走判定闭集的 [`assembly::QuantityJudgment::has_error_at`]
/// / [`assembly::PriceJudgment::has_error_at`]（形状非法即时红、空值红在失
/// 焦或提交尝试后——旧壳 useFieldErrors 时机同源）。数量（两形态共用）必
/// 判；价格仅非基金参与（基金无单价输入面）；金额仅基金参与（旧壳金额是
/// NInputNumber 无红态，本壳文本承载后一并接入判定——提示红框早拦，偏差
/// 留痕见模块头）；费用可选恒判（空放行）。
fn has_field_error(state: &TradeFormState, is_fund: Memo<bool>) -> Memo<bool> {
    let state = *state;
    computed(move || {
        let fund = is_fund.get();
        let quantity_bad = assembly::judge_quantity_text(state.quantity_text.get().as_str())
            .has_error_at(state.quantity_marked.get());
        let price_bad = !fund
            && assembly::judge_price_text(state.price_text.get().as_str())
                .has_error_at(state.price_marked.get());
        let amount_bad =
            fund && optional_amount_error(state.amount_text.get(), state.amount_marked.get());
        let fee_bad = optional_amount_error(state.fee_text.get(), state.fee_marked.get());
        quantity_bad || price_bad || amount_bad || fee_bad
    })
}

/// 基金反算单价（元文本）：与后端 prepare 同一公式——(金额 ∓ 手续费) × 100
/// ÷ 份额，万分之一元刻度单次舍入；买入减费（净投入）、卖出加费（费在收
/// 入外另收）。非基金形态或输入不完整时 None（表单此时展示占位词）。
fn derived_price_text(state: &TradeFormState, is_fund: Memo<bool>) -> Option<String> {
    if !is_fund.get() {
        return None;
    }
    let amount_cents = assembly::amount_text_to_cents(&state.amount_text.get())?;
    let quantity = match assembly::judge_quantity_text(state.quantity_text.get().as_str()) {
        assembly::QuantityJudgment::Ok(quantity) => quantity,
        _ => return None,
    };
    if quantity <= 0.0 {
        return None;
    }
    let fee_cents = if state.fee_text.get().trim().is_empty() {
        0
    } else {
        assembly::amount_text_to_cents(&state.fee_text.get()).unwrap_or(0)
    };
    let base_cents = match state.kind.get() {
        ledger_transaction::TransactionKind::Sell => amount_cents + fee_cents,
        _ => amount_cents - fee_cents,
    };
    if base_cents <= 0 {
        return None;
    }
    // 金额（分）→ 价格（万分之一元）：× 100 后按份额均摊（单次舍入）。
    let price_units = ((base_cents * 100) as f64 / quantity).round() as i64;
    Some(assembly::price_cents_to_yuan_text(price_units))
}

/// 非基金形态的成交金额展示（自动计算位）：数量 × 单价 ± 手续费（分域一
/// 次舍入——旧 shell investmentAmount 同公式同序）。输入不完整/换算失败不
/// 展示（占位「-」——不猜零）。
fn auto_amount_text(
    state: &TradeFormState,
    fmt: &CurrencyFormat,
    is_fund: Memo<bool>,
) -> Option<String> {
    if is_fund.get() {
        return None;
    }
    let quantity = match assembly::judge_quantity_text(state.quantity_text.get().as_str()) {
        assembly::QuantityJudgment::Ok(quantity) => quantity,
        _ => return None,
    };
    let price_units = assembly::price_text_to_price_cents(state.price_text.get().as_str())?;
    let fee_cents = if state.fee_text.get().trim().is_empty() {
        0
    } else {
        assembly::amount_text_to_cents(&state.fee_text.get()).unwrap_or(0)
    };
    // 万分之一元刻度的价格 → 分：÷ 100；买卖换算：买入加费、卖出减费
    //（旧壳 investmentAmount 同公式同序）；展示用统一金额格式化口径
    // [`assembly::format_amount_basic`]（币种 = 所在账户币）。
    let raw_cents = (quantity * price_units as f64 / 100.0).round() as i64
        + match state.kind.get() {
            ledger_transaction::TransactionKind::Buy => fee_cents,
            _ => -fee_cents,
        };
    Some(assembly::format_amount_basic(raw_cents, fmt))
}

// ---------------------------------------------------------------------------
// 提交
// ---------------------------------------------------------------------------

/// 提交装配：表单状态 → [`assembly::assemble_trade_transaction`]（fail
/// fast 词表归装配器；kind 方向词「买入」/「卖出」由本面传入）。
fn assemble_input(
    state: &TradeFormState,
    sources: &TradeFormSources,
    is_fund: Memo<bool>,
) -> Result<ledger_transaction::TransactionInput, String> {
    let account_id = state.account_id.get();
    let currency_code = account_id
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
        .unwrap_or_else(|| "CNY".to_owned());
    let submit = assembly::TradeFormSubmit {
        kind: state.kind.get(),
        is_fund: is_fund.get(),
        currency_code,
        account_id,
        instrument_id: state.instrument_id.get(),
        funding_account_id: state.funding_account_id.get(),
        amount_text: state.amount_text.get(),
        quantity_text: state.quantity_text.get(),
        price_text: state.price_text.get(),
        fee_text: state.fee_text.get(),
        note: state.note.get(),
        date: state.date.get(),
        source_order_no: state.source_order_no.get(),
    };
    assembly::assemble_trade_transaction(submit)
        .map_err(|e| e.as_text(kind_word(state.kind.get()), is_fund.get()))
}

/// 提交执行（红态静默守卫 → 前置校验 → 域公开写入口直调 → 成功广播失效
/// 信号；失败错误条位。任务挂 `view_owner`——弹窗子树卸载不取消，交易删
/// 除先例同形）。
fn perform_submit(
    state: TradeFormState,
    sources: TradeFormSources,
    is_fund: Memo<bool>,
    view_owner: Owner,
) -> impl Fn() + Copy + 'static {
    move || {
        state.error.set(None);
        // 提交尝试即触发空值兜底红态（saveAttempted 输入）。
        state.amount_marked.set(true);
        state.quantity_marked.set(true);
        state.price_marked.set(true);
        state.fee_marked.set(true);
        // 格式类错误由「红框 + 提交禁用」拦截（ADR-0058 决策 1/3）：错误态
        // 下静默中止提交（先于账户/标的提示，与旧壳同序——红框已在字段上
        // 呈现，账户提示延后到格式修正后的下次尝试）。
        if has_field_error(&state, is_fund).get() {
            return;
        }
        let input = match assemble_input(&state, &sources, is_fund) {
            Ok(input) => input,
            Err(text) => {
                state.error.set(Some(text));
                return;
            }
        };
        let editing = state.editing_id.get();
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
                        // 失效信号广播（交易写路径统一接线点：列表重读；删
                        // 除即红条目钉在 tests/app_headless.rs 既有交易写路
                        // 径用例，本模块不新增广播面——持仓页不随买卖写重
                        // 拉，旧壳同口径）。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => {
                        let prefix = if state.editing_id.get_untracked().is_some() {
                            SAVE_FAILED_PREFIX
                        } else {
                            RECORD_FAILED_PREFIX
                        };
                        state.error.set(Some(format!("{prefix}{e}")));
                    }
                    Err(_) => state
                        .error
                        .set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------
// 候选面
// ---------------------------------------------------------------------------

/// 账户候选投影（可见 − 归档 × 投资账户 + 编辑面补值——已存值恰是归档账户
/// 时补回并标「已归档」，值原样保留；txn_form 同形先例。旧壳候选谓词 =
/// 可选择 × 投资账户（spec #1952 / ADR-0142 决策 4）。
fn pinned_investment_account_options(
    rows: &[ledger_accounts::Account],
    selected: Option<&str>,
) -> Vec<CategoryOption> {
    let mut options: Vec<CategoryOption> = rows
        .iter()
        .filter(|a| !a.is_archived && a.kind == ledger_accounts::AccountType::Investment)
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && !options.iter().any(|o| o.id == id)
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

/// 出资账户候选投影：可见 − 归档 − 投资账户（出资人不是投资账户本账——
/// 旧壳 fundingCandidateAccounts 谓词同族（结算账户面））+ 同币种过滤（出
/// 资币种一致是记账币种一致的前提，旧壳 #1191 候选准入同源）− 储蓄目标绑
/// 定防呆（旧壳 goalAccountIds 排除同源；后端行为层准入是唯一权威）+ 编辑
/// 面补值（已存值恰是归档/越侧账户时补回标「已归档」）。币种 = 所选投资
/// 账户币种（issue #1191）；未选账户前不收窄（选择顺序自由）。
fn pinned_funding_options(
    accounts: &Accounts,
    goal_accounts: &GoalAccounts,
    currency: Option<&str>,
    selected: Option<&str>,
) -> Vec<CategoryOption> {
    let mut options: Vec<CategoryOption> = accounts
        .0
        .iter()
        .filter(|a| {
            !a.is_archived
                && a.kind != ledger_accounts::AccountType::Investment
                && currency.is_none_or(|c| a.currency_code == c)
                && !goal_accounts.0.iter().any(|g| g == &a.id)
        })
        .map(|a| CategoryOption {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let Some(id) = selected
        && !options.iter().any(|o| o.id == id)
        && let Some(row) = accounts.0.iter().find(|a| a.id == id)
    {
        options.push(CategoryOption {
            id: row.id.clone(),
            label: format!("{}（已归档）", row.name),
        });
    }
    options
}

/// 标的候选投影（全集 →「代码 · 名称」CategoryOption；名称可空退化裸代码——
/// 盈亏标的候选投影同款）。
fn instrument_options(instruments: &Instruments) -> Vec<CategoryOption> {
    instruments
        .0
        .iter()
        .map(|i| CategoryOption {
            id: i.id.clone(),
            label: match i.name.as_deref().filter(|n| !n.trim().is_empty()) {
                Some(name) => format!("{} · {}", i.symbol, name),
                None => i.symbol.clone(),
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 弹窗
// ---------------------------------------------------------------------------

/// 表单字段标签（弱化小字；txn_form field_label 同形）。
fn field_label(label: &'static str, muted: AdaptiveColor) -> impl IntoElement {
    text(label)
        .text_size(px(font_size::CAPTION))
        .text_color(muted)
}

/// 弹窗标题（创建「记一笔 · {词}」/ 编辑「编辑交易」——txn_form 同形，旧
/// 壳词表同源）。
fn modal_title(editing: bool, kind_word: &str) -> String {
    if editing {
        "编辑交易".to_owned()
    } else {
        format!("记一笔 · {kind_word}")
    }
}

/// 提交按钮文案（创建 = 记{类型词}；编辑 = 保存修改——旧壳 submitBuy /
/// submitSell / saveEdit 同词表）。
fn submit_label(editing: bool, kind_word: &str) -> String {
    if editing {
        SAVE_EDIT.to_owned()
    } else {
        format!("记{kind_word}")
    }
}

/// 判定红态输入框边框类（红框逐字段落位；正常态回标准边框）。
fn error_border(bad: bool) -> &'static str {
    if bad {
        "bg-background rounded-md border border-destructive px-2 py-1 \
         placeholder:text-muted-foreground"
    } else {
        "bg-background rounded-md border border-border px-2 py-1 \
         placeholder:text-muted-foreground"
    }
}

/// 表单弹窗挂载面（调用方视图树内消费；`view_owner` 是调用方视图实例
/// owner——写任务经它挂载，弹窗子树卸载不取消）。
pub(crate) fn trade_form_modal(
    state: TradeFormState,
    sources: TradeFormSources,
    view_owner: Owner,
) -> impl IntoElement {
    // 是否基金（表单功能分流；判定面归 is_fund_computed 单点）。
    let is_fund = is_fund_computed(&state, &sources);
    // 任意字段错误 → 提交禁用（红框 + 提交禁用两件同发，ADR-0058 决策 1）。
    let field_error = has_field_error(&state, is_fund);
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    let danger_fg = crate::theme::color("destructive");

    let submit = perform_submit(state, sources, is_fund, view_owner);

    modal::app_modal(
        move || state.open.get(),
        move || {
            modal_title(
                state.editing_id.get().is_some(),
                kind_word(state.kind.get()),
            )
        },
        move || state.close(),
        move || {
            // 字段族随基金形态分流（is_fund 响应式——选中标的带出类型即重
            // 排，旧壳 isFundInstrument 同语义）。
            let fund = is_fund.get();
            // 金额红态（基金形态；非基金无输入面）。
            let amount_error =
                fund && optional_amount_error(state.amount_text.get(), state.amount_marked.get());
            let quantity_error = assembly::judge_quantity_text(state.quantity_text.get().as_str())
                .has_error_at(state.quantity_marked.get());
            let price_error = !fund
                && assembly::judge_price_text(state.price_text.get().as_str())
                    .has_error_at(state.price_marked.get());
            let fee_error = optional_amount_error(state.fee_text.get(), state.fee_marked.get());
            // 币种展示（账户推导锁定只读——旧壳 disabled AppSelect 同语义；
            // 未选账户回空）。
            let currency_text = state
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
                        .map(|a| a.currency_code.clone())
                })
                .unwrap_or_default();
            // 金额（基金 = 权威输入原样文本；其余 = 自动计算只读说明——旧
            // 壳 disabled NInputNumber 同语义，文本格式化走统一金额口径）。
            let auto_amount_fmt = sources.currencies.get_untracked().format_of(&currency_text);
            let auto_amount = auto_amount_text(&state, &auto_amount_fmt, is_fund);
            let amount_row = view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label(LABEL_AMOUNT, muted))
                .child(if fund {
                    input()
                        .test_id(AMOUNT_ID)
                        .value(move || state.amount_text.get())
                        .placeholder(AMOUNT_PLACEHOLDER)
                        .optional_label("确认单金额")
                        .text_color(fg)
                        .w(px(160.0))
                        .app_class(error_border(amount_error))
                        .on_change(move |value: &str| state.amount_text.set(value.to_owned()))
                        .on_blur(move || state.amount_marked.set(true))
                        .into_element()
                } else {
                    text(auto_amount.unwrap_or_else(|| AMOUNT_AUTO.to_owned()))
                        .text_size(px(font_size::SECONDARY))
                        .text_color(muted)
                        .test_id(AUTO_AMOUNT_ID)
                        .into_element()
                })
                .child(
                    view().test_id(CURRENCY_ID).child(
                        text(currency_text)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(muted),
                    ),
                );
            let body = view()
                .test_id(FORM_ID)
                .flex_col()
                .gap_2()
                .child(amount_row)
                // 投资账户（可搜下拉；币种随之推导、锁定只读）。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(LABEL_ACCOUNT, muted))
                        .child(pinyin_select::pinyin_select(
                            ACCOUNT_PREFIX,
                            move || {
                                pinned_investment_account_options(
                                    &sources.accounts.get().0,
                                    state.account_id.get().as_deref(),
                                )
                            },
                            state.account_id,
                            ACCOUNT_PLACEHOLDER,
                            false,
                        )),
                )
                // 出资账户（可选，可清空；候选同币种 − 目标绑定 + 编辑补值）。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(LABEL_FUNDING, muted))
                        .child(pinyin_select::pinyin_select(
                            FUNDING_PREFIX,
                            move || {
                                // 出资候选的币种收窄锚 = 记账币种（= 所选投
                                // 资账户币种，issue #1191）；未选账户不收窄。
                                let currency = state.account_id.get().as_deref().and_then(|id| {
                                    sources
                                        .accounts
                                        .get()
                                        .0
                                        .iter()
                                        .find(|a| a.id == id)
                                        .map(|a| a.currency_code.clone())
                                });
                                pinned_funding_options(
                                    &sources.accounts.get(),
                                    &sources.goal_accounts.get(),
                                    currency.as_deref(),
                                    state.funding_account_id.get().as_deref(),
                                )
                            },
                            state.funding_account_id,
                            FUNDING_PLACEHOLDER,
                            true,
                        )),
                )
                // 标的（可搜下拉；全量候选 + 本地模糊搜索——空态词表随封装
                // 占位承载）。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(LABEL_INSTRUMENT, muted))
                        .child(pinyin_select::pinyin_select(
                            INSTRUMENT_PREFIX,
                            move || instrument_options(&sources.instruments.get()),
                            state.instrument_id,
                            INSTRUMENT_PLACEHOLDER,
                            true,
                        )),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(
                            if fund { LABEL_SHARES } else { LABEL_QUANTITY },
                            muted,
                        ))
                        .child(
                            input()
                                .test_id(QUANTITY_ID)
                                .value(move || state.quantity_text.get())
                                .placeholder(if fund {
                                    SHARES_PLACEHOLDER
                                } else {
                                    QUANTITY_PLACEHOLDER
                                })
                                .optional_label("数量/份额")
                                .text_color(fg)
                                .w(px(160.0))
                                .app_class(error_border(quantity_error))
                                .on_change(move |value: &str| {
                                    state.quantity_text.set(value.to_owned())
                                })
                                .on_blur(move || state.quantity_marked.set(true)),
                        ),
                );
            let body = if fund {
                // 基金形态：单价（净值）反算只读（后端反算不落 wire，与存
                // 储同公式；旧壳 derivedPrice disabled 同语义）。
                body.child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(LABEL_UNIT_PRICE_NAV, muted))
                        .child(
                            view().test_id(DERIVED_PRICE_ID).child(
                                text(
                                    derived_price_text(&state, is_fund)
                                        .unwrap_or_else(|| DERIVED_PRICE_PLACEHOLDER.to_owned()),
                                )
                                .text_size(px(font_size::SECONDARY))
                                .text_color(muted),
                            ),
                        ),
                )
            } else {
                // 其余形态：单价权威输入（数量红态同族，万分之一元刻度）。
                body.child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label(LABEL_UNIT_PRICE, muted))
                        .child(
                            input()
                                .test_id(PRICE_ID)
                                .value(move || state.price_text.get())
                                .placeholder(UNIT_PRICE_PLACEHOLDER)
                                .optional_label("单价")
                                .text_color(fg)
                                .w(px(160.0))
                                .app_class(error_border(price_error))
                                .on_change(move |value: &str| {
                                    state.price_text.set(value.to_owned())
                                })
                                .on_blur(move || state.price_marked.set(true)),
                        ),
                )
            };
            body.child(
                // 手续费（可空：空 = fee_cents 不填而非 0；红框可选字段同族）。
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(LABEL_FEE, muted))
                    .child(
                        input()
                            .test_id(FEE_ID)
                            .value(move || state.fee_text.get())
                            .placeholder(FEE_PLACEHOLDER)
                            .optional_label("手续费")
                            .text_color(fg)
                            .w(px(160.0))
                            .app_class(error_border(fee_error))
                            .on_change(move |value: &str| state.fee_text.set(value.to_owned()))
                            .on_blur(move || state.fee_marked.set(true)),
                    ),
            )
            // 日期（可键入 + 日历点选；txn_form 同件消费）。
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(LABEL_DATE, muted))
                    .child(date_select::date_select(DATE_PREFIX, state.date)),
            )
            // 备注。
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(LABEL_NOTE, muted))
                    .child(
                        input()
                            .test_id(NOTE_ID)
                            .value(move || state.note.get())
                            .placeholder(NOTE_PLACEHOLDER)
                            .optional_label("投资交易备注")
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
                            .text_size(px(font_size::CAPTION))
                            .text_color(danger_fg)
                            .test_id(ERROR_ID)
                    },
                )
                .otherwise(view),
            )
            // 动作行：取消 + 提交（红态禁用——两件同发）。
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        nsh_base::button()
                            .app_class(
                                "px-3 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .child(
                                text("取消")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(fg)
                                    .test_id(CANCEL_ID),
                            )
                            .on_press(move |_: &()| state.close()),
                    )
                    .child(
                        nsh_base::button()
                            .disabled(move || field_error.get())
                            .app_class(
                                "px-3 py-1 rounded-md bg-primary hover:opacity-90 disabled:opacity-40",
                            )
                            .child(
                                text(move || {
                                    submit_label(
                                        state.editing_id.get().is_some(),
                                        kind_word(state.kind.get()),
                                    )
                                })
                                .text_size(px(font_size::SECONDARY))
                                .text_color(crate::theme::color("primary-foreground"))
                                .test_id(SUBMIT_ID),
                            )
                            .on_press(move |_: &()| submit()),
                    ),
            )
        },
    )
}

// ---------------------------------------------------------------------------
// lib 探针（headless 引擎内驱动；investments.rs / investments_holdings.rs
// 探针同形——表单旅程按真实组件全链路驱动，供弹窗弹态、字段绑定与写路径
// 消费同一 #[component] 路径）。
//
// 入口归属（见模块头）：旧壳 buy/sell 行的编辑入口住户投资明细页签
//（ADR-0135 主列表默认排除投资 kind，NSH 同口径——主列表触不到 buy 行，
// headless 用户路径无法走通）；本票探针直挂表单组件 + begin_edit 编排（同
// 一组件路径承载回填 / 红态 / 保存 / 失效广播），创建入口随⑯ 落位。
// 「删除即红」负向条目：删 [`begin_edit`] 的回填线任一段 → 回填断言落空
//（本探针红——明细带出数量/单价/备注缺失即红）；删 [`perform_submit`] 的
// 失效广播接线 → 写后列表失效不发生（交易列表重读接缝既有 headless 删除
// 即红条目钉在此，探针以「写后广播触发数」为断言面，本探针 assert 写库成
// 功即承载同一接缝存在性）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod probe {
    use super::*;
    use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
    use nsh_testing::Headless;

    use crate::app::{reset_book_assembly, run_startup, set_book_dir, test_assembly_guard};

    /// 探针触发载荷（目标 buy 行 id）：Root::of 的 prop 面是 Props 依托引擎
    /// 声明的字段族，探针避开引擎声明（lib 探针的载荷传递走同线程静态槽，
    /// 仅供 #[cfg(test)]）。
    static PROBE_TX_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

    /// 探针根组件：begin_edit 触发按钮 + 真实买卖表单弹窗（参考源束挂在探
    /// 针根，begin_edit 批量装载回填）。
    #[component]
    fn TradeFormProbeApp() -> impl IntoElement {
        let state = TradeFormState::new();
        let sources = TradeFormSources::new();
        let tx_id = PROBE_TX_ID
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or_default();
        view()
            .flex_1()
            .flex_col()
            .app_class("bg-background")
            .child(
                view()
                    .test_id("probe-begin-edit")
                    .on_press(move |_: &PressEvent| {
                        let db = crate::app::db_slot().clone();
                        let tx = db.as_ref().and_then(|db| list_buy_tx(db, &tx_id));
                        if let Some(tx) = tx {
                            state.begin_edit((&tx).into(), sources, Owner::detached());
                        }
                    })
                    .child(text("开始编辑")),
            )
            .child(trade_form_modal(state, sources, Owner::detached()))
    }

    /// 库内 buy 行（探针造数为唯一买行；行身份由 id 认领）。
    fn list_buy_tx(
        db: &ledger_infra::db::DbState,
        id: &str,
    ) -> Option<ledger_transaction::Transaction> {
        if id.is_empty() {
            return None;
        }
        let conn = db.conn.lock().unwrap();
        ledger_transaction::list_transactions(&conn, &Default::default())
            .ok()
            .and_then(|page| {
                page.items
                    .into_iter()
                    .find(|tx| tx.id == id && tx.kind == ledger_transaction::TransactionKind::Buy)
            })
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

    fn seed_book(dir: &std::path::Path) -> (String, String) {
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
                    market: None,
                },
            )
            .unwrap();
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
                    account_id: Some(invest.clone()),
                    instrument_id: Some(instrument.clone()),
                    quantity: Some(2.0),
                    price_cents: Some(10_000),
                    fee_cents: Some(0),
                    date: "2026-01-05".to_owned(),
                    note: Some("买入原款".to_owned()),
                    ..probe_input()
                },
            )
            .unwrap();
        }
        (invest, instrument)
    }

    /// 表单编辑旅程：回填全部业务字段（账户/标的/数量/单价/备注）后改数量
    /// 与单价提交 → 弹窗关 -> 库态核对（全字段替换落库：数量 3、单价 ¥2、
    /// 币 CNY 由账户推导）。删除即红条目见模块头注。
    #[test]
    fn trade_form_backfills_edit_and_saves() {
        let _guard = test_assembly_guard();
        reset_book_assembly();
        let dir = std::env::temp_dir().join(format!(
            "nsh-app-trade-probe-{}",
            ledger_infra::ids::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ledger.db"), b"").unwrap();
        let (account_id, instrument_id) = seed_book(&dir);

        set_book_dir(dir);
        assert!(matches!(run_startup(), crate::app::StartupView::Ready));

        // 探针按钮的载荷 = 目标 buy 行 id（静态槽，见 PROBE_TX_ID 注）：测试
        // 内先查库取 id 再启动引擎（引擎线程组件体读同进程静态面）。
        {
            let session = crate::app::session_handle().unwrap();
            let conn = session.conn.lock().unwrap();
            let id: String = conn
                .query_row(
                    "SELECT id FROM transactions WHERE kind='buy' LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            *PROBE_TX_ID.lock().unwrap() = Some(id);
        }
        let _ = (account_id.clone(), instrument_id.clone());

        let mut app = Headless::start(EngineConfig {
            viewport: nsh_engine::Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
            environment: Environment {
                os: Os::Linux,
                idiom: Idiom::Desktop,
                ..Environment::default()
            },
            ui_sets: vec![nsh_base::registry()],
            ..EngineConfig::new(Root::of::<TradeFormProbeApp>())
        })
        .unwrap();
        app.wait_for("text:开始编辑").unwrap();
        app.press("id:probe-begin-edit").unwrap();
        app.wait_for("text:编辑交易").unwrap();
        app.wait_for("id:trade-form-quantity").unwrap();

        // 回填断言（先取明细再开窗的装载完成态）：全部业务字段随行。
        assert_eq!(
            app.screen().input_text("id:trade-form-quantity").unwrap(),
            Some("2"),
            "数量回填"
        );
        assert_eq!(
            app.screen().input_text("id:trade-form-price").unwrap(),
            Some("1"),
            "单价回填（万位刻度 → 元文本）"
        );
        assert_eq!(
            app.screen().input_text("id:trade-form-note").unwrap(),
            Some("买入原款"),
            "备注回填"
        );

        // 修改提交：数量 3、单价 2 → 弹窗关、库态核对。
        app.clear("id:trade-form-quantity").unwrap();
        app.type_text("id:trade-form-quantity", "3").unwrap();
        app.clear("id:trade-form-price").unwrap();
        app.type_text("id:trade-form-price", "2").unwrap();
        app.press("id:trade-form-submit").unwrap();
        app.wait_for_gone("text:编辑交易", std::time::Duration::from_secs(5))
            .unwrap();
        {
            let session = crate::app::session_handle().unwrap();
            let conn = session.conn.lock().unwrap();
            let tx_id: String = conn
                .query_row(
                    "SELECT id FROM transactions WHERE kind='buy' LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let trade = ledger_investment::get_transaction_trade(&conn, &tx_id).unwrap();
            assert_eq!(trade.quantity, 3.0, "数量全字段替换落库");
            assert_eq!(trade.price_cents, 20_000, "单价万位刻度落库：¥2 = 20_000");
        }
        let _ = account_id;
    }
}
