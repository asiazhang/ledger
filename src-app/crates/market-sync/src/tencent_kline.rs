//! 腾讯周线 K 线取数单元（ADR-0130 决策 2 / issue #1559 接线；#1906 起改周粒度）。
//!
//! 历史补全的场内标的由腾讯承担周线取数（新浪 K 线对港美返回空，是分链路
//! 选源里最硬的一条约束）。#1906 起车道由场内日 K 换为场内**周 K**（ADR-0019
//! 2026-10-07 修订）：周线单请求整段取回（研究文档 §14.3 实测 ≥1200 周 ≈ 23.7
//! 年），取数形态与周采样同构——周行收盘 = 该周最后一个有报价交易日的价格、
//! 周行日期 = 该交易日本身（非恒周五），本地不再做日线降采样。本单元只做
//! 「取数 + 解析」：把「市场 + 代码」与区间 / 根数参数变成腾讯查询键与请求，
//! 再解析出周线 [`KlineBar`] 序列；窗口裁剪、周采样、落库与队列编排归既有
//! 链路（窗口深度语义由消费方本地裁剪表达，与新浪单只全历史面同式）。
//!
//! 三块来源侧事实收口在本模块（ADR-0130 决策 2「各来源自己的代码形态」）：
//! - **查询键**（[`kline_symbol`]）：沪深 `sh`/`sz` 前缀、港 `hk` 前缀、美股三
//!   市场交易所后缀 `.OQ`/`.N`/`.AM`。后缀是取整段序列的必要条件——实测带区间时
//!   裸 `usAAPL` 取不回 K 线（`day: []`；空区间形态只回两条残缺行：一条 2011 年
//!   遗留 + 最新一日），带后缀的 `usAAPL.OQ` 才回整段；后缀与既有市场闭集三值
//!   一一对应（ADR-0130 决策 4）。
//! - **请求形态**（[`kline_param`]）：`{查询键},week,{起始},{结束},{根数},`——末段
//!   留空即**不复权**（响应键 `week`；复权时为 `qfqweek`/`hfqweek`）。历史库存真实
//!   成交价，前复权会随后续除权整体重算（与既有口径一致）。
//! - **报文布局**：`data[查询键].week` 每行 `[日期, 开, 收, 高, 低, 量, …]`——**收盘
//!   价在下标 2 而非 1**（1 是开盘价；行形态与日线同，研究文档 §14.3 实测）。行尾
//!   在**分红 / 回购日多带一个对象元素**（日线实测沪深港美都有，周线行形态相同）
//!   ——行可能是 6 或 7 字段，**不按市场分支**，解析统一只取前 6 个下标。
//!
//! **当周行是活动行**：周内即有、随行情更新（研究文档 §14.3 港美当日行实测），
//! 与「当周点位由现价刷新落采样、整周覆盖幂等」（ADR-0122 决策 2）语义相容，
//! 由整周覆盖幂等照旧吸收，无新增口径。
//!
//! 预期外的形状分两类处置：
//! - **无效代码**（`sh999999` / `usZZZZ.OQ`）返回 `data[键].week: []`（`code` 仍为 0）
//!   ——空序列而非错误，单只无效不中断补全（「停牌 / 无效样本优雅降级」语义）。
//! - **被拦截页 / 契约漂移**：非 JSON（风控页）由请求层 JSON 通道判失败并走既有长
//!   冷却重试（ADR-0121 决策 5），最终报错；合法 JSON 但 `data` 缺失或非对象同样
//!   是解析失败（[`TencentKlineResponse`] 要求 `data` 为对象），不退化为「无数据」。
//!   超限参数被拒的形态是 `data:[]`（研究文档 §14.2 实测 `param error` 体）——
//!   会被判解析失败而非静默空，正是不依赖「恰好 N 根」的理由。
//!
//! 根数与区间都是**上限**，本单元不依赖「收到恰好 `count` 行」：服务端按区间裁剪、
//! 超额根数砍旧头保近尾，也可能把超额根数压缩返回。返回比请求少不报错、不补偿，
//! 按返回行照常解析——周点只是少几根，缺的那段靠派生队列在下一窗口自然重进
//! （fail-closed 自愈，ADR-0122 修订记录）。

use std::collections::HashMap;

use serde::Deserialize;

use ledger_infra::error::Result;
use ledger_investment::QuoteMarket;

use super::channels::QuoteQuery;
use super::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts};

/// 生产主机（腾讯行情 K 线站；ADR-0130 决策 2）。入口按参数收主机，测试经本地
/// HTTP 服务注入假响应。
pub(super) const TENCENT_KLINE_HOSTS: &[&str] = &["https://web.ifzq.gtimg.cn"];

/// 周线 K 线接口路径。
pub(super) const TENCENT_KLINE_PATH: &str = "/appstock/app/fqkline/get";

/// 周期参数：只取周线（周行即周采样形态，#1906 起本地不做日线降采样）。
const PERIOD_WEEK: &str = "week";

/// 一次请求的根数上限：**1200 周**（研究文档 §14.3 实测可用，≈23.7 年——1200 周
/// 帽触顶后首行仍在 2003-01；更高根数未探）。区间与根数都是上限，服务端可能
/// 返回更少，本单元按返回照常解析（不做补偿）。
pub(super) const KLINE_COUNT: u32 = 1200;

/// 请求区间起点（`YYYY-MM-DD`）：取深于数据源有效起点（研究文档 §14.3：beg
/// `2000-01-01`、count 1200 帽触顶后首行仍在 2003-01-10）——区间与根数双上限
/// 下整段周线一次取回，**回填窗口深度不在此表达**：首刷与存量深回填的窗口
/// （近两年 / 首笔持仓周，#1906）由消费方本地裁剪（与新浪单只全历史面同式）。
pub(super) const KLINE_BEG: &str = "2000-01-01";

/// 查询单元 → 腾讯 K 线查询键（来源侧代码形态的唯一构造点，ADR-0130 决策 2 /
/// issue #1673 全函数化）。`query.market` 是可路由市场子集 [`QuoteMarket`]，
/// `query.code` 为响应回显形态的裸代码（如 `600519` / `00700`，已去市场后缀）。
/// 六值全部可构造键——**全函数**：「市场构造不出 K 线键」的运行时兜底在类型上
/// 不可表达。交易所后缀是源请求词汇（模块本地）：美股三市场必须带交易所后缀
/// 才能取到整段序列（裸 `usAAPL` 取不回 K 线），后缀与市场闭集三值一一对应
///（ADR-0130 决策 4），由本单点承担。
pub(super) fn kline_symbol(query: &QuoteQuery) -> String {
    match query.market {
        QuoteMarket::Sh | QuoteMarket::Sz | QuoteMarket::Hk => {
            // 沪深港同一个形态：市场前缀 + 裸代码（`sh600519` / `sz000001` / `hk00700`）。
            format!("{}{}", query.market.as_str(), query.code)
        }
        QuoteMarket::Nasdaq => format!("us{}.OQ", query.code),
        QuoteMarket::Nyse => format!("us{}.N", query.code),
        QuoteMarket::Amex => format!("us{}.AM", query.code),
    }
}

/// 请求参数串（`param` 查询参数的值）：`{查询键},week,{起始},{结束},{根数},`。
/// 末段留空 = 不复权（响应键 `week`）；**这个尾逗号不能省**——实测少一段会被
/// 服务端当作参数不完整，只回 `version` 而无 `week` 键。
fn kline_param(symbol: &str, beg: &str, end: &str, count: u32) -> String {
    format!("{symbol},{PERIOD_WEEK},{beg},{end},{count},")
}

/// 周 K 接口响应：`data` 以查询键为键（无效代码同样带键，值为空 `week` 数组）。
///
/// `data` 无缺省值——缺失或非对象（参数形态漂移 / 接口变更 / 服务端错误体）即
/// 解析失败，不退化成「无周线数据」：契约漂移必须可见，而不是让每只标的静默
/// 变成「无数据」。
#[derive(Debug, Deserialize)]
pub(super) struct TencentKlineResponse {
    data: HashMap<String, TencentSymbolKline>,
}

#[derive(Debug, Deserialize)]
struct TencentSymbolKline {
    /// 不复权周线行。缺 `week` 键按空序列处理（无效代码 / 无周线能力的标的），
    /// 这是「无效代码返回空序列而非错误」的落点。
    #[serde(default)]
    week: Option<Vec<Vec<serde_json::Value>>>,
}

impl TencentKlineResponse {
    /// 取查询键对应的周线序列（键缺失或空数组 → 空序列）。
    pub(super) fn into_bars(self, symbol: &str) -> Vec<KlineBar> {
        self.data
            .get(symbol)
            .and_then(|payload| payload.week.as_deref())
            .map(parse_week_rows)
            .unwrap_or_default()
    }
}

/// 周线行 → [`KlineBar`]：`[日期, 开, 收, 高, 低, 量, …]`，收盘价取**下标 2**
///（下标 1 是开盘价）；第 7 个对象元素与更靠后的字段一律忽略。无效行
///（缺日期 / 收盘价非数值或 ≤ 0，如停牌）静默跳过、不中断整段（与既有 K 线
/// 解析同款语义）。
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

/// JSON 字段的宽容数值取值：字符串与数值都接受——腾讯 K 线行的价格恒为字符串，
/// 而美股成交量实测出现过裸数值（两者可能出现在同一行）。
fn value_as_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::String(text) => text.trim().parse().ok(),
        serde_json::Value::Number(number) => number.as_f64(),
        _ => None,
    }
}

/// 拉取单个查询键的周线。区间 `beg`/`end`（`YYYY-MM-DD`）与根数 `count` 都是上限，
/// 服务端可压缩返回更少；`hosts` 供测试注入本地服务，生产传 [`TENCENT_KLINE_HOSTS`]。
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

    // -----------------------------------------------------------------------
    // 报文 fixture：2026-09 真实日线报文形态移植（研究文档 §14.3 实测：周线行
    // 形态与日线同、响应键 `week`），各 trim 到 3 根周线、行日期取各周最后一个
    // 交易日（周五），其余字段如 `qt` / `version` 原样保留——旁路字段的存在本身
    // 就是「未知字段被忽略」的覆盖。请求形态：
    // `param={查询键},week,2026-09-01,2026-09-19,3,`。
    // -----------------------------------------------------------------------
    const SH: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2026-09-04","1273.930","1258.000","1274.980","1254.100","26235.000"],["2026-09-11","1257.980","1266.980","1267.600","1254.000","17554.000"],["2026-09-18","1262.990","1257.120","1265.880","1256.100","24891.000"]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519","1257.12","1266.98","1262.99","24891","12061","12829","1257.12","8","1257.11","2","1257.08","1","1257.06","2","1257.05","2","1257.13","1","1257.24","2","1257.28","1","1258.00","16","1258.28","1","","20260918161436","-9.86","-0.78","1265.88","1256.10","1257.12\/24891\/3135849108","24891","313585","0.20","19.30","","1265.88","1256.10","0.77","15715.03","15715.03","6.25","1393.68","1140.28","1.14","-6","1259.84","17.65","19.09","","","0.08","313584.9108","527.9904","42","   A","GP-A","-6.82","-1.41","4.14","32.41","27.30","1539.98","1151.01","-5.48","-1.23","7.57","1250081601","1250081601","-16.67","-8.89","1250081601","","","-11.22","-0.42","","CNY","0","___D__F__N","1257.00","102",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"1272.750","version":"16"}}}"#;
    const SZ: &str = r#"{"code":0,"msg":"","data":{"sz000001":{"week":[["2026-09-04","11.800","11.700","11.840","11.570","949626.000"],["2026-09-11","11.680","11.610","11.740","11.570","691925.000"],["2026-09-18","11.590","11.700","11.820","11.560","853038.000"]],"qt":{"sz000001":["51","\u5e73\u5b89\u94f6\u884c","000001","11.70","11.61","11.59","853038","452206","400831","11.70","7029","11.69","2100","11.68","2873","11.67","1086","11.66","1423","11.71","1849","11.72","1006","11.73","1250","11.74","4120","11.75","4537","","20260918161427","0.09","0.78","11.82","11.56","11.70\/853038\/999918140","853038","99992","0.44","5.22","","11.82","11.56","2.24","2270.47","2270.49","0.48","12.77","10.45","1.07","1749","11.72","4.42","5.33","","","0.18","99991.8140","19.4220","166","   A","GP-A","5.88","-0.34","5.09","7.93","0.72","12.08","9.99","-1.60","2.54","14.37","19405684991","19405918198","6.41","4.46","19405684991","","","8.19","-0.17","","CNY","0","","11.80","-13699",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"11.820","version":"16"}}}"#;
    const HK: &str = r#"{"code":0,"msg":"","data":{"hk00700":{"week":[["2026-09-04","438.800","433.400","438.800","432.600","12686315.000",{"cqr":"2026-09-04","FHcontent":"","HGcontent":"\u56de\u8d2d23.10\u4e07\u80a1\uff0c\u5747\u4ef7434.844\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}],["2026-09-11","426.200","426.000","431.000","425.000","16185147.000",{"cqr":"2026-09-11","FHcontent":"","HGcontent":"\u56de\u8d2d23.50\u4e07\u80a1\uff0c\u5747\u4ef7426.707\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}],["2026-09-18","428.000","419.000","430.400","419.000","28796138.000",{"cqr":"2026-09-18","FHcontent":"","HGcontent":"\u56de\u8d2d23.70\u4e07\u80a1\uff0c\u5747\u4ef7423.851\u6e2f\u5143","paixiri":"","hgcgContent":"","ggContent":""}]],"qt":{"hk00700":["100","\u817e\u8baf\u63a7\u80a1","00700","419.000","426.000","428.000","28796138.0","0","0","419.000","0","0","0","0","0","0","0","0","419.000","0","0","0","0","0","0","0","0","0","28796138.0","2026\/09\/18 16:08:32","-7.000","-1.64","430.400","419.000","419.000","28796138.0","12180786280.956","0","15.31","","0","0","2.68","38108.4103","38108.4103","TENCENT","1.27","677.700","411.000","1.85","-25.88","0","0","0","0","0","14.05","2.93","0.32","100","-29.43","-2.19","GP","20.41","11.00","-5.37","-8.32","-0.57","9095085993.00","9095085993.00","14.50","5.313","423.001","-29.78","HKD","1","50"],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"prec":"438.800","vcm":"","version":"16"}}}"#;
    const NQ: &str = r#"{"code":0,"msg":"","data":{"usAAPL.OQ":{"week":[["2026-09-04","332.530","332.410","335.480","330.700","35981000.000"],["2026-09-11","334.770","337.000","338.340","330.180","36700225.000"],["2026-09-18","337.910","336.130","338.490","332.530","86588203.000"]],"qt":{"usAAPL.OQ":["delay","\u82f9\u679c","AAPL.OQ","336.13","337.00","337.91","86588203","0","0","334.75","440","0","0","0","0","0","0","0","0","334.88","40","0","0","0","0","0","0","0","0","","2026-09-18 16:00:02","-0.87","-0.26","338.49","332.53","USD","86588203","29101577281","0.59","38.55","","45.06","","1.77","49024.92647","49055.41723","Apple Inc.","8.72","344.26","239.32","400","45.62","0.32","49055.41723","23.98","1.16","GP","148.75","36.08","2.41","7.98","14.79","14594180000","14585108878","2.23","36.64","1.06","336.09","","","","",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"pandata":{"last":"334.80","volume":"86588203","pct":"-0.40","netchange":"-1.33","time":"2026-09-18 20:01:00","tag":"after","season":"EST"},"prec":"331.340","version":"16"}}}"#;
    const AM: &str = r#"{"code":0,"msg":"","data":{"usSPY.AM":{"week":[["2026-09-04","759.500","754.050","761.670","749.600","59217653.000"],["2026-09-11","763.150","762.600","763.570","759.960","49652754.000"],["2026-09-18","761.310","761.690","762.000","757.970","65395148.000",{"FHcontent":"\u6bcf\u80a1\u5206\u914d1.889\u7f8e\u5143","hgcgContent":"","cqr":"2026-09-18"}]],"qt":{"usSPY.AM":["delay","\u6807\u666e500\u6307\u6570ETF-SPDR","SPY.AM","761.69","760.71","761.31","65395148","0","0","762.94","80","0","0","0","0","0","0","0","0","762.99","2720","0","0","0","0","0","0","0","0","","2026-09-18 16:00:01","0.98","0.13","762.00","757.97","USD","65395148","49730810811","","","","","","0.53","","","State Street Spdr S&P 500 Etf","","777.42","626.07","-2640","","","","12.57","-0.09","GP-ETF","","","-1.24","0.13","4.14","","","760.47","","","","1031882000",""],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"pandata":{"last":"762.94","volume":"65395148","pct":"0.16","netchange":"1.25","time":"2026-09-18 20:04:00","tag":"after","season":"EST"},"prec":"757.390","version":"16"}}}"#;
    const BAD: &str = r#"{"code":0,"msg":"","data":{"sh999999":{"week":[],"qt":{"sh999999":[],"market":["2026-09-20 12:43:40|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"","version":"16"}}}"#;
    /// 沪深分红周行（日线实测形态移植：`sh600519` 除息周行尾多带分红对象元素
    /// ——行都可能是 6 或 7 字段，不是按市场分行）。
    const SH_DIVIDEND: &str = r#"{"code":0,"msg":"","data":{"sh600519":{"week":[["2023-06-16","1713.180","1728.380","1734.000","1711.000","18574.000"],["2023-06-23","1731.000","1713.710","1734.990","1713.010","14231.000"],["2023-06-30","1700.000","1691.000","1708.990","1686.480","20459.000",{"nd":"2022","fh_sh":"259.11","djr":"2023-06-29","cqr":"2023-06-30","FHcontent":"10\u6d3e259.11\u5143"}]],"qt":{"sh600519":["1","\u8d35\u5dde\u8305\u53f0","600519","1257.12","1266.98","1262.99","24891","12061","12829","1257.12","8","1257.11","2","1257.08","1","1257.06","2","1257.05","2","1257.13","1","1257.24","2","1257.28","1","1258.00","16","1258.28","1","","20260918161436","-9.86","-0.78","1265.88","1256.10","1257.12\/24891\/3135849108","24891","313585","0.20","19.30","","1265.88","1256.10","0.77","15715.03","15715.03","6.25","1393.68","1140.28","1.14","-6","1259.84","17.65","19.09","","","0.08","313584.9108","527.9904","42","   A","GP-A","-6.82","-1.41","4.14","32.41","27.30","1539.98","1151.01","-5.48","-1.23","7.57","1250081601","1250081601","-16.67","-8.89","1250081601","","","-11.22","-0.42","","CNY","0","___D__F__N","1257.00","102",""],"market":["2026-09-20 13:01:58|HK_close_\u5df2\u4f11\u5e02|SH_close_\u5df2\u4f11\u5e02|SZ_close_\u5df2\u4f11\u5e02|US_close_\u5df2\u4f11\u5e02|SQ_close_\u5df2\u4f11\u5e02|DS_close_\u5df2\u4f11\u5e02|ZS_close_\u5df2\u4f11\u5e02|NEWSH_close_\u5df2\u4f11\u5e02|NEWSZ_close_\u5df2\u4f11\u5e02|NEWHK_close_\u5df2\u4f11\u5e02|NEWUS_close_\u5df2\u4f11\u5e02|REPO_close_\u5df2\u4f11\u5e02|UK_close_\u5df2\u4f11\u5e02|KCB_close_\u5df2\u4f11\u5e02|HSZB_close_\u5df2\u4f11\u5e02|IT_close_\u5df2\u4f11\u5e02|MY_close_\u5df2\u4f11\u5e02|EU_close_\u5df2\u4f11\u5e02|AH_close_\u5df2\u4f11\u5e02|DE_close_\u5df2\u4f11\u5e02|JW_close_\u5df2\u4f11\u5e02|CYB_close_\u5df2\u4f11\u5e02|USA_close_\u5df2\u4f11\u5e02|USB_close_\u5df2\u4f11\u5e02|ZQ_close_\u5df2\u4f11\u5e02"]},"mx_price":{"mx":[],"price":[]},"prec":"31.390","version":"16"}}}"#;

    /// fixture → 解析结果（解析路径与生产同一条：serde 反序列化 + 行解析）。
    fn bars(fixture: &str, symbol: &str) -> Vec<KlineBar> {
        serde_json::from_str::<TencentKlineResponse>(fixture)
            .expect("报文 fixture 应可解析")
            .into_bars(symbol)
    }

    /// 「市场 + 代码」查询单元的测试构造（市场字符串经闭集解析，测试侧词汇钉住）。
    fn q(market: QuoteMarket, code: &str) -> QuoteQuery {
        QuoteQuery {
            market,
            code: code.to_string(),
        }
    }

    /// 沪深周线解析各有一份报文 fixture：收盘价取**下标 2**（下标 1 是开盘价）
    /// ——钉值里收盘价与同行开盘价互异，把下标改回 1 本断言即红。
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

    /// 港美周线解析（各一份报文 fixture）：行在分红 / 回购日多带第 7 个对象元素
    ///（解析只取前 6 个下标、多余字段忽略）；美股查询键带交易所后缀。
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

    /// 无效代码返回空序列而非错误（`code` 仍为 0、`week` 为空数组）——
    /// 单只无效不中断补全；响应里没有该查询键同样空序列。
    #[test]
    fn invalid_code_yields_empty_series() {
        assert!(bars(BAD, "sh999999").is_empty());
        assert!(bars(BAD, "sh600519").is_empty());
    }

    /// 个别坏行（停牌 `-` / 缺字段 / 零价 / 无交易日）跳过，整段不中断；数值形态的
    /// 量与字符串形态的价格同处一行（美股实测形态）照常解析。
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

    /// 查询键形态（来源侧代码形态的唯一构造点）：沪深港为市场前缀 + 裸代码，美股
    /// 三市场必须带交易所后缀（裸 `usAAPL` 实测只回两条残缺行，后缀是取整段序列
    /// 的必要条件）。查询单元市场是可路由子集（issue #1673），六值全函数逐值钉形。
    #[test]
    fn kline_symbol_pins_each_market_code_form() {
        assert_eq!(kline_symbol(&q(QuoteMarket::Sh, "600519")), "sh600519");
        assert_eq!(kline_symbol(&q(QuoteMarket::Sz, "000001")), "sz000001");
        assert_eq!(kline_symbol(&q(QuoteMarket::Hk, "00700")), "hk00700");
        assert_eq!(kline_symbol(&q(QuoteMarket::Nasdaq, "AAPL")), "usAAPL.OQ");
        assert_eq!(kline_symbol(&q(QuoteMarket::Nyse, "BABA")), "usBABA.N");
        assert_eq!(kline_symbol(&q(QuoteMarket::Amex, "SPY")), "usSPY.AM");
        // 可路由子集全量遍历：六个可路由市场全部可构造键（全函数，无 None 臂）。
        assert_eq!(QuoteMarket::ALL.len(), 6);
    }

    /// 沪深行同样会在分红日多带第 7 个对象元素（`sh600519` 2023-06-30 除息日所在
    /// 周行）——字段布局差异不是「沪深恒 6 / 港美可 7」，解析不按市场分支。
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

    /// 请求参数串：`week` 周期 + 末段留空（不复权 → 响应键 `week`）的尾逗号不能
    /// 省——实测少一段服务端只回 `version`、无 `week` 键；删掉尾逗号本断言即红。
    /// 根数上限 1200（研究文档 §14.3 实测 ≈23.7 年）。
    #[test]
    fn kline_param_keeps_the_empty_unadjusted_segment() {
        assert_eq!(
            kline_param("usAAPL.OQ", "2024-09-19", "2026-09-19", KLINE_COUNT),
            "usAAPL.OQ,week,2024-09-19,2026-09-19,1200,"
        );
    }

    /// 响应类型 fail-closed：非 JSON（风控页）与合法 JSON 但 `data` 缺失 / 非对象
    /// 都是解析失败，不退化成「无周线数据」（契约漂移必须可见）。
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
