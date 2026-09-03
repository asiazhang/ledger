use rusqlite::{Connection, OptionalExtension};

use super::prices::PRICE_UNITS_PER_FEN;
use crate::accounts::AccountType;
use crate::db::query::{FromRow, query_all, query_one};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{NormalizedTransaction, TransactionInput, TransactionTrade};
use crate::transaction::amount;
use crate::transaction::amount::TransactionKind;

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

/// 投资交易对外出口（issue #72 / spec #69）：只暴露 `prepare / apply / revert` 三件套。
///
/// - [`prepare`]：校验并归一化一笔 buy/sell 输入（不落库、不产生副作用），产出 [`Plan`]；
/// - [`apply`]：应用计划的副作用（buy 建仓 / sell 卖出匹配），由编排层在行落库后调用；
/// - [`revert`]：回退一笔已存在 buy/sell 交易的副作用（buy 守卫+清理 / sell 回补），
///   供删除/修改前清理。
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
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::coded(
            "trade.buy-account-not-investment",
            "买入交易必须使用投资账户",
        ));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
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
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            rusqlite::params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::coded(
            "trade.sell-account-not-investment",
            "卖出交易必须使用投资账户",
        ));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    // 本位币金额经 Amount 接缝折算到全局默认币种（issue #70）：不再硬编码 1:1，
    // 与通用 kind / 定时引擎共用同一折算路径（convert_to_native，基准为默认币种）。
    let amount_native_cents = amount::convert_to_native(conn, amount_cents, &account_currency)?;

    let lots: Vec<ActiveLot> = query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY created_at ASC, id ASC",
        rusqlite::params![input.account_id, instrument_id],
    )?;
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

    let mut remaining_to_sell = plan.quantity;
    // 匹配记录附带「是否耗尽该批次」标记：耗尽匹配的成本按买入行权威金额闭合（见下）。
    struct MatchedLot {
        lot: ActiveLot,
        exhausts_lot: bool,
    }
    let mut matched_lots: Vec<MatchedLot> = Vec::new();
    for lot in &plan.lots {
        if remaining_to_sell <= 0.0 {
            break;
        }
        let matched = lot.remaining_quantity.min(remaining_to_sell);
        let exhausts_lot = remaining_to_sell >= lot.remaining_quantity;
        matched_lots.push(MatchedLot {
            lot: ActiveLot {
                id: lot.id.clone(),
                remaining_quantity: matched,
                cost_per_unit_cents: lot.cost_per_unit_cents,
                currency_code: lot.currency_code.clone(),
            },
            exhausts_lot,
        });
        remaining_to_sell -= matched;
    }

    // 分摊双闭合（issue #302）：①收入按匹配末位吸收余数，Σ 匹配收入 = 毛收入
    // （基金 = 权威金额 + 手续费）精确到分；②耗尽批次的匹配把批次总成本闭合到
    // 买入行权威金额（减去该批次此前各次匹配的 round 重建成本）——两者合起来
    // 钉死舍入不变式：全平仓后 Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额，精确到分。
    let match_count = matched_lots.len();
    let mut allocated_fee_total = 0i64;
    let mut allocated_proceeds_total = 0i64;
    for (i, matched) in matched_lots.iter().enumerate() {
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
        // 匹配成本：耗尽批次 → 买入行权威金额 − 该批次此前匹配成本之和（闭合）；
        // 否则 round(匹配数量 × 每份成本 ÷ 换算因子)。
        let lot_cost = if matched.exhausts_lot {
            let lot_total_cents: i64 = conn.query_row(
                "SELECT t.amount_cents FROM security_lots l \n                 JOIN transactions t ON t.id = l.buy_transaction_id WHERE l.id=?1",
                rusqlite::params![lot.id],
                |r| r.get(0),
            )?;
            let prior_cost: i64 = conn
                .prepare("SELECT quantity FROM security_lot_sales WHERE lot_id=?1")?
                .query_map(rusqlite::params![lot.id], |r| r.get::<_, f64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
                .iter()
                .map(|q| (q * lot.cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64)
                .sum();
            lot_total_cents - prior_cost
        } else {
            (lot.remaining_quantity * lot.cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round()
                as i64
        };
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
            rusqlite::params![lot.remaining_quantity, now, device_id(), lot.id],
        )?;
    }

    Ok(())
}

/// 清理一笔买入交易的持仓关联（行为层删除/修改编排入口共用的守卫 + 清理）。
///
/// 若该买入已有部分卖出（`remaining_quantity < initial_quantity`）则拒绝清理——避免破坏
/// 对应卖出的已实现盈亏。`partially_sold_msg` 为调用入口单点定义的措辞
/// （见 `transaction::behavior` 的入口文案常量，ADR-0033 决策 #4）。
fn cleanup_buy_side_effects(
    conn: &Connection,
    id: &str,
    partially_sold_code: &str,
    partially_sold_msg: &str,
) -> Result<()> {
    let partially_sold: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_lots \
         WHERE buy_transaction_id=?1 AND remaining_quantity < initial_quantity",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if partially_sold > 0 {
        return Err(AppError::coded(partially_sold_code, partially_sold_msg));
    }
    conn.execute(
        "DELETE FROM security_lots WHERE buy_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='buy'",
        rusqlite::params![id],
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
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in sales {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(), lot_id],
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

/// 投资交易计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
pub enum Plan {
    Buy(BuyPlan),
    Sell(SellPlan),
}

impl Plan {
    /// 归一化交易行（供行为层经 `writer::NormalizedRow::try_from` 落库）。
    pub fn normalized(&self) -> &NormalizedTransaction {
        match self {
            Plan::Buy(p) => &p.normalized,
            Plan::Sell(p) => &p.normalized,
        }
    }
}

/// 校验并归一化一笔 buy/sell 输入为 [`Plan`]（不落库、不产生副作用）。
///
/// 由行为层（`transaction`）在创建/修改路径按 kind 分派调用；
/// `kind` 为已解析的 [`TransactionKind`]，收到非 buy/sell 的 kind 属编排错误，报错防误用。
pub fn prepare(conn: &Connection, kind: TransactionKind, input: &TransactionInput) -> Result<Plan> {
    match kind {
        TransactionKind::Buy => Ok(Plan::Buy(prepare_buy(conn, input)?)),
        TransactionKind::Sell => Ok(Plan::Sell(prepare_sell(conn, input)?)),
        // 行为层穷尽分派保证仅转发 buy/sell；其余 kind 属编排错误，显式拒绝防误用
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell，收到: {kind}"
        ))),
    }
}

/// 应用计划的副作用（buy 建仓 / sell 卖出匹配）。由编排层在交易行落库后调用，
/// 与行写入同处一个事务；`id` 为已落库的交易行 id。
pub fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Buy(p) => create_buy_lot(conn, id, p),
        Plan::Sell(p) => write_sell_side_effects(conn, id, p),
    }
}

/// 回退一笔已存在 buy/sell 交易的副作用，供行为层删除/修改编排入口在清理阶段调用。
///
/// - buy：守卫（已有部分卖出则拒绝）+ 清理持仓/买入关联；
/// - sell：回补持仓扣减并清空卖出关联。
///
/// `partial_sold_code` / `partial_sold_msg` 为 buy 守卫的错误码与措辞，由行为层各编排入口传入其单点定义的
/// 文案（修改/删除各持自己的措辞，ADR-0033 决策 #4）——本函数不自带措辞；
/// 非 buy/sell 的 kind 无持仓副作用，防御性返回成功。
pub fn revert(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    partial_sold_code: &str,
    partial_sold_msg: &str,
) -> Result<()> {
    match kind {
        TransactionKind::Buy => {
            cleanup_buy_side_effects(conn, id, partial_sold_code, partial_sold_msg)
        }
        TransactionKind::Sell => reverse_sell(conn, id),
        // 行为层仅对 buy/sell 调用本函数；其余 kind 无持仓副作用，no-op
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund
        | TransactionKind::Dividend
        | TransactionKind::Split => Ok(()),
    }
}

fn create_buy_lot(conn: &Connection, transaction_id: &str, plan: &BuyPlan) -> Result<()> {
    let lot_id = new_uuid();
    let now = now_iso();
    // 每份成本已在 prepare 按标的类型算定（基金锚定权威金额、其余锚定成交单价 +
    // 费用摊薄，均单次舍入），此处原样落批次——摊薄算法单一归属 prepare（issue #302）。
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![transaction_id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![lot_id, plan.normalized.account_id, plan.instrument_id, transaction_id, plan.quantity, plan.cost_per_unit_cents, plan.normalized.currency_code, now, 1, device_id()],
    )?;
    Ok(())
}
