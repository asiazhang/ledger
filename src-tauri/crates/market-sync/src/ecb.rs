//! ECB 参考汇率取数单元（ADR-0019 修订记录「汇率序列换源 ECB」/ issue #1542）。
//!
//! 全量历史与 90 天增量两个取数入口（同一 Cube 报文形状、不同文件），解析后按
//! **同日两腿交叉**推导币种对（如 HKD/CNY = CNY 腿 ÷ HKD 腿；ECB 报文以 EUR 为
//! 基准腿，1 EUR = ? 各币种），再降采样为周采样（每周取该周最后一个有报价交易日），
//! 产出「可落库的周采样序列」——本单元只产出序列，不落库、不接 UI（落库与触发
//! 编排归后续票）。
//!
//! 窗口与触发归调用方：全量入口服务历史回填（按账本最早非本位币日期定深度，
//! ADR-0019 修订记录），90 天增量入口服务每日自动同步；两者都与标的 K 线的
//! 「近两年」窗口和东财额度无关（独立主机、独立限速）。
//!
//! 数据源返回非预期形状（空文件、非 XML、截断）时报 [`fx.source-malformed`]
//! 码化错误，不静默产出空序列——空序列会让「该周无点」与「数据坏了」不可分辨；
//! 源畸形的截断日志与降速信号经取数尾部契约（`source_tail`，spec #1675——
//! 降速是 ADR-0121 决策 5 的补齐执行）统一补齐。ECB 拉取按可重建缓存对待
//!（ADR-0019 修订记录），不进同步日志。

use std::collections::BTreeMap;

use chrono::NaiveDate;
use quick_xml::Reader;
use quick_xml::events::Event;

use ledger_infra::error::{AppError, Result};

use super::http::{Pacer, RetryConfig, request_text_from_hosts};
use super::source_tail;
use super::weekly::downsample_weekly_points;

/// 单元标识（取数尾部契约的 `source` 日志字段）：源畸形 warn 按此分源 grep。
const SOURCE: &str = "ecb-document";

/// 生产主机（ECB 官方站，免费、无 key、有公开契约；ADR-0019 修订记录）。
/// 入口按参数收主机，测试经本地 HTTP 服务注入假响应。
pub(super) const ECB_HOSTS: &[&str] = &["https://www.ecb.europa.eu"];

/// 全量历史文件路径（1999-01-04 起全部已发布参考汇率）。
pub(super) const FULL_HISTORY_PATH: &str = "/stats/eurofxref/eurofxref-hist.xml";
/// 90 天增量文件路径（每日自动增量的数据源）。
pub(super) const INCREMENTAL_90D_PATH: &str = "/stats/eurofxref/eurofxref-hist-90d.xml";

const FULL_HISTORY_LABEL: &str = "ECB 全量历史";
const INCREMENTAL_90D_LABEL: &str = "ECB 90 天增量";

/// 非预期形状的码化错误映射（空文件 / 非 XML / 截断 / 零可用日共用一码；
/// 构造时机与形状归取数尾部契约，spec #1675）。具体是哪个文件、什么形状，
/// 由日志 ctx（`fetch_ecb:{label}`）与契约统一 warn 的 error 字段定位；
/// 两个入口的用户补救动作相同（稍后重试），不区分错误参数（ADR-0050：params
/// 须 locale 无关，中文数据集名不进 params）。
fn malformed_source() -> AppError {
    AppError::coded(
        "fx.source-malformed",
        "汇率数据源返回了无法解析的内容，请稍后重试同步",
    )
}

/// ECB 参考汇率的一日快照：日期 + 各币种腿（1 EUR = ? 该币种，EUR 为基准腿）。
#[derive(Debug, Clone, PartialEq)]
pub struct EcbDayRates {
    pub date: NaiveDate,
    /// 键 = ISO 币种代码，值 = 1 EUR 兑该币种的比率（恒 > 0，非正值在解析层即弃）。
    pub rates: BTreeMap<String, f64>,
}

/// 单币种对的可落库周采样序列：`points` 每周至多一点（该周最后一个有报价交易日），
/// 形态与 [`super::weekly::commit_fx_rate_history_weekly`] 的入参同形（载体中立
/// 逐日点集，spec #1677；采样日为 ISO 日期，rate 口径「1 base = ? quote」与
/// exchange_rates / fx_rate_history 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct FxPairWeeklySeries {
    pub base: String,
    pub quote: String,
    /// (采样日, rate)，按日期升序。
    pub points: Vec<(NaiveDate, f64)>,
}

/// 拉取 ECB 参考汇率**全量历史**文件并解析（历史回填入口：窗口深度由调用方按
/// 账本最早非本位币日期裁剪，ADR-0019 修订记录）。`hosts` 供测试注入本地服务，
/// 生产传 [`ECB_HOSTS`]。
pub(super) async fn fetch_ecb_full_history(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
) -> Result<Vec<EcbDayRates>> {
    fetch_ecb_document(client, pacer, hosts, FULL_HISTORY_PATH, FULL_HISTORY_LABEL).await
}

/// 拉取 ECB 参考汇率 **90 天增量**文件并解析（每日自动增量入口）。`hosts` 供
/// 测试注入本地服务，生产传 [`ECB_HOSTS`]。
pub(super) async fn fetch_ecb_90d_incremental(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
) -> Result<Vec<EcbDayRates>> {
    fetch_ecb_document(
        client,
        pacer,
        hosts,
        INCREMENTAL_90D_PATH,
        INCREMENTAL_90D_LABEL,
    )
    .await
}

/// 两个取数入口的共用通道：文本通道取回原文（纯文本通道的解析恒成功，复用既有
/// 多主机切换 / 重试 / 限流冷却），Cube 报文的形状校验在解析闭包判定——报文坏了
/// 重试也修不好，不进「疑似风控页」的长冷却重试循环；解析失败经取数尾部契约
///（[`source_tail::finish`]，spec #1675）统一截断 warn、补降速信号并映射码化
/// 错误（裁决 4：本单元原是五单元中唯一漏补降速的，随契约接线补齐）。
async fn fetch_ecb_document(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    path: &str,
    label: &str,
) -> Result<Vec<EcbDayRates>> {
    tracing::debug!(path, "ECB 参考汇率文件查询");
    let text = request_text_from_hosts(
        client,
        &[],
        path,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_ecb:{label}"),
        None,
    )
    .await?;
    source_tail::finish(
        SOURCE,
        text.as_bytes(),
        pacer,
        source_tail::utf8,
        parse_ecb_rates,
        malformed_source,
    )
}

/// 解析 ECB 参考汇率 XML（gesmes:Envelope → Cube → 按日 Cube@time → 每币种
/// Cube@currency/@rate，日期任意序）为按日期**升序**的日快照序列。
///
/// 形状宽容度：命名空间前缀不参与匹配（按本地名 Cube 认元素），报文里个别坏腿
/// （rate 非数值 / ≤ 0）与坏日（time 缺失或不可解析）按「该腿 / 该日缺失」跳过
/// ——单点损坏不中断整体。文件级非预期形状（空 / 非 XML / 截断 / 零可用日）报
/// `Err(detail)`（失败原因保留在错误详情，由取数尾部契约统一入日志、补降速并
/// 映射 `fx.source-malformed`）。
pub(super) fn parse_ecb_rates(xml: &str) -> std::result::Result<Vec<EcbDayRates>, String> {
    if xml.trim().is_empty() {
        return Err("空文件（无任何报文内容）".to_string());
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut days: BTreeMap<NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();
    // 当前腿条目归属的日期：日条目（带 time）切换，坏日期使后续腿无处落位即弃。
    let mut current: Option<NaiveDate> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                on_cube(&e, &mut days, &mut current)?;
            }
            Ok(Event::Empty(e)) => {
                on_cube(&e, &mut days, &mut current)?;
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(format!("XML 解析失败：{e}"));
            }
        }
    }
    // 截断文档（元素未闭合，读到 EOF 仍在开标签内）按非预期形状报错；
    // 零可用日（如被拦截页恰好是合法 XML）同样不静默产出空序列。
    if depth != 0 || days.is_empty() {
        return Err(format!(
            "报文不可信（未闭合元素深度 {depth}、可用日 {} 天）",
            days.len()
        ));
    }
    Ok(days
        .into_iter()
        .map(|(date, rates)| EcbDayRates { date, rates })
        .collect())
}

/// 单个 Cube 元素处理：带 `time` 属性即日条目（切换归属日），带 `currency`+`rate`
/// 即腿条目（归属当前日；rate 非数值或 ≤ 0 视为该腿缺失跳过）。非 Cube 元素忽略。
fn on_cube(
    e: &quick_xml::events::BytesStart<'_>,
    days: &mut BTreeMap<NaiveDate, BTreeMap<String, f64>>,
    current: &mut Option<NaiveDate>,
) -> std::result::Result<(), String> {
    if e.name().local_name().as_ref() != "Cube" {
        return Ok(());
    }
    let mut time_raw: Option<String> = None;
    let mut currency: Option<String> = None;
    let mut rate: Option<f64> = None;
    for attr in e.attributes() {
        let attr = attr.map_err(|_| "Cube 元素属性不可读".to_string())?;
        match attr.key.local_name().as_ref() {
            "time" => time_raw = Some(attr.value.trim().to_owned()),
            "currency" => currency = Some(attr.value.trim().to_owned()),
            "rate" => rate = attr.value.trim().parse::<f64>().ok(),
            _ => {}
        }
    }
    if let Some(raw) = time_raw {
        *current = NaiveDate::parse_from_str(&raw, "%Y-%m-%d").ok();
        return Ok(());
    }
    if let (Some(code), Some(value)) = (currency, rate.filter(|v| *v > 0.0))
        && let Some(date) = current
    {
        days.entry(*date).or_default().insert(code, value);
    }
    Ok(())
}
/// 按同日两腿交叉把日快照序列推导为各币种对的周采样序列（可落库形态）。
///
/// 交叉规则（ECB 报文 1 EUR = ? X，EUR 为基准腿）：
/// - 双非 EUR 对（base→quote）= quote 腿 ÷ base 腿（如 HKD/CNY = CNY 腿 ÷ HKD 腿）；
/// - base=EUR 直取 quote 腿；quote=EUR 取 base 腿的倒数。
///
/// 任一腿在当日缺失即跳过该日（不猜值）——CNY 腿 2005-04 才存在，此前的日期对
/// CNY 对天然无点。同币种对跳过（无需折算）。某币种对整段无点的输出为空序列
/// （调用方可据此上报覆盖缺口），文件级损坏由解析层报错、不会静默走到这里。
pub(super) fn derive_ecb_weekly_series(
    days: &[EcbDayRates],
    pairs: &[(String, String)],
) -> Vec<FxPairWeeklySeries> {
    pairs
        .iter()
        .filter(|(base, quote)| base != quote)
        .map(|(base, quote)| FxPairWeeklySeries {
            base: base.clone(),
            quote: quote.clone(),
            points: downsample_weekly_points(days.iter().filter_map(|day| {
                cross_rate(&day.rates, base, quote).map(|rate| (day.date, rate))
            })),
        })
        .collect()
}

/// 同日两腿交叉取值（1 base = ? quote）；腿缺失返回 None（调用方跳过该日）。
fn cross_rate(rates: &BTreeMap<String, f64>, base: &str, quote: &str) -> Option<f64> {
    if base == quote {
        return None;
    }
    let leg = |code: &str| rates.get(code).copied();
    match (base, quote) {
        // 1 base = ? EUR = 1 / base 腿
        (_, "EUR") => Some(1.0 / leg(base)?),
        // 1 EUR = ? quote（EUR 自身即基准腿，直取）
        ("EUR", _) => Some(leg(quote)?),
        // 1 base = (quote 腿 / base 腿) quote
        _ => {
            let base_leg = leg(base)?;
            Some(leg(quote)? / base_leg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试侧周采样点日期解析（points 已载体中立为 NaiveDate，spec #1677）。
    fn day(date: &str) -> NaiveDate {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()
    }

    /// 真实报文形状钉值：gesmes 前缀 + 默认命名空间 + 自闭腿条目 + 日期降序（ECB 原样）。
    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref"><gesmes:subject>Reference rates</gesmes:subject><gesmes:Sender><gesmes:name>European Central Bank</gesmes:name></gesmes:Sender><Cube>
<Cube time="2026-09-18"><Cube currency="USD" rate="1.146"/><Cube currency="HKD" rate="8.9903"/><Cube currency="CNY" rate="7.6755"/></Cube>
<Cube time="2026-09-17"><Cube currency="USD" rate="1.1481"/><Cube currency="HKD" rate="9.0071"/><Cube currency="CNY" rate="7.7009"/></Cube>
</Cube></gesmes:Envelope>"#;

    fn assert_malformed(detail: String) {
        assert!(
            !detail.is_empty(),
            "文件级非预期形状应回 Err(detail)（失败原因入契约 warn 的 error 字段）"
        );
    }

    /// 真实形状解析钉值：日期升序化、腿值原样。
    #[test]
    fn parse_pins_real_envelope_shape() {
        let days = parse_ecb_rates(SAMPLE_XML).unwrap();
        assert_eq!(
            days.iter().map(|d| d.date).collect::<Vec<_>>(),
            vec![
                NaiveDate::from_ymd_opt(2026, 9, 17).unwrap(),
                NaiveDate::from_ymd_opt(2026, 9, 18).unwrap(),
            ],
            "输出按日期升序（原报文降序）"
        );
        let latest = &days[1];
        assert_eq!(latest.rates.get("USD"), Some(&1.146));
        assert_eq!(latest.rates.get("HKD"), Some(&8.9903));
        assert_eq!(latest.rates.get("CNY"), Some(&7.6755));
    }

    /// 无命名空间前缀的裸 Cube 形态同样可解析（前缀不参与匹配）。
    #[test]
    fn parse_accepts_plain_cube_without_namespaces() {
        let xml =
            r#"<Cube><Cube time="2026-09-18"><Cube currency="CNY" rate="7.6755"/></Cube></Cube>"#;
        let days = parse_ecb_rates(xml).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].rates.get("CNY"), Some(&7.6755));
    }

    /// 空文件报码化错误，不产出空序列。
    #[test]
    fn parse_rejects_empty_file() {
        for empty in ["", "   \n\t"] {
            assert_malformed(parse_ecb_rates(empty).unwrap_err());
        }
    }

    /// 非 XML 文本（含被拦截页常见形态）报码化错误。
    #[test]
    fn parse_rejects_non_xml_text() {
        for garbage in [
            "risk control page",
            "<<>>",
            "<html><body>blocked</body></html>",
        ] {
            assert_malformed(parse_ecb_rates(garbage).unwrap_err());
        }
    }

    /// 截断文档（元素未闭合 / 信封未关闭）报码化错误——截断的前半段不冒充完整数据。
    #[test]
    fn parse_rejects_truncated_document() {
        let cut_mid_element = &SAMPLE_XML[..SAMPLE_XML.len() - 40];
        assert_malformed(parse_ecb_rates(cut_mid_element).unwrap_err());
        let missing_envelope_close = SAMPLE_XML.strip_suffix("</gesmes:Envelope>").unwrap();
        assert_malformed(parse_ecb_rates(missing_envelope_close).unwrap_err());
    }

    /// 个别坏腿（rate 非数值 / ≤ 0）与坏日（time 不可解析）跳过，其余照常解析。
    #[test]
    fn parse_skips_unusable_legs_and_days() {
        let xml = r#"<Cube>
<Cube time="2026-13-99"><Cube currency="CNY" rate="7.6755"/></Cube>
<Cube time="2026-09-18"><Cube currency="BAD" rate="n/a"/><Cube currency="ZERO" rate="0"/><Cube currency="NEG" rate="-1.5"/><Cube currency="CNY" rate="7.6755"/></Cube>
</Cube>"#;
        let days = parse_ecb_rates(xml).unwrap();
        assert_eq!(days.len(), 1, "坏日期的日条目跳过");
        let day = &days[0];
        assert_eq!(day.rates.len(), 1, "坏腿跳过，只留好腿");
        assert_eq!(day.rates.get("CNY"), Some(&7.6755));
    }

    /// EUR 作基准腿的专门断言：双非 EUR 对取两腿之商，EUR 端直取腿 / 倒数。
    #[test]
    fn derive_eur_is_the_pivot_leg() {
        let days = parse_ecb_rates(SAMPLE_XML).unwrap();
        let day = &days[1];
        let pairs = [
            ("HKD".to_string(), "CNY".to_string()),
            ("EUR".to_string(), "CNY".to_string()),
            ("CNY".to_string(), "EUR".to_string()),
            ("USD".to_string(), "EUR".to_string()),
        ];
        let series = derive_ecb_weekly_series(std::slice::from_ref(day), &pairs);
        let rate = |base: &str| series.iter().find(|s| s.base == base).unwrap().points[0].1;
        // HKD/CNY = CNY 腿 ÷ HKD 腿（同日两腿交叉）；独立数值锥区间锚定（除法公式
        // 同源表达式之外，另行锚定结果的绝对取值范围）。
        assert_eq!(rate("HKD"), 7.6755f64 / 8.9903f64);
        assert!(
            rate("HKD") > 0.8537 && rate("HKD") < 0.8538,
            "独立数值锥：7.6755/8.9903 ≈ 0.85375，实际 {}",
            rate("HKD")
        );
        // EUR 为基准腿：EUR→X 直取 X 腿，X→EUR 取 X 腿倒数
        assert_eq!(rate("EUR"), 7.6755f64);
        assert_eq!(rate("CNY"), 1.0f64 / 7.6755f64);
        assert_eq!(rate("USD"), 1.0f64 / 1.146f64);
    }

    /// 任一腿缺失的日期跳过而不是猜值：CNY 腿缺席的首日在序列里无点。
    #[test]
    fn derive_skips_days_with_missing_leg() {
        let xml = r#"<Cube>
<Cube time="2026-09-17"><Cube currency="HKD" rate="9.0071"/></Cube>
<Cube time="2026-09-18"><Cube currency="HKD" rate="8.9903"/><Cube currency="CNY" rate="7.6755"/></Cube>
</Cube>"#;
        let days = parse_ecb_rates(xml).unwrap();
        let series = derive_ecb_weekly_series(&days, &[("HKD".to_string(), "CNY".to_string())]);
        assert_eq!(series[0].points, vec![(day("2026-09-18"), 7.6755 / 8.9903)]);
    }

    /// 周采样：每周取该周最后一个有报价交易日；整周无报价的周不出点。
    #[test]
    fn derive_weekly_takes_last_quoted_day_and_skips_quoteless_weeks() {
        // 第 1 周（09-14 周一）：周二 + 周五两天有价 → 取周五；
        // 第 2 周（09-21 起）：整周无报价 → 不出点；
        // 第 3 周（09-28 周一）：仅周一有价 → 取周一。
        let xml = r#"<Cube>
<Cube time="2026-09-15"><Cube currency="CNY" rate="7.70"/><Cube currency="HKD" rate="9.00"/></Cube>
<Cube time="2026-09-18"><Cube currency="CNY" rate="7.6755"/><Cube currency="HKD" rate="8.9903"/></Cube>
<Cube time="2026-09-28"><Cube currency="CNY" rate="7.80"/><Cube currency="HKD" rate="9.10"/></Cube>
</Cube>"#;
        let days = parse_ecb_rates(xml).unwrap();
        let series = derive_ecb_weekly_series(&days, &[("HKD".to_string(), "CNY".to_string())]);
        assert_eq!(
            series[0].points,
            vec![
                (day("2026-09-18"), 7.6755 / 8.9903),
                (day("2026-09-28"), 7.80 / 9.10),
            ]
        );
    }

    /// 同币种对无需折算，不产出序列。
    #[test]
    fn derive_skips_same_currency_pair() {
        let days = parse_ecb_rates(SAMPLE_XML).unwrap();
        let series = derive_ecb_weekly_series(&days, &[("CNY".to_string(), "CNY".to_string())]);
        assert!(series.is_empty(), "同币种对不产出序列");
    }

    /// 币种字典全量覆盖：字典内每个非本位币币种（含 EUR 自身）在正常响应下都
    /// 能推导出与本位币的币种对。字典读种子库、本位币走既有接缝，与生产同源。
    /// 真实 ECB 文件对种子币种的覆盖已实测核对（父 spec #1540 事实依据节：
    /// 全量文件含字典全部非 EUR 币种，CNY 自 2005-04）；本测试以同形夹具钉住
    /// 推导面，真实文件级复核随 #1543 落库接线再验。
    #[test]
    fn dictionary_covers_all_currencies_against_base() {
        let conn = tauri_app_lib::test_support::open();
        let codes: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT code FROM currencies ORDER BY code")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<String>>>()
                .unwrap()
        };
        let native = ledger_transaction::amount::default_currency_code(&conn).unwrap();
        let pairs: Vec<(String, String)> = codes
            .iter()
            .map(|code| (code.clone(), native.clone()))
            .filter(|(base, quote)| base != quote)
            .collect();
        assert_eq!(
            pairs.len(),
            10,
            "种子字典 11 币种，本位币 CNY 除外应有 10 对"
        );

        // 两周 × 两日，腿覆盖字典全部币种（含 EUR 基准腿与 CNY 腿），每腿数值可解析且互异。
        let mut xml = String::from("<Cube>");
        for (day, cny) in [("2026-09-17", "7.7009"), ("2026-09-18", "7.6755")] {
            xml.push_str(&format!(r#"<Cube time="{day}">"#));
            for (idx, code) in codes.iter().enumerate() {
                let rate = if code == "CNY" {
                    cny.to_string()
                } else if code == "EUR" {
                    "0.9".to_string()
                } else {
                    format!("{:.4}", 1.0 + (idx as f64) / 10.0)
                };
                xml.push_str(&format!(r#"<Cube currency="{code}" rate="{rate}"/>"#));
            }
            xml.push_str("</Cube>");
        }
        xml.push_str("</Cube>");

        let days = parse_ecb_rates(&xml).unwrap();
        let series = derive_ecb_weekly_series(&days, &pairs);
        assert_eq!(series.len(), pairs.len(), "每个币种对都有序列");
        for s in &series {
            assert!(
                !s.points.is_empty(),
                "{}→{} 在正常响应下应有周采样点",
                s.base,
                s.quote
            );
            assert_eq!(s.quote, native, "方向口径：字典币种 → 本位币");
        }
        // EUR 自身虽是基准腿，其与本位币的对同样产点（EUR→CNY 直取 CNY 腿）。
        let eur = series.iter().find(|s| s.base == "EUR").unwrap();
        assert_eq!(eur.points[0].1, 7.6755_f64);
    }
}
