//! 新浪场外基金取数单元（ADR-0130 / issue #1564）：批量面行形态判别（普通行 /
//! 货基错位行 / 已终止基金行）、全历史面末点与可信空、fail-closed、请求形态与
//! 批量承载量。报文为取数面实测（2026-09-20 采集）的真实报文（GBK 解码后原样；
//! 全历史 fixture 的 `total_num` 调整为截取行数以保持自洽——真实报文中该值是
//! 全窗口总数，单请求 `num=10000` 恒取全），请求形态经本地 HTTP 服务验证，
//! 不依赖真实网络。

use std::time::Duration;

use crate::bulk::BulkNavPoint;
use crate::http::Pacer;
use crate::sina_fund::{
    SINA_FUND_BATCH_HOSTS, SINA_FUND_BATCH_PATH_PREFIX, SINA_FUND_BATCH_REFERER,
    SINA_FUND_BATCH_SIZE, SINA_FUND_HISTORY_HOSTS, SinaFundNavForm, fetch_fund_nav_history,
    fetch_sina_fund_nav_rows, parse_fund_nav_history, parse_sina_fund_nav_rows,
};

// ---------------------------------------------------------------------------
// 真实报文 fixture（2026-09-20 采集，GBK 解码后原样）
// ---------------------------------------------------------------------------

/// 批量面真实报文（GBK 解码后原样）：普通行（000001）、货基错位行（000198，
/// 万份收益 0.2229 在单位净值位）、已终止普通基金行（002503，末点 2023-09-18 /
/// 1.144 与官方披露逐值一致）、已终止货基行（000659，收益位为空）、查无此码
/// 空值语句（999999）。
const BATCH_BODY: &str = "\
var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";
var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";
var hq_str_f_002503=\"中银腾利混合C,1.144,1.415,1.137,2023-09-18,0.071287\";
var hq_str_f_000659=\"银华活钱宝货币C,,1.233,,2015-01-16,0\";
var hq_str_f_999999=\"\";
";

/// 全历史面普通基金（000001 华夏成长混合A）真实报文截取（3 行 + 自洽 total_num）。
const HISTORY_BODY_NORMAL: &str = r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"1.333","ljjz":"3.906"},{"fbrq":"2026-09-17 00:00:00","jjjz":"1.298","ljjz":"3.871"},{"fbrq":"2026-09-16 00:00:00","jjjz":"1.296","ljjz":"3.869"}],"total_num":"3"}}}"#;

/// 全历史面已终止基金（002503 中银腾利混合C）真实报文截取：末点 2023-09-18 /
/// 1.144 与官方披露、东财三方差值一致（调研 13.4 节）。
const HISTORY_BODY_TERMINATED: &str = r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2023-09-18 00:00:00","jjjz":"1.144","ljjz":"1.415"},{"fbrq":"2023-09-15 00:00:00","jjjz":"1.137","ljjz":"1.408"},{"fbrq":"2023-09-14 00:00:00","jjjz":"1.138","ljjz":"1.409"}],"total_num":"3"}}}"#;

/// 全历史面可信空（货基 000198 / 查无此码 / 参数缺失同形，2026-09-20 实测）。
const HISTORY_BODY_EMPTY: &str =
    r#"{"result":{"status":{"code":0},"data":{"data":[],"total_num":"0"}}}"#;

fn parse_batch(body: &str) -> Vec<crate::sina_fund::SinaFundNavRow> {
    parse_sina_fund_nav_rows(body).expect("真实报文应可解析")
}

// ---------------------------------------------------------------------------
// 批量面行形态判别（ADR-0130 决策 6 / AC：普通基金与货基行的字段差异）
// ---------------------------------------------------------------------------

#[test]
fn batch_fixture_pins_row_forms_names_and_dates() {
    let rows = parse_batch(BATCH_BODY);
    assert_eq!(rows.len(), 4, "查无此码的空值语句不产出行");

    // 普通行：单位净值 / 累计净值 / 前一日单位净值三值齐备，名称与净值日期在场。
    let normal = &rows[0];
    assert_eq!(normal.code, "000001");
    assert_eq!(normal.name, "华夏成长混合A");
    assert_eq!(normal.nav_date, "2026-09-18");
    assert_eq!(
        normal.form,
        SinaFundNavForm::UnitNav {
            unit_nav: 1.333,
            accumulated_nav: Some(3.906),
            prev_unit_nav: Some(1.298),
        }
    );

    // 货基错位行：单位净值位的 0.2229 是万份收益，七日年化 0.824 在累计净值位。
    let money = &rows[1];
    assert_eq!(money.code, "000198");
    assert_eq!(money.name, "天弘余额宝货币");
    assert_eq!(money.nav_date, "2026-09-19");
    assert_eq!(
        money.form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: Some(0.2229),
            seven_day_annual_percent: Some(0.824),
        }
    );

    // 已终止普通基金行：末点净值照常在批量面在场（1.144 / 2023-09-18）。
    let terminated = &rows[2];
    assert_eq!(terminated.code, "002503");
    assert_eq!(
        terminated.form,
        SinaFundNavForm::UnitNav {
            unit_nav: 1.144,
            accumulated_nav: Some(1.415),
            prev_unit_nav: Some(1.137),
        }
    );
    assert_eq!(terminated.nav_date, "2023-09-18");

    // 已终止货基行：收益位为空、七日年化为陈旧值，形态仍判别为货基错位行。
    let terminated_money = &rows[3];
    assert_eq!(terminated_money.code, "000659");
    assert_eq!(
        terminated_money.form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: None,
            seven_day_annual_percent: Some(1.233),
        }
    );
}

/// 货基行的字段错位必须被显式识别，不得当作单位净值直接采信（AC / ADR-0130
/// 决策 6 的守门判据「删除货基判定改写，货基被写成万份收益的测试变红」）：
/// 错位行经 [`crate::sina_fund::SinaFundNavRow::into_nav_point`] 投影不产出
/// 价格点——把形态判别简化掉、把万份收益写进净值的回归在此变红。
#[test]
fn money_fund_misalignment_is_never_trusted_as_unit_nav() {
    let rows = parse_batch(BATCH_BODY);
    let money = rows
        .iter()
        .find(|row| row.code == "000198")
        .expect("货基行应在场")
        .clone();
    assert!(money.into_nav_point().is_none(), "货基错位行不产出价格点");

    // 纯货基批量（活钱宝 + 余额宝，含已终止形态）产不出任何价格点；负万份
    // 收益按事实保留（收益不是价格意义上的非正无效值），同样不产出价格点。
    let money_only = parse_batch(
        "var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";\nvar hq_str_f_000659=\"银华活钱宝货币C,,1.233,,2015-01-16,0\";\nvar hq_str_f_000658=\"负收益货基,-0.1234,0.5,,2026-09-19,10\";\n",
    );
    assert_eq!(
        money_only[2].form,
        SinaFundNavForm::MoneyYield {
            gain_per_wan: Some(-0.1234),
            seven_day_annual_percent: Some(0.5)
        }
    );
    let points: Vec<Option<BulkNavPoint>> = money_only
        .into_iter()
        .map(|row| row.into_nav_point())
        .collect();
    assert_eq!(points, vec![None, None, None]);
}

#[test]
fn into_nav_point_projects_unit_nav_rows_only() {
    let rows = parse_batch(BATCH_BODY);
    let normal = rows
        .iter()
        .find(|row| row.code == "000001")
        .expect("普通行应在场")
        .clone();
    assert_eq!(
        normal.into_nav_point(),
        Some(BulkNavPoint {
            date: "2026-09-18".into(),
            nav: 1.333,
        }),
        "普通行投影「净值日期 + 单位净值」"
    );
}

#[test]
fn invalid_batch_rows_are_skipped_as_gaps_not_failures() {
    // 名称空 / 字段数不足 / 日期缺失 / 普通行单位净值非正：逐行跳过（该只按
    // 缺口走逐只通道），其余行照常解析；全部代码查无此码（空值语句）是可信零。
    let body = "\
var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";
var hq_str_f_000002=\",1.0,1.0,1.0,2026-09-18,1\";
var hq_str_f_000003=\"短字段,1.0,1.0,1.0\";
var hq_str_f_000004=\"无日期,1.0,1.0,1.0,,1\";
var hq_str_f_000005=\"零净值,0,1.0,1.0,2026-09-18,1\";
var hq_str_f_000006=\"负净值,-1.5,1.0,1.0,2026-09-18,1\";
var hq_str_f_999999=\"\";
";
    let rows = parse_batch(body);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].code, "000001");

    assert!(parse_batch("var hq_str_f_999999=\"\";\nvar hq_str_f_888888=\"\";\n").is_empty());
}

// ---------------------------------------------------------------------------
// 批量面 fail-closed：被拦截 / 非预期形状不退化为「零覆盖」
// ---------------------------------------------------------------------------

#[test]
fn intercepted_batch_response_fails_closed() {
    // 缺 Referer 的 403 Forbidden 文本、风控 HTML、空体、无语句的杂讯：报错，
    // 不当「零覆盖」（零覆盖会把整场同步静默降级成逐只请求）。
    for body in [
        "",
        "   \n\t",
        "Forbidden",
        "<html><body>risk control</body></html>",
        "ErrCode=-999",
    ] {
        assert!(
            parse_sina_fund_nav_rows(body).is_err(),
            "应 fail-closed：{body:?}"
        );
    }
}

#[test]
fn batch_statement_scanner_tolerates_line_shapes() {
    // 多行 / 同行多条 / 无 `var ` 前缀的键值形态都可扫出（标记扫描不依赖行切分）。
    let body = concat!(
        "var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";",
        "var hq_str_f_000198=\"天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46\";",
    );
    assert_eq!(parse_batch(body).len(), 2);
    let no_var = "hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,2026-09-18,24.0598\";\n";
    assert_eq!(parse_batch(no_var).len(), 1);
}

// ---------------------------------------------------------------------------
// 单只全历史面：末点、可信空、行纪律与 fail-closed
// ---------------------------------------------------------------------------

#[test]
fn history_fixture_reaches_last_point_of_terminated_fund() {
    // AC：已终止基金可取到末点——002503 末点 2023-09-18 / 1.144 与官方披露逐值一致。
    let points = parse_fund_nav_history(HISTORY_BODY_TERMINATED).expect("已终止基金应可解析");
    assert_eq!(points.len(), 3);
    // 末点 = 净值日期最大的点（wire 序先新后旧，末点在首行；按日期取最大
    // 与序无关）。
    let last = points
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .expect("末点在场");
    assert_eq!(last.date, "2023-09-18");
    assert_eq!(last.nav, 1.144);

    // 普通基金：wire 序（先新后旧）原样产出，日期剥掉 ` 00:00:00` 后缀。
    let points = parse_fund_nav_history(HISTORY_BODY_NORMAL).expect("普通基金应可解析");
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].date, "2026-09-18");
    assert_eq!(points[0].nav, 1.333);
    assert_eq!(points[2].date, "2026-09-16");
    assert_eq!(points[2].nav, 1.296);
}

#[test]
fn history_empty_payload_is_trusted_empty() {
    // 货基 / 查无此码 / 参数缺失同形的空数组是可信空（≠「查无此码」结论，
    // 存在性归官方披露面——见模块文档的空序列语义）。
    assert!(
        parse_fund_nav_history(HISTORY_BODY_EMPTY)
            .expect("可信空应返回空序列")
            .is_empty()
    );
}

#[test]
fn history_row_discipline_filters_invalid_rows() {
    // 无效行（日期缺失 / 日期非 ISO / 净值非正）静默过滤，有效行保留；
    // 累计净值列在场不消费（单位净值即价格，ADR-0038 决策 3 / ADR-0126）。
    let body = r#"{"result":{"status":{"code":0},"data":{"data":[
        {"fbrq":"2026-09-18 00:00:00","jjjz":"1.333","ljjz":"3.906"},
        {"fbrq":"","jjjz":"1.0"},
        {"fbrq":"not-a-date","jjjz":"1.0"},
        {"fbrq":"2026-09-17 00:00:00","jjjz":"0"},
        {"fbrq":"2026-09-16 00:00:00","jjjz":"-1.0"}
    ],"total_num":"1"}}}"#;
    let points = parse_fund_nav_history(body).expect("有有效行即产出");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, "2026-09-18");

    // 完整性锚在原始行数而非过滤后的有效行：声明 5 行全部在场、其中坏行被
    // 行纪律过滤，不构成「窗口不完整」（行纪律与窗口完整性两件事不打架）。
    let body = r#"{"result":{"status":{"code":0},"data":{"data":[
        {"fbrq":"2026-09-18 00:00:00","jjjz":"1.333"},
        {"fbrq":"","jjjz":"1.0"},
        {"fbrq":"not-a-date","jjjz":"1.0"},
        {"fbrq":"2026-09-17 00:00:00","jjjz":"0"},
        {"fbrq":"2026-09-16 00:00:00","jjjz":"-1.0"}
    ],"total_num":"5"}}}"#;
    let points = parse_fund_nav_history(body).expect("原始行数取全即完整");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, "2026-09-18");
}

#[test]
fn history_untrusted_shapes_fail_closed() {
    // 非 JSON（风控 HTML）、缺 result、缺 data、缺 data.data 数组、status.code
    // 非 0、声明总数未取全、有行但全部未通过解析纪律：一律报错，不产出半截序列。
    for body in [
        "<html><body>risk control</body></html>",
        "",
        "{}",
        r#"{"result":null}"#,
        r#"{"result":{"status":{"code":0}}}"#,
        r#"{"result":{"status":{"code":0},"data":{}}}"#,
        r#"{"result":{"status":{"code":-1},"data":{"data":[],"total_num":"0"}}}"#,
        r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"1.333"}],"total_num":"6010"}}}"#,
        r#"{"result":{"status":{"code":0},"data":{"data":[{"fbrq":"2026-09-18 00:00:00","jjjz":"x"}],"total_num":"1"}}}"#,
    ] {
        assert!(
            parse_fund_nav_history(body).is_err(),
            "应 fail-closed：{body:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 请求形态与批量承载量（本地 HTTP 服务；实测 ~850 只 / URI ≈ 8 KB 上限）
// ---------------------------------------------------------------------------

#[test]
fn fetch_pins_request_shape_referer_and_batch_capacity() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let gbk = encoding_rs::GBK.encode(BATCH_BODY).0.into_owned();

    // 小批量四只：一次请求，请求目标是路径段 `list=f_<逗号串>`，必须携带
    // 新浪财经站 Referer（缺省 403）。
    let codes: Vec<String> = ["000001", "000198", "002503", "999999"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    let (url, requests) = crate::tests::spawn_header_capture_server(gbk.clone());
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &codes,
    ))
    .expect("假响应应解析成功");
    assert_eq!(rows.len(), 4, "假响应含四只非空行");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "一批一次请求");
    assert_eq!(
        request_target(&captured[0]),
        "/list=f_000001,f_000198,f_002503,f_999999",
        "请求形态：路径段 list=f_<代码逗号串>"
    );
    assert!(
        captured[0].to_ascii_lowercase().contains(&format!(
            "referer: {}",
            SINA_FUND_BATCH_REFERER.to_ascii_lowercase()
        )),
        "批量面必须携带 Referer（缺省 403）：{}",
        captured[0]
    );

    // 批量承载量：超过单请求承载量的代码分多次请求，单请求键数不超过常量；
    // 最坏请求行（f_ 键每只 10 字节）须低于实测 ~8.1 KB 的 431 触发线。
    let many: Vec<String> = (0..SINA_FUND_BATCH_SIZE + 1)
        .map(|i| format!("{i:06}"))
        .collect();
    let (url, requests) = crate::tests::spawn_header_capture_server(gbk);
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &many,
    ))
    .expect("分批请求应成功");
    assert_eq!(rows.len(), 8, "两批各回同一份 fixture 的四行");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 2, "超过承载量应分两次请求");
    assert_eq!(
        key_count(request_target(&captured[0])),
        SINA_FUND_BATCH_SIZE
    );
    assert_eq!(key_count(request_target(&captured[1])), 1);
    let worst_request_line = SINA_FUND_BATCH_PATH_PREFIX.len() + SINA_FUND_BATCH_SIZE * 10;
    assert!(
        worst_request_line < 8_100,
        "承载量最坏请求行 {worst_request_line} 应低于实测 431 触发线（~8.1 KB）"
    );
}

#[test]
fn fetch_batch_decodes_gbk_and_fails_closed_on_intercepted_response() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let codes = vec!["000001".to_string()];

    // GBK 报文正确解码出中文权威名称（未按 UTF-8 误解）。
    let gbk = encoding_rs::GBK.encode(BATCH_BODY).0.into_owned();
    let (url, _) = crate::tests::spawn_header_capture_server(gbk);
    let rows = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
        &client,
        &mut pacer,
        &[url.as_str()],
        &codes,
    ))
    .expect("GBK 报文应解析成功");
    assert_eq!(rows[0].name, "华夏成长混合A");

    // 被风控拦截（200 + HTML）与非法 GBK 字节：报错，不退化为空序列。
    for body in [
        b"<html><body>risk control</body></html>".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x01],
    ] {
        let (url, _) = crate::tests::spawn_header_capture_server(body);
        let error = tauri::async_runtime::block_on(fetch_sina_fund_nav_rows(
            &client,
            &mut pacer,
            &[url.as_str()],
            &codes,
        ))
        .expect_err("被拦截响应应报错");
        assert!(
            error
                .to_string()
                .contains("新浪场外基金批量净值响应不可解析"),
            "实际 {error:?}"
        );
    }
}

#[test]
fn fetch_pins_history_request_shape() {
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);
    let (url, requests) = crate::tests::spawn_header_capture_server(HISTORY_BODY_TERMINATED);

    let points = tauri::async_runtime::block_on(fetch_fund_nav_history(
        &client,
        &mut pacer,
        &[url.as_str()],
        "002503",
        None,
        None,
    ))
    .expect("全历史假响应应解析成功");
    assert_eq!(
        points
            .iter()
            .max_by(|a, b| a.date.cmp(&b.date))
            .expect("末点在场")
            .date,
        "2023-09-18"
    );

    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 1, "单只一次请求取全");
    let target = request_target(&captured[0]);
    assert!(
        target.starts_with("/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav?"),
        "请求目标应是全历史接口路径：{target}"
    );
    for expected in [
        "symbol=002503",
        "datefrom=",
        "dateto=",
        "page=1",
        "num=10000",
    ] {
        assert!(target.contains(expected), "请求参数缺 {expected}：{target}");
    }
}

/// 生产主机、端点形态与 Referer 的接线钉（删除即红）：换源或改端点形态须显式
/// 改此处（#1565 / #1566 接线时按消费面再导出）。
#[test]
fn production_hosts_and_path_pin_to_sina_fund_endpoints() {
    assert_eq!(SINA_FUND_BATCH_HOSTS, ["https://hq.sinajs.cn"]);
    assert_eq!(SINA_FUND_BATCH_PATH_PREFIX, "/list=");
    assert_eq!(SINA_FUND_BATCH_REFERER, "https://finance.sina.com.cn/");
    assert_eq!(
        SINA_FUND_HISTORY_HOSTS,
        ["https://stock.finance.sina.com.cn"]
    );
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

/// 请求目标里携带的查询键数（`/list=f_000001,f_000198` → 2）。
fn key_count(target: &str) -> usize {
    let keys = target
        .strip_prefix(SINA_FUND_BATCH_PATH_PREFIX)
        .unwrap_or(target);
    if keys.is_empty() {
        0
    } else {
        keys.split(',').count()
    }
}
