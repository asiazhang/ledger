//! 步骤输入工厂（L1，issue #760 / ADR-0086 决策 1、2）：BDD 步骤层构造领域输入
//! 结构体的**纯函数**——热点字段作位置参数（金额/账户/日期），其余取合法且语义
//! 中性的默认值，冷字段经结构体更新语法覆盖（`TransactionInput { note: Some(..),
//! ..expense_input(..) }`）。与物品域构造助手、域内测试工厂两先例同形，不引入
//! builder。
//!
//! 分工边界（ADR-0086 决策 1）：
//! - 本模块（L1）：无 world 依赖、id 进结构体出，可独立构造与单测；
//! - [`super::step_verbs`]（L2）：名称注册表解析 → L1 构造 → 经域层/行为层公开
//!   函数写入 → 结果注册回 world。
//!
//! per-kind 合法字段集差异在此显式化（决策 2）：转账要两端账户、退款要原交易、
//! buy/sell 金额置零（行金额由后端按数量 × 单价重算，基金则金额权威）——单工厂
//! 会把矩阵知识摊回调用点。dividend/split 未实现，不设构造函数。
//!
//! 默认值合法性依据：币种默认 CNY（全局默认币种，折算 1:1，与既有步骤字面量同
//! 款）；备注/分类/商户/保单/幂等键为 None（后端全部接受）；buy/sell 的
//! `currency_code` 后端以账户币种为准（investment 域读账户），提交值不落行。
//!
//! 与 TransactionInput 装配器（前端产品装配接缝，核心交易域词汇表）互不替代、
//! 互不调用。单测暂缺：e2e 目标 `harness = false`（Cargo.toml），进程内 `#[test]`
//! 不会执行；正确性由迁移票调用点落地后的 BDD 全量背书（#761 交易域、#762 计划
//! 域、#763 其余域已全部背书）。

// #762 起全部工厂均已被迁移票消费（交易域 #761 + 计划域 #762），移除 dead_code 豁免。

use tauri_app_lib::scheduled_transactions::{CreateScheduledInput, RecurrenceType, ScheduledKind};
use tauri_app_lib::transaction::Transaction;
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

// ---------------------------------------------------------------------------
// 交易输入工厂：按已实现 kind 各设构造函数
// ---------------------------------------------------------------------------

/// 交易底座（income/expense/transfer/refund/buy/sell 共用）：合法且语义中性
/// 的默认值 + 热点字段占位。私有实现细节，per-kind 构造函数在此上做差异覆盖。
fn txn_base(
    kind: TransactionKind,
    amount_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

/// 支出输入：金额、账户、日期为热点；分类/商户/备注等冷字段经结构体更新覆盖。
pub fn expense_input(amount_cents: i64, account_id: &str, date: &str) -> TransactionInput {
    txn_base(TransactionKind::Expense, amount_cents, account_id, date)
}

/// 收入输入：同 [`expense_input`] 形态。
pub fn income_input(amount_cents: i64, account_id: &str, date: &str) -> TransactionInput {
    txn_base(TransactionKind::Income, amount_cents, account_id, date)
}

/// 转账输入：两端账户都是热点（per-kind 矩阵：转账必须指定目标账户，writer 守卫
/// `transfer.to-account-required`）；金额从 `from` 账户流出。
pub fn transfer_input(
    amount_cents: i64,
    from_account_id: &str,
    to_account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        to_account_id: Some(to_account_id.into()),
        ..txn_base(
            TransactionKind::Transfer,
            amount_cents,
            from_account_id,
            date,
        )
    }
}

/// 退款输入：原支出交易 id 为热点（per-kind 矩阵：退款必须关联未删除的原支出，
/// writer 校验 `refund.source-required`）；账户/币种/分类/商户由后端**继承原支出**，
/// `account_id` 形参仅为满足结构体完整性（写路径以原支出为准），取原支出账户即
/// 与继承结果一致。
pub fn refund_input(
    amount_cents: i64,
    account_id: &str,
    refund_of_transaction_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        refund_of_transaction_id: Some(refund_of_transaction_id.into()),
        ..txn_base(TransactionKind::Refund, amount_cents, account_id, date)
    }
}

/// 买入输入（非基金路径为默认）：标的、数量、单价为热点；**金额置零**（per-kind
/// 矩阵：非基金标的走单价权威，行金额 = 数量 × 单价 + 手续费由后端重算，提交值
/// 不被采信）。场外基金走金额权威（ADR-0038）：经结构体更新覆盖
/// `TransactionInput { price_cents: None, amount_cents: 确认金额, ..buy_input(..) }`。
/// 币种以后端读到的账户币种为准，提交值不落行。
pub fn buy_input(
    instrument_id: &str,
    quantity: f64,
    price_cents: Option<i64>,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents,
        ..txn_base(TransactionKind::Buy, 0, account_id, date)
    }
}

/// 卖出输入：同 [`buy_input`] 形态（金额置零、单价权威为默认；基金经结构体更新
/// 改走金额权威）。手续费为冷字段（`fee_cents: Some(..)`），sell 不得超过卖出收入。
pub fn sell_input(
    instrument_id: &str,
    quantity: f64,
    price_cents: Option<i64>,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents,
        ..txn_base(TransactionKind::Sell, 0, account_id, date)
    }
}

/// 按已解析 kind 分派买卖工厂（买卖铺垫/直提标的步骤共用，#761 收编 write/edit
/// 两文件同形 match）：buy/sell 之外不是合法买卖铺垫，场景文本错误即 panic。
pub fn trade_input(
    kind: TransactionKind,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    match kind {
        TransactionKind::Buy => {
            buy_input(instrument_id, quantity, Some(price_cents), account_id, date)
        }
        TransactionKind::Sell => {
            sell_input(instrument_id, quantity, Some(price_cents), account_id, date)
        }
        other => panic!("买卖铺垫仅支持 buy/sell，收到: {other}"),
    }
}

/// 步骤文本 kind 解析（#761 收编）：未知值 panic（测试步骤即败）——原先散布在
/// 各动态 kind 步骤函数内的 `TransactionKind::parse + panic` 同款形态。归本模块
/// 是因 kind 是下方工厂的热点入参：解析与构造同一处，步骤函数只留文本解析调用。
pub fn parse_kind(kind: &str) -> TransactionKind {
    TransactionKind::parse(kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）"))
}

/// 通用「创建/尝试创建交易 类型 …」步骤短语的工厂形态（#761）：kind 显式给出、
/// 其余取中性默认（即 [`txn_base`]）。三用途：
/// - expense/income：与 per-kind 工厂同底座等价，通用步骤短语无 per-kind 热点
///   字段可给，直接落底座；
/// - transfer 缺转入账户、refund 缺原支出：「不合法形态被后端守卫拒绝」场景的
///   被测前提（`transfer.to-account-required` / `refund.source-required`），非法性
///   由后端背书、测试层不预判（CONTEXT-testing「步骤动词」：写入失败进入错误
///   断言路径）；
/// - dividend/split 未实现：场景断言「暂不支持」显式拒绝，本就无合法形态可设
///   per-kind 工厂。
///
/// 与 ADR-0086 否决的「单一 transaction_input(kind) 工厂」不同界：那把 per-kind
/// 矩阵知识摊到每个调用点；本函数只服务无 per-kind 热点字段的通用步骤短语与
/// 「不合法形态被拒」场景，专用步骤（转账/退款/买卖）仍走 per-kind 工厂。
pub fn plain_input(
    kind: TransactionKind,
    amount_cents: i64,
    account_id: &str,
    date: &str,
) -> TransactionInput {
    txn_base(kind, amount_cents, account_id, date)
}

/// 既有交易行 → 全量替换入参（#761 收编，原 transactions_policy_steps 的
/// existing_to_input）：修改是全字段替换，未提及字段原样保留——edit/policy 修改
/// 步骤共用的 L1 底座。买卖扩展字段不在快照行上，修改买卖输入须在本底座上显式
/// 覆盖 instrument/quantity/price/fee（见 transactions_edit_steps::trade_edit_input）。
pub fn existing_input(existing: &Transaction) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: existing.kind,
        amount_cents: existing.amount_cents,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
        to_account_id: existing.to_account_id.clone(),
        category_id: existing.category_id.clone(),
        merchant_id: existing.merchant_id.clone(),
        policy_id: existing.policy_id.clone(),
        refund_of_transaction_id: existing.refund_of_transaction_id.clone(),
        funding_account_id: None,
        note: existing.note.clone(),
        date: existing.date.clone(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

// ---------------------------------------------------------------------------
// 计划输入工厂：按三形态各设构造函数（CreateScheduledInput 全字段默认值）
// ---------------------------------------------------------------------------

/// 三形态共用的中性底座：月度、间隔 1、无指定日（与既有计划步骤字面量同款）。
fn plan_base(
    kind: ScheduledKind,
    amount_cents: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        kind,
        account_id: account_id.into(),
        category_id: None,
        amount_cents,
        currency_code: currency_code.into(),
        recurrence_type: RecurrenceType::Monthly,
        recurrence_interval: 1,
        recurrence_day: None,
        start_date: start_date.into(),
        note: None,
        merchant_id: None,
        policy_id: None,
        total_amount_cents: None,
        total_occurrences: None,
        to_account_id: None,
    }
}

/// 订阅计划输入：金额、账户、币种、起始日期为热点（币种被计划行原样存储，场景
/// 关心时必须显式给出）；商户/保单（保费协议）等冷字段经结构体更新覆盖。
pub fn subscription_plan_input(
    amount_cents: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    plan_base(
        ScheduledKind::Subscription,
        amount_cents,
        account_id,
        currency_code,
        start_date,
    )
}

/// 分期计划输入：总额 + 期数为热点，每期金额 = 总额 ÷ 期数（与既有分期步骤同
/// 口径）；`total_amount_cents` / `total_occurrences` 是分期形态的必备字段（engine
/// 守卫 `scheduled-plan.installment-total-required`），由本构造函数按热点推导。
///
/// 尖角：`total_occurrences <= 0` 时除法在工厂内先行 panic（结构体更新救不回，
/// 除法发生在构造函数内部）——非法期数场景不可经本工厂构造，直接字面量构造
/// `CreateScheduledInput` 后走 `try_create_plan_verb`。
pub fn installment_plan_input(
    total_amount_cents: i64,
    total_occurrences: i64,
    account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        amount_cents: total_amount_cents / total_occurrences,
        total_amount_cents: Some(total_amount_cents),
        total_occurrences: Some(total_occurrences),
        ..plan_base(
            ScheduledKind::Installment,
            0,
            account_id,
            currency_code,
            start_date,
        )
    }
}

/// 定时转账计划输入：金额、两端账户为热点（per-kind 矩阵：转账计划要两端账户，
/// 且转出 ≠ 转入、两账户同币种由 engine 守卫）；不得携带商户/保单（engine 显式
/// 拒绝，字段保持默认 None）。币种取转出账户实际币种（与既有步骤「计划币种取转
/// 出账户」同款，同币种校验比的是两账户，不比提交值）；`total_occurrences` 默认
/// None 即无限循环，有期数场景经结构体更新覆盖
/// `CreateScheduledInput { total_occurrences: Some(n), ..scheduled_transfer_plan_input(..) }`。
pub fn scheduled_transfer_plan_input(
    amount_cents: i64,
    from_account_id: &str,
    to_account_id: &str,
    currency_code: &str,
    start_date: &str,
) -> CreateScheduledInput {
    CreateScheduledInput {
        to_account_id: Some(to_account_id.into()),
        ..plan_base(
            ScheduledKind::ScheduledTransfer,
            amount_cents,
            from_account_id,
            currency_code,
            start_date,
        )
    }
}
