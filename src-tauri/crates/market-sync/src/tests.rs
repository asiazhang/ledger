//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算与增量同步行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。
//!
//! #256 按行为主题拆为子模块（纯移动）：
//! - `bulk_fetch`：行情批量取数面报文解析、请求形态与跨同步记忆（ADR-0121，issue #1374）；
//! - `csrc`：证监会基金电子披露区间查询报文解析与请求形态（issue #1562 / ADR-0130，fixture 为真实报文 + 本地 HTTP 服务）；
//! - `http_client`：HTTP 重试、多主机切换与各来源取数入口的请求形态 / 上限行为（ECB 参考汇率、腾讯日线 K 线）；
//! - `instrument_info_sync`：标的信息同步与日 K 报文解析；
//! - `fund_search`：东财基金搜索报文解析与命中挑选（issue #301，fixture 驱动）；
//! - `fund_nav`：历史净值报文解析、水位窗口与 Referer 传播（issue #303，fixture 驱动）；
//! - `stock_quote`：股票单点行情报文解析、类型特征探测与命中挑选（issue #693，fixture 驱动）；
//! - `tencent`：腾讯行情批量报价取数（issue #1558，fixture 驱动）——三套字段布局的类型码 / 币种 /
//!   交易所后缀、请求形态与批量承载量、被拦截响应 fail-closed。
//! - `sina_fund`：新浪场外基金取数（issue #1564，fixture 驱动）——批量面普通行 /
//!   货基错位行 / 已终止基金行的形态判别与 fail-closed，全历史面末点 / 可信空 /
//!   非可信形状，请求形态与批量承载量。
//!
//! 全量同步（clist 报文解析、分页编排、取消与重入守卫）已随 ADR-0081 决策 3
//! 退役删除（issue #698）。

use std::sync::{Arc, Mutex};

use ledger_infra::error::Result;
use rusqlite::{Connection, params};
use tauri_app_lib::test_support::scan::{mask_non_code, matching_brace_end};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

mod bulk_fetch;
mod csrc;
mod fund_nav;
mod fund_search;
mod history_backfill;
mod http_client;
mod instrument_info_sync;
mod sina_fund;
mod stock_quote;
mod tencent;

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
/// bond/other/fund 等域变体经类型修正表达——类型是本域行情通道路由/跳过规则的
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
pub(super) fn spawn_header_capture_server(
    body: impl Into<Vec<u8>>,
) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};

    let body: Vec<u8> = body.into();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let Ok(reader_stream) = stream.try_clone() else {
                break;
            };
            // 循环读到请求头结束：请求行可能很长（批量承载量用例 ~7KB），单次 read
            // 可能截断。
            let mut reader = BufReader::new(reader_stream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let ends_head = line == "\r\n" || line == "\n";
                        head.push_str(&line);
                        if ends_head {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            heads_clone.lock().unwrap().push(head);
            let mut stream = stream;
            let resp_head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp_head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (url, heads)
}

// ---------------------------------------------------------------------------
// 异步运行时形态守门（ADR-0125 决策 5/7 / issue #1413，删除即变红）
// ---------------------------------------------------------------------------

/// 抹掉内联 `#[cfg(test)] mod … { … }` 块：本守门只辖生产路径，内联测试模块里
/// 的驱动代码（block_on 驱动 async 编排的同步测试、本地 HTTP 服务的线程）不是
/// 生产执行环境；外挂测试面（tests.rs / tests/）本就不在扫描文件清单里。
/// 输入须先经 [`mask_non_code`]（词法器具单点住 `test_support::scan`，#1433
/// 上收——原轻量行注释掩码退役，块注释与字符串字面量同受掩码，与
/// `scripts/check-structure.ts` 的 `maskNonCode` 双源同规）；花括号配对经
/// [`matching_brace_end`] 单一实现。
fn blank_inline_test_modules(source: &str) -> String {
    let mut out = source.to_string();
    let mut i = 0usize;
    while let Some(rel) = out[i..].find("#[cfg(test)]") {
        let anchor = i + rel;
        let head = anchor + "#[cfg(test)]".len();
        let after = &out[head..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with("mod") {
            // #1413 原语义保留：非 mod 附属的出现即终止。原式 `i = head + rel.len()`
            // 中 rel 是锦点之后的余下全文，恒等/于文本末尾——即终止而非跳过锦点
            // 续扫（经验测试钉住：非 mod 锦点后的 mod 块原实现同样不抹除；当前
            // 生产面无非 mod 附属的 cfg(test)，两形态不可区分）。
            break;
        }
        // 找到模块体开括号（跳过 mod 名与修饰符）。
        let Some(open) = after.find('{').map(|p| head + p) else {
            break;
        };
        let Some(end) = matching_brace_end(&out, open) else {
            break; // 不配对（不会发生），防御性退出
        };
        let blanked: String = out[anchor..end]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(anchor..end, &blanked);
        i = anchor + blanked.len();
    }
    out
}

/// 列出本 crate 全部生产源文件（相对 `src/`，排除 tests.rs 与 tests/ 目录），
/// 排序固定保证失败输出确定。
fn production_source_files() -> Vec<(&'static str, String)> {
    let mut files: Vec<(&'static str, String)> = vec![
        ("bulk.rs", include_str!("bulk.rs").to_string()),
        ("channels.rs", include_str!("channels.rs").to_string()),
        ("csrc.rs", include_str!("csrc.rs").to_string()),
        (
            "daily_refresh.rs",
            include_str!("daily_refresh.rs").to_string(),
        ),
        ("ecb.rs", include_str!("ecb.rs").to_string()),
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
        ("lane.rs", include_str!("lane.rs").to_string()),
        ("lib.rs", include_str!("lib.rs").to_string()),
        ("model.rs", include_str!("model.rs").to_string()),
        ("persist.rs", include_str!("persist.rs").to_string()),
        ("progress.rs", include_str!("progress.rs").to_string()),
        ("session.rs", include_str!("session.rs").to_string()),
        ("sina_fund.rs", include_str!("sina_fund.rs").to_string()),
        ("stock.rs", include_str!("stock.rs").to_string()),
        ("tencent.rs", include_str!("tencent.rs").to_string()),
        (
            "tencent_kline.rs",
            include_str!("tencent_kline.rs").to_string(),
        ),
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
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_non_code(&src))))
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
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_non_code(&src))))
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

// ---------------------------------------------------------------------------
// 后台车道单轮骨架单点（issue #1426，删除即变红）
// ---------------------------------------------------------------------------

/// 单轮骨架的机制不得回流到车道模块（issue #1426，删除即变红）：换装（生产束
/// 构造）、门面会话构造、收尾裁决（见证读取与提交点置脏）与进度发射一旦出现在
/// `daily_refresh.rs` / `history.rs`，即「下一次车道形态调整还要再改两处」的
/// Shotgun Surgery 回归（骨架又裂成两份平行实现）。
///
/// 断言强度（CONTEXT-testing）：判据只取**负向**——车道模块出现骨架机制即红，
/// 故对合法重命名（骨架 / trait / 转接单点改名）零误报；「两车道都经骨架」这一
/// 正向事实无法由行为断言观察（两条路径行为等价），按 ADR-0125 决策 7 的车道接线
/// 扫描先例由本结构守门承担，行为面归 `tests/` 下两条接线 IT 的端到端断言。
#[test]
fn background_lanes_share_single_round_skeleton() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, mask_non_code(&src)))
        .collect();

    // 机制指纹：换装 / 会话 / 裁决（见证读取、提交点置脏、失效信号）/ 进度发射。
    const SKELETON_MECHANICS: [&str; 7] = [
        "production_backfill()",
        "FacadeWriteSession::new",
        "any_written()",
        "write.run(",
        "emit_for(",
        "emit_backfill_progress",
        "emit_progress",
    ];
    for name in ["daily_refresh.rs", "history.rs"] {
        let src = sources
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, src)| src.as_str())
            .unwrap_or_else(|| panic!("{name} 应在守门源文件清单内"));
        for needle in SKELETON_MECHANICS {
            assert!(
                !src.contains(needle),
                "{name} 出现骨架机制 {needle}——换装 / 会话 / 见证 / 裁决 / 发射归 \
                 lane.rs 单点，回流到车道模块即平行骨架复活（issue #1426）"
            );
        }
    }
}
