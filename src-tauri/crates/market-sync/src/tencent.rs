//! 腾讯行情批量报价取数单元（ADR-0130 决策 2/3 / issue #1558）。
//!
//! 一次请求携带多只沪深港美股票与场内基金（`GET https://qt.gtimg.cn/q=<代码逗号串>`，
//! GBK 编码、无需 Referer），解析出代码、名称、价格、价格日期、证券类型码、币种与
//! 交易所后缀。类型码按市场分三套字段布局（A 股 88 字段 / 港股 78 / 美股 73，下标
//! 随之不同，不能按固定下标取），字段布局由 [`quote_layout`] 单点定义、类型码探测
//! 收口在 [`detect_kind_hint`] 单点；币种与美股交易所后缀同样自报（`.OQ` → 纳斯达
//! 克 / `.N` → 纽交所 / `.AM` → 美交所）。
//! 行情日期取数据源给出的交易所当地交易日的日期部分，**不做时区换算**（ADR-0130
//! 决策 5）——沿用北京时间切分会把美股周五的收盘记成周六。
//!
//! 本单元只取数与解析，不落库、不接 UI；现价刷新接线见通道束（issue #1560），
//! 按代码查询 / 创建接线见 [`super::stock`]（issue #1567，[`TencentQuote::into_quote`
//! ] 投影统一载荷）。
//!
//! fail-closed：被风控拦截的响应（HTML 页 / 空体）与非预期形状（缺报价语句、GBK
//! 解码出错、字段数少于该市场布局下界、未知市场前缀、未知美股交易所后缀）一律报
//! 错，**不**退化为「无数据」空序列——空序列会让「今天没有行情」与「数据源坏了」
//! 不可分辨。合法的「批量内全部代码无效」由数据源以 `v_pv_none_match="1"` 明示，
//! 仍按零命中（空序列）返回。
//!
//! 报文形态是未公开字段（腾讯无公开契约），探测与解析收口于本模块，漂移时改一处；
//! fixture 单测钉住取数面实测（2026-09-19 采集，行情日为 2026-09-18）的真实报文
//!（三套布局的类型码、币种与交易所后缀）。

use ledger_infra::error::{AppError, Result};
use ledger_investment::{InstrumentType, Quote};

use super::channels::{QuoteItem, QuoteQuery};
use super::http::{Pacer, RetryConfig, request_bytes_from_hosts};

/// 生成主机：腾讯财经公开报价端点（免费、无需 key 与 Referer；ADR-0130 决策 2）。
/// 入口按参数收主机，测试经本地 HTTP 服务注入假响应。
pub(super) const TENCENT_QUOTE_HOSTS: &[&str] = &["https://qt.gtimg.cn"];

/// 端点形态是**路径段** `q=<代码逗号串>`（不是查询参数）：
/// `GET https://qt.gtimg.cn/q=sh600000,sz000001`。
pub(super) const TENCENT_QUOTE_PATH_PREFIX: &str = "/q=";

/// 单次请求最多携带的查询键数：实测约 900 只撞 ~8KB 请求行上限（950 只 → HTTP 414），
/// 取 800 留足余量（2026-09-19 取数面实测，`docs/research/market-quote-data-sources.md`
/// §13.2）。超出的查询由 [`fetch_tencent_quotes`] 分批成多次请求。
pub(super) const TENCENT_QUOTE_BATCH_SIZE: usize = 800;

/// 腾讯行情单条报价（取数层产出）：数据源自报字段全量解出，交消费方投影。
#[derive(Debug, Clone, PartialEq)]
pub struct TencentQuote {
    /// 裸代码：沪深/港股为回显数字代码（港股已 5 位补零），美股为去交易所后缀的大写 ticker。
    pub code: String,
    /// 数据源权威名称（中文简称）。
    pub name: String,
    /// 最新价（万分之一元，0.0001 元，价格刻度 ADR-0038）；停牌/无效价（≤0）为 None。
    pub price_cents: Option<i64>,
    /// 价格日期（ISO 日期）：取数据源交易所当地交易日的日期部分，不做时区换算。
    pub price_date: Option<String>,
    /// 精确市场（sh / sz / hk / nasdaq / nyse / amex）：美股由自报交易所后缀判定。
    pub market: String,
    /// 证券类型码原值（未公开字段）：`GP-A` / `ETF` / `LOF` / `ZQ-KZZ` / `GP` / `GP-ETF`。
    pub security_type: String,
    /// 类型提示（stock / etf）：类型码经 [`detect_kind_hint`] 单点探测。
    pub kind_hint: InstrumentType,
    /// 报价币种（数据源自报）：CNY / HKD / USD。
    pub currency_code: String,
}

impl TencentQuote {
    /// 投影为通道束载荷 [`QuoteItem`]（issue #1560）：编排只需代码 / 名称 /
    /// 价格 / 行情日期，数据源私有字段（类型码 / 币种 / 精确市场）留在本层。
    /// 投影定义在取数单元内（而非接缝模块）：换源时改的是投影端，接缝零知识。
    pub(super) fn into_quote_item(self) -> QuoteItem {
        QuoteItem {
            code: self.code,
            name: self.name,
            price_cents: self.price_cents,
            price_date: self.price_date,
        }
    }

    /// 投影为行情接入接缝的统一载荷 [`Quote`]（ADR-0103）：场内通道成员（代码 /
    /// 名称 / 价格 / 价格日期 / 精确市场 / 类型提示）就位，场外成员（基金分类 /
    /// 净值日期 / 恒定价格信号）恒缺省。消费方为按代码查询 / 创建接线（#1567）。
    /// 币种不随投影携带：统一载荷无币种成员，落库与响应按精确市场推导
    ///（`derive_quote_currency`，ADR-0037 决策 2）——当前闭集内与自报币种
    ///（CNY/HKD/USD）恒等；若数据源自报漂移，判据在本层 `currency_code` 字段
    ///（fixture 钉住），届时显式改投影，不静默沿用推导。
    pub fn into_quote(self) -> Quote {
        Quote {
            code: self.code,
            name: self.name,
            price_cents: self.price_cents,
            price_date: self.price_date,
            market: Some(self.market),
            kind_hint: Some(self.kind_hint),
            fund_class: None,
            nav_date: None,
            constant_unit_price_cents: None,
        }
    }
}

/// 「市场 + 代码」→ 腾讯查询键（键构造单点，ADR-0130 决策 2 / issue #1555 的换源
/// 落点）：沪深港用 `<市场前缀><代码>`（`sh600000` / `sz161725` / `hk00700`），美股
/// 三市场与聚合路由值 `us`（按代码查询的解析产物，issue #1567——腾讯不区分交易
/// 所，精确交易所由响应自报后缀判定）统用 `us<代码>`。市场未知返回 None（同步
/// 编排侧不发请求、跳过该查询单元；查询侧为码化内部不一致）。
pub(super) fn tencent_query_key(market: &str, code: &str) -> Option<String> {
    match market {
        "sh" | "sz" | "hk" => Some(format!("{market}{code}")),
        "nasdaq" | "nyse" | "amex" | "us" => Some(format!("us{code}")),
        _ => None,
    }
}

/// 证券类型码 → 类型提示单点（ADR-0130 决策 3）：场内基金类（`ETF` / `LOF` /
/// `GP-ETF`）探测为 etf，其余（`GP-A` / `GP`）探测为 stock。
///
/// 可转债（A 股 `ZQ-KZZ`）与股票同走行情通道：ADR-0081 的场内类型提示只有 stock /
/// etf 两值，既有东财口径同样把「非场内基金特征」判为 stock，且 `InstrumentType::Bond`
/// 不在行情通道（会退成手动报价）——沿旧判据保持行为不变。类型码是未公开字段，
/// 探测漂移只改本函数一处。
pub(super) fn detect_kind_hint(security_type: &str) -> InstrumentType {
    match security_type {
        "ETF" | "LOF" | "GP-ETF" => InstrumentType::Etf,
        _ => InstrumentType::Stock,
    }
}

/// 十进制价格串（`9.07` / `4.582` / `419.000`）→ 万分之一元（0.0001 元，价格刻度
/// ADR-0038）。腾讯报价不超过 4 位小数，不足补零、超出截断；≤0 或非数值返回 None
/// （停牌 / 无效价）。走整数换算避开浮点误差。
pub(super) fn price_cents_from_decimal(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (int_part, frac_part) = match raw.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (raw, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let int_value: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let mut frac: String = frac_part.chars().take(4).collect();
    while frac.len() < 4 {
        frac.push('0');
    }
    let frac_value: i64 = frac.parse().ok()?;
    let cents = int_value.checked_mul(10_000)?.checked_add(frac_value)?;
    (cents > 0).then_some(cents)
}

/// 数据源时间戳 → 价格日期（ISO 日期）：取交易所当地交易日的**日期部分**，不做
/// 时区换算（ADR-0130 决策 5）。三套布局的时间戳形态不同——A 股 `YYYYMMDDhhmmss`、
/// 港股 `YYYY/MM/DD hh:mm:ss`、美股 `YYYY-MM-DD hh:mm:ss`——按形态解析后统一输出
/// `YYYY-MM-DD`；不可解析为 None。
pub(super) fn price_date_from_timestamp(raw: &str) -> Option<String> {
    let raw = raw.trim();
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
        "%Y%m%d%H%M%S",
        "%Y%m%d",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d",
    ] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, format) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

/// GBK 字节 → 文本：解码原语收口在 HTTP 层单点（[`super::http::decode_gbk`]）——
/// 腾讯报价与新浪场外基金批量面同为 GBK 通道，共用一份实现（issue #1564 上收）。
use super::http::decode_gbk;

/// 三套字段布局的唯一下标表（ADR-0130 决策 3 / 2026-09-19 取数面实测）：类型码、
/// 币种与字段数下界按市场不同，不能按固定下标取。`min_fields` 是布局漂移的判据——
/// 字段数低于该市场下界即 fail-closed，不静默当「行情缺一只」。
struct QuoteLayout {
    /// 证券类型码下标。
    kind_index: usize,
    /// 币种下标。
    currency_index: usize,
    /// 正常报文的字段数下界。
    min_fields: usize,
}

/// 报文键前缀 →（精确市场前缀，字段布局）。前缀即请求键的市场段（`sh` / `sz` /
/// `hk` / `us`）；未知前缀返回 None（非预期响应）。
fn quote_layout(key: &str) -> Option<(&'static str, QuoteLayout)> {
    let prefix = ["sh", "sz", "hk", "us"]
        .into_iter()
        .find(|prefix| key.len() > prefix.len() && key.starts_with(prefix))?;
    let layout = match prefix {
        // A 股：类型码 61（`GP-A` / `ETF` / `LOF` / `ZQ-KZZ`）、币种 82（CNY）、88 字段。
        "sh" | "sz" => QuoteLayout {
            kind_index: 61,
            currency_index: 82,
            min_fields: 88,
        },
        // 港股：类型码 63（`GP`）、币种 75（HKD）、78 字段。
        "hk" => QuoteLayout {
            kind_index: 63,
            currency_index: 75,
            min_fields: 78,
        },
        // 美股：类型码 56（`GP` / `GP-ETF`）、币种 35（USD）、73 字段；交易所后缀在
        // 回显代码字段（下标 2）内。
        _ => QuoteLayout {
            kind_index: 56,
            currency_index: 35,
            min_fields: 73,
        },
    };
    Some((prefix, layout))
}

/// 解析腾讯批量报价报文（GBK 已解码的文本）为按响应序的报价序列。
///
/// 逐条 `v_<键>="<字段串>";` 语句解析；`v_pv_none_match`（数据源明示「批量内全部
/// 代码无效」）忽略。fail-closed：整段无任何报价语句（风控 HTML 页 / 空体）、未知
/// 市场前缀、字段数少于该市场布局下界（布局漂移 / 截断）一律报错；空名称/空代码行
/// 按无效行丢弃。零命中（只有 `pv_none_match`）返回空序列，是可信结果。
///
/// 字段数偏少是**整批**报错而非丢单行：布局漂移会让全部行同形缩短，丢单行会把
/// 「数据源改版」静默伪装成「这只今天缺行情」；空名称则是字段自身的合法缺值
///（与既有行情解析丢弃空名行同口径），两者性质不同。
pub(super) fn parse_tencent_quotes(body: &str) -> Result<Vec<TencentQuote>> {
    let mut quotes = Vec::new();
    let mut saw_statement = false;
    for (key, value) in quote_statements(body) {
        saw_statement = true;
        if key == "pv_none_match" {
            continue;
        }
        let Some((prefix, layout)) = quote_layout(key) else {
            return Err(unexpected_response(format!("响应含非预期的报价键 {key}")));
        };
        let fields: Vec<&str> = value.split('~').collect();
        if fields.len() < layout.min_fields {
            return Err(unexpected_response(format!(
                "{prefix} 报价字段数 {} 少于布局下界 {}（疑似布局漂移或被截断）",
                fields.len(),
                layout.min_fields
            )));
        }
        if let Some(quote) = build_quote(prefix, &fields, &layout)? {
            quotes.push(quote);
        }
    }
    if !saw_statement {
        return Err(unexpected_response(
            "响应不含任何报价语句（疑似被风控拦截）",
        ));
    }
    Ok(quotes)
}

/// 单条报价语句 → [`TencentQuote`]。空名称/空代码行返回 `Ok(None)`（无效行丢弃）；
/// 美股缺交易所后缀或后缀未收录返回 `Err`（fail-closed，不猜市场）。
fn build_quote(
    prefix: &str,
    fields: &[&str],
    layout: &QuoteLayout,
) -> Result<Option<TencentQuote>> {
    let name = field(fields, 1).trim();
    let echo_code = field(fields, 2).trim();
    if name.is_empty() || echo_code.is_empty() {
        return Ok(None);
    }
    let (code, market) = if prefix == "us" {
        let (ticker, exchange) = split_us_code(echo_code)?;
        (ticker.to_string(), us_market_from_suffix(exchange)?)
    } else {
        (echo_code.to_string(), prefix)
    };
    let security_type = field(fields, layout.kind_index).trim().to_string();
    Ok(Some(TencentQuote {
        code,
        name: name.to_string(),
        price_cents: price_cents_from_decimal(field(fields, 3)),
        price_date: price_date_from_timestamp(field(fields, 30)),
        market: market.to_string(),
        kind_hint: detect_kind_hint(&security_type),
        security_type,
        currency_code: field(fields, layout.currency_index).trim().to_string(),
    }))
}

/// 按下标取字段（越界回空串；布局下界已在解析入口拦截，此处只作防御）。
fn field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("")
}

/// 美股回显代码（`AAPL.OQ`）→（裸 ticker，交易所后缀）。缺后缀即非预期形态。
fn split_us_code(echo_code: &str) -> Result<(&str, &str)> {
    echo_code
        .rsplit_once('.')
        .ok_or_else(|| unexpected_response(format!("美股报价缺少交易所后缀：{echo_code}")))
}

/// 交易所后缀 → 既有市场闭集三值（ADR-0081 决策 2）：`.OQ` 纳斯达克 / `.N` 纽交所 /
/// `.AM` 美交所。未收录后缀 fail-closed——静默丢弃会少一只，猜值会错挂市场。
fn us_market_from_suffix(suffix: &str) -> Result<&'static str> {
    match suffix {
        "OQ" => Ok("nasdaq"),
        "N" => Ok("nyse"),
        "AM" => Ok("amex"),
        _ => Err(unexpected_response(format!(
            "美股报价含未知交易所后缀 .{suffix}"
        ))),
    }
}

/// 从报文体里取出全部 `v_<键>="<字段串>"` 语句（键与字段串均不含引号）。报文按行
/// 分隔也可能同行多条，故按 `v_` 起点扫描而非按行切分；畸形片段跳过，不中断后续。
fn quote_statements(body: &str) -> Vec<(&str, &str)> {
    let mut statements = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = body[cursor..].find("v_") {
        let start = cursor + offset;
        let rest = &body[start + 2..];
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        let Some(value) = rest[eq + 1..].strip_prefix('"') else {
            cursor = start + 2;
            continue;
        };
        let Some(end) = value.find('"') else { break };
        statements.push((key, &value[..end]));
        cursor = start + 2 + eq + 2 + end + 1;
    }
    statements
}

/// 拉取一批腾讯行情报价（分批，单请求最多 [`TENCENT_QUOTE_BATCH_SIZE`] 只），
/// 合并各批结果为按请求序的报价序列。市场未知的查询单元不进请求。
pub(super) async fn fetch_tencent_quotes(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    queries: &[QuoteQuery],
) -> Result<Vec<TencentQuote>> {
    let mut quotes = Vec::new();
    for chunk in queries.chunks(TENCENT_QUOTE_BATCH_SIZE) {
        let keys: Vec<String> = chunk
            .iter()
            .filter_map(|query| tencent_query_key(&query.market, &query.code))
            .collect();
        if keys.is_empty() {
            continue;
        }
        quotes.extend(fetch_tencent_batch(client, pacer, hosts, &keys.join(",")).await?);
    }
    Ok(quotes)
}

/// 单批请求：请求行 `GET /q=<代码逗号串>`（无 Referer），GBK 解码后按
/// [`parse_tencent_quotes`] 解析。解析失败按疑似风控页补降速信号（文本形状判定在
/// HTTP 层看不见，先例：`bulk` 的两个批量面）。按代码查询的单只取数（#1567）
/// 复用本请求原语：单只 = 一批一条。
pub(super) async fn fetch_tencent_batch(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    keys: &str,
) -> Result<Vec<TencentQuote>> {
    tracing::debug!(keys, "腾讯行情批量报价查询");
    let path = format!("{TENCENT_QUOTE_PATH_PREFIX}{keys}");
    let bytes = request_bytes_from_hosts(
        client,
        &[],
        &path,
        hosts,
        RetryConfig::production(),
        pacer,
        "fetch_tencent_quotes",
        None,
    )
    .await?;
    // GBK 解码与报文形状两道判据都归本层：任一失败都补降速信号
    //（ADR-0121 决策 5，先例：bulk 的两个批量面）。解码错误统一包装为本单元
    // 的非预期响应错误（解码原语归 HTTP 层单点，单元上下文在此补齐）。
    decode_gbk(&bytes)
        .map_err(unexpected_response)
        .and_then(|body| parse_tencent_quotes(&body))
        .map_err(|error| {
            tracing::warn!(%error, "腾讯行情报价响应不可信");
            pacer.record_throttled();
            error
        })
}

/// 非预期形状的统一错误（被拦截 / 截断 / 布局漂移）：退出取数与解析，不回退为空
/// 序列。文案内部化（用户可见面是编排的「已降级、本次较慢」），细节留日志。
fn unexpected_response(detail: impl std::fmt::Display) -> AppError {
    AppError::Parse(format!("腾讯行情报价响应不可解析：{detail}"))
}
