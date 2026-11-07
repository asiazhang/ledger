//! 交易表单弹窗（issue #2034，NSH 迁移③表单①基础收支）：单账户收入/支出的
//! 新增与编辑——日期、类型（入口锁定）、金额、分类、账户、备注与来源关联
//! 语义（商户 / 保单 / 来源订单号透传）。
//!
//! **形态裁决**：弹窗挂交易列表视图入口（工具行「记一笔」弹类型菜单两项；
//! 行右键「编辑」仅对收入/支出行开放——转账/退款随表单②、投资形态随表单③
//! 就位时扩展），呈现用 [`crate::modal`] 编辑类模态（旧壳 cardSize sm 档同
//! 位；新增与编辑同形态，编辑回填初值——旧壳「编辑复用同表单」语义）。
//!
//! 状态与所有权：表单信号束住调用方视图实例（与预算视图表单态同形），本模
//! 块只提供信号束、开关动作与弹窗挂载面；写路径经 [`crate::boot`] 域公开写
//! 入口直调（不经 IPC），重活走 `blocking` 后台线程、任务挂**视图实例
//! owner**（弹窗子树随关窗卸载不取消写任务，交易删除先例同形）；成功广播
//! [`crate::signals`] 交易失效信号（列表重读——「写→失效信号→列表重读」的
//! 广播接线点，删除即红条目钉在 tests/app_headless.rs）。
//!
//! 装配纪律：wire 字段拼装收口 [`crate::assembly::assemble_basic_transaction`]
//! 单点（per-kind 矩阵 / 元转分 / 日期转换不散表单）；金额红态走判定族
//! [`crate::assembly::judge_amount_text`]（红框 + 提交禁用，ADR-0058 两件同
//! 发）；业务类校验（账户未选 / 金额非正 / 换算兜底）与域码化错误走错误条
//! 位（旧壳 toast 同语义的条位呈现）。币种随所选账户推导锁定只读（记账币种
//! = 账户币种，ADR-0134 决策 5）。
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
const CATEGORY_PREFIX: &str = "tx-form-category";
const MERCHANT_PREFIX: &str = "tx-form-merchant";
const POLICY_PREFIX: &str = "tx-form-policy";
const DATE_PREFIX: &str = "tx-form-date";
const NOTE_ID: &str = "tx-form-note";
const SUBMIT_ID: &str = "tx-form-submit";
const ERROR_ID: &str = "tx-form-error";
/// 表单数据载荷锚点（回填断言用：弹窗内只读字段行）。
const MERCHANT_LABEL_ID: &str = "tx-form-merchant-label";

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
                    && a.category_id == b.category_id
                    && a.merchant_id == b.merchant_id
                    && a.policy_id == b.policy_id
                    && a.note == b.note
                    && a.date == b.date
                    && a.source_order_no == b.source_order_no
            })
    }
}

/// 账户源投影（id / 名 / 币种 / 归档位——候选面与币种推导的消费字段）。
#[derive(Clone, Default)]
pub(crate) struct FormAccounts(pub(crate) Vec<ledger_accounts::Account>);

impl PartialEq for FormAccounts {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| {
                a.id == b.id
                    && a.name == b.name
                    && a.currency_code == b.currency_code
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

/// 表单信号束（住调用方视图实例；Copy 句柄可随意移入闭包）。缺省关窗、
/// 支出 kind、今天日期。
#[derive(Clone, Copy)]
pub(crate) struct TxnFormState {
    /// 弹窗开合（受控；关窗统一置回——模态封装消费）。
    pub open: Signal<bool>,
    /// 提交 kind（创建入口锁定 / 编辑按行锁定；表单内无切换控件——旧壳
    /// #150「类型由入口单点表达，中途换类型 = 关闭重开」同语义）。
    pub kind: Signal<ledger_transaction::TransactionKind>,
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
    /// 账户（必选）。
    pub account_id: Signal<Option<String>>,
    /// 分类（可选；None = 无分类交易）。
    pub category_id: Signal<Option<String>>,
    /// 商户输入文本（自由文本语义的原样承载）。
    pub merchant_typed: Signal<String>,
    /// 商户选定建议项 id（键入即失效——[`combo_select`] 双态值语义）。
    pub merchant_selected: Signal<Option<String>>,
    /// 保单（可选挂单）。
    pub policy_id: Signal<Option<String>>,
    /// 备注。
    pub note: Signal<String>,
    /// 交易日期（缺省今天，旧壳 NDatePicker 缺省同语义）。
    pub date: Signal<chrono::NaiveDate>,
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
}

impl TxnFormState {
    /// 表单信号束（缺省关窗、支出 kind、今天日期）。
    pub(crate) fn new() -> Self {
        Self {
            open: signal(false),
            kind: signal(ledger_transaction::TransactionKind::Expense),
            editing_id: signal(None),
            source_order_no: signal(None),
            editing_merchant_id: signal(None),
            amount_text: signal(String::new()),
            amount_marked: signal(false),
            account_id: signal(None),
            category_id: signal(None),
            merchant_typed: signal(String::new()),
            merchant_selected: signal(None),
            policy_id: signal(None),
            note: signal(String::new()),
            date: signal(chrono::Local::now().date_naive()),
            error: signal(None),
        }
    }

    /// 打开新增表单（kind 由入口锁定；全部字段回默认态——旧壳「记一笔」开窗
    /// 重建实例同语义）。
    pub(crate) fn open_create(&self, kind: ledger_transaction::TransactionKind) {
        self.kind.set(kind);
        self.editing_id.set(None);
        self.source_order_no.set(None);
        self.editing_merchant_id.set(None);
        self.amount_text.set(String::new());
        self.amount_marked.set(false);
        self.account_id.set(None);
        self.category_id.set(None);
        self.merchant_typed.set(String::new());
        self.merchant_selected.set(None);
        self.policy_id.set(None);
        self.note.set(String::new());
        self.date.set(chrono::Local::now().date_naive());
        self.error.set(None);
        self.open.set(true);
    }

    /// 打开编辑表单（回填既有行全部业务字段；kind 随行锁定）。`merchant_label`
    /// 是原商户的显示名（含软删全量口径解析；None = 行上无商户引用）。
    pub(crate) fn open_edit(
        &self,
        tx: &ledger_transaction::Transaction,
        currencies: &CurrencyTable,
        merchant_label: Option<String>,
    ) {
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
        self.amount_marked.set(false);
        self.account_id.set(tx.account_id.clone());
        self.category_id.set(tx.category_id.clone());
        self.merchant_typed.set(merchant_label.unwrap_or_default());
        self.merchant_selected.set(tx.merchant_id.clone());
        self.policy_id.set(tx.policy_id.clone());
        self.note.set(tx.note.clone().unwrap_or_default());
        // 日期回填：行上 ISO 文本解析；解析失败保持今天（数据侧契约文本恒可
        // 解析，防御兜底不静默造错值）。
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
        let currency = selected_currency(&state, &sources);
        let merchant = merchant_ref(
            state.editing_merchant_id.get().as_deref(),
            state.merchant_selected.get().as_deref(),
            &state.merchant_typed.get(),
        );
        let submit = BasicFormSubmit {
            kind: state.kind.get(),
            amount_text: state.amount_text.get(),
            account_id: state.account_id.get(),
            category_id: state.category_id.get(),
            merchant,
            policy_id: state.policy_id.get(),
            note: state.note.get(),
            date: state.date.get(),
            source_order_no: state.source_order_no.get(),
        };
        // 装配器 fail fast（账户/金额业务校验与换算兜底；红态拦截是第一道
        // 防线，错误条位是本拦截的呈现位）。
        let input = match assembly::assemble_basic_transaction(submit, currency) {
            Ok(input) => input,
            Err(e) => {
                state.error.set(Some(e.as_text().to_owned()));
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

/// 币种随所选账户推导（记账币种 = 账户币种，ADR-0134 决策 5；无账户缺省
/// CNY——默认币种缺 key 回默认 CNY 同口径，有账户上下文的提交必选账户）。
fn selected_currency(state: &TxnFormState, sources: &TxnFormSources) -> String {
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
                .map(|a| a.currency_code.clone())
        })
        .unwrap_or_else(|| "CNY".to_owned())
}

/// 表单字段标签（弱化小字；预算视图同形小工具——封装层尚无 form 原语，
/// 不为此立新封装件）。
fn field_label(label: &'static str, muted: AdaptiveColor) -> impl IntoElement {
    text(label).text_size(px(12.0)).text_color(muted)
}

/// 提交按钮文案（创建 = 记支出/记收入；编辑 = 保存修改——旧壳词表同形）。
fn submit_label(kind: ledger_transaction::TransactionKind, editing: bool) -> &'static str {
    match (editing, kind) {
        (true, _) => "保存修改",
        (false, ledger_transaction::TransactionKind::Income) => "记收入",
        (false, _) => "记支出",
    }
}

/// 弹窗标题（创建「记一笔 · {kind}」/ 编辑「编辑交易」——旧壳词表同形）。
fn modal_title(kind: ledger_transaction::TransactionKind, editing: bool) -> String {
    if editing {
        "编辑交易".to_owned()
    } else {
        let label = match kind {
            ledger_transaction::TransactionKind::Income => "收入",
            _ => "支出",
        };
        format!("记一笔 · {label}")
    }
}

/// 表单弹窗挂载面（调用方视图树内消费；`view_owner` 是调用方视图实例 owner
/// ——写任务经它挂载，弹窗子树卸载不取消）。
pub(crate) fn transaction_form_modal(
    state: TxnFormState,
    sources: TxnFormSources,
    view_owner: Owner,
) -> impl IntoElement {
    // 账户候选面：可见 − 归档（旧壳 selectableAccounts 口径）+ 编辑面补值
    // ——已存值恰是归档账户时补回选项并标「已归档」，值原样保留
    // （spec #1952 / ADR-0142 决策 4）。
    let account_options = computed(move || {
        let rows = sources.accounts.get().0;
        let mut options: Vec<CategoryOption> = rows
            .iter()
            .filter(|a| !a.is_archived)
            .map(|a| CategoryOption {
                id: a.id.clone(),
                label: a.name.clone(),
            })
            .collect();
        let selected = state.account_id.get();
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
    // 币种展示（随所选账户推导锁定只读——后端 Writer 守卫是唯一权威，前端
    // 只读呈现）。
    let currency_text = move || selected_currency(&state, &sources);
    // 金额红态（判定族单点；红框 + 提交禁用两件同发，ADR-0058 决策 1）。
    let amount_error = move || amount_has_error(&state);
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

    modal::app_modal(
        move || state.open.get(),
        move || modal_title(state.kind.get(), state.editing_id.get().is_some()),
        move || state.close(),
        move || {
            view()
                .test_id(FORM_ID)
                .flex_col()
                .gap_2()
                // 金额（原始文本承载；判定错误红框）+ 币种（随账户推导锁定）。
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
                                .placeholder("金额")
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
                            text(currency_text)
                                .text_size(px(13.0))
                                .text_color(muted)
                                .test_id(CURRENCY_ID),
                        ),
                )
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
                .child(
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
                        )),
                )
                // 编辑态商户锚点（回填断言面：创建态不渲染）。
                .child(
                    when(
                        move || merchant_label().is_some(),
                        move || {
                            text(move || {
                                format!("原商户：{}", merchant_label().unwrap_or_default())
                            })
                            .text_size(px(11.0))
                            .text_color(muted)
                            .test_id(MERCHANT_LABEL_ID)
                        },
                    )
                    .otherwise(view),
                )
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
                        )),
                )
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
                // 动作行：取消（显式关闭不写）+ 提交（红态禁用）。
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
                                .disabled(amount_error)
                                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                                .child(
                                    text(move || {
                                        submit_label(
                                            state.kind.get(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(k: ledger_transaction::TransactionKind) -> ledger_transaction::TransactionKind {
        k
    }

    #[test]
    fn merchant_ref_maps_selector_state_to_semantics() {
        use ledger_transaction::TransactionKind as K;
        let _ = kind_of(K::Expense);
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
    fn submit_labels_and_titles_follow_old_shell_vocabulary() {
        use ledger_transaction::TransactionKind as K;
        assert_eq!(submit_label(K::Expense, false), "记支出");
        assert_eq!(submit_label(K::Income, false), "记收入");
        assert_eq!(submit_label(K::Expense, true), "保存修改");
        assert_eq!(modal_title(K::Expense, false), "记一笔 · 支出");
        assert_eq!(modal_title(K::Income, false), "记一笔 · 收入");
        assert_eq!(modal_title(K::Income, true), "编辑交易");
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
