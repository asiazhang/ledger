use std::collections::HashMap;

use serde::Deserialize;

use ledger_infra::error::Result;
use ledger_investment::QuoteMarket;

use super::channels::QuoteQuery;
use super::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts};

pub(super) const TENCENT_KLINE_HOSTS: &[&str] = &["https://web.ifzq.gtimg.cn"];

pub(super) const TENCENT_KLINE_PATH: &str = "/appstock/app/fqkline/get";

const PERIOD_WEEK: &str = "week";

pub(super) const KLINE_COUNT: u32 = 1200;

pub(super) const KLINE_BEG: &str = "2000-01-01";

pub(super) fn kline_symbol(query: &QuoteQuery) -> String {
    match query.market {
        QuoteMarket::Sh | QuoteMarket::Sz | QuoteMarket::Hk => {
            format!("{}{}", query.market.as_str(), query.code)
        }
        QuoteMarket::Nasdaq => format!("us{}.OQ", query.code),
        QuoteMarket::Nyse => format!("us{}.N", query.code),
        QuoteMarket::Amex => format!("us{}.AM", query.code),
    }
}

fn kline_param(symbol: &str, beg: &str, end: &str, count: u32) -> String {
    format!("{symbol},{PERIOD_WEEK},{beg},{end},{count},")
}

#[derive(Debug, Deserialize)]
pub(super) struct TencentKlineResponse {
    data: HashMap<String, TencentSymbolKline>,
}

#[derive(Debug, Deserialize)]
struct TencentSymbolKline {
    #[serde(default)]
    week: Option<Vec<Vec<serde_json::Value>>>,
}

impl TencentKlineResponse {
    pub(super) fn into_bars(self, symbol: &str) -> Vec<KlineBar> {
        self.data
            .get(symbol)
            .and_then(|payload| payload.week.as_deref())
            .map(parse_week_rows)
            .unwrap_or_default()
    }
}

fn parse_week_rows(rows: &[Vec<serde_json::Value>]) -> Vec<KlineBar> {
    rows.iter()
        .filter_map(|row| {
            let date = row.first()?.as_str()?.trim();
            if date.is_empty() {
                return None;
            }
            let close = value_as_f64(row.get(2)?)?;
            (close > 0.0).then(|| KlineBar::new(date, close))
        })
        .collect()
}

fn value_as_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::String(text) => text.trim().parse().ok(),
        serde_json::Value::Number(number) => number.as_f64(),
        _ => None,
    }
}

pub(super) async fn fetch_tencent_week_kline(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    symbol: &str,
    beg: &str,
    end: &str,
    count: u32,
) -> Result<Vec<KlineBar>> {
    tracing::debug!(symbol, beg, end, count, "腾讯周线 K 线查询");
    let param = kline_param(symbol, beg, end, count);
    let params = [("param", param.as_str())];
    let response: TencentKlineResponse = request_json_from_hosts(
        client,
        &params,
        TENCENT_KLINE_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_tencent_week_kline:{symbol}"),
        None,
    )
    .await?;
    Ok(response.into_bars(symbol))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SH: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2026-09-04","1273.930","1258.000","1274.980","1254.100","26235.000"],["2026-09-11","1257.980","1266.980","1267.600","1254.000","17554.000"],["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519","1257.12","1266.98","1262.99","24891","12061","12829","1257.12","8","1257.11","2","1257.08","1","1257.06","2","1257.05","2","1257.13","1","1257.24","2","1257.28","1","1258.00","16","1258.28","1","","20260918161436","-9.86","-0.78","1265.88","1256.10","1257.12\/24891\/3135849108","24891","313585","0.20","19.30","","1265.88","1256.10","0.77","15715.03","15715.03","6.25","1393.68","1140.28","1.14","-6","1259.84","17.65","19.09","","","0.08","313584.9108","527.9904","42","   A","GP-A","-6.82","-1.41","4.14","32.41","27.30","1539.98","1151.01","-5.48","-1.23","7.57","1250081601","1250081601","-16.67","-8.89","1250081601","","","-11.22","-0.42","","CNY","0","___D__F__N","1257.00","102",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"1272.750","version":"16"}}}"#;
    const SZ: &str = r#"{"code":0,"msg":"","data":{"sz000001":{"week":[["2026-09-04","11.800","11.700","11.840","11.570","949626.000"],["2026-09-11","11.680","11.610","11.740","11.570","691925.000"],["2026-09-18","11.590","11.700","11.820","11.560","853038.000"]],"qt":{"sz000001":["51","\u5e73\u5b89\u94f6\u884c","000001","11.70","11.61","11.59","853038","452206","400831","11.70","7029","11.69","2100","11.68","2873","11.67","1086","11.66","1423","11.71","1849","11.72","1006","11.73","1250","11.74","4120","11.75","4537","","20260918161427","0.09","0.78","11.82","11.56","11.70\/853038\/999918140","853038","99992","0.44","5.22","","11.82","11.56","2.24","2270.47","2270.49","0.48","12.77","10.45","1.07","1749","11.72","4.42","5.33","","","0.18","99991.8140","19.4220","166","   A","GP-A","5.88","-0.34","5.09","7.93","0.72","12.08","9.99","-1.60","2.54","14.37","19405684991","19405918198","6.41","4.46","19405684991","","","8.19","-0.17","","CNY","0","","11.80","-13699",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"11.820","version":"16"}}}"#;
    const HK: &str = r#"{"code":0,"msg":"","data":{"hk00700":{"week":[["2026-09-04","438.800","433.400","438.800","432.600","12686315.000",{"cqr":"2026-09-04","FHcontent":"","HGcontent":"\u56de\u8d2d23.10\u4e07\u80a1\uff0c\u5747\u4ef7434.844\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}],["2026-09-11","426.200","426.000","431.000","425.000","16185147.000",{"cqr":"2026-09-11","FHcontent":"","HGcontent":"\u56de\u8d2d23.50\u4e07\u80a1\uff0c\u5747\u4ef7426.707\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}],["2026-09-18","428.000","419.000","430.400","419.000","28796138.000",{"cqr":"2026-09-18","FHcontent":"","HGcontent":"\u56de\u8d2d23.70\u4e07\u80a1\uff0c\u5747\u4ef7423.851\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}]],"qt":{"hk00700":["100","\u817e\u8baf\u63a7\u80a1","00700","419.000","426.000","428.000","28796138.0","0","0","419.000","0","0","0","0","0","0","0","0","419.000","0","0","0","0","0","0","0","0","0","28796138.0","2026\/09\/18 16:08:32","-7.000","-1.64","430.400","419.000","419.000","28796138.0","12180786280.956","0","15.31","","0","0","2.68","38108.4103","38108.4103","TENCENT","1.27","677.700","411.000","1.85","-25.88","0","0","0","0","0","14.05","2.93","0.32","100","-29.43","-2.19","GP","20.41","11.00","-5.37","-8.32","-0.57","9095085993.00","9095085993.00","14.50","5.313","423.001","-29.78","HKD","1","50"],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"prec":"438.800","vcm":"","version":"16"}}}"#;
    const NQ: &str = r#"{"code":0,"msg":"","data":{"usAAPL.OQ":{"week":[["2026-09-04","332.530","332.410","335.480","330.700","35981000.000"],["2026-09-11","334.770","337.000","338.340","330.180","36700225.000"],["2026-09-18","337.910","336.130","338.490","332.530","86588203.000"]],"qt":{"usAAPL.OQ":["delay","\u82f9\u679c","AAPL.OQ","336.13","337.00","337.91","86588203","0","0","334.75","440","0","0","0","0","0","0","0","0","334.88","40","0","0","0","0","0","0","0","0","","2026-09-18 16:00:02","-0.87","-0.26","338.49","332.53","USD","86588203","29101577281","0.59","38.55","","45.06","","1.77","49024.92647","49055.41723","Apple Inc.","8.72","344.26","239.32","400","45.62","0.32","49055.41723","23.98","1.16","GP","148.75","36.08","2.41","7.98","14.79","14594180000","14585108878","2.23","36.64","1.06","336.09","","","","",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"pandata":{"last":"334.80","volume":"86588203","pct":"-0.40","netchange":"-1.33","time":"2026-09-18 20:01:00","tag":"after","season":"EST"},"prec":"331.340","version":"16"}}}"#;
    const AM: &str = r#"{"code":0,"msg":"","data":{"usSPY.AM":{"week":[["2026-09-04","759.500","754.050","761.670","749.600","59217653.000"],["2026-09-11","763.150","762.600","763.570","759.960","49652754.000"],["2026-09-18","761.310","761.690","762.000","757.970","65395148.000",{"FHcontent":"\u6bcf\u80a1\u5206\u914d1.889\u7f8e\u5143","hgcgContent":"","cqr":"2026-09-18"}]],"qt":{"usSPY.AM":["delay","\u6807\u666e500\u6307\u6570ETF-SPDR","SPY.AM","761.69","760.71","761.31","65395148","0","0","762.94","80","0","0","0","0","0","0","0","0","762.99","2720","0","0","0","0","0","0","0","0","","2026-09-18 16:00:01","0.98","0.13","762.00","757.97","USD","65395148","49730810811","","","","","","0.53","","","State Street Spdr S&P 500 Etf","","777.42","626.07","-2640","","","","12.57","-0.09","GP-ETF","","","-1.24","0.13","4.14","","","760.47","","","","1031882000",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"pandata":{"last":"762.94","volume":"65395148","pct":"0.16","netchange":"1.25","time":"2026-09-18 20:04:00","tag":"after","season":"EST"},"prec":"757.390","version":"16"}}}"#;
    const BAD: &str = r#"{"code":0,"msg":"","data":{"sh999999":{"week":[],"qt":{"sh999999":[],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"","version":"16"}}}"#;
    const SH_DIVIDEND: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2023-06-16","1713.180","1728.380","1734.000","1711.000","18574.000"],["2023-06-23","1731.000","1713.710","1734.990","1713.010","14231.000"],["2023-06-30","1700.000","1691.000","1708.990","1686.480","20459.000",{"nd":"2022","fh_sh":"259.11","djr":"2023-06-29","cqr":"2023-06-30","FHcontent":"10\u6d3e259.11\u5143"}]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519","1257.12","1266.98","1262.99","24891","12061","12829","1257.12","8","1257.11","2","1257.08","1","1257.06","2","1257.05","2","1257.13","1","1257.24","2","1257.28","1","1258.00","16","1258.28","1","","20260918161436","-9.86","-0.78","1265.88","1256.10","1257.12\/24891\/3135849108","24891","313585","0.20","19.30","","1265.88","1256.10","0.77","15715.03","15715.03","6.25","1393.68","1140.28","1.14","-6","1259.84","17.65","19.09","","","0.08","313584.9108","527.9904","42","   A","GP-A","-6.82","-1.41","4.14","32.41","27.30","1539.98","1151.01","-5.48","-1.23","7.57","1250081601","1250081601","-16.67","-8.89","1250081601","","","-11.22","-0.42","","CNY","0","___D__F__N","1257.00","102",""],"market":["2026-09-20 13:01:58|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"31.390","version":"16"}}}"#;

    fn bars(fixture: &str, symbol: &str) -> Vec<KlineBar> {
        serde_json::from_str::<TencentKlineResponse>(fixture)
            .expect("报文 fixture 应可解析")
            .into_bars(symbol)
    }

    fn q(market: QuoteMarket, code: &str) -> QuoteQuery {
        QuoteQuery {
            market,
            code: code.to_string(),
        }
    }

    #[test]
    fn parses_shanghai_and_shenzhen_week_lines() {
        assert_eq!(
            bars(SH, "sh600519"),
            vec![
                KlineBar::new("2026-09-04", 1258.0),
                KlineBar::new("2026-09-11", 1266.98),
                KlineBar::new("2026-09-18", 1257.12),
            ]
        );
        assert_eq!(
            bars(SZ, "sz000001"),
            vec![
                KlineBar::new("2026-09-04", 11.7),
                KlineBar::new("2026-09-11", 11.61),
                KlineBar::new("2026-09-18", 11.7),
            ]
        );
    }

    #[test]
    fn parses_hong_kong_and_us_week_lines_with_extra_row_element() {
        assert_eq!(
            bars(HK, "hk00700"),
            vec![
                KlineBar::new("2026-09-04", 433.4),
                KlineBar::new("2026-09-11", 426.0),
                KlineBar::new("2026-09-18", 419.0),
            ]
        );
        assert_eq!(
            bars(NQ, "usAAPL.OQ"),
            vec![
                KlineBar::new("2026-09-04", 332.41),
                KlineBar::new("2026-09-11", 337.0),
                KlineBar::new("2026-09-18", 336.13),
            ]
        );
        assert_eq!(
            bars(AM, "usSPY.AM"),
            vec![
                KlineBar::new("2026-09-04", 754.05),
                KlineBar::new("2026-09-11", 762.6),
                KlineBar::new("2026-09-18", 761.69),
            ]
        );
    }

    #[test]
    fn invalid_code_yields_empty_series() {
        assert!(bars(BAD, "sh999999").is_empty());
        assert!(bars(BAD, "sh600519").is_empty());
    }

    #[test]
    fn skips_unusable_rows_without_interrupting_the_series() {
        let json = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[
            ["2026-09-11","1.000","1.100","1.200","1.000","100"],
            ["2026-09-14","-","-","-","-","0"],
            ["","1.000","1.100","1.200","1.000","100"],
            ["2026-09-15","1.000","0","1.200","1.000","100"],
            ["2026-09-16"],
            ["2026-09-18","1.000","1.250","1.300","1.100",100]
        ]}}}"#;
        assert_eq!(
            bars(json, "sh600519"),
            vec![
                KlineBar::new("2026-09-11", 1.1),
                KlineBar::new("2026-09-18", 1.25),
            ]
        );
    }

    #[test]
    fn kline_symbol_pins_each_market_code_form() {
        assert_eq!(kline_symbol(&q(QuoteMarket::Sh, "600519")), "sh600519");
        assert_eq!(kline_symbol(&q(QuoteMarket::Sz, "000001")), "sz000001");
        assert_eq!(kline_symbol(&q(QuoteMarket::Hk, "00700")), "hk00700");
        assert_eq!(kline_symbol(&q(QuoteMarket::Nasdaq, "AAPL")), "usAAPL.OQ");
        assert_eq!(kline_symbol(&q(QuoteMarket::Nyse, "BABA")), "usBABA.N");
        assert_eq!(kline_symbol(&q(QuoteMarket::Amex, "SPY")), "usSPY.AM");
        assert_eq!(QuoteMarket::ALL.len(), 6);
    }

    #[test]
    fn parses_a_share_dividend_row_with_extra_element() {
        assert_eq!(
            bars(SH_DIVIDEND, "sh600519"),
            vec![
                KlineBar::new("2023-06-16", 1728.38),
                KlineBar::new("2023-06-23", 1713.71),
                KlineBar::new("2023-06-30", 1691.0),
            ]
        );
    }

    #[test]
    fn kline_param_keeps_the_empty_unadjusted_segment() {
        assert_eq!(
            kline_param("usAAPL.OQ", "2024-09-19", "2026-09-19", KLINE_COUNT),
            "usAAPL.OQ,week,2024-09-19,2026-09-19,1200,"
        );
    }

    #[test]
    fn response_type_is_fail_closed_on_unexpected_shapes() {
        for shape in [
            "<html>waf blocked</html>",
            r#"{"code":1,"msg":"bad params"}"#,
            r#"{"code":0,"msg":"param error","data":[]}"#,
            r#"{"code":0,"msg":"","data":null}"#,
        ] {
            assert!(
                serde_json::from_str::<TencentKlineResponse>(shape).is_err(),
                "非预期形状应解析失败：{shape}"
            );
        }
    }
}
