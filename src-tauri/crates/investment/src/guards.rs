use ledger_accounts::AccountType;
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::TransactionKind;

#[derive(Clone, Copy)]
pub(crate) enum InstrumentUse {
    Buy,
    Sell,
    ConvertOut,
    ConvertIn,
    Split,
    Dividend,
}

pub(crate) fn require_instrument_type(
    instrument_type: Option<String>,
    instrument_id: &str,
    use_case: InstrumentUse,
) -> Result<String> {
    instrument_type.ok_or_else(|| {
        let (code, action) = match use_case {
            InstrumentUse::Buy => ("trade.buy-instrument-not-found", "买入"),
            InstrumentUse::Sell => ("trade.sell-instrument-not-found", "卖出"),
            InstrumentUse::ConvertOut => ("trade.convert-instrument-not-found", "转换转出"),
            InstrumentUse::ConvertIn => ("trade.convert-to-instrument-not-found", "转换转入"),
            InstrumentUse::Split => ("trade.split-instrument-not-found", "份额调整"),
            InstrumentUse::Dividend => ("trade.dividend-instrument-not-found", "分红"),
        };
        AppError::codedp(
            code,
            format!("{action}标的不存在: {instrument_id}"),
            &[instrument_id],
        )
    })
}

#[derive(Clone, Copy)]
pub(crate) enum InvestmentAccountUse {
    Buy,
    Sell,
    Convert,
    Split,
}

pub(crate) fn require_investment_account(
    account_type: AccountType,
    use_case: InvestmentAccountUse,
) -> Result<()> {
    if account_type == AccountType::Investment {
        return Ok(());
    }
    let (code, message) = match use_case {
        InvestmentAccountUse::Buy => (
            "trade.buy-account-not-investment",
            "买入交易必须使用投资账户",
        ),
        InvestmentAccountUse::Sell => (
            "trade.sell-account-not-investment",
            "卖出交易必须使用投资账户",
        ),
        InvestmentAccountUse::Convert => (
            "trade.convert-account-not-investment",
            "转换交易必须使用投资账户",
        ),
        InvestmentAccountUse::Split => (
            "trade.split-account-not-investment",
            "份额调整必须使用投资账户",
        ),
    };
    Err(AppError::coded(code, message))
}

pub(crate) fn reject_to_account(kind: TransactionKind, present: bool) -> Result<()> {
    if !present {
        return Ok(());
    }
    let (code, message) = match kind {
        TransactionKind::Buy => (
            "trade.buy-to-account-forbidden",
            "买入不能携带转入账户：资金流出归结算账户（出资账户或投资账户），账户间划转请使用转账",
        ),
        TransactionKind::Sell => (
            "trade.sell-to-account-forbidden",
            "卖出不能携带转入账户：资金流入归结算账户（出资账户或投资账户），账户间划转请使用转账",
        ),
        TransactionKind::Convert => (
            "trade.convert-to-account-forbidden",
            "转换不跨账户：转出与转入必须同属一个投资账户，不能携带转入账户",
        ),
        TransactionKind::Split => (
            "trade.split-to-account-forbidden",
            "份额调整不跨账户：只能调整同一投资账户内的标的，不能携带转入账户",
        ),
        TransactionKind::Dividend => (
            "trade.dividend-to-account-forbidden",
            "分红不跨账户，不能携带转入账户",
        ),
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => return Ok(()),
    };
    Err(AppError::coded(code, message))
}

#[derive(Clone, Copy)]
pub(crate) enum PositiveValue {
    BuyQuantity,
    SellQuantity,
    ConvertQuantity,
    ConvertToQuantity,
    ConvertOutAmount,
    ConvertInAmount,
    DividendAmount,
}

pub(crate) fn require_positive(value: PositiveValue, amount: f64) -> Result<()> {
    if amount <= 0.0 {
        let (code, message) = match value {
            PositiveValue::BuyQuantity => ("trade.buy-quantity-positive", "买入数量必须大于 0"),
            PositiveValue::SellQuantity => ("trade.sell-quantity-positive", "卖出数量必须大于 0"),
            PositiveValue::ConvertQuantity => {
                ("trade.convert-quantity-positive", "转换转出份额必须大于 0")
            }
            PositiveValue::ConvertToQuantity => (
                "trade.convert-to-quantity-positive",
                "转换转入份额必须大于 0",
            ),
            PositiveValue::ConvertOutAmount => (
                "trade.convert-out-amount-positive",
                "转换转出金额必须大于 0",
            ),
            PositiveValue::ConvertInAmount => {
                ("trade.convert-in-amount-positive", "转换转入金额必须大于 0")
            }
            PositiveValue::DividendAmount => {
                ("trade.dividend-amount-positive", "分红金额必须大于 0")
            }
        };
        return Err(AppError::coded(code, message));
    }
    Ok(())
}

pub(crate) fn require_positive_cents(value: PositiveValue, amount: i64) -> Result<()> {
    require_positive(value, amount as f64)
}

pub(crate) fn require_nonzero_split_quantity(amount: f64) -> Result<()> {
    if amount != 0.0 {
        Ok(())
    } else {
        Err(AppError::coded(
            "trade.split-quantity-zero",
            "份额调整数量不能为 0",
        ))
    }
}

pub(crate) fn reject_split_cash_leg(carried: bool) -> Result<()> {
    if carried {
        Err(AppError::coded(
            "trade.split-amount-forbidden",
            "份额调整无现金腿，金额必须为 0",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn reject_same_instrument(same: bool) -> Result<()> {
    if same {
        Err(AppError::coded(
            "trade.convert-same-instrument",
            "转换的转出标的与转入标的不能相同",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn require_split_holding(has_holding: bool) -> Result<()> {
    if has_holding {
        Ok(())
    } else {
        Err(AppError::coded(
            "trade.split-no-holding",
            "份额调整要求该标的有在用持仓（零持仓无从重述批次成本）",
        ))
    }
}

pub(crate) fn shrink_not_less_than_holding_error(
    total_holding: f64,
    shrink_quantity: f64,
) -> AppError {
    let holding_display = format_quantity_for_message(total_holding);
    let shrink_display = format_quantity_for_message(shrink_quantity);
    AppError::codedp(
        "trade.split-shrink-not-less-than-holding",
        format!("缩股幅度必须小于当前持仓，当前持有 {holding_display}，尝试缩股 {shrink_display}"),
        &[&holding_display, &shrink_display],
    )
}

pub(crate) fn require_currency_match(currency: &str, account_currency: &str) -> Result<()> {
    if currency == account_currency {
        return Ok(());
    }
    Err(AppError::codedp(
        "trade.dividend-currency-mismatch",
        format!("分红币种（{currency}）必须与到账账户币种（{account_currency}）一致"),
        &[currency, account_currency],
    ))
}

pub(crate) fn insufficient_holding_error(total_available: f64, quantity: f64) -> AppError {
    let available_display = format_quantity_for_message(total_available);
    let quantity_display = format_quantity_for_message(quantity);
    AppError::codedp(
        "trade.insufficient-holding",
        format!("可卖出数量不足，当前持有 {available_display}，尝试卖出 {quantity_display}"),
        &[&available_display, &quantity_display],
    )
}

pub(crate) fn format_quantity_for_message(quantity: f64) -> String {
    format!("{quantity:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coded_parts(err: AppError) -> (String, String, Vec<String>) {
        match err {
            AppError::Coded {
                code,
                message,
                params,
                ..
            } => (code, message, params),
            other => panic!("应为码化错误，实际: {other}"),
        }
    }

    #[test]
    fn instrument_existence_guards_pin_codes_and_full_messages() {
        let cases = [
            (
                InstrumentUse::Buy,
                "trade.buy-instrument-not-found",
                "买入标的不存在: inst-x",
            ),
            (
                InstrumentUse::Sell,
                "trade.sell-instrument-not-found",
                "卖出标的不存在: inst-x",
            ),
            (
                InstrumentUse::ConvertOut,
                "trade.convert-instrument-not-found",
                "转换转出标的不存在: inst-x",
            ),
            (
                InstrumentUse::ConvertIn,
                "trade.convert-to-instrument-not-found",
                "转换转入标的不存在: inst-x",
            ),
            (
                InstrumentUse::Split,
                "trade.split-instrument-not-found",
                "份额调整标的不存在: inst-x",
            ),
            (
                InstrumentUse::Dividend,
                "trade.dividend-instrument-not-found",
                "分红标的不存在: inst-x",
            ),
        ];
        for (use_case, code, message) in cases {
            let err = require_instrument_type(None, "inst-x", use_case).unwrap_err();
            let (actual_code, actual_message, params) = coded_parts(err);
            assert_eq!(actual_code, code);
            assert_eq!(actual_message, message);
            assert_eq!(params, ["inst-x"], "插值参数应携带标的 id: {code}");
        }
        assert!(require_instrument_type(Some("fund".into()), "inst-x", InstrumentUse::Buy).is_ok());
    }

    #[test]
    fn investment_account_guards_pin_codes_and_full_messages() {
        let cases = [
            (
                InvestmentAccountUse::Buy,
                "trade.buy-account-not-investment",
                "买入交易必须使用投资账户",
            ),
            (
                InvestmentAccountUse::Sell,
                "trade.sell-account-not-investment",
                "卖出交易必须使用投资账户",
            ),
            (
                InvestmentAccountUse::Convert,
                "trade.convert-account-not-investment",
                "转换交易必须使用投资账户",
            ),
            (
                InvestmentAccountUse::Split,
                "trade.split-account-not-investment",
                "份额调整必须使用投资账户",
            ),
        ];
        for (use_case, code, message) in cases {
            let err = require_investment_account(AccountType::Cash, use_case).unwrap_err();
            let (actual_code, actual_message, params) = coded_parts(err);
            assert_eq!(actual_code, code);
            assert_eq!(actual_message, message);
            assert!(params.is_empty(), "无插值守卫不应携带参数: {code}");
        }
        assert!(
            require_investment_account(AccountType::Investment, InvestmentAccountUse::Buy).is_ok()
        );
    }

    #[test]
    fn to_account_forbidden_guards_pin_codes_and_full_messages() {
        let cases = [
            (
                TransactionKind::Buy,
                "trade.buy-to-account-forbidden",
                "买入不能携带转入账户：资金流出归结算账户（出资账户或投资账户），账户间划转请使用转账",
            ),
            (
                TransactionKind::Sell,
                "trade.sell-to-account-forbidden",
                "卖出不能携带转入账户：资金流入归结算账户（出资账户或投资账户），账户间划转请使用转账",
            ),
            (
                TransactionKind::Convert,
                "trade.convert-to-account-forbidden",
                "转换不跨账户：转出与转入必须同属一个投资账户，不能携带转入账户",
            ),
            (
                TransactionKind::Split,
                "trade.split-to-account-forbidden",
                "份额调整不跨账户：只能调整同一投资账户内的标的，不能携带转入账户",
            ),
            (
                TransactionKind::Dividend,
                "trade.dividend-to-account-forbidden",
                "分红不跨账户，不能携带转入账户",
            ),
        ];
        for (kind, code, message) in cases {
            let err = reject_to_account(kind, true).unwrap_err();
            let (actual_code, actual_message, params) = coded_parts(err);
            assert_eq!(actual_code, code);
            assert_eq!(actual_message, message);
            assert!(params.is_empty(), "无插值守卫不应携带参数: {code}");
            assert!(reject_to_account(kind, false).is_ok());
        }
        for kind in [
            TransactionKind::Income,
            TransactionKind::Expense,
            TransactionKind::Transfer,
            TransactionKind::Refund,
        ] {
            assert!(reject_to_account(kind, true).is_ok());
        }
    }

    #[test]
    fn positive_value_guards_pin_codes_and_full_messages() {
        let cases = [
            (
                PositiveValue::BuyQuantity,
                "trade.buy-quantity-positive",
                "买入数量必须大于 0",
            ),
            (
                PositiveValue::SellQuantity,
                "trade.sell-quantity-positive",
                "卖出数量必须大于 0",
            ),
            (
                PositiveValue::ConvertQuantity,
                "trade.convert-quantity-positive",
                "转换转出份额必须大于 0",
            ),
            (
                PositiveValue::ConvertToQuantity,
                "trade.convert-to-quantity-positive",
                "转换转入份额必须大于 0",
            ),
            (
                PositiveValue::ConvertOutAmount,
                "trade.convert-out-amount-positive",
                "转换转出金额必须大于 0",
            ),
            (
                PositiveValue::ConvertInAmount,
                "trade.convert-in-amount-positive",
                "转换转入金额必须大于 0",
            ),
            (
                PositiveValue::DividendAmount,
                "trade.dividend-amount-positive",
                "分红金额必须大于 0",
            ),
        ];
        for (value, code, message) in cases {
            let err = require_positive(value, 0.0).unwrap_err();
            let (actual_code, actual_message, params) = coded_parts(err);
            assert_eq!(actual_code, code);
            assert_eq!(actual_message, message);
            assert!(params.is_empty(), "无插值守卫不应携带参数: {code}");
            assert!(require_positive(value, -1.0).is_err());
            assert!(require_positive(value, 1.0).is_ok());
            assert_eq!(
                require_positive_cents(value, 0).unwrap_err().code(),
                Some(code)
            );
        }
    }

    #[test]
    fn split_semantic_guards_pin_codes_and_full_messages() {
        let (code, message, params) = coded_parts(require_nonzero_split_quantity(0.0).unwrap_err());
        assert_eq!(code, "trade.split-quantity-zero");
        assert_eq!(message, "份额调整数量不能为 0");
        assert!(params.is_empty());
        assert!(require_nonzero_split_quantity(1.0).is_ok());

        let (code, message, params) = coded_parts(reject_split_cash_leg(true).unwrap_err());
        assert_eq!(code, "trade.split-amount-forbidden");
        assert_eq!(message, "份额调整无现金腿，金额必须为 0");
        assert!(params.is_empty());
        assert!(reject_split_cash_leg(false).is_ok());

        let (code, message, params) = coded_parts(reject_same_instrument(true).unwrap_err());
        assert_eq!(code, "trade.convert-same-instrument");
        assert_eq!(message, "转换的转出标的与转入标的不能相同");
        assert!(params.is_empty());
        assert!(reject_same_instrument(false).is_ok());

        let (code, message, params) = coded_parts(require_split_holding(false).unwrap_err());
        assert_eq!(code, "trade.split-no-holding");
        assert_eq!(
            message,
            "份额调整要求该标的有在用持仓（零持仓无从重述批次成本）"
        );
        assert!(params.is_empty());
        assert!(require_split_holding(true).is_ok());

        let (code, message, params) = coded_parts(shrink_not_less_than_holding_error(100.0, 100.0));
        assert_eq!(code, "trade.split-shrink-not-less-than-holding");
        assert_eq!(
            message,
            "缩股幅度必须小于当前持仓，当前持有 100，尝试缩股 100"
        );
        assert_eq!(params, ["100", "100"]);
    }

    #[test]
    fn dividend_currency_guard_pins_code_and_full_message() {
        let (code, message, params) =
            coded_parts(require_currency_match("USD", "CNY").unwrap_err());
        assert_eq!(code, "trade.dividend-currency-mismatch");
        assert_eq!(message, "分红币种（USD）必须与到账账户币种（CNY）一致");
        assert_eq!(params, ["USD", "CNY"]);
        assert!(require_currency_match("CNY", "CNY").is_ok());
    }

    #[test]
    fn insufficient_holding_error_pins_code_and_full_message() {
        let (code, message, params) = coded_parts(insufficient_holding_error(100.0, 200.0));
        assert_eq!(code, "trade.insufficient-holding");
        assert_eq!(message, "可卖出数量不足，当前持有 100，尝试卖出 200");
        assert_eq!(params, ["100", "200"]);

        let (code, message, params) =
            coded_parts(insufficient_holding_error(99.99999999999999, 200.0));
        assert_eq!(code, "trade.insufficient-holding");
        assert_eq!(message, "可卖出数量不足，当前持有 100，尝试卖出 200");
        assert_eq!(params, ["100", "200"]);
        assert!(!message.contains("99.9"), "不应出现位噪声: {message}");
    }
}
