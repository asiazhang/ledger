use std::collections::BTreeMap;
use std::time::Duration;

use chrono::NaiveDate;
use quick_xml::Reader;
use quick_xml::events::Event;

use ledger_infra::error::{AppError, Result};

use super::http::{Pacer, RetryConfig, request_text_from_hosts};
use super::source_tail;
use super::weekly::downsample_weekly_points;

const SOURCE: &str = "ecb-document";

pub(super) const ECB_HOSTS: &[&str] = &["https://www.ecb.europa.eu"];

pub(super) const FULL_HISTORY_PATH: &str = "/stats/eurofxref/eurofxref-hist.xml";
pub(super) const INCREMENTAL_90D_PATH: &str = "/stats/eurofxref/eurofxref-hist-90d.xml";

const FULL_HISTORY_LABEL: &str = "ECB 全量历史";
const INCREMENTAL_90D_LABEL: &str = "ECB 90 天增量";

const FULL_HISTORY_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn full_history_cfg() -> RetryConfig {
    RetryConfig {
        request_timeout: FULL_HISTORY_REQUEST_TIMEOUT,
        ..RetryConfig::production()
    }
}
fn malformed_source() -> AppError {
    AppError::coded(
        "fx.source-malformed",
        "汇率数据源返回了无法解析的内容，请稍后重试同步",
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct EcbDayRates {
    pub date: NaiveDate,
    pub rates: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FxPairWeeklySeries {
    pub base: String,
    pub quote: String,
    pub points: Vec<(NaiveDate, f64)>,
}

pub(super) async fn fetch_ecb_full_history(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    cfg: RetryConfig,
) -> Result<Vec<EcbDayRates>> {
    fetch_ecb_document(
        client,
        pacer,
        hosts,
        FULL_HISTORY_PATH,
        FULL_HISTORY_LABEL,
        cfg,
    )
    .await
}

pub(super) async fn fetch_ecb_90d_incremental(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    cfg: RetryConfig,
) -> Result<Vec<EcbDayRates>> {
    fetch_ecb_document(
        client,
        pacer,
        hosts,
        INCREMENTAL_90D_PATH,
        INCREMENTAL_90D_LABEL,
        cfg,
    )
    .await
}

async fn fetch_ecb_document(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    path: &str,
    label: &str,
    cfg: RetryConfig,
) -> Result<Vec<EcbDayRates>> {
    tracing::debug!(path, "ECB 参考汇率文件查询");
    let text = request_text_from_hosts(
        client,
        &[],
        path,
        hosts,
        cfg,
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

pub(super) fn parse_ecb_rates(xml: &str) -> std::result::Result<Vec<EcbDayRates>, String> {
    if xml.trim().is_empty() {
        return Err("空文件（无任何报文内容）".to_string());
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut days: BTreeMap<NaiveDate, BTreeMap<String, f64>> = BTreeMap::new();
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

fn cross_rate(rates: &BTreeMap<String, f64>, base: &str, quote: &str) -> Option<f64> {
    if base == quote {
        return None;
    }
    let leg = |code: &str| rates.get(code).copied();
    match (base, quote) {
        (_, "EUR") => Some(1.0 / leg(base)?),
        ("EUR", _) => Some(leg(quote)?),
        _ => {
            let base_leg = leg(base)?;
            Some(leg(quote)? / base_leg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str) -> NaiveDate {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()
    }

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

    #[test]
    fn parse_accepts_plain_cube_without_namespaces() {
        let xml =
            r#"<Cube><Cube time="2026-09-18"><Cube currency="CNY" rate="7.6755"/></Cube></Cube>"#;
        let days = parse_ecb_rates(xml).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].rates.get("CNY"), Some(&7.6755));
    }

    #[test]
    fn parse_rejects_empty_file() {
        for empty in ["", "   \n\t"] {
            assert_malformed(parse_ecb_rates(empty).unwrap_err());
        }
    }

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

    #[test]
    fn parse_rejects_truncated_document() {
        let cut_mid_element = &SAMPLE_XML[..SAMPLE_XML.len() - 40];
        assert_malformed(parse_ecb_rates(cut_mid_element).unwrap_err());
        let missing_envelope_close = SAMPLE_XML.strip_suffix("</gesmes:Envelope>").unwrap();
        assert_malformed(parse_ecb_rates(missing_envelope_close).unwrap_err());
    }

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
        assert_eq!(rate("HKD"), 7.6755f64 / 8.9903f64);
        assert!(
            rate("HKD") > 0.8537 && rate("HKD") < 0.8538,
            "独立数值锥：7.6755/8.9903 ≈ 0.85375，实际 {}",
            rate("HKD")
        );
        assert_eq!(rate("EUR"), 7.6755f64);
        assert_eq!(rate("CNY"), 1.0f64 / 7.6755f64);
        assert_eq!(rate("USD"), 1.0f64 / 1.146f64);
    }

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

    #[test]
    fn derive_weekly_takes_last_quoted_day_and_skips_quoteless_weeks() {
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

    #[test]
    fn derive_skips_same_currency_pair() {
        let days = parse_ecb_rates(SAMPLE_XML).unwrap();
        let series = derive_ecb_weekly_series(&days, &[("CNY".to_string(), "CNY".to_string())]);
        assert!(series.is_empty(), "同币种对不产出序列");
    }

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
        let eur = series.iter().find(|s| s.base == "EUR").unwrap();
        assert_eq!(eur.points[0].1, 7.6755_f64);
    }
}
