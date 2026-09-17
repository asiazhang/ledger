//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算与增量同步行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。
//!
//! #256 按行为主题拆为子模块（纯移动）：
//! - `bulk_fetch`：行情批量取数面报文解析、请求形态与跨同步记忆（ADR-0121，issue #1374）；
//! - `http_client`：HTTP 重试与多主机切换；
//! - `instrument_info_sync`：标的信息同步与 ulist / 日 K 报文解析；
//! - `fund_search`：东财基金搜索报文解析与命中挑选（issue #301，fixture 驱动）；
//! - `fund_nav`：历史净值报文解析、水位窗口与 Referer 传播（issue #303，fixture 驱动）；
//! - `stock_quote`：股票单点行情报文解析、类型特征探测与命中挑选（issue #693，fixture 驱动）。
//!
//! 全量同步（clist 报文解析、分页编排、取消与重入守卫）已随 ADR-0081 决策 3
//! 退役删除（issue #698）。

use std::sync::{Arc, Mutex};

use ledger_infra::error::Result;
use rusqlite::{Connection, params};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

mod bulk_fetch;
mod fund_nav;
mod fund_search;
mod history_backfill;
mod http_client;
mod instrument_info_sync;
mod stock_quote;

// ---------------------------------------------------------------------------
// 共享测试脚手架（一份）
// ---------------------------------------------------------------------------

/// 桩适配器（issue #1412）：同步应答值 → 通道 future（抓取闭包 async 形态后的
/// 最小包装——既有桩闭包的应答逻辑保持同步表达式，只在出口装箱为立即就绪的
/// future，调用点零改动）。
pub(super) fn ready<T: Send + 'static>(value: Result<T>) -> crate::channels::FetchFuture<T> {
    Box::pin(std::future::ready(value))
}

/// 直插一条持仓（账户 + 标的 + 交易 + 批次），绕过交易行为层以聚焦增量同步自身逻辑。
/// 账户/标的经工厂种子（spec #728 / ADR-0084 决策 4）；标的类型工厂固定 stock，
/// bond/other/fund 等域变体经类型修正表达——类型是本域 secid 构造/跳过规则的
/// 行为输入，不入工厂种子。
pub(super) fn insert_holding(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    seed_account(
        conn,
        account_id,
        &format!("账户-{account_id}"),
        "investment",
        currency,
        0,
    );
    seed_instrument(
        conn,
        instrument_id,
        symbol,
        &format!("名称-{symbol}"),
        currency,
        market,
    );
    if kind != "stock" {
        conn.execute(
            "UPDATE instruments SET instrument_type=?1 WHERE id=?2",
            params![kind, instrument_id],
        )
        .unwrap();
    }
    insert_lot(conn, account_id, instrument_id, currency);
}

/// 直插一笔买入交易 + 持仓批次（绕过交易行为层，聚焦同步自身逻辑）。
pub(super) fn insert_lot(conn: &Connection, account_id: &str, instrument_id: &str, currency: &str) {
    let txn_id = format!("txn-{account_id}-{instrument_id}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,?2,1000,?3,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, currency, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,100,?5,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{account_id}-{instrument_id}"),
            account_id,
            instrument_id,
            txn_id,
            currency
        ],
    )
    .unwrap();
}

/// 起一个捕获请求头的本地 HTTP 服务（响应体固定、按顺序收集请求头），返回
/// (基础地址, 请求头收集器)——文本 / JSON 两类通道的请求形态断言共用一份脚手架。
pub(super) fn spawn_header_capture_server(body: String) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            heads_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (url, heads)
}

// ---------------------------------------------------------------------------
// 异步运行时形态守门（ADR-0125 决策 5/7 / issue #1413，删除即变红）
// ---------------------------------------------------------------------------

/// 生产面源码轻掩码：掐掉行注释与文档注释（`//` 起至行尾；字符串字面量内的
/// `//` 不受影响——引号配对检测，转义引号不计）。块注释未处理（本 crate 生产面
/// 无以块注释包裹受守令牌的形态），经别名改名的间接引用文本不可达——两者均靠
/// 评审兜底，与守门家族同款取舍。
fn mask_line_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            in_string = !in_string;
            out.push(c);
        } else if !in_string && c == '/' && chars.peek() == Some(&'/') {
            // 行注释：吞到行尾（换行保留，列位不保——本守门只做令牌判定）。
            for c in chars.by_ref() {
                if c == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if !in_string && c == '\'' {
            // 生命周期与 char 字面量在本 crate 生产面无受守令牌冲突，原样透传。
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// 抹掉内联 `#[cfg(test)] mod … { … }` 块（花括号配对）：本守门只辖生产路径，
/// 内联测试模块里的驱动代码（block_on 驱动 async 编排的同步测试、本地 HTTP
/// 服务的线程）不是生产执行环境；外挂测试面（tests.rs / tests/）本就不在
/// 扫描文件清单里。
fn blank_inline_test_modules(source: &str) -> String {
    let bytes: Vec<char> = source.chars().collect();
    let mut out = bytes.clone();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest: String = bytes[i..].iter().collect();
        if let Some(rel) = rest.strip_prefix("#[cfg(test)]") {
            let head = i + "#[cfg(test)]".len();
            let after: String = bytes[head..].iter().collect();
            let trimmed = after.trim_start();
            if trimmed.starts_with("mod") {
                // 找到模块体开括号（跳过 mod 名与修饰符）。
                if let Some(open_off) = after.find('{') {
                    let open_idx = head + open_off;
                    let mut depth = 0usize;
                    let mut j = open_idx;
                    while j < bytes.len() {
                        match bytes[j] {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    for k in out.iter_mut().take(j + 1).skip(i) {
                                        if *k != '\n' {
                                            *k = ' ';
                                        }
                                    }
                                    i = j + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    if depth != 0 {
                        break; // 不配对（不会发生），防御性退出
                    }
                    continue;
                }
            }
            i = head + rel.len();
            continue;
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// 列出本 crate 全部生产源文件（相对 `src/`，排除 tests.rs 与 tests/ 目录），
/// 排序固定保证失败输出确定。
fn production_source_files() -> Vec<(&'static str, String)> {
    let mut files: Vec<(&'static str, String)> = vec![
        ("bulk.rs", include_str!("bulk.rs").to_string()),
        ("channels.rs", include_str!("channels.rs").to_string()),
        (
            "daily_refresh.rs",
            include_str!("daily_refresh.rs").to_string(),
        ),
        ("fund.rs", include_str!("fund.rs").to_string()),
        (
            "fund_backfill.rs",
            include_str!("fund_backfill.rs").to_string(),
        ),
        ("fund_nav.rs", include_str!("fund_nav.rs").to_string()),
        (
            "fund_price_refresh.rs",
            include_str!("fund_price_refresh.rs").to_string(),
        ),
        ("history.rs", include_str!("history.rs").to_string()),
        ("http.rs", include_str!("http.rs").to_string()),
        ("incremental.rs", include_str!("incremental.rs").to_string()),
        ("js.rs", include_str!("js.rs").to_string()),
        ("lib.rs", include_str!("lib.rs").to_string()),
        ("model.rs", include_str!("model.rs").to_string()),
        ("persist.rs", include_str!("persist.rs").to_string()),
        ("progress.rs", include_str!("progress.rs").to_string()),
        ("session.rs", include_str!("session.rs").to_string()),
        ("stock.rs", include_str!("stock.rs").to_string()),
    ];
    files.sort_by_key(|(name, _)| *name);
    files
}

/// 守门清单对磁盘全等（fail loud，防漂移）：`production_source_files` 的硬编码
/// 清单必须与 `src/` 目录的实际生产文件集（排除 tests.rs 与 tests/）严格相等——
/// 新增生产文件不入清单即红，杜绝「新文件静默漏扫」。与
/// `scripts/check-background-services.ts` 车道守门（同规则双面）的分工：TS 面管
/// 全量生产文件的线程禁令与车道死条目，本测试面管车道接线与阻塞驱动点的域内
/// 断言；两处规则同源 ADR-0125 决策 7，任一处红即堵住回归。
#[test]
fn guard_source_list_matches_directory_exactly() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let src_dir = std::path::Path::new(manifest).join("src");
    let mut on_disk: Vec<String> = std::fs::read_dir(&src_dir)
        .expect("src 目录应可枚举")
        .map(|entry| entry.expect("目录项应可读").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            path.file_name()
                .expect("文件名应存在")
                .to_string_lossy()
                .to_string()
        })
        .filter(|name| name != "tests.rs")
        .collect();
    on_disk.sort();
    let mut in_list: Vec<&str> = production_source_files()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    in_list.sort();
    assert_eq!(
        in_list, on_disk,
        "守门源文件清单与 src/ 目录漂移——新增/删除生产文件须同步 production_source_files"
    );
}

/// 后台两条车道必须是挂全局运行时的 async 任务（ADR-0125 决策 7 / issue #1413，
/// 删除即变红）：调度入口以 `tauri::async_runtime::spawn` 拉起 async 任务，启动
/// 延迟与自然日窗口用 `tokio::time::sleep` 异步定时；生产面零自建线程。把车道
/// 改回 `std::thread::spawn` + `std::thread::sleep`（或删掉异步执行器接线）本测
/// 即红——「删除即变红」的负向半边，与 `scripts/check-background-services.ts`
/// 的成对拉起守门互补（那边管「在哪拉起」，本测管「以什么执行器拉起」）。
#[test]
fn background_lanes_are_global_runtime_async_tasks() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_line_comments(&src))))
        .collect();

    // 生产面零自建线程：车道线程是 ADR-0125 决策 7 显式去除的执行环境，回归
    // 即在异步上下文之外多出一条自持运行时状态的线程。
    let thread_hits: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| src.contains("thread::spawn"))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        thread_hits.is_empty(),
        "行情同步域生产面出现自建线程 {thread_hits:?}——后台车道必须是挂全局运行时的 \
         async 任务（tauri::async_runtime::spawn + tokio::time::sleep，ADR-0125 决策 7）"
    );

    for (name, src) in &sources {
        let is_lane = matches!(*name, "daily_refresh.rs" | "history.rs");
        if !is_lane {
            continue;
        }
        assert!(
            src.contains("tauri::async_runtime::spawn"),
            "{name} 调度入口应以 tauri::async_runtime::spawn 拉起 async 任务 \
             （ADR-0125 决策 7 / issue #1413）"
        );
        assert!(
            src.contains("tokio::time::sleep"),
            "{name} 的启动延迟与自然日窗口应用 tokio::time::sleep 异步定时 \
             （ADR-0125 决策 7 / issue #1413）"
        );
        assert!(
            !src.contains("std::thread::sleep"),
            "{name} 不得回归 std::thread::sleep 阻塞定时（ADR-0125 决策 7）"
        );
    }
}

/// 生产面零阻塞驱动点（ADR-0125 决策 5/7，删除即变红）：#1411 过渡同步桥随
/// #1413 拆除后，本 crate 生产面不得再出现 `block_on`——回归形态（两壳生产入口
/// 改回同步形状并重新引入桥驱动）在异步任务路径上触达即运行时 panic（#1403
/// 同款现场）；两壳生产入口必须保持 `async fn`（壳层接缝直接 `await`，无包装）。
#[test]
fn production_face_has_no_blocking_bridge() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_line_comments(&src))))
        .collect();

    let bridge_hits: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| src.contains("block_on"))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        bridge_hits.is_empty(),
        "行情同步域生产面出现阻塞驱动点 block_on {bridge_hits:?}——过渡同步桥已随 \
         issue #1413 拆除，网络等待一律以 await 表达（ADR-0125 决策 5/7）"
    );

    for (name, src) in &sources {
        let is_entry = matches!((*name, ()), ("fund.rs", ()) | ("stock.rs", ()));
        if !is_entry {
            continue;
        }
        let needle = if *name == "fund.rs" {
            "pub async fn fetch_fund_quote_production"
        } else {
            "pub async fn fetch_stock_quote_production"
        };
        assert!(
            src.contains(needle),
            "{name} 的生产拉取入口必须保持 async fn（{needle}）——壳层接缝在异步上下文 \
             直接 await，改回同步形状即重引阻塞驱动（ADR-0125 决策 5/7 / issue #1413）"
        );
    }
}
