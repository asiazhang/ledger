use rusqlite::{Connection, OptionalExtension};

use super::model::{TransactionConvert, TransactionTrade};
use super::prices::PRICE_UNITS_PER_FEN;
use crate::accounts::AccountType;
use crate::db::query::{FromRow, query_all, query_one};
use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::sync_engine::device_id;
use crate::transaction::amount;
use crate::transaction::amount::TransactionKind;
use crate::transaction::command::InvestmentCommandFields;
use crate::transaction::{ConvertFields, NormalizedTransaction, TransactionInput};

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
/// 删除路径专用的 [`release_for_delete`]（issue #940 / ADR-0097），承载三类投资 kind：
/// buy（建仓）/ sell（FIFO 卖出匹配与已实现盈亏）/ convert（基金转换，ADR-0099：
/// 转出腿 FIFO 消耗与结转成本、转入批次以结转成本建仓、零已实现盈亏）。
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
/// 与 lot/匹配副作用同处一个事务。
pub(crate) struct ActiveLot {
    pub(crate) id: String,
    pub(crate) remaining_quantity: f64,
    pub(crate) cost_per_unit_cents: i64,
    pub(crate) currency_code: String,
}

impl FromRow for ActiveLot {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ActiveLot {
            id: row.get(0)?,
            remaining_quantity: row.get(1)?,
            cost_per_unit_cents: row.get(2)?,
            currency_code: row.get(3)?,
        })
    }
}

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
        // 不静默吞掉（非法输入 fail fast，与 dividend/split 显式拒绝同一原则）。
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
            to_account_id: input.to_account_id.clone(),
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
/// 卖出匹配持仓等副作用由 [`apply`] 在落库时按其身份执行。
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

    // FIFO 排序键 = rowid（本端插入序，先买先卖）：不得用 created_at/id——
    // now_iso 为秒级精度，同秒建仓时随机 id tiebreak 在重放端会排出与源端
    // 不同的顺序，卖出匹配发散（确定性重放，ADR-0091 决策 2/3，issue #861）；
    // 重放端按 op 序插入批次，rowid 相对序与源端恒一致。
    let lots: Vec<ActiveLot> = fifo_active_lots(conn, &input.account_id, &instrument_id)?;
    let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if total_available < quantity {
        let avail = total_available.to_string();
        let qty = quantity.to_string();
        return Err(AppError::codedp(
            "trade.insufficient-holding",
            format!("可卖出数量不足，当前持有 {total_available}，尝试卖出 {quantity}"),
            &[&avail, &qty],
        ));
    }

    Ok(SellPlan {
        normalized: NormalizedTransaction {
            kind: TransactionKind::Sell,
            amount_cents,
            currency_code: account_currency,
            amount_native_cents,
            account_id: input.account_id.clone(),
            to_account_id: input.to_account_id.clone(),
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
        lots,
        gross_proceeds_cents: gross_proceeds,
    })
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
    // 转出腿展示单价：确认单金额 ÷ 份额反算到万分之一元（金额权威、单价反算，与场外基金同款）。
    let price_cents = (out_amount_cents as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
    if price_cents <= 0 {
        return Err(AppError::coded(
            "trade.derived-price-positive",
            "反算单价必须大于 0（确认金额过小或份额过大）",
        ));
    }
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
    let lots = fifo_active_lots(conn, &input.account_id, &instrument_id)?;
    let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if total_available < quantity {
        let avail = total_available.to_string();
        let qty = quantity.to_string();
        return Err(AppError::codedp(
            "trade.insufficient-holding",
            format!("可卖出数量不足，当前持有 {total_available}，尝试卖出 {quantity}"),
            &[&avail, &qty],
        ));
    }
    // 转出腿 FIFO 消耗与逐批次结转成本（含耗尽批次闭合）在 prepare 阶段算定：
    // 它是行金额锚点与转入批次成本的唯一依据，apply 原样落消耗记录与批次。
    let consumed = plan_lot_consumption(conn, &lots, quantity)?;
    let carried_cost_cents: i64 = consumed.iter().map(|c| c.cost_cents).sum();
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

    // 匹配记录与逐批次成本（含耗尽批次闭合）由消耗规划单点算定，与转换转出同源。
    let consumptions = plan_lot_consumption(conn, &plan.lots, plan.quantity)?;

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

/// 单批次消耗的分摊结果（卖出匹配与转换转出共用，ADR-0038 / ADR-0099）。
pub(crate) struct Consumption {
    /// 被消耗的批次；`remaining_quantity` 为**本次消耗数量**（非批次剩余）。
    pub(crate) lot: ActiveLot,
    /// 本次消耗成本（分）——卖出为匹配成本、转换为结转成本。
    pub(crate) cost_cents: i64,
}

/// 该批次此前已消耗成本合计（分）：卖出匹配与转换转出两条消耗记录同口径求和。
///
/// 逐条按「数量 × 批次每份成本 ÷ 换算因子」单次舍入后求和，与既有卖出匹配的
/// 重建口径一致；批次被转换后再被卖出/再转换时，已结转走的部分不被重复计入。
fn prior_consumed_cost_cents(
    conn: &Connection,
    lot_id: &str,
    cost_per_unit_cents: i64,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT quantity FROM security_lot_sales WHERE lot_id=?1 \
         UNION ALL \
         SELECT quantity FROM security_lot_conversions WHERE lot_id=?1",
    )?;
    let quantities = stmt
        .query_map(rusqlite::params![lot_id], |r| r.get::<_, f64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(quantities
        .iter()
        .map(|q| (q * cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64)
        .sum())
}

/// FIFO 排序键 = rowid（本端插入序，先买先消耗）：不得用 created_at/id——
/// now_iso 为秒级精度，同秒建仓时随机 id tiebreak 在重放端会排出与源端
/// 不同的顺序，消耗匹配发散（确定性重放，ADR-0091 决策 2/3，issue #861）；
/// 重放端按 op 序插入批次，rowid 相对序与源端恒一致。
///
/// 卖出匹配与转换转出共用同一取数口径（同一批次的消耗次序在两种路径下必须一致）。
fn fifo_active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> Result<Vec<ActiveLot>> {
    query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY rowid ASC",
        rusqlite::params![account_id, instrument_id],
    )
}

/// 按 FIFO 顺序把 `quantity` 分摊到 `lots` 上，并逐批次算定成本（分）。
///
/// 成本口径（卖出匹配与转换转出共用，ADR-0038 / ADR-0099）：
/// - 耗尽批次 → 买入锚点行权威金额 − 该批次此前已消耗成本之和（闭合，精确到分）；
/// - 非耗尽批次 → round(消耗数量 × 批次每份成本 ÷ 换算因子)。
fn plan_lot_consumption(
    conn: &Connection,
    lots: &[ActiveLot],
    quantity: f64,
) -> Result<Vec<Consumption>> {
    let mut remaining = quantity;
    let mut out: Vec<Consumption> = Vec::new();
    for lot in lots {
        if remaining <= 0.0 {
            break;
        }
        let matched = lot.remaining_quantity.min(remaining);
        let exhausts_lot = remaining >= lot.remaining_quantity;
        let cost_cents = if exhausts_lot {
            let lot_total_cents: i64 = conn.query_row(
                "SELECT t.amount_cents FROM security_lots l \n                 JOIN transactions t ON t.id = l.buy_transaction_id WHERE l.id=?1",
                rusqlite::params![lot.id],
                |r| r.get(0),
            )?;
            lot_total_cents - prior_consumed_cost_cents(conn, &lot.id, lot.cost_per_unit_cents)?
        } else {
            (matched * lot.cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64
        };
        out.push(Consumption {
            lot: ActiveLot {
                id: lot.id.clone(),
                remaining_quantity: matched,
                cost_per_unit_cents: lot.cost_per_unit_cents,
                currency_code: lot.currency_code.clone(),
            },
            cost_cents,
        });
        remaining -= matched;
    }
    Ok(out)
}

/// 消费某买入/转换持仓批次的**在用** sell id 列表（issue #940 级联删除的级联对象）。
///
/// 「在用卖出占用」归因谓词：按 `security_lot_sales` 归因到 sell 交易行、只计未软删
/// 者——已删 sell 的历史匹配（幽灵占用）不计入，既不触发守卫、也不阻塞级联查询。
fn active_sell_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT s.sell_transaction_id FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 消费某买入/转换持仓批次的**在用** convert id 列表（转换链守卫的对象）。
///
/// 与 [`active_sell_ids_on_own_lots`] 同款归因谓词：按 `security_lot_conversions`
/// 归因到 convert 交易行、只计未软删者。一条转换单拆多腿时，后腿消耗前腿建起的
/// 转入批次，链式依赖由此谓词可见。
fn active_convert_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.transaction_id FROM security_lot_conversions c \
         JOIN transactions t ON t.id = c.transaction_id \
         WHERE t.is_deleted = 0 AND c.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 转换链守卫：本行的持仓批次若已被**在用**的后续转换消耗，则拒绝修改/删除。
///
/// 修改会重建批次、删除会连带清空批次，两者都会把后续转换的转出消耗记录一并抹掉
/// （那是它精确回补与结转成本的唯一依据）——链式转换（一条转换单拆多腿）是常态，
/// 所以这条链必须在原始 FIFO 状态还在时从最后一腿往前处理。
///
/// 措辞与错误码由调用入口单点定义（ADR-0033 决策 #4），本域不持文案。
fn guard_no_convert_consumed(conn: &Connection, id: &str, code: &str, msg: &str) -> Result<()> {
    if !active_convert_ids_on_own_lots(conn, id)?.is_empty() {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

/// 持仓副作用守卫的措辞与错误码组（修改入口）：由行为层单点定义后下传
/// （ADR-0033 决策 #4）——本域只持守卫语义，不持用户可见文案。
///
/// sell 占用守卫两对：buy 与 convert 的「在用卖出占用」谓词同一，但用户可见
/// 主体不同（买入 vs 转换的转入份额），措辞与错误码各自单点（同一入口同一文案）。
pub struct GuardMessages<'a> {
    /// 买入已有部分卖出（修改入口）。
    pub partially_sold_code: &'a str,
    pub partially_sold_msg: &'a str,
    /// 转换的转入份额已被后续卖出（修改入口）。
    pub convert_partially_sold_code: &'a str,
    pub convert_partially_sold_msg: &'a str,
    /// 持仓份额已被后续转换消耗（修改入口）。
    pub consumed_by_convert_code: &'a str,
    pub consumed_by_convert_msg: &'a str,
}

/// 整批清理一笔买入或转换**转入腿**的持仓关联：批次、批次的全部卖出匹配与自身的
/// `security_transactions` 明细行（`transaction_id` 为主键，一交易至多一行——buy 行
/// 或 convert 行之一）。
///
/// 卖出匹配按 `lot_id` 归批清理——指向已软删 sell 的历史匹配行（旧版
/// 「sell 删除不回补」遗留的幽灵占用，issue #940）随批次一并消失；
/// 删除路径（级联后）与修改路径重建（在用占用守卫放行后）共用。
/// 转换行还需先经 [`reverse_convert_out_leg`] 回补转出腿（本函数删目标行会按
/// `security_lot_conversions.transaction_id` 的外键级联删掉消耗记录）。
fn purge_lot_artifacts(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM security_lot_sales WHERE lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id=?1)",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_lots WHERE buy_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 回补一笔转换的**转出腿**：把逐批次消耗记录的数量加回原批次剩余，再删除本转换的
/// `security_lot_conversions` 记录。
///
/// 这是删除/修改精确回补的唯一依据（转出时的 FIFO 状态在后续交易发生后不可重建，
/// ADR-0099 决策 2）——回补必须发生在清目标行之前：删 `security_transactions` 行会按
/// 外键级联清掉消耗记录，数量就再也回不去了。
fn reverse_convert_out_leg(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_conversions WHERE transaction_id=?1")?;
    let consumptions: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in consumptions {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_conversions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 在用卖出占用守卫：该行（买入或转换转入腿）的持仓批次已被**在用** sell 的匹配
/// 消耗则拒绝。守卫谓词按在用归因（见 [`active_sell_ids_on_own_lots`]），不再按批次
/// 剩余数量判定：已删 sell 的幽灵扣减不再把行永久锁死（issue #940 修复的死锁根源）。
///
/// `code` / `msg` 为调用入口单点定义的措辞（见 `transaction::behavior` 的入口文案
/// 常量，ADR-0033 决策 #4）。删除路径不经本守卫：在用 sell 已被级联消化。
fn guard_no_active_sell(conn: &Connection, id: &str, code: &str, msg: &str) -> Result<()> {
    let partially_sold: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.sell_transaction_id) FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if partially_sold > 0 {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

/// 修改路径的买入持仓关联守卫 + 清理。
///
/// 若该买入已有**在用**卖出占用（在用 sell 的匹配消耗本买入批次）则拒绝清理——
/// 修改会重建批次、破坏在用卖出的已实现盈亏。`partially_sold_msg` 为调用入口单点
/// 定义的措辞（见 `transaction::behavior` 的入口文案常量，ADR-0033 决策 #4）。
fn cleanup_buy_side_effects(
    conn: &Connection,
    id: &str,
    partially_sold_code: &str,
    partially_sold_msg: &str,
) -> Result<()> {
    guard_no_active_sell(conn, id, partially_sold_code, partially_sold_msg)?;
    purge_lot_artifacts(conn, id)
}

/// 修改路径的转换持仓关联守卫 + 清理（ADR-0099 决策 5：`revert` 复用 buy 清理语义）。
///
/// 顺序即语义：
/// 1. 转换链守卫——本转换建起的**转入批次**若已被在用的后续转换消耗，修改会连带
///    抹掉下游的转出消耗记录（链式转换须从最后一腿往前处理）；
/// 2. 在用卖出占用守卫——转入批次若已被后续卖出，修改会重建批次、破坏该卖出的
///    已实现盈亏（「部分卖出禁改」对转换批次同等生效）；
/// 3. 回补**转出腿**（逐批次精确回补，必须先于清目标行，见 [`reverse_convert_out_leg`]）；
/// 4. 整批清理转入批次与转换明细行。
fn cleanup_convert_side_effects(
    conn: &Connection,
    id: &str,
    guards: &GuardMessages<'_>,
) -> Result<()> {
    guard_no_convert_consumed(
        conn,
        id,
        guards.consumed_by_convert_code,
        guards.consumed_by_convert_msg,
    )?;
    guard_no_active_sell(
        conn,
        id,
        guards.convert_partially_sold_code,
        guards.convert_partially_sold_msg,
    )?;
    reverse_convert_out_leg(conn, id)?;
    purge_lot_artifacts(conn, id)
}

/// 删除路径的持仓副作用回退（行为层 delete 编排入口专用，issue #940 / ADR-0097）。
///
/// - sell：回补其扣减的持仓并清空卖出关联（与修改路径同一 [`reverse_sell`]）；
/// - buy：**级联**——消费其持仓批次的在用 sell 逐笔回退持仓副作用，再整批清理
///   批次与匹配（含指向已软删 sell 的历史匹配行，随批次消失），返回被级联的
///   sell id 列表，供行为层软删其交易行并各自产出 delete op 与余额刷新；
/// - convert：**级联**——转入批次被在用 sell 消费时与 buy 同规逐笔级联；另回补
///   转出腿（逐批次精确回补）并整批清理转入批次与转换明细行；
/// - 其余 kind 无持仓副作用，返回空表。
///
/// buy / convert 的级联仅覆盖**在用 sell**；若该行的持仓批次已被在用的后续转换
/// 消耗，先以 [`guard_no_convert_consumed`] 拒绝（否则批次连同下游转换的转出消耗
/// 记录一并消失，下游失去回退与结转的唯一依据）。删除路径不经 [`revert`] 的在用
/// 占用守卫：在用 sell 已被级联消化，守卫谓词天然为空。
pub fn release_for_delete(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    consumed_by_convert_code: &str,
    consumed_by_convert_msg: &str,
) -> Result<Vec<String>> {
    match kind {
        TransactionKind::Sell => {
            reverse_sell(conn, id)?;
            Ok(Vec::new())
        }
        TransactionKind::Buy => {
            guard_no_convert_consumed(conn, id, consumed_by_convert_code, consumed_by_convert_msg)?;
            let cascaded_sell_ids = active_sell_ids_on_own_lots(conn, id)?;
            for sell_id in &cascaded_sell_ids {
                reverse_sell(conn, sell_id)?;
            }
            purge_lot_artifacts(conn, id)?;
            Ok(cascaded_sell_ids)
        }
        // 转换的删除（ADR-0099 决策 5）：转出腿逐批次精确回补 + 转入批次级联清理。
        // 回补先于清目标行（删 `security_transactions` 行会级联掉消耗记录）；
        // 转入批次被在用后续转换消耗时先拒绝（转换链从最后一腿往前处理）。
        TransactionKind::Convert => {
            guard_no_convert_consumed(conn, id, consumed_by_convert_code, consumed_by_convert_msg)?;
            let cascaded_sell_ids = active_sell_ids_on_own_lots(conn, id)?;
            for sell_id in &cascaded_sell_ids {
                reverse_sell(conn, sell_id)?;
            }
            reverse_convert_out_leg(conn, id)?;
            purge_lot_artifacts(conn, id)?;
            Ok(cascaded_sell_ids)
        }
        // 行为层仅对 buy/sell/convert 调用本函数；其余 kind 无持仓副作用，no-op
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Ok(Vec::new()),
    }
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
    pub(crate) lots: Vec<ActiveLot>,
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

/// 投资交易计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
pub enum Plan {
    Buy(BuyPlan),
    Sell(SellPlan),
    Convert(ConvertPlan),
}

impl Plan {
    /// 归一化交易行（供行为层经 `writer::NormalizedRow::try_from` 落库）。
    pub fn normalized(&self) -> &NormalizedTransaction {
        match self {
            Plan::Buy(p) => &p.normalized,
            Plan::Sell(p) => &p.normalized,
            Plan::Convert(p) => &p.normalized,
        }
    }
}

/// 校验并归一化一笔 buy/sell/convert 输入为 [`Plan`]（不落库、不产生副作用）。
///
/// 由行为层（`transaction`）在创建/修改路径按 kind 分派调用；
/// `kind` 为已解析的 [`TransactionKind`]，收到非 buy/sell/convert 的 kind 属编排错误，报错防误用。
pub fn prepare(conn: &Connection, kind: TransactionKind, input: &TransactionInput) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => Ok(Plan::Buy(prepare_buy(conn, input)?)),
        TransactionKind::Sell => Ok(Plan::Sell(prepare_sell(conn, input)?)),
        TransactionKind::Convert => Ok(Plan::Convert(prepare_convert(conn, input)?)),
        // 行为层穷尽分派保证仅转发 buy/sell/convert；其余 kind 属编排错误，显式拒绝防误用
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell/convert，收到: {kind}"
        ))),
    }
}

/// 应用计划的副作用（buy 建仓 / sell 卖出匹配 / convert 结转）。由编排层在交易行落库后调用，
/// 与行写入同处一个事务；`id` 为已落库的交易行 id。
pub fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Buy(p) => create_buy_lot(conn, id, p),
        Plan::Sell(p) => write_sell_side_effects(conn, id, p),
        Plan::Convert(p) => write_convert_side_effects(conn, id, p),
    }
}

/// 回退一笔已存在 buy/sell/convert 的副作用，供行为层**修改**编排入口在清理阶段调用。
///
/// - buy：转换链守卫（批次已被在用后续转换消耗则拒绝）+ 在用占用守卫
///   （在用 sell 的匹配消耗本买入批次则拒绝）+ 清理持仓/买入关联；
/// - sell：回补持仓扣减并清空卖出关联；
/// - convert：转换链守卫 + 在用卖出占用守卫（转入批次被后续卖出则拒绝）
///   + 转出腿逐批次精确回补 + 转入批次与转换明细行清理（复用 buy 清理语义）。
///
/// 删除路径不经本函数（其持仓语义由 [`release_for_delete`] 承载：sell 回补 /
/// buy 与 convert 级联，issue #940 / ADR-0097 / ADR-0099）。`guards` 为调用入口
/// 单点定义的错误码与措辞（ADR-0033 决策 #4）——本函数不自带措辞；
/// 非 buy/sell/convert 的 kind 无持仓副作用，防御性返回成功。
pub fn revert(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    guards: &GuardMessages<'_>,
) -> Result<()> {
    match kind {
        TransactionKind::Buy => {
            guard_no_convert_consumed(
                conn,
                id,
                guards.consumed_by_convert_code,
                guards.consumed_by_convert_msg,
            )?;
            cleanup_buy_side_effects(
                conn,
                id,
                guards.partially_sold_code,
                guards.partially_sold_msg,
            )
        }
        TransactionKind::Sell => reverse_sell(conn, id),
        // 转换：复用 buy 清理语义（转换链守卫 + 在用卖出占用守卫 + 转出腿精确回补
        // + 转入批次与明细行清理，ADR-0099 决策 5）。行为层在分派前已先拒
        // 「从 convert 出 / 改为 convert」的 kind 变更，此处只处理就地修改。
        TransactionKind::Convert => cleanup_convert_side_effects(conn, id, guards),
        // 行为层仅对 buy/sell/convert 调用本函数；其余 kind 无持仓副作用，no-op
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Ok(()),
    }
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
    let carried_cost_cents: i64 = plan.consumed.iter().map(|c| c.cost_cents).sum();
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

/// 回补一笔卖出交易曾扣减的持仓并清空其卖出关联：把每笔 `security_lot_sales` 的数量
/// 加回对应 lot，再清空该卖出的 `security_lot_sales` 与 `security_transactions` 记录。
fn reverse_sell(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_sales WHERE sell_transaction_id=?1")?;
    let sales: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in sales {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_sales WHERE sell_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='sell'",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 重放形态的计划重建（issue #861 / ADR-0091 决策 2/3）：从命令字段与随行
/// 归一化行重建 [`apply`] 所需计划，不重折算、不产出 op。
///
/// 与本地 [`prepare`] 的分工：归一化行已随命令携带（金额与本位币折算结果为
/// 落定值，重放不依赖本地汇率表），买入每份成本随命令携带、不重算（重算需
/// 读标的类型，属本地状态）；本函数只做「apply 依赖在位」校验与副作用计划装配：
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
            // FIFO 批次快照在本端重建：同序重放下与源端同状态 ⇒ 同一匹配结果
            //（排序键 rowid 的跨端确定性依据见 prepare_sell 同码查询处注释）。
            let lots: Vec<ActiveLot> =
                fifo_active_lots(conn, &row.account_id, &fields.instrument_id)?;
            let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
            if total_available < fields.quantity {
                let avail = total_available.to_string();
                let qty = fields.quantity.to_string();
                return Err(AppError::codedp(
                    "trade.insufficient-holding",
                    format!(
                        "可卖出数量不足，当前持有 {total_available}，尝试卖出 {}",
                        fields.quantity
                    ),
                    &[&avail, &qty],
                ));
            }
            Ok(Plan::Sell(SellPlan {
                normalized: row.clone(),
                instrument_id: fields.instrument_id.clone(),
                quantity: fields.quantity,
                price_cents: fields.price_cents,
                fee_cents: fields.fee_cents,
                lots,
                gross_proceeds_cents,
            }))
        }
        // 行为层重放入口穷尽分派保证仅转发 buy/sell；其余 kind 属编排错误，
        // 显式拒绝防误用（不引入 panic 构造，ADR-0060）。
        // 转换的同步命令字段（ConvertCommandFields）随同步票（issue #980）落地；
        // 在此之前重放端不落转换副作用（防半套数据），由行为层重放入口先行拒绝。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split
        | TransactionKind::Convert => Err(AppError::Invalid(format!(
            "投资层重放仅处理 buy/sell，收到: {kind}"
        ))),
    }
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
