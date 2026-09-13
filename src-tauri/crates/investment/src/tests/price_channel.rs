//! 价格通道（PriceChannel）判定测试（issue #1060）：后端派生事实的分区矩阵
//! 与读路径接线——「有价格写入通道」的口径单点（行情 / 净值 / 手动报价 /
//! 无来源），此前由前端两处镜像谓词（走势场内白名单、录价入口）各自推断，
//! 已漂移成第二口径，自本测试钉住的单点派生收口。

use crate::{
    InstrumentListFilter, InstrumentType, PriceChannel, derive_price_channel, get_instrument,
    list_instruments,
};

use super::common::insert_instrument_with_market;
use tauri_app_lib::test_support::open;

/// 行情通道：股票与场内 ETF（有市场 + 代码即可构造 secid，issue #695）。
#[test]
fn stock_and_listed_etf_with_known_market_sit_in_quote_channel() {
    for market in ["sh", "sz", "hk", "nasdaq", "nyse", "amex"] {
        assert_eq!(
            derive_price_channel(InstrumentType::Stock, market, "600000"),
            PriceChannel::Quote,
            "stock/{market} 应属行情通道"
        );
        assert_eq!(
            derive_price_channel(InstrumentType::Etf, market, "510300"),
            PriceChannel::Quote,
            "etf/{market} 应属行情通道"
        );
    }
}

/// 市场未知的股票类标的：既不进行情分区（无法构造 secid）、也不开录价入口
/// （ADR-0036 决策 1：股票现价归同步）——真正没有价格来源的行（issue #1060）。
#[test]
fn stock_with_unknown_market_has_no_price_source() {
    assert_eq!(
        derive_price_channel(InstrumentType::Stock, "unknown", "600000"),
        PriceChannel::None
    );
}

/// 市场未知的 ETF（自建标的的合法类型白名单成员，ADR-0036）：行情不可达但
/// 录价入口开放——手动报价通道。
#[test]
fn etf_with_unknown_market_falls_into_manual_channel() {
    assert_eq!(
        derive_price_channel(InstrumentType::Etf, "unknown", "稳稳地幸福"),
        PriceChannel::Manual
    );
}

/// 场外基金净值通道（ADR-0038 决策 6）：6 位纯数字代码进净值通道；
/// 名称充代码的基金行（非 6 位）无净值来源、归手动报价通道。
#[test]
fn fund_channel_splits_by_six_digit_code() {
    assert_eq!(
        derive_price_channel(InstrumentType::Fund, "unknown", "000198"),
        PriceChannel::FundNav
    );
    assert_eq!(
        derive_price_channel(InstrumentType::Fund, "unknown", "稳稳地幸福"),
        PriceChannel::Manual
    );
}

/// 债券 / 其他类型：无行情通道，录价入口开放——手动报价通道（与市场无关）。
#[test]
fn bond_and_other_types_sit_in_manual_channel() {
    for kind in [InstrumentType::Bond, InstrumentType::Other] {
        assert_eq!(
            derive_price_channel(kind, "unknown", "019547"),
            PriceChannel::Manual,
            "{kind} 应属手动报价通道"
        );
        assert_eq!(
            derive_price_channel(kind, "sh", "019547"),
            PriceChannel::Manual,
            "{kind}/sh 仍属手动报价通道"
        );
    }
}

/// 读路径接线：标的行（列表与按 id 精确取同一投影）携带派生通道——前端据此
/// 放行走势与开放录价入口，不再自行按类型与市场推断（issue #1060）。
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

    // 按 id 精确取（focus 参数落点）同一形状
    assert_eq!(
        get_instrument(&conn, "inst-fund").unwrap().price_channel,
        PriceChannel::FundNav
    );
    assert_eq!(
        get_instrument(&conn, "inst-none").unwrap().price_channel,
        PriceChannel::None
    );
}
