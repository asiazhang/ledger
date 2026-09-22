//! 腾讯行情批量报价取数单元（ADR-0130 / issue #1558）：三套字段布局的类型码、币种与
//! 交易所后缀解析，价格 / 价格日期换算，非预期响应 fail-closed，请求形态与批量承载量。
//! 报文为取数面实测（2026-09-19 采集，行情日 2026-09-18）的真实报文（GBK 解码后截取），
//! 请求形态经本地 HTTP 服务验证，不依赖真实网络。

use std::time::Duration;

use ledger_investment::InstrumentType;

use crate::channels::QuoteQuery;
use crate::http::Pacer;
use crate::tencent::{
    TENCENT_QUOTE_BATCH_SIZE, TENCENT_QUOTE_HOSTS, TENCENT_QUOTE_PATH_PREFIX, detect_kind_hint,
    fetch_tencent_quotes, parse_tencent_quotes, price_cents_from_decimal,
    price_date_from_timestamp, tencent_query_key,
};
use crate::tests::spawn_header_capture_server;

// ---------------------------------------------------------------------------
// 真实报文 fixture（2026-09-19 采集、行情日 2026-09-18，GBK 解码后原样；下标注释为 0 基）
// ---------------------------------------------------------------------------

/// 沪市股票（88 字段，类型码 GP-A @61、币种 CNY @82） — 2026-09-18 实测报文截取。
pub(super) const SH_STOCK: &str = r#"v_sh600000="1~浦发银行~600000~9.07~9.06~9.05~517593~277236~240357~9.07~4467~9.06~11048~9.05~7788~9.04~5161~9.03~4586~9.08~2564~9.09~1316~9.10~2574~9.11~5680~9.12~6306~~20260918161458~0.01~0.11~9.15~9.00~9.07/517593/469969405~517593~46997~0.16~5.90~~9.15~9.00~1.66~3020.84~3020.84~0.40~9.97~8.15~0.74~14610~9.08~4.88~6.04~~~0.01~46996.9405~33.3776~368~   A~GP-A~-24.54~-2.05~4.63~6.14~0.50~13.11~8.07~-3.82~0.22~8.75~33305838300~33305838300~28.37~-20.51~33305838300~~~-26.44~-0.22~~CNY~0~___D__F__N~9.00~19524~";"#;

/// 沪市 ETF（88 字段，类型码 ETF @61） — 2026-09-18 实测报文截取。
pub(super) const SH_ETF: &str = r#"v_sh510300="1~沪深300ETF华泰柏瑞~510300~4.582~4.532~4.556~6297211~3436640~2860571~4.581~57097~4.580~23250~4.579~7791~4.578~3323~4.577~1104~4.582~8638~4.583~11641~4.584~6276~4.585~15727~4.586~4926~~20260918161452~0.050~1.10~4.596~4.548~4.582/6297211/2877462543~6297211~287746~2.66~~~4.596~4.548~1.06~1084.09~1084.09~0.00~4.985~4.079~0.84~45357~4.569~~~~~~287746.2543~160.8740~3511~   A~ETF~-1.04~0.07~~~~5.095~4.405~-0.74~-2.09~-6.62~23659687700~23659687700~32.45~-0.76~23659687700~0.06~4.5791~2.48~0.04~4.5793~CNY~0~___D__F__N~4.590~-14031~";"#;

/// 深市 LOF（88 字段，类型码 LOF @61） — 2026-09-18 实测报文截取。
pub(super) const SZ_LOF: &str = r#"v_sz161725="51~白酒基金LOF~161725~0.529~0.527~0.525~341318~176517~164801~0.529~16369~0.528~19713~0.527~1585~0.526~6890~0.525~11985~0.530~6205~0.531~19317~0.532~8785~0.533~5053~0.534~1940~~20260918161430~0.002~0.38~0.531~0.525~0.529/341318/18055543~341318~1806~0.99~~~0.531~0.525~1.14~18.17~18.17~0.00~0.580~0.474~1.00~15242~0.529~~~~~~1805.5543~0.0000~0~ ~LOF~-25.60~-0.75~~~~0.810~0.490~-6.04~-2.94~5.38~3434674618~3434674618~15.58~-27.34~3434674618~0.13~~-34.37~-0.19~0.5283~CNY~0~~0.536~-4816~";"#;

/// 沪市可转债（88 字段，类型码 ZQ-KZZ @61） — 2026-09-18 实测报文截取。
pub(super) const SH_BOND: &str = r#"v_sh113050="1~南银转债~113050~144.967~144.967~0.000~0~0~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~0.000~0~~20260918090000~0.000~0.00~0.000~0.000~144.967/0/0~0~0~~~D~0.000~0.000~0.00~~~0.00~-1~-1~0.00~0~0.000~~~~~~0.0000~0.0000~0~ ~ZQ-KZZ~0.00~0.00~~~~~~0.00~0.00~0.00~~~~0.00~~~~0.00~0.00~~CNY~0~~0.000~0~";"#;

/// 港股（78 字段，类型码 GP @63、币种 HKD @75） — 2026-09-18 实测报文截取。
pub(super) const HK_STOCK: &str = r#"v_hk00700="100~腾讯控股~00700~419.000~426.000~428.000~28796138.0~0~0~419.000~0~0~0~0~0~0~0~0~0~419.000~0~0~0~0~0~0~0~0~0~28796138.0~2026/09/18 16:08:32~-7.000~-1.64~430.400~419.000~419.000~28796138.0~12180786280.956~0~15.31~~0~0~2.68~38108.4103~38108.4103~TENCENT~1.27~677.700~411.000~1.85~-25.88~0~0~0~0~0~14.05~2.93~0.32~100~-29.43~-2.19~GP~20.41~11.00~-5.37~-8.32~-0.57~9095085993.00~9095085993.00~14.50~5.313~423.001~-29.78~HKD~1~50";"#;

/// 美股纳斯达克（73 字段，代码 AAPL.OQ、类型码 GP @56、币种 USD @35） — 2026-09-18 实测报文截取。
pub(super) const US_AAPL: &str = r#"v_usAAPL="200~苹果~AAPL.OQ~336.13~337.00~337.91~86588203~0~0~334.75~440~0~0~0~0~0~0~0~0~334.88~40~0~0~0~0~0~0~0~0~~2026-09-18 16:00:02~-0.87~-0.26~338.49~332.53~USD~86588203~29101577281~0.59~38.55~~45.06~~1.77~49024.92647~49055.41723~Apple Inc.~8.72~344.26~239.32~400~45.62~0.32~49055.41723~23.98~1.16~GP~148.75~36.08~2.41~7.98~14.79~14594180000~14585108878~2.23~36.64~1.06~336.09~~~~~";"#;

/// 美股纽交所（73 字段，代码 IBM.N） — 2026-09-18 实测报文截取。
pub(super) const US_IBM: &str = r#"v_usIBM="200~IBM~IBM.N~229.55~237.75~237.85~13413822~0~0~230.04~600~0~0~0~0~0~0~0~0~230.18~100~0~0~0~0~0~0~0~0~~2026-09-18 16:05:15~-8.20~-3.45~237.89~229.38~USD~13413822~3092330115~1.42~20.33~~20.55~~3.58~1947.73751~2162.66949~International Business Machines Corporation~11.29~330.10~197.78~500~6.28~2.94~2162.66949~-20.93~-5.65~GP~34.55~7.12~-2.20~-1.77~-12.08~942134390~848502508~2.35~32.33~6.74~230.53~~~~~";"#;

/// 美股美交所 ETF（73 字段，代码 SPY.AM、类型码 GP-ETF @56） — 2026-09-18 实测报文截取。
pub(super) const US_SPY: &str = r#"v_usSPY="200~标普500指数ETF-SPDR~SPY.AM~761.69~760.71~761.31~65395148~0~0~762.94~80~0~0~0~0~0~0~0~0~762.99~2720~0~0~0~0~0~0~0~0~~2026-09-18 16:00:01~0.98~0.13~762.00~757.97~USD~65395148~49730810811~~~~~~0.53~~~State Street Spdr S&P 500 Etf~~777.42~626.07~-2640~~~~12.57~-0.09~GP-ETF~~~-1.24~0.13~4.14~~~1.34~~~760.47~~~~1031882000~";"#;

fn parse_one(body: &str) -> Vec<crate::tencent::TencentQuote> {
    parse_tencent_quotes(body).expect("真实报文应可解析")
}

// ---------------------------------------------------------------------------
// 三套字段布局的类型码（ADR-0130 决策 3 / 取数面事实 §13.2）：同一探测单点按市场
// 取不同下标，不能按固定下标取。
// ---------------------------------------------------------------------------

#[test]
fn a_share_layout_pins_type_codes_and_fields() {
    // 沪市股票 / ETF / LOF / 可转债共用 A 股布局（88 字段）：类型码 @61、币种 @82。
    for (body, market, code, name, price_cents, security_type, kind) in [
        (
            SH_STOCK,
            "sh",
            "600000",
            "浦发银行",
            90_700,
            "GP-A",
            InstrumentType::Stock,
        ),
        (
            SH_ETF,
            "sh",
            "510300",
            "沪深300ETF华泰柏瑞",
            45_820,
            "ETF",
            InstrumentType::Etf,
        ),
        (
            SZ_LOF,
            "sz",
            "161725",
            "白酒基金LOF",
            5_290,
            "LOF",
            InstrumentType::Etf,
        ),
        (
            SH_BOND,
            "sh",
            "113050",
            "南银转债",
            1_449_670,
            "ZQ-KZZ",
            InstrumentType::Stock,
        ),
    ] {
        let quotes = parse_one(body);
        assert_eq!(quotes.len(), 1, "{code}");
        let quote = &quotes[0];
        assert_eq!(quote.code, code);
        assert_eq!(quote.name, name);
        assert_eq!(quote.price_cents, Some(price_cents), "{code} 价格换算");
        assert_eq!(quote.market, market, "{code} 精确市场");
        assert_eq!(quote.security_type, security_type, "{code} 类型码");
        assert_eq!(quote.kind_hint, kind, "{code} 类型提示");
        assert_eq!(quote.currency_code, "CNY", "{code} 币种");
        assert_eq!(
            quote.price_date.as_deref(),
            Some("2026-09-18"),
            "{code} 价格日期"
        );
    }
}

#[test]
fn hk_layout_pins_type_code_currency_and_date() {
    let quotes = parse_one(HK_STOCK);
    assert_eq!(quotes.len(), 1);
    let quote = &quotes[0];
    assert_eq!(quote.code, "00700");
    assert_eq!(quote.name, "腾讯控股");
    assert_eq!(quote.price_cents, Some(4_190_000), "419.000 港元");
    assert_eq!(quote.market, "hk");
    assert_eq!(quote.security_type, "GP", "港股类型码 @63");
    assert_eq!(quote.kind_hint, InstrumentType::Stock);
    assert_eq!(quote.currency_code, "HKD", "港股币种 @75");
    assert_eq!(quote.price_date.as_deref(), Some("2026-09-18"));
}

#[test]
fn us_layout_pins_exchange_suffix_and_currency() {
    // 美股三后缀与既有市场闭集三值一一对应（ADR-0081 决策 2）：.OQ/.N/.AM；
    // 类型码 @56、币种 @35。
    for (body, code, market, security_type, kind, price_cents) in [
        (
            US_AAPL,
            "AAPL",
            "nasdaq",
            "GP",
            InstrumentType::Stock,
            3_361_300,
        ),
        (
            US_IBM,
            "IBM",
            "nyse",
            "GP",
            InstrumentType::Stock,
            2_295_500,
        ),
        (
            US_SPY,
            "SPY",
            "amex",
            "GP-ETF",
            InstrumentType::Etf,
            7_616_900,
        ),
    ] {
        let quotes = parse_one(body);
        assert_eq!(quotes.len(), 1, "{code}");
        let quote = &quotes[0];
        assert_eq!(quote.code, code, "去交易所后缀的裸代码");
        assert_eq!(quote.market, market, ".{code} 后缀对应市场");
        assert_eq!(quote.security_type, security_type);
        assert_eq!(quote.kind_hint, kind);
        assert_eq!(quote.currency_code, "USD", "美股币种 @35");
        assert_eq!(quote.price_cents, Some(price_cents));
        assert_eq!(quote.price_date.as_deref(), Some("2026-09-18"));
    }
}

#[test]
fn detect_kind_hint_maps_fund_like_codes_to_etf() {
    for fund_like in ["ETF", "LOF", "GP-ETF"] {
        assert_eq!(
            detect_kind_hint(fund_like),
            InstrumentType::Etf,
            "{fund_like}"
        );
    }
    // 非基金特征一律 stock：可转债沿旧判据同走行情通道（ADR-0081 两值类型提示）。
    for stock_like in ["GP-A", "GP", "ZQ-KZZ"] {
        assert_eq!(
            detect_kind_hint(stock_like),
            InstrumentType::Stock,
            "{stock_like}"
        );
    }
}

#[test]
fn tencent_query_key_pins_market_prefixes() {
    // 「市场 + 代码」→ 腾讯查询键：沪深港用市场前缀，美股三市场统用 us（精确交易所
    // 由响应自报后缀判定）。
    assert_eq!(
        tencent_query_key("sh", "600000").as_deref(),
        Some("sh600000")
    );
    assert_eq!(
        tencent_query_key("sz", "161725").as_deref(),
        Some("sz161725")
    );
    assert_eq!(tencent_query_key("hk", "00700").as_deref(), Some("hk00700"));
    for us_market in ["nasdaq", "nyse", "amex"] {
        assert_eq!(
            tencent_query_key(us_market, "AAPL").as_deref(),
            Some("usAAPL"),
            "{us_market}"
        );
    }
    // 市场未知不构造键（调用侧跳过该查询单元）。
    assert_eq!(tencent_query_key("unknown", "NVDA"), None);
    assert_eq!(tencent_query_key("", "NVDA"), None);
}

// ---------------------------------------------------------------------------
// 价格换算与行情日期（ADR-0130 决策 5：取交易所当地交易日，不做时区换算）
// ---------------------------------------------------------------------------

#[test]
fn price_cents_from_decimal_pins_scales_and_rejects_invalid() {
    assert_eq!(price_cents_from_decimal("9.07"), Some(90_700));
    assert_eq!(price_cents_from_decimal("4.582"), Some(45_820));
    assert_eq!(price_cents_from_decimal("144.967"), Some(1_449_670));
    assert_eq!(price_cents_from_decimal("419.000"), Some(4_190_000));
    assert_eq!(price_cents_from_decimal("12"), Some(120_000));
    // 停牌 / 无效价（≤0）与非数值不投影价格。
    for invalid in ["0.000", "0", "-1.5", "", "  ", "n/a", "--"] {
        assert_eq!(price_cents_from_decimal(invalid), None, "{invalid:?}");
    }
}

#[test]
fn price_date_takes_local_trading_day_without_timezone_shift() {
    // 三套布局的时间戳形态不同，统一取当地交易日的日期部分。
    assert_eq!(
        price_date_from_timestamp("20260918161458").as_deref(),
        Some("2026-09-18")
    );
    assert_eq!(
        price_date_from_timestamp("2026/09/18 16:08:32").as_deref(),
        Some("2026-09-18")
    );
    // 2026-09-18 是周五；美股收盘 16:00 ET（= 北京次日 04:00）取当地日仍是周五，
    // 不得按北京时间切分记成周六。
    assert_eq!(
        price_date_from_timestamp("2026-09-18 16:00:02").as_deref(),
        Some("2026-09-18")
    );
    assert_eq!(price_date_from_timestamp(""), None);
    assert_eq!(price_date_from_timestamp("not a date"), None);
}

// ---------------------------------------------------------------------------
// fail-closed：被拦截 / 非预期形状不退化为「无数据」
// ---------------------------------------------------------------------------

#[test]
fn intercepted_or_unexpected_response_fails_closed() {
    for body in [
        "",
        "   \n\t",
        "<html><body>risk control</body></html>",
        "{\"error\":\"blocked\"}",
        "访问过于频繁，请稍后再试",
    ] {
        assert!(
            parse_tencent_quotes(body).is_err(),
            "应 fail-closed：{body:?}"
        );
    }
    // 数据源明示「批量内全部代码无效」是可信零命中，不是错误。
    assert!(parse_one(r#"v_pv_none_match="1";"#).is_empty());
}

#[test]
fn drifted_layout_or_unknown_market_fails_closed() {
    // 字段数少于该市场布局下界（A 股 85 < 88）：不静默当「无数据」，报错。
    let short_a = format!(r#"v_sh600000="{}";"#, vec!["0"; 85].join("~"));
    assert!(parse_tencent_quotes(&short_a).is_err(), "A 股布局下界");
    let short_hk = format!(r#"v_hk00700="{}";"#, vec!["0"; 70].join("~"));
    assert!(parse_tencent_quotes(&short_hk).is_err(), "港股布局下界");
    let short_us = format!(r#"v_usAAPL="{}";"#, vec!["0"; 60].join("~"));
    assert!(parse_tencent_quotes(&short_us).is_err(), "美股布局下界");

    // 未知市场前缀：非预期响应。
    assert!(parse_tencent_quotes(r#"v_bj430047="1~x~430047";"#).is_err());
    // 未知 / 缺失美股交易所后缀：不猜市场，报错。
    assert!(parse_tencent_quotes(&US_IBM.replace("IBM.N", "IBM.XX")).is_err());
    assert!(parse_tencent_quotes(&US_IBM.replace("IBM.N", "IBM")).is_err());
}

#[test]
fn batch_response_parses_all_statements_and_projects_to_payload() {
    let body = format!("{SH_STOCK}\n{US_AAPL}");
    let quotes = parse_one(&body);
    assert_eq!(quotes.len(), 2);
    assert_eq!(quotes[1].code, "AAPL", "同一响应多条语句按序解析");

    let quote = quotes.into_iter().nth(1).expect("两").into_quote();
    assert_eq!(quote.code, "AAPL");
    assert_eq!(quote.market.as_deref(), Some("nasdaq"));
    assert_eq!(quote.stock_kind_hint(), InstrumentType::Stock);
    assert_eq!(quote.price_cents, Some(3_361_300));
    assert_eq!(quote.fund_class, None);
    assert_eq!(quote.nav_date, None);
}

// ---------------------------------------------------------------------------
// 请求形态与批量承载量（本地 HTTP 服务；实测 ~900 只 / ~8KB 请求行）
// ---------------------------------------------------------------------------

#[test]
fn fetch_pins_request_shape_and_batch_capacity() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let gbk = encoding_rs::GBK.encode(SH_STOCK).0.into_owned();

    // 小批量四只（沪深港美各一）：一次请求，请求目标是路径段 `q=<逗号串>`（端点形态，
    // 非查询参数），未携带 Referer。
    let (url, requests) = spawn_header_capture_server(gbk.clone());
    let queries = vec![
        QuoteQuery {
            market: "sh".into(),
            code: "600000".into(),
        },
        QuoteQuery {
            market: "sz".into(),
            code: "161725".into(),
        },
        QuoteQuery {
            market: "hk".into(),
            code: "00700".into(),
        },
        QuoteQuery {
            market: "nasdaq".into(),
            code: "AAPL".into(),
        },
    ];
    let quotes = tauri::async_runtime::block_on(fetch_tencent_quotes(
        &client,
        &mut pacer,
        &[url.as_str()],
        &queries,
    ))
    .expect("假响应应解析成功");
    assert_eq!(quotes.len(), 1, "假响应只含 sh600000 一条");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "一批一次请求");
    assert_eq!(
        request_target(&captured[0]),
        "/q=sh600000,sz161725,hk00700,usAAPL",
        "请求形态：路径段 q=<市场前缀代码逗号串>"
    );
    assert!(
        !captured[0].to_ascii_lowercase().contains("referer"),
        "腾讯端点无需 Referer"
    );

    // 批量承载量：超过单请求承载量的查询分多次请求，单请求键数不超过常量。
    let many: Vec<QuoteQuery> = (0..TENCENT_QUOTE_BATCH_SIZE + 1)
        .map(|i| QuoteQuery {
            market: "sh".into(),
            code: format!("{i:06}"),
        })
        .collect();
    let (url, requests) = spawn_header_capture_server(gbk);
    let quotes = tauri::async_runtime::block_on(fetch_tencent_quotes(
        &client,
        &mut pacer,
        &[url.as_str()],
        &many,
    ))
    .expect("分批请求应成功");
    assert_eq!(quotes.len(), 2, "两次请求各回一条 sh600000");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 2, "超过承载量应分两次请求");
    assert_eq!(
        key_count(request_target(&captured[0])),
        TENCENT_QUOTE_BATCH_SIZE
    );
    assert_eq!(key_count(request_target(&captured[1])), 1);
    // 承载量的最坏请求行（每键 ≤8 字符 + 1 个逗号）须低于实测的 ~8KB 上限。
    let worst_request_line = TENCENT_QUOTE_PATH_PREFIX.len() + TENCENT_QUOTE_BATCH_SIZE * 9;
    assert!(
        worst_request_line < 8192,
        "承载量最坏请求行 {worst_request_line} 应低于 8KB 上限"
    );
}

#[test]
fn fetch_decodes_gbk_and_fails_closed_on_intercepted_response() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let queries = vec![QuoteQuery {
        market: "sh".into(),
        code: "600000".into(),
    }];

    // GBK 报文正确解码出中文权威名称（未按 UTF-8 误解）。
    let gbk = encoding_rs::GBK.encode(SH_STOCK).0.into_owned();
    let (url, _) = spawn_header_capture_server(gbk);
    let quotes = tauri::async_runtime::block_on(fetch_tencent_quotes(
        &client,
        &mut pacer,
        &[url.as_str()],
        &queries,
    ))
    .expect("GBK 报文应解析成功");
    assert_eq!(quotes[0].name, "浦发银行");

    // 被风控拦截（200 + HTML）与非法 GBK 字节：报错，不退化为空序列——
    // 用户可见的源畸形报本单元专码（spec #1675 裁决 3，解析 detail 留日志）。
    for body in [
        b"<html><body>risk control</body></html>".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x01],
    ] {
        let (url, _) = spawn_header_capture_server(body);
        let error = tauri::async_runtime::block_on(fetch_tencent_quotes(
            &client,
            &mut pacer,
            &[url.as_str()],
            &queries,
        ))
        .expect_err("被拦截响应应报错");
        assert!(
            error.is_code("sync.quote-source-malformed"),
            "实际 {error:?}"
        );
    }
}

/// 生产主机与端点形态的接线钉（删除即红）：换源或改端点形态须显式改此处。
#[test]
fn production_host_and_path_pin_to_tencent_quote_endpoint() {
    assert_eq!(TENCENT_QUOTE_HOSTS, ["https://qt.gtimg.cn"]);
    assert_eq!(TENCENT_QUOTE_PATH_PREFIX, "/q=");
}

// ---------------------------------------------------------------------------
// 捕获到的请求头解析辅助（本地 HTTP 服务本体是共享单点
// `tests::spawn_header_capture_server`）
// ---------------------------------------------------------------------------

/// 取请求头首行的请求目标（`GET <target> HTTP/1.1` 第二段）。
fn request_target(head: &str) -> &str {
    head.lines()
        .next()
        .unwrap_or("")
        .split(' ')
        .nth(1)
        .unwrap_or("")
}

/// 请求目标里携带的查询键数（`/q=a,b,c` → 3）。
fn key_count(target: &str) -> usize {
    let keys = target
        .strip_prefix(TENCENT_QUOTE_PATH_PREFIX)
        .unwrap_or(target);
    if keys.is_empty() {
        0
    } else {
        keys.split(',').count()
    }
}
