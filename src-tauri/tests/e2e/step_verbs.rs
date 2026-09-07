//! 步骤动词（L2，issue #760 / ADR-0086 决策 1、3）：BDD 共享层中代表一个场景
//! 前置（Given）或动作（When）的命名函数。完整管线：**名称注册表解析 → L1 输入
//! 工厂构造 → 经域层/行为层公开函数写入 → 结果注册回 world**；写入一律经公开写
//! 入口（域层/行为层函数，测试无应用运行时、不经壳层、不裸 SQL——见
//! CONTEXT-testing「公开写入口（测试侧）」），业务不变量（余额缓存行等派生
//! 数据维护）由产品代码保证。置脏语义（`db.write` 写仪式）按步骤语义取舍：
//! 被测写路径动词与 IPC 命令体同款走 `db.write`；账户域创建动词服务「存在账户」
//! 类前置夹具，不经 `db.write`（夹具自身不触发自动备份脏标记，见该动词文档）。
//!
//! 两类形态：
//! - **成功形态**：写入失败即 panic（场景失败），返回生成 id；
//! - **try 形态**（`try_*`）：不 panic、返回 `Result`，供「应返回错误」断言场景
//!   复用；非法形态输入（如缺转入账户的转账）由调用方经 L1 工厂 + 结构体更新
//!   构造后走通用 try 入口。
//!
//! 本模块自 #761 起由交易域步骤消费（创建/修改/删除动词接线），自 #762 起由
//! 定时计划域步骤消费（三形态创建 + 生命周期动词接线），自 #763 起由账户域
//! 步骤消费（「存在账户」前置旁路归零接线）。步骤动词是测试层唯一允许触发
//! 写入的形态；写入失败被静默吞掉属违规（CONTEXT-testing「步骤动词」）。

use tauri_app_lib::accounts::{AccountInput, AccountType, create_account};
use tauri_app_lib::currencies::ExchangeRateInput;
use tauri_app_lib::error::AppError;
use tauri_app_lib::investment::create_exchange_rate;
use tauri_app_lib::scheduled_transactions::{
    CreateScheduledInput, ScheduledStatus, create_plan, update_plan_status,
};
use tauri_app_lib::transaction::{
    TransactionInput, TransactionWrite, create_transaction, delete_transaction, update_transaction,
};

use crate::step_inputs::{
    installment_plan_input, refund_input, scheduled_transfer_plan_input, subscription_plan_input,
    transfer_input,
};
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 账户动词：经 accounts 域公开创建入口（非 IPC 命令、非裸 SQL），注册名称→id
// ---------------------------------------------------------------------------

/// 创建账户并注册名称→id：类型为账户类型字符串（"cash"/"bank"/…，解析失败即场
/// 景文本错误）；`initial_balance_cents` 经 [`AccountInput`] 公开入参传入（None =
/// 零初始余额）。返回生成 id。
/// 经域公开创建入口（非 IPC 命令、非裸 SQL）；**不经 `db.write` 置脏包装**——
/// 本动词服务「存在账户」类前置夹具，夹具自身不得触发自动备份脏标记
/// （backup.feature 置脏语义场景以「存在账户」为未写基线，issue #243 行为保持）；
/// 需要置脏的账户写路径场景由 backup_steps 的 When 创建账户（db.write + 同一
/// 域入口）表达。
pub fn create_account_verb(
    world: &mut LedgerWorld,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: Option<i64>,
) -> String {
    try_create_account_verb(world, name, kind, currency, initial_balance_cents)
        .expect("创建账户失败")
}

/// [`create_account_verb`] 的 try 形态：不 panic，创建失败原样返回错误。
pub fn try_create_account_verb(
    world: &mut LedgerWorld,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: Option<i64>,
) -> Result<String, AppError> {
    let input = AccountInput {
        name: name.into(),
        kind: kind
            .parse::<AccountType>()
            .map_err(|e| AppError::Invalid(format!("非法账户类型 {kind}: {e}")))?,
        currency_code: currency.into(),
        initial_balance_cents,
    };
    let id = create_account(&world_conn!(world), input)?;
    world.account_name_to_id.insert(name.into(), id.clone());
    Ok(id)
}

// ---------------------------------------------------------------------------
// 交易动词：名称解析 → L1 工厂 → 行为层 create 编排入口 → 注册回 world
// ---------------------------------------------------------------------------

/// 通用成功动词：接受任意已构造输入（L1 工厂产物 + 冷字段覆盖），经行为层
/// create 编排入口（连接 + 输入进，ADR-0033）写入并注册
/// `world.txn.last_transaction_id`；失败即 panic。返回新交易 id。
pub fn create_transaction_verb(world: &mut LedgerWorld, input: TransactionInput) -> String {
    try_create_transaction_verb(world, input).expect("创建交易失败")
}

/// [`create_transaction_verb`] 的 try 形态：不 panic，写入失败原样返回行为层
/// 错误（码化错误信息可供「应返回错误」断言）。
pub fn try_create_transaction_verb(
    world: &mut LedgerWorld,
    input: TransactionInput,
) -> Result<String, AppError> {
    let write: TransactionWrite = world.db.write(|conn| create_transaction(conn, input))?;
    world.txn.last_transaction_id = Some(write.id.clone());
    Ok(write.id)
}

/// 转账动词：两端账户按名称解析，输入经 [`transfer_input`] 构造。
pub fn create_transfer(
    world: &mut LedgerWorld,
    amount_cents: i64,
    from: &str,
    to: &str,
    date: &str,
) -> String {
    let from_id = world.account_id(from);
    let to_id = world.account_id(to);
    create_transaction_verb(world, transfer_input(amount_cents, &from_id, &to_id, date))
}

/// 退款动词：按原交易 id 关联退款；原支出账户直接读库取（账户/币种后端继承原
/// 支出，此处取值只为输入完整、语义一致）。
pub fn create_refund(
    world: &mut LedgerWorld,
    amount_cents: i64,
    original_id: &str,
    date: &str,
) -> String {
    let account_id = transaction_account_id(world, original_id);
    create_transaction_verb(
        world,
        refund_input(amount_cents, &account_id, original_id, date),
    )
}

/// 退款动词（关联最近一笔交易）：「关联上一笔交易创建退款」场景形态，原交易取
/// `world.txn.last_transaction_id`。
pub fn refund_last_transaction(world: &mut LedgerWorld, amount_cents: i64, date: &str) -> String {
    let original_id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有上一笔交易可关联退款");
    create_refund(world, amount_cents, &original_id, date)
}

/// 交易行的账户 id（退款动词派生账户用）。
fn transaction_account_id(world: &LedgerWorld, transaction_id: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT account_id FROM transactions WHERE id=?1",
        [transaction_id],
        |r| r.get(0),
    )
    .expect("查询原交易账户失败")
}

// ---------------------------------------------------------------------------
// 修改/删除动词：update / delete 编排入口（同 create 形态，#761 接线）
// ---------------------------------------------------------------------------

/// 修改动词（全字段替换）：经 update 编排入口写入；失败即 panic。写证据
/// （WriteEvidence，ADR-0044）属壳层信号语义，BDD 步骤不消费。
pub fn update_transaction_verb(world: &mut LedgerWorld, id: &str, input: TransactionInput) {
    try_update_transaction_verb(world, id, input).expect("修改交易失败");
}

/// [`update_transaction_verb`] 的 try 形态：不 panic，修改失败原样返回行为层错误。
pub fn try_update_transaction_verb(
    world: &mut LedgerWorld,
    id: &str,
    input: TransactionInput,
) -> Result<(), AppError> {
    world
        .db
        .write(|conn| update_transaction(conn, id, input))
        .map(|_| ())
}

/// 删除动词（软删）：经 delete 编排入口写入；失败即 panic。
pub fn delete_transaction_verb(world: &mut LedgerWorld, id: &str) {
    try_delete_transaction_verb(world, id).expect("删除交易失败");
}

/// [`delete_transaction_verb`] 的 try 形态：不 panic，删除失败原样返回行为层错误。
pub fn try_delete_transaction_verb(world: &mut LedgerWorld, id: &str) -> Result<(), AppError> {
    world.db.write(|conn| delete_transaction(conn, id))
}

// ---------------------------------------------------------------------------
// 计划动词：同形，走计划域公开创建入口（scheduled_transactions::create_plan）
// ---------------------------------------------------------------------------

/// 通用成功动词：接受任意已构造计划输入（L1 工厂产物 + 冷字段覆盖），经计划域
/// 公开创建入口写入并注册 `world.plan.last_plan_id`；失败即 panic。返回计划 id。
pub fn create_plan_verb(world: &mut LedgerWorld, input: CreateScheduledInput) -> String {
    try_create_plan_verb(world, input).expect("创建计划失败")
}

/// [`create_plan_verb`] 的 try 形态：不 panic，创建失败原样返回领域错误。
pub fn try_create_plan_verb(
    world: &mut LedgerWorld,
    input: CreateScheduledInput,
) -> Result<String, AppError> {
    let id = world.db.write(|conn| create_plan(conn, input))?;
    world.plan.last_plan_id = Some(id.clone());
    Ok(id)
}

/// 订阅计划动词：币种随步骤文本显式给出（计划行原样存储），备注为冷字段
/// （`CreateScheduledInput { note: Some(..), ..subscription_plan_input(..) }` +
/// [`create_plan_verb`]）。
pub fn create_subscription_plan(
    world: &mut LedgerWorld,
    amount_cents: i64,
    currency: &str,
    account: &str,
    start: &str,
) -> String {
    let account_id = world.account_id(account);
    create_plan_verb(
        world,
        subscription_plan_input(amount_cents, &account_id, currency, start),
    )
}

/// 分期计划动词：总额 + 期数为热点；币种取账户实际币种（与既有分期步骤的 CNY
/// 硬编码在 CNY 账户场景等价，且不引入「币种与账户不符」的隐蔽数据）。
pub fn create_installment_plan(
    world: &mut LedgerWorld,
    total_amount_cents: i64,
    total_occurrences: i64,
    account: &str,
    start: &str,
) -> String {
    let account_id = world.account_id(account);
    let currency = account_currency_code(world, &account_id);
    create_plan_verb(
        world,
        installment_plan_input(
            total_amount_cents,
            total_occurrences,
            &account_id,
            &currency,
            start,
        ),
    )
}

/// 定时转账计划动词：两端账户按名称解析，币种取转出账户实际币种；`total_occurrences`
/// 为 None 即无限循环（与既有带期数/无限循环两步骤变体同形）。
pub fn create_scheduled_transfer_plan(
    world: &mut LedgerWorld,
    amount_cents: i64,
    from: &str,
    to: &str,
    total_occurrences: Option<i64>,
    start: &str,
) -> String {
    let from_id = world.account_id(from);
    let to_id = world.account_id(to);
    let currency = account_currency_code(world, &from_id);
    create_plan_verb(
        world,
        CreateScheduledInput {
            total_occurrences,
            ..scheduled_transfer_plan_input(amount_cents, &from_id, &to_id, &currency, start)
        },
    )
}

/// 账户的币种代码（计划动词派生币种用；#762 起步骤侧冷字段覆盖路径亦消费——
/// 分期/转账计划的商户/备注变体经 L1 工厂构造，币种取账户实际币种同一口径）。
pub(crate) fn account_currency_code(world: &LedgerWorld, account_id: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        [account_id],
        |r| r.get(0),
    )
    .expect("查询账户币种失败")
}

/// 计划生命周期动词（#762 接线）：经既有生命周期命令形态的域函数
/// `update_plan_status`（暂停/恢复/取消命令体）变更状态；失败即 panic。
/// 不为测试开旁路——期次状态回写等无公开入口的直置不在此列（归 #764 例外裁决）。
pub fn update_plan_status_verb(world: &mut LedgerWorld, id: &str, status: ScheduledStatus) {
    world
        .db
        .write(|conn| update_plan_status(conn, id, status))
        .expect("计划状态变更失败");
}

// ---------------------------------------------------------------------------
// 投资域动词：汇率夹具经 investment::create_exchange_rate（upsert 单点）
// ---------------------------------------------------------------------------

/// 汇率夹具动词（#764 接线，替代「存在汇率」类步骤的内联直写）：经投资域公开
/// 创建入口写入一条汇率（base → quote，upsert 单点）；失败即 panic。**不经
/// `db.write` 置脏包装**——与 [`create_account_verb`] 同款取舍：本动词服务
/// 「存在汇率」类前置夹具，夹具自身不触发自动备份脏标记；需要置脏的汇率
/// 写路径场景由 backup_steps 的「写入汇率」步骤（db.write + 同一域入口）表达。
/// source 固定 'manual'（手动汇率语义，与既有直置夹具同值）；`priced_at` 为
/// 报价时刻，折算查询不消费，值无断言语义（取非 FIXED_NOW 值避免守门规则 3）。
pub fn create_exchange_rate_verb(
    world: &mut LedgerWorld,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
) {
    let input = ExchangeRateInput {
        base_code: base.into(),
        quote_code: quote.into(),
        rate,
        priced_at: priced_at.into(),
        source: Some("manual".into()),
    };
    create_exchange_rate(&world_conn!(world), input).expect("存在汇率夹具：写入失败");
}
