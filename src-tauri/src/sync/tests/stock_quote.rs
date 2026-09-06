//! 东财股票单点行情报文解析、类型特征探测、价格换算与命中挑选（issue #693）：
//! fixture 驱动，不依赖真实网络。夹具字段值取自 stock/get 真实响应（2026-09
//! 实测截取，字段拼读保持不变）：f62 类型特征钉住沪深 ETF/LOF/股票已知样本
//!（场内基金类恒为 0），f59 精度位钉住 2 位/3 位两类缩放。

use crate::investment::InstrumentType;
use crate::sync::stock::{
    StockQuoteResponse, detect_kind_hint, pick_stock_quote, price_cents_from_raw,
    price_date_from_timestamp,
};

fn parse(raw: &str) -> StockQuoteResponse {
    serde_json::from_str(raw).expect("fixture 应为合法 JSON")
}

fn pick(raw: &str, market: &str, code: &str) -> Option<crate::investment::StockQuote> {
    pick_stock_quote(parse(raw), market, code)
}

/// 沪市股票真实形态：f43 按精度 2 位缩放、f62 非零、f86 时间戳。
const SH_STOCK: &str = r#"{
  "rc": 0, "rt": 4, "svr": 177622161, "lt": 1, "full": 1, "dlmkts": "8,10,128", "dsc": "0",
  "data": {"f43": 133000, "f57": "600519", "f58": "贵州茅台", "f59": 2, "f60": 129888, "f62": 2, "f86": 1788509493}
}"#;

/// 沪 ETF 真实形态：精度 3 位、f62 = 0（场内基金类特征）。
const SH_ETF: &str = r#"{
  "rc": 0, "data": {"f43": 4616, "f57": "510300", "f58": "沪深300ETF华泰柏瑞", "f59": 3, "f62": 0, "f86": 1788509509}
}"#;

/// 深 ETF 真实形态。
const SZ_ETF: &str = r#"{
  "rc": 0, "data": {"f43": 3305, "f57": "159915", "f58": "创业板ETF易方达", "f59": 3, "f62": 0, "f86": 1788507267}
}"#;

/// 深 LOF 真实形态。
const SZ_LOF: &str = r#"{
  "rc": 0, "data": {"f43": 563, "f57": "161725", "f58": "白酒基金LOF", "f59": 3, "f62": 0, "f86": 1788507273}
}"#;

/// 沪 LOF 真实形态。
const SH_LOF: &str = r#"{
  "rc": 0, "data": {"f43": 1897, "f57": "501018", "f58": "南方原油LOF", "f59": 3, "f62": 0, "f86": 1788509499}
}"#;

/// 港股真实形态：精度 3 位、代码 5 位补零回显。
const HK_STOCK: &str = r#"{
  "rc": 0, "data": {"f43": 442800, "f57": "00700", "f58": "腾讯控股", "f59": 3, "f60": 433000, "f62": 2, "f86": 1788509281}
}"#;

// ---------------------------------------------------------------------------
// 命中投影：权威名称 / 精度换算万分之一元 / 价格日期 / 类型提示
// ---------------------------------------------------------------------------

#[test]
fn picks_sh_stock_with_scaled_price_and_date() {
    let q = pick(SH_STOCK, "sh", "600519").expect("应命中");
    assert_eq!(q.code, "600519");
    assert_eq!(q.name, "贵州茅台", "应返回东财权威名称");
    assert_eq!(q.market, "sh");
    assert_eq!(
        q.price_cents,
        Some(13_300_000),
        "f43=133000 精度 2 位 → 1330.00 元 → 万分之一元 13300000"
    );
    assert_eq!(
        q.price_date,
        Some("2026-09-04".to_string()),
        "f86 unix 秒应投影为北京日历日"
    );
    assert_eq!(q.kind_hint, InstrumentType::Stock, "f62 非零应为股票");
}

#[test]
fn exchange_traded_fund_samples_pin_zero_feature_and_3_digit_scale() {
    // 场内基金类已知样本（沪 ETF / 深 ETF / 深 LOF / 沪 LOF）：f62 恒为 0 → etf；
    // 精度 3 位：f43 × 10（≠ 股票的 ×100，精度位不可按市场粗粒度推断）。
    for (raw, market, code, name, cents) in [
        (SH_ETF, "sh", "510300", "沪深300ETF华泰柏瑞", 46_160),
        (SZ_ETF, "sz", "159915", "创业板ETF易方达", 33_050),
        (SZ_LOF, "sz", "161725", "白酒基金LOF", 5_630),
        (SH_LOF, "sh", "501018", "南方原油LOF", 18_970),
    ] {
        let q = pick(raw, market, code).unwrap_or_else(|| panic!("{code} 应命中"));
        assert_eq!(
            q.kind_hint,
            InstrumentType::Etf,
            "{code} f62=0 应探测为 etf"
        );
        assert_eq!(q.name, name);
        assert_eq!(q.price_cents, Some(cents), "{code} 应按精度 3 位换算");
        assert_eq!(q.market, market);
    }
}

#[test]
fn picks_hk_stock_with_5_digit_echo_and_scale() {
    let q = pick(HK_STOCK, "hk", "00700").expect("应命中");
    assert_eq!(q.code, "00700");
    assert_eq!(q.name, "腾讯控股");
    assert_eq!(q.market, "hk");
    assert_eq!(
        q.price_cents,
        Some(4_428_000),
        "f43=442800 精度 3 位 → 442.800 港元 → 万分之一元 4428000"
    );
    assert_eq!(q.kind_hint, InstrumentType::Stock, "港股股票 f62 非零");
}

// ---------------------------------------------------------------------------
// 未命中与缺省形态：data:null、回显不等、名称为空、停牌无价、无时间戳
// ---------------------------------------------------------------------------

#[test]
fn null_data_or_mismatched_echo_means_miss() {
    // secid 无效（如港股未补零 / 代码不存在）：data 为 null。
    let null_data = r#"{"rc": 100, "rt": 1, "data": null}"#;
    assert!(
        pick(null_data, "hk", "00700").is_none(),
        "data:null 应按查无此码"
    );

    // 回显代码与请求代码不等（防御错前缀返回其他标的）。
    let other =
        r#"{"rc": 0, "data": {"f43": 100000, "f57": "501018", "f58": "奇消23B", "f59": 3}}"#;
    assert!(
        pick(other, "sz", "000001").is_none(),
        "回显不等应按查无此码"
    );

    // 名称缺省/空白（接口异常形态）按未命中。
    let empty_name = r#"{"rc": 0, "data": {"f43": 100000, "f57": "600519", "f58": ""}}"#;
    assert!(pick(empty_name, "sh", "600519").is_none());
}

#[test]
fn suspended_quote_yields_null_price_and_missing_timestamp_yields_null_date() {
    // f43 为 "-"（停牌/无有效报价）→ 价格 None；f86 缺省 → 价格日期 None。
    let suspended = r#"{"rc": 0, "data": {"f43": "-", "f57": "600519", "f58": "贵州茅台", "f59": 2, "f62": 2}}"#;
    let q = pick(suspended, "sh", "600519").expect("停牌仍是命中行");
    assert_eq!(q.price_cents, None, "无有效报价应投影 null 价格");
    assert_eq!(q.price_date, None, "无时间戳应投影 null 日期");
    assert_eq!(q.kind_hint, InstrumentType::Stock);
}

// ---------------------------------------------------------------------------
// 单点探测与换算函数：注入钉样本（漂移时改访问层一处即可）
// ---------------------------------------------------------------------------

#[test]
fn kind_hint_detection_pins_known_samples() {
    assert_eq!(
        detect_kind_hint(Some(0.0)),
        InstrumentType::Etf,
        "场内基金类恒为 0"
    );
    assert_eq!(detect_kind_hint(Some(2.0)), InstrumentType::Stock);
    assert_eq!(
        detect_kind_hint(Some(-1.0)),
        InstrumentType::Stock,
        "非零即股票"
    );
    assert_eq!(
        detect_kind_hint(None),
        InstrumentType::Stock,
        "缺省按股票（行情命中默认 stock）"
    );
}

#[test]
fn price_scale_by_precision_pins_two_and_three_digit_samples() {
    assert_eq!(price_cents_from_raw(133_000.0, Some(2.0), "sh"), 13_300_000);
    assert_eq!(price_cents_from_raw(4_616.0, Some(3.0), "sh"), 46_160);
    assert_eq!(price_cents_from_raw(442_800.0, Some(3.0), "hk"), 4_428_000);
    // 精度缺省/越界回退按市场粗粒度（A 股 ×100、港股 ×10，与 f2_to_price 同口径）。
    assert_eq!(price_cents_from_raw(951.0, None, "sh"), 95_100);
    assert_eq!(price_cents_from_raw(475_200.0, None, "hk"), 4_752_000);
    assert_eq!(
        price_cents_from_raw(951.0, Some(9.0), "sh"),
        95_100,
        "越界精度走回退"
    );
}

#[test]
fn price_date_follows_beijing_calendar_day() {
    assert_eq!(
        price_date_from_timestamp(Some(1_780_756_800)),
        Some("2026-06-06".to_string()),
        "UTC 14:40 → 北京 22:40 同日"
    );
    assert_eq!(
        price_date_from_timestamp(Some(1_780_771_200)),
        Some("2026-06-07".to_string()),
        "UTC 18:40 → 北京次日 02:40（UTC+8 跨日边界）"
    );
    assert_eq!(price_date_from_timestamp(None), None);
    assert_eq!(
        price_date_from_timestamp(Some(0)),
        None,
        "无有效时间不投影日期"
    );
}
