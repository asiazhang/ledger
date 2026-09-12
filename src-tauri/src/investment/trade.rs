use rusqlite::{Connection, OptionalExtension};

use super::lots::{self, Consumption};
use super::model::{TransactionConvert, TransactionSplit, TransactionTrade};
use super::prices::PRICE_UNITS_PER_FEN;
use super::split::{self, LotRestatement};
use super::unwind;
use crate::accounts::AccountType;
use crate::db::query::query_one;
use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::transaction::amount;
use crate::transaction::amount::TransactionKind;
use crate::transaction::command::{
    ConvertCommandFields, InvestmentCommandFields, SplitCommandFields,
};
use crate::transaction::{ConvertFields, NormalizedTransaction, TransactionInput};
use ledger_sync_protocol::device::device_id;

/// 查询账户本位币代码（原 `commands::fx::account_currency_code`，随投资域归位
/// 迁入唯一消费方；交易行折算语义归核心交易域 `transaction::amount` 接缝）。
fn account_currency_code(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// 标的存在性校验 + 类型读取（issue #295 / #302）：prepare 阶段拦截引用不存在标的的
/// buy/sell，返回可读回自纠的码化 [`AppError::Coded`] 中文错误（HTTP 侧 400）——否则
/// prepare 通过、apply 落 `security_transactions` 时才触发 `instrument_id` 外键违规的
/// 「数据库错误」（HTTP 侧 500，批量导入路径还会整批回滚），AI 无法据此纠错。
/// 创建与修改（全字段替换）共用 prepare，自然同时生效；`action` 为「买入/卖出」
/// 措辞前缀，与既有「必须指定标的」等错误同风格，消息携带标的 id 供回自纠；
/// `code` 由调用方按入口传入（`trade.buy-instrument-not-found` / `trade.sell-instrument-not-found`）。
/// 返回标的类型闭集字面量（`fund` 等，ADR-0038）——场外基金申赎据此切换金额权威语义。
fn fetch_instrument_type(
    conn: &Connection,
    instrument_id: &str,
    action: &str,
    code: &str,
) -> Result<String> {
    let instrument_type: Option<String> = conn
        .query_row(
            "SELECT instrument_type FROM instruments WHERE id=?1",
            rusqlite::params![instrument_id],
            |r| r.get(0),
        )
        .optional()?;
    instrument_type.ok_or_else(|| {
        AppError::codedp(
            code,
            format!("{action}标的不存在: {instrument_id}"),
            &[instrument_id],
        )
    })
}

/// 投资交易对外出口（issue #72 / spec #69）：`prepare / apply / revert` 三件套 +
/// 删除路径专用的 [`release_for_delete`]（issue #940 / ADR-0097），承载投资 kind：
/// buy（建仓）/ sell（FIFO 卖出匹配与已实现盈亏）/ convert（基金转换，ADR-0099：
/// 转出腿 FIFO 消耗与结转成本、转入批次以结转成本建仓、零已实现盈亏）/
/// split（份额调整，ADR-0106：按比例重述在用批次成本、零已实现盈亏，#1049）。
///
/// - [`prepare`]：校验并归一化一笔 buy/sell/convert 输入（不落库、不产生副作用），产出 [`Plan`]；
/// - [`apply`]：应用计划的副作用（buy 建仓 / sell 卖出匹配 / convert 两腿结转），由编排层在行落库后调用；
/// - [`revert`]：回退一笔已存在 buy/sell/convert 的副作用（buy/convert 在用占用守卫+清理 /
///   sell 回补 / convert 另回补转出腿消耗），供修改前清理；
/// - [`release_for_delete`]：删除路径的持仓副作用回退（sell 回补 / buy 与 convert 级联+清理），
///   供行为层删除编排入口调用。
///
/// 交易行字段的 INSERT/UPDATE 一律经 `transaction::writer` 接缝（issue #70），
/// 本模块不再反向依赖 transactions 的行更新函数；行写入由编排层（行为层）持有，
/// 与 lot/匹配副作用同处一个事务。FIFO 取批次/分摊/回补知识归 [`lots`]（#1018），
/// 修改/删除路径的守卫与清理模板归 [`unwind`]（#1020）——本模块的 [`revert`] /
/// [`release_for_delete`] 只是薄委托。
/// 读取一笔 buy/sell 交易的买卖明细（issue #180）：从 `security_transactions`
/// 扩展表按交易 id 取标的/数量/价格/费用，JOIN `instruments` 带出展示字段。
/// 供投资表单编辑模式回填；无明细（交易不存在/非 buy/sell）返回 `NotFound`。
pub fn get_transaction_trade(conn: &Connection, transaction_id: &str) -> Result<TransactionTrade> {
    query_one::<TransactionTrade, _>(
        conn,
        "SELECT st.instrument_id, i.symbol, i.name, i.instrument_type, st.quantity, st.price_cents, st.fee_cents \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.instrument_id \
         WHERE st.transaction_id = ?1",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.detail-not-found",
            format!("交易不存在或无买卖明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

/// 读取一笔 convert 交易的两腿明细（ADR-0099 / issue #979）：从 `security_transactions`
/// 的 convert 行取两侧标的/份额/金额/手续费，JOIN `instruments` 两次带出两侧展示字段、
/// JOIN `transactions` 带出结转成本锚点与币种。供转换表单编辑模式回填「A → B」全量信息；
/// 无明细（交易不存在/非 convert）返回 `NotFound`。两侧展示单价不随投影携带：前端按
/// 确认单金额 ÷ 份额与 `prepare_convert` 同一公式反算（余额权威、单价反算）。
pub fn get_transaction_convert(
    conn: &Connection,
    transaction_id: &str,
) -> Result<TransactionConvert> {
    query_one::<TransactionConvert, _>(
        conn,
        "SELECT st.instrument_id, oi.symbol, oi.name, st.quantity, st.out_amount_cents, \
         st.to_instrument_id, ii.symbol, ii.name, st.to_quantity, st.in_amount_cents, \
         st.fee_cents, t.amount_cents, t.currency_code \
         FROM security_transactions st \
         JOIN instruments oi ON oi.id = st.instrument_id \
         JOIN instruments ii ON ii.id = st.to_instrument_id \
         JOIN transactions t ON t.id = st.transaction_id \
         WHERE st.transaction_id = ?1 AND st.action='convert'",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.convert-detail-not-found",
            format!("交易不存在或无转换明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

/// 读取一笔 split 交易的份额调整明细（ADR-0106 / issue #1052）：从
/// `security_transactions` 的 split 行取**带符号**份额增量 Δ（`quantity`，
/// `price_cents` 留 NULL），JOIN `instruments` 带出标的展示字段。供交易列表
/// 「只读详情」呈现标的与份额变动；无明细（交易不存在 / 非 split）返回 `NotFound`。
pub fn get_transaction_split(conn: &Connection, transaction_id: &str) -> Result<TransactionSplit> {
    query_one::<TransactionSplit, _>(
        conn,
        "SELECT st.instrument_id, i.symbol, i.name, st.quantity \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.instrument_id \
         WHERE st.transaction_id = ?1 AND st.action='split'",
        rusqlite::params![transaction_id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.split-detail-not-found",
            format!("交易不存在或无份额调整明细: {transaction_id}"),
            &[transaction_id],
        )
    })
}

/// 按交易 id 批量反查基金转换两腿扩展（一次 IN 查询，调用方按 id 归位到交易行）。
///
/// `security_transactions` 的 convert 行以 `transaction_id` 为主键——一笔转换至多
/// 一行；行金额锚点是结转成本而不是确认单金额，列表/详情展示须读本扩展。
/// 非转换 id 自然不命中（调用方传的是已按 kind 过滤的 id 集）。
pub fn convert_fields_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<(String, ConvertFields)>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT st.transaction_id,st.to_instrument_id,i.symbol,st.to_quantity,st.out_amount_cents,st.in_amount_cents \
         FROM security_transactions st \
         JOIN instruments i ON i.id = st.to_instrument_id \
         WHERE st.action='convert' AND st.transaction_id IN ({placeholders})"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(transaction_ids.iter()), |r| {
        Ok((
            r.get::<_, String>(0)?,
            ConvertFields {
                to_instrument_id: r.get(1)?,
                to_symbol: r.get(2)?,
                to_quantity: r.get(3)?,
                out_amount_cents: r.get(4)?,
                in_amount_cents: r.get(5)?,
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub struct BuyPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    /// 每份成本（万分之一元，含费用摊薄）：prepare 按标的类型单次舍入算定，
    /// apply 原样落批次——基金锚定权威金额、其余锚定成交单价（见 [`prepare_buy`]）。
    pub(crate) cost_per_unit_cents: i64,
}

/// 校验并归一化一笔买入交易（不落库）。创建与修改共用；
/// 只做校验与字段解析，持仓建仓等副作用由 [`apply`] 在落库时按其身份（新增或替换）执行。
fn prepare_buy(conn: &Connection, input: &TransactionInput) -> Result<BuyPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.buy-instrument-required", "买入必须指定标的"))?
        .clone();
    // 标的存在性（兼类型读取）先于数量/单价校验：身份错了，数值对错无从谈起（issue #295）。
    let instrument_type = fetch_instrument_type(
        conn,
        &instrument_id,
        "买入",
        "trade.buy-instrument-not-found",
    )?;
    // 转入账户拒绝（issue #1187）：buy 的现金腿归结算账户（出资账户 ?? 投资账户，
    // ADR-0096），不存在转入侧——携带 `to_account_id` 会被余额写路径误用于覆盖
    // 转入侧余额（凭空污染第三个账户）。与 convert / split / dividend 的同款
    // forbidden 守卫同一纪律：不接受的字段一律拒绝，不静默吞掉。
    if input.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.buy-to-account-forbidden",
            "买入不能携带转入账户：资金流出归结算账户（出资账户或投资账户），账户间划转请使用转账",
        ));
    }
    let quantity = input.quantity.unwrap_or(0.0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.buy-quantity-positive",
            "买入数量必须大于 0",
        ));
    }
    // 录入权威按标的类型分流（issue #302 / ADR-0038 决策 2）：场外基金以确认单为权威——
    // 整分金额 + 确认份额必填、成交单价由两者反算到万分之一元（确认单抄写即记账，
    // 行金额不被单价舍入污染）；其余类型维持单价权威，行金额由数量 × 单价重算。
    let is_fund = instrument_type == "fund";
    let price_cents;
    let amount_cents;
    let cost_per_unit_cents;
    if is_fund {
        amount_cents = input.amount_cents;
        if amount_cents <= 0 {
            return Err(AppError::coded(
                "trade.buy-amount-positive",
                "买入金额必须大于 0（基金申赎以确认单金额为权威）",
            ));
        }
        // 金额权威与单价权威互斥：wire 上误传单价显式拒绝（与前端装配器同源），
        // 不静默吞掉（非法输入 fail fast，与 split 的单价拒绝同一原则）。
        if input.price_cents.is_some() {
            return Err(AppError::coded(
                "trade.fund-price-forbidden",
                "基金申赎以确认单金额为权威，不可提供单价（由金额与份额反算）",
            ));
        }
        if fee_cents >= amount_cents {
            return Err(AppError::coded(
                "trade.buy-fee-exceeds-amount",
                "买入手续费不能超过买入金额",
            ));
        }
        price_cents =
            ((amount_cents - fee_cents) as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.derived-price-positive",
                "反算单价必须大于 0（确认金额过小或份额过大）",
            ));
        }
        // 每份成本锚定权威金额单次舍入（含手续费摊薄），全平仓时盈亏按权威金额闭合。
        cost_per_unit_cents = (amount_cents as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
    } else {
        price_cents = input.price_cents.unwrap_or(0);
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.buy-price-positive",
                "买入单价必须大于 0",
            ));
        }
        // 金额分 = 数量 × 单价（万分之一元）÷ 换算因子 + 手续费（分）；价格刻度见 ADR-0038。
        amount_cents =
            (quantity * price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64 + fee_cents;
        // 每份成本（万分之一元）=（数量 × 单价 + 手续费分 × 换算因子）÷ 数量，单次舍入：
        // 手续费是金额（分），先归一到万分之一元刻度再参与摊薄，与 v_holdings
        // 的 cost_basis 换算同口径（ADR-0038）。
        cost_per_unit_cents =
            ((quantity * price_cents as f64 + fee_cents as f64 * PRICE_UNITS_PER_FEN) / quantity)
                .round() as i64;
    }
    ensure_investment_account(
        conn,
        &input.account_id,
        "trade.buy-account-not-investment",
        "买入交易必须使用投资账户",
    )?;
    let account_currency = account_currency_code(conn, &input.account_id)?;
    // 出资账户准入（issue #935 / ADR-0096）：buy 的结算币种 = 投资账户币种，
    // 出资账户币种必须与其一致。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Buy,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    // 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：不再硬编码 1:1，
    // 与通用 kind / 定时引擎共用同一折算路径（convert_to_native，基准为默认币种）。
    let amount_native_cents = amount::convert_to_native(conn, amount_cents, &account_currency)?;

    Ok(BuyPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Buy,
            amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            // 现金腿归结算账户，不存在转入侧：携带已在守卫段拒绝，恒 None（issue #1187）。
            to_account_id: None,
            funding_account_id: input.funding_account_id.clone(),
            category_id: None,
            merchant_id: None,
            // 投资 kind 不涉保单（行为层准入已拒绝携带，issue #361）：恒 None。
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        cost_per_unit_cents,
    })
}

/// 校验并归一化一笔卖出交易（不落库）。创建与修改共用；
/// FIFO 消耗（含「可卖出数量不足」守卫）在本阶段算定，卖出匹配等副作用由
/// [`apply`] 按该消耗快照落盘、不再复算（与 convert 同一形态，issue #1019）。
fn prepare_sell(conn: &Connection, input: &TransactionInput) -> Result<SellPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.sell-instrument-required", "卖出必须指定标的"))?
        .clone();
    // 标的存在性（兼类型读取）先于可卖数量校验：不存在的标的不该误报「可卖出数量不足」
    // （issue #295）。
    let instrument_type = fetch_instrument_type(
        conn,
        &instrument_id,
        "卖出",
        "trade.sell-instrument-not-found",
    )?;
    // 转入账户拒绝（issue #1187）：与 prepare_buy 同款守卫——sell 的现金腿归
    // 结算账户（出资账户 ?? 投资账户，ADR-0096），不存在转入侧，携带即拒绝。
    if input.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.sell-to-account-forbidden",
            "卖出不能携带转入账户：资金流入归结算账户（出资账户或投资账户），账户间划转请使用转账",
        ));
    }
    let quantity = input.quantity.unwrap_or(0.0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.sell-quantity-positive",
            "卖出数量必须大于 0",
        ));
    }
    // 录入权威按标的类型分流（与 prepare_buy 同一口径，issue #302 / ADR-0038）：
    // 场外基金以确认单为权威——整分金额必填，毛收入 = 金额 + 手续费，单价反算。
    let is_fund = instrument_type == "fund";
    let price_cents;
    let amount_cents;
    let gross_proceeds;
    if is_fund {
        amount_cents = input.amount_cents;
        if amount_cents <= 0 {
            return Err(AppError::coded(
                "trade.sell-amount-positive",
                "卖出金额必须大于 0（基金申赎以确认单金额为权威）",
            ));
        }
        // 同买入：金额权威与单价权威互斥，wire 误传单价显式拒绝。
        if input.price_cents.is_some() {
            return Err(AppError::coded(
                "trade.fund-price-forbidden",
                "基金申赎以确认单金额为权威，不可提供单价（由金额与份额反算）",
            ));
        }
        gross_proceeds = amount_cents + fee_cents;
        price_cents = (gross_proceeds as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.derived-price-positive",
                "反算单价必须大于 0（确认金额过小或份额过大）",
            ));
        }
    } else {
        price_cents = input.price_cents.unwrap_or(0);
        if price_cents <= 0 {
            return Err(AppError::coded(
                "trade.sell-price-positive",
                "卖出单价必须大于 0",
            ));
        }
        // 金额分 = 数量 × 单价（万分之一元）÷ 换算因子；与买入同口径（ADR-0038）。
        gross_proceeds = (quantity * price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64;
        if fee_cents > gross_proceeds {
            return Err(AppError::coded(
                "trade.sell-fee-exceeds-proceeds",
                "卖出手续费不能超过卖出收入",
            ));
        }
        amount_cents = gross_proceeds - fee_cents;
    }
    ensure_investment_account(
        conn,
        &input.account_id,
        "trade.sell-account-not-investment",
        "卖出交易必须使用投资账户",
    )?;
    let account_currency = account_currency_code(conn, &input.account_id)?;
    // 出资账户准入（issue #935 / ADR-0096）：sell 的结算币种 = 投资账户币种，
    // 出资账户币种必须与其一致。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Sell,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    // 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：不再硬编码 1:1，
    // 与通用 kind / 定时引擎共用同一折算路径（convert_to_native，基准为默认币种）。
    let amount_native_cents = amount::convert_to_native(conn, amount_cents, &account_currency)?;

    // 取批次 → 分摊（含「可卖出数量不足」守卫）两步单点算定消耗规划：apply 只落盘。
    let active_lots = lots::active_lots(conn, &input.account_id, &instrument_id)?;
    let consumed = lots::plan(conn, &active_lots, quantity)?;

    Ok(SellPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Sell,
            amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            // 现金腿归结算账户，不存在转入侧：携带已在守卫段拒绝，恒 None（issue #1187）。
            to_account_id: None,
            funding_account_id: input.funding_account_id.clone(),
            category_id: None,
            merchant_id: None,
            // 投资 kind 不涉保单（行为层准入已拒绝携带，issue #361）：恒 None。
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        consumed,
        gross_proceeds_cents: gross_proceeds,
    })
}

/// 转出腿展示单价：确认单金额 ÷ 份额反算到万分之一元（金额权威、单价反算，
/// 与场外基金同款）。录入与重放共用本单点（同一公式，两端不得算出不同单价）。
fn derived_out_price_cents(out_amount_cents: i64, quantity: f64) -> Result<i64> {
    let price_cents = (out_amount_cents as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
    if price_cents <= 0 {
        return Err(AppError::coded(
            "trade.derived-price-positive",
            "反算单价必须大于 0（确认金额过小或份额过大）",
        ));
    }
    Ok(price_cents)
}

/// 校验并归一化一笔基金转换（不落库）。创建与修改共用。
///
/// 一笔转换 = 一条记录、两腿同记录、无现金腿（ADR-0099 决策 1）：
/// 转出腿按 FIFO 消耗持仓批次（与 sell 同款守卫与闭合口径），结转成本写入行金额
/// 锚点；转入腿以结转成本建立新批次（不按转入日市值重置）。转换本身不写卖出
/// 匹配、不产生已实现盈亏；六度量系数全 0，落账前后全部账户余额不变。
///
/// 守卫（全部码化中文错误，ADR-0050）：转出/转入标的存在且互不相同、账户为投资账户、
/// 不得携带转入账户（不跨账户）与出资账户（出资闭集仅 buy/sell，ADR-0096）、
/// 转出份额不得超当前持仓（与 sell 同码 `trade.insufficient-holding`）。
fn prepare_convert(conn: &Connection, input: &TransactionInput) -> Result<ConvertPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| {
            AppError::coded("trade.convert-instrument-required", "转换必须指定转出标的")
        })?
        .clone();
    let to_instrument_id = input
        .to_instrument_id
        .as_ref()
        .ok_or_else(|| {
            AppError::coded(
                "trade.convert-to-instrument-required",
                "转换必须指定转入标的",
            )
        })?
        .clone();
    if instrument_id == to_instrument_id {
        return Err(AppError::coded(
            "trade.convert-same-instrument",
            "转换的转出标的与转入标的不能相同",
        ));
    }
    // 标的存在性（兼类型读取）先于金额/份额校验：身份错了，数值对错无从谈起（卖出的同一顺序）。
    fetch_instrument_type(
        conn,
        &instrument_id,
        "转换转出",
        "trade.convert-instrument-not-found",
    )?;
    fetch_instrument_type(
        conn,
        &to_instrument_id,
        "转换转入",
        "trade.convert-to-instrument-not-found",
    )?;
    let quantity = input.quantity.unwrap_or(0.0);
    if quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.convert-quantity-positive",
            "转换转出份额必须大于 0",
        ));
    }
    let to_quantity = input.to_quantity.unwrap_or(0.0);
    if to_quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.convert-to-quantity-positive",
            "转换转入份额必须大于 0",
        ));
    }
    let fee_cents = input.fee_cents.unwrap_or(0);
    // 两侧金额是确认单权威（列表展示与多腿分摊口径的输入）；行金额锚点不读它，
    // 而是服务端按 FIFO 消耗算出的结转成本（与 buy/sell「金额占位、服务端重算」同款）。
    let out_amount_cents = input.out_amount_cents.unwrap_or(0);
    if out_amount_cents <= 0 {
        return Err(AppError::coded(
            "trade.convert-out-amount-positive",
            "转换转出金额必须大于 0",
        ));
    }
    let in_amount_cents = input.in_amount_cents.unwrap_or(0);
    if in_amount_cents <= 0 {
        return Err(AppError::coded(
            "trade.convert-in-amount-positive",
            "转换转入金额必须大于 0",
        ));
    }
    // 转出腿展示单价：确认单金额 ÷ 份额反算到万分之一元（金额权威、单价反算，
    // 与场外基金同款）；录入与重放共用同一公式（单点归属，两端不得算出不同单价）。
    let price_cents = derived_out_price_cents(out_amount_cents, quantity)?;
    ensure_investment_account(
        conn,
        &input.account_id,
        "trade.convert-account-not-investment",
        "转换交易必须使用投资账户",
    )?;
    // 不跨账户（ADR-0099 决策 1）：两腿是同一投资账户内的两个标的，不是两个账户；
    // 携带转入账户即意图跨账户，显式拒绝。
    if input.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.convert-to-account-forbidden",
            "转换不跨账户：转出与转入必须同属一个投资账户，不能携带转入账户",
        ));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    // 出资账户准入（ADR-0096）：convert 不在出资闭集内，携带即被既有「不能携带
    // 出资账户」拒绝——「无现金保障」由此天然成立，不另设第二份判定。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Convert,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    // 转出腿 FIFO 消耗与逐批次结转成本（含耗尽批次闭合）在 prepare 阶段算定：
    // 它是行金额锚点与转入批次成本的唯一依据，apply 原样落消耗记录与批次
    // （取批次 → 分摊两步，含「可卖出数量不足」守卫，issue #1019）。
    let active_lots = lots::active_lots(conn, &input.account_id, &instrument_id)?;
    let consumed = lots::plan(conn, &active_lots, quantity)?;
    let carried_cost_cents = lots::total_cost(&consumed);
    let amount_native_cents =
        amount::convert_to_native(conn, carried_cost_cents, &account_currency)?;

    Ok(ConvertPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Convert,
            // 行金额锚点 = 结转成本（六度量系数全 0，不影响任何账户余额）；
            // 转入批次以其锚定闭合（既有「耗尽批次成本闭合」机制零改动）。
            amount_cents: carried_cost_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            // 两腿是标的而非账户：转入账户已拒绝、出资账户已被准入拒绝，恒 None。
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        to_instrument_id,
        quantity,
        to_quantity,
        price_cents,
        fee_cents,
        out_amount_cents,
        in_amount_cents,
        consumed,
    })
}

/// 份额调整计划（ADR-0106）：归一化行（无现金腿，金额恒 0）、带符号份额增量 Δ、
/// 在用批次重述快照（prepare 算定、apply 原样落库），以及最终持仓与批次总成本
/// 两个跨端重放比对锚点（ADR-0106 决策 9：载荷不携逐批次重述结果、只携 Δ 与这
/// 两个总量，重放端本地重建重述后比对，不一致显式挂起）。
pub struct SplitPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    /// 带符号份额增量 Δ（绝对增量语义，不是换算比例）：`+Δ` = 折算 / 结转 /
    /// 送股，`−Δ` = 缩股（ADR-0106 决策 1）；缩股幅度取严 `|Δ| < 当前持仓`
    /// （`trade.split-shrink-not-less-than-holding`）。
    pub(crate) delta_quantity: f64,
    /// 在用批次重述快照（逐批次 before / after，落 `security_lot_adjustments` 审计）。
    pub(crate) restated: Vec<LotRestatement>,
    /// 重述后总持仓（Σ remaining_after）：同步命令携带的比对锚点之一。
    pub(crate) final_quantity: f64,
    /// 批次总成本（分，重述精确不变）：同步命令携带的比对锚点之二。
    pub(crate) total_cost_cents: i64,
}

/// 现金分红计划（issue #1078）：归一化行（现金腿记 `amount_cents`，kind→度量
/// 矩阵既有行 account_flow + / income_net +）+ 归属标的。
///
/// 分红不触碰 FIFO 批次、不写卖出匹配、不改变任何持仓数量——apply 的唯一副作用
/// 是落 `security_transactions` 的 dividend 扩展行（`quantity` / `price_cents`
/// 恒 NULL），它同时是来源列标的反查与累计收益第三腿的数据源。
pub struct DividendPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
}

/// 校验并归一化一笔份额调整（不落库）。本地创建与本地修改共用（修改路径以
/// `existing_id` 限定重述范围，ADR-0106 决策 6）；重放形态走
/// [`replay_split_plan`]（同一守卫集、从命令携带字段装配）。
///
/// 语义（ADR-0106 决策 1/3/7）：同一投资账户内、单标的的非现金份额变动——
/// 一条记录表达标的、带符号份额增量 Δ 与调整日；**无现金腿**：行金额恒 0、
/// 六度量系数全 0，落账前后全部账户余额（含黑洞）完全不变；成本口径为按比例
/// 重述在用批次（[`split::plan_restatement`]），零已实现盈亏。
///
/// 守卫（全部码化中文错误，ADR-0050）：标的存在、账户为投资账户、Δ ≠ 0、
/// `Δ > 0` 须有在用持仓（价值已含在原批次，零持仓无从重述）、`Δ < 0` 缩股幅度
/// 取严 `|Δ| < 当前持仓`、不接受手续费（非 0 拒绝）/ 单价 / 非零金额 / 转入标的 /
/// 转入账户；商户 / 分类 / 保单由行为层参考数据携带准入拒绝（split 不在任何准入
/// 集）；出资账户由出资准入闭集拒绝（仅 buy/sell 可携带，ADR-0096）。
///
/// `existing_id`：修改路径传本行 id，重述目标以本行落账序为界（ADR-0106 决策 6）——
/// 只重述它落账那一刻在场的批次，其后建立的批次（后续买入）不受历史份额调整影响；
/// 创建路径传 `None`，取当前全部在用批次。
fn prepare_split(
    conn: &Connection,
    input: &TransactionInput,
    existing_id: Option<&str>,
) -> Result<SplitPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.split-instrument-required", "份额调整必须指定标的"))?
        .clone();
    // 标的存在性（兼类型读取）先于数值校验：身份错了，数值对错无从谈起
    // （buy/sell/convert 同一顺序）。标的类型不限（基金/股票同构）。
    fetch_instrument_type(
        conn,
        &instrument_id,
        "份额调整",
        "trade.split-instrument-not-found",
    )?;
    // 单标的、不跨账户、无现金腿（ADR-0106 决策 1）：携带 convert/交易形态的
    // 专属字段即意图漂移，fail fast 显式拒绝，不静默吞掉。
    if input.to_instrument_id.is_some() {
        return Err(AppError::coded(
            "trade.split-to-instrument-forbidden",
            "份额调整是单标的份额变动，不能携带转入标的",
        ));
    }
    if input.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.split-to-account-forbidden",
            "份额调整不跨账户：只能调整同一投资账户内的标的，不能携带转入账户",
        ));
    }
    if input.price_cents.is_some() {
        return Err(AppError::coded(
            "trade.split-price-forbidden",
            "份额调整无现金腿，不可提供单价",
        ));
    }
    let fee_cents = input.fee_cents.unwrap_or(0);
    if fee_cents != 0 {
        return Err(AppError::coded(
            "trade.split-fee-forbidden",
            "份额调整无现金腿，不接受手续费",
        ));
    }
    if input.amount_cents != 0 {
        return Err(AppError::coded(
            "trade.split-amount-forbidden",
            "份额调整无现金腿，金额必须为 0",
        ));
    }
    let delta_quantity = input.quantity.unwrap_or(0.0);
    // Δ = 0 不改变任何持仓，无意义：显式拒绝。`+` / `−` 两向都合法（ADR-0106
    // 决策 1：`+` = 折算/结转/送股、`−` = 缩股）；缩股幅度守卫（`|Δ|` 严格小于
    // 当前持仓）归投资域 [`split::plan_restatement`]，与批次快照同源、不在此另算。
    if delta_quantity == 0.0 {
        return Err(AppError::coded(
            "trade.split-quantity-zero",
            "份额调整数量不能为 0",
        ));
    }
    ensure_investment_account(
        conn,
        &input.account_id,
        "trade.split-account-not-investment",
        "份额调整必须使用投资账户",
    )?;
    // 出资账户准入（ADR-0096）：split 不在出资闭集内，携带即被既有
    // 「不能携带出资账户」拒绝——「无现金腿」由此天然成立，不另设第二份判定。
    let account_currency = account_currency_code(conn, &input.account_id)?;
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Split,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    // 在用批次重述快照（prepare 算定，apply 原样落盘）：零在用持仓在此拒绝——
    // 价值已含在原批次中，零持仓无从重述（ADR-0106 决策 7）。
    // 修改路径以本行落账序为界（ADR-0106 决策 6）：历史份额调整不回头改后续买入。
    let before_rowid = match existing_id {
        Some(id) => Some(conn.query_row(
            "SELECT rowid FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0),
        )?),
        None => None,
    };
    let restatement = split::plan_restatement(
        conn,
        &input.account_id,
        &instrument_id,
        delta_quantity,
        before_rowid,
    )?;
    if restatement.lots.is_empty() {
        return Err(AppError::coded(
            "trade.split-no-holding",
            "份额调整要求该标的有在用持仓（零持仓无从重述批次成本）",
        ));
    }

    Ok(SplitPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Split,
            // 无现金腿：行金额恒 0、六度量系数全 0。本位币金额同恒 0——
            // 0 在任何币种下的本位币折算恒为 0，不经汇率表（无现金腿不依赖汇率）。
            amount_cents: 0,
            currency_code: account_currency,
            amount_native_cents: 0,
            account_id: input.account_id.clone(),
            // 单标的、不跨账户：转入账户已拒绝、出资账户已被准入拒绝，恒 None。
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        delta_quantity,
        restated: restatement.lots,
        final_quantity: restatement.final_quantity,
        total_cost_cents: restatement.total_cost_cents,
    })
}

/// 校验并归一化一笔现金分红（dividend）输入（不落库）。本地创建与本地修改共用；
/// 重放形态走 [`replay_plan`] 的 Dividend 臂（同一守卫集、从命令携带字段装配）。
///
/// 语义（issue #1078 / ADR-0109）：分红是**归属于某标的**的现金收入——现金腿记
/// `amount_cents`（kind→度量矩阵既有行：account_flow +、income_net +），标的信息
/// 落 `security_transactions` 扩展行（action='dividend'，quantity / price 恒 NULL）。
/// 分红**不摊薄持仓成本**：不触碰 FIFO 批次、不写卖出匹配、不改变任何持仓数量；
/// 与已实现盈亏的关系是「累计收益的第三腿」（见投资域 `reports`）。
///
/// 守卫（grilling 定案，全部码化中文错误，ADR-0050）：
/// - 标的必填且存在；**不要求当前有持仓**（除息 / 到账错位合法）；
/// - 金额 > 0；
/// - 到账账户必须是**在用账户**（任意类型——分红可直达银行卡 / 现金 / 投资账户，
///   不限于投资账户），币种须与账户币种一致（现金腿不折算跨币种）；
/// - 不接受份额 / 成交单价 / 手续费 / 转入标的 / 转入账户（意图漂移 fail fast）；
/// - 商户 / 分类 / 保单由行为层参考数据携带准入拒绝（dividend 不在任何准入集）；
///   出资账户由出资准入闭集拒绝（仅 buy/sell 可携带，ADR-0096）。
fn prepare_dividend(conn: &Connection, input: &TransactionInput) -> Result<DividendPlan> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::coded("trade.dividend-instrument-required", "分红必须指定标的"))?
        .clone();
    // 标的存在性先于数值校验：身份错了，数值对错无从谈起（buy/sell/convert/split
    // 同一顺序）。标的类型不限（股票 / 基金 / 自建组合同构）。
    fetch_instrument_type(
        conn,
        &instrument_id,
        "分红",
        "trade.dividend-instrument-not-found",
    )?;
    if input.to_instrument_id.is_some() {
        return Err(AppError::coded(
            "trade.dividend-to-instrument-forbidden",
            "分红是单标的现金收入，不能携带转入标的",
        ));
    }
    if input.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.dividend-to-account-forbidden",
            "分红不跨账户，不能携带转入账户",
        ));
    }
    if input.quantity.is_some() {
        return Err(AppError::coded(
            "trade.dividend-quantity-forbidden",
            "分红无份额变动，不可提供数量",
        ));
    }
    if input.price_cents.is_some() {
        return Err(AppError::coded(
            "trade.dividend-price-forbidden",
            "分红无成交单价，不可提供单价",
        ));
    }
    if input.fee_cents.unwrap_or(0) != 0 {
        return Err(AppError::coded(
            "trade.dividend-fee-forbidden",
            "分红不接受手续费",
        ));
    }
    if input.amount_cents <= 0 {
        return Err(AppError::coded(
            "trade.dividend-amount-positive",
            "分红金额必须大于 0",
        ));
    }
    // 到账账户：任意在用账户（非投资账户亦合法），币种须与账户币种一致。
    let account_currency = active_account_currency(conn, &input.account_id)?;
    if input.currency_code != account_currency {
        return Err(AppError::codedp(
            "trade.dividend-currency-mismatch",
            format!(
                "分红币种（{}）必须与到账账户币种（{account_currency}）一致",
                input.currency_code
            ),
            &[&input.currency_code, &account_currency],
        ));
    }
    // 出资账户准入（ADR-0096）：dividend 不在出资闭集内，携带即被既有「不能携带
    // 出资账户」拒绝——到账账户就是现金腿端点，不另设第二份判定。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Dividend,
        input.funding_account_id.as_deref(),
        &account_currency,
    )?;
    let amount_native_cents =
        amount::convert_to_native(conn, input.amount_cents, &account_currency)?;
    Ok(DividendPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Dividend,
            amount_cents: input.amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            // 单标的、不跨账户：转入标的 / 转入账户已拒绝，出资账户已被准入拒绝。
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
    })
}

/// 分红到账账户校验（issue #1078）：账户必须存在且未软删除（任意类型），返回
/// 账户币种供币种一致性守卫。软删账户不可被**新选择**，历史引用照常保留——
/// 与商户 / 保单 / 出资账户同款语义。
fn active_account_currency(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "trade.dividend-account-not-found",
            format!("分红到账账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })
}

/// 分红副作用落库：`security_transactions` 扩展行（action='dividend'，
/// `quantity` / `price_cents` 恒 NULL）——来源列标的反查与累计收益第三腿的数据源。
/// 不触碰批次与卖出匹配：分红不改变持仓数量、不产生已实现盈亏。
fn write_dividend_side_effects(conn: &Connection, id: &str, instrument_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'dividend',NULL,NULL,0)",
        rusqlite::params![id, instrument_id],
    )?;
    Ok(())
}

/// 卖出交易的持仓/卖出关联副作用（创建与修改共用）。
///
/// 只写 `security_transactions` 记录、`security_lot_sales` 匹配与持仓扣减，不写交易行——
/// 修改路径先 [`revert`] 清空旧卖出再由本函数按新输入重建，创建路径在插入交易行后复用。
fn write_sell_side_effects(conn: &Connection, id: &str, plan: &SellPlan) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;

    // 匹配记录与逐批次成本（含耗尽批次闭合）由 prepare 的消耗快照单点算定，
    // 此处原样落盘、不复算（与转换转出同源，issue #1019）。
    let consumptions = &plan.consumed;

    // 分摊双闭合（issue #302）：①收入按匹配末位吸收余数，Σ 匹配收入 = 毛收入
    // （基金 = 权威金额 + 手续费）精确到分；②耗尽批次的匹配把批次总成本闭合到
    // 买入行权威金额（减去该批次此前各次匹配的 round 重建成本）——两者合起来
    // 钉死舍入不变式：全平仓后 Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额，精确到分。
    let match_count = consumptions.len();
    let mut allocated_fee_total = 0i64;
    let mut allocated_proceeds_total = 0i64;
    for (i, matched) in consumptions.iter().enumerate() {
        let lot = &matched.lot;
        // 匹配收入：非末匹配按 round(匹配数量 × 单价 ÷ 换算因子)，末匹配 = 毛收入 − 已分摊。
        let lot_proceeds = if i == match_count - 1 {
            plan.gross_proceeds_cents - allocated_proceeds_total
        } else {
            let proceeds = (lot.remaining_quantity * plan.price_cents as f64 / PRICE_UNITS_PER_FEN)
                .round() as i64;
            allocated_proceeds_total += proceeds;
            proceeds
        };
        let lot_cost = matched.cost_cents;
        // 费用按数量比例 floor 分摊、末匹配吸收余数（Σ 分摊 = 手续费精确到分）。
        let allocated_fee = if i == match_count - 1 {
            plan.fee_cents - allocated_fee_total
        } else {
            let fee =
                (plan.fee_cents as f64 * lot.remaining_quantity / plan.quantity).floor() as i64;
            allocated_fee_total += fee;
            fee
        };
        // 已实现盈亏 = 匹配收入 − 匹配成本 − 分摊费用（均整数分）。
        let realized_pnl = lot_proceeds - lot_cost - allocated_fee;
        let sale_id = new_uuid();
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,cost_per_unit_cents,realized_pnl_cents,currency_code,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![sale_id, id, lot.id, lot.remaining_quantity, lot.cost_per_unit_cents, realized_pnl, lot.currency_code, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![lot.remaining_quantity, now, device_id(conn)?, lot.id],
        )?;
    }

    Ok(())
}

/// 删除路径的持仓副作用回退（行为层 delete 编排入口专用，issue #940 / ADR-0097）。
///
/// - sell：回补其扣减的持仓并清空卖出关联（与修改路径同一 [`lots::restore_sell`]）；
/// - buy：**级联**——消费其持仓批次的在用 sell 逐笔回退持仓副作用，再整批清理
///   批次与匹配（含指向已软删 sell 的历史匹配行，随批次消失），返回被级联的
///   sell id 列表，供行为层软删其交易行并各自产出 delete op 与余额刷新；
/// - convert：**级联**——转入批次被在用 sell 消费时与 buy 同规逐笔级联；另回补
///   转出腿（逐批次精确回补）并整批清理转入批次与转换明细行；
/// - 其余 kind 无持仓副作用，返回空表。
///
/// buy / convert 的级联仅覆盖**在用 sell**；若该行的持仓批次已被在用的后续转换
/// 消耗，先拒绝（否则批次连同下游转换的转出消耗记录一并消失，下游失去回退与结转的
/// 唯一依据）。删除路径不经过 [`revert`] 的在用占用守卫：在用 sell 已被级联消化，
/// 守卫谓词天然为空。
///
/// 守卫、级联与清理模板单点归 `investment::unwind`（issue #1020），本函数只做委托。
pub fn release_for_delete(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
) -> Result<Vec<String>> {
    unwind::remove(conn, id, kind, unwind::Mode::Delete)
}

fn create_buy_lot(conn: &Connection, transaction_id: &str, plan: &BuyPlan) -> Result<()> {
    let lot_id = new_uuid();
    let now = now_iso();
    // 每份成本已在 prepare 按标的类型算定（基金锚定权威金额、其余锚定成交单价 +
    // 费用摊薄，均单次舍入），此处原样落批次——摊薄算法单一归属 prepare（issue #302）；
    // 重放路径的成本随命令携带（源端折算），经 replay_plan 装配后同样原样落批次。
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![transaction_id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![lot_id, plan.normalized.account_id, plan.instrument_id, transaction_id, plan.quantity, plan.cost_per_unit_cents, plan.normalized.currency_code, now, 1, device_id(conn)?],
    )?;
    Ok(())
}

pub struct SellPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    /// 卖出匹配的逐批次消耗规划（prepare 算定、apply 原样落库）：含各匹配成本
    /// 与耗尽批次闭合，apply 不再复算（与 `ConvertPlan.consumed` 同一形态）。
    pub(crate) consumed: Vec<Consumption>,
    /// 毛收入（分，费前）：基金 = 权威金额 + 手续费，其余 = round(数量 × 单价 ÷ 换算因子)。
    /// 卖出副作用的收入分摊以其为闭合基准（末匹配吸收余数，Σ 匹配收入 = 毛收入精确到分）。
    pub(crate) gross_proceeds_cents: i64,
}

/// 基金转换计划（ADR-0099）：归一化行（金额锚点 = 结转成本）+ 两侧腿数据 +
/// 转出腿 FIFO 消耗规划（prepare 算定、apply 原样落库）。
pub struct ConvertPlan {
    pub(crate) normalized: NormalizedTransaction,
    /// 转出标的（`security_transactions.instrument_id`）。
    pub(crate) instrument_id: String,
    /// 转入标的（`security_transactions.to_instrument_id`）。
    pub(crate) to_instrument_id: String,
    /// 转出份额（FIFO 消耗量）。
    pub(crate) quantity: f64,
    /// 转入份额（转入批次建仓量）。
    pub(crate) to_quantity: f64,
    /// 转出腿反算展示单价（万分之一元）。
    pub(crate) price_cents: i64,
    /// 手续费（整数分）：如实记录，不进支出口径、不摊入持仓成本。
    pub(crate) fee_cents: i64,
    /// 转出金额（确认单权威，分）：列表展示口径。
    pub(crate) out_amount_cents: i64,
    /// 转入金额（确认单权威，分）。
    pub(crate) in_amount_cents: i64,
    /// 转出腿逐批次消耗规划（含结转成本，合计 = 行金额锚点 = 转入批次总成本）。
    pub(crate) consumed: Vec<Consumption>,
}

impl ConvertPlan {
    /// 结转成本合计（分）：见 [`lots::total_cost`]。产出侧与重放侧均经本
    /// 访问器取值（单一来源）。
    pub(crate) fn carried_cost_cents(&self) -> i64 {
        lots::total_cost(&self.consumed)
    }
}

/// 投资交易计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
pub enum Plan {
    Buy(BuyPlan),
    Sell(SellPlan),
    Convert(ConvertPlan),
    /// 份额调整（ADR-0106）：批次重述快照随计划走，apply 原样落库。
    Split(SplitPlan),
    /// 现金分红（issue #1078）：现金腿 + 标的扩展行，不触碰 FIFO 批次。
    Dividend(DividendPlan),
}

impl Plan {
    /// 归一化交易行（供行为层经 `writer::NormalizedRow::try_from` 落库）。
    pub fn normalized(&self) -> &NormalizedTransaction {
        match self {
            Plan::Buy(p) => &p.normalized,
            Plan::Sell(p) => &p.normalized,
            Plan::Convert(p) => &p.normalized,
            Plan::Split(p) => &p.normalized,
            Plan::Dividend(p) => &p.normalized,
        }
    }
}

/// 校验并归一化一笔 buy/sell/convert/split/dividend 输入为 [`Plan`]（不落库、不产生副作用）。
///
/// 由行为层（`transaction`）在创建/修改路径按 kind 分派调用；
/// `kind` 为已解析的 [`TransactionKind`]，收到其余 kind 属编排错误，报错防误用。
/// `existing_id`：修改路径的既有交易 id（创建路径 `None`）——仅 split 消费，用于把
/// 重述目标限定在该行落账那一刻在场的批次（ADR-0106 决策 6）；其余 kind 忽略。
pub fn prepare(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => Ok(Plan::Buy(prepare_buy(conn, input)?)),
        TransactionKind::Sell => Ok(Plan::Sell(prepare_sell(conn, input)?)),
        TransactionKind::Convert => Ok(Plan::Convert(prepare_convert(conn, input)?)),
        TransactionKind::Split => Ok(Plan::Split(prepare_split(conn, input, existing_id)?)),
        TransactionKind::Dividend => Ok(Plan::Dividend(prepare_dividend(conn, input)?)),
        // 行为层穷尽分派保证仅转发投资 kind；其余 kind 属编排错误，显式拒绝防误用
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell/convert/split/dividend，收到: {kind}"
        ))),
    }
}

/// 应用计划的副作用（buy 建仓 / sell 卖出匹配 / convert 结转 / split 批次重述 /
/// dividend 扩展行）。由编排层在交易行落库后调用，与行写入同处一个事务；
/// `id` 为已落库的交易行 id。
pub fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Buy(p) => create_buy_lot(conn, id, p),
        Plan::Sell(p) => write_sell_side_effects(conn, id, p),
        Plan::Convert(p) => write_convert_side_effects(conn, id, p),
        Plan::Split(p) => split::write_split_side_effects(
            conn,
            id,
            &p.instrument_id,
            p.delta_quantity,
            &p.restated,
        ),
        Plan::Dividend(p) => write_dividend_side_effects(conn, id, &p.instrument_id),
    }
}

/// 回退一笔已存在 buy/sell/convert/split/dividend 的副作用，供行为层**修改**编排入口在清理阶段调用。
///
/// - buy：转换链守卫（批次已被在用后续转换消耗则拒绝）+ 在用占用守卫
///   （在用 sell 的匹配消耗本买入批次则拒绝）+ 清理持仓/买入关联；
/// - sell：回补持仓扣减并清空卖出关联；
/// - convert：转换链守卫 + 在用卖出占用守卫（转入批次被后续卖出则拒绝）
///   + 转出腿逐批次精确回补 + 转入批次与转换明细行清理（复用 buy 清理语义）。
///
/// 删除路径不经本函数（其持仓语义由 [`release_for_delete`] 承载：sell 回补 /
/// buy 与 convert 级联，issue #940 / ADR-0097 / ADR-0099）。
/// 守卫语义、用户可见措辞与清理模板单点归 `investment::unwind`（issue #1020）；
/// 非 buy/sell/convert 的 kind 无持仓副作用，防御性返回成功。
pub fn revert(conn: &Connection, id: &str, kind: TransactionKind) -> Result<()> {
    unwind::remove(conn, id, kind, unwind::Mode::Update).map(|_| ())
}

/// 转换副作用落库：`security_transactions`（两腿同记录）+ 转出腿逐批次消耗记录
/// 与持仓扣减 + 转入批次建仓（成本 = 结转成本，单次舍入到价格刻度）。
///
/// 不写 `security_lot_sales`：转换零已实现盈亏，假匹配会污染盈亏明细（ADR-0099 备选与否决）。
fn write_convert_side_effects(conn: &Connection, id: &str, plan: &ConvertPlan) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents,to_instrument_id,to_quantity,out_amount_cents,in_amount_cents) \
         VALUES (?1,?2,'convert',?3,?4,?5,?6,?7,?8,?9)",
        rusqlite::params![
            id,
            plan.instrument_id,
            plan.quantity,
            plan.price_cents,
            plan.fee_cents,
            plan.to_instrument_id,
            plan.to_quantity,
            plan.out_amount_cents,
            plan.in_amount_cents
        ],
    )?;
    // 转出腿：逐批次消耗记录（修改回退 / 删除精确回补的唯一依据）+ 持仓扣减。
    for consumed in &plan.consumed {
        conn.execute(
            "INSERT INTO security_lot_conversions (id,transaction_id,lot_id,quantity,cost_per_unit_cents,cost_cents,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                new_uuid(),
                id,
                consumed.lot.id,
                consumed.lot.remaining_quantity,
                consumed.lot.cost_per_unit_cents,
                consumed.cost_cents,
                now
            ],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![consumed.lot.remaining_quantity, now, device_id(conn)?, consumed.lot.id],
        )?;
    }
    // 转入腿：以结转成本建立批次（每份成本 = 结转成本 ÷ 转入份额，单次舍入）——
    // 批次锚定本转换行，既有「耗尽批次成本闭合」在此闭合到结转成本。
    let carried_cost_cents = plan.carried_cost_cents();
    let cost_per_unit_cents =
        (carried_cost_cents as f64 * PRICE_UNITS_PER_FEN / plan.to_quantity).round() as i64;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![
            new_uuid(),
            plan.normalized.account_id,
            plan.to_instrument_id,
            id,
            plan.to_quantity,
            cost_per_unit_cents,
            plan.normalized.currency_code,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(())
}

/// 重放形态的计划重建（issue #861 / ADR-0091 决策 2/3）：从命令字段与随行
/// 归一化行重建 [`apply`] 所需计划，不重折算、不产出 op。
///
/// 与本地 [`prepare`] 的分工：归一化行已随命令携带（金额与本位币折算结果为
/// 落定值，重放不依赖本地汇率表），买入每份成本随命令携带、不重算（重算需
/// 读标的类型，属本地状态）；本函数只做「apply 依赖在位」校验与副作用计划装配：
/// - buy/sell 恒无转入账户（形态漂移与本地同码拒绝，伪造载荷不得绕开本地
///   不变量，issue #1187）；
/// - 数量为正（与本地 prepare 同码，防伪造载荷落出零数量副作用）；
/// - 标的存在（兼类型读取：基金/非基金的权威分流语义一致，与本地同码）；
/// - 账户在位且为投资账户（与本地同码；账户存活已由行为层重放入口先行校验）；
/// - 卖出的可卖数量（FIFO 批次快照在本端重建——同序重放 ⇒ 同一匹配结果，
///   与本地同码）。
///
/// 依赖缺失以与本地写入同码的码化错误上抛，由同步引擎挂起进队列（issue #856），
/// 依赖方 op 补齐后重投递自然重试。
pub(crate) fn replay_plan(
    conn: &Connection,
    kind: TransactionKind,
    row: &NormalizedTransaction,
    fields: &InvestmentCommandFields,
) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => {
            // 形态漂移守卫（与本地 prepare 同码，issue #1187）：buy 行恒无
            // to_account_id，携带即伪造/漂移载荷，重放不得绕开本地不变量。
            if row.to_account_id.is_some() {
                return Err(AppError::coded(
                    "trade.buy-to-account-forbidden",
                    "买入不能携带转入账户：资金流出归结算账户（出资账户或投资账户），账户间划转请使用转账",
                ));
            }
            if fields.quantity <= 0.0 {
                return Err(AppError::coded(
                    "trade.buy-quantity-positive",
                    "买入数量必须大于 0",
                ));
            }
            // 标的存在性校验（与本地同码）；类型不参与买入重放装配（每份成本
            // 随命令携带），仅作依赖在位检查。
            fetch_instrument_type(
                conn,
                &fields.instrument_id,
                "买入",
                "trade.buy-instrument-not-found",
            )?;
            ensure_investment_account(
                conn,
                &row.account_id,
                "trade.buy-account-not-investment",
                "买入交易必须使用投资账户",
            )?;
            // 每份成本是 prepare 单次舍入的派生结果，随命令携带（源端折算）；
            // 缺失属载荷伪造或程序缺陷（产出侧永不产 None），fail loud 由引擎
            // 挂起承接，不以本地重算静默兑底。
            let cost_per_unit_cents = fields
                .cost_per_unit_cents
                .ok_or_else(|| AppError::Invalid("买入命令缺少每份成本字段（程序缺陷）".into()))?;
            Ok(Plan::Buy(BuyPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
                quantity: fields.quantity,
                price_cents: fields.price_cents,
                fee_cents: fields.fee_cents,
                cost_per_unit_cents,
            }))
        }
        TransactionKind::Sell => {
            // 形态漂移守卫（与本地 prepare 同码，issue #1187）：sell 行恒无
            // to_account_id，携带即伪造/漂移载荷，重放不得绕开本地不变量。
            if row.to_account_id.is_some() {
                return Err(AppError::coded(
                    "trade.sell-to-account-forbidden",
                    "卖出不能携带转入账户：资金流入归结算账户（出资账户或投资账户），账户间划转请使用转账",
                ));
            }
            if fields.quantity <= 0.0 {
                return Err(AppError::coded(
                    "trade.sell-quantity-positive",
                    "卖出数量必须大于 0",
                ));
            }
            let instrument_type = fetch_instrument_type(
                conn,
                &fields.instrument_id,
                "卖出",
                "trade.sell-instrument-not-found",
            )?;
            ensure_investment_account(
                conn,
                &row.account_id,
                "trade.sell-account-not-investment",
                "卖出交易必须使用投资账户",
            )?;
            // 毛收入重建（与本地 prepare 同式）：基金 = 权威金额 + 手续费
            // （金额随行携带），其余 = round(数量 × 单价 ÷ 换算因子)。
            let gross_proceeds_cents = if instrument_type == "fund" {
                row.amount_cents + fields.fee_cents
            } else {
                (fields.quantity * fields.price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64
            };
            // FIFO 批次快照在本端重建（同序重放 ⇒ 与源端同状态 ⇒ 同一匹配结果；
            // 排序键 rowid 的跨端确定性依据见 `lots::active_lots`），消耗规划
            //（含「可卖出数量不足」守卫）由 `lots::plan` 单点算定。
            let active_lots = lots::active_lots(conn, &row.account_id, &fields.instrument_id)?;
            let consumed = lots::plan(conn, &active_lots, fields.quantity)?;
            Ok(Plan::Sell(SellPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
                quantity: fields.quantity,
                price_cents: fields.price_cents,
                fee_cents: fields.fee_cents,
                consumed,
                gross_proceeds_cents,
            }))
        }
        TransactionKind::Dividend => {
            // 与本地 prepare 同序的守卫：身份（标的存在）→ 数值 → 到账账户。
            // 金额是本位币折算结果的原始腿，随归一化行携带（源端折算，ADR-0091
            // 决策 3），重放端校验其正性；形态漂移（携带转入账户 / 出资账户 /
            // 币种与账户不符）与本地同码拒绝——伪造载荷不得绕开本地不变量。
            fetch_instrument_type(
                conn,
                &fields.instrument_id,
                "分红",
                "trade.dividend-instrument-not-found",
            )?;
            if row.amount_cents <= 0 {
                return Err(AppError::coded(
                    "trade.dividend-amount-positive",
                    "分红金额必须大于 0",
                ));
            }
            if row.to_account_id.is_some() {
                return Err(AppError::coded(
                    "trade.dividend-to-account-forbidden",
                    "分红不跨账户，不能携带转入账户",
                ));
            }
            crate::transaction::funding::validate_funding_account(
                conn,
                TransactionKind::Dividend,
                row.funding_account_id.as_deref(),
                &row.currency_code,
            )?;
            let account_currency = active_account_currency(conn, &row.account_id)?;
            if row.currency_code != account_currency {
                return Err(AppError::codedp(
                    "trade.dividend-currency-mismatch",
                    format!(
                        "分红币种（{}）必须与到账账户币种（{account_currency}）一致",
                        row.currency_code
                    ),
                    &[&row.currency_code, &account_currency],
                ));
            }
            Ok(Plan::Dividend(DividendPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
            }))
        }
        // 行为层重放入口穷尽分派保证仅转发 buy/sell 至本函数（转换走
        // [`replay_convert_plan`]，份额调整走 [`replay_split_plan`]）；其余 kind
        // 属编排错误，显式拒绝防误用（不引入 panic 构造，ADR-0060）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Split
        | TransactionKind::Convert => Err(AppError::Invalid(format!(
            "投资层重放仅处理 buy/sell/dividend（convert/split 走各自入口），收到: {kind}"
        ))),
    }
}

/// 转换（convert）重放形态的计划重建（issue #980 / ADR-0099 决策 6）：从随命令
/// 携带的转换字段与归一化行重建 [`apply`] 所需计划。
///
/// 与本地 [`prepare_convert`] 的分工：归一化行（金额锚点 = 结转成本、本位币折算
/// 结果）与两侧确认金额/份额随命令携带，重放不重折算、不读本地汇率表；**逐批次
/// 消耗记录按本地 FIFO 快照重建**（转出时的 FIFO 状态在源端后续交易发生后同样
/// 不可重建，两端只能靠同序重放得到同一快照，ADR-0091 决策 3）；源端算定的
/// `carried_cost_cents` 是权威比对基准——本地重建的逐批次成本合计与它不一致
/// （或与行金额锚点不一致）即本地快照发散，显式失败挂起，不静默落出错误成本基础。
///
/// 依赖缺失（标的不存在、账户非投资、可卖数量不足）以与本地写入同码的码化错误
/// 上抛，由同步引擎挂起进队列（issue #856），依赖方 op 补齐后重投递自然重试。
pub(crate) fn replay_convert_plan(
    conn: &Connection,
    row: &NormalizedTransaction,
    fields: &ConvertCommandFields,
) -> Result<Plan> {
    // 与本地 prepare 同序的守卫：身份（两标的互异 + 存在）→ 数值 → 账户 → 持仓。
    if fields.instrument_id == fields.to_instrument_id {
        return Err(AppError::coded(
            "trade.convert-same-instrument",
            "转换的转出标的与转入标的不能相同",
        ));
    }
    fetch_instrument_type(
        conn,
        &fields.instrument_id,
        "转换转出",
        "trade.convert-instrument-not-found",
    )?;
    fetch_instrument_type(
        conn,
        &fields.to_instrument_id,
        "转换转入",
        "trade.convert-to-instrument-not-found",
    )?;
    if fields.quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.convert-quantity-positive",
            "转换转出份额必须大于 0",
        ));
    }
    if fields.to_quantity <= 0.0 {
        return Err(AppError::coded(
            "trade.convert-to-quantity-positive",
            "转换转入份额必须大于 0",
        ));
    }
    if fields.out_amount_cents <= 0 {
        return Err(AppError::coded(
            "trade.convert-out-amount-positive",
            "转换转出金额必须大于 0",
        ));
    }
    if fields.in_amount_cents <= 0 {
        return Err(AppError::coded(
            "trade.convert-in-amount-positive",
            "转换转入金额必须大于 0",
        ));
    }
    // 展示单价由确认单金额 ÷ 份额反算（与本地录入同一公式单点）。
    let price_cents = derived_out_price_cents(fields.out_amount_cents, fields.quantity)?;
    ensure_investment_account(
        conn,
        &row.account_id,
        "trade.convert-account-not-investment",
        "转换交易必须使用投资账户",
    )?;
    // 不跨账户（与本地录入同码）：两腿是同一投资账户内的两个标的，携带转入账户即
    // 伪造/漂移载荷，重放不得绕开本地不变量（CONTEXT-sync「经同一接缝执行」）。
    if row.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.convert-to-account-forbidden",
            "转换不跨账户：转出与转入必须同属一个投资账户，不能携带转入账户",
        ));
    }
    // 出资账户准入（与本地录入共用同一条接缝）：convert 不在出资闭集内，携带即拒绝。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Convert,
        row.funding_account_id.as_deref(),
        &row.currency_code,
    )?;
    // FIFO 批次快照在本端重建（排序键 rowid 的跨端确定性依据同 [`prepare_sell`]）：
    // 同序重放 ⇒ 与源端同状态 ⇒ 同一逐批次消耗结果；消耗规划（含「可卖出数量不足」
    // 守卫）由 `lots::plan` 单点算定。
    let active_lots = lots::active_lots(conn, &row.account_id, &fields.instrument_id)?;
    let consumed = lots::plan(conn, &active_lots, fields.quantity)?;
    // 源端结转成本与本地重建的逐批次成本合计必须一致（兼行金额锚点校验）：
    // 不一致即本地 FIFO 快照发散（前序 op 缺失或非确定），挂起待裁决，
    // 不静默落出错误成本基础（ADR-0099 决策 6「不静默错账」）。
    let rebuilt_cost_cents = lots::total_cost(&consumed);
    if rebuilt_cost_cents != fields.carried_cost_cents
        || row.amount_cents != fields.carried_cost_cents
    {
        return Err(AppError::codedp(
            "transaction.convert-carried-cost-mismatch",
            format!(
                "转换结转成本校验不一致（源端 {}，本机重建 {rebuilt_cost_cents}，行金额 {}），已挂起等待处理",
                fields.carried_cost_cents, row.amount_cents
            ),
            &[
                &fields.carried_cost_cents.to_string(),
                &rebuilt_cost_cents.to_string(),
                &row.amount_cents.to_string(),
            ],
        ));
    }
    Ok(Plan::Convert(ConvertPlan {
        normalized: row.clone(),
        instrument_id: fields.instrument_id.clone(),
        to_instrument_id: fields.to_instrument_id.clone(),
        quantity: fields.quantity,
        to_quantity: fields.to_quantity,
        price_cents,
        fee_cents: fields.fee_cents,
        out_amount_cents: fields.out_amount_cents,
        in_amount_cents: fields.in_amount_cents,
        consumed,
    }))
}

/// 份额调整（split）重放形态的计划重建（issue #1053 / ADR-0106 决策 9）：从随命令
/// 携带的份额调整字段与归一化行重建 [`apply`] 所需计划。
///
/// 与本地 [`prepare_split`] 的分工：归一化行（无现金腿，金额恒 0）与 Δ 随命令携带；
/// **逐批次重述结果不随载荷携带**（重述是「当前批次快照 + Δ」的纯函数，重放端按 op
/// 序重建的批次快照与源端一致），本端以本地快照独立重建。比对锚点 = 源端携带的
/// **最终持仓与批次总成本**——本地重建的两个总量与其不一致（前序 op 未达、载荷被
/// 篡改）即本地快照发散，码化挂起待裁决，不静默落出错误持仓与成本基础（同 convert
/// 的结转成本比对，ADR-0099 决策 6）。
///
/// `existing_id`：修改路径传本行 id（重述目标以本行落账序为界，ADR-0106 决策 6，
/// 与本地修改同口径），创建路径传 `None` 取当前全部在用批次。
///
/// 依赖缺失（标的不存在、账户非投资、零在用持仓、缩股越界）以与本地写入同码的
/// 码化错误上抛，由同步引擎挂起进队列（issue #856），依赖方 op 补齐后重投递自然重试。
pub(crate) fn replay_split_plan(
    conn: &Connection,
    row: &NormalizedTransaction,
    fields: &SplitCommandFields,
    existing_id: Option<&str>,
) -> Result<Plan> {
    // 与本地 prepare 同序的守卫：身份（标的存在）→ 数值 → 账户 → 持仓。
    fetch_instrument_type(
        conn,
        &fields.instrument_id,
        "份额调整",
        "trade.split-instrument-not-found",
    )?;
    if fields.delta_quantity == 0.0 {
        return Err(AppError::coded(
            "trade.split-quantity-zero",
            "份额调整数量不能为 0",
        ));
    }
    // 无现金腿（ADR-0106 决策 1）：行金额恒 0，携带非零金额即伪造/漂移载荷，
    // 重放不得绕开本地不变量（CONTEXT-sync「经同一接缝执行」）。
    if row.amount_cents != 0 || row.amount_native_cents != 0 {
        return Err(AppError::coded(
            "trade.split-amount-forbidden",
            "份额调整无现金腿，金额必须为 0",
        ));
    }
    ensure_investment_account(
        conn,
        &row.account_id,
        "trade.split-account-not-investment",
        "份额调整必须使用投资账户",
    )?;
    // 不跨账户（与本地录入同码）：单标的份额变动携带转入账户即伪造载荷。
    if row.to_account_id.is_some() {
        return Err(AppError::coded(
            "trade.split-to-account-forbidden",
            "份额调整不跨账户：只能调整同一投资账户内的标的，不能携带转入账户",
        ));
    }
    // 出资账户准入（与本地录入共用同一条接缝）：split 不在出资闭集内，携带即拒绝。
    crate::transaction::funding::validate_funding_account(
        conn,
        TransactionKind::Split,
        row.funding_account_id.as_deref(),
        &row.currency_code,
    )?;
    // 修改路径以本行落账序为界（ADR-0106 决策 6）：取本端该交易行的 rowid，
    // 只重述它落账那一刻在场的批次。
    let before_rowid = match existing_id {
        Some(id) => Some(conn.query_row(
            "SELECT rowid FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0),
        )?),
        None => None,
    };
    let restatement = split::plan_restatement(
        conn,
        &row.account_id,
        &fields.instrument_id,
        fields.delta_quantity,
        before_rowid,
    )?;
    // 零在用持仓（前序买入 op 未达）在此码化拒绝，与本地创建同码（ADR-0106 决策 7）。
    if restatement.lots.is_empty() {
        return Err(AppError::coded(
            "trade.split-no-holding",
            "份额调整要求该标的有在用持仓（零持仓无从重述批次成本）",
        ));
    }
    // 源端锚点比对（ADR-0106 决策 9）：本地重建的最终持仓与批次总成本必须与源端
    // 携带值一致；不一致即本地快照发散（前序 op 未达 / 载荷被篡改），挂起待裁决。
    let quantity_diverges =
        (restatement.final_quantity - fields.final_quantity).abs() > lots::QTY_GUARD_EPSILON;
    if quantity_diverges || restatement.total_cost_cents != fields.total_cost_cents {
        let source_quantity = lots::format_quantity_for_message(fields.final_quantity);
        let local_quantity = lots::format_quantity_for_message(restatement.final_quantity);
        let source_cost = fields.total_cost_cents.to_string();
        let local_cost = restatement.total_cost_cents.to_string();
        return Err(AppError::codedp(
            "transaction.split-restatement-mismatch",
            format!(
                "份额调整重放校验不一致（源端 最终持仓 {source_quantity}、总成本 {source_cost}；本机重建 {local_quantity}、{local_cost}），已挂起等待处理"
            ),
            &[&source_quantity, &source_cost, &local_quantity, &local_cost],
        ));
    }
    Ok(Plan::Split(SplitPlan {
        normalized: row.clone(),
        instrument_id: fields.instrument_id.clone(),
        delta_quantity: fields.delta_quantity,
        restated: restatement.lots,
        final_quantity: restatement.final_quantity,
        total_cost_cents: restatement.total_cost_cents,
    }))
}

/// 账户投资类型校验（重放形态）：行内引用的账户必须存在且为投资类型；
/// 与本地 prepare 同码同文案，失败由引擎挂起待裁决。账户存活（存在且未软删）
/// 已由行为层重放入口先行校验，此处只补类型语义。
fn ensure_investment_account(
    conn: &Connection,
    account_id: &str,
    code: &str,
    msg: &str,
) -> Result<()> {
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}
