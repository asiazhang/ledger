use crate::{
    InstrumentListFilter, InstrumentType, Market, PriceChannel, QuoteMarket, derive_price_channel,
    derive_quote_market, list_instruments,
};

use super::common::{insert_instrument_with_market, instrument_by_id};
use tauri_app_lib::test_support::open;

#[test]
fn stock_and_listed_etf_with_known_market_sit_in_quote_channel() {
    for quote_market in QuoteMarket::ALL {
        let market = quote_market.as_market();
        assert_eq!(market.as_quote_market(), Some(quote_market));
        assert_eq!(Market::Unknown.as_quote_market(), None);
        assert_eq!(
            derive_price_channel(InstrumentType::Stock, market, "600000", None),
            PriceChannel::Quote,
            "stock/{market} 应属行情通道"
        );
        assert_eq!(
            derive_price_channel(InstrumentType::Etf, market, "510300", None),
            PriceChannel::Quote,
            "etf/{market} 应属行情通道"
        );
        assert_eq!(
            derive_quote_market(InstrumentType::Stock, market, "600000", None),
            Some(quote_market)
        );
        assert_eq!(
            derive_quote_market(InstrumentType::Stock, Market::Unknown, "600000", None),
            None
        );
    }
}

#[test]
fn stock_with_unknown_market_has_no_price_source() {
    assert_eq!(
        derive_price_channel(InstrumentType::Stock, Market::Unknown, "600000", None),
        PriceChannel::None
    );
}

#[test]
fn etf_with_unknown_market_falls_into_manual_channel() {
    assert_eq!(
        derive_price_channel(InstrumentType::Etf, Market::Unknown, "稳稳地幸福", None),
        PriceChannel::Manual
    );
}

#[test]
fn fund_channel_splits_by_six_digit_code() {
    assert_eq!(
        derive_price_channel(InstrumentType::Fund, Market::Unknown, "000198", None),
        PriceChannel::FundNav
    );
    assert_eq!(
        derive_price_channel(InstrumentType::Fund, Market::Unknown, "稳稳地幸福", None),
        PriceChannel::Manual
    );
}

#[test]
fn bond_and_other_types_sit_in_manual_channel() {
    for kind in [InstrumentType::Bond, InstrumentType::Other] {
        assert_eq!(
            derive_price_channel(kind, Market::Unknown, "019547", None),
            PriceChannel::Manual,
            "{kind} 应属手动报价通道"
        );
        assert_eq!(
            derive_price_channel(kind, Market::Sh, "019547", None),
            PriceChannel::Manual,
            "{kind}/sh 仍属手动报价通道"
        );
    }
}

#[test]
fn instrument_rows_carry_derived_price_channel() {
    let conn = open();
    insert_instrument_with_market(
        &conn,
        "inst-quote",
        "600000",
        "浦发银行",
        "CNY",
        "sh",
        "stock",
    );
    insert_instrument_with_market(
        &conn,
        "inst-fund",
        "000198",
        "天弘基金",
        "CNY",
        "unknown",
        "fund",
    );
    insert_instrument_with_market(
        &conn,
        "inst-manual",
        "稳稳地幸福",
        "且慢组合",
        "CNY",
        "unknown",
        "other",
    );
    insert_instrument_with_market(
        &conn,
        "inst-none",
        "ghost1",
        "幽灵股票",
        "CNY",
        "unknown",
        "stock",
    );

    let rows = list_instruments(&conn, &InstrumentListFilter::default()).unwrap();
    let channel_of = |id: &str| {
        rows.items
            .iter()
            .find(|i| i.id == id)
            .unwrap()
            .price_channel
    };
    assert_eq!(channel_of("inst-quote"), PriceChannel::Quote);
    assert_eq!(channel_of("inst-fund"), PriceChannel::FundNav);
    assert_eq!(channel_of("inst-manual"), PriceChannel::Manual);
    assert_eq!(channel_of("inst-none"), PriceChannel::None);

    assert_eq!(
        instrument_by_id(&conn, "inst-fund").price_channel,
        PriceChannel::FundNav
    );
    assert_eq!(
        instrument_by_id(&conn, "inst-none").price_channel,
        PriceChannel::None
    );
}
