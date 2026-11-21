use crate::market::{Market, QuoteMarket, StockRoute, derive_quote_currency};
use crate::stock::{ResolvedStockCode, explicit_quote_market, resolve_stock_code};
use ledger_infra::error::AppError;

fn resolved(route: StockRoute, code: &str) -> ResolvedStockCode {
    ResolvedStockCode {
        route,
        code: code.to_string(),
    }
}

#[test]
fn six_digit_codes_infer_sh_and_sz_by_leading_digit() {
    assert_eq!(
        resolve_stock_code(None, "600519").unwrap(),
        resolved(StockRoute::Sh, "600519"),
        "6 开头 6 位数字推断沪市"
    );
    assert_eq!(
        resolve_stock_code(None, "000001").unwrap(),
        resolved(StockRoute::Sz, "000001"),
        "0 开头 6 位数字推断深市"
    );
    assert_eq!(
        resolve_stock_code(None, "300750").unwrap(),
        resolved(StockRoute::Sz, "300750"),
        "3 开头 6 位数字推断深市（创业板）"
    );
}

#[test]
fn exchange_traded_fund_code_segments_infer_their_exchange() {
    assert_eq!(
        resolve_stock_code(None, "510300").unwrap(),
        resolved(StockRoute::Sh, "510300"),
        "5 开头（沪场内基金段）推断沪市"
    );
    assert_eq!(
        resolve_stock_code(None, "159915").unwrap(),
        resolved(StockRoute::Sz, "159915"),
        "1 开头（深场内基金段）推断深市"
    );
    assert_eq!(
        resolve_stock_code(None, "161725").unwrap(),
        resolved(StockRoute::Sz, "161725")
    );
}

#[test]
fn short_numeric_codes_infer_hk_with_zero_padding() {
    assert_eq!(
        resolve_stock_code(None, "700").unwrap(),
        resolved(StockRoute::Hk, "00700"),
        "3 位数字推断港股并左补零归一"
    );
    assert_eq!(
        resolve_stock_code(None, "00700").unwrap(),
        resolved(StockRoute::Hk, "00700"),
        "已 5 位数字的港股代码归一后不变"
    );
    assert_eq!(
        resolve_stock_code(None, "12345").unwrap(),
        resolved(StockRoute::Hk, "12345"),
        "5 位数字推断港股"
    );
}

#[test]
fn letter_ticker_resolves_to_single_us_aggregate_query() {
    assert_eq!(
        resolve_stock_code(None, "AAPL").unwrap(),
        resolved(StockRoute::Us, "AAPL"),
        "纯字母 ticker 单查询（聚合路由值 us，精确交易所由行情源自报；候选遍历退役）"
    );
    assert_eq!(
        resolve_stock_code(None, "aapl").unwrap(),
        resolved(StockRoute::Us, "AAPL"),
        "小写 ticker 应大写归一"
    );
}

#[test]
fn letter_ticker_with_explicit_us_market_resolves_to_same_aggregate_query() {
    for market in [QuoteMarket::Nasdaq, QuoteMarket::Nyse, QuoteMarket::Amex] {
        assert_eq!(
            resolve_stock_code(Some(market), "aapl").unwrap(),
            resolved(StockRoute::Us, "AAPL"),
            "显式美股市场 {market} 同解为聚合查询"
        );
    }
}

#[test]
fn explicit_market_consistent_with_shape_passes() {
    assert_eq!(
        resolve_stock_code(Some(QuoteMarket::Sh), "600519").unwrap(),
        resolved(StockRoute::Sh, "600519"),
        "显式 market 与形态一致时放行"
    );
    assert_eq!(
        resolve_stock_code(Some(QuoteMarket::Sz), "000001").unwrap(),
        resolved(StockRoute::Sz, "000001")
    );
    assert_eq!(
        resolve_stock_code(Some(QuoteMarket::Hk), "700").unwrap(),
        resolved(StockRoute::Hk, "00700"),
        "显式 hk 与短数字代码一致，仍左补零归一"
    );
}

#[test]
fn explicit_market_contradicting_shape_returns_coded_400() {
    for (market, code) in [
        (QuoteMarket::Sz, "600519"),
        (QuoteMarket::Sh, "000001"),
        (QuoteMarket::Sh, "300750"),
        (QuoteMarket::Sz, "510300"),
        (QuoteMarket::Sh, "159915"),
        (QuoteMarket::Sz, "00700"),
        (QuoteMarket::Hk, "600519"),
        (QuoteMarket::Sh, "AAPL"),
        (QuoteMarket::Nasdaq, "600519"),
        (QuoteMarket::Nyse, "00700"),
        (QuoteMarket::Sh, "1234567"),
    ] {
        let err = resolve_stock_code(Some(market), code).unwrap_err();
        assert!(
            err.is_code("stock.market-conflict"),
            "{market}/{code} 应报参数矛盾，实际: {err:?}"
        );
        match &err {
            AppError::Coded { params, .. } => {
                assert!(
                    params.contains(&code.to_string())
                        && params.contains(&market.as_str().to_string()),
                    "{market}/{code} 错误参数应含市场与代码: {params:?}"
                );
            }
            other => panic!("{market}/{code} 应为码化 400 错误，实际: {other:?}"),
        }
    }
}

#[test]
fn unsupported_market_outside_closed_set_returns_400() {
    for market in ["bse", "unknown", "us", "NasDaQ"] {
        let err = explicit_quote_market(market).unwrap_err();
        assert!(
            err.is_code("stock.market-unsupported"),
            "{market} 应报暂不支持，实际: {err:?}"
        );
        match &err {
            AppError::Coded {
                message, params, ..
            } => {
                assert!(
                    message.contains("暂不支持"),
                    "应中文说明暂不支持，实际: {message}"
                );
                assert!(
                    params.contains(&market.to_string()),
                    "错误参数应含市场: {params:?}"
                );
            }
            other => panic!("{market} 应为码化错误，实际: {other:?}"),
        }
    }
}

#[test]
fn beijing_exchange_codes_rejected_as_unsupported() {
    for code in ["430047", "830799", "871981"] {
        let err = resolve_stock_code(None, code).unwrap_err();
        assert!(
            err.is_code("stock.bse-unsupported"),
            "{code} 应报北交所暂不支持"
        );
        match &err {
            AppError::Coded {
                message, params, ..
            } => {
                assert!(
                    message.contains("北交所"),
                    "北交所错误信息应为中文明示，实际: {message}"
                );
                assert!(
                    params.contains(&code.to_string()),
                    "错误参数应含代码: {params:?}"
                );
            }
            other => panic!("{code} 应为码化错误，实际: {other:?}"),
        }
        assert!(resolve_stock_code(Some(QuoteMarket::Sh), code).is_err());
    }
}

#[test]
fn unresolvable_shapes_rejected_with_explicit_market_hint() {
    for code in ["1234A6", "BRK.B", "900001", "1234567", ""] {
        let err = resolve_stock_code(None, code).unwrap_err();
        assert!(
            err.is_code("stock.code-unresolvable"),
            "{code} 应报无法推断"
        );
        match &err {
            AppError::Coded { message, .. } => {
                assert!(
                    message.contains("market"),
                    "无法推断的错误应提示显式传 market，实际: {message}"
                );
            }
            other => panic!("{code} 应为码化错误，实际: {other:?}"),
        }
    }
}

#[test]
fn quote_currency_derives_from_market() {
    assert_eq!(derive_quote_currency(Market::Sh), "CNY");
    assert_eq!(derive_quote_currency(Market::Sz), "CNY");
    assert_eq!(derive_quote_currency(Market::Hk), "HKD");
    assert_eq!(derive_quote_currency(Market::Nasdaq), "USD");
    assert_eq!(derive_quote_currency(Market::Nyse), "USD");
    assert_eq!(derive_quote_currency(Market::Amex), "USD");
    assert_eq!(
        derive_quote_currency(Market::Unknown),
        "CNY",
        "未知市场缺省人民币"
    );
}
